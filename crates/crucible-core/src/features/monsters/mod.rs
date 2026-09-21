//! Monster traits.

mod legendary_resistance;

use crate::features::FeatureRegistry;
pub use legendary_resistance::LegendaryResistancePlugin;

pub(super) fn register(registry: &mut FeatureRegistry) {
    legendary_resistance::register(registry);
}
