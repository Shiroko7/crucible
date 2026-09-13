//! Combatants, their moves, and the triggered modifiers hanging off them.

pub mod action;
pub mod combatant;
pub mod damage;
pub mod rider;
pub mod types;

pub use action::{Effect, Move, SaveEffect, Strike, Uses};
pub use combatant::Creature;
pub use damage::{DamageKind, DamageRoll};
pub use rider::Rider;
pub use types::{Ability, Condition, Cost, Duration, Resource};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::combat::Reduction;

    fn dummy(ac: i32) -> Creature {
        Creature::new("dummy", ac, 100)
    }

    #[test]
    fn a_strike_damage_distribution_is_a_distribution() {
        let s = Strike::new(
            7,
            vec![
                DamageRoll::new(1, 10, 8, DamageKind::Slashing),
                DamageRoll::new(2, 4, 0, DamageKind::Fire),
            ],
        );
        let pmf = s.damage_pmf(&dummy(16));
        assert!((pmf.total() - 1.0).abs() < 1e-12);
        assert!(pmf.min() >= 0);
        assert!(pmf.prob(0) > 0.0, "a miss must be possible");
        // On a crit the dice double but the +8 does not: 2d10 + 8 plus 4d4.
        assert_eq!(pmf.max(), 20 + 8 + 16);
    }

    /// The whole reason damage is a list rather than one pool.
    #[test]
    fn each_damage_type_is_reduced_on_its_own() {
        let mut target = dummy(1);
        target
            .reductions
            .push((DamageKind::Fire, Reduction::Immune));

        let s = Strike::new(
            20,
            vec![
                DamageRoll::new(0, 6, 10, DamageKind::Slashing),
                DamageRoll::new(0, 6, 10, DamageKind::Fire),
            ],
        );
        // Every roll but a natural 1 hits, and the fire half is deleted.
        let pmf = s.damage_pmf(&target);
        assert!((pmf.prob(10) - 19.0 / 20.0).abs() < 1e-12);
        assert!((pmf.prob(0) - 1.0 / 20.0).abs() < 1e-12);
    }

    #[test]
    fn a_save_has_no_natural_twenty() {
        let mut target = dummy(10);
        target.saves[Ability::Dex.index()] = 2;
        let save = SaveEffect {
            ability: Ability::Dex,
            dc: 25,
            damage: vec![DamageRoll::new(1, 6, 0, DamageKind::Fire)],
            half_on_success: true,
            on_failure: None,
            max_targets: None,
        };
        // Needs a 23 on a d20; unlike an attack roll, a natural 20 does not
        // rescue it.
        assert!((save.failure_chance(&target) - 1.0).abs() < 1e-12);

        let trivial = SaveEffect { dc: -5, ..save };
        assert!(trivial.failure_chance(&target).abs() < 1e-12);
    }

    #[test]
    fn a_successful_save_halves_before_resistance_does() {
        let mut target = dummy(10);
        target.saves[Ability::Dex.index()] = 100; // always saves
        target
            .reductions
            .push((DamageKind::Fire, Reduction::Resistant));
        let save = SaveEffect {
            ability: Ability::Dex,
            dc: 10,
            damage: vec![DamageRoll::new(0, 6, 21, DamageKind::Fire)],
            half_on_success: true,
            on_failure: None,
            max_targets: None,
        };
        // 21 -> 10 on the save, then 5 from resistance. Rounding down twice is
        // not the same as quartering, which is why the order is pinned here.
        assert!((save.damage_pmf(&target).mean() - 5.0).abs() < 1e-12);
    }

    /// Evasion turns the usual shape inside out, and stacks with resistance
    /// rather than replacing it. 40 fire, resisted, is the case to check
    /// because every step divides.
    #[test]
    fn evasion_inverts_the_save_and_still_lets_resistance_apply() {
        let base = SaveEffect {
            ability: Ability::Dex,
            dc: 10,
            damage: vec![DamageRoll::new(0, 6, 40, DamageKind::Fire)],
            half_on_success: true,
            on_failure: None,
            max_targets: None,
        };

        let mut always_saves = dummy(10);
        always_saves.saves = [100; 6];
        assert!((base.damage_pmf(&always_saves).mean() - 20.0).abs() < 1e-12);

        let evasive = always_saves.clone().with_rider(Rider::NothingOnSuccess {
            ability: Ability::Dex,
        });
        assert!(
            base.damage_pmf(&evasive).mean().abs() < 1e-12,
            "a saved Dex save with Evasion deals nothing"
        );

        // Failing with Evasion is the old success: half. Then resistance.
        let mut evasive_fails = evasive.clone();
        evasive_fails.saves[Ability::Dex.index()] = -100;
        assert!((base.damage_pmf(&evasive_fails).mean() - 20.0).abs() < 1e-12);
        evasive_fails
            .reductions
            .push((DamageKind::Fire, Reduction::Resistant));
        assert!(
            (base.damage_pmf(&evasive_fails).mean() - 10.0).abs() < 1e-12,
            "Evasion halves and resistance halves again"
        );

        // Evasion is keyed to the ability, so a Con save is untouched.
        let con = SaveEffect {
            ability: Ability::Con,
            ..base.clone()
        };
        assert!(!evasive.has_evasion(Ability::Con));
        assert!((con.damage_pmf(&always_saves).mean() - 20.0).abs() < 1e-12);
    }

    #[test]
    fn strikes_add_their_means_and_a_sequence_adds_its_parts() {
        let target = dummy(15);
        let profile = || Strike::new(5, vec![DamageRoll::new(1, 8, 3, DamageKind::Slashing)]);
        let one = Effect::Strikes {
            strike: profile(),
            count: 1,
        };
        let three = Effect::Strikes {
            strike: profile(),
            count: 3,
        };
        assert!((three.mean_damage(&target) - 3.0 * one.mean_damage(&target)).abs() < 1e-9);
        assert!((three.damage_pmf(&target).total() - 1.0).abs() < 1e-12);

        let combo = Effect::Sequence(vec![one.clone(), one.clone(), one.clone()]);
        assert!((combo.mean_damage(&target) - three.mean_damage(&target)).abs() < 1e-9);

        // A stance contributes nothing to damage but is still findable.
        let mixed = Effect::Sequence(vec![
            one,
            Effect::Stance {
                condition: Condition::Dodging,
            },
        ]);
        assert_eq!(mixed.stance(), Some(Condition::Dodging));
    }

    #[test]
    fn conditions_answer_the_questions_the_duel_asks() {
        assert!(Condition::Stunned.incapacitated());
        assert!(Condition::Stunned.advantage_to_attackers());
        assert!(Condition::Stunned.auto_fails(Ability::Dex));
        assert!(!Condition::Stunned.auto_fails(Ability::Wis));
        assert!(Condition::Dodging.disadvantage_to_attackers());
        assert!(!Condition::Dodging.incapacitated());
        assert!(Condition::Prone.advantage_to_attackers());
        assert!(Condition::Prone.disadvantage_on_attacks());
    }
}
