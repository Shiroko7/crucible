//! Simulation engine and statistical analysis for combat resolution.

pub mod analysis;
pub mod duel;

pub use analysis::{evaluate, evaluate_teams, sustained_rounds_to_kill, Summary};
pub use duel::{run, run_teams, Budget, Outcome, Policy, Side};
