//! Evasion (Monk 7, Rogue 7).

use crate::creature::Rider;
use crate::features::{
    CreatureBuilder, FeatureError, FeaturePlugin, FeatureRegistry, FeatureResult,
};
use crate::rules::Ability;

/// Evasion (Monk 7, Rogue 7): On a Dexterity save, take no damage on a success and half on a fail.
#[derive(Debug, Clone)]
pub struct EvasionPlugin {
    pub ability: Ability,
}

impl EvasionPlugin {
    pub fn new(ability: Ability) -> Self {
        Self { ability }
    }
}

impl FeaturePlugin for EvasionPlugin {
    fn id(&self) -> &'static str {
        "evasion"
    }

    fn name(&self) -> &str {
        "Evasion"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        builder.add_rider(Rider::NothingOnSuccess {
            ability: self.ability,
        });
        Ok(())
    }
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // Evasion
    registry.register("evasion", |val| {
        let ability_str = val.get("ability").and_then(|v| v.as_str()).unwrap_or("dex");
        let ability = Ability::parse(ability_str)
            .ok_or_else(|| FeatureError::UnknownAbility(ability_str.to_string()))?;
        Ok(Box::new(EvasionPlugin::new(ability)))
    });
}
