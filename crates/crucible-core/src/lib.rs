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

pub mod creature;
pub mod dsl;
pub mod features;
pub mod prob;
pub mod rules;
pub mod sim;
