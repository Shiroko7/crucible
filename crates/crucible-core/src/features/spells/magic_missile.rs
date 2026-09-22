//! Magic Missile (SRD 5.2, 1st level).

use crate::creature::{Effect, Move, MoveKind};
use crate::features::spells::{charge, parse_spell_cost, SpellCost};
use crate::features::{CreatureBuilder, FeaturePlugin, FeatureRegistry, FeatureResult};
use crate::rules::{DamageKind, DamageRoll};

/// Magic Missile (SRD 5.2, 1st-level evocation, Action): three darts, each
/// dealing `1d4 + 1` force damage, automatically hitting - no attack roll,
/// no saving throw. See [`Effect::AutoHit`] for the mechanism this rests on.
#[derive(Debug, Clone)]
pub struct MagicMissilePlugin {
    pub cost: Option<SpellCost>,
}

impl MagicMissilePlugin {
    pub fn new(cost: Option<SpellCost>) -> Self {
        Self { cost }
    }
}

impl FeaturePlugin for MagicMissilePlugin {
    fn id(&self) -> &'static str {
        "magic_missile"
    }

    fn name(&self) -> &str {
        "Magic Missile"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let darts = vec![DamageRoll::new(1, 4, 1, DamageKind::Force); 3];

        let mv = Move::new("Magic Missile", Effect::AutoHit { damage: darts })
            .with_kind(MoveKind::Spell);
        let mv = charge(builder, &self.cost, mv)?;
        builder.add_action(mv);
        Ok(())
    }
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // Magic Missile (SRD 5.2, 1st level)
    registry.register("magic_missile", |val| {
        let cost = parse_spell_cost(val)?;
        Ok(Box::new(MagicMissilePlugin::new(cost)))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::Creature;
    use crate::features::FeatureError;
    use crate::prob::{Pmf, Rng};
    use crate::rules::{Ability, Reduction, SpellCastingProfile};

    const SAMPLES: usize = 200_000;

    fn tolerance(p: f64, n: usize) -> f64 {
        5.0 * (p * (1.0 - p) / n as f64).sqrt() + 1e-4
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
    fn magic_missile_is_three_darts_of_1d4_plus_1_force() {
        let mut builder = CreatureBuilder::new("Caster", 12, 20);
        MagicMissilePlugin::new(None).apply(&mut builder).unwrap();
        let Effect::AutoHit { damage } = effect_of(&builder.creature.actions[0]) else {
            panic!("expected an AutoHit effect");
        };
        assert_eq!(damage.len(), 3);
        for roll in damage {
            assert_eq!(*roll, DamageRoll::new(1, 4, 1, DamageKind::Force));
        }
    }

    #[test]
    fn magic_missile_cost_is_injectable_and_optional() {
        let mut free_builder = CreatureBuilder::new("Caster", 12, 20);
        MagicMissilePlugin::new(None)
            .apply(&mut free_builder)
            .unwrap();
        assert!(free_builder.creature.actions[0].is_free());

        // A wand's charge pool works exactly like a spell slot pool would -
        // same mechanism, different name, entirely caller-supplied.
        let mut wanded = CreatureBuilder::new("Wand", 10, 1);
        wanded.ensure_resource("wand_charges", 7);
        MagicMissilePlugin::new(Some(SpellCost::new("wand_charges", 1)))
            .apply(&mut wanded)
            .unwrap();
        let cost = wanded.creature.actions[0].cost.expect("cost was resolved");
        assert_eq!(
            wanded.creature.resources[cost.resource].name,
            "wand_charges"
        );

        let mut unresolved = CreatureBuilder::new("Caster", 12, 20);
        let err = MagicMissilePlugin::new(Some(SpellCost::new("spell_slots_1", 1)))
            .apply(&mut unresolved)
            .expect_err("an undeclared resource must fail loudly");
        assert!(matches!(err, FeatureError::MissingResource(name) if name == "spell_slots_1"));
    }

    /// Three darts, no roll: mean damage cannot depend on the target's AC at
    /// all - the acceptance test for "bypasses AC/hit-chance entirely".
    #[test]
    fn magic_missile_ignores_ac_entirely() {
        let effect = Effect::AutoHit {
            damage: vec![DamageRoll::new(1, 4, 1, DamageKind::Force); 3],
        };
        let easy = target(1, 0, &[]);
        let impossible = target(999, 0, &[]);
        // 3 * (1d4 + 1): mean of 1d4 is 2.5, so 3 * 3.5 = 10.5.
        assert!((effect.mean_damage(&easy) - 10.5).abs() < 1e-9);
        assert_eq!(effect.mean_damage(&easy), effect.mean_damage(&impossible));
    }

    /// Reduction still applies - what Magic Missile skips is the roll, not
    /// `Creature::reduction` - so force resistance and immunity must still
    /// bite even though nothing ever rolled to hit.
    #[test]
    fn magic_missile_still_respects_force_resistance_and_immunity() {
        let effect = Effect::AutoHit {
            damage: vec![DamageRoll::new(1, 4, 1, DamageKind::Force); 3],
        };
        let bare = target(15, 0, &[]);
        let resistant = target(15, 0, &[(DamageKind::Force, Reduction::Resistant)]);
        let immune = target(15, 0, &[(DamageKind::Force, Reduction::Immune)]);

        assert!(effect.mean_damage(&resistant) < effect.mean_damage(&bare));
        assert_eq!(effect.mean_damage(&immune), 0.0);
    }

    /// The exact-vs-sampled agreement the project is built to require of
    /// every damage source, applied to the one effect that skips a roll
    /// entirely: three independent `DamageRoll`s convolved on the exact side
    /// must match three summed samples on the sampled side.
    #[test]
    fn magic_missile_damage_agrees_exact_vs_sampled() {
        fn agree(name: &str, seed: u64, exact: &Pmf, mut draw: impl FnMut(&mut Rng) -> i32) {
            let mut rng = Rng::new(seed);
            let (lo, hi) = (exact.min(), exact.max());
            assert!(lo >= 0, "{name}: damage should never be negative");
            let mut counts = vec![0usize; (hi - lo + 1) as usize];
            for _ in 0..SAMPLES {
                let d = draw(&mut rng);
                assert!(
                    d >= lo && d <= hi,
                    "{name}: sampled {d} outside the exact support {lo}..={hi}"
                );
                counts[(d - lo) as usize] += 1;
            }
            for (i, &c) in counts.iter().enumerate() {
                let value = lo + i as i32;
                let want = exact.prob(value);
                let got = c as f64 / SAMPLES as f64;
                let tol = tolerance(want, SAMPLES);
                assert!(
                    (got - want).abs() < tol,
                    "{name}: P(damage = {value}) sampled {got:.5}, exact {want:.5}, tolerance {tol:.5}"
                );
            }
        }

        let darts = vec![DamageRoll::new(1, 4, 1, DamageKind::Force); 3];
        let effect = Effect::AutoHit {
            damage: darts.clone(),
        };

        let cases: Vec<(&str, Creature)> = vec![
            ("bare target", target(15, 0, &[])),
            (
                "force-resistant target",
                target(15, 0, &[(DamageKind::Force, Reduction::Resistant)]),
            ),
            (
                "unhittable-by-AC target (irrelevant here, but must still agree)",
                target(999, 0, &[]),
            ),
        ];

        for (seed, (name, defender)) in cases.into_iter().enumerate() {
            let exact = effect.damage_pmf(&defender);
            agree(name, seed as u64 + 500, &exact, |rng| {
                darts
                    .iter()
                    .map(|roll| roll.sample(rng, false, defender.reduction(roll.kind)))
                    .sum()
            });
        }
    }

    fn spellcaster() -> CreatureBuilder {
        let mut builder = CreatureBuilder::new("Wizard", 12, 30);
        builder.set_spellcasting(SpellCastingProfile::new(Ability::Int, 4, 3));
        builder
    }

    #[test]
    fn magic_missile_builds_from_toml_and_spends_a_named_pool() {
        let registry = FeatureRegistry::new();
        let mut builder = spellcaster();
        builder.ensure_resource("wand_charges", 7);

        let params: toml::Value =
            toml::from_str("plugin = \"magic_missile\"\nresource = \"wand_charges\"").unwrap();
        let plugin = registry
            .build_plugin("magic_missile", &params)
            .expect("magic_missile builds from toml");
        assert_eq!(plugin.id(), "magic_missile");
        plugin.apply(&mut builder).unwrap();

        let cost = builder.creature.actions[0].cost.expect("cost resolved");
        assert_eq!(
            builder.creature.resources[cost.resource].name,
            "wand_charges"
        );
        assert_eq!(cost.amount, 1);
        assert_eq!(builder.creature.actions[0].spell_slot_level, None);
    }
}
