//! A caster that fights with other creatures' own strength - it stops their
//! spells as they are cast, turns their blows on their friends, marks one of
//! them so that nothing can heal it, and shrugs off the magic thrown back -
//! declared as TOML through every trait keyword and clause it needs,
//! compiled, and fought.
//!
//! The mastermind is a stand-in with neutral names, not any specific
//! creature: a table's own monsters live in its own local files (see the
//! README). What it proves is that every mechanism such a creature leans on
//! is not only loadable but *live* - that a real fight actually applies it.

use crucible_core::creature::{Creature, Effect, ReactionTrigger, Rider};
use crucible_core::dsl::config::load_creature_from_str;
use crucible_core::features::FeatureRegistry;
use crucible_core::prob::Rng;
use crucible_core::rules::{Ability, Condition, Duration};
use crucible_core::sim::{run_teams, Budget, Outcome, Policy};

const MASTERMIND: &str = r#"
[monster]
name = "Test Mastermind"
ac = 17
hp = 400
initiative = 30
legendary_uses = 3
traits = [
    "magic resistance",
    "advantage against frightened",
    "advantage against quarry",
    "bonus 1d8 necrotic vs quarry",
    "redirect attacks wis dc 30",
]

[monster.saves]
dex = 2
con = 4

[monster.resources.slots]
5 = 6

[[monster.actions]]
name = "Test Beam"
effect = "spell | ranged | range 120 | strikes 2 | hit +12 | 1d10+5 psychic"

[[monster.actions]]
name = "Test Bloom"
effect = "recharge 5 | range 60 | area 30 | targets 6 | save wis dc 30 | 8d10 psychic | half on success | on fail reeling until victim end"

[[monster.actions]]
name = "Test Unmaking"
effect = "spell | uses 1 | range 150 | targets 1 | save int dc 30 | 12d6 psychic | on fail addled for 1 hour"

[[monster.bonus]]
name = "Test Mark"
effect = "range 60 | targets 1 | save wis dc 30 | on fail quarry for 1 minute | on fail unhealable for 1 minute"

[[monster.reactions]]
name = "Test Drink"
effect = "when targeted by a spell | item | counter"

[[monster.reactions]]
name = "Test Unravel"
effect = "when enemy casts | spell | slot 5 | range 60 | save con dc 30"

[[monster.legendary]]
name = "Test Whisper"
effect = "points 2 | range 60 | targets 1 | save wis dc 30 | on fail frightened until victim end | bonus 3d8 psychic vs quarry"
"#;

/// Swings at whatever is in front of it, and answers a blow with one.
const BRAWLER: &str = r#"
[pc]
name = "Test Brawler"
ac = 16
hp = 300
[[pc.actions]]
name = "Maul"
effect = "strikes 2 | hit +10 | 2d6+5 bludgeoning"
[[pc.reactions]]
name = "Riposte"
effect = "when hit | hit +10 | 1d8+5 slashing"
"#;

/// Casts: a bolt at one creature, a burst at everything, a cantrip, a heal.
const CASTER: &str = r#"
[pc]
name = "Test Caster"
ac = 14
hp = 300
[pc.resources.slots]
1 = 4
3 = 4
[[pc.actions]]
name = "Bolt"
effect = "spell | slot 3 | ranged | range 120 | strikes 1 | hit +9 | 8d6 lightning"
[[pc.actions]]
name = "Burst"
effect = "spell | slot 3 | range 120 | save dex dc 16 | 8d6 fire | half on success"
[[pc.actions]]
name = "Spark"
effect = "spell | ranged | range 120 | strikes 1 | hit +9 | 2d10 fire"
[[pc.actions]]
name = "Club"
effect = "strikes 1 | hit +4 | 1d6 bludgeoning"
[[pc.bonus]]
name = "Mend"
effect = "spell | slot 1 | heal 2d4+4"
"#;

fn load(text: &str) -> Creature {
    load_creature_from_str(text, &FeatureRegistry::new()).expect("it compiles")
}

fn mastermind() -> Creature {
    let mut m = load(MASTERMIND);
    m.team = 1;
    m
}

/// `fights` of them, seeded, with what was said in each.
fn fought(roster: &[&Creature], fights: u32, seed: u64) -> (Vec<Outcome>, String) {
    let mut rng = Rng::new(seed);
    let (mut outcomes, mut narration) = (Vec::new(), String::new());
    for _ in 0..fights {
        let mut log = Some(Vec::new());
        outcomes.push(run_teams(
            &mut rng,
            roster,
            [Policy::Greedy; 2],
            6,
            Budget::default(),
            &mut log,
        ));
        narration.push_str(&log.unwrap().join("\n"));
        narration.push('\n');
    }
    (outcomes, narration)
}

fn mean_damage_by_party(outcomes: &[Outcome]) -> f64 {
    outcomes
        .iter()
        .map(|o| o.damage_dealt[0] as f64)
        .sum::<f64>()
        / outcomes.len() as f64
}

#[test]
fn the_mastermind_compiles_to_what_it_declares() {
    let m = load(MASTERMIND);
    assert_eq!(
        m.riders,
        [
            Rider::MagicResistance,
            Rider::AdvantageAgainst {
                condition: Condition::Frightened
            },
            Rider::AdvantageAgainst {
                condition: Condition::Quarry
            },
            Rider::BonusDamageVsCondition {
                dice_count: 1,
                dice_sides: 8,
                bonus: 0,
                damage_kind: crucible_core::rules::DamageKind::Necrotic,
                condition: Condition::Quarry,
            },
            Rider::RedirectAttacks {
                ability: Ability::Wis,
                dc: 30
            },
        ]
    );
    assert_eq!(
        m.reactions.iter().map(|r| r.trigger).collect::<Vec<_>>(),
        [
            ReactionTrigger::Cast { aimed: true },
            ReactionTrigger::Cast { aimed: false },
        ]
    );
    assert_eq!(m.reactions[0].action.effect, Effect::Counter);
    assert!(matches!(m.reactions[1].action.effect, Effect::Save(_)));

    // The bloom's daze lasts through the end of its victim's next turn, and
    // the mark lands two conditions for a minute.
    let Effect::Save(bloom) = &m.actions[1].effect else {
        panic!("the bloom is a save");
    };
    assert_eq!(
        bloom.on_failure,
        [(Condition::Reeling, Duration::VictimNextTurnEnd)]
    );
    let Effect::Save(mark) = &m.bonus_actions[0].effect else {
        panic!("the mark is a save");
    };
    assert_eq!(
        mark.on_failure,
        [
            (Condition::Quarry, Duration::Rounds(10)),
            (Condition::Unhealable, Duration::Rounds(10)),
        ]
    );
    // What the whisper adds against whoever is marked rides the move.
    assert!(matches!(
        m.legendary[0].riders[..],
        [Rider::BonusDamageVsCondition {
            condition: Condition::Quarry,
            ..
        }]
    ));
}

/// Every mechanism shows up in the narration of real fights.
#[test]
fn every_part_of_the_mastermind_is_live_in_a_fight() {
    let (m, brawler, caster) = (mastermind(), load(BRAWLER), load(CASTER));
    let mut second = brawler.clone();
    second.name = "Test Second".to_string();
    let (_, mut narration) = fought(&[&brawler, &second, &caster, &m], 200, 11);
    // And once with a caster that only ever aims at one creature.
    let mut aiming = caster.clone();
    aiming.actions.retain(|a| a.name == "Bolt");
    narration.push_str(&fought(&[&aiming, &m], 20, 11).1);
    for sign in [
        // A spell aimed at it alone is drunk; one that is not is unravelled,
        // by a save its caster cannot make.
        "Test Mastermind's Test Drink stops Test Caster's Bolt",
        "Test Mastermind's Test Unravel stops Test Caster's Burst",
        // A blow at it lands on a friend.
        "turned on Test Second",
        "turned on Test Brawler",
        // The mark; the daze; the unmaking; the whisper.
        "quarry and unhealable",
        "and reeling",
        "and addled",
        "and frightened",
    ] {
        assert!(narration.contains(sign), "never saw `{sign}` in 200 fights");
    }
}

/// A costly answer is kept for a spell that cost its caster something: a
/// cantrip is let through, and so is anything that is not a spell.
#[test]
fn a_cantrip_is_not_worth_a_slot_to_stop() {
    let mut m = mastermind();
    m.reactions.remove(0); // only the one that costs a slot
    let mut caster = load(CASTER);
    caster
        .actions
        .retain(|a| a.name == "Spark" || a.name == "Club");
    caster.bonus_actions.clear();
    let (_, narration) = fought(&[&caster, &m], 100, 3);
    assert!(narration.contains("Spark"));
    assert!(!narration.contains("Test Unravel"));
}

/// The answer aimed spells get is no answer to one that catches an area.
#[test]
fn a_spell_that_is_not_aimed_at_it_alone_is_not_drunk() {
    let mut m = mastermind();
    m.reactions.remove(1); // only the one for a spell aimed at it
    let mut caster = load(CASTER);
    caster.actions.retain(|a| a.name == "Burst");
    let (_, narration) = fought(&[&caster, &m], 100, 3);
    assert!(narration.contains("Burst (Test Mastermind"));
    assert!(!narration.contains("Test Drink"));
}

/// A stopped spell costs its caster the action and nothing else: the slot
/// is still there to try again with.
#[test]
fn a_stopped_spell_keeps_its_slot() {
    let mut m = mastermind();
    m.actions.clear();
    m.bonus_actions.clear();
    m.legendary.clear();
    let mut caster = load(CASTER);
    caster.actions.retain(|a| a.name == "Bolt");
    caster.bonus_actions.clear();
    // Four slots, six rounds, and every bolt drunk: were a slot spent each
    // time, the last two rounds would have nothing to cast.
    let (outcomes, narration) = fought(&[&caster, &m], 20, 5);
    assert_eq!(mean_damage_by_party(&outcomes), 0.0);
    assert_eq!(narration.matches("Test Drink stops").count(), 20 * 6);
}

/// With nobody else in reach there is nothing to turn a blow on, and it
/// lands where it was aimed.
#[test]
fn a_lone_attacker_cannot_be_turned_on_anyone() {
    let (m, brawler) = (mastermind(), load(BRAWLER));
    let (outcomes, narration) = fought(&[&brawler, &m], 100, 7);
    assert!(!narration.contains("turned on"));
    assert!(mean_damage_by_party(&outcomes) > 0.0);
}

/// Turned blows are blows the mastermind does not take.
#[test]
fn turning_blows_spares_it_and_costs_its_enemies() {
    let (brawler, mut second) = (load(BRAWLER), load(BRAWLER));
    second.name = "Test Second".to_string();
    let quiet = |m: &mut Creature| {
        m.actions.clear();
        m.bonus_actions.clear();
        m.legendary.clear();
        m.reactions.clear();
    };
    let mut turning = mastermind();
    quiet(&mut turning);
    let mut plain = turning.clone();
    plain
        .riders
        .retain(|r| !matches!(r, Rider::RedirectAttacks { .. }));

    let (with, _) = fought(&[&brawler, &second, &turning], 200, 9);
    let (without, _) = fought(&[&brawler, &second, &plain], 200, 9);
    let hp_left = |o: &[Outcome], side: usize| {
        o.iter().map(|o| f64::from(o.hp_left[side])).sum::<f64>() / o.len() as f64
    };
    // It does nothing itself, so every hit point its enemies lose is one
    // they took off each other - and it keeps more of its own.
    assert_eq!(hp_left(&without, 0), 600.0);
    assert!(hp_left(&with, 0) < 500.0, "{}", hp_left(&with, 0));
    assert!(hp_left(&with, 1) > hp_left(&without, 1) + 50.0);
}

/// Magic it has to save against lands less often than the same thing
/// thrown at it by hand.
#[test]
fn magic_resistance_is_advantage_against_spells_and_nothing_else() {
    let mut m = mastermind();
    m.actions.clear();
    m.bonus_actions.clear();
    m.legendary.clear();
    m.reactions.clear();
    let mut plain = m.clone();
    plain.riders.retain(|r| *r != Rider::MagicResistance);

    let thrower = |spell: bool| {
        let mut c = load(CASTER);
        c.actions.retain(|a| a.name == "Burst");
        c.bonus_actions.clear();
        if !spell {
            c.actions[0].kind = crucible_core::creature::MoveKind::Standard;
            c.actions[0].spell_slot_level = None;
        }
        c
    };
    let dealt = |caster: &Creature, target: &Creature| {
        mean_damage_by_party(&fought(&[caster, target], 400, 13).0)
    };
    let (spell, hand) = (thrower(true), thrower(false));
    assert!(dealt(&spell, &m) < dealt(&spell, &plain) * 0.93);
    let (resisting, not) = (dealt(&hand, &m), dealt(&hand, &plain));
    assert!((resisting - not).abs() < not * 0.02, "{resisting} vs {not}");
}

/// A marked creature cannot be healed, and a reeling one answers nothing.
#[test]
fn the_mark_stops_healing_and_the_daze_stops_reactions() {
    let mut m = mastermind();
    m.reactions.clear(); // or the heal would be stopped before it is refused
    let mut healer = load(CASTER);
    healer.actions.retain(|a| a.name == "Club");
    let (_, narration) = fought(&[&healer, &m], 100, 17);
    // Alone, the only creature it can heal is itself - marked, from the
    // first round on, by a save it cannot make.
    assert!(narration.contains("cannot heal Test Caster"));
    assert!(!narration.contains("heals Test Caster"));

    // The brawler answers every blow that lands on it - until the bloom,
    // which it cannot save against, takes its reactions away.
    let brawler = load(BRAWLER);
    let mut blooming = mastermind();
    blooming.actions.retain(|a| a.name == "Test Bloom");
    blooming.actions[0].uses = crucible_core::creature::Uses::Unlimited;
    blooming.bonus_actions.clear();
    blooming.legendary[0] = load(MASTERMIND).actions[0].clone();
    blooming.legendary[0].legendary_cost = 1;
    let mut unblooming = blooming.clone();
    unblooming.actions = vec![load(MASTERMIND).actions[0].clone()];
    let (_, dazed) = fought(&[&brawler, &blooming], 100, 19);
    let (_, clear) = fought(&[&brawler, &unblooming], 100, 19);
    assert!(clear.contains("reacts: Riposte"));
    assert!(!dazed.contains("reacts: Riposte"));
}
