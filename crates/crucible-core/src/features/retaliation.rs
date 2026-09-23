//! A reaction that answers whoever just hit this creature.

use crate::creature::{
    AttackTrigger, Cost, Effect, Move, MoveKind, Reaction, ReactionTrigger, SaveEffect, Uses,
};
use crate::features::{
    CreatureBuilder, FeatureError, FeaturePlugin, FeatureRegistry, FeatureResult,
};
use crate::rules::{Ability, DamageRoll};

/// A burst aimed back at whoever landed a blow: it makes a saving throw
/// against this creature's own save DC, and takes damage on a failure - half
/// on a success, unless it is the kind that spares a made save nothing.
///
/// A storm answering whoever closes with its priest, a mantle of thorns, a
/// shell that cracks and sprays, a wizard's fire shield. One mechanism, and
/// the plugin names none of them: what differs between them is the damage
/// type, the save, which blows it answers and how many times a day it can.
///
/// The counterpart to [`crate::creature::Rider::ReactionOnTargeted`], which
/// answers an attack *before* it is resolved by raising Armor Class against
/// it. This one answers a blow that already landed, so it never changes
/// whether the hit happened - it only makes hitting cost something. Both
/// spend the same single reaction a round, and neither is available while
/// Incapacitated.
///
/// The DC is read from the creature's own casting profile rather than written
/// down here, exactly as a spell's is, so a build whose Wisdom changes never
/// leaves a stale number in one of its features. A creature with no profile
/// can still declare one by hand with `dc`.
#[derive(Debug, Clone, PartialEq)]
pub struct RetaliationPlugin {
    pub move_name: String,
    /// Which blows it answers - a melee attack, a ranged weapon, or anything.
    pub trigger: AttackTrigger,
    pub ability: Ability,
    /// An explicit DC, for something with no casting profile to read one
    /// from. `None` reads the creature's own.
    pub dc: Option<i32>,
    pub damage: Vec<DamageRoll>,
    pub half_on_success: bool,
    /// How many times a fight, `Uses::Unlimited` for a trait that never runs
    /// out. A feature refreshing on a rest is `Uses::Limited(n)` here: this
    /// engine plays one fight, so a per-rest budget is a per-fight one.
    pub uses: Uses,
    pub kind: MoveKind,
    /// A pool each use spends, for a version that is paid for rather than
    /// budgeted: `(pool name, amount)`.
    pub resource: Option<(String, u32)>,
}

impl FeaturePlugin for RetaliationPlugin {
    fn id(&self) -> &'static str {
        "retaliation"
    }

    fn name(&self) -> &str {
        &self.move_name
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let dc = match self.dc {
            Some(dc) => dc,
            None => builder.save_dc(&self.move_name)?,
        };
        let mut answer = Move::new(
            self.move_name.clone(),
            Effect::Save(SaveEffect {
                ability: self.ability,
                dc,
                damage: self.damage.clone(),
                half_on_success: self.half_on_success,
                on_failure: Vec::new(),
                // It answers the one creature that set it off, which the
                // fight pins down as the reaction resolves; the cap is here
                // so nothing else is ever caught by it.
                max_targets: Some(1),
                requires_type: None,
            }),
        )
        .with_uses(self.uses)
        .with_kind(self.kind);
        if let Some((pool, amount)) = &self.resource {
            answer = answer.with_cost(Cost {
                resource: builder.resource_index(pool)?,
                amount: *amount,
            });
        }
        builder.creature.reactions.push(Reaction {
            trigger: ReactionTrigger::Hit(self.trigger),
            action: answer,
        });
        Ok(())
    }
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // A reaction answering whoever hit it: its `name`, the `damage` it deals
    // (`2d8 lightning`), the `save` it forces, which blows it answers
    // (`trigger`), how many `uses` it has and, optionally, the pool each one
    // spends.
    registry.register("retaliation", |val| {
        let name = val.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
            FeatureError::InvalidConfiguration("retaliation needs a `name`".to_string())
        })?;
        let damage_text = val.get("damage").and_then(|v| v.as_str()).ok_or_else(|| {
            FeatureError::InvalidConfiguration(format!(
                "retaliation `{name}` needs `damage` - `2d8 lightning`, or \
                 `2d8 lightning or thunder` for a choice of type"
            ))
        })?;
        let damage = crate::dsl::grammar::parse_damage_external(damage_text)
            .map_err(FeatureError::InvalidConfiguration)?;
        let ability = match val.get("save").and_then(|v| v.as_str()) {
            None => Ability::Dex,
            Some(word) => Ability::parse(word)
                .ok_or_else(|| FeatureError::UnknownAbility(word.to_string()))?,
        };
        let trigger = match val.get("trigger").and_then(|v| v.as_str()) {
            None | Some("melee") => AttackTrigger::MeleeAttack,
            Some("any") => AttackTrigger::AnyAttack,
            Some("ranged weapon") | Some("ranged") => AttackTrigger::RangedWeaponAttack,
            Some(other) => {
                return Err(FeatureError::InvalidConfiguration(format!(
                    "retaliation `trigger` is melee, ranged weapon or any, got `{other}`"
                )))
            }
        };
        let uses = match val.get("uses").and_then(|v| v.as_integer()) {
            None => Uses::Unlimited,
            Some(n) if n > 0 => Uses::Limited(n as u32),
            Some(_) => {
                return Err(FeatureError::InvalidConfiguration(
                    "retaliation `uses` is at least one".to_string(),
                ))
            }
        };
        let kind = match val.get("kind").and_then(|v| v.as_str()) {
            None | Some("standard") => MoveKind::Standard,
            Some("spell") => MoveKind::Spell,
            Some("item") | Some("magic_item") => MoveKind::MagicItem,
            Some("object") => MoveKind::ObjectUse,
            Some(other) => {
                return Err(FeatureError::InvalidConfiguration(format!(
                    "retaliation `kind` is standard, spell, item or object, got `{other}`"
                )))
            }
        };
        let resource = val.get("resource").and_then(|v| v.as_str()).map(|pool| {
            let amount = val.get("cost").and_then(|v| v.as_integer()).unwrap_or(1) as u32;
            (pool.to_string(), amount)
        });
        Ok(Box::new(RetaliationPlugin {
            move_name: name.to_string(),
            trigger,
            ability,
            dc: val
                .get("dc")
                .and_then(|v| v.as_integer())
                .map(|dc| dc as i32),
            damage,
            half_on_success: val
                .get("half_on_success")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
            uses,
            kind,
            resource,
        }))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::FeatureRegistry;
    use crate::prob::Rng;
    use crate::rules::{DamageKind, SpellCastingProfile};

    fn build(params: &str) -> FeatureResult<CreatureBuilder> {
        let mut builder = CreatureBuilder::new("priest", 18, 60);
        builder.set_spellcasting(SpellCastingProfile::new(Ability::Wis, 4, 3));
        let value: toml::Value = toml::from_str(params).expect("valid TOML");
        FeatureRegistry::new()
            .build_plugin("retaliation", &value)?
            .apply(&mut builder)?;
        Ok(builder)
    }

    const WRATH: &str = r#"
        plugin = "retaliation"
        name = "Wrath"
        damage = "2d8 lightning"
        save = "dex"
        uses = 3
    "#;

    /// It reads the creature's own save DC, answers melee attacks by
    /// default, and carries the budget it was given.
    #[test]
    fn it_registers_a_reaction_at_the_creatures_own_dc() {
        let builder = build(WRATH).expect("applies");
        let reaction = &builder.creature.reactions[0];
        assert_eq!(
            reaction.trigger,
            ReactionTrigger::Hit(AttackTrigger::MeleeAttack)
        );
        assert_eq!(reaction.action.uses, Uses::Limited(3));
        let Effect::Save(save) = &reaction.action.effect else {
            panic!("expected a saving throw")
        };
        assert_eq!(save.ability, Ability::Dex);
        assert_eq!(save.dc, 15, "8 + 4 Wis + 3 proficiency");
        assert!(save.half_on_success);
        assert_eq!(save.max_targets, Some(1), "only whoever set it off");
        assert_eq!(
            save.damage,
            vec![DamageRoll::new(2, 8, 0, DamageKind::Lightning)]
        );
    }

    /// A creature with no casting profile has no DC to read, and is told so
    /// rather than given one.
    #[test]
    fn without_a_profile_it_needs_a_dc_written_down() {
        let mut plain = CreatureBuilder::new("thornback", 14, 30);
        let value: toml::Value = toml::from_str(WRATH).expect("valid TOML");
        let registry = FeatureRegistry::new();
        let err = registry
            .build_plugin("retaliation", &value)
            .expect("builds")
            .apply(&mut plain)
            .expect_err("no profile");
        assert!(
            matches!(&err, FeatureError::InvalidConfiguration(m) if m.contains("save DC")),
            "{err}"
        );

        let mut explicit: toml::Value = toml::from_str(WRATH).expect("valid TOML");
        explicit.as_table_mut().unwrap().insert(
            "dc".to_string(),
            toml::Value::Integer(i64::from(17_u8.min(i64::MAX as u8))),
        );
        let mut plain = CreatureBuilder::new("thornback", 14, 30);
        registry
            .build_plugin("retaliation", &explicit)
            .expect("builds")
            .apply(&mut plain)
            .expect("an explicit DC needs no profile");
        let Effect::Save(save) = &plain.creature.reactions[0].action.effect else {
            unreachable!()
        };
        assert_eq!(save.dc, 17);
    }

    /// Live, through a whole fight: an attacker trading blows with a
    /// retaliating defender loses hit points it would not otherwise lose -
    /// the plugin's numbers really reaching the fight loop, where
    /// `sim::fight`'s own tests pin down when a reaction fires and what it
    /// costs.
    #[test]
    fn a_retaliating_defender_costs_its_attacker_hit_points_over_a_fight() {
        let raider = &crate::dsl::scenario::parse(
            "creature: raider
ac: 10
hp: 400
initiative: 10
             action: Axe | strikes 2 | hit +10 | 1d12+4 slashing
",
        )
        .expect("parses")[0];
        let taken = |defender: &crate::creature::Creature| {
            let mut rng = Rng::new(7);
            let mut total = 0i64;
            for _ in 0..200 {
                let mut log = None;
                let outcome = crate::sim::run(
                    &mut rng,
                    [defender, raider],
                    [crate::sim::Policy::Greedy; 2],
                    6,
                    &mut log,
                );
                total += outcome.damage_dealt[0];
            }
            total
        };
        let storm = build(
            r#"
            plugin = "retaliation"
            name = "Wrath"
            damage = "2d8 lightning"
            dc = 99
            "#,
        )
        .expect("applies")
        .creature;
        let plain = CreatureBuilder::new("priest", 18, 400).creature;
        let (with, without) = (taken(&storm), taken(&plain));
        assert!(
            with > without,
            "the storm should cost whoever swings at it: {with} vs {without}"
        );
    }
}
