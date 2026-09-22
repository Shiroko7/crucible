//! Cunning Strike's riders: the DC they read, and spending Sneak Attack
//! dice on a Trip or a Withdraw.

use crate::creature::Rider;
use crate::prob::Rng;
use crate::rules::{Condition, DamageRider, Size};

impl Rider {
    /// The Cunning Strike DC this rider carries, or `None` if it is not
    /// [`Rider::CunningStrike`].
    ///
    /// A creature's full rider list is a `Vec<Rider>`, so the usual way to
    /// call this is `creature.riders.iter().find_map(Rider::cunning_strike_dc)` -
    /// the same "scan the list for the variant you care about" shape
    /// [`Creature::has_evasion`] already uses for
    /// [`Rider::NothingOnSuccess`].
    ///
    /// [`Creature::has_evasion`]: crate::creature::Creature::has_evasion
    pub fn cunning_strike_dc(&self) -> Option<i32> {
        match self {
            Rider::CunningStrike { dc } => Some(*dc),
            _ => None,
        }
    }

    /// Resolve a Cunning Strike: Trip attempt, or `None` if this rider is
    /// not [`Rider::CunningStrikeTrip`], `target_size` is bigger than Large
    /// (a Huge or Gargantuan target cannot be tripped at all, so the die is
    /// never spent - an illegal declaration, the same shape
    /// [`DamageRider::spend`] already gives an overdrawn pool), or `pool`
    /// cannot afford the 1d6 cost.
    ///
    /// On a legal attempt, spends the die and rolls a Dexterity save
    /// against `dc` (see [`Rider::cunning_strike_dc`]) at `target_dex_save`,
    /// returning the reduced pool alongside [`Condition::Prone`] on a
    /// failed save or `None` on a successful one.
    ///
    /// This rolls a bare save with `rng`, for resolving one attack outside a
    /// fight. `sim::fight` makes the same choice and spend live, but rolls the
    /// save through its own save handling - which also lets
    /// [`Rider::AlwaysSucceed`] buy back a failure and auto-fails Strength
    /// and Dexterity saves for an Incapacitated target.
    pub fn resolve_cunning_strike_trip(
        &self,
        pool: DamageRider,
        target_size: Size,
        target_dex_save: i32,
        dc: i32,
        rng: &mut Rng,
    ) -> Option<(DamageRider, Option<Condition>)> {
        if !matches!(self, Rider::CunningStrikeTrip) {
            return None;
        }
        if target_size > Size::Large {
            return None;
        }
        let reduced = pool.spend(1)?;
        let saved = rng.die(20) + target_dex_save >= dc;
        Some((reduced, (!saved).then_some(Condition::Prone)))
    }

    /// Resolve a Cunning Strike: Withdraw attempt, or `None` if this rider
    /// is not [`Rider::CunningStrikeWithdraw`] or `pool` cannot afford the
    /// 1d6 cost.
    ///
    /// On a legal attempt, spends the die and returns the reduced pool
    /// alongside `true`: a flag for "the rogue is treated as having safely
    /// repositioned this turn" and nothing more, per
    /// [`Rider::CunningStrikeWithdraw`]'s doc comment - there is no
    /// movement or opportunity-attack model here for it to actually do
    /// anything to.
    pub fn resolve_cunning_strike_withdraw(
        &self,
        pool: DamageRider,
    ) -> Option<(DamageRider, bool)> {
        if !matches!(self, Rider::CunningStrikeWithdraw) {
            return None;
        }
        let reduced = pool.spend(1)?;
        Some((reduced, true))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{damage_pmf, sample_damage, Attack, Defense, RollMode};

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

    fn trip_pool() -> DamageRider {
        // Stand-in for a qualifying Sneak Attack's pool: 4d6, same as the
        // rest of this file's Cunning Strike tests.
        DamageRider::new(4, 6)
    }

    #[test]
    fn trip_is_refused_against_a_target_bigger_than_large() {
        let mut rng = Rng::new(1);
        for size in [Size::Huge, Size::Gargantuan] {
            assert_eq!(
                Rider::CunningStrikeTrip.resolve_cunning_strike_trip(
                    trip_pool(),
                    size,
                    100, // even a save bonus this high must not matter
                    10,
                    &mut rng,
                ),
                None,
                "{size:?} is too big to trip; the die must not be spent either"
            );
        }
    }

    #[test]
    fn trip_is_allowed_against_large_and_everything_smaller() {
        let mut rng = Rng::new(2);
        for size in [Size::Tiny, Size::Small, Size::Medium, Size::Large] {
            let (reduced, _) = Rider::CunningStrikeTrip
                .resolve_cunning_strike_trip(trip_pool(), size, 0, 10, &mut rng)
                .unwrap_or_else(|| panic!("{size:?} should be a legal Trip target"));
            assert_eq!(reduced.dice_count, 3, "1d6 spent on the attempt");
        }
    }

    #[test]
    fn trip_spends_1d6_from_the_sneak_attack_pool() {
        let mut rng = Rng::new(3);
        let (reduced, _) = Rider::CunningStrikeTrip
            .resolve_cunning_strike_trip(trip_pool(), Size::Medium, 0, 10, &mut rng)
            .expect("a Medium target can be tripped");
        assert_eq!(reduced, DamageRider::new(3, 6));
    }

    #[test]
    fn trip_knocks_a_failed_save_prone_and_leaves_a_successful_one_standing() {
        let mut rng = Rng::new(4);
        // A save bonus far below any roll a d20 can produce always fails.
        let (_, failed) = Rider::CunningStrikeTrip
            .resolve_cunning_strike_trip(trip_pool(), Size::Medium, -100, 10, &mut rng)
            .expect("legal attempt");
        assert_eq!(failed, Some(Condition::Prone));

        // A save bonus far above any DC always succeeds.
        let (_, succeeded) = Rider::CunningStrikeTrip
            .resolve_cunning_strike_trip(trip_pool(), Size::Medium, 100, 10, &mut rng)
            .expect("legal attempt");
        assert_eq!(succeeded, None);
    }

    #[test]
    fn trip_refuses_to_overdraw_the_pool() {
        let mut rng = Rng::new(5);
        let empty = DamageRider::new(0, 6);
        assert_eq!(
            Rider::CunningStrikeTrip.resolve_cunning_strike_trip(
                empty,
                Size::Medium,
                0,
                10,
                &mut rng
            ),
            None,
            "no dice left to spend on the attempt"
        );
    }

    #[test]
    fn trip_resolution_is_gated_on_the_matching_rider_variant() {
        let mut rng = Rng::new(6);
        assert_eq!(
            Rider::CunningStrikeWithdraw.resolve_cunning_strike_trip(
                trip_pool(),
                Size::Medium,
                0,
                10,
                &mut rng
            ),
            None,
            "an unrelated rider is not mistaken for the Trip marker"
        );
    }

    #[test]
    fn withdraw_spends_1d6_and_flags_a_safe_reposition() {
        let pool = DamageRider::new(4, 6);
        let (reduced, repositioned) = Rider::CunningStrikeWithdraw
            .resolve_cunning_strike_withdraw(pool)
            .expect("4 dice can afford the 1d6 cost");
        assert_eq!(reduced, DamageRider::new(3, 6));
        assert!(repositioned, "a legal Withdraw always repositions safely");
    }

    #[test]
    fn withdraw_refuses_to_overdraw_the_pool() {
        let empty = DamageRider::new(0, 6);
        assert_eq!(
            Rider::CunningStrikeWithdraw.resolve_cunning_strike_withdraw(empty),
            None
        );
    }

    #[test]
    fn withdraw_resolution_is_gated_on_the_matching_rider_variant() {
        assert_eq!(
            Rider::CunningStrikeTrip.resolve_cunning_strike_withdraw(trip_pool()),
            None,
            "an unrelated rider is not mistaken for the Withdraw marker"
        );
    }
}
