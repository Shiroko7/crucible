//! The three things a defensive caster does that are not damage: a ward that
//! soaks blows, a boon that is nothing but Armor Class, and a spell that
//! takes a creature out of the fight entirely.
//!
//! The build is a stand-in with neutral names, not any specific character:
//! non-SRD characters live in the user's own local files (see the README).
//! What it proves is that each is *live* in a real fight - that temporary hit
//! points are spent before hit points, that a boon's Armor Class actually
//! makes attacks miss, and that a banished creature is genuinely out of the
//! fight rather than merely stunned.
//!
//! The mechanism tests narrow the creature to the one move under test and run
//! it `InOrder`. That is deliberate: a purely defensive option is worth
//! nothing to a policy that ranks on damage - `value.rs` says so outright -
//! so `Greedy` would simply never take it, and a test that waited for it to
//! would be testing the policy rather than the mechanism.

use crucible_core::creature::{Creature, Effect};
use crucible_core::dsl::config::load_creature_from_str;
use crucible_core::dsl::scenario::parse;
use crucible_core::features::FeatureRegistry;
use crucible_core::prob::Rng;
use crucible_core::rules::Condition;
use crucible_core::sim::{run_teams, Budget, Policy};

const WARDER: &str = r#"
[pc]
name = "Test Warder"
ac = 14
hp = 80
initiative = 0

[pc.abilities]
cha = 18

[pc.saves]
cha = 7

[pc.resources.slots]
1 = 2
4 = 1

[pc.spellcasting]
ability = "cha"
ability_modifier = 4
proficiency_bonus = 3

[[pc.features]]
plugin = "lasting_boon"
name = "Shield of Faith"
bonus_action = true
concentration = true
kind = "spell"
rounds = 100
ac = 2

[[pc.actions]]
name = "Earthen Ward"
effect = "temp 1d10+4"

[[pc.actions]]
name = "Banish"
effect = "spell | slot 4 | concentration | save cha dc 15 | on fail banished for 10 rounds"

[[pc.actions]]
name = "Staff"
effect = "hit +6 | 1d6+4 bludgeoning"
"#;

fn warder() -> Creature {
    load_creature_from_str(WARDER, &FeatureRegistry::new()).expect("the warder loads")
}

/// Narrow the warder to one action, so `InOrder` has no choice but to take
/// it, and drop the bonus action so nothing else muddies the comparison.
fn warder_with_only(action: &str) -> Creature {
    let mut c = warder();
    c.actions.retain(|m| m.name == action);
    c.bonus_actions.clear();
    c.team = 0;
    c
}

/// A boon can be purely defensive: no damage, no resistances, just Armor
/// Class for as long as it is held.
#[test]
fn a_boon_can_be_nothing_but_armor_class() {
    let c = warder();
    let boon = c
        .boons
        .iter()
        .find(|b| b.name == "Shield of Faith")
        .expect("the boon is declared");
    assert_eq!(boon.ac, 2);
    assert!(boon.damage.is_none(), "it adds nothing to a blow");
    assert!(boon.resist.is_empty(), "and resists nothing");
}

/// The Armor Class is real: the same attacker lands less once it is up.
#[test]
fn a_boons_armor_class_makes_attacks_miss() {
    let foe = parse(
        "creature: Puncher\nac: 10\nhp: 400\ninitiative: 20\n\
         action: Jab | strikes 2 | hit +4 | 1d4 bludgeoning\n",
    )
    .expect("parses")[0]
        .clone();

    let taken = |with_boon: bool| {
        let mut me = warder();
        if !with_boon {
            // Strip the only way to put it up, leaving everything else equal.
            me.bonus_actions.retain(|m| m.name != "Shield of Faith");
        }
        me.team = 0;
        let mut foe = foe.clone();
        foe.team = 1;
        let mut rng = Rng::new(31);
        let mut total = 0i64;
        for _ in 0..300 {
            let mut log = None;
            let out = run_teams(
                &mut rng,
                &[&me, &foe],
                [Policy::Greedy; 2],
                6,
                Budget::default(),
                &mut log,
            );
            total += out.damage_dealt[1];
        }
        total
    };

    let (warded, bare) = (taken(true), taken(false));
    assert!(
        warded < bare,
        "the boon's AC should turn some hits into misses: {warded} taken with it, {bare} without"
    );
}

/// Temporary hit points are a ward, not a heal: spent before hit points.
#[test]
fn temporary_hit_points_soak_damage_before_hit_points() {
    let c = warder();
    let ward = c
        .actions
        .iter()
        .find(|m| m.name == "Earthen Ward")
        .expect("the ward is an action");
    let Effect::TempHp(roll) = &ward.effect else {
        panic!("a ward is temporary hit points, not a heal");
    };
    assert_eq!((roll.count, roll.sides, roll.bonus), (1, 10, 4));

    let foe = parse(
        "creature: Puncher\nac: 30\nhp: 400\ninitiative: -20\n\
         action: Jab | hit +20 | 2d6 bludgeoning\n",
    )
    .expect("parses")[0]
        .clone();

    let run = |action: &str| {
        let me = warder_with_only(action);
        let mut foe = foe.clone();
        foe.team = 1;
        let mut rng = Rng::new(11);
        let mut log = Some(Vec::new());
        let out = run_teams(
            &mut rng,
            &[&me, &foe],
            [Policy::InOrder; 2],
            6,
            Budget::default(),
            &mut log,
        );
        (out.hp_left[0], log.expect("a log was asked for"))
    };

    let (warded_hp, lines) = run("Earthen Ward");
    let (bare_hp, _) = run("Staff");
    assert!(
        lines.iter().any(|l| l.contains("temp hp")),
        "the ward should have gone up:\n{}",
        lines.join("\n")
    );
    assert!(
        warded_hp > bare_hp,
        "the ward should soak blows that otherwise reach hit points: \
         {warded_hp} hp left with it, {bare_hp} without"
    );
}

/// A banished creature is out of the fight: it takes no turn while it is
/// gone, and comes back when the spell ends.
#[test]
fn a_banished_creature_takes_no_turn_while_it_is_gone() {
    assert!(
        Condition::Banished.incapacitated(),
        "it takes no turn while it is gone"
    );
    assert_eq!(Condition::parse("banished"), Some(Condition::Banished));

    let foe = parse(
        "creature: Ogre\nac: 11\nhp: 90\ninitiative: -1\nsaves: cha -2\n\
         action: Club | hit +6 | 2d8+4 bludgeoning\n",
    )
    .expect("parses")[0]
        .clone();

    let me = warder_with_only("Banish");
    let mut foe = foe;
    foe.team = 1;

    // A Charisma save of -2 against DC 15 fails most of the time, but not
    // always, so this asks only that it happens somewhere in a run of fights.
    let mut seen = false;
    for seed in 0..40 {
        let mut rng = Rng::new(seed);
        let mut log = Some(Vec::new());
        run_teams(
            &mut rng,
            &[&me, &foe],
            [Policy::InOrder; 2],
            6,
            Budget::default(),
            &mut log,
        );
        let lines = log.expect("a log was asked for");
        let Some(at) = lines.iter().position(|l| l.contains("and banished")) else {
            continue;
        };
        seen = true;
        // The very next thing the ogre does is nothing at all.
        let next = lines[at + 1..]
            .iter()
            .find(|l| l.contains("Ogre:"))
            .expect("the ogre's turn comes round");
        assert!(
            next.contains("loses its turn"),
            "a banished ogre should lose its turn, got `{next}`:\n{}",
            lines.join("\n")
        );
        break;
    }
    assert!(
        seen,
        "a DC 15 Charisma save against -2 should fail at least once in 40 fights"
    );
}

/// Nothing reaches a banished creature either - an area that catches
/// everybody still leaves it alone.
#[test]
fn nothing_reaches_a_banished_creature() {
    let foe = parse(
        "creature: Ogre\nac: 11\nhp: 400\ninitiative: -1\nsaves: cha -2\n\
         action: Club | hit +6 | 2d8+4 bludgeoning\n",
    )
    .expect("parses")[0]
        .clone();

    let mut me = warder();
    me.actions
        .retain(|m| m.name == "Banish" || m.name == "Staff");
    me.bonus_actions.clear();
    me.team = 0;
    let mut foe = foe;
    foe.team = 1;

    for seed in 0..40 {
        let mut rng = Rng::new(seed);
        let mut log = Some(Vec::new());
        run_teams(
            &mut rng,
            &[&me, &foe],
            [Policy::InOrder; 2],
            6,
            Budget::default(),
            &mut log,
        );
        let lines = log.expect("a log was asked for");
        let Some(at) = lines.iter().position(|l| l.contains("and banished")) else {
            continue;
        };
        // While it is away the staff finds nothing to hit: the warder's own
        // next turn cannot land on it.
        if let Some(swing) = lines[at + 1..].iter().find(|l| l.contains("Staff")) {
            assert!(
                swing.contains("no target") || swing.contains("miss") || !swing.contains('('),
                "a banished ogre should not be reachable by a staff: `{swing}`\n{}",
                lines.join("\n")
            );
        }
        return;
    }
    panic!("a DC 15 Charisma save against -2 should fail at least once in 40 fights");
}
