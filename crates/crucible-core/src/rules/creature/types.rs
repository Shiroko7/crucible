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
}

impl Condition {
    pub fn parse(word: &str) -> Option<Self> {
        Some(match word.to_ascii_lowercase().as_str() {
            "stunned" => Self::Stunned,
            "dodging" | "dodge" => Self::Dodging,
            "prone" => Self::Prone,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Stunned => "stunned",
            Self::Dodging => "dodging",
            Self::Prone => "prone",
        }
    }

    /// Can the creature act at all? Stunned includes Incapacitated, which is
    /// what takes away legendary actions as well as the turn.
    pub fn incapacitated(self) -> bool {
        matches!(self, Self::Stunned)
    }

    /// Does an attacker striking this creature get advantage?
    ///
    /// Prone grants it only to melee attackers; reach and positioning do not
    /// exist here, so every attack is treated as melee.
    pub fn advantage_to_attackers(self) -> bool {
        matches!(self, Self::Stunned | Self::Prone)
    }

    pub fn disadvantage_to_attackers(self) -> bool {
        matches!(self, Self::Dodging)
    }

    /// Does this creature's own attack roll suffer?
    pub fn disadvantage_on_attacks(self) -> bool {
        matches!(self, Self::Prone)
    }

    pub fn auto_fails(self, ability: Ability) -> bool {
        matches!(self, Self::Stunned) && matches!(ability, Ability::Str | Ability::Dex)
    }

    /// Evasion is explicitly unavailable while Incapacitated.
    pub fn blocks_riders(self) -> bool {
        self.incapacitated()
    }
}

/// How long an applied condition lasts.
///
/// Two variants because the two real wordings differ and the difference
/// matters: Stunning Strike lasts "until the start of *your* next turn", so the
/// stunner's turn ends it, while something a victim shakes off - standing up
/// from Prone - ends at the start of the victim's own turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Duration {
    /// Until the start of the next turn of whoever applied it.
    ApplierTurn,
    /// Until the start of the victim's next turn.
    VictimTurn,
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
