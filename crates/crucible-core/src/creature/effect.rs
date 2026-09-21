//! What a move does: strikes, saving throws, heals, stances and buffs.

use crate::creature::Creature;
use crate::prob::{Pmf, Rng};
use crate::rules::{
    hit_outcomes_with, hit_outcomes_with_reaction, sample_hit_with, sample_hit_with_reaction,
    Ability, AttackModifier, Condition, DamageKind, DamageRoll, Duration, HealRoll, Landed,
    Reduction, RollMode, SaveModifier,
};

/// What kind of attack roll a [`Strike`] is - the part of 5e's "melee weapon
/// attack", "ranged spell attack" wording that features gate on.
///
/// Sneak Attack wants a finesse or ranged *weapon* (or, with a feature that
/// extends it, a spell attack); a reaction like a shielding umbrella answers
/// only a ranged *weapon* attack; the ally-adjacency proxy in `sim::duel`
/// asks whether a creature's primary attack is melee. Four independent flags
/// rather than one enum of four cases because they genuinely overlap: True
/// Strike is a weapon attack made as part of casting a spell, so it is both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttackKind {
    /// Made with a weapon (natural weapons and unarmed strikes included).
    pub weapon: bool,
    /// A spell attack roll.
    pub spell: bool,
    /// Ranged rather than melee.
    pub ranged: bool,
    /// The weapon has the finesse property.
    pub finesse: bool,
}

impl AttackKind {
    /// A plain melee weapon attack - what an attack is unless it says
    /// otherwise, and the default for every [`Strike::new`].
    pub const MELEE_WEAPON: Self = Self {
        weapon: true,
        spell: false,
        ranged: false,
        finesse: false,
    };

    /// A ranged weapon attack - a bow, a thrown dagger.
    pub const RANGED_WEAPON: Self = Self {
        weapon: true,
        spell: false,
        ranged: true,
        finesse: false,
    };

    /// A melee spell attack - Spiritual Weapon, Inflict Wounds.
    pub const MELEE_SPELL: Self = Self {
        weapon: false,
        spell: true,
        ranged: false,
        finesse: false,
    };

    /// A ranged spell attack - Guiding Bolt, Scorching Ray.
    pub const RANGED_SPELL: Self = Self {
        weapon: false,
        spell: true,
        ranged: true,
        finesse: false,
    };

    /// Sneak Attack's weapon gate: a weapon attack whose weapon is finesse
    /// or ranged.
    pub fn finesse_or_ranged_weapon(self) -> bool {
        self.weapon && (self.finesse || self.ranged)
    }

    /// A ranged weapon attack, the trigger a shielding reaction can be
    /// limited to.
    pub fn ranged_weapon(self) -> bool {
        self.weapon && self.ranged
    }
}

impl Default for AttackKind {
    fn default() -> Self {
        Self::MELEE_WEAPON
    }
}

/// One attack roll and everything it deals on a hit.
#[derive(Debug, Clone, PartialEq)]
pub struct Strike {
    pub to_hit: i32,
    pub mode: RollMode,
    pub damage: Vec<DamageRoll>,
    /// What kind of attack roll this is - see [`AttackKind`].
    pub kind: AttackKind,
}

impl Strike {
    pub fn new(to_hit: i32, damage: Vec<DamageRoll>) -> Self {
        Self {
            to_hit,
            mode: RollMode::Normal,
            damage,
            kind: AttackKind::default(),
        }
    }

    /// Declare what kind of attack roll this is - see [`AttackKind`].
    pub fn with_kind(mut self, kind: AttackKind) -> Self {
        self.kind = kind;
        self
    }

    /// The damage type of this strike's first damage component - the
    /// "weapon's type" that 2024 Sneak Attack's extra dice take on, and the
    /// "spell's type" they take on when a spell attack triggers them.
    /// `None` only for a strike that deals no damage at all.
    pub fn primary_kind(&self) -> Option<DamageKind> {
        self.damage.first().map(|r| r.kind)
    }

    /// `extra` is what [`Strike::damage_pmf_with_modifiers`] and
    /// [`Strike::sample_forcing_crit_with_modifiers`] append for a
    /// [`crate::creature::Rider`]-style bonus - Sneak Attack's dice, a
    /// dragonslaying weapon's bonus - which is exactly [`DamageRoll`] since a
    /// multi-typed strike already carries its own damage as a `Vec` of them.
    ///
    /// `reduce` is how `target` reduces each damage type: normally just
    /// [`Creature::reduction`], but attacker-scoped
    /// ([`Creature::reduction_from`]) when the attacker carries something that
    /// changes it - see [`Strike::damage_pmf_from`].
    fn landed_pmf_by(
        &self,
        reduce: &dyn Fn(DamageKind) -> Reduction,
        crit: bool,
        extra: &[DamageRoll],
    ) -> Pmf {
        self.damage
            .iter()
            .chain(extra)
            .fold(Pmf::constant(0), |acc, roll| {
                acc.convolve(&roll.pmf(crit, reduce(roll.kind)))
            })
    }

    fn landed_pmf(&self, target: &Creature, crit: bool, extra: &[DamageRoll]) -> Pmf {
        self.landed_pmf_by(&|kind| target.reduction(kind), crit, extra)
    }

    /// The exact damage distribution `attacker` deals to `target` with this
    /// strike: as [`Strike::damage_pmf_with_modifiers`], but every damage
    /// component is reduced by [`Creature::reduction_from`] rather than the
    /// target's plain [`Creature::reduction`], so an attacker-side trait that
    /// softens an immunity (see
    /// [`crate::creature::Rider::DowngradeImmunity`]) is honoured. This is the
    /// exact counterpart of [`Strike::sample_from`], which `sim::duel` rolls.
    pub fn damage_pmf_from(
        &self,
        attacker: &Creature,
        target: &Creature,
        mode: RollMode,
        modifiers: &[AttackModifier],
        extra_damage: &[DamageRoll],
    ) -> Pmf {
        let reduce = |kind| target.reduction_from(kind, attacker);
        let o = hit_outcomes_with(self.to_hit, mode, target.ac, modifiers);
        Pmf::mixture(&[
            (o.miss, Pmf::constant(0)),
            (o.hit, self.landed_pmf_by(&reduce, false, extra_damage)),
            (o.crit, self.landed_pmf_by(&reduce, true, extra_damage)),
        ])
    }

    /// One sampled strike from `attacker` against `target`, distributed
    /// according to [`Strike::damage_pmf_from`] when no reaction is spent.
    ///
    /// The single entry point `sim::duel` resolves every attack roll through:
    /// `force_crit` for Paralyzed, `modifiers` for Bless/Bane, `extra_damage`
    /// for every damage rider that qualified for this one attack, and the
    /// defender's reactive AC boost
    /// ([`crate::creature::Rider::ReactionOnTargeted`]) as
    /// `ac_bonus`/`reaction_available`. Returns the damage, how it landed, and
    /// whether the reaction was actually spent.
    #[allow(clippy::too_many_arguments)]
    pub fn sample_from(
        &self,
        rng: &mut Rng,
        attacker: &Creature,
        target: &Creature,
        mode: RollMode,
        force_crit: bool,
        modifiers: &[AttackModifier],
        extra_damage: &[DamageRoll],
        ac_bonus: i32,
        reaction_available: bool,
    ) -> (i32, Landed, bool) {
        let (landed, consumed) = sample_hit_with_reaction(
            rng,
            self.to_hit,
            mode,
            target.ac,
            modifiers,
            ac_bonus,
            reaction_available,
        );
        let landed = if force_crit && landed == Landed::Hit {
            Landed::Crit
        } else {
            landed
        };
        let crit = match landed {
            Landed::Miss => return (0, landed, consumed),
            Landed::Hit => false,
            Landed::Crit => true,
        };
        let total = self
            .damage
            .iter()
            .chain(extra_damage)
            .map(|roll| roll.sample(rng, crit, target.reduction_from(roll.kind, attacker)))
            .sum();
        (total, landed, consumed)
    }

    /// Exact distribution of the damage this strike deals to `target`,
    /// including the zero from a miss.
    ///
    /// `mode` is passed in rather than read off the strike because advantage
    /// is mostly a property of the *situation* - who is prone, who is dodging -
    /// and only sometimes of the weapon.
    pub fn damage_pmf_with(&self, target: &Creature, mode: RollMode) -> Pmf {
        self.damage_pmf_with_modifiers(target, mode, &[], &[])
    }

    /// As [`Strike::damage_pmf_with`], with an [`AttackModifier`] list applied
    /// to the roll (Bless, Bane, a flat bonus, forced advantage) and extra
    /// [`DamageRoll`]s appended on a hit (a damage rider).
    ///
    /// Both lists are decided by the caller for this one attack - see the
    /// module docs on [`crate::rules::AttackModifier`] - so many
    /// unrelated sources can be active on the same strike without this
    /// function, or `sim::duel`, growing a branch per source.
    pub fn damage_pmf_with_modifiers(
        &self,
        target: &Creature,
        mode: RollMode,
        modifiers: &[AttackModifier],
        extra_damage: &[DamageRoll],
    ) -> Pmf {
        let o = hit_outcomes_with(self.to_hit, mode, target.ac, modifiers);
        Pmf::mixture(&[
            (o.miss, Pmf::constant(0)),
            (o.hit, self.landed_pmf(target, false, extra_damage)),
            (o.crit, self.landed_pmf(target, true, extra_damage)),
        ])
    }

    pub fn damage_pmf(&self, target: &Creature) -> Pmf {
        self.damage_pmf_with(target, self.mode)
    }

    /// As [`Strike::damage_pmf_with_modifiers`], with the same reactive AC
    /// boost as [`Strike::sample_from`] -
    /// [`crate::creature::Rider::ReactionOnTargeted`]. See
    /// [`hit_outcomes_with_reaction`] for why spending the reaction whenever
    /// `available` is exactly equivalent, in the aggregate, to fighting
    /// against a raised AC.
    pub fn damage_pmf_with_reaction(
        &self,
        target: &Creature,
        mode: RollMode,
        ac_bonus: i32,
        available: bool,
    ) -> Pmf {
        let o = hit_outcomes_with_reaction(self.to_hit, mode, target.ac, &[], ac_bonus, available);
        Pmf::mixture(&[
            (o.miss, Pmf::constant(0)),
            (o.hit, self.landed_pmf(target, false, &[])),
            (o.crit, self.landed_pmf(target, true, &[])),
        ])
    }

    /// One sampled strike, reporting how it landed so a rider can key off the
    /// hit. Must be distributed according to [`Strike::damage_pmf_with`];
    /// `tests/duel_agreement.rs` requires it.
    pub fn sample_with(&self, rng: &mut Rng, target: &Creature, mode: RollMode) -> (i32, Landed) {
        self.sample_forcing_crit(rng, target, mode, false)
    }

    /// As [`Strike::sample_with`], but `force_crit` upgrades an ordinary hit to
    /// a critical one before damage is rolled.
    ///
    /// This is Paralyzed's "a hit against this creature is a critical hit" -
    /// see `Condition::auto_crits` - folded into the roll rather than the
    /// caller re-deriving damage after the fact, which would have to
    /// duplicate the crit-doubling logic below to get the same distribution.
    pub fn sample_forcing_crit(
        &self,
        rng: &mut Rng,
        target: &Creature,
        mode: RollMode,
        force_crit: bool,
    ) -> (i32, Landed) {
        self.sample_forcing_crit_with_modifiers(rng, target, mode, force_crit, &[], &[])
    }

    /// As [`Strike::sample_forcing_crit`], with the same [`AttackModifier`]
    /// list and extra [`DamageRoll`]s as [`Strike::damage_pmf_with_modifiers`].
    /// Must be distributed according to it; `tests/duel_agreement.rs` requires
    /// that agreement the same way it does for the unmodified strike.
    #[allow(clippy::too_many_arguments)]
    pub fn sample_forcing_crit_with_modifiers(
        &self,
        rng: &mut Rng,
        target: &Creature,
        mode: RollMode,
        force_crit: bool,
        modifiers: &[AttackModifier],
        extra_damage: &[DamageRoll],
    ) -> (i32, Landed) {
        let landed = sample_hit_with(rng, self.to_hit, mode, target.ac, modifiers);
        let landed = if force_crit && landed == Landed::Hit {
            Landed::Crit
        } else {
            landed
        };
        let crit = match landed {
            Landed::Miss => return (0, landed),
            Landed::Hit => false,
            Landed::Crit => true,
        };
        let total = self
            .damage
            .iter()
            .chain(extra_damage)
            .map(|roll| roll.sample(rng, crit, target.reduction(roll.kind)))
            .sum();
        (total, landed)
    }

    pub fn sample(&self, rng: &mut Rng, target: &Creature) -> i32 {
        self.sample_with(rng, target, self.mode).0
    }

    pub fn mean_damage(&self, target: &Creature) -> f64 {
        self.damage_pmf(target).mean()
    }

    /// Does this strike deal any of the listed types? Asked by
    /// [`crate::creature::Rider::ReduceDamage`], which only triggers on some
    /// damage.
    pub fn deals_any(&self, kinds: &[DamageKind]) -> bool {
        self.damage.iter().any(|r| kinds.contains(&r.kind))
    }
}

/// A saving throw for damage - a breath weapon, a fireball.
#[derive(Debug, Clone, PartialEq)]
pub struct SaveEffect {
    pub ability: Ability,
    pub dc: i32,
    pub damage: Vec<DamageRoll>,
    pub half_on_success: bool,
    /// Conditions applied when the save fails. Hold Person and every breath
    /// weapon whose text does more than deal damage need only one; empty for
    /// the plain damaging kind. A `Vec` rather than a single `Option` because
    /// some single saves land more than one condition at once - Command's
    /// "Grovel" both drops the target Prone and denies it the rest of its
    /// turn (see [`Condition::Compelled`]), and both have to come off the
    /// *same* roll rather than two independently-rolled ones.
    pub on_failure: Vec<(Condition, Duration)>,
    /// How many enemies it can catch. `None` means all of them, which is what a
    /// cone or a sphere does in the absence of a positioning model - the
    /// pessimistic reading. `Some(2)` is for something that names a number, like
    /// a second-level Command.
    pub max_targets: Option<u32>,
    /// Restricts who this can even affect to creatures whose
    /// [`Creature::creature_type`] matches, case-insensitively - Hold
    /// Person's "you can only target a humanoid", Charm Person's identical
    /// restriction. `None` for anything untargeted this way, which is every
    /// save effect that is not itself type-restricted. Checked before a save
    /// is even rolled: an excluded creature is not caught by this effect at
    /// all, the same as being out of `max_targets`' cap.
    pub requires_type: Option<String>,
}

impl SaveEffect {
    /// P(the target fails the save), ignoring conditions and Legendary
    /// Resistance, both of which are state rather than statistics.
    ///
    /// Saving throws have no natural-1 or natural-20 rule in 5e, unlike attack
    /// rolls, so this really is a flat count of faces.
    pub fn failure_chance(&self, target: &Creature) -> f64 {
        let needed = self.dc - target.save(self.ability);
        let successes = (21 - needed).clamp(0, 20);
        1.0 - f64::from(successes) / 20.0
    }

    /// Damage from one outcome, as a fraction: full, half, or none.
    ///
    /// Evasion inverts the usual shape - a success takes nothing and a failure
    /// takes half - which is why this is one function of two booleans rather
    /// than two separate paths.
    fn share(&self, saved: bool, evasion: bool) -> Share {
        match (saved, evasion, self.half_on_success) {
            (false, false, _) => Share::Full,
            (false, true, _) => Share::Half,
            (true, true, _) => Share::None,
            (true, false, true) => Share::Half,
            (true, false, false) => Share::None,
        }
    }

    fn outcome_pmf(&self, target: &Creature, share: Share) -> Pmf {
        if share == Share::None {
            return Pmf::constant(0);
        }
        self.damage.iter().fold(Pmf::constant(0), |acc, roll| {
            let mut p = Pmf::pool(roll.count, roll.sides)
                .offset(roll.bonus)
                .floor_at(0);
            if share == Share::Half {
                p = p.map_values(|d| d / 2);
            }
            let reduction = target.reduction(roll.kind);
            acc.convolve(&p.map_values(move |d| reduction.apply(d)))
        })
    }

    /// Exact distribution of damage dealt, over both save outcomes.
    ///
    /// Accounts for the target's save bonus, Evasion and resistances, all of
    /// which are stateless. It cannot account for Legendary Resistance or a
    /// damage-reducing reaction, which depend on what has already been spent;
    /// `duel` handles those and `tests/duel_agreement.rs` tests them
    /// separately.
    pub fn damage_pmf(&self, target: &Creature) -> Pmf {
        let evasion = target.has_evasion(self.ability);
        let fail = self.failure_chance(target);
        Pmf::mixture(&[
            (fail, self.outcome_pmf(target, self.share(false, evasion))),
            (
                1.0 - fail,
                self.outcome_pmf(target, self.share(true, evasion)),
            ),
        ])
    }

    /// The sampled counterpart, with the pieces the exact path cannot see
    /// passed in: whether a condition forces a failure, and whether Evasion
    /// applies at all.
    pub fn sample_with(
        &self,
        rng: &mut Rng,
        target: &Creature,
        auto_fail: bool,
        evasion: bool,
    ) -> (i32, bool) {
        let saved = !auto_fail && rng.die(20) + target.save(self.ability) >= self.dc;
        (
            self.sample_share(rng, target, self.share(saved, evasion)),
            saved,
        )
    }

    fn sample_share(&self, rng: &mut Rng, target: &Creature, share: Share) -> i32 {
        self.sample_share_by(rng, &|kind| target.reduction(kind), share)
    }

    fn sample_share_by(
        &self,
        rng: &mut Rng,
        reduce: &dyn Fn(DamageKind) -> Reduction,
        share: Share,
    ) -> i32 {
        if share == Share::None {
            return 0;
        }
        self.damage
            .iter()
            .map(|roll| {
                let raw: i32 = (0..roll.count).map(|_| rng.die(roll.sides)).sum();
                let mut dealt = (raw + roll.bonus).max(0);
                if share == Share::Half {
                    dealt /= 2;
                }
                reduce(roll.kind).apply(dealt)
            })
            .sum()
    }

    /// As [`SaveEffect::sample_known`], with `target`'s reductions scoped to
    /// `attacker` via [`Creature::reduction_from`] - the path `sim::duel`
    /// takes, so an attacker-side immunity downgrade applies to a save's
    /// damage exactly as it does to a hit's.
    pub fn sample_known_from(
        &self,
        rng: &mut Rng,
        attacker: &Creature,
        target: &Creature,
        saved: bool,
        evasion: bool,
    ) -> i32 {
        self.sample_share_by(
            rng,
            &|kind| target.reduction_from(kind, attacker),
            self.share(saved, evasion),
        )
    }

    /// Roll the save only, for a caller that wants to react to the result -
    /// spending Legendary Resistance - before damage is computed.
    pub fn roll_save(&self, rng: &mut Rng, target: &Creature, auto_fail: bool) -> bool {
        !auto_fail && rng.die(20) + target.save(self.ability) >= self.dc
    }

    /// Damage for a save whose result is already known.
    pub fn sample_known(
        &self,
        rng: &mut Rng,
        target: &Creature,
        saved: bool,
        evasion: bool,
    ) -> i32 {
        self.sample_share(rng, target, self.share(saved, evasion))
    }

    pub fn sample(&self, rng: &mut Rng, target: &Creature) -> (i32, bool) {
        self.sample_with(rng, target, false, target.has_evasion(self.ability))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Share {
    None,
    Half,
    Full,
}

/// What a move does.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// `count` separate attack rolls sharing one profile: a Multiattack of
    /// three identical claws, or a monk's two unarmed strikes.
    Strikes {
        strike: Strike,
        count: u32,
    },
    Save(SaveEffect),
    /// A condition the user puts on itself: Dodge, and every "until the start
    /// of your next turn" defensive stance.
    Stance {
        condition: Condition,
    },
    /// Restores hit points to one of the user's own side - Healing Word, Cure
    /// Wounds, a potion; `sim::duel` picks who, a downed ally first.
    /// The only effect that makes HP go up instead of down, which is why it
    /// contributes nothing to `mean_damage`/`damage_pmf` and gets its own
    /// `heal_pmf` instead.
    Heal(HealRoll),
    /// Damage that always lands: no attack roll, no saving throw. Magic
    /// Missile's mechanism - "you create three glowing darts of magical
    /// force... each dart hits a creature of your choice" - and the third way
    /// 5e deals damage alongside `Strikes` and `Save`, which otherwise has
    /// nowhere to live.
    ///
    /// Resistance and immunity still apply - what this skips is the *roll*,
    /// via `Strike`'s to-hit or `Save`'s ability check, not
    /// `Creature::reduction`, which every `DamageRoll` still goes through.
    AutoHit {
        damage: Vec<DamageRoll>,
    },
    /// Several effects in one move. A Multiattack of two claws and a bite, or
    /// a monk replacing one of its attacks with a breath weapon.
    Sequence(Vec<Effect>),
    /// An unconditional buff to some of the user's own side: each of up to
    /// `max_targets` allies - the user included - gains `attack_modifier` on
    /// its attack rolls and `save_modifier` on its saving throws, including
    /// its own concentration save - correct 5e behaviour for Bless, not a bug,
    /// since `sim::duel` resolves every saving throw a fighter makes through
    /// the same modifier list. Lasts until this move's concentration ends, so
    /// a move using this should also set
    /// [`crate::creature::Move::concentration`].
    ///
    /// Bless is `Buff { attack_modifier: BonusDice(1d4), save_modifier:
    /// BonusDice(1d4), max_targets: Some(3) }`.
    Buff {
        attack_modifier: AttackModifier,
        save_modifier: SaveModifier,
        max_targets: Option<u32>,
    },
    /// Up to `max_targets` of the opposing side each make an `ability` save
    /// against the caster's own spell save DC (read from
    /// [`crate::rules::SpellCastingProfile`] at the moment this
    /// resolves rather than a number carried on the move, so this can never
    /// drift into a second, stale copy of what that profile already computes)
    /// or take `attack_modifier` on attack rolls and `save_modifier` on saving
    /// throws for the duration. As with [`Effect::Buff`], the move should set
    /// [`crate::creature::Move::concentration`].
    ///
    /// Bane is `SaveOrModifier { ability: Cha, attack_modifier:
    /// PenaltyDice(1d4), save_modifier: PenaltyDice(1d4), max_targets:
    /// Some(3) }`.
    SaveOrModifier {
        ability: Ability,
        attack_modifier: AttackModifier,
        save_modifier: SaveModifier,
        max_targets: Option<u32>,
    },
}

impl Effect {
    /// Exact mean damage against `target`, used by the greedy policy and by
    /// the report. Independent parts add, so this is a sum of means.
    ///
    /// A stance scores zero, which is exactly why a purely greedy policy never
    /// defends and why the library needs more than one policy in it.
    pub fn mean_damage(&self, target: &Creature) -> f64 {
        match self {
            Effect::Strikes { strike, count } => strike.mean_damage(target) * f64::from(*count),
            Effect::Save(save) => save.damage_pmf(target).mean(),
            // Heals nothing, damages nothing: it has its own accounting.
            Effect::Heal(_) => 0.0,
            Effect::AutoHit { .. } => self.damage_pmf(target).mean(),
            // None of these deal damage - a Stance changes footing, Buff/
            // SaveOrModifier alter attack rolls and saves rather than
            // dealing damage themselves.
            Effect::Stance { .. } | Effect::Buff { .. } | Effect::SaveOrModifier { .. } => 0.0,
            Effect::Sequence(parts) => parts.iter().map(|p| p.mean_damage(target)).sum(),
        }
    }

    /// Exact distribution of the damage this effect deals in one use.
    pub fn damage_pmf(&self, target: &Creature) -> Pmf {
        match self {
            Effect::Strikes { strike, count } => {
                let one = strike.damage_pmf(target);
                let mut acc = Pmf::constant(0);
                for _ in 0..*count {
                    acc = acc.convolve(&one);
                }
                acc
            }
            Effect::Save(save) => save.damage_pmf(target),
            Effect::Heal(_) => Pmf::constant(0),
            Effect::AutoHit { damage } => damage.iter().fold(Pmf::constant(0), |acc, roll| {
                acc.convolve(&roll.pmf(false, target.reduction(roll.kind)))
            }),
            Effect::Stance { .. } | Effect::Buff { .. } | Effect::SaveOrModifier { .. } => {
                Pmf::constant(0)
            }
            Effect::Sequence(parts) => parts.iter().fold(Pmf::constant(0), |acc, p| {
                acc.convolve(&p.damage_pmf(target))
            }),
        }
    }

    /// Exact distribution of the HP this effect restores in one use - the
    /// mirror of [`Effect::damage_pmf`] for the one effect that heals rather
    /// than harms.
    pub fn heal_pmf(&self) -> Pmf {
        match self {
            Effect::Heal(roll) => roll.pmf(),
            Effect::Sequence(parts) => parts
                .iter()
                .fold(Pmf::constant(0), |acc, p| acc.convolve(&p.heal_pmf())),
            _ => Pmf::constant(0),
        }
    }

    pub fn mean_heal(&self) -> f64 {
        self.heal_pmf().mean()
    }

    /// Does any part of this apply a condition to the user?
    pub fn stance(&self) -> Option<Condition> {
        match self {
            Effect::Stance { condition } => Some(*condition),
            Effect::Sequence(parts) => parts.iter().find_map(|p| p.stance()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::Rider;

    fn dummy(ac: i32) -> Creature {
        Creature::new("dummy", ac, 100)
    }

    #[test]
    fn a_strike_damage_distribution_is_a_distribution() {
        let s = Strike::new(
            7,
            vec![
                DamageRoll::new(1, 10, 8, DamageKind::Slashing),
                DamageRoll::new(2, 4, 0, DamageKind::Fire),
            ],
        );
        let pmf = s.damage_pmf(&dummy(16));
        assert!((pmf.total() - 1.0).abs() < 1e-12);
        assert!(pmf.min() >= 0);
        assert!(pmf.prob(0) > 0.0, "a miss must be possible");
        // On a crit the dice double but the +8 does not: 2d10 + 8 plus 4d4.
        assert_eq!(pmf.max(), 20 + 8 + 16);
    }

    /// The whole reason damage is a list rather than one pool.
    #[test]
    fn each_damage_type_is_reduced_on_its_own() {
        let mut target = dummy(1);
        target
            .reductions
            .push((DamageKind::Fire, Reduction::Immune));

        let s = Strike::new(
            20,
            vec![
                DamageRoll::new(0, 6, 10, DamageKind::Slashing),
                DamageRoll::new(0, 6, 10, DamageKind::Fire),
            ],
        );
        // Every roll but a natural 1 hits, and the fire half is deleted.
        let pmf = s.damage_pmf(&target);
        assert!((pmf.prob(10) - 19.0 / 20.0).abs() < 1e-12);
        assert!((pmf.prob(0) - 1.0 / 20.0).abs() < 1e-12);
    }

    #[test]
    fn a_save_has_no_natural_twenty() {
        let mut target = dummy(10);
        target.saves[Ability::Dex.index()] = 2;
        let save = SaveEffect {
            ability: Ability::Dex,
            dc: 25,
            damage: vec![DamageRoll::new(1, 6, 0, DamageKind::Fire)],
            half_on_success: true,
            on_failure: vec![],
            max_targets: None,
            requires_type: None,
        };
        // Needs a 23 on a d20; unlike an attack roll, a natural 20 does not
        // rescue it.
        assert!((save.failure_chance(&target) - 1.0).abs() < 1e-12);

        let trivial = SaveEffect { dc: -5, ..save };
        assert!(trivial.failure_chance(&target).abs() < 1e-12);
    }

    #[test]
    fn a_successful_save_halves_before_resistance_does() {
        let mut target = dummy(10);
        target.saves[Ability::Dex.index()] = 100; // always saves
        target
            .reductions
            .push((DamageKind::Fire, Reduction::Resistant));
        let save = SaveEffect {
            ability: Ability::Dex,
            dc: 10,
            damage: vec![DamageRoll::new(0, 6, 21, DamageKind::Fire)],
            half_on_success: true,
            on_failure: vec![],
            max_targets: None,
            requires_type: None,
        };
        // 21 -> 10 on the save, then 5 from resistance. Rounding down twice is
        // not the same as quartering, which is why the order is pinned here.
        assert!((save.damage_pmf(&target).mean() - 5.0).abs() < 1e-12);
    }

    /// Evasion turns the usual shape inside out, and stacks with resistance
    /// rather than replacing it. 40 fire, resisted, is the case to check
    /// because every step divides.
    #[test]
    fn evasion_inverts_the_save_and_still_lets_resistance_apply() {
        let base = SaveEffect {
            ability: Ability::Dex,
            dc: 10,
            damage: vec![DamageRoll::new(0, 6, 40, DamageKind::Fire)],
            half_on_success: true,
            on_failure: vec![],
            max_targets: None,
            requires_type: None,
        };

        let mut always_saves = dummy(10);
        always_saves.saves = [100; 6];
        assert!((base.damage_pmf(&always_saves).mean() - 20.0).abs() < 1e-12);

        let evasive = always_saves.clone().with_rider(Rider::NothingOnSuccess {
            ability: Ability::Dex,
        });
        assert!(
            base.damage_pmf(&evasive).mean().abs() < 1e-12,
            "a saved Dex save with Evasion deals nothing"
        );

        // Failing with Evasion is the old success: half. Then resistance.
        let mut evasive_fails = evasive.clone();
        evasive_fails.saves[Ability::Dex.index()] = -100;
        assert!((base.damage_pmf(&evasive_fails).mean() - 20.0).abs() < 1e-12);
        evasive_fails
            .reductions
            .push((DamageKind::Fire, Reduction::Resistant));
        assert!(
            (base.damage_pmf(&evasive_fails).mean() - 10.0).abs() < 1e-12,
            "Evasion halves and resistance halves again"
        );

        // Evasion is keyed to the ability, so a Con save is untouched.
        let con = SaveEffect {
            ability: Ability::Con,
            ..base.clone()
        };
        assert!(!evasive.has_evasion(Ability::Con));
        assert!((con.damage_pmf(&always_saves).mean() - 20.0).abs() < 1e-12);
    }

    #[test]
    fn strikes_add_their_means_and_a_sequence_adds_its_parts() {
        let target = dummy(15);
        let profile = || Strike::new(5, vec![DamageRoll::new(1, 8, 3, DamageKind::Slashing)]);
        let one = Effect::Strikes {
            strike: profile(),
            count: 1,
        };
        let three = Effect::Strikes {
            strike: profile(),
            count: 3,
        };
        assert!((three.mean_damage(&target) - 3.0 * one.mean_damage(&target)).abs() < 1e-9);
        assert!((three.damage_pmf(&target).total() - 1.0).abs() < 1e-12);

        let combo = Effect::Sequence(vec![one.clone(), one.clone(), one.clone()]);
        assert!((combo.mean_damage(&target) - three.mean_damage(&target)).abs() < 1e-9);

        // A stance contributes nothing to damage but is still findable.
        let mixed = Effect::Sequence(vec![
            one,
            Effect::Stance {
                condition: Condition::Dodging,
            },
        ]);
        assert_eq!(mixed.stance(), Some(Condition::Dodging));
    }

    /// The four standard shapes of attack roll, and the two questions
    /// features ask of them.
    #[test]
    fn attack_kinds_answer_the_sneak_attack_and_ranged_weapon_gates() {
        assert!(!AttackKind::MELEE_WEAPON.finesse_or_ranged_weapon());
        let rapier = AttackKind {
            finesse: true,
            ..AttackKind::MELEE_WEAPON
        };
        assert!(rapier.finesse_or_ranged_weapon());
        assert!(!rapier.ranged_weapon());
        assert!(AttackKind::RANGED_WEAPON.finesse_or_ranged_weapon());
        assert!(AttackKind::RANGED_WEAPON.ranged_weapon());
        assert!(!AttackKind::RANGED_SPELL.finesse_or_ranged_weapon());
        assert!(!AttackKind::RANGED_SPELL.ranged_weapon());
        assert!(!AttackKind::MELEE_SPELL.finesse_or_ranged_weapon());
        assert_eq!(Strike::new(5, Vec::new()).kind, AttackKind::MELEE_WEAPON);
    }
}
