//! Feature registry for looking up and instantiating known feature plugins.

use crate::features::{
    boon, classes, items, monsters, spellcasting, spells, summons, FeatureError, FeaturePlugin,
    FeatureResult,
};
use std::collections::HashMap;
use std::sync::Arc;

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

    /// Every feature this crate ships, each registered by its own module.
    fn register_defaults(&mut self) {
        boon::register(self);
        classes::register(self);
        items::register(self);
        monsters::register(self);
        spellcasting::register(self);
        spells::register(self);
        summons::register(self);
    }
}
