//! Triggered modifiers and reactions (riders).

use super::damage::{DamageKind, DamageRoll};
use super::types::{Ability, Condition, Cost, Duration};
use crate::rules::combat::{Attack, DamageRider, RollMode};

/// What kind of incoming attack a [`Rider::ReactionOnTargeted`] answers.
///
/// One variant today, because nothing in the engine yet distinguishes a
/// melee attack roll from a ranged or spell one the way
/// [`Attack::finesse_or_ranged`] distinguishes weapon properties. The field
/// exists anyway so a future distinction - a reaction that only answers a
/// melee attack, say - is a new variant matched at the same call site, not a
/// new field threaded through every caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackTrigger {
    /// Any attack roll made against this creature.
    AnyAttack,
}

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
    /// A reaction spent on being *targeted* by an attack, before its hit or
    /// miss is finalized, that adds `ac_bonus` to this creature's AC against
    /// that one attack - capable of turning what would have been a hit into
    /// a miss.
    ///
    /// The mirror of [`Rider::ReduceDamage`]: that one reacts to an attack
    /// that already hit, on its damage; this one reacts to being targeted,
    /// before the roll against AC is decided. A reaction that boosts AC
    /// against a targeting attack - the Shield spell, a Ring of Protection's
    /// reactive bonus, and any homebrew item shaped the same way - is this
    /// mechanism; nothing here is specific to any one of them.
    ReactionOnTargeted {
        trigger: AttackTrigger,
        ac_bonus: i32,
        /// Reactions refresh at the start of the creature's turn, the same
        /// as [`Rider::ReduceDamage::per_round`].
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
}

impl Rider {
    /// Riders with a budget need somewhere to count it down.
    pub fn initial_uses(&self) -> u32 {
        match self {
            Rider::AlwaysSucceed { uses } => *uses,
            Rider::ReduceDamage { per_round, .. } => *per_round,
            Rider::ReactionOnTargeted { per_round, .. } => *per_round,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prob::rng::Rng;
    use crate::rules::combat::{damage_pmf, sample_damage, Defense};

    fn sneak_attack() -> Rider {
        Rider::ConditionalExtraDamage {
            dice_count: 4,
            dice_sides: 6,
            once_per_turn: true,
        }
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
