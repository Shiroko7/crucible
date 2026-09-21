//! Simulation engine and statistical analysis for combat resolution.

pub mod analysis;
mod fight;
mod policy;

pub use analysis::{evaluate, evaluate_teams, sustained_rounds_to_kill, Summary};
pub use fight::{expected_damage, run, run_teams, run_with, Budget, Outcome, Plan, Side};
pub use policy::Policy;
