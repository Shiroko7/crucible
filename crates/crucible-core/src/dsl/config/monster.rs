//! Monster / Creature definition abstraction.
//!
//! Represents a monster or NPC combatant defined by its statblock, challenge rating (CR),
//! traits, actions, and legendary actions.

use crate::creature::Creature;
use crate::features::{CreatureBuilder, FeatureError, FeatureRegistry, FeatureResult};
use crate::rules::{Ability, Condition, CreatureType, DamageKind, Reduction, Size};
use serde::Deserialize;
use std::collections::HashMap;

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
    pub resources: super::ResourcesConfig,
    #[serde(default)]
    pub spellcasting: Option<super::SpellcastingConfig>,
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
    pub traits: Vec<String>,
    #[serde(default)]
    pub features: Vec<toml::Value>,
    #[serde(default)]
    pub actions: Vec<super::MoveEntry>,
    #[serde(default)]
    pub bonus: Vec<super::MoveEntry>,
    #[serde(default)]
    pub legendary_uses: u32,
    #[serde(default)]
    pub legendary: Vec<super::MoveEntry>,
}

impl MonsterDefinition {
    /// Compiles this monster definition into a `Creature` combatant using the monadic plugin architecture.
    pub fn to_creature(&self, registry: &FeatureRegistry) -> FeatureResult<Creature> {
        let mut builder = CreatureBuilder::new(&self.name, self.ac, self.hp);
        builder.creature.initiative = self.initiative;
        builder.creature.legendary_uses = self.legendary_uses;
        // Apply the creature type - gates both a target-type-restricted
        // spell (Hold Person) and `BonusDamageVsCreatureType` (a slaying
        // weapon's bonus, a favoured-enemy bonus).
        if let Some(type_name) = &self.creature_type {
            let creature_type = CreatureType::parse(type_name)
                .ok_or_else(|| FeatureError::UnknownCreatureType(type_name.clone()))?;
            builder.set_creature_type(creature_type);
        }

        // Apply the size category - the gate Cunning Strike's Trip option
        // reads (a Huge or Gargantuan target cannot be tripped at all).
        // Left at `Size::default()` (Medium) for a statblock that never
        // declares one, same as `creature_type` staying `None`.
        if let Some(size_name) = &self.size {
            let size = Size::parse(size_name)
                .ok_or_else(|| FeatureError::UnknownSize(size_name.clone()))?;
            builder.set_size(size);
        }

        // Apply saves
        for (ability_name, &bonus) in &self.saves {
            let ability = Ability::parse(ability_name)
                .ok_or_else(|| FeatureError::UnknownAbility(ability_name.clone()))?;
            builder.set_save(ability, bonus);
        }

        // Apply resources, including any spell slot pools
        super::apply_resources(&mut builder, &self.resources)?;

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
        // bonus, resistance) - see `dsl::grammar::TraitEffect`.
        for trait_line in &self.traits {
            let effect = super::parse_trait_str(trait_line)?;
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
            let m = super::parse_move_entry(entry, &builder.creature)?;
            builder.add_action(m);
        }

        // Apply bonus actions
        for entry in &self.bonus {
            let m = super::parse_move_entry(entry, &builder.creature)?;
            builder.add_bonus_action(m);
        }

        // Apply legendary actions
        for entry in &self.legendary {
            let m = super::parse_move_entry(entry, &builder.creature)?;
            builder.add_legendary_action(m);
        }

        builder.build()
    }
}
