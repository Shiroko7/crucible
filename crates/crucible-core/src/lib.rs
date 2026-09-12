//! Probability for 5e combat resolution, computed two ways on purpose.
//!
//! The project's question is whether a given encounter is actually dangerous
//! for a specific party, which is answered by simulating it many times under
//! several playstyles. Simulation is easy to get subtly wrong and impossible
//! to check against itself: a rollout engine with a biased d20 or a mishandled
//! critical hit produces perfectly plausible numbers.
//!
//! So every rule is implemented twice.
//!
//! - [`dice`] and [`exact`] compute distributions in closed form, by
//!   convolution and dynamic programming. Correct by construction, and far too
//!   slow to scale past a couple of combatants.
//! - [`rng`] and [`combat`] sample. Fast enough for millions of rollouts, and
//!   the path everything downstream will actually use.
//!
//! `tests/exact_vs_sampled.rs` requires them to agree, to a tolerance derived
//! from the standard error rather than from whatever number happened to pass.
//! It is the same idea as a chess engine's perft suite: the cheap path proves
//! the fast path before any of the interesting machinery is built on top.
//!
//! See `DESIGN.md` for where this is going.

pub mod combat;
pub mod dice;
pub mod exact;
pub mod rng;

pub use combat::{
    damage_pmf, outcomes, sample_attacks_to_kill, sample_damage, Attack, Defense, Outcomes,
    Reduction, RollMode,
};
pub use dice::Pmf;
pub use exact::{expected_attacks_to_kill, kill_curve};
pub use rng::Rng;
