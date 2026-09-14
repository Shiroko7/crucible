//! Healing and triage spells: Healing Word and Cure Wounds (SRD 5.2, 2024
//! rules).
//!
//! Both restore hit points using `1..2 dice + the caster's spellcasting
//! ability modifier`, read from the creature's own
//! [`crate::rules::creature::SpellCastingProfile`] rather than a hardcoded
//! number, and both spend one 1st-level spell slot from the caster's
//! [`crate::rules::creature::SpellSlots`].
//!
//! Reviving a creature at 0 HP is not spell-specific text - it is 5e's
//! general "a creature that regains any hit points while it has 0 becomes
//! conscious" rule - so it is implemented once, in
//! [`crate::rules::creature::apply_healing`], and inherited by both spells
//! (and anything else that ever heals) rather than re-implemented per spell.
//!
//! Not modelled, on purpose:
//! - **Range.** `DESIGN.md` already rules positioning out of scope entirely
//!   ("Positioning is the gap that matters") - there is no notion of distance
//!   for a melee weapon either, so Healing Word's 60 feet and Cure Wounds'
//!   touch are flavour text here, not a mechanic.
//! - **Ally targeting.** The duel engine (`sim::duel`) only ever targets the
//!   opposing side right now - no move of any kind can target a friendly
//!   creature yet. These plugins produce fully-formed, fully-testable
//!   `Move`s (the right action economy, slot cost, and heal formula), but
//!   wiring "cast this on a bloodied ally" into the automated turn engine is
//!   a separate, considerably larger feature (self/ally targeting for every
//!   effect, plus a policy that decides when to heal) and is left for a
//!   follow-up rather than bolted on here.
//! - **Upcasting.** Both spells are implemented as their base 1st-level
//!   cast only; scaling the healing dice with a higher slot is skipped.

use crate::rules::creature::{Effect, HealRoll, Move};

use super::traits::{CreatureBuilder, FeatureError, FeaturePlugin, FeatureResult};

/// Both spells are cast here at their base, 1st-level, rate. See the module
/// doc: upcasting is out of scope.
const BASE_SLOT_LEVEL: u32 = 1;

/// The ability modifier a healing spell adds, read off the creature's own
/// casting profile - never a hardcoded number.
fn spellcasting_ability_modifier(builder: &CreatureBuilder, spell_name: &str) -> FeatureResult<i32> {
    builder
        .creature
        .spellcasting
        .map(|profile| profile.ability_modifier)
        .ok_or_else(|| {
            FeatureError::InvalidConfiguration(format!(
                "{spell_name} needs a [*.spellcasting] profile to compute its healing"
            ))
        })
}

/// Healing Word (SRD 5.2): Bonus Action, 60 feet, 1d4 + spellcasting ability
/// modifier. If the target is at 0 HP, it revives instead of only healing -
/// see [`crate::rules::creature::apply_healing`].
#[derive(Debug, Clone, Copy, Default)]
pub struct HealingWordPlugin;

impl HealingWordPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl FeaturePlugin for HealingWordPlugin {
    fn id(&self) -> &'static str {
        "healing_word"
    }

    fn name(&self) -> &str {
        "Healing Word"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let modifier = spellcasting_ability_modifier(builder, "Healing Word")?;
        let heal = Move::new("Healing Word", Effect::Heal(HealRoll::new(1, 4, modifier)))
            .with_spell_slot(BASE_SLOT_LEVEL);
        builder.add_bonus_action(heal);
        Ok(())
    }
}

/// Cure Wounds (SRD 5.2, 2024 rules): Action, touch, 2d8 + spellcasting
/// ability modifier. Base 1st-level cast only - see the module doc.
#[derive(Debug, Clone, Copy, Default)]
pub struct CureWoundsPlugin;

impl CureWoundsPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl FeaturePlugin for CureWoundsPlugin {
    fn id(&self) -> &'static str {
        "cure_wounds"
    }

    fn name(&self) -> &str {
        "Cure Wounds"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let modifier = spellcasting_ability_modifier(builder, "Cure Wounds")?;
        let heal = Move::new("Cure Wounds", Effect::Heal(HealRoll::new(2, 8, modifier)))
            .with_spell_slot(BASE_SLOT_LEVEL);
        builder.add_action(heal);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prob::rng::Rng;
    use crate::rules::creature::{apply_healing, is_down, Ability, Creature, SpellCastingProfile};

    fn wisdom_caster(ability_modifier: i32) -> CreatureBuilder {
        let mut builder = CreatureBuilder::new("Cleric", 14, 30);
        builder.set_spellcasting(SpellCastingProfile::new(Ability::Wis, ability_modifier, 3));
        builder.set_spell_slot_max(1, 2);
        builder
    }

    #[test]
    fn healing_word_is_a_bonus_action_with_the_right_formula_and_slot() {
        let builder = wisdom_caster(3)
            .apply_feature(&HealingWordPlugin::new())
            .expect("Healing Word applies to a caster");

        assert!(builder.creature.actions.is_empty());
        assert_eq!(builder.creature.bonus_actions.len(), 1);
        let mv = &builder.creature.bonus_actions[0];
        assert_eq!(mv.name, "Healing Word");
        assert_eq!(mv.spell_slot_level, Some(1));
        match &mv.effect {
            Effect::Heal(roll) => {
                assert_eq!((roll.count, roll.sides, roll.bonus), (1, 4, 3));
            }
            other => panic!("expected a Heal effect, got {other:?}"),
        }
    }

    #[test]
    fn cure_wounds_is_an_action_with_the_right_formula_and_slot() {
        let builder = wisdom_caster(2)
            .apply_feature(&CureWoundsPlugin::new())
            .expect("Cure Wounds applies to a caster");

        assert!(builder.creature.bonus_actions.is_empty());
        assert_eq!(builder.creature.actions.len(), 1);
        let mv = &builder.creature.actions[0];
        assert_eq!(mv.name, "Cure Wounds");
        assert_eq!(mv.spell_slot_level, Some(1));
        match &mv.effect {
            Effect::Heal(roll) => {
                assert_eq!((roll.count, roll.sides, roll.bonus), (2, 8, 2));
            }
            other => panic!("expected a Heal effect, got {other:?}"),
        }
    }

    /// Neither spell hardcodes its modifier: two different casters produce
    /// two different heal formulas.
    #[test]
    fn the_modifier_comes_from_the_casting_profile_not_a_constant() {
        let low = wisdom_caster(0)
            .apply_feature(&CureWoundsPlugin::new())
            .unwrap();
        let high = wisdom_caster(5)
            .apply_feature(&CureWoundsPlugin::new())
            .unwrap();
        let Effect::Heal(low_roll) = &low.creature.actions[0].effect else {
            unreachable!()
        };
        let Effect::Heal(high_roll) = &high.creature.actions[0].effect else {
            unreachable!()
        };
        assert_eq!(low_roll.bonus, 0);
        assert_eq!(high_roll.bonus, 5);
    }

    #[test]
    fn a_non_caster_is_rejected_rather_than_silently_healing_for_zero() {
        let builder = CreatureBuilder::new("Mute", 10, 10);
        let err = builder
            .apply_feature(&HealingWordPlugin::new())
            .expect_err("no spellcasting profile means no formula to bake in");
        assert!(matches!(err, FeatureError::InvalidConfiguration(_)));
    }

    #[test]
    fn casting_either_spell_deducts_a_first_level_slot() {
        let builder = wisdom_caster(3)
            .apply_feature(&HealingWordPlugin::new())
            .unwrap();
        let mut caster = builder.creature;
        let word = caster.bonus_actions[0].clone();

        assert_eq!(caster.spell_slots.available(1), 2);
        assert!(word.pay_spell_cost(&mut caster));
        assert_eq!(caster.spell_slots.available(1), 1);
        assert!(word.pay_spell_cost(&mut caster));
        assert_eq!(caster.spell_slots.available(1), 0);
        // The pool is empty: casting fails rather than going negative.
        assert!(!word.pay_spell_cost(&mut caster));
        assert_eq!(caster.spell_slots.available(1), 0);
    }

    /// A move with no `spell_slot_level` (every other move in the game so
    /// far) must not be affected by this: `pay_spell_cost` is a no-op that
    /// always succeeds.
    #[test]
    fn a_move_without_a_spell_slot_pays_nothing() {
        let mut caster = Creature::new("Fighter", 16, 40);
        let punch = Move::new(
            "Punch",
            Effect::Strikes {
                strike: crate::rules::creature::Strike::new(
                    5,
                    vec![crate::rules::creature::DamageRoll::new(
                        1,
                        4,
                        2,
                        crate::rules::creature::DamageKind::Bludgeoning,
                    )],
                ),
                count: 1,
            },
        );
        assert!(punch.pay_spell_cost(&mut caster));
    }

    /// The core invariant this whole project is built on: the exact
    /// distribution and many samples of the same roll must agree, applied
    /// here to healing instead of damage.
    #[test]
    fn healing_word_amount_matches_between_exact_and_sampled() {
        let roll = HealRoll::new(1, 4, 3);
        let exact = roll.pmf();
        assert!((exact.total() - 1.0).abs() < 1e-12);
        assert_eq!((exact.min(), exact.max()), (4, 7));

        let mut rng = Rng::new(7);
        const N: usize = 200_000;
        let mut total = 0i64;
        for _ in 0..N {
            let sampled = roll.sample(&mut rng);
            assert!((4..=7).contains(&sampled));
            total += i64::from(sampled);
        }
        let mean_sampled = total as f64 / N as f64;
        let tol = 5.0 * (exact.variance() / N as f64).sqrt() + 1e-3;
        assert!(
            (mean_sampled - exact.mean()).abs() < tol,
            "sampled mean {mean_sampled} vs exact {}",
            exact.mean()
        );
    }

    #[test]
    fn cure_wounds_amount_matches_between_exact_and_sampled() {
        let roll = HealRoll::new(2, 8, 4);
        let exact = roll.pmf();
        assert_eq!((exact.min(), exact.max()), (6, 20));

        let mut rng = Rng::new(11);
        const N: usize = 200_000;
        let mut total = 0i64;
        for _ in 0..N {
            let sampled = roll.sample(&mut rng);
            assert!((6..=20).contains(&sampled));
            total += i64::from(sampled);
        }
        let mean_sampled = total as f64 / N as f64;
        let tol = 5.0 * (exact.variance() / N as f64).sqrt() + 1e-3;
        assert!(
            (mean_sampled - exact.mean()).abs() < tol,
            "sampled mean {mean_sampled} vs exact {}",
            exact.mean()
        );
    }

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

    /// End to end: cast Healing Word on a downed ally - pay the slot, roll
    /// the heal, apply it, and confirm the revive.
    #[test]
    fn casting_healing_word_on_a_downed_ally_revives_them() {
        let builder = wisdom_caster(4)
            .apply_feature(&HealingWordPlugin::new())
            .unwrap();
        let mut caster = builder.creature;
        let word = caster.bonus_actions[0].clone();

        assert!(word.pay_spell_cost(&mut caster));

        let Effect::Heal(roll) = &word.effect else {
            unreachable!()
        };
        let mut rng = Rng::new(42);
        let healed = roll.sample(&mut rng);
        assert!((5..=8).contains(&healed), "1d4 + 4 is 5..=8");

        let ally_max_hp = 24;
        let (new_hp, revived) = apply_healing(0, ally_max_hp, healed);
        assert!(new_hp > 0);
        assert!(revived);
    }
}
