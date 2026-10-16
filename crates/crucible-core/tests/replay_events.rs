//! A recorded fight, beat by beat: every roll is a frame of its own, carries
//! the dice that actually fell, and adds up to what the fight did.

use crucible_core::creature::{AttackKind, Creature, Effect, Move, MoveKind, Strike};
use crucible_core::dsl::scenario;
use crucible_core::prob::Rng;
use crucible_core::rules::{DamageKind, DamageRoll, HealRoll};
use crucible_core::sim::arena::place;
use crucible_core::sim::{replay_teams, Budget, Detail, FrameKind, Policy, Replay};

const BRAWL: &str = "\
creature: Monk
ac: 16
hp: 60
initiative: +3
saves: con +2
resource: focus 4
action: Staff | strikes 2 | hit +7 | 1d8+4 bludgeoning
             | on hit save con dc 15 stunned once cost focus 1
action: Chill Touch | spell | ranged | hit +5 | 2d8 necrotic
team: b
creature: Ogre
size: large
ac: 11
hp: 59
initiative: -1
saves: con +3
action: Greatclub | hit +6 | 2d8+4 bludgeoning
count: 2
";

fn brawl(seed: u64) -> Replay {
    let mut creatures = scenario::parse(BRAWL).expect("parses");
    place(&mut creatures, 30);
    let roster: Vec<&Creature> = creatures.iter().collect();
    let mut rng = Rng::new(seed);
    replay_teams(
        &mut rng,
        &roster,
        [Policy::Greedy; 2],
        50,
        Budget::default(),
    )
    .1
}

#[test]
fn a_fight_opens_with_one_initiative_roll_each_and_the_order_they_give() {
    let r = brawl(1);
    assert_eq!(r.frames[0].kind, FrameKind::Start);
    let rolls: Vec<(usize, i32)> = r.frames[1..4]
        .iter()
        .map(|f| match (&f.kind, &f.detail) {
            (
                FrameKind::Initiative,
                Some(Detail::Initiative {
                    die,
                    modifier,
                    total,
                }),
            ) => {
                assert!((1..=20).contains(die));
                assert_eq!(*total, die + modifier);
                (f.actor.expect("somebody rolled"), *total)
            }
            other => panic!("expected an initiative roll, got {other:?}"),
        })
        .collect();
    assert_eq!(rolls.iter().map(|r| r.0).collect::<Vec<_>>(), [0, 1, 2]);
    assert_eq!(r.frames[4].kind, FrameKind::Turn, "then the first turn");

    // Highest first; the engine's own tie-breaks decide the rest.
    let total = |seat: usize| rolls.iter().find(|r| r.0 == seat).unwrap().1;
    for pair in r.order.windows(2) {
        assert!(total(pair[0]) >= total(pair[1]), "{:?} {rolls:?}", r.order);
    }
    assert_eq!(r.frames[4].actor, Some(r.order[0]));
}

#[test]
fn every_attack_roll_shows_its_dice_and_agrees_with_its_outcome() {
    let mut seen = [0usize; 3];
    for seed in 0..30 {
        let r = brawl(seed);
        for (i, f) in r.frames.iter().enumerate() {
            let Some(Detail::Attack {
                dice,
                kept,
                modifier,
                total,
                ac,
                outcome,
                ..
            }) = &f.detail
            else {
                continue;
            };
            assert_eq!(f.kind, FrameKind::Attack);
            assert!(
                dice.contains(kept) && (1..=2).contains(&dice.len()),
                "{f:?}"
            );
            assert!(dice.iter().all(|d| (1..=20).contains(d)));
            assert_eq!(*total, kept + modifier);
            // A natural 20 always hits and a natural 1 never does.
            let lands = *kept == 20 || (*kept != 1 && total >= ac);
            assert_eq!(lands, outcome != "miss", "{}", f.text);
            assert_eq!(outcome == "crit", *kept == 20, "{}", f.text);
            seen[["miss", "hit", "crit"]
                .iter()
                .position(|o| o == outcome)
                .unwrap()] += 1;

            // A hit is followed by its damage; a miss by none.
            let next = &r.frames[i + 1];
            assert_eq!(
                next.kind == FrameKind::Damage,
                outcome != "miss",
                "{} then {}",
                f.text,
                next.text
            );
            // Nobody's hit points move on the roll itself.
            assert_eq!(
                f.units.iter().map(|u| u.hp).collect::<Vec<_>>(),
                r.frames[i - 1]
                    .units
                    .iter()
                    .map(|u| u.hp)
                    .collect::<Vec<_>>()
            );
        }
    }
    assert!(seen.iter().all(|&n| n > 0), "misses, hits, crits: {seen:?}");
}

#[test]
fn damage_frames_are_what_the_hit_points_lost() {
    let r = brawl(3);
    let mut frames = 0;
    for (i, f) in r.frames.iter().enumerate() {
        let Some(Detail::Damage {
            amount,
            kinds,
            dice,
            ..
        }) = &f.detail
        else {
            continue;
        };
        frames += 1;
        let who = f.target.expect("somebody was hurt");
        let before = r.frames[i - 1].units[who].hp;
        assert_eq!(f.units[who].hp, (before - amount).max(0), "{}", f.text);
        assert!(
            !kinds.is_empty(),
            "an attack's damage has a type: {}",
            f.text
        );
        assert!(!dice.is_empty(), "and dice: {}", f.text);
        if f.units[who].hp == 0 && before > 0 {
            assert_eq!(r.frames[i + 1].kind, FrameKind::Down);
            assert_eq!(r.frames[i + 1].target, Some(who));
        }
    }
    assert!(frames > 3);
}

#[test]
fn a_forced_save_shows_its_roll_against_the_dc_and_what_it_cost() {
    let (mut passed, mut failed) = (0, 0);
    for seed in 0..40 {
        let r = brawl(seed);
        for (i, f) in r.frames.iter().enumerate() {
            let Some(Detail::Save {
                ability,
                dice,
                kept,
                modifier,
                total,
                dc,
                passed: saved,
                how,
            }) = &f.detail
            else {
                continue;
            };
            assert_eq!((ability.as_str(), *dc, how.as_str()), ("con", 15, "rolled"));
            assert_eq!(dice, &vec![*kept]);
            assert_eq!(*modifier, 3, "the ogre's con save");
            assert_eq!(*saved, total >= dc, "{}", f.text);
            let stunned = matches!(
                &r.frames[i + 1].detail,
                Some(Detail::Condition { name }) if name == "stunned"
            );
            assert_eq!(stunned, !saved, "a failure is what stuns: {}", f.text);
            if *saved {
                passed += 1;
            } else {
                failed += 1;
            }
        }
    }
    assert!(passed > 0 && failed > 0, "{passed} saved, {failed} failed");
}

#[test]
fn a_move_is_announced_as_what_it_is_before_it_resolves() {
    let sword = Move::new(
        "Sword",
        Effect::Strikes {
            strike: Strike::new(100, vec![DamageRoll::new(1, 8, 3, DamageKind::Slashing)]),
            count: 1,
        },
    );
    let mut bolt = Move::new(
        "Fire Bolt",
        Effect::Strikes {
            strike: Strike::new(100, vec![DamageRoll::new(2, 10, 0, DamageKind::Fire)])
                .with_kind(AttackKind::RANGED_SPELL),
            count: 1,
        },
    );
    bolt.kind = MoveKind::Spell;
    let mut potion = Move::new("Potion of Healing", Effect::Heal(HealRoll::new(2, 4, 2)));
    potion.kind = MoveKind::ObjectUse;

    let used = |m: Move, bonus: Option<Move>| {
        let mut hero = Creature::new("hero", 15, 40).with_action(m);
        hero.initiative = 100;
        hero.at = Some((4, 4));
        hero.bonus_actions.extend(bonus);
        let mut dummy = Creature::new("dummy", 10, 500)
            .with_action(Move::new("Wait", Effect::Sequence(vec![])));
        dummy.team = 1;
        dummy.at = Some((6, 4));
        let mut rng = Rng::new(5);
        let r = replay_teams(
            &mut rng,
            &[&hero, &dummy],
            [Policy::Greedy; 2],
            1,
            Budget::default(),
        )
        .1;
        r.frames
            .iter()
            .filter(|f| f.actor == Some(0))
            .filter_map(|f| match &f.detail {
                Some(Detail::Use {
                    name,
                    school,
                    slot,
                    ranged,
                }) => Some((
                    name.clone(),
                    school.clone(),
                    slot.clone(),
                    *ranged,
                    f.text.clone(),
                )),
                _ => None,
            })
            .collect::<Vec<_>>()
    };

    let swung = used(sword.clone(), None);
    assert_eq!(swung.len(), 1);
    assert_eq!((swung[0].1.as_str(), swung[0].3), ("weapon", false));
    assert_eq!(swung[0].4, "hero attacks with Sword");

    let cast = used(bolt, None);
    assert_eq!((cast[0].1.as_str(), cast[0].3), ("spell", true));
    assert_eq!(cast[0].4, "hero casts Fire Bolt");

    // A potion as a bonus action, after the sword has drawn blood on nobody:
    // the hero is unhurt, so nothing is healed, but the potion is still used.
    let both = used(sword, Some(potion));
    let drunk = both.iter().find(|u| u.0 == "Potion of Healing");
    if let Some(drunk) = drunk {
        assert_eq!(
            (drunk.1.as_str(), drunk.2.as_str()),
            ("item", "bonus action")
        );
        assert_eq!(drunk.4, "hero uses Potion of Healing (bonus action)");
    }
}

#[test]
fn healing_is_a_beat_of_its_own_with_the_hit_points_it_gave_back() {
    let mut potion = Move::new("Potion of Healing", Effect::Heal(HealRoll::new(2, 4, 2)));
    potion.kind = MoveKind::ObjectUse;
    let mut hero = Creature::new("hero", 15, 40).with_action(potion);
    hero.initiative = -100;
    hero.at = Some((4, 4));
    let mut ogre = Creature::new("ogre", 10, 500).with_action(Move::new(
        "Club",
        Effect::Strikes {
            strike: Strike::new(
                100,
                vec![DamageRoll::new(0, 6, 12, DamageKind::Bludgeoning)],
            ),
            count: 1,
        },
    ));
    ogre.team = 1;
    ogre.initiative = 100;
    ogre.at = Some((5, 4));
    let mut rng = Rng::new(2);
    let r = replay_teams(
        &mut rng,
        &[&hero, &ogre],
        [Policy::Greedy; 2],
        1,
        Budget::default(),
    )
    .1;
    let (i, heal) = r
        .frames
        .iter()
        .enumerate()
        .find(|(_, f)| f.kind == FrameKind::Heal)
        .expect("the potion heals");
    let Some(Detail::Heal { amount, school }) = &heal.detail else {
        panic!("{heal:?}");
    };
    assert_eq!(school, "item");
    assert!((4..=10).contains(amount), "2d4+2, got {amount}");
    assert_eq!(heal.units[0].hp, r.frames[i - 1].units[0].hp + amount);
    assert_eq!((heal.actor, heal.target), (Some(0), Some(0)));
}

/// A hero that goes first with `m`, against something that only waits.
fn hero_using(m: Move) -> Replay {
    let mut hero = Creature::new("hero", 15, 40).with_action(m);
    hero.initiative = 100;
    hero.at = Some((4, 4));
    let mut dummy =
        Creature::new("dummy", 10, 500).with_action(Move::new("Wait", Effect::Sequence(vec![])));
    dummy.team = 1;
    dummy.at = Some((6, 4));
    let mut rng = Rng::new(9);
    replay_teams(
        &mut rng,
        &[&hero, &dummy],
        [Policy::Greedy; 2],
        1,
        Budget::default(),
    )
    .1
}

fn damage_frames(r: &Replay) -> Vec<(i32, Vec<String>, Vec<crucible_core::sim::Die>)> {
    r.frames
        .iter()
        .filter_map(|f| match &f.detail {
            Some(Detail::Damage {
                amount,
                kinds,
                dice,
                ..
            }) => Some((*amount, kinds.clone(), dice.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn damage_that_needs_no_attack_roll_still_shows_the_dice_it_rolled() {
    // Three darts of 1d4+1 force each: no roll to hit, a roll for damage.
    let darts = Move::new(
        "Darts",
        Effect::AutoHit {
            damage: vec![DamageRoll::new(1, 4, 1, DamageKind::Force); 3],
        },
    );
    let hits = damage_frames(&hero_using(darts));
    assert_eq!(hits.len(), 3, "a beat a dart");
    for (amount, kinds, dice) in hits {
        assert_eq!(kinds, ["force"]);
        assert_eq!(dice.len(), 1);
        assert_eq!((dice[0].sides, dice[0].maxed), (4, false));
        assert_eq!(amount, dice[0].value + 1);
    }
}

#[test]
fn a_pool_taken_at_its_maximum_shows_its_dice_on_their_highest_faces() {
    // 5d6 lightning taken at its maximum is a flat 30 that remembers the
    // five dice it stands for.
    let mut bolt = DamageRoll::new(0, 1, 30, DamageKind::Lightning);
    bolt.maximised = Some((5, 6));
    let storm = Move::new(
        "Storm",
        Effect::Save(crucible_core::creature::SaveEffect {
            ability: crucible_core::rules::Ability::Dex,
            dc: 99,
            damage: vec![bolt, DamageRoll::new(1, 8, 0, DamageKind::Thunder)],
            half_on_success: true,
            on_failure: vec![],
            max_targets: None,
            requires_type: None,
        }),
    );
    let hits = damage_frames(&hero_using(storm));
    assert_eq!(hits.len(), 1);
    let (amount, kinds, dice) = &hits[0];
    assert_eq!(kinds, &["lightning", "thunder"]);
    // The thunder was rolled; the lightning was not, and shows as five sixes.
    let rolled: Vec<_> = dice.iter().filter(|d| !d.maxed).collect();
    let forced: Vec<_> = dice.iter().filter(|d| d.maxed).collect();
    assert_eq!(rolled.len(), 1);
    assert_eq!(rolled[0].sides, 8);
    assert_eq!(forced.len(), 5);
    assert!(forced.iter().all(|d| d.sides == 6 && d.value == 6));
    assert_eq!(*amount, 30 + rolled[0].value);
}

#[test]
fn a_frame_carries_each_creatures_armor_class() {
    let replay = brawl(3);
    let acs: Vec<i32> = replay.units.iter().map(|u| u.ac).collect();
    assert_eq!(acs, [16, 11, 11]);
    // Nothing in this fight raises one, so every frame says the same.
    for frame in &replay.frames {
        let now: Vec<i32> = frame.units.iter().map(|u| u.ac).collect();
        assert_eq!(now, acs, "{}", frame.text);
    }
}

#[test]
fn a_frame_says_what_everyone_is_under_and_what_they_are_concentrating_on() {
    let mut hold = Move::new(
        "Hold",
        Effect::Save(crucible_core::creature::SaveEffect {
            ability: crucible_core::rules::Ability::Wis,
            dc: 99,
            damage: vec![],
            half_on_success: false,
            on_failure: vec![(
                crucible_core::rules::Condition::Paralyzed,
                crucible_core::rules::Duration::ApplierTurn,
            )],
            max_targets: Some(1),
            requires_type: None,
        }),
    );
    hold.concentration = true;
    let r = hero_using(hold);

    let cast = r
        .frames
        .iter()
        .position(|f| f.kind == FrameKind::Use)
        .expect("the spell is cast");
    assert_eq!(r.frames[cast - 1].units[0].concentrating, None);
    let landed = r
        .frames
        .iter()
        .position(|f| f.kind == FrameKind::Condition)
        .expect("and lands");
    assert!(landed > cast);
    // The caster's own frame entry does not name the spell until it has
    // something to hold: concentration starts once the condition lands.
    let end = r.frames.last().unwrap();
    assert_eq!(end.units[0].concentrating.as_deref(), Some("Hold"));
    assert_eq!(end.units[1].conditions, ["paralyzed"]);
    assert_eq!(end.units[1].concentrating, None);
}
