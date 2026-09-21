//! 5e size categories, Tiny through Gargantuan.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_names_round_trip_through_parse() {
        for s in [
            Size::Tiny,
            Size::Small,
            Size::Medium,
            Size::Large,
            Size::Huge,
            Size::Gargantuan,
        ] {
            assert_eq!(Size::parse(s.name()), Some(s));
        }
        assert_eq!(Size::parse("colossal"), None);
    }

    /// Ord is the whole point of `Size` existing as a type: a size-gated
    /// effect (Cunning Strike's Trip: "Large size or smaller") compares
    /// directly instead of matching every qualifying variant.
    #[test]
    fn size_orders_smallest_to_largest() {
        assert!(Size::Tiny < Size::Small);
        assert!(Size::Large < Size::Huge);
        assert!(Size::Medium <= Size::Large);
        assert!(Size::Huge > Size::Large);
        assert!(Size::Gargantuan > Size::Large);
    }
}
