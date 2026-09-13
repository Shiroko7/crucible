//! Actions, strikes, saves, stances, and moves.

use crate::prob::dice::Pmf;
use crate::prob::rng::Rng;
use crate::rules::combat::{hit_outcomes, sample_hit, Landed, RollMode};

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

    fn landed_pmf(&self, target: &Creature, crit: bool) -> Pmf {
        self.damage.iter().fold(Pmf::constant(0), |acc, roll| {
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
        let o = hit_outcomes(self.to_hit, mode, target.ac);
        Pmf::mixture(&[
            (o.miss, Pmf::constant(0)),
            (o.hit, self.landed_pmf(target, false)),
            (o.crit, self.landed_pmf(target, true)),
        ])
    }

    pub fn damage_pmf(&self, target: &Creature) -> Pmf {
        self.damage_pmf_with(target, self.mode)
    }

    /// One sampled strike, reporting how it landed so a rider can key off the
    /// hit. Must be distributed according to [`Strike::damage_pmf_with`];
    /// `tests/duel_agreement.rs` requires it.
    pub fn sample_with(&self, rng: &mut Rng, target: &Creature, mode: RollMode) -> (i32, Landed) {
        let landed = sample_hit(rng, self.to_hit, mode, target.ac);
        let crit = match landed {
            Landed::Miss => return (0, landed),
            Landed::Hit => false,
            Landed::Crit => true,
        };
        let total = self
            .damage
            .iter()
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
    /// A condition applied when the save fails. Command, Hold Person, and
    /// every breath weapon whose text does more than deal damage. `None` for
    /// the plain damaging kind.
    pub on_failure: Option<(Condition, Duration)>,
    /// How many enemies it can catch. `None` means all of them, which is what a
    /// cone or a sphere does in the absence of a positioning model - the
    /// pessimistic reading. `Some(2)` is for something that names a number, like
    /// a second-level Command.
    pub max_targets: Option<u32>,
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
            Effect::Sequence(parts) => parts.iter().fold(Pmf::constant(0), |acc, p| {
                acc.convolve(&p.damage_pmf(target))
            }),
        }
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

#[derive(Debug, Clone, PartialEq)]
pub struct Move {
    pub name: String,
    pub uses: Uses,
    /// Paid from a shared pool, on top of `uses`.
    pub cost: Option<Cost>,
    /// Fire when this move hits.
    pub riders: Vec<Rider>,
    pub effect: Effect,
}

impl Move {
    pub fn new(name: impl Into<String>, effect: Effect) -> Self {
        Self {
            name: name.into(),
            uses: Uses::Unlimited,
            cost: None,
            riders: Vec::new(),
            effect,
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

    pub fn with_rider(mut self, rider: Rider) -> Self {
        self.riders.push(rider);
        self
    }

    /// A move that spends nothing is one a hoarding policy will still take.
    pub fn is_free(&self) -> bool {
        matches!(self.uses, Uses::Unlimited) && self.cost.is_none()
    }
}
