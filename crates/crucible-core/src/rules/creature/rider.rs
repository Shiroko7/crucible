//! Triggered modifiers and reactions (riders).

use super::combatant::Creature;
use super::damage::{DamageKind, DamageRoll};
use super::types::{Ability, Condition, Cost, CreatureType, Duration, Size};
use crate::prob::rng::Rng;
use crate::rules::combat::{Attack, DamageRider, RollMode};

/// What kind of incoming attack a [`Rider::ReactionOnTargeted`] answers.
///
/// One variant today, because nothing in the engine yet distinguishes a
/// melee attack roll from a ranged or spell one the way
/// [`Attack::finesse_or_ranged`] distinguishes weapon properties. The field
/// exists anyway so a future distinction - a reaction that only answers a
/// melee attack, say - is a new variant matched at the same call site, not a
/// new field threaded through every caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackTrigger {
    /// Any attack roll made against this creature.
    AnyAttack,
}

/// A triggered modifier.
///
/// Each variant is a mechanism, not a feature. The comments name the features
/// that map onto it, which is the test of whether the abstraction is pulling
/// its weight: a variant only one ability can use is a branch in disguise.
#[derive(Debug, Clone, PartialEq)]
pub enum Rider {
    /// On a hit, the target saves or takes a condition.
    ///
    /// Stunning Strike. Also every knockdown, every on-hit poison, and the
    /// secondary effect on most breath weapons.
    SaveOrCondition {
        ability: Ability,
        dc: i32,
        condition: Condition,
        duration: Duration,
        cost: Option<Cost>,
        /// Stunning Strike is once per turn however many times you hit.
        once_per_turn: bool,
    },
    /// A successful save against an effect that would deal half takes none
    /// instead, and a failed one takes half.
    ///
    /// Evasion, for the ability the effect names. Danger Sense and a rogue's
    /// Evasion are the same shape.
    NothingOnSuccess { ability: Ability },
    /// Turn a failed save into a success, a fixed number of times per fight.
    ///
    /// Legendary Resistance. A fighter's Indomitable is the same shape with
    /// one use and a reroll instead of a pass.
    AlwaysSucceed { uses: u32 },
    /// A reaction that reduces the damage of an incoming *attack* whose types
    /// include one of `kinds`.
    ///
    /// Deflect Attacks. Uncanny Dodge and Heavy Armor Master are variations.
    ReduceDamage {
        kinds: Vec<DamageKind>,
        roll: DamageRoll,
        /// Reactions refresh at the start of the creature's turn.
        per_round: u32,
    },
    /// A reaction spent on being *targeted* by an attack, before its hit or
    /// miss is finalized, that adds `ac_bonus` to this creature's AC against
    /// that one attack - capable of turning what would have been a hit into
    /// a miss.
    ///
    /// The mirror of [`Rider::ReduceDamage`]: that one reacts to an attack
    /// that already hit, on its damage; this one reacts to being targeted,
    /// before the roll against AC is decided. A reaction that boosts AC
    /// against a targeting attack - the Shield spell, a Ring of Protection's
    /// reactive bonus, and any homebrew item shaped the same way - is this
    /// mechanism; nothing here is specific to any one of them.
    ReactionOnTargeted {
        trigger: AttackTrigger,
        ac_bonus: i32,
        /// Reactions refresh at the start of the creature's turn, the same
        /// as [`Rider::ReduceDamage::per_round`].
        per_round: u32,
    },
    /// Extra damage dice on a hit, gated on the attack roll having advantage
    /// or an ally next to the target - and never at all if the attacker also
    /// has disadvantage, which overrides an ally in place. Spendable once per
    /// turn if `once_per_turn`.
    ///
    /// Sneak Attack. The gate itself is evaluated by whoever resolves the
    /// attack, from flags on the attack rather than derived geometry - see
    /// [`crate::rules::combat::Attack::ally_adjacent`] and
    /// [`crate::rules::combat::Attack::finesse_or_ranged`] for why. This
    /// variant only carries the dice pool and the once-per-turn budget, so
    /// anything else that shares the exact same gate is this variant too,
    /// not a new branch.
    ConditionalExtraDamage {
        dice_count: u32,
        dice_sides: u32,
        once_per_turn: bool,
    },
    /// The 2024 Rogue's Cunning Strike (Rogue 5) is unlocked, at this save
    /// DC: `8 + Dexterity modifier + proficiency bonus`, computed once by
    /// [`crate::dsl::plugin::CunningStrikePlugin`] and never the creature's
    /// spellcasting DC - Cunning Strike is not spellcasting, and a Rogue
    /// without a caster subclass has no [`SpellCastingProfile`] to read one
    /// from at all.
    ///
    /// A pure marker, deliberately carrying no dice of its own: what it
    /// unlocks is spending part of a *qualifying Sneak Attack's* pool - the
    /// [`DamageRider`] [`Rider::extra_damage_for`] returns for
    /// [`Rider::ConditionalExtraDamage`] - on a rider effect instead of
    /// rolling it for damage, [`DamageRider::spend`] dice at a time (1d6 per
    /// the 2024 rules), each spend costed and combined by whoever resolves
    /// the attack. Several spends can share one hit's pool as long as their
    /// total fits, because [`DamageRider::spend`] is exactly the same
    /// operation chained.
    ///
    /// Which specific effects a spend buys - poison, a shove, breaking a
    /// grapple - is deliberately not here: this is the generic framework, and
    /// a later plugin per effect reads this same DC rather than inventing its
    /// own.
    ///
    /// [`SpellCastingProfile`]: super::types::SpellCastingProfile
    CunningStrike { dc: i32 },
    /// Extra damage dice on a hit against a creature of one specific type -
    /// gated on the *target's* [`CreatureType`] rather than anything about the
    /// attack roll itself.
    ///
    /// A slaying weapon's bonus against its favoured prey, a paladin's Smite
    /// against fiends and undead, a ranger's Favored Enemy damage - every
    /// "extra dice when the target is a `X`" feature is this variant, not a
    /// branch per weapon or feature. Unlike [`Rider::ConditionalExtraDamage`]
    /// there is no once-per-turn budget: none of those features are limited
    /// that way, so every qualifying hit gets the bonus.
    ///
    /// `damage_kind` is parsed and validated same as every other damage
    /// component in this DSL, but - like [`Rider::ConditionalExtraDamage`]'s
    /// dice, which do not carry the weapon's own damage type either - the
    /// [`DamageRider`] this contributes has no type of its own to compose with
    /// `combat::damage_pmf`'s single [`crate::rules::combat::Reduction`]. A
    /// future type-aware resistance path on the exact/sampled attack model
    /// would read it from here rather than needing a new field.
    BonusDamageVsCreatureType {
        dice_count: u32,
        dice_sides: u32,
        bonus: i32,
        damage_kind: DamageKind,
        creature_type: CreatureType,
    },
    /// The 2024 Rogue's Cunning Strike: Trip option (Rogue 5) is unlocked:
    /// spend 1d6 of a qualifying Sneak Attack's pool (see
    /// [`Rider::resolve_cunning_strike_trip`]) to force a Dexterity save,
    /// against [`Rider::CunningStrike`]'s DC, on a target that is Large size
    /// or smaller - knocking it [`Condition::Prone`] on a failure.
    ///
    /// A pure marker like [`Rider::CunningStrike`] itself: it carries no
    /// dice or DC of its own, always reading [`Rider::CunningStrike`]'s.
    CunningStrikeTrip,
    /// The 2024 Rogue's Cunning Strike: Withdraw option (Rogue 5) is
    /// unlocked: spend 1d6 of a qualifying Sneak Attack's pool (see
    /// [`Rider::resolve_cunning_strike_withdraw`]) to move up to half speed
    /// without provoking opportunity attacks.
    ///
    /// There is no movement or opportunity-attack model here for that to
    /// actually change - see `DESIGN.md` and the README's "Positioning is
    /// the gap that matters" note - so resolving this spends the die and
    /// does no more than flag that the rogue withdrew safely, the same shape
    /// ROG-05's Cunning Action lands on for Dash and Disengage (zero-effect
    /// moves, because the engine has nothing for them to change either).
    CunningStrikeWithdraw,
    /// An attacker-side trait: this creature's own damage and inflicted
    /// conditions punch through a target's immunity, though not all the way
    /// to full effect.
    ///
    /// `damage` softens a target normally [`super::combatant::Creature`]-immune to
    /// that damage type down to merely resistant (half instead of zero)
    /// against a hit *this* creature lands - see
    /// [`super::combatant::Creature::reduction_from`]. `condition` does the
    /// same for a condition this creature inflicts: a target normally immune
    /// to it still has to make the save, rolled with Advantage instead of
    /// auto-succeeding - see [`saving_throw_against_condition`].
    ///
    /// Both are attacker-scoped, never a change to the target's own stat
    /// sheet: a different attacker without this trait, against the very
    /// same target, still sees it as fully immune either way.
    ///
    /// The two fields are independent - an attacker might carry only one
    /// half of this, punching through only a damage type or only a
    /// condition - which is why this is one variant with two `Option`s
    /// rather than two variants: the parser reads whichever of `... damage`
    /// and `... condition` is present in one trait string, in any
    /// combination.
    DowngradeImmunity {
        damage: Option<DamageKind>,
        condition: Option<Condition>,
    },
    /// Marks a creature whose [`Rider::ConditionalExtraDamage`] also accepts
    /// a qualifying spell attack roll
    /// ([`crate::rules::combat::Attack::is_spell_attack`]), not only a
    /// finesse-or-ranged weapon attack
    /// ([`crate::rules::combat::Attack::finesse_or_ranged`]).
    ///
    /// Most creatures with `ConditionalExtraDamage` do not carry this - it
    /// is the "some builds grant a feature that lets a non-weapon spell
    /// attack also qualify" extension, gated the same way
    /// [`Rider::NothingOnSuccess`]'s evasion check is: a second rider in the
    /// same list, queried by [`crate::rules::creature::Creature::extra_damage_applies_to_spell_attacks`]
    /// rather than a field added to `ConditionalExtraDamage` itself, so a
    /// creature can carry the extension without every existing
    /// `ConditionalExtraDamage` construction site needing to know about it.
    /// A no-op on its own; it only changes what
    /// [`Rider::extra_damage_for_with_spell_attack_extension`] does with a
    /// sibling `ConditionalExtraDamage` rider.
    ExtraDamageAppliesToSpellAttacks,
    /// An attacker-side buff, dormant until this creature inflicts `trigger`
    /// on a target via a weapon attack - after which its weapon attacks
    /// carry `dice_count`d`dice_sides` (plus `bonus`) extra `damage_kind`
    /// damage for the rest of the encounter.
    ///
    /// Generic over which condition arms it: a weapon that empowers itself
    /// after poisoning something is the flavour ITM-06 names, but nothing
    /// here reads [`Condition::Poisoned`] specifically, and the same shape
    /// covers "hits harder after landing a knockdown" or any other
    /// "inflict X, then hit harder" item.
    ///
    /// A pure marker, deliberately carrying no notion of whether it has
    /// fired yet - that is per-fight state, not a creature's static kit, the
    /// same split [`Rider::AlwaysSucceed`]'s remaining uses and
    /// [`Rider::ReduceDamage`]'s per-round budget already draw between the
    /// rider's fixed parameters and `sim::duel`'s own bookkeeping. See
    /// [`Rider::arms_on_condition`] and [`Rider::weapon_damage_if_armed`],
    /// which take that armed/not-armed flag as a plain `bool` rather than
    /// storing it here.
    ///
    /// `damage_kind` is recorded and validated like any other damage
    /// component in this DSL, but - like [`Rider::ConditionalExtraDamage`]'s
    /// dice - the [`DamageRider`] this contributes has no type of its own to
    /// compose with `combat::damage_pmf`'s single
    /// [`crate::rules::combat::Reduction`]; a future type-aware resistance
    /// path on the exact/sampled attack model would read it from here.
    ConditionTriggeredWeaponDamage {
        trigger: Condition,
        dice_count: u32,
        dice_sides: u32,
        bonus: i32,
        damage_kind: DamageKind,
    },
    /// A consumable injury poison coating a weapon: the next hit forces
    /// `ability`/`dc` as a saving throw, and a failure burdens the target's
    /// own future `debuffed_ability` saving throws with
    /// [`RollMode::Disadvantage`] for `duration`.
    ///
    /// Not a [`Condition`] at all - 5e's condition list has nothing this
    /// general ("disadvantage on one specific kind of saving throw"), so this
    /// is a dedicated variant rather than stretching
    /// [`Rider::SaveOrCondition`] to cover a debuff it cannot express. See
    /// [`injury_poison_forcing_save`] for the forcing save, and
    /// [`save_with_mode`] for the generic "roll a save under a [`RollMode`]"
    /// mechanism the resulting debuff itself uses once applied - the same
    /// [`RollMode`] an attack roll already rolls under, generalised to saves.
    ///
    /// Generic over both abilities and never named after a specific poison:
    /// `ability`/`dc` is typically a Constitution save against a poison, and
    /// `debuffed_ability` is whichever save the poison burdens, but nothing
    /// here reads either as such. Using up the coating itself - "the next
    /// hit" - is the caller's business: a single-use item is a rider with no
    /// [`Rider::initial_uses`] budget of its own, present on the wielder only
    /// while the coating lasts.
    InjuryPoison {
        ability: Ability,
        dc: i32,
        debuffed_ability: Ability,
        duration: Duration,
    },
}

impl Rider {
    /// Riders with a budget need somewhere to count it down.
    pub fn initial_uses(&self) -> u32 {
        match self {
            Rider::AlwaysSucceed { uses } => *uses,
            Rider::ReduceDamage { per_round, .. } => *per_round,
            Rider::ReactionOnTargeted { per_round, .. } => *per_round,
            _ => 0,
        }
    }

    /// The [`DamageRider`] this rider contributes to `attack`, or `None` if
    /// it does not apply - either because this variant is not
    /// [`Rider::ConditionalExtraDamage`], or because its gate does not hold.
    ///
    /// The gate (Sneak Attack's, specifically): a finesse or ranged weapon,
    /// on a roll that is not at disadvantage, with either advantage or an
    /// ally next to the target. `used_this_turn` is the once-per-turn
    /// budget, tracked by the caller - the same shared per-creature flag
    /// `sim::duel` already keeps for Stunning-Strike-style riders (see
    /// `README.md`'s note that a creature gets one once-per-turn rider
    /// trigger per turn in total, not one per rider).
    ///
    /// Returns the *full* qualifying dice pool. A feature that spends part
    /// of it on something other than damage (Cunning Strike) reduces
    /// `dice_count` on the result before handing it to
    /// [`Attack::with_damage_rider`] - [`DamageRider`] is a plain count of
    /// dice, so rolling fewer of them is not a special case.
    ///
    /// This is the plain weapon-only gate - equivalent to calling
    /// [`Rider::extra_damage_for_with_spell_attack_extension`] with
    /// `spell_attacks_extended: false`, kept as its own method so the common
    /// case (a creature with no spell-attack extension) reads without an
    /// extra argument that would always be `false` for it.
    pub fn extra_damage_for(&self, attack: &Attack, used_this_turn: bool) -> Option<DamageRider> {
        self.extra_damage_for_with_spell_attack_extension(attack, used_this_turn, false)
    }

    /// As [`Rider::extra_damage_for`], but also accepts a spell attack roll
    /// ([`Attack::is_spell_attack`]) when `spell_attacks_extended` is `true`.
    /// That flag is what a creature carrying
    /// [`Rider::ExtraDamageAppliesToSpellAttacks`] passes in, via
    /// [`crate::rules::creature::Creature::extra_damage_applies_to_spell_attacks`].
    ///
    /// The gate: a finesse-or-ranged weapon attack always qualifies; a spell
    /// attack qualifies only when the extension is present; either way, the
    /// roll still needs advantage or an ally adjacent, and disadvantage
    /// still overrides both. Saving-throw spells never reach this check at
    /// all - they are resolved via `SaveEffect`, not an [`Attack`], so there
    /// is no hit roll and no [`Rider::extra_damage_for`] call in the first
    /// place; this only ever fires from attack-roll resolution.
    ///
    /// The damage type: when `attack` carries an
    /// [`Attack::spell_damage_kind`] - set only for a spell attack - the
    /// returned [`DamageRider::kind`] is pinned to match it, the 5e rule
    /// that this kind of extra damage takes on the triggering *spell's* own
    /// damage type rather than a fixed default. A weapon attack never sets
    /// `spell_damage_kind`, so it falls through unchanged: the rider's
    /// `kind` stays `None` and it is reduced exactly like the rest of the
    /// hit, same as before this distinction existed.
    pub fn extra_damage_for_with_spell_attack_extension(
        &self,
        attack: &Attack,
        used_this_turn: bool,
        spell_attacks_extended: bool,
    ) -> Option<DamageRider> {
        let Rider::ConditionalExtraDamage {
            dice_count,
            dice_sides,
            once_per_turn,
        } = self
        else {
            return None;
        };
        if *once_per_turn && used_this_turn {
            return None;
        }
        let qualifying_attack =
            attack.finesse_or_ranged || (attack.is_spell_attack && spell_attacks_extended);
        if !qualifying_attack || attack.mode == RollMode::Disadvantage {
            return None;
        }
        if attack.mode != RollMode::Advantage && !attack.ally_adjacent {
            return None;
        }
        let rider = DamageRider::new(*dice_count, *dice_sides);
        Some(match attack.spell_damage_kind {
            Some(kind) => rider.with_kind(kind),
            None => rider,
        })
    }

    /// The Cunning Strike DC this rider carries, or `None` if it is not
    /// [`Rider::CunningStrike`].
    ///
    /// A creature's full rider list is a `Vec<Rider>`, so the usual way to
    /// call this is `creature.riders.iter().find_map(Rider::cunning_strike_dc)` -
    /// the same "scan the list for the variant you care about" shape
    /// [`Creature::has_evasion`] already uses for
    /// [`Rider::NothingOnSuccess`].
    ///
    /// [`Creature::has_evasion`]: super::combatant::Creature::has_evasion
    pub fn cunning_strike_dc(&self) -> Option<i32> {
        match self {
            Rider::CunningStrike { dc } => Some(*dc),
            _ => None,
        }
    }

    /// The [`DamageRider`] this rider contributes against a target of
    /// `target_creature_type`, or `None` if this variant is not
    /// [`Rider::BonusDamageVsCreatureType`] or the target's type does not
    /// match - including a target with no declared type at all, which can
    /// never match a specific one.
    ///
    /// Takes the type rather than the whole target [`super::combatant::Creature`]
    /// because the gate only ever needs that one field - the same shape as
    /// [`Rider::extra_damage_for`] taking the attack's own flags rather than
    /// the whole [`Attack`]'s owner.
    ///
    /// Composes with any other active rider rather than replacing it: this
    /// and [`Rider::extra_damage_for`] each contribute their own
    /// [`DamageRider`] to the same [`Attack::damage_riders`] list, so a
    /// qualifying hit from a rogue wielding a favoured weapon gets both.
    pub fn bonus_damage_vs_creature_type(
        &self,
        target_creature_type: Option<CreatureType>,
    ) -> Option<DamageRider> {
        let Rider::BonusDamageVsCreatureType {
            dice_count,
            dice_sides,
            bonus,
            creature_type,
            ..
        } = self
        else {
            return None;
        };
        if target_creature_type != Some(*creature_type) {
            return None;
        }
        Some(DamageRider::new(*dice_count, *dice_sides).with_bonus(*bonus))
    }

    /// Resolve a Cunning Strike: Trip attempt, or `None` if this rider is
    /// not [`Rider::CunningStrikeTrip`], `target_size` is bigger than Large
    /// (a Huge or Gargantuan target cannot be tripped at all, so the die is
    /// never spent - an illegal declaration, the same shape
    /// [`DamageRider::spend`] already gives an overdrawn pool), or `pool`
    /// cannot afford the 1d6 cost.
    ///
    /// On a legal attempt, spends the die and rolls a Dexterity save
    /// against `dc` (see [`Rider::cunning_strike_dc`]) at `target_dex_save`,
    /// returning the reduced pool alongside [`Condition::Prone`] on a
    /// failed save or `None` on a successful one.
    ///
    /// This rolls the save directly with `rng` rather than through
    /// `sim::duel`'s save handling - which also lets
    /// [`Rider::AlwaysSucceed`] buy back a failure and auto-fails Strength
    /// and Dexterity saves for an Incapacitated target - because no policy
    /// layer yet decides *when* a rogue spends Sneak Attack dice on Trip
    /// instead of full damage. Wiring that choice into a live fight is
    /// future work, same as the rest of "which effects a spend buys" per
    /// [`Rider::CunningStrike`]'s doc comment.
    pub fn resolve_cunning_strike_trip(
        &self,
        pool: DamageRider,
        target_size: Size,
        target_dex_save: i32,
        dc: i32,
        rng: &mut Rng,
    ) -> Option<(DamageRider, Option<Condition>)> {
        if !matches!(self, Rider::CunningStrikeTrip) {
            return None;
        }
        if target_size > Size::Large {
            return None;
        }
        let reduced = pool.spend(1)?;
        let saved = rng.die(20) + target_dex_save >= dc;
        Some((reduced, (!saved).then_some(Condition::Prone)))
    }

    /// Resolve a Cunning Strike: Withdraw attempt, or `None` if this rider
    /// is not [`Rider::CunningStrikeWithdraw`] or `pool` cannot afford the
    /// 1d6 cost.
    ///
    /// On a legal attempt, spends the die and returns the reduced pool
    /// alongside `true`: a flag for "the rogue is treated as having safely
    /// repositioned this turn" and nothing more, per
    /// [`Rider::CunningStrikeWithdraw`]'s doc comment - there is no
    /// movement or opportunity-attack model here for it to actually do
    /// anything to.
    pub fn resolve_cunning_strike_withdraw(
        &self,
        pool: DamageRider,
    ) -> Option<(DamageRider, bool)> {
        if !matches!(self, Rider::CunningStrikeWithdraw) {
            return None;
        }
        let reduced = pool.spend(1)?;
        Some((reduced, true))
    }

    /// Does this rider downgrade a target's immunity to `kind` (Immune to
    /// Resistant) for damage this creature deals? See
    /// [`super::combatant::Creature::reduction_from`], which is the usual
    /// way this actually gets asked - it scans a whole rider list rather
    /// than one rider at a time.
    pub fn downgrades_damage_immunity(&self, kind: DamageKind) -> bool {
        match self {
            Rider::DowngradeImmunity {
                damage: Some(d), ..
            } => *d == kind,
            _ => false,
        }
    }

    /// Does this rider downgrade a target's immunity to `condition` (an
    /// auto-succeeding save to one rolled with Advantage) for a condition
    /// this creature inflicts? See [`saving_throw_against_condition`].
    pub fn downgrades_condition_immunity(&self, condition: Condition) -> bool {
        match self {
            Rider::DowngradeImmunity {
                condition: Some(c), ..
            } => *c == condition,
            _ => false,
        }
    }

    /// Does inflicting `condition` on a target via a weapon attack arm this
    /// rider's buff? `false` for every rider variant except
    /// [`Rider::ConditionTriggeredWeaponDamage`], and for that variant unless
    /// `condition` is exactly its `trigger`.
    pub fn arms_on_condition(&self, condition: Condition) -> bool {
        matches!(
            self,
            Rider::ConditionTriggeredWeaponDamage { trigger, .. } if *trigger == condition
        )
    }

    /// The [`DamageRider`] this rider contributes to a weapon attack once its
    /// buff has fired, or `None` if this is not
    /// [`Rider::ConditionTriggeredWeaponDamage`] or `armed` is `false`.
    ///
    /// `armed` is per-fight state the caller tracks - see
    /// [`Rider::arms_on_condition`] - the same shape
    /// [`Rider::extra_damage_for`] already takes `used_this_turn` as an
    /// external gate rather than deciding it from data stored on the rider
    /// itself.
    pub fn weapon_damage_if_armed(&self, armed: bool) -> Option<DamageRider> {
        let Rider::ConditionTriggeredWeaponDamage {
            dice_count,
            dice_sides,
            bonus,
            ..
        } = self
        else {
            return None;
        };
        if !armed {
            return None;
        }
        Some(DamageRider::new(*dice_count, *dice_sides).with_bonus(*bonus))
    }

    /// The `(ability, dc, debuffed_ability, duration)` this rider carries, or
    /// `None` if it is not [`Rider::InjuryPoison`].
    pub fn injury_poison(&self) -> Option<(Ability, i32, Ability, Duration)> {
        match self {
            Rider::InjuryPoison {
                ability,
                dc,
                debuffed_ability,
                duration,
            } => Some((*ability, *dc, *debuffed_ability, *duration)),
            _ => None,
        }
    }
}

/// Roll a saving throw `target` makes against `condition`, which `attacker`
/// is trying to inflict.
///
/// A target with no immunity to `condition` rolls exactly like any other
/// saving throw: one d20 plus its own bonus against `dc`. A target that
/// *is* immune to `condition` (see
/// [`super::combatant::Creature::immune_to_condition`]) is ordinarily
/// unaffected outright, no roll at all, and this returns `true`
/// unconditionally, unless `attacker` carries a [`Rider::DowngradeImmunity`]
/// naming this exact `condition`. Then the free pass is gone: the target
/// still has to make the save, just with Advantage instead of
/// auto-succeeding.
pub fn saving_throw_against_condition(
    rng: &mut Rng,
    target: &Creature,
    attacker: &Creature,
    ability: Ability,
    dc: i32,
    condition: Condition,
) -> bool {
    let bonus = target.save(ability);
    if target.immune_to_condition(condition) {
        if !attacker
            .riders
            .iter()
            .any(|r| r.downgrades_condition_immunity(condition))
        {
            return true;
        }
        return RollMode::Advantage.roll(rng) + bonus >= dc;
    }
    RollMode::Normal.roll(rng) + bonus >= dc
}

/// The exact counterpart of [`saving_throw_against_condition`]: `target`'s
/// probability of succeeding, closed-form rather than sampled - the same
/// "everything twice" split every other rule in this crate is held to.
pub fn save_success_probability(
    target: &Creature,
    attacker: &Creature,
    ability: Ability,
    dc: i32,
    condition: Condition,
) -> f64 {
    let bonus = target.save(ability);
    if target.immune_to_condition(condition) {
        if !attacker
            .riders
            .iter()
            .any(|r| r.downgrades_condition_immunity(condition))
        {
            return 1.0;
        }
        return probability_at_least(RollMode::Advantage, dc - bonus);
    }
    probability_at_least(RollMode::Normal, dc - bonus)
}

/// P(a roll under `mode` is at least `needed`), read off
/// [`RollMode::distribution`] rather than rederived.
fn probability_at_least(mode: RollMode, needed: i32) -> f64 {
    mode.distribution()
        .iter()
        .enumerate()
        .map(|(i, &p)| if i as i32 + 1 >= needed { p } else { 0.0 })
        .sum()
}

/// A saving throw rolled under `mode` rather than a flat d20 - the same
/// [`RollMode`] an attack roll already rolls under
/// ([`crate::rules::combat::sample_hit_with`]), generalised to saves. 5e's
/// saving throws have no natural-1/natural-20 override the way attack rolls
/// do, so unlike an attack roll this is exactly `mode.roll(rng) + bonus >=
/// dc` with no exception carved out.
///
/// The generic mechanism [`Rider::InjuryPoison`]'s debuff half uses once it
/// has taken hold: a target under it rolls its burdened ability's saves with
/// [`RollMode::Disadvantage`] through this same function rather than a
/// special case.
pub fn save_with_mode(rng: &mut Rng, mode: RollMode, bonus: i32, dc: i32) -> bool {
    mode.roll(rng) + bonus >= dc
}

/// The exact counterpart of [`save_with_mode`], read off
/// [`RollMode::distribution`] like [`probability_at_least`] already is for
/// [`save_success_probability`].
pub fn save_probability_with_mode(mode: RollMode, bonus: i32, dc: i32) -> f64 {
    probability_at_least(mode, dc - bonus)
}

/// Resolve an [`Rider::InjuryPoison`] coating's forcing save - the hit that
/// uses the coating up: `target` rolls its `ability` save at
/// [`RollMode::Normal`] against `dc`, via [`save_with_mode`]. `None` if
/// `rider` is not [`Rider::InjuryPoison`] at all.
///
/// Only the forcing save is resolved here. Applying the resulting
/// disadvantage-on-saves debuff for its stated `duration` is bookkeeping for
/// whoever tracks conditions and effects over time to do with the `false`
/// this returns on a failure - the same division [`Rider::SaveOrCondition`]'s
/// own `duration` field already leaves to `sim::duel` rather than resolving
/// itself.
pub fn injury_poison_forcing_save(rng: &mut Rng, target: &Creature, rider: &Rider) -> Option<bool> {
    let (ability, dc, ..) = rider.injury_poison()?;
    Some(save_with_mode(
        rng,
        RollMode::Normal,
        target.save(ability),
        dc,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prob::rng::Rng;
    use crate::rules::combat::{damage_pmf, sample_damage, Defense, Reduction};
    use crate::rules::creature::Creature;

    fn sneak_attack() -> Rider {
        Rider::ConditionalExtraDamage {
            dice_count: 4,
            dice_sides: 6,
            once_per_turn: true,
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-12
    }

    fn slaying_rider() -> Rider {
        Rider::BonusDamageVsCreatureType {
            dice_count: 3,
            dice_sides: 6,
            bonus: 0,
            damage_kind: DamageKind::Piercing,
            creature_type: CreatureType::Dragon,
        }
    }

    #[test]
    fn downgrade_immunity_rider_reports_only_what_it_names() {
        let both = Rider::DowngradeImmunity {
            damage: Some(DamageKind::Poison),
            condition: Some(Condition::Poisoned),
        };
        assert!(both.downgrades_damage_immunity(DamageKind::Poison));
        assert!(!both.downgrades_damage_immunity(DamageKind::Fire));
        assert!(both.downgrades_condition_immunity(Condition::Poisoned));
        assert!(!both.downgrades_condition_immunity(Condition::Stunned));

        let damage_only = Rider::DowngradeImmunity {
            damage: Some(DamageKind::Fire),
            condition: None,
        };
        assert!(damage_only.downgrades_damage_immunity(DamageKind::Fire));
        assert!(!damage_only.downgrades_condition_immunity(Condition::Poisoned));

        let condition_only = Rider::DowngradeImmunity {
            damage: None,
            condition: Some(Condition::Stunned),
        };
        assert!(!condition_only.downgrades_damage_immunity(DamageKind::Fire));
        assert!(condition_only.downgrades_condition_immunity(Condition::Stunned));

        // An unrelated rider never answers yes to either question - the same
        // "not mistaken for a different marker" check `cunning_strike_dc`'s
        // own test makes.
        let unrelated = Rider::AlwaysSucceed { uses: 3 };
        assert!(!unrelated.downgrades_damage_immunity(DamageKind::Poison));
        assert!(!unrelated.downgrades_condition_immunity(Condition::Poisoned));
    }

    /// The damage half: a target normally immune to a damage type instead
    /// takes half damage from an attacker carrying the downgrade trait, and
    /// full immunity is unaffected for every other attacker against that
    /// same target - the attacker-scoping the rider promises.
    #[test]
    fn damage_downgrade_softens_immunity_to_resistance_for_this_attacker_only() {
        let mut target = Creature::new("golem", 15, 60);
        target
            .reductions
            .push((DamageKind::Poison, Reduction::Immune));

        let plain_attacker = Creature::new("fighter", 15, 40);
        let downgrading_attacker =
            Creature::new("rogue", 15, 40).with_rider(Rider::DowngradeImmunity {
                damage: Some(DamageKind::Poison),
                condition: None,
            });

        assert_eq!(
            target.reduction_from(DamageKind::Poison, &plain_attacker),
            Reduction::Immune,
            "an attacker without the trait still sees full immunity"
        );
        assert_eq!(
            target.reduction_from(DamageKind::Poison, &downgrading_attacker),
            Reduction::Resistant,
            "the trait-carrying attacker softens immune to resistant"
        );
        // Scoped to the trait, not the damage type in general: this same
        // attacker sees an unrelated damage type's immunity untouched.
        target
            .reductions
            .push((DamageKind::Cold, Reduction::Immune));
        assert_eq!(
            target.reduction_from(DamageKind::Cold, &downgrading_attacker),
            Reduction::Immune,
            "the trait only names poison, so cold immunity is untouched"
        );
    }

    /// The exact-vs-sampled agreement every rule in this crate is held to,
    /// applied to the reduction the downgrade trait actually produces: fold
    /// `reduction_from`'s result into the same `Attack`/`Defense` machinery
    /// [`cunning_strike_spends_from_the_same_pool_sneak_attack_would_roll`]
    /// already uses, so the mechanism is checked at the same level a
    /// standalone rider mechanism always is here, not merely asserted.
    #[test]
    fn damage_downgrade_reduction_agrees_with_the_exact_path() {
        let mut target = Creature::new("golem", 12, 60);
        target
            .reductions
            .push((DamageKind::Poison, Reduction::Immune));
        let downgrading_attacker =
            Creature::new("rogue", 15, 40).with_rider(Rider::DowngradeImmunity {
                damage: Some(DamageKind::Poison),
                condition: None,
            });
        let plain_attacker = Creature::new("fighter", 15, 40);

        let cases = [
            (
                "downgraded to resistant: half damage gets through",
                target.reduction_from(DamageKind::Poison, &downgrading_attacker),
            ),
            (
                "no trait: still fully immune",
                target.reduction_from(DamageKind::Poison, &plain_attacker),
            ),
        ];
        let mut means = Vec::new();
        for (seed, (name, reduction)) in cases.into_iter().enumerate() {
            let attack = Attack::new(6, 2, 8, 4);
            let defense = Defense::new(target.ac, target.hp).with_reduction(reduction);
            let exact = damage_pmf(&attack, &defense);
            let (lo, hi) = (exact.min(), exact.max());
            let mut rng = Rng::new(seed as u64 + 3200);
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
            means.push((name, exact.mean()));
        }
        // And the two cases actually differ: proof the downgrade is doing
        // something rather than both silently computing zero.
        assert!(means[0].1 > 0.0, "{}", means[0].0);
        assert_eq!(means[1].1, 0.0, "{}", means[1].0);
    }

    /// The condition half: a target normally immune to a condition is
    /// ordinarily unaffected outright (no roll, always saved) - but an
    /// attacker carrying the downgrade trait takes that free pass away, and
    /// the target instead rolls with Advantage rather than auto-succeeding.
    #[test]
    fn condition_downgrade_rolls_with_advantage_instead_of_auto_succeeding() {
        let mut immune_target = Creature::new("golem", 12, 60);
        immune_target.condition_immunities.push(Condition::Poisoned);
        // A save bonus of 0 against a DC of 15 needs a 15+ on the die - low
        // enough that Advantage clearly is not the same as auto-succeeding.
        let dc = 15;

        let plain_attacker = Creature::new("fighter", 15, 40);
        let downgrading_attacker =
            Creature::new("rogue", 15, 40).with_rider(Rider::DowngradeImmunity {
                damage: None,
                condition: Some(Condition::Poisoned),
            });

        // Without the trait: unaffected outright, deterministically, however
        // the dice would have landed.
        assert_eq!(
            save_success_probability(
                &immune_target,
                &plain_attacker,
                Ability::Con,
                dc,
                Condition::Poisoned
            ),
            1.0
        );
        let mut rng = Rng::new(4100);
        for _ in 0..1000 {
            assert!(saving_throw_against_condition(
                &mut rng,
                &immune_target,
                &plain_attacker,
                Ability::Con,
                dc,
                Condition::Poisoned
            ));
        }

        // With the trait: the free pass is gone. The save is now rolled with
        // Advantage - strictly better than a flat d20, but not certain -
        // which is the whole difference between "downgraded" and "immune".
        let flat_p = probability_at_least(RollMode::Normal, dc);
        let advantage_p = save_success_probability(
            &immune_target,
            &downgrading_attacker,
            Ability::Con,
            dc,
            Condition::Poisoned,
        );
        assert!(advantage_p > flat_p, "Advantage must beat a flat roll");
        assert!(advantage_p < 1.0, "still not a free pass");

        // Exact-vs-sampled agreement, the same Bernoulli-proportion check
        // every probability in this crate is held to.
        let mut rng = Rng::new(4200);
        let n = 200_000;
        let successes = (0..n)
            .filter(|_| {
                saving_throw_against_condition(
                    &mut rng,
                    &immune_target,
                    &downgrading_attacker,
                    Ability::Con,
                    dc,
                    Condition::Poisoned,
                )
            })
            .count();
        let got = successes as f64 / f64::from(n);
        let tol = 5.0 * (advantage_p * (1.0 - advantage_p) / f64::from(n)).sqrt() + 1e-4;
        assert!(
            (got - advantage_p).abs() < tol,
            "sampled {got:.5}, exact {advantage_p:.5}, tol {tol:.5}"
        );

        // A target with no relevant immunity is untouched by any of this,
        // trait or no trait: same probability either way.
        let mut mundane_target = Creature::new("bandit", 12, 20);
        mundane_target.saves[Ability::Con.index()] = 0;
        let with_trait = save_success_probability(
            &mundane_target,
            &downgrading_attacker,
            Ability::Con,
            dc,
            Condition::Poisoned,
        );
        let without_trait = save_success_probability(
            &mundane_target,
            &plain_attacker,
            Ability::Con,
            dc,
            Condition::Poisoned,
        );
        assert_eq!(with_trait, without_trait);
        assert_eq!(with_trait, flat_p);
    }

    #[test]
    fn cunning_strike_dc_reads_off_the_marker_rider_and_nothing_else() {
        assert_eq!(
            Rider::CunningStrike { dc: 15 }.cunning_strike_dc(),
            Some(15)
        );
        assert_eq!(sneak_attack().cunning_strike_dc(), None);
        assert_eq!(
            Rider::AlwaysSucceed { uses: 3 }.cunning_strike_dc(),
            None,
            "an unrelated rider is not mistaken for the Cunning Strike marker"
        );

        // The usual call shape: scan a creature's rider list for the one
        // that carries the DC.
        let riders = [sneak_attack(), Rider::CunningStrike { dc: 15 }];
        assert_eq!(riders.iter().find_map(Rider::cunning_strike_dc), Some(15));
    }

    /// The framework end to end: a qualifying Sneak Attack's full pool is
    /// inspectable, part of it can be spent on a stand-in "does nothing"
    /// Cunning Strike option (or two, combined) at the DC the marker rider
    /// carries, and the dice that do get rolled for damage are exactly the
    /// dice that were not spent - not the full pool with a discount applied
    /// after the fact.
    #[test]
    fn cunning_strike_spends_from_the_same_pool_sneak_attack_would_roll() {
        let attack = Attack::new(7, 1, 4, 3)
            .with_mode(RollMode::Advantage)
            .with_finesse_or_ranged(true);

        let full = sneak_attack()
            .extra_damage_for(&attack, false)
            .expect("qualifies for Sneak Attack");
        assert_eq!(
            full,
            DamageRider::new(4, 6),
            "the full pool before any spend"
        );

        let dc = Rider::CunningStrike { dc: 15 }
            .cunning_strike_dc()
            .expect("Cunning Strike is unlocked");

        // Spend 1d6 on a trivial "costs 1d6, does nothing" test option -
        // proving the framework needs no real effect plugin to exercise it -
        // then spend a second 1d6 on a stand-in for a different option, the
        // same way two later plugins (Poison, Trip/Withdraw) would each
        // spend their own share of one hit's pool.
        let after_first_option = full.spend(1).expect("4 dice can afford 1");
        assert_eq!(after_first_option.dice_count, 3);
        let after_second_option = after_first_option
            .spend(1)
            .expect("3 dice can afford 1 more");
        assert_eq!(
            after_second_option.dice_count, 2,
            "two combined 1d6 spends leave 2 of the original 4"
        );
        assert_eq!(dc, 15, "both stand-in options are funded at the same DC");

        // A third spend of everything remaining is still within budget...
        assert!(after_second_option.spend(2).is_some());
        // ...but spending even one more die than is left is refused, not
        // silently clamped.
        assert_eq!(after_second_option.spend(3), None);

        // What actually gets rolled for damage is the *reduced* rider: two
        // fewer dice actually thrown, not four dice thrown and two ignored.
        let defense = Defense::new(10, 40);
        let reduced_attack = attack.clone().with_damage_rider(after_second_option);
        let exact = damage_pmf(&reduced_attack, &defense);
        let full_attack = attack.with_damage_rider(full);
        let exact_full = damage_pmf(&full_attack, &defense);
        assert!(
            exact.mean() < exact_full.mean(),
            "spending dice away must lower expected damage, not just relabel it"
        );
        // The reduced pool's maximum is exactly the base 1d4+3 plus 2d6 (spent
        // dice cannot show up in the damage distribution at all), the dice
        // doubled on a crit like any other rider and the flat bonus not
        // doubled at all.
        assert_eq!(exact.max(), 2 * 4 + 2 * 2 * 6 + 3);
    }

    /// The sampled path must actually draw fewer dice, not merely report a
    /// smaller number - the same exact-vs-sampled agreement check every
    /// other rider in this codebase is held to.
    #[test]
    fn a_cunning_strike_reduced_pool_agrees_with_the_exact_path() {
        let defense = Defense::new(12, 60);
        let attack = Attack::new(6, 1, 8, 4)
            .with_mode(RollMode::Advantage)
            .with_finesse_or_ranged(true);
        let full = sneak_attack().extra_damage_for(&attack, false).unwrap();

        let cases = [
            ("full pool, nothing spent for Cunning Strike", full),
            ("spend 1d6 on one stand-in option", full.spend(1).unwrap()),
            (
                "spend 1d6 twice on two stand-in options, combined",
                full.spend(1).unwrap().spend(1).unwrap(),
            ),
        ];
        for (seed, (name, rider)) in cases.into_iter().enumerate() {
            let attack = attack.clone().with_damage_rider(rider);
            let exact = damage_pmf(&attack, &defense);
            let (lo, hi) = (exact.min(), exact.max());
            let mut rng = Rng::new(seed as u64 + 1500);
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

    #[test]
    fn applies_only_against_a_matching_creature_type() {
        let got = slaying_rider().bonus_damage_vs_creature_type(Some(CreatureType::Dragon));
        assert_eq!(got, Some(DamageRider::new(3, 6)));
    }

    #[test]
    fn does_not_apply_against_a_different_creature_type() {
        assert_eq!(
            slaying_rider().bonus_damage_vs_creature_type(Some(CreatureType::Giant)),
            None
        );
    }

    #[test]
    fn does_not_apply_against_a_target_with_no_declared_type() {
        assert_eq!(slaying_rider().bonus_damage_vs_creature_type(None), None);
    }

    #[test]
    fn a_non_matching_rider_variant_never_contributes_this_gate() {
        let sneak = Rider::ConditionalExtraDamage {
            dice_count: 4,
            dice_sides: 6,
            once_per_turn: true,
        };
        assert_eq!(
            sneak.bonus_damage_vs_creature_type(Some(CreatureType::Dragon)),
            None
        );
    }

    /// The acceptance criterion this task turns on: a qualifying hit stacks
    /// this rider's dice with Sneak Attack's rather than either replacing the
    /// other, because both riders contribute separately to the same
    /// `Attack::damage_riders` list.
    #[test]
    fn stacks_with_sneak_attack_on_the_same_qualifying_hit() {
        let sneak = Rider::ConditionalExtraDamage {
            dice_count: 4,
            dice_sides: 6,
            once_per_turn: true,
        };
        let attack = Attack::new(7, 1, 4, 3)
            .with_mode(RollMode::Advantage)
            .with_finesse_or_ranged(true);

        let sneak_extra = sneak
            .extra_damage_for(&attack, false)
            .expect("sneak attack qualifies");
        let slaying_extra = slaying_rider()
            .bonus_damage_vs_creature_type(Some(CreatureType::Dragon))
            .expect("the target is a dragon");

        let neither = attack.clone();
        let both = attack
            .clone()
            .with_damage_rider(sneak_extra)
            .with_damage_rider(slaying_extra);

        let defense = Defense::new(1, 200); // AC 1: every non-fumble roll hits
        let mean_neither = damage_pmf(&neither, &defense).mean();
        let mean_both = damage_pmf(&both, &defense).mean();
        assert!(
            mean_both > mean_neither,
            "both riders' dice should raise the mean damage over neither firing"
        );

        // Two separate d6 riders (4d6 and 3d6) convolve to exactly the same
        // distribution as one 7d6 rider - dice of the same size are additive
        // under convolution, doubling on a crit included - so comparing
        // against that single merged rider is an exact check that both
        // riders' dice are present and neither replaced the other.
        let merged_reference = attack.with_damage_rider(DamageRider::new(4 + 3, 6));
        let pmf_both = damage_pmf(&both, &defense);
        let pmf_merged = damage_pmf(&merged_reference, &defense);
        assert!(close(pmf_both.mean(), pmf_merged.mean()));
        assert_eq!(pmf_both.min(), pmf_merged.min());
        assert_eq!(pmf_both.max(), pmf_merged.max());

        // And the max damage is the base plus every die maxed, doubled on a
        // crit exactly like the base pool - `rider_pmf` handles both riders
        // identically, so there is nothing special about there being two.
        let base_crit_max = 2 * 4; // 1d4 base, doubled on a crit
        let riders_crit_max = 2 * (4 * 6) + 2 * (3 * 6);
        assert_eq!(
            pmf_both.max(),
            base_crit_max + riders_crit_max + 3 /* damage_bonus */
        );
    }

    #[test]
    fn sampled_bonus_vs_creature_type_agrees_with_the_exact_path_alone_and_stacked() {
        let defense = Defense::new(14, 80);
        let sneak = Rider::ConditionalExtraDamage {
            dice_count: 4,
            dice_sides: 6,
            once_per_turn: true,
        };
        let attack = Attack::new(6, 1, 8, 4)
            .with_mode(RollMode::Advantage)
            .with_finesse_or_ranged(true);

        let slaying_extra = slaying_rider()
            .bonus_damage_vs_creature_type(Some(CreatureType::Dragon))
            .expect("the target is a dragon");
        let sneak_extra = sneak
            .extra_damage_for(&attack, false)
            .expect("sneak attack qualifies");

        let cases = [
            (
                "vs-creature-type rider alone",
                attack.clone().with_damage_rider(slaying_extra),
            ),
            (
                "stacked with sneak attack",
                attack
                    .clone()
                    .with_damage_rider(slaying_extra)
                    .with_damage_rider(sneak_extra),
            ),
        ];

        for (seed, (name, attack)) in cases.into_iter().enumerate() {
            let exact = damage_pmf(&attack, &defense);
            let (lo, hi) = (exact.min(), exact.max());
            let mut rng = Rng::new(seed as u64 + 1300);
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

    // --- Cunning Strike: Trip & Withdraw -----------------------------

    fn trip_pool() -> DamageRider {
        // Stand-in for a qualifying Sneak Attack's pool: 4d6, same as the
        // rest of this file's Cunning Strike tests.
        DamageRider::new(4, 6)
    }

    #[test]
    fn trip_is_refused_against_a_target_bigger_than_large() {
        let mut rng = Rng::new(1);
        for size in [Size::Huge, Size::Gargantuan] {
            assert_eq!(
                Rider::CunningStrikeTrip.resolve_cunning_strike_trip(
                    trip_pool(),
                    size,
                    100, // even a save bonus this high must not matter
                    10,
                    &mut rng,
                ),
                None,
                "{size:?} is too big to trip; the die must not be spent either"
            );
        }
    }

    #[test]
    fn trip_is_allowed_against_large_and_everything_smaller() {
        let mut rng = Rng::new(2);
        for size in [Size::Tiny, Size::Small, Size::Medium, Size::Large] {
            let (reduced, _) = Rider::CunningStrikeTrip
                .resolve_cunning_strike_trip(trip_pool(), size, 0, 10, &mut rng)
                .unwrap_or_else(|| panic!("{size:?} should be a legal Trip target"));
            assert_eq!(reduced.dice_count, 3, "1d6 spent on the attempt");
        }
    }

    #[test]
    fn trip_spends_1d6_from_the_sneak_attack_pool() {
        let mut rng = Rng::new(3);
        let (reduced, _) = Rider::CunningStrikeTrip
            .resolve_cunning_strike_trip(trip_pool(), Size::Medium, 0, 10, &mut rng)
            .expect("a Medium target can be tripped");
        assert_eq!(reduced, DamageRider::new(3, 6));
    }

    #[test]
    fn trip_knocks_a_failed_save_prone_and_leaves_a_successful_one_standing() {
        let mut rng = Rng::new(4);
        // A save bonus far below any roll a d20 can produce always fails.
        let (_, failed) = Rider::CunningStrikeTrip
            .resolve_cunning_strike_trip(trip_pool(), Size::Medium, -100, 10, &mut rng)
            .expect("legal attempt");
        assert_eq!(failed, Some(Condition::Prone));

        // A save bonus far above any DC always succeeds.
        let (_, succeeded) = Rider::CunningStrikeTrip
            .resolve_cunning_strike_trip(trip_pool(), Size::Medium, 100, 10, &mut rng)
            .expect("legal attempt");
        assert_eq!(succeeded, None);
    }

    #[test]
    fn trip_refuses_to_overdraw_the_pool() {
        let mut rng = Rng::new(5);
        let empty = DamageRider::new(0, 6);
        assert_eq!(
            Rider::CunningStrikeTrip.resolve_cunning_strike_trip(
                empty,
                Size::Medium,
                0,
                10,
                &mut rng
            ),
            None,
            "no dice left to spend on the attempt"
        );
    }

    #[test]
    fn trip_resolution_is_gated_on_the_matching_rider_variant() {
        let mut rng = Rng::new(6);
        assert_eq!(
            Rider::CunningStrikeWithdraw.resolve_cunning_strike_trip(
                trip_pool(),
                Size::Medium,
                0,
                10,
                &mut rng
            ),
            None,
            "an unrelated rider is not mistaken for the Trip marker"
        );
    }

    #[test]
    fn withdraw_spends_1d6_and_flags_a_safe_reposition() {
        let pool = DamageRider::new(4, 6);
        let (reduced, repositioned) = Rider::CunningStrikeWithdraw
            .resolve_cunning_strike_withdraw(pool)
            .expect("4 dice can afford the 1d6 cost");
        assert_eq!(reduced, DamageRider::new(3, 6));
        assert!(repositioned, "a legal Withdraw always repositions safely");
    }

    #[test]
    fn withdraw_refuses_to_overdraw_the_pool() {
        let empty = DamageRider::new(0, 6);
        assert_eq!(
            Rider::CunningStrikeWithdraw.resolve_cunning_strike_withdraw(empty),
            None
        );
    }

    #[test]
    fn withdraw_resolution_is_gated_on_the_matching_rider_variant() {
        assert_eq!(
            Rider::CunningStrikeTrip.resolve_cunning_strike_withdraw(trip_pool()),
            None,
            "an unrelated rider is not mistaken for the Withdraw marker"
        );
    }

    fn rider() -> Rider {
        Rider::ConditionalExtraDamage {
            dice_count: 4,
            dice_sides: 6,
            once_per_turn: true,
        }
    }

    /// A weapon attack triggers through the extended gate exactly like it
    /// does through the plain one - the extension flag never widens or
    /// narrows the weapon path, regardless of which way it is set.
    #[test]
    fn a_weapon_attack_is_unaffected_by_the_spell_attack_extension_either_way() {
        let attack = Attack::new(7, 1, 4, 3)
            .with_mode(RollMode::Advantage)
            .with_finesse_or_ranged(true);
        assert_eq!(
            rider().extra_damage_for_with_spell_attack_extension(&attack, false, false),
            Some(DamageRider::new(4, 6))
        );
        assert_eq!(
            rider().extra_damage_for_with_spell_attack_extension(&attack, false, true),
            Some(DamageRider::new(4, 6)),
            "the extension is additive, never a restriction on the weapon path"
        );
    }

    /// A spell attack qualifies only when the creature carries the
    /// extension - the "some builds have this, most don't" gate the marker
    /// rider exists for.
    #[test]
    fn a_spell_attack_triggers_only_with_the_extension() {
        let attack = Attack::new(7, 1, 4, 3)
            .with_mode(RollMode::Advantage)
            .with_is_spell_attack(true);
        assert_eq!(
            rider().extra_damage_for_with_spell_attack_extension(&attack, false, true),
            Some(DamageRider::new(4, 6)),
            "a spell attack should qualify once the extension is present"
        );
        assert_eq!(
            rider().extra_damage_for_with_spell_attack_extension(&attack, false, false),
            None,
            "a spell attack must not qualify without the extension"
        );
    }

    /// The plain two-argument [`Rider::extra_damage_for`] never grants the
    /// extension - a spell attack never triggers through it, which is
    /// exactly the "no argument means `false`" contract
    /// [`Rider::extra_damage_for_with_spell_attack_extension`]'s doc comment
    /// promises.
    #[test]
    fn the_plain_gate_never_extends_to_spell_attacks() {
        let attack = Attack::new(7, 1, 4, 3)
            .with_mode(RollMode::Advantage)
            .with_is_spell_attack(true);
        assert_eq!(rider().extra_damage_for(&attack, false), None);
    }

    /// Disadvantage still overrides everything, spell attack or not.
    #[test]
    fn disadvantage_still_overrides_a_spell_attack_with_the_extension() {
        let attack = Attack::new(7, 1, 4, 3)
            .with_mode(RollMode::Disadvantage)
            .with_is_spell_attack(true)
            .with_ally_adjacent(true);
        assert_eq!(
            rider().extra_damage_for_with_spell_attack_extension(&attack, false, true),
            None
        );
    }

    /// A spell attack that declares its own damage type pins the returned
    /// rider's [`DamageRider::kind`] to match it - the 5e rule this
    /// mechanism exists for: this kind of extra damage takes on the
    /// triggering *spell's* damage type, not a fixed default.
    #[test]
    fn a_spell_attacks_declared_kind_overrides_the_riders_damage_type() {
        let attack = Attack::new(7, 1, 4, 3)
            .with_mode(RollMode::Advantage)
            .with_is_spell_attack(true)
            .with_spell_damage_kind(DamageKind::Radiant);
        assert_eq!(
            rider().extra_damage_for_with_spell_attack_extension(&attack, false, true),
            Some(DamageRider::new(4, 6).with_kind(DamageKind::Radiant)),
            "the rider's damage type should match the triggering spell's"
        );
    }

    /// A weapon attack never declares a `spell_damage_kind` - the field is
    /// only ever set on a spell attack - so the rider it triggers keeps
    /// `kind: None`: reduced exactly like the rest of the hit, unaffected by
    /// this mechanism, same as before it existed.
    #[test]
    fn a_weapon_attack_never_gets_a_damage_type_override() {
        let attack = Attack::new(7, 1, 4, 3)
            .with_mode(RollMode::Advantage)
            .with_finesse_or_ranged(true);
        let extra = rider()
            .extra_damage_for_with_spell_attack_extension(&attack, false, false)
            .expect("a finesse-or-ranged weapon attack at advantage qualifies");
        assert_eq!(
            extra.kind, None,
            "a weapon-triggered rider's damage type is unaffected by this mechanism"
        );
    }

    /// A spell attack that never declares a damage type is treated the same
    /// as a weapon attack: the rider's `kind` stays `None` rather than
    /// defaulting to something invented.
    #[test]
    fn a_spell_attack_with_no_declared_kind_leaves_the_riders_type_unaffected() {
        let attack = Attack::new(7, 1, 4, 3)
            .with_mode(RollMode::Advantage)
            .with_is_spell_attack(true);
        let extra = rider()
            .extra_damage_for_with_spell_attack_extension(&attack, false, true)
            .expect("qualifies via the extension");
        assert_eq!(extra.kind, None);
    }

    /// The acceptance case for this whole mechanism: the SAME rider,
    /// triggered off two attacks that differ only in which spell cast them,
    /// deals different net damage against a target that resists one of
    /// those spells' damage types but not the other. If this only changed a
    /// label on the rider and never reached damage reduction, `radiant` and
    /// `force` below would come out equal.
    #[test]
    fn the_same_rider_deals_different_net_damage_depending_on_the_triggering_spells_type() {
        let defense =
            Defense::new(1, 60).with_kind_reduction(DamageKind::Radiant, Reduction::Resistant);

        let radiant_spell = Attack::new(7, 1, 4, 0)
            .with_mode(RollMode::Advantage)
            .with_is_spell_attack(true)
            .with_spell_damage_kind(DamageKind::Radiant);
        let force_spell = Attack::new(7, 1, 4, 0)
            .with_mode(RollMode::Advantage)
            .with_is_spell_attack(true)
            .with_spell_damage_kind(DamageKind::Force);

        let radiant_extra = rider()
            .extra_damage_for_with_spell_attack_extension(&radiant_spell, false, true)
            .expect("qualifies");
        let force_extra = rider()
            .extra_damage_for_with_spell_attack_extension(&force_spell, false, true)
            .expect("qualifies");
        // It is genuinely the same rider - only the declared type differs.
        assert_eq!(radiant_extra.dice_count, force_extra.dice_count);
        assert_eq!(radiant_extra.dice_sides, force_extra.dice_sides);
        assert_eq!(radiant_extra.kind, Some(DamageKind::Radiant));
        assert_eq!(force_extra.kind, Some(DamageKind::Force));

        // Isolate the rider's own contribution: a base attack pool of zero
        // dice, so the whole PMF is exactly what the rider deals.
        let carrier = |extra: DamageRider| {
            Attack::new(7, 0, 4, 0)
                .with_mode(RollMode::Advantage)
                .with_damage_rider(extra)
        };
        let radiant_pmf = damage_pmf(&carrier(radiant_extra), &defense);
        let force_pmf = damage_pmf(&carrier(force_extra), &defense);

        assert!(
            radiant_pmf.mean() > 0.0,
            "resistance still lets some through"
        );
        assert!(
            radiant_pmf.mean() < force_pmf.mean(),
            "the Radiant-triggered rider should deal less net damage than the \
             Force-triggered one, against a target resistant only to Radiant"
        );

        // And Force is genuinely untouched by a Radiant-only resistance -
        // exactly what an identical rider would deal with no resistance
        // in play at all, not merely "less reduced than Radiant".
        let no_resistance = Defense::new(1, 60);
        let force_pmf_unresisted = damage_pmf(&carrier(force_extra), &no_resistance);
        assert!(close(force_pmf.mean(), force_pmf_unresisted.mean()));
    }

    /// The sampled path must agree with the exact one for this mechanism
    /// end to end: a spell's declared damage type flowing from
    /// [`Attack::spell_damage_kind`] through the rider into
    /// [`Defense::reduction_for`], for both the resisted and the unresisted
    /// spell type from the scenario above.
    #[test]
    fn sampled_damage_agrees_with_the_exact_path_for_a_spell_inherited_damage_type() {
        let defense =
            Defense::new(14, 60).with_kind_reduction(DamageKind::Radiant, Reduction::Resistant);
        let cases = [
            (
                "resisted: the spell's declared type is Radiant",
                DamageKind::Radiant,
            ),
            (
                "unresisted: the spell's declared type is Force",
                DamageKind::Force,
            ),
        ];
        for (seed, (name, kind)) in cases.into_iter().enumerate() {
            let attack = Attack::new(6, 1, 8, 4)
                .with_mode(RollMode::Advantage)
                .with_is_spell_attack(true)
                .with_spell_damage_kind(kind);
            let extra = rider()
                .extra_damage_for_with_spell_attack_extension(&attack, false, true)
                .expect("qualifies");
            let attack = attack.with_damage_rider(extra);

            let exact = damage_pmf(&attack, &defense);
            let (lo, hi) = (exact.min(), exact.max());
            let mut rng = Rng::new(seed as u64 + 1_700);
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

    /// [`Creature::extra_damage_applies_to_spell_attacks`] reads the marker
    /// straight off the creature's own rider list, the same way
    /// [`Creature::has_evasion`] reads [`Rider::NothingOnSuccess`].
    #[test]
    fn a_creature_reports_the_extension_only_when_it_carries_the_marker_rider() {
        let plain = Creature::new("Plain Rogue", 15, 40).with_rider(rider());
        assert!(!plain.extra_damage_applies_to_spell_attacks());

        let extended = Creature::new("Extended Build", 15, 40)
            .with_rider(rider())
            .with_rider(Rider::ExtraDamageAppliesToSpellAttacks);
        assert!(extended.extra_damage_applies_to_spell_attacks());
    }

    /// The exact and sampled paths agree across the weapon-attack, extended
    /// spell-attack, and unextended spell-attack cases - the same kind of
    /// trigger matrix `dsl::plugin::rogue`'s sneak attack test covers for
    /// the plain weapon-only gate, extended here to the new spell-attack
    /// cases.
    #[test]
    fn sampled_extra_damage_agrees_with_the_exact_path_across_spell_attack_cases() {
        let defense = Defense::new(14, 60);
        let cases = [
            (
                "weapon attack, advantage: triggers regardless of the extension",
                Attack::new(6, 1, 8, 4)
                    .with_mode(RollMode::Advantage)
                    .with_finesse_or_ranged(true),
                false,
            ),
            (
                "spell attack, advantage, with the extension: triggers",
                Attack::new(6, 1, 8, 4)
                    .with_mode(RollMode::Advantage)
                    .with_is_spell_attack(true),
                true,
            ),
            (
                "spell attack, advantage, without the extension: does not trigger",
                Attack::new(6, 1, 8, 4)
                    .with_mode(RollMode::Advantage)
                    .with_is_spell_attack(true),
                false,
            ),
        ];
        for (seed, (name, attack, spell_attacks_extended)) in cases.into_iter().enumerate() {
            let attack = match rider().extra_damage_for_with_spell_attack_extension(
                &attack,
                false,
                spell_attacks_extended,
            ) {
                Some(extra) => attack.with_damage_rider(extra),
                None => attack,
            };
            let exact = damage_pmf(&attack, &defense);
            let (lo, hi) = (exact.min(), exact.max());
            let mut rng = Rng::new(seed as u64 + 1_300);
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
        // The weapon path and the extended spell-attack path should agree
        // exactly once both qualify: the gate is an OR over roll type, not
        // a different amount of damage for one kind or the other.
        let weapon = damage_pmf(
            &Attack::new(6, 1, 8, 4)
                .with_mode(RollMode::Advantage)
                .with_finesse_or_ranged(true)
                .with_damage_rider(DamageRider::new(4, 6)),
            &defense,
        );
        let spell = damage_pmf(
            &Attack::new(6, 1, 8, 4)
                .with_mode(RollMode::Advantage)
                .with_is_spell_attack(true)
                .with_damage_rider(DamageRider::new(4, 6)),
            &defense,
        );
        assert!(close(weapon.mean(), spell.mean()));
    }

    // --- ITM-06: a condition-triggered weapon damage buff ------------------

    fn poisoner_buff() -> Rider {
        Rider::ConditionTriggeredWeaponDamage {
            trigger: Condition::Poisoned,
            dice_count: 2,
            dice_sides: 6,
            bonus: 0,
            damage_kind: DamageKind::Poison,
        }
    }

    #[test]
    fn arms_only_on_its_own_trigger_condition() {
        assert!(poisoner_buff().arms_on_condition(Condition::Poisoned));
        assert!(!poisoner_buff().arms_on_condition(Condition::Stunned));
        // An unrelated rider never answers yes either, the same "not
        // mistaken for a different marker" check every other rider accessor
        // in this module is held to.
        assert!(!Rider::AlwaysSucceed { uses: 3 }.arms_on_condition(Condition::Poisoned));
    }

    #[test]
    fn contributes_no_damage_rider_until_armed() {
        assert_eq!(poisoner_buff().weapon_damage_if_armed(false), None);
        assert_eq!(
            poisoner_buff().weapon_damage_if_armed(true),
            Some(DamageRider::new(2, 6))
        );
    }

    #[test]
    fn a_non_matching_rider_never_contributes_this_buff() {
        assert_eq!(sneak_attack().weapon_damage_if_armed(true), None);
    }

    /// The framework end to end: an attacker who has not yet poisoned
    /// anything hits for its base damage alone; the same attacker, once
    /// `arms_on_condition` says the buff is live, appends the extra dice to
    /// every subsequent weapon attack - checked exactly, not merely by field
    /// value, against the same `damage_pmf`/`sample_damage` machinery every
    /// other rider in this module is held to.
    #[test]
    fn armed_buff_raises_mean_damage_and_agrees_with_the_exact_path() {
        let buff = poisoner_buff();
        let defense = Defense::new(12, 60);
        let attack = Attack::new(6, 1, 8, 4);

        let unarmed = attack.clone();
        let armed = attack.with_damage_rider(
            buff.weapon_damage_if_armed(true)
                .expect("armed buff contributes a damage rider"),
        );

        let exact_unarmed = damage_pmf(&unarmed, &defense);
        let exact_armed = damage_pmf(&armed, &defense);
        assert!(
            exact_armed.mean() > exact_unarmed.mean(),
            "an armed buff must raise expected damage over an unarmed attacker"
        );
        // 1d8+4 base, doubled on a crit, plus the buff's 2d6 doubled too.
        assert_eq!(exact_armed.max(), 2 * 8 + 2 * 2 * 6 + 4);

        for (seed, (name, attack)) in [("unarmed", unarmed), ("armed", armed)]
            .into_iter()
            .enumerate()
        {
            let exact = damage_pmf(&attack, &defense);
            let (lo, hi) = (exact.min(), exact.max());
            let mut rng = Rng::new(seed as u64 + 6100);
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

    // --- ITM-06: a generic consumable injury poison -------------------------

    fn injury_poison() -> Rider {
        Rider::InjuryPoison {
            ability: Ability::Con,
            dc: 13,
            debuffed_ability: Ability::Str,
            duration: Duration::ApplierTurn,
        }
    }

    #[test]
    fn injury_poison_reads_back_its_own_parameters() {
        assert_eq!(
            injury_poison().injury_poison(),
            Some((Ability::Con, 13, Ability::Str, Duration::ApplierTurn))
        );
        assert_eq!(sneak_attack().injury_poison(), None);
    }

    #[test]
    fn injury_poison_forcing_save_returns_none_for_an_unrelated_rider() {
        let target = Creature::new("target", 12, 20);
        let mut rng = Rng::new(7700);
        assert_eq!(
            injury_poison_forcing_save(&mut rng, &target, &sneak_attack()),
            None
        );
    }

    /// The forcing save is a plain, flat roll - no advantage or disadvantage
    /// of its own - so its pass rate must land on the same closed form every
    /// other flat save in this crate already agrees with.
    #[test]
    fn injury_poison_forcing_save_agrees_with_a_flat_save_chance() {
        let mut target = Creature::new("target", 12, 30);
        target.saves[Ability::Con.index()] = 2;
        let rider = injury_poison();

        let exact = save_probability_with_mode(RollMode::Normal, 2, 13);
        let mut rng = Rng::new(7701);
        let n = 200_000;
        let successes = (0..n)
            .filter(|_| {
                injury_poison_forcing_save(&mut rng, &target, &rider).expect("this is InjuryPoison")
            })
            .count();
        let got = successes as f64 / f64::from(n);
        let tol = 5.0 * (exact * (1.0 - exact) / f64::from(n)).sqrt() + 1e-4;
        assert!(
            (got - exact).abs() < tol,
            "sampled {got:.5}, exact {exact:.5}, tol {tol:.5}"
        );
    }

    /// The debuff half: once the poison has taken hold, the target's
    /// burdened saves roll with Disadvantage rather than a flat d20 - worse
    /// than normal, and checked exact-vs-sampled the same way every
    /// probability in this crate is.
    #[test]
    fn a_disadvantaged_save_is_worse_than_a_flat_one_and_agrees_with_the_exact_path() {
        let bonus = 3;
        let dc = 15;
        let flat = save_probability_with_mode(RollMode::Normal, bonus, dc);
        let disadvantaged = save_probability_with_mode(RollMode::Disadvantage, bonus, dc);
        assert!(
            disadvantaged < flat,
            "disadvantage on a burdened save must be worse than a flat roll"
        );

        let mut rng = Rng::new(7702);
        let n = 200_000;
        let successes = (0..n)
            .filter(|_| save_with_mode(&mut rng, RollMode::Disadvantage, bonus, dc))
            .count();
        let got = successes as f64 / f64::from(n);
        let tol = 5.0 * (disadvantaged * (1.0 - disadvantaged) / f64::from(n)).sqrt() + 1e-4;
        assert!(
            (got - disadvantaged).abs() < tol,
            "sampled {got:.5}, exact {disadvantaged:.5}, tol {tol:.5}"
        );
    }
}
