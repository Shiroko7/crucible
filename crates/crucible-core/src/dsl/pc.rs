//! Player Character (PC) abstraction.
//!
//! Represents a player character composed of class, level, ability scores,
//! equipment, resources, and pluggable features.

use crate::creature::Creature;
use crate::dsl::plugin::{CreatureBuilder, FeatureError, FeatureRegistry, FeatureResult};
use crate::rules::{Ability, Condition, DamageKind, Reduction};
use serde::Deserialize;
use std::collections::HashMap;

/// Structured representation of a Player Character (PC).
#[derive(Debug, Clone, Deserialize)]
pub struct PlayerCharacter {
    pub name: String,
    #[serde(default)]
    pub level: u32,
    #[serde(default)]
    pub class: String,
    pub subclass: Option<String>,
    pub species: Option<String>,
    pub ac: i32,
    pub hp: i32,
    #[serde(default)]
    pub initiative: i32,
    #[serde(default)]
    pub abilities: HashMap<String, i32>,
    #[serde(default)]
    pub saves: HashMap<String, i32>,
    #[serde(default)]
    pub resources: super::config::ResourcesConfig,
    #[serde(default)]
    pub spellcasting: Option<super::config::SpellcastingConfig>,
    #[serde(default)]
    pub resist: Vec<String>,
    #[serde(default)]
    pub immune: Vec<String>,
    #[serde(default)]
    pub vulnerable: Vec<String>,
    /// Conditions this creature cannot be given at all - see
    /// [`crate::creature::Creature::condition_immunities`].
    #[serde(default)]
    pub condition_immune: Vec<String>,
    #[serde(default)]
    pub equipment: Vec<String>,
    #[serde(default)]
    pub traits: Vec<String>,
    #[serde(default)]
    pub features: Vec<toml::Value>,
    #[serde(default)]
    pub actions: Vec<super::config::MoveEntry>,
    #[serde(default)]
    pub bonus: Vec<super::config::MoveEntry>,
}

impl PlayerCharacter {
    /// Compiles this PC definition into a `Creature` combatant using the monadic plugin architecture.
    pub fn to_creature(&self, registry: &FeatureRegistry) -> FeatureResult<Creature> {
        let mut builder = CreatureBuilder::new(&self.name, self.ac, self.hp);
        builder.creature.initiative = self.initiative;
        builder.creature.player_character = true;

        // Apply raw ability scores, ahead of `features` below - a
        // prerequisite-gated feature (a prestige spellcasting grant, say)
        // needs these already in place when it runs.
        for (ability_name, &score) in &self.abilities {
            let ability = Ability::parse(ability_name)
                .ok_or_else(|| FeatureError::UnknownAbility(ability_name.clone()))?;
            builder.set_ability_score(ability, score);
        }

        // Apply saves
        for (ability_name, &bonus) in &self.saves {
            let ability = Ability::parse(ability_name)
                .ok_or_else(|| FeatureError::UnknownAbility(ability_name.clone()))?;
            builder.set_save(ability, bonus);
        }

        // Apply resources, including any spell slot pools
        super::config::apply_resources(&mut builder, &self.resources)?;

        // Apply the spellcasting profile (spell attack bonus / save DC)
        if let Some(spellcasting) = &self.spellcasting {
            builder.set_spellcasting(spellcasting.to_profile()?);
        }

        // Apply damage reductions
        for kind_name in &self.resist {
            let kind = DamageKind::parse(kind_name)
                .ok_or_else(|| FeatureError::UnknownDamageKind(kind_name.clone()))?;
            builder.add_reduction(kind, Reduction::Resistant);
        }
        for kind_name in &self.immune {
            let kind = DamageKind::parse(kind_name)
                .ok_or_else(|| FeatureError::UnknownDamageKind(kind_name.clone()))?;
            builder.add_reduction(kind, Reduction::Immune);
        }
        for kind_name in &self.vulnerable {
            let kind = DamageKind::parse(kind_name)
                .ok_or_else(|| FeatureError::UnknownDamageKind(kind_name.clone()))?;
            builder.add_reduction(kind, Reduction::Vulnerable);
        }
        for condition_name in &self.condition_immune {
            let condition = Condition::parse(condition_name)
                .ok_or_else(|| FeatureError::UnknownCondition(condition_name.clone()))?;
            builder.creature.condition_immunities.push(condition);
        }

        // Apply trait strings: each is a Rider, or a flat passive stat bonus
        // applied straight onto the creature (AC, saves, spellcasting item
        // bonus, resistance) - see `dsl::scenario::TraitEffect`.
        for trait_line in &self.traits {
            let effect = super::config::parse_trait_str(trait_line)?;
            effect
                .apply(&mut builder.creature)
                .map_err(FeatureError::InvalidConfiguration)?;
        }

        // Apply registered feature plugins via monadic bind
        for feat_val in &self.features {
            if let Some(plugin_id) = feat_val.get("plugin").and_then(|v| v.as_str()) {
                let plugin = registry.build_plugin(plugin_id, feat_val)?;
                builder = builder.apply_feature(plugin.as_ref())?;
            }
        }

        // Apply actions
        for entry in &self.actions {
            let m = super::config::parse_move_entry(entry, &builder.creature)?;
            builder.add_action(m);
        }

        // Apply bonus actions
        for entry in &self.bonus {
            let m = super::config::parse_move_entry(entry, &builder.creature)?;
            builder.add_bonus_action(m);
        }

        builder.build()
    }
}
