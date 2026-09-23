//! Sacred Flame (SRD 5.2, cantrip).

use crate::creature::{Effect, Move, MoveKind, SaveEffect};
use crate::features::spells::save_dc;
use crate::features::{CreatureBuilder, FeaturePlugin, FeatureRegistry, FeatureResult};
use crate::rules::{Ability, DamageKind, DamageRoll};

/// Sacred Flame (SRD 5.2, Cleric cantrip): flame-like radiance descends on
/// one creature, which makes a Dexterity saving throw against the caster's
/// own DC or takes `dice`d8 Radiant damage. A made save takes nothing - this
/// is one of the saving throws with no half - and cover does not help, which
/// there is no positioning here to model anyway.
///
/// The die pool is a parameter because a cantrip's is a function of the
/// caster's level (1d8, and another die at 5th, 11th and 17th), and the sheet
/// already knows which row it is on. `bonus` is what a feature like Potent
/// Spellcasting adds - "add your Wisdom modifier to the damage you deal with
/// any Cleric cantrip" - written as a number rather than reaching for the
/// profile, because a cleric without that feature adds nothing and the two
/// cases should not look the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SacredFlamePlugin {
    pub dice: u32,
    pub bonus: i32,
}

impl Default for SacredFlamePlugin {
    fn default() -> Self {
        Self { dice: 1, bonus: 0 }
    }
}

impl FeaturePlugin for SacredFlamePlugin {
    fn id(&self) -> &'static str {
        "sacred_flame"
    }

    fn name(&self) -> &str {
        "Sacred Flame"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let dc = save_dc(builder, "sacred_flame")?;
        builder.add_action(
            Move::new(
                "Sacred Flame",
                Effect::Save(SaveEffect {
                    ability: Ability::Dex,
                    dc,
                    damage: vec![DamageRoll::new(
                        self.dice.max(1),
                        8,
                        self.bonus,
                        DamageKind::Radiant,
                    )],
                    half_on_success: false,
                    on_failure: Vec::new(),
                    max_targets: Some(1),
                    requires_type: None,
                }),
            )
            // A cantrip: tagged as a spell, and paying nothing for it.
            .with_kind(MoveKind::Spell),
        );
        Ok(())
    }
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // Sacred Flame: `dice` d8s (one, plus one at 5th, 11th and 17th level)
    // and the `bonus` a feature like Potent Spellcasting adds.
    registry.register("sacred_flame", |val| {
        Ok(Box::new(SacredFlamePlugin {
            dice: val.get("dice").and_then(|v| v.as_integer()).unwrap_or(1) as u32,
            bonus: val.get("bonus").and_then(|v| v.as_integer()).unwrap_or(0) as i32,
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
            .build_plugin("sacred_flame", &value)?
            .apply(&mut builder)?;
        Ok(builder)
    }

    /// A free cast at the caster's own DC, and a made save takes nothing.
    #[test]
    fn it_costs_nothing_and_a_made_save_takes_nothing() {
        let builder = build("plugin = \"sacred_flame\"\ndice = 2\nbonus = 4").expect("applies");
        let m = &builder.creature.actions[0];
        assert_eq!(m.kind, MoveKind::Spell);
        assert_eq!(m.spell_slot_level, None, "a cantrip spends no slot");
        assert!(m.is_free());
        let Effect::Save(save) = &m.effect else {
            panic!("expected a saving throw, got {:?}", m.effect)
        };
        assert_eq!(save.ability, Ability::Dex);
        assert_eq!(save.dc, 15);
        assert!(!save.half_on_success);
        assert_eq!(
            save.damage,
            vec![DamageRoll::new(2, 8, 4, DamageKind::Radiant)]
        );
    }

    #[test]
    fn it_needs_a_casting_profile_for_its_dc() {
        let mut bare = CreatureBuilder::new("cleric", 18, 60);
        let value: toml::Value = toml::from_str("plugin = \"sacred_flame\"").expect("valid TOML");
        let err = FeatureRegistry::new()
            .build_plugin("sacred_flame", &value)
            .expect("builds")
            .apply(&mut bare)
            .expect_err("no profile");
        assert!(
            matches!(&err, FeatureError::InvalidConfiguration(m) if m.contains("save DC")),
            "{err}"
        );
    }
}
