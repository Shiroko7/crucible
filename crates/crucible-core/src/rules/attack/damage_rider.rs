//! Extra damage dice an attack carries on a hit - Sneak Attack, a slaying
//! weapon's bonus - and how a crit doubles them.

use crate::prob::{Pmf, Rng};
use crate::rules::{DamageKind, Defense};

/// Extra damage dice appended to a hit's damage, on top of an attack's own
/// pool - Sneak Attack, a dragonslaying weapon's bonus against its favoured
/// prey, a smite.
///
/// Doubled on a crit exactly like the attack's own dice: a critical hit in
/// 5e doubles all of the attack's damage dice, not only the weapon's.
/// `bonus` is a flat addition and, like [`crate::rules::Attack::damage_bonus`], is never
/// doubled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamageRider {
    pub dice_count: u32,
    pub dice_sides: u32,
    pub bonus: i32,
    /// This rider's own damage type, when it is not the same as whatever the
    /// attack it rides on is already reduced as.
    ///
    /// `None` - the default from [`DamageRider::new`] - means "reduced
    /// exactly like the rest of this hit": [`Defense::reduction`], the same
    /// single [`crate::rules::Reduction`] the base attack uses. That is the pre-existing
    /// behaviour, and it is correct for a weapon-triggered rider (2024 Sneak
    /// Attack's extra dice are the same damage type as the weapon that
    /// qualified it). A rider whose damage type is pinned to something the
    /// attack itself does not carry - the 5e rule that this kind of extra
    /// damage matches the *spell's* damage type when a spell attack
    /// triggered it, not a fixed default - sets `Some(kind)` instead; see
    /// [`crate::rules::Attack::spell_damage_kind`] and
    /// [`crate::creature::Rider::extra_damage_for_with_spell_attack_extension`].
    pub kind: Option<DamageKind>,
}

impl DamageRider {
    pub fn new(dice_count: u32, dice_sides: u32) -> Self {
        Self {
            dice_count,
            dice_sides,
            bonus: 0,
            kind: None,
        }
    }

    pub fn with_bonus(mut self, bonus: i32) -> Self {
        self.bonus = bonus;
        self
    }

    /// Remove `dice` dice from this pool before it is rolled, for something
    /// other than damage - the generic mechanism the 2024 Rogue's Cunning
    /// Strike needs: give up part of a qualifying Sneak Attack's dice, 1d6 at
    /// a time, in exchange for a rider effect instead of extra damage (see
    /// [`crate::creature::Rider::ConditionalExtraDamage`] and
    /// [`crate::creature::Rider::CunningStrike`]).
    ///
    /// This shrinks `dice_count` itself, so fewer dice are actually rolled -
    /// not merely fewer counted after the fact - because every consumer of a
    /// [`DamageRider`] ([`rider_pmf`], [`sample_riders`]) reads `dice_count`
    /// straight off the value with no other bookkeeping to keep in sync.
    ///
    /// Chaining this - `.spend(1)?.spend(1)?` - combines multiple costs
    /// exactly like spending them in one call, since each spend checks
    /// against what is left *after* the previous one: two 1d6 costs can never
    /// together exceed the pool they came from.
    ///
    /// `None` if `dice` is more than `self.dice_count` - spending more dice
    /// than were ever rolled for is a caller bug, not a state worth
    /// representing.
    pub fn spend(mut self, dice: u32) -> Option<Self> {
        if dice > self.dice_count {
            return None;
        }
        self.dice_count -= dice;
        Some(self)
    }

    /// Pin this rider's damage to `kind` rather than whatever [`crate::rules::Reduction`]
    /// the rest of the attack uses - see [`DamageRider::kind`].
    pub fn with_kind(mut self, kind: DamageKind) -> Self {
        self.kind = Some(kind);
        self
    }
}

/// Exact distribution of one damage rider's contribution to one hit, dice
/// doubled on a crit exactly like the base pool.
///
/// Floored at zero and reduced by *its own* effective [`Reduction`] -
/// [`Defense::reduction_for`] - before it is convolved with anything else,
/// the same per-component order [`crate::creature::Strike`] uses for
/// a hit with more than one damage type. That is what lets a rider whose
/// [`DamageRider::kind`] differs from the rest of the attack be resisted (or
/// not) independently of it; a rider with no `kind` of its own falls back to
/// exactly the reduction the base attack gets, which reproduces the
/// pre-existing single-`Reduction` behaviour bit for bit.
fn rider_component_pmf(rider: &DamageRider, crit: bool, defense: &Defense) -> Pmf {
    let dice = if crit {
        rider.dice_count * 2
    } else {
        rider.dice_count
    };
    let reduction = defense.reduction_for(rider.kind);
    Pmf::pool(dice, rider.dice_sides)
        .offset(rider.bonus)
        .floor_at(0)
        .map_values(move |d| reduction.apply(d))
}

/// Every active rider's contribution, each reduced on its own via
/// [`rider_component_pmf`] and then summed.
pub(super) fn riders_pmf(riders: &[DamageRider], crit: bool, defense: &Defense) -> Pmf {
    riders.iter().fold(Pmf::constant(0), |acc, r| {
        acc.convolve(&rider_component_pmf(r, crit, defense))
    })
}

/// The sampled counterpart of [`rider_component_pmf`].
fn sample_rider_component(
    rng: &mut Rng,
    rider: &DamageRider,
    crit: bool,
    defense: &Defense,
) -> i32 {
    let dice = if crit {
        rider.dice_count * 2
    } else {
        rider.dice_count
    };
    let raw: i32 = (0..dice).map(|_| rng.die(rider.dice_sides)).sum();
    defense
        .reduction_for(rider.kind)
        .apply((raw + rider.bonus).max(0))
}

/// The sampled counterpart of [`riders_pmf`].
pub(super) fn sample_riders(
    rng: &mut Rng,
    riders: &[DamageRider],
    crit: bool,
    defense: &Defense,
) -> i32 {
    riders
        .iter()
        .map(|r| sample_rider_component(rng, r, crit, defense))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{damage_pmf, sample_damage, Attack, Reduction};

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-12
    }

    /// A damage rider appends dice on a hit, doubles on a crit like the
    /// attack's own dice, and vanishes entirely from a miss - and whether it
    /// is present at all is exactly how "the condition held" is expressed,
    /// with no predicate machinery needed inside `damage_pmf` itself.
    #[test]
    fn a_damage_rider_only_adds_dice_on_a_hit_and_doubles_on_a_crit() {
        let defense = Defense::new(1, 30); // AC 1: every non-fumble roll hits
        let plain = damage_pmf(&Attack::new(5, 1, 6, 0), &defense);
        let sneak_attack = damage_pmf(
            &Attack::new(5, 1, 6, 0).with_damage_rider(DamageRider::new(2, 6)),
            &defense,
        );
        assert!(close(sneak_attack.total(), 1.0));
        // Miss (a natural 1) is unaffected: the rider never fires.
        assert!(close(sneak_attack.prob(0), plain.prob(0)));
        // A rider absent altogether cannot be conditionally active on the
        // predicate not holding; leaving it out of the list is that "off".
        assert_eq!(
            damage_pmf(&Attack::new(5, 1, 6, 0), &defense).prob(0),
            plain.prob(0)
        );
        assert!(
            sneak_attack.mean() > plain.mean(),
            "extra dice on a hit should raise the mean damage"
        );
        // The base die is 1d6 (max 6); with a crit doubling both pools, the
        // maximum possible is 2*6 (base) + 2*2*6 (rider) = 36.
        assert_eq!(sneak_attack.max(), 2 * 6 + 2 * 2 * 6);
    }

    /// [`DamageRider::spend`] is the generic mechanism Cunning Strike needs:
    /// shrink the pool before it is rolled, and combine as many spends as the
    /// pool allows.
    #[test]
    fn spending_a_damage_rider_reduces_its_dice_count() {
        let full = DamageRider::new(4, 6);
        let after_one = full.spend(1).expect("4 dice can afford spending 1");
        assert_eq!(after_one.dice_count, 3);
        assert_eq!(after_one.dice_sides, 6, "the die size never changes");

        // Two 1d6 spends combine exactly like one 2d6 spend.
        let two_singles = full.spend(1).unwrap().spend(1).unwrap();
        let one_double = full.spend(2).unwrap();
        assert_eq!(two_singles, one_double);
        assert_eq!(two_singles.dice_count, 2);
    }

    #[test]
    fn spending_more_dice_than_the_pool_has_is_rejected() {
        let full = DamageRider::new(2, 6);
        assert_eq!(full.spend(3), None, "only 2 dice are in the pool");
        // Spending the pool down to zero and then trying for one more is the
        // same rejection, reached by combination rather than in one call.
        assert_eq!(full.spend(2).unwrap().spend(1), None);
    }

    #[test]
    fn spending_the_entire_pool_leaves_zero_dice_but_still_a_valid_rider() {
        let full = DamageRider::new(3, 6);
        let spent = full.spend(3).expect("spending exactly what is available");
        assert_eq!(spent.dice_count, 0);
        // A zero-dice rider contributes nothing to damage, which is exactly
        // what "every die went to a Cunning Strike option" should do.
        let attack = Attack::new(10, 1, 6, 0).with_damage_rider(spent);
        let pmf = damage_pmf(&attack, &Defense::new(1, 30));
        let plain = damage_pmf(&Attack::new(10, 1, 6, 0), &Defense::new(1, 30));
        assert_eq!(pmf.mean(), plain.mean());
    }

    /// The reduced pool that Cunning Strike leaves behind must roll exactly
    /// like any other [`DamageRider`] - fewer dice actually rolled, not fewer
    /// dice counted - so the exact and sampled paths still have to agree.
    #[test]
    fn a_spent_damage_rider_agrees_with_the_exact_path() {
        let defense = Defense::new(14, 60);
        let full = DamageRider::new(4, 6);
        let cases = [
            ("nothing spent", full),
            ("spend 1 of 4", full.spend(1).unwrap()),
            ("spend 2 of 4, combined from two single spends", {
                full.spend(1).unwrap().spend(1).unwrap()
            }),
            ("spend all 4", full.spend(4).unwrap()),
        ];
        for (seed, (name, rider)) in cases.into_iter().enumerate() {
            let attack = Attack::new(6, 1, 8, 4).with_damage_rider(rider);
            let exact = damage_pmf(&attack, &defense);
            let (lo, hi) = (exact.min(), exact.max());
            let mut rng = Rng::new(seed as u64 + 1200);
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

    /// The whole reason [`DamageRider::kind`] exists: a rider pinned to a
    /// damage type the target is immune to deals nothing, even though the
    /// base attack it rides on - and an identical rider with no `kind` of
    /// its own - are entirely unaffected by that immunity. The type has to
    /// actually reach damage reduction, not just sit on the struct as a
    /// label nothing reads.
    #[test]
    fn a_damage_rider_with_its_own_kind_is_reduced_independently_of_the_base_attack() {
        let defense = Defense::new(1, 40) // AC 1: every non-fumble roll hits
            .with_kind_reduction(DamageKind::Radiant, Reduction::Immune);

        let unmarked_rider = damage_pmf(
            &Attack::new(5, 1, 6, 0).with_damage_rider(DamageRider::new(4, 6)),
            &defense,
        );
        let radiant_rider = damage_pmf(
            &Attack::new(5, 1, 6, 0)
                .with_damage_rider(DamageRider::new(4, 6).with_kind(DamageKind::Radiant)),
            &defense,
        );
        let force_rider = damage_pmf(
            &Attack::new(5, 1, 6, 0)
                .with_damage_rider(DamageRider::new(4, 6).with_kind(DamageKind::Force)),
            &defense,
        );

        // A rider with no kind of its own is reduced like the rest of the
        // hit - Normal here - so the immunity entry for Radiant never
        // applies to it at all.
        assert!(unmarked_rider.mean() > 0.0);
        // A rider explicitly pinned to Force is likewise untouched by a
        // Radiant-only immunity.
        assert!(close(force_rider.mean(), unmarked_rider.mean()));
        // A rider pinned to Radiant against Radiant immunity contributes
        // nothing: only the unaffected 1d6 base attack remains.
        let base_only = damage_pmf(&Attack::new(5, 1, 6, 0), &defense);
        assert!(close(radiant_rider.mean(), base_only.mean()));
        assert!(
            radiant_rider.mean() < force_rider.mean(),
            "the same rider shape should deal less net damage as Radiant than as Force \
             against a target immune only to Radiant"
        );
    }

    /// The sampled path for a kind-bearing rider must agree with the exact
    /// one exactly as strictly as every other case here - this is the same
    /// per-outcome comparison `sampled_bless_bane_and_a_rider_agree_with_the_exact_path`
    /// uses, extended to a rider whose [`DamageRider::kind`] differs from
    /// the base attack's own reduction.
    #[test]
    fn sampled_damage_agrees_with_the_exact_path_when_a_rider_has_its_own_kind() {
        let defense =
            Defense::new(14, 40).with_kind_reduction(DamageKind::Radiant, Reduction::Resistant);
        let cases = [
            (
                "rider pinned to the resisted kind",
                Attack::new(6, 1, 8, 4)
                    .with_damage_rider(DamageRider::new(3, 6).with_kind(DamageKind::Radiant)),
            ),
            (
                "rider pinned to an unresisted kind",
                Attack::new(6, 1, 8, 4)
                    .with_damage_rider(DamageRider::new(3, 6).with_kind(DamageKind::Force)),
            ),
            (
                "rider with no kind of its own, alongside an override for another kind",
                Attack::new(6, 1, 8, 4).with_damage_rider(DamageRider::new(3, 6)),
            ),
        ];
        for (seed, (name, attack)) in cases.into_iter().enumerate() {
            let exact = damage_pmf(&attack, &defense);
            let (lo, hi) = (exact.min(), exact.max());
            let mut rng = Rng::new(seed as u64 + 900);
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
