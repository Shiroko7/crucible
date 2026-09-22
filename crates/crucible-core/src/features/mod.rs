//! The ruleset: every class feature, spell, monster trait and item this crate
//! knows, each one a [`FeaturePlugin`] in its own file.
//!
//! Rather than hardcoding creatures or branching inside engine procedures,
//! features are modular plugins that transform a [`CreatureBuilder`].
//! Missing mechanics are implemented as plugins; creatures themselves are
//! data. A feature lives in the narrowest folder that covers everything
//! that has it - Evasion sits directly under [`classes`] because both the
//! Monk and the Rogue get it - and registers its own TOML factory, so adding
//! one never means editing a shared list.

pub mod classes;
pub mod items;
pub mod monsters;
pub mod spellcasting;
pub mod spells;
pub mod summons;

mod boon;
mod plugin;
mod registry;

pub use boon::LastingBoonPlugin;
pub use plugin::{CreatureBuilder, FeatureError, FeaturePlugin, FeatureResult};
pub use registry::{FeatureRegistry, PluginFactory};
