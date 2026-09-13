//! Standard feature plugins for 5e abilities, traits, and combat actions.

use crate::rules::combat::Reduction;
use crate::rules::creature::{Ability, DamageKind, DamageRoll, Move, Rider};

use super::traits::{CreatureBuilder, FeaturePlugin, FeatureResult};

/// Evasion (Monk 7, Rogue 7): On a Dexterity save, take no damage on a success and half on a fail.
#[derive(Debug, Clone)]
pub struct EvasionPlugin {
    pub ability: Ability,
}

impl EvasionPlugin {
    pub fn new(ability: Ability) -> Self {
        Self { ability }
    }
}

impl FeaturePlugin for EvasionPlugin {
    fn id(&self) -> &'static str {
        "evasion"
    }

    fn name(&self) -> &str {
        "Evasion"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        builder.add_rider(Rider::NothingOnSuccess {
            ability: self.ability,
        });
        Ok(())
    }
}

/// Deflect Attacks (Monk 3): Reaction to reduce damage from physical attacks by a damage roll.
#[derive(Debug, Clone)]
pub struct DeflectAttacksPlugin {
    pub dice_count: u32,
    pub dice_sides: u32,
    pub bonus: i32,
    pub kinds: Vec<DamageKind>,
    pub per_round: u32,
}

impl DeflectAttacksPlugin {
    pub fn new(
        dice_count: u32,
        dice_sides: u32,
        bonus: i32,
        kinds: Vec<DamageKind>,
        per_round: u32,
    ) -> Self {
        Self {
            dice_count,
            dice_sides,
            bonus,
            kinds,
            per_round,
        }
    }
}

impl FeaturePlugin for DeflectAttacksPlugin {
    fn id(&self) -> &'static str {
        "deflect_attacks"
    }

    fn name(&self) -> &str {
        "Deflect Attacks"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        builder.add_rider(Rider::ReduceDamage {
            kinds: self.kinds.clone(),
            roll: DamageRoll::new(
                self.dice_count,
                self.dice_sides,
                self.bonus,
                DamageKind::Force,
            ),
            per_round: self.per_round,
        });
        Ok(())
    }
}

/// Legendary Resistance: Turn a failed save into a success N times per fight.
#[derive(Debug, Clone)]
pub struct LegendaryResistancePlugin {
    pub uses: u32,
}

impl LegendaryResistancePlugin {
    pub fn new(uses: u32) -> Self {
        Self { uses }
    }
}

impl FeaturePlugin for LegendaryResistancePlugin {
    fn id(&self) -> &'static str {
        "legendary_resistance"
    }

    fn name(&self) -> &str {
        "Legendary Resistance"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        builder.add_rider(Rider::AlwaysSucceed { uses: self.uses });
        Ok(())
    }
}

/// A standard Action feature plugin.
#[derive(Debug, Clone)]
pub struct ActionPlugin {
    pub name: String,
    pub action_move: Move,
}

impl ActionPlugin {
    pub fn new(name: impl Into<String>, action_move: Move) -> Self {
        Self {
            name: name.into(),
            action_move,
        }
    }
}

impl FeaturePlugin for ActionPlugin {
    fn id(&self) -> &'static str {
        "action"
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        builder.add_action(self.action_move.clone());
        Ok(())
    }
}

/// A standard Bonus Action feature plugin.
#[derive(Debug, Clone)]
pub struct BonusActionPlugin {
    pub name: String,
    pub bonus_move: Move,
}

impl BonusActionPlugin {
    pub fn new(name: impl Into<String>, bonus_move: Move) -> Self {
        Self {
            name: name.into(),
            bonus_move,
        }
    }
}

impl FeaturePlugin for BonusActionPlugin {
    fn id(&self) -> &'static str {
        "bonus_action"
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        builder.add_bonus_action(self.bonus_move.clone());
        Ok(())
    }
}

/// A Legendary Action feature plugin.
#[derive(Debug, Clone)]
pub struct LegendaryActionPlugin {
    pub name: String,
    pub legendary_move: Move,
}

impl LegendaryActionPlugin {
    pub fn new(name: impl Into<String>, legendary_move: Move) -> Self {
        Self {
            name: name.into(),
            legendary_move,
        }
    }
}

impl FeaturePlugin for LegendaryActionPlugin {
    fn id(&self) -> &'static str {
        "legendary_action"
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        builder.add_legendary_action(self.legendary_move.clone());
        Ok(())
    }
}

/// Resource pool declaration plugin.
#[derive(Debug, Clone)]
pub struct ResourcePoolPlugin {
    pub name: String,
    pub max: u32,
}

impl ResourcePoolPlugin {
    pub fn new(name: impl Into<String>, max: u32) -> Self {
        Self {
            name: name.into(),
            max,
        }
    }
}

impl FeaturePlugin for ResourcePoolPlugin {
    fn id(&self) -> &'static str {
        "resource_pool"
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        builder.ensure_resource(&self.name, self.max);
        Ok(())
    }
}

/// Damage reduction plugin (resistances, immunities, vulnerabilities).
#[derive(Debug, Clone)]
pub struct DamageReductionPlugin {
    pub kind: DamageKind,
    pub reduction: Reduction,
}

impl DamageReductionPlugin {
    pub fn new(kind: DamageKind, reduction: Reduction) -> Self {
        Self { kind, reduction }
    }
}

impl FeaturePlugin for DamageReductionPlugin {
    fn id(&self) -> &'static str {
        "damage_reduction"
    }

    fn name(&self) -> &str {
        match self.reduction {
            Reduction::Resistant => "Resistance",
            Reduction::Immune => "Immunity",
            Reduction::Vulnerable => "Vulnerability",
            Reduction::Normal => "Normal",
        }
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        builder.add_reduction(self.kind, self.reduction);
        Ok(())
    }
}
