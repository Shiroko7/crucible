//! Features about *how* a creature casts, rather than any one spell: a
//! granted spellcasting progression, a way past casting restrictions.

mod bypass_restrictions;
mod prestige;

use crate::features::FeatureRegistry;
pub use bypass_restrictions::BypassCastingRestrictionsPlugin;
pub use prestige::{AbilityRequirement, PrestigeSpellcastingPlugin};

pub(super) fn register(registry: &mut FeatureRegistry) {
    bypass_restrictions::register(registry);
    prestige::register(registry);
}
