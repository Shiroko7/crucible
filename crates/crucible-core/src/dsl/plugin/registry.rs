//! Feature registry for looking up and instantiating known feature plugins.

use super::casting::BypassCastingRestrictionsPlugin;
use super::items::LimitedUseDebuffItemPlugin;
use super::prestige_spellcasting::{AbilityRequirement, PrestigeSpellcastingPlugin};
use super::rogue::*;
use super::spells::*;
use super::standard::*;
use super::traits::{CreatureBuilder, FeatureError, FeaturePlugin, FeatureResult};
use crate::creature::{MoveKind, Rider};
use crate::dsl::scenario::{self, DurationSpec, TraitEffect};
use crate::rules::{Ability, DamageKind, SPELL_LEVELS};
use std::collections::HashMap;
use std::sync::Arc;

/// Read a spell plugin's optional cost into a [`SpellCost`]: `slot = N`
/// spends a spell slot of that level, while `resource = "<pool>"` (with an
/// optional `cost` amount, default 1) spends from a pool the caster already
/// declared under `[*.resources]` - a wand's charges, a feature's own uses.
/// Leaving both off makes the cast free.
///
/// Shared by every spell factory below so "how it's paid for" stays a
/// declared parameter rather than a name a plugin invents itself.
fn parse_spell_cost(val: &toml::Value) -> FeatureResult<Option<SpellCost>> {
    if let Some(level) = val.get("slot").and_then(|v| v.as_integer()) {
        if !(1..=i64::from(SPELL_LEVELS)).contains(&level) {
            return Err(FeatureError::InvalidConfiguration(format!(
                "a spell slot is level 1 to 9, got `slot = {level}`"
            )));
        }
        return Ok(Some(SpellCost::Slot(level as u32)));
    }
    let Some(resource) = val.get("resource").and_then(|v| v.as_str()) else {
        return Ok(None);
    };
    let amount = val.get("cost").and_then(|v| v.as_integer()).unwrap_or(1) as u32;
    Ok(Some(SpellCost::new(resource, amount)))
}

/// A trait phrase that must describe a [`Rider`] - `bonus 3d6 piercing vs
/// dragon` - rather than a flat stat bonus, for a plugin that attaches it to
/// one of its own moves.
fn parse_rider_phrase(phrase: &str) -> FeatureResult<Rider> {
    match scenario::parse_trait_external(phrase).map_err(FeatureError::InvalidConfiguration)? {
        TraitEffect::Rider(rider) => Ok(rider),
        other => Err(FeatureError::InvalidConfiguration(format!(
            "`{phrase}` is a flat bonus ({other:?}), not something a weapon carries onto a hit"
        ))),
    }
}

/// A move written in the scenario DSL, parsed only when it is applied - it
/// may name a resource pool (`cost potions 1`), and pools are resolved
/// against the creature being built - then promoted to a bonus action by
/// [`FastHandsPlugin`].
#[derive(Debug, Clone)]
struct DeclaredFastHandsMove {
    name: String,
    effect: String,
    kind: Option<MoveKind>,
}

/// A spell written in the scenario DSL, granted as a limited-use cast that
/// bypasses casting restrictions - parsed when applied, like
/// [`DeclaredFastHandsMove`].
#[derive(Debug, Clone)]
struct DeclaredBypassCast {
    name: String,
    effect: String,
    uses: u32,
}

impl FeaturePlugin for DeclaredBypassCast {
    fn id(&self) -> &'static str {
        "bypasses_casting_restrictions"
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let mut m = scenario::parse_move_external(
            &format!("{} | {}", self.name, self.effect),
            &builder.creature,
        )
        .map_err(FeatureError::InvalidConfiguration)?;
        m.kind = MoveKind::Spell;
        BypassCastingRestrictionsPlugin::new(m, self.uses).apply(builder)
    }
}

impl FeaturePlugin for DeclaredFastHandsMove {
    fn id(&self) -> &'static str {
        "fast_hands"
    }

    fn name(&self) -> &str {
        "Fast Hands"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let mut m = scenario::parse_move_external(
            &format!("{} | {}", self.name, self.effect),
            &builder.creature,
        )
        .map_err(FeatureError::InvalidConfiguration)?;
        if let Some(kind) = self.kind {
            m.kind = kind;
        }
        FastHandsPlugin::new(m).apply(builder)
    }
}

pub type PluginFactory =
    Arc<dyn Fn(&toml::Value) -> FeatureResult<Box<dyn FeaturePlugin>> + Send + Sync>;

/// Registry of known feature plugins.
///
/// When the agent reads a PC or monster statblock, it matches abilities against
/// this registry. The agent only implements new Rust plugins when an ability
/// cannot be resolved by the registry.
#[derive(Clone, Default)]
pub struct FeatureRegistry {
    factories: HashMap<String, PluginFactory>,
}

impl FeatureRegistry {
    pub fn new() -> Self {
        let mut reg = Self {
            factories: HashMap::new(),
        };
        reg.register_defaults();
        reg
    }

    pub fn register<F>(&mut self, id: impl Into<String>, factory: F)
    where
        F: Fn(&toml::Value) -> FeatureResult<Box<dyn FeaturePlugin>> + Send + Sync + 'static,
    {
        self.factories.insert(id.into(), Arc::new(factory));
    }

    pub fn build_plugin(
        &self,
        id: &str,
        params: &toml::Value,
    ) -> FeatureResult<Box<dyn FeaturePlugin>> {
        let factory = self.factories.get(id).ok_or_else(|| {
            FeatureError::InvalidConfiguration(format!("unregistered feature plugin '{id}'"))
        })?;
        factory(params)
    }

    fn register_defaults(&mut self) {
        // Evasion
        self.register("evasion", |val| {
            let ability_str = val.get("ability").and_then(|v| v.as_str()).unwrap_or("dex");
            let ability = Ability::parse(ability_str)
                .ok_or_else(|| FeatureError::UnknownAbility(ability_str.to_string()))?;
            Ok(Box::new(EvasionPlugin::new(ability)))
        });

        // Legendary Resistance
        self.register("legendary_resistance", |val| {
            let uses = val.get("uses").and_then(|v| v.as_integer()).unwrap_or(3) as u32;
            Ok(Box::new(LegendaryResistancePlugin::new(uses)))
        });

        // Sneak Attack (2024 Rogue 1)
        self.register("sneak_attack", |val| {
            let dice_count = val.get("dice_count").and_then(|v| v.as_integer()).ok_or_else(|| {
                FeatureError::InvalidConfiguration(
                    "sneak_attack needs a `dice_count` (its level-scaled d6 count, e.g. 4 at level 7)"
                        .to_string(),
                )
            })? as u32;
            let dice_sides = val
                .get("dice_sides")
                .and_then(|v| v.as_integer())
                .unwrap_or(6) as u32;
            Ok(Box::new(SneakAttackPlugin::with_sides(
                dice_count,
                dice_sides,
            )))
        });

        // Healing Word
        self.register("healing_word", |_val| {
            Ok(Box::new(HealingWordPlugin::new()))
        });

        // Cure Wounds
        self.register("cure_wounds", |_val| Ok(Box::new(CureWoundsPlugin::new())));

        // Fast Hands (2024 Thief 3): an object or magic-item move, written in
        // the scenario DSL, taken as a Bonus Action.
        self.register("fast_hands", |val| {
            let name = val.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
                FeatureError::InvalidConfiguration(
                    "fast_hands needs a `name` for the move it promotes".to_string(),
                )
            })?;
            let effect = val.get("effect").and_then(|v| v.as_str()).ok_or_else(|| {
                FeatureError::InvalidConfiguration(
                    "fast_hands needs an `effect` (the move, in the scenario DSL)".to_string(),
                )
            })?;
            let kind = match val.get("kind").and_then(|v| v.as_str()) {
                None => None,
                Some("object") => Some(MoveKind::ObjectUse),
                Some("magic_item") | Some("item") => Some(MoveKind::MagicItem),
                Some(other) => {
                    return Err(FeatureError::InvalidConfiguration(format!(
                        "fast_hands `kind` is `object` or `magic_item`, got `{other}`"
                    )))
                }
            };
            Ok(Box::new(DeclaredFastHandsMove {
                name: name.to_string(),
                effect: effect.to_string(),
                kind,
            }))
        });

        // A limited-use way to cast a spell without components, getting
        // through a silence (AT-04's Tricky Spells, Subtle Spell): the spell
        // itself written in the scenario DSL, on its own charge budget.
        self.register("bypasses_casting_restrictions", |val| {
            let name = val.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
                FeatureError::InvalidConfiguration(
                    "bypasses_casting_restrictions needs a `name`".to_string(),
                )
            })?;
            let effect = val.get("effect").and_then(|v| v.as_str()).ok_or_else(|| {
                FeatureError::InvalidConfiguration(
                    "bypasses_casting_restrictions needs an `effect` (the spell, in the scenario DSL)"
                        .to_string(),
                )
            })?;
            let uses = val.get("uses").and_then(|v| v.as_integer()).unwrap_or(1) as u32;
            Ok(Box::new(DeclaredBypassCast {
                name: name.to_string(),
                effect: effect.to_string(),
                uses,
            }))
        });

        // A limited-use item that forces a save or a debuff, used through
        // Fast Hands (ITM-05). `dc` left off means "against your spell save
        // DC"; `duration` is written the way the scenario DSL writes one.
        self.register("limited_use_debuff_item", |val| {
            let name = val.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
                FeatureError::InvalidConfiguration(
                    "limited_use_debuff_item needs a `name`".to_string(),
                )
            })?;
            let uses = val.get("uses").and_then(|v| v.as_integer()).unwrap_or(1) as u32;
            let ability_str = val.get("ability").and_then(|v| v.as_str()).ok_or_else(|| {
                FeatureError::InvalidConfiguration(
                    "limited_use_debuff_item needs the `ability` its save uses".to_string(),
                )
            })?;
            let ability = Ability::parse(ability_str)
                .ok_or_else(|| FeatureError::UnknownAbility(ability_str.to_string()))?;
            let dc = val.get("dc").and_then(|v| v.as_integer()).map(|d| d as i32);
            let phrase = val
                .get("duration")
                .and_then(|v| v.as_str())
                .unwrap_or("until applier");
            let words: Vec<&str> = phrase.split_whitespace().collect();
            let (spec, used) = scenario::parse_duration(&words, 0, phrase)
                .map_err(FeatureError::InvalidConfiguration)?;
            if used != words.len() {
                return Err(FeatureError::InvalidConfiguration(format!(
                    "`{phrase}` is not a duration"
                )));
            }
            let duration = match (spec, dc) {
                (DurationSpec::Fixed(d), _) => d,
                (DurationSpec::UntilSave, Some(dc)) => {
                    crate::rules::Duration::SaveEndTurn { ability, dc }
                }
                (DurationSpec::UntilSave, None) => {
                    return Err(FeatureError::InvalidConfiguration(format!(
                        "{name}: `until save` needs a fixed `dc` to repeat"
                    )))
                }
            };
            Ok(Box::new(match dc {
                Some(dc) => LimitedUseDebuffItemPlugin::new(name, uses, ability, dc, duration),
                None => LimitedUseDebuffItemPlugin::against_spell_dc(name, uses, ability, duration),
            }))
        });

        // Reliable Talent (2024 Rogue 7).
        self.register("reliable_talent", |val| {
            let floor = val.get("floor").and_then(|v| v.as_integer()).unwrap_or(10) as i32;
            Ok(Box::new(ReliableTalentPlugin::with_floor(floor)))
        });

        // Hold Person
        self.register("hold_person", |_val| Ok(Box::new(HoldPersonPlugin::new())));

        // Blindness/Deafness (SRD 5.2, 2nd level)
        self.register("blindness_deafness", |val| {
            let deafen = val.get("deafen").and_then(|v| v.as_bool()).unwrap_or(false);
            let cost = parse_spell_cost(val)?;
            Ok(Box::new(BlindnessDeafnessPlugin::new(deafen, cost)))
        });

        // Command (SRD 5.2, 1st level)
        self.register("command", |val| {
            let word_str = val.get("word").and_then(|v| v.as_str()).unwrap_or("grovel");
            let word = CommandWord::parse(word_str).ok_or_else(|| {
                FeatureError::InvalidConfiguration(format!("unknown command word '{word_str}'"))
            })?;
            let cost = parse_spell_cost(val)?;
            Ok(Box::new(CommandPlugin::new(word, cost)))
        });

        // Magic Missile (SRD 5.2, 1st level)
        self.register("magic_missile", |val| {
            let cost = parse_spell_cost(val)?;
            Ok(Box::new(MagicMissilePlugin::new(cost)))
        });

        // Bless (SRD 5.2, 1st level concentration)
        self.register("bless", |_val| Ok(Box::new(BlessPlugin)));

        // Bane (SRD 5.2, 1st level concentration)
        self.register("bane", |_val| Ok(Box::new(BanePlugin)));

        // Cunning Strike (2024 Rogue 5)
        self.register("cunning_strike", |val| {
            let dex_modifier = val
                .get("dex_modifier")
                .and_then(|v| v.as_integer())
                .ok_or_else(|| {
                    FeatureError::InvalidConfiguration(
                        "cunning_strike needs a `dex_modifier` (the Rogue's Dexterity modifier)"
                            .to_string(),
                    )
                })? as i32;
            let proficiency_bonus = val
                .get("proficiency_bonus")
                .and_then(|v| v.as_integer())
                .ok_or_else(|| {
                    FeatureError::InvalidConfiguration(
                        "cunning_strike needs a `proficiency_bonus`".to_string(),
                    )
                })? as i32;
            // A magic item's flat bonus to the DC (ITM-06) - optional, and
            // zero (no change at all) when the config never mentions it.
            let item_bonus = val
                .get("item_bonus")
                .and_then(|v| v.as_integer())
                .unwrap_or(0) as i32;
            Ok(Box::new(
                CunningStrikePlugin::new(dex_modifier, proficiency_bonus)
                    .with_item_bonus(item_bonus),
            ))
        });

        // Steady Aim (2024 Rogue 3)
        self.register("steady_aim", |_val| Ok(Box::new(SteadyAimPlugin::new())));

        // Cunning Action (2024 Rogue 2)
        self.register("cunning_action", |_val| {
            Ok(Box::new(CunningActionPlugin::new()))
        });

        // Cunning Strike: Trip (2024 Rogue 5)
        self.register("cunning_strike_trip", |_val| {
            Ok(Box::new(CunningStrikeTripPlugin))
        });

        // Cunning Strike: Withdraw (2024 Rogue 5)
        self.register("cunning_strike_withdraw", |_val| {
            Ok(Box::new(CunningStrikeWithdrawPlugin))
        });

        // Prestige / secondary spellcasting grant: a build-specific entry
        // gate over a build-specific slot table and casting bonus - every
        // field is a TOML parameter, see `prestige_spellcasting`.
        self.register("prestige_spellcasting", |val| {
            let name = val
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Prestige Spellcasting")
                .to_string();

            let ability_requirement = |n: u32| -> FeatureResult<AbilityRequirement> {
                let ability_key = format!("ability_{n}");
                let min_key = format!("ability_{n}_min");
                let ability_str = val.get(&ability_key).and_then(|v| v.as_str()).ok_or_else(|| {
                    FeatureError::InvalidConfiguration(format!(
                        "prestige_spellcasting needs a `{ability_key}` (which ability score this entry requirement checks)"
                    ))
                })?;
                let ability = Ability::parse(ability_str)
                    .ok_or_else(|| FeatureError::UnknownAbility(ability_str.to_string()))?;
                let minimum = val.get(&min_key).and_then(|v| v.as_integer()).ok_or_else(|| {
                    FeatureError::InvalidConfiguration(format!(
                        "prestige_spellcasting needs a `{min_key}` (the minimum score required to qualify)"
                    ))
                })? as i32;
                Ok(AbilityRequirement::new(ability, minimum))
            };
            let ability_requirements = [ability_requirement(1)?, ability_requirement(2)?];

            let minimum_sneak_attack_dice = val
                .get("min_sneak_attack_dice")
                .and_then(|v| v.as_integer())
                .ok_or_else(|| {
                    FeatureError::InvalidConfiguration(
                        "prestige_spellcasting needs a `min_sneak_attack_dice` (the minimum existing Sneak-Attack-shaped rider dice count required to enter)"
                            .to_string(),
                    )
                })? as u32;

            let spellcasting_ability_str = val
                .get("spellcasting_ability")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    FeatureError::InvalidConfiguration(
                        "prestige_spellcasting needs a `spellcasting_ability` (which score fuels the granted spellcasting)"
                            .to_string(),
                    )
                })?;
            let ability = Ability::parse(spellcasting_ability_str)
                .ok_or_else(|| FeatureError::UnknownAbility(spellcasting_ability_str.to_string()))?;

            let proficiency_bonus = val
                .get("proficiency_bonus")
                .and_then(|v| v.as_integer())
                .ok_or_else(|| {
                    FeatureError::InvalidConfiguration(
                        "prestige_spellcasting needs a `proficiency_bonus` (the ability modifier comes from the creature's own score, and the save DC is derived from both)"
                            .to_string(),
                    )
                })? as i32;

            let mut slots = [0u32; SPELL_LEVELS as usize];
            if let Some(table) = val.get("slots").and_then(|v| v.as_table()) {
                for (level_str, max_val) in table {
                    let level: u32 = level_str.parse().map_err(|_| {
                        FeatureError::InvalidConfiguration(format!(
                            "prestige_spellcasting slot level '{level_str}' is not a number 1-9"
                        ))
                    })?;
                    if !(1..=SPELL_LEVELS).contains(&level) {
                        return Err(FeatureError::InvalidConfiguration(format!(
                            "prestige_spellcasting slot level must be 1-9, got {level}"
                        )));
                    }
                    let max = max_val.as_integer().ok_or_else(|| {
                        FeatureError::InvalidConfiguration(format!(
                            "prestige_spellcasting slots.{level_str} must be an integer"
                        ))
                    })? as u32;
                    slots[(level - 1) as usize] = max;
                }
            }

            Ok(Box::new(PrestigeSpellcastingPlugin::new(
                name,
                ability_requirements,
                minimum_sneak_attack_dice,
                slots,
                ability,
                proficiency_bonus,
            )))
        });

        // True Strike (2024 cantrip): a weapon attack using the caster's own
        // SpellCastingProfile, plus scaling Radiant dice - every number here
        // is a TOML parameter, see `spells::TrueStrikePlugin`.
        self.register("true_strike", |val| {
            let damage_kind = |key: &str, default: Option<&str>| -> FeatureResult<DamageKind> {
                let word = match (val.get(key).and_then(|v| v.as_str()), default) {
                    (Some(word), _) => word,
                    (None, Some(default)) => default,
                    (None, None) => {
                        return Err(FeatureError::InvalidConfiguration(format!(
                            "true_strike needs a `{key}`"
                        )))
                    }
                };
                DamageKind::parse(word).ok_or_else(|| FeatureError::UnknownDamageKind(word.to_string()))
            };

            let weapon_dice_count = val
                .get("weapon_dice_count")
                .and_then(|v| v.as_integer())
                .ok_or_else(|| {
                    FeatureError::InvalidConfiguration(
                        "true_strike needs a `weapon_dice_count` (the wielded weapon's own dice count)"
                            .to_string(),
                    )
                })? as u32;
            let weapon_dice_sides = val
                .get("weapon_dice_sides")
                .and_then(|v| v.as_integer())
                .ok_or_else(|| {
                    FeatureError::InvalidConfiguration(
                        "true_strike needs a `weapon_dice_sides` (the wielded weapon's own die size)"
                            .to_string(),
                    )
                })? as u32;
            let weapon_damage_kind = damage_kind("weapon_damage_kind", None)?;
            let finesse_or_ranged = val
                .get("finesse_or_ranged")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let ranged = val.get("ranged").and_then(|v| v.as_bool()).unwrap_or(false);
            let weapon_bonus = val
                .get("weapon_bonus")
                .and_then(|v| v.as_integer())
                .unwrap_or(0) as i32;
            let weapon_riders = match val.get("weapon_traits").and_then(|v| v.as_array()) {
                None => Vec::new(),
                Some(list) => list
                    .iter()
                    .map(|v| {
                        v.as_str()
                            .ok_or_else(|| {
                                FeatureError::InvalidConfiguration(
                                    "true_strike `weapon_traits` is a list of trait phrases"
                                        .to_string(),
                                )
                            })
                            .and_then(parse_rider_phrase)
                    })
                    .collect::<FeatureResult<Vec<Rider>>>()?,
            };
            let radiant_dice_count = val
                .get("radiant_dice_count")
                .and_then(|v| v.as_integer())
                .ok_or_else(|| {
                    FeatureError::InvalidConfiguration(
                        "true_strike needs a `radiant_dice_count` (its cantrip-scaling tier's bonus dice, 2 at the base tier)"
                            .to_string(),
                    )
                })? as u32;
            let radiant_dice_sides = val
                .get("radiant_dice_sides")
                .and_then(|v| v.as_integer())
                .unwrap_or(6) as u32;
            let radiant_damage_kind = damage_kind("radiant_damage_kind", Some("radiant"))?;

            let mut plugin = TrueStrikePlugin::new(
                weapon_dice_count,
                weapon_dice_sides,
                weapon_damage_kind,
                finesse_or_ranged,
                radiant_dice_count,
                radiant_dice_sides,
                radiant_damage_kind,
            )
            .with_weapon_bonus(weapon_bonus);
            if ranged {
                plugin = plugin.with_ranged();
            }
            for rider in weapon_riders {
                plugin = plugin.with_weapon_rider(rider);
            }
            Ok(Box::new(plugin))
        });

        // Spiritual Weapon (SRD 5.2, 2nd level, Bonus Action strike, no
        // concentration) - every number it needs comes off the creature's
        // own `spellcasting` profile, so there is nothing to read from TOML.
        self.register("spiritual_weapon", |_val| {
            Ok(Box::new(SpiritualWeaponPlugin))
        });

        // Guiding Bolt (SRD 5.2, 1st level): a ranged spell attack for 4d6
        // Radiant using the caster's own `SpellCastingProfile`, marking the
        // target on a hit. `dice_count`/`dice_sides` default to the printed
        // 4d6 but can be overridden, the same way `sneak_attack`'s dice are -
        // see `spells::GuidingBoltPlugin`.
        self.register("guiding_bolt", |val| {
            let dice_count = val
                .get("dice_count")
                .and_then(|v| v.as_integer())
                .unwrap_or(4) as u32;
            let dice_sides = val
                .get("dice_sides")
                .and_then(|v| v.as_integer())
                .unwrap_or(6) as u32;
            Ok(Box::new(super::spells::GuidingBoltPlugin::with_dice(
                dice_count, dice_sides,
            )))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::Effect;
    use crate::rules::{Condition, Duration, SpellCastingProfile};

    /// `dice_count` is the whole point of making Sneak Attack a plugin
    /// parameter rather than a hardcoded 4d6 - a level-9 rogue's TOML feature
    /// declares 5, and the registry has to carry that through.
    #[test]
    fn sneak_attack_reads_its_dice_count_from_toml() {
        let registry = FeatureRegistry::new();
        let params: toml::Value =
            toml::from_str("plugin = \"sneak_attack\"\ndice_count = 5").unwrap();
        let plugin = registry
            .build_plugin("sneak_attack", &params)
            .expect("sneak_attack builds from toml");
        assert_eq!(plugin.id(), "sneak_attack");
        assert_eq!(plugin.name(), "Sneak Attack");
    }

    #[test]
    fn sneak_attack_defaults_to_d6_but_can_be_overridden() {
        let registry = FeatureRegistry::new();
        let params: toml::Value = toml::from_str("dice_count = 4").unwrap();
        let plugin = registry.build_plugin("sneak_attack", &params).unwrap();
        let mut builder = crate::dsl::plugin::CreatureBuilder::new("Rogue", 15, 40);
        plugin.apply(&mut builder).unwrap();
        assert_eq!(
            builder.creature.riders,
            vec![crate::creature::Rider::ConditionalExtraDamage {
                dice_count: 4,
                dice_sides: 6,
                once_per_turn: true,
            }]
        );
    }

    #[test]
    fn sneak_attack_requires_a_dice_count() {
        let registry = FeatureRegistry::new();
        let params: toml::Value = toml::from_str("plugin = \"sneak_attack\"").unwrap();
        assert!(matches!(
            registry.build_plugin("sneak_attack", &params),
            Err(FeatureError::InvalidConfiguration(_))
        ));
    }

    fn spellcaster() -> CreatureBuilder {
        let mut builder = CreatureBuilder::new("Wizard", 12, 30);
        builder.set_spellcasting(SpellCastingProfile::new(Ability::Int, 4, 3));
        builder
    }

    #[test]
    fn magic_missile_builds_from_toml_and_spends_a_named_pool() {
        let registry = FeatureRegistry::new();
        let mut builder = spellcaster();
        builder.ensure_resource("wand_charges", 7);

        let params: toml::Value =
            toml::from_str("plugin = \"magic_missile\"\nresource = \"wand_charges\"").unwrap();
        let plugin = registry
            .build_plugin("magic_missile", &params)
            .expect("magic_missile builds from toml");
        assert_eq!(plugin.id(), "magic_missile");
        plugin.apply(&mut builder).unwrap();

        let cost = builder.creature.actions[0].cost.expect("cost resolved");
        assert_eq!(
            builder.creature.resources[cost.resource].name,
            "wand_charges"
        );
        assert_eq!(cost.amount, 1);
        assert_eq!(builder.creature.actions[0].spell_slot_level, None);
    }

    /// `slot = N` spends from the caster's real slot pool - the same one
    /// Bless and Healing Word draw on - rather than a named resource.
    #[test]
    fn a_spell_cost_can_be_a_real_spell_slot() {
        let registry = FeatureRegistry::new();
        let mut builder = spellcaster();
        let params: toml::Value = toml::from_str("plugin = \"command\"\nslot = 1").unwrap();
        registry
            .build_plugin("command", &params)
            .unwrap()
            .apply(&mut builder)
            .unwrap();
        let m = &builder.creature.actions[0];
        assert_eq!(m.spell_slot_level, Some(1));
        assert_eq!(m.cost, None);
        assert_eq!(m.kind, MoveKind::Spell);

        let bad: toml::Value = toml::from_str("plugin = \"command\"\nslot = 10").unwrap();
        assert!(matches!(
            registry.build_plugin("command", &bad),
            Err(FeatureError::InvalidConfiguration(_))
        ));
    }

    #[test]
    fn blindness_deafness_builds_from_toml_with_a_choice_of_condition() {
        let registry = FeatureRegistry::new();
        let mut builder = spellcaster();

        let params: toml::Value =
            toml::from_str("plugin = \"blindness_deafness\"\ndeafen = true").unwrap();
        let plugin = registry
            .build_plugin("blindness_deafness", &params)
            .expect("blindness_deafness builds from toml");
        plugin.apply(&mut builder).unwrap();

        let Effect::Save(save) = &builder.creature.actions[0].effect else {
            panic!("expected a Save effect");
        };
        assert_eq!(save.on_failure[0].0, Condition::Deafened);
    }

    #[test]
    fn command_builds_from_toml_and_rejects_an_unknown_word() {
        let registry = FeatureRegistry::new();
        let mut builder = spellcaster();

        let params: toml::Value = toml::from_str("plugin = \"command\"\nword = \"halt\"").unwrap();
        let plugin = registry
            .build_plugin("command", &params)
            .expect("command builds from toml");
        plugin.apply(&mut builder).unwrap();
        let Effect::Save(save) = &builder.creature.actions[0].effect else {
            panic!("expected a Save effect");
        };
        assert_eq!(
            save.on_failure,
            vec![(Condition::Compelled, Duration::ApplierTurn)]
        );

        let bad_params: toml::Value =
            toml::from_str("plugin = \"command\"\nword = \"flee\"").unwrap();
        assert!(matches!(
            registry.build_plugin("command", &bad_params),
            Err(FeatureError::InvalidConfiguration(_))
        ));
    }

    #[test]
    fn cunning_strike_reads_its_dc_inputs_from_toml() {
        let registry = FeatureRegistry::new();
        let params: toml::Value =
            toml::from_str("plugin = \"cunning_strike\"\ndex_modifier = 4\nproficiency_bonus = 3")
                .unwrap();
        let plugin = registry
            .build_plugin("cunning_strike", &params)
            .expect("cunning_strike builds from toml");
        assert_eq!(plugin.id(), "cunning_strike");
        assert_eq!(plugin.name(), "Cunning Strike");

        let mut builder = crate::dsl::plugin::CreatureBuilder::new("Rogue", 15, 40);
        plugin.apply(&mut builder).unwrap();
        assert_eq!(
            builder.creature.riders,
            vec![crate::creature::Rider::CunningStrike { dc: 15 }]
        );
    }

    /// ITM-06: an optional `item_bonus` in the TOML flows through to the DC,
    /// and is zero - unchanged from before this field existed - when the
    /// config never mentions it.
    #[test]
    fn cunning_strike_reads_an_optional_item_bonus_from_toml() {
        let registry = FeatureRegistry::new();

        let without: toml::Value =
            toml::from_str("plugin = \"cunning_strike\"\ndex_modifier = 4\nproficiency_bonus = 3")
                .unwrap();
        let plugin = registry.build_plugin("cunning_strike", &without).unwrap();
        let mut builder = crate::dsl::plugin::CreatureBuilder::new("Rogue", 15, 40);
        plugin.apply(&mut builder).unwrap();
        assert_eq!(
            builder.creature.riders,
            vec![crate::creature::Rider::CunningStrike { dc: 15 }]
        );

        let with: toml::Value = toml::from_str(
            "plugin = \"cunning_strike\"\ndex_modifier = 4\nproficiency_bonus = 3\nitem_bonus = 2",
        )
        .unwrap();
        let plugin = registry.build_plugin("cunning_strike", &with).unwrap();
        let mut builder = crate::dsl::plugin::CreatureBuilder::new("Rogue", 15, 40);
        plugin.apply(&mut builder).unwrap();
        assert_eq!(
            builder.creature.riders,
            vec![crate::creature::Rider::CunningStrike { dc: 17 }]
        );
    }

    #[test]
    fn cunning_strike_requires_both_dc_inputs() {
        let registry = FeatureRegistry::new();
        let missing_proficiency: toml::Value =
            toml::from_str("plugin = \"cunning_strike\"\ndex_modifier = 4").unwrap();
        assert!(matches!(
            registry.build_plugin("cunning_strike", &missing_proficiency),
            Err(FeatureError::InvalidConfiguration(_))
        ));

        let missing_dex: toml::Value =
            toml::from_str("plugin = \"cunning_strike\"\nproficiency_bonus = 3").unwrap();
        assert!(matches!(
            registry.build_plugin("cunning_strike", &missing_dex),
            Err(FeatureError::InvalidConfiguration(_))
        ));
    }

    #[test]
    fn steady_aim_builds_from_toml_with_no_parameters() {
        let registry = FeatureRegistry::new();
        let params: toml::Value = toml::from_str("plugin = \"steady_aim\"").unwrap();
        let plugin = registry
            .build_plugin("steady_aim", &params)
            .expect("steady_aim builds");
        assert_eq!(plugin.id(), "steady_aim");
        assert_eq!(plugin.name(), "Steady Aim");
    }

    #[test]
    fn cunning_action_builds_from_toml_with_no_parameters() {
        let registry = FeatureRegistry::new();
        let params: toml::Value = toml::from_str("plugin = \"cunning_action\"").unwrap();
        let plugin = registry
            .build_plugin("cunning_action", &params)
            .expect("cunning_action builds");
        assert_eq!(plugin.id(), "cunning_action");
        assert_eq!(plugin.name(), "Cunning Action");
        let mut builder = crate::dsl::plugin::CreatureBuilder::new("Rogue", 15, 40);
        plugin.apply(&mut builder).unwrap();
        assert_eq!(builder.creature.bonus_actions.len(), 2);
    }

    #[test]
    fn cunning_strike_trip_and_withdraw_register_their_marker_riders() {
        let registry = FeatureRegistry::new();
        let no_params: toml::Value = toml::from_str("plugin = \"cunning_strike_trip\"").unwrap();

        let trip = registry
            .build_plugin("cunning_strike_trip", &no_params)
            .expect("cunning_strike_trip builds from toml");
        assert_eq!(trip.id(), "cunning_strike_trip");
        assert_eq!(trip.name(), "Cunning Strike: Trip");
        let mut builder = crate::dsl::plugin::CreatureBuilder::new("Rogue", 15, 40);
        trip.apply(&mut builder).unwrap();
        assert_eq!(
            builder.creature.riders,
            vec![crate::creature::Rider::CunningStrikeTrip]
        );

        let withdraw = registry
            .build_plugin("cunning_strike_withdraw", &no_params)
            .expect("cunning_strike_withdraw builds from toml");
        assert_eq!(withdraw.id(), "cunning_strike_withdraw");
        assert_eq!(withdraw.name(), "Cunning Strike: Withdraw");
        let mut builder = crate::dsl::plugin::CreatureBuilder::new("Rogue", 15, 40);
        withdraw.apply(&mut builder).unwrap();
        assert_eq!(
            builder.creature.riders,
            vec![crate::creature::Rider::CunningStrikeWithdraw]
        );
    }

    /// Every number in this TOML is a stand-in - different ability names,
    /// different thresholds, a different slot split and casting bonus than
    /// any other test uses - which is the whole point of the plugin being
    /// parameterized rather than hardcoded to one specific build.
    fn prestige_spellcasting_toml() -> &'static str {
        r#"
            plugin = "prestige_spellcasting"
            name = "Test Prestige Caster"
            ability_1 = "dex"
            ability_1_min = 13
            ability_2 = "int"
            ability_2_min = 13
            min_sneak_attack_dice = 2
            spellcasting_ability = "wis"
            proficiency_bonus = 4

            [slots]
            1 = 4
            2 = 3
        "#
    }

    #[test]
    fn prestige_spellcasting_reads_its_parameters_from_toml_and_applies() {
        let registry = FeatureRegistry::new();
        let params: toml::Value = toml::from_str(prestige_spellcasting_toml()).unwrap();
        let plugin = registry
            .build_plugin("prestige_spellcasting", &params)
            .expect("prestige_spellcasting builds from toml");
        assert_eq!(plugin.id(), "prestige_spellcasting");
        assert_eq!(plugin.name(), "Test Prestige Caster");

        let mut builder = crate::dsl::plugin::CreatureBuilder::new("Qualifying Rogue", 15, 40);
        builder.set_ability_score(Ability::Dex, 13);
        builder.set_ability_score(Ability::Int, 13);
        builder.set_ability_score(Ability::Wis, 20);
        builder.add_rider(crate::creature::Rider::ConditionalExtraDamage {
            dice_count: 2,
            dice_sides: 6,
            once_per_turn: true,
        });

        plugin.apply(&mut builder).expect("prerequisites are met");

        assert_eq!(builder.creature.spell_slots.max(1), 4);
        assert_eq!(builder.creature.spell_slots.max(2), 3);
        let profile = builder.creature.spellcasting.expect("profile granted");
        assert_eq!(profile.ability, Ability::Wis);
        assert_eq!(profile.ability_modifier, 5);
        assert_eq!(profile.attack_bonus(), 9);
        assert_eq!(profile.save_dc(), 17);
    }

    #[test]
    fn prestige_spellcasting_refuses_a_creature_that_does_not_qualify() {
        let registry = FeatureRegistry::new();
        let params: toml::Value = toml::from_str(prestige_spellcasting_toml()).unwrap();
        let plugin = registry
            .build_plugin("prestige_spellcasting", &params)
            .unwrap();

        // No ability scores, no sneak attack dice at all: none of the three
        // prerequisites hold.
        let mut builder = crate::dsl::plugin::CreatureBuilder::new("Unqualified Rogue", 15, 40);
        assert!(matches!(
            plugin.apply(&mut builder),
            Err(FeatureError::PrerequisiteNotMet(_))
        ));
        assert!(builder.creature.spellcasting.is_none());
    }

    #[test]
    fn prestige_spellcasting_requires_both_ability_requirement_fields() {
        let registry = FeatureRegistry::new();
        let params: toml::Value = toml::from_str(
            r#"
                ability_1 = "dex"
                ability_1_min = 13
                min_sneak_attack_dice = 2
                spellcasting_ability = "wis"
                proficiency_bonus = 4
            "#,
        )
        .unwrap();
        assert!(matches!(
            registry.build_plugin("prestige_spellcasting", &params),
            Err(FeatureError::InvalidConfiguration(_))
        ));
    }

    /// `weapon_dice_count`/`weapon_dice_sides` and `radiant_dice_count` are
    /// the whole point of making True Strike a plugin rather than a
    /// hardcoded 1d8 rapier at the base 2d6 tier - a different weapon or a
    /// higher cantrip-scaling tier is just different TOML, not a code change.
    #[test]
    fn true_strike_reads_its_parameters_from_toml_and_applies() {
        let registry = FeatureRegistry::new();
        let params: toml::Value = toml::from_str(
            r#"
                plugin = "true_strike"
                weapon_dice_count = 1
                weapon_dice_sides = 8
                weapon_damage_kind = "piercing"
                finesse_or_ranged = true
                radiant_dice_count = 2
            "#,
        )
        .unwrap();
        let plugin = registry
            .build_plugin("true_strike", &params)
            .expect("true_strike builds from toml");
        assert_eq!(plugin.id(), "true_strike");
        assert_eq!(plugin.name(), "True Strike");

        let mut builder = crate::dsl::plugin::CreatureBuilder::new("Caster", 15, 30);
        builder.set_spellcasting(crate::rules::SpellCastingProfile::new(Ability::Wis, 4, 3));
        plugin
            .apply(&mut builder)
            .expect("spellcasting is declared");

        let action = builder.creature.actions.last().expect("action added");
        assert_eq!(action.name, "True Strike");
    }

    /// The weapon a True Strike is made with brings its own magic bonus and
    /// its own riders: a +3 bow of slaying hits and damages 3 better, and
    /// carries its bonus dice onto the spell's attack.
    #[test]
    fn true_strike_carries_the_wielded_weapons_bonus_and_riders() {
        let registry = FeatureRegistry::new();
        let params: toml::Value = toml::from_str(
            r#"
                plugin = "true_strike"
                weapon_dice_count = 1
                weapon_dice_sides = 8
                weapon_damage_kind = "piercing"
                ranged = true
                weapon_bonus = 3
                weapon_traits = ["bonus 3d6 piercing vs dragon"]
                radiant_dice_count = 2
            "#,
        )
        .unwrap();
        let plugin = registry.build_plugin("true_strike", &params).unwrap();
        let mut builder = crate::dsl::plugin::CreatureBuilder::new("Caster", 15, 30);
        builder.set_spellcasting(crate::rules::SpellCastingProfile::new(Ability::Wis, 5, 4));
        plugin.apply(&mut builder).unwrap();
        let action = &builder.creature.actions[0];
        let Effect::Strikes { strike, .. } = &action.effect else {
            panic!("expected strikes");
        };
        assert_eq!(strike.to_hit, 12, "5 + 4 + the weapon's 3");
        assert_eq!(strike.damage[0].bonus, 8, "5 + the weapon's 3");
        assert!(strike.kind.ranged && strike.kind.weapon && strike.kind.spell);
        assert!(matches!(
            action.riders[..],
            [crate::creature::Rider::BonusDamageVsCreatureType { dice_count: 3, .. }]
        ));

        let flat: toml::Value = toml::from_str(
            r#"
                weapon_dice_count = 1
                weapon_dice_sides = 8
                weapon_damage_kind = "piercing"
                radiant_dice_count = 2
                weapon_traits = ["ac 2"]
            "#,
        )
        .unwrap();
        assert!(matches!(
            registry.build_plugin("true_strike", &flat),
            Err(FeatureError::InvalidConfiguration(_))
        ));
    }

    #[test]
    fn fast_hands_promotes_a_declared_object_move_to_a_bonus_action() {
        let registry = FeatureRegistry::new();
        let params: toml::Value = toml::from_str(
            r#"
                plugin = "fast_hands"
                name = "Potion of Healing"
                effect = "object | cost potions 1 | heal 2d4+2"
            "#,
        )
        .unwrap();
        let plugin = registry.build_plugin("fast_hands", &params).unwrap();
        let mut builder = crate::dsl::plugin::CreatureBuilder::new("Thief", 15, 30);
        builder.ensure_resource("potions", 3);
        plugin.apply(&mut builder).unwrap();
        assert!(builder.creature.actions.is_empty());
        let potion = &builder.creature.bonus_actions[0];
        assert_eq!(potion.name, "Potion of Healing");
        assert_eq!(potion.kind, MoveKind::ObjectUse);
        assert!(matches!(potion.effect, Effect::Heal(_)));
        assert!(potion.cost.is_some());

        // A plain attack is neither an object nor a magic item.
        let attack: toml::Value = toml::from_str(
            r#"
                name = "Stab"
                effect = "hit +5 | 1d4+3 piercing"
            "#,
        )
        .unwrap();
        let plugin = registry.build_plugin("fast_hands", &attack).unwrap();
        assert!(matches!(
            plugin.apply(&mut builder),
            Err(FeatureError::InvalidConfiguration(_))
        ));
    }

    #[test]
    fn a_components_free_cast_builds_from_toml() {
        let registry = FeatureRegistry::new();
        let params: toml::Value = toml::from_str(
            r#"
                plugin = "bypasses_casting_restrictions"
                name = "Quiet Bolt"
                effect = "ranged | hit +7 | 4d6 radiant"
                uses = 1
            "#,
        )
        .unwrap();
        let plugin = registry
            .build_plugin("bypasses_casting_restrictions", &params)
            .unwrap();
        let mut builder = spellcaster();
        plugin.apply(&mut builder).unwrap();
        let m = &builder.creature.actions[0];
        assert!(m.bypasses_casting_restrictions);
        assert_eq!(m.kind, MoveKind::Spell);
        assert_eq!(m.uses, crate::creature::Uses::Limited(1));
    }

    #[test]
    fn limited_use_debuff_item_builds_from_toml() {
        let registry = FeatureRegistry::new();
        let params: toml::Value = toml::from_str(
            r#"
                plugin = "limited_use_debuff_item"
                name = "Test Card"
                ability = "wis"
                duration = "for 1 minute"
            "#,
        )
        .unwrap();
        let plugin = registry
            .build_plugin("limited_use_debuff_item", &params)
            .unwrap();
        let mut builder = spellcaster();
        plugin.apply(&mut builder).unwrap();
        let item = &builder.creature.bonus_actions[0];
        assert_eq!(item.kind, MoveKind::MagicItem);
        let Effect::Save(save) = &item.effect else {
            panic!("expected a save");
        };
        assert_eq!(save.dc, 15, "the caster's own spell save DC");
        assert_eq!(
            save.on_failure,
            vec![(Condition::Suppressed, Duration::Rounds(10))]
        );

        let repeat_without_dc: toml::Value = toml::from_str(
            r#"
                name = "Test Card"
                ability = "wis"
                duration = "until save"
            "#,
        )
        .unwrap();
        assert!(matches!(
            registry.build_plugin("limited_use_debuff_item", &repeat_without_dc),
            Err(FeatureError::InvalidConfiguration(_))
        ));
    }

    #[test]
    fn true_strike_requires_its_weapon_and_radiant_dice() {
        let registry = FeatureRegistry::new();
        let params: toml::Value = toml::from_str(
            r#"
                plugin = "true_strike"
                weapon_damage_kind = "piercing"
            "#,
        )
        .unwrap();
        assert!(matches!(
            registry.build_plugin("true_strike", &params),
            Err(FeatureError::InvalidConfiguration(_))
        ));
    }

    #[test]
    fn true_strike_requires_spellcasting_to_already_be_declared_on_the_creature() {
        let registry = FeatureRegistry::new();
        let params: toml::Value = toml::from_str(
            r#"
                weapon_dice_count = 1
                weapon_dice_sides = 8
                weapon_damage_kind = "piercing"
                radiant_dice_count = 2
            "#,
        )
        .unwrap();
        let plugin = registry.build_plugin("true_strike", &params).unwrap();
        let mut builder = crate::dsl::plugin::CreatureBuilder::new("Not Yet A Caster", 15, 30);
        assert!(matches!(
            plugin.apply(&mut builder),
            Err(FeatureError::PrerequisiteNotMet(_))
        ));
    }

    /// Spiritual Weapon takes no TOML parameters of its own - every number
    /// it needs comes off the creature's own `spellcasting` profile - so
    /// building it from an empty table has to succeed.
    #[test]
    fn spiritual_weapon_builds_from_toml_with_no_parameters() {
        let registry = FeatureRegistry::new();
        let params: toml::Value = toml::from_str("").unwrap();
        let plugin = registry
            .build_plugin("spiritual_weapon", &params)
            .expect("spiritual_weapon builds from an empty toml table");
        assert_eq!(plugin.id(), "spiritual_weapon");
        assert_eq!(plugin.name(), "Spiritual Weapon");
    }

    /// Guiding Bolt builds from an empty TOML table (its printed 4d6),
    /// proving the registry entry actually reaches `spells::GuidingBoltPlugin`
    /// rather than only being reachable by constructing it directly in Rust.
    #[test]
    fn guiding_bolt_defaults_to_4d6_but_can_be_overridden() {
        let registry = FeatureRegistry::new();
        let params: toml::Value = toml::from_str("plugin = \"guiding_bolt\"").unwrap();
        let plugin = registry
            .build_plugin("guiding_bolt", &params)
            .expect("guiding_bolt builds from an empty toml table");
        assert_eq!(plugin.id(), "guiding_bolt");
        assert_eq!(plugin.name(), "Guiding Bolt");

        let mut builder = crate::dsl::plugin::CreatureBuilder::new("Cleric", 16, 30);
        builder.set_spellcasting(crate::rules::SpellCastingProfile::new(Ability::Wis, 3, 2));
        builder.set_spell_slot_max(1, 2);
        let built = builder
            .apply_feature(plugin.as_ref())
            .expect("guiding_bolt applies to a caster")
            .build()
            .expect("builds");
        let crate::creature::Effect::Strikes { strike, .. } = &built.actions[0].effect else {
            panic!("expected a Strikes effect");
        };
        assert_eq!(
            strike.damage,
            vec![crate::rules::DamageRoll::new(
                4,
                6,
                0,
                crate::rules::DamageKind::Radiant
            )]
        );

        // A different dice pool overrides the printed default, the same way
        // `sneak_attack`'s does.
        let overridden: toml::Value =
            toml::from_str("plugin = \"guiding_bolt\"\ndice_count = 5\ndice_sides = 8").unwrap();
        let plugin = registry
            .build_plugin("guiding_bolt", &overridden)
            .expect("guiding_bolt builds with overridden dice");
        let mut builder = crate::dsl::plugin::CreatureBuilder::new("Cleric", 16, 30);
        builder.set_spellcasting(crate::rules::SpellCastingProfile::new(Ability::Wis, 3, 2));
        builder.set_spell_slot_max(1, 2);
        let built = builder
            .apply_feature(plugin.as_ref())
            .expect("guiding_bolt applies")
            .build()
            .expect("builds");
        let crate::creature::Effect::Strikes { strike, .. } = &built.actions[0].effect else {
            panic!("expected a Strikes effect");
        };
        assert_eq!(
            strike.damage,
            vec![crate::rules::DamageRoll::new(
                5,
                8,
                0,
                crate::rules::DamageKind::Radiant
            )]
        );
    }
}
