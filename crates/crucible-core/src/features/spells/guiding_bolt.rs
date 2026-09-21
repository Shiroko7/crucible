//! Guiding Bolt (SRD 5.2, 1st level).

use crate::creature::{AttackKind, Effect, Move, MoveKind, Rider, Strike};
use crate::features::{
    CreatureBuilder, FeatureError, FeaturePlugin, FeatureRegistry, FeatureResult,
};
use crate::rules::{Condition, DamageKind, DamageRoll, Duration};

/// Guiding Bolt (SRD 5.2, 1st-level Evocation): a ranged spell attack for
/// 4d6 Radiant damage, using the caster's own spell attack bonus. On a hit,
/// the target is marked - [`Condition::Marked`] - so the next attack roll
/// made against it before the end of the caster's next turn, by anyone, has
/// Advantage ([`Duration::ApplierNextTurnEnd`]).
///
/// `dice_count`/`dice_sides` are plugin parameters rather than `4` and `6`
/// baked in, the same way
/// [`crate::features::classes::rogue::SneakAttackPlugin`]'s dice are - a
/// homebrew variant or a future upcast-aware caller can hand this a different
/// pool without a code change. Upcasting Guiding Bolt itself (more dice from a
/// higher-level slot) is not modelled: this always spends exactly one
/// 1st-level slot, from the same
/// [`crate::rules::SpellSlots`] pool every other 1st-level spell
/// draws on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuidingBoltPlugin {
    pub dice_count: u32,
    pub dice_sides: u32,
}

impl GuidingBoltPlugin {
    /// The printed 4d6.
    pub fn new() -> Self {
        Self::with_dice(4, 6)
    }

    /// As [`GuidingBoltPlugin::new`], with an overridden dice pool.
    pub fn with_dice(dice_count: u32, dice_sides: u32) -> Self {
        Self {
            dice_count,
            dice_sides,
        }
    }
}

impl Default for GuidingBoltPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl FeaturePlugin for GuidingBoltPlugin {
    fn id(&self) -> &'static str {
        "guiding_bolt"
    }

    fn name(&self) -> &str {
        "Guiding Bolt"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let profile = builder.creature.spellcasting.ok_or_else(|| {
            FeatureError::InvalidConfiguration(
                "guiding_bolt needs the creature's spellcasting profile set first \
                 (declare `[*.spellcasting]`, or list a spellcasting-granting feature \
                 before `guiding_bolt` in `features`)"
                    .to_string(),
            )
        })?;
        let strike = Strike::new(
            profile.attack_bonus(),
            vec![DamageRoll::new(
                self.dice_count,
                self.dice_sides,
                0,
                DamageKind::Radiant,
            )],
        )
        .with_kind(AttackKind::RANGED_SPELL);

        let action = Move::new("Guiding Bolt", Effect::Strikes { strike, count: 1 })
            .with_spell_slot(1)
            .with_kind(MoveKind::Spell)
            .with_rider(Rider::ConditionOnHit {
                condition: Condition::Marked,
                duration: Duration::ApplierNextTurnEnd,
            });

        builder.add_action(action);
        Ok(())
    }
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // Guiding Bolt (SRD 5.2, 1st level): a ranged spell attack for 4d6
    // Radiant using the caster's own `SpellCastingProfile`, marking the
    // target on a hit. `dice_count`/`dice_sides` default to the printed
    // 4d6 but can be overridden, the same way `sneak_attack`'s dice are -
    // see `spells::GuidingBoltPlugin`.
    registry.register("guiding_bolt", |val| {
        let dice_count = val
            .get("dice_count")
            .and_then(|v| v.as_integer())
            .unwrap_or(4) as u32;
        let dice_sides = val
            .get("dice_sides")
            .and_then(|v| v.as_integer())
            .unwrap_or(6) as u32;
        Ok(Box::new(GuidingBoltPlugin::with_dice(
            dice_count, dice_sides,
        )))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::spells::BlessPlugin;
    use crate::prob::Rng;
    use crate::rules::{
        damage_pmf, sample_damage, Ability, Attack, Defense, RollMode, SpellCastingProfile,
    };

    /// Distinct from [`caster_builder`]: Guiding Bolt's own tests need a
    /// caster whose 1st-level slot count varies, which a fixed profile does
    /// not give them.
    fn guiding_bolt_caster(
        ability_modifier: i32,
        proficiency_bonus: i32,
        slots: u32,
    ) -> CreatureBuilder {
        let mut builder = CreatureBuilder::new("Cleric", 16, 30);
        builder.set_spellcasting(SpellCastingProfile::new(
            Ability::Wis,
            ability_modifier,
            proficiency_bonus,
        ));
        builder.set_spell_slot_max(1, slots);
        builder
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn applying_grants_an_action_using_the_casters_computed_attack_bonus() {
        let builder = guiding_bolt_caster(3, 2, 2); // attack bonus 5
        let built = builder
            .apply_feature(&GuidingBoltPlugin::new())
            .expect("guiding bolt applies to a caster")
            .build()
            .expect("builds");

        assert_eq!(built.actions.len(), 1);
        let action = &built.actions[0];
        assert_eq!(action.name, "Guiding Bolt");
        match &action.effect {
            Effect::Strikes { strike, count } => {
                assert_eq!(*count, 1);
                assert_eq!(strike.to_hit, 5, "5 = 3 (ability mod) + 2 (proficiency)");
                assert_eq!(
                    strike.damage,
                    vec![DamageRoll::new(4, 6, 0, DamageKind::Radiant)]
                );
            }
            other => panic!("expected a Strikes effect, got {other:?}"),
        }

        // A 1st-level slot from the caster's own slot pool - no resource
        // pool of its own - and a ranged spell attack.
        assert_eq!(action.spell_slot_level, Some(1));
        assert_eq!(action.cost, None);
        assert!(built.resources.is_empty());
        assert_eq!(action.kind, MoveKind::Spell);
        let Effect::Strikes { strike, .. } = &action.effect else {
            unreachable!()
        };
        assert_eq!(strike.kind, AttackKind::RANGED_SPELL);

        // The mark, applied unconditionally on a hit, lasting until the end
        // of the caster's next turn.
        assert_eq!(
            action.riders,
            vec![Rider::ConditionOnHit {
                condition: Condition::Marked,
                duration: Duration::ApplierNextTurnEnd,
            }]
        );
    }

    /// A different profile and slot count produce a different bonus and a
    /// different pool size, proving neither was a hardcoded constant.
    #[test]
    fn a_differently_configured_caster_gets_its_own_numbers() {
        let mut builder = guiding_bolt_caster(4, 3, 1); // attack bonus 7
        builder.set_spellcasting(SpellCastingProfile::new(Ability::Wis, 4, 3).with_item_bonus(1));
        let built = builder
            .apply_feature(&GuidingBoltPlugin::new())
            .expect("guiding bolt applies")
            .build()
            .expect("builds");

        let Effect::Strikes { strike, .. } = &built.actions[0].effect else {
            panic!("expected Strikes");
        };
        assert_eq!(strike.to_hit, 8, "4 (mod) + 3 (proficiency) + 1 (item)");
        assert_eq!(built.spell_slots.max(1), 1);
    }

    /// `dice_count`/`dice_sides` are parameters, not the printed 4d6 baked
    /// in - the same test shape `sneak_attack`'s own parameterization gets.
    #[test]
    fn dice_are_a_parameter_not_a_hardcoded_constant() {
        let builder = guiding_bolt_caster(2, 2, 1);
        let built = builder
            .apply_feature(&GuidingBoltPlugin::with_dice(5, 6))
            .expect("guiding bolt applies")
            .build()
            .expect("builds");

        let Effect::Strikes { strike, .. } = &built.actions[0].effect else {
            panic!("expected Strikes");
        };
        assert_eq!(
            strike.damage,
            vec![DamageRoll::new(5, 6, 0, DamageKind::Radiant)]
        );
    }

    #[test]
    fn applying_without_a_spellcasting_profile_is_rejected() {
        // Calls the plugin's own `apply` (`&mut CreatureBuilder`) rather than
        // the builder's consuming `apply_feature`, the same way
        // `registry`'s `prestige_spellcasting_refuses_a_creature_that_does_not_qualify`
        // test does - `apply_feature` takes `self` by value and does not hand
        // it back on an `Err`, so there would be nothing left to inspect.
        let mut builder = CreatureBuilder::new("Not A Caster", 16, 30);
        assert!(matches!(
            GuidingBoltPlugin::new().apply(&mut builder),
            Err(FeatureError::InvalidConfiguration(_))
        ));
        assert!(builder.creature.actions.is_empty());
    }

    /// Guiding Bolt draws on exactly the same 1st-level slots as every
    /// other 1st-level spell - Bless here - rather than a pool of its own,
    /// so a caster with three slots gets three 1st-level casts between them,
    /// not three of each.
    #[test]
    fn a_second_first_level_spell_shares_the_same_slot_pool() {
        let built = guiding_bolt_caster(3, 2, 3)
            .apply_feature(&GuidingBoltPlugin::new())
            .expect("guiding bolt applies")
            .apply_feature(&BlessPlugin)
            .expect("bless applies")
            .build()
            .expect("builds");

        assert!(built.resources.is_empty(), "no pool of its own");
        assert_eq!(built.actions[0].spell_slot_level, Some(1));
        assert_eq!(built.actions[1].spell_slot_level, Some(1));
        assert_eq!(built.spell_slots.max(1), 3);
    }

    /// The `Strike`'s own damage distribution: a miss deals nothing, a hit
    /// or crit deals 4d6, and the sampled path must land on exactly the
    /// exact one - the project's standing exact-vs-sampled contract.
    #[test]
    fn sampled_damage_agrees_with_the_exact_path() {
        let builder = guiding_bolt_caster(4, 3, 1); // attack bonus 7
        let built = builder
            .apply_feature(&GuidingBoltPlugin::new())
            .expect("guiding bolt applies")
            .build()
            .expect("builds");
        let Effect::Strikes { strike, .. } = &built.actions[0].effect else {
            panic!("expected Strikes");
        };

        let target = crate::creature::Creature::new("target", 15, 40);
        let exact = strike.damage_pmf(&target);
        assert!(close(exact.total(), 1.0));
        assert!(exact.min() >= 0, "damage floors at zero");
        assert!(exact.prob(0) > 0.0, "a miss must be possible");
        // 4d6 tops at 24, doubled to 48 on a crit.
        assert_eq!(exact.max(), 48);

        let (lo, hi) = (exact.min(), exact.max());
        let mut rng = Rng::new(4_200);
        let n = 100_000;
        let mut counts = vec![0usize; (hi - lo + 1) as usize];
        for _ in 0..n {
            let d = strike.sample(&mut rng, &target);
            assert!(d >= lo && d <= hi, "sampled {d} outside {lo}..={hi}");
            counts[(d - lo) as usize] += 1;
        }
        for (i, &c) in counts.iter().enumerate() {
            let value = lo + i as i32;
            let want = exact.prob(value);
            let got = c as f64 / n as f64;
            let tol = 5.0 * (want * (1.0 - want) / n as f64).sqrt() + 1e-4;
            assert!(
                (got - want).abs() < tol,
                "P(damage = {value}) sampled {got:.5}, exact {want:.5}, tol {tol:.5}"
            );
        }
    }

    /// Acceptance criterion: Guiding Bolt is a spell attack roll
    /// ([`Attack::is_spell_attack`]) whose damage profile composes with the
    /// AT-02 sneak-attack-style extension exactly like a weapon attack does,
    /// as soon as the attack has Advantage - from this exact mark's own
    /// prior use, or any other source. Verified at the [`Attack`]
    /// level, the same way AT-02's own tests prove the gate, and checked
    /// exact-vs-sampled.
    #[test]
    fn guiding_bolts_profile_composes_with_the_sneak_attack_style_spell_extension_under_advantage()
    {
        let attack = Attack::new(7, 4, 6, 0)
            .with_is_spell_attack(true)
            .with_mode(RollMode::Advantage);

        // A rogue-shaped rider that opted into the spell-attack extension.
        let sneak = Rider::ConditionalExtraDamage {
            dice_count: 3,
            dice_sides: 6,
            once_per_turn: true,
        };
        let extra = sneak
            .extra_damage_for_with_spell_attack_extension(&attack, false, true)
            .expect("a spell attack at advantage should qualify with the extension");
        let with_sneak = attack.with_damage_rider(extra);

        let defense = Defense::new(14, 60);
        let exact = damage_pmf(&with_sneak, &defense);
        let (lo, hi) = (exact.min(), exact.max());
        let mut rng = Rng::new(4_300);
        let n = 100_000;
        let mut counts = vec![0usize; (hi - lo + 1) as usize];
        for _ in 0..n {
            let d = sample_damage(&mut rng, &with_sneak, &defense);
            assert!(d >= lo && d <= hi, "sampled {d} outside {lo}..={hi}");
            counts[(d - lo) as usize] += 1;
        }
        for (i, &c) in counts.iter().enumerate() {
            let value = lo + i as i32;
            let want = exact.prob(value);
            let got = c as f64 / n as f64;
            let tol = 5.0 * (want * (1.0 - want) / n as f64).sqrt() + 1e-4;
            assert!(
                (got - want).abs() < tol,
                "P(damage = {value}) sampled {got:.5}, exact {want:.5}, tol {tol:.5}"
            );
        }

        // Without the extension, the very same spell attack must not
        // qualify - the extension is what makes it additive, not automatic.
        let attack_unextended = Attack::new(7, 4, 6, 0)
            .with_is_spell_attack(true)
            .with_mode(RollMode::Advantage);
        assert_eq!(
            sneak.extra_damage_for_with_spell_attack_extension(&attack_unextended, false, false),
            None
        );
    }

    /// Guiding Bolt builds from an empty TOML table (its printed 4d6),
    /// proving the registry entry actually reaches `spells::GuidingBoltPlugin`
    /// rather than only being reachable by constructing it directly in Rust.
    #[test]
    fn guiding_bolt_defaults_to_4d6_but_can_be_overridden() {
        let registry = FeatureRegistry::new();
        let params: toml::Value = toml::from_str("plugin = \"guiding_bolt\"").unwrap();
        let plugin = registry
            .build_plugin("guiding_bolt", &params)
            .expect("guiding_bolt builds from an empty toml table");
        assert_eq!(plugin.id(), "guiding_bolt");
        assert_eq!(plugin.name(), "Guiding Bolt");

        let mut builder = crate::features::CreatureBuilder::new("Cleric", 16, 30);
        builder.set_spellcasting(crate::rules::SpellCastingProfile::new(Ability::Wis, 3, 2));
        builder.set_spell_slot_max(1, 2);
        let built = builder
            .apply_feature(plugin.as_ref())
            .expect("guiding_bolt applies to a caster")
            .build()
            .expect("builds");
        let crate::creature::Effect::Strikes { strike, .. } = &built.actions[0].effect else {
            panic!("expected a Strikes effect");
        };
        assert_eq!(
            strike.damage,
            vec![crate::rules::DamageRoll::new(
                4,
                6,
                0,
                crate::rules::DamageKind::Radiant
            )]
        );

        // A different dice pool overrides the printed default, the same way
        // `sneak_attack`'s does.
        let overridden: toml::Value =
            toml::from_str("plugin = \"guiding_bolt\"\ndice_count = 5\ndice_sides = 8").unwrap();
        let plugin = registry
            .build_plugin("guiding_bolt", &overridden)
            .expect("guiding_bolt builds with overridden dice");
        let mut builder = crate::features::CreatureBuilder::new("Cleric", 16, 30);
        builder.set_spellcasting(crate::rules::SpellCastingProfile::new(Ability::Wis, 3, 2));
        builder.set_spell_slot_max(1, 2);
        let built = builder
            .apply_feature(plugin.as_ref())
            .expect("guiding_bolt applies")
            .build()
            .expect("builds");
        let crate::creature::Effect::Strikes { strike, .. } = &built.actions[0].effect else {
            panic!("expected a Strikes effect");
        };
        assert_eq!(
            strike.damage,
            vec![crate::rules::DamageRoll::new(
                5,
                8,
                0,
                crate::rules::DamageKind::Radiant
            )]
        );
    }
}
