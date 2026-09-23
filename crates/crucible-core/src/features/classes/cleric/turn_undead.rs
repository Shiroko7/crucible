//! Turn Undead, and the searing version of it (2024 Cleric 2 and 5).

use crate::creature::{Cost, Effect, Move, SaveEffect};
use crate::features::{
    CreatureBuilder, FeatureError, FeaturePlugin, FeatureRegistry, FeatureResult,
};
use crate::rules::{Ability, Condition, DamageKind, DamageRoll, Duration};

/// How long a turned Undead stays turned: a minute, unless something hits it
/// first.
const A_MINUTE: u32 = 10;

/// Turn Undead (2024 Cleric 2, Channel Divinity): every Undead within 30 feet
/// makes a Wisdom saving throw, and one that fails is Frightened and
/// Incapacitated for a minute or until it takes any damage. Sear Undead
/// (Cleric 5) adds Radiant damage to each one that fails.
///
/// Three existing mechanisms and no new ones:
///
/// - the save is type-restricted ([`SaveEffect::requires_type`]), which is
///   Hold Person's "only a humanoid" read the other way round - anything that
///   is not Undead is not caught by it at all, rather than rolling a save it
///   cannot fail;
/// - what it lands is [`Condition::Frightened`] and
///   [`Condition::Incapacitated`] off the one roll, the way a slam lands
///   Prone and Pushed off one save;
/// - "or until it takes any damage" is
///   [`Duration::RoundsOrDamaged`], which ends wherever damage lands rather
///   than at a turn boundary.
///
/// One thing to know about who it catches: a creature whose stat block never
/// said what it is matches any type asked about (see
/// [`crate::creature::Creature::is_creature_type`]), so turning catches it
/// too. That is the same permissive reading Hold Person already gets, and it
/// means a fight against unlabelled monsters overstates what turning is
/// worth - `type:` on the creature, or `creature_type` in its config, is what
/// settles it.
///
/// **A table ruling, stated rather than hidden:** Sear Undead's own damage is
/// dealt by the same effect that turns them, so read strictly it would break
/// the turning the instant it landed. This resolves the damage first and the
/// conditions after, so a seared Undead is still turned - which is what the
/// feature is plainly for, and what a table reading both features together
/// would rule. Any *other* damage, from anyone, ends it as written.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnUndeadPlugin {
    /// Sear Undead's dice: `sear_dice`d8 Radiant to each Undead that fails,
    /// which for a cleric of 5th level or more is its Wisdom modifier. Zero
    /// turns them without searing them, which is the feature before 5th.
    pub sear_dice: u32,
    /// Which creature type it turns. Undead for a cleric; a paladin's
    /// Abjure Foes and anything else shaped like this names its own.
    pub creature_type: String,
    pub resource: String,
    pub cost: u32,
}

impl Default for TurnUndeadPlugin {
    fn default() -> Self {
        Self {
            sear_dice: 0,
            creature_type: "Undead".to_string(),
            resource: "channel_divinity".to_string(),
            cost: 1,
        }
    }
}

impl FeaturePlugin for TurnUndeadPlugin {
    fn id(&self) -> &'static str {
        "turn_undead"
    }

    fn name(&self) -> &str {
        "Turn Undead"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let dc = builder.save_dc("turn_undead")?;
        let charge = Cost {
            resource: builder.resource_index(&self.resource)?,
            amount: self.cost,
        };
        let damage = match self.sear_dice {
            0 => Vec::new(),
            dice => vec![DamageRoll::new(dice, 8, 0, DamageKind::Radiant)],
        };
        let name = match self.sear_dice {
            0 => "Turn Undead",
            _ => "Turn Undead (Sear)",
        };

        builder.add_action(
            Move::new(
                name,
                Effect::Save(SaveEffect {
                    ability: Ability::Wis,
                    dc,
                    damage,
                    // A made save takes nothing at all: Sear Undead damages
                    // "each Undead that fails its saving throw".
                    half_on_success: false,
                    on_failure: vec![
                        (Condition::Frightened, Duration::RoundsOrDamaged(A_MINUTE)),
                        (
                            Condition::Incapacitated,
                            Duration::RoundsOrDamaged(A_MINUTE),
                        ),
                    ],
                    // "each Undead of your choice within 30 feet": as many as
                    // are there.
                    max_targets: None,
                    requires_type: Some(self.creature_type.clone()),
                }),
            )
            .with_cost(charge),
        );
        Ok(())
    }
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // Turn Undead: `sear_dice` d8s of Radiant damage to each one that fails
    // (the cleric's Wisdom modifier, from 5th level; none before that), the
    // `creature_type` it turns, and `cost` from `resource`.
    registry.register("turn_undead", |val| {
        let sear_dice = val
            .get("sear_dice")
            .and_then(|v| v.as_integer())
            .unwrap_or(0);
        if sear_dice < 0 {
            return Err(FeatureError::InvalidConfiguration(
                "turn_undead `sear_dice` is not negative".to_string(),
            ));
        }
        let creature_type = val
            .get("creature_type")
            .and_then(|v| v.as_str())
            .unwrap_or("Undead");
        if crate::rules::CreatureType::parse(creature_type).is_none() {
            return Err(FeatureError::UnknownCreatureType(creature_type.to_string()));
        }
        Ok(Box::new(TurnUndeadPlugin {
            sear_dice: sear_dice as u32,
            creature_type: creature_type.to_string(),
            resource: val
                .get("resource")
                .and_then(|v| v.as_str())
                .unwrap_or("channel_divinity")
                .to_string(),
            cost: val.get("cost").and_then(|v| v.as_integer()).unwrap_or(1) as u32,
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
        builder.ensure_resource("channel_divinity", 3);
        let value: toml::Value = toml::from_str(params).expect("valid TOML");
        FeatureRegistry::new()
            .build_plugin("turn_undead", &value)?
            .apply(&mut builder)?;
        Ok(builder)
    }

    /// The plain version: a Wisdom save nothing but Undead is caught by,
    /// landing both conditions off the one roll, for a minute or until
    /// something hits it.
    #[test]
    fn it_turns_undead_and_nothing_else() {
        let builder = build("plugin = \"turn_undead\"").expect("applies");
        let m = &builder.creature.actions[0];
        assert_eq!(m.name, "Turn Undead");
        assert_eq!(m.cost.map(|c| c.amount), Some(1));
        let Effect::Save(save) = &m.effect else {
            panic!("expected a saving throw, got {:?}", m.effect)
        };
        assert_eq!(save.ability, Ability::Wis);
        assert_eq!(save.dc, 15);
        assert_eq!(save.requires_type.as_deref(), Some("Undead"));
        assert_eq!(save.max_targets, None, "every Undead in reach");
        assert!(save.damage.is_empty(), "no searing before 5th level");
        assert_eq!(
            save.on_failure,
            vec![
                (Condition::Frightened, Duration::RoundsOrDamaged(10)),
                (Condition::Incapacitated, Duration::RoundsOrDamaged(10)),
            ]
        );
    }

    /// Sear Undead: radiant damage on a failed save, and none at all on a
    /// made one.
    #[test]
    fn searing_adds_radiant_damage_only_to_what_fails() {
        let builder = build(
            r#"
            plugin = "turn_undead"
            sear_dice = 4
            "#,
        )
        .expect("applies");
        let m = &builder.creature.actions[0];
        assert_eq!(m.name, "Turn Undead (Sear)");
        let Effect::Save(save) = &m.effect else {
            unreachable!()
        };
        assert_eq!(
            save.damage,
            vec![DamageRoll::new(4, 8, 0, DamageKind::Radiant)]
        );
        assert!(!save.half_on_success, "a made save takes nothing");
    }

    /// The type it turns is declared, and a type nothing in 5e has is a
    /// mistake worth reporting.
    #[test]
    fn the_creature_type_it_turns_is_checked() {
        let builder = build(
            r#"
            plugin = "turn_undead"
            creature_type = "Fiend"
            "#,
        )
        .expect("applies");
        let Effect::Save(save) = &builder.creature.actions[0].effect else {
            unreachable!()
        };
        assert_eq!(save.requires_type.as_deref(), Some("Fiend"));

        let err = build(
            r#"
            plugin = "turn_undead"
            creature_type = "Eldritch"
            "#,
        )
        .expect_err("not a creature type");
        assert_eq!(
            err,
            FeatureError::UnknownCreatureType("Eldritch".to_string())
        );
    }
}
