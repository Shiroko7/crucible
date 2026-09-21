//! A limited-use item that forces a save or suffer a debuff.

use crate::creature::{Effect, Move, MoveKind, SaveEffect, Uses};
use crate::dsl::grammar;
use crate::dsl::grammar::DurationSpec;
use crate::features::classes::rogue::thief::FastHandsPlugin;
use crate::features::{
    CreatureBuilder, FeatureError, FeaturePlugin, FeatureRegistry, FeatureResult,
};
use crate::rules::{Ability, Condition, Duration};

/// A limited-use item, activatable as a Bonus Action via Fast Hands, that
/// forces a saving throw on a single target and, on a failure, applies a
/// three-part debuff for `duration`:
///
/// - it cannot cast a spell or activate a magic item
///   ([`MoveKind::Spell`], [`MoveKind::MagicItem`]) - see
///   [`Condition::blocks_magic`];
/// - it has disadvantage on every saving throw it makes - see
///   [`crate::rules::Condition::disadvantage_on_saves`];
/// - any damage it deals, of any type, to anyone, is halved - see
///   [`Condition::halves_own_damage`].
///
/// All three live behind one [`Condition::Suppressed`], because a real item
/// of this shape applies them together, with one applier, one victim and one
/// duration - not as three separately-tracked effects.
///
/// Generic and parameterised rather than any one published item: any number
/// of "so many charges a day, save-or-suffer" trinkets share exactly this
/// shape, differing only in name, save ability/DC, charges per day and how
/// long the debuff lasts. Reuses [`Uses::Limited`] for the charge count
/// rather than a new resource mechanism - the same "N uses for the whole
/// fight" budget a breath weapon's limited charges already use - and reuses
/// [`FastHandsPlugin`] to make the activation a Bonus Action rather than
/// growing a second way to promote a move into that slot.
///
/// `duration` is typically [`Duration::ApplierTurn`] ("until the start of
/// your next turn", Stunning Strike's shape) or a [`Duration::SaveEndTurn`]
/// repeat ("for 1 minute, repeat the save"): both last through the victim's
/// own next turn, which is the point of a debuff meant to hinder it.
/// [`Duration::VictimTurn`] would clear at the very *start* of the victim's
/// next turn - before it has acted - and is almost never what a debuff like
/// this one wants; see [`Duration`]'s own docs for why the three shapes
/// differ.
///
/// `dc` is either a fixed number printed on the item, or `None` for an item
/// whose save is "against your spell save DC" - read off the wielder's own
/// [`crate::rules::SpellCastingProfile`] when the plugin applies.
#[derive(Debug, Clone, PartialEq)]
pub struct LimitedUseDebuffItemPlugin {
    pub name: String,
    pub uses_per_day: u32,
    pub ability: Ability,
    pub dc: Option<i32>,
    pub duration: Duration,
}

impl LimitedUseDebuffItemPlugin {
    pub fn new(
        name: impl Into<String>,
        uses_per_day: u32,
        ability: Ability,
        dc: i32,
        duration: Duration,
    ) -> Self {
        Self {
            name: name.into(),
            uses_per_day,
            ability,
            dc: Some(dc),
            duration,
        }
    }

    /// As [`LimitedUseDebuffItemPlugin::new`], for an item whose save is
    /// against the wielder's own spell save DC rather than a printed number.
    pub fn against_spell_dc(
        name: impl Into<String>,
        uses_per_day: u32,
        ability: Ability,
        duration: Duration,
    ) -> Self {
        Self {
            name: name.into(),
            uses_per_day,
            ability,
            dc: None,
            duration,
        }
    }

    /// The `Move` this plugin registers: a single-target save with no direct
    /// damage of its own, applying [`Condition::Suppressed`] on a failure.
    /// `max_targets: Some(1)` is how a single-target effect is expressed in
    /// this engine's absence of a positioning model (see `DESIGN.md`'s
    /// "Positioning is the gap that matters") - the same reading a
    /// second-level Command's `Some(2)` already uses.
    fn item_move(&self, dc: i32) -> Move {
        Move::new(
            self.name.clone(),
            Effect::Save(SaveEffect {
                ability: self.ability,
                dc,
                damage: Vec::new(),
                half_on_success: false,
                on_failure: vec![(Condition::Suppressed, self.duration)],
                max_targets: Some(1),
                requires_type: None,
            }),
        )
        .with_uses(Uses::Limited(self.uses_per_day))
        .with_kind(MoveKind::MagicItem)
    }
}

impl FeaturePlugin for LimitedUseDebuffItemPlugin {
    fn id(&self) -> &'static str {
        "limited_use_debuff_item"
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let dc = match self.dc {
            Some(dc) => dc,
            None => builder.creature.spell_save_dc().ok_or_else(|| {
                FeatureError::InvalidConfiguration(format!(
                    "{}'s save is against its wielder's spell save DC, so {} needs a \
                     spellcasting profile declared first",
                    self.name, builder.creature.name
                ))
            })?,
        };
        FastHandsPlugin::new(self.item_move(dc)).apply(builder)
    }
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // A limited-use item that forces a save or a debuff, used through
    // Fast Hands (ITM-05). `dc` left off means "against your spell save
    // DC"; `duration` is written the way the scenario DSL writes one.
    registry.register("limited_use_debuff_item", |val| {
        let name = val.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
            FeatureError::InvalidConfiguration("limited_use_debuff_item needs a `name`".to_string())
        })?;
        let uses = val.get("uses").and_then(|v| v.as_integer()).unwrap_or(1) as u32;
        let ability_str = val.get("ability").and_then(|v| v.as_str()).ok_or_else(|| {
            FeatureError::InvalidConfiguration(
                "limited_use_debuff_item needs the `ability` its save uses".to_string(),
            )
        })?;
        let ability = Ability::parse(ability_str)
            .ok_or_else(|| FeatureError::UnknownAbility(ability_str.to_string()))?;
        let dc = val.get("dc").and_then(|v| v.as_integer()).map(|d| d as i32);
        let phrase = val
            .get("duration")
            .and_then(|v| v.as_str())
            .unwrap_or("until applier");
        let words: Vec<&str> = phrase.split_whitespace().collect();
        let (spec, used) = grammar::parse_duration(&words, 0, phrase)
            .map_err(FeatureError::InvalidConfiguration)?;
        if used != words.len() {
            return Err(FeatureError::InvalidConfiguration(format!(
                "`{phrase}` is not a duration"
            )));
        }
        let duration = match (spec, dc) {
            (DurationSpec::Fixed(d), _) => d,
            (DurationSpec::UntilSave, Some(dc)) => {
                crate::rules::Duration::SaveEndTurn { ability, dc }
            }
            (DurationSpec::UntilSave, None) => {
                return Err(FeatureError::InvalidConfiguration(format!(
                    "{name}: `until save` needs a fixed `dc` to repeat"
                )))
            }
        };
        Ok(Box::new(match dc {
            Some(dc) => LimitedUseDebuffItemPlugin::new(name, uses, ability, dc, duration),
            None => LimitedUseDebuffItemPlugin::against_spell_dc(name, uses, ability, duration),
        }))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::SpellCastingProfile;

    fn plugin() -> LimitedUseDebuffItemPlugin {
        LimitedUseDebuffItemPlugin::new("Test Trinket", 1, Ability::Wis, 15, Duration::ApplierTurn)
    }

    #[test]
    fn registers_a_single_use_magic_item_move_as_a_bonus_action() {
        let built = CreatureBuilder::new("Rogue", 15, 40)
            .apply_feature(&plugin())
            .expect("the item applies via Fast Hands")
            .build()
            .expect("builds");

        assert!(
            built.actions.is_empty(),
            "only promoted to a bonus action, not also declared as an Action"
        );
        assert_eq!(built.bonus_actions.len(), 1);
        let m = &built.bonus_actions[0];
        assert_eq!(m.name, "Test Trinket");
        assert_eq!(m.uses, Uses::Limited(1));
        assert_eq!(m.kind, MoveKind::MagicItem);
        match &m.effect {
            Effect::Save(save) => {
                assert_eq!((save.ability, save.dc), (Ability::Wis, 15));
                assert!(
                    save.damage.is_empty(),
                    "the item deals no direct damage itself"
                );
                assert!(!save.half_on_success);
                assert_eq!(save.max_targets, Some(1));
                assert_eq!(
                    save.on_failure,
                    vec![(Condition::Suppressed, Duration::ApplierTurn)]
                );
            }
            other => panic!("expected a save effect, got {other:?}"),
        }
    }

    /// "Against your spell save DC" reads the wielder's own profile, and a
    /// wielder without one is a configuration error rather than a DC of 0.
    #[test]
    fn a_spell_dc_item_reads_the_wielders_own_save_dc() {
        use crate::rules::SpellCastingProfile;
        let item = LimitedUseDebuffItemPlugin::against_spell_dc(
            "Test Card",
            1,
            Ability::Wis,
            Duration::Rounds(10),
        );
        let mut caster = CreatureBuilder::new("Caster", 15, 40);
        caster.set_spellcasting(SpellCastingProfile::new(Ability::Wis, 5, 4).with_item_bonus(2));
        let built = caster.apply_feature(&item).unwrap().build().unwrap();
        let Effect::Save(save) = &built.bonus_actions[0].effect else {
            panic!("expected a save effect");
        };
        assert_eq!(save.dc, 19);

        let mut not_a_caster = CreatureBuilder::new("Fighter", 15, 40);
        assert!(matches!(
            item.apply(&mut not_a_caster),
            Err(FeatureError::InvalidConfiguration(_))
        ));
    }

    /// The whole point of a plugin over a hardcoded item: name, charges,
    /// save ability/DC and duration are all parameters.
    #[test]
    fn charges_ability_dc_and_duration_are_parameters_not_constants() {
        let custom = LimitedUseDebuffItemPlugin::new(
            "Another Trinket",
            3,
            Ability::Con,
            20,
            Duration::SaveEndTurn {
                ability: Ability::Con,
                dc: 20,
            },
        );
        let built = CreatureBuilder::new("Warlock", 14, 30)
            .apply_feature(&custom)
            .expect("applies")
            .build()
            .expect("builds");
        assert_eq!(built.bonus_actions[0].uses, Uses::Limited(3));
        match &built.bonus_actions[0].effect {
            Effect::Save(save) => {
                assert_eq!((save.ability, save.dc), (Ability::Con, 20));
                assert_eq!(
                    save.on_failure,
                    vec![(
                        Condition::Suppressed,
                        Duration::SaveEndTurn {
                            ability: Ability::Con,
                            dc: 20
                        }
                    )]
                );
            }
            other => panic!("expected a save effect, got {other:?}"),
        }
    }

    fn spellcaster() -> CreatureBuilder {
        let mut builder = CreatureBuilder::new("Wizard", 12, 30);
        builder.set_spellcasting(SpellCastingProfile::new(Ability::Int, 4, 3));
        builder
    }

    #[test]
    fn limited_use_debuff_item_builds_from_toml() {
        let registry = FeatureRegistry::new();
        let params: toml::Value = toml::from_str(
            r#"
                plugin = "limited_use_debuff_item"
                name = "Test Card"
                ability = "wis"
                duration = "for 1 minute"
            "#,
        )
        .unwrap();
        let plugin = registry
            .build_plugin("limited_use_debuff_item", &params)
            .unwrap();
        let mut builder = spellcaster();
        plugin.apply(&mut builder).unwrap();
        let item = &builder.creature.bonus_actions[0];
        assert_eq!(item.kind, MoveKind::MagicItem);
        let Effect::Save(save) = &item.effect else {
            panic!("expected a save");
        };
        assert_eq!(save.dc, 15, "the caster's own spell save DC");
        assert_eq!(
            save.on_failure,
            vec![(Condition::Suppressed, Duration::Rounds(10))]
        );

        let repeat_without_dc: toml::Value = toml::from_str(
            r#"
                name = "Test Card"
                ability = "wis"
                duration = "until save"
            "#,
        )
        .unwrap();
        assert!(matches!(
            registry.build_plugin("limited_use_debuff_item", &repeat_without_dc),
            Err(FeatureError::InvalidConfiguration(_))
        ));
    }
}
