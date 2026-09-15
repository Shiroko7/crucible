//! Feature registry for looking up and instantiating known feature plugins.

use std::collections::HashMap;
use std::sync::Arc;

use super::rogue::*;
use super::standard::*;
use super::traits::{FeatureError, FeaturePlugin, FeatureResult};
use crate::rules::creature::Ability;

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
}
