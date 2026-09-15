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
pub use types::{
    Ability, Condition, Cost, Duration, Resource, Size, SpellCastingProfile, SpellSlots,
    SPELL_LEVELS,
};

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

    #[test]
    fn casting_a_spell_on_a_creature_spends_from_its_own_pool() {
        let mut caster = Creature::new("caster", 15, 20);
        caster.spell_slots.set_max(1, 2);

        assert!(caster.cast_spell(1));
        assert!(caster.cast_spell(1));
        assert!(!caster.cast_spell(1), "the pool is empty");

        caster.recover_spell_slots();
        assert!(caster.cast_spell(1), "a rest refills it");
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

    #[test]
    fn poisoned_only_burdens_its_own_attacks() {
        assert!(Condition::Poisoned.disadvantage_on_attacks());
        assert!(!Condition::Poisoned.advantage_to_attackers());
        assert!(!Condition::Poisoned.incapacitated());
        assert!(!Condition::Poisoned.auto_crits());
    }

    #[test]
    fn blinded_burdens_its_own_attacks_and_helps_attackers() {
        assert!(Condition::Blinded.disadvantage_on_attacks());
        assert!(Condition::Blinded.advantage_to_attackers());
        assert!(!Condition::Blinded.incapacitated());
        assert!(!Condition::Blinded.auto_crits());
    }

    /// Paralyzed is Stunned's three effects plus the auto-crit, not a fresh
    /// set - the compiler should catch it if a future edit to Stunned's
    /// semantics forgets its sibling.
    #[test]
    fn paralyzed_is_stunned_plus_the_close_range_crit() {
        assert!(Condition::Paralyzed.incapacitated());
        assert!(Condition::Paralyzed.advantage_to_attackers());
        assert!(Condition::Paralyzed.auto_fails(Ability::Str));
        assert!(Condition::Paralyzed.auto_fails(Ability::Dex));
        assert!(!Condition::Paralyzed.auto_fails(Ability::Con));
        assert!(Condition::Paralyzed.blocks_riders());
        assert!(Condition::Paralyzed.auto_crits());
        assert!(!Condition::Stunned.auto_crits());
    }

    #[test]
    fn condition_names_round_trip_through_parse() {
        for c in [
            Condition::Stunned,
            Condition::Dodging,
            Condition::Prone,
            Condition::Poisoned,
            Condition::Blinded,
            Condition::Paralyzed,
        ] {
            assert_eq!(Condition::parse(c.name()), Some(c));
        }
        assert_eq!(Condition::parse("paralysed"), Some(Condition::Paralyzed));
        assert_eq!(Condition::parse("nonsense"), None);
    }

    #[test]
    fn size_names_round_trip_through_parse() {
        for s in [
            Size::Tiny,
            Size::Small,
            Size::Medium,
            Size::Large,
            Size::Huge,
            Size::Gargantuan,
        ] {
            assert_eq!(Size::parse(s.name()), Some(s));
        }
        assert_eq!(Size::parse("colossal"), None);
    }

    /// Ord is the whole point of `Size` existing as a type: a size-gated
    /// effect (Cunning Strike's Trip: "Large size or smaller") compares
    /// directly instead of matching every qualifying variant.
    #[test]
    fn size_orders_smallest_to_largest() {
        assert!(Size::Tiny < Size::Small);
        assert!(Size::Large < Size::Huge);
        assert!(Size::Medium <= Size::Large);
        assert!(Size::Huge > Size::Large);
        assert!(Size::Gargantuan > Size::Large);
    }

    #[test]
    fn a_creature_defaults_to_medium_size_but_can_be_overridden() {
        let creature = Creature::new("dummy", 12, 10);
        assert_eq!(creature.size, Size::Medium);
        let huge = Creature::new("big", 12, 10).with_size(Size::Huge);
        assert_eq!(huge.size, Size::Huge);
    }
}
