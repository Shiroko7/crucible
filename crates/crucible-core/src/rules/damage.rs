//! Damage kinds, individual damage rolls, and how resistance, immunity and
//! vulnerability reduce them.

use crate::prob::{Pmf, Rng};

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

/// One damage component: `2d6 + 3` of a single type - or of whichever of two
/// types its wielder prefers, for a weapon that offers the choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamageRoll {
    pub count: u32,
    pub sides: u32,
    pub bonus: i32,
    pub kind: DamageKind,
    /// A second type this same damage may be dealt as instead, chosen per
    /// attack: a weapon whose wielder can deal cold "instead of the weapon's
    /// normal damage type", a flame blade that can burn or cut. `None` for
    /// every ordinary damage component.
    ///
    /// The choice is made against the target rather than declared in advance -
    /// see [`DamageRoll::reduction_against`] - because that is how it is made
    /// at a table: nobody chooses the type their target is immune to.
    pub alternative: Option<DamageKind>,
}

impl DamageRoll {
    pub fn new(count: u32, sides: u32, bonus: i32, kind: DamageKind) -> Self {
        assert!(sides > 0, "a d0 has no faces");
        Self {
            count,
            sides,
            bonus,
            kind,
            alternative: None,
        }
    }

    /// This component, dealt as either `kind` or `alternative`, whichever
    /// serves its wielder better against the creature in front of it.
    pub fn or(mut self, alternative: DamageKind) -> Self {
        self.alternative = Some(alternative);
        self
    }

    /// Which of this component's types actually lands, given how the target
    /// reduces each: the one it reduces least. Ties keep the printed type,
    /// which is what a stat sheet reads.
    pub fn kind_against(&self, reduce: &dyn Fn(DamageKind) -> Reduction) -> DamageKind {
        match self.alternative {
            Some(alt) if rank(reduce(alt)) > rank(reduce(self.kind)) => alt,
            _ => self.kind,
        }
    }

    /// How the target reduces this component once its type is chosen - what
    /// every damage path feeds into [`DamageRoll::pmf`] and
    /// [`DamageRoll::sample`] in place of `reduce(roll.kind)`.
    pub fn reduction_against(&self, reduce: &dyn Fn(DamageKind) -> Reduction) -> Reduction {
        reduce(self.kind_against(reduce))
    }

    /// The exact distribution of this component alone.
    ///
    /// Flooring at zero happens before the reduction is applied, matching
    /// [`crate::rules::damage_pmf`] and the rule it encodes: resistance and
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

/// Resistance halves and rounds down; vulnerability doubles; immunity zeroes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Reduction {
    #[default]
    Normal,
    Resistant,
    Vulnerable,
    Immune,
}

/// How good a reduction is for the attacker facing it: immunity is worst,
/// vulnerability best. The order a weapon's [`DamageRoll::alternative`] type
/// is chosen by.
fn rank(reduction: Reduction) -> u8 {
    match reduction {
        Reduction::Immune => 0,
        Reduction::Resistant => 1,
        Reduction::Normal => 2,
        Reduction::Vulnerable => 3,
    }
}

impl Reduction {
    #[inline]
    pub fn apply(self, damage: i32) -> i32 {
        match self {
            Reduction::Normal => damage,
            // Integer division truncates toward zero, which is what "round
            // down" means for the non-negative values this ever sees.
            Reduction::Resistant => damage / 2,
            Reduction::Vulnerable => damage * 2,
            Reduction::Immune => 0,
        }
    }

    /// This reduction with a resistance from a second source added. Two
    /// resistances are one, immunity stays immunity, and resistance on top of
    /// vulnerability cancels it - the rules apply both, halving and then
    /// doubling, which is the damage rounded down to an even number: `Normal`
    /// to within a point.
    pub fn with_resistance(self) -> Reduction {
        match self {
            Reduction::Normal => Reduction::Resistant,
            Reduction::Vulnerable => Reduction::Normal,
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prob::Rng;

    #[test]
    fn resistance_halves_and_rounds_down() {
        assert_eq!(Reduction::Resistant.apply(7), 3);
        assert_eq!(Reduction::Resistant.apply(8), 4);
        assert_eq!(Reduction::Resistant.apply(1), 0);
        assert_eq!(Reduction::Vulnerable.apply(7), 14);
        assert_eq!(Reduction::Immune.apply(7), 0);
    }

    /// A weapon whose wielder chooses the damage type picks the type the
    /// creature in front of it reduces least - and the printed one when
    /// there is nothing to choose between them.
    #[test]
    fn damage_with_a_choice_of_type_lands_as_whichever_the_target_reduces_least() {
        let roll = DamageRoll::new(1, 6, 7, DamageKind::Slashing).or(DamageKind::Cold);
        let against = |reduce: &dyn Fn(DamageKind) -> Reduction| {
            (roll.kind_against(reduce), roll.reduction_against(reduce))
        };

        // Nothing to choose: the printed type stands.
        let plain = |_: DamageKind| Reduction::Normal;
        assert_eq!(against(&plain), (DamageKind::Slashing, Reduction::Normal));

        // A shell that shrugs off blades: cold goes round it.
        let armoured = |kind: DamageKind| match kind {
            DamageKind::Slashing => Reduction::Resistant,
            _ => Reduction::Normal,
        };
        assert_eq!(against(&armoured), (DamageKind::Cold, Reduction::Normal));

        // Something out of the deep, immune to cold: the blade it is.
        let frozen = |kind: DamageKind| match kind {
            DamageKind::Cold => Reduction::Immune,
            _ => Reduction::Normal,
        };
        assert_eq!(against(&frozen), (DamageKind::Slashing, Reduction::Normal));

        // Both bad, one worse.
        let tough = |kind: DamageKind| match kind {
            DamageKind::Cold => Reduction::Immune,
            _ => Reduction::Resistant,
        };
        assert_eq!(
            against(&tough),
            (DamageKind::Slashing, Reduction::Resistant)
        );

        // A component with no alternative never changes type.
        let plain_roll = DamageRoll::new(1, 6, 7, DamageKind::Slashing);
        assert_eq!(plain_roll.kind_against(&armoured), DamageKind::Slashing);
        assert_eq!(
            plain_roll.reduction_against(&armoured),
            Reduction::Resistant
        );
    }

    /// The exact and sampled paths choose the same way, which is the property
    /// that keeps a choice of damage type from quietly drifting between them.
    #[test]
    fn a_chosen_damage_type_samples_like_its_exact_distribution() {
        let roll = DamageRoll::new(2, 6, 3, DamageKind::Slashing).or(DamageKind::Cold);
        let armoured = |kind: DamageKind| match kind {
            DamageKind::Slashing => Reduction::Resistant,
            _ => Reduction::Normal,
        };
        let reduction = roll.reduction_against(&armoured);
        let exact = roll.pmf(false, reduction);
        let mut rng = Rng::new(99);
        let n = 20_000;
        let mut total = 0i64;
        for _ in 0..n {
            let d = roll.sample(&mut rng, false, reduction);
            assert!(d >= exact.min() && d <= exact.max());
            total += i64::from(d);
        }
        let sampled_mean = total as f64 / f64::from(n);
        assert!(
            (sampled_mean - exact.mean()).abs() < 0.1,
            "sampled {sampled_mean:.3}, exact {:.3}",
            exact.mean()
        );
    }

    #[test]
    fn a_second_resistance_never_stacks_and_never_undoes_immunity() {
        assert_eq!(Reduction::Normal.with_resistance(), Reduction::Resistant);
        assert_eq!(Reduction::Resistant.with_resistance(), Reduction::Resistant);
        assert_eq!(Reduction::Immune.with_resistance(), Reduction::Immune);
        assert_eq!(Reduction::Vulnerable.with_resistance(), Reduction::Normal);
    }
}
