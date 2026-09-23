//! A move taken at its maximum roll, bought out of a pool.

use crate::creature::{Cost, Effect, SaveEffect, Strike};
use crate::dsl::grammar;
use crate::features::{
    CreatureBuilder, FeatureError, FeaturePlugin, FeatureRegistry, FeatureResult,
};
use crate::rules::{DamageKind, DamageRoll};

/// A second way to take a move this creature already has: every die of the
/// named damage types lands on its highest face, for a price.
///
/// A storm domain spending its Channel Divinity to deal maximum lightning or
/// thunder damage, a metamagic that empowers a blast, an artefact charge that
/// makes one blow land at its best. All of them are the same mechanism - "do
/// not roll this damage, take its maximum" - which is why nothing here names
/// any of them.
///
/// It is a compile step rather than a decision inside the engine: the
/// maximised cast is registered as its own [`Move`], with the pool it spends
/// on top of whatever the cast itself costs, and every policy then sees two
/// moves and picks between them the way it picks between anything else. A
/// playstyle that hoards never spends the charge, one that ranks on damage
/// spends it on the biggest die pool it can, and the solver plays both out.
///
/// Two consequences worth stating, both of which follow from maximising the
/// dice rather than rolling them:
///
/// - **A maximised die pool cannot crit.** A critical hit in 5e doubles the
///   dice rolled, and this leaves none to double - the damage is a flat
///   amount. Nothing this shape is printed on an attack roll in the first
///   place (they are all saving throws), but a build that puts one on a
///   weapon should know the swing keeps only its untouched components.
/// - **Only the types named are maximised.** A move dealing 5d6 thunder and
///   5d6 radiant, maximised for thunder, lands 30 thunder and still rolls the
///   radiant - which is exactly what a domain that maximises one element
///   does. Declaring no types at all maximises every die the move deals.
///
/// What it does *not* reach is damage that is not written on the move itself:
/// a rider's extra dice, a boon riding the hit, the bonus dice a feature adds
/// as the attack is rolled. Those are decided as the blow lands, and this
/// runs before the fight starts.
#[derive(Debug, Clone, PartialEq)]
pub struct MaximisedDamagePlugin {
    /// What this way of taking the move is called - the name that shows up in
    /// the report, so it should say which move it is a version of.
    pub move_name: String,
    /// The move itself, written in the scenario DSL's move grammar, exactly
    /// as the ordinary version of it is written.
    pub effect: String,
    /// Which damage types land at their maximum. Empty maximises all of them.
    pub kinds: Vec<DamageKind>,
    pub bonus_action: bool,
    /// The pool this way of taking it spends, on top of whatever the move
    /// already costs: `(pool name, amount)`.
    pub resource: Option<(String, u32)>,
}

impl FeaturePlugin for MaximisedDamagePlugin {
    fn id(&self) -> &'static str {
        "maximised_damage"
    }

    fn name(&self) -> &str {
        &self.move_name
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let mut mv = grammar::parse_move_external(
            &format!("{} | {}", self.move_name, self.effect),
            &builder.creature,
        )
        .map_err(FeatureError::InvalidConfiguration)?;

        let before = mv.effect.clone();
        mv.effect = maximise(mv.effect, &self.kinds);
        if mv.effect == before {
            return Err(FeatureError::InvalidConfiguration(format!(
                "`{}` maximises {} damage, and the move deals none of it",
                self.move_name,
                match self.kinds.is_empty() {
                    true => "no".to_string(),
                    false => self
                        .kinds
                        .iter()
                        .map(|k| k.name())
                        .collect::<Vec<_>>()
                        .join("/"),
                }
            )));
        }

        if let Some((pool, amount)) = &self.resource {
            if mv.cost.is_some() {
                return Err(FeatureError::InvalidConfiguration(format!(
                    "`{}` already spends a pool of its own, so it cannot also be charged \
                     `{pool}` - a move has one resource cost",
                    self.move_name
                )));
            }
            mv = mv.with_cost(Cost {
                resource: builder.resource_index(pool)?,
                amount: *amount,
            });
        }

        match self.bonus_action {
            true => builder.add_bonus_action(mv),
            false => builder.add_action(mv),
        }
        Ok(())
    }
}

/// Every damage roll of one of `kinds` (or all of them, if `kinds` is empty)
/// replaced by the flat amount it can never exceed.
fn maximise(effect: Effect, kinds: &[DamageKind]) -> Effect {
    let rolls = |damage: Vec<DamageRoll>| -> Vec<DamageRoll> {
        damage
            .into_iter()
            .map(|roll| at_most(roll, kinds))
            .collect()
    };
    match effect {
        Effect::Strikes { strike, count } => Effect::Strikes {
            strike: Strike {
                damage: rolls(strike.damage),
                ..strike
            },
            count,
        },
        Effect::Save(save) => Effect::Save(SaveEffect {
            damage: rolls(save.damage),
            ..save
        }),
        Effect::AutoHit { damage } => Effect::AutoHit {
            damage: rolls(damage),
        },
        Effect::HarmSwallowed { damage } => Effect::HarmSwallowed {
            damage: rolls(damage),
        },
        Effect::Sequence(parts) => {
            Effect::Sequence(parts.into_iter().map(|p| maximise(p, kinds)).collect())
        }
        Effect::Part {
            effect,
            riders,
            reach,
        } => Effect::Part {
            effect: Box::new(maximise(*effect, kinds)),
            riders,
            reach,
        },
        // Nothing to maximise: a stance, a heal, a ward, a buff, a mark.
        other => other,
    }
}

/// One damage component at its maximum - `3d8+2` becoming a flat `26` - if
/// its type is one of `kinds`, or if no types were named at all.
///
/// The alternative type a component can be dealt as comes along unchanged:
/// which of the two lands is chosen against the target, and both are worth
/// the same maximum.
fn at_most(roll: DamageRoll, kinds: &[DamageKind]) -> DamageRoll {
    let named = |kind| kinds.is_empty() || kinds.contains(&kind);
    if !named(roll.kind) && !roll.alternative.is_some_and(named) {
        return roll;
    }
    let max = i32::try_from(roll.count * roll.sides).unwrap_or(i32::MAX) + roll.bonus;
    let flat = DamageRoll::new(0, 1, max, roll.kind);
    match roll.alternative {
        Some(alternative) => flat.or(alternative),
        None => flat,
    }
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // A maximised way of taking a move: its `name`, the move itself written
    // in the move grammar (`effect`), which damage `kinds` land at their
    // maximum (all of them if unnamed), and the `resource`/`cost` pair that
    // buys it.
    registry.register("maximised_damage", |val| {
        let name = val.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
            FeatureError::InvalidConfiguration("maximised_damage needs a `name`".to_string())
        })?;
        let effect = val.get("effect").and_then(|v| v.as_str()).ok_or_else(|| {
            FeatureError::InvalidConfiguration(format!(
                "maximised_damage `{name}` needs an `effect` - the move it is a version of, \
                 written the same way the ordinary one is"
            ))
        })?;
        let kinds = match val.get("kinds").and_then(|v| v.as_array()) {
            None => Vec::new(),
            Some(list) => list
                .iter()
                .map(|entry| {
                    let word = entry.as_str().ok_or_else(|| {
                        FeatureError::InvalidConfiguration(
                            "maximised_damage `kinds` is a list of damage types".to_string(),
                        )
                    })?;
                    DamageKind::parse(word)
                        .ok_or_else(|| FeatureError::UnknownDamageKind(word.to_string()))
                })
                .collect::<FeatureResult<Vec<_>>>()?,
        };
        let resource = val.get("resource").and_then(|v| v.as_str()).map(|pool| {
            let amount = val.get("cost").and_then(|v| v.as_integer()).unwrap_or(1) as u32;
            (pool.to_string(), amount)
        });
        Ok(Box::new(MaximisedDamagePlugin {
            move_name: name.to_string(),
            effect: effect.to_string(),
            kinds,
            bonus_action: val
                .get("bonus_action")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            resource,
        }))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::FeatureRegistry;
    use crate::rules::{Ability, SpellCastingProfile};

    fn storm_priest() -> CreatureBuilder {
        let mut builder = CreatureBuilder::new("priest", 18, 60);
        builder.set_spellcasting(SpellCastingProfile::new(Ability::Wis, 4, 3));
        builder.ensure_resource("channel_divinity", 2);
        builder
    }

    fn build(params: &str) -> FeatureResult<CreatureBuilder> {
        let mut builder = storm_priest();
        let value: toml::Value = toml::from_str(params).expect("valid TOML");
        FeatureRegistry::new()
            .build_plugin("maximised_damage", &value)?
            .apply(&mut builder)?;
        Ok(builder)
    }

    /// The whole mechanism: the dice are gone and what is left is the number
    /// they could never beat, the move keeps everything else it was written
    /// with, and the pool it spends is on top of the slot the cast costs.
    #[test]
    fn a_maximised_cast_deals_its_dice_at_their_highest_face() {
        let builder = build(
            r#"
            plugin = "maximised_damage"
            name = "Shatter (maximised)"
            effect = "spell | slot 2 | save con dc 16 | 3d8 thunder | half on success"
            kinds = ["thunder"]
            resource = "channel_divinity"
            "#,
        )
        .expect("applies");
        let m = &builder.creature.actions[0];
        assert_eq!(m.spell_slot_level, Some(2), "still a 2nd-level cast");
        assert_eq!(m.cost.map(|c| c.amount), Some(1), "and a charge on top");
        let Effect::Save(save) = &m.effect else {
            panic!("expected a saving throw, got {:?}", m.effect)
        };
        assert!(save.half_on_success);
        assert_eq!(
            save.damage,
            vec![DamageRoll::new(0, 1, 24, DamageKind::Thunder)],
            "3d8 thunder maximised is a flat 24"
        );
    }

    /// Only what it names: a move dealing two types, maximised for one of
    /// them, still rolls the other.
    #[test]
    fn a_type_it_does_not_name_is_left_rolling() {
        let builder = build(
            r#"
            plugin = "maximised_damage"
            name = "Wave (maximised)"
            effect = "spell | slot 5 | save con dc 16 | 5d6 thunder, 5d6 radiant | half on success"
            kinds = ["thunder"]
            "#,
        )
        .expect("applies");
        let Effect::Save(save) = &builder.creature.actions[0].effect else {
            unreachable!()
        };
        assert_eq!(
            save.damage,
            vec![
                DamageRoll::new(0, 1, 30, DamageKind::Thunder),
                DamageRoll::new(5, 6, 0, DamageKind::Radiant),
            ]
        );
    }

    /// Naming no types at all maximises everything, bonus included.
    #[test]
    fn naming_no_types_maximises_every_die_the_move_deals() {
        let builder = build(
            r#"
            plugin = "maximised_damage"
            name = "Bolt (maximised)"
            effect = "spell | ranged | hit +7 | 2d10+3 lightning"
            "#,
        )
        .expect("applies");
        let Effect::Strikes { strike, .. } = &builder.creature.actions[0].effect else {
            unreachable!()
        };
        assert_eq!(
            strike.damage,
            vec![DamageRoll::new(0, 1, 23, DamageKind::Lightning)],
            "2d10+3 is at most 23"
        );
    }

    /// A move that deals none of the damage it claims to maximise is a
    /// mistake in the sheet, not a move worth registering.
    #[test]
    fn maximising_a_type_the_move_never_deals_is_rejected() {
        let err = build(
            r#"
            plugin = "maximised_damage"
            name = "Cure (maximised)"
            effect = "spell | slot 1 | heal 2d8"
            kinds = ["thunder"]
            "#,
        )
        .expect_err("nothing to maximise");
        assert!(
            matches!(&err, FeatureError::InvalidConfiguration(m) if m.contains("thunder")),
            "{err}"
        );
    }

    /// A move already spending a pool cannot also be charged for being
    /// maximised: a move has one resource cost, and silently dropping either
    /// one would be a free charge.
    #[test]
    fn a_move_that_already_spends_a_pool_is_rejected_rather_than_charged_twice() {
        let err = build(
            r#"
            plugin = "maximised_damage"
            name = "Breath (maximised)"
            effect = "cost channel_divinity 1 | save dex dc 16 | 2d8 lightning"
            resource = "channel_divinity"
            "#,
        )
        .expect_err("two costs");
        assert!(
            matches!(&err, FeatureError::InvalidConfiguration(m) if m.contains("one resource cost")),
            "{err}"
        );
    }
}
