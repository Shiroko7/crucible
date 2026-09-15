//! Configuration loading and deserialization for PC and Monster files.

use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use super::monster::MonsterDefinition;
use super::pc::PlayerCharacter;
use super::plugin::{CreatureBuilder, FeatureError, FeatureRegistry, FeatureResult};
use super::scenario::TraitEffect;
use crate::rules::creature::{Ability, Creature, Move, SpellCastingProfile};

#[derive(Debug, Clone, Deserialize)]
pub struct MoveEntry {
    pub name: String,
    pub effect: String,
}

/// A creature's `[*.resources]` table: named pools (`focus = 8`) plus the
/// nested, special-cased spell slot table.
///
/// Spell slots are nested under `resources` rather than living alongside
/// `focus` and friends because they are not one flat pool - they are nine
/// independent counters, one per level - so `[*.resources.slots]` gets its own
/// sub-table instead of a scalar entry.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ResourcesConfig {
    #[serde(flatten)]
    pub pools: HashMap<String, u32>,
    #[serde(default)]
    pub slots: HashMap<String, u32>,
}

/// A creature's `[*.spellcasting]` table: the ability and numbers behind its
/// spell attack bonus and save DC.
#[derive(Debug, Clone, Deserialize)]
pub struct SpellcastingConfig {
    pub ability: String,
    #[serde(default)]
    pub ability_modifier: i32,
    #[serde(default)]
    pub proficiency_bonus: i32,
    #[serde(default)]
    pub item_bonus: i32,
}

impl SpellcastingConfig {
    pub fn to_profile(&self) -> FeatureResult<SpellCastingProfile> {
        let ability = Ability::parse(&self.ability)
            .ok_or_else(|| FeatureError::UnknownAbility(self.ability.clone()))?;
        Ok(
            SpellCastingProfile::new(ability, self.ability_modifier, self.proficiency_bonus)
                .with_item_bonus(self.item_bonus),
        )
    }
}

/// Parse a `[*.resources]` table into the builder: scalar entries become
/// named resource pools, and the nested `slots` table (level -> max, e.g.
/// `[pc.resources.slots]` with `1 = 4`) becomes the creature's spell slots.
///
/// TOML bare keys are always strings even when they look like integers, which
/// is why `slots` is keyed by level number written as text.
pub fn apply_resources(
    builder: &mut CreatureBuilder,
    resources: &ResourcesConfig,
) -> FeatureResult<()> {
    for (name, &max) in &resources.pools {
        builder.ensure_resource(name, max);
    }
    for (level_str, &max) in &resources.slots {
        let level: u32 = level_str.parse().map_err(|_| {
            FeatureError::InvalidConfiguration(format!(
                "spell slot level '{level_str}' is not a number"
            ))
        })?;
        if !(1..=crate::rules::creature::SPELL_LEVELS).contains(&level) {
            return Err(FeatureError::InvalidConfiguration(format!(
                "spell slot level {level} is out of range 1-9"
            )));
        }
        builder.set_spell_slot_max(level, max);
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
struct RootDocument {
    pub pc: Option<PlayerCharacter>,
    pub monster: Option<MonsterDefinition>,
}

/// Parse a trait definition string (e.g. "evasion dex", "legendary resistance 3",
/// "ac 2").
pub fn parse_trait_str(value: &str) -> FeatureResult<TraitEffect> {
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
        assert_eq!(
            dragon.creature_type,
            Some(crate::rules::creature::CreatureType::Dragon)
        );
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
        assert_eq!(
            ogre.creature_type,
            Some(crate::rules::creature::CreatureType::Giant)
        );
    }

    #[test]
    fn an_unknown_creature_type_is_rejected() {
        let registry = FeatureRegistry::new();
        let toml = r#"
            [monster]
            name = "Mystery"
            ac = 10
            hp = 10
            creature_type = "Beholder-Kin"
        "#;
        let err = load_creature_from_str(toml, &registry)
            .expect_err("an unrecognised creature type must be rejected, not silently dropped");
        assert!(matches!(err, FeatureError::UnknownCreatureType(_)));
    }

    /// `[pc.resources.slots]` mirrors the existing `[pc.resources]`
    /// convention for a flat pool, but nests because a caster's slots are
    /// nine independent counters rather than one.
    #[test]
    fn a_spellcaster_pc_loads_slots_and_a_casting_profile() {
        let registry = FeatureRegistry::new();
        let toml = r#"
            [pc]
            name = "Test Wizard"
            ac = 12
            hp = 30

            [pc.resources]
            focus = 2

            [pc.resources.slots]
            1 = 4
            2 = 3
            3 = 2

            [pc.spellcasting]
            ability = "int"
            ability_modifier = 4
            proficiency_bonus = 3
            item_bonus = 1
        "#;

        let wizard = load_creature_from_str(toml, &registry)
            .expect("a pc with slots and a spellcasting profile parses");

        // The flat pool alongside the nested slot table still loads.
        assert_eq!(wizard.resources[0].name, "focus");
        assert_eq!(wizard.resources[0].max, 2);

        assert_eq!(wizard.spell_slots.max(1), 4);
        assert_eq!(wizard.spell_slots.available(1), 4);
        assert_eq!(wizard.spell_slots.max(2), 3);
        assert_eq!(wizard.spell_slots.max(3), 2);
        // A level never mentioned in the config starts at zero.
        assert_eq!(wizard.spell_slots.max(4), 0);

        // 4 (INT mod) + 3 (proficiency) + 1 (item) = 8; DC is 8 + that.
        assert_eq!(wizard.spell_attack_bonus(), Some(8));
        assert_eq!(wizard.spell_save_dc(), Some(16));
    }

    /// Real items grant different subsets of AC, saves, spellcasting and
    /// resistance - one bundle might only grant two of the four - so a PC has
    /// to be able to carry several such items (here folded into one `traits`
    /// list, the way several equipped items would each contribute their own
    /// line) and have every one of them apply at once.
    #[test]
    fn a_pc_can_carry_every_stat_boost_trait_at_once() {
        let registry = FeatureRegistry::new();
        let toml = r#"
            [pc]
            name = "Test Caster"
            ac = 12
            hp = 30
            resist = ["cold"]
            traits = ["ac 2", "saves 1", "spell 2", "resistance fire, necrotic"]

            [pc.spellcasting]
            ability = "int"
            ability_modifier = 4
            proficiency_bonus = 3
        "#;

        let caster =
            load_creature_from_str(toml, &registry).expect("every stat-boost trait applies");

        assert_eq!(caster.ac, 14, "12 base + a 2-point item bonus");
        for ability in [
            crate::rules::creature::Ability::Str,
            crate::rules::creature::Ability::Dex,
            crate::rules::creature::Ability::Con,
            crate::rules::creature::Ability::Int,
            crate::rules::creature::Ability::Wis,
            crate::rules::creature::Ability::Cha,
        ] {
            assert_eq!(caster.save(ability), 1, "every save gets the flat +1");
        }
        // 4 (INT mod) + 3 (proficiency) + 2 (item, from the trait rather than
        // an inline `item_bonus`) = 9; DC is 8 + that.
        assert_eq!(caster.spell_attack_bonus(), Some(9));
        assert_eq!(caster.spell_save_dc(), Some(17));
        // The item-granted resistances sit alongside `resist = ["cold"]`
        // rather than replacing it.
        use crate::rules::combat::Reduction;
        use crate::rules::creature::DamageKind;
        assert_eq!(caster.reduction(DamageKind::Cold), Reduction::Resistant);
        assert_eq!(caster.reduction(DamageKind::Fire), Reduction::Resistant);
        assert_eq!(caster.reduction(DamageKind::Necrotic), Reduction::Resistant);
        assert_eq!(caster.reduction(DamageKind::Acid), Reduction::Normal);
    }

    /// The same guard as the scenario-format DSL: an item's spell attack/DC
    /// bonus has nothing to add itself to on a creature with no
    /// `[pc.spellcasting]` table at all.
    #[test]
    fn a_spell_bonus_trait_without_spellcasting_is_rejected() {
        let registry = FeatureRegistry::new();
        let toml = r#"
            [pc]
            name = "Not A Caster"
            ac = 12
            hp = 30
            traits = ["spell 2"]
        "#;

        let err = load_creature_from_str(toml, &registry)
            .expect_err("no spellcasting profile to add the bonus to");
        assert!(matches!(err, FeatureError::InvalidConfiguration(_)));
    }

    #[test]
    fn an_out_of_range_slot_level_is_rejected() {
        let registry = FeatureRegistry::new();
        let toml = r#"
            [pc]
            name = "Bad Config"
            ac = 10
            hp = 10

            [pc.resources.slots]
            10 = 1
        "#;

        let err = load_creature_from_str(toml, &registry)
            .expect_err("a slot level outside 1-9 must be rejected, not silently ignored");
        assert!(matches!(err, FeatureError::InvalidConfiguration(_)));
    }
}
