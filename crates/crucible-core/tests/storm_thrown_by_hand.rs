//! A storm thrown by hand rather than raised overhead: a bolt that jumps
//! from one creature to the next, a line of lightning its caster rides, a
//! whirlpool put down at a distance and left there, a thrown blade that half
//! lands even when it misses and bursts around what it struck - each
//! written through the clause it needs, and fought on a grid.
//!
//! The creatures here are stand-ins with neutral names, not any specific
//! character: a table's own live in its own local files (see the README).
//! What this proves is that every mechanism such a caster leans on is not
//! only loadable but *live* - that a real fight actually applies it.

use crucible_core::creature::{Creature, Rider};
use crucible_core::dsl::config::load_creature_from_str;
use crucible_core::dsl::scenario::parse;
use crucible_core::features::FeatureRegistry;
use crucible_core::prob::Rng;
use crucible_core::sim::arena::gap;
use crucible_core::sim::{replay_teams, Budget, FrameKind, Policy, Replay};

fn replayed(creatures: &[Creature], rounds: u32, seed: u64) -> Replay {
    let roster: Vec<&Creature> = creatures.iter().collect();
    let mut rng = Rng::new(seed);
    replay_teams(
        &mut rng,
        &roster,
        [Policy::Greedy; 2],
        rounds,
        Budget::default(),
    )
    .1
}

fn fought(text: &str, rounds: u32) -> Replay {
    replayed(&parse(text).expect("the scenario loads"), rounds, 11)
}

fn hp(replay: &Replay, seat: usize) -> i32 {
    replay.frames.last().unwrap().units[seat].hp
}

fn at(replay: &Replay, seat: usize) -> (i32, i32) {
    let u = &replay.frames.last().unwrap().units[seat];
    (u.x, u.y)
}

fn said(replay: &Replay, words: &str) -> bool {
    replay.lines.iter().any(|l| l.contains(words))
        || replay.frames.iter().any(|f| f.text.contains(words))
}

/// The caster every test here gives one move to: it goes first and stands
/// still.
fn caster(action: &str) -> String {
    format!(
        "creature: caster\nteam: a\nac: 10\nhp: 100\ninitiative: 50\nspeed: 0\nat: 2, 5\n\
         {action}\n\n"
    )
}

/// Something to hit: it stands where it is put and does nothing.
fn dummy(name: &str, team: char, at: (i32, i32)) -> String {
    format!(
        "creature: {name}\nteam: {team}\nac: 5\nhp: 200\ninitiative: -50\nspeed: 0\n\
         at: {}, {}\n\n",
        at.0, at.1
    )
}

#[test]
fn a_bolt_jumps_from_each_creature_that_fails_to_the_next() {
    let field = |dc: i32| {
        format!(
            "{}{}{}{}{}",
            caster(&format!(
                "action: Arc | range 120 | targets 1 | save dex dc {dc} | 0d6+10 lightning \
                 | half on success | chains 30"
            )),
            dummy("first", 'b', (6, 5)),
            // Thirty feet on from the first, and thirty on from that.
            dummy("second", 'b', (12, 5)),
            dummy("third", 'b', (18, 5)),
            // Too far from any of them for the arc to reach.
            dummy("fourth", 'b', (30, 5)),
        )
    };
    let all_fail = fought(&field(100), 1);
    assert_eq!(
        [1, 2, 3, 4].map(|seat| hp(&all_fail, seat)),
        [190, 190, 190, 200],
        "one after another, as far as thirty feet carries it"
    );
    let first_saves = fought(&field(-100), 1);
    assert_eq!(
        [1, 2, 3, 4].map(|seat| hp(&first_saves, seat)),
        [195, 200, 200, 200],
        "a save stops it where it is"
    );
}

#[test]
fn it_rides_a_line_through_everyone_on_it_and_steps_out_at_the_far_end() {
    let text = format!(
        "{}{}{}{}{}",
        caster("action: Ride | range 60 | line 60 | save dex dc 100 | 0d6+10 lightning"),
        dummy("near", 'b', (5, 5)),
        dummy("further", 'b', (9, 5)),
        dummy("aside", 'b', (5, 9)),
        // One of its own, off the line: a line through it would cost more
        // than it gained.
        dummy("friend", 'a', (2, 9)),
    );
    let before = fought(&text, 0);
    let r = fought(&text, 1);
    assert_eq!((hp(&r, 1), hp(&r, 2)), (190, 190), "both on the line");
    assert_eq!((hp(&r, 3), hp(&r, 4)), (200, 200), "and nobody off it");
    let (was, is) = (at(&before, 0), at(&r, 0));
    assert_eq!((is.0 - was.0, is.1 - was.1), (12, 0), "sixty feet on");
    assert!(said(&r, "reappears"));

    // With nobody to catch, it is not worth taking at all.
    let alone = format!(
        "{}{}",
        caster("action: Ride | range 60 | line 60 | save dex dc 100 | 0d6+10 lightning"),
        dummy("out of reach", 'b', (30, 5)),
    );
    let r = fought(&alone, 1);
    assert_eq!(hp(&r, 1), 200);
    assert_eq!(at(&r, 0), at(&fought(&alone, 0), 0));
}

#[test]
fn a_miss_still_lands_half_and_the_burst_leaves_its_target_out() {
    let field = |to_hit: i32| {
        format!(
            "{}{}{}{}",
            caster(&format!(
                "action: Arrow | ranged | range 60 | hit {to_hit:+} | 0d6+10 lightning \
                 | half on miss && area 10 | spares target | save dex dc 100 | 0d6+4 lightning"
            )),
            dummy("struck", 'b', (8, 5)),
            dummy("beside it", 'b', (9, 5)),
            dummy("far", 'b', (14, 5)),
        )
    };
    let hit = fought(&field(100), 1);
    assert_eq!(
        (hp(&hit, 1), hp(&hit, 2), hp(&hit, 3)),
        (190, 196, 200),
        "the blow for its target, the burst for whoever is near it"
    );
    let miss = fought(&field(-100), 1);
    assert_eq!(
        (hp(&miss, 1), hp(&miss, 2), hp(&miss, 3)),
        (195, 196, 200),
        "half of it on a miss, and the burst all the same"
    );
    assert!(said(&miss, "all the same"));
}

#[test]
fn an_action_that_takes_the_bonus_action_leaves_none() {
    let field = |clause: &str| {
        format!(
            "creature: caster\nteam: a\nac: 10\nhp: 100\ninitiative: 50\nspeed: 0\nat: 2, 5\n\
             action: Throw | ranged | hit +100 | 0d6+10 fire{clause}\n\
             bonus: Off Hand | ranged | hit +100 | 0d6+7 fire\n\n{}",
            dummy("prey", 'b', (6, 5)),
        )
    };
    assert_eq!(200 - hp(&fought(&field(""), 1), 1), 17);
    assert_eq!(200 - hp(&fought(&field(" | takes bonus action"), 1), 1), 10);
}

#[test]
fn a_second_pool_is_paid_beside_the_first_and_both_have_to_hold_out() {
    let text = format!(
        "creature: caster\nteam: a\nac: 10\nhp: 100\ninitiative: 50\nspeed: 0\nat: 2, 5\n\
         resource: essence 4\nresource: charge 1\n\
         action: Wrath | cost essence 2 | also cost charge 1 | range 60 | autohit | 0d6+30 fire\n\
         action: Spark | range 60 | autohit | 0d6+1 fire\n\n{}",
        dummy("prey", 'b', (6, 5)),
    );
    // Essence for two of them, and a charge for one.
    assert_eq!(200 - hp(&fought(&text, 3), 1), 30 + 1 + 1);
}

#[test]
fn jolted_it_cannot_strike_at_what_walks_away() {
    let field = |clause: &str| {
        format!(
            "creature: grasper\nteam: a\nac: 10\nhp: 100\ninitiative: 50\nspeed: 0\nat: 6, 5\n\
             action: Grasp | hit +100 | 0d6+1 lightning{clause}\n\n\
             creature: archer\nteam: a\nac: 10\nhp: 100\ninitiative: 40\nat: 5, 6\n\
             action: Bow | ranged | range 120 | hit +100 | 0d6+1 piercing\n\n\
             creature: brute\nteam: b\nac: 5\nhp: 200\ninitiative: -50\nspeed: 0\nat: 5, 5\n\
             action: Club | hit +100 | 0d6+9 bludgeoning\n"
        )
    };
    let apart = |r: &Replay| gap(at(r, 1), 1, at(r, 2), 1);
    // With the brute at its elbow and ready to strike, the archer stays.
    assert_eq!(apart(&fought(&field(""), 1)), 1);
    // With the brute jolted, it backs off, and nothing swings at it.
    let jolted = fought(&field(" | on hit shocked until victim"), 1);
    assert!(apart(&jolted) > 1, "{:?}", jolted.lines);
    assert!(!said(&jolted, "opportunity attack"));
}

#[test]
fn advantage_on_a_save_is_for_the_saves_it_names() {
    let saved = |traits: &str, kind: &str| {
        let text = format!(
            "{}creature: prey\nteam: b\nac: 5\nhp: 200\ninitiative: -50\nspeed: 0\nat: 6, 5\n\
             {traits}\n",
            caster(&format!(
                "action: Bolt | range 60 | targets 1 | save dex dc 11 | 0d6+10 {kind}"
            )),
        );
        let creatures = parse(&text).unwrap();
        (0..600)
            .filter(|&seed| hp(&replayed(&creatures, 1, seed), 1) == 200)
            .count()
    };
    // A coin toss as it stands; three in four with a second die.
    let (plain, keen) = (
        saved("", "lightning"),
        saved("trait: advantage on dex saves", "lightning"),
    );
    assert!((255..=345).contains(&plain), "{plain} of 600");
    assert!((405..=495).contains(&keen), "{keen} of 600");
    let narrowed = "trait: advantage on dex saves against lightning";
    assert!((405..=495).contains(&saved(narrowed, "lightning")));
    assert!((255..=345).contains(&saved(narrowed, "fire")));
}

/// A caster whose one spell is a whirlpool put down at a distance.
const WHIRLPOOL: &str = r#"
[monster]
name = "Test Caller"
ac = 10
hp = 100
initiative = 50
speed = 0

[[monster.features]]
plugin = "lasting_aura"
name = "Test Whirlpool"
effect = "range 10 | placed 120 | difficult terrain | save str dc 100 | 0d6+6 bludgeoning | on fail pulled"
rounds = 10
concentration = true
"#;

#[test]
fn a_whirlpool_stays_where_it_is_put_and_drags_at_what_starts_a_turn_in_it() {
    let registry = FeatureRegistry::new();
    let mut caller = load_creature_from_str(WHIRLPOOL, &registry).expect("it loads");
    assert_eq!(caller.actions[0].range, Some(120), "raised from a distance");
    assert!(caller.lasting_auras[0].riders.contains(&Rider::Hinders));
    caller.at = Some((2, 5));

    let mut others = parse(&format!(
        "creature: runner\nteam: b\nac: 5\nhp: 200\ninitiative: -40\nat: 14, 5\n\
         action: Punch | hit +100 | 0d6+1 bludgeoning\n\n{}{}",
        dummy("beside it", 'b', (15, 5)),
        // Further from the caller than the runner, and nowhere near the
        // water.
        dummy("well away", 'b', (2, 25)),
    ))
    .unwrap();
    let mut creatures = vec![caller];
    creatures.append(&mut others);

    let r = replayed(&creatures, 1, 11);
    assert_eq!(hp(&r, 1), 194, "whoever it was put around");
    assert_eq!(hp(&r, 2), 194, "and whoever stands near them");
    assert_eq!(hp(&r, 3), 200, "and nobody outside it");
    // The runner starts its turn in it and Dashes for the caller: half its
    // thirty feet for the leg that starts in the water, and all of it once
    // it is out - forty-five feet where dry ground would have given sixty.
    let run = r
        .frames
        .iter()
        .find(|f| f.kind == FrameKind::Move && f.actor == Some(1) && f.path.len() > 2)
        .expect("the runner sets off for the caller");
    assert_eq!(run.path.len(), 9, "{}", run.text);
}

#[test]
fn a_storm_can_be_raised_on_something_other_than_a_slot() {
    let text = r#"
[pc]
name = "Test Stormcaller"
class = "Cleric"
level = 8
ac = 16
hp = 50

[pc.spellcasting]
ability = "wis"
ability_modifier = 4
proficiency_bonus = 3

[pc.resources]
storm = 4

[[pc.features]]
plugin = "call_lightning"
resource = "storm"
cost = 2
"#;
    let c = load_creature_from_str(text, &FeatureRegistry::new()).expect("it loads");
    let first = &c.actions[0];
    assert_eq!(first.spell_slot_level, None);
    assert_eq!(first.cost.map(|cost| cost.amount), Some(2));
    assert!(c.actions[1].cost.is_none(), "later bolts are still free");
}
