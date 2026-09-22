//! Generic magic item mechanisms.
//!
//! Reusable engine-level plugins for the *shape* an item's activation takes,
//! parameterised rather than hardcoded to any one published item. Non-SRD
//! items are user-supplied data (see `README.md`'s "Content and
//! Configuration"), so nothing in this module names one.

mod limited_use_debuff;

use crate::features::FeatureRegistry;
pub use limited_use_debuff::LimitedUseDebuffItemPlugin;

pub(super) fn register(registry: &mut FeatureRegistry) {
    limited_use_debuff::register(registry);
}
