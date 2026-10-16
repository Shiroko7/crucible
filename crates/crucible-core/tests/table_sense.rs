//! What anybody at a table does without thinking, which a fight therefore
//! has to do too: walk over to whoever is down before laying hands on them,
//! heal with the bonus action and keep the action, run at an enemy that is
//! out of reach rather than cast something idle at it, stand still to take
//! aim, and not let go of one spell to cast its twin.
//!
//! The creatures here are stand-ins with neutral names (see the README).

use crucible_core::creature::Creature;
use crucible_core::dsl::config::load_creature_from_str;
use crucible_core::dsl::scenario::parse;
use crucible_core::features::FeatureRegistry;
use crucible_core::prob::Rng;
use crucible_core::sim::arena::gap;
use crucible_core::sim::{replay_teams, Budget, FrameKind, Policy, Replay};

fn replayed(creatures: &[Creature], rounds: u32) -> Replay {
    let roster: Vec<&Creature> = creatures.iter().collect();
    let mut rng = Rng::new(11);
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
    replayed(&parse(text).expect("the scenario loads"), rounds)
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
}

/// A player character, so that it drops rather than dies, standing at `at`.
fn character(name: &str, hp: i32, initiative: i32, at: (i32, i32), rest: &str) -> Creature {
    let text = format!(
        "[pc]\nname = \"{name}\"\nclass = \"Cleric\"\nlevel = 5\nac = 10\nhp = {hp}\n\
         initiative = {initiative}\n{rest}"
    );
    let mut c = load_creature_from_str(&text, &FeatureRegistry::new()).expect("it loads");
    c.at = Some(at);
    c
}

/// An archer that drops the fragile one first, and then has nothing left to
/// shoot it with.
fn archer(at: (i32, i32)) -> Creature {
    let mut c = parse(&format!(
        "creature: archer\nteam: b\nac: 5\nhp: 200\ninitiative: 90\nspeed: 0\n\
         at: {}, {}\n\
         action: Volley | uses 1 | ranged | range 300 | hit +100 | 0d6+12 piercing\n",
        at.0, at.1
    ))
    .unwrap()
    .remove(0);
    c.team = 1;
    c
}

fn fragile(at: (i32, i32)) -> Creature {
    character("fragile", 10, 80, at, "")
}

#[test]
fn a_touch_has_to_be_walked_over_to() {
    let healer = |speed: u32| {
        let mut c = character(
            "healer",
            50,
            50,
            (4, 5),
            "\n[[pc.actions]]\nname = \"Mend\"\neffect = \"heal 0d6+5\"\n",
        );
        c.speed = speed;
        c
    };
    // Thirty feet from the one who drops: a walk, and then a hand on them.
    let r = replayed(&[fragile((10, 5)), healer(30), archer((30, 5))], 1);
    assert_eq!(hp(&r, 0), 5, "{:?}", r.lines);
    assert_eq!(gap(at(&r, 0), 1, at(&r, 1), 1), 1, "it went to them");
    // Rooted to the spot, it cannot reach, and nobody is healed from afar.
    let r = replayed(&[fragile((10, 5)), healer(0), archer((30, 5))], 1);
    assert_eq!(hp(&r, 0), 0, "{:?}", r.lines);
}

#[test]
fn a_heal_goes_on_the_bonus_action_and_the_action_is_kept() {
    let healer = character(
        "healer",
        50,
        50,
        (4, 5),
        "\n[[pc.actions]]\nname = \"Mend\"\neffect = \"range 60 | heal 0d6+9\"\n\
         \n[[pc.actions]]\nname = \"Bolt\"\neffect = \"ranged | range 300 | hit +100 | 0d6+10 fire\"\n\
         \n[[pc.bonus]]\nname = \"Word\"\neffect = \"range 60 | heal 0d6+3\"\n",
    );
    let r = replayed(&[fragile((10, 5)), healer, archer((30, 5))], 1);
    assert_eq!(hp(&r, 0), 3, "the word, not the mending: {:?}", r.lines);
    assert_eq!(hp(&r, 2), 190, "and the action still spent on the enemy");
}

#[test]
fn a_weapon_swung_as_part_of_a_spell_is_not_a_spell_aimed_at_its_target() {
    let against = |attack: &str| {
        let text = format!(
            "creature: caster\nteam: a\nac: 10\nhp: 50\ninitiative: 50\nspeed: 0\nat: 4, 5\n\
             action: Shot | {attack}\n\n\
             creature: drinker\nteam: b\nac: 5\nhp: 200\ninitiative: -50\nspeed: 0\nat: 8, 5\n\
             reaction: Drink | when targeted by a spell | item | counter\n"
        );
        200 - hp(&fought(&text, 1), 1)
    };
    assert_eq!(
        against("spell | ranged | hit +100 | 0d6+10 fire"),
        0,
        "a bolt of fire aimed at it is drunk"
    );
    assert_eq!(
        against("spell | weapon | ranged | hit +100 | 0d6+10 piercing"),
        10,
        "an arrow loosed by a spell cast on the archer is not"
    );
}

#[test]
fn it_does_not_drop_one_emanation_to_raise_its_twin() {
    let mut bearer = load_creature_from_str(
        r#"
[monster]
name = "Test Bearer"
ac = 10
hp = 100
initiative = 50
speed = 0

[[monster.features]]
plugin = "lasting_aura"
name = "Test Ring"
rounds = 100
concentration = true
effect = "save con dc 99 | 0d6+4 radiant"

[[monster.features]]
plugin = "lasting_aura"
name = "Test Greater Ring"
rounds = 100
concentration = true
effect = "save con dc 99 | 0d6+6 radiant"

[[monster.actions]]
name = "Test Spark"
effect = "ranged | range 120 | hit +100 | 0d6+1 force"
"#,
        &FeatureRegistry::new(),
    )
    .expect("it loads");
    bearer.at = Some((4, 5));
    let mut foe =
        parse("creature: foe\nteam: b\nac: 5\nhp: 200\ninitiative: -50\nspeed: 0\nat: 5, 5\n")
            .unwrap()
            .remove(0);
    foe.team = 1;
    let r = replayed(&[bearer, foe], 4);
    let raised = r
        .lines
        .iter()
        .filter(|l| l.contains("Test Bearer:") && l.contains("Ring"))
        .count();
    assert_eq!(raised, 1, "{:?}", r.lines);
    assert!(said(&r, "Test Spark"), "and then it gets on with the fight");
}

#[test]
fn out_of_reach_it_runs_rather_than_casting_something_idle() {
    let text = "creature: brawler\nteam: a\nac: 10\nhp: 50\ninitiative: 50\nat: 2, 5\n\
                action: Glitter | spell | range 300 | targets 1 | save dex dc 10 \
                | on fail outlined for 1 minute\n\
                action: Club | hit +100 | 0d6+10 bludgeoning\n\n\
                creature: foe\nteam: b\nac: 5\nhp: 200\ninitiative: -50\nspeed: 0\nat: 20, 5\n";
    let r = fought(text, 1);
    assert!(said(&r, "dashes 60 ft"), "{:?}", r.lines);
    assert!(!said(&r, "Glitter"));
}

#[test]
fn taking_aim_is_standing_still() {
    let marksman = |foe_at: i32| {
        let mut c = character(
            "marksman",
            50,
            50,
            (4, 5),
            "\n[[pc.features]]\nplugin = \"steady_aim\"\n\
             \n[[pc.actions]]\nname = \"Bow\"\neffect = \"ranged | range 60 | hit +5 | 1d8 piercing\"\n",
        );
        c.speed = 30;
        let mut foe = parse(&format!(
            "creature: foe\nteam: b\nac: 15\nhp: 200\ninitiative: -50\nspeed: 0\nat: {foe_at}, 5\n"
        ))
        .unwrap()
        .remove(0);
        foe.team = 1;
        replayed(&[c, foe], 1)
    };
    let walked = |r: &Replay| {
        r.frames
            .iter()
            .any(|f| f.kind == FrameKind::Move && f.actor == Some(0))
    };
    // In range from where it stands: it aims, and does not move.
    let near = marksman(10);
    assert!(
        said(&near, "Steady Aim") && !walked(&near),
        "{:?}",
        near.lines
    );
    // Out of range: it walks, and looses without the aim.
    let far = marksman(20);
    assert!(!said(&far, "Steady Aim") && walked(&far), "{:?}", far.lines);
}

#[test]
fn something_that_fights_up_close_walks_into_the_press_to_cast() {
    let field = |traits: &str| {
        format!(
            "creature: priest\nteam: a\nac: 10\nhp: 50\ninitiative: 50\nat: 2, 5\n{traits}\
             action: Flame | spell | range 120 | targets 1 | save dex dc 100 | 0d6+10 fire\n\n\
             creature: foe\nteam: b\nac: 5\nhp: 200\ninitiative: -50\nspeed: 0\nat: 12, 5\n\
             action: Club | hit -100 | 1d4 bludgeoning\n"
        )
    };
    let apart = |r: &Replay| gap(at(r, 0), 1, at(r, 1), 1);
    let back = fought(&field(""), 2);
    assert!(
        apart(&back) >= 10,
        "it keeps its distance: {}",
        apart(&back)
    );
    let close = fought(&field("trait: fights up close\n"), 2);
    assert_eq!(apart(&close), 1, "{:?}", close.lines);
    assert_eq!(hp(&close, 1), 180, "and casts all the same");
}

/// How many of 600 fights of one round leave seat `seat` hurt.
fn hurt_in(text: &str, seat: usize, full: i32) -> usize {
    let creatures = parse(text).expect("the scenario loads");
    (0..600u64)
        .filter(|&seed| {
            let roster: Vec<&Creature> = creatures.iter().collect();
            let mut rng = Rng::new(seed);
            let r = replay_teams(&mut rng, &roster, [Policy::Greedy; 2], 1, Budget::default()).1;
            hp(&r, seat) < full
        })
        .count()
}

#[test]
fn it_goes_for_whoever_is_nearest_not_whoever_is_listed_first() {
    let text = "creature: archer\nteam: a\nac: 10\nhp: 50\ninitiative: 50\nspeed: 0\nat: 2, 5\n\
                action: Bow | ranged | range 300 | hit +100 | 0d6+10 piercing\n\n\
                creature: far\nteam: b\nac: 5\nhp: 200\ninitiative: -50\nspeed: 0\nat: 30, 5\n\n\
                creature: near\nteam: b\nac: 5\nhp: 200\ninitiative: -50\nspeed: 0\nat: 8, 5\n";
    let r = fought(text, 1);
    assert_eq!((hp(&r, 1), hp(&r, 2)), (200, 190));
}

#[test]
fn shooting_with_an_enemy_at_your_elbow_is_at_disadvantage() {
    let field = |kind: &str| {
        format!(
            "creature: shooter\nteam: a\nac: 10\nhp: 50\ninitiative: 50\nspeed: 0\nat: 2, 5\n\
             action: Shot | {kind} | hit +0 | 0d6+10 piercing\n\n\
             creature: foe\nteam: b\nac: 11\nhp: 200\ninitiative: -50\nspeed: 0\nat: 3, 5\n"
        )
    };
    // A coin toss with a blade; one in four with a bow in the same hands.
    let (blade, bow) = (
        hurt_in(&field("melee"), 1, 200),
        hurt_in(&field("ranged"), 1, 200),
    );
    assert!((255..=345).contains(&blade), "{blade} of 600");
    assert!((105..=195).contains(&bow), "{bow} of 600");
}

#[test]
fn a_weapon_reaches_its_long_range_at_disadvantage() {
    let field = |clause: &str, to_hit: i32| {
        format!(
            "creature: thrower\nteam: a\nac: 10\nhp: 50\ninitiative: 50\nspeed: 0\nat: 2, 5\n\
             action: Throw | ranged | range 20{clause} | hit {to_hit:+} | 0d6+10 piercing\n\n\
             creature: foe\nteam: b\nac: 11\nhp: 200\ninitiative: -50\nspeed: 0\nat: 10, 5\n"
        )
    };
    // Forty feet off: out of reach without a long range, in it with one.
    assert_eq!(hp(&fought(&field("", 100), 1), 1), 200);
    assert_eq!(hp(&fought(&field(" | long 60", 100), 1), 1), 190);
    let far = hurt_in(&field(" | long 60", 0), 1, 200);
    assert!((105..=195).contains(&far), "{far} of 600");
}

#[test]
fn somebody_standing_in_the_way_is_half_cover() {
    let field = |between: &str| {
        format!(
            "creature: archer\nteam: a\nac: 10\nhp: 50\ninitiative: 50\nspeed: 0\nat: 2, 5\n\
             action: Bow | ranged | range 300 | hit +10 | 0d6+10 piercing\n\n\
             creature: foe\nteam: b\nac: 20\nhp: 200\ninitiative: -50\nspeed: 0\nat: 12, 5\n\n\
             {between}"
        )
    };
    let friend = "creature: friend\nteam: a\nac: 10\nhp: 50\ninitiative: -60\nspeed: 0\nat: 7, 5\n";
    // Eleven faces of twenty hit an Armor Class of 20; nine hit 22.
    let (open, screened) = (hurt_in(&field(""), 1, 200), hurt_in(&field(friend), 1, 200));
    assert!((285..=375).contains(&open), "{open} of 600");
    assert!((225..=315).contains(&screened), "{screened} of 600");
    assert!(open > screened + 20, "{open} against {screened}");
}

#[test]
fn whoever_cannot_hurt_it_helps_whoever_can() {
    let squire = character(
        "squire",
        50,
        60,
        (4, 5),
        "\n[[pc.actions]]\nname = \"Poke\"\neffect = \"hit -100 | 1d4 piercing\"\n",
    );
    let knight = character(
        "knight",
        50,
        50,
        (4, 6),
        "\n[[pc.actions]]\nname = \"Sword\"\neffect = \"hit +5 | 2d6+4 slashing\"\n",
    );
    let mut foe =
        parse("creature: foe\nteam: b\nac: 16\nhp: 200\ninitiative: -50\nspeed: 0\nat: 5, 5\n")
            .unwrap()
            .remove(0);
    foe.team = 1;
    let r = replayed(&[squire, knight, foe], 1);
    assert!(said(&r, "squire: Help"), "{:?}", r.lines);
    // What anybody can do is there for everybody loaded from a file, after
    // what is its own.
    let names: Vec<String> = character("anyone", 10, 0, (0, 0), "")
        .actions
        .iter()
        .map(|m| m.name.clone())
        .collect();
    assert_eq!(names, ["Dodge", "Disengage", "Help", "Shove", "Grapple"]);
}

#[test]
fn a_swordsman_closes_even_on_a_turn_it_spends_marking_its_prey() {
    // Its best first turn is a mark that reaches sixty feet - which is no
    // reason to back off to sixty feet.
    let text = "creature: hunter\nteam: a\nac: 10\nhp: 50\ninitiative: 50\nat: 4, 5\n\
                action: Sword | hit +5 | 1d8+3 slashing\n\
                action: Mark | range 60 | worth 50 | targets 1 | save wis dc 100 \
                | on fail quarry for 1 minute\n\n\
                creature: foe\nteam: b\nac: 15\nhp: 200\ninitiative: -50\nspeed: 0\nat: 11, 5\n";
    let r = fought(text, 1);
    assert!(said(&r, "Mark"), "{:?}", r.lines);
    assert_eq!(gap(at(&r, 0), 1, at(&r, 1), 1), 1, "{:?}", r.lines);
}

#[test]
fn it_does_not_cast_two_things_to_concentrate_on_in_one_turn() {
    let text = "creature: caster\nteam: a\nac: 10\nhp: 50\ninitiative: 50\nspeed: 0\nat: 4, 5\n\
                action: Bind | spell | concentration | range 60 | targets 1 | worth 20 \
                | save wis dc 100 | on fail restrained for 1 minute\n\
                bonus: Veil | spell | concentration | stance invisible\n\n\
                creature: foe\nteam: b\nac: 5\nhp: 200\ninitiative: -50\nspeed: 0\nat: 8, 5\n";
    let r = fought(text, 2);
    assert!(said(&r, "Bind"), "{:?}", r.lines);
    assert!(
        !said(&r, "Veil"),
        "the veil would end the binding: {:?}",
        r.lines
    );
}
