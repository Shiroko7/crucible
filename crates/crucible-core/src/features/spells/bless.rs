//! Bless (SRD 5.2, 1st level concentration).
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
use crate::rules::{AttackModifier, SaveModifier};

/// Bless (1st level, concentration, up to 1 minute): up to three creatures -
/// the caster included - each add `1d4` to every attack roll and every
/// saving throw they make for the duration, their own concentration save
/// among them. That last part is correct 5e text, not a bug: Bless can help
/// a blessed caster hold their own concentration, and nothing here has to
/// special-case "the target of the buff is also the one rolling the save"
/// for that to happen - `sim::fight` resolves every saving throw a fighter
/// makes through the same modifier list, its own concentration check
/// included.
#[derive(Debug, Clone, Copy, Default)]
pub struct BlessPlugin;

impl FeaturePlugin for BlessPlugin {
    fn id(&self) -> &'static str {
        "bless"
    }

    fn name(&self) -> &str {
        "Bless"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        builder.add_action(
            Move::new(
                "Bless",
                Effect::Buff {
                    attack_modifier: AttackModifier::BonusDice { count: 1, sides: 4 },
                    save_modifier: SaveModifier::BonusDice { count: 1, sides: 4 },
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
    // Bless (SRD 5.2, 1st level concentration)
    registry.register("bless", |_val| Ok(Box::new(BlessPlugin)));
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn bless_registers_a_concentration_action_with_bonus_dice_on_attacks_and_saves() {
        let builder = CreatureBuilder::new("Cleric", 15, 30);
        let built = builder
            .apply_feature(&BlessPlugin)
            .expect("bless applies")
            .build()
            .expect("builds");
        assert_eq!(built.actions.len(), 1);
        let m = &built.actions[0];
        assert_eq!(m.name, "Bless");
        assert!(m.concentration, "Bless requires concentration");
        assert_eq!(m.spell_slot_level, Some(1), "Bless spends a 1st-level slot");
        assert_eq!(
            m.effect,
            Effect::Buff {
                attack_modifier: AttackModifier::BonusDice { count: 1, sides: 4 },
                save_modifier: SaveModifier::BonusDice { count: 1, sides: 4 },
                max_targets: Some(3),
            }
        );
    }
}
