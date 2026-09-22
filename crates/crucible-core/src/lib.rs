//! Probability for 5e combat resolution, computed two ways on purpose.
//!
//! The project's question is whether a given encounter is actually dangerous
//! for a specific party, which is answered by simulating it many times under
//! several playstyles. Simulation is easy to get subtly wrong and impossible
//! to check against itself: a rollout engine with a biased d20 or a mishandled
//! critical hit produces perfectly plausible numbers.
//!
//! Subsystems, in dependency order - each uses only the ones before it:
//! - [`prob`]: Probability mass functions, dice convolution, exact curves, and deterministic PRNG.
//! - [`rules`]: Core 5e rules - conditions, damage, saves, checks, spellcasting, attack resolution.
//! - [`creature`]: The combatant model - moves, what they do, and the riders hanging off them.
//! - [`sim`]: The fight engine, playstyle policies, the solver, and CVaR analysis.
//! - [`dsl`]: The move and trait grammar ([`dsl::grammar`]), `.crucible` scenarios, and TOML configs.
//! - [`features`]: The ruleset as plugins, one file per class feature, spell, monster trait or item.
//!
//! One exception: the file formats ([`dsl::scenario`], [`dsl::config`]) build
//! creatures through [`features`], which comes after them. [`features`] itself
//! only ever reaches back into [`dsl::grammar`], never a file format.

pub mod creature;
pub mod dsl;
pub mod features;
pub mod prob;
pub mod rules;
pub mod sim;
