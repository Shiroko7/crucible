//! Cure Wounds (SRD 5.2, 1st level).

use crate::creature::{Effect, Move, MoveKind};
use crate::features::spells::{spellcasting_ability_modifier, BASE_SLOT_LEVEL};
use crate::features::{CreatureBuilder, FeaturePlugin, FeatureRegistry, FeatureResult};
use crate::rules::HealRoll;

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
            .with_spell_slot(BASE_SLOT_LEVEL)
            .with_kind(MoveKind::Spell);
        builder.add_action(heal);
        Ok(())
    }
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // Cure Wounds
    registry.register("cure_wounds", |_val| Ok(Box::new(CureWoundsPlugin::new())));
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::prob::Rng;
    use crate::rules::{Ability, SpellCastingProfile};

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
    fn cure_wounds_is_an_action_with_the_right_formula_and_slot() {
        let builder = wisdom_caster(2, 3)
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

    /// Neither healing spell hardcodes its modifier: two different casters
    /// produce two different heal formulas.
    #[test]
    fn the_modifier_comes_from_the_casting_profile_not_a_constant() {
        let low = wisdom_caster(0, 3)
            .apply_feature(&CureWoundsPlugin::new())
            .unwrap();
        let high = wisdom_caster(5, 3)
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
}
