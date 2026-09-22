//! Class features. One folder per class (and per subclass inside it);
//! a feature more than one class gets lives directly in this folder.

pub mod monk;
pub mod rogue;

mod evasion;

use crate::features::FeatureRegistry;
pub use evasion::EvasionPlugin;

pub(super) fn register(registry: &mut FeatureRegistry) {
    evasion::register(registry);
    rogue::register(registry);
}
