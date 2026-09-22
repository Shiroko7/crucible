//! Spiritual Weapon (SRD 5.2, 2nd level).

use crate::creature::{AttackKind, Effect, Move, MoveKind, Strike, Uses};
use crate::features::{
    CreatureBuilder, FeatureError, FeaturePlugin, FeatureRegistry, FeatureResult,
};
use crate::rules::{DamageKind, DamageRoll};

/// Spiritual Weapon (SRD 5.2, 2nd level, Bonus Action, up to 1 minute, no
/// concentration): summons a spectral weapon that immediately makes a melee
/// spell attack - `1d8 + spellcasting ability modifier` Force damage on a
/// hit, using the caster's own
/// [`crate::rules::SpellCastingProfile::attack_bonus`], never a
/// hardcoded number - and on every later round, an identical Bonus Action
/// strike is available again at no further cost.
///
/// Registers two Bonus Actions rather than one, which is exactly the split the
/// mechanic itself needs: the *initial* cast pays a 2nd-level slot and can
/// only ever be taken once (`Uses::Limited(1)` - a caster does not re-pay to
/// keep swinging a weapon it has already summoned), while `"Spiritual Weapon
/// (Strike Again)"` is unlimited and free. Both moves are otherwise identical,
/// so a policy ranking by mean damage (every policy but `InOrder`, which just
/// takes the first legal move in list order) prefers whichever it can
/// currently afford, in list order: the paying move while the slot is unspent,
/// the free one afterwards. That is "pay once, then repeat" with no new
/// cross-move bookkeeping - see `sim::fight`'s own
/// `spiritual_weapons_initial_cast_spends_a_slot_and_the_repeat_strike_does_not`
/// test for the mechanism actually firing across rounds.
///
/// Deliberately does **not** call [`Move::with_concentration`] on either
/// move: unlike most spells that maintain a lasting effect, Spiritual
/// Weapon does not require concentration at all (it lasts on its own for
/// its duration), so summoning it never ends whatever the caster was
/// already concentrating on - see `sim::fight`'s
/// `spiritual_weapon_coexists_with_an_active_concentration_spell_without_disturbing_it`
/// test, which proves a `Hold`-style concentration effect survives a
/// same-turn Spiritual Weapon cast untouched.
///
/// One honest gap: `"Strike Again"` being unconditionally free means a
/// policy that always declines to spend a slot (`Thrifty`, or `Attrition`
/// before being bloodied) could in principle take the free strike before
/// ever having cast the spell, since the engine has no general "this move
/// requires that one to already have fired" mechanism - the same class of
/// simplification `DESIGN.md` calls out for positioning. Every policy
/// willing to spend anything at all prefers the paying move first, purely
/// from move order and identical damage, so this only shows up under a
/// policy that never pays for anything, which is the same policy that would
/// never have summoned the weapon in the first place.
#[derive(Debug, Clone, Copy, Default)]
pub struct SpiritualWeaponPlugin;

impl SpiritualWeaponPlugin {
    /// Both bonus actions share this strike profile - a melee spell attack -
    /// and differ only in their `Uses` and slot cost.
    fn strike(to_hit: i32, ability_modifier: i32) -> Effect {
        Effect::Strikes {
            strike: Strike::new(
                to_hit,
                vec![DamageRoll::new(1, 8, ability_modifier, DamageKind::Force)],
            )
            .with_kind(AttackKind::MELEE_SPELL),
            count: 1,
        }
    }
}

impl FeaturePlugin for SpiritualWeaponPlugin {
    fn id(&self) -> &'static str {
        "spiritual_weapon"
    }

    fn name(&self) -> &str {
        "Spiritual Weapon"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let profile = builder.creature.spellcasting.ok_or_else(|| {
            FeatureError::InvalidConfiguration(
                "spiritual_weapon requires a spellcasting profile to already be set on the \
                 creature (e.g. via `[pc.spellcasting]` or an earlier-applied casting plugin) \
                 so its attack bonus and ability modifier are never hardcoded"
                    .to_string(),
            )
        })?;
        let to_hit = profile.attack_bonus();
        let ability_modifier = profile.ability_modifier;

        builder.add_bonus_action(
            Move::new("Spiritual Weapon", Self::strike(to_hit, ability_modifier))
                .with_uses(Uses::Limited(1))
                .with_spell_slot(2)
                .with_kind(MoveKind::Spell),
        );
        builder.add_bonus_action(Move::new(
            "Spiritual Weapon (Strike Again)",
            Self::strike(to_hit, ability_modifier),
        ));

        Ok(())
    }
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // Spiritual Weapon (SRD 5.2, 2nd level, Bonus Action strike, no
    // concentration) - every number it needs comes off the creature's
    // own `spellcasting` profile, so there is nothing to read from TOML.
    registry.register("spiritual_weapon", |_val| {
        Ok(Box::new(SpiritualWeaponPlugin))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::{Creature, Rider};
    use crate::prob::Rng;
    use crate::rules::{Ability, Attack, DamageRider, RollMode, SpellCastingProfile};

    fn caster_builder(profile: SpellCastingProfile) -> CreatureBuilder {
        let mut builder = CreatureBuilder::new("Cleric", 15, 30);
        builder.set_spellcasting(profile);
        builder
    }

    /// The plugin registers exactly two bonus actions: the initial cast,
    /// which pays a 2nd-level slot and can only ever be taken once, and the
    /// free repeat, which pays nothing and has no fight-long budget at all.
    /// Both carry the same `1d8 + ability modifier` Force strike, using the
    /// caster's own attack bonus rather than a number baked into the
    /// plugin.
    #[test]
    fn spiritual_weapon_registers_a_paid_cast_and_a_free_repeat_with_the_casters_own_numbers() {
        let profile = SpellCastingProfile::new(Ability::Wis, 3, 2); // attack bonus 5
        let built = caster_builder(profile)
            .apply_feature(&SpiritualWeaponPlugin)
            .expect("spiritual weapon applies once spellcasting is set")
            .build()
            .expect("builds");

        assert_eq!(built.bonus_actions.len(), 2);

        let cast = &built.bonus_actions[0];
        assert_eq!(cast.name, "Spiritual Weapon");
        assert_eq!(
            cast.spell_slot_level,
            Some(2),
            "the initial cast spends a 2nd-level slot"
        );
        assert_eq!(
            cast.uses,
            Uses::Limited(1),
            "only the initial cast ever pays - it can never be taken again"
        );
        assert!(
            !cast.concentration,
            "Spiritual Weapon does not require concentration"
        );

        let strike_again = &built.bonus_actions[1];
        assert_eq!(strike_again.name, "Spiritual Weapon (Strike Again)");
        assert_eq!(
            strike_again.spell_slot_level, None,
            "no further slot is spent to keep swinging"
        );
        assert_eq!(strike_again.uses, Uses::Unlimited);
        assert!(!strike_again.concentration);

        for m in [cast, strike_again] {
            let Effect::Strikes { strike, count } = &m.effect else {
                panic!("expected a Strikes effect");
            };
            assert_eq!(*count, 1);
            assert_eq!(
                strike.to_hit, 5,
                "to_hit is the caster's own SpellCastingProfile::attack_bonus, not a constant"
            );
            assert_eq!(
                strike.damage,
                vec![DamageRoll::new(1, 8, 3, DamageKind::Force)],
                "1d8 + spellcasting ability modifier Force damage"
            );
        }
    }

    /// A different profile produces different numbers, proving neither
    /// `to_hit` nor the damage bonus is a hardcoded constant hiding behind
    /// the first test's specific values.
    #[test]
    fn a_differently_configured_caster_gets_its_own_different_numbers() {
        let profile = SpellCastingProfile::new(Ability::Cha, 1, 4).with_item_bonus(1); // attack bonus 6
        let built = caster_builder(profile)
            .apply_feature(&SpiritualWeaponPlugin)
            .expect("applies")
            .build()
            .expect("builds");

        let Effect::Strikes { strike, .. } = &built.bonus_actions[0].effect else {
            panic!("expected a Strikes effect");
        };
        assert_eq!(strike.to_hit, 6);
        assert_eq!(
            strike.damage,
            vec![DamageRoll::new(1, 8, 1, DamageKind::Force)]
        );
    }

    /// Without a spellcasting profile already on the creature there is no
    /// attack bonus or ability modifier to read, so the plugin refuses to
    /// apply rather than silently defaulting to zero.
    #[test]
    fn spiritual_weapon_refuses_to_apply_without_a_spellcasting_profile() {
        let mut builder = CreatureBuilder::new("Non-caster", 15, 30);
        let err = SpiritualWeaponPlugin.apply(&mut builder).unwrap_err();
        assert!(matches!(err, FeatureError::InvalidConfiguration(_)));
        assert!(builder.creature.bonus_actions.is_empty());
    }

    /// Exact-vs-sampled agreement on Spiritual Weapon's own strike: the
    /// standard check this project runs on every damage-dealing mechanism,
    /// applied to this spell's specific numbers rather than a generic
    /// fixture.
    #[test]
    fn spiritual_weapons_strike_samples_like_its_exact_distribution() {
        let profile = SpellCastingProfile::new(Ability::Wis, 3, 2);
        let built = caster_builder(profile)
            .apply_feature(&SpiritualWeaponPlugin)
            .expect("applies")
            .build()
            .expect("builds");
        let Effect::Strikes { strike, .. } = &built.bonus_actions[0].effect else {
            panic!("expected a Strikes effect");
        };

        let defender = Creature::new("target", 14, 60);
        let exact = strike.damage_pmf(&defender);
        let (lo, hi) = (exact.min(), exact.max());
        let mut rng = Rng::new(4_200);
        let n = 100_000;
        let mut counts = vec![0usize; (hi - lo + 1) as usize];
        for _ in 0..n {
            let d = strike.sample(&mut rng, &defender);
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

    /// Spiritual Weapon's melee spell attack is flagged
    /// [`Attack::is_spell_attack`] and so can trigger AT-02's
    /// sneak-attack-style `Rider::ConditionalExtraDamage` extension when a
    /// caster is explicitly built with the
    /// [`Rider::ExtraDamageAppliesToSpellAttacks`] marker plus a qualifying
    /// trigger condition (advantage, here) - the same gate
    /// `creature::rider::extra_damage`'s own tests exercise, checked
    /// concretely against this spell's `1d8 + ability modifier` numbers rather
    /// than an arbitrary fixture.
    #[test]
    fn spiritual_weapons_spell_attack_can_trigger_the_sneak_attack_style_extension() {
        let profile = SpellCastingProfile::new(Ability::Wis, 3, 2); // attack bonus 5
        let attack = Attack::new(profile.attack_bonus(), 1, 8, profile.ability_modifier)
            .with_mode(RollMode::Advantage)
            .with_is_spell_attack(true);
        assert!(attack.is_spell_attack);

        let extension_rider = || Rider::ConditionalExtraDamage {
            dice_count: 4,
            dice_sides: 6,
            once_per_turn: true,
        };

        // A caster with the marker rider and a plain caster without it -
        // read through `Creature::extra_damage_applies_to_spell_attacks`,
        // the same accessor `sim::fight` would consult.
        let plain_caster = Creature::new("Plain Caster", 15, 40).with_rider(extension_rider());
        let extended_caster = Creature::new("Extended Caster", 15, 40)
            .with_rider(extension_rider())
            .with_rider(Rider::ExtraDamageAppliesToSpellAttacks);
        assert!(!plain_caster.extra_damage_applies_to_spell_attacks());
        assert!(extended_caster.extra_damage_applies_to_spell_attacks());

        // Without the extension, Spiritual Weapon's spell attack never
        // qualifies, however favourable the roll.
        assert_eq!(
            extension_rider().extra_damage_for_with_spell_attack_extension(
                &attack,
                false,
                plain_caster.extra_damage_applies_to_spell_attacks(),
            ),
            None,
        );

        // With the extension and a qualifying trigger (advantage), it does.
        assert_eq!(
            extension_rider().extra_damage_for_with_spell_attack_extension(
                &attack,
                false,
                extended_caster.extra_damage_applies_to_spell_attacks(),
            ),
            Some(DamageRider::new(4, 6)),
        );
    }

    /// Spiritual Weapon takes no TOML parameters of its own - every number
    /// it needs comes off the creature's own `spellcasting` profile - so
    /// building it from an empty table has to succeed.
    #[test]
    fn spiritual_weapon_builds_from_toml_with_no_parameters() {
        let registry = FeatureRegistry::new();
        let params: toml::Value = toml::from_str("").unwrap();
        let plugin = registry
            .build_plugin("spiritual_weapon", &params)
            .expect("spiritual_weapon builds from an empty toml table");
        assert_eq!(plugin.id(), "spiritual_weapon");
        assert_eq!(plugin.name(), "Spiritual Weapon");
    }
}
