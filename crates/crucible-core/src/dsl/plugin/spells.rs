//! Healing and triage spells: Healing Word and Cure Wounds (SRD 5.2, 2024
//! rules). Also Spiritual Weapon: see [`SpiritualWeaponPlugin`] below.
//!
//! Both healing spells restore hit points using `1..2 dice + the caster's
//! spellcasting ability modifier`, read from the creature's own
//! [`crate::rules::creature::SpellCastingProfile`] rather than a hardcoded
//! number, and both spend one 1st-level spell slot from the caster's
//! [`crate::rules::creature::SpellSlots`].
//!
//! Reviving a creature at 0 HP is not spell-specific text - it is 5e's
//! general "a creature that regains any hit points while it has 0 becomes
//! conscious" rule - so it is implemented once, in
//! [`crate::rules::creature::apply_healing`], and inherited by both spells
//! (and anything else that ever heals) rather than re-implemented per spell.
//!
//! Not modelled, on purpose:
//! - **Range.** `DESIGN.md` already rules positioning out of scope entirely
//!   ("Positioning is the gap that matters") - there is no notion of distance
//!   for a melee weapon either, so Healing Word's 60 feet and Cure Wounds'
//!   touch are flavour text here, not a mechanic.
//! - **Ally targeting.** The duel engine (`sim::duel`) only ever targets the
//!   opposing side right now - no move of any kind can target a friendly
//!   creature yet. These plugins produce fully-formed, fully-testable
//!   `Move`s (the right action economy, slot cost, and heal formula), but
//!   wiring "cast this on a bloodied ally" into the automated turn engine is
//!   a separate, considerably larger feature (self/ally targeting for every
//!   effect, plus a policy that decides when to heal) and is left for a
//!   follow-up rather than bolted on here.
//! - **Upcasting.** Both healing spells are implemented as their base
//!   1st-level cast only; scaling the healing dice with a higher slot is
//!   skipped.

use crate::rules::creature::{DamageKind, DamageRoll, Effect, HealRoll, Move, Strike, Uses};

use super::traits::{CreatureBuilder, FeatureError, FeaturePlugin, FeatureResult};

/// Both healing spells are cast here at their base, 1st-level, rate. See the
/// module doc: upcasting is out of scope.
const BASE_SLOT_LEVEL: u32 = 1;

/// The ability modifier a healing spell adds, read off the creature's own
/// casting profile - never a hardcoded number.
fn spellcasting_ability_modifier(
    builder: &CreatureBuilder,
    spell_name: &str,
) -> FeatureResult<i32> {
    builder
        .creature
        .spellcasting
        .map(|profile| profile.ability_modifier)
        .ok_or_else(|| {
            FeatureError::InvalidConfiguration(format!(
                "{spell_name} needs a [*.spellcasting] profile to compute its healing"
            ))
        })
}

/// Healing Word (SRD 5.2): Bonus Action, 60 feet, 1d4 + spellcasting ability
/// modifier. If the target is at 0 HP, it revives instead of only healing -
/// see [`crate::rules::creature::apply_healing`].
#[derive(Debug, Clone, Copy, Default)]
pub struct HealingWordPlugin;

impl HealingWordPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl FeaturePlugin for HealingWordPlugin {
    fn id(&self) -> &'static str {
        "healing_word"
    }

    fn name(&self) -> &str {
        "Healing Word"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let modifier = spellcasting_ability_modifier(builder, "Healing Word")?;
        let heal = Move::new("Healing Word", Effect::Heal(HealRoll::new(1, 4, modifier)))
            .with_spell_slot(BASE_SLOT_LEVEL);
        builder.add_bonus_action(heal);
        Ok(())
    }
}

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
            .with_spell_slot(BASE_SLOT_LEVEL);
        builder.add_action(heal);
        Ok(())
    }
}

/// Spiritual Weapon (SRD 5.2, 2nd level, Bonus Action, up to 1 minute, no
/// concentration): summons a spectral weapon that immediately makes a melee
/// spell attack - `1d8 + spellcasting ability modifier` Force damage on a
/// hit, using the caster's own
/// [`crate::rules::creature::SpellCastingProfile::attack_bonus`], never a
/// hardcoded number - and on every later round, an identical Bonus Action
/// strike is available again at no further cost.
///
/// Registers two Bonus Actions rather than one, which is exactly the split
/// the mechanic itself needs: the *initial* cast pays a 2nd-level slot and
/// can only ever be taken once (`Uses::Limited(1)` - a caster does not
/// re-pay to keep swinging a weapon it has already summoned), while
/// `"Spiritual Weapon (Strike Again)"` is unlimited and free. Both moves are
/// otherwise identical, so a policy ranking by mean damage (every policy but
/// `InOrder`, which just takes the first legal move in list order) prefers
/// whichever it can currently afford, in list order: the paying move while
/// the slot is unspent, the free one afterwards. That is "pay once, then
/// repeat" with no new cross-move bookkeeping - see
/// `sim::duel`'s own `spiritual_weapons_initial_cast_spends_a_slot_and_the_repeat_strike_does_not`
/// test for the mechanism actually firing across rounds.
///
/// Deliberately does **not** call [`Move::with_concentration`] on either
/// move: unlike most spells that maintain a lasting effect, Spiritual
/// Weapon does not require concentration at all (it lasts on its own for
/// its duration), so summoning it never ends whatever the caster was
/// already concentrating on - see `sim::duel`'s
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
    /// Both bonus actions share this strike profile; only their `Uses` and
    /// `spell_level` differ.
    fn strike(to_hit: i32, ability_modifier: i32) -> Effect {
        Effect::Strikes {
            strike: Strike::new(
                to_hit,
                vec![DamageRoll::new(1, 8, ability_modifier, DamageKind::Force)],
            ),
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
                .with_spell_level(2),
        );
        builder.add_bonus_action(Move::new(
            "Spiritual Weapon (Strike Again)",
            Self::strike(to_hit, ability_modifier),
        ));

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prob::rng::Rng;
    use crate::rules::combat::{Attack, DamageRider, RollMode};
    use crate::rules::creature::{
        apply_healing, is_down, Ability, Creature, Rider, SpellCastingProfile,
    };

    fn wisdom_caster(ability_modifier: i32) -> CreatureBuilder {
        let mut builder = CreatureBuilder::new("Cleric", 14, 30);
        builder.set_spellcasting(SpellCastingProfile::new(Ability::Wis, ability_modifier, 3));
        builder.set_spell_slot_max(1, 2);
        builder
    }

    fn caster_builder(profile: SpellCastingProfile) -> CreatureBuilder {
        let mut builder = CreatureBuilder::new("Cleric", 15, 30);
        builder.set_spellcasting(profile);
        builder
    }

    #[test]
    fn healing_word_is_a_bonus_action_with_the_right_formula_and_slot() {
        let builder = wisdom_caster(3)
            .apply_feature(&HealingWordPlugin::new())
            .expect("Healing Word applies to a caster");

        assert!(builder.creature.actions.is_empty());
        assert_eq!(builder.creature.bonus_actions.len(), 1);
        let mv = &builder.creature.bonus_actions[0];
        assert_eq!(mv.name, "Healing Word");
        assert_eq!(mv.spell_slot_level, Some(1));
        match &mv.effect {
            Effect::Heal(roll) => {
                assert_eq!((roll.count, roll.sides, roll.bonus), (1, 4, 3));
            }
            other => panic!("expected a Heal effect, got {other:?}"),
        }
    }

    #[test]
    fn cure_wounds_is_an_action_with_the_right_formula_and_slot() {
        let builder = wisdom_caster(2)
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

    /// Neither spell hardcodes its modifier: two different casters produce
    /// two different heal formulas.
    #[test]
    fn the_modifier_comes_from_the_casting_profile_not_a_constant() {
        let low = wisdom_caster(0)
            .apply_feature(&CureWoundsPlugin::new())
            .unwrap();
        let high = wisdom_caster(5)
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
    fn a_non_caster_is_rejected_rather_than_silently_healing_for_zero() {
        let builder = CreatureBuilder::new("Mute", 10, 10);
        let err = builder
            .apply_feature(&HealingWordPlugin::new())
            .expect_err("no spellcasting profile means no formula to bake in");
        assert!(matches!(err, FeatureError::InvalidConfiguration(_)));
    }

    #[test]
    fn casting_either_spell_deducts_a_first_level_slot() {
        let builder = wisdom_caster(3)
            .apply_feature(&HealingWordPlugin::new())
            .unwrap();
        let mut caster = builder.creature;
        let word = caster.bonus_actions[0].clone();

        assert_eq!(caster.spell_slots.available(1), 2);
        assert!(word.pay_spell_cost(&mut caster));
        assert_eq!(caster.spell_slots.available(1), 1);
        assert!(word.pay_spell_cost(&mut caster));
        assert_eq!(caster.spell_slots.available(1), 0);
        // The pool is empty: casting fails rather than going negative.
        assert!(!word.pay_spell_cost(&mut caster));
        assert_eq!(caster.spell_slots.available(1), 0);
    }

    /// A move with no `spell_slot_level` (every other move in the game so
    /// far) must not be affected by this: `pay_spell_cost` is a no-op that
    /// always succeeds.
    #[test]
    fn a_move_without_a_spell_slot_pays_nothing() {
        let mut caster = Creature::new("Fighter", 16, 40);
        let punch = Move::new(
            "Punch",
            Effect::Strikes {
                strike: crate::rules::creature::Strike::new(
                    5,
                    vec![crate::rules::creature::DamageRoll::new(
                        1,
                        4,
                        2,
                        crate::rules::creature::DamageKind::Bludgeoning,
                    )],
                ),
                count: 1,
            },
        );
        assert!(punch.pay_spell_cost(&mut caster));
    }

    /// The core invariant this whole project is built on: the exact
    /// distribution and many samples of the same roll must agree, applied
    /// here to healing instead of damage.
    #[test]
    fn healing_word_amount_matches_between_exact_and_sampled() {
        let roll = HealRoll::new(1, 4, 3);
        let exact = roll.pmf();
        assert!((exact.total() - 1.0).abs() < 1e-12);
        assert_eq!((exact.min(), exact.max()), (4, 7));

        let mut rng = Rng::new(7);
        const N: usize = 200_000;
        let mut total = 0i64;
        for _ in 0..N {
            let sampled = roll.sample(&mut rng);
            assert!((4..=7).contains(&sampled));
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

    #[test]
    fn healing_from_zero_revives_and_healing_from_positive_does_not() {
        let (new_hp, revived) = apply_healing(0, 30, 5);
        assert_eq!(new_hp, 5);
        assert!(revived, "regaining HP from 0 wakes the creature up");

        let (new_hp, revived) = apply_healing(12, 30, 5);
        assert_eq!(new_hp, 17);
        assert!(!revived, "never went down, so there is nothing to revive");
    }

    /// This engine never clamps HP at zero (a fighter's HP can read
    /// negative from overkill damage), so "down" has to mean "at or below
    /// zero", not "exactly zero".
    #[test]
    fn a_deeply_negative_target_still_revives_once_healed_past_zero() {
        assert!(is_down(-15));
        let (new_hp, revived) = apply_healing(-15, 30, 20);
        assert_eq!(new_hp, 5);
        assert!(revived);
    }

    /// Healing that does not clear zero leaves the creature down - reaching
    /// exactly 0 is still "at 0 HP", not revived, matching 5e's own wording.
    #[test]
    fn healing_that_does_not_cross_zero_does_not_revive() {
        let (new_hp, revived) = apply_healing(-15, 30, 10);
        assert_eq!(new_hp, -5);
        assert!(!revived);

        let (new_hp, revived) = apply_healing(-10, 30, 10);
        assert_eq!(new_hp, 0);
        assert!(!revived, "landing exactly on 0 is still down");
    }

    #[test]
    fn healing_never_exceeds_max_hp() {
        let (new_hp, _) = apply_healing(28, 30, 100);
        assert_eq!(new_hp, 30);
    }

    /// End to end: cast Healing Word on a downed ally - pay the slot, roll
    /// the heal, apply it, and confirm the revive.
    #[test]
    fn casting_healing_word_on_a_downed_ally_revives_them() {
        let builder = wisdom_caster(4)
            .apply_feature(&HealingWordPlugin::new())
            .unwrap();
        let mut caster = builder.creature;
        let word = caster.bonus_actions[0].clone();

        assert!(word.pay_spell_cost(&mut caster));

        let Effect::Heal(roll) = &word.effect else {
            unreachable!()
        };
        let mut rng = Rng::new(42);
        let healed = roll.sample(&mut rng);
        assert!((5..=8).contains(&healed), "1d4 + 4 is 5..=8");

        let ally_max_hp = 24;
        let (new_hp, revived) = apply_healing(0, ally_max_hp, healed);
        assert!(new_hp > 0);
        assert!(revived);
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
            cast.spell_level,
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
            strike_again.spell_level, None,
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
    /// `rider.rs`'s own tests exercise, checked concretely against this
    /// spell's `1d8 + ability modifier` numbers rather than an arbitrary
    /// fixture.
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
        // the same accessor `sim::duel` would consult.
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
}
