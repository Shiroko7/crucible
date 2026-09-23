//! Call Lightning (SRD 5.2, 3rd level).

use crate::creature::{Effect, Move, MoveKind, SaveEffect, Uses};
use crate::features::spells::{save_dc, slot_level, target_cap};
use crate::features::{CreatureBuilder, FeaturePlugin, FeatureRegistry, FeatureResult};
use crate::rules::{Ability, DamageKind, DamageRoll};

/// Call Lightning (SRD 5.2, 3rd level, concentration, up to 10 minutes): a
/// storm cloud appears, and a bolt falls from it on a point the caster
/// chooses. Each creature under it makes a Dexterity saving throw, taking
/// `dice`d10 Lightning damage - half as much on a success. On each later
/// turn the caster can call another bolt for an action, paying nothing more.
///
/// Registered as two moves, the same split
/// [`crate::features::spells::SpiritualWeaponPlugin`] uses and for the same
/// reason: the first cast pays a slot and can only happen once
/// (`Uses::Limited(1)`), while every later bolt is free and unlimited. A
/// policy ranking by damage takes the paying one first, purely from move
/// order and identical damage, and the free one from then on.
///
/// It carries that plugin's honest gap too: there is no general "this move
/// needs that one to have fired" mechanism, so a policy that never spends
/// anything could in principle call a bolt out of a cloud it never raised.
/// The policies that would do that are the ones that would never have cast
/// the spell at all.
///
/// The concentration is on the first cast alone - which is where the cloud
/// comes from - so a caster who loses it and re-raises the storm pays the
/// slot again only if the sheet says it can.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallLightningPlugin {
    pub dice: u32,
    pub slot: u32,
    pub max_targets: Option<u32>,
}

impl Default for CallLightningPlugin {
    fn default() -> Self {
        Self {
            dice: 3,
            slot: 3,
            max_targets: Some(1),
        }
    }
}

impl CallLightningPlugin {
    /// Both moves throw the same bolt; they differ only in what they cost.
    fn bolt(&self, dc: i32) -> Effect {
        Effect::Save(SaveEffect {
            ability: Ability::Dex,
            dc,
            damage: vec![DamageRoll::new(
                self.dice.max(1),
                10,
                0,
                DamageKind::Lightning,
            )],
            half_on_success: true,
            on_failure: Vec::new(),
            max_targets: self.max_targets,
            requires_type: None,
        })
    }
}

impl FeaturePlugin for CallLightningPlugin {
    fn id(&self) -> &'static str {
        "call_lightning"
    }

    fn name(&self) -> &str {
        "Call Lightning"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let dc = save_dc(builder, "call_lightning")?;
        builder.add_action(
            Move::new("Call Lightning", self.bolt(dc))
                .with_uses(Uses::Limited(1))
                .with_spell_slot(self.slot)
                .with_kind(MoveKind::Spell)
                .with_concentration(),
        );
        builder.add_action(
            Move::new("Call Lightning (Again)", self.bolt(dc)).with_kind(MoveKind::Spell),
        );
        Ok(())
    }
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // Call Lightning: `dice` d10s (3 at its base level, one more per level it
    // is upcast), the `slot` the first cast spends, and how many `targets`
    // one bolt catches - one unless a scenario says otherwise, since the
    // area under a bolt is small.
    registry.register("call_lightning", |val| {
        Ok(Box::new(CallLightningPlugin {
            dice: val.get("dice").and_then(|v| v.as_integer()).unwrap_or(3) as u32,
            slot: slot_level(val, 3)?,
            max_targets: target_cap(val)?.or(Some(1)),
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
        builder.set_spell_slot_max(3, 3);
        let value: toml::Value = toml::from_str(params).expect("valid TOML");
        FeatureRegistry::new()
            .build_plugin("call_lightning", &value)?
            .apply(&mut builder)?;
        Ok(builder)
    }

    /// Pay once, then keep calling: the first bolt spends a slot and is
    /// concentration, the rest are free repeats of the same bolt.
    #[test]
    fn the_first_bolt_pays_a_slot_and_every_later_one_is_free() {
        let builder = build("plugin = \"call_lightning\"").expect("applies");
        let first = &builder.creature.actions[0];
        assert_eq!(first.spell_slot_level, Some(3));
        assert_eq!(first.uses, Uses::Limited(1));
        assert!(first.concentration, "the cloud has to be held up");

        let again = &builder.creature.actions[1];
        assert_eq!(again.name, "Call Lightning (Again)");
        assert_eq!(again.spell_slot_level, None);
        assert_eq!(again.uses, Uses::Unlimited);
        assert!(!again.concentration, "the cloud is already up");
        assert_eq!(
            again.effect, first.effect,
            "the same bolt, whatever it cost to call"
        );

        let Effect::Save(save) = &first.effect else {
            panic!("expected a saving throw")
        };
        assert_eq!(save.ability, Ability::Dex);
        assert_eq!(save.dc, 15);
        assert!(save.half_on_success);
        assert_eq!(
            save.damage,
            vec![DamageRoll::new(3, 10, 0, DamageKind::Lightning)]
        );
        assert_eq!(save.max_targets, Some(1));
    }

    /// Upcast, and aimed at a cluster rather than one creature.
    #[test]
    fn its_dice_slot_and_target_cap_are_declared() {
        let builder =
            build("plugin = \"call_lightning\"\ndice = 5\nslot = 5\ntargets = 3").expect("applies");
        assert_eq!(builder.creature.actions[0].spell_slot_level, Some(5));
        let Effect::Save(save) = &builder.creature.actions[1].effect else {
            unreachable!()
        };
        assert_eq!(save.damage[0].count, 5);
        assert_eq!(save.max_targets, Some(3));
    }
}
