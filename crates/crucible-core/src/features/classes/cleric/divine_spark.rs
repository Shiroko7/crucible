//! Divine Spark (2024 Cleric 2, Channel Divinity).

use crate::creature::{Cost, Effect, Move, SaveEffect};
use crate::features::{
    CreatureBuilder, FeatureError, FeaturePlugin, FeatureRegistry, FeatureResult,
};
use crate::rules::{Ability, DamageKind, DamageRoll, HealRoll};

/// Divine Spark (2024 Cleric 2): a Channel Divinity use aimed at one
/// creature within 30 feet - roll `dice`d8 and add the cleric's Wisdom
/// modifier, then either restore that many hit points or force a Constitution
/// save for that much Radiant or Necrotic damage, half as much on a success.
///
/// It registers both halves as separate moves, because they are separate
/// decisions with different targets: `sim::fight` aims a heal at the
/// caster's own side (a downed ally first) and a saving throw at the enemy,
/// so writing them as one move would mean choosing who it helps before the
/// fight starts. Both spend the same pool, so a cleric with two uses left has
/// two, whichever way it spends them.
///
/// The dice scale with the cleric's own level - 1d8 at 2nd, 2d8 at 7th, 3d8
/// at 13th, 4d8 at 18th - which is a `dice` parameter here rather than a
/// level: a plugin that took a level would have to carry the whole table, and
/// the sheet already knows which row it is on. The Wisdom modifier and the
/// save DC come from the creature's casting profile, never written down
/// twice.
#[derive(Debug, Clone, PartialEq)]
pub struct DivineSparkPlugin {
    /// How many d8s - see above; the sheet's row of the table.
    pub dice: u32,
    /// Radiant or Necrotic, the cleric's choice at each use. Modelled as a
    /// choice made once, on the sheet: nothing in a fight here turns on which
    /// of the two it is except a target's own resistances, and a cleric who
    /// wants both can declare the plugin twice.
    pub damage_kind: DamageKind,
    /// The pool a use spends, and how much of it.
    pub resource: String,
    pub cost: u32,
    /// Register the healing half as well. `false` for a build that only ever
    /// burns its Channel Divinity offensively, and keeps its move list short.
    pub healing: bool,
}

impl Default for DivineSparkPlugin {
    fn default() -> Self {
        Self {
            dice: 1,
            damage_kind: DamageKind::Radiant,
            resource: "channel_divinity".to_string(),
            cost: 1,
            healing: true,
        }
    }
}

impl FeaturePlugin for DivineSparkPlugin {
    fn id(&self) -> &'static str {
        "divine_spark"
    }

    fn name(&self) -> &str {
        "Divine Spark"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let dc = builder.save_dc("divine_spark")?;
        let modifier = builder.casting_modifier("divine_spark")?;
        let charge = Cost {
            resource: builder.resource_index(&self.resource)?,
            amount: self.cost,
        };
        let dice = self.dice.max(1);

        builder.add_action(
            Move::new(
                "Divine Spark",
                Effect::Save(SaveEffect {
                    ability: Ability::Con,
                    dc,
                    damage: vec![DamageRoll::new(dice, 8, modifier, self.damage_kind)],
                    half_on_success: true,
                    on_failure: Vec::new(),
                    // "another creature you can see": one, not a burst.
                    max_targets: Some(1),
                    requires_type: None,
                }),
            )
            .with_cost(charge),
        );

        if self.healing {
            builder.add_action(
                Move::new(
                    "Divine Spark (Heal)",
                    Effect::Heal(HealRoll::new(dice, 8, modifier)),
                )
                .with_cost(charge),
            );
        }
        Ok(())
    }
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // Divine Spark: `dice` d8s (the sheet's row of the scaling table),
    // `damage_kind` radiant or necrotic, spending `cost` from `resource`
    // (`channel_divinity` unless said otherwise). `healing = false` leaves
    // the healing half off the move list.
    registry.register("divine_spark", |val| {
        let dice = val.get("dice").and_then(|v| v.as_integer()).unwrap_or(1);
        if dice < 1 {
            return Err(FeatureError::InvalidConfiguration(
                "divine_spark rolls at least one d8".to_string(),
            ));
        }
        let damage_kind = match val.get("damage_kind").and_then(|v| v.as_str()) {
            None => DamageKind::Radiant,
            Some(word) => DamageKind::parse(word)
                .ok_or_else(|| FeatureError::UnknownDamageKind(word.to_string()))?,
        };
        Ok(Box::new(DivineSparkPlugin {
            dice: dice as u32,
            damage_kind,
            resource: val
                .get("resource")
                .and_then(|v| v.as_str())
                .unwrap_or("channel_divinity")
                .to_string(),
            cost: val.get("cost").and_then(|v| v.as_integer()).unwrap_or(1) as u32,
            healing: val.get("healing").and_then(|v| v.as_bool()).unwrap_or(true),
        }))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::FeatureRegistry;
    use crate::rules::SpellCastingProfile;

    fn cleric() -> CreatureBuilder {
        let mut builder = CreatureBuilder::new("cleric", 18, 60);
        builder.set_spellcasting(SpellCastingProfile::new(Ability::Wis, 4, 3));
        builder.ensure_resource("channel_divinity", 3);
        builder
    }

    fn build(params: &str) -> FeatureResult<CreatureBuilder> {
        let mut builder = cleric();
        let value: toml::Value = toml::from_str(params).expect("valid TOML");
        FeatureRegistry::new()
            .build_plugin("divine_spark", &value)?
            .apply(&mut builder)?;
        Ok(builder)
    }

    /// Both halves, both paid for out of the same pool, and every number -
    /// the DC, the modifier added to the dice - read off the profile rather
    /// than written into the feature.
    #[test]
    fn it_registers_a_burst_and_a_heal_that_both_spend_channel_divinity() {
        let builder = build(
            r#"
            plugin = "divine_spark"
            dice = 2
            "#,
        )
        .expect("applies");
        let harm = &builder.creature.actions[0];
        let Effect::Save(save) = &harm.effect else {
            panic!("expected a saving throw, got {:?}", harm.effect)
        };
        assert_eq!(save.ability, Ability::Con);
        assert_eq!(save.dc, 15, "8 + 4 Wis + 3 proficiency");
        assert!(save.half_on_success, "half as much on a success");
        assert_eq!(save.max_targets, Some(1), "one creature, not a burst");
        assert_eq!(
            save.damage,
            vec![DamageRoll::new(2, 8, 4, DamageKind::Radiant)],
            "2d8 + the Wisdom modifier"
        );
        assert_eq!(harm.cost.map(|c| c.amount), Some(1));

        let heal = &builder.creature.actions[1];
        assert_eq!(heal.name, "Divine Spark (Heal)");
        assert_eq!(heal.effect, Effect::Heal(HealRoll::new(2, 8, 4)));
        assert_eq!(
            heal.cost, harm.cost,
            "the same pool, whichever way it is spent"
        );
    }

    /// Necrotic instead of radiant, and a build that never heals with it.
    #[test]
    fn the_damage_type_and_the_healing_half_are_both_declared() {
        let builder = build(
            r#"
            plugin = "divine_spark"
            damage_kind = "necrotic"
            healing = false
            "#,
        )
        .expect("applies");
        assert_eq!(builder.creature.actions.len(), 1, "no healing half");
        let Effect::Save(save) = &builder.creature.actions[0].effect else {
            unreachable!()
        };
        assert_eq!(save.damage[0].kind, DamageKind::Necrotic);
        assert_eq!(save.damage[0].count, 1, "one d8 by default");
    }

    /// Without a pool to spend, the feature is a mistake in the sheet: a
    /// Channel Divinity option with no Channel Divinity.
    #[test]
    fn it_needs_the_pool_it_spends_to_be_declared() {
        let mut bare = CreatureBuilder::new("cleric", 18, 60);
        bare.set_spellcasting(SpellCastingProfile::new(Ability::Wis, 4, 3));
        let value: toml::Value = toml::from_str("plugin = \"divine_spark\"").expect("valid TOML");
        let err = FeatureRegistry::new()
            .build_plugin("divine_spark", &value)
            .expect("builds")
            .apply(&mut bare)
            .expect_err("no pool");
        assert_eq!(
            err,
            FeatureError::MissingResource("channel_divinity".to_string())
        );
    }

    /// And without a casting profile there is no DC to force a save against.
    #[test]
    fn it_needs_a_casting_profile_for_its_dc() {
        let mut bare = CreatureBuilder::new("cleric", 18, 60);
        bare.ensure_resource("channel_divinity", 2);
        let value: toml::Value = toml::from_str("plugin = \"divine_spark\"").expect("valid TOML");
        let err = FeatureRegistry::new()
            .build_plugin("divine_spark", &value)
            .expect("builds")
            .apply(&mut bare)
            .expect_err("no profile");
        assert!(
            matches!(&err, FeatureError::InvalidConfiguration(m) if m.contains("save DC")),
            "{err}"
        );
    }
}
