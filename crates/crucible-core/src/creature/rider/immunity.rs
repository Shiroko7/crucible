//! Punching through immunity: the attacker-side trait that softens a
//! target's damage immunity to resistance and its condition immunity to a
//! save rolled with advantage.

use crate::creature::{Creature, Rider};
use crate::prob::Rng;
use crate::rules::{probability_at_least, Ability, Condition, DamageKind, RollMode};

impl Rider {
    /// Does this rider downgrade a target's immunity to `kind` (Immune to
    /// Resistant) for damage this creature deals? See
    /// [`crate::creature::Creature::reduction_from`], which is the usual
    /// way this actually gets asked - it scans a whole rider list rather
    /// than one rider at a time.
    pub fn downgrades_damage_immunity(&self, kind: DamageKind) -> bool {
        match self {
            Rider::DowngradeImmunity {
                damage: Some(d), ..
            } => *d == kind,
            _ => false,
        }
    }

    /// Does this rider downgrade a target's immunity to `condition` (an
    /// auto-succeeding save to one rolled with Advantage) for a condition
    /// this creature inflicts? See [`saving_throw_against_condition`].
    pub fn downgrades_condition_immunity(&self, condition: Condition) -> bool {
        match self {
            Rider::DowngradeImmunity {
                condition: Some(c), ..
            } => *c == condition,
            _ => false,
        }
    }

    /// Does this rider ignore a target's resistance to `kind` (Resistant to
    /// Normal) for damage this creature deals? See
    /// [`crate::creature::Creature::reduction_from`].
    pub fn ignores_damage_resistance(&self, kind: DamageKind) -> bool {
        match self {
            Rider::IgnoreResistance { kinds } => kinds.contains(&kind),
            _ => false,
        }
    }
}

/// Roll a saving throw `target` makes against `condition`, which `attacker`
/// is trying to inflict.
///
/// A target with no immunity to `condition` rolls exactly like any other
/// saving throw: one d20 plus its own bonus against `dc`. A target that
/// *is* immune to `condition` (see
/// [`crate::creature::Creature::immune_to_condition`]) is ordinarily
/// unaffected outright, no roll at all, and this returns `true`
/// unconditionally, unless `attacker` carries a [`Rider::DowngradeImmunity`]
/// naming this exact `condition`. Then the free pass is gone: the target
/// still has to make the save, just with Advantage instead of
/// auto-succeeding.
pub fn saving_throw_against_condition(
    rng: &mut Rng,
    target: &Creature,
    attacker: &Creature,
    ability: Ability,
    dc: i32,
    condition: Condition,
) -> bool {
    let bonus = target.save(ability);
    if target.immune_to_condition(condition) {
        if !attacker
            .riders
            .iter()
            .any(|r| r.downgrades_condition_immunity(condition))
        {
            return true;
        }
        return RollMode::Advantage.roll(rng) + bonus >= dc;
    }
    RollMode::Normal.roll(rng) + bonus >= dc
}

/// The exact counterpart of [`saving_throw_against_condition`]: `target`'s
/// probability of succeeding, closed-form rather than sampled - the same
/// "everything twice" split every other rule in this crate is held to.
pub fn save_success_probability(
    target: &Creature,
    attacker: &Creature,
    ability: Ability,
    dc: i32,
    condition: Condition,
) -> f64 {
    let bonus = target.save(ability);
    if target.immune_to_condition(condition) {
        if !attacker
            .riders
            .iter()
            .any(|r| r.downgrades_condition_immunity(condition))
        {
            return 1.0;
        }
        return probability_at_least(RollMode::Advantage, dc - bonus);
    }
    probability_at_least(RollMode::Normal, dc - bonus)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{damage_pmf, sample_damage, Attack, Defense, Reduction};

    #[test]
    fn downgrade_immunity_rider_reports_only_what_it_names() {
        let both = Rider::DowngradeImmunity {
            damage: Some(DamageKind::Poison),
            condition: Some(Condition::Poisoned),
        };
        assert!(both.downgrades_damage_immunity(DamageKind::Poison));
        assert!(!both.downgrades_damage_immunity(DamageKind::Fire));
        assert!(both.downgrades_condition_immunity(Condition::Poisoned));
        assert!(!both.downgrades_condition_immunity(Condition::Stunned));

        let damage_only = Rider::DowngradeImmunity {
            damage: Some(DamageKind::Fire),
            condition: None,
        };
        assert!(damage_only.downgrades_damage_immunity(DamageKind::Fire));
        assert!(!damage_only.downgrades_condition_immunity(Condition::Poisoned));

        let condition_only = Rider::DowngradeImmunity {
            damage: None,
            condition: Some(Condition::Stunned),
        };
        assert!(!condition_only.downgrades_damage_immunity(DamageKind::Fire));
        assert!(condition_only.downgrades_condition_immunity(Condition::Stunned));

        // An unrelated rider never answers yes to either question - the same
        // "not mistaken for a different marker" check `cunning_strike_dc`'s
        // own test makes.
        let unrelated = Rider::AlwaysSucceed {
            uses: 3,
            ability: None,
            reaction: false,
        };
        assert!(!unrelated.downgrades_damage_immunity(DamageKind::Poison));
        assert!(!unrelated.downgrades_condition_immunity(Condition::Poisoned));
    }

    /// The damage half: a target normally immune to a damage type instead
    /// takes half damage from an attacker carrying the downgrade trait, and
    /// full immunity is unaffected for every other attacker against that
    /// same target - the attacker-scoping the rider promises.
    #[test]
    fn damage_downgrade_softens_immunity_to_resistance_for_this_attacker_only() {
        let mut target = Creature::new("golem", 15, 60);
        target
            .reductions
            .push((DamageKind::Poison, Reduction::Immune));

        let plain_attacker = Creature::new("fighter", 15, 40);
        let downgrading_attacker =
            Creature::new("rogue", 15, 40).with_rider(Rider::DowngradeImmunity {
                damage: Some(DamageKind::Poison),
                condition: None,
            });

        assert_eq!(
            target.reduction_from(DamageKind::Poison, &plain_attacker),
            Reduction::Immune,
            "an attacker without the trait still sees full immunity"
        );
        assert_eq!(
            target.reduction_from(DamageKind::Poison, &downgrading_attacker),
            Reduction::Resistant,
            "the trait-carrying attacker softens immune to resistant"
        );
        // Scoped to the trait, not the damage type in general: this same
        // attacker sees an unrelated damage type's immunity untouched.
        target
            .reductions
            .push((DamageKind::Cold, Reduction::Immune));
        assert_eq!(
            target.reduction_from(DamageKind::Cold, &downgrading_attacker),
            Reduction::Immune,
            "the trait only names poison, so cold immunity is untouched"
        );
    }

    /// The exact-vs-sampled agreement every rule in this crate is held to,
    /// applied to the reduction the downgrade trait actually produces: fold
    /// `reduction_from`'s result into the same `Attack`/`Defense` machinery
    /// [`cunning_strike_spends_from_the_same_pool_sneak_attack_would_roll`]
    /// already uses, so the mechanism is checked at the same level a
    /// standalone rider mechanism always is here, not merely asserted.
    #[test]
    fn damage_downgrade_reduction_agrees_with_the_exact_path() {
        let mut target = Creature::new("golem", 12, 60);
        target
            .reductions
            .push((DamageKind::Poison, Reduction::Immune));
        let downgrading_attacker =
            Creature::new("rogue", 15, 40).with_rider(Rider::DowngradeImmunity {
                damage: Some(DamageKind::Poison),
                condition: None,
            });
        let plain_attacker = Creature::new("fighter", 15, 40);

        let cases = [
            (
                "downgraded to resistant: half damage gets through",
                target.reduction_from(DamageKind::Poison, &downgrading_attacker),
            ),
            (
                "no trait: still fully immune",
                target.reduction_from(DamageKind::Poison, &plain_attacker),
            ),
        ];
        let mut means = Vec::new();
        for (seed, (name, reduction)) in cases.into_iter().enumerate() {
            let attack = Attack::new(6, 2, 8, 4);
            let defense = Defense::new(target.ac, target.hp).with_reduction(reduction);
            let exact = damage_pmf(&attack, &defense);
            let (lo, hi) = (exact.min(), exact.max());
            let mut rng = Rng::new(seed as u64 + 3200);
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
            means.push((name, exact.mean()));
        }
        // And the two cases actually differ: proof the downgrade is doing
        // something rather than both silently computing zero.
        assert!(means[0].1 > 0.0, "{}", means[0].0);
        assert_eq!(means[1].1, 0.0, "{}", means[1].0);
    }

    /// The condition half: a target normally immune to a condition is
    /// ordinarily unaffected outright (no roll, always saved) - but an
    /// attacker carrying the downgrade trait takes that free pass away, and
    /// the target instead rolls with Advantage rather than auto-succeeding.
    #[test]
    fn condition_downgrade_rolls_with_advantage_instead_of_auto_succeeding() {
        let mut immune_target = Creature::new("golem", 12, 60);
        immune_target.condition_immunities.push(Condition::Poisoned);
        // A save bonus of 0 against a DC of 15 needs a 15+ on the die - low
        // enough that Advantage clearly is not the same as auto-succeeding.
        let dc = 15;

        let plain_attacker = Creature::new("fighter", 15, 40);
        let downgrading_attacker =
            Creature::new("rogue", 15, 40).with_rider(Rider::DowngradeImmunity {
                damage: None,
                condition: Some(Condition::Poisoned),
            });

        // Without the trait: unaffected outright, deterministically, however
        // the dice would have landed.
        assert_eq!(
            save_success_probability(
                &immune_target,
                &plain_attacker,
                Ability::Con,
                dc,
                Condition::Poisoned
            ),
            1.0
        );
        let mut rng = Rng::new(4100);
        for _ in 0..1000 {
            assert!(saving_throw_against_condition(
                &mut rng,
                &immune_target,
                &plain_attacker,
                Ability::Con,
                dc,
                Condition::Poisoned
            ));
        }

        // With the trait: the free pass is gone. The save is now rolled with
        // Advantage - strictly better than a flat d20, but not certain -
        // which is the whole difference between "downgraded" and "immune".
        let flat_p = probability_at_least(RollMode::Normal, dc);
        let advantage_p = save_success_probability(
            &immune_target,
            &downgrading_attacker,
            Ability::Con,
            dc,
            Condition::Poisoned,
        );
        assert!(advantage_p > flat_p, "Advantage must beat a flat roll");
        assert!(advantage_p < 1.0, "still not a free pass");

        // Exact-vs-sampled agreement, the same Bernoulli-proportion check
        // every probability in this crate is held to.
        let mut rng = Rng::new(4200);
        let n = 200_000;
        let successes = (0..n)
            .filter(|_| {
                saving_throw_against_condition(
                    &mut rng,
                    &immune_target,
                    &downgrading_attacker,
                    Ability::Con,
                    dc,
                    Condition::Poisoned,
                )
            })
            .count();
        let got = successes as f64 / f64::from(n);
        let tol = 5.0 * (advantage_p * (1.0 - advantage_p) / f64::from(n)).sqrt() + 1e-4;
        assert!(
            (got - advantage_p).abs() < tol,
            "sampled {got:.5}, exact {advantage_p:.5}, tol {tol:.5}"
        );

        // A target with no relevant immunity is untouched by any of this,
        // trait or no trait: same probability either way.
        let mut mundane_target = Creature::new("bandit", 12, 20);
        mundane_target.saves[Ability::Con.index()] = 0;
        let with_trait = save_success_probability(
            &mundane_target,
            &downgrading_attacker,
            Ability::Con,
            dc,
            Condition::Poisoned,
        );
        let without_trait = save_success_probability(
            &mundane_target,
            &plain_attacker,
            Ability::Con,
            dc,
            Condition::Poisoned,
        );
        assert_eq!(with_trait, without_trait);
        assert_eq!(with_trait, flat_p);
    }

    #[test]
    fn ignoring_resistance_turns_resistant_into_normal() {
        use crate::rules::Reduction;

        let mut resistant_target = Creature::new("fire_elemental", 13, 100);
        resistant_target
            .reductions
            .push((DamageKind::Fire, Reduction::Resistant));

        let plain_attacker = Creature::new("wizard", 12, 30);
        let mut adept_attacker = Creature::new("elemental_adept", 12, 30);
        adept_attacker.riders.push(Rider::IgnoreResistance {
            kinds: vec![DamageKind::Fire],
        });

        assert_eq!(
            resistant_target.reduction_from(DamageKind::Fire, &plain_attacker),
            Reduction::Resistant
        );
        assert_eq!(
            resistant_target.reduction_from(DamageKind::Fire, &adept_attacker),
            Reduction::Normal
        );

        // Does not affect other types
        resistant_target
            .reductions
            .push((DamageKind::Cold, Reduction::Resistant));
        assert_eq!(
            resistant_target.reduction_from(DamageKind::Cold, &adept_attacker),
            Reduction::Resistant
        );

        // Works together with immunity downgrade: Immune -> Resistant -> Normal
        let mut immune_target = Creature::new("iron_golem", 17, 200);
        immune_target
            .reductions
            .push((DamageKind::Fire, Reduction::Immune));

        let mut piercing_attacker = Creature::new("penetrator", 12, 30);
        piercing_attacker.riders.push(Rider::DowngradeImmunity {
            damage: Some(DamageKind::Fire),
            condition: None,
        });
        piercing_attacker.riders.push(Rider::IgnoreResistance {
            kinds: vec![DamageKind::Fire],
        });

        assert_eq!(
            immune_target.reduction_from(DamageKind::Fire, &piercing_attacker),
            Reduction::Normal
        );
    }
}
