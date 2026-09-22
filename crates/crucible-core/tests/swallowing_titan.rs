//! A whole titan - a shell only a big enough hit breaks, a maw that swallows,
//! a maelstrom that drags prey into reach, reactions that are moves of their
//! own, and legendary actions that cost more than one - declared as TOML
//! through every trait keyword and clause it needs, compiled, and fought.
//!
//! The titan is a stand-in with neutral names, not any specific creature:
//! non-SRD monsters live in the user's own local files (see the README).
//! What it proves is that every mechanism such a monster leans on is not only
//! loadable but *live* - that a real fight actually applies it.

use crucible_core::creature::{Creature, Effect, ReactionTrigger, Rider};
use crucible_core::dsl::config::load_creature_from_str;
use crucible_core::features::FeatureRegistry;
use crucible_core::prob::Rng;
use crucible_core::rules::{Condition, DamageKind, Size};
use crucible_core::sim::{run_teams, Budget, Policy};

const TITAN: &str = r#"
[monster]
name = "Test Titan"
size = "Gargantuan"
creature_type = "Monstrosity"
ac = 15
hp = 600
condition_immune = ["prone", "frightened", "charmed", "petrified", "exhaustion"]
legendary_uses = 3
traits = [
    "legendary resistance 1",
    "damage threshold 30 cracks weak spot resists bludgeoning, piercing, slashing",
    "digest 4d6 acid",
    "regurgitate 25 con dc 99",
]

[monster.saves]
str = 10
con = 10

[[monster.auras]]
name = "Test Undertow"
effect = "save str dc 15 | on fail pulled"

[[monster.actions]]
name = "Test Multiattack"
effect = "hit +10 | 2d10+5 piercing | on hit swallow large && strikes 2 | hit +10 | 2d8+5 bludgeoning | on hit save str dc 15 prone until victim && stance exposed"

[[monster.bonus]]
name = "Test Clamp"
effect = "stance sealed"

[[monster.reactions]]
name = "Test Snap"
effect = "when enemy pulled | hit +10 | 2d10+5 piercing | on hit swallow large"

[[monster.reactions]]
name = "Test Spray"
effect = "when breached | save dex dc 15 | 3d10 piercing | half on success"

[[monster.legendary]]
name = "Test Squeeze"
effect = "points 2 | swallowed | 3d10 bludgeoning"

[[monster.legendary]]
name = "Test Swipe"
effect = "hit +10 | 2d8+5 bludgeoning"
"#;

/// Hits hard enough to breach the shell most of the time from outside, and
/// to force a regurgitation save from inside.
const HEAVY: &str = r#"
[pc]
name = "Test Heavy"
ac = 18
hp = 400
initiative = 20
[pc.saves]
con = 5
[[pc.actions]]
name = "Maul"
effect = "strikes 2 | hit +12 | 4d12+12 slashing"
"#;

/// Never breaches it: only from inside, or through a crack, does it land.
const LIGHT: &str = r#"
[pc]
name = "Test Light"
ac = 16
hp = 300
[[pc.actions]]
name = "Dagger"
effect = "strikes 2 | hit +8 | 1d4+4 piercing"
"#;

fn load(text: &str) -> Creature {
    load_creature_from_str(text, &FeatureRegistry::new()).expect("it compiles")
}

#[test]
fn the_titan_compiles_to_what_it_declares() {
    let t = load(TITAN);
    assert_eq!(t.size, Size::Gargantuan);
    assert_eq!(t.damage_threshold(), Some((30, true)));
    assert_eq!(
        t.weak_spot_resists(),
        [
            DamageKind::Bludgeoning,
            DamageKind::Piercing,
            DamageKind::Slashing
        ]
    );
    assert_eq!(t.digestion().len(), 1);
    assert_eq!(t.regurgitation().map(|(n, _, _)| n), Some(25));
    assert!(t.immune_to_condition(Condition::Frightened));
    assert!(t.immune_to_condition(Condition::Petrified));
    assert_eq!(t.auras.len(), 1);
    assert_eq!(
        t.reactions.iter().map(|r| r.trigger).collect::<Vec<_>>(),
        [
            ReactionTrigger::EnemyGains(Condition::Pulled),
            ReactionTrigger::Breached
        ]
    );
    assert_eq!(t.legendary[0].legendary_cost, 2);
    assert!(matches!(
        t.legendary[0].effect,
        Effect::HarmSwallowed { .. }
    ));

    // The bite swallows and the slams knock down - each on its own part.
    let Effect::Sequence(parts) = &t.actions[0].effect else {
        panic!("the multiattack is a sequence");
    };
    let riders = |i: usize| match &parts[i] {
        Effect::WithRiders { riders, .. } => riders.clone(),
        other => panic!("part {i} carries no riders: {other:?}"),
    };
    assert!(matches!(riders(0)[..], [Rider::Swallow { .. }]));
    assert!(matches!(riders(1)[..], [Rider::SaveOrCondition { .. }]));
}

/// Every mechanism shows up in the narration of real fights.
#[test]
fn every_part_of_the_titan_is_live_in_a_fight() {
    let titan = load(TITAN);
    let heavy = load(HEAVY);
    let light = load(LIGHT);
    let mut titan_b = titan.clone();
    titan_b.team = 1;

    let mut narration = String::new();
    let mut rng = Rng::new(11);
    for _ in 0..200 {
        let mut log = Some(Vec::new());
        run_teams(
            &mut rng,
            &[&heavy, &light, &titan_b],
            [Policy::Greedy; 2],
            8,
            Budget::default(),
            &mut log,
        );
        narration.push_str(&log.unwrap().join("\n"));
        narration.push('\n');
    }
    for sign in [
        "(aura)",
        "reacts: Test Snap",
        "swallowed",
        "digests",
        "(breach)",
        "reacts: Test Spray",
        "(shell took",
        "regurgitates",
        "Test Squeeze",
        "Test Clamp (sealed)",
    ] {
        assert!(narration.contains(sign), "never saw `{sign}` in 200 fights");
    }
}

/// A shell only a breach gets through: with its mouth never opened, a light
/// attacker alone does nothing to it at all - and the same attacks do land on
/// the same creature without one.
#[test]
fn the_shell_holds_against_light_blows() {
    let light = load(LIGHT);
    let mut shell = load(TITAN);
    shell.team = 1;
    shell.actions.clear();
    shell.bonus_actions.clear();
    shell.legendary.clear();
    shell.reactions.clear();
    shell.auras.clear();
    let mut bare = shell.clone();
    bare.riders
        .retain(|r| !matches!(r, Rider::DamageThreshold { .. }));

    let dealt = |target: &Creature| {
        let mut rng = Rng::new(5);
        let mut total = 0;
        for _ in 0..200 {
            let o = run_teams(
                &mut rng,
                &[&light, target],
                [Policy::Greedy; 2],
                3,
                Budget::default(),
                &mut None,
            );
            total += o.damage_dealt[0];
        }
        total
    };
    let (against_shell, against_bare) = (dealt(&shell), dealt(&bare));
    assert!(against_bare > 0);
    assert_eq!(against_shell, 0, "1d4+4 never reaches 30");
}
