//! Property tests for Sneak Attack.
//!
//! `features/classes/rogue/sneak_attack.rs`'s own unit tests fix a handful of
//! representative cases by hand: advantage triggers it, an adjacent ally
//! triggers it without advantage, disadvantage cancels it outright, and a crit
//! doubles its dice like any other rider. This file asks the same question
//! `invariants.rs` and `exact_vs_sampled.rs` ask of the rest of combat - not
//! "does this one case agree" but "is there any input in the space of dice
//! pools, to-hit bonuses, ACs and trigger flags where the exact convolution
//! and the sampled roll disagree." `proptest` searches that space and shrinks
//! whatever it finds.
//!
//! Sneak Attack is [`Rider::ConditionalExtraDamage`], gated on flags read
//! straight off the [`Attack`] being resolved (see
//! `features/classes/rogue/sneak_attack.rs` for the gate itself, and
//! `DESIGN.md` for why positioning is a flag rather than geometry). Three of
//! its four trigger conditions get their own property test here, fixing the
//! roll mode and letting dice pools, to-hit, AC, weapon-qualification and
//! ally-adjacency vary freely: normal (only an adjacent ally can open the
//! gate), advantage (the gate is open regardless of an ally), and disadvantage
//! (the gate never opens, even with a qualifying weapon and an ally in place).
//! The fourth trigger condition - critical-hit doubling of the *rider's own*
//! dice, not just the weapon's - gets a dedicated property test, because
//! "matches the exact PMF" and "matches what un-doubled rider dice would
//! produce" are numerically close enough that a coarse check (comparing means,
//! say) could pass on a subtly broken implementation that only doubles the
//! weapon's pool.
//!
//! Tolerances are the same derived formula as everywhere else in this
//! project: five standard errors of the estimate, plus a floor so a
//! probability-zero outcome is not held to a tolerance of exactly zero.

use crucible_core::creature::Rider;
use crucible_core::prob::Rng;
use crucible_core::rules::{damage_pmf, sample_damage, Attack, Defense, RollMode};
use proptest::prelude::*;
/// Matches the sample counts `exact_vs_sampled.rs` and `duel_agreement.rs`
/// use for a single attack, scaled down a little because `proptest` repeats
/// this over many generated cases rather than a fixed handful.
const SAMPLES: usize = 40_000;

/// Five standard errors, with a small floor so that outcomes of probability
/// zero are not held to a tolerance of exactly zero - see `exact_vs_sampled.rs`.
fn tolerance(p: f64, n: usize) -> f64 {
    5.0 * (p * (1.0 - p) / n as f64).sqrt() + 1e-4
}

/// Sneak Attack as a bare `Rider`, before the gate decides whether it applies
/// to a given attack - mirrors `features/classes/rogue/sneak_attack.rs`'s own
/// `rider()` test helper.
fn sneak_attack(dice_count: u32, dice_sides: u32) -> Rider {
    Rider::ConditionalExtraDamage {
        dice_count,
        dice_sides,
        once_per_turn: true,
    }
}

/// Applies Sneak Attack's rider to `attack` if its gate holds, exactly the way
/// a real attack resolution would - see `Rider::extra_damage_for`. A fresh
/// once-per-turn budget (`used_this_turn: false`) throughout: that budget is a
/// gate-state concern `features/classes/rogue/sneak_attack.rs` already covers,
/// not an exact-vs-sampled one.
fn with_sneak_attack(attack: Attack, dice_count: u32, dice_sides: u32) -> Attack {
    match sneak_attack(dice_count, dice_sides).extra_damage_for(&attack, false) {
        Some(rider) => attack.with_damage_rider(rider),
        None => attack,
    }
}

/// Compares a sampled damage histogram against the exact PMF, outcome by
/// outcome - stronger than comparing means, which a distribution with the
/// right average in the wrong places would still pass.
fn assert_samples_match_exact(name: &str, seed: u64, attack: &Attack, defense: &Defense) {
    let exact = damage_pmf(attack, defense);
    let (lo, hi) = (exact.min(), exact.max());
    assert!(lo >= 0, "{name}: damage should never be negative");

    let mut rng = Rng::new(seed);
    let mut counts = vec![0usize; (hi - lo + 1) as usize];
    for _ in 0..SAMPLES {
        let d = sample_damage(&mut rng, attack, defense);
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

proptest! {
    #![proptest_config(ProptestConfig::with_cases(20))]

    /// Normal rolls: Sneak Attack triggers only when an ally is adjacent to
    /// the target and the weapon qualifies - advantage is not in play, so
    /// the ally clause is the only gate left open.
    #[test]
    fn sneak_attack_agrees_under_normal_rolls(
        seed: u64,
        to_hit in -2i32..12, ac in 6i32..24,
        count in 1u32..4, sides in 2u32..9, bonus in -3i32..6,
        sneak_count in 1u32..5, sneak_sides in 2u32..9,
        finesse: bool, ally_adjacent: bool,
    ) {
        let attack = Attack::new(to_hit, count, sides, bonus)
            .with_mode(RollMode::Normal)
            .with_finesse_or_ranged(finesse)
            .with_ally_adjacent(ally_adjacent);
        let attack = with_sneak_attack(attack, sneak_count, sneak_sides);
        let defense = Defense::new(ac, 200);
        assert_samples_match_exact("normal", seed, &attack, &defense);
    }

    /// Advantage: Sneak Attack triggers outright on a qualifying weapon,
    /// whether or not an ally happens to be adjacent too.
    #[test]
    fn sneak_attack_agrees_under_advantage_rolls(
        seed: u64,
        to_hit in -2i32..12, ac in 6i32..24,
        count in 1u32..4, sides in 2u32..9, bonus in -3i32..6,
        sneak_count in 1u32..5, sneak_sides in 2u32..9,
        finesse: bool, ally_adjacent: bool,
    ) {
        let attack = Attack::new(to_hit, count, sides, bonus)
            .with_mode(RollMode::Advantage)
            .with_finesse_or_ranged(finesse)
            .with_ally_adjacent(ally_adjacent);
        let attack = with_sneak_attack(attack, sneak_count, sneak_sides);
        let defense = Defense::new(ac, 200);
        assert_samples_match_exact("advantage", seed, &attack, &defense);
    }

    /// Disadvantage cancels Sneak Attack outright - the gate returns `None`
    /// even with a qualifying weapon and an ally adjacent, so the attack that
    /// reaches `damage_pmf`/`sample_damage` never carries the rider at all,
    /// and the two paths agree on nothing more than the base weapon damage.
    #[test]
    fn sneak_attack_agrees_under_disadvantage_rolls(
        seed: u64,
        to_hit in -2i32..12, ac in 6i32..24,
        count in 1u32..4, sides in 2u32..9, bonus in -3i32..6,
        sneak_count in 1u32..5, sneak_sides in 2u32..9,
        finesse: bool, ally_adjacent: bool,
    ) {
        let attack = Attack::new(to_hit, count, sides, bonus)
            .with_mode(RollMode::Disadvantage)
            .with_finesse_or_ranged(finesse)
            .with_ally_adjacent(ally_adjacent);
        prop_assert!(
            sneak_attack(sneak_count, sneak_sides)
                .extra_damage_for(&attack, false)
                .is_none(),
            "disadvantage should cancel Sneak Attack even with a qualifying weapon and an ally adjacent"
        );
        let attack = with_sneak_attack(attack, sneak_count, sneak_sides);
        let defense = Defense::new(ac, 200);
        assert_samples_match_exact("disadvantage", seed, &attack, &defense);
    }

    /// The rider's own dice double on a crit exactly like the weapon's - not
    /// "the weapon's dice double and the rider stays flat," which lands close
    /// enough in total damage that a mean-only check could miss the
    /// difference. Forcing advantage keeps the crit rate at a fixed 39/400
    /// regardless of the dice pools generated, and forcing a qualifying
    /// weapon keeps the rider attached on every case, so the region of the
    /// distribution that is reachable only if the rider's dice also doubled
    /// shows up often enough in tens of thousands of samples to compare
    /// against its exact probability.
    #[test]
    fn crit_doubles_the_sneak_attack_dice_pool_not_just_the_weapon_dice(
        seed: u64,
        count in 1u32..4, sides in 2u32..9, bonus in 0i32..4,
        sneak_count in 1u32..5, sneak_sides in 2u32..9,
    ) {
        let attack = Attack::new(8, count, sides, bonus)
            .with_mode(RollMode::Advantage)
            .with_finesse_or_ranged(true);
        let attack = with_sneak_attack(attack, sneak_count, sneak_sides);
        // AC 10 with a +8 to-hit and advantage: almost everything but a
        // natural 1 hits, so both the hit and crit branches contribute.
        let defense = Defense::new(10, 500);

        let exact = damage_pmf(&attack, &defense);

        // The most damage possible if the weapon's dice doubled on a crit but
        // the rider's did not - the bug this test exists to catch.
        let undoubled_crit_cap =
            2 * (count * sides) as i32 + bonus + (sneak_count * sneak_sides) as i32;
        prop_assert!(
            exact.max() > undoubled_crit_cap,
            "exact max {} should exceed {undoubled_crit_cap}, the cap if the rider's dice never doubled on a crit",
            exact.max()
        );

        // How much probability mass sits strictly above that cap - reachable
        // only through a crit that also doubled the rider's dice.
        let want: f64 = ((undoubled_crit_cap + 1)..=exact.max())
            .map(|v| exact.prob(v))
            .sum();
        prop_assert!(
            want > 0.0,
            "the region above the un-doubled-rider cap should carry some probability"
        );

        let mut rng = Rng::new(seed);
        let mut above = 0usize;
        for _ in 0..SAMPLES {
            if sample_damage(&mut rng, &attack, &defense) > undoubled_crit_cap {
                above += 1;
            }
        }
        let got = above as f64 / SAMPLES as f64;
        let tol = tolerance(want, SAMPLES);
        prop_assert!(
            (got - want).abs() < tol,
            "P(damage > {undoubled_crit_cap}) sampled {got:.5}, exact {want:.5}, tolerance {tol:.5}"
        );
    }
}
