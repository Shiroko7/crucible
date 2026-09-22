//! 5e creature types - Dragon, Humanoid, Undead and the rest.

/// A 5e creature type - Dragon, Giant, Undead, and so on.
///
/// The only thing anything here asks of it today is equality, as the gate for
/// [`crate::creature::Rider::BonusDamageVsCreatureType`]: a slaying
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creature_type_names_round_trip_through_parse() {
        for t in [
            CreatureType::Aberration,
            CreatureType::Beast,
            CreatureType::Celestial,
            CreatureType::Construct,
            CreatureType::Dragon,
            CreatureType::Elemental,
            CreatureType::Fey,
            CreatureType::Fiend,
            CreatureType::Giant,
            CreatureType::Humanoid,
            CreatureType::Monstrosity,
            CreatureType::Ooze,
            CreatureType::Plant,
            CreatureType::Undead,
        ] {
            assert_eq!(CreatureType::parse(t.name()), Some(t));
        }
        assert_eq!(CreatureType::parse("Dragon"), Some(CreatureType::Dragon));
        assert_eq!(CreatureType::parse("nonsense"), None);
    }
}
