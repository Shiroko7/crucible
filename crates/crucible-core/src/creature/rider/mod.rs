//! Triggered modifiers and reactions (riders).
//!
//! [`Rider`] is one closed enum on purpose - adding a variant should make the
//! compiler find every place that has to handle it - but its mechanics are
//! split by what they do: extra damage on a hit, Cunning Strike's spends,
//! immunity downgrades, and injury poisons each get their own file.

mod cunning_strike;
mod extra_damage;
mod immunity;
mod injury_poison;

use crate::creature::{AttackKind, Cost};
use crate::rules::{Ability, Condition, CreatureType, DamageKind, DamageRoll, Duration, Size};
pub use immunity::{save_success_probability, saving_throw_against_condition};
pub use injury_poison::injury_poison_forcing_save;

/// What kind of incoming attack a [`Rider::ReactionOnTargeted`] answers,
/// read against the incoming strike's own [`AttackKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackTrigger {
    /// Any attack roll made against this creature - the Shield spell.
    AnyAttack,
    /// Only a ranged weapon attack - an item that can be raised against
    /// arrows but not against a sword or a spell.
    RangedWeaponAttack,
}

impl AttackTrigger {
    /// Does an incoming attack of `kind` set this trigger off?
    pub fn answers(self, kind: AttackKind) -> bool {
        match self {
            AttackTrigger::AnyAttack => true,
            AttackTrigger::RangedWeaponAttack => kind.ranged_weapon(),
        }
    }
}

/// A triggered modifier.
///
/// Each variant is a mechanism, not a feature. The comments name the features
/// that map onto it, which is the test of whether the abstraction is pulling
/// its weight: a variant only one ability can use is a branch in disguise.
#[derive(Debug, Clone, PartialEq)]
pub enum Rider {
    /// On a hit, the target saves or takes a condition - or several, all off
    /// the one save: a slam that knocks its target prone and pushes it away.
    ///
    /// Stunning Strike. Also every knockdown, every on-hit poison, and the
    /// secondary effect on most breath weapons.
    SaveOrCondition {
        ability: Ability,
        dc: i32,
        conditions: Vec<Condition>,
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
    /// include one of `kinds` by `roll`.
    ///
    /// Deflect Attacks. Heavy Armor Master's flat reduction is the same
    /// shape with a constant roll.
    ///
    /// Like every reaction rider, it spends the creature's one reaction per
    /// round - shared with [`Rider::ReactionOnTargeted`] and
    /// [`Rider::HalveAttackDamage`] - which comes back at the start of its
    /// own turn, and is unavailable while it is Incapacitated.
    ReduceDamage {
        kinds: Vec<DamageKind>,
        roll: DamageRoll,
    },
    /// A reaction spent on being *targeted* by an attack that matches
    /// `trigger`, before its hit or miss is finalized, that adds `ac_bonus`
    /// to this creature's AC against that one attack - capable of turning
    /// what would have been a hit into a miss.
    ///
    /// The mirror of [`Rider::ReduceDamage`]: that one reacts to an attack
    /// that already hit, on its damage; this one reacts to being targeted,
    /// before the roll against AC is decided. A reaction that boosts AC
    /// against a targeting attack - the Shield spell against anything, an
    /// item that is raised only against ranged weapon attacks - is this
    /// mechanism; nothing here is specific to any one of them.
    ReactionOnTargeted {
        trigger: AttackTrigger,
        ac_bonus: i32,
    },
    /// A reaction that halves (rounding down) the damage of one attack that
    /// hits this creature.
    ///
    /// Uncanny Dodge (2024 Rogue 5). Spent on the first hit that lands while
    /// the reaction is available - see `sim::fight` - since the engine has no
    /// way to foresee a bigger hit later in the round.
    HalveAttackDamage,
    /// Extra damage dice on a hit, gated on the attack roll having advantage
    /// or an ally next to the target - and never at all if the attacker also
    /// has disadvantage, which overrides an ally in place. Spendable once per
    /// turn if `once_per_turn`.
    ///
    /// Sneak Attack. The gate itself is evaluated by whoever resolves the
    /// attack, from flags on the attack rather than derived geometry - see
    /// [`crate::rules::Attack::ally_adjacent`] and
    /// [`crate::rules::Attack::finesse_or_ranged`] for why. This
    /// variant only carries the dice pool and the once-per-turn budget, so
    /// anything else that shares the exact same gate is this variant too,
    /// not a new branch.
    ConditionalExtraDamage {
        dice_count: u32,
        dice_sides: u32,
        once_per_turn: bool,
    },
    /// The 2024 Rogue's Cunning Strike (Rogue 5) is unlocked, at this save DC:
    /// `8 + Dexterity modifier + proficiency bonus`, computed once by
    /// [`crate::features::classes::rogue::CunningStrikePlugin`] and never the
    /// creature's spellcasting DC - Cunning Strike is not spellcasting, and a
    /// Rogue without a caster subclass has no [`SpellCastingProfile`] to read
    /// one from at all.
    ///
    /// A pure marker, deliberately carrying no dice of its own: what it
    /// unlocks is spending part of a *qualifying Sneak Attack's* pool - the
    /// [`crate::rules::DamageRider`] [`Rider::extra_damage_for`] returns for
    /// [`Rider::ConditionalExtraDamage`] - on a rider effect instead of
    /// rolling it for damage, [`crate::rules::DamageRider::spend`] dice at a
    /// time (1d6 per the 2024 rules), each spend costed and combined by
    /// whoever resolves the attack. Several spends can share one hit's pool as
    /// long as their total fits, because [`crate::rules::DamageRider::spend`]
    /// is exactly the same operation chained.
    ///
    /// Which specific effects a spend buys - poison, a shove, breaking a
    /// grapple - is deliberately not here: this is the generic framework, and
    /// a later plugin per effect reads this same DC rather than inventing its
    /// own.
    ///
    /// [`SpellCastingProfile`]: crate::rules::SpellCastingProfile
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
    /// [`crate::rules::DamageRider`] this contributes has no type of its own
    /// to compose with `combat::damage_pmf`'s single
    /// [`crate::rules::Reduction`]. A future type-aware resistance path on the
    /// exact/sampled attack model would read it from here rather than needing
    /// a new field.
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
    /// `damage` softens a target normally [`crate::creature::Creature`]-immune to
    /// that damage type down to merely resistant (half instead of zero)
    /// against a hit *this* creature lands - see
    /// [`crate::creature::Creature::reduction_from`]. `condition` does the
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
    /// On a hit, unconditionally applies a condition to the target - no
    /// saving throw offered, unlike [`Rider::SaveOrCondition`].
    ///
    /// Guiding Bolt's mark:
    /// [`crate::rules::Condition::Marked`], granting Advantage to
    /// the next attack roll made against the target by anyone, cleared the
    /// moment that roll happens (see `sim::fight::Fight`'s attack resolution)
    /// or at the start of the target's own next turn, whichever comes first.
    /// Distinct from `SaveOrCondition` because nothing about the mark is
    /// resistible - it lands whenever the attack does - and it carries no
    /// cost or once-per-turn budget of its own; the spell's own casting cost
    /// (a spell slot) already gates it.
    ConditionOnHit {
        condition: Condition,
        duration: Duration,
    },
    /// Marks a creature whose [`Rider::ConditionalExtraDamage`] also accepts
    /// a qualifying spell attack roll
    /// ([`crate::rules::Attack::is_spell_attack`]), not only a
    /// finesse-or-ranged weapon attack
    /// ([`crate::rules::Attack::finesse_or_ranged`]).
    ///
    /// Most creatures with `ConditionalExtraDamage` do not carry this - it
    /// is the "some builds grant a feature that lets a non-weapon spell
    /// attack also qualify" extension, gated the same way
    /// [`Rider::NothingOnSuccess`]'s evasion check is: a second rider in the
    /// same list, queried by
    /// [`crate::creature::Creature::extra_damage_applies_to_spell_attacks`]
    /// rather than a field added to `ConditionalExtraDamage` itself, so a
    /// creature can carry the extension without every existing
    /// `ConditionalExtraDamage` construction site needing to know about it. A
    /// no-op on its own; it only changes what
    /// [`Rider::extra_damage_for_with_spell_attack_extension`] does with a
    /// sibling `ConditionalExtraDamage` rider.
    ExtraDamageAppliesToSpellAttacks,
    /// An attacker-side buff, dormant until this creature inflicts `trigger`
    /// on a target by any means - after which its weapon attacks carry
    /// `dice_count`d`dice_sides` (plus `bonus`) extra `damage_kind` damage
    /// for the rest of the encounter.
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
    /// rider's fixed parameters and `sim::fight`'s own bookkeeping. See
    /// [`Rider::arms_on_condition`] and [`Rider::weapon_damage_if_armed`],
    /// which take that armed/not-armed flag as a plain `bool` rather than
    /// storing it here.
    ///
    /// `damage_kind` is recorded and validated like any other damage component
    /// in this DSL, but - like [`Rider::ConditionalExtraDamage`]'s dice - the
    /// [`crate::rules::DamageRider`] this contributes has no type of its own
    /// to compose with `combat::damage_pmf`'s single
    /// [`crate::rules::Reduction`]; a future type-aware resistance
    /// path on the exact/sampled attack model would read it from here.
    ConditionTriggeredWeaponDamage {
        trigger: Condition,
        dice_count: u32,
        dice_sides: u32,
        bonus: i32,
        damage_kind: DamageKind,
    },
    /// A consumable injury poison coating a weapon: the next weapon hit forces
    /// `ability`/`dc` as a saving throw, and a failure burdens the target's
    /// own future `debuffed_ability` saving throws with
    /// [`crate::rules::RollMode::Disadvantage`] -
    /// [`Condition::SaveDisadvantage`] - and applies `condition` too if there
    /// is one (most such poisons also leave the target Poisoned), both for
    /// `duration`.
    ///
    /// See [`injury_poison_forcing_save`] for the forcing save, and
    /// [`crate::rules::save_with_mode`] for the generic "roll a save under a
    /// [`crate::rules::RollMode`]" mechanism the resulting debuff itself uses
    /// once applied - the same [`crate::rules::RollMode`] an attack roll
    /// already rolls under, generalised to saves.
    ///
    /// Generic over both abilities and never named after a specific poison:
    /// `ability`/`dc` is typically a Constitution save against a poison, and
    /// `debuffed_ability` is whichever save the poison burdens, but nothing
    /// here reads either as such. The coating is one dose: its
    /// [`Rider::initial_uses`] is 1, spent by the first weapon hit whether or
    /// not the save then succeeds.
    InjuryPoison {
        ability: Ability,
        dc: i32,
        debuffed_ability: Ability,
        condition: Option<Condition>,
        duration: Duration,
    },
    /// Immune to any single instance of damage below `threshold`: one hit,
    /// one creature's share of one saving throw, one dart. An instance at or
    /// over it lands in full. The damage threshold objects and vehicles
    /// have, on a creature - an armoured shell, a carapace.
    ///
    /// A shell can have a way through it, which skips the threshold
    /// entirely, and then `weak_spot_resists` are resisted instead - see
    /// `sim::fight`'s damage landing for who reaches it: anything from inside
    /// the creature (a creature it swallowed), and attack rolls while it is
    /// [`Condition::Exposed`] and not [`Condition::Sealed`], or
    /// [`Condition::Cracked`].
    ///
    /// Compared against the damage after resistance and every other
    /// reduction - the damage it would actually take.
    DamageThreshold {
        threshold: i32,
        /// A breach cracks the shell: attack rolls go through the crack until
        /// the end of the round ([`Condition::Cracked`]).
        cracks: bool,
        weak_spot_resists: Vec<DamageKind>,
    },
    /// On a hit, a target of `max_size` or smaller is swallowed:
    /// [`Condition::Swallowed`] until this creature regurgitates it or dies.
    ///
    /// A purple worm's bite, a behir's, a tarrasque's. What being inside
    /// costs the victim each round is this creature's [`Rider::Digestion`];
    /// what gets it out is [`Rider::Regurgitate`] or the swallower's death.
    Swallow { max_size: Size },
    /// At the start of each of this creature's turns, every creature it has
    /// swallowed takes `damage` - no roll, only the victim's own resistances.
    /// The acid in a purple worm's gut.
    Digestion { damage: Vec<DamageRoll> },
    /// If this creature takes `threshold` damage or more on a single turn
    /// from creatures it has swallowed, it makes an `ability` save against
    /// `dc` at the end of that turn, and on a failure regurgitates every one
    /// of them, Prone.
    Regurgitate {
        threshold: i32,
        ability: Ability,
        dc: i32,
    },
}

impl Rider {
    /// Riders with a per-fight budget need somewhere to count it down:
    /// Legendary Resistance's uses, an injury poison's single dose.
    /// Reactions are not counted here - they share the creature's one
    /// reaction per round, which `sim::fight` tracks itself.
    pub fn initial_uses(&self) -> u32 {
        match self {
            Rider::AlwaysSucceed { uses } => *uses,
            Rider::InjuryPoison { .. } => 1,
            _ => 0,
        }
    }

    /// Does using this rider cost the creature its reaction?
    pub fn is_reaction(&self) -> bool {
        matches!(
            self,
            Rider::ReduceDamage { .. }
                | Rider::ReactionOnTargeted { .. }
                | Rider::HalveAttackDamage
        )
    }
}
