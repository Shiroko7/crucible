//! Deflect Attacks (Monk 3).
//!
//! Not in the registry yet: every Monk so far writes it as a `deflect ...`
//! trait phrase, which builds the same [`crate::creature::Rider::ReduceDamage`].

use crate::creature::Rider;
use crate::features::{CreatureBuilder, FeaturePlugin, FeatureResult};
use crate::rules::{DamageKind, DamageRoll};

/// Deflect Attacks (Monk 3): Reaction to reduce damage from physical attacks by a damage roll.
#[derive(Debug, Clone)]
pub struct DeflectAttacksPlugin {
    pub dice_count: u32,
    pub dice_sides: u32,
    pub bonus: i32,
    pub kinds: Vec<DamageKind>,
}

impl DeflectAttacksPlugin {
    pub fn new(dice_count: u32, dice_sides: u32, bonus: i32, kinds: Vec<DamageKind>) -> Self {
        Self {
            dice_count,
            dice_sides,
            bonus,
            kinds,
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
        });
        Ok(())
    }
}
