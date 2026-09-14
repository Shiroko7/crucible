//! Actions, strikes, saves, stances, and moves.

use crate::prob::dice::Pmf;
use crate::prob::rng::Rng;
use crate::rules::combat::{
    hit_outcomes_with, hit_outcomes_with_reaction, sample_hit_with, sample_hit_with_reaction,
    AttackModifier, Landed, RollMode,
};

use super::combatant::Creature;
use super::damage::{DamageKind, DamageRoll};
use super::rider::Rider;
use super::types::{Ability, Condition, Cost, Duration};

/// One attack roll and everything it deals on a hit.
#[derive(Debug, Clone, PartialEq)]
pub struct Strike {
    pub to_hit: i32,
    pub mode: RollMode,
    pub damage: Vec<DamageRoll>,
}

impl Strike {
    pub fn new(to_hit: i32, damage: Vec<DamageRoll>) -> Self {
        Self {
            to_hit,
            mode: RollMode::Normal,
            damage,
        }
    }

    /// `extra` is what [`Strike::damage_pmf_with_modifiers`] and
    /// [`Strike::sample_forcing_crit_with_modifiers`] append for a
    /// [`super::rider::Rider`]-style bonus - Sneak Attack's dice, a
    /// dragonslaying weapon's bonus - which is exactly [`DamageRoll`] since a
    /// multi-typed strike already carries its own damage as a `Vec` of them.
    fn landed_pmf(&self, target: &Creature, crit: bool, extra: &[DamageRoll]) -> Pmf {
        self.damage
            .iter()
            .chain(extra)
            .fold(Pmf::constant(0), |acc, roll| {
                acc.convolve(&roll.pmf(crit, target.reduction(roll.kind)))
            })
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
    /// module docs on [`crate::rules::combat::AttackModifier`] - so many
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
    /// boost as [`Strike::sample_forcing_crit_with_reaction`] -
    /// [`super::rider::Rider::ReactionOnTargeted`]. See
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

    /// As [`Strike::sample_forcing_crit`], but the defender may spend a
    /// reaction to add `ac_bonus` to its AC against this one attack before
    /// hit or miss is finalized - [`super::rider::Rider::ReactionOnTargeted`],
    /// the mirror of [`super::rider::Rider::ReduceDamage`]: that one reacts
    /// to an attack that already hit, on its damage; this one reacts to
    /// being targeted, before the roll against AC is decided, and can turn
    /// what would have been a hit into a miss. Returns whether the reaction
    /// actually fired alongside the damage and how the attack landed, so the
    /// caller - `sim::duel`, which owns the per-round budget - can debit it
    /// only when it does.
    pub fn sample_forcing_crit_with_reaction(
        &self,
        rng: &mut Rng,
        target: &Creature,
        mode: RollMode,
        force_crit: bool,
        ac_bonus: i32,
        reaction_available: bool,
    ) -> (i32, Landed, bool) {
        let (landed, consumed) = sample_hit_with_reaction(
            rng,
            self.to_hit,
            mode,
            target.ac,
            &[],
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
            .map(|roll| roll.sample(rng, crit, target.reduction(roll.kind)))
            .sum();
        (total, landed, consumed)
    }

    pub fn mean_damage(&self, target: &Creature) -> f64 {
        self.damage_pmf(target).mean()
    }

    /// Does this strike deal any of the listed types? Asked by
    /// [`Rider::ReduceDamage`], which only triggers on some damage.
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
                target.reduction(roll.kind).apply(dealt)
            })
            .sum()
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

/// A healing roll: dice plus a flat modifier, restoring hit points instead of
/// removing them.
///
/// Deliberately not a [`DamageRoll`] wearing a different sign: there is no
/// damage type to resist, no crit to double the dice, and nothing reduces it.
/// Keeping it a separate, smaller type means `Effect::Heal` cannot
/// accidentally inherit any of that machinery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealRoll {
    pub count: u32,
    pub sides: u32,
    /// Baked in at construction time from whichever ability score fuels the
    /// cast - the caster's [`crate::rules::creature::SpellCastingProfile`]
    /// modifier, not a hardcoded number. See the plugins in
    /// `dsl::plugin::spells`.
    pub bonus: i32,
}

impl HealRoll {
    pub fn new(count: u32, sides: u32, bonus: i32) -> Self {
        assert!(sides > 0, "a d0 has no faces");
        Self {
            count,
            sides,
            bonus,
        }
    }

    /// Exact distribution of the amount healed, floored at zero: a roll with
    /// a very negative modifier cannot make a healing spell drain HP.
    pub fn pmf(&self) -> Pmf {
        Pmf::pool(self.count, self.sides)
            .offset(self.bonus)
            .floor_at(0)
    }

    /// The sampled counterpart of [`HealRoll::pmf`]; `tests/duel_agreement.rs`-style
    /// agreement is asserted in this module's own tests.
    pub fn sample(&self, rng: &mut Rng) -> i32 {
        let raw: i32 = (0..self.count).map(|_| rng.die(self.sides)).sum();
        (raw + self.bonus).max(0)
    }

    pub fn mean(&self) -> f64 {
        f64::from(self.count) * (f64::from(self.sides) + 1.0) / 2.0 + f64::from(self.bonus)
    }
}

/// Is a creature at this HP down - unconscious, in 5e terms?
///
/// There is no death-save subsystem here, so "down" is exactly "at zero HP or
/// below": the minimal state a healing spell needs to know whether it is
/// reviving someone or merely topping them up. Named rather than repeating
/// `hp <= 0` at each call site, the same reasoning `Condition` gets.
pub fn is_down(hp: i32) -> bool {
    hp <= 0
}

/// Apply `amount` of healing to `current_hp`, capped at `max_hp`.
///
/// Returns the new HP and whether this revived the target: 5e's general
/// "regaining hit points" rule is that any creature at 0 HP that regains any
/// HP becomes conscious again, which is not specific to any one spell - both
/// Healing Word and Cure Wounds get it for free by going through this.
pub fn apply_healing(current_hp: i32, max_hp: i32, amount: i32) -> (i32, bool) {
    let was_down = is_down(current_hp);
    let new_hp = (current_hp + amount.max(0)).min(max_hp);
    (new_hp, was_down && !is_down(new_hp))
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
    /// Restores hit points to whoever it targets - Healing Word, Cure Wounds.
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
            Effect::Stance { .. } => 0.0,
            // Heals nothing, damages nothing: it has its own accounting.
            Effect::Heal(_) => 0.0,
            Effect::AutoHit { .. } => self.damage_pmf(target).mean(),
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
            Effect::Stance { .. } => Pmf::constant(0),
            Effect::Heal(_) => Pmf::constant(0),
            Effect::AutoHit { damage } => damage.iter().fold(Pmf::constant(0), |acc, roll| {
                acc.convolve(&roll.pmf(false, target.reduction(roll.kind)))
            }),
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

/// How often a move can be taken, out of its own budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Uses {
    #[default]
    Unlimited,
    /// A fixed budget for the whole fight: a breath weapon's free uses, a
    /// once-per-day ability.
    Limited(u32),
    /// Spent on use, and back on a `d6` of at least this value rolled at the
    /// start of each of the creature's turns. `Recharge(5)` is the printed
    /// "Recharge 5-6", which is why a dragon's breath is certain on round one
    /// and intermittent afterwards.
    Recharge(u32),
}

/// What kind of activity a move represents, beyond its damage/effect shape.
///
/// Almost every move needs nothing here - `Standard` covers plain attacks,
/// stances and saves, and nothing reads this tag at all by default. The two
/// other variants exist only so a plugin can recognise "this move is RAW an
/// Action" without the engine needing a separate "Use an Object" or "Magic"
/// action type of its own: `FastHandsPlugin` is the reader, and rejects
/// anything tagged `Standard` rather than silently promoting it - see
/// `crate::dsl::plugin::rogue`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MoveKind {
    #[default]
    Standard,
    /// The Use an Object action - drinking a potion, retrieving a hidden
    /// blade, activating a non-magical object.
    ObjectUse,
    /// Activating a magic item that would otherwise cost the Magic action -
    /// a wand, a staff, most consumable magic items.
    MagicItem,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Move {
    pub name: String,
    pub uses: Uses,
    /// Paid from a shared pool, on top of `uses`.
    pub cost: Option<Cost>,
    /// A spell slot level this move spends from the caster's own
    /// [`crate::rules::creature::SpellSlots`], separate from `cost`: a
    /// caster's slots are nine independent counters rather than one named
    /// pool, so they are not representable as a [`Cost`]. `None` for
    /// anything that is not a spell.
    pub spell_slot_level: Option<u32>,
    /// Fire when this move hits.
    pub riders: Vec<Rider>,
    pub effect: Effect,
    /// Requires concentration: taking this move ends whatever the user was
    /// already concentrating on, before anything else happens - even if this
    /// cast goes on to land nothing. Whatever condition it then applies, on
    /// whichever targets, becomes the new thing concentration is
    /// maintaining; see [`crate::sim::duel`] for how that is tracked and torn
    /// down. `false` for every ordinary move, which is why this defaults with
    /// the rest of [`Move::new`] rather than needing its own builder call.
    pub concentration: bool,
    /// RAW-Action bookkeeping for plugins like Fast Hands; see [`MoveKind`].
    /// Irrelevant to how the move actually resolves - only which list it
    /// ends up in.
    pub kind: MoveKind,
}

impl Move {
    pub fn new(name: impl Into<String>, effect: Effect) -> Self {
        Self {
            name: name.into(),
            uses: Uses::Unlimited,
            cost: None,
            spell_slot_level: None,
            riders: Vec::new(),
            effect,
            concentration: false,
            kind: MoveKind::Standard,
        }
    }

    pub fn with_uses(mut self, uses: Uses) -> Self {
        self.uses = uses;
        self
    }

    pub fn with_cost(mut self, cost: Cost) -> Self {
        self.cost = Some(cost);
        self
    }

    /// Mark this move as spending one spell slot of `level` when cast. See
    /// [`Move::pay_spell_cost`].
    pub fn with_spell_slot(mut self, level: u32) -> Self {
        self.spell_slot_level = Some(level);
        self
    }

    pub fn with_rider(mut self, rider: Rider) -> Self {
        self.riders.push(rider);
        self
    }

    pub fn with_concentration(mut self) -> Self {
        self.concentration = true;
        self
    }

    pub fn with_kind(mut self, kind: MoveKind) -> Self {
        self.kind = kind;
        self
    }

    /// A move that spends nothing is one a hoarding policy will still take.
    pub fn is_free(&self) -> bool {
        matches!(self.uses, Uses::Unlimited)
            && self.cost.is_none()
            && self.spell_slot_level.is_none()
    }

    /// Spend this move's spell slot, if it has one, from `caster`'s own
    /// pool. `true` and no change for a move that costs no slot; `false` and
    /// no change if the slot it needs is not available - the same shape as
    /// [`crate::rules::creature::SpellSlots::cast`], which this calls.
    pub fn pay_spell_cost(&self, caster: &mut Creature) -> bool {
        match self.spell_slot_level {
            None => true,
            Some(level) => caster.cast_spell(level),
        }
    }
}
