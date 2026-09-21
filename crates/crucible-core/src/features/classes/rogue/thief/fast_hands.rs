//! Fast Hands (2024 Thief 3).

use crate::creature::{Move, MoveKind};
use crate::dsl::scenario;
use crate::features::{
    CreatureBuilder, FeatureError, FeaturePlugin, FeatureRegistry, FeatureResult,
};

/// Fast Hands (2024 Rogue Thief 3): use a Bonus Action to take the Use an
/// Object action, or to activate a magic item that would otherwise cost the
/// Magic action - drinking a potion, retrieving a hidden blade, waving a
/// wand - freeing the Action for something else that turn.
///
/// The engine has no generic "Use an Object" or "Magic" action of its own
/// (see [`MoveKind`]); a creature's actual item-activation move is just
/// another [`Move`], declared wherever the rest of its actions are. This
/// plugin's whole job is to take that move and register it as a bonus
/// action too: `crate::sim::duel` already picks one move from `actions` and,
/// independently, one from `bonus_actions` each turn, so having the same
/// move available in both lists *is* the feature - no special-casing in the
/// turn loop required. Whatever resource or `Uses` budget the move is under
/// still gates it exactly once, whichever slot spends it.
///
/// Takes ownership of the `Move` itself rather than a name to look up; the
/// registry's `fast_hands` entry builds one from the scenario DSL, where the
/// `object` and `item` clauses tag it `ObjectUse` or `MagicItem`.
#[derive(Debug, Clone, PartialEq)]
pub struct FastHandsPlugin {
    pub item_move: Move,
}

impl FastHandsPlugin {
    pub fn new(item_move: Move) -> Self {
        Self { item_move }
    }
}

impl FeaturePlugin for FastHandsPlugin {
    fn id(&self) -> &'static str {
        "fast_hands"
    }

    fn name(&self) -> &str {
        "Fast Hands"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        match self.item_move.kind {
            MoveKind::ObjectUse | MoveKind::MagicItem => {
                builder.add_bonus_action(self.item_move.clone());
                Ok(())
            }
            MoveKind::Standard | MoveKind::Spell => {
                Err(FeatureError::InvalidConfiguration(format!(
                    "Fast Hands only promotes a Use an Object or magic item move to a bonus \
                 action; '{}' is tagged neither",
                    self.item_move.name
                )))
            }
        }
    }
}

/// A move written in the scenario DSL, parsed only when it is applied - it
/// may name a resource pool (`cost potions 1`), and pools are resolved
/// against the creature being built - then promoted to a bonus action by
/// [`FastHandsPlugin`].
#[derive(Debug, Clone)]
struct DeclaredFastHandsMove {
    name: String,
    effect: String,
    kind: Option<MoveKind>,
}

impl FeaturePlugin for DeclaredFastHandsMove {
    fn id(&self) -> &'static str {
        "fast_hands"
    }

    fn name(&self) -> &str {
        "Fast Hands"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let mut m = scenario::parse_move_external(
            &format!("{} | {}", self.name, self.effect),
            &builder.creature,
        )
        .map_err(FeatureError::InvalidConfiguration)?;
        if let Some(kind) = self.kind {
            m.kind = kind;
        }
        FastHandsPlugin::new(m).apply(builder)
    }
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // Fast Hands (2024 Thief 3): an object or magic-item move, written in
    // the scenario DSL, taken as a Bonus Action.
    registry.register("fast_hands", |val| {
        let name = val.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
            FeatureError::InvalidConfiguration(
                "fast_hands needs a `name` for the move it promotes".to_string(),
            )
        })?;
        let effect = val.get("effect").and_then(|v| v.as_str()).ok_or_else(|| {
            FeatureError::InvalidConfiguration(
                "fast_hands needs an `effect` (the move, in the scenario DSL)".to_string(),
            )
        })?;
        let kind = match val.get("kind").and_then(|v| v.as_str()) {
            None => None,
            Some("object") => Some(MoveKind::ObjectUse),
            Some("magic_item") | Some("item") => Some(MoveKind::MagicItem),
            Some(other) => {
                return Err(FeatureError::InvalidConfiguration(format!(
                    "fast_hands `kind` is `object` or `magic_item`, got `{other}`"
                )))
            }
        };
        Ok(Box::new(DeclaredFastHandsMove {
            name: name.to_string(),
            effect: effect.to_string(),
            kind,
        }))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::Effect;

    fn item_move(kind: MoveKind) -> Move {
        use crate::creature::Effect;
        Move::new("Potion of Healing", Effect::Sequence(Vec::new())).with_kind(kind)
    }

    #[test]
    fn fast_hands_registers_an_object_use_move_as_a_bonus_action() {
        let builder = CreatureBuilder::new("Thief", 15, 40);
        let built = builder
            .apply_feature(&FastHandsPlugin::new(item_move(MoveKind::ObjectUse)))
            .expect("fast hands applies to an object-use move")
            .build()
            .expect("builds");
        assert_eq!(built.bonus_actions.len(), 1);
        assert_eq!(built.bonus_actions[0].name, "Potion of Healing");
        // The Action slot is untouched - it stays free for something else.
        assert!(built.actions.is_empty());
    }

    #[test]
    fn fast_hands_registers_a_magic_item_move_as_a_bonus_action() {
        let builder = CreatureBuilder::new("Thief", 15, 40);
        let built = builder
            .apply_feature(&FastHandsPlugin::new(item_move(MoveKind::MagicItem)))
            .expect("fast hands applies to a magic item activation")
            .build()
            .expect("builds");
        assert_eq!(built.bonus_actions.len(), 1);
        assert_eq!(built.bonus_actions[0].name, "Potion of Healing");
    }

    #[test]
    fn fast_hands_rejects_a_plain_standard_move() {
        let builder = CreatureBuilder::new("Thief", 15, 40);
        let err = builder
            .apply_feature(&FastHandsPlugin::new(item_move(MoveKind::Standard)))
            .expect_err("a move not tagged ObjectUse or MagicItem must not be silently promoted");
        assert!(matches!(err, FeatureError::InvalidConfiguration(_)));
    }

    /// Casting a spell is not "Use an Object or a magic item" either - Fast
    /// Hands still has nothing to say about it.
    #[test]
    fn fast_hands_rejects_a_spell_move() {
        let builder = CreatureBuilder::new("Thief", 15, 40);
        let err = builder
            .apply_feature(&FastHandsPlugin::new(item_move(MoveKind::Spell)))
            .expect_err("a spell-tagged move must not be silently promoted either");
        assert!(matches!(err, FeatureError::InvalidConfiguration(_)));
    }

    #[test]
    fn fast_hands_leaves_an_already_declared_action_copy_alone() {
        // A creature can have the same move declared as its Action (the
        // baseline "Use an Object" everyone can already take) and, once Fast
        // Hands applies, also as a Bonus Action - both slots usable the same
        // turn, gated by whatever `Uses`/`Cost` budget the move itself
        // carries.
        let mut builder = CreatureBuilder::new("Thief", 15, 40);
        builder.add_action(item_move(MoveKind::ObjectUse));
        let built = builder
            .apply_feature(&FastHandsPlugin::new(item_move(MoveKind::ObjectUse)))
            .expect("fast hands applies")
            .build()
            .expect("builds");
        assert_eq!(built.actions.len(), 1);
        assert_eq!(built.bonus_actions.len(), 1);
    }

    #[test]
    fn fast_hands_promotes_a_declared_object_move_to_a_bonus_action() {
        let registry = FeatureRegistry::new();
        let params: toml::Value = toml::from_str(
            r#"
                plugin = "fast_hands"
                name = "Potion of Healing"
                effect = "object | cost potions 1 | heal 2d4+2"
            "#,
        )
        .unwrap();
        let plugin = registry.build_plugin("fast_hands", &params).unwrap();
        let mut builder = crate::features::CreatureBuilder::new("Thief", 15, 30);
        builder.ensure_resource("potions", 3);
        plugin.apply(&mut builder).unwrap();
        assert!(builder.creature.actions.is_empty());
        let potion = &builder.creature.bonus_actions[0];
        assert_eq!(potion.name, "Potion of Healing");
        assert_eq!(potion.kind, MoveKind::ObjectUse);
        assert!(matches!(potion.effect, Effect::Heal(_)));
        assert!(potion.cost.is_some());

        // A plain attack is neither an object nor a magic item.
        let attack: toml::Value = toml::from_str(
            r#"
                name = "Stab"
                effect = "hit +5 | 1d4+3 piercing"
            "#,
        )
        .unwrap();
        let plugin = registry.build_plugin("fast_hands", &attack).unwrap();
        assert!(matches!(
            plugin.apply(&mut builder),
            Err(FeatureError::InvalidConfiguration(_))
        ));
    }
}
