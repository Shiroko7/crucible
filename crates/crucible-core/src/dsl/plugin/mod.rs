//! Monad Plugin Architecture for combatant features.
//!
//! Rather than hardcoding creatures or branching inside engine procedures,
//! features are modular plugins that transform a `CreatureBuilder` state monad.
//! Missing mechanics are implemented as plugins; creatures themselves are data.

pub mod items;
pub mod registry;
pub mod rogue;
pub mod standard;
pub mod traits;

pub use items::*;
pub use registry::FeatureRegistry;
pub use rogue::*;
pub use standard::*;
pub use traits::{CreatureBuilder, FeatureError, FeaturePlugin, FeatureResult};
