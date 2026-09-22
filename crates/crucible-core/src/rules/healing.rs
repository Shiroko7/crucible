//! Healing rolls, and 5e's general rule for regaining hit points at 0.

use crate::prob::{Pmf, Rng};

/// A healing roll: dice plus a flat modifier, restoring hit points instead of
/// removing them.
///
/// Deliberately not a [`crate::rules::DamageRoll`] wearing a different sign:
/// there is no damage type to resist, no crit to double the dice, and nothing
/// reduces it. Keeping it a separate, smaller type means `Effect::Heal` cannot
/// accidentally inherit any of that machinery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealRoll {
    pub count: u32,
    pub sides: u32,
    /// Baked in at construction time from whichever ability score fuels the
    /// cast - the caster's [`crate::rules::SpellCastingProfile`]
    /// modifier, not a hardcoded number. See the healing spells in
    /// `features::spells`.
    pub bonus: i32,
}

impl HealRoll {
    pub fn new(count: u32, sides: u32, bonus: i32) -> Self {
        assert!(sides > 0, "a d0 has no faces");
        Self {
            count,
            sides,
            bonus,
        }
    }

    /// Exact distribution of the amount healed, floored at zero: a roll with
    /// a very negative modifier cannot make a healing spell drain HP.
    pub fn pmf(&self) -> Pmf {
        Pmf::pool(self.count, self.sides)
            .offset(self.bonus)
            .floor_at(0)
    }

    /// The sampled counterpart of [`HealRoll::pmf`]; `tests/duel_agreement.rs`-style
    /// agreement is asserted in this module's own tests.
    pub fn sample(&self, rng: &mut Rng) -> i32 {
        let raw: i32 = (0..self.count).map(|_| rng.die(self.sides)).sum();
        (raw + self.bonus).max(0)
    }

    pub fn mean(&self) -> f64 {
        f64::from(self.count) * (f64::from(self.sides) + 1.0) / 2.0 + f64::from(self.bonus)
    }
}

/// Is a creature at this HP down - unconscious, in 5e terms?
///
/// There is no death-save subsystem here, so "down" is exactly "at zero HP or
/// below": the minimal state a healing spell needs to know whether it is
/// reviving someone or merely topping them up. Named rather than repeating
/// `hp <= 0` at each call site, the same reasoning `Condition` gets.
pub fn is_down(hp: i32) -> bool {
    hp <= 0
}

/// Apply `amount` of healing to `current_hp`, capped at `max_hp`.
///
/// Returns the new HP and whether this revived the target: 5e's general
/// "regaining hit points" rule is that any creature at 0 HP that regains any
/// HP becomes conscious again, which is not specific to any one spell - both
/// Healing Word and Cure Wounds get it for free by going through this.
pub fn apply_healing(current_hp: i32, max_hp: i32, amount: i32) -> (i32, bool) {
    let was_down = is_down(current_hp);
    let new_hp = (current_hp + amount.max(0)).min(max_hp);
    (new_hp, was_down && !is_down(new_hp))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn healing_from_zero_revives_and_healing_from_positive_does_not() {
        let (new_hp, revived) = apply_healing(0, 30, 5);
        assert_eq!(new_hp, 5);
        assert!(revived, "regaining HP from 0 wakes the creature up");

        let (new_hp, revived) = apply_healing(12, 30, 5);
        assert_eq!(new_hp, 17);
        assert!(!revived, "never went down, so there is nothing to revive");
    }

    /// This engine never clamps HP at zero (a fighter's HP can read
    /// negative from overkill damage), so "down" has to mean "at or below
    /// zero", not "exactly zero".
    #[test]
    fn a_deeply_negative_target_still_revives_once_healed_past_zero() {
        assert!(is_down(-15));
        let (new_hp, revived) = apply_healing(-15, 30, 20);
        assert_eq!(new_hp, 5);
        assert!(revived);
    }

    /// Healing that does not clear zero leaves the creature down - reaching
    /// exactly 0 is still "at 0 HP", not revived, matching 5e's own wording.
    #[test]
    fn healing_that_does_not_cross_zero_does_not_revive() {
        let (new_hp, revived) = apply_healing(-15, 30, 10);
        assert_eq!(new_hp, -5);
        assert!(!revived);

        let (new_hp, revived) = apply_healing(-10, 30, 10);
        assert_eq!(new_hp, 0);
        assert!(!revived, "landing exactly on 0 is still down");
    }

    #[test]
    fn healing_never_exceeds_max_hp() {
        let (new_hp, _) = apply_healing(28, 30, 100);
        assert_eq!(new_hp, 30);
    }
}
