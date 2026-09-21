//! Spell slots, and the spell attack bonus and save DC formula.
//!
//! Spell slots and the attack/DC formula are the kind of thing several other
//! features (upcasting, Warlock slots, item bonuses) will want to extend.

use crate::rules::Ability;

/// How many spell levels a caster can have slots at: 1st through 9th.
pub const SPELL_LEVELS: u32 = 9;

/// A caster's spell slot pools: one independent counter per level, 1st
/// through 9th.
///
/// Distinct from [`crate::creature::Resource`], which is a single named pool
/// shared across several moves (focus, ki, sorcery points). A caster's slots
/// are nine separate counters instead, each with its own maximum, and a slot
/// spent at one level can never fill a different one - so this earns its own
/// type rather than being nine `Resource`s wearing a trenchcoat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SpellSlots {
    max: [u32; SPELL_LEVELS as usize],
    available: [u32; SPELL_LEVELS as usize],
}

impl SpellSlots {
    pub fn new() -> Self {
        Self::default()
    }

    fn index(level: u32) -> usize {
        assert!(
            (1..=SPELL_LEVELS).contains(&level),
            "spell slot level must be 1-9, got {level}"
        );
        (level - 1) as usize
    }

    /// Declare (or redeclare) the maximum slots at `level`, refilling
    /// `available` to match. This is how a config loader sets a caster's
    /// starting pool, before anything has been spent.
    pub fn set_max(&mut self, level: u32, max: u32) {
        let i = Self::index(level);
        self.max[i] = max;
        self.available[i] = max;
    }

    pub fn max(&self, level: u32) -> u32 {
        self.max[Self::index(level)]
    }

    pub fn available(&self, level: u32) -> u32 {
        self.available[Self::index(level)]
    }

    /// Spend one slot of exactly `level`. `false` and no change if none are
    /// left - upcasting and slot substitution are a policy decision for
    /// whatever calls this, not this type's job.
    pub fn cast(&mut self, level: u32) -> bool {
        let i = Self::index(level);
        if self.available[i] == 0 {
            return false;
        }
        self.available[i] -= 1;
        true
    }

    /// A long rest: every slot returns.
    pub fn recover_all(&mut self) {
        self.available = self.max;
    }

    /// Return `amount` slots at `level`, capped at the maximum. A short-rest
    /// feature (Arcane Recovery, a Warlock's own slots) recovers less than
    /// everything, which is why this takes an amount rather than always
    /// filling the pool.
    pub fn recover(&mut self, level: u32, amount: u32) {
        let i = Self::index(level);
        self.available[i] = (self.available[i] + amount).min(self.max[i]);
    }
}

/// How a creature's spell attacks and save DCs are computed - kept distinct
/// from physical weapon stats, and generic over which ability fuels it, since
/// Wisdom, Intelligence and Charisma casters share this formula and differ
/// only in which score feeds it.
///
/// Fields are public and the formula is two small methods rather than one
/// hardcoded number, so a magic item can inspect and adjust `item_bonus`
/// without reconstructing the rest of the profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpellCastingProfile {
    pub ability: Ability,
    pub ability_modifier: i32,
    pub proficiency_bonus: i32,
    /// A flat bonus from equipment: a +1 spell focus, a Rod of the Pact
    /// Keeper. Kept separate from the other two fields so an item can be
    /// swapped without recomputing them.
    pub item_bonus: i32,
}

impl SpellCastingProfile {
    pub fn new(ability: Ability, ability_modifier: i32, proficiency_bonus: i32) -> Self {
        Self {
            ability,
            ability_modifier,
            proficiency_bonus,
            item_bonus: 0,
        }
    }

    pub fn with_item_bonus(mut self, bonus: i32) -> Self {
        self.item_bonus = bonus;
        self
    }

    /// Spell attack modifier: ability modifier + proficiency bonus + item bonus.
    pub fn attack_bonus(&self) -> i32 {
        self.ability_modifier + self.proficiency_bonus + self.item_bonus
    }

    /// Spell save DC: 8 + ability modifier + proficiency bonus + item bonus.
    pub fn save_dc(&self) -> i32 {
        8 + self.attack_bonus()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::Creature;

    #[test]
    fn spell_slots_deduct_on_cast_and_come_back_on_a_long_rest() {
        let mut slots = SpellSlots::new();
        slots.set_max(1, 4);
        slots.set_max(3, 2);
        // Untouched levels stay at zero, whether or not they were ever set.
        assert_eq!(slots.available(2), 0);
        assert_eq!(slots.max(2), 0);

        assert!(slots.cast(1));
        assert!(slots.cast(1));
        assert_eq!(slots.available(1), 2);
        assert_eq!(slots.max(1), 4, "casting never touches the maximum");

        assert!(slots.cast(3));
        assert!(slots.cast(3));
        assert_eq!(slots.available(3), 0);
        // Nothing left at 3rd: casting fails rather than going negative or
        // borrowing from another level.
        assert!(!slots.cast(3));
        assert_eq!(slots.available(3), 0);

        slots.recover_all();
        assert_eq!(slots.available(1), 4);
        assert_eq!(slots.available(3), 2);
    }

    #[test]
    fn a_partial_recovery_stops_at_the_maximum() {
        let mut slots = SpellSlots::new();
        slots.set_max(2, 3);
        slots.cast(2);
        slots.cast(2);
        assert_eq!(slots.available(2), 1);

        // Recovering more than was spent still cannot exceed the max.
        slots.recover(2, 10);
        assert_eq!(slots.available(2), 3);
    }

    #[test]
    #[should_panic(expected = "1-9")]
    fn a_spell_slot_level_outside_1_to_9_panics() {
        let mut slots = SpellSlots::new();
        slots.set_max(10, 1);
    }

    /// The formula is generic over the casting ability - Wisdom here, but
    /// nothing about it hardcodes that choice.
    #[test]
    fn spell_attack_and_save_dc_follow_the_5e_formula() {
        let profile = SpellCastingProfile::new(Ability::Wis, 3, 4);
        assert_eq!(profile.attack_bonus(), 7);
        assert_eq!(profile.save_dc(), 15);

        // An item bonus is additive on top, and composable rather than baked
        // into the ability or proficiency numbers.
        let with_focus = profile.with_item_bonus(1);
        assert_eq!(with_focus.attack_bonus(), 8);
        assert_eq!(with_focus.save_dc(), 16);

        // The formula does not care which ability it is keyed to.
        let int_caster = SpellCastingProfile::new(Ability::Int, 2, 3);
        assert_eq!(int_caster.attack_bonus(), 5);
        assert_eq!(int_caster.save_dc(), 13);

        let mut creature = Creature::new("wizard", 12, 30);
        assert_eq!(creature.spell_attack_bonus(), None);
        creature.spellcasting = Some(int_caster);
        assert_eq!(creature.spell_attack_bonus(), Some(5));
        assert_eq!(creature.spell_save_dc(), Some(13));
    }
}
