//! Simulation engine and statistical analysis for combat resolution.

pub mod analysis;
pub mod arena;
mod fight;
mod policy;
pub mod showcase;

pub use analysis::{evaluate, evaluate_teams, sustained_rounds_to_kill, Summary};
pub use fight::{
    expected_damage, replay_teams, run, run_teams, run_with, Budget, Detail, Die, Frame, FrameKind,
    Outcome, Plan, Replay, Side, Unit, UnitState,
};
pub use policy::Policy;
