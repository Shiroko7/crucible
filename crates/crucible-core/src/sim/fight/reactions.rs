//! Reactions: a creature's one per round, spent on an AC boost against an
//! incoming attack or on cutting a hit's damage.

use crate::creature::{AttackKind, Rider, Strike};
use crate::prob::Rng;
use crate::sim::fight::{Answer, Fighter};

/// The AC bonus a [`Rider::ReactionOnTargeted`] offers against an incoming
/// attack of `kind`, if this creature has one that answers it and a reaction
/// left to spend on it - never while Incapacitated.
pub(super) fn ac_boost_reaction(f: &Fighter<'_>, kind: AttackKind) -> Option<i32> {
    if !f.reaction || f.incapacitated() {
        return None;
    }
    f.creature.riders.iter().find_map(|rider| match rider {
        Rider::ReactionOnTargeted { trigger, ac_bonus } if trigger.answers(kind) => Some(*ac_bonus),
        _ => None,
    })
}

/// Spend this creature's reaction to cut an incoming hit's damage, if it has
/// one left and a rider that answers this hit - the first in declaration
/// order: [`Rider::ReduceDamage`] against the damage types it names, or
/// [`Rider::HalveAttackDamage`] against anything.
pub(super) fn react_to_hit(
    rng: &mut Rng,
    f: &mut Fighter<'_>,
    strike: &Strike,
    damage: i32,
) -> (i32, Option<Answer>) {
    if damage <= 0 || !f.reaction || f.incapacitated() {
        return (damage, None);
    }
    let creature = f.creature;
    for rider in &creature.riders {
        match rider {
            Rider::ReduceDamage { kinds, roll } if strike.deals_any(kinds) => {
                f.reaction = false;
                let cut = roll.sample_raw(rng).min(damage);
                return (damage - cut, Some(Answer::Deflected(cut)));
            }
            Rider::HalveAttackDamage => {
                f.reaction = false;
                return (damage / 2, Some(Answer::Halved));
            }
            _ => {}
        }
    }
    (damage, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::{AttackTrigger, Creature, Effect, Move};
    use crate::rules::{Condition, DamageKind, DamageRoll};
    use crate::sim::fight::fighter::refresh;
    use crate::sim::fight::test_support::{no_log, puncher};
    use crate::sim::fight::Expiry;
    use crate::sim::{run, Policy, Side};

    // Every damage rider, Cunning Strike choice, injury dose, weapon buff and
    // reaction exercised here existed and was unit-tested as a mechanism (in
    // `creature::rider`) before the fight loop actually used any of them.
    // These pin them to the loop.

    /// Dodge and a reaction that cuts damage both have to actually reduce what
    /// lands, and the stance has to expire on its own.
    #[test]
    fn dodging_and_a_damage_reducing_reaction_both_bite() {
        let attacker = {
            let mut c = puncher("attacker", 10, 10_000, 5, 0);
            c.actions[0].effect = Effect::Strikes {
                strike: Strike::new(5, vec![DamageRoll::new(2, 6, 4, DamageKind::Slashing)]),
                count: 3,
            };
            c.initiative = -100;
            c
        };
        let plain = {
            let mut c = Creature::new("plain", 15, 10_000);
            c.initiative = 100;
            c
        };
        let dodger = {
            let mut c = plain.clone();
            c.bonus_actions.push(Move::new(
                "Dodge",
                Effect::Stance {
                    condition: Condition::Dodging,
                },
            ));
            c
        };
        let deflector = {
            let mut c = plain.clone();
            c.riders.push(Rider::ReduceDamage {
                kinds: vec![DamageKind::Slashing],
                roll: DamageRoll::new(1, 10, 7, DamageKind::Slashing),
            });
            c
        };

        let taken = |defender: &Creature| {
            let mut rng = Rng::new(8);
            let mut total = 0i64;
            for _ in 0..400 {
                let mut log = no_log();
                let o = run(
                    &mut rng,
                    [defender, &attacker],
                    [Policy::Greedy; 2],
                    5,
                    &mut log,
                );
                total += o.damage_dealt[1];
            }
            total
        };

        let base = taken(&plain);
        assert!(taken(&dodger) < base, "dodging must reduce incoming damage");
        assert!(
            taken(&deflector) < base,
            "a damage-reducing reaction must reduce incoming damage"
        );
        // One reaction a round, so it cannot blunt all three attacks.
        assert!(taken(&deflector) > base / 2);
    }

    /// [`Rider::ReactionOnTargeted`] is the mirror of [`Rider::ReduceDamage`]:
    /// it acts before the hit is even decided, converting what would have
    /// been a hit into a miss instead of shaving damage off one that already
    /// landed.
    #[test]
    fn a_reactive_ac_boost_converts_a_would_be_hit_into_a_miss() {
        let attacker = {
            let mut c = puncher("attacker", 10, 10_000, 5, 0);
            c.actions[0].effect = Effect::Strikes {
                strike: Strike::new(5, vec![DamageRoll::new(2, 6, 4, DamageKind::Slashing)]),
                count: 3,
            };
            c.initiative = -100;
            c
        };
        let plain = {
            let mut c = Creature::new("plain", 15, 10_000);
            c.initiative = 100;
            c
        };
        let shielded = {
            let mut c = plain.clone();
            c.riders.push(Rider::ReactionOnTargeted {
                trigger: AttackTrigger::AnyAttack,
                // Large enough that, whenever the reaction fires, it always
                // succeeds in turning the hit into a miss.
                ac_bonus: 100,
            });
            c
        };

        let taken = |defender: &Creature| {
            let mut rng = Rng::new(8);
            let mut total = 0i64;
            for _ in 0..400 {
                let mut log = no_log();
                let o = run(
                    &mut rng,
                    [defender, &attacker],
                    [Policy::Greedy; 2],
                    5,
                    &mut log,
                );
                total += o.damage_dealt[1];
            }
            total
        };

        let base = taken(&plain);
        let with_reaction = taken(&shielded);
        assert!(
            with_reaction < base,
            "a reactive AC boost must reduce incoming damage: {with_reaction} vs {base}"
        );
        // One reaction a round protects at most one of the three attacks each
        // round, so it cannot blunt them all.
        assert!(with_reaction > base / 4);
    }

    /// The reaction in isolation: available at the start, gone the instant
    /// it is spent, and back only once the creature's turn refreshes it.
    #[test]
    fn a_reaction_on_targeted_is_available_once_then_spent_for_the_round() {
        let mut c = Creature::new("defender", 15, 20);
        c.riders.push(Rider::ReactionOnTargeted {
            trigger: AttackTrigger::AnyAttack,
            ac_bonus: 5,
        });
        let mut f = Fighter::new(&c, Side::A, Policy::Greedy, 0);

        assert_eq!(
            ac_boost_reaction(&f, AttackKind::MELEE_WEAPON),
            Some(5),
            "the reaction should be available before anything spends it"
        );

        // Spend it exactly the way the strike-resolution loop does.
        f.reaction = false;
        assert_eq!(
            ac_boost_reaction(&f, AttackKind::MELEE_WEAPON),
            None,
            "spent this round, it must not be offered again"
        );

        // Refreshing at the start of a turn is the only thing that brings a
        // reaction back.
        let mut rng = Rng::new(1);
        refresh(&mut f, &mut rng);
        assert_eq!(
            ac_boost_reaction(&f, AttackKind::MELEE_WEAPON),
            Some(5),
            "a new turn should refresh the reaction"
        );

        // And nothing is spent while Incapacitated.
        f.conditions
            .push((Condition::Stunned, Expiry::TurnStart(0)));
        assert_eq!(ac_boost_reaction(&f, AttackKind::MELEE_WEAPON), None);
    }

    /// A reaction limited to ranged weapon attacks answers exactly those:
    /// not a sword, and not a spell attack from range.
    #[test]
    fn a_ranged_weapon_reaction_ignores_melee_and_spell_attacks() {
        let mut c = Creature::new("defender", 15, 20);
        c.riders.push(Rider::ReactionOnTargeted {
            trigger: AttackTrigger::RangedWeaponAttack,
            ac_bonus: 5,
        });
        let f = Fighter::new(&c, Side::A, Policy::Greedy, 0);
        assert_eq!(ac_boost_reaction(&f, AttackKind::RANGED_WEAPON), Some(5));
        assert_eq!(ac_boost_reaction(&f, AttackKind::MELEE_WEAPON), None);
        assert_eq!(ac_boost_reaction(&f, AttackKind::RANGED_SPELL), None);
    }

    /// A multiattack throws several attack rolls in one turn, but a reaction
    /// is still only spendable once: at most one of them should ever be the
    /// one it was spent on.
    #[test]
    fn the_reaction_only_converts_one_attack_per_round_even_in_a_multiattack() {
        let attacker = {
            // AC 1 and a +30 to hit: every roll but a natural 1 is an
            // ordinary hit against the base AC, so the reaction has every
            // chance to fire on each of the three swings if it could.
            let mut c = puncher("attacker", 10, 10_000, 30, 0);
            c.actions[0].effect = Effect::Strikes {
                strike: Strike::new(30, vec![DamageRoll::new(1, 4, 0, DamageKind::Bludgeoning)]),
                count: 3,
            };
            c.initiative = -100;
            c
        };
        let mut defender = Creature::new("defender", 1, 10_000);
        defender.initiative = 100;
        defender.riders.push(Rider::ReactionOnTargeted {
            trigger: AttackTrigger::AnyAttack,
            ac_bonus: 100,
        });

        let mut rng = Rng::new(3);
        let mut saw_a_boosted_miss = false;
        for _ in 0..200 {
            let mut log = Some(Vec::new());
            run(
                &mut rng,
                [&defender, &attacker],
                [Policy::Greedy; 2],
                1,
                &mut log,
            );
            let narration = log.unwrap().join("\n");
            let boosted = narration.matches("AC boosted").count();
            assert!(
                boosted <= 1,
                "one reaction a round should never convert more than one attack:\n{narration}"
            );
            saw_a_boosted_miss |= boosted == 1;
        }
        assert!(
            saw_a_boosted_miss,
            "an available reaction against a would-be hit should have fired at least once in 200 rounds"
        );
    }

    /// Uncanny Dodge halves one hit a round, and shares that one reaction
    /// with every other reaction the creature has.
    #[test]
    fn halving_an_attack_spends_the_one_reaction_a_round() {
        let rogue = Creature::new("rogue", 1, 1_000)
            .with_rider(Rider::HalveAttackDamage)
            .with_rider(Rider::ReactionOnTargeted {
                trigger: AttackTrigger::AnyAttack,
                ac_bonus: 100,
            });
        let mut f = Fighter::new(&rogue, Side::A, Policy::Greedy, 0);
        let strike = Strike::new(5, vec![DamageRoll::new(1, 6, 0, DamageKind::Slashing)]);
        let mut rng = Rng::new(1);
        assert_eq!(
            react_to_hit(&mut rng, &mut f, &strike, 21),
            (10, Some(Answer::Halved))
        );
        assert_eq!(react_to_hit(&mut rng, &mut f, &strike, 21), (21, None));
        assert_eq!(
            ac_boost_reaction(&f, AttackKind::MELEE_WEAPON),
            None,
            "the same reaction cannot also boost AC"
        );
        f.conditions
            .push((Condition::Stunned, Expiry::TurnStart(0)));
        refresh(&mut f, &mut rng);
        assert_eq!(
            react_to_hit(&mut rng, &mut f, &strike, 21),
            (21, None),
            "nothing while Incapacitated"
        );
    }
}
