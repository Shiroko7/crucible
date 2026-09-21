//! Property-based tests.
//!
//! The unit tests check the cases someone thought of. These state what must
//! hold for *every* input and let `proptest` go looking for a counterexample,
//! shrinking anything it finds to the smallest case that still breaks. The
//! difference matters most at the boundaries nobody writes a test for by hand:
//! AC 30 against a +0 attacker, a damage modifier that drives the whole pool
//! below zero, a target with one hit point.
//!
//! Ranges are kept small deliberately. These are correctness properties, and a
//! bug that needs 40d12 to appear would appear at 4d12 too.

use crucible_core::prob::{expected_attacks_to_kill, kill_curve, Pmf, Rng};
use crucible_core::rules::{
    damage_pmf, outcomes, sample_damage, Attack, Defense, Reduction, RollMode,
};
use proptest::prelude::*;
fn any_mode() -> impl Strategy<Value = RollMode> {
    prop_oneof![
        Just(RollMode::Normal),
        Just(RollMode::Advantage),
        Just(RollMode::Disadvantage),
    ]
}

fn any_reduction() -> impl Strategy<Value = Reduction> {
    prop_oneof![
        Just(Reduction::Normal),
        Just(Reduction::Resistant),
        Just(Reduction::Vulnerable),
    ]
}

proptest! {
    /// Mass is conserved, and the support is exactly `n..=n*s`.
    #[test]
    fn a_dice_pool_is_a_distribution(count in 0u32..8, sides in 1u32..13) {
        let p = Pmf::pool(count, sides);
        prop_assert!((p.total() - 1.0).abs() < 1e-9, "total was {}", p.total());
        if count == 0 {
            prop_assert_eq!((p.min(), p.max()), (0, 0));
        } else {
            prop_assert_eq!(p.min(), count as i32);
            prop_assert_eq!(p.max(), (count * sides) as i32);
        }
    }

    /// n(s+1)/2, the closed form, for every pool in range.
    #[test]
    fn pool_means_match_the_closed_form(count in 1u32..8, sides in 1u32..13) {
        let want = f64::from(count) * (f64::from(sides) + 1.0) / 2.0;
        let got = Pmf::pool(count, sides).mean();
        prop_assert!((got - want).abs() < 1e-9, "{}d{} mean {} vs {}", count, sides, got, want);
    }

    /// Addition of independent variables commutes, so convolution must too.
    #[test]
    fn convolution_commutes(a in 1u32..5, b in 1u32..5, s in 2u32..9) {
        let x = Pmf::pool(a, s).convolve(&Pmf::pool(b, s));
        let y = Pmf::pool(b, s).convolve(&Pmf::pool(a, s));
        prop_assert_eq!(x.min(), y.min());
        for (v, p) in x.iter() {
            prop_assert!((p - y.prob(v)).abs() < 1e-12, "differ at {}", v);
        }
    }

    /// Flooring never loses mass and never leaves anything below the floor -
    /// the property that catches a `map_values` that drops colliding outcomes
    /// instead of summing them.
    #[test]
    fn flooring_conserves_mass(count in 1u32..6, sides in 2u32..13, offset in -30i32..10) {
        let p = Pmf::pool(count, sides).offset(offset).floor_at(0);
        prop_assert!((p.total() - 1.0).abs() < 1e-9, "total was {}", p.total());
        prop_assert!(p.min() >= 0, "min was {}", p.min());
    }

    /// Miss, hit and crit are exhaustive and mutually exclusive, at every AC,
    /// every bonus, and every roll mode.
    #[test]
    fn attack_outcomes_partition_the_probability(
        to_hit in -5i32..20, ac in 1i32..35, mode in any_mode(),
    ) {
        let o = outcomes(&Attack::new(to_hit, 1, 8, 0).with_mode(mode), &Defense::new(ac, 10));
        prop_assert!(o.miss >= -1e-12 && o.hit >= -1e-12 && o.crit >= -1e-12);
        prop_assert!((o.miss + o.hit + o.crit - 1.0).abs() < 1e-12);
        // A natural 20 always hits and a natural 1 always misses, so neither
        // certainty is ever reachable however lopsided the numbers get.
        prop_assert!(o.crit > 0.0, "a crit must always be possible");
        prop_assert!(o.miss > 0.0, "a miss must always be possible");
    }

    /// Advantage is never worse than normal, which is never worse than
    /// disadvantage. Obvious, and precisely the thing an off-by-one in the
    /// `(2k-1)/400` derivation would break.
    #[test]
    fn advantage_never_hurts(to_hit in -5i32..20, ac in 1i32..35) {
        let at = |mode| {
            let a = Attack::new(to_hit, 1, 8, 0).with_mode(mode);
            let o = outcomes(&a, &Defense::new(ac, 10));
            o.hit + o.crit
        };
        let (dis, normal, adv) = (
            at(RollMode::Disadvantage),
            at(RollMode::Normal),
            at(RollMode::Advantage),
        );
        prop_assert!(adv >= normal - 1e-12, "advantage {} < normal {}", adv, normal);
        prop_assert!(normal >= dis - 1e-12, "normal {} < disadvantage {}", normal, dis);
    }

    #[test]
    fn damage_is_a_distribution_and_never_negative(
        to_hit in -5i32..15, ac in 5i32..30,
        count in 1u32..6, sides in 2u32..13, bonus in -20i32..15,
        mode in any_mode(), reduction in any_reduction(),
    ) {
        let attack = Attack::new(to_hit, count, sides, bonus).with_mode(mode);
        let defense = Defense::new(ac, 20).with_reduction(reduction);
        let pmf = damage_pmf(&attack, &defense);
        prop_assert!((pmf.total() - 1.0).abs() < 1e-9, "total was {}", pmf.total());
        prop_assert!(pmf.min() >= 0, "min damage was {}", pmf.min());
        prop_assert!(pmf.prob(0) > 0.0, "a miss deals zero, so zero is always possible");
    }

    /// Whatever the sampler produces must be something the exact distribution
    /// says is possible. Cheap, and it catches a sampled path that applies the
    /// rules in a different order from the exact one.
    #[test]
    fn sampled_damage_stays_inside_the_exact_support(
        seed: u64,
        to_hit in -5i32..15, ac in 5i32..30,
        count in 1u32..6, sides in 2u32..13, bonus in -20i32..15,
        mode in any_mode(), reduction in any_reduction(),
    ) {
        let attack = Attack::new(to_hit, count, sides, bonus).with_mode(mode);
        let defense = Defense::new(ac, 20).with_reduction(reduction);
        let pmf = damage_pmf(&attack, &defense);

        let mut rng = Rng::new(seed);
        for _ in 0..200 {
            let d = sample_damage(&mut rng, &attack, &defense);
            prop_assert!(d >= pmf.min() && d <= pmf.max(), "sampled {} outside support", d);
            prop_assert!(pmf.prob(d) > 0.0, "sampled {} has exact probability zero", d);
        }
    }

    /// A kill curve is a CDF: bounded, and never decreasing. Dead targets do
    /// not come back.
    #[test]
    fn the_kill_curve_is_a_cdf(
        hp in 1i32..60,
        to_hit in 0i32..12, ac in 8i32..22,
        count in 1u32..5, sides in 2u32..13, bonus in -4i32..8,
    ) {
        let attack = Attack::new(to_hit, count, sides, bonus);
        let defense = Defense::new(ac, hp);
        let curve = kill_curve(hp, &damage_pmf(&attack, &defense), 25);

        prop_assert!(curve[0] == 0.0, "nothing dies before the first attack");
        for w in curve.windows(2) {
            prop_assert!(w[1] >= w[0] - 1e-12, "curve decreased: {} then {}", w[0], w[1]);
        }
        prop_assert!(curve.iter().all(|&p| (-1e-12..=1.0 + 1e-12).contains(&p)));
    }

    /// More hit points never means a quicker kill.
    #[test]
    fn expected_attacks_grows_with_hit_points(
        hp in 1i32..40, extra in 1i32..30,
        to_hit in 0i32..12, ac in 8i32..22, count in 1u32..5, sides in 2u32..13,
    ) {
        let attack = Attack::new(to_hit, count, sides, 2);
        let pmf = damage_pmf(&attack, &Defense::new(ac, hp));
        let a = expected_attacks_to_kill(hp, &pmf).unwrap();
        let b = expected_attacks_to_kill(hp + extra, &pmf).unwrap();
        prop_assert!(b >= a - 1e-9, "{} HP took {}, {} HP took {}", hp, a, hp + extra, b);
    }

    /// Resistance can only ever reduce, vulnerability can only ever increase.
    #[test]
    fn reduction_moves_damage_in_the_expected_direction(damage in 0i32..500) {
        prop_assert!(Reduction::Resistant.apply(damage) <= damage);
        prop_assert!(Reduction::Vulnerable.apply(damage) >= damage);
        prop_assert!(Reduction::Resistant.apply(damage) >= 0);
        prop_assert_eq!(Reduction::Normal.apply(damage), damage);
    }
}
