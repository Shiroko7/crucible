//! The Creature combatant model.

use crate::rules::combat::Reduction;

use super::action::Move;
use super::damage::DamageKind;
use super::rider::Rider;
use super::types::{Ability, CreatureType, Resource, Size, SpellCastingProfile, SpellSlots};

/// One side of a fight.
///
/// `actions`, `bonus_actions` and `legendary` are ordered, and that order is
/// itself the simplest policy: take the first usable one. See
/// [`crate::sim::duel::Policy`].
#[derive(Debug, Clone, PartialEq)]
pub struct Creature {
    pub name: String,
    /// 0 or 1. Read by [`crate::sim::duel::run_teams`]; the one-against-one [`crate::sim::duel::run`]
    /// ignores it and uses argument order instead.
    pub team: u8,
    /// How many copies of this creature are in the fight. The loader expands it;
    /// nothing downstream sees a count greater than one.
    pub count: u32,
    pub ac: i32,
    pub hp: i32,
    /// The statblock's type line - Dragon, Giant, Undead - or `None` for
    /// anything that never stated one: every player character, and any
    /// monster whose config omitted it. Gates both spells whose targeting
    /// rules name a creature type (Hold Person, Charm Person; see
    /// [`Creature::is_creature_type`]) and
    /// [`Rider::BonusDamageVsCreatureType`] - a target with no declared type
    /// never matches either.
    pub creature_type: Option<CreatureType>,
    pub initiative: i32,
    /// This creature's 5e size category. Defaults to Medium - see
    /// [`Size`]'s `Default` impl - for a statblock that never declares one,
    /// the same "silent common case" default weapon and monster stats
    /// already get elsewhere. The gate for Cunning Strike's Trip option:
    /// [`Rider::resolve_cunning_strike_trip`].
    pub size: Size,
    pub saves: [i32; 6],
    pub reductions: Vec<(DamageKind, Reduction)>,
    pub resources: Vec<Resource>,
    /// This creature's spell slot pools, 1st through 9th level. Zeroed out -
    /// and therefore free to ignore - for anything that does not cast spells.
    pub spell_slots: SpellSlots,
    /// How this creature's spell attacks and save DCs are computed. `None`
    /// for a non-caster; distinct from `resources` and from the physical
    /// attack bonuses baked into its `actions`.
    pub spellcasting: Option<SpellCastingProfile>,
    /// Always-on and reactive modifiers: Evasion, Legendary Resistance,
    /// Deflect Attacks.
    pub riders: Vec<Rider>,
    pub actions: Vec<Move>,
    pub bonus_actions: Vec<Move>,
    pub legendary: Vec<Move>,
    /// Legendary actions available each round, each move costing one. Real
    /// blocks have moves costing two or three; nothing here needs that yet.
    pub legendary_uses: u32,
    /// Reliable Talent (2024 Rogue 11): a floor under a d20 roll for a check
    /// the creature is proficient in - `Some(10)` for the standard feature,
    /// `None` for a creature without it. Plain field rather than a
    /// [`Rider`] because, like `legendary_uses`, nothing about it is
    /// triggered or conditional; see [`Creature::check_floor`] for where the
    /// "proficient" half of the rule is applied.
    pub reliable_talent_floor: Option<i32>,
}

impl Creature {
    pub fn new(name: impl Into<String>, ac: i32, hp: i32) -> Self {
        Self {
            name: name.into(),
            team: 0,
            count: 1,
            ac,
            hp,
            creature_type: None,
            initiative: 0,
            size: Size::default(),
            saves: [0; 6],
            reductions: Vec::new(),
            resources: Vec::new(),
            spell_slots: SpellSlots::default(),
            spellcasting: None,
            riders: Vec::new(),
            actions: Vec::new(),
            bonus_actions: Vec::new(),
            legendary: Vec::new(),
            legendary_uses: 0,
            reliable_talent_floor: None,
        }
    }

    pub fn save(&self, ability: Ability) -> i32 {
        self.saves[ability.index()]
    }

    pub fn reduction(&self, kind: DamageKind) -> Reduction {
        self.reductions
            .iter()
            .find(|&&(k, _)| k == kind)
            .map(|&(_, r)| r)
            .unwrap_or_default()
    }

    /// Does this creature turn a successful `ability` save for half into no
    /// damage at all?
    pub fn has_evasion(&self, ability: Ability) -> bool {
        self.riders
            .iter()
            .any(|r| matches!(r, Rider::NothingOnSuccess { ability: a } if *a == ability))
    }

    /// Does this creature's stated type match `type_name`, case-insensitively?
    ///
    /// Missing type information matches anything: the restriction exists to
    /// keep a spell like Hold Person off creatures explicitly stated to be
    /// something else, not off ones nobody labelled - every PC in this
    /// simulator, and a monster whose config simply left the field out.
    pub fn is_creature_type(&self, type_name: &str) -> bool {
        match self.creature_type {
            Some(t) => CreatureType::parse(type_name) == Some(t),
            None => true,
        }
    }

    pub fn resource_index(&self, name: &str) -> Option<usize> {
        self.resources
            .iter()
            .position(|r| r.name.eq_ignore_ascii_case(name))
    }

    /// Spend one spell slot of `level`. `false` and no change if none are left.
    pub fn cast_spell(&mut self, level: u32) -> bool {
        self.spell_slots.cast(level)
    }

    /// A long rest: every spell slot returns.
    pub fn recover_spell_slots(&mut self) {
        self.spell_slots.recover_all();
    }

    /// Spell attack modifier, for a creature that casts spells at all.
    pub fn spell_attack_bonus(&self) -> Option<i32> {
        self.spellcasting.map(|p| p.attack_bonus())
    }

    /// The floor Reliable Talent (or anything shaped like it) puts under a
    /// d20 check, for a check the creature is `proficient` in - `None`
    /// otherwise, and `None` for a creature without the feature at all
    /// regardless of proficiency. Feed the result straight into
    /// [`crate::rules::check::CheckRoll::with_floor`].
    pub fn check_floor(&self, proficient: bool) -> Option<i32> {
        if proficient {
            self.reliable_talent_floor
        } else {
            None
        }
    }

    /// Spell save DC, for a creature that casts spells at all.
    pub fn spell_save_dc(&self) -> Option<i32> {
        self.spellcasting.map(|p| p.save_dc())
    }

    pub fn with_action(mut self, m: Move) -> Self {
        self.actions.push(m);
        self
    }

    pub fn with_bonus_action(mut self, m: Move) -> Self {
        self.bonus_actions.push(m);
        self
    }

    pub fn with_rider(mut self, rider: Rider) -> Self {
        self.riders.push(rider);
        self
    }

    /// Reliable Talent (or anything shaped like it): floor proficient checks
    /// at `floor`.
    pub fn with_reliable_talent_floor(mut self, floor: i32) -> Self {
        self.reliable_talent_floor = Some(floor);
        self
    }

    pub fn with_creature_type(mut self, creature_type: CreatureType) -> Self {
        self.creature_type = Some(creature_type);
        self
    }

    pub fn with_size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }
}
