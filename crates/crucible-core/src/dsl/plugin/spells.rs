//! Spell feature plugins: official SRD 5.2 spells as `FeaturePlugin`s.
//!
//! Each spell here is a `Move` built the same way any other feature builds
//! one - see `standard.rs` - rather than new engine branches. The one thing
//! genuinely new to the engine is [`crate::rules::creature::Effect::AutoHit`],
//! for Magic Missile's "no attack roll, no save" damage; Blindness/Deafness
//! and Command both fit entirely inside the existing `Effect::Save` /
//! `Condition` / `Duration` machinery ARCH-01 and ARCH-05 already built.
//!
//! ## What pays for a cast
//!
//! Two payment mechanisms coexist here, both legitimate:
//!
//! - Healing Word and Cure Wounds spend directly from the caster's own
//!   [`crate::rules::creature::SpellSlots`] via [`crate::rules::creature::Move::with_spell_slot`]
//!   and [`crate::rules::creature::Move::pay_spell_cost`].
//! - Blindness/Deafness, Command, and Magic Missile take their cost as data
//!   instead: [`SpellCost`] names a resource pool and an amount, resolved
//!   against whatever the caster's config already declared (see
//!   `dsl::config::apply_resources`). `SpellSlots` (ARCH-05) exists as a data
//!   type on `Creature`, but nothing in `sim::duel`'s action economy spends
//!   from it yet - `Fighter::pay`/`can_pay` only ever look at the generic
//!   `resources` pool via `Cost`. Wiring real per-level slot spending into the
//!   duel's turn loop would be a change to that shared economy, not a
//!   spell-plugin concern, so it is left alone here and noted as a found gap
//!   rather than worked around with something bespoke. A wizard's real spell
//!   slot and a wand's limited charges are the same mechanism under this
//!   scheme - "spend `amount` from a named pool" - so which one a cast draws
//!   from is a constructor parameter, never hardcoded to a resource name
//!   inside a plugin.
//!
//! Also True Strike (2024 cantrip): see [`TrueStrikePlugin`] below. Also
//! Spiritual Weapon: see [`SpiritualWeaponPlugin`] below.
//!
//! Reviving a creature at 0 HP is not spell-specific text - it is 5e's
//! general "a creature that regains any hit points while it has 0 becomes
//! conscious" rule - so it is implemented once, in
//! [`crate::rules::creature::apply_healing`], and inherited by both healing
//! spells (and anything else that ever heals) rather than re-implemented per
//! spell.
//!
//! Not modelled, on purpose:
//! - **Range.** `DESIGN.md` already rules positioning out of scope entirely
//!   ("Positioning is the gap that matters") - there is no notion of distance
//!   for a melee weapon either, so a spell's range in feet is flavour text
//!   here, not a mechanic.
//! - **Ally targeting.** The duel engine (`sim::duel`) only ever targets the
//!   opposing side right now - no move of any kind can target a friendly
//!   creature yet. These plugins produce fully-formed, fully-testable
//!   `Move`s (the right action economy, slot cost, and heal formula), but
//!   wiring "cast this on a bloodied ally" into the automated turn engine is
//!   a separate, considerably larger feature (self/ally targeting for every
//!   effect, plus a policy that decides when to heal) and is left for a
//!   follow-up rather than bolted on here.
//! - **Upcasting.** Every spell here is implemented at its base cast only;
//!   scaling with a higher slot is skipped.
//!
//! Concentration spell plugins whose mechanism is an ongoing attack-roll and
//! saving-throw modifier rather than a condition - Bless and Bane. Neither
//! registers anything new: [`AttackModifier::BonusDice`] /
//! [`AttackModifier::PenaltyDice`] and their [`SaveModifier`] siblings
//! already exist for exactly this (see `rules::combat`'s module docs), and
//! `sim::duel` already knows how to apply them for the duration of a
//! concentration spell and strip them when concentration ends - see
//! [`Effect::Buff`] and [`Effect::SaveOrModifier`]. This module only builds
//! the two [`Move`]s that reach for that mechanism.
//!
//! Hold Person (SRD 5.2, 2024 rules): a 2nd-level spell that paralyzes a
//! humanoid who fails a Wisdom saving throw, for as long as the caster keeps
//! concentrating (up to 1 minute), repeating the save at the end of the
//! target's own turns.
//!
//! Every clause of that sentence is machinery this engine already has,
//! rather than anything new:
//! - the save and its DC are `Effect::Save`'s ordinary business, with the DC
//!   read off the caster's own [`crate::rules::creature::SpellCastingProfile`]
//!   rather than hardcoded;
//! - "a humanoid" is [`crate::rules::creature::SaveEffect::requires_type`],
//!   checked before a save is even rolled - a non-humanoid is not caught at
//!   all, not caught-and-then-unaffected;
//! - "repeating the save at the end of its turns" is
//!   [`crate::rules::creature::Duration::SaveEndTurn`], which already drives
//!   `sim::duel`'s end-of-turn save loop for any condition that carries it;
//! - "for as long as the caster concentrates" is
//!   [`crate::rules::creature::Move::concentration`] - `sim::duel`'s
//!   concentration tracker clears the condition from every target it is
//!   maintaining the instant that ends, save or no save;
//! - the auto-crit a Paralyzed target grants an attacker is
//!   [`crate::rules::creature::Condition::auto_crits`], already read by every
//!   strike this engine resolves.
//!
//! Hold Person is not modelled beyond its base cast either, for the same
//! reasons as the healing spells above: range/positioning is out of scope,
//! and upcasting (catching more than one humanoid) is skipped.

use crate::rules::combat::{Attack, AttackModifier, DamageRider, SaveModifier};
use crate::rules::creature::{
    Ability, Condition, Cost, DamageKind, DamageRoll, Duration, Effect, HealRoll, Move, SaveEffect,
    SpellCastingProfile, Strike, Uses,
};

use super::traits::{CreatureBuilder, FeatureError, FeaturePlugin, FeatureResult};

/// Both healing spells are cast here at their base, 1st-level, rate. See the
/// module doc: upcasting is out of scope.
const BASE_SLOT_LEVEL: u32 = 1;

/// Hold Person is always cast from a 2nd-level slot here; see the module doc.
const SLOT_LEVEL: u32 = 2;

/// The base, non-upcast version catches exactly one creature; see the module
/// doc's note on upcasting.
const MAX_TARGETS: u32 = 1;

/// Hold Person's own targeting restriction: the SRD names the creature type
/// by this exact word, matched case-insensitively by
/// [`crate::rules::creature::Creature::is_creature_type`].
const TARGET_TYPE: &str = "Humanoid";

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

/// Hold Person (SRD 5.2): Action, 2nd level, concentration up to 1 minute.
/// One humanoid within range makes a Wisdom save or becomes Paralyzed,
/// repeating the save at the end of each of its own turns.
#[derive(Debug, Clone, Copy, Default)]
pub struct HoldPersonPlugin;

impl HoldPersonPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl FeaturePlugin for HoldPersonPlugin {
    fn id(&self) -> &'static str {
        "hold_person"
    }

    fn name(&self) -> &str {
        "Hold Person"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let dc = builder.creature.spell_save_dc().ok_or_else(|| {
            FeatureError::InvalidConfiguration(
                "Hold Person needs a [*.spellcasting] profile to compute its save DC".to_string(),
            )
        })?;

        let hold_person = Move::new(
            "Hold Person",
            Effect::Save(SaveEffect {
                ability: Ability::Wis,
                dc,
                damage: Vec::new(),
                half_on_success: false,
                on_failure: vec![(
                    Condition::Paralyzed,
                    Duration::SaveEndTurn {
                        ability: Ability::Wis,
                        dc,
                    },
                )],
                max_targets: Some(MAX_TARGETS),
                requires_type: Some(TARGET_TYPE.to_string()),
            }),
        )
        .with_spell_slot(SLOT_LEVEL)
        .with_concentration();

        builder.add_action(hold_person);
        Ok(())
    }
}

/// How a spell's use is paid for: a named resource pool, and how much of it
/// one cast spends.
///
/// Deliberately not tied to [`crate::rules::creature::SpellSlots`] - see this
/// module's own doc for why - so `resource_name` is free to point at
/// anything the builder has declared: a `spell_slots_1` pool standing in for
/// a real slot, a `wand_charges` pool for an item, or nothing at all if the
/// caster building this creature leaves the cost off entirely (an at-will
/// version, for a homebrew ring or the like).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpellCost {
    pub resource_name: String,
    pub amount: u32,
}

impl SpellCost {
    pub fn new(resource_name: impl Into<String>, amount: u32) -> Self {
        Self {
            resource_name: resource_name.into(),
            amount,
        }
    }

    /// Resolve against `builder`'s already-declared resources, turning the
    /// name into the indexed [`Cost`] a `Move` actually carries.
    fn resolve(&self, builder: &CreatureBuilder) -> FeatureResult<Cost> {
        Ok(Cost {
            resource: builder.resource_index(&self.resource_name)?,
            amount: self.amount,
        })
    }
}

/// Resolve an optional [`SpellCost`] into an optional indexed [`Cost`], the
/// shape every plugin below needs before it can call `Move::with_cost`.
fn resolve_cost(
    builder: &CreatureBuilder,
    cost: &Option<SpellCost>,
) -> FeatureResult<Option<Cost>> {
    cost.as_ref().map(|c| c.resolve(builder)).transpose()
}

/// A caster's save DC, or a configuration error naming which spell needed
/// one. Every spell below targets a save, so every one of them calls this.
fn save_dc(builder: &CreatureBuilder, spell_name: &str) -> FeatureResult<i32> {
    builder.creature.spell_save_dc().ok_or_else(|| {
        FeatureError::InvalidConfiguration(format!(
            "{spell_name} needs a `[*.spellcasting]` profile to compute its save DC"
        ))
    })
}

// --- Blindness/Deafness -------------------------------------------------

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
        let cost = resolve_cost(builder, &self.cost)?;
        let condition = if self.deafen {
            Condition::Deafened
        } else {
            Condition::Blinded
        };

        let mut mv = Move::new(
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
        );
        if let Some(cost) = cost {
            mv = mv.with_cost(cost);
        }
        builder.add_action(mv);
        Ok(())
    }
}

/// Bless (1st level, concentration, up to 1 minute): up to three creatures -
/// the caster included - each add `1d4` to every attack roll and every
/// saving throw they make for the duration, their own concentration save
/// among them. That last part is correct 5e text, not a bug: Bless can help
/// a blessed caster hold their own concentration, and nothing here has to
/// special-case "the target of the buff is also the one rolling the save"
/// for that to happen - `sim::duel` resolves every saving throw a fighter
/// makes through the same modifier list, its own concentration check
/// included.
#[derive(Debug, Clone, Copy, Default)]
pub struct BlessPlugin;

impl FeaturePlugin for BlessPlugin {
    fn id(&self) -> &'static str {
        "bless"
    }

    fn name(&self) -> &str {
        "Bless"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        builder.add_action(
            Move::new(
                "Bless",
                Effect::Buff {
                    attack_modifier: AttackModifier::BonusDice { count: 1, sides: 4 },
                    save_modifier: SaveModifier::BonusDice { count: 1, sides: 4 },
                    max_targets: Some(3),
                },
            )
            .with_concentration()
            .with_spell_level(1),
        );
        Ok(())
    }
}

// --- Command -------------------------------------------------------------

/// A one-word command Command can cast. The SRD lists five (Approach, Drop,
/// Flee, Grovel, Halt); only the two this codebase's roadmap asks for are
/// implemented, but the shape leaves room for the rest without touching
/// anything above it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandWord {
    Grovel,
    Halt,
}

impl CommandWord {
    pub fn parse(word: &str) -> Option<Self> {
        Some(match word.to_ascii_lowercase().as_str() {
            "grovel" => Self::Grovel,
            "halt" => Self::Halt,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Grovel => "Grovel",
            Self::Halt => "Halt",
        }
    }

    /// What a failed save applies. Every word shares `Condition::Compelled`
    /// with `Duration::ApplierTurn` - the engine's handle on "obeys a
    /// directive on its own very next turn" (see that variant's own doc) and
    /// denies the rest of that turn's action economy via
    /// `sim::duel::Fighter::loses_turn`, without touching legendary actions.
    /// `ApplierTurn` rather than `VictimTurn` is deliberate: it is the
    /// mechanism Stunning Strike already proves bounds a condition to
    /// *exactly* the victim's next turn regardless of relative initiative
    /// (see `Duration::ApplierTurn`'s own doc) - `VictimTurn` clears at the
    /// start of the victim's own turn, before that turn's incapacitation
    /// check runs, so it cannot gate the very turn Command means to deny.
    ///
    /// Grovel's real text is "the target falls prone and then ends its
    /// turn", so it stacks `Condition::Prone` on top of the same duration.
    /// There is no engine mechanic for standing back up (no movement model;
    /// see `README.md`, "Positioning is the gap that matters"), so tying
    /// Prone to Compelled's clock is a documented simplification, not a claim
    /// that a real target could not still be prone once its next turn
    /// passes. Halt's text - "the target doesn't move and takes no actions" -
    /// has nothing left over once "takes no actions" is modelled: there is no
    /// movement to additionally restrict, so `Compelled` alone is the whole
    /// of it.
    fn on_failure(self) -> Vec<(Condition, Duration)> {
        let compelled = (Condition::Compelled, Duration::ApplierTurn);
        match self {
            Self::Grovel => vec![(Condition::Prone, Duration::ApplierTurn), compelled],
            Self::Halt => vec![compelled],
        }
    }
}

/// Command (SRD 5.2, 1st-level enchantment, Action): a Wisdom save or the
/// target obeys a one-word command on its next turn.
#[derive(Debug, Clone)]
pub struct CommandPlugin {
    pub word: CommandWord,
    pub cost: Option<SpellCost>,
}

impl CommandPlugin {
    pub fn new(word: CommandWord, cost: Option<SpellCost>) -> Self {
        Self { word, cost }
    }

    fn label(&self) -> String {
        format!("Command (\"{}\")", self.word.as_str())
    }
}

impl FeaturePlugin for CommandPlugin {
    fn id(&self) -> &'static str {
        "command"
    }

    fn name(&self) -> &str {
        self.word.as_str()
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let dc = save_dc(builder, "Command")?;
        let cost = resolve_cost(builder, &self.cost)?;

        let mut mv = Move::new(
            self.label(),
            Effect::Save(SaveEffect {
                ability: Ability::Wis,
                dc,
                damage: Vec::new(),
                half_on_success: false,
                on_failure: self.word.on_failure(),
                max_targets: Some(1),
                requires_type: None,
            }),
        );
        if let Some(cost) = cost {
            mv = mv.with_cost(cost);
        }
        builder.add_action(mv);
        Ok(())
    }
}

// --- Magic Missile ---------------------------------------------------------

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
        let cost = resolve_cost(builder, &self.cost)?;
        let darts = vec![DamageRoll::new(1, 4, 1, DamageKind::Force); 3];

        let mut mv = Move::new("Magic Missile", Effect::AutoHit { damage: darts });
        if let Some(cost) = cost {
            mv = mv.with_cost(cost);
        }
        builder.add_action(mv);
        Ok(())
    }
}

// --- Bane -------------------------------------------------------------

/// Bane (1st level, concentration, up to 1 minute): up to three creatures
/// each make a Charisma save against the caster's own spell save DC - read
/// from [`crate::rules::creature::SpellCastingProfile`] at the moment the
/// spell resolves, never a fixed number baked in here - or subtract `1d4`
/// from every attack roll and every saving throw they make for the
/// duration.
#[derive(Debug, Clone, Copy, Default)]
pub struct BanePlugin;

impl FeaturePlugin for BanePlugin {
    fn id(&self) -> &'static str {
        "bane"
    }

    fn name(&self) -> &str {
        "Bane"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        builder.add_action(
            Move::new(
                "Bane",
                Effect::SaveOrModifier {
                    ability: Ability::Cha,
                    attack_modifier: AttackModifier::PenaltyDice { count: 1, sides: 4 },
                    save_modifier: SaveModifier::PenaltyDice { count: 1, sides: 4 },
                    max_targets: Some(3),
                },
            )
            .with_concentration()
            .with_spell_level(1),
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::plugin::rogue::SneakAttackPlugin;
    use crate::prob::dice::Pmf;
    use crate::prob::rng::Rng;
    use crate::rules::combat::{damage_pmf, sample_damage, Attack, DamageRider, Defense, Reduction, RollMode};
    use crate::rules::creature::{
        apply_healing, is_down, Ability, Creature, CreatureType, DamageKind, DamageRoll, Rider,
        SpellCastingProfile, Strike, Uses,
    };
    use crate::sim::duel::{run, run_teams, Budget, Policy};

    const SAMPLES: usize = 200_000;

    fn tolerance(p: f64, n: usize) -> f64 {
        5.0 * (p * (1.0 - p) / n as f64).sqrt() + 1e-4
    }

    fn wisdom_caster(ability_modifier: i32, proficiency_bonus: i32) -> CreatureBuilder {
        let mut builder = CreatureBuilder::new("Cleric", 14, 30);
        builder.set_spellcasting(SpellCastingProfile::new(
            Ability::Wis,
            ability_modifier,
            proficiency_bonus,
        ));
        builder.set_spell_slot_max(1, 2);
        builder.set_spell_slot_max(2, 1);
        builder
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

    fn caster_builder(profile: SpellCastingProfile) -> CreatureBuilder {
        let mut builder = CreatureBuilder::new("Cleric", 15, 30);
        builder.set_spellcasting(profile);
        builder
    }

    // --- Healing Word / Cure Wounds ---

    #[test]
    fn healing_word_is_a_bonus_action_with_the_right_formula_and_slot() {
        let builder = wisdom_caster(3, 3)
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
        let builder = wisdom_caster(2, 3)
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

    /// Neither healing spell hardcodes its modifier: two different casters
    /// produce two different heal formulas.
    #[test]
    fn the_modifier_comes_from_the_casting_profile_not_a_constant() {
        let low = wisdom_caster(0, 3)
            .apply_feature(&CureWoundsPlugin::new())
            .unwrap();
        let high = wisdom_caster(5, 3)
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
        let builder = wisdom_caster(3, 3)
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
        let builder = wisdom_caster(4, 3)
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

    // --- Hold Person ---

    #[test]
    fn hold_person_is_a_concentration_action_with_the_right_slot_and_save() {
        let builder = wisdom_caster(3, 3)
            .apply_feature(&HoldPersonPlugin::new())
            .expect("Hold Person applies to a caster");

        assert!(builder.creature.bonus_actions.is_empty());
        assert_eq!(builder.creature.actions.len(), 1);
        let mv = &builder.creature.actions[0];
        assert_eq!(mv.name, "Hold Person");
        assert_eq!(mv.spell_slot_level, Some(2));
        assert!(mv.concentration, "Hold Person must require concentration");

        match &mv.effect {
            Effect::Save(save) => {
                assert_eq!(save.ability, Ability::Wis);
                assert_eq!(save.dc, 14); // 8 + 3 (mod) + 3 (prof)
                assert!(save.damage.is_empty(), "Hold Person deals no damage");
                assert_eq!(save.max_targets, Some(1));
                assert_eq!(save.requires_type.as_deref(), Some("Humanoid"));
                assert_eq!(
                    save.on_failure,
                    vec![(
                        Condition::Paralyzed,
                        Duration::SaveEndTurn {
                            ability: Ability::Wis,
                            dc: 14,
                        },
                    )]
                );
            }
            other => panic!("expected a Save effect, got {other:?}"),
        }
    }

    #[test]
    fn a_non_caster_is_rejected_rather_than_baking_in_no_dc() {
        let builder = CreatureBuilder::new("Mute", 10, 10);
        let err = builder
            .apply_feature(&HoldPersonPlugin::new())
            .expect_err("no spellcasting profile means no DC to bake in");
        assert!(matches!(err, FeatureError::InvalidConfiguration(_)));
    }

    /// Two different casters must bake in two different DCs: nothing here is
    /// allowed to hardcode "DC 13" the way a homebrew shortcut would.
    #[test]
    fn the_dc_comes_from_the_casting_profile_not_a_constant() {
        let low = wisdom_caster(0, 2)
            .apply_feature(&HoldPersonPlugin::new())
            .unwrap();
        let high = wisdom_caster(5, 4)
            .apply_feature(&HoldPersonPlugin::new())
            .unwrap();
        let Effect::Save(low_save) = &low.creature.actions[0].effect else {
            unreachable!()
        };
        let Effect::Save(high_save) = &high.creature.actions[0].effect else {
            unreachable!()
        };
        assert_eq!(low_save.dc, 10); // 8 + 0 + 2
        assert_eq!(high_save.dc, 17); // 8 + 5 + 4
        assert_ne!(low_save.dc, high_save.dc);
    }

    /// The core invariant this whole project is built on, applied to a save
    /// that produces no damage at all: the exact failure chance and a large
    /// sample of rolled saves must agree.
    #[test]
    fn hold_person_failure_rate_matches_between_exact_and_sampled() {
        let builder = wisdom_caster(4, 3)
            .apply_feature(&HoldPersonPlugin::new())
            .unwrap();
        let Effect::Save(save) = &builder.creature.actions[0].effect else {
            unreachable!()
        };

        let mut target = Creature::new("Guard", 12, 11);
        target.creature_type = Some(CreatureType::Humanoid);
        target.saves[Ability::Wis.index()] = 1;

        let exact = save.failure_chance(&target);
        let mut rng = Rng::new(99);
        const N: usize = 200_000;
        let mut fails = 0usize;
        for _ in 0..N {
            let (_, saved) = save.sample(&mut rng, &target);
            if !saved {
                fails += 1;
            }
        }
        let got = fails as f64 / N as f64;
        let tol = 5.0 * (exact * (1.0 - exact) / N as f64).sqrt() + 1e-4;
        assert!(
            (got - exact).abs() < tol,
            "sampled failure rate {got:.5}, exact {exact:.5}, tolerance {tol:.5}"
        );
    }

    /// End to end: casting Hold Person on a humanoid that fails its save
    /// paralyzes it, and a follow-up strike in the *same* turn (a bonus
    /// action, after the action that cast the spell) auto-crits - the whole
    /// reason Paralyzed is worse than Stunned.
    #[test]
    fn casting_hold_person_paralyzes_a_humanoid_and_a_same_turn_strike_auto_crits() {
        let mut caster = wisdom_caster(4, 3)
            .apply_feature(&HoldPersonPlugin::new())
            .unwrap()
            .creature;
        caster.initiative = 100;
        caster.bonus_actions.push(Move::new(
            "Dagger",
            Effect::Strikes {
                strike: Strike::new(30, vec![DamageRoll::new(1, 4, 0, DamageKind::Piercing)]),
                count: 1,
            },
        ));

        let mut victim = Creature::new("Bandit", 10, 20);
        victim.creature_type = Some(CreatureType::Humanoid);
        victim.saves[Ability::Wis.index()] = -50; // fails every Wisdom save, unconditionally
        victim.initiative = -100;

        let mut rng = Rng::new(7);
        let mut log = Some(Vec::new());
        run(
            &mut rng,
            [&caster, &victim],
            [Policy::Greedy; 2],
            1,
            &mut log,
        );
        let narration = log.unwrap().join("\n");
        assert!(
            narration.contains("paralyzed"),
            "the save should fail and land Paralyzed:\n{narration}"
        );
        assert!(
            narration.contains("crit"),
            "a follow-up hit against a paralyzed target must be an automatic crit:\n{narration}"
        );
    }

    /// The type restriction is checked before a save is even rolled: a
    /// non-humanoid is never caught, no matter how unbeatable the save would
    /// have been.
    #[test]
    fn hold_person_never_catches_a_non_humanoid() {
        let mut caster = wisdom_caster(4, 3)
            .apply_feature(&HoldPersonPlugin::new())
            .unwrap()
            .creature;
        caster.initiative = 100;

        let mut dragon = Creature::new("Wyrmling", 17, 60);
        dragon.creature_type = Some(CreatureType::Dragon);
        dragon.saves[Ability::Wis.index()] = -50; // would always fail, if it were even caught
        dragon.initiative = -100;

        let mut rng = Rng::new(3);
        let mut log = Some(Vec::new());
        let o = run(
            &mut rng,
            [&caster, &dragon],
            [Policy::Greedy; 2],
            3,
            &mut log,
        );
        let narration = log.unwrap().join("\n");
        assert!(
            !narration.contains("paralyzed"),
            "a dragon must never be caught by a humanoid-only spell:\n{narration}"
        );
        assert_eq!(
            o.turns_lost[1], 0,
            "never paralyzed, so it never loses a turn to it"
        );
    }

    /// A single 2nd-level slot casts Hold Person exactly once, even when the
    /// caster's only action is Hold Person and the target's save can never
    /// succeed: after the first cast, the empty pool - not a lack of
    /// desire - is what stops a second one.
    #[test]
    fn hold_person_only_casts_as_many_times_as_it_has_slots_for() {
        let caster = wisdom_caster(4, 3)
            .apply_feature(&HoldPersonPlugin::new())
            .unwrap()
            .creature;
        assert_eq!(caster.spell_slots.available(2), 1);
        let mut caster = caster;
        caster.initiative = 100;

        let mut victim = Creature::new("Bandit", 10, 20);
        victim.creature_type = Some(CreatureType::Humanoid);
        victim.saves[Ability::Wis.index()] = -50;
        victim.initiative = -100;

        let mut rng = Rng::new(11);
        let mut log = None;
        let o = run(
            &mut rng,
            [&caster, &victim],
            [Policy::Greedy; 2],
            4,
            &mut log,
        );
        assert_eq!(
            o.resources_spent[0], 1,
            "one slot means one cast, however many turns follow"
        );
    }

    /// Hold Person's own repeat save is not academic: with a beatable Wisdom
    /// save, the check `sim::duel`'s end-of-turn loop runs at the end of
    /// every one of the victim's turns actually clears the condition, the
    /// same persistent-vs-easy contrast `sim::duel`'s own
    /// `a_repeatable_save_can_end_a_condition_the_turn_it_lands` draws for
    /// `Duration::SaveEndTurn` in general, reproduced here for the concrete
    /// spell rather than a synthetic rider.
    #[test]
    fn hold_persons_own_repeat_save_can_end_it_the_turn_it_lands() {
        let tally = |save_bonus: i32| {
            let mut rng = Rng::new(23);
            let (mut lost, mut fights) = (0u32, 0u32);
            for _ in 0..300 {
                let mut caster = wisdom_caster(2, 2) // dc = 8 + 2 + 2 = 12
                    .apply_feature(&HoldPersonPlugin::new())
                    .unwrap()
                    .creature;
                caster.initiative = 100;

                let mut victim = Creature::new("Bandit", 10, 20);
                victim.creature_type = Some(CreatureType::Humanoid);
                victim.saves[Ability::Wis.index()] = save_bonus;
                victim.initiative = -100;

                let mut log = None;
                let o = run(
                    &mut rng,
                    [&caster, &victim],
                    [Policy::Greedy; 2],
                    6,
                    &mut log,
                );
                lost += o.turns_lost[1];
                fights += 1;
            }
            f64::from(lost) / f64::from(fights)
        };

        let persistent = tally(-50); // needs a 62 on a d20: never happens
        let escapable = tally(8); // dc 12 needs only a 4+: usually saves

        assert!(
            persistent > 3.0,
            "an unbeatable Hold Person save should cost most turns: {persistent}"
        );
        assert!(
            escapable < persistent,
            "a beatable repeat save should free the victim sooner: {escapable} vs {persistent}"
        );
    }

    /// Hold Person is a concentration spell: when the caster's concentration
    /// breaks, the paralysis it was maintaining lifts immediately, without
    /// waiting for - or needing - the victim's own repeated save at all.
    #[test]
    fn losing_concentration_ends_hold_persons_paralysis_early() {
        fn caster() -> Creature {
            let mut c = wisdom_caster(4, 3)
                .apply_feature(&HoldPersonPlugin::new())
                .unwrap()
                .creature;
            c.initiative = 100;
            // Forced to roll, this save always fails, so any damage that
            // reaches the caster ends concentration outright.
            c.saves[Ability::Con.index()] = -50;
            c
        }

        fn victim() -> Creature {
            let mut v = Creature::new("Bandit", 10, 20);
            v.creature_type = Some(CreatureType::Humanoid);
            v.team = 1;
            v.initiative = -50;
            v.saves[Ability::Wis.index()] = -50; // never saves on its own
            v
        }

        // Control: nothing threatens the caster, so concentration never
        // breaks and the victim's own Wisdom save never succeeds either -
        // the paralysis should hold for every round that follows.
        let held = {
            let c = caster();
            let v = victim();
            let mut rng = Rng::new(4);
            let mut log = None;
            run(&mut rng, [&c, &v], [Policy::Greedy; 2], 3, &mut log).turns_lost[1]
        };
        assert!(
            held >= 2,
            "an unbroken Hold Person should cost the victim multiple turns: {held}"
        );

        // An ally that reaches the caster forces - and always fails - the
        // concentration save, ending Hold Person before the victim's own
        // save ever gets the credit for freeing it.
        let broken = {
            let c = caster();
            let v = victim();
            let mut ally = Creature::new("Wolf", 12, 15).with_action(Move::new(
                "Bite",
                Effect::Strikes {
                    strike: Strike::new(30, vec![DamageRoll::new(2, 6, 4, DamageKind::Piercing)]),
                    count: 1,
                },
            ));
            ally.creature_type = Some(CreatureType::Beast);
            ally.team = 1;
            ally.initiative = 50; // after the caster, before the victim

            let mut rng = Rng::new(4);
            let mut log = None;
            run_teams(
                &mut rng,
                &[&c, &v, &ally],
                [Policy::Greedy; 2],
                3,
                Budget::default(),
                &mut log,
            )
            .turns_lost[1]
        };
        assert!(
            broken < held,
            "a broken concentration should cost the victim fewer turns than an unbroken one: {broken} vs {held}"
        );
    }

    // --- Blindness/Deafness ---

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

    // --- Command ---

    #[test]
    fn command_needs_a_spellcasting_profile() {
        let mut builder = CreatureBuilder::new("Caster", 12, 20);
        let err = CommandPlugin::new(CommandWord::Grovel, None)
            .apply(&mut builder)
            .expect_err("no spellcasting profile means no save DC to compute");
        assert!(matches!(err, FeatureError::InvalidConfiguration(_)));
    }

    #[test]
    fn grovel_forces_prone_and_denies_the_rest_of_the_turn() {
        let mut builder = caster(Ability::Wis, 3, 2); // DC 8 + 3 + 2 = 13
        CommandPlugin::new(CommandWord::Grovel, None)
            .apply(&mut builder)
            .unwrap();
        let Effect::Save(save) = effect_of(&builder.creature.actions[0]) else {
            panic!("expected a Save effect");
        };
        assert_eq!(save.ability, Ability::Wis);
        assert_eq!(save.dc, 13);
        assert_eq!(
            save.on_failure,
            vec![
                (Condition::Prone, Duration::ApplierTurn),
                (Condition::Compelled, Duration::ApplierTurn),
            ]
        );
    }

    #[test]
    fn halt_only_compels_no_prone() {
        let mut builder = caster(Ability::Wis, 3, 2);
        CommandPlugin::new(CommandWord::Halt, None)
            .apply(&mut builder)
            .unwrap();
        let Effect::Save(save) = effect_of(&builder.creature.actions[0]) else {
            panic!("expected a Save effect");
        };
        assert_eq!(
            save.on_failure,
            vec![(Condition::Compelled, Duration::ApplierTurn)]
        );
    }

    #[test]
    fn command_word_names_round_trip() {
        assert_eq!(CommandWord::parse("grovel"), Some(CommandWord::Grovel));
        assert_eq!(CommandWord::parse("HALT"), Some(CommandWord::Halt));
        assert_eq!(CommandWord::parse("flee"), None);
    }

    /// The same closed-form-vs-sampled agreement as Blindness/Deafness,
    /// exercised against Command's Wisdom save instead of a Constitution one.
    #[test]
    fn command_failure_chance_agrees_with_sampled_saves() {
        let mut builder = caster(Ability::Cha, 2, 3); // DC 8 + 2 + 3 = 13
        CommandPlugin::new(CommandWord::Grovel, None)
            .apply(&mut builder)
            .unwrap();
        let Effect::Save(save) = effect_of(&builder.creature.actions[0]) else {
            panic!("expected a Save effect");
        };

        let victim = target(14, -1, &[]);
        let exact = save.failure_chance(&victim);

        let mut rng = Rng::new(77);
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

    /// An end-to-end fight, not just the `Move` shape: Grovel really does
    /// cost the target its very next turn (via `turns_lost`) and really does
    /// leave it Prone, using the duel engine exactly as any other condition
    /// does - the acceptance test for `sim::duel::Fighter::loses_turn`.
    #[test]
    fn grovel_costs_the_target_its_next_turn_in_a_real_fight() {
        use crate::sim::duel::{run, Policy};

        let mut builder = caster(Ability::Wis, 5, 4); // DC 8 + 5 + 4 = 17
        builder.creature.initiative = 100;
        CommandPlugin::new(CommandWord::Grovel, None)
            .apply(&mut builder)
            .unwrap();
        let mut caster = builder.build().unwrap();
        // One cast only: a caster who keeps recasting Command every round
        // would keep the target compelled continuously, which is correct
        // behaviour but would defeat what this test checks - that a single
        // application clears on schedule rather than lingering.
        caster.actions[0].uses = Uses::Limited(1);

        let mut victim = Creature::new("victim", 10, 100);
        victim.saves[Ability::Wis.index()] = -20; // never saves
        victim.initiative = -100;

        let mut rng = Rng::new(31);
        let mut log = Some(Vec::new());
        let o = run(
            &mut rng,
            [&caster, &victim],
            [Policy::Greedy; 2],
            3,
            &mut log,
        );
        let narration = log.unwrap().join("\n");

        assert!(
            narration.contains("prone") && narration.contains("compelled"),
            "Grovel should land both conditions:\n{narration}"
        );
        assert!(
            o.turns_lost[1] >= 1,
            "the target must lose at least the one turn Grovel denies it"
        );
        // The fight ran three rounds and the victim cannot hurt back (no
        // actions of its own), so it must not have lost *every* turn -
        // Compelled expires after the one turn it was meant for.
        assert!(
            o.turns_lost[1] < o.rounds,
            "Compelled must not persist past the one turn it denies: lost {} of {} rounds",
            o.turns_lost[1],
            o.rounds
        );
    }

    // --- Magic Missile ---

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

    #[test]
    fn bless_registers_a_concentration_action_with_bonus_dice_on_attacks_and_saves() {
        let builder = CreatureBuilder::new("Cleric", 15, 30);
        let built = builder
            .apply_feature(&BlessPlugin)
            .expect("bless applies")
            .build()
            .expect("builds");
        assert_eq!(built.actions.len(), 1);
        let m = &built.actions[0];
        assert_eq!(m.name, "Bless");
        assert!(m.concentration, "Bless requires concentration");
        assert_eq!(m.spell_level, Some(1), "Bless spends a 1st-level slot");
        assert_eq!(
            m.effect,
            Effect::Buff {
                attack_modifier: AttackModifier::BonusDice { count: 1, sides: 4 },
                save_modifier: SaveModifier::BonusDice { count: 1, sides: 4 },
                max_targets: Some(3),
            }
        );
    }

    #[test]
    fn bane_registers_a_concentration_action_gated_on_a_charisma_save() {
        let builder = CreatureBuilder::new("Warlock", 15, 30);
        let built = builder
            .apply_feature(&BanePlugin)
            .expect("bane applies")
            .build()
            .expect("builds");
        assert_eq!(built.actions.len(), 1);
        let m = &built.actions[0];
        assert_eq!(m.name, "Bane");
        assert!(m.concentration, "Bane requires concentration");
        assert_eq!(m.spell_level, Some(1), "Bane spends a 1st-level slot");
        assert_eq!(
            m.effect,
            Effect::SaveOrModifier {
                ability: Ability::Cha,
                attack_modifier: AttackModifier::PenaltyDice { count: 1, sides: 4 },
                save_modifier: SaveModifier::PenaltyDice { count: 1, sides: 4 },
                max_targets: Some(3),
            }
        );
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
