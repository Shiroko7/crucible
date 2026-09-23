//! Cleric class features (2024 rules), one file per feature.
//!
//! Only the two that need a plugin of their own are here, and both are
//! Channel Divinity options: they read the cleric's own save DC and ability
//! modifier off its casting profile, which is exactly what a written-down
//! number cannot do without going stale.
//!
//! Channel Divinity itself needs nothing: it is a pool of uses, declared the
//! way every pool is (`channel_divinity = 3` under `[pc.resources]`), and
//! each option spends from it. A cleric's other features are already
//! expressible:
//!
//! - **Blessed Strikes / Divine Strike** - "once on each of your turns when
//!   you hit, extra damage" - is [`crate::creature::Rider::OncePerTurnDamage`]
//!   with its numbers filled in, which is a `trait: once per turn 1d8 radiant`
//!   line rather than a plugin (see [`crate::features::classes`]).
//! - **Potent Spellcasting** - "add your Wisdom modifier to the damage of
//!   your cantrips" - is a number on the cantrip's own damage.
//! - **Divine Order**, armour and weapon training, and everything else that
//!   only changes what a cleric is proficient with, is the Armor Class and
//!   attack bonus the sheet already states.
//! - **Divine Intervention** is a free cast of something the cleric already
//!   has, which is `uses 1` on a copy of that move.
//!
//! A domain's own features live wherever their mechanism does rather than in
//! a folder here, because none of them are cleric-specific shapes: a storm
//! domain's reaction against whoever hits it is
//! [`crate::features::RetaliationPlugin`], its maximised lightning is
//! [`crate::features::MaximisedDamagePlugin`], and its "lightning also shoves
//! you" is a `trait: push on lightning up to large` line.

mod divine_spark;
mod turn_undead;

use crate::features::FeatureRegistry;
pub use divine_spark::DivineSparkPlugin;
pub use turn_undead::TurnUndeadPlugin;

pub(super) fn register(registry: &mut FeatureRegistry) {
    divine_spark::register(registry);
    turn_undead::register(registry);
}
