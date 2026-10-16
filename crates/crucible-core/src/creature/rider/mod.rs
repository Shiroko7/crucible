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
    /// Only a melee attack - a parry, which answers a blade and not an arrow
    /// or a spell cast from across the room.
    MeleeAttack,
}

impl AttackTrigger {
    /// Does an incoming attack of `kind` set this trigger off?
    pub fn answers(self, kind: AttackKind) -> bool {
        match self {
            AttackTrigger::AnyAttack => true,
            AttackTrigger::RangedWeaponAttack => kind.ranged_weapon(),
            AttackTrigger::MeleeAttack => !kind.ranged,
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
    /// Legendary Resistance, which answers any save at no cost. `ability`
    /// narrows it to one kind of save and `reaction` makes it cost the
    /// creature's reaction, which together are a ring that saves its wearer
    /// from a failed Dexterity save a few times a day - the same mechanism,
    /// not a second one. A fighter's Indomitable is this shape too, with one
    /// use and a reroll instead of a pass.
    AlwaysSucceed {
        uses: u32,
        /// The only save it answers, or `None` for any of them.
        ability: Option<Ability>,
        /// Spends the creature's one reaction a round, shared with every
        /// other reaction it has - see [`Rider::is_reaction`].
        reaction: bool,
    },
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
        /// The bonus stays up against every further attack of the same kind
        /// until the start of this creature's next turn, rather than
        /// answering only the one attack that set it off: the Shield spell's
        /// "until the start of your next turn", a parry that leaves the blade
        /// raised. `false` is the one-attack kind - an item raised against a
        /// single arrow.
        ///
        /// Either way it is one reaction: what lasts is the AC, not a licence
        /// to react again.
        lasting: bool,
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
    /// Extra damage dice on the first hit this creature lands on each of its
    /// own turns, whatever it hits with and whatever the situation.
    ///
    /// A swarm of nature spirits that joins one blow a turn, a once-a-turn
    /// elemental strike. Unlike [`Rider::ConditionalExtraDamage`] there is no
    /// gate to qualify for - the budget *is* the limit - and unlike
    /// [`Rider::BonusDamageVsCreatureType`] it applies to everything. It is
    /// counted on the creature's own turns only, which is what "once on each
    /// of your turns" means, and it keeps its own budget rather than sharing
    /// the Stunning-Strike-style one (see `README.md`).
    OncePerTurnDamage {
        dice_count: u32,
        dice_sides: u32,
        bonus: i32,
        damage_kind: DamageKind,
        /// Only a weapon attack carries it - "when you hit a creature with an
        /// attack roll using a weapon", which is how a divine strike and most
        /// class features of this shape are written. `false` is the kind that
        /// rides anything at all: a swarm joining whatever its host does.
        weapon_only: bool,
    },
    /// Damage this creature deals of one of these types also shoves whoever
    /// took it, as long as that creature is `max_size` or smaller: it gains
    /// [`Condition::Pushed`], which moves it a zone further out around a
    /// creature with a mouth (see [`crate::creature::Zone::pushed`]).
    ///
    /// A storm domain's thunderbolt - "when you deal lightning damage to a
    /// Large or smaller creature, you can push it" - and anything else whose
    /// damage type is what does the shoving: a gust, a wave, a shockwave
    /// hammer. Distinct from an `on fail pushed` clause, which belongs to one
    /// move: this rides every blow the creature lands of the named types,
    /// whether it was an attack roll, a saving throw or a dart that simply
    /// hits, and it asks nothing of the roll - only that the damage landed.
    ///
    /// Nothing is pushed by damage that was entirely soaked or ignored, since
    /// what the text answers is damage actually dealt.
    ///
    /// Where the shove actually *moves* anything is the same place every
    /// other push here does: around a creature with a mouth, whose enemies
    /// stand somewhere (see `sim::fight`'s zones). Everywhere else there is
    /// no positioning for it to change, so the condition lands and means
    /// nothing - the same honest gap an `on fail pushed` clause on a spell
    /// already has.
    PushOnDamage {
        kinds: Vec<DamageKind>,
        max_size: Size,
    },
    /// Extra damage dice on every hit against the creature this one has
    /// marked as its quarry - the [`Condition::Quarry`] its own
    /// concentration is holding.
    ///
    /// Hunter's Mark, and Hex. The mark itself is applied by a move
    /// ([`crate::creature::Effect::Afflict`]) and lasts as long as the hunter
    /// concentrates; this is the other half, and it is inert until the mark
    /// is out there: no concentration, no quarry, no dice.
    BonusDamageVsQuarry {
        dice_count: u32,
        dice_sides: u32,
        bonus: i32,
        damage_kind: DamageKind,
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
    /// Attacks and spells this creature lands ignore the target's resistance
    /// to the named damage types: resistance becomes normal damage.
    ///
    /// Elemental Adept, Shatter Resistance, Exploit Vulnerability.
    IgnoreResistance { kinds: Vec<DamageKind> },
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
    /// Magic Resistance: advantage on saving throws against spells and
    /// other magical effects - any save a move tagged
    /// [`crate::creature::MoveKind::Spell`] or
    /// [`crate::creature::MoveKind::MagicItem`] asks for.
    MagicResistance,
    /// Advantage on this creature's attack rolls against anything under
    /// `condition`: a predator that smells fear, a killer and the victim it
    /// has marked.
    AdvantageAgainst { condition: Condition },
    /// Extra damage against a target under `condition`. On a creature it
    /// joins every hit it lands on such a target; on a move it joins that
    /// move's own damage, so a save's bonus is taken or halved with the rest.
    BonusDamageVsCondition {
        dice_count: u32,
        dice_sides: u32,
        bonus: i32,
        damage_kind: DamageKind,
        condition: Condition,
    },
    /// Once a turn, whoever makes an attack roll against this creature first
    /// makes an `ability` save against `dc`. On a failure that attack, and
    /// every other it makes against this creature that turn, lands on one of
    /// the attacker's own side within its reach instead - if there is one.
    RedirectAttacks { ability: Ability, dc: i32 },
    /// Whoever deals this creature damage of one of `kinds` takes as much
    /// of that kind itself - a mind that answers a thought with the same
    /// thought.
    ReflectDamage { kinds: Vec<DamageKind> },
    /// A store of spell levels - a rod that drinks spells in. A slot of
    /// `up_to` or lower that this creature does not have is paid for out of
    /// resource pool `pool`, a level a point; and a spell it stops outright
    /// (see [`crate::creature::Effect::Counter`]) is added to the pool, which
    /// never holds more than `holds`.
    SpellStore { pool: usize, up_to: u32, holds: u32 },
    /// Never healed by anybody but itself: a curse that will not let it
    /// accept aid.
    RefusesHealing,
    /// Sees what is hidden within `feet`: an invisible creature that close
    /// gets nothing from being unseen.
    Truesight { feet: u32 },
    /// On a move: what it lands is borne by one creature at a time. Landing
    /// it on another lifts it from whoever had it.
    OneAtATime,
    /// Extra damage on the first hit this creature lands in the fight, and
    /// then never again: a blow struck out of hiding.
    FirstHitDamage {
        dice_count: u32,
        dice_sides: u32,
        bonus: i32,
        damage_kind: DamageKind,
    },
    /// On a move: a creature it kills outright rises at the start of its
    /// user's next turn as the `which`th of the user's own
    /// [`crate::creature::Creature::summons`], on the user's side and taking
    /// turns of its own.
    RaiseSlain { which: usize },
    /// On a move: whoever is held by what it lands can break out of it at
    /// the start of its own turn by taking this damage - which also ends the
    /// concentration holding it.
    EscapeFor { roll: DamageRoll },
    /// Resistance to `kinds` from attacks that are not magical: a weapon
    /// attack made with nothing enchanted. A spell attack, a move that is a
    /// spell or a magic item at work, a swing its move calls `magical`, one
    /// a boon is riding, and anything from a creature with
    /// [`Rider::MagicWeapons`] all get through whole - and so does damage
    /// that was never an attack roll at all.
    ResistNonmagical { kinds: Vec<DamageKind> },
    /// Every weapon attack this creature makes is magical - see
    /// [`Rider::ResistNonmagical`]. On a move: that move's attacks are.
    MagicWeapons,
    /// Rolls initiative with advantage.
    InitiativeAdvantage,
    /// Its movement never provokes an opportunity attack. On a move: the
    /// turn it is taken, its user also covers its speed a second time - "it
    /// can move up to its speed without provoking opportunity attacks".
    NeverProvokes,
    /// It walks through other creatures as through difficult terrain, and
    /// each one it passes through takes `roll` - once a turn, which is once
    /// a walk.
    PhaseThrough { roll: DamageRoll },
    /// Damage of `kinds` it would take lands on every creature within
    /// `feet` of it instead, and none of it on itself.
    MirrorDamage { kinds: Vec<DamageKind>, feet: u32 },
    /// A reaction: as an attack roll hits it, it changes places with one of
    /// its own side within `feet` whose name has `kin` in it, and that one
    /// is hit instead.
    SwapWhenHit { feet: u32, kin: String },
    /// What a Stealth check has to beat to hide from it: 10 when nothing
    /// says.
    PassivePerception(i32),
    /// The light it stands in - the last one said is the one that holds,
    /// so a scenario can put a creature somewhere brighter than its file
    /// does. See [`crate::creature::Requirement::Shadow`].
    Lit(Light),
    /// On a move: extra damage on a hit whose attack roll had advantage.
    BonusDamageWithAdvantage { roll: DamageRoll },
    /// On a move of several strikes: when every one of them hits the same
    /// target, it takes `damage` more and whatever `then` does - which is a
    /// [`Rider::SaveOrCondition`], when there is one.
    AllHit {
        damage: Vec<DamageRoll>,
        then: Option<Box<Rider>>,
    },
    /// On a move: its strike is made once against each creature in its
    /// reach, whichever side that creature is on, rather than `count` times
    /// against one.
    EachInReach,
    /// Advantage on its `ability` saving throws - against anything dealing
    /// one of `kinds`, or against everything when it names none.
    SaveAdvantage {
        ability: Ability,
        kinds: Vec<DamageKind>,
    },
    /// On a saving throw aimed at one creature: each one that fails passes
    /// it on to the nearest enemy within `feet` of it that it has not
    /// reached yet, which makes the same save - until one succeeds, or
    /// nobody is left in reach.
    Chains { feet: u32 },
    /// On an attack: a miss still deals half its damage.
    HalfOnMiss,
    /// On a saving throw around a target: the target itself is not caught.
    SparesTarget,
    /// On an action: taking it takes the bonus action too.
    TakesBonusAction,
    /// On a saving throw: its user becomes the area - a line from where it
    /// stands to a free space within `feet`, catching every creature on the
    /// way, where it then reappears.
    RideLine { feet: u32 },
    /// On an aura that is raised: it stays where it was put - around
    /// whoever it was aimed at, up to `feet` away - rather than going where
    /// its owner goes, and what it pulls it pulls to its middle. It catches
    /// what starts a turn in it, not what walks in.
    Anchored { feet: u32 },
    /// On an aura that is anchored: difficult terrain, so whoever starts a
    /// turn in it covers half its speed.
    Hinders,
    /// On a move: a second pool it spends, beside its own cost.
    AlsoCosts(Cost),
    /// On an attack: it can be made out to `feet`, past its move's `range`,
    /// at disadvantage - a weapon's long range.
    LongRange { feet: u32 },
    /// On a move: one of the things anybody can do - Dodge, Disengage,
    /// Help, Shove, Grapple - added to every creature loaded from a file
    /// rather than written in its stat block. Taken only when its moment
    /// has come: see `sim::fight`'s `universal_value`.
    Universal,
    /// It closes with whatever it is fighting, whatever it means to do
    /// there: a caster who would rather stand in the press than behind it.
    ClosesIn,
}

/// How much light a creature is standing in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Light {
    #[default]
    Bright,
    Dim,
    Dark,
}

impl Light {
    pub fn parse(word: &str) -> Option<Self> {
        Some(match word.to_ascii_lowercase().as_str() {
            "bright" => Self::Bright,
            "dim" => Self::Dim,
            "dark" | "darkness" => Self::Dark,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Bright => "bright light",
            Self::Dim => "dim light",
            Self::Dark => "darkness",
        }
    }
}

impl Rider {
    /// Riders with a per-fight budget need somewhere to count it down:
    /// Legendary Resistance's uses, an injury poison's single dose.
    /// Reactions are not counted here - they share the creature's one
    /// reaction per round, which `sim::fight` tracks itself.
    pub fn initial_uses(&self) -> u32 {
        match self {
            Rider::AlwaysSucceed { uses, .. } => *uses,
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
                | Rider::AlwaysSucceed { reaction: true, .. }
                | Rider::SwapWhenHit { .. }
        )
    }
}
