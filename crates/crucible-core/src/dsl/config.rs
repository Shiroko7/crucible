//! Configuration loading and deserialization for PC and Monster files.

use serde::Deserialize;
use std::fs;
use std::path::Path;

use super::monster::MonsterDefinition;
use super::pc::PlayerCharacter;
use super::plugin::{FeatureError, FeatureRegistry, FeatureResult};
use crate::rules::creature::{Creature, Move, Rider};

#[derive(Debug, Clone, Deserialize)]
pub struct MoveEntry {
    pub name: String,
    pub effect: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RootDocument {
    pub pc: Option<PlayerCharacter>,
    pub monster: Option<MonsterDefinition>,
}

/// Parse a trait definition string (e.g. "evasion dex", "legendary resistance 3").
pub fn parse_trait_str(value: &str) -> FeatureResult<Rider> {
    crate::dsl::scenario::parse_trait_external(value).map_err(FeatureError::InvalidConfiguration)
}

/// Parse a move entry into a `Move` given the current combatant context.
pub fn parse_move_entry(entry: &MoveEntry, owner: &Creature) -> FeatureResult<Move> {
    let full = format!("{} | {}", entry.name, entry.effect);
    crate::dsl::scenario::parse_move_external(&full, owner)
        .map_err(FeatureError::InvalidConfiguration)
}

/// Load a `Creature` from a formatted TOML configuration string.
pub fn load_creature_from_str(
    content: &str,
    registry: &FeatureRegistry,
) -> FeatureResult<Creature> {
    let root: RootDocument = toml::from_str(content)
        .map_err(|e| FeatureError::InvalidConfiguration(format!("TOML syntax error: {e}")))?;

    if let Some(pc) = root.pc {
        return pc.to_creature(registry);
    }
    if let Some(monster) = root.monster {
        return monster.to_creature(registry);
    }

    // Try parsing directly as PlayerCharacter
    if let Ok(pc) = toml::from_str::<PlayerCharacter>(content) {
        if !pc.name.is_empty() && pc.hp > 0 {
            return pc.to_creature(registry);
        }
    }

    // Try parsing directly as MonsterDefinition
    if let Ok(monster) = toml::from_str::<MonsterDefinition>(content) {
        if !monster.name.is_empty() && monster.hp > 0 {
            return monster.to_creature(registry);
        }
    }

    Err(FeatureError::InvalidConfiguration(
        "expected a [pc] or [monster] table in configuration file".into(),
    ))
}

/// Load a `Creature` from a TOML configuration file on disk.
pub fn load_creature_from_file(
    path: impl AsRef<Path>,
    registry: &FeatureRegistry,
) -> FeatureResult<Creature> {
    let path_ref = path.as_ref();
    let text = match fs::read_to_string(path_ref) {
        Ok(t) => t,
        Err(_) => {
            // Check relative to workspace root or ancestors if running from a crate subdir
            let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
            let mut resolved = None;
            if !manifest_dir.is_empty() {
                let p = Path::new(&manifest_dir).join("../../").join(path_ref);
                if p.exists() {
                    resolved = Some(p);
                } else {
                    let p2 = Path::new(&manifest_dir).join("../").join(path_ref);
                    if p2.exists() {
                        resolved = Some(p2);
                    }
                }
            }
            if let Some(r) = resolved {
                fs::read_to_string(&r).map_err(|e| {
                    FeatureError::InvalidConfiguration(format!(
                        "failed to read file '{}': {e}",
                        r.display()
                    ))
                })?
            } else {
                return Err(FeatureError::InvalidConfiguration(format!(
                    "failed to find or read file '{}'",
                    path_ref.display()
                )));
            }
        }
    };
    load_creature_from_str(&text, registry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_gio_pc_config() {
        let registry = FeatureRegistry::new();
        let gio = load_creature_from_file("content/characters/gio.toml", &registry)
            .expect("gio config parses and compiles to creature");
        assert_eq!(gio.name, "Gio");
        assert_eq!(gio.ac, 20);
        assert_eq!(gio.hp, 69);
        assert_eq!(gio.initiative, -1);
        assert_eq!(gio.actions.len(), 2);
        assert_eq!(gio.bonus_actions.len(), 3);
        assert_eq!(gio.riders.len(), 2);
        assert!(gio.has_evasion(crate::rules::creature::Ability::Dex));
        assert_eq!(gio.resources[0].name, "focus");
        assert_eq!(gio.resources[0].max, 8);
    }

    #[test]
    fn test_load_adult_red_dragon_monster_config() {
        let registry = FeatureRegistry::new();
        let dragon = load_creature_from_file("content/monsters/adult-red-dragon.toml", &registry)
            .expect("adult red dragon config parses and compiles to creature");
        assert_eq!(dragon.name, "Adult Red Dragon");
        assert_eq!(dragon.ac, 19);
        assert_eq!(dragon.hp, 256);
        assert_eq!(dragon.initiative, 12);
        assert_eq!(dragon.actions.len(), 4);
        assert_eq!(dragon.legendary_uses, 3);
        assert_eq!(dragon.legendary.len(), 3);
        assert_eq!(dragon.riders.len(), 1);
    }

    #[test]
    fn test_load_ogre_monster_config() {
        let registry = FeatureRegistry::new();
        let ogre = load_creature_from_file("content/monsters/ogre.toml", &registry)
            .expect("ogre config parses and compiles to creature");
        assert_eq!(ogre.name, "Ogre");
        assert_eq!(ogre.ac, 11);
        assert_eq!(ogre.hp, 68);
        assert_eq!(ogre.initiative, -1);
        assert_eq!(ogre.actions.len(), 2);
    }
}
