//! Steady Aim (2024 Rogue 3).

use crate::creature::{Effect, Move};
use crate::features::{CreatureBuilder, FeaturePlugin, FeatureRegistry, FeatureResult};
use crate::rules::Condition;

/// Steady Aim (2024 Rogue 3): Bonus Action. Grants advantage on your own next
/// attack roll before the end of the turn, and your speed becomes 0 until the
/// end of the turn.
///
/// Modelled as a bonus-action [`Move`] whose effect is
/// [`Effect::Stance { condition: Condition::SteadyAim }`](Effect::Stance) -
/// exactly the mechanism Dodge already uses for "a condition you apply to
/// yourself that lasts until the start of your own next turn." Whichever
/// attack is resolved while [`Condition::SteadyAim`] is active gets
/// [`RollMode::Advantage`](crate::rules::RollMode::Advantage) from
/// it - see `sim::duel`'s `attack_mode`, which reads
/// [`Condition::advantage_on_attacks`] the same way it already read
/// [`Condition::disadvantage_on_attacks`] for Poisoned and Blinded. The speed
/// clause is [`Condition::zeroes_speed`]: nothing in this engine has a
/// position or a speed to zero yet (see `DESIGN.md`'s "Positioning is the gap
/// that matters"), so that flag is tracked and exposed generically rather
/// than acted on.
///
/// The move is marked [`Move::before_action`], so `sim::duel` resolves it
/// before the same turn's action - the attack it exists to set up - and the
/// attack roll uses the advantage up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SteadyAimPlugin;

impl SteadyAimPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl FeaturePlugin for SteadyAimPlugin {
    fn id(&self) -> &'static str {
        "steady_aim"
    }

    fn name(&self) -> &str {
        "Steady Aim"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        builder.add_bonus_action(
            Move::new(
                "Steady Aim",
                Effect::Stance {
                    condition: Condition::SteadyAim,
                },
            )
            .with_before_action(),
        );
        Ok(())
    }
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // Steady Aim (2024 Rogue 3)
    registry.register("steady_aim", |_val| Ok(Box::new(SteadyAimPlugin::new())));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applying_steady_aim_registers_a_bonus_action_that_applies_its_condition() {
        let builder = CreatureBuilder::new("Rogue", 15, 40);
        let built = builder
            .apply_feature(&SteadyAimPlugin::new())
            .expect("steady aim applies")
            .build()
            .expect("builds");
        assert_eq!(built.bonus_actions.len(), 1);
        let steady_aim = &built.bonus_actions[0];
        assert_eq!(steady_aim.name, "Steady Aim");
        assert_eq!(
            steady_aim.effect,
            Effect::Stance {
                condition: Condition::SteadyAim,
            }
        );
        // Free to take: it costs the bonus action slot, not a resource.
        assert!(steady_aim.is_free());
        assert!(
            steady_aim.before_action,
            "Steady Aim is taken before the attack it sets up"
        );
        // Zero damage in its own right - the advantage it grants only shows
        // up on whatever attack rolls against it, which `sim::duel`'s
        // `attack_mode` and `Condition::advantage_on_attacks` cover.
        let dummy = crate::creature::Creature::new("dummy", 10, 10);
        assert_eq!(steady_aim.effect.mean_damage(&dummy), 0.0);
        assert_eq!(steady_aim.effect.stance(), Some(Condition::SteadyAim));
    }

    #[test]
    fn steady_aim_builds_from_toml_with_no_parameters() {
        let registry = FeatureRegistry::new();
        let params: toml::Value = toml::from_str("plugin = \"steady_aim\"").unwrap();
        let plugin = registry
            .build_plugin("steady_aim", &params)
            .expect("steady_aim builds");
        assert_eq!(plugin.id(), "steady_aim");
        assert_eq!(plugin.name(), "Steady Aim");
    }
}
