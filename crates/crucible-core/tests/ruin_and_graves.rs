//! The rest of what a killer of minds can do, one mechanism at a time:
//! damage thrown back at whoever dealt it, exhaustion in levels, creatures
//! shoved, routed and gone somewhere else on a grid, minds muddled and
//! sapped, a prison with a price, the dead raised, the unseen, a store of
//! spell levels, a mark only one can bear, death saving throws, and a move
//! its author says is worth taking.
//!
//! Every creature here is a stand-in with a neutral name, built to show one
//! thing and nothing else: a table's own monsters live in its own local
//! files (see the README).

use crucible_core::creature::Creature;
use crucible_core::dsl::config::load_creature_from_str;
use crucible_core::features::FeatureRegistry;
use crucible_core::prob::Rng;
use crucible_core::sim::arena::gap;
use crucible_core::sim::{replay_teams, run_teams, Budget, Outcome, Policy, Replay};

fn load(text: &str) -> Creature {
    load_creature_from_str(text, &FeatureRegistry::new()).expect("it compiles")
}

/// A player character with one action.
fn hero(name: &str, hp: i32, action: &str) -> Creature {
    load(&format!(
        "[pc]\nname = \"{name}\"\nac = 10\nhp = {hp}\ninitiative = 10\n\n\
         [[pc.actions]]\nname = \"Act\"\neffect = \"{action}\"\n"
    ))
}

/// A monster on the other side, written out in full.
fn foe(text: &str) -> Creature {
    let mut c = load(text);
    c.team = 1;
    c
}

fn fought(roster: &[&Creature], fights: u32, rounds: u32, seed: u64) -> (Vec<Outcome>, String) {
    let mut rng = Rng::new(seed);
    let (mut outcomes, mut narration) = (Vec::new(), String::new());
    for _ in 0..fights {
        let mut log = Some(Vec::new());
        outcomes.push(run_teams(
            &mut rng,
            roster,
            [Policy::Greedy; 2],
            rounds,
            Budget::default(),
            &mut log,
        ));
        narration.push_str(&log.unwrap().join("\n"));
        narration.push('\n');
    }
    (outcomes, narration)
}

fn mean(outcomes: &[Outcome], of: impl Fn(&Outcome) -> f64) -> f64 {
    outcomes.iter().map(of).sum::<f64>() / outcomes.len() as f64
}

fn recorded(roster: &[&Creature], rounds: u32) -> Replay {
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

/// How far `seat` is, in the frame that says `text`, from where it began.
fn moved(replay: &Replay, seat: usize, text: &str) -> (i32, i32) {
    let frame = replay
        .frames
        .iter()
        .find(|f| f.text == text)
        .unwrap_or_else(|| panic!("no frame says `{text}`: {:?}", replay.lines));
    let (was, is) = (&replay.frames[0].units[seat], &frame.units[seat]);
    (is.x - was.x, is.y - was.y)
}

const DUMMY: &str = "[monster]\nname = \"Test Dummy\"\nac = 10\nhp = 1000\ninitiative = -10\n";

// --- damage thrown back ---------------------------------------------------------

#[test]
fn damage_of_the_type_it_answers_is_thrown_back_and_no_other() {
    let mirror = foe(&format!("{DUMMY}traits = [\"reflect psychic\"]\n"));
    let dealt_and_taken = |action: &str, resist: &str| {
        let mut thinker = hero("Test Thinker", 300, action);
        if !resist.is_empty() {
            thinker = load(&format!(
                "[pc]\nname = \"Test Thinker\"\nac = 10\nhp = 300\nresist = [\"{resist}\"]\n\n\
                 [[pc.actions]]\nname = \"Act\"\neffect = \"{action}\"\n"
            ));
        }
        let (outcomes, _) = fought(&[&thinker, &mirror], 1, 3, 1);
        (1000 - outcomes[0].hp_left[1], 300 - outcomes[0].hp_left[0])
    };
    // Ten psychic a round for three rounds: as much comes back.
    assert_eq!(dealt_and_taken("autohit | 1d1+9 psychic", ""), (30, 30));
    // Half as much to something that resists it, and none of a blade's.
    assert_eq!(
        dealt_and_taken("autohit | 1d1+9 psychic", "psychic"),
        (30, 15)
    );
    assert_eq!(dealt_and_taken("autohit | 1d1+9 slashing", ""), (30, 0));
    // Of a blow that is half one and half the other, the half that counts.
    assert_eq!(
        dealt_and_taken("save dex dc 40 | 1d1+9 slashing | 1d1+5 psychic", ""),
        (48, 18)
    );
}

// --- exhaustion, in levels ---------------------------------------------------------

#[test]
fn exhaustion_is_counted_and_the_sixth_level_kills() {
    let drainer = foe(
        "[monster]\nname = \"Test Drainer\"\nac = 10\nhp = 1000\ninitiative = 20\n\n\
         [[monster.actions]]\nname = \"Drain\"\neffect = \"targets 1 | save con dc 40 | on fail exhaustion\"\n",
    );
    let victim = hero("Test Victim", 500, "strikes 1 | hit +9 | 1d1+9 slashing");
    // Five levels are not yet fatal; the sixth is, whatever it has left.
    let (five, _) = fought(&[&victim, &drainer], 1, 5, 1);
    assert_eq!(five[0].survivors[0], 1);
    let (six, narration) = fought(&[&victim, &drainer], 1, 6, 1);
    assert_eq!(six[0].survivors[0], 0, "{narration}");
    assert_eq!(narration.matches("and exhaustion").count(), 6);

    // And each level before it is two off every attack roll: at +9 against
    // AC 10 it starts by missing only on a 1, and ends needing a 9.
    let fresh = foe(DUMMY);
    let (tired, _) = fought(&[&victim, &drainer], 400, 5, 3);
    let (rested, _) = fought(&[&victim, &fresh], 400, 5, 3);
    let dealt = |o: &[Outcome]| mean(o, |o| o.damage_dealt[0] as f64);
    assert!(
        dealt(&tired) < dealt(&rested) * 0.9,
        "{} vs {}",
        dealt(&tired),
        dealt(&rested)
    );
}

// --- somewhere else on the grid ------------------------------------------------------

/// Stands still and looses something at whoever is across from it.
fn archer(name: &str, x: i32, action: &str) -> Creature {
    let mut c = hero(name, 300, action);
    c.at = Some((x, 5));
    c
}

#[test]
fn a_shove_moves_what_it_hits_ten_feet_away() {
    let target = archer(
        "Test Target",
        8,
        "ranged | range 120 | strikes 1 | hit -30 | 1d1 force",
    );
    let mut pusher = foe(
        "[monster]\nname = \"Test Pusher\"\nac = 10\nhp = 300\ninitiative = 30\nspeed = 0\n\n\
         [[monster.actions]]\nname = \"Gust\"\neffect = \"ranged | range 120 | strikes 1 | hit +50 | 1d1 force | on hit pushed\"\n",
    );
    pusher.at = Some((12, 5));
    let replay = recorded(&[&target, &pusher], 1);
    assert_eq!(
        moved(&replay, 0, "Test Target is pushed 10 ft"),
        (-2, 0),
        "two squares further from the gust, straight back"
    );
}

#[test]
fn a_rout_costs_its_victim_its_reaction_and_its_ground() {
    let lash = "\n[[pc.reactions]]\nname = \"Lash\"\neffect = \"when damaged | ranged | range 120 | strikes 1 | hit +50 | 1d1 force\"\n";
    let runner = |x: i32| {
        let mut c = load(&format!(
            "[pc]\nname = \"Test Runner\"\nac = 10\nhp = 300\nspeed = 30\n\n\
             [[pc.actions]]\nname = \"Act\"\neffect = \"ranged | range 120 | strikes 1 | hit -30 | 1d1 force\"\n{lash}"
        ));
        c.at = Some((x, 5));
        c
    };
    let howler = |effect: &str| {
        let mut c = foe(&format!(
            "[monster]\nname = \"Test Howler\"\nac = 10\nhp = 300\ninitiative = 30\nspeed = 0\n\n\
             [[monster.actions]]\nname = \"Howl\"\neffect = \"range 120 | targets 1 | save wis dc 40 | 1d1 psychic{effect}\"\n"
        ));
        c.at = Some((14, 5));
        c
    };
    // Hurt and nothing more, it lashes back from where it stands.
    let stood = recorded(&[&runner(12), &howler("")], 1);
    assert!(stood.lines.iter().any(|l| l.contains("reacts: Lash")));
    assert!(!stood.frames.iter().any(|f| f.text.contains("flees")));
    // Routed, the reaction is what it runs on: thirty feet, and no lash.
    let ran = recorded(&[&runner(12), &howler(" | on fail fleeing")], 1);
    assert_eq!(moved(&ran, 0, "Test Runner flees 30 ft"), (-6, 0));
    assert!(!ran.lines.iter().any(|l| l.contains("reacts: Lash")));
}

#[test]
fn a_creature_with_somebody_on_top_of_it_steps_out_of_reach() {
    // Slow enough that thirty feet is further than it can walk back.
    let mut brute = hero("Test Brute", 300, "strikes 1 | hit +5 | 2d10+10 slashing");
    brute.speed = 20;
    brute.at = Some((9, 5));
    // One that can simply follow is not worth leaving.
    let mut quick = brute.clone();
    quick.speed = 30;
    let stepper = |legendary: &str| {
        let mut c = foe(&format!(
            "[monster]\nname = \"Test Stepper\"\nac = 10\nhp = 300\ninitiative = -10\nlegendary_uses = 1\n\n\
             [[monster.actions]]\nname = \"Dart\"\neffect = \"ranged | range 120 | strikes 1 | hit +5 | 1d4 piercing\"\n\n\
             [[monster.legendary]]\nname = \"Step\"\neffect = \"{legendary}\"\n"
        ));
        c.at = Some((10, 5));
        c
    };
    let stayed = recorded(&[&quick, &stepper("teleport 30")], 1);
    assert!(
        !stayed.lines.iter().any(|l| l.contains("Step (")),
        "{:?}",
        stayed.lines
    );
    let replay = recorded(&[&brute, &stepper("teleport 30")], 1);
    assert!(
        replay.lines.iter().any(|l| l.contains("Step (teleports)")),
        "{:?}",
        replay.lines
    );
    // Six squares off by the end of the brute's turn - and it has to be
    // walked after.
    let after_step = replay
        .frames
        .iter()
        .find(|f| f.text == "Test Stepper is somewhere else")
        .expect("it steps");
    let (b, s) = (&after_step.units[0], &after_step.units[1]);
    assert!(
        gap((b.x, b.y), 1, (s.x, s.y), 1) >= 5,
        "as far as the map goes"
    );
}

// --- minds muddled and sapped -----------------------------------------------------

#[test]
fn a_muddled_mind_misses_more() {
    let swinger = hero("Test Swinger", 300, "strikes 1 | hit +5 | 1d1+9 slashing");
    let fogger = foe(&format!(
        "{DUMMY}\n[[monster.actions]]\nname = \"Fog\"\neffect = \"targets 1 | save int dc 40 | on fail muddled for 1 minute\"\n"
    ));
    let plain = foe(DUMMY);
    let dealt = |target: &Creature| {
        mean(&fought(&[&swinger, target], 600, 6, 5).0, |o| {
            o.damage_dealt[0] as f64
        })
    };
    // 1d6 off a roll that needed a 5: about a fifth fewer land, after the
    // first swing.
    let (fogged, clear) = (dealt(&fogger), dealt(&plain));
    assert!(
        fogged < clear * 0.88 && fogged > clear * 0.7,
        "{fogged} vs {clear}"
    );
}

#[test]
fn a_sapped_mind_fails_its_next_save_more_often_and_only_that_one() {
    let sapper = |sliver: bool| {
        let bonus = match sliver {
            true => "\n[[monster.bonus]]\nname = \"Sliver\"\neffect = \"targets 1 | save int dc 40 | on fail sapped until end\"\n",
            false => "",
        };
        foe(&format!(
            "{DUMMY}\n[[monster.actions]]\nname = \"Flame\"\neffect = \"targets 1 | save dex dc 11 | 1d1+19 fire\"\n{bonus}"
        ))
    };
    let victim = hero("Test Victim", 100_000, "strikes 1 | hit -30 | 1d1 slashing");
    let taken = |sliver: bool| {
        mean(&fought(&[&victim, &sapper(sliver)], 600, 6, 9).0, |o| {
            f64::from(100_000 - o.hp_left[0])
        })
    };
    // A 50% save, 1d4 worse on all but the first: about an eighth more fire.
    let (sapped, plain) = (taken(true), taken(false));
    assert!(
        sapped > plain * 1.12 && sapped < plain * 1.35,
        "{sapped} vs {plain}"
    );
}

// --- a prison with a price ---------------------------------------------------------

#[test]
fn what_is_held_breaks_out_when_it_can_afford_to() {
    let holder = |price: &str| {
        foe(&format!(
            "[monster]\nname = \"Test Holder\"\nac = 10\nhp = 1000\ninitiative = 30\n\n\
             [[monster.actions]]\nname = \"Cage\"\neffect = \"concentration | targets 1 | save int dc 40 | on fail restrained for 1 minute | escape {price} psychic\"\n"
        ))
    };
    let held = hero("Test Held", 300, "strikes 1 | hit +5 | 1d1 slashing");
    // Twenty for its freedom, out of three hundred: paid, every time it is
    // caged, and the cage is gone with the concentration that held it.
    let (_, cheap) = fought(&[&held, &holder("20d1")], 1, 3, 1);
    assert_eq!(
        cheap
            .matches("Test Held breaks out of Test Holder's hold, for 20")
            .count(),
        3
    );
    // Two hundred would leave it under half: it stays where it is.
    let (_, dear) = fought(&[&held, &holder("200d1")], 1, 3, 1);
    assert!(!dear.contains("breaks out"), "{dear}");
}

// --- the dead, raised ---------------------------------------------------------------

#[test]
fn what_it_kills_outright_gets_up_and_fights_for_it() {
    let reaper = |damage: &str| {
        foe(&format!(
            "[monster]\nname = \"Test Reaper\"\nac = 10\nhp = 1000\ninitiative = 30\n\n\
             [[monster.minions]]\nname = \"Test Husk\"\nac = 8\nhp = 15\n\n\
             [[monster.minions.actions]]\nname = \"Slam\"\neffect = \"strikes 1 | hit +50 | 1d1+2 bludgeoning\"\n\n\
             [[monster.actions]]\nname = \"Reap\"\neffect = \"uses 1 | targets 1 | save con dc 40 | {damage} necrotic | on kill raise Test Husk\"\n\n\
             [[monster.actions]]\nname = \"Wait\"\neffect = \"strikes 1 | hit -30 | 1d1 force\"\n"
        ))
    };
    // Somebody who is not a player character dies at 0, and rises.
    let guard = load("[monster]\nname = \"Test Guard\"\ncreature_type = \"Humanoid\"\nac = 10\nhp = 50\n\n[[monster.actions]]\nname = \"Act\"\neffect = \"strikes 1 | hit -30 | 1d1 slashing\"\n");
    let friend = hero("Test Friend", 300, "strikes 1 | hit -30 | 1d1 slashing");
    let (outcomes, raised) = fought(&[&guard, &friend, &reaper("60d1")], 1, 3, 1);
    assert!(
        raised.contains("r2 Test Guard rises as Test Reaper's Test Husk"),
        "{raised}"
    );
    // It has turns of its own from then on, and what it does lands.
    assert_eq!(raised.matches("Test Husk: ").count(), 2, "{raised}");
    assert_eq!(outcomes[0].hp_left[0], 300 - 6);

    // A player character that has only dropped is not dead, and does not;
    // one killed outright is, and does.
    let (_, dropped) = fought(
        &[
            &hero("Test Fallen", 50, "strikes 1 | hit -30 | 1d1 slashing"),
            &friend,
            &reaper("60d1"),
        ],
        1,
        3,
        1,
    );
    assert!(!dropped.contains("rises as"), "{dropped}");
    let (_, slain) = fought(
        &[
            &hero("Test Fallen", 50, "strikes 1 | hit -30 | 1d1 slashing"),
            &friend,
            &reaper("100d1"),
        ],
        1,
        3,
        1,
    );
    assert!(slain.contains("Test Fallen rises as"), "{slain}");
    // And not something that was never a humanoid.
    let beast = load("[monster]\nname = \"Test Beast\"\ncreature_type = \"Beast\"\nac = 10\nhp = 50\n\n[[monster.actions]]\nname = \"Act\"\neffect = \"strikes 1 | hit -30 | 1d1 slashing\"\n");
    let (_, eaten) = fought(&[&beast, &friend, &reaper("60d1")], 1, 3, 1);
    assert!(!eaten.contains("rises as"), "{eaten}");
}

// --- the unseen ------------------------------------------------------------------------

#[test]
fn the_unseen_are_harder_to_hit_unless_they_are_seen_anyway() {
    let shade = foe(&format!(
        "{DUMMY}\n[[monster.bonus]]\nname = \"Vanish\"\neffect = \"stance invisible\"\n"
    ));
    let plain = foe(DUMMY);
    let seer = |traits: &str| {
        load(&format!(
            "[pc]\nname = \"Test Seer\"\nac = 10\nhp = 300\ninitiative = -20\ntraits = [{traits}]\n\n\
             [[pc.actions]]\nname = \"Act\"\neffect = \"strikes 1 | hit +0 | 1d1+9 slashing\"\n"
        ))
    };
    let dealt = |who: &Creature, target: &Creature| {
        mean(&fought(&[who, target], 600, 6, 21).0, |o| {
            o.damage_dealt[0] as f64
        })
    };
    let blind = seer("");
    // A roll that hit 55% of the time, at disadvantage: about 30%.
    let (hidden, open) = (dealt(&blind, &shade), dealt(&blind, &plain));
    assert!(
        hidden < open * 0.65 && hidden > open * 0.45,
        "{hidden} vs {open}"
    );
    let sighted = seer("\"truesight 60\"");
    let (hidden, open) = (dealt(&sighted, &shade), dealt(&sighted, &plain));
    assert!((hidden - open).abs() < open * 0.06, "{hidden} vs {open}");
}

#[test]
fn striking_from_hiding_has_advantage_and_gives_the_hiding_up() {
    // It vanishes after its blow each turn, so every blow is struck unseen -
    // and every one of them ends it, so the answer is aimed at something
    // seen.
    let lurker = |vanish: bool| {
        let bonus = match vanish {
            true => "\n[[monster.bonus]]\nname = \"Vanish\"\neffect = \"stance invisible\"\n",
            false => "",
        };
        foe(&format!(
            "[monster]\nname = \"Test Lurker\"\nac = 10\nhp = 100000\ninitiative = 30\n\n\
             [[monster.actions]]\nname = \"Knife\"\neffect = \"strikes 1 | hit +0 | 1d1+9 piercing\"\n{bonus}"
        ))
    };
    let mark = hero("Test Mark", 100_000, "strikes 1 | hit +0 | 1d1+9 slashing");
    let (hidden, _) = fought(&[&mark, &lurker(true)], 600, 6, 23);
    let (open, _) = fought(&[&mark, &lurker(false)], 600, 6, 23);
    let lurker_dealt = |o: &[Outcome]| mean(o, |o| o.damage_dealt[1] as f64);
    // 55% becomes about 80% on every blow after the first.
    assert!(lurker_dealt(&hidden) > lurker_dealt(&open) * 1.25);
    // The mark swings back after the knife, at something it can now see...
    // and then it vanishes again before the mark's next turn: so the mark
    // swings at the unseen. Hidden when it matters, to both.
    let mark_dealt = |o: &[Outcome]| mean(o, |o| o.damage_dealt[0] as f64);
    assert!(mark_dealt(&hidden) < mark_dealt(&open) * 0.7);
}

// --- a store of spell levels -----------------------------------------------------------

const KEEPER: &str = r#"
[pc]
name = "Test Keeper"
ac = 10
hp = 300
initiative = 30
traits = ["spell store rod up to 5 holds 7"]

[pc.resources]
rod = 4

[[pc.actions]]
name = "Bolt"
effect = "spell | slot 2 | range 120 | targets 1 | save dex dc 40 | 1d1+9 force"

[[pc.reactions]]
name = "Drink"
effect = "when targeted by a spell | item | counter"
"#;

#[test]
fn stored_levels_pay_for_slots_and_what_is_stopped_is_stored() {
    let keeper = load(KEEPER);
    // Four levels and no slots of its own: two bolts, and then nothing.
    let (alone, _) = fought(&[&keeper, &foe(DUMMY)], 1, 6, 1);
    assert_eq!(alone[0].damage_dealt[0], 20);

    // Against something that throws a third-level spell at it every round,
    // each one drunk is three more levels - never more than it holds.
    let thrower = foe(&format!(
        "{DUMMY}\n[monster.resources.slots]\n3 = 9\n\n[[monster.actions]]\nname = \"Ray\"\neffect = \"spell | slot 3 | targets 1 | save dex dc 40 | 1d1+9 fire\"\n"
    ));
    let (fed, narration) = fought(&[&keeper, &thrower], 1, 6, 1);
    assert_eq!(fed[0].hp_left[0], 300, "{narration}");
    // 4, cast (2), drink (5), cast (3), drink (6), cast (4), drink (7)...
    assert_eq!(
        fed[0].damage_dealt[0], 60,
        "a bolt every round: {narration}"
    );
}

// --- a mark only one can bear ------------------------------------------------------------

#[test]
fn a_mark_borne_by_one_at_a_time_moves_with_whoever_it_is_put_on() {
    let marker = |clause: &str| {
        foe(&format!(
            "[monster]\nname = \"Test Marker\"\nac = 10\nhp = 1000\ninitiative = 30\n\n\
             [[monster.actions]]\nname = \"Fell\"\neffect = \"targets 1 | save con dc 40 | 60d1 necrotic\"\n\n\
             [[monster.bonus]]\nname = \"Mark\"\neffect = \"targets 1{clause} | save wis dc 40 | on fail quarry for 1 minute\"\n"
        ))
    };
    let first = hero("Test First", 100, "strikes 1 | hit -30 | 1d1 slashing");
    let second = hero("Test Second", 300, "strikes 1 | hit -30 | 1d1 slashing");
    let most_marked = |m: &Creature| {
        let replay = recorded(&[&first, &second, m], 2);
        assert!(
            replay
                .frames
                .iter()
                .any(|f| f.units[1].conditions.iter().any(|c| c == "quarry")),
            "the second is marked once the first has fallen"
        );
        replay
            .frames
            .iter()
            .map(|f| {
                f.units[..2]
                    .iter()
                    .filter(|u| u.conditions.iter().any(|c| c == "quarry"))
                    .count()
            })
            .max()
            .unwrap()
    };
    assert_eq!(most_marked(&marker("")), 2);
    assert_eq!(most_marked(&marker(" | one at a time")), 1);
}

// --- hurt by anything ----------------------------------------------------------------------

#[test]
fn a_reaction_to_being_hurt_answers_a_spell_as_well_as_a_blow() {
    let rebuker = foe(&format!(
        "{DUMMY}\n[[monster.reactions]]\nname = \"Rebuke\"\neffect = \"when damaged | targets 1 | save dex dc 40 | 1d1+4 fire\"\n"
    ));
    for action in ["save dex dc 40 | 1d1+9 cold", "autohit | 1d1+9 force"] {
        let (outcomes, narration) = fought(&[&hero("Test Hurter", 300, action), &rebuker], 1, 3, 1);
        assert_eq!(
            narration.matches("reacts: Rebuke").count(),
            3,
            "{action}: {narration}"
        );
        assert_eq!(outcomes[0].hp_left[0], 300 - 15);
    }
    // A blow that lands is the same; one that misses hurt nothing.
    let (_, struck) = fought(
        &[
            &hero("Test Hurter", 300, "strikes 1 | hit +50 | 1d1+9 slashing"),
            &rebuker,
        ],
        1,
        3,
        1,
    );
    assert!(struck.contains("reacts: Rebuke"), "{struck}");
    let (_, wild) = fought(
        &[
            &hero("Test Hurter", 300, "strikes 1 | hit -30 | 1d1+9 slashing"),
            &rebuker,
        ],
        40,
        3,
        1,
    );
    let landed = wild
        .lines()
        .filter(|l| l.contains("Act (") && !l.contains("Act (miss"))
        .count();
    assert!(landed > 0 && landed < 20, "only a 20 lands: {landed}");
    assert_eq!(wild.matches("reacts: Rebuke").count(), landed);
}

// --- rolling for its life ------------------------------------------------------------------

#[test]
fn a_character_that_is_down_rolls_for_its_life_and_a_doomed_one_rolls_worse() {
    let killer = |doom: &str| {
        foe(&format!(
            "[monster]\nname = \"Test Killer\"\nac = 10\nhp = 100000\ninitiative = 30\n\n\
             [[monster.actions]]\nname = \"Fell\"\neffect = \"targets 1 | save con dc 40 | 30d1 necrotic{doom}\"\n"
        ))
    };
    // Low in the roster, so it is what the killer goes for first; the other
    // cannot be killed in the time there is, so the fight goes on over it.
    let frail = hero("Test Frail", 20, "strikes 1 | hit -30 | 1d1 slashing");
    let stout = hero("Test Stout", 100_000, "strikes 1 | hit -30 | 1d1 slashing");
    let fates = |doom: &str| {
        let (_, narration) = fought(&[&frail, &stout, &killer(doom)], 1500, 12, 31);
        (
            narration.matches("Test Frail").filter(|_| true).count(),
            narration.matches("and dies").count(),
            narration.matches("and is stable").count(),
            narration.matches("is back up on a 20").count(),
        )
    };
    let (_, dies, stable, up) = fates("");
    // Three failures before three successes at 55% a success, with a 1
    // counting twice and a 20 ending it: about four in ten die.
    assert!((480..=720).contains(&dies), "{dies} die");
    assert!(stable > dies, "{stable} stable");
    assert!(up > 100, "{up} get up");
    // Doomed, each roll is the worse of two.
    let (_, doomed, _, _) = fates(" | on fail doomed for 1 hour");
    assert!(doomed > dies + 300, "{doomed} doomed against {dies}");
}

// --- one blow out of hiding, and a curse ---------------------------------------------------

#[test]
fn the_first_hit_of_the_fight_carries_more_and_no_other_does() {
    let stalker = load(
        "[pc]\nname = \"Test Stalker\"\nac = 10\nhp = 300\ntraits = [\"first hit 100d1 necrotic\"]\n\n\
         [[pc.actions]]\nname = \"Act\"\neffect = \"strikes 2 | hit +50 | 1d1 slashing\"\n",
    );
    // Ten swings at one each, give or take a miss on a 1 and a die doubled
    // on a 20 - and a hundred more, once: two hundred when that first hit
    // is itself a crit, and never a third.
    let (outcomes, narration) = fought(&[&stalker, &foe(DUMMY)], 1, 5, 1);
    let dealt = outcomes[0].damage_dealt[0];
    assert!(matches!(dealt / 100, 1 | 2), "{dealt}: {narration}");
    assert!((5..=20).contains(&(dealt % 100)), "{dealt}: {narration}");
}

#[test]
fn what_refuses_healing_is_healed_by_nobody_else() {
    let burner = hero("Test Burner", 100_000, "save dex dc 40 | 1d1+19 fire");
    let medic = foe(
        "[monster]\nname = \"Test Medic\"\nac = 10\nhp = 100000\ninitiative = -10\n\n\
         [[monster.bonus]]\nname = \"Mend\"\neffect = \"range 120 | heal 1d1+9\"\n",
    );
    let cursed = |traits: &str| {
        foe(&format!(
            "[monster]\nname = \"Test Cursed\"\nac = 10\nhp = 500\ninitiative = -10\ntraits = [{traits}]\n\n\
             [[monster.actions]]\nname = \"Wait\"\neffect = \"strikes 1 | hit -30 | 1d1 force\"\n"
        ))
    };
    let (_, helped) = fought(&[&burner, &cursed(""), &medic], 1, 4, 1);
    assert!(helped.contains("heals Test Cursed"), "{helped}");
    let (_, refused) = fought(&[&burner, &cursed("\"refuses healing\""), &medic], 1, 4, 1);
    assert!(!refused.contains("heals Test Cursed"), "{refused}");
}

// --- what a move is worth -------------------------------------------------------------------

#[test]
fn a_move_worth_taking_is_taken_until_it_would_change_nothing() {
    let hexer = |worth: &str| {
        foe(&format!(
            "[monster]\nname = \"Test Hexer\"\nac = 10\nhp = 1000\ninitiative = 30\n\n\
             [[monster.actions]]\nname = \"Hit\"\neffect = \"strikes 1 | hit +50 | 1d1+9 slashing\"\n\n\
             [[monster.actions]]\nname = \"Hex\"\neffect = \"targets 1{worth} | save wis dc 40 | on fail frightened for 1 minute\"\n"
        ))
    };
    let victim = hero("Test Victim", 300, "strikes 1 | hit -30 | 1d1 slashing");
    // Ranked on damage alone, a hex is never worth a turn.
    let (_, plain) = fought(&[&victim, &hexer("")], 1, 4, 1);
    assert!(!plain.contains("Hex ("), "{plain}");
    // Told it is worth fifty, it goes first - once: frightened already,
    // the victim has nothing more to fear from it.
    let (outcomes, weighed) = fought(&[&victim, &hexer(" | worth 50")], 1, 4, 1);
    assert!(weighed.starts_with("initiative"), "{weighed}");
    assert_eq!(weighed.matches("Hex (").count(), 1, "{weighed}");
    assert!(weighed.contains("r1 Test Hexer: Hex ("), "{weighed}");
    assert_eq!(weighed.matches("Test Hexer: Hit (").count(), 3, "{weighed}");
    assert!(outcomes[0].hp_left[0] < 300);
}
