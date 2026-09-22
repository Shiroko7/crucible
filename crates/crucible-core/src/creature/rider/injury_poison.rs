//! Injury poisons: a single-dose weapon coating whose forcing save leaves
//! the target worse at one kind of saving throw.

use crate::creature::{Creature, Rider};
use crate::prob::Rng;
use crate::rules::{save_with_mode, Ability, Condition, Duration, RollMode};

impl Rider {
    /// The `(ability, dc, debuffed_ability, duration)` this rider carries, or
    /// `None` if it is not [`Rider::InjuryPoison`].
    pub fn injury_poison(&self) -> Option<(Ability, i32, Ability, Duration)> {
        match self {
            Rider::InjuryPoison {
                ability,
                dc,
                debuffed_ability,
                duration,
                ..
            } => Some((*ability, *dc, *debuffed_ability, *duration)),
            _ => None,
        }
    }

    /// Every condition an [`Rider::InjuryPoison`] leaves on a failed save:
    /// the [`Condition::SaveDisadvantage`] burden, plus its extra
    /// `condition` if it has one. Empty for any other rider.
    pub fn injury_poison_conditions(&self) -> Vec<(Condition, Duration)> {
        match self {
            Rider::InjuryPoison {
                debuffed_ability,
                condition,
                duration,
                ..
            } => {
                let mut out = vec![(Condition::SaveDisadvantage(*debuffed_ability), *duration)];
                out.extend(condition.map(|c| (c, *duration)));
                out
            }
            _ => Vec::new(),
        }
    }
}

/// Resolve an [`Rider::InjuryPoison`] coating's forcing save - the hit that
/// uses the coating up: `target` rolls its `ability` save at
/// [`RollMode::Normal`] against `dc`, via [`save_with_mode`]. `None` if
/// `rider` is not [`Rider::InjuryPoison`] at all.
///
/// Only the forcing save is resolved here. Applying the resulting
/// disadvantage-on-saves debuff for its stated `duration` is bookkeeping for
/// whoever tracks conditions and effects over time to do with the `false`
/// this returns on a failure - the same division [`Rider::SaveOrCondition`]'s
/// own `duration` field already leaves to `sim::fight` rather than resolving
/// itself.
pub fn injury_poison_forcing_save(rng: &mut Rng, target: &Creature, rider: &Rider) -> Option<bool> {
    let (ability, dc, ..) = rider.injury_poison()?;
    Some(save_with_mode(
        rng,
        RollMode::Normal,
        target.save(ability),
        dc,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::save_probability_with_mode;

    fn sneak_attack() -> Rider {
        Rider::ConditionalExtraDamage {
            dice_count: 4,
            dice_sides: 6,
            once_per_turn: true,
        }
    }

    fn injury_poison() -> Rider {
        Rider::InjuryPoison {
            ability: Ability::Con,
            dc: 13,
            debuffed_ability: Ability::Str,
            condition: None,
            duration: Duration::ApplierTurn,
        }
    }

    #[test]
    fn injury_poison_reads_back_its_own_parameters() {
        assert_eq!(
            injury_poison().injury_poison(),
            Some((Ability::Con, 13, Ability::Str, Duration::ApplierTurn))
        );
        assert_eq!(sneak_attack().injury_poison(), None);
    }

    #[test]
    fn injury_poison_forcing_save_returns_none_for_an_unrelated_rider() {
        let target = Creature::new("target", 12, 20);
        let mut rng = Rng::new(7700);
        assert_eq!(
            injury_poison_forcing_save(&mut rng, &target, &sneak_attack()),
            None
        );
    }

    /// The forcing save is a plain, flat roll - no advantage or disadvantage
    /// of its own - so its pass rate must land on the same closed form every
    /// other flat save in this crate already agrees with.
    #[test]
    fn injury_poison_forcing_save_agrees_with_a_flat_save_chance() {
        let mut target = Creature::new("target", 12, 30);
        target.saves[Ability::Con.index()] = 2;
        let rider = injury_poison();

        let exact = save_probability_with_mode(RollMode::Normal, 2, 13);
        let mut rng = Rng::new(7701);
        let n = 200_000;
        let successes = (0..n)
            .filter(|_| {
                injury_poison_forcing_save(&mut rng, &target, &rider).expect("this is InjuryPoison")
            })
            .count();
        let got = successes as f64 / f64::from(n);
        let tol = 5.0 * (exact * (1.0 - exact) / f64::from(n)).sqrt() + 1e-4;
        assert!(
            (got - exact).abs() < tol,
            "sampled {got:.5}, exact {exact:.5}, tol {tol:.5}"
        );
    }

    /// The debuff half: once the poison has taken hold, the target's
    /// burdened saves roll with Disadvantage rather than a flat d20 - worse
    /// than normal, and checked exact-vs-sampled the same way every
    /// probability in this crate is.
    #[test]
    fn a_disadvantaged_save_is_worse_than_a_flat_one_and_agrees_with_the_exact_path() {
        let bonus = 3;
        let dc = 15;
        let flat = save_probability_with_mode(RollMode::Normal, bonus, dc);
        let disadvantaged = save_probability_with_mode(RollMode::Disadvantage, bonus, dc);
        assert!(
            disadvantaged < flat,
            "disadvantage on a burdened save must be worse than a flat roll"
        );

        let mut rng = Rng::new(7702);
        let n = 200_000;
        let successes = (0..n)
            .filter(|_| save_with_mode(&mut rng, RollMode::Disadvantage, bonus, dc))
            .count();
        let got = successes as f64 / f64::from(n);
        let tol = 5.0 * (disadvantaged * (1.0 - disadvantaged) / f64::from(n)).sqrt() + 1e-4;
        assert!(
            (got - disadvantaged).abs() < tol,
            "sampled {got:.5}, exact {disadvantaged:.5}, tol {tol:.5}"
        );
    }
}
