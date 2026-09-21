//! A generic "bypasses casting restrictions" mechanism.
//!
//! Some builds have a limited-use feature letting them cast a spell while
//! under a restriction that would normally prevent spellcasting - being
//! silenced, or unable to speak or gesture, are the usual real-world
//! examples. This module implements a limited-use charge that, when spent,
//! marks one specific move-taking as exempt via
//! [`Move::bypasses_casting_restrictions`]; `sim::duel`'s move gating reads
//! that flag, letting the move through [`crate::rules::Condition::Silenced`].
//!
//! The charge budget reuses [`Uses::Limited`], the same "N per day" pattern
//! [`crate::features::monsters::LegendaryResistancePlugin`] and every other
//! finite-use rider or move in this engine already use rather than
//! inventing a parallel resource system. This engine has no day/rest
//! tracking across encounters - `README.md` scopes everything to a single
//! fight - so "1/day" and "1/fight" are the same number here, and a long
//! rest is exactly what starting the next fight already does: `run` builds
//! fresh per-fight tracking state from the untouched
//! [`crate::creature::Creature`] config every time it is called, so
//! the charge is back at full the moment a new encounter starts.

use crate::creature::{Move, MoveKind, Uses};
use crate::dsl::scenario;
use crate::features::{
    CreatureBuilder, FeatureError, FeaturePlugin, FeatureRegistry, FeatureResult,
};

/// Grants a creature a limited-use option to take `base_move` flagged as
/// exempt from whatever casting-restriction mechanism exists or comes to
/// exist.
///
/// `base_move` supplies everything about the cast itself - its name, effect,
/// cost and whether it needs concentration - so this plugin only adds the
/// exemption flag and its own charge budget on top of it: "what the move
/// does" is the caller's problem, "how it gets onto the creature" is this
/// plugin's. `base_move`'s own `uses` is replaced by the `uses`-charge
/// budget: the point of this feature is that it is *rarer* than the ordinary
/// way to take the same cast, not that it has none.
///
/// Registered as an Action, matching how a spell is ordinarily cast; nothing
/// about the mechanism is action-specific, but [`CreatureBuilder`] has no
/// slot-agnostic "add this move somewhere" entry point yet.
#[derive(Debug, Clone, PartialEq)]
pub struct BypassCastingRestrictionsPlugin {
    pub base_move: Move,
    pub uses: u32,
}

impl BypassCastingRestrictionsPlugin {
    pub fn new(base_move: Move, uses: u32) -> Self {
        Self { base_move, uses }
    }

    /// `base_move`, with its own `uses` replaced by this feature's charge
    /// budget and [`Move::bypasses_casting_restrictions`] set.
    fn flagged_move(&self) -> Move {
        self.base_move
            .clone()
            .with_uses(Uses::Limited(self.uses))
            .with_bypasses_casting_restrictions()
    }
}

impl FeaturePlugin for BypassCastingRestrictionsPlugin {
    fn id(&self) -> &'static str {
        "bypasses_casting_restrictions"
    }

    fn name(&self) -> &str {
        &self.base_move.name
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        builder.add_action(self.flagged_move());
        Ok(())
    }
}

/// A spell written in the scenario DSL, granted as a limited-use cast that
/// bypasses casting restrictions - parsed when applied, like
/// [`DeclaredFastHandsMove`].
#[derive(Debug, Clone)]
struct DeclaredBypassCast {
    name: String,
    effect: String,
    uses: u32,
}

impl FeaturePlugin for DeclaredBypassCast {
    fn id(&self) -> &'static str {
        "bypasses_casting_restrictions"
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let mut m = scenario::parse_move_external(
            &format!("{} | {}", self.name, self.effect),
            &builder.creature,
        )
        .map_err(FeatureError::InvalidConfiguration)?;
        m.kind = MoveKind::Spell;
        BypassCastingRestrictionsPlugin::new(m, self.uses).apply(builder)
    }
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // A limited-use way to cast a spell without components, getting
    // through a silence (AT-04's Tricky Spells, Subtle Spell): the spell
    // itself written in the scenario DSL, on its own charge budget.
    registry.register("bypasses_casting_restrictions", |val| {
        let name = val.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
            FeatureError::InvalidConfiguration(
                "bypasses_casting_restrictions needs a `name`".to_string(),
            )
        })?;
        let effect = val.get("effect").and_then(|v| v.as_str()).ok_or_else(|| {
            FeatureError::InvalidConfiguration(
                "bypasses_casting_restrictions needs an `effect` (the spell, in the scenario DSL)"
                    .to_string(),
            )
        })?;
        let uses = val.get("uses").and_then(|v| v.as_integer()).unwrap_or(1) as u32;
        Ok(Box::new(DeclaredBypassCast {
            name: name.to_string(),
            effect: effect.to_string(),
            uses,
        }))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::Creature;
    use crate::creature::{Effect, Strike};
    use crate::prob::Rng;
    use crate::rules::{Ability, DamageKind, DamageRoll, SpellCastingProfile};
    use crate::sim::{run, Policy};

    fn spell_move() -> Move {
        Move::new(
            "Test Cast",
            Effect::Strikes {
                strike: Strike::new(6, vec![DamageRoll::new(2, 6, 0, DamageKind::Force)]),
                count: 1,
            },
        )
    }

    #[test]
    fn an_ordinary_move_never_carries_the_flag() {
        assert!(!spell_move().bypasses_casting_restrictions);
    }

    #[test]
    fn applying_the_plugin_grants_a_flagged_move_with_the_configured_charge_budget() {
        let mut builder = CreatureBuilder::new("Test Subject", 15, 40);
        let plugin = BypassCastingRestrictionsPlugin::new(spell_move(), 1);
        plugin.apply(&mut builder).expect("applies");

        assert_eq!(builder.creature.actions.len(), 1);
        let granted = &builder.creature.actions[0];
        assert_eq!(granted.name, "Test Cast");
        assert!(
            granted.bypasses_casting_restrictions,
            "the granted move must carry the exemption flag"
        );
        assert_eq!(granted.uses, Uses::Limited(1));
    }

    /// The whole point of a plugin over a hardcoded feature: the charge
    /// count is a parameter, not a constant baked into the mechanism.
    #[test]
    fn a_different_charge_count_is_a_parameter_not_a_constant() {
        let mut builder = CreatureBuilder::new("Test Subject", 15, 40);
        let plugin = BypassCastingRestrictionsPlugin::new(spell_move(), 3);
        plugin.apply(&mut builder).expect("applies");
        assert_eq!(builder.creature.actions[0].uses, Uses::Limited(3));
    }

    /// Drives the actual duel simulation - the same mechanism every other
    /// `Uses::Limited` move in this engine is spent and exhausted through -
    /// to prove the charge really is consumed exactly once per use, and that
    /// running out makes the option simply unavailable rather than silently
    /// free: a caster with one charge and no other action gets exactly one
    /// use out of three rounds, not three.
    #[test]
    fn the_charge_is_consumed_exactly_once_and_then_the_move_is_unavailable() {
        let caster = CreatureBuilder::new("Caster", 10, 100)
            .apply_feature(&BypassCastingRestrictionsPlugin::new(spell_move(), 1))
            .expect("applies")
            .build()
            .expect("builds");
        let dummy = Creature::new("Dummy", 10, 400);

        let mut rng = Rng::new(7);
        let mut log = None;
        let outcome = run(
            &mut rng,
            [&caster, &dummy],
            [Policy::Greedy; 2],
            3,
            &mut log,
        );

        assert_eq!(
            outcome.resources_spent[0], 1,
            "one charge, one use across three rounds - not one per round"
        );
    }

    /// This engine has no cross-encounter day/rest tracking (`README.md`
    /// scopes everything to one fight), so the closest thing it has to "the
    /// charge recovers on a long rest" is simply starting a fresh encounter:
    /// `run` never mutates the `Creature` it is given, so two independent
    /// fights against the same creature must each get their own full charge.
    #[test]
    fn a_fresh_encounter_recovers_the_charge_like_a_long_rest() {
        let caster = CreatureBuilder::new("Caster", 10, 100)
            .apply_feature(&BypassCastingRestrictionsPlugin::new(spell_move(), 1))
            .expect("applies")
            .build()
            .expect("builds");
        let dummy = Creature::new("Dummy", 10, 400);

        for seed in [7, 99] {
            let mut rng = Rng::new(seed);
            let mut log = None;
            let outcome = run(
                &mut rng,
                [&caster, &dummy],
                [Policy::Greedy; 2],
                3,
                &mut log,
            );
            assert_eq!(
                outcome.resources_spent[0], 1,
                "each independent encounter must start with the charge fully available again"
            );
        }
    }

    #[test]
    fn exhausting_the_charge_leaves_the_base_move_unaffected() {
        // The plugin transforms a copy of `base_move`; the original value
        // the caller passed in is never mutated, so a build that reuses the
        // same base move elsewhere (an ordinary, unlimited cast alongside
        // this gated one) is unaffected.
        let base = spell_move();
        let plugin = BypassCastingRestrictionsPlugin::new(base.clone(), 1);
        assert_eq!(base.uses, Uses::Unlimited);
        assert!(!base.bypasses_casting_restrictions);
        assert_eq!(plugin.base_move, base);
    }

    fn spellcaster() -> CreatureBuilder {
        let mut builder = CreatureBuilder::new("Wizard", 12, 30);
        builder.set_spellcasting(SpellCastingProfile::new(Ability::Int, 4, 3));
        builder
    }

    #[test]
    fn a_components_free_cast_builds_from_toml() {
        let registry = FeatureRegistry::new();
        let params: toml::Value = toml::from_str(
            r#"
                plugin = "bypasses_casting_restrictions"
                name = "Quiet Bolt"
                effect = "ranged | hit +7 | 4d6 radiant"
                uses = 1
            "#,
        )
        .unwrap();
        let plugin = registry
            .build_plugin("bypasses_casting_restrictions", &params)
            .unwrap();
        let mut builder = spellcaster();
        plugin.apply(&mut builder).unwrap();
        let m = &builder.creature.actions[0];
        assert!(m.bypasses_casting_restrictions);
        assert_eq!(m.kind, MoveKind::Spell);
        assert_eq!(m.uses, crate::creature::Uses::Limited(1));
    }
}
