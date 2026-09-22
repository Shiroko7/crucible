//! Ability and skill checks: 5e's third kind of d20 roll, after attack rolls
//! ([`crate::rules::Attack`]) and saving throws
//! ([`crate::creature::SaveEffect`]).
//!
//! Nothing here rolled one before this module existed - `Condition::Poisoned`'s
//! disadvantage on its own ability checks and `Condition::Blinded`'s
//! auto-failed sight-based checks both cite this exact gap in their doc
//! comments (see [`crate::rules::Condition`]). Rather than build the
//! full subsystem those conditions would eventually want - skills, tools,
//! contested checks, a positioning-dependent DC - this is the minimal
//! mechanism Reliable Talent actually needs: a d20 roll, under a
//! [`RollMode`], with an optional floor under the raw roll.
//!
//! Like a saving throw and unlike an attack roll, a check has no natural-1 or
//! natural-20 rule - as [`crate::creature::SaveEffect::failure_chance`]
//! already notes for saves, this is a flat count of faces, not three special
//! cases.

use crate::prob::dice::Pmf;
use crate::prob::rng::Rng;
use crate::rules::RollMode;

/// One ability, skill or tool check against a DC.
///
/// `floor` is Reliable Talent's whole mechanism: a raw roll below it is
/// treated as it instead - a floor, not a reroll, so it can only ever raise
/// what was rolled, never lower it. Set it from
/// [`crate::creature::Creature::check_floor`] when the check being
/// made is one the creature is proficient in; leave it `None` otherwise,
/// including for a creature that has no such feature at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckRoll {
    /// Ability modifier plus proficiency bonus, already summed.
    pub bonus: i32,
    pub dc: i32,
    pub mode: RollMode,
    pub floor: Option<i32>,
}

impl CheckRoll {
    pub fn new(bonus: i32, dc: i32) -> Self {
        Self {
            bonus,
            dc,
            mode: RollMode::Normal,
            floor: None,
        }
    }

    pub fn with_mode(mut self, mode: RollMode) -> Self {
        self.mode = mode;
        self
    }

    /// Reliable Talent, or anything shaped like it: a raw d20 roll below
    /// `floor` counts as `floor` instead.
    pub fn with_floor(mut self, floor: i32) -> Self {
        self.floor = Some(floor);
        self
    }

    /// Exact distribution of the raw d20 roll, after `mode` and any floor -
    /// before `bonus` is added. Collapsing the low faces onto the floor is
    /// exactly [`Pmf::floor_at`], the same operation damage flooring at zero
    /// already uses.
    fn raw_d20_pmf(&self) -> Pmf {
        let dist = self.mode.distribution();
        let raw = Pmf::from_pairs((1..=20i32).map(|face| (face, dist[(face - 1) as usize])));
        match self.floor {
            Some(floor) => raw.floor_at(floor),
            None => raw,
        }
    }

    /// P(this check succeeds). Like a saving throw, and unlike an attack
    /// roll, a natural 1 or 20 does nothing special here.
    pub fn success_chance(&self) -> f64 {
        self.raw_d20_pmf().at_least(self.dc - self.bonus)
    }

    /// The sampled counterpart of [`CheckRoll::success_chance`]. Returns the
    /// total rolled (after any floor) and whether it met the DC; must be
    /// distributed according to `success_chance`.
    pub fn sample(&self, rng: &mut Rng) -> (i32, bool) {
        let raw = self.mode.roll(rng);
        let raw = match self.floor {
            Some(floor) => raw.max(floor),
            None => raw,
        };
        let total = raw + self.bonus;
        (total, total >= self.dc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn without_a_floor_a_check_is_a_flat_count_of_faces() {
        // +0 vs dc 15 needs at least a 15: rolls 15..=20, six faces.
        let check = CheckRoll::new(0, 15);
        assert!(close(check.success_chance(), 6.0 / 20.0));
    }

    #[test]
    fn a_floor_collapses_the_low_rolls_mass_onto_it() {
        let check = CheckRoll::new(0, 100).with_floor(10); // dc unreachable either way
        let pmf = check.raw_d20_pmf();
        assert!(close(pmf.total(), 1.0));
        assert_eq!(pmf.min(), 10, "nothing below the floor should remain");
        // Rolls 1..=10 (ten faces) all collapse onto 10.
        assert!(close(pmf.prob(10), 10.0 / 20.0));
        for face in 11..=20 {
            assert!(close(pmf.prob(face), 1.0 / 20.0), "face {face}");
        }
    }

    /// Reliable Talent's headline case: a proficient rogue with +0 can never
    /// fail a dc 10 check once every roll below 10 becomes a 10.
    #[test]
    fn a_floor_of_ten_against_dc_ten_always_succeeds() {
        let check = CheckRoll::new(0, 10).with_floor(10);
        assert!(close(check.success_chance(), 1.0));
    }

    #[test]
    fn a_floor_never_makes_a_check_worse_at_any_dc() {
        for dc in 1..=25 {
            let plain = CheckRoll::new(0, dc);
            let floored = CheckRoll::new(0, dc).with_floor(10);
            assert!(
                floored.success_chance() >= plain.success_chance() - 1e-12,
                "dc {dc}: floored {} should never beat plain {}",
                floored.success_chance(),
                plain.success_chance()
            );
        }
        // And strictly better at a dc the floor actually rescues.
        let plain = CheckRoll::new(0, 10);
        let floored = CheckRoll::new(0, 10).with_floor(10);
        assert!(floored.success_chance() > plain.success_chance());
    }

    #[test]
    fn a_floor_never_lowers_a_roll_that_already_cleared_it() {
        // A raw 15 stays 15 whether or not Reliable Talent is in play - the
        // floor only ever raises what would otherwise be below it.
        let plain = CheckRoll::new(5, 20);
        let floored = CheckRoll::new(5, 20).with_floor(10);
        assert!(close(plain.success_chance(), floored.success_chance()));
    }

    #[test]
    fn sampled_checks_agree_with_the_exact_success_chance() {
        let cases = [
            CheckRoll::new(3, 15),
            CheckRoll::new(0, 12).with_floor(10),
            CheckRoll::new(-2, 18).with_mode(RollMode::Advantage),
            CheckRoll::new(5, 10)
                .with_mode(RollMode::Disadvantage)
                .with_floor(10),
        ];
        for (seed, check) in cases.into_iter().enumerate() {
            let want = check.success_chance();
            let mut rng = Rng::new(seed as u64 + 900);
            let n = 100_000;
            let mut successes = 0usize;
            for _ in 0..n {
                let (_, ok) = check.sample(&mut rng);
                if ok {
                    successes += 1;
                }
            }
            let got = successes as f64 / n as f64;
            let tol = 5.0 * (want * (1.0 - want) / n as f64).sqrt() + 1e-4;
            assert!(
                (got - want).abs() < tol,
                "{check:?}: sampled {got:.5}, exact {want:.5}, tol {tol:.5}"
            );
        }
    }
}
