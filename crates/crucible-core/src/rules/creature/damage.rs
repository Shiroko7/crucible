//! Damage kinds and individual damage rolls.

use crate::prob::dice::Pmf;
use crate::prob::rng::Rng;
use crate::rules::combat::Reduction;

/// The damage types, which exist here so resistance can be looked up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DamageKind {
    Acid,
    Bludgeoning,
    Cold,
    Fire,
    Force,
    Lightning,
    Necrotic,
    Piercing,
    Poison,
    Psychic,
    Radiant,
    Slashing,
    Thunder,
}

impl DamageKind {
    pub fn parse(word: &str) -> Option<Self> {
        Some(match word.to_ascii_lowercase().as_str() {
            "acid" => Self::Acid,
            "bludgeoning" => Self::Bludgeoning,
            "cold" => Self::Cold,
            "fire" => Self::Fire,
            "force" => Self::Force,
            "lightning" => Self::Lightning,
            "necrotic" => Self::Necrotic,
            "piercing" => Self::Piercing,
            "poison" => Self::Poison,
            "psychic" => Self::Psychic,
            "radiant" => Self::Radiant,
            "slashing" => Self::Slashing,
            "thunder" => Self::Thunder,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Acid => "acid",
            Self::Bludgeoning => "bludgeoning",
            Self::Cold => "cold",
            Self::Fire => "fire",
            Self::Force => "force",
            Self::Lightning => "lightning",
            Self::Necrotic => "necrotic",
            Self::Piercing => "piercing",
            Self::Poison => "poison",
            Self::Psychic => "psychic",
            Self::Radiant => "radiant",
            Self::Slashing => "slashing",
            Self::Thunder => "thunder",
        }
    }

    /// The three types Deflect Attacks and friends single out.
    pub fn is_physical(self) -> bool {
        matches!(self, Self::Bludgeoning | Self::Piercing | Self::Slashing)
    }
}

/// One damage component: `2d6 + 3` of a single type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamageRoll {
    pub count: u32,
    pub sides: u32,
    pub bonus: i32,
    pub kind: DamageKind,
}

impl DamageRoll {
    pub fn new(count: u32, sides: u32, bonus: i32, kind: DamageKind) -> Self {
        assert!(sides > 0, "a d0 has no faces");
        Self {
            count,
            sides,
            bonus,
            kind,
        }
    }

    /// The exact distribution of this component alone.
    ///
    /// Flooring at zero happens before the reduction is applied, matching
    /// [`crate::rules::combat::damage_pmf`] and the rule it encodes: resistance and
    /// vulnerability come after every other modifier to the damage.
    pub fn pmf(&self, crit: bool, reduction: Reduction) -> Pmf {
        let dice = if crit { self.count * 2 } else { self.count };
        Pmf::pool(dice, self.sides)
            .offset(self.bonus)
            .floor_at(0)
            .map_values(move |d| reduction.apply(d))
    }

    /// The sampled counterpart of [`DamageRoll::pmf`].
    pub fn sample(&self, rng: &mut Rng, crit: bool, reduction: Reduction) -> i32 {
        let dice = if crit { self.count * 2 } else { self.count };
        let raw: i32 = (0..dice).map(|_| rng.die(self.sides)).sum();
        reduction.apply((raw + self.bonus).max(0))
    }

    /// Undiced total, for a reaction that just subtracts a roll.
    pub fn sample_raw(&self, rng: &mut Rng) -> i32 {
        let raw: i32 = (0..self.count).map(|_| rng.die(self.sides)).sum();
        (raw + self.bonus).max(0)
    }

    pub fn mean(&self) -> f64 {
        f64::from(self.count) * (f64::from(self.sides) + 1.0) / 2.0 + f64::from(self.bonus)
    }
}
