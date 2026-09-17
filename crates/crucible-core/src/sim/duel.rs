//! A fight: two sides, any number of creatures each, resolved round by round.
//!
//! It began as strictly one-against-one, and the 1v1 case is still what [`run`]
//! takes, because most of what has to be right is visible there: the action
//! economy, shared resource pools, recharge, reactions, conditions with
//! lifetimes, and legendary actions landing between turns rather than on them.
//!
//! Sides now hold more than one creature, which makes three things real that
//! were previously placeholders. Target selection actually chooses something, so
//! [`Policy::FocusFire`] and [`Policy::Scattered`] stop being aliases for
//! greedy. Area effects hit more than one creature. And a legendary monster gets
//! a window after *every* enemy's turn rather than one a round, which is most of
//! what a party of four changes about fighting one.
//!
//! Riders fire at five fixed points, which is where the event pipeline
//! `DESIGN.md` describes will eventually go:
//!
//! - **on being targeted, before an attack's hit or miss is finalized**,
//!   [`Rider::ReactionOnTargeted`] spends a reaction to add an AC bonus,
//!   capable of turning that one attack's hit into a miss;
//! - **on an incoming attack's damage**, [`Rider::ReduceDamage`] spends a
//!   reaction to cut it;
//! - **on a hit**, [`Rider::SaveOrCondition`] forces a save and may apply a
//!   condition;
//! - **on a failed save**, [`Rider::AlwaysSucceed`] may buy it back;
//! - **on a save for half**, [`Rider::NothingOnSuccess`] reshapes the outcome.
//!
//! Those points are chosen here rather than subscribed to, and a rider cannot
//! modify another rider. Getting from this to the real pipeline is a refactor of
//! this file, not of the data.
//!
//! **Positioning is still absent**, and with more than one creature a side that
//! costs more than it used to. There is no movement, reach or flight, so an area
//! effect is assumed to catch every enemy - the pessimistic reading, since a
//! party that spread out would not all be in one cone. `max_targets` on a saving
//! throw is the only control over that.

use crate::prob::rng::Rng;
use crate::rules::combat::{Landed, RollMode};
use crate::rules::creature::{
    apply_healing, Ability, AttackTrigger, Condition, Cost, Creature, Duration, Effect, Move,
    Rider, SpellSlots, Uses,
};

/// Which side of the fight. A side is a team, of any size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    A,
    B,
}

impl Side {
    pub fn index(self) -> usize {
        match self {
            Side::A => 0,
            Side::B => 1,
        }
    }

    pub fn other(self) -> Side {
        match self {
            Side::A => Side::B,
            Side::B => Side::A,
        }
    }

    pub fn from_team(team: u8) -> Side {
        if team == 0 {
            Side::A
        } else {
            Side::B
        }
    }
}

/// How a combatant picks its move, its target, and whether it spends anything.
///
/// This is the seam where the playstyle library from `DESIGN.md` plugs in. The
/// point of having nine rather than one is that the *spread between them* is the
/// output: if every playstyle wins, the encounter is filler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    /// Take the first usable move as written. Declaration order in the scenario
    /// file is the policy, which makes it the easiest to author against and the
    /// one to reach for when debugging.
    InOrder,
    /// Take whichever usable move has the highest exact mean damage. The ceiling
    /// of the non-searching policies, and still nowhere near optimal play: it
    /// never defends, never retreats, and never holds a resource for a better
    /// moment.
    Greedy,
    /// Spend nothing - no limited uses, no recharge moves, no resource pools, no
    /// Legendary Resistance. Models the player saving the slot for later and the
    /// DM running a dragon as a set of claws, which turn out to be the same
    /// policy.
    Thrifty,
    /// Front-load: spend the most expensive thing available first, breaking ties
    /// on damage. The table that ends fights in two rounds and has nothing left
    /// for the third.
    Nova,
    /// Spend nothing until bloodied, then everything. The most common real
    /// player - "I might need it later" - and the row that most often surprises
    /// a DM.
    Attrition,
    /// Concentrate on the enemy with the least HP left: finish the wounded one.
    /// A party that communicates, and a monster that knows which character to
    /// kill. Only distinguishable when the enemies differ, so against a single
    /// target it is greedy.
    FocusFire,
    /// Each combatant attacks a different enemy, spreading damage around. A
    /// party that does not communicate, and the mistake a DM makes when they
    /// want a fight to feel fair. Usually the weakest targeting rule there is.
    Scattered,
    /// Greedy until bloodied, then prefer a defensive stance for the bonus
    /// action. The spooked table.
    Defensive,
    /// Search: try every legal turn, play each out to a depth budget many times,
    /// and keep the one with the best estimated value.
    ///
    /// This is flat Monte Carlo - one ply of real choice, then rollouts - not
    /// the UCT tree search `DESIGN.md` asks for, and the difference is worth
    /// being honest about: it picks the best turn but cannot plan a sequence of
    /// them, and it does not search over targets. Putting a tree in its place is
    /// a change to one function.
    Solver,
}

impl Policy {
    /// Every policy, best-play first, so a report reads top to bottom as a
    /// descent from the ceiling.
    pub const ALL: [Policy; 9] = [
        Policy::Solver,
        Policy::Nova,
        Policy::Greedy,
        Policy::FocusFire,
        Policy::Scattered,
        Policy::InOrder,
        Policy::Defensive,
        Policy::Attrition,
        Policy::Thrifty,
    ];

    /// Everything except the search, for sweeps where rollouts inside rollouts
    /// are not worth the time.
    pub const CHEAP: [Policy; 8] = [
        Policy::Nova,
        Policy::Greedy,
        Policy::FocusFire,
        Policy::Scattered,
        Policy::InOrder,
        Policy::Defensive,
        Policy::Attrition,
        Policy::Thrifty,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Policy::InOrder => "in-order",
            Policy::Greedy => "greedy",
            Policy::Thrifty => "thrifty",
            Policy::Nova => "nova",
            Policy::Attrition => "attrition",
            Policy::FocusFire => "focus-fire",
            Policy::Scattered => "scattered",
            Policy::Defensive => "defensive",
            Policy::Solver => "solver",
        }
    }

    /// One line on what the row means, for a report that has nine of them.
    pub fn blurb(self) -> &'static str {
        match self {
            Policy::InOrder => "the first usable move, as written",
            Policy::Greedy => "always the highest mean damage available",
            Policy::Thrifty => "spends nothing, ever",
            Policy::Nova => "spends the most expensive thing first",
            Policy::Attrition => "spends nothing until bloodied",
            Policy::FocusFire => "finishes the most wounded enemy first",
            Policy::Scattered => "spreads attacks across enemies",
            Policy::Defensive => "defends once bloodied",
            Policy::Solver => "searches one turn ahead by rollout",
        }
    }
}

/// The per-fight availability of one move.
#[derive(Debug, Clone)]
struct MoveState {
    /// `None` for a move with no fixed budget.
    remaining: Option<u32>,
    /// Recharge moves are spent on use and roll to come back. Everything else
    /// stays charged forever.
    charged: bool,
}

impl MoveState {
    fn new(uses: Uses) -> Self {
        Self {
            remaining: match uses {
                Uses::Limited(n) => Some(n),
                _ => None,
            },
            charged: true,
        }
    }

    fn available(&self) -> bool {
        self.charged && self.remaining.is_none_or(|r| r > 0)
    }

    fn spend(&mut self, uses: Uses) {
        if let Some(r) = self.remaining.as_mut() {
            *r = r.saturating_sub(1);
        }
        if matches!(uses, Uses::Recharge(_)) {
            self.charged = false;
        }
    }

    fn roll_recharge(&mut self, uses: Uses, rng: &mut Rng) {
        if let Uses::Recharge(floor) = uses {
            if !self.charged && rng.die(6) >= floor as i32 {
                self.charged = true;
            }
        }
    }
}

#[derive(Clone)]
struct Fighter<'a> {
    creature: &'a Creature,
    side: Side,
    policy: Policy,
    /// Position in the roster, which is what [`Policy::Scattered`] rotates on so
    /// different party members pick different enemies.
    seat: usize,
    hp: i32,
    actions: Vec<MoveState>,
    bonus_actions: Vec<MoveState>,
    legendary: Vec<MoveState>,
    legendary_left: u32,
    /// Remaining points in each of the creature's shared pools.
    resources: Vec<u32>,
    /// Remaining spell slots, mirroring `creature.spell_slots` the same way
    /// `resources` mirrors `creature.resources`: a per-fight mutable copy, so
    /// the shared `Creature` behind every rollout stays untouched.
    spell_slots: SpellSlots,
    /// Remaining uses of each of the creature's own riders, parallel to
    /// `creature.riders`.
    rider_uses: Vec<u32>,
    /// Active conditions, each tagged with how it ends. See [`Expiry`].
    conditions: Vec<(Condition, Expiry)>,
    /// The one spell this creature is concentrating on, if any. See
    /// [`ActiveConcentration`].
    concentration: Option<ActiveConcentration>,
    once_per_turn_spent: bool,
    dealt: i64,
    /// Resource points and limited uses burnt. This is the second column of the
    /// table `DESIGN.md` wants, because a win probability means nothing without
    /// what it cost to get.
    spent: u32,
}

impl<'a> Fighter<'a> {
    fn new(creature: &'a Creature, side: Side, policy: Policy, seat: usize) -> Self {
        Self {
            creature,
            side,
            policy,
            seat,
            hp: creature.hp,
            actions: creature
                .actions
                .iter()
                .map(|m| MoveState::new(m.uses))
                .collect(),
            bonus_actions: creature
                .bonus_actions
                .iter()
                .map(|m| MoveState::new(m.uses))
                .collect(),
            legendary: creature
                .legendary
                .iter()
                .map(|m| MoveState::new(m.uses))
                .collect(),
            // Available from the start of the fight, not from the creature's
            // first turn. The difference only shows when the legendary creature
            // loses initiative, which is exactly when it matters.
            legendary_left: creature.legendary_uses,
            resources: creature.resources.iter().map(|r| r.max).collect(),
            spell_slots: creature.spell_slots,
            rider_uses: creature.riders.iter().map(|r| r.initial_uses()).collect(),
            conditions: Vec::new(),
            concentration: None,
            once_per_turn_spent: false,
            dealt: 0,
            spent: 0,
        }
    }

    fn alive(&self) -> bool {
        self.hp > 0
    }

    fn bloodied(&self) -> bool {
        self.hp * 2 <= self.creature.hp
    }

    fn has(&self, f: impl Fn(Condition) -> bool) -> bool {
        self.conditions.iter().any(|&(c, _)| f(c))
    }

    fn incapacitated(&self) -> bool {
        self.has(Condition::incapacitated)
    }

    /// Is this creature willing to spend a finite resource right now?
    ///
    /// One predicate for both hoarding policies, which differ only in when they
    /// relent. It gates move choice, on-hit riders and Legendary Resistance
    /// alike, so a thrifty dragon really does forget it has resistances.
    /// Reactions cost nothing and never consult it.
    fn will_spend(&self) -> bool {
        match self.policy {
            Policy::Thrifty => false,
            Policy::Attrition => self.bloodied(),
            _ => true,
        }
    }

    fn can_pay(&self, cost: Option<Cost>) -> bool {
        match cost {
            None => true,
            Some(c) => {
                self.will_spend()
                    && self
                        .resources
                        .get(c.resource)
                        .is_some_and(|&r| r >= c.amount)
            }
        }
    }

    fn pay(&mut self, cost: Option<Cost>) {
        if let Some(c) = cost {
            if let Some(r) = self.resources.get_mut(c.resource) {
                *r = r.saturating_sub(c.amount);
            }
            self.spent += c.amount;
        }
    }

    /// Is a slot of exactly `level` available and something this policy is
    /// willing to spend? A spell slot is exactly the kind of finite resource
    /// `will_spend` already gates `cost` behind.
    fn can_cast(&self, level: Option<u32>) -> bool {
        match level {
            None => true,
            Some(lvl) => self.will_spend() && self.spell_slots.available(lvl) > 0,
        }
    }

    /// Spend a slot of `level`, counting it the same way `pay` counts a
    /// resource cost.
    fn cast_spell_slot(&mut self, level: Option<u32>) {
        if let Some(lvl) = level {
            self.spell_slots.cast(lvl);
            self.spent += 1;
        }
    }

    /// Spend a move's own budget, counting it as a resource burnt.
    fn spend_move(&mut self, slot: Slot, pick: usize, uses: Uses) {
        if !matches!(uses, Uses::Unlimited) {
            self.spent += 1;
        }
        slot.states_mut(self)[pick].spend(uses);
    }

    fn add_condition(&mut self, condition: Condition, expiry: Expiry) {
        if !self.conditions.iter().any(|&(c, _)| c == condition) {
            self.conditions.push((condition, expiry));
        }
    }
}

/// How one active condition instance ends.
///
/// Splitting this out of [`Duration`] rather than storing the duration itself
/// keeps the roster indices - which only make sense once a condition has
/// actually been pinned to an applier and a victim - out of the data an
/// ability declares.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Expiry {
    /// Cleared at the start of this roster index's turn.
    TurnStart(usize),
    /// A saving throw at the end of the victim's own turn, clearing the
    /// condition on a success. See [`Duration::SaveEndTurn`].
    SaveEachTurn {
        victim: usize,
        ability: Ability,
        dc: i32,
    },
}

/// What a concentration spell is maintaining, so ending concentration can
/// clear it without the tracker needing to know which spell this is.
///
/// One variant today, because a condition applied to one or more targets is
/// the only kind of ongoing effect this engine can express yet - Hold
/// Person, Hypnotic Pattern. A later spell that maintains something else
/// instead - Bless and Bane are a flat modifier to a roll, not a condition -
/// adds a sibling here rather than teaching the clearing logic a hardcoded
/// case per spell.
#[derive(Debug, Clone, PartialEq)]
enum ConcentrationEffect {
    /// A condition maintained on one or more targets, all cleared at once
    /// when concentration ends.
    Condition {
        targets: Vec<usize>,
        condition: Condition,
    },
}

/// One creature's single slot of concentration.
///
/// 5e allows exactly one at a time, which is why this is an `Option` on
/// [`Fighter`] rather than a collection: starting a new one always replaces
/// whatever was here, clearing it first exactly as if it had failed a save.
#[derive(Debug, Clone, PartialEq)]
struct ActiveConcentration {
    effect: ConcentrationEffect,
}

/// DC for the Constitution save concentration takes when its holder takes
/// `damage`: 10, or half the damage taken, whichever is higher. 5e rounds
/// the half down, which integer division already does for a non-negative
/// dividend.
fn concentration_dc(damage: i32) -> i32 {
    (damage / 2).max(10)
}

/// How one fight ended. Everything is aggregated per side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// `None` when the round cap was reached with both sides still standing,
    /// which is a real result and not an error: some matchups genuinely cannot
    /// resolve, and averaging them in as a win would be a lie.
    pub winner: Option<Side>,
    pub rounds: u32,
    /// Creatures still standing on each side.
    pub survivors: [u32; 2],
    /// Creatures dropped on each side. For a party, the number that matters more
    /// than the win probability.
    pub deaths: [u32; 2],
    /// Total HP left across each side, a downed creature counting as zero.
    pub hp_left: [i32; 2],
    pub damage_dealt: [i64; 2],
    /// Turns lost entirely to being incapacitated.
    pub turns_lost: [u32; 2],
    /// Resource points and limited uses burnt.
    pub resources_spent: [u32; 2],
}

/// A turn's worth of choices.
///
/// Named and returnable because the search has to hand one back and have it
/// executed exactly, rather than re-deriving it afterwards and hoping the second
/// derivation matches the first. It carries no target: targeting is the policy's
/// job, so the search plans moves and not aim.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Plan {
    pub action: Option<usize>,
    pub bonus: Option<usize>,
}

/// What [`Policy::Solver`] is allowed to spend thinking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// Rollouts per candidate turn.
    pub rollouts: u32,
    /// Rounds to play out before giving up and scoring the position on health.
    /// The "depth budget" in `DESIGN.md`, and what keeps a search inside a
    /// rollout inside an evaluation affordable.
    pub depth: u32,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            rollouts: 16,
            depth: 4,
        }
    }
}

/// How much a spent resource counts against a win in the search's scoring.
///
/// The Lagrange multiplier from `DESIGN.md`, pinned very small rather than swept.
/// It has to stay well below the value of what a resource buys, or the search
/// hoards: one focus point spent on a Flurry moves the health margin by about
/// 0.015, so a penalty anywhere near that turns "spend it" into "do not".
/// Sweeping it to draw the Pareto frontier is a separate job, and this is the end
/// of the sweep where winning is all that matters.
const LAMBDA: f64 = 0.0005;

/// A fight in progress.
///
/// Split out of [`run`] so a search can clone one mid-fight and play it to the
/// end. A round is cut into two phases per combatant - its turn, then the
/// legendary windows that turn opens for the other side - so "resume from here"
/// is an index rather than a re-entrant call. That is also the shape a real tree
/// search over decision points needs, so it is a step towards one.
#[derive(Clone)]
struct Fight<'a> {
    fighters: Vec<Fighter<'a>>,
    /// Roster indices in initiative order.
    order: Vec<usize>,
    turns_lost: [u32; 2],
    max_rounds: u32,
    budget: Budget,
    /// Set only inside a search rollout: the creature under test keeps repeating
    /// the plan being evaluated instead of consulting a policy.
    ///
    /// Without this the rollout falls back to greedy play, and a plan whose whole
    /// value is in being repeated - a stun lock is the obvious one - scores the
    /// same as the move it is meant to beat. The rollout then answers "what is
    /// this turn worth if I keep doing it", which has a useful answer, rather
    /// than "what is this turn worth if I immediately stop", which does not.
    rollout_plan: Option<(usize, Plan)>,
}

impl<'a> Fight<'a> {
    fn new(
        rng: &mut Rng,
        roster: &[(&'a Creature, Side)],
        policies: [Policy; 2],
        max_rounds: u32,
        budget: Budget,
        log: &mut Option<Vec<String>>,
    ) -> Self {
        let fighters: Vec<Fighter<'a>> = roster
            .iter()
            .enumerate()
            .map(|(seat, &(creature, side))| {
                Fighter::new(creature, side, policies[side.index()], seat)
            })
            .collect();

        // One initiative roll each, highest first, ties to the better modifier
        // and then to the earlier seat.
        let mut rolled: Vec<(i32, i32, usize)> = fighters
            .iter()
            .enumerate()
            .map(|(i, f)| {
                (
                    rng.die(20) + f.creature.initiative,
                    f.creature.initiative,
                    i,
                )
            })
            .collect();
        rolled.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
        if let Some(l) = log.as_mut() {
            let line: Vec<String> = rolled
                .iter()
                .map(|&(total, _, i)| format!("{} {total}", fighters[i].creature.name))
                .collect();
            l.push(format!("initiative: {}", line.join(", ")));
        }

        Self {
            order: rolled.into_iter().map(|(_, _, i)| i).collect(),
            fighters,
            turns_lost: [0; 2],
            max_rounds,
            budget,
            rollout_plan: None,
        }
    }

    fn phases(&self) -> usize {
        self.order.len() * 2
    }

    /// Play from a given round and phase to the end.
    fn play(
        &mut self,
        rng: &mut Rng,
        from_round: u32,
        from_phase: usize,
        log: &mut Option<Vec<String>>,
    ) -> Outcome {
        let phases = self.phases();
        let mut phase = from_phase;
        for round in from_round..=self.max_rounds {
            while phase < phases {
                self.phase(round, phase, rng, log);
                if let Some(o) = self.finished(round) {
                    return o;
                }
                phase += 1;
            }
            phase = 0;
        }
        self.unresolved(self.max_rounds)
    }

    fn phase(&mut self, round: u32, phase: usize, rng: &mut Rng, log: &mut Option<Vec<String>>) {
        let who = self.order[phase / 2];
        if phase.is_multiple_of(2) {
            self.take_turn(round, who, rng, log, None);
        } else {
            self.legendary_windows(round, who, rng, log);
        }
    }

    /// Every combatant's turn opens a legendary window for the other side, and
    /// **one** use may be spent per window - not the whole pool.
    ///
    /// That distinction is invisible in a duel and decisive out of it. A dragon
    /// with three legendary uses facing one enemy gets one window a round and so
    /// spends one; facing four enemies it gets four windows and spends all three.
    /// Its legendary output scales with the size of the party opposing it, which
    /// is the opposite of the intuition that more bodies is straightforwardly
    /// better.
    fn legendary_windows(
        &mut self,
        round: u32,
        who: usize,
        rng: &mut Rng,
        log: &mut Option<Vec<String>>,
    ) {
        let side = self.fighters[who].side;
        for i in 0..self.fighters.len() {
            if self.fighters[i].side != side && self.fighters[i].alive() {
                self.legendary(round, i, rng, log);
            }
        }
    }

    fn living(&self, side: Side) -> u32 {
        self.fighters
            .iter()
            .filter(|f| f.side == side && f.alive())
            .count() as u32
    }

    fn finished(&self, round: u32) -> Option<Outcome> {
        let (a, b) = (self.living(Side::A), self.living(Side::B));
        let winner = match (a > 0, b > 0) {
            (true, false) => Some(Side::A),
            (false, true) => Some(Side::B),
            // A mutual wipe becomes possible once area effects exist, and a draw
            // is the honest answer rather than an assertion.
            (false, false) => None,
            (true, true) => return None,
        };
        let mut out = self.unresolved(round);
        out.winner = winner;
        Some(out)
    }

    fn unresolved(&self, round: u32) -> Outcome {
        let mut survivors = [0u32; 2];
        let mut deaths = [0u32; 2];
        let mut hp_left = [0i32; 2];
        let mut damage = [0i64; 2];
        let mut spent = [0u32; 2];
        for f in &self.fighters {
            let s = f.side.index();
            if f.alive() {
                survivors[s] += 1;
            } else {
                deaths[s] += 1;
            }
            hp_left[s] += f.hp.max(0);
            damage[s] += f.dealt;
            spent[s] += f.spent;
        }
        Outcome {
            winner: None,
            rounds: round,
            survivors,
            deaths,
            hp_left,
            damage_dealt: damage,
            turns_lost: self.turns_lost,
            resources_spent: spent,
        }
    }

    fn take_turn(
        &mut self,
        round: u32,
        who: usize,
        rng: &mut Rng,
        log: &mut Option<Vec<String>>,
        plan: Option<Plan>,
    ) {
        if !self.fighters[who].alive() {
            return;
        }
        if !self.turn(round, who, rng, log, plan) {
            self.turns_lost[self.fighters[who].side.index()] += 1;
        }
        // Runs whether or not the creature actually got to act: a paralyzed
        // creature still reaches the end of its own turn, which is exactly
        // when its next chance to shake the condition off falls.
        self.end_of_turn_saves(round, who, rng, log);
    }

    /// Returns whether the creature actually got to act.
    fn turn(
        &mut self,
        round: u32,
        me: usize,
        rng: &mut Rng,
        log: &mut Option<Vec<String>>,
        plan: Option<Plan>,
    ) -> bool {
        // Conditions that end at the start of this creature's turn, wherever they
        // sit - the stun a monk landed ends on the monk's turn, not the dragon's.
        // A `SaveEachTurn` condition is untouched here; it only ever leaves via
        // `end_of_turn_saves`.
        for f in self.fighters.iter_mut() {
            f.conditions.retain(
                |&(_, expiry)| !matches!(expiry, Expiry::TurnStart(cleared_by) if cleared_by == me),
            );
        }

        let creature = self.fighters[me].creature;
        // Recharge is rolled at the start of the creature's turn; reactions and
        // legendary actions come back then too.
        refresh(&mut self.fighters[me], rng);

        if self.fighters[me].incapacitated() {
            if let Some(l) = log.as_mut() {
                let names: Vec<&str> = self.fighters[me]
                    .conditions
                    .iter()
                    .map(|&(c, _)| c.name())
                    .collect();
                l.push(format!(
                    "r{round} {}: loses its turn ({})",
                    creature.name,
                    names.join(", ")
                ));
            }
            return false;
        }

        let Some(mut target) = self.pick_target(me) else {
            return true; // nothing left to hit
        };

        let plan = match plan {
            Some(p) => p,
            None => self.decide(round, me, target, rng),
        };

        let record = log.is_some();
        let mut line = String::new();
        for (slot, pick) in [(Slot::Action, plan.action), (Slot::Bonus, plan.bonus)] {
            let Some(pick) = pick else { continue };
            if !self.fighters[target].alive() {
                let Some(new_target) = self.pick_target(me) else {
                    break;
                };
                target = new_target;
            }
            let moves = slot.moves(creature);
            // A plan is checked rather than trusted: a searched one was legal
            // when it was chosen, and nothing since should have changed that, but
            // "should" is not a guarantee.
            if pick >= moves.len() {
                continue;
            }
            let chosen = &moves[pick];
            if !slot.states(&self.fighters[me])[pick].available()
                || !self.fighters[me].can_pay(chosen.cost)
                || !self.fighters[me].can_cast(chosen.spell_slot_level)
            {
                continue;
            }
            self.fighters[me].pay(chosen.cost);
            self.fighters[me].cast_spell_slot(chosen.spell_slot_level);
            self.fighters[me].spend_move(slot, pick, chosen.uses);
            self.apply(chosen, rng, me, target, record, &mut line);
        }

        if let Some(l) = log.as_mut() {
            if !line.is_empty() {
                l.push(format!(
                    "r{round} {}: {line}  [{} {} hp]",
                    creature.name,
                    self.fighters[target].creature.name,
                    self.fighters[target].hp.max(0)
                ));
            }
        }
        true
    }

    /// Repeat the saving throw behind any `Duration::SaveEndTurn` condition
    /// `me` is carrying, at the end of `me`'s own turn, clearing it on a
    /// success.
    ///
    /// A fixed point rather than something a condition polls for itself,
    /// because "the end of its turn" is a moment in the round structure and
    /// this is the one place that moment is visible. Runs after `turn`
    /// whether or not it returned `true`: an incapacitating condition still
    /// reaches the end of the turn it stole, which is exactly when it is next
    /// due to be shaken off.
    fn end_of_turn_saves(
        &mut self,
        round: u32,
        me: usize,
        rng: &mut Rng,
        log: &mut Option<Vec<String>>,
    ) {
        if !self.fighters[me].alive() {
            return;
        }
        let pending: Vec<(Condition, Ability, i32)> = self.fighters[me]
            .conditions
            .iter()
            .filter_map(|&(condition, expiry)| match expiry {
                Expiry::SaveEachTurn { ability, dc, .. } => Some((condition, ability, dc)),
                Expiry::TurnStart(_) => None,
            })
            .collect();

        for (condition, ability, dc) in pending {
            let (saved, resisted) = saving_throw(&mut self.fighters, rng, me, ability, dc);
            if !saved {
                continue;
            }
            self.fighters[me]
                .conditions
                .retain(|&(c, _)| c != condition);
            if let Some(l) = log.as_mut() {
                let how = if resisted {
                    "legendary resistance"
                } else {
                    "a save"
                };
                l.push(format!(
                    "r{round} {}: shakes off {} ({how})",
                    self.fighters[me].creature.name,
                    condition.name()
                ));
            }
        }
    }

    /// Which enemy this creature goes after.
    ///
    /// Three rules, and the differences between them only exist because a side
    /// can hold more than one creature:
    ///
    /// - `FocusFire` finishes the most wounded enemy. Against a party this is how
    ///   a monster removes damage output fastest, since a dead character
    ///   contributes nothing and a nearly-dead one contributes everything.
    /// - `Scattered` rotates on seat, so different attackers hit different
    ///   enemies and nobody dies.
    /// - everything else attacks the first living enemy in roster order, which is
    ///   neither coordinated nor deliberately spread.
    fn pick_target(&self, me: usize) -> Option<usize> {
        let side = self.fighters[me].side;
        let enemies: Vec<usize> = (0..self.fighters.len())
            .filter(|&i| self.fighters[i].side != side && self.fighters[i].alive())
            .collect();
        if enemies.is_empty() {
            return None;
        }
        Some(match self.fighters[me].policy {
            Policy::FocusFire => *enemies
                .iter()
                .min_by_key(|&&i| self.fighters[i].hp)
                .expect("non-empty"),
            Policy::Scattered => enemies[self.fighters[me].seat % enemies.len()],
            _ => enemies[0],
        })
    }

    /// What to do this turn: ask the search if this creature searches, otherwise
    /// ask the policy for each slot.
    fn decide(&self, round: u32, me: usize, target: usize, rng: &mut Rng) -> Plan {
        if let Some((who, plan)) = self.rollout_plan {
            if who == me {
                return self.repair(me, target, plan);
            }
        }
        let f = &self.fighters[me];
        if f.policy == Policy::Solver {
            return self.search(round, me, rng);
        }
        let against = self.fighters[target].creature;
        Plan {
            action: f
                .policy
                .choose(Slot::Action.moves(f.creature), &f.actions, f, against),
            bonus: f
                .policy
                .choose(Slot::Bonus.moves(f.creature), &f.bonus_actions, f, against),
        }
    }

    /// Keep a rollout on the plan under test, falling back slot by slot to greedy
    /// play when a pick is no longer legal - a pool that has run dry, a breath
    /// weapon that has not recharged.
    ///
    /// A deliberate `None` stays `None`, so "do nothing with the bonus action" is
    /// still evaluated as itself.
    fn repair(&self, me: usize, target: usize, plan: Plan) -> Plan {
        let f = &self.fighters[me];
        let against = self.fighters[target].creature;
        let fix = |slot: Slot, pick: Option<usize>| -> Option<usize> {
            let still_legal = pick.is_some_and(|i| {
                slot.moves(f.creature).get(i).is_some_and(|m| {
                    slot.states(f)[i].available()
                        && f.can_pay(m.cost)
                        && f.can_cast(m.spell_slot_level)
                })
            });
            if pick.is_none() || still_legal {
                pick
            } else {
                Policy::Greedy.choose(slot.moves(f.creature), slot.states(f), f, against)
            }
        };
        Plan {
            action: fix(Slot::Action, plan.action),
            bonus: fix(Slot::Bonus, plan.bonus),
        }
    }

    /// Every move that could legally be taken in this slot, plus doing nothing.
    ///
    /// Doing nothing goes last so the strict comparison in [`Fight::search`]
    /// breaks ties towards acting. A search that idles because idling scored
    /// equal-worst looks broken and is hard to tell from one that is.
    fn legal(&self, me: usize, slot: Slot) -> Vec<Option<usize>> {
        let f = &self.fighters[me];
        let mut out = Vec::new();
        for (i, (m, state)) in slot
            .moves(f.creature)
            .iter()
            .zip(slot.states(f))
            .enumerate()
        {
            if state.available() && f.can_pay(m.cost) && f.can_cast(m.spell_slot_level) {
                out.push(Some(i));
            }
        }
        out.push(None);
        out
    }

    /// Flat Monte Carlo over this turn's legal plans.
    ///
    /// For each candidate, clone the fight, play that exact turn, then let
    /// everyone play on to the depth budget, and average the value of where it
    /// lands. One ply of real choice; see [`Policy::Solver`] for what that does
    /// and does not buy.
    fn search(&self, round: u32, me: usize, rng: &mut Rng) -> Plan {
        let side = self.fighters[me].side;
        let max_hp = self.side_max_hp();
        let seat = self
            .order
            .iter()
            .position(|&i| i == me)
            .expect("every fighter has a place in the order");
        let resume = seat * 2 + 1;
        let depth_limit = self.max_rounds.min(round + self.budget.depth);

        let mut best = (Plan::default(), f64::NEG_INFINITY);
        for action in self.legal(me, Slot::Action) {
            for bonus in self.legal(me, Slot::Bonus) {
                let plan = Plan { action, bonus };
                let mut total = 0.0;
                for _ in 0..self.budget.rollouts {
                    let mut trial = self.clone();
                    trial.max_rounds = depth_limit;
                    // The rollout repeats the plan under test, and greedy play is
                    // only its fallback - so a search can never recurse into
                    // another search, even with multiple searchers on the team.
                    for f in &mut trial.fighters {
                        if f.policy == Policy::Solver {
                            f.policy = Policy::Greedy;
                        }
                    }
                    trial.rollout_plan = Some((me, plan));
                    let mut quiet = None;
                    trial.take_turn(round, me, rng, &mut quiet, Some(plan));
                    let outcome = match trial.finished(round) {
                        Some(o) => o,
                        None => trial.play(rng, round, resume, &mut quiet),
                    };
                    total += value(&outcome, side, max_hp);
                }
                let score = total / f64::from(self.budget.rollouts.max(1));
                if score > best.1 {
                    best = (plan, score);
                }
            }
        }
        best.0
    }

    fn side_max_hp(&self) -> [i32; 2] {
        let mut out = [0i32; 2];
        for f in &self.fighters {
            out[f.side.index()] += f.creature.hp;
        }
        out
    }

    fn legendary(&mut self, round: u32, me: usize, rng: &mut Rng, log: &mut Option<Vec<String>>) {
        let creature = self.fighters[me].creature;
        if creature.legendary.is_empty()
            || !self.fighters[me].alive()
            || self.fighters[me].incapacitated()
        {
            return;
        }

        if self.fighters[me].legendary_left == 0 {
            return;
        }
        let Some(target) = self.pick_target(me) else {
            return;
        };

        let record = log.is_some();
        let mut line = String::new();
        let against = self.fighters[target].creature;
        let pick = {
            let f = &self.fighters[me];
            // Legendary actions are chosen by policy even for the search, which
            // only plans whole turns. Searching them too would multiply the
            // rollout count by the number of windows.
            f.policy
                .choose(&creature.legendary, &f.legendary, f, against)
        };
        let Some(pick) = pick else { return };
        let chosen = &creature.legendary[pick];

        self.fighters[me].legendary_left -= 1;
        self.fighters[me].pay(chosen.cost);
        self.fighters[me].spend_move(Slot::Legendary, pick, chosen.uses);
        self.apply(chosen, rng, me, target, record, &mut line);

        if let Some(l) = log.as_mut() {
            if !line.is_empty() {
                l.push(format!(
                    "r{round} {} (legendary): {line}  [{} {} hp]",
                    creature.name,
                    self.fighters[target].creature.name,
                    self.fighters[target].hp.max(0)
                ));
            }
        }
    }

    /// Resolve one move and apply everything it does.
    ///
    /// A concentration move drops whatever the user was already maintaining
    /// before it does anything else - even a cast that lands on nobody still
    /// ends the old spell - and then, if it landed a condition on anyone,
    /// that becomes the new thing concentration maintains.
    fn apply(
        &mut self,
        m: &Move,
        rng: &mut Rng,
        me: usize,
        target: usize,
        record: bool,
        line: &mut String,
    ) {
        if m.concentration {
            self.end_concentration(me);
        }
        let mut notes: Vec<String> = Vec::new();
        let mut landed: Vec<(usize, Condition)> = Vec::new();
        self.resolve(
            &m.effect,
            rng,
            me,
            target,
            &m.riders,
            record,
            &mut notes,
            &mut landed,
        );
        if m.concentration {
            if let Some(&(_, condition)) = landed.first() {
                self.fighters[me].concentration = Some(ActiveConcentration {
                    effect: ConcentrationEffect::Condition {
                        targets: landed.iter().map(|&(t, _)| t).collect(),
                        condition,
                    },
                });
            }
        }
        if record {
            if !line.is_empty() {
                line.push_str(" | ");
            }
            line.push_str(&m.name);
            if !notes.is_empty() {
                line.push_str(" (");
                line.push_str(&notes.join(", "));
                line.push(')');
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve(
        &mut self,
        effect: &Effect,
        rng: &mut Rng,
        me: usize,
        target: usize,
        move_riders: &[Rider],
        record: bool,
        notes: &mut Vec<String>,
        landed_conditions: &mut Vec<(usize, Condition)>,
    ) {
        match effect {
            Effect::Strikes { strike, count } => {
                let mut current_target = target;
                let mut against = self.fighters[current_target].creature;
                let mut mode = attack_mode(
                    strike.mode,
                    &self.fighters[me],
                    &self.fighters[current_target],
                );
                // Paralyzed: any hit against it is an automatic critical hit.
                let mut force_crit = self.fighters[current_target].has(Condition::auto_crits);
                for _ in 0..*count {
                    if !self.fighters[current_target].alive() {
                        let Some(new_target) = self.pick_target(me) else {
                            break;
                        };
                        current_target = new_target;
                        against = self.fighters[current_target].creature;
                        mode = attack_mode(
                            strike.mode,
                            &self.fighters[me],
                            &self.fighters[current_target],
                        );
                        force_crit = self.fighters[current_target].has(Condition::auto_crits);
                    }
                    let reaction =
                        ac_boost_reaction(&self.fighters[current_target], AttackTrigger::AnyAttack);
                    let (ac_bonus, reaction_available) = match reaction {
                        Some((_, bonus)) => (bonus, true),
                        None => (0, false),
                    };
                    let (raw, landed, consumed) = strike.sample_forcing_crit_with_reaction(
                        rng,
                        against,
                        mode,
                        force_crit,
                        ac_bonus,
                        reaction_available,
                    );
                    if consumed {
                        if let Some((i, _)) = reaction {
                            self.fighters[current_target].rider_uses[i] -= 1;
                        }
                    }
                    let (dealt, cut) =
                        reduce_incoming(rng, &mut self.fighters[current_target], strike, raw);
                    self.fighters[me].dealt += i64::from(dealt);
                    self.apply_damage(rng, current_target, dealt);
                    if record {
                        notes.push(match landed {
                            Landed::Miss if consumed => "miss (AC boosted)".to_string(),
                            Landed::Miss => "miss".to_string(),
                            _ if cut > 0 => format!("{dealt} (deflected {cut})"),
                            Landed::Crit => format!("{dealt} crit"),
                            Landed::Hit => dealt.to_string(),
                        });
                    }
                    if landed != Landed::Miss {
                        self.fire_on_hit(
                            rng,
                            me,
                            current_target,
                            move_riders,
                            record,
                            notes,
                            landed_conditions,
                        );
                    }
                }
            }
            Effect::Save(save) => {
                // No positioning, so an area effect catches every enemy up to its
                // target cap. Pessimistic, and stated as such.
                let side = self.fighters[me].side;
                let mut caught: Vec<usize> = (0..self.fighters.len())
                    .filter(|&i| self.fighters[i].side != side && self.fighters[i].alive())
                    // A type-restricted save (Hold Person's "humanoid") never
                    // catches anything else at all - not even a rolled save
                    // that then does nothing, the same way `max_targets` caps
                    // who is caught rather than who saves.
                    .filter(|&i| match &save.requires_type {
                        None => true,
                        Some(t) => self.fighters[i].creature.is_creature_type(t),
                    })
                    .collect();
                if let Some(max) = save.max_targets {
                    caught.truncate(max as usize);
                }
                for i in caught {
                    let against = self.fighters[i].creature;
                    let (saved, resisted) =
                        saving_throw(&mut self.fighters, rng, i, save.ability, save.dc);
                    // Evasion is explicitly unavailable while Incapacitated.
                    let evasion = against.has_evasion(save.ability)
                        && !self.fighters[i].has(Condition::blocks_riders);
                    let dealt = save.sample_known(rng, against, saved, evasion);
                    self.fighters[me].dealt += i64::from(dealt);
                    self.apply_damage(rng, i, dealt);
                    if let (false, Some((condition, duration))) = (saved, save.on_failure) {
                        self.apply_condition(i, condition, expiry(me, i, duration));
                        landed_conditions.push((i, condition));
                    }
                    if record {
                        let how = match (saved, resisted) {
                            (true, true) => "legendary resistance",
                            (true, false) => "saved",
                            _ => "failed",
                        };
                        let extra = match (saved, save.on_failure) {
                            (false, Some((condition, _))) => format!(" and {}", condition.name()),
                            _ => String::new(),
                        };
                        notes.push(format!(
                            "{} {how} for {dealt}{extra}",
                            self.fighters[i].creature.name
                        ));
                    }
                }
            }
            Effect::Stance { condition } => {
                self.apply_condition(me, *condition, Expiry::TurnStart(me));
                landed_conditions.push((me, *condition));
                if record {
                    notes.push(condition.name().to_string());
                }
            }
            Effect::Heal(roll) => {
                // Healing Word, Cure Wounds. `target` is whoever this move was
                // aimed at, exactly like a strike or a save - the engine only
                // ever targets the opposing side today (see
                // `dsl::plugin::spells`'s module doc), so in the current duel
                // loop this only ever fires on an enemy. The math is written
                // to be right regardless of who it lands on, ready for the
                // day a policy can aim it at a downed ally instead.
                let healed = roll.sample(rng);
                let current_hp = self.fighters[target].hp;
                let max_hp = self.fighters[target].creature.hp;
                let (new_hp, revived) = apply_healing(current_hp, max_hp, healed);
                self.fighters[target].hp = new_hp;
                if record {
                    notes.push(if revived {
                        format!("heals {healed} (revives)")
                    } else {
                        format!("heals {healed}")
                    });
                }
            }
            Effect::Sequence(parts) => {
                for part in parts {
                    self.resolve(
                        part,
                        rng,
                        me,
                        target,
                        move_riders,
                        record,
                        notes,
                        landed_conditions,
                    );
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn fire_on_hit(
        &mut self,
        rng: &mut Rng,
        me: usize,
        target: usize,
        move_riders: &[Rider],
        record: bool,
        notes: &mut Vec<String>,
        landed_conditions: &mut Vec<(usize, Condition)>,
    ) {
        for rider in move_riders {
            let Rider::SaveOrCondition {
                ability,
                dc,
                condition,
                duration,
                cost,
                once_per_turn,
            } = rider
            else {
                continue;
            };
            if *once_per_turn && self.fighters[me].once_per_turn_spent {
                continue;
            }
            if !self.fighters[me].can_pay(*cost) {
                continue;
            }
            self.fighters[me].pay(*cost);
            if *once_per_turn {
                self.fighters[me].once_per_turn_spent = true;
            }

            let (saved, resisted) = saving_throw(&mut self.fighters, rng, target, *ability, *dc);
            if !saved {
                self.apply_condition(target, *condition, expiry(me, target, *duration));
                landed_conditions.push((target, *condition));
            }
            if record {
                notes.push(format!(
                    "{} {}",
                    condition.name(),
                    match (saved, resisted) {
                        (true, true) => "shrugged off (legendary resistance)",
                        (true, false) => "saved",
                        _ => "LANDED",
                    }
                ));
            }
        }
    }

    /// Add `condition` to `victim`, then end their concentration if this
    /// takes away their turn. An Incapacitated creature cannot concentrate on
    /// anything, and 5e offers no save against losing it that way - unlike
    /// damage, which gets one.
    fn apply_condition(&mut self, victim: usize, condition: Condition, expiry: Expiry) {
        self.fighters[victim].add_condition(condition, expiry);
        if condition.incapacitated() {
            self.end_concentration(victim);
        }
    }

    /// Apply damage already run through resistance and reactions, then
    /// handle what it does to the target's concentration: dropping to 0 HP
    /// ends it outright (no save offered, same as Incapacitated), and
    /// surviving damage forces the save that might end it anyway.
    fn apply_damage(&mut self, rng: &mut Rng, target: usize, dealt: i32) {
        self.fighters[target].hp -= dealt;
        if !self.fighters[target].alive() {
            self.end_concentration(target);
        } else if dealt > 0 {
            self.concentration_check(rng, target, dealt);
        }
    }

    /// The Constitution save concentration takes when its holder is
    /// damaged. Reuses [`saving_throw`], so an auto-failing condition and
    /// Legendary Resistance both apply to it exactly as they do to any other
    /// save.
    fn concentration_check(&mut self, rng: &mut Rng, who: usize, damage: i32) {
        if self.fighters[who].concentration.is_none() {
            return;
        }
        let dc = concentration_dc(damage);
        let (saved, _resisted) = saving_throw(&mut self.fighters, rng, who, Ability::Con, dc);
        if !saved {
            self.end_concentration(who);
        }
    }

    /// End `who`'s concentration, if they have any, clearing whatever it was
    /// maintaining from every combatant it was maintained on.
    fn end_concentration(&mut self, who: usize) {
        let Some(active) = self.fighters[who].concentration.take() else {
            return;
        };
        match active.effect {
            ConcentrationEffect::Condition { targets, condition } => {
                for t in targets {
                    if let Some(f) = self.fighters.get_mut(t) {
                        f.conditions.retain(|&(c, _)| c != condition);
                    }
                }
            }
        }
    }
}

/// How a just-applied condition ends, once `duration` is pinned to the
/// specific applier and victim that made it real.
fn expiry(applier: usize, victim: usize, duration: Duration) -> Expiry {
    match duration {
        Duration::ApplierTurn => Expiry::TurnStart(applier),
        Duration::VictimTurn => Expiry::TurnStart(victim),
        Duration::SaveEndTurn { ability, dc } => Expiry::SaveEachTurn {
            victim,
            ability,
            dc,
        },
    }
}

/// Which of the three move lists is being read. Exists so the borrow of a list
/// and the borrow of its per-fight state cannot drift apart.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    Action,
    Bonus,
    Legendary,
}

impl Slot {
    fn moves(self, c: &Creature) -> &[Move] {
        match self {
            Slot::Action => &c.actions,
            Slot::Bonus => &c.bonus_actions,
            Slot::Legendary => &c.legendary,
        }
    }

    fn states<'f>(self, f: &'f Fighter<'_>) -> &'f [MoveState] {
        match self {
            Slot::Action => &f.actions,
            Slot::Bonus => &f.bonus_actions,
            Slot::Legendary => &f.legendary,
        }
    }

    fn states_mut<'f>(self, f: &'f mut Fighter<'_>) -> &'f mut [MoveState] {
        match self {
            Slot::Action => &mut f.actions,
            Slot::Bonus => &mut f.bonus_actions,
            Slot::Legendary => &mut f.legendary,
        }
    }
}

/// Everything that comes back at the start of a creature's turn: recharge rolls,
/// its reaction, its legendary actions, and its once-per-turn riders.
fn refresh(f: &mut Fighter<'_>, rng: &mut Rng) {
    let creature = f.creature;
    for slot in [Slot::Action, Slot::Bonus, Slot::Legendary] {
        let moves = slot.moves(creature);
        let states = slot.states_mut(f);
        for (state, m) in states.iter_mut().zip(moves) {
            state.roll_recharge(m.uses, rng);
        }
    }
    f.legendary_left = creature.legendary_uses;
    f.once_per_turn_spent = false;
    for (uses, rider) in f.rider_uses.iter_mut().zip(&creature.riders) {
        match rider {
            Rider::ReduceDamage { per_round, .. } | Rider::ReactionOnTargeted { per_round, .. } => {
                *uses = *per_round;
            }
            _ => {}
        }
    }
}

impl Policy {
    /// Index of the chosen move, or `None` if nothing is available.
    ///
    /// Ties go to the earlier move, so the choice is a function of the scenario
    /// file and not of floating-point noise.
    fn choose(
        self,
        moves: &[Move],
        states: &[MoveState],
        f: &Fighter<'_>,
        target: &Creature,
    ) -> Option<usize> {
        let bloodied = f.bloodied();
        let mut best: Option<(usize, (f64, f64))> = None;
        for (i, (m, state)) in moves.iter().zip(states).enumerate() {
            if !state.available() || !f.can_pay(m.cost) || !f.can_cast(m.spell_slot_level) {
                continue;
            }
            // A hoarding policy passes over anything with a price on it, not just
            // anything drawn from a pool.
            if !m.is_free() && !f.will_spend() {
                continue;
            }
            if self == Policy::InOrder {
                return Some(i);
            }
            let rank = self.rank(m, target, bloodied);
            if best.is_none_or(|(_, b)| rank.0 > b.0 || (rank.0 == b.0 && rank.1 > b.1)) {
                best = Some((i, rank));
            }
        }
        best.map(|(i, _)| i)
    }

    /// A move's desirability, primary key then tie-break.
    ///
    /// Two keys rather than one number because Nova ranks on cost first and
    /// damage second, and folding that into a single score needs a magic
    /// multiplier that silently breaks the moment a damage figure exceeds it.
    ///
    /// `FocusFire` and `Scattered` land in the same arm as `Greedy`: they are
    /// rules about *which target*, which is [`Fight::pick_target`]'s job.
    fn rank(self, m: &Move, target: &Creature, bloodied: bool) -> (f64, f64) {
        let damage = m.effect.mean_damage(target);
        match self {
            Policy::Nova => (f64::from(spend_weight(m)), damage),
            // Once things are going badly a stance beats any amount of damage,
            // and before that it is worth nothing.
            Policy::Defensive if bloodied && m.effect.stance().is_some() => (f64::INFINITY, 0.0),
            _ => (damage, 0.0),
        }
    }
}

/// How much of a finite resource a move burns, for a policy that wants to burn
/// them. A pool cost counts by its size; any private budget counts as one.
fn spend_weight(m: &Move) -> u32 {
    m.cost.map_or(0, |c| c.amount)
        + u32::from(!matches!(m.uses, Uses::Unlimited))
        + m.spell_slot_level.unwrap_or(0)
}

/// The 5e stacking rule: any advantage and any disadvantage cancel to a flat
/// roll, however many of each there are.
fn attack_mode(base: RollMode, attacker: &Fighter<'_>, target: &Fighter<'_>) -> RollMode {
    let mut advantage = base == RollMode::Advantage;
    let mut disadvantage = base == RollMode::Disadvantage;
    for &(c, _) in &target.conditions {
        advantage |= c.advantage_to_attackers();
        disadvantage |= c.disadvantage_to_attackers();
    }
    for &(c, _) in &attacker.conditions {
        disadvantage |= c.disadvantage_on_attacks();
    }
    match (advantage, disadvantage) {
        (true, false) => RollMode::Advantage,
        (false, true) => RollMode::Disadvantage,
        _ => RollMode::Normal,
    }
}

/// Roll a saving throw, letting conditions force a failure and
/// [`Rider::AlwaysSucceed`] buy one back.
fn saving_throw(
    fighters: &mut [Fighter<'_>],
    rng: &mut Rng,
    who: usize,
    ability: crate::rules::creature::Ability,
    dc: i32,
) -> (bool, bool) {
    let f = &fighters[who];
    let auto_fail = f.has(|c| c.auto_fails(ability));
    let rolled = !auto_fail && rng.die(20) + f.creature.save(ability) >= dc;
    if rolled {
        return (true, false);
    }

    // Legendary Resistance, and anything else shaped like it. A hoarding policy
    // never reaches for it, which is most of what separates a well-run monster
    // from a badly run one.
    if fighters[who].will_spend() {
        let creature = fighters[who].creature;
        let mut slot = None;
        for (i, rider) in creature.riders.iter().enumerate() {
            if matches!(rider, Rider::AlwaysSucceed { .. }) && fighters[who].rider_uses[i] > 0 {
                slot = Some(i);
                break;
            }
        }
        if let Some(i) = slot {
            fighters[who].rider_uses[i] -= 1;
            return (true, true);
        }
    }
    (false, false)
}

/// The AC bonus [`Rider::ReactionOnTargeted`] offers against an incoming
/// attack matching `trigger`, and which rider slot it would spend - so the
/// caller can debit the right per-round counter once it learns whether the
/// reaction actually fired. `None` when the creature has no such rider for
/// this trigger, or its reaction is already spent this round.
fn ac_boost_reaction(f: &Fighter<'_>, trigger: AttackTrigger) -> Option<(usize, i32)> {
    f.creature.riders.iter().enumerate().find_map(|(i, rider)| {
        let Rider::ReactionOnTargeted {
            trigger: t,
            ac_bonus,
            ..
        } = rider
        else {
            return None;
        };
        if *t != trigger || f.rider_uses[i] == 0 {
            return None;
        }
        Some((i, *ac_bonus))
    })
}

/// Spend a reaction to cut an incoming attack's damage, if anything can.
fn reduce_incoming(
    rng: &mut Rng,
    f: &mut Fighter<'_>,
    strike: &crate::rules::creature::Strike,
    damage: i32,
) -> (i32, i32) {
    if damage <= 0 {
        return (damage, 0);
    }
    let creature = f.creature;
    for (i, rider) in creature.riders.iter().enumerate() {
        if let Rider::ReduceDamage { kinds, roll, .. } = rider {
            if f.rider_uses[i] == 0 || !strike.deals_any(kinds) {
                continue;
            }
            f.rider_uses[i] -= 1;
            let cut = roll.sample_raw(rng).min(damage);
            return (damage - cut, cut);
        }
    }
    (damage, 0)
}

/// What a finished or truncated rollout is worth to `side`.
///
/// Winning dominates: it is worth 2, against a margin term that can never
/// exceed 1. The margin is how much healthier this side finished, and it counts
/// on every rollout rather than only the truncated ones.
///
/// That last part matters more than it looks. `P(win) - lambda * spent` alone,
/// which is the objective `DESIGN.md` states, is degenerate in a position that
/// cannot be won: every line scores zero, so the only term left is the resource
/// penalty, and the search correctly concludes the best thing to do is nothing at
/// all. It stands still saving its focus points while it is eaten. That is
/// optimal play under the stated objective and useless as a report, so the margin
/// term is what keeps a hopeless row describing a real fight - and it is also
/// what lets the search prefer a line that nearly won.
fn value(o: &Outcome, side: Side, max_hp: [i32; 2]) -> f64 {
    let (me, you) = (side.index(), side.other().index());
    let share = |hp: i32, max: i32| f64::from(hp.max(0)) / f64::from(max.max(1));
    let margin =
        0.5 + 0.5 * (share(o.hp_left[me], max_hp[me]) - share(o.hp_left[you], max_hp[you]));
    let outcome = match o.winner {
        Some(w) if w == side => 2.0,
        _ => 0.0,
    };
    outcome + margin - LAMBDA * f64::from(o.resources_spent[me])
}

/// Resolve a single one-against-one fight.
///
/// `log`, when `Some`, collects a readable round-by-round account. It is off by
/// default because formatting a string per attack costs more than the fight does,
/// and a hundred thousand rollouts do not want narrating.
pub fn run(
    rng: &mut Rng,
    sides: [&Creature; 2],
    policies: [Policy; 2],
    max_rounds: u32,
    log: &mut Option<Vec<String>>,
) -> Outcome {
    run_with(rng, sides, policies, max_rounds, Budget::default(), log)
}

/// [`run`], with the search budget spelled out.
pub fn run_with(
    rng: &mut Rng,
    sides: [&Creature; 2],
    policies: [Policy; 2],
    max_rounds: u32,
    budget: Budget,
    log: &mut Option<Vec<String>>,
) -> Outcome {
    let roster = [(sides[0], Side::A), (sides[1], Side::B)];
    let mut fight = Fight::new(rng, &roster, policies, max_rounds, budget, log);
    fight.play(rng, 1, 0, log)
}

/// Resolve a fight between two sides of any size.
///
/// Each creature's [`Creature::team`] says which side it is on.
pub fn run_teams(
    rng: &mut Rng,
    roster: &[&Creature],
    policies: [Policy; 2],
    max_rounds: u32,
    budget: Budget,
    log: &mut Option<Vec<String>>,
) -> Outcome {
    let roster: Vec<(&Creature, Side)> = roster
        .iter()
        .map(|&c| (c, Side::from_team(c.team)))
        .collect();
    let mut fight = Fight::new(rng, &roster, policies, max_rounds, budget, log);
    fight.play(rng, 1, 0, log)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::combat::Reduction;
    use crate::rules::creature::{
        Ability, DamageKind, DamageRoll, HealRoll, Resource, SaveEffect, Strike,
    };

    fn puncher(name: &str, ac: i32, hp: i32, to_hit: i32, bonus: i32) -> Creature {
        Creature::new(name, ac, hp).with_action(Move::new(
            "Punch",
            Effect::Strikes {
                strike: Strike::new(
                    to_hit,
                    vec![DamageRoll::new(1, 6, bonus, DamageKind::Bludgeoning)],
                ),
                count: 1,
            },
        ))
    }

    fn no_log() -> Option<Vec<String>> {
        None
    }

    #[test]
    fn somebody_wins_and_the_loser_is_the_one_on_zero() {
        let a = puncher("a", 10, 30, 10, 3);
        let b = puncher("b", 10, 30, 10, 3);
        let mut rng = Rng::new(5);
        for _ in 0..500 {
            let mut log = no_log();
            let o = run(&mut rng, [&a, &b], [Policy::Greedy; 2], 100, &mut log);
            let winner = o.winner.expect("this matchup always resolves");
            assert!(o.hp_left[winner.other().index()] <= 0);
            assert!(o.hp_left[winner.index()] > 0);
            assert_eq!(o.survivors[winner.index()], 1);
            assert_eq!(o.deaths[winner.other().index()], 1);
            assert!(o.rounds >= 1);
        }
    }

    /// Nobody can hurt anybody, so the cap is the only way out. A silent hang
    /// would be a far worse failure than an unresolved result.
    #[test]
    fn a_fight_that_cannot_end_reports_the_cap() {
        let mut a = puncher("a", 40, 30, 0, 0);
        a.actions[0].effect = Effect::Strikes {
            strike: Strike::new(
                -30,
                vec![DamageRoll::new(1, 4, -20, DamageKind::Bludgeoning)],
            ),
            count: 1,
        };
        let b = a.clone();
        let mut rng = Rng::new(9);
        let mut log = no_log();
        let o = run(&mut rng, [&a, &b], [Policy::Greedy; 2], 12, &mut log);
        assert_eq!(o.winner, None);
        assert_eq!(o.rounds, 12);
        assert_eq!(o.damage_dealt, [0, 0]);
    }

    /// A shared pool, not a per-move budget: spending it on one move has to take
    /// it away from the other.
    #[test]
    fn moves_drawing_on_one_pool_compete_for_it() {
        let cost = Cost {
            resource: 0,
            amount: 1,
        };
        let mut hero = puncher("hero", 10, 100, 10, 0);
        hero.resources.push(Resource {
            name: "focus".into(),
            max: 2,
        });
        hero.bonus_actions.push(
            Move::new(
                "Flurry",
                Effect::Strikes {
                    strike: Strike::new(10, vec![DamageRoll::new(0, 6, 100, DamageKind::Force)]),
                    count: 1,
                },
            )
            .with_cost(cost),
        );
        let dummy = Creature::new("dummy", 10, 100_000);

        let mut rng = Rng::new(2);
        let mut log = no_log();
        let o = run(&mut rng, [&hero, &dummy], [Policy::Greedy; 2], 6, &mut log);
        let from_flurry = 200;
        assert!(
            o.damage_dealt[0] >= from_flurry && o.damage_dealt[0] < from_flurry + 100,
            "expected exactly two Flurries, got {} damage",
            o.damage_dealt[0]
        );
    }

    /// Stunning Strike, and the three things that have to happen in order: the
    /// save is forced, Legendary Resistance eats the first failures, and a
    /// stunned creature loses both its turn and its legendary actions.
    #[test]
    fn a_stun_takes_the_turn_and_legendary_resistance_delays_it() {
        let cost = Cost {
            resource: 0,
            amount: 1,
        };
        let mut monk = puncher("monk", 20, 200, 20, 0);
        monk.initiative = 100;
        monk.resources.push(Resource {
            name: "focus".into(),
            max: 20,
        });
        monk.actions[0].riders.push(Rider::SaveOrCondition {
            ability: Ability::Con,
            dc: 99, // never saved, so the only defence is Legendary Resistance
            condition: Condition::Stunned,
            duration: Duration::ApplierTurn,
            cost: Some(cost),
            once_per_turn: true,
        });

        let mut dragon = puncher("dragon", 10, 200, 20, 0);
        dragon.initiative = -100;
        dragon.legendary_uses = 2;
        dragon.legendary.push(Move::new(
            "Pounce",
            Effect::Strikes {
                strike: Strike::new(20, vec![DamageRoll::new(0, 6, 5, DamageKind::Slashing)]),
                count: 1,
            },
        ));

        let with_resistance = {
            let mut d = dragon.clone();
            d.riders.push(Rider::AlwaysSucceed { uses: 3 });
            d
        };

        let tally = |monster: &Creature, policy| {
            let mut rng = Rng::new(4);
            let (mut lost, mut fights) = (0u32, 0u32);
            for _ in 0..200 {
                let mut log = no_log();
                let o = run(
                    &mut rng,
                    [&monk, monster],
                    [Policy::Greedy, policy],
                    6,
                    &mut log,
                );
                lost += o.turns_lost[1];
                fights += 1;
            }
            f64::from(lost) / f64::from(fights)
        };

        let bare = tally(&dragon, Policy::Greedy);
        let resistant = tally(&with_resistance, Policy::Greedy);
        let forgetful = tally(&with_resistance, Policy::Thrifty);

        assert!(
            bare > 3.0,
            "an unsaveable stun should cost most turns: {bare}"
        );
        assert!(
            resistant < bare,
            "Legendary Resistance has to buy turns back: {resistant} vs {bare}"
        );
        assert!(
            (forgetful - bare).abs() < 1e-9,
            "a policy that never spends resistance should fare like having none: {forgetful} vs {bare}"
        );
    }

    /// Paralyzed adds one thing Stunned does not: a hit against it is an
    /// automatic critical. The paralyzer lands the condition with its action,
    /// then a bonus action against the same, now-paralyzed target should read
    /// as a crit even though the attack roll itself never approached one.
    #[test]
    fn paralyzed_turns_a_landed_bonus_action_hit_into_a_crit() {
        let mut paralyzer = puncher("paralyzer", 30, 200, 30, 0);
        paralyzer.initiative = 100;
        paralyzer.actions[0].riders.push(Rider::SaveOrCondition {
            ability: Ability::Con,
            dc: 99, // never saved, so the first hit always paralyzes
            condition: Condition::Paralyzed,
            duration: Duration::ApplierTurn,
            cost: None,
            once_per_turn: false,
        });
        // A second, separate strike so its own `force_crit` check runs after
        // the action above has already applied the condition.
        paralyzer.bonus_actions.push(Move::new(
            "Follow-up",
            Effect::Strikes {
                strike: Strike::new(30, vec![DamageRoll::new(1, 4, 0, DamageKind::Bludgeoning)]),
                count: 1,
            },
        ));

        let mut victim = puncher("victim", 1, 200, -100, 0);
        victim.initiative = -100;

        let mut rng = Rng::new(21);
        let mut log = Some(Vec::new());
        run(
            &mut rng,
            [&paralyzer, &victim],
            [Policy::Greedy; 2],
            1,
            &mut log,
        );
        let narration = log.unwrap().join("\n");
        assert!(
            narration.contains("crit"),
            "a hit against a paralyzed target must be an automatic crit:\n{narration}"
        );
    }

    /// `Duration::SaveEndTurn` does not expire on a fixed timer at all: it
    /// repeats its save at the end of the victim's own turn and can clear the
    /// condition the very turn it landed. A save that easy should cost at most
    /// the one turn it interrupted; a save that is unbeatable should behave
    /// exactly like a condition with no expiry.
    #[test]
    fn a_repeatable_save_can_end_a_condition_the_turn_it_lands() {
        let cost = Cost {
            resource: 0,
            amount: 1,
        };
        let paralyzer = |ability: Ability, dc: i32| {
            let mut c = puncher("paralyzer", 20, 200, 20, 0);
            c.initiative = 100;
            // One shot only, so the condition is never re-applied - the test
            // is about how long a single application lasts, not how often it
            // lands.
            c.actions[0].uses = Uses::Limited(1);
            c.resources.push(Resource {
                name: "focus".into(),
                max: 1,
            });
            c.actions[0].riders.push(Rider::SaveOrCondition {
                ability: Ability::Con,
                dc: 99, // the one hit always lands the condition
                condition: Condition::Paralyzed,
                duration: Duration::SaveEndTurn { ability, dc },
                cost: Some(cost),
                once_per_turn: true,
            });
            c
        };
        let mut victim = puncher("victim", 10, 200, 0, 0);
        victim.initiative = -100;

        let tally = |monster: &Creature| {
            let mut rng = Rng::new(13);
            let (mut lost, mut fights) = (0u32, 0u32);
            for _ in 0..300 {
                let mut log = no_log();
                let o = run(
                    &mut rng,
                    [monster, &victim],
                    [Policy::Greedy; 2],
                    6,
                    &mut log,
                );
                lost += o.turns_lost[1];
                fights += 1;
            }
            f64::from(lost) / f64::from(fights)
        };

        // Auto-fails Paralyzed's own Str/Dex saves, so Dex can never clear it.
        let unbeatable = tally(&paralyzer(Ability::Dex, 99));
        // Wisdom is untouched by that auto-fail, and DC 1 is a near-certainty.
        let easy = tally(&paralyzer(Ability::Wis, 1));

        assert!(
            unbeatable > 3.0,
            "a save Paralyzed auto-fails should behave like a condition with no expiry: {unbeatable}"
        );
        assert!(
            easy < 1.5,
            "a near-certain save should clear before it costs a second turn: {easy}"
        );
    }

    /// Dodge and a reaction that cuts damage both have to actually reduce what
    /// lands, and the stance has to expire on its own.
    #[test]
    fn dodging_and_a_damage_reducing_reaction_both_bite() {
        let attacker = {
            let mut c = puncher("attacker", 10, 10_000, 5, 0);
            c.actions[0].effect = Effect::Strikes {
                strike: Strike::new(5, vec![DamageRoll::new(2, 6, 4, DamageKind::Slashing)]),
                count: 3,
            };
            c.initiative = -100;
            c
        };
        let plain = {
            let mut c = Creature::new("plain", 15, 10_000);
            c.initiative = 100;
            c
        };
        let dodger = {
            let mut c = plain.clone();
            c.bonus_actions.push(Move::new(
                "Dodge",
                Effect::Stance {
                    condition: Condition::Dodging,
                },
            ));
            c
        };
        let deflector = {
            let mut c = plain.clone();
            c.riders.push(Rider::ReduceDamage {
                kinds: vec![DamageKind::Slashing],
                roll: DamageRoll::new(1, 10, 7, DamageKind::Slashing),
                per_round: 1,
            });
            c
        };

        let taken = |defender: &Creature| {
            let mut rng = Rng::new(8);
            let mut total = 0i64;
            for _ in 0..400 {
                let mut log = no_log();
                let o = run(
                    &mut rng,
                    [defender, &attacker],
                    [Policy::Greedy; 2],
                    5,
                    &mut log,
                );
                total += o.damage_dealt[1];
            }
            total
        };

        let base = taken(&plain);
        assert!(taken(&dodger) < base, "dodging must reduce incoming damage");
        assert!(
            taken(&deflector) < base,
            "a damage-reducing reaction must reduce incoming damage"
        );
        // One reaction a round, so it cannot blunt all three attacks.
        assert!(taken(&deflector) > base / 2);
    }

    /// [`Rider::ReactionOnTargeted`] is the mirror of [`Rider::ReduceDamage`]:
    /// it acts before the hit is even decided, converting what would have
    /// been a hit into a miss instead of shaving damage off one that already
    /// landed.
    #[test]
    fn a_reactive_ac_boost_converts_a_would_be_hit_into_a_miss() {
        let attacker = {
            let mut c = puncher("attacker", 10, 10_000, 5, 0);
            c.actions[0].effect = Effect::Strikes {
                strike: Strike::new(5, vec![DamageRoll::new(2, 6, 4, DamageKind::Slashing)]),
                count: 3,
            };
            c.initiative = -100;
            c
        };
        let plain = {
            let mut c = Creature::new("plain", 15, 10_000);
            c.initiative = 100;
            c
        };
        let shielded = {
            let mut c = plain.clone();
            c.riders.push(Rider::ReactionOnTargeted {
                trigger: AttackTrigger::AnyAttack,
                // Large enough that, whenever the reaction fires, it always
                // succeeds in turning the hit into a miss.
                ac_bonus: 100,
                per_round: 1,
            });
            c
        };

        let taken = |defender: &Creature| {
            let mut rng = Rng::new(8);
            let mut total = 0i64;
            for _ in 0..400 {
                let mut log = no_log();
                let o = run(
                    &mut rng,
                    [defender, &attacker],
                    [Policy::Greedy; 2],
                    5,
                    &mut log,
                );
                total += o.damage_dealt[1];
            }
            total
        };

        let base = taken(&plain);
        let with_reaction = taken(&shielded);
        assert!(
            with_reaction < base,
            "a reactive AC boost must reduce incoming damage: {with_reaction} vs {base}"
        );
        // One reaction a round protects at most one of the three attacks each
        // round, so it cannot blunt them all.
        assert!(with_reaction > base / 4);
    }

    /// The per-round budget in isolation: available at the start, gone the
    /// instant it is spent, and back only once the creature's turn refreshes
    /// it - the same lifecycle [`Rider::ReduceDamage`] already has.
    #[test]
    fn a_reaction_on_targeted_is_available_once_then_spent_for_the_round() {
        let mut c = Creature::new("defender", 15, 20);
        c.riders.push(Rider::ReactionOnTargeted {
            trigger: AttackTrigger::AnyAttack,
            ac_bonus: 5,
            per_round: 1,
        });
        let mut f = Fighter::new(&c, Side::A, Policy::Greedy, 0);

        let reaction = ac_boost_reaction(&f, AttackTrigger::AnyAttack);
        assert_eq!(
            reaction,
            Some((0, 5)),
            "the reaction should be available before anything spends it"
        );

        // Spend it exactly the way the strike-resolution loop does.
        let (i, _) = reaction.unwrap();
        f.rider_uses[i] -= 1;
        assert_eq!(
            ac_boost_reaction(&f, AttackTrigger::AnyAttack),
            None,
            "spent this round, it must not be offered again"
        );

        // Refreshing at the start of a turn is the only thing that brings a
        // reaction back.
        let mut rng = Rng::new(1);
        refresh(&mut f, &mut rng);
        assert_eq!(
            ac_boost_reaction(&f, AttackTrigger::AnyAttack),
            Some((0, 5)),
            "a new turn should refresh the reaction"
        );
    }

    /// A multiattack throws several attack rolls in one turn, but a reaction
    /// is still only spendable once: at most one of them should ever be the
    /// one it was spent on.
    #[test]
    fn the_reaction_only_converts_one_attack_per_round_even_in_a_multiattack() {
        let attacker = {
            // AC 1 and a +30 to hit: every roll but a natural 1 is an
            // ordinary hit against the base AC, so the reaction has every
            // chance to fire on each of the three swings if it could.
            let mut c = puncher("attacker", 10, 10_000, 30, 0);
            c.actions[0].effect = Effect::Strikes {
                strike: Strike::new(30, vec![DamageRoll::new(1, 4, 0, DamageKind::Bludgeoning)]),
                count: 3,
            };
            c.initiative = -100;
            c
        };
        let mut defender = Creature::new("defender", 1, 10_000);
        defender.initiative = 100;
        defender.riders.push(Rider::ReactionOnTargeted {
            trigger: AttackTrigger::AnyAttack,
            ac_bonus: 100,
            per_round: 1,
        });

        let mut rng = Rng::new(3);
        let mut saw_a_boosted_miss = false;
        for _ in 0..200 {
            let mut log = Some(Vec::new());
            run(
                &mut rng,
                [&defender, &attacker],
                [Policy::Greedy; 2],
                1,
                &mut log,
            );
            let narration = log.unwrap().join("\n");
            let boosted = narration.matches("AC boosted").count();
            assert!(
                boosted <= 1,
                "one reaction a round should never convert more than one attack:\n{narration}"
            );
            saw_a_boosted_miss |= boosted == 1;
        }
        assert!(
            saw_a_boosted_miss,
            "an available reaction against a would-be hit should have fired at least once in 200 rounds"
        );
    }

    #[test]
    fn legendary_actions_land_between_turns() {
        let mut monster = puncher("monster", 10, 200, 20, 0);
        monster.legendary_uses = 3;
        monster.legendary.push(Move::new(
            "Pounce",
            Effect::Strikes {
                strike: Strike::new(20, vec![DamageRoll::new(0, 6, 5, DamageKind::Slashing)]),
                count: 1,
            },
        ));
        monster.initiative = -100;
        let mut hero = puncher("hero", 10, 200, 20, 0);
        hero.initiative = 100;

        let mut rng = Rng::new(3);
        let mut log = Some(Vec::new());
        let o = run(
            &mut rng,
            [&hero, &monster],
            [Policy::Greedy; 2],
            4,
            &mut log,
        );
        let narration = log.unwrap().join("\n");
        assert!(
            narration.contains("legendary"),
            "no legendary actions in:\n{narration}"
        );
        assert!(o.damage_dealt[1] > o.damage_dealt[0]);
    }

    /// A policy has to score its moves against the creature it is hitting, not
    /// against itself. A fire-immune dragon rating its own breath weapon sees a
    /// zero and never breathes - which looks entirely plausible in the output,
    /// and is wrong.
    #[test]
    fn a_policy_scores_its_moves_against_the_target() {
        let breath = Move::new(
            "Fire Breath",
            Effect::Save(SaveEffect {
                ability: Ability::Dex,
                dc: 30,
                damage: vec![DamageRoll::new(0, 6, 60, DamageKind::Fire)],
                half_on_success: false,
                on_failure: None,
                max_targets: None,
                requires_type: None,
            }),
        );
        let claw = Move::new(
            "Claw",
            Effect::Strikes {
                strike: Strike::new(20, vec![DamageRoll::new(0, 6, 5, DamageKind::Slashing)]),
                count: 1,
            },
        );

        let mut dragon = Creature::new("dragon", 19, 500);
        dragon
            .reductions
            .push((DamageKind::Fire, Reduction::Immune));
        dragon.actions.push(claw);
        dragon.actions.push(breath);
        dragon.initiative = 100;

        let victim = Creature::new("victim", 10, 10_000);
        let mut rng = Rng::new(6);
        let mut log = Some(Vec::new());
        run(
            &mut rng,
            [&dragon, &victim],
            [Policy::Greedy; 2],
            3,
            &mut log,
        );
        let narration = log.unwrap().join("\n");
        assert!(
            narration.contains("Fire Breath"),
            "60 fire should beat a 5-damage claw:\n{narration}"
        );
    }

    /// A creature with a pool and something worth spending it on, against a
    /// target that cannot hurt back.
    fn spender() -> Creature {
        let mut c = puncher("spender", 10, 1_000, 20, 0);
        c.resources.push(Resource {
            name: "focus".into(),
            max: 6,
        });
        c.bonus_actions.push(
            Move::new(
                "Haymaker",
                Effect::Strikes {
                    strike: Strike::new(20, vec![DamageRoll::new(0, 6, 20, DamageKind::Force)]),
                    count: 1,
                },
            )
            .with_cost(Cost {
                resource: 0,
                amount: 1,
            }),
        );
        c
    }

    /// The hoarding policies differ in *when* they relent, and a creature that
    /// never drops below half never does.
    #[test]
    fn attrition_holds_its_pool_until_it_is_hurt() {
        let hero = spender();
        let harmless = Creature::new("harmless", 10, 100_000);

        let spent = |policy| {
            let mut rng = Rng::new(21);
            let mut total = 0u32;
            for _ in 0..50 {
                let mut log = no_log();
                let o = run(
                    &mut rng,
                    [&hero, &harmless],
                    [policy, Policy::Greedy],
                    5,
                    &mut log,
                );
                total += o.resources_spent[0];
            }
            total
        };

        assert_eq!(
            spent(Policy::Attrition),
            0,
            "unhurt, attrition should not have touched the pool"
        );
        assert_eq!(spent(Policy::Thrifty), 0);
        assert!(spent(Policy::Greedy) > 0);
        assert!(
            spent(Policy::Nova) >= spent(Policy::Greedy),
            "front-loading cannot spend less than greedy"
        );
    }

    /// Nova ranks on cost before damage, so it reaches for the expensive move
    /// even when a free one hits harder.
    #[test]
    fn nova_prefers_the_expensive_move_over_the_stronger_one() {
        let mut hero = puncher("hero", 10, 1_000, 20, 0);
        hero.resources.push(Resource {
            name: "focus".into(),
            max: 1,
        });
        hero.bonus_actions.push(Move::new(
            "Big free swing",
            Effect::Strikes {
                strike: Strike::new(20, vec![DamageRoll::new(0, 6, 50, DamageKind::Force)]),
                count: 1,
            },
        ));
        hero.bonus_actions.push(
            Move::new(
                "Small costly swing",
                Effect::Strikes {
                    strike: Strike::new(20, vec![DamageRoll::new(0, 6, 5, DamageKind::Force)]),
                    count: 1,
                },
            )
            .with_cost(Cost {
                resource: 0,
                amount: 1,
            }),
        );
        let dummy = Creature::new("dummy", 10, 100_000);

        let first_move = |policy| {
            let mut rng = Rng::new(3);
            let mut log = Some(Vec::new());
            run(
                &mut rng,
                [&hero, &dummy],
                [policy, Policy::Greedy],
                1,
                &mut log,
            );
            log.unwrap().join("\n")
        };
        assert!(first_move(Policy::Nova).contains("Small costly swing"));
        assert!(first_move(Policy::Greedy).contains("Big free swing"));
    }

    /// Against a single enemy the targeting policies have nothing to choose, so
    /// their rows must be *identical* to greedy rather than approximately equal.
    #[test]
    fn the_target_selection_policies_are_greedy_against_one_enemy() {
        let a = puncher("a", 14, 60, 6, 3);
        let b = puncher("b", 13, 55, 5, 4);

        let play = |policy| {
            let mut rng = Rng::new(17);
            let mut results = Vec::new();
            for _ in 0..100 {
                let mut log = no_log();
                results.push(run(
                    &mut rng,
                    [&a, &b],
                    [policy, Policy::Greedy],
                    40,
                    &mut log,
                ));
            }
            results
        };
        let greedy = play(Policy::Greedy);
        assert_eq!(play(Policy::FocusFire), greedy);
        assert_eq!(play(Policy::Scattered), greedy);
    }

    /// And against several they must not be. Four attackers against three
    /// defenders: spreading damage keeps every defender alive and swinging, which
    /// is the worst thing a party can do and one of the most common.
    #[test]
    fn spreading_damage_is_worse_than_concentrating_it() {
        let mut attacker = puncher("hitter", 12, 30, 8, 4);
        attacker.team = 0;
        let mut defender = puncher("target", 12, 30, 8, 4);
        defender.team = 1;

        // Evenly matched, so the only thing separating the two runs is how the
        // first side chooses targets.
        let mut roster: Vec<&Creature> = Vec::new();
        for _ in 0..3 {
            roster.push(&attacker);
        }
        for _ in 0..3 {
            roster.push(&defender);
        }

        let wins = |policy| {
            let mut rng = Rng::new(31);
            let mut won = 0;
            for _ in 0..400 {
                let mut log = no_log();
                let o = run_teams(
                    &mut rng,
                    &roster,
                    [policy, Policy::FocusFire],
                    30,
                    Budget::default(),
                    &mut log,
                );
                if o.winner == Some(Side::A) {
                    won += 1;
                }
            }
            f64::from(won) / 400.0
        };

        let focused = wins(Policy::FocusFire);
        let spread = wins(Policy::Scattered);
        assert!(
            focused > spread + 0.05,
            "concentrating fire should beat spreading it: {focused:.3} vs {spread:.3}"
        );
    }

    /// An area effect catches every enemy, which is the whole reason a party
    /// cannot simply out-number a dragon.
    #[test]
    fn an_area_effect_hits_the_whole_other_side() {
        let mut breather = Creature::new("breather", 20, 500);
        breather.team = 0;
        breather.initiative = 100;
        breather.actions.push(Move::new(
            "Breath",
            Effect::Save(SaveEffect {
                ability: Ability::Dex,
                dc: 99,
                damage: vec![DamageRoll::new(0, 6, 10, DamageKind::Fire)],
                half_on_success: true,
                on_failure: None,
                max_targets: None,
                requires_type: None,
            }),
        ));
        let mut victim = Creature::new("victim", 10, 1_000);
        victim.team = 1;

        let roster: Vec<&Creature> = vec![&breather, &victim, &victim, &victim];
        let mut rng = Rng::new(12);
        let mut log = no_log();
        let o = run_teams(
            &mut rng,
            &roster,
            [Policy::Greedy; 2],
            1,
            Budget::default(),
            &mut log,
        );
        // Three victims, 10 damage each, in one round.
        assert_eq!(o.damage_dealt[0], 30);

        // With a cap, only that many are caught.
        let mut capped = breather.clone();
        if let Effect::Save(save) = &mut capped.actions[0].effect {
            save.max_targets = Some(2);
        }
        let roster: Vec<&Creature> = vec![&capped, &victim, &victim, &victim];
        let mut rng = Rng::new(12);
        let mut log = no_log();
        let o = run_teams(
            &mut rng,
            &roster,
            [Policy::Greedy; 2],
            1,
            Budget::default(),
            &mut log,
        );
        assert_eq!(o.damage_dealt[0], 20);
    }

    /// A legendary monster gets a window after every enemy turn, so a bigger
    /// party hands it more of them - up to its pool.
    #[test]
    fn more_enemies_means_more_legendary_windows() {
        let mut monster = Creature::new("monster", 10, 10_000);
        monster.team = 0;
        monster.initiative = -100; // acts last, so every window opens first
        monster.legendary_uses = 3;
        monster.legendary.push(Move::new(
            "Pounce",
            Effect::Strikes {
                strike: Strike::new(40, vec![DamageRoll::new(0, 6, 3, DamageKind::Slashing)]),
                count: 1,
            },
        ));
        let mut hero = Creature::new("hero", 10, 10_000);
        hero.team = 1;
        hero.initiative = 100;

        let pounces = |party: usize| {
            let mut roster: Vec<&Creature> = vec![&monster];
            for _ in 0..party {
                roster.push(&hero);
            }
            let mut rng = Rng::new(44);
            let mut log = no_log();
            run_teams(
                &mut rng,
                &roster,
                [Policy::Greedy; 2],
                1,
                Budget::default(),
                &mut log,
            )
            .damage_dealt[0]
                / 3
        };

        // One use per window, and a window after each enemy turn: one enemy
        // buys the dragon one legendary action a round, three buy it three.
        assert_eq!(pounces(1), 1);
        assert_eq!(pounces(2), 2);
        assert_eq!(pounces(3), 3);
        // And the pool still caps it.
        assert_eq!(pounces(5), 3);
    }

    /// The case that justifies having a search at all.
    ///
    /// Two attacks with identical mean damage, one of which also stuns. No
    /// damage-ranking policy can tell them apart, so greedy takes the one listed
    /// first and loses; the search plays each out and finds that denying the
    /// opponent its turn is worth everything.
    #[test]
    fn the_solver_finds_a_line_no_damage_ranking_can_see() {
        let hit = || Strike::new(40, vec![DamageRoll::new(0, 6, 10, DamageKind::Force)]);
        let mut hero = Creature::new("hero", 25, 60);
        hero.initiative = 100;
        hero.actions.push(Move::new(
            "Plain swing",
            Effect::Strikes {
                strike: hit(),
                count: 1,
            },
        ));
        hero.actions.push(
            Move::new(
                "Stunning swing",
                Effect::Strikes {
                    strike: hit(),
                    count: 1,
                },
            )
            .with_rider(Rider::SaveOrCondition {
                ability: Ability::Con,
                dc: 99,
                condition: Condition::Stunned,
                duration: Duration::ApplierTurn,
                cost: None,
                once_per_turn: true,
            }),
        );

        let mut monster = Creature::new("monster", 1, 100);
        monster.initiative = -100;
        monster.actions.push(Move::new(
            "Smash",
            Effect::Strikes {
                strike: Strike::new(40, vec![DamageRoll::new(0, 6, 20, DamageKind::Slashing)]),
                count: 1,
            },
        ));

        let sides = [&hero, &monster];
        let wins = |policy| {
            crate::sim::analysis::evaluate(5, sides, [policy, Policy::Greedy], 60, 30).wins[0]
        };

        let greedy = wins(Policy::Greedy);
        let solver = wins(Policy::Solver);
        assert!(
            greedy < 0.1,
            "greedy should take the first of two equal-damage swings and lose: {greedy}"
        );
        assert!(
            solver > 0.9,
            "the search should find the stun lock: {solver}"
        );
    }

    /// The whole point of having more than one policy: the same two creatures
    /// produce different fights.
    #[test]
    fn thrifty_leaves_its_limited_moves_alone() {
        let spender = puncher("spender", 10, 40, 10, 0).with_bonus_action(
            Move::new(
                "Haymaker",
                Effect::Strikes {
                    strike: Strike::new(
                        10,
                        vec![DamageRoll::new(4, 6, 10, DamageKind::Bludgeoning)],
                    ),
                    count: 1,
                },
            )
            .with_uses(Uses::Limited(3)),
        );
        let dummy = Creature::new("dummy", 10, 400);

        let total = |policy| {
            let mut rng = Rng::new(11);
            let mut sum = 0i64;
            for _ in 0..200 {
                let mut log = no_log();
                let o = run(
                    &mut rng,
                    [&spender, &dummy],
                    [policy, Policy::Greedy],
                    6,
                    &mut log,
                );
                sum += o.damage_dealt[0];
            }
            sum
        };
        assert!(
            total(Policy::Thrifty) < total(Policy::Greedy),
            "hoarding the limited move has to cost damage"
        );
    }

    /// When a multiattack drops an enemy with strikes left, the remaining
    /// strikes should retarget rather than being discarded.
    #[test]
    fn strikes_retarget_when_a_target_drops_mid_turn() {
        let mut attacker = Creature::new("attacker", 10, 100);
        attacker.team = 0;
        attacker.initiative = 100;
        attacker.actions.push(Move::new(
            "Double Strike",
            Effect::Strikes {
                strike: Strike::new(30, vec![DamageRoll::new(0, 6, 10, DamageKind::Bludgeoning)]),
                count: 2,
            },
        ));

        let mut target1 = Creature::new("target1", 10, 5);
        target1.team = 1;
        let mut target2 = Creature::new("target2", 10, 50);
        target2.team = 1;

        let roster = [&attacker, &target1, &target2];
        let mut rng = Rng::new(42);
        let mut log = no_log();
        let o = run_teams(
            &mut rng,
            &roster,
            [Policy::Greedy; 2],
            1,
            Budget::default(),
            &mut log,
        );

        // 10 damage to target1 (dropping it from 5 to 0) + 10 damage to target2
        assert_eq!(o.damage_dealt[0], 20);
        assert_eq!(o.deaths[1], 1);
        assert_eq!(o.survivors[1], 1);
    }

    /// Multiple searching creatures on a team must not trigger exponential recursive searches.
    #[test]
    fn multiple_searchers_on_a_team_do_not_recursively_explode() {
        let mut hero1 = puncher("hero1", 10, 30, 8, 4);
        hero1.team = 0;
        let mut hero2 = puncher("hero2", 10, 30, 8, 4);
        hero2.team = 0;
        let mut monster = puncher("monster", 10, 60, 8, 4);
        monster.team = 1;

        let roster = [&hero1, &hero2, &monster];
        let mut rng = Rng::new(17);
        let mut log = no_log();
        let o = run_teams(
            &mut rng,
            &roster,
            [Policy::Solver, Policy::Greedy],
            5,
            Budget {
                rollouts: 4,
                depth: 2,
            },
            &mut log,
        );
        assert!(o.rounds >= 1);
    }

    /// DC 10, or half the damage taken, whichever is higher - and 5e's "half,
    /// rounded down" so the boundary sits exactly at 20/21 damage rather than
    /// drifting up at odd numbers.
    #[test]
    fn concentration_dc_is_ten_or_half_the_damage_whichever_is_higher() {
        assert_eq!(concentration_dc(0), 10);
        assert_eq!(concentration_dc(10), 10);
        assert_eq!(concentration_dc(19), 10);
        assert_eq!(concentration_dc(20), 10);
        assert_eq!(concentration_dc(21), 10, "half of 21 rounds down to 10");
        assert_eq!(concentration_dc(22), 11);
        assert_eq!(concentration_dc(41), 20, "half of 41 rounds down to 20");
        assert_eq!(concentration_dc(42), 21);
    }

    /// Casting a second concentration spell ends the first immediately, even
    /// though nothing here ever deals damage - a check that only fires on
    /// "took damage and failed the save" would miss the rule that *starting*
    /// a new one is what breaks the old one.
    #[test]
    fn a_second_concentration_spell_ends_the_first() {
        let cast = |condition: Condition| {
            Move::new(
                "Spell",
                Effect::Save(SaveEffect {
                    ability: Ability::Wis,
                    dc: 99, // never saved
                    damage: vec![],
                    half_on_success: false,
                    on_failure: Some((condition, Duration::ApplierTurn)),
                    max_targets: None,
                    requires_type: None,
                }),
            )
            .with_concentration()
        };

        let mut caster = Creature::new("caster", 10, 100);
        caster.actions.push(cast(Condition::Prone));
        caster.bonus_actions.push(cast(Condition::Blinded));
        let victim = Creature::new("victim", 10, 100);

        let roster = [(&caster, Side::A), (&victim, Side::B)];
        let mut rng = Rng::new(1);
        let mut log = no_log();
        let mut fight = Fight::new(
            &mut rng,
            &roster,
            [Policy::InOrder; 2],
            5,
            Budget::default(),
            &mut log,
        );
        fight.take_turn(1, 0, &mut rng, &mut log, None);

        assert!(
            !fight.fighters[1].has(|c| c == Condition::Prone),
            "the action's condition must be cleared once the bonus action starts concentrating on something else"
        );
        assert!(
            fight.fighters[1].has(|c| c == Condition::Blinded),
            "the second spell's condition must still land"
        );
        let active = fight.fighters[0]
            .concentration
            .as_ref()
            .expect("still concentrating on the second spell");
        match &active.effect {
            ConcentrationEffect::Condition { targets, condition } => {
                assert_eq!(*condition, Condition::Blinded);
                assert_eq!(*targets, vec![1]);
            }
        }
    }

    /// A failed concentration save clears the condition from every combatant
    /// it was maintained on, not just one - the shape an area concentration
    /// spell (Hypnotic Pattern) needs.
    #[test]
    fn a_failed_concentration_save_clears_the_effect_from_every_target() {
        let mut caster = Creature::new("caster", 10, 100);
        caster.saves[Ability::Con.index()] = -100; // never saves
        let a = Creature::new("a", 10, 100);
        let b = Creature::new("b", 10, 100);

        let roster = [(&caster, Side::A), (&a, Side::B), (&b, Side::B)];
        let mut rng = Rng::new(2);
        let mut log = no_log();
        let mut fight = Fight::new(
            &mut rng,
            &roster,
            [Policy::InOrder; 2],
            5,
            Budget::default(),
            &mut log,
        );

        fight.fighters[1]
            .conditions
            .push((Condition::Poisoned, Expiry::TurnStart(1)));
        fight.fighters[2]
            .conditions
            .push((Condition::Poisoned, Expiry::TurnStart(2)));
        fight.fighters[0].concentration = Some(ActiveConcentration {
            effect: ConcentrationEffect::Condition {
                targets: vec![1, 2],
                condition: Condition::Poisoned,
            },
        });

        fight.concentration_check(&mut rng, 0, 100); // dc 50, and the save always fails
        assert!(fight.fighters[0].concentration.is_none());
        assert!(!fight.fighters[1].has(|c| c == Condition::Poisoned));
        assert!(!fight.fighters[2].has(|c| c == Condition::Poisoned));
    }

    /// A save that is beaten leaves concentration alone - the counterpart to
    /// the failure case above, so a check that always breaks it cannot pass
    /// both.
    #[test]
    fn a_beaten_concentration_save_keeps_it_active() {
        let mut caster = Creature::new("caster", 10, 100);
        caster.saves[Ability::Con.index()] = 100; // always saves
        let victim = Creature::new("victim", 10, 100);
        let roster = [(&caster, Side::A), (&victim, Side::B)];
        let mut rng = Rng::new(3);
        let mut log = no_log();
        let mut fight = Fight::new(
            &mut rng,
            &roster,
            [Policy::InOrder; 2],
            5,
            Budget::default(),
            &mut log,
        );

        fight.fighters[1]
            .conditions
            .push((Condition::Poisoned, Expiry::TurnStart(1)));
        fight.fighters[0].concentration = Some(ActiveConcentration {
            effect: ConcentrationEffect::Condition {
                targets: vec![1],
                condition: Condition::Poisoned,
            },
        });

        fight.concentration_check(&mut rng, 0, 100);
        assert!(fight.fighters[0].concentration.is_some());
        assert!(fight.fighters[1].has(|c| c == Condition::Poisoned));
    }

    /// Dropping to 0 HP ends concentration outright - no save offered, unlike
    /// ordinary damage. A Con save bonus of +100 would beat any DC, so if the
    /// implementation quietly rolled one anyway this would still pass; the
    /// point is that it must not need to.
    #[test]
    fn dropping_to_zero_hp_ends_concentration_without_a_save() {
        let mut caster = Creature::new("caster", 10, 100);
        caster.saves[Ability::Con.index()] = 100;
        let victim = Creature::new("victim", 10, 100);
        let roster = [(&caster, Side::A), (&victim, Side::B)];
        let mut rng = Rng::new(4);
        let mut log = no_log();
        let mut fight = Fight::new(
            &mut rng,
            &roster,
            [Policy::InOrder; 2],
            5,
            Budget::default(),
            &mut log,
        );

        fight.fighters[1]
            .conditions
            .push((Condition::Poisoned, Expiry::TurnStart(1)));
        fight.fighters[0].concentration = Some(ActiveConcentration {
            effect: ConcentrationEffect::Condition {
                targets: vec![1],
                condition: Condition::Poisoned,
            },
        });

        fight.apply_damage(&mut rng, 0, fight.fighters[0].hp);
        assert!(fight.fighters[0].concentration.is_none());
        assert!(!fight.fighters[1].has(|c| c == Condition::Poisoned));
    }

    /// Becoming Incapacitated ends concentration immediately too, the same
    /// way 0 HP does - no save, because an Incapacitated creature cannot
    /// concentrate on anything at all.
    #[test]
    fn becoming_incapacitated_ends_concentration_without_a_save() {
        let mut caster = Creature::new("caster", 10, 100);
        caster.saves[Ability::Con.index()] = 100;
        let victim = Creature::new("victim", 10, 100);
        let roster = [(&caster, Side::A), (&victim, Side::B)];
        let mut rng = Rng::new(5);
        let mut log = no_log();
        let mut fight = Fight::new(
            &mut rng,
            &roster,
            [Policy::InOrder; 2],
            5,
            Budget::default(),
            &mut log,
        );

        fight.fighters[1]
            .conditions
            .push((Condition::Poisoned, Expiry::TurnStart(1)));
        fight.fighters[0].concentration = Some(ActiveConcentration {
            effect: ConcentrationEffect::Condition {
                targets: vec![1],
                condition: Condition::Poisoned,
            },
        });

        fight.apply_condition(0, Condition::Stunned, Expiry::TurnStart(0));
        assert!(fight.fighters[0].concentration.is_none());
        assert!(!fight.fighters[1].has(|c| c == Condition::Poisoned));
    }

    /// `Fight::resolve` is exercised directly rather than through a whole
    /// `run`: the duel engine has no ally targeting yet (see
    /// `dsl::plugin::spells`'s module doc), so there is no scenario today
    /// where a policy actually aims Healing Word or Cure Wounds at a downed
    /// friendly. This pins the mechanical half - the HP math and the revive -
    /// so it is right once targeting catches up.
    #[test]
    fn heal_effect_revives_a_downed_target() {
        let healer = puncher("healer", 10, 20, 5, 2);
        let downed = puncher("downed", 10, 30, 5, 2);
        let roster = [(&healer, Side::A), (&downed, Side::B)];
        let mut rng = Rng::new(1);
        let mut fight = Fight::new(
            &mut rng,
            &roster,
            [Policy::InOrder; 2],
            1,
            Budget::default(),
            &mut None,
        );
        fight.fighters[1].hp = 0;

        // 1d4+3 is 4..=7: always enough to clear zero against a 30 hp max, so
        // the revive is deterministic without pinning the roll.
        let heal = Effect::Heal(HealRoll::new(1, 4, 3));
        let mut notes = Vec::new();
        fight.resolve(
            &heal,
            &mut rng,
            0,
            1,
            &[],
            true,
            &mut notes,
            &mut Vec::new(),
        );

        assert!((4..=7).contains(&fight.fighters[1].hp));
        assert!(
            notes.iter().any(|n| n.contains("revives")),
            "regaining hp from 0 should revive: {notes:?}"
        );
    }

    #[test]
    fn heal_effect_clamps_at_max_hp_and_does_not_revive_the_merely_wounded() {
        let healer = puncher("healer", 10, 20, 5, 2);
        let wounded = puncher("wounded", 10, 10, 5, 2);
        let roster = [(&healer, Side::A), (&wounded, Side::B)];
        let mut rng = Rng::new(1);
        let mut fight = Fight::new(
            &mut rng,
            &roster,
            [Policy::InOrder; 2],
            1,
            Budget::default(),
            &mut None,
        );
        // Already above zero, and close enough to its own 10 hp max that
        // even the smallest roll (4) would overshoot it.
        fight.fighters[1].hp = 8;

        let heal = Effect::Heal(HealRoll::new(1, 4, 3));
        let mut notes = Vec::new();
        fight.resolve(
            &heal,
            &mut rng,
            0,
            1,
            &[],
            true,
            &mut notes,
            &mut Vec::new(),
        );

        assert_eq!(
            fight.fighters[1].hp, 10,
            "healing cannot push a creature past its own max hp"
        );
        assert!(
            notes.iter().all(|n| !n.contains("revives")),
            "was never down, so nothing to revive: {notes:?}"
        );
    }
}
