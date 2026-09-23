//! Class features. One folder per class (and per subclass inside it);
//! a feature more than one class gets lives directly in this folder.
//!
//! A class folder holds only features that need a plugin of their own - one
//! that registers moves, or reads the creature it is applied to. A feature
//! that is nothing but a [`crate::creature::Rider`] with its numbers filled
//! in does not belong here at all: it is a `trait:` phrase, written straight
//! onto whatever has it. A monk's Deflect Attacks, a Heavy Armor Master's
//! flat reduction and any other reaction that eats part of a blow are all
//! `reduce <dice> <types>`, which builds
//! [`crate::creature::Rider::ReduceDamage`] without naming a class.

pub mod cleric;
pub mod rogue;

mod evasion;

use crate::features::FeatureRegistry;
pub use evasion::EvasionPlugin;

pub(super) fn register(registry: &mut FeatureRegistry) {
    cleric::register(registry);
    evasion::register(registry);
    rogue::register(registry);
}
