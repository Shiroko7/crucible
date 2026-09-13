//! Feature registry for looking up and instantiating known feature plugins.

use std::collections::HashMap;
use std::sync::Arc;

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
    }
}
