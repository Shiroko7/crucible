//! Reliable Talent (2024 Rogue 7).

use crate::features::{CreatureBuilder, FeaturePlugin, FeatureRegistry, FeatureResult};

/// Reliable Talent (2024 Rogue 7): treat any d20 roll of less than 10 as a
/// 10, for an ability check using a skill or tool the rogue is proficient
/// in - a floor under the roll, not a reroll, so it can only ever help.
///
/// This only sets [`crate::creature::Creature::reliable_talent_floor`]
/// via [`CreatureBuilder::set_reliable_talent_floor`]; the "proficient" half
/// of the rule is up to whoever builds the [`crate::rules::CheckRoll`]
/// for a given check, via [`crate::creature::Creature::check_floor`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReliableTalentPlugin {
    pub floor: i32,
}

impl ReliableTalentPlugin {
    /// The standard feature: a floor of 10.
    pub fn new() -> Self {
        Self { floor: 10 }
    }

    /// As [`ReliableTalentPlugin::new`], with an overridden floor - kept
    /// configurable for the same reason Sneak Attack's dice count is, even
    /// though 5e only ever prints 10.
    pub fn with_floor(floor: i32) -> Self {
        Self { floor }
    }
}

impl Default for ReliableTalentPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl FeaturePlugin for ReliableTalentPlugin {
    fn id(&self) -> &'static str {
        "reliable_talent"
    }

    fn name(&self) -> &str {
        "Reliable Talent"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        builder.set_reliable_talent_floor(self.floor);
        Ok(())
    }
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // Reliable Talent (2024 Rogue 7).
    registry.register("reliable_talent", |val| {
        let floor = val.get("floor").and_then(|v| v.as_integer()).unwrap_or(10) as i32;
        Ok(Box::new(ReliableTalentPlugin::with_floor(floor)))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reliable_talent_defaults_to_a_floor_of_ten() {
        let built = CreatureBuilder::new("Rogue", 15, 40)
            .apply_feature(&ReliableTalentPlugin::new())
            .expect("reliable talent applies")
            .build()
            .expect("builds");
        assert_eq!(built.check_floor(true), Some(10));
        // Never applies to a check the creature isn't proficient in.
        assert_eq!(built.check_floor(false), None);
    }

    #[test]
    fn a_creature_without_reliable_talent_has_no_floor_even_when_proficient() {
        let built = CreatureBuilder::new("Fighter", 15, 40)
            .build()
            .expect("builds");
        assert_eq!(built.check_floor(true), None);
    }

    #[test]
    fn reliable_talent_floor_can_be_overridden() {
        let built = CreatureBuilder::new("Homebrew Rogue", 15, 40)
            .apply_feature(&ReliableTalentPlugin::with_floor(12))
            .expect("reliable talent applies")
            .build()
            .expect("builds");
        assert_eq!(built.check_floor(true), Some(12));
    }

    /// End-to-end with `CheckRoll`: a proficient check against a dc a floor
    /// of 10 always meets, once Reliable Talent is applied.
    #[test]
    fn reliable_talent_floor_feeds_check_roll_and_guarantees_success() {
        use crate::rules::CheckRoll;

        let built = CreatureBuilder::new("Rogue", 15, 40)
            .apply_feature(&ReliableTalentPlugin::new())
            .expect("reliable talent applies")
            .build()
            .expect("builds");

        let proficient_check = CheckRoll::new(0, 10);
        let proficient_check = match built.check_floor(true) {
            Some(floor) => proficient_check.with_floor(floor),
            None => proficient_check,
        };
        assert!((proficient_check.success_chance() - 1.0).abs() < 1e-12);

        // The same DC, without the floor because this check is not one the
        // creature is proficient in, is not a guaranteed success.
        let unproficient_check = CheckRoll::new(0, 10);
        let unproficient_check = match built.check_floor(false) {
            Some(floor) => unproficient_check.with_floor(floor),
            None => unproficient_check,
        };
        assert!(unproficient_check.success_chance() < 1.0);
    }
}
