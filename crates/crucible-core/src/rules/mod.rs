//! 5e's core rules: the vocabulary a stat block is written in, and how a d20
//! roll, an attack, a saving throw, a check and a heal resolve - exactly and
//! by sampling.
//!
//! Nothing here is named after a class, spell or item; those are
//! [`crate::features`], built out of these pieces.

mod ability;
mod attack;
mod check;
mod condition;
mod creature_type;
mod damage;
mod healing;
mod roll_mode;
mod save;
mod size;
mod spellcasting;

pub use ability::Ability;
pub use attack::{
    damage_pmf, hit_outcomes, hit_outcomes_with, hit_outcomes_with_reaction, outcomes,
    resolve_mode, sample_attacks_to_kill, sample_damage, sample_hit, sample_hit_with,
    sample_hit_with_reaction, Attack, AttackModifier, DamageRider, Defense, Landed, Outcomes,
};
pub use check::CheckRoll;
pub use condition::{Condition, Duration};
pub use creature_type::CreatureType;
pub use damage::{DamageKind, DamageRoll, Reduction};
pub use healing::{apply_healing, is_down, HealRoll};
pub use roll_mode::RollMode;
pub(crate) use save::{probability_at_least, sample_save_modifier_bonus};
pub use save::{
    sample_save_with, save_probability_with_mode, save_success_chance, save_with_mode, SaveModifier,
};
pub use size::Size;
pub use spellcasting::{SpellCastingProfile, SpellSlots, SPELL_LEVELS};
