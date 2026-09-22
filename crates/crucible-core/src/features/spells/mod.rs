//! Spell feature plugins: official SRD 5.2 spells as `FeaturePlugin`s, one
//! file per spell.
//!
//! Each spell here is a `Move` built the same way any other feature builds
//! one, rather than new engine branches. The one thing genuinely new to the
//! engine is [`crate::creature::Effect::AutoHit`], for Magic Missile's "no
//! attack roll, no save" damage; Blindness/Deafness and Command both fit
//! entirely inside the existing `Effect::Save` / `Condition` / `Duration`
//! machinery ARCH-01 and ARCH-05 already built.
//!
//! Every move a spell plugin registers is tagged
//! [`crate::creature::MoveKind::Spell`], so a condition that stops a creature
//! casting ([`crate::rules::Condition::blocks_magic`]) stops these too.
//!
//! ## What pays for a cast
//!
//! A slotted spell spends from the caster's own
//! [`crate::rules::SpellSlots`] via
//! [`crate::creature::Move::with_spell_slot`] - one pool per level,
//! shared by every spell of that level, which `sim::fight` spends from and
//! refuses to overdraw. The spells a build might instead power from
//! somewhere else - Blindness/Deafness, Command and Magic Missile, which a
//! wand or a once-a-rest feature can cast as easily as a slot can - take
//! [`SpellCost`] as data: either a slot level, or a named resource pool
//! (`wand_charges`, a feature's own uses) resolved against whatever the
//! caster's config already declared, or nothing at all for an at-will cast.
//!
//! Reviving a creature at 0 HP is not spell-specific text - it is 5e's
//! general "a creature that regains any hit points while it has 0 becomes
//! conscious" rule - so it is implemented once, in
//! [`crate::rules::apply_healing`], and inherited by both healing
//! spells (and anything else that ever heals) rather than re-implemented per
//! spell. `sim::fight` aims a heal at the caster's own side: a downed ally
//! first, otherwise whoever is missing the most hit points.
//!
//! Not modelled, on purpose:
//! - **Range.** `DESIGN.md` already rules positioning out of scope entirely
//!   ("Positioning is the gap that matters") - there is no notion of distance
//!   for a melee weapon either, so a spell's range in feet is flavour text
//!   here, not a mechanic.
//! - **Upcasting.** Every spell here is implemented at its base cast only;
//!   scaling with a higher slot is skipped.

mod bane;
mod bless;
mod blindness_deafness;
mod command;
mod cure_wounds;
mod guiding_bolt;
mod healing_word;
mod hold_person;
mod hunters_mark;
mod magic_missile;
mod spiritual_weapon;
mod true_strike;

use crate::creature::{Cost, Move};
use crate::features::{CreatureBuilder, FeatureError, FeatureRegistry, FeatureResult};
use crate::rules::SPELL_LEVELS;
pub use bane::BanePlugin;
pub use bless::BlessPlugin;
pub use blindness_deafness::BlindnessDeafnessPlugin;
pub use command::{CommandPlugin, CommandWord};
pub use cure_wounds::CureWoundsPlugin;
pub use guiding_bolt::GuidingBoltPlugin;
pub use healing_word::HealingWordPlugin;
pub use hold_person::HoldPersonPlugin;
pub use hunters_mark::HuntersMarkPlugin;
pub use magic_missile::MagicMissilePlugin;
pub use spiritual_weapon::SpiritualWeaponPlugin;
pub use true_strike::TrueStrikePlugin;

/// Both healing spells are cast here at their base, 1st-level, rate. See the
/// module doc: upcasting is out of scope.
pub(super) const BASE_SLOT_LEVEL: u32 = 1;

/// The ability modifier a healing spell adds, read off the creature's own
/// casting profile - never a hardcoded number.
pub(super) fn spellcasting_ability_modifier(
    builder: &CreatureBuilder,
    spell_name: &str,
) -> FeatureResult<i32> {
    builder
        .creature
        .spellcasting
        .map(|profile| profile.ability_modifier)
        .ok_or_else(|| {
            FeatureError::InvalidConfiguration(format!(
                "{spell_name} needs a [*.spellcasting] profile to compute its healing"
            ))
        })
}

/// How a spell's use is paid for, when a build might power it from more than
/// one place: a spell slot of some level, or `amount` from a named resource
/// pool - a wand's charges, a once-a-rest feature's single use. Leaving the
/// cost off entirely is an at-will cast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpellCost {
    /// One spell slot of this level, from the caster's own
    /// [`crate::rules::SpellSlots`].
    Slot(u32),
    /// `amount` from the resource pool named `resource_name`, resolved
    /// against whatever the caster's config declared.
    Resource { resource_name: String, amount: u32 },
}

impl SpellCost {
    /// `amount` from the named resource pool - see [`SpellCost::Resource`].
    pub fn new(resource_name: impl Into<String>, amount: u32) -> Self {
        Self::Resource {
            resource_name: resource_name.into(),
            amount,
        }
    }

    /// Put this cost on `mv`, resolving a pool name against `builder`'s
    /// already-declared resources.
    fn charge(&self, builder: &CreatureBuilder, mv: Move) -> FeatureResult<Move> {
        Ok(match self {
            SpellCost::Slot(level) => mv.with_spell_slot(*level),
            SpellCost::Resource {
                resource_name,
                amount,
            } => mv.with_cost(Cost {
                resource: builder.resource_index(resource_name)?,
                amount: *amount,
            }),
        })
    }
}

/// Put an optional [`SpellCost`] on `mv` - the last step every plugin below
/// takes before registering its move.
pub(super) fn charge(
    builder: &CreatureBuilder,
    cost: &Option<SpellCost>,
    mv: Move,
) -> FeatureResult<Move> {
    match cost {
        Some(c) => c.charge(builder, mv),
        None => Ok(mv),
    }
}

/// A caster's save DC, or a configuration error naming which spell needed
/// one. Every spell below targets a save, so every one of them calls this.
pub(super) fn save_dc(builder: &CreatureBuilder, spell_name: &str) -> FeatureResult<i32> {
    builder.creature.spell_save_dc().ok_or_else(|| {
        FeatureError::InvalidConfiguration(format!(
            "{spell_name} needs a `[*.spellcasting]` profile to compute its save DC"
        ))
    })
}

/// Read a spell plugin's optional cost into a [`SpellCost`]: `slot = N`
/// spends a spell slot of that level, while `resource = "<pool>"` (with an
/// optional `cost` amount, default 1) spends from a pool the caster already
/// declared under `[*.resources]` - a wand's charges, a feature's own uses.
/// Leaving both off makes the cast free.
///
/// Shared by every spell factory below so "how it's paid for" stays a
/// declared parameter rather than a name a plugin invents itself.
pub(super) fn parse_spell_cost(val: &toml::Value) -> FeatureResult<Option<SpellCost>> {
    if let Some(level) = val.get("slot").and_then(|v| v.as_integer()) {
        if !(1..=i64::from(SPELL_LEVELS)).contains(&level) {
            return Err(FeatureError::InvalidConfiguration(format!(
                "a spell slot is level 1 to 9, got `slot = {level}`"
            )));
        }
        return Ok(Some(SpellCost::Slot(level as u32)));
    }
    let Some(resource) = val.get("resource").and_then(|v| v.as_str()) else {
        return Ok(None);
    };
    let amount = val.get("cost").and_then(|v| v.as_integer()).unwrap_or(1) as u32;
    Ok(Some(SpellCost::new(resource, amount)))
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    bane::register(registry);
    bless::register(registry);
    blindness_deafness::register(registry);
    command::register(registry);
    cure_wounds::register(registry);
    guiding_bolt::register(registry);
    healing_word::register(registry);
    hold_person::register(registry);
    hunters_mark::register(registry);
    magic_missile::register(registry);
    spiritual_weapon::register(registry);
    true_strike::register(registry);
}

#[cfg(test)]
mod tests {
    use crate::creature::MoveKind;
    use crate::features::{CreatureBuilder, FeatureError, FeatureRegistry};
    use crate::rules::{Ability, SpellCastingProfile};

    fn spellcaster() -> CreatureBuilder {
        let mut builder = CreatureBuilder::new("Wizard", 12, 30);
        builder.set_spellcasting(SpellCastingProfile::new(Ability::Int, 4, 3));
        builder
    }

    /// `slot = N` spends from the caster's real slot pool - the same one
    /// Bless and Healing Word draw on - rather than a named resource.
    #[test]
    fn a_spell_cost_can_be_a_real_spell_slot() {
        let registry = FeatureRegistry::new();
        let mut builder = spellcaster();
        let params: toml::Value = toml::from_str("plugin = \"command\"\nslot = 1").unwrap();
        registry
            .build_plugin("command", &params)
            .unwrap()
            .apply(&mut builder)
            .unwrap();
        let m = &builder.creature.actions[0];
        assert_eq!(m.spell_slot_level, Some(1));
        assert_eq!(m.cost, None);
        assert_eq!(m.kind, MoveKind::Spell);

        let bad: toml::Value = toml::from_str("plugin = \"command\"\nslot = 10").unwrap();
        assert!(matches!(
            registry.build_plugin("command", &bad),
            Err(FeatureError::InvalidConfiguration(_))
        ));
    }
}
