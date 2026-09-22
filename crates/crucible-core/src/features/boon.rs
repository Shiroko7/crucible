//! A lasting boon a creature puts on itself, and the move that puts it there.

use crate::creature::{Boon, Effect, Move, MoveKind, Requirement, Spend, Uses};
use crate::dsl::grammar;
use crate::features::{
    CreatureBuilder, FeatureError, FeaturePlugin, FeatureRegistry, FeatureResult,
};
use crate::rules::{DamageKind, DamageRoll, Duration};

/// A move that puts a lasting [`Boon`] on its user: a card that turns its
/// bearer into something with tougher hide and a heavier blow for a minute, a
/// spell laid on one blade until the caster's attention breaks, a stance of
/// ice.
///
/// One mechanism rather than one plugin per item and spell, because all three
/// of those are the same three questions with different answers: what it adds
/// to a hit, which hits it rides, and what its holder shrugs off while it
/// lasts. Nothing here names a published item or spell, and the numbers are
/// all declared.
///
/// It registers up to two moves:
///
/// - the one that puts the boon up - an Action unless `bonus_action`, under
///   whatever budget it declares (`uses`, a resource, a spell slot), and
///   `concentration` if it needs holding;
/// - optionally, a move that ends it for one last effect - a blade's
///   enchantment discharged in a burst. That one is legal only while the boon
///   is up ([`Requirement::Boon`]) and spends it ([`Spend::Boon`]), so it can
///   never be taken twice or before there is anything to dismiss.
#[derive(Debug, Clone, PartialEq)]
pub struct LastingBoonPlugin {
    pub boon: Boon,
    /// What the move that puts it up is called.
    pub move_name: String,
    pub rounds: u32,
    pub bonus_action: bool,
    pub concentration: bool,
    pub kind: MoveKind,
    pub uses: Uses,
    pub slot: Option<u32>,
    pub resource: Option<(String, u32)>,
    /// A move, written in the scenario DSL, that ends the boon - and its
    /// name.
    pub dismiss: Option<(String, String)>,
}

impl FeaturePlugin for LastingBoonPlugin {
    fn id(&self) -> &'static str {
        "lasting_boon"
    }

    fn name(&self) -> &str {
        &self.move_name
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let which = builder.creature.add_boon(self.boon.clone());
        if u8::try_from(which).is_err() {
            return Err(FeatureError::InvalidConfiguration(
                "a creature can hold at most 256 declared boons".to_string(),
            ));
        }

        let mut raise = Move::new(
            self.move_name.clone(),
            Effect::Boon {
                which,
                duration: Duration::Rounds(self.rounds.max(1)),
            },
        )
        .with_uses(self.uses)
        .with_kind(self.kind);
        if self.concentration {
            raise = raise.with_concentration();
        }
        if let Some(level) = self.slot {
            raise = raise.with_spell_slot(level);
        }
        if let Some((pool, amount)) = &self.resource {
            raise = raise.with_cost(crate::creature::Cost {
                resource: builder.resource_index(pool)?,
                amount: *amount,
            });
        }
        match self.bonus_action {
            true => builder.add_bonus_action(raise),
            false => builder.add_action(raise),
        }

        if let Some((name, effect)) = &self.dismiss {
            let dismissal =
                grammar::parse_move_external(&format!("{name} | {effect}"), &builder.creature)
                    .map_err(FeatureError::InvalidConfiguration)?
                    .requiring(Requirement::Boon { which })
                    .spending(Spend::Boon(which));
            builder.add_bonus_action(dismissal);
        }
        Ok(())
    }
}

/// Read `resist = ["fire", "cold"]` into damage types.
fn resistances(val: &toml::Value) -> FeatureResult<Vec<DamageKind>> {
    let Some(list) = val.get("resist").and_then(|v| v.as_array()) else {
        return Ok(Vec::new());
    };
    list.iter()
        .map(|entry| {
            let name = entry.as_str().ok_or_else(|| {
                FeatureError::InvalidConfiguration(
                    "lasting_boon `resist` is a list of damage types".to_string(),
                )
            })?;
            DamageKind::parse(name).ok_or_else(|| FeatureError::UnknownDamageKind(name.to_string()))
        })
        .collect()
}

/// Read the optional `dice_count`/`dice_sides`/`damage_bonus`/`damage_kind`
/// block into the extra damage the boon puts on a hit.
fn extra_damage(val: &toml::Value) -> FeatureResult<Option<DamageRoll>> {
    let count = val.get("dice_count").and_then(|v| v.as_integer());
    let sides = val.get("dice_sides").and_then(|v| v.as_integer());
    let bonus = val
        .get("damage_bonus")
        .and_then(|v| v.as_integer())
        .unwrap_or(0) as i32;
    let kind_name = val.get("damage_kind").and_then(|v| v.as_str());
    match (count, sides, kind_name) {
        (None, None, None) => Ok(None),
        (Some(count), Some(sides), Some(kind)) if sides > 0 => {
            let kind = DamageKind::parse(kind)
                .ok_or_else(|| FeatureError::UnknownDamageKind(kind.to_string()))?;
            Ok(Some(DamageRoll::new(
                count as u32,
                sides as u32,
                bonus,
                kind,
            )))
        }
        _ => Err(FeatureError::InvalidConfiguration(
            "lasting_boon's extra damage needs `dice_count`, `dice_sides` (above zero) and \
             `damage_kind` together"
                .to_string(),
        )),
    }
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // A lasting boon and the move that puts it up: `name`, how long it lasts
    // (`rounds`, default 10 - a minute), what it adds to a hit
    // (`dice_count`/`dice_sides`/`damage_bonus`/`damage_kind`, optionally
    // narrowed by `weapon_only` or one named `weapon`), what it lets its
    // holder resist (`resist`), what it costs (`uses`, `slot`,
    // `resource`+`cost`), whether it is a `bonus_action`, whether it needs
    // `concentration`, and optionally a `dismiss` move ending it.
    registry.register("lasting_boon", |val| {
        let name = val.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
            FeatureError::InvalidConfiguration("lasting_boon needs a `name`".to_string())
        })?;
        let mut boon = Boon::new(name);
        boon.damage = extra_damage(val)?;
        boon.weapon_only = val
            .get("weapon_only")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if let Some(weapon) = val.get("weapon").and_then(|v| v.as_str()) {
            boon.weapon = Some(weapon.to_string());
            // A boon on one named blade rides that blade's swings, which are
            // weapon attacks by construction - saying so twice is harmless
            // and saying it once is clearer.
            boon.weapon_only = true;
        }
        boon.resist = resistances(val)?;
        if boon.damage.is_none() && boon.resist.is_empty() {
            return Err(FeatureError::InvalidConfiguration(format!(
                "lasting_boon `{name}` does nothing - give it extra damage, resistances, or both"
            )));
        }

        let uses = match val.get("uses").and_then(|v| v.as_integer()) {
            Some(n) if n > 0 => Uses::Limited(n as u32),
            Some(_) => {
                return Err(FeatureError::InvalidConfiguration(
                    "lasting_boon `uses` is at least one".to_string(),
                ))
            }
            None => Uses::Unlimited,
        };
        let slot = match val.get("slot").and_then(|v| v.as_integer()) {
            Some(level) if (1..=i64::from(crate::rules::SPELL_LEVELS)).contains(&level) => {
                Some(level as u32)
            }
            Some(level) => {
                return Err(FeatureError::InvalidConfiguration(format!(
                    "a spell slot is level 1 to 9, got `slot = {level}`"
                )))
            }
            None => None,
        };
        let resource = val.get("resource").and_then(|v| v.as_str()).map(|pool| {
            let amount = val.get("cost").and_then(|v| v.as_integer()).unwrap_or(1) as u32;
            (pool.to_string(), amount)
        });
        let kind = match val.get("kind").and_then(|v| v.as_str()) {
            None | Some("standard") => MoveKind::Standard,
            Some("spell") => MoveKind::Spell,
            Some("item") | Some("magic_item") => MoveKind::MagicItem,
            Some("object") => MoveKind::ObjectUse,
            Some(other) => {
                return Err(FeatureError::InvalidConfiguration(format!(
                    "lasting_boon `kind` is standard, spell, item or object, got `{other}`"
                )))
            }
        };
        let dismiss = match (
            val.get("dismiss").and_then(|v| v.as_str()),
            val.get("dismiss_name").and_then(|v| v.as_str()),
        ) {
            (None, _) => None,
            (Some(effect), Some(name)) => Some((name.to_string(), effect.to_string())),
            (Some(effect), None) => Some((format!("{name} (dismissed)"), effect.to_string())),
        };

        Ok(Box::new(LastingBoonPlugin {
            boon,
            move_name: name.to_string(),
            rounds: val
                .get("rounds")
                .and_then(|v| v.as_integer())
                .unwrap_or(10)
                .max(1) as u32,
            bonus_action: val
                .get("bonus_action")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            concentration: val
                .get("concentration")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            kind,
            uses,
            slot,
            resource,
            dismiss,
        }))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::{AttackKind, Strike};
    use crate::features::FeatureRegistry;
    use crate::rules::{Ability, SpellCastingProfile};

    fn build(toml_text: &str) -> CreatureBuilder {
        let mut builder = CreatureBuilder::new("Bearer", 15, 40);
        builder.set_spellcasting(SpellCastingProfile::new(Ability::Wis, 3, 4));
        let val: toml::Value = toml::from_str(toml_text).expect("valid toml");
        FeatureRegistry::new()
            .build_plugin("lasting_boon", &val)
            .expect("the plugin builds")
            .apply(&mut builder)
            .expect("the plugin applies");
        builder
    }

    /// A card that turns its bearer into something tougher: an Action, once a
    /// day, ten rounds of resistances and a heavier weapon hit.
    #[test]
    fn a_form_is_an_action_with_a_budget_and_a_lifetime() {
        let builder = build(
            r#"
            name = "Beast Form"
            uses = 1
            kind = "item"
            rounds = 10
            resist = ["bludgeoning", "piercing", "slashing"]
            dice_count = 1
            dice_sides = 6
            damage_kind = "force"
            weapon_only = true
            "#,
        );
        let c = &builder.creature;
        assert_eq!(c.boons.len(), 1);
        assert!(c.bonus_actions.is_empty());
        let m = &c.actions[0];
        assert_eq!(m.name, "Beast Form");
        assert_eq!(m.uses, Uses::Limited(1));
        assert_eq!(m.kind, MoveKind::MagicItem);
        assert!(!m.concentration);
        assert_eq!(
            m.effect,
            Effect::Boon {
                which: 0,
                duration: Duration::Rounds(10)
            }
        );

        let boon = &c.boons[0];
        assert_eq!(boon.resist.len(), 3);
        let weapon_swing = Strike::new(9, vec![DamageRoll::new(1, 6, 4, DamageKind::Slashing)]);
        let spell = weapon_swing.clone().with_kind(AttackKind::RANGED_SPELL);
        assert_eq!(
            boon.damage_on(&weapon_swing),
            Some(DamageRoll::new(1, 6, 0, DamageKind::Force))
        );
        assert_eq!(
            boon.damage_on(&spell),
            None,
            "a form's blows, not its spells"
        );
    }

    /// A spell laid on one blade: a concentration bonus action, riding only
    /// that blade, with a burst that can only be let off while it is lit and
    /// puts it out.
    #[test]
    fn a_blade_enchantment_can_be_dismissed_exactly_once() {
        let builder = build(
            r#"
            name = "Holy Weapon"
            bonus_action = true
            concentration = true
            kind = "spell"
            rounds = 600
            uses = 1
            dice_count = 2
            dice_sides = 8
            damage_kind = "radiant"
            weapon = "Frostreaver"
            dismiss_name = "Holy Weapon (Burst)"
            dismiss = "spell | save con dc 15 | 4d8 radiant | half | on fail blinded until save"
            "#,
        );
        let c = &builder.creature;
        let raise = &c.bonus_actions[0];
        assert_eq!(raise.name, "Holy Weapon");
        assert!(raise.concentration);
        assert_eq!(raise.kind, MoveKind::Spell);

        let burst = &c.bonus_actions[1];
        assert_eq!(burst.name, "Holy Weapon (Burst)");
        assert_eq!(burst.requires, Some(Requirement::Boon { which: 0 }));
        assert_eq!(burst.spends, Some(Spend::Boon(0)));

        // Named a weapon, it rides that weapon and nothing else.
        let boon = &c.boons[0];
        let frostreaver = Strike::new(11, vec![DamageRoll::new(1, 6, 7, DamageKind::Slashing)])
            .with_weapon("Frostreaver");
        let other = Strike::new(10, vec![DamageRoll::new(1, 6, 6, DamageKind::Slashing)])
            .with_weapon("Shortsword");
        assert!(boon.damage_on(&frostreaver).is_some());
        assert_eq!(boon.damage_on(&other), None);
    }

    /// A boon that neither adds anything nor takes anything away is a
    /// configuration mistake, not a move worth a turn.
    #[test]
    fn a_boon_that_does_nothing_is_rejected() {
        let val: toml::Value = toml::from_str("name = \"Empty\"").unwrap();
        let built = FeatureRegistry::new().build_plugin("lasting_boon", &val);
        assert!(
            matches!(built, Err(FeatureError::InvalidConfiguration(_))),
            "a boon has to do something"
        );
    }
}
