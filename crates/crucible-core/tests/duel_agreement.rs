//! The agreement test, extended to the duel layer.
//!
//! `exact_vs_sampled.rs` covers a single-pool attack. Everything the duel
//! layer added on top of it is a new place the two paths can drift apart, and
//! each one is the kind of mistake that produces a plausible number rather
//! than an obvious one:
//!
//! - damage split across types, each reduced separately, so halving twice and
//!   rounding down each time is not the same as halving the total;
//! - a critical hit doubling the dice in *every* component while doubling none
//!   of the modifiers;
//! - a saving throw, which has no natural-20 rule, halving before resistance
//!   halves again;
//! - several strikes in one move, where the exact side convolves and the
//!   sampled side adds.
//!
//! Tolerances are five standard errors, derived the same way and for the same
//! reason as in the original: a number picked because it passed would test
//! nothing. Seeds are fixed so a failure is reproducible.

use crucible_core::creature::{Creature, Effect, Rider, SaveEffect, Strike};
use crucible_core::prob::Rng;
use crucible_core::rules::{Ability, AttackModifier, DamageKind, DamageRoll, Reduction, RollMode};
const SAMPLES: usize = 200_000;

fn tolerance(p: f64, n: usize) -> f64 {
    5.0 * (p * (1.0 - p) / n as f64).sqrt() + 1e-4
}

fn target(ac: i32, save: i32, reductions: &[(DamageKind, Reduction)]) -> Creature {
    let mut c = Creature::new("target", ac, 1_000);
    c.saves = [save; 6];
    c.reductions = reductions.to_vec();
    c
}

/// Compares a sampled histogram against an exact PMF, outcome by outcome.
/// Stronger than comparing means, which a distribution with the right average
/// in the wrong places would pass.
fn agree(
    name: &str,
    seed: u64,
    exact: &crucible_core::prob::Pmf,
    mut draw: impl FnMut(&mut Rng) -> i32,
) {
    let mut rng = Rng::new(seed);
    let (lo, hi) = (exact.min(), exact.max());
    assert!(lo >= 0, "{name}: damage should never be negative");

    let mut counts = vec![0usize; (hi - lo + 1) as usize];
    for _ in 0..SAMPLES {
        let d = draw(&mut rng);
        assert!(
            d >= lo && d <= hi,
            "{name}: sampled {d} outside the exact support {lo}..={hi}"
        );
        counts[(d - lo) as usize] += 1;
    }

    for (i, &c) in counts.iter().enumerate() {
        let value = lo + i as i32;
        let want = exact.prob(value);
        let got = c as f64 / SAMPLES as f64;
        let tol = tolerance(want, SAMPLES);
        assert!(
            (got - want).abs() < tol,
            "{name}: P(damage = {value}) sampled {got:.5}, exact {want:.5}, tolerance {tol:.5}"
        );
    }
}

/// An adult red dragon's Rend is the motivating case: one attack roll, two
/// damage types, against targets that treat those types differently.
fn rend() -> Strike {
    Strike::new(
        14,
        vec![
            DamageRoll::new(1, 10, 8, DamageKind::Slashing),
            DamageRoll::new(2, 4, 0, DamageKind::Fire),
        ],
    )
}

#[test]
fn a_multi_type_strike_samples_like_its_exact_distribution() {
    let cases: Vec<(&str, Strike, Creature)> = vec![
        ("plain rend", rend(), target(20, 0, &[])),
        (
            "rend against fire immunity, where one component vanishes",
            rend(),
            target(20, 0, &[(DamageKind::Fire, Reduction::Immune)]),
        ),
        (
            "rend against resistance to both, rounding down twice",
            rend(),
            target(
                18,
                0,
                &[
                    (DamageKind::Fire, Reduction::Resistant),
                    (DamageKind::Slashing, Reduction::Resistant),
                ],
            ),
        ),
        (
            "a component floored at zero before the other is doubled",
            Strike::new(
                10,
                vec![
                    DamageRoll::new(1, 4, -6, DamageKind::Piercing),
                    DamageRoll::new(1, 6, 2, DamageKind::Cold),
                ],
            ),
            target(14, 0, &[(DamageKind::Cold, Reduction::Vulnerable)]),
        ),
        (
            "unhittable except on a natural 20, so every hit is a crit",
            rend(),
            target(40, 0, &[]),
        ),
    ];

    for (seed, (name, strike, defender)) in cases.into_iter().enumerate() {
        let exact = strike.damage_pmf(&defender);
        agree(name, seed as u64 + 1, &exact, |rng| {
            strike.sample(rng, &defender)
        });
    }
}

#[test]
fn a_saving_throw_samples_like_its_exact_distribution() {
    let breath = |dc: i32, half: bool| SaveEffect {
        ability: Ability::Dex,
        dc,
        damage: vec![DamageRoll::new(17, 6, 0, DamageKind::Fire)],
        half_on_success: half,
        on_failure: vec![],
        max_targets: None,
        requires_type: None,
    };

    let cases: Vec<(&str, SaveEffect, Creature)> = vec![
        (
            "fire breath, half on a success",
            breath(21, true),
            target(20, 2, &[]),
        ),
        (
            "fire breath, nothing on a success",
            breath(15, false),
            target(20, 6, &[]),
        ),
        (
            "half on a success and then resistance, halving twice",
            breath(17, true),
            target(20, 4, &[(DamageKind::Fire, Reduction::Resistant)]),
        ),
        (
            "a save nobody can make",
            breath(40, true),
            target(20, 0, &[]),
        ),
        (
            "a save nobody can fail",
            breath(0, true),
            target(20, 0, &[]),
        ),
    ];

    for (seed, (name, save, defender)) in cases.into_iter().enumerate() {
        let exact = save.damage_pmf(&defender);
        agree(name, seed as u64 + 100, &exact, |rng| {
            save.sample(rng, &defender).0
        });
    }
}

/// Evasion is the single largest change a character sheet can make to a breath
/// weapon, and it composes with resistance rather than replacing it. Both paths
/// have to agree about that, because getting the order wrong produces a
/// perfectly plausible number.
#[test]
fn evasion_and_resistance_together_sample_like_the_exact_shape() {
    let breath = SaveEffect {
        ability: Ability::Dex,
        dc: 21,
        damage: vec![DamageRoll::new(17, 6, 0, DamageKind::Fire)],
        half_on_success: true,
        on_failure: vec![],
        max_targets: None,
        requires_type: None,
    };

    let mut monk = target(20, 2, &[(DamageKind::Fire, Reduction::Resistant)]);
    monk.riders.push(Rider::NothingOnSuccess {
        ability: Ability::Dex,
    });

    let exact = breath.damage_pmf(&monk);
    agree(
        "17d6 fire, dc 21, evasion and fire resistance",
        42,
        &exact,
        |rng| breath.sample(rng, &monk).0,
    );

    // And the size of the effect, which is the reason it cannot be left out:
    // a cloak and one class feature take a breath weapon under a third of what
    // it does to someone without them.
    let bare = breath.damage_pmf(&target(20, 2, &[])).mean();
    assert!(
        exact.mean() < 0.30 * bare,
        "evasion plus resistance took {:.1} of a bare {bare:.1}",
        exact.mean()
    );
}

/// A Multiattack is several independent rolls, so the exact side convolves
/// where the sampled side adds. Getting this wrong by convolving once too few
/// times is invisible in a mean and obvious here.
#[test]
fn several_strikes_in_one_move_agree_with_the_convolution() {
    let defender = target(20, 0, &[(DamageKind::Fire, Reduction::Immune)]);
    let effect = Effect::Strikes {
        strike: rend(),
        count: 3,
    };
    let Effect::Strikes { strike, count } = &effect else {
        unreachable!()
    };
    let exact = effect.damage_pmf(&defender);
    agree("three rends", 7, &exact, |rng| {
        (0..*count).map(|_| strike.sample(rng, &defender)).sum()
    });
}

/// The means have to line up too, and this is the cheap check that catches a
/// component silently dropped from the list.
#[test]
fn mean_damage_matches_a_hand_calculation() {
    // +14 against AC 20 hits on a 6 or better: 14 ordinary hits, one crit,
    // five misses. On a hit, 1d10+8 is 13.5 and 2d4 is 5. On a crit the dice
    // double but the +8 does not: 2d10+8 is 19, 4d4 is 10.
    let defender = target(20, 0, &[]);
    let expected = (14.0 / 20.0) * 18.5 + (1.0 / 20.0) * 29.0;
    let got = rend().mean_damage(&defender);
    assert!(
        (got - expected).abs() < 1e-9,
        "mean rend damage {got:.6}, hand-computed {expected:.6}"
    );
}

/// The same `AttackModifier`/damage-rider hook `exact_vs_sampled.rs` checks
/// against the single-pool [`crucible_core::rules::Attack`], checked again here
/// against a multi-type [`Strike`] - Bless on the roll, Sneak Attack's dice on
/// the damage - since that is the type `sim::duel` actually resolves against,
/// and a hook that only worked on the standalone test fixture would not be a
/// hook at all.
#[test]
fn attack_modifiers_and_damage_riders_agree_on_a_multi_type_strike() {
    let cases: Vec<(&str, Vec<AttackModifier>, Vec<DamageRoll>, Creature)> = vec![
        (
            "bless on a rend",
            vec![AttackModifier::BonusDice { count: 1, sides: 4 }],
            vec![],
            target(20, 0, &[]),
        ),
        (
            "bane on a rend",
            vec![AttackModifier::PenaltyDice { count: 1, sides: 4 }],
            vec![],
            target(18, 0, &[]),
        ),
        (
            "sneak attack dice, conditionally appended on a hit",
            vec![],
            vec![DamageRoll::new(3, 6, 0, DamageKind::Piercing)],
            target(16, 0, &[]),
        ),
        (
            "bless and sneak attack together, against resistance",
            vec![AttackModifier::BonusDice { count: 1, sides: 4 }],
            vec![DamageRoll::new(3, 6, 0, DamageKind::Piercing)],
            target(17, 0, &[(DamageKind::Slashing, Reduction::Resistant)]),
        ),
    ];

    for (seed, (name, modifiers, extra, defender)) in cases.into_iter().enumerate() {
        let strike = rend();
        let exact =
            strike.damage_pmf_with_modifiers(&defender, RollMode::Normal, &modifiers, &extra);
        agree(name, seed as u64 + 900, &exact, |rng| {
            strike
                .sample_forcing_crit_with_modifiers(
                    rng,
                    &defender,
                    RollMode::Normal,
                    false,
                    &modifiers,
                    &extra,
                )
                .0
        });
    }
}

/// [`Rider::ReactionOnTargeted`] is a reaction spent before hit or miss is
/// finalized rather than a modifier folded into the roll like
/// [`AttackModifier`] or a rider appended to the damage like the sneak
/// attack case above, so it gets its own agreement check - on the same
/// multi-type [`Strike`] `sim::duel` actually resolves against, the same way
/// the case above does for `AttackModifier`/`DamageRider`.
#[test]
fn a_reactive_ac_boost_agrees_with_the_exact_path_on_a_multi_type_strike() {
    let cases: Vec<(&str, i32, bool, Creature)> = vec![
        (
            "available, and large enough to matter",
            5,
            true,
            target(15, 0, &[]),
        ),
        (
            "unavailable: no change from the plain strike",
            5,
            false,
            target(15, 0, &[]),
        ),
        (
            "available, against resistance on both damage types too",
            5,
            true,
            target(
                15,
                0,
                &[
                    (DamageKind::Slashing, Reduction::Resistant),
                    (DamageKind::Fire, Reduction::Resistant),
                ],
            ),
        ),
    ];

    for (seed, (name, ac_bonus, available, defender)) in cases.into_iter().enumerate() {
        let strike = rend();
        let exact =
            strike.damage_pmf_with_reaction(&defender, RollMode::Normal, ac_bonus, available);
        let attacker = Creature::new("attacker", 10, 10);
        agree(name, seed as u64 + 950, &exact, |rng| {
            strike
                .sample_from(
                    rng,
                    &attacker,
                    &defender,
                    RollMode::Normal,
                    false,
                    &[],
                    &[],
                    ac_bonus,
                    available,
                )
                .0
        });
    }
}

/// A passive item's flat AC bonus (`trait: ac 2`) is not a new engine hook -
/// it folds straight into the creature's own `ac` field before combat
/// resolution ever runs, unlike [`Rider::ReactionOnTargeted`]'s *reactive*
/// boost above. So the agreement this needs is the plainest kind: build a
/// creature through the same `trait:` DSL a real item's bundle would use, and
/// check the exact and sampled paths still agree on the AC that comes out.
#[test]
fn an_item_ac_bonus_trait_composes_into_ac_and_still_agrees_with_the_exact_path() {
    let boosted = crucible_core::dsl::scenario::parse(
        "creature: x\nac: 14\nhp: 1000\ntrait: ac 2\ntrait: ac bonus 2\n",
    )
    .expect("an ac-boosted creature parses")
    .remove(0);
    // Two stacked AC-granting traits, on top of a 14 base.
    assert_eq!(boosted.ac, 18);

    let strike = rend();
    let exact = strike.damage_pmf(&boosted);
    agree(
        "a rend against a target wearing two AC-granting items",
        970,
        &exact,
        |rng| strike.sample(rng, &boosted),
    );
}

/// The mirror case for the spell attack/DC bonus: it composes additively
/// into [`crucible_core::rules::SpellCastingProfile::item_bonus`], and the
/// resulting DC is then just an ordinary saving throw DC, so the same
/// agreement machinery [`a_saving_throw_samples_like_its_exact_distribution`]
/// uses applies unchanged - the point is that the number the trait produces
/// is the number both paths already agree on.
#[test]
fn an_item_spell_dc_bonus_composes_and_still_agrees_with_the_exact_path() {
    let registry = crucible_core::features::FeatureRegistry::new();
    let toml = r#"
        [pc]
        name = "Test Caster"
        ac = 12
        hp = 30
        traits = ["spell 2"]

        [pc.spellcasting]
        ability = "int"
        ability_modifier = 3
        proficiency_bonus = 2
    "#;
    let caster = crucible_core::dsl::load_creature_from_str(toml, &registry)
        .expect("a caster with a spell-bonus trait parses");
    // 3 (INT mod) + 2 (proficiency) + 2 (item, from the trait) = 7; DC 15.
    assert_eq!(caster.spell_save_dc(), Some(15));
    let dc = caster.spell_save_dc().unwrap();

    let breath = SaveEffect {
        ability: Ability::Dex,
        dc,
        damage: vec![DamageRoll::new(8, 6, 0, DamageKind::Necrotic)],
        half_on_success: true,
        on_failure: Vec::new(),
        max_targets: None,
        requires_type: None,
    };
    let defender = target(16, 2, &[]);
    let exact = breath.damage_pmf(&defender);
    agree(
        "a save against a dc raised by an item's spell bonus trait",
        980,
        &exact,
        |rng| breath.sample(rng, &defender).0,
    );
}
