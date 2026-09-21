//! Data specification language, formatted configurations, PC/Monster models, and plugin architecture.

pub mod config;
pub mod monster;
pub mod pc;
pub mod scenario;

pub use config::{load_creature_from_file, load_creature_from_str};
pub use monster::MonsterDefinition;
pub use pc::PlayerCharacter;
pub use scenario::parse;
