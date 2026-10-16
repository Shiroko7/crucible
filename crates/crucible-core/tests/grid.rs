//! A fight on a grid: creatures with somewhere to stand walk before they
//! act, a blow lands only on what it reaches, and the whole thing comes back
//! as frames a viewer can play.

use crucible_core::creature::{
    AttackKind, AttackTrigger, Creature, Effect, Move, Reaction, ReactionTrigger, SaveEffect,
    Strike,
};
use crucible_core::prob::Rng;
use crucible_core::rules::{Ability, DamageKind, DamageRoll};
use crucible_core::sim::arena::{gap, place};
use crucible_core::sim::showcase::showcase;
use crucible_core::sim::{replay_teams, run_teams, Budget, FrameKind, Policy, Replay};

fn swing(kind: AttackKind) -> Move {
    Move::new(
        "Swing",
        Effect::Strikes {
            strike: Strike::new(100, vec![DamageRoll::new(0, 6, 10, DamageKind::Slashing)])
                .with_kind(kind),
            count: 1,
        },
    )
}

/// A creature that always hits for 10, on `team`, standing at `x`.
fn fighter(name: &str, team: u8, x: i32, m: Move) -> Creature {
    let mut c = Creature::new(name, 10, 100).with_action(m);
    c.team = team;
    c.at = Some((x, 5));
    c
}

fn replay(roster: &[&Creature], rounds: u32) -> Replay {
    let mut rng = Rng::new(7);
    replay_teams(
        &mut rng,
        roster,
        [Policy::Greedy; 2],
        rounds,
        Budget::default(),
    )
    .1
}

fn hp_after(replay: &Replay, seat: usize) -> i32 {
    replay.frames.last().unwrap().units[seat].hp
}

#[test]
fn a_sword_walks_up_to_its_target_and_then_swings() {
    let a = fighter("a", 0, 4, swing(AttackKind::MELEE_WEAPON));
    let mut b = fighter("b", 1, 11, swing(AttackKind::MELEE_WEAPON));
    b.initiative = -100;
    let mut a = a;
    a.initiative = 100;
    let r = replay(&[&a, &b], 1);

    let walk = r
        .frames
        .iter()
        .find(|f| f.kind == FrameKind::Move)
        .expect("somebody walks");
    assert_eq!(walk.actor, Some(0), "the faster one closes the distance");
    assert_eq!(walk.path.len(), 6, "30 feet, to the square beside it");
    let (a_at, b_at) = (&walk.units[0], &walk.units[1]);
    assert_eq!(gap((a_at.x, a_at.y), 1, (b_at.x, b_at.y), 1), 1);
    assert_eq!(hp_after(&r, 1), 90, "and its blow lands the same turn");
    assert_eq!(hp_after(&r, 0), 90, "the other was already in reach");
    assert!(r.lines.iter().any(|l| l.contains("moves 30 ft toward b")));
}

#[test]
fn out_of_reach_is_a_dash_for_a_sword_and_nothing_to_a_bow() {
    let mut sword = fighter("sword", 0, 4, swing(AttackKind::MELEE_WEAPON));
    sword.initiative = 100;
    let mut bow = fighter(
        "bow",
        1,
        24,
        swing(AttackKind::RANGED_WEAPON).with_range(150),
    );
    bow.initiative = -100;
    let r = replay(&[&sword, &bow], 1);

    assert_eq!(hp_after(&r, 1), 100, "100 feet is two turns away");
    assert_eq!(hp_after(&r, 0), 90, "but well inside a longbow's range");
    let dash = r.frames.iter().find(|f| f.kind == FrameKind::Move).unwrap();
    assert_eq!(dash.path.len(), 12, "a Dash: twice its speed");
    assert!(dash.text.contains("dashes"), "{}", dash.text);
    // The archer does not wait for it: it backs off as far as its own
    // speed takes it, which is still well inside its range.
    let retreat = r
        .frames
        .iter()
        .find(|f| f.kind == FrameKind::Move && f.actor == Some(1))
        .expect("the archer keeps its distance");
    assert_eq!(retreat.path.len(), 6, "thirty feet");
    let last = r.frames.last().unwrap();
    let (s, b) = (&last.units[0], &last.units[1]);
    assert_eq!(gap((s.x, s.y), 1, (b.x, b.y), 1), 20 - 12 + 6);

    // Without the range written down a ranged attack reaches 60 feet, so
    // the same archer has to close in first.
    let mut short = bow.clone();
    short.actions[0].range = None;
    sword.speed = 0;
    let r = replay(&[&sword, &short], 1);
    assert!(r
        .frames
        .iter()
        .any(|f| f.kind == FrameKind::Move && f.actor == Some(1)));
}

#[test]
fn a_slower_creature_takes_longer_to_arrive() {
    let mut slow = fighter("slow", 0, 4, swing(AttackKind::MELEE_WEAPON));
    slow.speed = 15;
    let mut dummy = fighter("dummy", 1, 11, swing(AttackKind::MELEE_WEAPON));
    dummy.speed = 0;
    let r = replay(&[&slow, &dummy], 1);
    assert_eq!(hp_after(&r, 1), 100, "15 feet a turn does not cover 30");
    let r = replay(&[&slow, &dummy], 2);
    assert_eq!(
        hp_after(&r, 1),
        90,
        "a Dash in the first round, a blow in the second"
    );
}

#[test]
fn an_area_catches_only_what_stands_close_to_its_target() {
    let burst = |area: Option<u32>| {
        let mut m = Move::new(
            "Burst",
            Effect::Save(SaveEffect {
                ability: Ability::Dex,
                dc: 99,
                damage: vec![DamageRoll::new(0, 6, 10, DamageKind::Fire)],
                half_on_success: false,
                on_failure: vec![],
                max_targets: None,
                requires_type: None,
            }),
        );
        m.area = area;
        m
    };
    let idle = || Move::new("Wait", Effect::Sequence(vec![]));
    let near = fighter("near", 1, 12, idle());
    let mut beside = fighter("beside", 1, 12, idle());
    beside.at = Some((12, 7));
    let mut far = fighter("far", 1, 12, idle());
    far.at = Some((14, 14));

    let after = |area| {
        let caster = fighter("caster", 0, 4, burst(area));
        let r = replay(&[&caster, &near, &beside, &far], 1);
        [hp_after(&r, 1), hp_after(&r, 2), hp_after(&r, 3)]
    };
    assert_eq!(
        after(None),
        [90, 90, 90],
        "everything in range, unless it says"
    );
    assert_eq!(after(Some(10)), [90, 90, 100], "ten feet around the target");
}

#[test]
fn only_so_many_fit_around_one_target() {
    // Nine swords against a single enemy: eight squares touch it, so at
    // most eight of them get a blow in.
    let mut target = fighter("target", 1, 14, Move::new("Wait", Effect::Sequence(vec![])));
    target.hp = 10_000;
    let mut roster: Vec<Creature> = (0..9)
        .map(|i| {
            let mut c = fighter(&format!("s{i}"), 0, 4, swing(AttackKind::MELEE_WEAPON));
            c.at = None;
            c.speed = 60;
            c
        })
        .collect();
    target.at = None;
    roster.push(target);
    place(&mut roster, 30);
    let refs: Vec<&Creature> = roster.iter().collect();
    let r = replay(&refs, 1);
    let end = &r.frames.last().unwrap().units;
    let beside = (0..9)
        .filter(|&i| gap((end[i].x, end[i].y), 1, (end[9].x, end[9].y), 1) == 1)
        .count();
    assert_eq!(beside, 8, "eight reach it, the ninth waits behind them");
    let swings = r
        .frames
        .iter()
        .filter(|f| f.kind == FrameKind::Attack)
        .count();
    assert_eq!(swings, 8, "and only those eight get to swing");
}

#[test]
fn without_somewhere_to_stand_a_fight_has_no_grid() {
    let mut a = fighter("a", 0, 4, swing(AttackKind::MELEE_WEAPON));
    let mut b = fighter("b", 1, 40, swing(AttackKind::MELEE_WEAPON));
    a.at = None;
    b.at = None;
    let r = replay(&[&a, &b], 1);
    assert_eq!((r.width, r.height), (0, 0));
    assert!(r.frames.iter().all(|f| f.kind != FrameKind::Move));
    assert_eq!(hp_after(&r, 0), 90);
    assert_eq!(hp_after(&r, 1), 90);
}

#[test]
fn a_recorded_fight_is_the_fight_that_was_run() {
    let a = fighter("a", 0, 4, swing(AttackKind::MELEE_WEAPON));
    let b = fighter("b", 1, 11, swing(AttackKind::MELEE_WEAPON));
    let roster = [&a, &b];
    let run = |seed| {
        let mut rng = Rng::new(seed);
        let mut log = None;
        run_teams(
            &mut rng,
            &roster,
            [Policy::Greedy; 2],
            50,
            Budget::default(),
            &mut log,
        )
    };
    for seed in 0..20 {
        let mut rng = Rng::new(seed);
        let (outcome, r) = replay_teams(
            &mut rng,
            &roster,
            [Policy::Greedy; 2],
            50,
            Budget::default(),
        );
        assert_eq!(outcome, run(seed), "recording changes nothing");
        assert_eq!(r.frames.first().unwrap().kind, FrameKind::Start);
        assert_eq!(r.frames.last().unwrap().kind, FrameKind::End);
        assert_eq!(r.winner, outcome.winner.map(|s| s.index() as u8));
    }
}

#[test]
fn a_showcase_holds_three_fights_a_playstyle_worst_first() {
    let a = fighter(
        "a",
        0,
        4,
        Move::new(
            "Swing",
            Effect::Strikes {
                strike: Strike::new(5, vec![DamageRoll::new(2, 8, 3, DamageKind::Slashing)]),
                count: 1,
            },
        ),
    );
    let b = fighter("b", 1, 11, a.actions[0].clone());
    let bundle = showcase(
        "duel",
        3,
        &[&a, &b],
        &[Policy::Greedy, Policy::Thrifty],
        Policy::Greedy,
        200,
        50,
        Budget::default(),
    );
    assert_eq!(bundle.entries.len(), 6);
    let picks: Vec<&str> = bundle.entries[..3]
        .iter()
        .map(|e| e.pick.as_str())
        .collect();
    assert_eq!(picks, ["worst", "median", "best"]);
    assert_eq!(
        bundle.entries[0].replay.winner,
        Some(1),
        "the worst is a loss"
    );
    assert_eq!(
        bundle.entries[2].replay.winner,
        Some(0),
        "the best is a win"
    );
    assert!(bundle.entries.iter().all(|e| e.replay.width > 0));
}

#[test]
fn the_map_is_never_small_and_is_as_big_as_it_is_asked_to_be() {
    let a = fighter("a", 0, 4, swing(AttackKind::MELEE_WEAPON));
    let b = fighter("b", 1, 11, swing(AttackKind::MELEE_WEAPON));
    let on = |field: Option<(i32, i32)>| {
        let mut rng = Rng::new(7);
        let budget = Budget {
            field,
            ..Budget::default()
        };
        replay_teams(&mut rng, &[&a, &b], [Policy::Greedy; 2], 1, budget).1
    };
    // How far apart the two are, and how far off the middle of a map this
    // size the pair of them is - a square, at most, where the numbers are
    // odd.
    let apart = |r: &Replay| {
        let (a, b) = (&r.frames[0].units[0], &r.frames[0].units[1]);
        let off = (a.x + b.x + 1 - r.width, a.y + b.y + 1 - r.height);
        ((b.x - a.x, b.y - a.y), off.0.abs() <= 1 && off.1.abs() <= 1)
    };

    // Two creatures seven squares apart need very little, and get 200 feet
    // by 150 all the same, standing in the middle of it as they stood.
    let least = on(None);
    assert_eq!((least.width, least.height), (40, 30));
    assert_eq!(apart(&least), ((7, 0), true));
    // Asking for less than that changes nothing; asking for more is given.
    let small = on(Some((10, 10)));
    assert_eq!((small.width, small.height), (40, 30));
    let big = on(Some((80, 60)));
    assert_eq!((big.width, big.height), (80, 60));
    assert_eq!(apart(&big), ((7, 0), true));
}

#[test]
fn an_archer_with_a_sword_already_upon_it_stands_its_ground() {
    let mut sword = fighter("sword", 0, 9, swing(AttackKind::MELEE_WEAPON));
    sword.initiative = -100;
    let mut bow = fighter(
        "bow",
        1,
        10,
        swing(AttackKind::RANGED_WEAPON).with_range(150),
    );
    bow.initiative = 100;
    let r = replay(&[&sword, &bow], 2);
    assert!(
        !r.frames.iter().any(|f| f.kind == FrameKind::Move),
        "nobody has anywhere better to be: {:?}",
        r.lines
    );
}

/// Holds a ring of harm up once it has cast it, and has only a ranged
/// cantrip besides - so nothing but the ring gives it a reason to close in.
fn ring_bearer(x: i32) -> Creature {
    let mut c = crucible_core::dsl::config::load_creature_from_str(
        r#"
[pc]
name = "bearer"
ac = 10
hp = 100
initiative = 100

[pc.resources.slots]
3 = 1

[pc.spellcasting]
ability = "wis"
ability_modifier = 4
proficiency_bonus = 3

[[pc.features]]
plugin = "lasting_aura"
name = "Ring"
slot = 3
rounds = 100
concentration = true
effect = "save con dc 99 | 1d1+9 radiant"

[[pc.actions]]
name = "Spark"
effect = "ranged | range 120 | strikes 1 | hit -99 | 1d1 force"
"#,
        &crucible_core::features::FeatureRegistry::new(),
    )
    .expect("it compiles");
    c.at = Some((x, 5));
    c
}

#[test]
fn whoever_holds_an_emanation_walks_it_onto_its_enemies() {
    let idle = || swing(AttackKind::MELEE_WEAPON);
    let mut near = fighter("near", 1, 18, idle());
    let mut beside = fighter("beside", 1, 18, idle());
    beside.at = Some((18, 7));
    for foe in [&mut near, &mut beside] {
        foe.speed = 0;
        foe.initiative = -100;
    }
    let r = replay(&[&ring_bearer(4), &near, &beside], 2);
    // It sets off for them before it casts - an emanation is raised where
    // its target will be inside it - and seventy feet is two turns' walk:
    // the ring goes up on the way, and is carried the rest.
    let walks: Vec<_> = r
        .frames
        .iter()
        .filter(|f| f.kind == FrameKind::Move)
        .collect();
    assert_eq!(walks.len(), 2, "{:?}", r.lines);
    assert_eq!((walks[0].round, walks[0].actor), (1, Some(0)));
    assert_eq!(walks[0].path.len(), 6, "all of its speed: {:?}", r.lines);
    assert_eq!((walks[1].round, walks[1].actor), (2, Some(0)));
    // What the ring is carried over takes it there and then, and again as
    // its own turn comes round inside it.
    let swept: Vec<&String> = r
        .lines
        .iter()
        .filter(|l| l.contains("(aura, on the move)"))
        .collect();
    assert_eq!(swept.len(), 2, "{:?}", r.lines);
    assert!(swept.iter().all(|l| l.starts_with("r2 bearer")));
    assert_eq!((hp_after(&r, 1), hp_after(&r, 2)), (80, 80));
}

#[test]
fn walking_into_an_emanation_is_meeting_it() {
    let mut bearer = ring_bearer(4);
    bearer.speed = 0;
    // It never lands a blow, so nothing ever breaks the concentration.
    let poke = Move::new(
        "Poke",
        Effect::Strikes {
            strike: Strike::new(-100, vec![DamageRoll::new(1, 4, 0, DamageKind::Piercing)]),
            count: 1,
        },
    );
    let mut sword = fighter("sword", 1, 9, poke);
    sword.initiative = -100;
    let r = replay(&[&bearer, &sword], 2);
    // Round one: the ring goes up, and the sword walks into it to swing.
    // Round two: it starts its turn there.
    let met: Vec<&String> = r.lines.iter().filter(|l| l.contains("(aura")).collect();
    assert_eq!(met.len(), 2, "{:?}", r.lines);
    assert!(
        met[0].starts_with("r1 bearer (aura, on the move)"),
        "{met:?}"
    );
    assert!(met[1].starts_with("r2 bearer (aura)"), "{met:?}");
    assert_eq!(hp_after(&r, 1), 80);
}

/// A sword with a guard at its elbow and a prize across the room: the guard
/// hits whatever it swings at for ten, and never moves.
fn past_a_guard(guard: Creature, hero: Policy, guards: Policy) -> Replay {
    let mut sword = fighter("sword", 0, 10, swing(AttackKind::MELEE_WEAPON));
    sword.initiative = 100;
    let mut guard = guard;
    guard.at = Some((11, 5));
    guard.speed = 0;
    guard.initiative = -100;
    let mut prize = fighter("prize", 1, 16, swing(AttackKind::MELEE_WEAPON));
    prize.hp = 5;
    prize.speed = 0;
    prize.initiative = -100;
    let mut rng = Rng::new(7);
    replay_teams(
        &mut rng,
        &[&sword, &guard, &prize],
        [hero, guards],
        1,
        Budget::default(),
    )
    .1
}

fn opportunity(r: &Replay) -> usize {
    r.lines
        .iter()
        .filter(|l| l.contains("(opportunity attack)"))
        .count()
}

#[test]
fn walking_out_of_a_reach_is_an_opening_for_whoever_it_was() {
    let guard = || fighter("guard", 1, 11, swing(AttackKind::MELEE_WEAPON));
    // Set on finishing the weakest thing in the room, it goes - and the
    // guard it turns its back on takes its swing as it does.
    let r = past_a_guard(guard(), Policy::FocusFire, Policy::Greedy);
    assert_eq!(opportunity(&r), 1, "{:?}", r.lines);
    assert!(r
        .lines
        .iter()
        .any(|l| l.contains("moves 25 ft toward prize")));
    assert_eq!(hp_after(&r, 2), 0, "and still gets there");
    // That swing is all the guard gets: rooted where it is, the sword is
    // out of its reach by the time its own turn comes.
    assert_eq!(hp_after(&r, 0), 100 - 10);

    // Played to win the exchange in front of it, it fights the guard it
    // can reach rather than take a blow to reach something else.
    let r = past_a_guard(guard(), Policy::Greedy, Policy::Greedy);
    assert_eq!(opportunity(&r), 0, "{:?}", r.lines);
    assert!(!r.frames.iter().any(|f| f.kind == FrameKind::Move));
    assert_eq!((hp_after(&r, 1), hp_after(&r, 2)), (90, 5));
}

#[test]
fn a_reaction_with_something_better_to_wait_for_is_kept() {
    // The same guard, with a riposte for whoever hits it - worth more to it,
    // its author says, than a swing at a back.
    let riposting = || {
        let mut guard = fighter("guard", 1, 11, swing(AttackKind::MELEE_WEAPON));
        let mut riposte = swing(AttackKind::MELEE_WEAPON);
        riposte.name = "Riposte".to_string();
        riposte.worth = 50;
        guard.reactions.push(Reaction {
            trigger: ReactionTrigger::Hit(AttackTrigger::AnyAttack),
            action: riposte,
        });
        guard
    };
    // Weighing the two, it lets the sword go by.
    let r = past_a_guard(riposting(), Policy::FocusFire, Policy::Greedy);
    assert_eq!(opportunity(&r), 0, "{:?}", r.lines);
    assert_eq!(hp_after(&r, 2), 0);
    // Spending whatever it has as it comes, it does not wait.
    let r = past_a_guard(riposting(), Policy::FocusFire, Policy::Nova);
    assert_eq!(opportunity(&r), 1, "{:?}", r.lines);
}

#[test]
fn nothing_swings_at_what_is_shoved_or_at_what_it_cannot_answer() {
    // A guard with no reaction left - spent, here, by being stunned out of
    // having one - is no reason to stay: the archer beside it backs off.
    let mut bow = fighter(
        "bow",
        0,
        10,
        swing(AttackKind::RANGED_WEAPON).with_range(150),
    );
    bow.initiative = 100;
    let mut guard = fighter(
        "guard",
        1,
        11,
        swing(AttackKind::RANGED_WEAPON).with_range(150),
    );
    guard.speed = 0;
    guard.initiative = -100;
    // Its only attack is from range: it has nothing to swing at a back.
    let r = replay(&[&bow, &guard], 1);
    assert_eq!(opportunity(&r), 0);
    assert!(
        r.frames
            .iter()
            .any(|f| f.kind == FrameKind::Move && f.actor == Some(0)),
        "free to go, it goes: {:?}",
        r.lines
    );
}
