//! Feature registry for looking up and instantiating known feature plugins.

use std::collections::HashMap;
use std::sync::Arc;

use super::rogue::*;
use super::spells::*;
use super::standard::*;
use super::traits::{FeatureError, FeaturePlugin, FeatureResult};
use crate::rules::creature::Ability;

/// Read a spell plugin's optional `resource` (a pool name) and `cost` (an
/// amount, default 1) TOML params into a [`SpellCost`].
///
/// Shared by every spell factory below so "how it's paid for" stays a
/// declared parameter rather than a name a plugin invents itself: a caster's
/// TOML names whichever pool it already declared under `[*.resources]` -
/// a real slot pool or a wand's own charges - and leaving `resource` off
/// entirely makes the cast free.
fn parse_spell_cost(val: &toml::Value) -> Option<SpellCost> {
    let resource = val.get("resource").and_then(|v| v.as_str())?;
    let amount = val.get("cost").and_then(|v| v.as_integer()).unwrap_or(1) as u32;
    Some(SpellCost::new(resource, amount))
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

        // Reliable Talent (2024 Rogue 11). `fast_hands` has no entry here -
        // it needs a full `Move` (see `FastHandsPlugin`'s doc comment),
        // which these toml-parameter factories cannot build yet.
        self.register("reliable_talent", |val| {
            let floor = val.get("floor").and_then(|v| v.as_integer()).unwrap_or(10) as i32;
            Ok(Box::new(ReliableTalentPlugin::with_floor(floor)))
        });

        // Hold Person
        self.register("hold_person", |_val| Ok(Box::new(HoldPersonPlugin::new())));

        // Blindness/Deafness (SRD 5.2, 2nd level)
        self.register("blindness_deafness", |val| {
            let deafen = val.get("deafen").and_then(|v| v.as_bool()).unwrap_or(false);
            let cost = parse_spell_cost(val);
            Ok(Box::new(BlindnessDeafnessPlugin::new(deafen, cost)))
        });

        // Command (SRD 5.2, 1st level)
        self.register("command", |val| {
            let word_str = val.get("word").and_then(|v| v.as_str()).unwrap_or("grovel");
            let word = CommandWord::parse(word_str).ok_or_else(|| {
                FeatureError::InvalidConfiguration(format!("unknown command word '{word_str}'"))
            })?;
            let cost = parse_spell_cost(val);
            Ok(Box::new(CommandPlugin::new(word, cost)))
        });

        // Magic Missile (SRD 5.2, 1st level)
        self.register("magic_missile", |val| {
            let cost = parse_spell_cost(val);
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
            Ok(Box::new(CunningStrikePlugin::new(
                dex_modifier,
                proficiency_bonus,
            )))
        });

        // Steady Aim (2024 Rogue 2)
        self.register("steady_aim", |_val| Ok(Box::new(SteadyAimPlugin::new())));

        // Cunning Action (2024 Rogue 2)
        self.register("cunning_action", |_val| {
            Ok(Box::new(CunningActionPlugin::new()))
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

    use crate::dsl::plugin::CreatureBuilder;
    use crate::rules::creature::{Ability, Condition, Duration, Effect, SpellCastingProfile};

    fn spellcaster() -> CreatureBuilder {
        let mut builder = CreatureBuilder::new("Wizard", 12, 30);
        builder.set_spellcasting(SpellCastingProfile::new(Ability::Int, 4, 3));
        builder
    }

    #[test]
    fn magic_missile_builds_from_toml_and_spends_a_named_pool() {
        let registry = FeatureRegistry::new();
        let mut builder = spellcaster();
        builder.ensure_resource("spell_slots_1", 4);

        let params: toml::Value =
            toml::from_str("plugin = \"magic_missile\"\nresource = \"spell_slots_1\"").unwrap();
        let plugin = registry
            .build_plugin("magic_missile", &params)
            .expect("magic_missile builds from toml");
        assert_eq!(plugin.id(), "magic_missile");
        plugin.apply(&mut builder).unwrap();

        let cost = builder.creature.actions[0].cost.expect("cost resolved");
        assert_eq!(
            builder.creature.resources[cost.resource].name,
            "spell_slots_1"
        );
        assert_eq!(cost.amount, 1);
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
            vec![crate::rules::creature::Rider::CunningStrike { dc: 15 }]
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
}
