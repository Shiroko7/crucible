//! Creatures that are not quite where they seem to be: one that walks
//! through its enemies and trades places with its own kind as a blow lands,
//! one that hunts out of the dark, one whose mind hands what hurts it to
//! whoever is standing near - each written in the scenario format through
//! every trait keyword and clause it needs, and fought on a grid.
//!
//! The creatures here are stand-ins with neutral names, not any specific
//! stat block: a table's own monsters live in its own local files (see the
//! README). What this proves is that every mechanism such creatures lean on
//! is not only loadable but *live* - that a real fight actually applies it.

use crucible_core::creature::Creature;
use crucible_core::dsl::scenario::parse;
use crucible_core::prob::Rng;
use crucible_core::sim::arena::gap;
use crucible_core::sim::{replay_teams, Budget, FrameKind, Policy, Replay};

/// A fight of `rounds` rounds between the creatures `text` declares, the
/// same one every time.
fn fought(text: &str, rounds: u32) -> Replay {
    fought_on(text, rounds, 11)
}

fn fought_on(text: &str, rounds: u32, seed: u64) -> Replay {
    let creatures: Vec<Creature> = parse(text).expect("the scenario loads");
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

fn hp(replay: &Replay, seat: usize) -> i32 {
    replay.frames.last().unwrap().units[seat].hp
}

fn said(replay: &Replay, words: &str) -> bool {
    replay.lines.iter().any(|l| l.contains(words))
        || replay.frames.iter().any(|f| f.text.contains(words))
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
fn only_a_magical_attack_gets_through_a_hide_that_resists_steel() {
    let against = |attacker: &str| {
        let text = format!(
            "creature: attacker\nteam: a\nac: 10\nhp: 50\ninitiative: 50\nat: 5, 5\n\
             {attacker}\n\n\
             creature: hide\nteam: b\nac: 5\nhp: 200\ninitiative: -50\nspeed: 0\nat: 6, 5\n\
             trait: resist nonmagical bludgeoning, piercing, slashing\n"
        );
        200 - hp(&fought(&text, 1), 1)
    };
    let sword = "action: Sword | hit +100 | 0d6+10 slashing";
    assert_eq!(against(sword), 5, "plain steel is halved");
    assert_eq!(
        against(&format!("{sword} | magical")),
        10,
        "an enchanted blade is not"
    );
    assert_eq!(
        against(&format!("trait: magic weapons\n{sword}")),
        10,
        "nor anything swung by something whose weapons all are"
    );
    assert_eq!(
        against("action: Blade of Force | spell | hit +100 | 0d6+10 slashing"),
        10,
        "nor a spell attack"
    );
    assert_eq!(
        against("action: Hail of Blades | save dex dc 100 | 0d6+10 slashing"),
        10,
        "and what is no attack roll at all was never resisted"
    );
    assert_eq!(
        against("action: Torch | hit +100 | 0d6+10 fire"),
        10,
        "only the types it names"
    );
}

#[test]
fn advantage_on_initiative_goes_first_more_often_than_not() {
    let text = "creature: ambusher\nteam: a\nac: 10\nhp: 50\nat: 5, 5\n\
                trait: advantage on initiative\n\
                action: Claw | hit +0 | 1d4 slashing\n\n\
                creature: prey\nteam: b\nac: 10\nhp: 50\nat: 6, 5\n\
                action: Claw | hit +0 | 1d4 slashing\n";
    let fights = 400;
    let first = (0..fights)
        .filter(|&seed| fought_on(text, 1, seed).order[0] == 0)
        .count();
    // Two dice against one: about two times in three.
    assert!(
        first * 100 > fights as usize * 58,
        "went first {first} times of {fights}"
    );
}

/// An ambusher: two claws that hurt more on a roll with advantage, and a
/// bonus action that hides it - while it stands in the dark.
fn ambusher(light: &str) -> String {
    format!(
        "creature: ambusher\nteam: a\nac: 10\nhp: 50\ninitiative: 50\nat: 5, 5\n\
         trait: {light}\n\
         action: Claws | strikes 2 | hit +100 | 0d6+10 slashing \
         | bonus 0d6+7 psychic with advantage\n\
         bonus: Fade | in dim light | hide +100\n\n{}",
        dummy("prey", 'b', (6, 5))
    )
}

#[test]
fn hiding_in_the_dark_puts_advantage_on_the_first_blow_and_only_that_one() {
    let dark = fought(&ambusher("in dim light"), 2);
    // Round one: two claws from plain sight, then it hides. Round two: the
    // first claw from hiding carries the extra 7, and gives it away.
    assert_eq!(200 - hp(&dark, 1), 20 + 27);
    assert!(said(&dark, "hidden"));

    let lit = fought(&ambusher("in bright light"), 2);
    assert_eq!(
        200 - hp(&lit, 1),
        40,
        "in the light there is nowhere to hide"
    );
    assert!(!said(&lit, "hidden"));
}

#[test]
fn a_keen_enough_eye_is_not_hidden_from() {
    let watched =
        ambusher("in darkness").replace("hide +100", "hide +0") + "trait: passive perception 30\n";
    let r = fought(&watched, 2);
    assert_eq!(200 - hp(&r, 1), 40);
    assert!(said(&r, "fails to hide"), "{:?}", r.lines);
}

#[test]
fn every_blow_landing_on_one_creature_adds_its_own_harm_and_a_save() {
    let text = "creature: brute\nteam: a\nac: 10\nhp: 50\ninitiative: 50\nat: 5, 5\n\
         action: Slams | strikes 2 | hit +100 | 0d6+10 bludgeoning \
         | all hit 0d6+5 psychic save con dc 100 stunned until victim end\n\n\
         creature: prey\nteam: b\nac: 5\nhp: 200\ninitiative: -50\nat: 6, 5\n\
         action: Punch | hit +100 | 0d6+9 bludgeoning\n";
    let r = fought(text, 2);
    assert_eq!(200 - hp(&r, 1), 2 * 25, "ten, ten, and five more");
    assert_eq!(hp(&r, 0), 50, "stunned through both of its own turns");
    assert!(said(&r, "every blow having landed"));
}

#[test]
fn a_swing_at_everything_in_reach_spares_nobody_near() {
    let text = format!(
        "creature: brute\nteam: a\nac: 10\nhp: 50\ninitiative: 50\nspeed: 0\nat: 5, 5\n\
         action: Reap | range 10 | strikes each | hit +100 | 0d6+10 bludgeoning \
         | on hit save dex dc 100 prone until victim\n\n{}{}{}{}",
        dummy("beside", 'b', (6, 5)),
        dummy("two off", 'b', (7, 5)),
        dummy("far", 'b', (9, 5)),
        dummy("friend", 'a', (4, 5)),
    );
    let r = fought(&text, 1);
    assert_eq!(hp(&r, 1), 190);
    assert_eq!(hp(&r, 2), 190, "ten feet is in reach");
    assert_eq!(hp(&r, 3), 200, "twenty is not");
    assert_eq!(hp(&r, 4), 190, "and its own side is not spared");
    // Each one hit saves or goes down - and is up again on its own turn.
    let knocked = r
        .frames
        .iter()
        .filter(|f| f.kind == FrameKind::Condition && f.text.contains("prone"))
        .count();
    assert_eq!(knocked, 3, "{:?}", r.lines);
}

#[test]
fn a_mirror_hands_what_would_hurt_it_to_everyone_near() {
    let text = format!(
        "creature: lancer\nteam: a\nac: 10\nhp: 100\ninitiative: 50\nspeed: 0\nat: 5, 5\n\
         action: Lance | hit +100 | 0d6+10 piercing, 0d6+8 psychic\n\n\
         creature: mirror\nteam: b\nac: 5\nhp: 200\ninitiative: -50\nspeed: 0\nat: 6, 5\n\
         trait: mirror psychic 10\n\n{}{}",
        dummy("its friend", 'b', (7, 5)),
        dummy("far friend", 'b', (12, 5)),
    );
    let r = fought(&text, 1);
    assert_eq!(hp(&r, 1), 190, "the piercing is its own to take");
    assert_eq!(hp(&r, 0), 92, "the psychic comes back on whoever is near");
    assert_eq!(hp(&r, 2), 192, "whichever side they are on");
    assert_eq!(hp(&r, 3), 200, "and not on anyone further off");

    // Given the choice, nobody aims a mind at it: the same blow in steel is
    // worth more, though it rolls for less.
    let choosing = text.replace(
        "action: Lance | hit +100 | 0d6+10 piercing, 0d6+8 psychic",
        "action: Thought | hit +100 | 0d6+30 psychic\n\
         action: Steel | hit +100 | 0d6+10 piercing",
    );
    let r = fought(&choosing, 1);
    assert_eq!(hp(&r, 1), 190);
    assert_eq!(hp(&r, 0), 100);
}

#[test]
fn hit_it_changes_places_with_one_of_its_own_which_is_hit_instead() {
    let text = format!(
        "{}creature: seer\nteam: b\nac: 5\nhp: 200\ninitiative: -50\nspeed: 0\nat: 6, 5\n\
         trait: swap when hit 60 rift\n\n\
         creature: rift guard\nteam: b\nac: 5\nhp: 150\ninitiative: -50\nspeed: 0\nat: 12, 5\n\
         resist: slashing\n\n{}",
        "creature: knight\nteam: a\nac: 10\nhp: 100\ninitiative: 50\nspeed: 0\nat: 5, 5\n\
         action: Sword | hit +100 | 0d6+10 slashing\n\n",
        dummy("bystander", 'b', (12, 8)),
    );
    let before = fought(&text, 0);
    let r = fought(&text, 1);
    assert_eq!(hp(&r, 1), 200, "the seer is not where the blow lands");
    assert_eq!(
        hp(&r, 2),
        145,
        "its guard is - and takes it as its own hide does"
    );
    assert_eq!(hp(&r, 3), 200, "something of another kind is no use to it");
    assert!(said(&r, "changes places"));
    let at = |replay: &Replay, seat: usize| {
        let u = &replay.frames.last().unwrap().units[seat];
        (u.x, u.y)
    };
    assert_eq!(at(&r, 1), at(&before, 2));
    assert_eq!(at(&r, 2), at(&before, 1));

    // One reaction a round: the second blow of two is the guard's again
    // only because the guard is now what stands in reach.
    let twice = text.replace("action: Sword | hit", "action: Sword | strikes 2 | hit");
    let r = fought(&twice, 1);
    assert_eq!(hp(&r, 1), 200);
    assert_eq!(hp(&r, 2), 140);

    // And with nobody of its kind in reach, it takes the blow itself.
    let alone = text.replace("swap when hit 60 rift", "swap when hit 20 rift");
    assert_eq!(hp(&fought(&alone, 1), 1), 190);
}

/// Space folded around `prey`, which stands beside `near` and well away
/// from `far`.
fn folding(clauses: &str) -> Replay {
    let text = format!(
        "creature: folder\nteam: a\nac: 10\nhp: 100\ninitiative: 50\nspeed: 0\nat: 2, 5\n\
         action: Fold | range 30 | {clauses}\n\n{}{}{}",
        dummy("prey", 'b', (7, 5)),
        dummy("near", 'b', (8, 5)),
        dummy("far", 'b', (14, 5)),
    );
    fought(&text, 1)
}

#[test]
fn folded_space_throws_its_victim_and_bursts_where_it_stood() {
    let spot = |replay: &Replay, frame: usize, seat: usize| {
        let u = &replay.frames[frame].units[seat];
        (u.x, u.y)
    };
    let fold = "on fail thrown 60 | blast 10 0d6+12 psychic | on success 0d6+5 psychic";

    // Failed: set down again inside the blast, which takes it with the rest.
    let r = folding(&format!("save wis dc 100 | {fold}"));
    assert_eq!((hp(&r, 1), hp(&r, 2), hp(&r, 3)), (188, 188, 200));
    let last = r.frames.len() - 1;
    let (was, is) = (spot(&r, 0, 1), spot(&r, last, 1));
    assert_ne!(was, is, "it is somewhere else");
    assert!(gap(was, 1, is, 1) <= 2, "but still within ten feet");

    // Thrown away instead: clear of the blast, as far as it will go.
    let r = folding(&format!("save wis dc 100 | {fold} away").replace(
        "on fail thrown 60 | blast 10 0d6+12 psychic | on success 0d6+5 psychic away",
        "on fail thrown 60 away | blast 10 0d6+12 psychic | on success 0d6+5 psychic",
    ));
    assert_eq!((hp(&r, 1), hp(&r, 2), hp(&r, 3)), (200, 188, 200));
    let last = r.frames.len() - 1;
    let (was, is) = (spot(&r, 0, 1), spot(&r, last, 1));
    assert!(gap(was, 1, is, 1) > 2 && gap(was, 1, is, 1) <= 12);

    // Saved: nobody moves, and only the one it was aimed at pays.
    let r = folding(&format!("save wis dc -100 | {fold}"));
    assert_eq!((hp(&r, 1), hp(&r, 2), hp(&r, 3)), (195, 200, 200));
    let last = r.frames.len() - 1;
    assert_eq!(spot(&r, 0, 1), spot(&r, last, 1));
}

#[test]
fn it_walks_through_its_enemies_and_nothing_gets_a_swing_at_it() {
    let text = "creature: phaser\nteam: a\nac: 10\nhp: 100\ninitiative: 50\nat: 3, 5\n\
                trait: phase through 0d6+4 psychic\n\
                trait: no opportunity attacks\n\
                action: Staff | hit +100 | 0d6+10 bludgeoning\n\n\
                creature: prey\nteam: b\nac: 5\nhp: 200\ninitiative: -50\nspeed: 0\nat: 6, 5\n\
                action: Punch | hit +100 | 0d6+1 bludgeoning\n";
    let r = fought(text, 3);
    // Each turn: through the prey for 4, out the other side, and a blow.
    assert_eq!(200 - hp(&r, 1), 3 * 14);
    assert!(said(&r, "passes through prey"));
    assert!(
        !said(&r, "opportunity attack"),
        "leaving its reach three times over"
    );
    let walk = r
        .frames
        .iter()
        .find(|f| f.kind == FrameKind::Move && f.actor == Some(0))
        .expect("it walks");
    let prey = &walk.units[1];
    assert!(walk.path.contains(&(prey.x, prey.y)), "{:?}", walk.path);

    // The same creature without the knack walks up, stops, and stays put.
    let plain = text
        .replace("trait: phase through 0d6+4 psychic\n", "")
        .replace("trait: no opportunity attacks\n", "");
    assert_eq!(200 - hp(&fought(&plain, 3), 1), 3 * 10);
}

#[test]
fn a_move_that_carries_its_user_reaches_twice_as_far() {
    let text = |clause: &str| {
        format!(
            "creature: stalker\nteam: a\nac: 10\nhp: 100\ninitiative: 50\nat: 2, 5\n\
             action: Rush | strikes 3 | hit +100 | 0d6+10 slashing{clause}\n\n{}",
            dummy("prey", 'b', (13, 5)),
        )
    };
    // Fifty feet from its prey with thirty feet of speed: a plain attack
    // spends the turn getting there.
    let plain = fought(&text(""), 1);
    assert_eq!(hp(&plain, 1), 200);
    assert!(said(&plain, "dashes"));
    // Carried by the move itself, it covers the fifty feet and strikes in
    // the one turn.
    let carried = fought(&text(" | no opportunity attacks"), 1);
    assert_eq!(hp(&carried, 1), 170, "{:?}", carried.lines);
}
