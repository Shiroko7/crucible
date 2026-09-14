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
            Self::Stunned | Self::Prone | Self::Blinded | Self::Paralyzed
        )
    }

    pub fn disadvantage_to_attackers(self) -> bool {
        matches!(self, Self::Dodging)
    }

    /// Does this creature's own attack roll suffer?
    pub fn disadvantage_on_attacks(self) -> bool {
        matches!(self, Self::Prone | Self::Poisoned | Self::Blinded)
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
