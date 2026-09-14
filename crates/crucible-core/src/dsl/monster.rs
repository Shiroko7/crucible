//! Monster / Creature definition abstraction.
//!
//! Represents a monster or NPC combatant defined by its statblock, challenge rating (CR),
//! traits, actions, and legendary actions.

use serde::Deserialize;
use std::collections::HashMap;

use crate::dsl::plugin::{CreatureBuilder, FeatureError, FeatureRegistry, FeatureResult};
use crate::rules::combat::Reduction;
use crate::rules::creature::{Ability, Creature, DamageKind};

/// Structured representation of a Monster / NPC.
#[derive(Debug, Clone, Deserialize)]
pub struct MonsterDefinition {
    pub name: String,
    pub cr: Option<toml::Value>,
    pub size: Option<String>,
    pub creature_type: Option<String>,
    pub ac: i32,
    pub hp: i32,
    #[serde(default)]
    pub initiative: i32,
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
    #[serde(default)]
    pub traits: Vec<String>,
    #[serde(default)]
    pub features: Vec<toml::Value>,
    #[serde(default)]
    pub actions: Vec<super::config::MoveEntry>,
    #[serde(default)]
    pub bonus: Vec<super::config::MoveEntry>,
    #[serde(default)]
    pub legendary_uses: u32,
    #[serde(default)]
    pub legendary: Vec<super::config::MoveEntry>,
}

impl MonsterDefinition {
    /// Compiles this monster definition into a `Creature` combatant using the monadic plugin architecture.
    pub fn to_creature(&self, registry: &FeatureRegistry) -> FeatureResult<Creature> {
        let mut builder = CreatureBuilder::new(&self.name, self.ac, self.hp);
        builder.creature.initiative = self.initiative;
        builder.creature.legendary_uses = self.legendary_uses;
        if let Some(creature_type) = &self.creature_type {
            builder.set_creature_type(creature_type.clone());
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

        // Apply trait strings
        for trait_line in &self.traits {
            let rider = super::config::parse_trait_str(trait_line)?;
            builder.add_rider(rider);
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

        // Apply legendary actions
        for entry in &self.legendary {
            let m = super::config::parse_move_entry(entry, &builder.creature)?;
            builder.add_legendary_action(m);
        }

        builder.build()
    }
}
