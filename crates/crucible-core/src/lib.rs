//! Probability for 5e combat resolution, computed two ways on purpose.
//!
//! The project's question is whether a given encounter is actually dangerous
//! for a specific party, which is answered by simulating it many times under
//! several playstyles. Simulation is easy to get subtly wrong and impossible
//! to check against itself: a rollout engine with a biased d20 or a mishandled
//! critical hit produces perfectly plausible numbers.
//!
//! Subsystems:
//! - [`prob`]: Probability mass functions, dice convolution, exact curves, and deterministic PRNG.
//! - [`rules`]: 5e combat rules, damage reduction, attack resolution, and combatant models.
//! - [`sim`]: Simulation loop, AI policies, MCTS solver, and statistical CVaR analysis.
//! - [`dsl`]: Data-driven configurations, Monad Plugin Architecture, PC/Monster abstractions, and scenario parsing.

pub mod dsl;
pub mod prob;
pub mod rules;
pub mod sim;

// Subsystem aliases for backward-compatibility
pub use dsl::scenario;
pub use prob::dice;
pub use prob::exact;
pub use prob::rng;
pub use rules::combat;
pub use rules::creature;
pub use sim::analysis;
pub use sim::duel;

// Top-level re-exports (backward compatibility)
pub use analysis::{evaluate, sustained_rounds_to_kill, Summary};
pub use combat::{
    damage_pmf, hit_outcomes, hit_outcomes_with, hit_outcomes_with_reaction, outcomes,
    resolve_mode, sample_attacks_to_kill, sample_damage, sample_hit, sample_hit_with,
    sample_hit_with_reaction, sample_save_with, save_success_chance, Attack, AttackModifier,
    DamageRider, Defense, Landed, Outcomes, Reduction, RollMode, SaveModifier,
};
pub use creature::{
    Ability, AttackTrigger, Condition, Cost, Creature, DamageKind, DamageRoll, Duration, Effect,
    HealRoll, Move, Resource, Rider, SaveEffect, Strike, Uses,
};
pub use dice::Pmf;
pub use duel::{run, Outcome, Policy, Side};
pub use exact::{expected_attacks_to_kill, kill_curve};
pub use rng::Rng;

// Monad Plugin and PC / Monster abstractions
pub use dsl::{
    load_creature_from_file, load_creature_from_str, CreatureBuilder, FeatureError, FeaturePlugin,
    FeatureRegistry, FeatureResult, MonsterDefinition, PlayerCharacter,
};
