//! 5e combat mechanics, damage systems, and combatant definitions.

pub mod check;
pub mod combat;
pub mod creature;

pub use check::CheckRoll;
pub use combat::{
    damage_pmf, hit_outcomes, outcomes, sample_attacks_to_kill, sample_damage, sample_hit, Attack,
    Defense, Landed, Outcomes, Reduction, RollMode,
};
pub use creature::{
    Ability, Condition, Cost, Creature, DamageKind, DamageRoll, Duration, Effect, Move, MoveKind,
    Resource, Rider, SaveEffect, Strike, Uses,
};
