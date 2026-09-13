//! The Creature combatant model.

use crate::rules::combat::Reduction;

use super::action::Move;
use super::damage::DamageKind;
use super::rider::Rider;
use super::types::{Ability, Resource};

/// One side of a fight.
///
/// `actions`, `bonus_actions` and `legendary` are ordered, and that order is
/// itself the simplest policy: take the first usable one. See
/// [`crate::sim::duel::Policy`].
#[derive(Debug, Clone, PartialEq)]
pub struct Creature {
    pub name: String,
    /// 0 or 1. Read by [`crate::sim::duel::run_teams`]; the one-against-one [`crate::sim::duel::run`]
    /// ignores it and uses argument order instead.
    pub team: u8,
    /// How many copies of this creature are in the fight. The loader expands it;
    /// nothing downstream sees a count greater than one.
    pub count: u32,
    pub ac: i32,
    pub hp: i32,
    pub initiative: i32,
    pub saves: [i32; 6],
    pub reductions: Vec<(DamageKind, Reduction)>,
    pub resources: Vec<Resource>,
    /// Always-on and reactive modifiers: Evasion, Legendary Resistance,
    /// Deflect Attacks.
    pub riders: Vec<Rider>,
    pub actions: Vec<Move>,
    pub bonus_actions: Vec<Move>,
    pub legendary: Vec<Move>,
    /// Legendary actions available each round, each move costing one. Real
    /// blocks have moves costing two or three; nothing here needs that yet.
    pub legendary_uses: u32,
}

impl Creature {
    pub fn new(name: impl Into<String>, ac: i32, hp: i32) -> Self {
        Self {
            name: name.into(),
            team: 0,
            count: 1,
            ac,
            hp,
            initiative: 0,
            saves: [0; 6],
            reductions: Vec::new(),
            resources: Vec::new(),
            riders: Vec::new(),
            actions: Vec::new(),
            bonus_actions: Vec::new(),
            legendary: Vec::new(),
            legendary_uses: 0,
        }
    }

    pub fn save(&self, ability: Ability) -> i32 {
        self.saves[ability.index()]
    }

    pub fn reduction(&self, kind: DamageKind) -> Reduction {
        self.reductions
            .iter()
            .find(|&&(k, _)| k == kind)
            .map(|&(_, r)| r)
            .unwrap_or_default()
    }

    /// Does this creature turn a successful `ability` save for half into no
    /// damage at all?
    pub fn has_evasion(&self, ability: Ability) -> bool {
        self.riders
            .iter()
            .any(|r| matches!(r, Rider::NothingOnSuccess { ability: a } if *a == ability))
    }

    pub fn resource_index(&self, name: &str) -> Option<usize> {
        self.resources
            .iter()
            .position(|r| r.name.eq_ignore_ascii_case(name))
    }

    pub fn with_action(mut self, m: Move) -> Self {
        self.actions.push(m);
        self
    }

    pub fn with_bonus_action(mut self, m: Move) -> Self {
        self.bonus_actions.push(m);
        self
    }

    pub fn with_rider(mut self, rider: Rider) -> Self {
        self.riders.push(rider);
        self
    }
}
