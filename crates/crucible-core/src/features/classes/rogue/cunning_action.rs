//! Cunning Action (2024 Rogue 2).

use crate::creature::{Effect, Move};
use crate::features::{CreatureBuilder, FeaturePlugin, FeatureRegistry, FeatureResult};

/// Cunning Action (2024 Rogue 2): Bonus Action. Take the Dash or Disengage
/// action as a bonus action instead of spending your action on it.
///
/// Registered as two separate bonus-action [`Move`]s rather than one, because
/// a [`crate::sim::duel::Plan`] only ever picks a single bonus action out of
/// the whole list regardless of how many are on it - offering both is exactly
/// "either one, never both" with no extra bookkeeping needed.
///
/// Neither move does anything mechanically here. Dash (double speed) and
/// Disengage (moving away provokes no opportunity attacks) are both about
/// movement and positioning, and this engine has neither (see `DESIGN.md`'s
/// "Positioning is the gap that matters") - so both are
/// `Effect::Sequence(Vec::new())`, a legal, zero-damage, zero-rider move a
/// plan can still select and spend the bonus-action slot on, rather than
/// invented movement mechanics standing in for rules that do not exist yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CunningActionPlugin;

impl CunningActionPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl FeaturePlugin for CunningActionPlugin {
    fn id(&self) -> &'static str {
        "cunning_action"
    }

    fn name(&self) -> &str {
        "Cunning Action"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        builder.add_bonus_action(Move::new(
            "Dash (Bonus Action)",
            Effect::Sequence(Vec::new()),
        ));
        builder.add_bonus_action(Move::new(
            "Disengage (Bonus Action)",
            Effect::Sequence(Vec::new()),
        ));
        Ok(())
    }
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // Cunning Action (2024 Rogue 2)
    registry.register("cunning_action", |_val| {
        Ok(Box::new(CunningActionPlugin::new()))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applying_cunning_action_registers_dash_and_disengage_as_free_bonus_actions() {
        let builder = CreatureBuilder::new("Rogue", 15, 40);
        let built = builder
            .apply_feature(&CunningActionPlugin::new())
            .expect("cunning action applies")
            .build()
            .expect("builds");
        assert_eq!(built.bonus_actions.len(), 2);
        let names: Vec<&str> = built
            .bonus_actions
            .iter()
            .map(|m| m.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["Dash (Bonus Action)", "Disengage (Bonus Action)"]
        );

        let dummy = crate::creature::Creature::new("dummy", 10, 10);
        for m in &built.bonus_actions {
            // Both are free to take (no resource cost, unlimited uses) and
            // have no mechanical effect - there is no movement model for
            // either to act on yet, but a plan can still legally select them.
            assert!(m.is_free());
            assert_eq!(m.effect, Effect::Sequence(Vec::new()));
            assert_eq!(m.effect.mean_damage(&dummy), 0.0);
            assert_eq!(m.effect.stance(), None);
        }
    }

    #[test]
    fn cunning_action_builds_from_toml_with_no_parameters() {
        let registry = FeatureRegistry::new();
        let params: toml::Value = toml::from_str("plugin = \"cunning_action\"").unwrap();
        let plugin = registry
            .build_plugin("cunning_action", &params)
            .expect("cunning_action builds");
        assert_eq!(plugin.id(), "cunning_action");
        assert_eq!(plugin.name(), "Cunning Action");
        let mut builder = crate::features::CreatureBuilder::new("Rogue", 15, 40);
        plugin.apply(&mut builder).unwrap();
        assert_eq!(builder.creature.bonus_actions.len(), 2);
    }
}
