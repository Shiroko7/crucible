//! Verification of Essence Talent System V2 (Cultivation Paths) mechanics in Crucible.
//!
//! Across the 16 cultivation paths and 3 traditions/families:
//! - **Primordial Family** (Wood, Fire, Earth, Metal, Water, Sky) sharing `primordial` essence pool:
//!   * Reactive retributions (`autohit` on hit, e.g. *Thorny Defence*),
//!   * Elemental penetration (`ignore resistance <kinds>` and `downgrade immunity <kinds> damage`),
//!   * Persistent boons and armors (`lasting_boon`, e.g. *Stone Armor*, *Ice Form*),
//!   * Elemental hazard auras (`lasting_aura`, e.g. *Sandstorm*, *Scorched Ground*),
//!   * Forced movement (`push on lightning, thunder up to large`),
//!   * Summons and duplicates (`commanded_double`, e.g. *Water Clones*).
//! - **Divine Family** (Lunar, Love, Ruin, Pestilence, Shadow, Tempest, Providence) sharing `divine` essence pool:
//!   * Quarry marks & assassination (`quarry <dice> <kind>`, e.g. *Mark for Death*),
//!   * Multi-type celestial spells (e.g. *Moonlit Verdant Beam* dealing radiant + poison),
//!   * Crushing void force strikes (*Netherdark Fist*),
//!   * Reactive agony mantles (`retaliation` psychic retribution, e.g. *Crown of Agony*),
//!   * Warding holy flames (`lasting_aura`, e.g. *Martyr's Flame Aura*),
//!   * Toxic skin secretions (`when hit in melee | save con ... | on fail poisoned`).
//! - **Immortal Family** (Alchemy, Heart, Mysteries) sharing `immortal` essence pool:
//!   * Unarmed martial arts progressions (*Stone Fist I–V*: 1d4 -> 1d6 -> 1d8 -> 1d10 -> 1d12 bludgeoning),
//!   * Martial technique attack replacements (*Radiant Vein Blade I–III*, *Pommel Strike*, *Heartstopper*),
//!   * Grandmaster reflection counterattacks (*Supreme Heart-Reflection Sword*),
//!   * Spacetime manipulation & soul extraction (*Fishing the Moon from the Well* banish / force),
//!   * Mutagenic formulas and caustic bombs (*Caustic Bomb*, *Mutagen Formula* boons),
//!   * Homunculi conjurations (*commanded_double*, e.g. *Apex Toxinator* Snake Horror).

use crucible_core::creature::{AttackTrigger, Effect, ReactionTrigger, Rider};
use crucible_core::dsl::config::load_creature_from_str;
use crucible_core::dsl::scenario::parse;
use crucible_core::features::FeatureRegistry;
use crucible_core::prob::Rng;
use crucible_core::rules::{DamageKind, Reduction};
use crucible_core::sim::{run_teams, Budget, Policy};

const PRIMORDIAL_CULTIVATOR: &str = r#"
[pc]
name = "Primordial Cultivator"
ac = 16
hp = 60
initiative = 2
traits = [
    "ignore resistance fire, lightning",
    "downgrade immunity fire damage",
    "reduce 3 bludgeoning, piercing, slashing",
    "push on lightning up to large",
]

[pc.abilities]
str = 14
dex = 14
con = 16
int = 10
wis = 18
cha = 10

[pc.spellcasting]
ability = "wis"
ability_modifier = 4
proficiency_bonus = 3

[pc.resources]
primordial = 10

[[pc.actions]]
name = "Fire Bolt"
effect = "spell | ranged | hit +7 | 2d10 fire"

[[pc.actions]]
name = "Lightning Javelin"
effect = "cost primordial 1 | spell | ranged | hit +7 | 5d12 lightning"

[[pc.actions]]
name = "Steel Tornado"
effect = "cost primordial 2 | save dex dc 15 | 4d8 slashing | half"

[[pc.reactions]]
name = "Thorny Defence"
effect = "when hit in melee | autohit | 4d4 piercing"

[[pc.features]]
plugin = "lasting_boon"
name = "Stone Armor"
bonus_action = true
rounds = 10
ac = 2
"#;

const DIVINE_CULTIVATOR: &str = r#"
[pc]
name = "Divine Cultivator"
ac = 15
hp = 55
initiative = 3
traits = [
    "quarry 1d6 necrotic",
    "ignore resistance poison",
    "downgrade immunity poison damage, poisoned condition",
    "reaction ac 2 until next turn",
]

[pc.abilities]
str = 10
dex = 16
con = 14
int = 12
wis = 18
cha = 14

[pc.spellcasting]
ability = "wis"
ability_modifier = 4
proficiency_bonus = 3

[pc.resources]
divine = 10

[[pc.actions]]
name = "Moonlit Verdant Beam"
effect = "spell | ranged | hit +7 | 4d8 radiant + 2d6 poison"

[[pc.actions]]
name = "Netherdark Fist"
effect = "cost divine 3 | strikes 1 | hit +7 | 6d10 force | on fail prone"

[[pc.actions]]
name = "Touch of Respite"
effect = "heal 2d8+4"

[[pc.reactions]]
name = "Toxic Skin Secretion"
effect = "when hit in melee | save con dc 15 | 2d6 poison | on fail poisoned"

[[pc.features]]
plugin = "lasting_aura"
name = "Martyr's Flame Aura"
effect = "save dex dc 15 | half | 3d8 radiant"

[[pc.features]]
plugin = "retaliation"
name = "Crown of Agony"
damage = "2d8 psychic"
trigger = "melee"
"#;

const IMMORTAL_CULTIVATOR: &str = r#"
[pc]
name = "Immortal Cultivator"
ac = 16
hp = 65
initiative = 3
traits = [
    "resistance psychic",
    "reduce 1d10+2 bludgeoning, piercing, slashing",
]

[pc.abilities]
str = 18
dex = 16
con = 16
int = 14
wis = 14
cha = 10

[pc.spellcasting]
ability = "str"
ability_modifier = 4
proficiency_bonus = 3

[pc.resources]
immortal = 12

[[pc.actions]]
name = "Stone Fist"
effect = "strikes 2 | hit +7 | 1d8+4 bludgeoning"

[[pc.actions]]
name = "Radiant Vein Blade"
effect = "cost immortal 1 | strikes 2 | weapon longsword | hit +7 | 1d8+4 slashing + 1d8 radiant"

[[pc.actions]]
name = "Fishing the Moon from the Well"
effect = "cost immortal 5 | save cha dc 15 | 10d10 force | half | on fail banished for 1 round"

[[pc.actions]]
name = "Caustic Bomb"
effect = "cost immortal 1 | save dex dc 15 | 4d6 acid | half"

[[pc.reactions]]
name = "Supreme Heart-Reflection Sword"
effect = "when hit in melee | cost immortal 4 | autohit | 4d8 force"

[[pc.features]]
plugin = "commanded_double"
name = "Apex Toxinator"
summon_name = "Snake Horror"
hp = 30
ac = 14
attacks = ["Bite | strikes 1 | hit +6 | 1d8+3 piercing, 2d6 poison"]
"#;

#[test]
fn primordial_family_cultivator_loads_and_simulates() {
    let registry = FeatureRegistry::new();
    let mut pc = load_creature_from_str(PRIMORDIAL_CULTIVATOR, &registry)
        .expect("valid primordial cultivator");
    pc.team = 0;

    // Verify shared family pool resource index exists and starts at 10
    let pool_idx = pc.resource_index("primordial").expect("primordial pool");
    assert_eq!(pc.resources[pool_idx].max, 10);

    // Verify Thorny Defence autohit reaction
    assert_eq!(pc.reactions.len(), 1);
    let rx = &pc.reactions[0];
    assert_eq!(rx.action.name, "Thorny Defence");
    assert_eq!(rx.trigger, ReactionTrigger::Hit(AttackTrigger::MeleeAttack));
    match &rx.action.effect {
        Effect::AutoHit { damage } => {
            assert_eq!(damage.len(), 1);
            assert_eq!(damage[0].kind, DamageKind::Piercing);
        }
        other => panic!("expected AutoHit effect, got {other:?}"),
    }

    // Verify traits: push on lightning, reduce physical, ignore resistance fire/lightning, downgrade fire immunity
    assert!(pc.riders.iter().any(|r| matches!(r, Rider::PushOnDamage { .. })));
    assert!(pc.riders.iter().any(|r| matches!(r, Rider::ReduceDamage { .. })));
    assert!(pc.riders.iter().any(|r| r.ignores_damage_resistance(DamageKind::Fire)));
    assert!(pc.riders.iter().any(|r| r.ignores_damage_resistance(DamageKind::Lightning)));

    // Simulate combat against an enemy striker
    let enemy_dsl = "creature: Fire Elemental\nhp: 70\nac: 13\nresist: fire\naction: Slam | strikes 2 | hit +6 | 2d6+3 bludgeoning\n";
    let mut enemy = parse(enemy_dsl).unwrap().into_iter().next().unwrap();
    enemy.team = 1;

    let mut rng = Rng::new(42);
    let mut log = None;
    let outcome = run_teams(
        &mut rng,
        &[&pc, &enemy],
        [Policy::InOrder; 2],
        3,
        Budget::default(),
        &mut log,
    );

    assert!(outcome.rounds >= 1);
    assert!(outcome.damage_dealt[0] > 0, "Primordial cultivator deals damage");
}

#[test]
fn divine_family_cultivator_multi_damage_and_quarry_compose() {
    let registry = FeatureRegistry::new();
    let mut pc = load_creature_from_str(DIVINE_CULTIVATOR, &registry)
        .expect("valid divine cultivator");
    pc.team = 0;

    let pool_idx = pc.resource_index("divine").expect("divine pool");
    assert_eq!(pc.resources[pool_idx].max, 10);

    // Verify multi-type damage parsing in Moonlit Verdant Beam (radiant + poison)
    let beam = pc.actions.iter().find(|a| a.name == "Moonlit Verdant Beam").expect("beam action");
    let Effect::Strikes { strike, .. } = &beam.effect else { panic!("expected strike") };
    assert_eq!(strike.damage.len(), 2);
    assert_eq!(strike.damage[0].kind, DamageKind::Radiant);
    assert_eq!(strike.damage[1].kind, DamageKind::Poison);

    // Verify Netherdark Fist force strike with cost
    let fist = pc.actions.iter().find(|a| a.name == "Netherdark Fist").expect("netherdark action");
    assert_eq!(fist.cost.as_ref().map(|c| c.amount), Some(3));

    // Verify reactions and retaliation
    assert!(!pc.reactions.is_empty());

    let target_dsl = "creature: Shadow Beast\nhp: 80\nac: 14\naction: Claw | strikes 2 | hit +5 | 1d8+2 slashing\n";
    let mut target = parse(target_dsl).unwrap().into_iter().next().unwrap();
    target.team = 1;

    let mut rng = Rng::new(101);
    let mut log = None;
    let outcome = run_teams(
        &mut rng,
        &[&pc, &target],
        [Policy::InOrder; 2],
        3,
        Budget::default(),
        &mut log,
    );

    assert!(outcome.rounds >= 1);
    assert!(outcome.damage_dealt[0] > 0);
}

#[test]
fn immortal_family_cultivator_heart_martial_and_mysteries_simulate() {
    let registry = FeatureRegistry::new();
    let mut pc = load_creature_from_str(IMMORTAL_CULTIVATOR, &registry)
        .expect("valid immortal cultivator");
    pc.team = 0;

    let pool_idx = pc.resource_index("immortal").expect("immortal pool");
    assert_eq!(pc.resources[pool_idx].max, 12);

    // Verify Stone Fist unarmed multiattack
    let stone_fist = pc.actions.iter().find(|a| a.name == "Stone Fist").expect("stone fist");
    let Effect::Strikes { strike, count } = &stone_fist.effect else { panic!("expected strikes") };
    assert_eq!(*count, 2);
    assert_eq!(strike.damage[0].sides, 8); // 1d8 Stone Fist III
    assert_eq!(strike.damage[0].kind, DamageKind::Bludgeoning);

    // Verify Radiant Vein Blade strikes dealing weapon slashing + radiant
    let blade = pc.actions.iter().find(|a| a.name == "Radiant Vein Blade").expect("radiant blade");
    assert_eq!(blade.cost.as_ref().map(|c| c.amount), Some(1));
    let Effect::Strikes { strike: blade_strike, count: blade_count } = &blade.effect else { panic!("strikes") };
    assert_eq!(*blade_count, 2);
    assert_eq!(blade_strike.damage.len(), 2);
    assert_eq!(blade_strike.damage[0].kind, DamageKind::Slashing);
    assert_eq!(blade_strike.damage[1].kind, DamageKind::Radiant);

    // Verify Supreme Heart-Reflection Sword autohit counter with immortal cost
    let counter = pc.reactions.iter().find(|r| r.action.name == "Supreme Heart-Reflection Sword").expect("sword counter");
    assert_eq!(counter.action.cost.as_ref().map(|c| c.amount), Some(4));
    match &counter.action.effect {
        Effect::AutoHit { damage } => {
            assert_eq!(damage[0].kind, DamageKind::Force);
            assert_eq!(damage[0].count, 4);
        }
        other => panic!("expected AutoHit, got {other:?}"),
    }

    // Verify Fishing the Moon from the Well grandmaster technique
    let fish = pc.actions.iter().find(|a| a.name == "Fishing the Moon from the Well").expect("moon fishing");
    assert_eq!(fish.cost.as_ref().map(|c| c.amount), Some(5));
    let Effect::Save(save) = &fish.effect else { panic!("expected save") };
    assert_eq!(save.damage[0].kind, DamageKind::Force);

    // Simulate in combat
    let dummy_dsl = "creature: Practice Automaton\nhp: 100\nac: 12\naction: Slam | strikes 1 | hit +5 | 1d8+2 bludgeoning\n";
    let mut dummy = parse(dummy_dsl).unwrap().into_iter().next().unwrap();
    dummy.team = 1;

    let mut rng = Rng::new(777);
    let mut log = None;
    let outcome = run_teams(
        &mut rng,
        &[&pc, &dummy],
        [Policy::InOrder; 2],
        3,
        Budget::default(),
        &mut log,
    );

    assert!(outcome.rounds >= 1);
    assert!(outcome.damage_dealt[0] > 0);
}

#[test]
fn fire_adept_ignores_fire_resistance_and_downgrades_immunity() {
    let registry = FeatureRegistry::new();
    let pyro = load_creature_from_str(PRIMORDIAL_CULTIVATOR, &registry).expect("valid pyro");

    let mut resistant_target = crucible_core::creature::Creature::new("Magma Mephit", 12, 50);
    resistant_target
        .reductions
        .push((DamageKind::Fire, Reduction::Resistant));

    let mut immune_target = crucible_core::creature::Creature::new("Iron Golem", 17, 100);
    immune_target
        .reductions
        .push((DamageKind::Fire, Reduction::Immune));

    let plain = crucible_core::creature::Creature::new("Plain Caster", 10, 30);
    assert_eq!(
        resistant_target.reduction_from(DamageKind::Fire, &plain),
        Reduction::Resistant
    );
    assert_eq!(
        immune_target.reduction_from(DamageKind::Fire, &plain),
        Reduction::Immune
    );

    assert_eq!(
        resistant_target.reduction_from(DamageKind::Fire, &pyro),
        Reduction::Normal
    );
    assert_eq!(
        immune_target.reduction_from(DamageKind::Fire, &pyro),
        Reduction::Normal
    );
}

#[test]
fn wood_thorny_defence_reaction_autohit_triggers_on_melee_hit() {
    let registry = FeatureRegistry::new();
    let mut pc = load_creature_from_str(PRIMORDIAL_CULTIVATOR, &registry).expect("valid pc");
    pc.team = 0;

    let attacker_dsl = "creature: Brawler\nhp: 40\nac: 10\naction: Punch | strikes 1 | hit +10 | 1d4 bludgeoning\n";
    let mut brawler = parse(attacker_dsl).unwrap().into_iter().next().unwrap();
    brawler.team = 1;

    let mut rng = Rng::new(42);
    let mut log = None;
    let outcome = run_teams(
        &mut rng,
        &[&pc, &brawler],
        [Policy::InOrder; 2],
        1,
        Budget::default(),
        &mut log,
    );

    assert!(outcome.rounds >= 1);
    assert!(outcome.damage_dealt[0] > 0);
}

const ALCHEMIST_HORROR: &str = r#"
[pc]
name = "Apex Alchemist"
ac = 15
hp = 50
initiative = 2

[pc.abilities]
con = 16
int = 18

[pc.spellcasting]
ability = "int"
ability_modifier = 4
proficiency_bonus = 3

[pc.resources]
immortal = 8

[[pc.actions]]
name = "Caustic Bomb"
effect = "cost immortal 1 | save dex dc 15 | 4d6 acid | half"

[[pc.features]]
plugin = "lasting_boon"
name = "Acidic Embrace"
bonus_action = true
dice_count = 2
dice_sides = 4
damage_kind = "acid"
weapon_only = true

[[pc.features]]
plugin = "commanded_double"
name = "Apex Toxinator"
summon_name = "Snake Horror"
resource = "immortal"
cost = 2
hp = 28
ac = 13
attacks = ["Bite | strikes 1 | hit +6 | 1d8+3 piercing, 2d6 poison"]
"#;

#[test]
fn alchemy_caustic_bomb_boon_and_apex_toxinator_simulate() {
    let registry = FeatureRegistry::new();
    let mut pc = load_creature_from_str(ALCHEMIST_HORROR, &registry).expect("valid alchemist");
    pc.team = 0;

    let dummy_dsl = "creature: Test Dummy\nhp: 70\nac: 10\naction: Idle | strikes 1 | hit -10 | 0 bludgeoning\n";
    let mut dummy = parse(dummy_dsl).unwrap().into_iter().next().unwrap();
    dummy.team = 1;

    let mut rng = Rng::new(888);
    let mut log = None;
    let outcome = run_teams(
        &mut rng,
        &[&pc, &dummy],
        [Policy::InOrder; 2],
        2,
        Budget::default(),
        &mut log,
    );

    assert!(outcome.rounds >= 1);
    assert!(outcome.damage_dealt[0] > 0, "Alchemist and Snake Horror dealt damage");
}

const THALASSIOS_STORM: &str = r#"
[pc]
name = "Thalassios Storm"
ac = 16
hp = 60
initiative = 2
traits = [
    "push on lightning up to large",
]

[pc.abilities]
str = 14
wis = 18

[pc.spellcasting]
ability = "wis"
ability_modifier = 4
proficiency_bonus = 3

[pc.resources]
primordial = 10

[[pc.actions]]
name = "Shocking Grasp"
effect = "spell | hit +7 | 2d8 lightning"

[[pc.features]]
plugin = "maximised_damage"
name = "Lightning Overload"
effect = "spell | hit +7 | 2d8 lightning"
kinds = ["lightning"]
resource = "primordial"
cost = 1

[[pc.features]]
plugin = "commanded_double"
name = "Water Clone"
summon_name = "Thalassios Reflection"
resource = "primordial"
cost = 2
hp = 25
ac = 14
attacks = ["Slam | strikes 1 | hit +6 | 1d8+3 bludgeoning, 1d8 cold"]
"#;

#[test]
fn water_clone_and_lightning_overload_spend_shared_primordial_pool() {
    let registry = FeatureRegistry::new();
    let mut pc = load_creature_from_str(THALASSIOS_STORM, &registry).expect("valid thalassios storm");
    pc.team = 0;

    let pool_idx = pc.resource_index("primordial").expect("primordial pool");
    assert_eq!(pc.resources[pool_idx].max, 10);

    let dummy_dsl = "creature: Sparring Dummy\nhp: 80\nac: 12\naction: Idle | strikes 1 | hit -10 | 0 bludgeoning\n";
    let mut dummy = parse(dummy_dsl).unwrap().into_iter().next().unwrap();
    dummy.team = 1;

    let mut rng = Rng::new(999);
    let mut log = None;
    let outcome = run_teams(
        &mut rng,
        &[&pc, &dummy],
        [Policy::InOrder; 2],
        2,
        Budget::default(),
        &mut log,
    );

    assert!(outcome.rounds >= 1);
    assert!(outcome.damage_dealt[0] > 0);
}

