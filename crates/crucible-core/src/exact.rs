//! Closed-form answers for scenarios simple enough to have them.
//!
//! This module exists to be the reference the sampled engine is measured
//! against. A full encounter - reactions, movement, concentration, an
//! adversary - is not going to be solvable this way, which is the entire
//! reason the Monte Carlo path exists. But the pieces underneath it are, and
//! an engine whose sampled attack resolution disagrees with its own exact
//! attack resolution is broken in a way no amount of rollouts will reveal.
//!
//! See `tests/exact_vs_sampled.rs` for the agreement test itself.

use crate::dice::Pmf;

/// `out[k]` is the probability the target is down within `k` attacks.
///
/// Forward iteration of the distribution over remaining HP, with zero as an
/// absorbing state. Overkill is discarded at the zero boundary rather than
/// carried, which is the rule the sampled path follows too - excess damage on
/// a killing blow goes nowhere.
///
/// `out[0]` is zero for a living target, and the vector has `attacks + 1`
/// entries so it can be indexed directly by attack count.
pub fn kill_curve(hp: i32, damage: &Pmf, attacks: usize) -> Vec<f64> {
    if hp <= 0 {
        return vec![1.0; attacks + 1];
    }
    let hp = hp as usize;

    let terms: Vec<(i32, f64)> = damage.iter().filter(|&(_, p)| p > 0.0).collect();
    let mut state = vec![0.0; hp + 1];
    state[hp] = 1.0;

    let mut out = Vec::with_capacity(attacks + 1);
    out.push(0.0);

    let mut next = vec![0.0; hp + 1];
    for _ in 0..attacks {
        next.iter_mut().for_each(|v| *v = 0.0);
        next[0] = state[0];
        for (h, &mass) in state.iter().enumerate().take(hp + 1).skip(1) {
            if mass == 0.0 {
                continue;
            }
            for &(d, p) in &terms {
                let remaining = (h as i32 - d.max(0)).max(0) as usize;
                next[remaining] += mass * p;
            }
        }
        std::mem::swap(&mut state, &mut next);
        out.push(state[0]);
    }
    out
}

/// Expected number of attacks to drop the target, or `None` if it cannot be.
///
/// Attacks dealing zero - misses, and hits absorbed entirely by resistance -
/// are absorbed into the recurrence rather than simulated. Writing `f(h)` for
/// the expected attacks from `h` HP:
///
/// ```text
/// f(h) = 1 + P(0)·f(h) + Σ_{d>0} P(d)·f(max(h-d, 0))
/// ```
///
/// which rearranges to divide out the zero-damage self-loop:
///
/// ```text
/// f(h) = [ 1 + Σ_{d>0} P(d)·f(max(h-d, 0)) ] / (1 - P(0))
/// ```
///
/// Every term on the right refers to strictly lower HP, so a single ascending
/// pass is enough. When `P(0) = 1` the expectation is infinite, and this
/// returns `None` rather than dividing by zero and reporting an infinity that
/// would propagate silently into an average.
pub fn expected_attacks_to_kill(hp: i32, damage: &Pmf) -> Option<f64> {
    if hp <= 0 {
        return Some(0.0);
    }
    let p_zero: f64 = damage.iter().filter(|&(d, _)| d <= 0).map(|(_, p)| p).sum();
    if p_zero >= 1.0 - 1e-12 {
        return None;
    }

    let hp = hp as usize;
    let mut f = vec![0.0; hp + 1];
    for h in 1..=hp {
        let mut acc = 1.0;
        for (d, p) in damage.iter() {
            if d <= 0 || p == 0.0 {
                continue;
            }
            let remaining = (h as i32 - d).max(0) as usize;
            acc += p * f[remaining];
        }
        f[h] = acc / (1.0 - p_zero);
    }
    Some(f[hp])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn deterministic_damage_kills_on_the_expected_attack() {
        let dmg = Pmf::constant(5);
        let curve = kill_curve(20, &dmg, 6);
        assert!(close(curve[3], 0.0), "15 damage leaves 5 HP");
        assert!(close(curve[4], 1.0), "20 damage is exactly lethal");
        assert!(close(expected_attacks_to_kill(20, &dmg).unwrap(), 4.0));
    }

    /// All-or-nothing damage makes attacks-to-kill geometric, so the curve is
    /// `1 - (1-p)^k` and the mean is `1/p`. Both are checked against the
    /// closed form rather than against a previous run.
    #[test]
    fn all_or_nothing_damage_is_geometric() {
        let p = 0.5;
        let dmg = Pmf::from_pairs([(0, 1.0 - p), (10, p)]);
        let curve = kill_curve(10, &dmg, 10);
        for (k, &got) in curve.iter().enumerate() {
            let want = 1.0 - (1.0 - p).powi(k as i32);
            assert!(close(got, want), "at k={k}: {got} vs {want}");
        }
        assert!(close(expected_attacks_to_kill(10, &dmg).unwrap(), 1.0 / p));
    }

    #[test]
    fn overkill_is_discarded_not_carried() {
        let curve = kill_curve(10, &Pmf::constant(1000), 3);
        assert!(close(curve[1], 1.0));
        assert!(close(curve[2], 1.0), "already dead stays dead");
    }

    #[test]
    fn the_curve_is_monotone_and_bounded() {
        let dmg = Pmf::from_pairs([(0, 0.4), (3, 0.35), (7, 0.25)]);
        let curve = kill_curve(25, &dmg, 40);
        for w in curve.windows(2) {
            assert!(w[1] >= w[0] - 1e-15, "curve went backwards");
        }
        assert!(curve.iter().all(|&p| (-1e-15..=1.0 + 1e-15).contains(&p)));
        assert!(
            *curve.last().unwrap() > 0.99,
            "should converge to certainty"
        );
    }

    #[test]
    fn damage_that_can_never_land_has_no_expectation() {
        assert_eq!(expected_attacks_to_kill(10, &Pmf::constant(0)), None);
    }

    /// The mean implied by the curve must match the closed-form expectation.
    /// Two independent derivations of the same quantity.
    #[test]
    fn the_curve_and_the_expectation_agree() {
        let dmg = Pmf::from_pairs([(0, 0.45), (4, 0.4), (9, 0.15)]);
        let curve = kill_curve(30, &dmg, 400);
        // E[N] = Σ_{k>=0} P(N > k)
        let mean_from_curve: f64 = curve.iter().map(|&dead| 1.0 - dead).sum();
        let expected = expected_attacks_to_kill(30, &dmg).unwrap();
        assert!(
            (mean_from_curve - expected).abs() < 1e-6,
            "{mean_from_curve} vs {expected}"
        );
    }
}
