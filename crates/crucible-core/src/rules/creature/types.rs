//! Fundamental types for combatants: abilities, conditions, durations, and resources.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ability {
    Str,
    Dex,
    Con,
    Int,
    Wis,
    Cha,
}

impl Ability {
    pub fn parse(word: &str) -> Option<Self> {
        Some(match word.to_ascii_lowercase().as_str() {
            "str" | "strength" => Self::Str,
            "dex" | "dexterity" => Self::Dex,
            "con" | "constitution" => Self::Con,
            "int" | "intelligence" => Self::Int,
            "wis" | "wisdom" => Self::Wis,
            "cha" | "charisma" => Self::Cha,
            _ => return None,
        })
    }

    pub fn index(self) -> usize {
        match self {
            Self::Str => 0,
            Self::Dex => 1,
            Self::Con => 2,
            Self::Int => 3,
            Self::Wis => 4,
            Self::Cha => 5,
        }
    }

    pub fn name(self) -> &'static str {
        ["str", "dex", "con", "int", "wis", "cha"][self.index()]
    }
}

/// A 5e creature type - Dragon, Giant, Undead, and so on.
///
/// The only thing anything here asks of it today is equality, as the gate for
/// [`crate::rules::creature::Rider::BonusDamageVsCreatureType`]: a slaying
/// weapon, a favoured-enemy bonus, a holy weapon's bite against fiends and
/// undead are all "extra damage when `target.creature_type` matches", never a
/// branch per weapon. A full type system - half-fiends, shapechangers reading
/// as their original type - is out of scope until a feature actually needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CreatureType {
    Aberration,
    Beast,
    Celestial,
    Construct,
    Dragon,
    Elemental,
    Fey,
    Fiend,
    Giant,
    Humanoid,
    Monstrosity,
    Ooze,
    Plant,
    Undead,
}

impl CreatureType {
    pub fn parse(word: &str) -> Option<Self> {
        Some(match word.to_ascii_lowercase().as_str() {
            "aberration" => Self::Aberration,
            "beast" => Self::Beast,
            "celestial" => Self::Celestial,
            "construct" => Self::Construct,
            "dragon" => Self::Dragon,
            "elemental" => Self::Elemental,
            "fey" => Self::Fey,
            "fiend" => Self::Fiend,
            "giant" => Self::Giant,
            "humanoid" => Self::Humanoid,
            "monstrosity" => Self::Monstrosity,
            "ooze" => Self::Ooze,
            "plant" => Self::Plant,
            "undead" => Self::Undead,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Aberration => "aberration",
            Self::Beast => "beast",
            Self::Celestial => "celestial",
            Self::Construct => "construct",
            Self::Dragon => "dragon",
            Self::Elemental => "elemental",
            Self::Fey => "fey",
            Self::Fiend => "fiend",
            Self::Giant => "giant",
            Self::Humanoid => "humanoid",
            Self::Monstrosity => "monstrosity",
            Self::Ooze => "ooze",
            Self::Plant => "plant",
            Self::Undead => "undead",
        }
    }
}

/// A 5e size category, Tiny through Gargantuan.
///
/// Ordered smallest to largest - the derived [`Ord`] is the whole reason this
/// is a type rather than the size word left as a `String` - so a size-gated
/// effect can compare directly (`target.size <= Size::Large`, Cunning
/// Strike's Trip) instead of matching every variant that qualifies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Size {
    Tiny,
    Small,
    Medium,
    Large,
    Huge,
    Gargantuan,
}

impl Size {
    pub fn parse(word: &str) -> Option<Self> {
        Some(match word.to_ascii_lowercase().as_str() {
            "tiny" => Self::Tiny,
            "small" => Self::Small,
            "medium" => Self::Medium,
            "large" => Self::Large,
            "huge" => Self::Huge,
            "gargantuan" => Self::Gargantuan,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Tiny => "tiny",
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
            Self::Huge => "huge",
            Self::Gargantuan => "gargantuan",
        }
    }
}

/// Most statblocks that bother to state a size are Medium, and plenty do not
/// bother at all - so a creature with nothing declared defaults to Medium
/// rather than to some third "unknown" state every size comparison would
/// then have to account for.
impl Default for Size {
    fn default() -> Self {
        Self::Medium
    }
}

/// A condition, in the 5e sense: a named bundle of effects with a lifetime.
///
/// Conditions are a system rather than a set of flags, which `DESIGN.md` calls
/// out as expensive to retrofit. This is the small version of that system: the
/// questions the duel needs to ask are methods here, so adding a condition
/// means adding a variant and letting the compiler find every site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Condition {
    /// No actions, bonus actions, reactions or legendary actions. Attacks
    /// against it have advantage, and Strength and Dexterity saves fail flat.
    Stunned,
    /// The Dodge action: attacks against it have disadvantage.
    Dodging,
    /// Melee attacks against it have advantage, and its own attacks have
    /// disadvantage.
    Prone,
    /// Disadvantage on its own attack rolls. The SRD also gives it
    /// disadvantage on ability checks, which is not modelled: nothing here
    /// rolls one, the same gap that leaves Blinded's sight-based checks and
    /// Poisoned's own out of scope until an ability-check mechanic exists.
    Poisoned,
    /// Disadvantage on its own attack rolls; attacks against it have
    /// advantage. Auto-failing a sight-based check is real 5e text with
    /// nowhere to attach: see [`Condition::Poisoned`].
    Blinded,
    /// Incapacitated (no actions, bonus actions, reactions or legendary
    /// actions), auto-fails Strength and Dexterity saves, and attacks against
    /// it have advantage - the same three as Stunned. What it adds is
    /// [`Condition::auto_crits`]: a hit landed against it is an automatic
    /// critical.
    Paralyzed,
    /// Can't hear. Carries none of Blinded's combat modifiers - nothing here
    /// rolls a hearing-based check any more than an ability check, so this is
    /// tracked for provenance (Blindness/Deafness names it explicitly as the
    /// caster's alternative choice to Blinded) without changing anything a
    /// duel resolves. The same gap [`Condition::Poisoned`] and
    /// [`Condition::Blinded`] already note.
    Deafened,
    /// Obeys, or resists, a directive on its own very next turn - Command's
    /// mechanism, and Suggestion's and Dominate's if they are ever added. Not
    /// itself a named SRD condition, the same way [`Condition::Dodging`]
    /// names a stance rather than a PHB condition: it is the engine's handle
    /// on "loses this turn to a compulsion" as its own mechanism, distinct
    /// from Incapacitated ([`Condition::incapacitated`]) because it carries
    /// none of that condition's side effects - attacks against a compelled
    /// creature gain no advantage, it does not auto-fail Strength or
    /// Dexterity saves, and (see `sim::duel::Fight::legendary`) it does not
    /// take away legendary actions, since Command's text only ever reaches
    /// the target's own next turn.
    Compelled,
    /// Steady Aim (2024 Rogue 2): advantage on the creature's own attack
    /// rolls, and its speed drops to 0, both for the rest of the turn.
    ///
    /// The advantage half is [`Condition::advantage_on_attacks`], the mirror
    /// of [`Condition::disadvantage_on_attacks`] that nothing needed until
    /// now. The speed half is [`Condition::zeroes_speed`] - there is no
    /// movement model here to apply it against (see `DESIGN.md`'s
    /// "Positioning is the gap that matters"), so it is exposed generically
    /// rather than acted on, ready for whenever one exists.
    SteadyAim,
    /// Cannot cast a spell or activate a magic item
    /// ([`Condition::blocks_magic`]), has disadvantage on every saving throw
    /// it makes ([`Condition::disadvantage_on_saves`]), and any damage it
    /// deals - of any type, to anyone - is halved
    /// ([`Condition::halves_own_damage`]).
    ///
    /// The bundle a limited-use item's forced save applies on a failure (see
    /// [`crate::dsl::plugin::LimitedUseDebuffItemPlugin`]), kept as one
    /// condition rather than three separately-tracked effects because all
    /// three share exactly one applier, one victim and one duration. Unlike
    /// Stunned or Paralyzed, this does **not** incapacitate: the creature
    /// still gets its turn, its ordinary attacks, and every move that is not
    /// tagged [`crate::rules::creature::MoveKind::Spell`] or
    /// [`crate::rules::creature::MoveKind::MagicItem`].
    Suppressed,
    /// Outlined by a fading light: no effect on its own actions or saves, but
    /// the next attack roll made against it - by anyone, not only whoever
    /// applied it - has advantage.
    ///
    /// Guiding Bolt's mark. Unlike every other condition here, it is not
    /// cleared only at a turn boundary: `sim::duel`'s attack resolution
    /// consumes it the moment that next roll happens, so it clears whichever
    /// comes first - a roll against its holder, or the start of the holder's
    /// own next turn (its usual [`Duration::VictimTurn`] expiry, for the
    /// "never got attacked" case).
    Marked,
}

impl Condition {
    pub fn parse(word: &str) -> Option<Self> {
        Some(match word.to_ascii_lowercase().as_str() {
            "stunned" => Self::Stunned,
            "dodging" | "dodge" => Self::Dodging,
            "prone" => Self::Prone,
            "poisoned" => Self::Poisoned,
            "blinded" => Self::Blinded,
            "paralyzed" | "paralysed" => Self::Paralyzed,
            "deafened" | "deafen" => Self::Deafened,
            "compelled" | "compel" => Self::Compelled,
            "steady_aim" | "steady aim" => Self::SteadyAim,
            "suppressed" => Self::Suppressed,
            "marked" => Self::Marked,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Stunned => "stunned",
            Self::Dodging => "dodging",
            Self::Prone => "prone",
            Self::Poisoned => "poisoned",
            Self::Blinded => "blinded",
            Self::Paralyzed => "paralyzed",
            Self::Deafened => "deafened",
            Self::Compelled => "compelled",
            Self::SteadyAim => "steady_aim",
            Self::Suppressed => "suppressed",
            Self::Marked => "marked",
        }
    }

    /// Can the creature act at all? Stunned and Paralyzed both include
    /// Incapacitated, which is what takes away legendary actions as well as
    /// the turn.
    pub fn incapacitated(self) -> bool {
        matches!(self, Self::Stunned | Self::Paralyzed)
    }

    /// Does an attacker striking this creature get advantage?
    ///
    /// Prone grants it only to melee attackers; reach and positioning do not
    /// exist here, so every attack is treated as melee.
    pub fn advantage_to_attackers(self) -> bool {
        matches!(
            self,
            Self::Stunned | Self::Prone | Self::Blinded | Self::Paralyzed | Self::Marked
        )
    }

    pub fn disadvantage_to_attackers(self) -> bool {
        matches!(self, Self::Dodging)
    }

    /// Does this creature's own attack roll suffer?
    pub fn disadvantage_on_attacks(self) -> bool {
        matches!(self, Self::Prone | Self::Poisoned | Self::Blinded)
    }

    /// Does this creature's own attack roll benefit? Steady Aim - the mirror
    /// of [`Condition::disadvantage_on_attacks`], and cancelled by it the same
    /// way any other advantage and disadvantage cancel.
    pub fn advantage_on_attacks(self) -> bool {
        matches!(self, Self::SteadyAim)
    }

    /// Does this condition drop the creature's speed to 0? Steady Aim.
    /// Nothing reads this yet - there is no movement model in this engine -
    /// so this exists purely to expose the flag for whenever one shows up,
    /// rather than leaving Steady Aim's speed clause unmodelled entirely.
    pub fn zeroes_speed(self) -> bool {
        matches!(self, Self::SteadyAim)
    }

    pub fn auto_fails(self, ability: Ability) -> bool {
        matches!(self, Self::Stunned | Self::Paralyzed)
            && matches!(ability, Ability::Str | Ability::Dex)
    }

    /// Evasion is explicitly unavailable while Incapacitated.
    pub fn blocks_riders(self) -> bool {
        self.incapacitated()
    }

    /// Does a hit against this creature land as an automatic critical?
    ///
    /// Paralyzed's actual text is "within 5 feet"; there is no positioning
    /// model to test that against, so - the same call Prone already makes by
    /// treating every attacker as melee - every hit is treated as being close
    /// enough.
    pub fn auto_crits(self) -> bool {
        matches!(self, Self::Paralyzed)
    }

    /// Blocks the two RAW action categories a debuff like this one takes
    /// away: casting a spell
    /// ([`crate::rules::creature::MoveKind::Spell`]) and activating a magic
    /// item ([`crate::rules::creature::MoveKind::MagicItem`]). Whoever
    /// selects a move checks this before taking one of either kind - see
    /// `sim::duel`'s move gating - this only answers the question.
    pub fn blocks_magic(self) -> bool {
        matches!(self, Self::Suppressed)
    }

    /// Does this creature roll every saving throw it makes at disadvantage?
    /// Composed with any other source of advantage or disadvantage on a save
    /// via the usual 5e cancellation rule rather than overriding it - see
    /// `sim::duel::save_mode`, the same stacking `attack_mode` already
    /// applies to attack rolls.
    pub fn disadvantage_on_saves(self) -> bool {
        matches!(self, Self::Suppressed)
    }

    /// Halves this creature's own outgoing damage, of any type, against
    /// anyone it attacks - the attacker-side counterpart to
    /// [`crate::rules::combat::Reduction`], which only ever halves by the
    /// *target's* damage type. See `sim::duel::halve_if_suppressed`.
    pub fn halves_own_damage(self) -> bool {
        matches!(self, Self::Suppressed)
    }
}

/// How long an applied condition lasts.
///
/// Three variants because the real wordings differ and the difference
/// matters: Stunning Strike lasts "until the start of *your* next turn", so the
/// stunner's turn ends it; something a victim shakes off - standing up from
/// Prone - ends at the start of the victim's own turn; and a spell that reads
/// "at the end of each of its turns, the target can make a save" - Hold
/// Person, most poisons - does not expire on a schedule at all, but on a
/// repeated die roll that can succeed the very turn it was applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Duration {
    /// Until the start of the next turn of whoever applied it.
    ApplierTurn,
    /// Until the start of the victim's next turn.
    VictimTurn,
    /// Repeats `ability` against `dc` at the end of the victim's own turn,
    /// clearing the condition on a success. Kept as data on the duration
    /// rather than a new engine branch, the same way `SaveOrCondition` keeps
    /// the on-hit save as data: whatever applies the condition - a rider, a
    /// save effect - just names the ability and DC once, and the repeat lives
    /// entirely in the duel's turn-end processing.
    SaveEndTurn { ability: Ability, dc: i32 },
}

/// A pool several moves draw on: focus points, ki, sorcery points, superiority
/// dice. Distinct from [`crate::rules::creature::Uses`], which is one move's private budget.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Resource {
    pub name: String,
    pub max: u32,
}

/// What a move or rider takes out of a pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cost {
    /// Index into the creature's `resources`, resolved when the scenario is
    /// parsed so the hot path never compares strings.
    pub resource: usize,
    pub amount: u32,
}

// --- Spellcasting -----------------------------------------------------
//
// Kept in its own section at the end of the file: unrelated to the types
// above it, and spell slots and the attack/DC formula are the kind of thing
// several other features (upcasting, Warlock slots, item bonuses) will want
// to extend without conflicting with edits elsewhere in this file.

/// How many spell levels a caster can have slots at: 1st through 9th.
pub const SPELL_LEVELS: u32 = 9;

/// A caster's spell slot pools: one independent counter per level, 1st
/// through 9th.
///
/// Distinct from [`Resource`], which is a single named pool shared across
/// several moves (focus, ki, sorcery points). A caster's slots are nine
/// separate counters instead, each with its own maximum, and a slot spent at
/// one level can never fill a different one - so this earns its own type
/// rather than being nine `Resource`s wearing a trenchcoat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SpellSlots {
    max: [u32; SPELL_LEVELS as usize],
    available: [u32; SPELL_LEVELS as usize],
}

impl SpellSlots {
    pub fn new() -> Self {
        Self::default()
    }

    fn index(level: u32) -> usize {
        assert!(
            (1..=SPELL_LEVELS).contains(&level),
            "spell slot level must be 1-9, got {level}"
        );
        (level - 1) as usize
    }

    /// Declare (or redeclare) the maximum slots at `level`, refilling
    /// `available` to match. This is how a config loader sets a caster's
    /// starting pool, before anything has been spent.
    pub fn set_max(&mut self, level: u32, max: u32) {
        let i = Self::index(level);
        self.max[i] = max;
        self.available[i] = max;
    }

    pub fn max(&self, level: u32) -> u32 {
        self.max[Self::index(level)]
    }

    pub fn available(&self, level: u32) -> u32 {
        self.available[Self::index(level)]
    }

    /// Spend one slot of exactly `level`. `false` and no change if none are
    /// left - upcasting and slot substitution are a policy decision for
    /// whatever calls this, not this type's job.
    pub fn cast(&mut self, level: u32) -> bool {
        let i = Self::index(level);
        if self.available[i] == 0 {
            return false;
        }
        self.available[i] -= 1;
        true
    }

    /// A long rest: every slot returns.
    pub fn recover_all(&mut self) {
        self.available = self.max;
    }

    /// Return `amount` slots at `level`, capped at the maximum. A short-rest
    /// feature (Arcane Recovery, a Warlock's own slots) recovers less than
    /// everything, which is why this takes an amount rather than always
    /// filling the pool.
    pub fn recover(&mut self, level: u32, amount: u32) {
        let i = Self::index(level);
        self.available[i] = (self.available[i] + amount).min(self.max[i]);
    }
}

/// How a creature's spell attacks and save DCs are computed - kept distinct
/// from physical weapon stats, and generic over which ability fuels it, since
/// Wisdom, Intelligence and Charisma casters share this formula and differ
/// only in which score feeds it.
///
/// Fields are public and the formula is two small methods rather than one
/// hardcoded number, so a magic item can inspect and adjust `item_bonus`
/// without reconstructing the rest of the profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpellCastingProfile {
    pub ability: Ability,
    pub ability_modifier: i32,
    pub proficiency_bonus: i32,
    /// A flat bonus from equipment: a +1 spell focus, a Rod of the Pact
    /// Keeper. Kept separate from the other two fields so an item can be
    /// swapped without recomputing them.
    pub item_bonus: i32,
}

impl SpellCastingProfile {
    pub fn new(ability: Ability, ability_modifier: i32, proficiency_bonus: i32) -> Self {
        Self {
            ability,
            ability_modifier,
            proficiency_bonus,
            item_bonus: 0,
        }
    }

    pub fn with_item_bonus(mut self, bonus: i32) -> Self {
        self.item_bonus = bonus;
        self
    }

    /// Spell attack modifier: ability modifier + proficiency bonus + item bonus.
    pub fn attack_bonus(&self) -> i32 {
        self.ability_modifier + self.proficiency_bonus + self.item_bonus
    }

    /// Spell save DC: 8 + ability modifier + proficiency bonus + item bonus.
    pub fn save_dc(&self) -> i32 {
        8 + self.attack_bonus()
    }
}
