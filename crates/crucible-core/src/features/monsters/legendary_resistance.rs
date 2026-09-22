//! Legendary Resistance.

use crate::creature::Rider;
use crate::features::{CreatureBuilder, FeaturePlugin, FeatureRegistry, FeatureResult};

/// Legendary Resistance: Turn a failed save into a success N times per fight.
#[derive(Debug, Clone)]
pub struct LegendaryResistancePlugin {
    pub uses: u32,
}

impl LegendaryResistancePlugin {
    pub fn new(uses: u32) -> Self {
        Self { uses }
    }
}

impl FeaturePlugin for LegendaryResistancePlugin {
    fn id(&self) -> &'static str {
        "legendary_resistance"
    }

    fn name(&self) -> &str {
        "Legendary Resistance"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        builder.add_rider(Rider::AlwaysSucceed {
            uses: self.uses,
            // Legendary Resistance answers any save, and costs nothing but
            // itself; a ring that rescues one kind of save at the price of a
            // reaction fills those two fields in instead.
            ability: None,
            reaction: false,
        });
        Ok(())
    }
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // Legendary Resistance
    registry.register("legendary_resistance", |val| {
        let uses = val.get("uses").and_then(|v| v.as_integer()).unwrap_or(3) as u32;
        Ok(Box::new(LegendaryResistancePlugin::new(uses)))
    });
}
