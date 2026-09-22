//! Healing Word (SRD 5.2, 1st level).

use crate::creature::{Effect, Move, MoveKind};
use crate::features::spells::{spellcasting_ability_modifier, BASE_SLOT_LEVEL};
use crate::features::{CreatureBuilder, FeaturePlugin, FeatureRegistry, FeatureResult};
use crate::rules::HealRoll;

/// Healing Word (SRD 5.2): Bonus Action, 60 feet, 1d4 + spellcasting ability
/// modifier. If the target is at 0 HP, it revives instead of only healing -
/// see [`crate::rules::apply_healing`].
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
            .with_spell_slot(BASE_SLOT_LEVEL)
            .with_kind(MoveKind::Spell);
        builder.add_bonus_action(heal);
        Ok(())
    }
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // Healing Word
    registry.register("healing_word", |_val| {
        Ok(Box::new(HealingWordPlugin::new()))
    });
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::features::FeatureError;
    use crate::prob::Rng;
    use crate::rules::{apply_healing, Ability, SpellCastingProfile};

    fn wisdom_caster(ability_modifier: i32, proficiency_bonus: i32) -> CreatureBuilder {
        let mut builder = CreatureBuilder::new("Cleric", 14, 30);
        builder.set_spellcasting(SpellCastingProfile::new(
            Ability::Wis,
            ability_modifier,
            proficiency_bonus,
        ));
        builder.set_spell_slot_max(1, 2);
        builder.set_spell_slot_max(2, 1);
        builder
    }

    #[test]
    fn healing_word_is_a_bonus_action_with_the_right_formula_and_slot() {
        let builder = wisdom_caster(3, 3)
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
    fn a_non_caster_is_rejected_rather_than_silently_healing_for_zero() {
        let builder = CreatureBuilder::new("Mute", 10, 10);
        let err = builder
            .apply_feature(&HealingWordPlugin::new())
            .expect_err("no spellcasting profile means no formula to bake in");
        assert!(matches!(err, FeatureError::InvalidConfiguration(_)));
    }

    #[test]
    fn casting_either_spell_deducts_a_first_level_slot() {
        let builder = wisdom_caster(3, 3)
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

    /// End to end: cast Healing Word on a downed ally - pay the slot, roll
    /// the heal, apply it, and confirm the revive.
    #[test]
    fn casting_healing_word_on_a_downed_ally_revives_them() {
        let builder = wisdom_caster(4, 3)
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
