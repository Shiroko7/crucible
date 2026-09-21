//! Advantage and disadvantage: how a d20 is rolled, for attacks, saves and
//! checks alike.

use crate::prob::Rng;

/// How the d20 is rolled. Advantage and disadvantage are distributions over
/// the *final* value, so "natural 20" means the kept die showed 20.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RollMode {
    Normal,
    Advantage,
    Disadvantage,
}

impl RollMode {
    /// P(final roll = k) for k in 1..=20, indexed from zero.
    ///
    /// With advantage the maximum of two dice is at most `k` exactly when both
    /// are, so `P(max <= k) = (k/20)^2` and the mass at `k` is the difference
    /// of consecutive squares - `(2k-1)/400`. Disadvantage is the mirror. This
    /// is why advantage is worth about +3.3 at the middle of the range and
    /// almost nothing at the ends.
    pub fn distribution(self) -> [f64; 20] {
        let mut out = [0.0; 20];
        for (i, slot) in out.iter_mut().enumerate() {
            let k = i as f64 + 1.0;
            *slot = match self {
                RollMode::Normal => 1.0 / 20.0,
                RollMode::Advantage => (2.0 * k - 1.0) / 400.0,
                RollMode::Disadvantage => (41.0 - 2.0 * k) / 400.0,
            };
        }
        out
    }

    /// The sampled counterpart of [`RollMode::distribution`].
    pub fn roll(self, rng: &mut Rng) -> i32 {
        match self {
            RollMode::Normal => rng.die(20),
            RollMode::Advantage => rng.die(20).max(rng.die(20)),
            RollMode::Disadvantage => rng.die(20).min(rng.die(20)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-12
    }

    #[test]
    fn every_roll_mode_is_a_distribution() {
        for mode in [
            RollMode::Normal,
            RollMode::Advantage,
            RollMode::Disadvantage,
        ] {
            let sum: f64 = mode.distribution().iter().sum();
            assert!(close(sum, 1.0), "{mode:?} sums to {sum}");
        }
    }

    /// The classic numbers: 39/400 to crit with advantage, 1/400 to fumble.
    #[test]
    fn advantage_and_disadvantage_mirror_each_other() {
        let adv = RollMode::Advantage.distribution();
        let dis = RollMode::Disadvantage.distribution();
        assert!(close(adv[19], 39.0 / 400.0));
        assert!(close(adv[0], 1.0 / 400.0));
        assert!(close(dis[19], 1.0 / 400.0));
        assert!(close(dis[0], 39.0 / 400.0));
        for k in 0..20 {
            assert!(close(adv[k], dis[19 - k]), "asymmetry at {k}");
        }
    }
}
