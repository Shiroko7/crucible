//! Triggered modifiers and reactions (riders).

use super::combatant::Creature;
use super::damage::{DamageKind, DamageRoll};
use super::types::{Ability, Condition, Cost, Duration};
use crate::prob::rng::Rng;
use crate::rules::combat::{Attack, DamageRider, RollMode};

/// A triggered modifier.
///
/// Each variant is a mechanism, not a feature. The comments name the features
/// that map onto it, which is the test of whether the abstraction is pulling
/// its weight: a variant only one ability can use is a branch in disguise.
#[derive(Debug, Clone, PartialEq)]
pub enum Rider {
    /// On a hit, the target saves or takes a condition.
    ///
    /// Stunning Strike. Also every knockdown, every on-hit poison, and the
    /// secondary effect on most breath weapons.
    SaveOrCondition {
        ability: Ability,
        dc: i32,
        condition: Condition,
        duration: Duration,
        cost: Option<Cost>,
        /// Stunning Strike is once per turn however many times you hit.
        once_per_turn: bool,
    },
    /// A successful save against an effect that would deal half takes none
    /// instead, and a failed one takes half.
    ///
    /// Evasion, for the ability the effect names. Danger Sense and a rogue's
    /// Evasion are the same shape.
    NothingOnSuccess { ability: Ability },
    /// Turn a failed save into a success, a fixed number of times per fight.
    ///
    /// Legendary Resistance. A fighter's Indomitable is the same shape with
    /// one use and a reroll instead of a pass.
    AlwaysSucceed { uses: u32 },
    /// A reaction that reduces the damage of an incoming *attack* whose types
    /// include one of `kinds`.
    ///
    /// Deflect Attacks. Uncanny Dodge and Heavy Armor Master are variations.
    ReduceDamage {
        kinds: Vec<DamageKind>,
        roll: DamageRoll,
        /// Reactions refresh at the start of the creature's turn.
        per_round: u32,
    },
    /// Extra damage dice on a hit, gated on the attack roll having advantage
    /// or an ally next to the target - and never at all if the attacker also
    /// has disadvantage, which overrides an ally in place. Spendable once per
    /// turn if `once_per_turn`.
    ///
    /// Sneak Attack. The gate itself is evaluated by whoever resolves the
    /// attack, from flags on the attack rather than derived geometry - see
    /// [`crate::rules::combat::Attack::ally_adjacent`] and
    /// [`crate::rules::combat::Attack::finesse_or_ranged`] for why. This
    /// variant only carries the dice pool and the once-per-turn budget, so
    /// anything else that shares the exact same gate is this variant too,
    /// not a new branch.
    ConditionalExtraDamage {
        dice_count: u32,
        dice_sides: u32,
        once_per_turn: bool,
    },
    /// The 2024 Rogue's Cunning Strike (Rogue 5) is unlocked, at this save
    /// DC: `8 + Dexterity modifier + proficiency bonus`, computed once by
    /// [`crate::dsl::plugin::CunningStrikePlugin`] and never the creature's
    /// spellcasting DC - Cunning Strike is not spellcasting, and a Rogue
    /// without a caster subclass has no [`SpellCastingProfile`] to read one
    /// from at all.
    ///
    /// A pure marker, deliberately carrying no dice of its own: what it
    /// unlocks is spending part of a *qualifying Sneak Attack's* pool - the
    /// [`DamageRider`] [`Rider::extra_damage_for`] returns for
    /// [`Rider::ConditionalExtraDamage`] - on a rider effect instead of
    /// rolling it for damage, [`DamageRider::spend`] dice at a time (1d6 per
    /// the 2024 rules), each spend costed and combined by whoever resolves
    /// the attack. Several spends can share one hit's pool as long as their
    /// total fits, because [`DamageRider::spend`] is exactly the same
    /// operation chained.
    ///
    /// Which specific effects a spend buys - poison, a shove, breaking a
    /// grapple - is deliberately not here: this is the generic framework, and
    /// a later plugin per effect reads this same DC rather than inventing its
    /// own.
    ///
    /// [`SpellCastingProfile`]: super::types::SpellCastingProfile
    CunningStrike { dc: i32 },
    /// An attacker-side trait: this creature's own damage and inflicted
    /// conditions punch through a target's immunity, though not all the way
    /// to full effect.
    ///
    /// `damage` softens a target normally [`super::combatant::Creature`]-immune to
    /// that damage type down to merely resistant (half instead of zero)
    /// against a hit *this* creature lands - see
    /// [`super::combatant::Creature::reduction_from`]. `condition` does the
    /// same for a condition this creature inflicts: a target normally immune
    /// to it still has to make the save, rolled with Advantage instead of
    /// auto-succeeding - see [`saving_throw_against_condition`].
    ///
    /// Both are attacker-scoped, never a change to the target's own stat
    /// sheet: a different attacker without this trait, against the very
    /// same target, still sees it as fully immune either way.
    ///
    /// The two fields are independent - an attacker might carry only one
    /// half of this, punching through only a damage type or only a
    /// condition - which is why this is one variant with two `Option`s
    /// rather than two variants: the parser reads whichever of `... damage`
    /// and `... condition` is present in one trait string, in any
    /// combination.
    DowngradeImmunity {
        damage: Option<DamageKind>,
        condition: Option<Condition>,
    },
}

impl Rider {
    /// Riders with a budget need somewhere to count it down.
    pub fn initial_uses(&self) -> u32 {
        match self {
            Rider::AlwaysSucceed { uses } => *uses,
            Rider::ReduceDamage { per_round, .. } => *per_round,
            _ => 0,
        }
    }

    /// The [`DamageRider`] this rider contributes to `attack`, or `None` if
    /// it does not apply - either because this variant is not
    /// [`Rider::ConditionalExtraDamage`], or because its gate does not hold.
    ///
    /// The gate (Sneak Attack's, specifically): a finesse or ranged weapon,
    /// on a roll that is not at disadvantage, with either advantage or an
    /// ally next to the target. `used_this_turn` is the once-per-turn
    /// budget, tracked by the caller - the same shared per-creature flag
    /// `sim::duel` already keeps for Stunning-Strike-style riders (see
    /// `README.md`'s note that a creature gets one once-per-turn rider
    /// trigger per turn in total, not one per rider).
    ///
    /// Returns the *full* qualifying dice pool. A feature that spends part
    /// of it on something other than damage (Cunning Strike) reduces
    /// `dice_count` on the result before handing it to
    /// [`Attack::with_damage_rider`] - [`DamageRider`] is a plain count of
    /// dice, so rolling fewer of them is not a special case.
    pub fn extra_damage_for(&self, attack: &Attack, used_this_turn: bool) -> Option<DamageRider> {
        let Rider::ConditionalExtraDamage {
            dice_count,
            dice_sides,
            once_per_turn,
        } = self
        else {
            return None;
        };
        if *once_per_turn && used_this_turn {
            return None;
        }
        if !attack.finesse_or_ranged || attack.mode == RollMode::Disadvantage {
            return None;
        }
        if attack.mode == RollMode::Advantage || attack.ally_adjacent {
            Some(DamageRider::new(*dice_count, *dice_sides))
        } else {
            None
        }
    }

    /// The Cunning Strike DC this rider carries, or `None` if it is not
    /// [`Rider::CunningStrike`].
    ///
    /// A creature's full rider list is a `Vec<Rider>`, so the usual way to
    /// call this is `creature.riders.iter().find_map(Rider::cunning_strike_dc)` -
    /// the same "scan the list for the variant you care about" shape
    /// [`Creature::has_evasion`] already uses for
    /// [`Rider::NothingOnSuccess`].
    ///
    /// [`Creature::has_evasion`]: super::combatant::Creature::has_evasion
    pub fn cunning_strike_dc(&self) -> Option<i32> {
        match self {
            Rider::CunningStrike { dc } => Some(*dc),
            _ => None,
        }
    }

    /// Does this rider downgrade a target's immunity to `kind` (Immune to
    /// Resistant) for damage this creature deals? See
    /// [`super::combatant::Creature::reduction_from`], which is the usual
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
}

/// Roll a saving throw `target` makes against `condition`, which `attacker`
/// is trying to inflict.
///
/// A target with no immunity to `condition` rolls exactly like any other
/// saving throw: one d20 plus its own bonus against `dc`. A target that
/// *is* immune to `condition` (see
/// [`super::combatant::Creature::immune_to_condition`]) is ordinarily
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

/// P(a roll under `mode` is at least `needed`), read off
/// [`RollMode::distribution`] rather than rederived.
fn probability_at_least(mode: RollMode, needed: i32) -> f64 {
    mode.distribution()
        .iter()
        .enumerate()
        .map(|(i, &p)| if i as i32 + 1 >= needed { p } else { 0.0 })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::combat::{damage_pmf, sample_damage, Defense, Reduction};

    fn sneak_attack() -> Rider {
        Rider::ConditionalExtraDamage {
            dice_count: 4,
            dice_sides: 6,
            once_per_turn: true,
        }
    }

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
        let unrelated = Rider::AlwaysSucceed { uses: 3 };
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
    fn cunning_strike_dc_reads_off_the_marker_rider_and_nothing_else() {
        assert_eq!(
            Rider::CunningStrike { dc: 15 }.cunning_strike_dc(),
            Some(15)
        );
        assert_eq!(sneak_attack().cunning_strike_dc(), None);
        assert_eq!(
            Rider::AlwaysSucceed { uses: 3 }.cunning_strike_dc(),
            None,
            "an unrelated rider is not mistaken for the Cunning Strike marker"
        );

        // The usual call shape: scan a creature's rider list for the one
        // that carries the DC.
        let riders = [sneak_attack(), Rider::CunningStrike { dc: 15 }];
        assert_eq!(riders.iter().find_map(Rider::cunning_strike_dc), Some(15));
    }

    /// The framework end to end: a qualifying Sneak Attack's full pool is
    /// inspectable, part of it can be spent on a stand-in "does nothing"
    /// Cunning Strike option (or two, combined) at the DC the marker rider
    /// carries, and the dice that do get rolled for damage are exactly the
    /// dice that were not spent - not the full pool with a discount applied
    /// after the fact.
    #[test]
    fn cunning_strike_spends_from_the_same_pool_sneak_attack_would_roll() {
        let attack = Attack::new(7, 1, 4, 3)
            .with_mode(RollMode::Advantage)
            .with_finesse_or_ranged(true);

        let full = sneak_attack()
            .extra_damage_for(&attack, false)
            .expect("qualifies for Sneak Attack");
        assert_eq!(
            full,
            DamageRider::new(4, 6),
            "the full pool before any spend"
        );

        let dc = Rider::CunningStrike { dc: 15 }
            .cunning_strike_dc()
            .expect("Cunning Strike is unlocked");

        // Spend 1d6 on a trivial "costs 1d6, does nothing" test option -
        // proving the framework needs no real effect plugin to exercise it -
        // then spend a second 1d6 on a stand-in for a different option, the
        // same way two later plugins (Poison, Trip/Withdraw) would each
        // spend their own share of one hit's pool.
        let after_first_option = full.spend(1).expect("4 dice can afford 1");
        assert_eq!(after_first_option.dice_count, 3);
        let after_second_option = after_first_option
            .spend(1)
            .expect("3 dice can afford 1 more");
        assert_eq!(
            after_second_option.dice_count, 2,
            "two combined 1d6 spends leave 2 of the original 4"
        );
        assert_eq!(dc, 15, "both stand-in options are funded at the same DC");

        // A third spend of everything remaining is still within budget...
        assert!(after_second_option.spend(2).is_some());
        // ...but spending even one more die than is left is refused, not
        // silently clamped.
        assert_eq!(after_second_option.spend(3), None);

        // What actually gets rolled for damage is the *reduced* rider: two
        // fewer dice actually thrown, not four dice thrown and two ignored.
        let defense = Defense::new(10, 40);
        let reduced_attack = attack.clone().with_damage_rider(after_second_option);
        let exact = damage_pmf(&reduced_attack, &defense);
        let full_attack = attack.with_damage_rider(full);
        let exact_full = damage_pmf(&full_attack, &defense);
        assert!(
            exact.mean() < exact_full.mean(),
            "spending dice away must lower expected damage, not just relabel it"
        );
        // The reduced pool's maximum is exactly the base 1d4+3 plus 2d6 (spent
        // dice cannot show up in the damage distribution at all), the dice
        // doubled on a crit like any other rider and the flat bonus not
        // doubled at all.
        assert_eq!(exact.max(), 2 * 4 + 2 * 2 * 6 + 3);
    }

    /// The sampled path must actually draw fewer dice, not merely report a
    /// smaller number - the same exact-vs-sampled agreement check every
    /// other rider in this codebase is held to.
    #[test]
    fn a_cunning_strike_reduced_pool_agrees_with_the_exact_path() {
        let defense = Defense::new(12, 60);
        let attack = Attack::new(6, 1, 8, 4)
            .with_mode(RollMode::Advantage)
            .with_finesse_or_ranged(true);
        let full = sneak_attack().extra_damage_for(&attack, false).unwrap();

        let cases = [
            ("full pool, nothing spent for Cunning Strike", full),
            ("spend 1d6 on one stand-in option", full.spend(1).unwrap()),
            (
                "spend 1d6 twice on two stand-in options, combined",
                full.spend(1).unwrap().spend(1).unwrap(),
            ),
        ];
        for (seed, (name, rider)) in cases.into_iter().enumerate() {
            let attack = attack.clone().with_damage_rider(rider);
            let exact = damage_pmf(&attack, &defense);
            let (lo, hi) = (exact.min(), exact.max());
            let mut rng = Rng::new(seed as u64 + 1500);
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
