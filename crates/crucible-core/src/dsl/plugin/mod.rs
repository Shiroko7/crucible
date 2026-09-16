//! Monad Plugin Architecture for combatant features.
//!
//! Rather than hardcoding creatures or branching inside engine procedures,
//! features are modular plugins that transform a `CreatureBuilder` state monad.
//! Missing mechanics are implemented as plugins; creatures themselves are data.

pub mod items;
pub mod prestige_spellcasting;
pub mod registry;
pub mod rogue;
pub mod spells;
pub mod standard;
pub mod traits;

pub use items::*;
pub use prestige_spellcasting::*;
pub use registry::FeatureRegistry;
pub use rogue::*;
pub use spells::*;
pub use standard::*;
pub use traits::{CreatureBuilder, FeatureError, FeaturePlugin, FeatureResult};
