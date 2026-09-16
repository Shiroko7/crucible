//! A generic "prestige / secondary spellcasting grant" plugin.
//!
//! Some builds graft a secondary spellcasting progression onto a base class
//! that is not normally a caster, gated on entry prerequisites named on the
//! feature itself: minimums on a couple of ability scores, plus (for a build
//! layered over a Rogue-shaped base) enough existing Sneak-Attack-style extra
//! damage dice to prove the base class has progressed far enough to qualify.
//!
//! Every number here - which two abilities gate it and by how much, the
//! sneak-attack-dice minimum, the granted slot table, and the granted
//! casting ability/bonus - belongs to whichever specific build's TOML
//! supplies it, the same way [`super::rogue::SneakAttackPlugin`]'s
//! `dice_count` belongs to the rogue wearing it rather than to the plugin.
//! None of it is hardcoded here.

use crate::rules::creature::{Ability, Rider, SpellCastingProfile, SPELL_LEVELS};

use super::traits::{CreatureBuilder, FeatureError, FeaturePlugin, FeatureResult};

/// One entry-prerequisite check against a raw ability score already recorded
/// on the creature - see [`crate::rules::creature::Creature::ability_score`]
/// and [`CreatureBuilder::set_ability_score`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbilityRequirement {
    pub ability: Ability,
    pub minimum: i32,
}

impl AbilityRequirement {
    pub fn new(ability: Ability, minimum: i32) -> Self {
        Self { ability, minimum }
    }
}

/// Grafts a secondary spellcasting progression onto a creature, refusing to
/// apply at all unless its entry prerequisites hold.
///
/// On success this sets the creature's
/// [`crate::rules::creature::SpellSlots`] pool to exactly `slots` (index 0 is
/// 1st level spell slots, ... index 8 is 9th) and its [`SpellCastingProfile`]
/// to `ability`/`attack_bonus`. There is no separate `dc` parameter: the
/// profile already derives the save DC as `8 + attack_bonus`, the standard
/// 5e formula ARCH-05 encodes once, so a second field here would just be a
/// second place for the same number to go stale.
///
/// Any `item_bonus` already sitting on the creature's spellcasting profile -
/// from an equipment plugin that ran earlier in the feature list - is
/// preserved rather than clobbered, so this plugin only ever supplies the
/// class-granted part of the bonus and composes with, rather than
/// duplicates, whatever equipment contributes.
#[derive(Debug, Clone)]
pub struct PrestigeSpellcastingPlugin {
    pub name: String,
    pub ability_requirements: [AbilityRequirement; 2],
    pub minimum_sneak_attack_dice: u32,
    pub slots: [u32; SPELL_LEVELS as usize],
    pub ability: Ability,
    pub attack_bonus: i32,
}

impl PrestigeSpellcastingPlugin {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: impl Into<String>,
        ability_requirements: [AbilityRequirement; 2],
        minimum_sneak_attack_dice: u32,
        slots: [u32; SPELL_LEVELS as usize],
        ability: Ability,
        attack_bonus: i32,
    ) -> Self {
        Self {
            name: name.into(),
            ability_requirements,
            minimum_sneak_attack_dice,
            slots,
            ability,
            attack_bonus,
        }
    }

    /// The largest `dice_count` across any
    /// [`Rider::ConditionalExtraDamage`] already on the creature - reusing
    /// Sneak Attack's own field as the generic "how far into the base class
    /// has this build progressed" check point, rather than inventing a
    /// parallel "prerequisite feature" system.
    fn existing_sneak_attack_dice(builder: &CreatureBuilder) -> u32 {
        builder
            .creature
            .riders
            .iter()
            .filter_map(|r| match r {
                Rider::ConditionalExtraDamage { dice_count, .. } => Some(*dice_count),
                _ => None,
            })
            .max()
            .unwrap_or(0)
    }
}

impl FeaturePlugin for PrestigeSpellcastingPlugin {
    fn id(&self) -> &'static str {
        "prestige_spellcasting"
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        for req in &self.ability_requirements {
            let score = builder.creature.ability_score(req.ability);
            if score < req.minimum {
                return Err(FeatureError::PrerequisiteNotMet(format!(
                    "{} requires {} {}, but {} has {} {}",
                    self.name,
                    req.minimum,
                    req.ability.name(),
                    builder.creature.name,
                    score,
                    req.ability.name(),
                )));
            }
        }

        let sneak_attack_dice = Self::existing_sneak_attack_dice(builder);
        if sneak_attack_dice < self.minimum_sneak_attack_dice {
            return Err(FeatureError::PrerequisiteNotMet(format!(
                "{} requires at least {} sneak attack dice, but {} has {}",
                self.name, self.minimum_sneak_attack_dice, builder.creature.name, sneak_attack_dice,
            )));
        }

        for level in 1..=SPELL_LEVELS {
            builder.set_spell_slot_max(level, self.slots[(level - 1) as usize]);
        }

        let item_bonus = builder
            .creature
            .spellcasting
            .map(|p| p.item_bonus)
            .unwrap_or(0);
        builder.set_spellcasting(
            SpellCastingProfile::new(self.ability, self.attack_bonus, 0)
                .with_item_bonus(item_bonus),
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::plugin::traits::CreatureBuilder;

    fn requirements() -> [AbilityRequirement; 2] {
        [
            AbilityRequirement::new(Ability::Dex, 13),
            AbilityRequirement::new(Ability::Int, 13),
        ]
    }

    fn slots_1_and_2(first: u32, second: u32) -> [u32; SPELL_LEVELS as usize] {
        let mut slots = [0u32; SPELL_LEVELS as usize];
        slots[0] = first;
        slots[1] = second;
        slots
    }

    fn qualifying_builder(dex: i32, int: i32, sneak_attack_dice: u32) -> CreatureBuilder {
        let mut builder = CreatureBuilder::new("Test Subject", 15, 40);
        builder.set_ability_score(Ability::Dex, dex);
        builder.set_ability_score(Ability::Int, int);
        if sneak_attack_dice > 0 {
            builder.add_rider(Rider::ConditionalExtraDamage {
                dice_count: sneak_attack_dice,
                dice_sides: 6,
                once_per_turn: true,
            });
        }
        builder
    }

    #[test]
    fn prerequisites_met_grants_exactly_the_configured_slots_and_profile() {
        let mut builder = qualifying_builder(13, 13, 2);
        let plugin = PrestigeSpellcastingPlugin::new(
            "Test Prestige Caster",
            requirements(),
            2,
            slots_1_and_2(4, 3),
            Ability::Wis,
            11,
        );

        plugin.apply(&mut builder).expect("prerequisites are met");

        assert_eq!(builder.creature.spell_slots.max(1), 4);
        assert_eq!(builder.creature.spell_slots.available(1), 4);
        assert_eq!(builder.creature.spell_slots.max(2), 3);
        // A level the config never mentions starts at zero, same as
        // `SpellSlots::set_max` for any other caster.
        assert_eq!(builder.creature.spell_slots.max(3), 0);

        let profile = builder
            .creature
            .spellcasting
            .expect("spellcasting profile granted");
        assert_eq!(profile.ability, Ability::Wis);
        assert_eq!(profile.attack_bonus(), 11);
        // DC is derived, not a separate parameter: 8 + attack bonus.
        assert_eq!(profile.save_dc(), 19);
    }

    /// A second, distinctly different parameter set, to prove none of the
    /// above was a hardcoded constant rather than a genuine parameter.
    #[test]
    fn a_differently_configured_instance_grants_its_own_different_numbers() {
        let mut builder = qualifying_builder(15, 15, 5);
        let plugin = PrestigeSpellcastingPlugin::new(
            "A Different Prestige Caster",
            [
                AbilityRequirement::new(Ability::Str, 15),
                AbilityRequirement::new(Ability::Wis, 15),
            ],
            5,
            slots_1_and_2(2, 1),
            Ability::Cha,
            6,
        );
        // This instance's requirements are Str/Wis, so give it those too.
        builder.set_ability_score(Ability::Str, 15);
        builder.set_ability_score(Ability::Wis, 15);

        plugin.apply(&mut builder).expect("prerequisites are met");

        assert_eq!(builder.creature.spell_slots.max(1), 2);
        assert_eq!(builder.creature.spell_slots.max(2), 1);

        let profile = builder
            .creature
            .spellcasting
            .expect("spellcasting profile granted");
        assert_eq!(profile.ability, Ability::Cha);
        assert_eq!(profile.attack_bonus(), 6);
        assert_eq!(profile.save_dc(), 14);
    }

    #[test]
    fn preserves_an_existing_item_bonus_instead_of_clobbering_it() {
        let mut builder = qualifying_builder(13, 13, 2);
        builder.set_spellcasting(SpellCastingProfile::new(Ability::Wis, 0, 0).with_item_bonus(3));
        let plugin = PrestigeSpellcastingPlugin::new(
            "Test Prestige Caster",
            requirements(),
            2,
            slots_1_and_2(4, 3),
            Ability::Wis,
            11,
        );

        plugin.apply(&mut builder).expect("prerequisites are met");

        let profile = builder.creature.spellcasting.expect("profile set");
        assert_eq!(profile.item_bonus, 3, "the item bonus survives the grant");
        assert_eq!(
            profile.attack_bonus(),
            14,
            "the granted bonus and the item bonus compose additively"
        );
    }

    #[test]
    fn rejects_when_the_first_ability_requirement_is_unmet() {
        let mut builder = qualifying_builder(12, 13, 2); // Dex one short
        let plugin = PrestigeSpellcastingPlugin::new(
            "Test Prestige Caster",
            requirements(),
            2,
            slots_1_and_2(4, 3),
            Ability::Wis,
            11,
        );

        let err = plugin.apply(&mut builder).unwrap_err();
        assert!(matches!(err, FeatureError::PrerequisiteNotMet(_)));
        assert!(
            builder.creature.spellcasting.is_none(),
            "a failed prerequisite must not partially apply"
        );
        assert_eq!(builder.creature.spell_slots.max(1), 0);
    }

    #[test]
    fn rejects_when_the_second_ability_requirement_is_unmet() {
        let mut builder = qualifying_builder(13, 12, 2); // Int one short
        let plugin = PrestigeSpellcastingPlugin::new(
            "Test Prestige Caster",
            requirements(),
            2,
            slots_1_and_2(4, 3),
            Ability::Wis,
            11,
        );

        let err = plugin.apply(&mut builder).unwrap_err();
        assert!(matches!(err, FeatureError::PrerequisiteNotMet(_)));
        assert!(builder.creature.spellcasting.is_none());
    }

    #[test]
    fn rejects_when_sneak_attack_dice_are_short() {
        let mut builder = qualifying_builder(13, 13, 1); // needs 2, has 1
        let plugin = PrestigeSpellcastingPlugin::new(
            "Test Prestige Caster",
            requirements(),
            2,
            slots_1_and_2(4, 3),
            Ability::Wis,
            11,
        );

        let err = plugin.apply(&mut builder).unwrap_err();
        assert!(matches!(err, FeatureError::PrerequisiteNotMet(_)));
        assert!(builder.creature.spellcasting.is_none());
    }

    #[test]
    fn rejects_when_there_is_no_sneak_attack_rider_at_all() {
        let mut builder = qualifying_builder(13, 13, 0);
        let plugin = PrestigeSpellcastingPlugin::new(
            "Test Prestige Caster",
            requirements(),
            2,
            slots_1_and_2(4, 3),
            Ability::Wis,
            11,
        );

        let err = plugin.apply(&mut builder).unwrap_err();
        assert!(matches!(err, FeatureError::PrerequisiteNotMet(_)));
    }
}
