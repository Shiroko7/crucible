//! Blindness/Deafness (SRD 5.2, 2nd level).

use crate::creature::{Effect, Move, MoveKind, SaveEffect};
use crate::features::spells::{charge, parse_spell_cost, save_dc, SpellCost};
use crate::features::{CreatureBuilder, FeaturePlugin, FeatureRegistry, FeatureResult};
use crate::rules::{Ability, Condition, Duration};

/// Blindness/Deafness (SRD 5.2, 2nd-level necromancy, Action): a Constitution
/// save or the target has the Blinded or Deafened condition - the caster's
/// choice - for the duration.
///
/// The SRD 5.2 wording is "At the end of each of its turns, the target can
/// make a Constitution saving throw. On a success, the spell ends on it" -
/// exactly the shape [`Duration::SaveEndTurn`] already exists for (see that
/// variant's own doc, and Hold Person), so this spell needs no new duration
/// mechanic. It never touches concentration because there is nothing here to
/// touch: this branch has no concentration tracker at all (ARCH-02), and
/// Blindness/Deafness would not use one anyway - it is one of the SRD's
/// non-concentration save-each-turn spells.
#[derive(Debug, Clone)]
pub struct BlindnessDeafnessPlugin {
    /// `false` blinds, `true` deafens - the caster's choice the spell text
    /// grants, made a constructor parameter rather than two separate plugins.
    pub deafen: bool,
    pub cost: Option<SpellCost>,
}

impl BlindnessDeafnessPlugin {
    pub fn new(deafen: bool, cost: Option<SpellCost>) -> Self {
        Self { deafen, cost }
    }

    fn label(&self) -> &'static str {
        if self.deafen {
            "Blindness/Deafness (Deafen)"
        } else {
            "Blindness/Deafness (Blind)"
        }
    }
}

impl FeaturePlugin for BlindnessDeafnessPlugin {
    fn id(&self) -> &'static str {
        "blindness_deafness"
    }

    fn name(&self) -> &str {
        self.label()
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let dc = save_dc(builder, "Blindness/Deafness")?;
        let condition = if self.deafen {
            Condition::Deafened
        } else {
            Condition::Blinded
        };

        let mv = Move::new(
            self.label(),
            Effect::Save(SaveEffect {
                ability: Ability::Con,
                dc,
                damage: Vec::new(),
                half_on_success: false,
                on_failure: vec![(
                    condition,
                    Duration::SaveEndTurn {
                        ability: Ability::Con,
                        dc,
                    },
                )],
                max_targets: Some(1),
                requires_type: None,
            }),
        )
        .with_kind(MoveKind::Spell);
        let mv = charge(builder, &self.cost, mv)?;
        builder.add_action(mv);
        Ok(())
    }
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // Blindness/Deafness (SRD 5.2, 2nd level)
    registry.register("blindness_deafness", |val| {
        let deafen = val.get("deafen").and_then(|v| v.as_bool()).unwrap_or(false);
        let cost = parse_spell_cost(val)?;
        Ok(Box::new(BlindnessDeafnessPlugin::new(deafen, cost)))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::Creature;
    use crate::features::FeatureError;
    use crate::prob::Rng;
    use crate::rules::{DamageKind, Reduction, SpellCastingProfile};

    const SAMPLES: usize = 200_000;

    fn tolerance(p: f64, n: usize) -> f64 {
        5.0 * (p * (1.0 - p) / n as f64).sqrt() + 1e-4
    }

    fn caster(ability: Ability, modifier: i32, proficiency: i32) -> CreatureBuilder {
        let mut builder = CreatureBuilder::new("Caster", 12, 20);
        builder.set_spellcasting(SpellCastingProfile::new(ability, modifier, proficiency));
        builder
    }

    fn target(ac: i32, save: i32, reductions: &[(DamageKind, Reduction)]) -> Creature {
        let mut c = Creature::new("target", ac, 1_000);
        c.saves = [save; 6];
        c.reductions = reductions.to_vec();
        c
    }

    fn effect_of(m: &Move) -> &Effect {
        &m.effect
    }

    #[test]
    fn blindness_deafness_needs_a_spellcasting_profile() {
        let mut builder = CreatureBuilder::new("Caster", 12, 20);
        let err = BlindnessDeafnessPlugin::new(false, None)
            .apply(&mut builder)
            .expect_err("no spellcasting profile means no save DC to compute");
        assert!(matches!(err, FeatureError::InvalidConfiguration(_)));
    }

    #[test]
    fn blindness_deafness_blinds_by_default_and_deafens_on_request() {
        let mut builder = caster(Ability::Wis, 3, 3); // DC 8 + 3 + 3 = 14

        BlindnessDeafnessPlugin::new(false, None)
            .apply(&mut builder)
            .expect("blinds cleanly");
        let Effect::Save(save) = effect_of(&builder.creature.actions[0]) else {
            panic!("expected a Save effect");
        };
        assert_eq!(save.ability, Ability::Con);
        assert_eq!(save.dc, 14);
        assert!(save.damage.is_empty(), "no damage, only a condition");
        assert_eq!(save.max_targets, Some(1));
        assert_eq!(
            save.on_failure,
            vec![(
                Condition::Blinded,
                Duration::SaveEndTurn {
                    ability: Ability::Con,
                    dc: 14
                }
            )]
        );

        BlindnessDeafnessPlugin::new(true, None)
            .apply(&mut builder)
            .expect("deafens cleanly");
        let Effect::Save(save) = effect_of(&builder.creature.actions[1]) else {
            panic!("expected a Save effect");
        };
        assert_eq!(
            save.on_failure,
            vec![(
                Condition::Deafened,
                Duration::SaveEndTurn {
                    ability: Ability::Con,
                    dc: 14
                }
            )]
        );
    }

    /// Never mentions - or needs - a concentration tracker: this branch has
    /// none (ARCH-02), and the spell would not use one even if it did.
    #[test]
    fn blindness_deafness_never_touches_concentration() {
        let mut builder = caster(Ability::Wis, 3, 3);
        BlindnessDeafnessPlugin::new(false, None)
            .apply(&mut builder)
            .unwrap();
        // The only state this spell adds is the one action; nothing about
        // applying it reaches for a resource, rider, or field named
        // "concentration" because no such thing exists on `Creature`.
        assert_eq!(builder.creature.actions.len(), 1);
        assert!(builder.creature.riders.is_empty());
    }

    #[test]
    fn blindness_deafness_cost_is_injectable_and_optional() {
        // With no cost declared, the move is free.
        let mut free_builder = caster(Ability::Wis, 3, 3);
        BlindnessDeafnessPlugin::new(false, None)
            .apply(&mut free_builder)
            .unwrap();
        assert!(free_builder.creature.actions[0].is_free());

        // Naming a resource that was never declared is a configuration
        // error, not a silent free cast.
        let mut unresolved = caster(Ability::Wis, 3, 3);
        let err = BlindnessDeafnessPlugin::new(false, Some(SpellCost::new("spell_slots_2", 1)))
            .apply(&mut unresolved)
            .expect_err("an undeclared resource must fail loudly");
        assert!(matches!(err, FeatureError::MissingResource(name) if name == "spell_slots_2"));

        // Once the resource exists, the same plugin spends from it instead -
        // "how it's paid for" is a parameter, not a hardcoded slot.
        let mut wired = caster(Ability::Wis, 3, 3);
        wired.ensure_resource("spell_slots_2", 3);
        BlindnessDeafnessPlugin::new(false, Some(SpellCost::new("spell_slots_2", 1)))
            .apply(&mut wired)
            .unwrap();
        let cost = wired.creature.actions[0].cost.expect("cost was resolved");
        assert_eq!(cost.amount, 1);
        assert_eq!(
            wired.creature.resources[cost.resource].name,
            "spell_slots_2"
        );
    }

    /// The exact/sampled agreement the project's whole `README.md` is built
    /// around, applied to a spell whose only randomness is the save itself -
    /// there is no damage roll to compare, so this is `SaveEffect`'s own
    /// closed-form failure chance against a Monte Carlo estimate of the same
    /// event.
    #[test]
    fn blindness_deafness_failure_chance_agrees_with_sampled_saves() {
        let mut builder = caster(Ability::Wis, 4, 3); // DC 15
        BlindnessDeafnessPlugin::new(false, None)
            .apply(&mut builder)
            .unwrap();
        let Effect::Save(save) = effect_of(&builder.creature.actions[0]) else {
            panic!("expected a Save effect");
        };

        let victim = target(14, 1, &[]);
        let exact = save.failure_chance(&victim);

        let mut rng = Rng::new(2024);
        let mut fails = 0u32;
        for _ in 0..SAMPLES {
            if !save.roll_save(&mut rng, &victim, false) {
                fails += 1;
            }
        }
        let sampled = f64::from(fails) / SAMPLES as f64;
        let tol = tolerance(exact, SAMPLES);
        assert!(
            (sampled - exact).abs() < tol,
            "sampled fail rate {sampled:.5} vs exact {exact:.5}, tolerance {tol:.5}"
        );
    }

    fn spellcaster() -> CreatureBuilder {
        let mut builder = CreatureBuilder::new("Wizard", 12, 30);
        builder.set_spellcasting(SpellCastingProfile::new(Ability::Int, 4, 3));
        builder
    }

    #[test]
    fn blindness_deafness_builds_from_toml_with_a_choice_of_condition() {
        let registry = FeatureRegistry::new();
        let mut builder = spellcaster();

        let params: toml::Value =
            toml::from_str("plugin = \"blindness_deafness\"\ndeafen = true").unwrap();
        let plugin = registry
            .build_plugin("blindness_deafness", &params)
            .expect("blindness_deafness builds from toml");
        plugin.apply(&mut builder).unwrap();

        let Effect::Save(save) = &builder.creature.actions[0].effect else {
            panic!("expected a Save effect");
        };
        assert_eq!(save.on_failure[0].0, Condition::Deafened);
    }
}
