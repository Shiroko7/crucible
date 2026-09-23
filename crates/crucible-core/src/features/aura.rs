//! An aura a creature raises and then holds up, and the move that raises it.

use crate::creature::{Effect, Move, MoveKind, Uses};
use crate::dsl::grammar;
use crate::features::{
    CreatureBuilder, FeatureError, FeaturePlugin, FeatureRegistry, FeatureResult,
};
use crate::rules::Duration;

/// A move that raises a lasting aura: a ring of spirits, a field of flame, a
/// wall of blades - anything every enemy has to answer at the start of its
/// own turn for as long as it stands.
///
/// [`crate::features::boon::LastingBoonPlugin`]'s outward-facing twin, and
/// deliberately the same shape. A boon rides its holder's own blows; this is
/// paid by whoever else is standing in it. Everything else - what it costs to
/// put up, how long it lasts, that concentration drops it - is identical, so
/// the two read the same way in a stat block and neither needed a new clock.
///
/// The aura itself is a move written in the scenario DSL, which is what keeps
/// this generic: the plugin says nothing about radiant damage or Wisdom
/// saves, only that something recurs. A creature that simply *has* an aura
/// never needs this - it writes an `aura:` line and pays nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct LastingAuraPlugin {
    /// What raising it is called.
    pub move_name: String,
    /// The aura, written in the scenario DSL's move grammar.
    pub effect: String,
    pub rounds: u32,
    pub bonus_action: bool,
    pub concentration: bool,
    pub kind: MoveKind,
    pub uses: Uses,
    pub slot: Option<u32>,
    pub resource: Option<(String, u32)>,
}

impl FeaturePlugin for LastingAuraPlugin {
    fn id(&self) -> &'static str {
        "lasting_aura"
    }

    fn name(&self) -> &str {
        &self.move_name
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let aura = grammar::parse_move_external(
            &format!("{} | {}", self.move_name, self.effect),
            &builder.creature,
        )
        .map_err(FeatureError::InvalidConfiguration)?;
        let which = builder.creature.add_lasting_aura(aura);
        if u8::try_from(which).is_err() {
            return Err(FeatureError::InvalidConfiguration(
                "a creature can hold at most 256 declared auras".to_string(),
            ));
        }

        let mut raise = Move::new(
            self.move_name.clone(),
            Effect::Aura {
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
        Ok(())
    }
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // A raised aura: `name`, the `effect` it washes over each enemy written
    // in the move grammar, how many `rounds` it lasts, and what putting it up
    // costs - a `slot`, a `resource`/`cost` pair, or a count of `uses`.
    registry.register("lasting_aura", |val| {
        let name = val.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
            FeatureError::InvalidConfiguration("lasting_aura needs a `name`".to_string())
        })?;
        let effect = val.get("effect").and_then(|v| v.as_str()).ok_or_else(|| {
            FeatureError::InvalidConfiguration(format!(
                "lasting_aura `{name}` needs an `effect` - what each enemy meets"
            ))
        })?;
        let uses = match val.get("uses").and_then(|v| v.as_integer()) {
            Some(n) if n > 0 => Uses::Limited(n as u32),
            Some(_) => {
                return Err(FeatureError::InvalidConfiguration(
                    "lasting_aura `uses` is at least one".to_string(),
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
            None | Some("spell") => MoveKind::Spell,
            Some("standard") => MoveKind::Standard,
            Some("item") | Some("magic_item") => MoveKind::MagicItem,
            Some("object") => MoveKind::ObjectUse,
            Some(other) => {
                return Err(FeatureError::InvalidConfiguration(format!(
                    "lasting_aura `kind` is spell, standard, item or object, got `{other}`"
                )))
            }
        };

        Ok(Box::new(LastingAuraPlugin {
            move_name: name.to_string(),
            effect: effect.to_string(),
            rounds: val
                .get("rounds")
                .and_then(|v| v.as_integer())
                .map_or(10, |n| n.max(1) as u32),
            bonus_action: val
                .get("bonus_action")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            concentration: val
                .get("concentration")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
            kind,
            uses,
            slot,
            resource,
        }))
    });
}
