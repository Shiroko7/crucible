//! Probability distributions, random number generation, and exact calculations.

pub mod dice;
pub mod exact;
pub mod rng;

pub use dice::Pmf;
pub use exact::{expected_attacks_to_kill, kill_curve};
pub use rng::Rng;
