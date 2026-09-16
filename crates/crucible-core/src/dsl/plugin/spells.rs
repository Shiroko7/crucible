//! Healing and triage spells: Healing Word and Cure Wounds (SRD 5.2, 2024
//! rules). Also True Strike (2024 cantrip): see [`TrueStrikePlugin`] below.
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

use crate::rules::combat::{Attack, DamageRider};
use crate::rules::creature::{
    DamageKind, DamageRoll, Effect, HealRoll, Move, SpellCastingProfile, Strike,
};

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

/// True Strike (2024 cantrip): an Action. Make a weapon attack, but use the
/// caster's spellcasting ability modifier instead of Strength or Dexterity
/// for both the attack roll and the weapon's damage roll. On a hit, the
/// target also takes extra Radiant damage that scales with the caster's
/// level - 2d6 at the base tier (character level 1-4), more at higher tiers
/// per the SRD's cantrip-scaling convention.
///
/// Every number that varies by build or by level is a plugin parameter
/// rather than baked in, the same reason [`super::rogue::SneakAttackPlugin`]
/// takes `dice_count` instead of a hardcoded "4d6": which weapon is wielded
/// changes `weapon_dice_count`/`weapon_dice_sides`/`weapon_damage_kind` and
/// `finesse_or_ranged`, and the caster's cantrip-scaling tier changes
/// `radiant_dice_count` (this task only wires up the base 2d6 tier as a
/// caller-supplied value, not a hardcoded one - a later config simply passes
/// a bigger number for a higher tier).
///
/// The attack roll and the weapon's flat damage bonus are read from the
/// caster's [`SpellCastingProfile`] rather than any Strength or Dexterity
/// score: [`SpellCastingProfile::attack_bonus`] (ability modifier +
/// proficiency + item bonus, the standard spell attack formula) replaces the
/// weapon's usual to-hit, and `ability_modifier` alone (no proficiency, the
/// same as a normal weapon's Strength or Dexterity modifier) replaces the
/// weapon's usual flat damage bonus. Neither is ever a literal number picked
/// by this plugin.
///
/// This still reads as a genuine weapon attack for anything that gates on
/// that - Sneak Attack, most obviously. [`TrueStrikePlugin::attack`] flags
/// the resulting [`Attack`] with both [`Attack::is_spell_attack`] (it is a
/// spell) and [`Attack::finesse_or_ranged`] (whenever the wielded weapon
/// itself has that property). The two are independent and additive: a rogue
/// using True Strike with a rapier still qualifies for Sneak Attack through
/// the ordinary weapon gate - [`crate::rules::creature::Rider::extra_damage_for`] -
/// with or without any build that also extends Sneak Attack to spell
/// attacks (see
/// [`crate::rules::creature::Rider::extra_damage_for_with_spell_attack_extension`]).
#[derive(Debug, Clone, Copy)]
pub struct TrueStrikePlugin {
    /// The wielded weapon's own damage dice - unrelated to the caster's
    /// level, since it is the weapon that determines this, not the spell.
    pub weapon_dice_count: u32,
    pub weapon_dice_sides: u32,
    pub weapon_damage_kind: DamageKind,
    /// Whether the wielded weapon has the finesse or ranged property - see
    /// [`Attack::finesse_or_ranged`]. `false` for anything else (a
    /// non-finesse melee weapon).
    pub finesse_or_ranged: bool,
    /// The cantrip-scaling tier's bonus Radiant dice count - 2 at the base
    /// tier, more at higher character levels.
    pub radiant_dice_count: u32,
    pub radiant_dice_sides: u32,
    pub radiant_damage_kind: DamageKind,
}

impl TrueStrikePlugin {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        weapon_dice_count: u32,
        weapon_dice_sides: u32,
        weapon_damage_kind: DamageKind,
        finesse_or_ranged: bool,
        radiant_dice_count: u32,
        radiant_dice_sides: u32,
        radiant_damage_kind: DamageKind,
    ) -> Self {
        Self {
            weapon_dice_count,
            weapon_dice_sides,
            weapon_damage_kind,
            finesse_or_ranged,
            radiant_dice_count,
            radiant_dice_sides,
            radiant_damage_kind,
        }
    }

    /// A standard True Strike: the base 2d6 Radiant tier, Radiant damage
    /// type, over whatever weapon `weapon_dice_count`/`weapon_dice_sides`/
    /// `weapon_damage_kind`/`finesse_or_ranged` describe.
    pub fn base_tier(
        weapon_dice_count: u32,
        weapon_dice_sides: u32,
        weapon_damage_kind: DamageKind,
        finesse_or_ranged: bool,
    ) -> Self {
        Self::new(
            weapon_dice_count,
            weapon_dice_sides,
            weapon_damage_kind,
            finesse_or_ranged,
            2,
            6,
            DamageKind::Radiant,
        )
    }

    /// The [`Attack`] True Strike resolves as, against `profile` - the
    /// caster's own [`SpellCastingProfile`], never a hardcoded number. The
    /// weapon's dice carry the caster's ability modifier as their flat bonus
    /// in place of Strength or Dexterity, and the scaling tier's Radiant
    /// dice ride alongside as a [`DamageRider`] - doubled on a crit exactly
    /// like the weapon's own dice, which is correct: a critical hit doubles
    /// every damage die an attack rolls, not only the weapon's (see
    /// `rules::combat`'s module docs).
    ///
    /// Roll mode and ally-adjacency are situational, not a property of the
    /// spell, so they are left at [`Attack`]'s defaults here - a caller
    /// chains [`Attack::with_mode`] / [`Attack::with_ally_adjacent`] for the
    /// attack actually being resolved, the same way any other [`Attack`] is
    /// built up.
    pub fn attack(&self, profile: SpellCastingProfile) -> Attack {
        Attack::new(
            profile.attack_bonus(),
            self.weapon_dice_count,
            self.weapon_dice_sides,
            profile.ability_modifier,
        )
        .with_is_spell_attack(true)
        .with_finesse_or_ranged(self.finesse_or_ranged)
        .with_damage_rider(DamageRider::new(
            self.radiant_dice_count,
            self.radiant_dice_sides,
        ))
    }
}

impl FeaturePlugin for TrueStrikePlugin {
    fn id(&self) -> &'static str {
        "true_strike"
    }

    fn name(&self) -> &str {
        "True Strike"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let profile = builder.creature.spellcasting.ok_or_else(|| {
            FeatureError::PrerequisiteNotMet(
                "True Strike needs this creature's spellcasting profile declared first".to_string(),
            )
        })?;
        let strike = Strike::new(
            profile.attack_bonus(),
            vec![
                DamageRoll::new(
                    self.weapon_dice_count,
                    self.weapon_dice_sides,
                    profile.ability_modifier,
                    self.weapon_damage_kind,
                ),
                DamageRoll::new(
                    self.radiant_dice_count,
                    self.radiant_dice_sides,
                    0,
                    self.radiant_damage_kind,
                ),
            ],
        );
        builder.add_action(Move::new(
            "True Strike",
            Effect::Strikes { strike, count: 1 },
        ));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::plugin::rogue::SneakAttackPlugin;
    use crate::prob::rng::Rng;
    use crate::rules::combat::{damage_pmf, sample_damage, Defense, RollMode};
    use crate::rules::creature::{apply_healing, is_down, Ability, Creature, Rider};

    fn wisdom_caster(ability_modifier: i32) -> CreatureBuilder {
        let mut builder = CreatureBuilder::new("Cleric", 14, 30);
        builder.set_spellcasting(SpellCastingProfile::new(Ability::Wis, ability_modifier, 3));
        builder.set_spell_slot_max(1, 2);
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

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    /// A rapier-wielding caster: finesse weapon, 1d8, Wisdom-based casting.
    fn rapier_true_strike() -> TrueStrikePlugin {
        TrueStrikePlugin::base_tier(1, 8, DamageKind::Piercing, true)
    }

    fn wis_profile(ability_modifier: i32, proficiency_bonus: i32) -> SpellCastingProfile {
        SpellCastingProfile::new(Ability::Wis, ability_modifier, proficiency_bonus)
    }

    /// The whole point of taking a `SpellCastingProfile` rather than a
    /// number: two different profiles must produce two different attacks,
    /// each matching that profile's own formula, not one fixed constant this
    /// plugin picked.
    #[test]
    fn attack_roll_and_damage_bonus_come_from_the_spellcasting_profile_not_hardcoded() {
        let plugin = rapier_true_strike();

        let modest = wis_profile(2, 3);
        let attack = plugin.attack(modest);
        assert_eq!(attack.to_hit, modest.attack_bonus());
        assert_eq!(attack.damage_bonus, modest.ability_modifier);

        let potent = wis_profile(5, 6).with_item_bonus(1);
        let attack = plugin.attack(potent);
        assert_eq!(attack.to_hit, potent.attack_bonus());
        assert_eq!(attack.damage_bonus, potent.ability_modifier);

        assert_ne!(
            modest.attack_bonus(),
            potent.attack_bonus(),
            "the two profiles must actually differ for this test to mean anything"
        );
    }

    /// [`Attack::is_spell_attack`] is always set; [`Attack::finesse_or_ranged`]
    /// tracks the wielded weapon, independently.
    #[test]
    fn the_attack_is_flagged_as_a_spell_attack_and_carries_the_weapons_own_finesse_or_ranged_flag()
    {
        let profile = wis_profile(3, 2);

        let finesse = TrueStrikePlugin::base_tier(1, 8, DamageKind::Piercing, true).attack(profile);
        assert!(finesse.is_spell_attack);
        assert!(finesse.finesse_or_ranged);

        let non_finesse =
            TrueStrikePlugin::base_tier(1, 8, DamageKind::Bludgeoning, false).attack(profile);
        assert!(non_finesse.is_spell_attack);
        assert!(!non_finesse.finesse_or_ranged);
    }

    /// On a hit, the weapon's own dice and the configured Radiant dice both
    /// land, and a crit doubles both pools identically - exactly the shape
    /// [`crate::rules::combat::rider_pmf`] already guarantees for any
    /// [`DamageRider`], not reimplemented here.
    #[test]
    fn on_hit_damage_is_the_weapons_own_dice_plus_the_configured_radiant_dice() {
        let profile = wis_profile(4, 3); // ability_modifier 4
        let plugin = TrueStrikePlugin::base_tier(2, 6, DamageKind::Slashing, false);
        let attack = plugin.attack(profile);
        let defense = Defense::new(1, 60); // AC 1: every non-fumble roll hits

        let pmf = damage_pmf(&attack, &defense);
        assert!(close(pmf.total(), 1.0));

        // A hit's maximum: weapon 2d6 (12) + damage_bonus 4 + radiant 2d6 (12) = 28.
        // A crit doubles every die but not the flat bonus: weapon 4d6 (24) +
        // damage_bonus 4 + radiant 4d6 (24) = 52 - the overall maximum, since
        // the crit branch dominates the hit branch.
        let hit_max = 2 * 6 + 4 + 2 * 6;
        let crit_max = 2 * 2 * 6 + 4 + 2 * 2 * 6;
        assert!(crit_max > hit_max);
        assert_eq!(pmf.max(), crit_max);

        // Every hit (crit or not) includes the flat damage_bonus plus at
        // least the two pools' minimums (zero), so the mean strictly exceeds
        // what the weapon alone would deal - the radiant dice are additive,
        // not a replacement.
        let weapon_only = Attack::new(attack.to_hit, 2, 6, 4);
        assert!(damage_pmf(&attack, &defense).mean() > damage_pmf(&weapon_only, &defense).mean());
    }

    /// The point of the whole exercise: True Strike is still a *weapon*
    /// attack, so a rogue using it with a finesse weapon and Advantage still
    /// triggers Sneak-Attack-style extra damage through the ordinary weapon
    /// gate - reading the dice count straight off the creature's own
    /// [`SneakAttackPlugin`] configuration, never a hardcoded "5d6".
    #[test]
    fn sneak_attack_style_extra_damage_still_triggers_under_advantage_with_a_finesse_weapon() {
        // An arbitrary, deliberately non-default dice count - the whole
        // point is that the test never repeats this number as a literal
        // anywhere else; it is read back off the plugin/rider instead.
        let sneak_attack = SneakAttackPlugin::new(3);
        let builder = CreatureBuilder::new("True Strike Rogue", 15, 40)
            .apply_feature(&sneak_attack)
            .expect("sneak attack applies");
        let rider = builder.creature.riders[0].clone();
        let Rider::ConditionalExtraDamage {
            dice_count,
            dice_sides,
            ..
        } = rider
        else {
            panic!("expected a ConditionalExtraDamage rider");
        };
        assert_eq!(dice_count, sneak_attack.dice_count);
        assert_eq!(dice_sides, sneak_attack.dice_sides);

        let profile = wis_profile(3, 2);
        let attack = rapier_true_strike()
            .attack(profile)
            .with_mode(RollMode::Advantage);
        assert!(
            attack.finesse_or_ranged,
            "True Strike over a rapier must still read as a finesse weapon attack"
        );

        let extra = rider
            .extra_damage_for(&attack, false)
            .expect("advantage plus a finesse weapon should qualify for Sneak Attack");
        assert_eq!(extra, DamageRider::new(dice_count, dice_sides));
    }

    /// Without a qualifying weapon (no finesse or ranged property), Sneak
    /// Attack does not trigger off True Strike either - the spell-attack
    /// flag alone is not enough through the plain gate, matching AT-02's
    /// existing "no extension present" behaviour.
    #[test]
    fn a_non_finesse_weapon_does_not_qualify_for_sneak_attack_even_at_advantage() {
        let rider = Rider::ConditionalExtraDamage {
            dice_count: 3,
            dice_sides: 6,
            once_per_turn: true,
        };
        let profile = wis_profile(3, 2);
        let attack = TrueStrikePlugin::base_tier(1, 10, DamageKind::Bludgeoning, false)
            .attack(profile)
            .with_mode(RollMode::Advantage);
        assert_eq!(rider.extra_damage_for(&attack, false), None);
    }

    /// Applying the plugin adds an Action built from the caster's profile,
    /// not from any Strength or Dexterity score, and requires spellcasting
    /// to already be declared - the same "well-formed feature, unmet
    /// prerequisite" shape `prestige_spellcasting` uses.
    #[test]
    fn applying_the_plugin_adds_an_action_using_the_declared_spellcasting_profile() {
        let mut builder = CreatureBuilder::new("Caster", 15, 30);
        let profile = wis_profile(4, 3);
        builder.set_spellcasting(profile);

        let plugin = TrueStrikePlugin::base_tier(1, 8, DamageKind::Piercing, true);
        plugin.apply(&mut builder).expect("prerequisite is met");

        let action = builder
            .creature
            .actions
            .last()
            .expect("an action was added");
        assert_eq!(action.name, "True Strike");
        let Effect::Strikes { strike, count } = &action.effect else {
            panic!("expected a Strikes effect");
        };
        assert_eq!(*count, 1);
        assert_eq!(strike.to_hit, profile.attack_bonus());
        assert_eq!(
            strike.damage,
            vec![
                DamageRoll::new(1, 8, profile.ability_modifier, DamageKind::Piercing),
                DamageRoll::new(2, 6, 0, DamageKind::Radiant),
            ]
        );
    }

    #[test]
    fn applying_the_plugin_without_spellcasting_declared_is_a_prerequisite_failure() {
        let mut builder = CreatureBuilder::new("Not Yet A Caster", 15, 30);
        let plugin = TrueStrikePlugin::base_tier(1, 8, DamageKind::Piercing, true);
        assert!(matches!(
            plugin.apply(&mut builder),
            Err(FeatureError::PrerequisiteNotMet(_))
        ));
        assert!(builder.creature.actions.is_empty());
    }

    /// The same agreement check every other rule in this codebase is held
    /// to: the sampled path must match the exact `damage_pmf` it is supposed
    /// to be distributed as, across both the base attack and the
    /// Sneak-Attack-composed one.
    #[test]
    fn sampled_true_strike_agrees_with_the_exact_path() {
        let profile = wis_profile(4, 3);
        let defense = Defense::new(14, 60);
        let base = rapier_true_strike()
            .attack(profile)
            .with_mode(RollMode::Advantage);

        let sneak_rider = Rider::ConditionalExtraDamage {
            dice_count: 3,
            dice_sides: 6,
            once_per_turn: true,
        };
        let extra = sneak_rider
            .extra_damage_for(&base, false)
            .expect("advantage plus a finesse weapon qualifies");
        let composed = base.clone().with_damage_rider(extra);

        for (seed, (name, attack)) in [
            ("true strike alone", base),
            ("true strike + sneak attack", composed),
        ]
        .into_iter()
        .enumerate()
        {
            let exact = damage_pmf(&attack, &defense);
            let (lo, hi) = (exact.min(), exact.max());
            let mut rng = Rng::new(seed as u64 + 4_200);
            let n = 100_000;
            let mut counts = vec![0usize; (hi - lo + 1) as usize];
            for _ in 0..n {
                let d = sample_damage(&mut rng, &attack, &defense);
                assert!(
                    d >= lo && d <= hi,
                    "{name}: sampled {d} outside {lo}..={hi}"
                );
                counts[(d - lo) as usize] += 1;
            }
            for (i, &c) in counts.iter().enumerate() {
                let value = lo + i as i32;
                let want = exact.prob(value);
                let got = c as f64 / n as f64;
                let tol = 5.0 * (want * (1.0 - want) / n as f64).sqrt() + 1e-4;
                assert!(
                    (got - want).abs() < tol,
                    "{name}: P(damage = {value}) sampled {got:.5}, exact {want:.5}, tol {tol:.5}"
                );
            }
        }
    }
}
