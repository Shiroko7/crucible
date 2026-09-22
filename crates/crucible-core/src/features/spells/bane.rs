//! Bane (SRD 5.2, 1st level concentration).
//!
//! A concentration spell whose mechanism is an ongoing attack-roll and
//! saving-throw modifier rather than a condition. It registers nothing new:
//! [`crate::rules::AttackModifier::BonusDice`] /
//! [`crate::rules::AttackModifier::PenaltyDice`] and their
//! [`crate::rules::SaveModifier`] siblings already exist for exactly this
//! (see `rules::attack`'s module docs), and `sim::fight` already knows how to
//! apply them for the duration of a concentration spell and strip them when
//! concentration ends - see [`crate::creature::Effect::Buff`] and
//! [`crate::creature::Effect::SaveOrModifier`]. This module only builds the
//! [`crate::creature::Move`] that reaches for that mechanism.

use crate::creature::{Effect, Move, MoveKind};
use crate::features::{CreatureBuilder, FeaturePlugin, FeatureRegistry, FeatureResult};
use crate::rules::{Ability, AttackModifier, SaveModifier};

/// Bane (1st level, concentration, up to 1 minute): up to three creatures
/// each make a Charisma save against the caster's own spell save DC - read
/// from [`crate::rules::SpellCastingProfile`] at the moment the
/// spell resolves, never a fixed number baked in here - or subtract `1d4`
/// from every attack roll and every saving throw they make for the
/// duration.
#[derive(Debug, Clone, Copy, Default)]
pub struct BanePlugin;

impl FeaturePlugin for BanePlugin {
    fn id(&self) -> &'static str {
        "bane"
    }

    fn name(&self) -> &str {
        "Bane"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        builder.add_action(
            Move::new(
                "Bane",
                Effect::SaveOrModifier {
                    ability: Ability::Cha,
                    attack_modifier: AttackModifier::PenaltyDice { count: 1, sides: 4 },
                    save_modifier: SaveModifier::PenaltyDice { count: 1, sides: 4 },
                    max_targets: Some(3),
                },
            )
            .with_concentration()
            .with_spell_slot(1)
            .with_kind(MoveKind::Spell),
        );
        Ok(())
    }
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // Bane (SRD 5.2, 1st level concentration)
    registry.register("bane", |_val| Ok(Box::new(BanePlugin)));
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn bane_registers_a_concentration_action_gated_on_a_charisma_save() {
        let builder = CreatureBuilder::new("Warlock", 15, 30);
        let built = builder
            .apply_feature(&BanePlugin)
            .expect("bane applies")
            .build()
            .expect("builds");
        assert_eq!(built.actions.len(), 1);
        let m = &built.actions[0];
        assert_eq!(m.name, "Bane");
        assert!(m.concentration, "Bane requires concentration");
        assert_eq!(m.spell_slot_level, Some(1), "Bane spends a 1st-level slot");
        assert_eq!(
            m.effect,
            Effect::SaveOrModifier {
                ability: Ability::Cha,
                attack_modifier: AttackModifier::PenaltyDice { count: 1, sides: 4 },
                save_modifier: SaveModifier::PenaltyDice { count: 1, sides: 4 },
                max_targets: Some(3),
            }
        );
    }
}
