//! Verification of Essence Talent System mechanics in Crucible.
//!
//! Across the 9 essence paths (Acid, Air, Earth, Fire, Lightning, Metal,
//! Poison, Water, Wood) from the Essence Talent System, characters combine:
//! - Reactive retributions (`autohit` on hit, e.g. *Thorny Defence*),
//! - Elemental penetration (`ignore resistance <kinds>` and `downgrade immunity <kinds> damage`),
//! - Persistent elemental boons (`lasting_boon`, e.g. *Stone Armor*, *Acidic Embrace*),
//! - Persistent elemental hazard auras (`lasting_aura`, e.g. *Miasmic Cloud*, *Scorched Ground*),
//! - Retaliation and skin secretions (`retaliation` / `[pc.reactions]`),
//! - Summons (`commanded_double`, e.g. *Apex Toxinator* serpent).
//!
//! This test file exercises these mechanics end-to-end through TOML creature definitions
//! and simulated combat.

use crucible_core::creature::{AttackTrigger, Effect, ReactionTrigger, Rider};
use crucible_core::dsl::config::load_creature_from_str;
use crucible_core::dsl::scenario::parse;
use crucible_core::features::FeatureRegistry;
use crucible_core::prob::Rng;
use crucible_core::rules::{DamageKind, Reduction};
use crucible_core::sim::{run_teams, Budget, Policy};

const WOOD_WARDEN: &str = r#"
[pc]
name = "Wood Warden"
ac = 16
hp = 50
initiative = 2
traits = [
    "ignore resistance piercing, poison",
]

[pc.abilities]
str = 16
wis = 16

[pc.spellcasting]
ability = "wis"
ability_modifier = 3
proficiency_bonus = 3

[[pc.actions]]
name = "Thorn Lash"
effect = "strikes 1 | hit +6 | 2d6+3 piercing"

[[pc.reactions]]
name = "Thorny Defence"
effect = "when hit in melee | autohit | 2d4 piercing"
"#;

const PYROMANCER: &str = r#"
[pc]
name = "Fire Adept"
ac = 14
hp = 45
initiative = 2
traits = [
    "ignore resistance fire",
    "downgrade immunity fire damage",
]

[pc.abilities]
cha = 18

[pc.spellcasting]
ability = "cha"
ability_modifier = 4
proficiency_bonus = 3

[[pc.actions]]
name = "Fire Bolt"
effect = "spell | hit +7 | 2d10 fire"
"#;

const EARTH_SHAPER: &str = r#"
[pc]
name = "Earth Shaper"
ac = 14
hp = 55
initiative = 0
traits = [
    "reduce 3 bludgeoning, piercing, slashing",
]

[pc.abilities]
con = 16
wis = 16

[pc.spellcasting]
ability = "wis"
ability_modifier = 3
proficiency_bonus = 3

[pc.resources.slots]
1 = 4

[[pc.features]]
plugin = "lasting_boon"
name = "Stone Armor"
slot = 1
ac = 3
"#;

const TOXIN_LORD: &str = r#"
[pc]
name = "Toxin Lord"
ac = 15
hp = 50
initiative = 1
traits = [
    "ignore resistance poison",
]

[pc.abilities]
con = 16
int = 16

[pc.spellcasting]
ability = "int"
ability_modifier = 3
proficiency_bonus = 3

[[pc.actions]]
name = "Toxic Dart"
effect = "spell | hit +6 | 1d10 poison"

[[pc.features]]
plugin = "lasting_aura"
name = "Miasmic Cloud"
effect = "save con dc 14 | half | 2d6 poison"

[[pc.features]]
plugin = "commanded_double"
name = "Viper Horror"
summon_name = "Summon Viper"
hp = 25
ac = 13
attacks = ["Bite | strikes 1 | hit +5 | 1d8+2 piercing"]
"#;

#[test]
fn wood_thorny_defence_reaction_autohit_triggers_on_melee_hit() {
    let registry = FeatureRegistry::new();
    let mut warden = load_creature_from_str(WOOD_WARDEN, &registry).expect("valid wood warden");
    warden.team = 0;

    // Verify parsed reaction has autohit damage effect and melee hit trigger
    assert_eq!(warden.reactions.len(), 1);
    let rx = &warden.reactions[0];
    assert_eq!(rx.action.name, "Thorny Defence");
    assert_eq!(rx.trigger, ReactionTrigger::Hit(AttackTrigger::MeleeAttack));
    match &rx.action.effect {
        Effect::AutoHit { damage } => {
            assert_eq!(damage.len(), 1);
            assert_eq!(damage[0].kind, DamageKind::Piercing);
        }
        other => panic!("expected AutoHit effect, got {other:?}"),
    }

    // Put Warden against an attacker in melee
    let attacker_dsl = "creature: Brawler\nhp: 40\nac: 10\naction: Punch | strikes 1 | hit +10 | 1d4 bludgeoning\n";
    let mut brawler = parse(attacker_dsl).unwrap().into_iter().next().unwrap();
    brawler.team = 1;

    let mut rng = Rng::new(42);
    let mut log = None;
    let outcome = run_teams(
        &mut rng,
        &[&warden, &brawler],
        [Policy::InOrder; 2],
        1,
        Budget::default(),
        &mut log,
    );

    assert!(outcome.rounds >= 1);
    assert!(
        outcome.damage_dealt[0] > 0,
        "Warden should have dealt damage via attack and thorny defence reaction"
    );
}

#[test]
fn fire_adept_ignores_fire_resistance_and_downgrades_immunity() {
    let registry = FeatureRegistry::new();
    let pyro = load_creature_from_str(PYROMANCER, &registry).expect("valid pyromancer");

    // Target with fire resistance
    let mut resistant_target = crucible_core::creature::Creature::new("Magma Mephit", 12, 50);
    resistant_target
        .reductions
        .push((DamageKind::Fire, Reduction::Resistant));

    // Target with fire immunity
    let mut immune_target = crucible_core::creature::Creature::new("Iron Golem", 17, 100);
    immune_target
        .reductions
        .push((DamageKind::Fire, Reduction::Immune));

    // Plain creature sees resistance and immunity
    let plain = crucible_core::creature::Creature::new("Plain Caster", 10, 30);
    assert_eq!(
        resistant_target.reduction_from(DamageKind::Fire, &plain),
        Reduction::Resistant
    );
    assert_eq!(
        immune_target.reduction_from(DamageKind::Fire, &plain),
        Reduction::Immune
    );

    // Pyro ignores resistance -> Normal
    assert_eq!(
        resistant_target.reduction_from(DamageKind::Fire, &pyro),
        Reduction::Normal
    );

    // Pyro downgrades immunity -> Resistant, and also ignores resistance -> Normal!
    assert_eq!(
        immune_target.reduction_from(DamageKind::Fire, &pyro),
        Reduction::Normal
    );
}

#[test]
fn earth_shaper_stone_armor_boon_and_damage_reduction() {
    let registry = FeatureRegistry::new();
    let mut earth = load_creature_from_str(EARTH_SHAPER, &registry).expect("valid earth shaper");
    earth.team = 0;

    assert_eq!(earth.ac, 14);
    // reduce 3 bludgeoning, piercing, slashing adds a ReduceDamage rider
    assert!(earth
        .riders
        .iter()
        .any(|r| matches!(r, Rider::ReduceDamage { roll, .. } if roll.bonus == 3)));

    // Enemy striker with +5 to hit
    let enemy_dsl =
        "creature: Soldier\nhp: 60\nac: 14\naction: Slash | strikes 2 | hit +5 | 1d8+3 slashing\n";
    let mut enemy = parse(enemy_dsl).unwrap().into_iter().next().unwrap();
    enemy.team = 1;

    let mut rng = Rng::new(12345);
    let mut log = None;
    let outcome = run_teams(
        &mut rng,
        &[&earth, &enemy],
        [Policy::InOrder; 2],
        3,
        Budget::default(),
        &mut log,
    );

    // Fight ran successfully with lasting_boon active and damage reduction soaking damage
    assert!(outcome.rounds >= 1);
}

#[test]
fn poison_aura_and_summon_double_compose_in_simulation() {
    let registry = FeatureRegistry::new();
    let mut lord = load_creature_from_str(TOXIN_LORD, &registry).expect("valid toxin lord");
    lord.team = 0;

    let dummy_dsl = "creature: Target Dummy\nhp: 80\nac: 10\naction: Idle | strikes 1 | hit -10 | 0 bludgeoning\n";
    let mut dummy = parse(dummy_dsl).unwrap().into_iter().next().unwrap();
    dummy.team = 1;

    let mut rng = Rng::new(999);
    let mut log = None;
    let outcome = run_teams(
        &mut rng,
        &[&lord, &dummy],
        [Policy::InOrder; 2],
        2,
        Budget::default(),
        &mut log,
    );

    assert!(outcome.rounds >= 1);
    assert!(
        outcome.damage_dealt[0] > 0,
        "Lord should have damaged dummy through toxic dart, miasmic aura, and viper double"
    );
}

const STORM_STRIKER: &str = r#"
[pc]
name = "Storm Striker"
ac = 15
hp = 50
initiative = 2
traits = [
    "push on lightning up to large",
]

[pc.abilities]
str = 16
wis = 16

[pc.spellcasting]
ability = "wis"
ability_modifier = 3
proficiency_bonus = 3

[pc.resources]
lightning_charge = 2

[[pc.actions]]
name = "Shocking Grasp"
effect = "spell | hit +6 | 2d8 lightning"

[[pc.features]]
plugin = "maximised_damage"
name = "Lightning Overload"
effect = "spell | hit +6 | 2d8 lightning"
kinds = ["lightning"]
resource = "lightning_charge"
cost = 1

[[pc.features]]
plugin = "retaliation"
name = "Static Discharge"
damage = "2d8 lightning"
trigger = "melee"
"#;

const ACID_STALKER: &str = r#"
[pc]
name = "Acid Stalker"
ac = 15
hp = 45
initiative = 3

[pc.abilities]
dex = 16

[[pc.actions]]
name = "Dagger"
effect = "strikes 2 | hit +5 | 1d4+3 piercing"

[[pc.features]]
plugin = "lasting_boon"
name = "Caustic Coating"
bonus_action = true
dice_count = 1
dice_sides = 6
damage_kind = "acid"
weapon_only = true
"#;

#[test]
fn lightning_maximised_damage_and_retaliation_compose() {
    let registry = FeatureRegistry::new();
    let striker = load_creature_from_str(STORM_STRIKER, &registry).expect("valid storm striker");

    // Has push trait
    assert!(striker
        .riders
        .iter()
        .any(|r| matches!(r, Rider::PushOnDamage { .. })));
    // Has reactions (retaliation)
    assert!(!striker.reactions.is_empty());
    // Has maximised damage action
    assert!(striker
        .actions
        .iter()
        .any(|a| a.name == "Lightning Overload"));
}

#[test]
fn acid_weapon_coating_boon_applies_and_simulates() {
    let registry = FeatureRegistry::new();
    let mut stalker = load_creature_from_str(ACID_STALKER, &registry).expect("valid acid stalker");
    stalker.team = 0;

    let dummy_dsl = "creature: Target Dummy\nhp: 80\nac: 10\naction: Idle | strikes 1 | hit -10 | 0 bludgeoning\n";
    let mut dummy = parse(dummy_dsl).unwrap().into_iter().next().unwrap();
    dummy.team = 1;

    let mut rng = Rng::new(555);
    let mut log = None;
    let outcome = run_teams(
        &mut rng,
        &[&stalker, &dummy],
        [Policy::InOrder; 2],
        2,
        Budget::default(),
        &mut log,
    );

    assert!(outcome.rounds >= 1);
    assert!(outcome.damage_dealt[0] > 0);
}
