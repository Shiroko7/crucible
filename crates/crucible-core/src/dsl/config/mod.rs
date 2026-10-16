//! Configuration loading and deserialization for PC and Monster files.
//!
//! A TOML file declares one `[pc]` or `[monster]`: stat block fields, trait
//! phrases and moves written in [`crate::dsl::grammar`], and a list of
//! registry features by id - see [`crate::features::FeatureRegistry`].

mod monster;
mod pc;

use crate::dsl::grammar::TraitEffect;
pub use monster::MonsterDefinition;
pub use pc::PlayerCharacter;

use crate::creature::{Creature, Move, Reaction};
use crate::features::{CreatureBuilder, FeatureError, FeatureRegistry, FeatureResult};
use crate::rules::{Ability, SpellCastingProfile};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

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
        if !(1..=crate::rules::SPELL_LEVELS).contains(&level) {
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
    crate::dsl::grammar::parse_trait_external(value).map_err(FeatureError::InvalidConfiguration)
}

/// Parse a move entry into a `Move` given the current combatant context.
pub fn parse_move_entry(entry: &MoveEntry, owner: &Creature) -> FeatureResult<Move> {
    let full = format!("{} | {}", entry.name, entry.effect);
    crate::dsl::grammar::parse_move_external(&full, owner)
        .map_err(FeatureError::InvalidConfiguration)
}

/// Parse a reaction entry - a move whose effect carries a `when ...` trigger
/// clause - given the current combatant context.
pub fn parse_reaction_entry(entry: &MoveEntry, owner: &Creature) -> FeatureResult<Reaction> {
    let full = format!("{} | {}", entry.name, entry.effect);
    crate::dsl::grammar::parse_reaction_external(&full, owner)
        .map_err(FeatureError::InvalidConfiguration)
}

/// Load a `Creature` from a formatted TOML configuration string.
/// Give `creature` the things anybody can do with an action - Dodge,
/// Disengage, Help, Shove and Grapple - each marked
/// [`crate::creature::Rider::Universal`], after everything its own file
/// gave it. `proficiency` is its proficiency bonus, which with its Strength
/// sets what a shove or a grapple has to be saved against.
pub(crate) fn add_universal_actions(creature: &mut Creature, proficiency: i32) {
    use crate::creature::{Effect, Move, Rider, SaveEffect};
    use crate::rules::{Ability, Condition, Duration};
    let strength = match creature.ability_score(Ability::Str) {
        0 => creature.save(Ability::Str),
        score => (score - 10).div_euclid(2),
    };
    let dc = 8 + strength + proficiency;
    let unarmed = |condition: Condition, duration: Duration| {
        Effect::Save(SaveEffect {
            ability: Ability::Str,
            dc,
            damage: Vec::new(),
            half_on_success: false,
            on_failure: vec![(condition, duration)],
            max_targets: Some(1),
            requires_type: None,
        })
    };
    let moves = [
        Move::new(
            "Dodge",
            Effect::Stance {
                condition: Condition::Dodging,
            },
        ),
        Move::new("Disengage", Effect::Sequence(Vec::new())),
        Move::new(
            "Help",
            Effect::Afflict {
                condition: Condition::Marked,
                duration: Duration::ApplierTurn,
            },
        )
        .with_range(5),
        Move::new("Shove", unarmed(Condition::Prone, Duration::VictimTurn)).with_range(5),
        Move::new(
            "Grapple",
            unarmed(
                Condition::Grappled,
                Duration::SaveEndTurn {
                    ability: Ability::Str,
                    dc,
                },
            ),
        )
        .with_range(5),
    ];
    for m in moves {
        creature.actions.push(m.with_rider(Rider::Universal));
    }
}

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

    /// The example character the repo commits: the one a new one is
    /// copied from, so it has to load.
    #[test]
    fn the_example_character_loads() {
        let registry = FeatureRegistry::new();
        let cleric = load_creature_from_file("content/characters/cleric.toml", &registry)
            .expect("the example character parses and compiles to a creature");
        assert_eq!(cleric.name, "Cleric");
        assert_eq!(cleric.ac, 18);
        assert_eq!(cleric.hp, 38);
        assert_eq!(cleric.saves[crate::rules::Ability::Wis.index()], 7);
        assert_eq!(cleric.creature_type, None);
        assert!(cleric.spellcasting.is_some());
        // Its weapon is written out; the rest of what it does is its spells.
        let names: Vec<&str> = cleric.actions.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"Mace"), "{names:?}");
        assert!(names.len() > 1, "{names:?}");
    }

    /// And the example monster.
    #[test]
    fn the_example_monster_loads() {
        let registry = FeatureRegistry::new();
        let brute = load_creature_from_file("content/monsters/brute.toml", &registry)
            .expect("the example monster parses and compiles to a creature");
        assert_eq!(brute.name, "Brute");
        assert_eq!(brute.ac, 11);
        assert_eq!(brute.hp, 59);
        assert_eq!(brute.initiative, -1);
        assert_eq!(brute.speed, 40);
        // Its own, and the five things anybody can do.
        assert_eq!(brute.actions.len(), 2 + 5);
        assert_eq!(brute.creature_type, Some(crate::rules::CreatureType::Giant));
        assert_eq!(brute.size, crate::rules::Size::Large);
    }

    #[test]
    fn a_monster_with_legendary_actions_and_spells_of_its_own_loads() {
        let registry = FeatureRegistry::new();
        let toml = r#"
            [monster]
            name = "Wyrm"
            size = "Huge"
            creature_type = "Dragon"
            ac = 19
            hp = 250
            initiative = 12
            immune = ["fire"]
            legendary_uses = 3
            traits = ["legendary resistance 3"]

            [[monster.actions]]
            name = "Breath"
            effect = "recharge 5 | save dex dc 21 | 17d6 fire | half on success"

            [[monster.actions]]
            name = "Claws"
            effect = "strikes 3 | hit +14 | 1d10+8 slashing, 2d4 fire"

            [[monster.actions]]
            name = "Burst"
            effect = "spell | uses 1 | save dex dc 20 | 8d6 fire | half on success"

            [[monster.legendary]]
            name = "Rays"
            effect = "spell | ranged | recharge 1 | strikes 3 | hit +12 | 2d6 fire"

            [[monster.legendary]]
            name = "Pounce"
            effect = "recharge 1 | strikes 1 | hit +14 | 1d10+8 slashing, 2d4 fire"
        "#;
        let wyrm = load_creature_from_str(toml, &registry).expect("it parses");
        assert_eq!(wyrm.initiative, 12);
        // Its own, and the five things anybody can do.
        assert_eq!(wyrm.actions.len(), 3 + 5);
        assert_eq!(wyrm.legendary_uses, 3);
        assert_eq!(wyrm.legendary.len(), 2);
        assert_eq!(wyrm.riders.len(), 1);
        assert_eq!(wyrm.creature_type, Some(crate::rules::CreatureType::Dragon));
        assert_eq!(wyrm.size, crate::rules::Size::Huge);
        // What it casts is a spell; its claws are melee weapon attacks.
        use crate::creature::MoveKind;
        for m in wyrm.actions.iter().chain(&wyrm.legendary) {
            let spell = matches!(m.name.as_str(), "Burst" | "Rays");
            assert_eq!(m.kind == MoveKind::Spell, spell, "{}", m.name);
        }
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

    #[test]
    fn a_monster_with_no_declared_size_defaults_to_medium() {
        let registry = FeatureRegistry::new();
        let toml = r#"
            [monster]
            name = "Bandit"
            ac = 12
            hp = 11
        "#;
        let bandit =
            load_creature_from_str(toml, &registry).expect("a monster with no size parses");
        assert_eq!(bandit.size, crate::rules::Size::Medium);
    }

    #[test]
    fn an_unknown_size_is_rejected() {
        let registry = FeatureRegistry::new();
        let toml = r#"
            [monster]
            name = "Mystery"
            ac = 10
            hp = 10
            size = "Colossal"
        "#;
        let err = load_creature_from_str(toml, &registry)
            .expect_err("an unrecognised size must be rejected, not silently dropped");
        assert!(matches!(err, FeatureError::UnknownSize(_)));
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
            crate::rules::Ability::Str,
            crate::rules::Ability::Dex,
            crate::rules::Ability::Con,
            crate::rules::Ability::Int,
            crate::rules::Ability::Wis,
            crate::rules::Ability::Cha,
        ] {
            assert_eq!(caster.save(ability), 1, "every save gets the flat +1");
        }
        // 4 (INT mod) + 3 (proficiency) + 2 (item, from the trait rather than
        // an inline `item_bonus`) = 9; DC is 8 + that.
        assert_eq!(caster.spell_attack_bonus(), Some(9));
        assert_eq!(caster.spell_save_dc(), Some(17));
        // The item-granted resistances sit alongside `resist = ["cold"]`
        // rather than replacing it.
        use crate::rules::{DamageKind, Reduction};
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
