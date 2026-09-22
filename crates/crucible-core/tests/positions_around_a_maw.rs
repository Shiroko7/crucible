//! A creature with a mouth - enemies standing at its mouth, beside its body
//! or out in front, a bite that reaches only the mouth, slams that knock
//! them back out, a cone out of the mouth, a whirlpool dragging them in -
//! declared as TOML through every keyword it needs, compiled, and fought
//! under each of the ways it can move.
//!
//! The creature is a stand-in with neutral names, not any specific one:
//! non-SRD monsters live in the user's own local files (see the README).
//! What it proves is that where everyone stands is not only loadable but
//! *live* - that it changes what a real fight does.

use crucible_core::creature::{Creature, Effect, Reach, Tactic};
use crucible_core::dsl::config::load_creature_from_str;
use crucible_core::features::FeatureRegistry;
use crucible_core::prob::Rng;
use crucible_core::sim::{evaluate_teams, run_teams, Budget, Policy};

const LEVIATHAN: &str = r#"
[monster]
name = "Test Leviathan"
size = "Gargantuan"
ac = 16
hp = 900
tactic = "charge"
condition_immune = ["prone"]
legendary_uses = 3
traits = [
    "mouth",
    "difficult terrain",
    "damage threshold 40 weak spot resists slashing, piercing",
]

[monster.saves]
str = 10
con = 10

[[monster.auras]]
name = "Test Undertow"
effect = "save str dc 14 | on fail pulled"

[[monster.actions]]
name = "Test Multiattack"
effect = "reach mouth | hit +11 | 3d10+6 piercing && reach near | strikes 2 | hit +11 | 2d8+6 bludgeoning | on hit save str dc 16 prone and pushed until victim"

[[monster.actions]]
name = "Test Surge"
effect = "recharge 6 | reach front | save con dc 16 | 8d8 cold | half on success | on fail pushed"

[[monster.bonus]]
name = "Test Clamp"
effect = "stance sealed"

[[monster.reactions]]
name = "Test Snap"
effect = "when enemy pulled | reach mouth | hit +11 | 3d10+6 piercing"

[[monster.legendary]]
name = "Test Sweep"
effect = "reach near | hit +11 | 2d8+6 bludgeoning | on hit save str dc 16 prone and pushed until victim"

[[monster.legendary]]
name = "Test Tug"
effect = "points 2 | save str dc 14 | on fail pulled"
"#;

const BLADE: &str = r#"
[pc]
name = "Test Blade"
ac = 19
hp = 180
initiative = 3
[pc.saves]
str = 6
[[pc.actions]]
name = "Sword"
effect = "strikes 2 | hit +10 | 2d6+6 slashing"
"#;

const BOW: &str = r#"
[pc]
name = "Test Bow"
ac = 16
hp = 140
initiative = 4
[[pc.actions]]
name = "Longbow"
effect = "ranged | strikes 2 | hit +10 | 1d8+6 piercing"
"#;

fn load(text: &str) -> Creature {
    load_creature_from_str(text, &FeatureRegistry::new()).expect("it compiles")
}

fn roster(tactic: Tactic) -> Vec<Creature> {
    let mut leviathan = load(LEVIATHAN);
    leviathan.team = 1;
    leviathan.tactic = tactic;
    vec![load(BLADE), load(BOW), leviathan]
}

#[test]
fn the_leviathan_compiles_to_what_it_declares() {
    let l = load(LEVIATHAN);
    assert!(l.mouth && l.difficult_terrain);
    assert_eq!(l.tactic, Tactic::Charge);
    let Effect::Sequence(parts) = &l.actions[0].effect else {
        panic!("the multiattack is a sequence");
    };
    let reach = |i: usize| match &parts[i] {
        Effect::Part { reach, .. } => *reach,
        other => panic!("part {i} has no reach of its own: {other:?}"),
    };
    assert_eq!(reach(0), Reach::Mouth);
    assert_eq!(reach(1), Reach::Near);
    assert_eq!(l.actions[1].reach, Reach::Front);
    assert_eq!(l.legendary[0].reach, Reach::Near);
}

/// Where everyone stands shows up in real fights: the sword walks to the
/// mouth, the bow stands off, the whirlpool drags them in to be bitten, the
/// slams throw them back out - and a hit-and-run leviathan withdraws.
#[test]
fn where_everyone_stands_is_live_in_a_fight() {
    let narrate = |tactic: Tactic| {
        let creatures = roster(tactic);
        let refs: Vec<&Creature> = creatures.iter().collect();
        let mut narration = String::new();
        let mut rng = Rng::new(3);
        for _ in 0..200 {
            let mut log = Some(Vec::new());
            run_teams(
                &mut rng,
                &refs,
                [Policy::Greedy; 2],
                8,
                Budget::default(),
                &mut log,
            );
            narration.push_str(&log.unwrap().join("\n"));
            narration.push('\n');
        }
        narration
    };

    let charging = narrate(Tactic::Charge);
    for sign in [
        "Test Blade: moves to the mouth",
        "Test Bow: moves to far off",
        "closes on",
        "and pulled, Test Leviathan reacts: Test Snap",
        "prone and pushed LANDED",
        "Test Clamp (sealed)",
    ] {
        assert!(charging.contains(sign), "never saw `{sign}` while charging");
    }
    assert!(!charging.contains("withdraws"));

    let running = narrate(Tactic::HitAndRun);
    assert!(
        running.contains("withdraws"),
        "a hit-and-run never withdrew"
    );
    assert!(
        !running.contains("Test Clamp"),
        "the getaway takes the bonus action"
    );

    let holding = narrate(Tactic::Hold);
    assert!(!holding.contains("closes on"), "holding still never moves");
}

/// How it moves changes the answer: a leviathan that comes for them hurts
/// them faster than one that sits still, which reaches only whoever walks
/// up to it or is dragged there - while the bow shoots into its mouth from
/// far off.
#[test]
fn how_it_moves_changes_how_the_fight_goes() {
    let damage_per_round = |tactic: Tactic| {
        let creatures = roster(tactic);
        let refs: Vec<&Creature> = creatures.iter().collect();
        evaluate_teams(9, &refs, [Policy::Greedy; 2], 400, 20, Budget::default())
            .mean_damage_per_round[1]
    };
    let (hold, charge) = (
        damage_per_round(Tactic::Hold),
        damage_per_round(Tactic::Charge),
    );
    assert!(
        charge > hold * 1.2,
        "charging should hurt faster than holding still: {charge} vs {hold} a round"
    );
}
