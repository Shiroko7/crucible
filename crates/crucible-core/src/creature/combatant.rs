//! The Creature combatant model.

use crate::creature::{Move, Resource, Rider};
use crate::rules::{
    Ability, Condition, CreatureType, DamageKind, Reduction, Size, SpellCastingProfile, SpellSlots,
};

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
    /// Raw ability scores (Strength through Charisma), not save or check
    /// bonuses - those live in `saves`. Zeroed for a creature that never
    /// declares any, which is fine for anything that only ever needs the
    /// bonuses; a prerequisite check gated on a raw score (a prestige
    /// feature's ability minimums, say) is the reason this exists at all.
    pub abilities: [i32; 6],
    pub reductions: Vec<(DamageKind, Reduction)>,
    /// Conditions this creature is flatly immune to - a construct's or an
    /// undead's usual immunity to Poisoned, say. A flat list rather than a
    /// taxonomy, the same shape [`Creature::reductions`] already uses for
    /// damage types: this exists only because [`Rider::DowngradeImmunity`]
    /// needs a target-side concept of "immune to this condition" to
    /// downgrade in the first place, so it stays exactly that minimal.
    pub condition_immunities: Vec<Condition>,
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
    /// Reliable Talent (2024 Rogue 7): a floor under a d20 roll for a check
    /// the creature is proficient in - `Some(10)` for the standard feature,
    /// `None` for a creature without it. Plain field rather than a
    /// [`Rider`] because, like `legendary_uses`, nothing about it is
    /// triggered or conditional; see [`Creature::check_floor`] for where the
    /// "proficient" half of the rule is applied.
    pub reliable_talent_floor: Option<i32>,
    /// A player character drops to 0 hit points unconscious rather than dead,
    /// so healing can bring it back mid-fight - unless the blow that dropped
    /// it had damage left over equal to its hit point maximum, which kills it
    /// outright. A monster simply dies at 0. Set by the PC loader.
    pub player_character: bool,
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
            abilities: [0; 6],
            reductions: Vec::new(),
            condition_immunities: Vec::new(),
            resources: Vec::new(),
            spell_slots: SpellSlots::default(),
            spellcasting: None,
            riders: Vec::new(),
            actions: Vec::new(),
            bonus_actions: Vec::new(),
            legendary: Vec::new(),
            legendary_uses: 0,
            reliable_talent_floor: None,
            player_character: false,
        }
    }

    pub fn save(&self, ability: Ability) -> i32 {
        self.saves[ability.index()]
    }

    /// This creature's raw ability score, as opposed to [`Creature::save`]'s
    /// bonus.
    pub fn ability_score(&self, ability: Ability) -> i32 {
        self.abilities[ability.index()]
    }

    pub fn reduction(&self, kind: DamageKind) -> Reduction {
        self.reductions
            .iter()
            .find(|&&(k, _)| k == kind)
            .map(|&(_, r)| r)
            .unwrap_or_default()
    }

    /// The [`Reduction`] `attacker` actually inflicts against this creature
    /// for `kind`.
    ///
    /// Ordinarily just [`Creature::reduction`]. The one exception: if this
    /// creature is [`Reduction::Immune`] to `kind` and `attacker` carries a
    /// [`Rider::DowngradeImmunity`] naming this exact `kind`, the hit lands
    /// as merely [`Reduction::Resistant`] instead - half damage rather than
    /// none.
    ///
    /// Scoped to `attacker` alone, never a change to this creature's own
    /// stat sheet: a different attacker with no such trait, against this
    /// very same target, still sees `Immune` from [`Creature::reduction`].
    pub fn reduction_from(&self, kind: DamageKind, attacker: &Creature) -> Reduction {
        let base = self.reduction(kind);
        if base == Reduction::Immune
            && attacker
                .riders
                .iter()
                .any(|r| r.downgrades_damage_immunity(kind))
        {
            Reduction::Resistant
        } else {
            base
        }
    }

    /// Is this creature flatly immune to `condition`? See
    /// [`Creature::condition_immunities`].
    pub fn immune_to_condition(&self, condition: Condition) -> bool {
        self.condition_immunities.contains(&condition)
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

    /// Does this creature's [`Rider::ConditionalExtraDamage`] also accept a
    /// qualifying spell attack roll, not only a finesse-or-ranged weapon
    /// attack? See [`Rider::ExtraDamageAppliesToSpellAttacks`] and
    /// [`Rider::extra_damage_for_with_spell_attack_extension`].
    pub fn extra_damage_applies_to_spell_attacks(&self) -> bool {
        self.riders
            .iter()
            .any(|r| matches!(r, Rider::ExtraDamageAppliesToSpellAttacks))
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
    /// [`crate::rules::CheckRoll::with_floor`].
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

    pub fn with_condition_immunity(mut self, condition: Condition) -> Self {
        self.condition_immunities.push(condition);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn casting_a_spell_on_a_creature_spends_from_its_own_pool() {
        let mut caster = Creature::new("caster", 15, 20);
        caster.spell_slots.set_max(1, 2);

        assert!(caster.cast_spell(1));
        assert!(caster.cast_spell(1));
        assert!(!caster.cast_spell(1), "the pool is empty");

        caster.recover_spell_slots();
        assert!(caster.cast_spell(1), "a rest refills it");
    }

    /// A creature with no declared type gates out every
    /// `BonusDamageVsCreatureType` rider, which is what a plain SRD monster
    /// with no `creature_type` line should do.
    #[test]
    fn a_creature_with_no_declared_type_never_matches_a_creature_type_gate() {
        let plain = dummy(15);
        assert_eq!(plain.creature_type, None);
    }

    #[test]
    fn a_creature_defaults_to_medium_size_but_can_be_overridden() {
        let creature = Creature::new("dummy", 12, 10);
        assert_eq!(creature.size, Size::Medium);
        let huge = Creature::new("big", 12, 10).with_size(Size::Huge);
        assert_eq!(huge.size, Size::Huge);
    }

    fn dummy(ac: i32) -> Creature {
        Creature::new("dummy", ac, 100)
    }
}
