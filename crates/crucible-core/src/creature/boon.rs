//! A lasting boon a creature puts on itself: what it adds to the creature's
//! hits and what it lets it shrug off, for as long as it lasts.

use crate::creature::Strike;
use crate::rules::{DamageKind, DamageRoll};

/// Something that rides on a creature for a while and then stops.
///
/// A mechanism, not a feature: a card that turns its bearer into a beast for
/// a minute (tougher hide, heavier blows), a spell laid on one blade (extra
/// dice, that blade only), a stance of ice (resistances, no extra damage).
/// All three are this one shape with different fields filled in, which is the
/// test of whether it is pulling its weight.
///
/// It is held as a [`crate::rules::Condition::Boon`] naming this boon's index
/// on its owner, so how long it lasts, that concentration ending clears it,
/// and that it is counted down at a turn boundary are all the condition
/// machinery `sim::fight` already runs rather than a second, parallel clock.
#[derive(Debug, Clone, PartialEq)]
pub struct Boon {
    pub name: String,
    /// Extra damage on every qualifying hit, on top of whatever else rides on
    /// it. `None` for a purely defensive boon.
    pub damage: Option<DamageRoll>,
    /// Restrict `damage` to weapon attacks - a form that makes its bearer's
    /// blows land harder, which does nothing for its spells.
    pub weapon_only: bool,
    /// Restrict `damage` further, to swings made with one named weapon: a
    /// spell cast on a single blade. Matched against
    /// [`crate::creature::Strike::weapon`], case-insensitively. `None`
    /// applies to every attack the other gates allow.
    pub weapon: Option<String>,
    /// Damage types its holder resists while it lasts.
    pub resist: Vec<DamageKind>,
    /// Armor Class its holder gains while it lasts - a ward held around it,
    /// a shield of faith. Distinct from
    /// [`crate::creature::Rider::ReactionOnTargeted`], which is spent
    /// against one attack, and from a flat `ac` trait, which is permanent:
    /// this one is up exactly as long as the boon is.
    pub ac: i32,
}

impl Boon {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            damage: None,
            weapon_only: false,
            weapon: None,
            resist: Vec::new(),
            ac: 0,
        }
    }

    /// Armor Class while it lasts.
    pub fn with_ac(mut self, ac: i32) -> Self {
        self.ac = ac;
        self
    }

    /// Extra damage on a qualifying hit.
    pub fn with_damage(mut self, damage: DamageRoll) -> Self {
        self.damage = Some(damage);
        self
    }

    /// Only weapon attacks carry the extra damage.
    pub fn weapon_attacks_only(mut self) -> Self {
        self.weapon_only = true;
        self
    }

    /// Only swings made with this weapon carry the extra damage - see
    /// [`Boon::weapon`].
    pub fn with_weapon(mut self, weapon: impl Into<String>) -> Self {
        self.weapon = Some(weapon.into());
        self
    }

    /// Resist these damage types while it lasts.
    pub fn with_resistance(mut self, kinds: Vec<DamageKind>) -> Self {
        self.resist = kinds;
        self
    }

    /// The extra damage this boon puts on `strike`, or `None` if that swing
    /// is not one it rides: a spell where it wants a weapon, or the wrong
    /// blade.
    pub fn damage_on(&self, strike: &Strike) -> Option<DamageRoll> {
        let damage = self.damage?;
        if self.weapon_only && !strike.kind.weapon {
            return None;
        }
        match &self.weapon {
            Some(weapon) if !strike.made_with(weapon) => None,
            _ => Some(damage),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::AttackKind;

    fn radiant() -> DamageRoll {
        DamageRoll::new(2, 8, 0, DamageKind::Radiant)
    }

    fn swing(weapon: Option<&str>, kind: AttackKind) -> Strike {
        let mut strike =
            Strike::new(7, vec![DamageRoll::new(1, 6, 4, DamageKind::Slashing)]).with_kind(kind);
        if let Some(w) = weapon {
            strike = strike.with_weapon(w);
        }
        strike
    }

    /// A spell laid on one blade rides that blade's swings and no others -
    /// not the off-hand weapon, not an unlabelled attack, not a spell.
    #[test]
    fn a_boon_on_one_weapon_rides_only_that_weapon() {
        let boon = Boon::new("Blessed Blade")
            .with_damage(radiant())
            .weapon_attacks_only()
            .with_weapon("Frostreaver");

        assert_eq!(
            boon.damage_on(&swing(Some("Frostreaver"), AttackKind::MELEE_WEAPON)),
            Some(radiant())
        );
        assert_eq!(
            boon.damage_on(&swing(Some("frostreaver"), AttackKind::MELEE_WEAPON)),
            Some(radiant()),
            "the name is matched case-insensitively, like every other name here"
        );
        assert_eq!(
            boon.damage_on(&swing(Some("Shortsword"), AttackKind::MELEE_WEAPON)),
            None
        );
        assert_eq!(boon.damage_on(&swing(None, AttackKind::MELEE_WEAPON)), None);
    }

    /// A form that makes its bearer hit harder rides every weapon swing
    /// whatever it is holding, and nothing it casts.
    #[test]
    fn a_weapon_only_boon_rides_every_weapon_swing_but_no_spell() {
        let boon = Boon::new("Beast Form")
            .with_damage(DamageRoll::new(1, 6, 0, DamageKind::Force))
            .weapon_attacks_only()
            .with_resistance(vec![DamageKind::Bludgeoning]);

        assert!(boon
            .damage_on(&swing(None, AttackKind::MELEE_WEAPON))
            .is_some());
        assert!(boon
            .damage_on(&swing(Some("Shortsword"), AttackKind::RANGED_WEAPON))
            .is_some());
        assert_eq!(boon.damage_on(&swing(None, AttackKind::RANGED_SPELL)), None);
        assert_eq!(boon.resist, vec![DamageKind::Bludgeoning]);
    }

    /// A purely defensive boon adds nothing to any hit, however it is asked.
    #[test]
    fn a_boon_with_no_damage_never_adds_any() {
        let boon = Boon::new("Ice Form").with_resistance(vec![DamageKind::Cold, DamageKind::Fire]);
        assert_eq!(boon.damage_on(&swing(None, AttackKind::MELEE_WEAPON)), None);
    }
}
