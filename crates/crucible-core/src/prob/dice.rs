//! Exact probability mass functions over integer outcomes.
//!
//! This is the half of the engine that is *obviously* correct. Anything simple
//! enough to solve in closed form gets solved here by convolution, and the
//! sampled path in [`crate::rules::sample_damage`] is then required to
//! converge to it. That agreement is the project's load-bearing test: the
//! sampled path is the one that scales to a full encounter, and this is the
//! one that can be checked.
//!
//! Outcomes are `i32` because damage, modifiers and saving-throw totals are
//! all small signed integers, and because a modifier can legitimately push a
//! total negative before it is floored.

/// A distribution over consecutive integers.
///
/// `probs[i]` is the probability of `min + i`. Storing a dense window rather
/// than a map keeps convolution a pair of nested loops over contiguous memory,
/// which is what it needs to be - `12d6` is 61 outcomes, not a hash table.
#[derive(Debug, Clone, PartialEq)]
pub struct Pmf {
    min: i32,
    probs: Vec<f64>,
}

impl Pmf {
    /// A value that always happens.
    pub fn constant(value: i32) -> Self {
        Self {
            min: value,
            probs: vec![1.0],
        }
    }

    /// One fair die, `1..=sides`.
    pub fn die(sides: u32) -> Self {
        assert!(sides > 0, "a d0 has no faces");
        Self {
            min: 1,
            probs: vec![1.0 / f64::from(sides); sides as usize],
        }
    }

    /// `count`d`sides`. An empty pool is a constant zero, which is what makes
    /// the crit path below work without a special case.
    pub fn pool(count: u32, sides: u32) -> Self {
        let mut acc = Self::constant(0);
        let one = Self::die(sides);
        for _ in 0..count {
            acc = acc.convolve(&one);
        }
        acc
    }

    /// Build from arbitrary (value, probability) pairs, summing duplicates.
    ///
    /// The entry point for any transformation that is not a shift - halving
    /// for resistance, flooring at zero - where several inputs collapse onto
    /// one output.
    pub fn from_pairs<I: IntoIterator<Item = (i32, f64)>>(pairs: I) -> Self {
        let pairs: Vec<(i32, f64)> = pairs.into_iter().filter(|&(_, p)| p > 0.0).collect();
        if pairs.is_empty() {
            return Self::constant(0);
        }
        let min = pairs.iter().map(|&(v, _)| v).min().unwrap();
        let max = pairs.iter().map(|&(v, _)| v).max().unwrap();
        let mut probs = vec![0.0; (max - min + 1) as usize];
        for (v, p) in pairs {
            probs[(v - min) as usize] += p;
        }
        Self { min, probs }
    }

    /// The distribution of the sum of two independent rolls.
    pub fn convolve(&self, other: &Self) -> Self {
        let mut probs = vec![0.0; self.probs.len() + other.probs.len() - 1];
        for (i, &a) in self.probs.iter().enumerate() {
            if a == 0.0 {
                continue;
            }
            for (j, &b) in other.probs.iter().enumerate() {
                probs[i + j] += a * b;
            }
        }
        Self {
            min: self.min + other.min,
            probs,
        }
    }

    /// Shift every outcome by a constant. Cheaper than convolving with one.
    pub fn offset(&self, by: i32) -> Self {
        Self {
            min: self.min + by,
            probs: self.probs.clone(),
        }
    }

    /// Weighted combination of distributions - "miss, hit, or crit".
    ///
    /// Weights are expected to sum to one; a debug build says so if they do
    /// not, since a silently sub-stochastic mixture would quietly deflate
    /// every probability downstream of it.
    pub fn mixture(parts: &[(f64, Self)]) -> Self {
        debug_assert!(
            (parts.iter().map(|&(w, _)| w).sum::<f64>() - 1.0).abs() < 1e-9,
            "mixture weights must sum to 1"
        );
        Self::from_pairs(parts.iter().flat_map(|(w, pmf)| {
            let w = *w;
            pmf.iter().map(move |(v, p)| (v, w * p))
        }))
    }

    /// Apply a function to every outcome, merging any that collide.
    pub fn map_values<F: Fn(i32) -> i32>(&self, f: F) -> Self {
        Self::from_pairs(self.iter().map(|(v, p)| (f(v), p)))
    }

    /// Raise every outcome to at least `floor`.
    ///
    /// Damage clamps at zero; a target's remaining HP clamps at zero. Both
    /// collapse mass onto the floor, which is why this is a `map_values` and
    /// not a shift.
    pub fn floor_at(&self, floor: i32) -> Self {
        self.map_values(|v| v.max(floor))
    }

    pub fn min(&self) -> i32 {
        self.min
    }

    pub fn max(&self) -> i32 {
        self.min + self.probs.len() as i32 - 1
    }

    /// Probability of exactly `value`; zero outside the support.
    pub fn prob(&self, value: i32) -> f64 {
        let i = value - self.min;
        if i < 0 || i as usize >= self.probs.len() {
            0.0
        } else {
            self.probs[i as usize]
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (i32, f64)> + '_ {
        self.probs
            .iter()
            .enumerate()
            .map(move |(i, &p)| (self.min + i as i32, p))
    }

    /// Should be 1.0. Exposed so tests can assert it rather than assume it.
    pub fn total(&self) -> f64 {
        self.probs.iter().sum()
    }

    pub fn mean(&self) -> f64 {
        self.iter().map(|(v, p)| f64::from(v) * p).sum()
    }

    pub fn variance(&self) -> f64 {
        let mu = self.mean();
        self.iter()
            .map(|(v, p)| p * (f64::from(v) - mu).powi(2))
            .sum()
    }

    /// P(X >= value).
    pub fn at_least(&self, value: i32) -> f64 {
        self.iter()
            .filter(|&(v, _)| v >= value)
            .map(|(_, p)| p)
            .sum()
    }

    /// P(X <= value).
    pub fn at_most(&self, value: i32) -> f64 {
        self.iter()
            .filter(|&(v, _)| v <= value)
            .map(|(_, p)| p)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-12
    }

    #[test]
    fn a_die_is_uniform_over_its_faces() {
        let d6 = Pmf::die(6);
        assert_eq!((d6.min(), d6.max()), (1, 6));
        for v in 1..=6 {
            assert!(close(d6.prob(v), 1.0 / 6.0));
        }
        assert!(close(d6.total(), 1.0));
    }

    /// The textbook 2d6 triangle. If convolution is wrong, this is wrong in a
    /// way that is immediately visible.
    #[test]
    fn two_d6_is_the_familiar_triangle() {
        let p = Pmf::pool(2, 6);
        assert_eq!((p.min(), p.max()), (2, 12));
        assert!(close(p.prob(7), 6.0 / 36.0));
        assert!(close(p.prob(2), 1.0 / 36.0));
        assert!(close(p.prob(12), 1.0 / 36.0));
        assert!(close(p.mean(), 7.0));
    }

    #[test]
    fn an_empty_pool_is_a_constant_zero() {
        let p = Pmf::pool(0, 8);
        assert_eq!((p.min(), p.max()), (0, 0));
        assert!(close(p.prob(0), 1.0));
    }

    /// n(s+1)/2, the closed form, for a spread of pools.
    #[test]
    fn pool_means_match_the_closed_form() {
        for (n, s) in [(1u32, 4u32), (3, 6), (8, 8), (2, 10), (12, 6)] {
            let expected = f64::from(n) * (f64::from(s) + 1.0) / 2.0;
            assert!(
                close(Pmf::pool(n, s).mean(), expected),
                "{n}d{s} mean should be {expected}"
            );
        }
    }

    #[test]
    fn flooring_collapses_mass_onto_the_floor() {
        let p = Pmf::die(6).offset(-4).floor_at(0); // -3..2, floored
        assert!(close(p.prob(0), 4.0 / 6.0), "1,2,3,4 all become 0");
        assert!(close(p.prob(1), 1.0 / 6.0));
        assert!(close(p.prob(2), 1.0 / 6.0));
        assert!(close(p.total(), 1.0));
    }

    #[test]
    fn halving_merges_pairs_of_outcomes() {
        let p = Pmf::die(6).map_values(|v| v / 2); // 0,1,1,2,2,3
        assert!(close(p.prob(0), 1.0 / 6.0));
        assert!(close(p.prob(1), 2.0 / 6.0));
        assert!(close(p.prob(2), 2.0 / 6.0));
        assert!(close(p.prob(3), 1.0 / 6.0));
    }

    #[test]
    fn a_mixture_weights_its_parts() {
        let p = Pmf::mixture(&[(0.25, Pmf::constant(0)), (0.75, Pmf::constant(10))]);
        assert!(close(p.prob(0), 0.25));
        assert!(close(p.prob(10), 0.75));
        assert!(close(p.mean(), 7.5));
    }

    #[test]
    fn tails_agree_with_each_other() {
        let p = Pmf::pool(2, 6);
        for v in 1..=13 {
            assert!(
                close(p.at_least(v) + p.at_most(v - 1), 1.0),
                "tails at {v} do not partition the mass"
            );
        }
    }
}
