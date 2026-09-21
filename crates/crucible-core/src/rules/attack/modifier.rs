//! Ongoing modifiers to an attack roll - Bless, Bane, a flat magic bonus -
//! and how forced advantage and disadvantage combine.

use crate::prob::{Pmf, Rng};
use crate::rules::RollMode;

/// A modifier to an attack roll's resolution.
///
/// Every variant is a mechanism a whole family of abilities reduces to, not a
/// feature in itself: Bless is `BonusDice { count: 1, sides: 4 }`, Bane is
/// the same shape as a penalty, Steady Aim is `ForceAdvantage`, a +1 weapon
/// is `Flat(1)`. A list of these is carried alongside an attack rather than
/// a single optional one, so unrelated sources - Bless from one PC, a magic
/// weapon's bonus, Steady Aim - can all be active on the same roll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackModifier {
    /// Extra dice added to the roll's total - Bless's `+1d4`. Rolled
    /// alongside the d20 and added to whatever it takes to clear the AC; it
    /// never touches whether the *natural* roll was a 1 or a 20.
    BonusDice { count: u32, sides: u32 },
    /// Dice subtracted from the roll's total - Bane's `-1d4`. The mirror of
    /// [`AttackModifier::BonusDice`].
    PenaltyDice { count: u32, sides: u32 },
    /// A flat bonus or penalty to the roll's total, for anything that is not
    /// dice - most simply, a magic weapon.
    Flat(i32),
    /// Forces advantage on this roll regardless of the situational mode -
    /// Steady Aim - subject to the same cancellation rule as everything
    /// else: one source of disadvantage anywhere still cancels it.
    ForceAdvantage,
    /// Forces disadvantage on this roll regardless of the situational mode.
    ForceDisadvantage,
}

/// The 5e stacking rule for advantage and disadvantage applied to a modifier
/// list: however many sources of each are present, they collapse to one flag
/// apiece, and one of each cancels to a flat roll. This is the same rule
/// `sim::fight` applies to a set of active conditions, applied here to a set
/// of modifiers instead.
pub fn resolve_mode(base: RollMode, modifiers: &[AttackModifier]) -> RollMode {
    let mut advantage = base == RollMode::Advantage;
    let mut disadvantage = base == RollMode::Disadvantage;
    for m in modifiers {
        match m {
            AttackModifier::ForceAdvantage => advantage = true,
            AttackModifier::ForceDisadvantage => disadvantage = true,
            _ => {}
        }
    }
    match (advantage, disadvantage) {
        (true, false) => RollMode::Advantage,
        (false, true) => RollMode::Disadvantage,
        _ => RollMode::Normal,
    }
}

/// Exact distribution of everything a modifier list adds to a roll's total,
/// besides the d20 itself - every [`AttackModifier::BonusDice`],
/// [`AttackModifier::PenaltyDice`] and [`AttackModifier::Flat`], combined by
/// convolution. Advantage and disadvantage do not appear here; they are
/// resolved separately by [`resolve_mode`].
pub(super) fn modifier_pmf(modifiers: &[AttackModifier]) -> Pmf {
    modifiers.iter().fold(Pmf::constant(0), |acc, m| match *m {
        AttackModifier::BonusDice { count, sides } => acc.convolve(&Pmf::pool(count, sides)),
        AttackModifier::PenaltyDice { count, sides } => {
            acc.convolve(&Pmf::pool(count, sides).map_values(|v| -v))
        }
        AttackModifier::Flat(n) => acc.offset(n),
        AttackModifier::ForceAdvantage | AttackModifier::ForceDisadvantage => acc,
    })
}

/// The sampled counterpart of [`modifier_pmf`].
pub(super) fn sample_modifier_bonus(rng: &mut Rng, modifiers: &[AttackModifier]) -> i32 {
    modifiers
        .iter()
        .map(|m| match *m {
            AttackModifier::BonusDice { count, sides } => (0..count).map(|_| rng.die(sides)).sum(),
            AttackModifier::PenaltyDice { count, sides } => {
                -(0..count).map(|_| rng.die(sides)).sum::<i32>()
            }
            AttackModifier::Flat(n) => n,
            AttackModifier::ForceAdvantage | AttackModifier::ForceDisadvantage => 0,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{damage_pmf, outcomes, sample_damage, Attack, DamageRider, Defense};

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-12
    }

    /// Bless adds `+1d4` to the roll, so hitting AC 15 with `+5` now succeeds
    /// on some rolls as low as 6 that used to miss - but a natural 1 still
    /// always misses and a natural 20 still always crits, bonus die or not.
    #[test]
    fn bless_widens_the_natural_rolls_that_hit() {
        let plain = outcomes(&Attack::new(5, 1, 8, 3), &Defense::new(15, 20));
        let blessed = outcomes(
            &Attack::new(5, 1, 8, 3)
                .with_attack_modifier(AttackModifier::BonusDice { count: 1, sides: 4 }),
            &Defense::new(15, 20),
        );
        assert!(close(blessed.miss + blessed.hit + blessed.crit, 1.0));
        assert!(
            blessed.hit + blessed.crit > plain.hit + plain.crit,
            "a +1d4 should only ever raise the chance to hit"
        );
        assert!(
            blessed.miss < plain.miss,
            "some rolls that used to miss should now clear the AC"
        );
        assert!(
            blessed.miss >= 1.0 / 20.0 - 1e-12,
            "a natural 1 still always misses under Bless"
        );
        assert!(
            close(blessed.crit, 1.0 / 20.0),
            "Bless does not change the crit chance, only whether a lower roll hits"
        );
    }

    /// Bane is the mirror: `-1d4` on the roll can only ever cost hits, never
    /// gain them, and a natural 20 still always crits.
    #[test]
    fn bane_narrows_the_natural_rolls_that_hit() {
        let plain = outcomes(&Attack::new(5, 1, 8, 3), &Defense::new(15, 20));
        let baned = outcomes(
            &Attack::new(5, 1, 8, 3)
                .with_attack_modifier(AttackModifier::PenaltyDice { count: 1, sides: 4 }),
            &Defense::new(15, 20),
        );
        assert!(close(baned.miss + baned.hit + baned.crit, 1.0));
        assert!(
            baned.hit + baned.crit < plain.hit + plain.crit,
            "a -1d4 should only ever lower the chance to hit"
        );
        assert!(
            close(baned.crit, 1.0 / 20.0),
            "Bane does not change the crit chance, a natural 20 always lands"
        );
    }

    /// Several unrelated modifiers on one roll compose rather than replace
    /// each other: Bless and a +1 weapon both apply, and Bless plus Bane
    /// still leaves the flat bonus in effect.
    #[test]
    fn several_attack_modifiers_stack() {
        let one = outcomes(
            &Attack::new(5, 1, 8, 3)
                .with_attack_modifier(AttackModifier::BonusDice { count: 1, sides: 4 }),
            &Defense::new(15, 20),
        );
        let both = outcomes(
            &Attack::new(5, 1, 8, 3)
                .with_attack_modifier(AttackModifier::BonusDice { count: 1, sides: 4 })
                .with_attack_modifier(AttackModifier::Flat(1)),
            &Defense::new(15, 20),
        );
        assert!(
            both.hit + both.crit > one.hit + one.crit,
            "stacking a flat +1 on top of Bless should hit more, not the same"
        );

        // Bless and Bane together: the dice do not cancel algebraically (a
        // +1d4 and a -1d4 are not the same distribution as +0), but the flat
        // bonus underneath is still exactly what it was.
        let cancelled = outcomes(
            &Attack::new(5, 1, 8, 3)
                .with_attack_modifier(AttackModifier::BonusDice { count: 1, sides: 4 })
                .with_attack_modifier(AttackModifier::PenaltyDice { count: 1, sides: 4 }),
            &Defense::new(15, 20),
        );
        assert!(close(cancelled.miss + cancelled.hit + cancelled.crit, 1.0));
    }

    /// Forced advantage and forced disadvantage from unrelated sources cancel
    /// to a flat roll, the same rule the duel layer applies to conditions.
    #[test]
    fn forced_advantage_and_disadvantage_cancel() {
        assert_eq!(
            resolve_mode(
                RollMode::Normal,
                &[
                    AttackModifier::ForceAdvantage,
                    AttackModifier::ForceDisadvantage,
                ],
            ),
            RollMode::Normal
        );
        assert_eq!(
            resolve_mode(RollMode::Disadvantage, &[AttackModifier::ForceAdvantage]),
            RollMode::Normal
        );
        assert_eq!(
            resolve_mode(RollMode::Normal, &[AttackModifier::ForceAdvantage]),
            RollMode::Advantage
        );
    }

    #[test]
    fn sampled_bless_bane_and_a_rider_agree_with_the_exact_path() {
        let defense = Defense::new(15, 40);
        let cases = [
            (
                "bless",
                Attack::new(5, 1, 8, 3)
                    .with_attack_modifier(AttackModifier::BonusDice { count: 1, sides: 4 }),
            ),
            (
                "bane",
                Attack::new(5, 1, 8, 3)
                    .with_attack_modifier(AttackModifier::PenaltyDice { count: 1, sides: 4 }),
            ),
            (
                "sneak attack rider",
                Attack::new(5, 1, 8, 3).with_damage_rider(DamageRider::new(2, 6)),
            ),
            (
                "bless and a rider together",
                Attack::new(5, 1, 8, 3)
                    .with_attack_modifier(AttackModifier::BonusDice { count: 1, sides: 4 })
                    .with_damage_rider(DamageRider::new(2, 6)),
            ),
        ];
        for (seed, (name, attack)) in cases.into_iter().enumerate() {
            let exact = damage_pmf(&attack, &defense);
            let (lo, hi) = (exact.min(), exact.max());
            let mut rng = Rng::new(seed as u64 + 500);
            let n = 100_000;
            let mut counts = vec![0usize; (hi - lo + 1) as usize];
            for _ in 0..n {
                let d = sample_damage(&mut rng, &attack, &defense);
                assert!(
                    d >= lo && d <= hi,
                    "{name}: sampled {d} outside {lo}..={hi}"
                );
                counts[(d - lo) as usize] += 1;
            }
            for (i, &c) in counts.iter().enumerate() {
                let value = lo + i as i32;
                let want = exact.prob(value);
                let got = c as f64 / n as f64;
                let tol = 5.0 * (want * (1.0 - want) / n as f64).sqrt() + 1e-4;
                assert!(
                    (got - want).abs() < tol,
                    "{name}: P(damage = {value}) sampled {got:.5}, exact {want:.5}, tol {tol:.5}"
                );
            }
        }
    }
}
