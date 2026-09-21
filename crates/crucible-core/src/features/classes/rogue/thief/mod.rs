//! Thief subclass features.

mod fast_hands;

use crate::features::FeatureRegistry;
pub use fast_hands::FastHandsPlugin;

pub(super) fn register(registry: &mut FeatureRegistry) {
    fast_hands::register(registry);
}
