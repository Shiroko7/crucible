//! Saving throws: ongoing modifiers to them, and their odds under a
//! [`RollMode`], exactly and by sampling.

use crate::prob::{Pmf, Rng};
use crate::rules::RollMode;

/// A modifier to a saving throw's resolution - the sibling of
/// [`crate::rules::AttackModifier`] for the d20 a saving throw rolls rather than an
/// attack. Bless's `+1d4` applies to every saving throw a blessed creature
/// makes, not only its attack rolls, and Bane's `-1d4` is the same shape as
/// a penalty; both need somewhere to plug in that is not `AttackModifier`,
/// because a saving throw has no advantage/disadvantage concept in this
/// engine (see `sim::duel`) and dragging `ForceAdvantage`/`ForceDisadvantage`
/// along as meaningless variants here would be worse than a second enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveModifier {
    /// Extra dice added to the roll's total - Bless's `+1d4`.
    BonusDice { count: u32, sides: u32 },
    /// Dice subtracted from the roll's total - Bane's `-1d4`.
    PenaltyDice { count: u32, sides: u32 },
}

/// Exact distribution of everything a save modifier list adds to a d20's
/// total - the save equivalent of [`modifier_pmf`].
fn save_modifier_pmf(modifiers: &[SaveModifier]) -> Pmf {
    modifiers.iter().fold(Pmf::constant(0), |acc, m| match *m {
        SaveModifier::BonusDice { count, sides } => acc.convolve(&Pmf::pool(count, sides)),
        SaveModifier::PenaltyDice { count, sides } => {
            acc.convolve(&Pmf::pool(count, sides).map_values(|v| -v))
        }
    })
}

/// The sampled counterpart of [`save_modifier_pmf`].
pub(crate) fn sample_save_modifier_bonus(rng: &mut Rng, modifiers: &[SaveModifier]) -> i32 {
    modifiers
        .iter()
        .map(|m| match *m {
            SaveModifier::BonusDice { count, sides } => (0..count).map(|_| rng.die(sides)).sum(),
            SaveModifier::PenaltyDice { count, sides } => {
                -(0..count).map(|_| rng.die(sides)).sum::<i32>()
            }
        })
        .sum()
}

/// P(a saving throw succeeds), exact: a flat d20 plus `save_bonus` against
/// `dc`, with a [`SaveModifier`] list folded in by convolution. Saving
/// throws have no natural-1/natural-20 rule in 5e, unlike attack rolls (see
/// [`crate::rules::hit_outcomes_with`]), so every face of the d20 is treated the same -
/// there is no separate crit/fumble carve-out to take out of the loop.
pub fn save_success_chance(save_bonus: i32, dc: i32, modifiers: &[SaveModifier]) -> f64 {
    let bonus = save_modifier_pmf(modifiers);
    let mut success = 0.0;
    for roll in 1..=20 {
        success += (1.0 / 20.0) * bonus.at_least(dc - roll - save_bonus);
    }
    success
}

/// The sampled counterpart of [`save_success_chance`]: one saving throw,
/// with the same modifier list rolled alongside the d20 exactly as at the
/// table, whether or not the natural roll turns out to make it irrelevant.
pub fn sample_save_with(
    rng: &mut Rng,
    save_bonus: i32,
    dc: i32,
    modifiers: &[SaveModifier],
) -> bool {
    let roll = rng.die(20);
    let bonus = sample_save_modifier_bonus(rng, modifiers);
    roll + save_bonus + bonus >= dc
}

/// P(a roll under `mode` is at least `needed`), read off
/// [`RollMode::distribution`] rather than rederived.
pub(crate) fn probability_at_least(mode: RollMode, needed: i32) -> f64 {
    mode.distribution()
        .iter()
        .enumerate()
        .map(|(i, &p)| if i as i32 + 1 >= needed { p } else { 0.0 })
        .sum()
}

/// A saving throw rolled under `mode` rather than a flat d20 - the same
/// [`RollMode`] an attack roll already rolls under
/// ([`crate::rules::sample_hit_with`]), generalised to saves. 5e's
/// saving throws have no natural-1/natural-20 override the way attack rolls
/// do, so unlike an attack roll this is exactly `mode.roll(rng) + bonus >=
/// dc` with no exception carved out.
///
/// The generic mechanism [`crate::creature::Rider::InjuryPoison`]'s debuff half uses once it
/// has taken hold: a target under it rolls its burdened ability's saves with
/// [`RollMode::Disadvantage`] through this same function rather than a
/// special case.
pub fn save_with_mode(rng: &mut Rng, mode: RollMode, bonus: i32, dc: i32) -> bool {
    mode.roll(rng) + bonus >= dc
}

/// The exact counterpart of [`save_with_mode`], read off
/// [`RollMode::distribution`] like [`probability_at_least`] already is for
/// [`crate::creature::save_success_probability`].
pub fn save_probability_with_mode(mode: RollMode, bonus: i32, dc: i32) -> f64 {
    probability_at_least(mode, dc - bonus)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-12
    }

    /// Bless's `+1d4` on a saving throw should only ever raise the chance of
    /// success, the same property `bless_widens_the_natural_rolls_that_hit`
    /// checks for the attack-roll sibling.
    #[test]
    fn bless_widens_the_saves_that_succeed() {
        let plain = save_success_chance(2, 15, &[]);
        let blessed = save_success_chance(2, 15, &[SaveModifier::BonusDice { count: 1, sides: 4 }]);
        assert!(
            blessed > plain,
            "a +1d4 should only ever raise the chance to save"
        );
        assert!((0.0..=1.0).contains(&blessed));
    }

    /// Bane is the mirror: `-1d4` on a save can only ever cost successes.
    #[test]
    fn bane_narrows_the_saves_that_succeed() {
        let plain = save_success_chance(2, 15, &[]);
        let baned = save_success_chance(2, 15, &[SaveModifier::PenaltyDice { count: 1, sides: 4 }]);
        assert!(
            baned < plain,
            "a -1d4 should only ever lower the chance to save"
        );
        assert!((0.0..=1.0).contains(&baned));
    }

    /// A save with no modifiers at all is a flat d20: exactly the classic
    /// "beat DC 15 with a +2" hand count.
    #[test]
    fn a_plain_save_matches_a_hand_count() {
        // +2 vs DC 15 needs a 13: 13..20 succeed, 8 faces out of 20.
        assert!(close(save_success_chance(2, 15, &[]), 8.0 / 20.0));
    }

    #[test]
    fn sampled_save_modifiers_agree_with_the_exact_path() {
        let cases = [
            ("no modifier", vec![]),
            (
                "bless: +1d4",
                vec![SaveModifier::BonusDice { count: 1, sides: 4 }],
            ),
            (
                "bane: -1d4",
                vec![SaveModifier::PenaltyDice { count: 1, sides: 4 }],
            ),
            (
                "bless and bane together",
                vec![
                    SaveModifier::BonusDice { count: 1, sides: 4 },
                    SaveModifier::PenaltyDice { count: 1, sides: 4 },
                ],
            ),
        ];
        let (save_bonus, dc) = (3, 14);
        for (seed, (name, modifiers)) in cases.into_iter().enumerate() {
            let exact = save_success_chance(save_bonus, dc, &modifiers);
            let mut rng = Rng::new(seed as u64 + 700);
            let n = 200_000;
            let mut successes = 0usize;
            for _ in 0..n {
                if sample_save_with(&mut rng, save_bonus, dc, &modifiers) {
                    successes += 1;
                }
            }
            let got = successes as f64 / n as f64;
            let tol = 5.0 * (exact * (1.0 - exact) / n as f64).sqrt() + 1e-4;
            assert!(
                (got - exact).abs() < tol,
                "{name}: P(save succeeds) sampled {got:.5}, exact {exact:.5}, tol {tol:.5}"
            );
        }
    }
}
