//! The agreement test.
//!
//! Everything this project will eventually report comes out of the sampled
//! path, because that is the only one that scales to a real encounter. The
//! sampled path is also the one that can be quietly wrong: a biased `d20`, a
//! critical hit that doubles the modifier, resistance applied before the
//! damage floor instead of after. None of those produce an implausible number.
//! They produce a slightly wrong one, forever.
//!
//! So every rule is also implemented in closed form, and the two are required
//! to agree here.
//!
//! Tolerances are derived, not tuned. For a proportion estimated from `n`
//! samples the standard error is `sqrt(p(1-p)/n)`, and five of those is a
//! threshold a correct implementation passes essentially always while a
//! genuinely biased one fails. Picking a round number that happened to pass
//! would defeat the purpose of the test.
//!
//! The seeds are fixed, so a failure here is reproducible rather than a flake
//! to be re-run until it goes away.

use crucible_core::prob::{expected_attacks_to_kill, kill_curve, Rng};
use crucible_core::rules::{
    damage_pmf, sample_attacks_to_kill, sample_damage, Attack, AttackModifier, DamageRider,
    Defense, Reduction, RollMode,
};
const DAMAGE_SAMPLES: usize = 200_000;
const KILL_SAMPLES: usize = 100_000;

/// Five standard errors, with a small floor so that outcomes of probability
/// zero are not held to a tolerance of exactly zero.
fn tolerance(p: f64, n: usize) -> f64 {
    5.0 * (p * (1.0 - p) / n as f64).sqrt() + 1e-4
}

/// The cases worth covering are the ones where the two implementations could
/// plausibly diverge, not a spread of arbitrary numbers.
fn cases() -> Vec<(&'static str, Attack, Defense)> {
    vec![
        (
            "ordinary longsword",
            Attack::new(5, 1, 8, 3),
            Defense::new(15, 30),
        ),
        (
            "advantage, where the d20 distribution is not uniform",
            Attack::new(7, 2, 6, 4).with_mode(RollMode::Advantage),
            Defense::new(16, 40),
        ),
        (
            "disadvantage against heavy armour",
            Attack::new(4, 1, 12, 2).with_mode(RollMode::Disadvantage),
            Defense::new(19, 35),
        ),
        (
            "resistance, where halving interacts with the damage floor",
            Attack::new(8, 3, 6, 5),
            Defense::new(14, 50).with_reduction(Reduction::Resistant),
        ),
        (
            "vulnerability against a big pool",
            Attack::new(6, 8, 6, 0),
            Defense::new(17, 120).with_reduction(Reduction::Vulnerable),
        ),
        (
            "unhittable except on a natural 20",
            Attack::new(0, 1, 6, 1),
            Defense::new(40, 20),
        ),
        (
            "a penalty large enough to floor ordinary hits at zero",
            Attack::new(10, 1, 4, -3),
            Defense::new(12, 15),
        ),
        (
            "bless: +1d4 to the attack roll",
            Attack::new(5, 1, 8, 3)
                .with_attack_modifier(AttackModifier::BonusDice { count: 1, sides: 4 }),
            Defense::new(15, 30),
        ),
        (
            "bane: -1d4 to the attack roll",
            Attack::new(7, 2, 6, 4)
                .with_attack_modifier(AttackModifier::PenaltyDice { count: 1, sides: 4 }),
            Defense::new(16, 40),
        ),
        (
            "a damage rider's extra dice, conditionally appended on a hit",
            Attack::new(6, 1, 6, 2).with_damage_rider(DamageRider::new(2, 6)),
            Defense::new(14, 30),
        ),
        (
            "bless, a magic weapon's flat bonus, and a damage rider all active at once",
            Attack::new(4, 1, 8, 2)
                .with_attack_modifier(AttackModifier::BonusDice { count: 1, sides: 4 })
                .with_attack_modifier(AttackModifier::Flat(1))
                .with_damage_rider(DamageRider::new(3, 6).with_bonus(2)),
            Defense::new(15, 40).with_reduction(Reduction::Resistant),
        ),
    ]
}

/// Compares the sampled damage histogram against the exact PMF, outcome by
/// outcome. Stronger than comparing means: a mixture that puts the right
/// average in the wrong places passes a mean check and fails this.
#[test]
fn sampled_damage_matches_the_exact_distribution() {
    for (seed, (name, attack, defense)) in cases().into_iter().enumerate() {
        let exact = damage_pmf(&attack, &defense);
        let (lo, hi) = (exact.min(), exact.max());
        assert!(lo >= 0, "{name}: damage should never be negative");

        let mut rng = Rng::new(seed as u64 + 1);
        let mut counts = vec![0usize; (hi - lo + 1) as usize];
        for _ in 0..DAMAGE_SAMPLES {
            let d = sample_damage(&mut rng, &attack, &defense);
            assert!(
                d >= lo && d <= hi,
                "{name}: sampled {d} outside the exact support {lo}..={hi}"
            );
            counts[(d - lo) as usize] += 1;
        }

        for (i, &c) in counts.iter().enumerate() {
            let value = lo + i as i32;
            let want = exact.prob(value);
            let got = c as f64 / DAMAGE_SAMPLES as f64;
            let tol = tolerance(want, DAMAGE_SAMPLES);
            assert!(
                (got - want).abs() < tol,
                "{name}: P(damage = {value}) sampled {got:.5}, exact {want:.5}, tolerance {tol:.5}"
            );
        }

        let sampled_mean: f64 = counts
            .iter()
            .enumerate()
            .map(|(i, &c)| f64::from(lo + i as i32) * c as f64 / DAMAGE_SAMPLES as f64)
            .sum();
        let spread = (exact.variance() / DAMAGE_SAMPLES as f64).sqrt();
        assert!(
            (sampled_mean - exact.mean()).abs() < 5.0 * spread + 1e-6,
            "{name}: mean damage sampled {sampled_mean:.4}, exact {:.4}",
            exact.mean()
        );
    }
}

/// The same agreement one level up: how many attacks it takes to drop the
/// target. This exercises the absorbing zero-HP boundary and the discarding of
/// overkill, neither of which the single-attack test can reach.
#[test]
fn sampled_kill_counts_match_the_exact_curve() {
    const CAP: u32 = 60;

    for (seed, (name, attack, defense)) in cases().into_iter().enumerate() {
        let exact = kill_curve(defense.hp, &damage_pmf(&attack, &defense), CAP as usize);

        let mut rng = Rng::new(seed as u64 + 1_000);
        let mut killed_on = vec![0usize; CAP as usize + 1];
        let mut survived = 0usize;
        for _ in 0..KILL_SAMPLES {
            match sample_attacks_to_kill(&mut rng, &attack, &defense, CAP) {
                Some(n) => killed_on[n as usize] += 1,
                None => survived += 1,
            }
        }

        // Compare the cumulative curves, which is what the exact side produces.
        let mut cumulative = 0usize;
        for k in 1..=CAP as usize {
            cumulative += killed_on[k];
            let got = cumulative as f64 / KILL_SAMPLES as f64;
            let want = exact[k];
            let tol = tolerance(want, KILL_SAMPLES);
            assert!(
                (got - want).abs() < tol,
                "{name}: P(dead within {k}) sampled {got:.5}, exact {want:.5}, tolerance {tol:.5}"
            );
        }

        let want_survivors = 1.0 - exact[CAP as usize];
        let got_survivors = survived as f64 / KILL_SAMPLES as f64;
        assert!(
            (got_survivors - want_survivors).abs() < tolerance(want_survivors, KILL_SAMPLES),
            "{name}: survival past {CAP} attacks sampled {got_survivors:.5}, exact {want_survivors:.5}"
        );
    }
}

/// The closed-form expectation against the sampled average. Only meaningful
/// where the target reliably dies inside the cap, so the truncation does not
/// bias the average downward.
#[test]
fn expected_attacks_matches_the_sampled_average() {
    const CAP: u32 = 400;

    for (seed, (name, attack, defense)) in cases().into_iter().enumerate() {
        let pmf = damage_pmf(&attack, &defense);
        let Some(expected) = expected_attacks_to_kill(defense.hp, &pmf) else {
            continue; // cannot be killed at all; covered by a unit test
        };

        let mut rng = Rng::new(seed as u64 + 2_000);
        let samples = 40_000;
        let mut total = 0u64;
        for _ in 0..samples {
            let n = sample_attacks_to_kill(&mut rng, &attack, &defense, CAP)
                .unwrap_or_else(|| panic!("{name}: survived {CAP} attacks, raise the cap"));
            total += u64::from(n);
        }
        let got = total as f64 / f64::from(samples);

        // Attacks-to-kill is roughly geometric, so its standard deviation is
        // on the order of its mean; scale the tolerance accordingly rather
        // than assuming a tight distribution.
        let tol = 5.0 * expected / (f64::from(samples)).sqrt() + 0.02;
        assert!(
            (got - expected).abs() < tol,
            "{name}: sampled {got:.4} attacks, exact {expected:.4}, tolerance {tol:.4}"
        );
    }
}
