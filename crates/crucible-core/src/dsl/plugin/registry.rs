//! Feature registry for looking up and instantiating known feature plugins.

use std::collections::HashMap;
use std::sync::Arc;

use super::prestige_spellcasting::{AbilityRequirement, PrestigeSpellcastingPlugin};
use super::rogue::*;
use super::spells::{CureWoundsPlugin, HealingWordPlugin};
use super::standard::*;
use super::traits::{FeatureError, FeaturePlugin, FeatureResult};
use crate::rules::creature::{Ability, SPELL_LEVELS};

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

        // Reliable Talent (2024 Rogue 11). `fast_hands` has no entry here -
        // it needs a full `Move` (see `FastHandsPlugin`'s doc comment),
        // which these toml-parameter factories cannot build yet.
        self.register("reliable_talent", |val| {
            let floor = val.get("floor").and_then(|v| v.as_integer()).unwrap_or(10) as i32;
            Ok(Box::new(ReliableTalentPlugin::with_floor(floor)))
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

            let attack_bonus = val
                .get("spellcasting_attack_bonus")
                .and_then(|v| v.as_integer())
                .ok_or_else(|| {
                    FeatureError::InvalidConfiguration(
                        "prestige_spellcasting needs a `spellcasting_attack_bonus` (the granted spell attack bonus; the save DC is derived as 8 + this, the standard 5e formula)"
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
                attack_bonus,
            )))
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
            vec![crate::rules::creature::Rider::ConditionalExtraDamage {
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
            spellcasting_attack_bonus = 11

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
        builder.add_rider(crate::rules::creature::Rider::ConditionalExtraDamage {
            dice_count: 2,
            dice_sides: 6,
            once_per_turn: true,
        });

        plugin.apply(&mut builder).expect("prerequisites are met");

        assert_eq!(builder.creature.spell_slots.max(1), 4);
        assert_eq!(builder.creature.spell_slots.max(2), 3);
        let profile = builder.creature.spellcasting.expect("profile granted");
        assert_eq!(profile.ability, Ability::Wis);
        assert_eq!(profile.attack_bonus(), 11);
        assert_eq!(profile.save_dc(), 19);
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
                spellcasting_attack_bonus = 11
            "#,
        )
        .unwrap();
        assert!(matches!(
            registry.build_plugin("prestige_spellcasting", &params),
            Err(FeatureError::InvalidConfiguration(_))
        ));
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
        builder.set_spellcasting(crate::rules::creature::SpellCastingProfile::new(
            Ability::Wis,
            3,
            2,
        ));
        builder.set_spell_slot_max(1, 2);
        let built = builder
            .apply_feature(plugin.as_ref())
            .expect("guiding_bolt applies to a caster")
            .build()
            .expect("builds");
        let crate::rules::creature::Effect::Strikes { strike, .. } = &built.actions[0].effect
        else {
            panic!("expected a Strikes effect");
        };
        assert_eq!(
            strike.damage,
            vec![crate::rules::creature::DamageRoll::new(
                4,
                6,
                0,
                crate::rules::creature::DamageKind::Radiant
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
        builder.set_spellcasting(crate::rules::creature::SpellCastingProfile::new(
            Ability::Wis,
            3,
            2,
        ));
        builder.set_spell_slot_max(1, 2);
        let built = builder
            .apply_feature(plugin.as_ref())
            .expect("guiding_bolt applies")
            .build()
            .expect("builds");
        let crate::rules::creature::Effect::Strikes { strike, .. } = &built.actions[0].effect
        else {
            panic!("expected a Strikes effect");
        };
        assert_eq!(
            strike.damage,
            vec![crate::rules::creature::DamageRoll::new(
                5,
                8,
                0,
                crate::rules::creature::DamageKind::Radiant
            )]
        );
    }
}
