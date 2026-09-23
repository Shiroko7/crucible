//! Shatter (SRD 5.2, 2nd level).

use crate::creature::{Effect, Move, MoveKind, SaveEffect};
use crate::features::spells::{save_dc, slot_level, target_cap};
use crate::features::{CreatureBuilder, FeaturePlugin, FeatureRegistry, FeatureResult};
use crate::rules::{Ability, DamageKind, DamageRoll};

/// Shatter (SRD 5.2, 2nd level): a sudden ringing noise in a 10-foot sphere.
/// Each creature in it makes a Constitution saving throw, taking `dice`d8
/// Thunder damage on a failure and half as much on a success.
///
/// The plainest shape a damaging spell has - a save, a die pool, half on a
/// success - which is exactly why it is worth having: it is the one a
/// domain's maximised-damage feature is most often spent on, and the
/// yardstick the rest of a storm caster's kit is measured against.
///
/// Creatures made of inorganic material have Disadvantage on the save, which
/// is not modelled: nothing here records what a creature is made of, and
/// inventing it for one spell would be a worse lie than leaving it out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShatterPlugin {
    pub dice: u32,
    pub slot: u32,
    pub max_targets: Option<u32>,
}

impl Default for ShatterPlugin {
    fn default() -> Self {
        Self {
            dice: 3,
            slot: 2,
            max_targets: None,
        }
    }
}

impl FeaturePlugin for ShatterPlugin {
    fn id(&self) -> &'static str {
        "shatter"
    }

    fn name(&self) -> &str {
        "Shatter"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let dc = save_dc(builder, "shatter")?;
        builder.add_action(
            Move::new(
                "Shatter",
                Effect::Save(SaveEffect {
                    ability: Ability::Con,
                    dc,
                    damage: vec![DamageRoll::new(self.dice.max(1), 8, 0, DamageKind::Thunder)],
                    half_on_success: true,
                    on_failure: Vec::new(),
                    max_targets: self.max_targets,
                    requires_type: None,
                }),
            )
            .with_spell_slot(self.slot)
            .with_kind(MoveKind::Spell),
        );
        Ok(())
    }
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // Shatter: `dice` d8s of thunder (3 at its base level, one more per level
    // it is upcast), the `slot` it spends, and how many `targets` the sphere
    // catches.
    registry.register("shatter", |val| {
        Ok(Box::new(ShatterPlugin {
            dice: val.get("dice").and_then(|v| v.as_integer()).unwrap_or(3) as u32,
            slot: slot_level(val, 2)?,
            max_targets: target_cap(val)?,
        }))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::{FeatureError, FeatureRegistry};
    use crate::rules::SpellCastingProfile;

    fn build(params: &str) -> FeatureResult<CreatureBuilder> {
        let mut builder = CreatureBuilder::new("cleric", 18, 60);
        builder.set_spellcasting(SpellCastingProfile::new(Ability::Wis, 4, 3));
        let value: toml::Value = toml::from_str(params).expect("valid TOML");
        FeatureRegistry::new()
            .build_plugin("shatter", &value)?
            .apply(&mut builder)?;
        Ok(builder)
    }

    #[test]
    fn it_is_a_second_level_constitution_save_for_half() {
        let builder = build("plugin = \"shatter\"").expect("applies");
        let m = &builder.creature.actions[0];
        assert_eq!(m.spell_slot_level, Some(2));
        assert_eq!(m.kind, MoveKind::Spell);
        let Effect::Save(save) = &m.effect else {
            panic!("expected a saving throw, got {:?}", m.effect)
        };
        assert_eq!(save.ability, Ability::Con);
        assert_eq!(save.dc, 15);
        assert!(save.half_on_success);
        assert_eq!(
            save.damage,
            vec![DamageRoll::new(3, 8, 0, DamageKind::Thunder)]
        );
    }

    /// A slot outside 1-9 is a mistake in the sheet, not a spell.
    #[test]
    fn an_impossible_slot_is_rejected() {
        let err = build("plugin = \"shatter\"\nslot = 12").expect_err("no such slot");
        assert!(
            matches!(&err, FeatureError::InvalidConfiguration(m) if m.contains("level 1 to 9")),
            "{err}"
        );
    }

    /// And so is a target cap of nobody.
    #[test]
    fn a_cap_of_zero_targets_is_rejected() {
        let err = build("plugin = \"shatter\"\ntargets = 0").expect_err("catches nobody");
        assert!(
            matches!(&err, FeatureError::InvalidConfiguration(m) if m.contains("at least one")),
            "{err}"
        );
    }
}
