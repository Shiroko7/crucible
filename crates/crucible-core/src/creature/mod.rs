//! What a combatant is made of: the creature itself, the moves it can take,
//! what each move does, and the triggered modifiers (riders) hanging off it.
//!
//! Everything here is data plus the closed-form maths over it. Running a
//! fight with it is [`crate::sim`]'s job; building one from a class feature,
//! a spell or an item is [`crate::features`]'.

mod combatant;
mod effect;
mod moves;
mod rider;

pub use combatant::Creature;
pub use effect::{AttackKind, Effect, SaveEffect, Strike};
pub use moves::{Cost, Move, MoveKind, Reaction, ReactionTrigger, Resource, Uses};
pub use rider::{
    injury_poison_forcing_save, save_success_probability, saving_throw_against_condition,
    AttackTrigger, Rider,
};
