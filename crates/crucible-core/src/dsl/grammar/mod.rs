//! The phrase grammar every creature format shares: a move
//! (`Longbow | ranged | hit +8 | 1d8+4 piercing`), a trait (`evasion dex`),
//! a condition's lifetime (`until save`).
//!
//! `.crucible` scenarios write whole stat blocks in it, TOML configs write
//! their `traits` and `actions` in it, and feature factories that take a
//! move as a parameter (Fast Hands, a components-free cast) parse it here
//! too. The syntax itself is documented on [`crate::dsl::scenario`].

mod duration;
mod lex;
mod moves;
mod traits;

pub use duration::{parse_duration, DurationSpec};
pub use moves::parse_move_external;
pub use traits::{parse_trait_external, TraitEffect};

pub(crate) use lex::{count, number};
pub(crate) use moves::parse_move;
pub(crate) use traits::parse_trait;
