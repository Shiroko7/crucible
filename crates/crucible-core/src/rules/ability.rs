//! The six ability scores: what every save, check and spellcasting profile
//! is keyed to.

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
