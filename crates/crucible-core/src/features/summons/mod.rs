//! Creatures a combatant can call up beside it.
//!
//! A summon is not a class feature, a spell or an item - a spell, an essence
//! art and a wondrous item can all produce the same thing - so it lives in
//! its own folder, parameterised by what the double is and what its summoner
//! can tell it to do.

mod commanded_double;

use crate::features::FeatureRegistry;
pub use commanded_double::CommandedDoublePlugin;

pub(super) fn register(registry: &mut FeatureRegistry) {
    commanded_double::register(registry);
}
