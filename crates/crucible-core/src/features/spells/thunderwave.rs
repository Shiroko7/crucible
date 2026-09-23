//! Thunderwave (SRD 5.2, 1st level).

use crate::creature::{Effect, Move, MoveKind, SaveEffect};
use crate::features::spells::{save_dc, target_cap};
use crate::features::{CreatureBuilder, FeaturePlugin, FeatureRegistry, FeatureResult};
use crate::rules::{Ability, Condition, DamageKind, DamageRoll, Duration};

/// Thunderwave (SRD 5.2, 1st level): a wave of thunderous force sweeps out
/// from the caster. Each creature in a 15-foot cube makes a Constitution
/// saving throw, taking `dice`d8 Thunder damage and being pushed 10 feet away
/// on a failure, or half as much damage and no shove on a success.
///
/// The push is [`Condition::Pushed`], which moves its victim a zone further
/// out around a creature with a mouth and means nothing anywhere else - the
/// same reading every other shove in this engine gets.
///
/// Who a 15-foot cube catches is the one thing no engine without positioning
/// can answer, so it is the `targets` parameter: unset, it catches every
/// enemy in reach, which is the generous reading for a party's own area
/// spell and the same rule [`Effect::Save`] already applies. A scenario that
/// knows better pins it down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThunderwavePlugin {
    pub dice: u32,
    pub slot: u32,
    pub max_targets: Option<u32>,
}

impl Default for ThunderwavePlugin {
    fn default() -> Self {
        Self {
            dice: 2,
            slot: 1,
            max_targets: None,
        }
    }
}

impl FeaturePlugin for ThunderwavePlugin {
    fn id(&self) -> &'static str {
        "thunderwave"
    }

    fn name(&self) -> &str {
        "Thunderwave"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let dc = save_dc(builder, "thunderwave")?;
        builder.add_action(
            Move::new(
                "Thunderwave",
                Effect::Save(SaveEffect {
                    ability: Ability::Con,
                    dc,
                    damage: vec![DamageRoll::new(self.dice.max(1), 8, 0, DamageKind::Thunder)],
                    half_on_success: true,
                    on_failure: vec![(Condition::Pushed, Duration::ApplierTurn)],
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
    // Thunderwave: `dice` d8s of thunder (2 at its base level, one more per
    // level it is upcast), the `slot` it spends, and how many `targets` the
    // cube catches.
    registry.register("thunderwave", |val| {
        Ok(Box::new(ThunderwavePlugin {
            dice: val.get("dice").and_then(|v| v.as_integer()).unwrap_or(2) as u32,
            slot: crate::features::spells::slot_level(val, 1)?,
            max_targets: target_cap(val)?,
        }))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::FeatureRegistry;
    use crate::rules::SpellCastingProfile;

    fn build(params: &str) -> FeatureResult<CreatureBuilder> {
        let mut builder = CreatureBuilder::new("cleric", 18, 60);
        builder.set_spellcasting(SpellCastingProfile::new(Ability::Wis, 4, 3));
        let value: toml::Value = toml::from_str(params).expect("valid TOML");
        FeatureRegistry::new()
            .build_plugin("thunderwave", &value)?
            .apply(&mut builder)?;
        Ok(builder)
    }

    /// Half on a success, a shove only on a failure, and a 1st-level slot.
    #[test]
    fn it_deals_thunder_and_shoves_what_fails() {
        let builder = build("plugin = \"thunderwave\"").expect("applies");
        let m = &builder.creature.actions[0];
        assert_eq!(m.spell_slot_level, Some(1));
        assert_eq!(m.kind, MoveKind::Spell);
        let Effect::Save(save) = &m.effect else {
            panic!("expected a saving throw, got {:?}", m.effect)
        };
        assert_eq!(save.ability, Ability::Con);
        assert_eq!(save.dc, 15);
        assert!(save.half_on_success);
        assert_eq!(
            save.damage,
            vec![DamageRoll::new(2, 8, 0, DamageKind::Thunder)]
        );
        assert_eq!(
            save.on_failure,
            vec![(Condition::Pushed, Duration::ApplierTurn)]
        );
        assert_eq!(save.max_targets, None, "everything the cube covers");
    }

    /// Upcast by hand - more dice, a bigger slot - and a cap on who it
    /// catches.
    #[test]
    fn its_dice_slot_and_target_cap_are_all_declared() {
        let builder =
            build("plugin = \"thunderwave\"\ndice = 4\nslot = 3\ntargets = 2").expect("applies");
        let m = &builder.creature.actions[0];
        assert_eq!(m.spell_slot_level, Some(3));
        let Effect::Save(save) = &m.effect else {
            unreachable!()
        };
        assert_eq!(save.damage[0].count, 4);
        assert_eq!(save.max_targets, Some(2));
    }
}
