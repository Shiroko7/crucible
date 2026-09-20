//! Traits and types for the Monad Plugin Architecture.
//!
//! A feature is a pluggable transformation over a combatant builder.
//! The agent only implements new or missing features as plugins; PCs and
//! creatures are data-driven configurations composed of these plugins.

use crate::rules::combat::Reduction;
use crate::rules::creature::{
    Ability, Creature, CreatureType, DamageKind, Move, Resource, Rider, Size, SpellCastingProfile,
};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeatureError {
    MissingResource(String),
    UnknownCondition(String),
    UnknownAbility(String),
    UnknownDamageKind(String),
    UnknownCreatureType(String),
    UnknownSize(String),
    InvalidConfiguration(String),
    ExecutionError(String),
    /// A feature with entry prerequisites (a prestige-style grant, typically)
    /// refused to apply because the creature does not meet them. Distinct
    /// from `InvalidConfiguration`, which is a malformed plugin declaration -
    /// this is a well-formed feature that correctly does not apply here.
    PrerequisiteNotMet(String),
}

impl fmt::Display for FeatureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingResource(r) => write!(f, "resource '{r}' was referenced but not declared"),
            Self::UnknownCondition(c) => write!(f, "unknown condition '{c}'"),
            Self::UnknownAbility(a) => write!(f, "unknown ability '{a}'"),
            Self::UnknownDamageKind(d) => write!(f, "unknown damage kind '{d}'"),
            Self::UnknownCreatureType(t) => write!(f, "unknown creature type '{t}'"),
            Self::UnknownSize(s) => write!(f, "unknown size '{s}'"),
            Self::InvalidConfiguration(msg) => write!(f, "invalid configuration: {msg}"),
            Self::ExecutionError(msg) => write!(f, "feature execution error: {msg}"),
            Self::PrerequisiteNotMet(msg) => write!(f, "prerequisite not met: {msg}"),
        }
    }
}

impl std::error::Error for FeatureError {}

pub type FeatureResult<T> = Result<T, FeatureError>;

/// A builder wrapping a `Creature` for monadic transformations.
///
/// In functional programming, `State<S, A>` models state transformations `S -> (S, A)`.
/// Here, each feature plugin is a state transformer that binds to this builder,
/// validating prerequisites, modifying combat statistics, and adding moves/riders.
#[derive(Debug, Clone)]
pub struct CreatureBuilder {
    pub creature: Creature,
    pub features_applied: Vec<String>,
}

impl CreatureBuilder {
    pub fn new(name: impl Into<String>, ac: i32, hp: i32) -> Self {
        Self {
            creature: Creature::new(name, ac, hp),
            features_applied: Vec::new(),
        }
    }

    /// Monadic bind: transforms the builder with a feature plugin.
    pub fn apply_feature(mut self, feature: &dyn FeaturePlugin) -> FeatureResult<Self> {
        feature.apply(&mut self)?;
        self.features_applied.push(feature.name().to_string());
        Ok(self)
    }

    /// Monadic sequence / foldM: applies an iterator of feature plugins sequentially.
    pub fn apply_features<'a>(
        self,
        features: impl IntoIterator<Item = &'a Box<dyn FeaturePlugin>>,
    ) -> FeatureResult<Self> {
        features
            .into_iter()
            .try_fold(self, |builder, feat| builder.apply_feature(feat.as_ref()))
    }

    /// Declare a resource pool if not already present.
    pub fn ensure_resource(&mut self, name: impl Into<String>, max: u32) -> usize {
        let name_str = name.into();
        if let Some(idx) = self.creature.resource_index(&name_str) {
            return idx;
        }
        self.creature.resources.push(Resource {
            name: name_str,
            max,
        });
        self.creature.resources.len() - 1
    }

    /// Get index of a declared resource.
    pub fn resource_index(&self, name: &str) -> FeatureResult<usize> {
        self.creature
            .resource_index(name)
            .ok_or_else(|| FeatureError::MissingResource(name.to_string()))
    }

    /// Add a regular action.
    pub fn add_action(&mut self, action: Move) {
        self.creature.actions.push(action);
    }

    /// Add a bonus action.
    pub fn add_bonus_action(&mut self, action: Move) {
        self.creature.bonus_actions.push(action);
    }

    /// Add a legendary action.
    pub fn add_legendary_action(&mut self, action: Move) {
        self.creature.legendary.push(action);
    }

    /// Add a passive trait or rider.
    pub fn add_rider(&mut self, rider: Rider) {
        self.creature.riders.push(rider);
    }

    /// Add damage reduction (resistance, immunity, vulnerability).
    pub fn add_reduction(&mut self, kind: DamageKind, reduction: Reduction) {
        self.creature.reductions.push((kind, reduction));
    }

    /// Set a saving throw bonus for an ability.
    pub fn set_save(&mut self, ability: Ability, bonus: i32) {
        self.creature.saves[ability.index()] = bonus;
    }

    /// Set the floor Reliable Talent (or anything shaped like it) puts under
    /// a proficient ability check's raw d20 roll; see
    /// [`crate::rules::creature::Creature::check_floor`].
    pub fn set_reliable_talent_floor(&mut self, floor: i32) {
        self.creature.reliable_talent_floor = Some(floor);
    }

    /// Declare this creature's 5e type - Humanoid, Dragon, ... Gates both
    /// spells with a target-type restriction (Hold Person) and
    /// [`crate::rules::creature::Rider::BonusDamageVsCreatureType`].
    pub fn set_creature_type(&mut self, creature_type: CreatureType) {
        self.creature.creature_type = Some(creature_type);
    }

    /// Set this creature's 5e size category.
    pub fn set_size(&mut self, size: Size) {
        self.creature.size = size;
    }

    /// Set a raw ability score, as opposed to [`CreatureBuilder::set_save`]'s
    /// bonus.
    pub fn set_ability_score(&mut self, ability: Ability, score: i32) {
        self.creature.abilities[ability.index()] = score;
    }

    /// Declare a spell slot pool's maximum (and starting available count) at
    /// `level`, 1st through 9th.
    pub fn set_spell_slot_max(&mut self, level: u32, max: u32) {
        self.creature.spell_slots.set_max(level, max);
    }

    /// Set how this creature's spell attacks and save DCs are computed.
    pub fn set_spellcasting(&mut self, profile: SpellCastingProfile) {
        self.creature.spellcasting = Some(profile);
    }

    /// Finalize and validate the combatant.
    pub fn build(self) -> FeatureResult<Creature> {
        if self.creature.hp <= 0 {
            return Err(FeatureError::InvalidConfiguration(format!(
                "creature '{}' must have positive HP, got {}",
                self.creature.name, self.creature.hp
            )));
        }
        Ok(self.creature)
    }
}

/// The feature plugin trait.
///
/// Features are modular plugins rather than hardcoded logic branches.
/// The agent only creates new `FeaturePlugin` implementations when new mechanics
/// are encountered.
pub trait FeaturePlugin: Send + Sync {
    /// Internal identifier for this feature (e.g., "evasion", "deflect_attacks", "legendary_resistance").
    fn id(&self) -> &'static str;

    /// Descriptive name of the specific feature instance (e.g., "Evasion", "Staff x2", "Fire Breath").
    fn name(&self) -> &str;

    /// Transform the creature builder.
    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()>;
}
