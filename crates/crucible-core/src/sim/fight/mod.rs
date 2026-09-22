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
//! Riders fire at fixed points, which is where the event pipeline `DESIGN.md`
//! describes will eventually go:
//!
//! - **at the start of a turn**, every aura on the other side resolves
//!   against the creature whose turn it is, and a swallower digests whatever
//!   it holds ([`Rider::Digestion`]);
//! - **as an attack roll is made**, the attacker's damage riders are gathered:
//!   Sneak Attack if the roll qualifies (see [`Fight::extra_damage_plan`]),
//!   bonus dice against the target's creature type, a weapon buff armed by an
//!   earlier condition - and Cunning Strike decides what to spend dice on;
//! - **on being targeted, before an attack's hit or miss is finalized**,
//!   [`Rider::ReactionOnTargeted`] spends a reaction to add an AC bonus,
//!   capable of turning that one attack's hit into a miss;
//! - **on an incoming attack's damage**, [`Rider::ReduceDamage`] or
//!   [`Rider::HalveAttackDamage`] spends a reaction to cut it - one reaction
//!   a round between all of them;
//! - **as damage lands**, a [`Rider::DamageThreshold`] absorbs it or is
//!   breached - unless it came through a weak spot - and a breach sets off a
//!   reaction waiting for one ([`ReactionTrigger::Breached`]);
//! - **on a hit**, [`Rider::SaveOrCondition`] forces a save and may apply a
//!   condition, [`Rider::ConditionOnHit`] applies one outright, a Cunning
//!   Strike effect resolves, an [`Rider::InjuryPoison`] dose is used up, and
//!   a [`Rider::Swallow`] takes the target in;
//! - **as a condition lands on an enemy**, a reaction waiting for it answers
//!   ([`ReactionTrigger::EnemyGains`]);
//! - **on a failed save**, [`Rider::AlwaysSucceed`] may buy it back;
//! - **on a save for half**, [`Rider::NothingOnSuccess`] reshapes the outcome;
//! - **at the end of a turn**, a swallower that took enough damage from inside
//!   saves or regurgitates ([`Rider::Regurgitate`]).
//!
//! Those points are chosen here rather than subscribed to, and a rider cannot
//! modify another rider. Getting from this to the real pipeline is a refactor of
//! this file, not of the data.
//!
//! **Positioning is mostly absent**, and with more than one creature a side that
//! costs more than it used to. There is no map, so an area effect is assumed to
//! catch every enemy - the pessimistic reading, since a party that spread out
//! would not all be in one cone. `max_targets` on a saving throw is the only
//! control over that. "An ally within 5 feet of the target", which Sneak Attack
//! asks about, is read off who is fighting what instead: see
//! [`Fight::ally_adjacent`].
//!
//! Two kinds of reach do exist. A swallowed creature is cut off from everything
//! but its swallower's insides - see [`Fight::reaches`]. And around a creature
//! with a mouth, every enemy stands in one of four zones, which decide what
//! reaches what, move with pulls, pushes and each side's movement, and are
//! chosen like any other part of a turn - see `zones` and
//! [`crate::creature::Zone`].
//!
//! [`Rider::ReactionOnTargeted`]: crate::creature::Rider::ReactionOnTargeted
//! [`Rider::ReduceDamage`]: crate::creature::Rider::ReduceDamage
//! [`Rider::HalveAttackDamage`]: crate::creature::Rider::HalveAttackDamage
//! [`Rider::SaveOrCondition`]: crate::creature::Rider::SaveOrCondition
//! [`Rider::ConditionOnHit`]: crate::creature::Rider::ConditionOnHit
//! [`Rider::InjuryPoison`]: crate::creature::Rider::InjuryPoison
//! [`Rider::AlwaysSucceed`]: crate::creature::Rider::AlwaysSucceed
//! [`Rider::NothingOnSuccess`]: crate::creature::Rider::NothingOnSuccess
//! [`Rider::Digestion`]: crate::creature::Rider::Digestion
//! [`Rider::DamageThreshold`]: crate::creature::Rider::DamageThreshold
//! [`Rider::Swallow`]: crate::creature::Rider::Swallow
//! [`Rider::Regurgitate`]: crate::creature::Rider::Regurgitate
//! [`ReactionTrigger::Breached`]: crate::creature::ReactionTrigger::Breached
//! [`ReactionTrigger::EnemyGains`]: crate::creature::ReactionTrigger::EnemyGains

mod attack;
mod concentration;
mod conditions;
mod decide;
mod fighter;
mod on_hit;
mod reactions;
mod resolve;
mod saves;
mod search;
mod summon;
mod swallow;
#[cfg(test)]
mod test_support;
mod threshold;
mod turn;
mod value;
mod zones;

use crate::creature::{AttackTrigger, Creature, Zone};
use crate::prob::Rng;
use crate::rules::{Ability, AttackModifier, Condition, DamageRoll, SaveModifier, SpellSlots};
use crate::sim::Policy;
pub use value::expected_damage;

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

/// The per-fight availability of one move.
#[derive(Debug, Clone)]
struct MoveState {
    /// `None` for a move with no fixed budget.
    remaining: Option<u32>,
    /// Recharge moves are spent on use and roll to come back. Everything else
    /// stays charged forever.
    charged: bool,
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
    /// Parallel to `creature.reactions`.
    reactions: Vec<MoveState>,
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
    /// Ongoing attack-roll modifiers from a maintained buff (Bless, Bane),
    /// applied to every attack roll this fighter makes until
    /// [`Fight::end_concentration`] removes them - see
    /// [`crate::creature::Effect::Buff`] and
    /// [`crate::creature::Effect::SaveOrModifier`].
    attack_modifiers: Vec<AttackModifier>,
    /// As `attack_modifiers`, for saving throws - including this fighter's own
    /// concentration save, since [`crate::sim::fight::saves::saving_throw`] is
    /// the one place both go through.
    save_modifiers: Vec<SaveModifier>,
    once_per_turn_spent: bool,
    /// Sneak Attack's own once-per-turn budget. "Once per turn" means any
    /// creature's turn, so it comes back at the start of every turn rather
    /// than only this creature's own, and it is kept apart from
    /// `once_per_turn_spent` so a creature with both a Stunning-Strike-style
    /// rider and Sneak Attack gets one of each.
    sneak_attack_spent: bool,
    /// This creature's one reaction per round, shared by every reaction rider
    /// it has - see [`crate::creature::Rider::is_reaction`]. Back at the start
    /// of its own turn.
    reaction: bool,
    /// Parallel to `creature.riders`: whether each
    /// [`crate::creature::Rider::ConditionTriggeredWeaponDamage`] has been
    /// armed by this creature landing its trigger condition on an enemy.
    armed: Vec<bool>,
    /// Whether this creature fights in melee - see
    /// `fighter::fights_in_melee` - which is what
    /// [`Fight::ally_adjacent`] reads.
    melee: bool,
    /// Called up mid-fight by this roster index - see
    /// [`crate::creature::Effect::Summon`]. A summon fights on its
    /// summoner's side and can be attacked and destroyed, but it is not one
    /// of the combatants the fight is *about*: it takes no turn of its own,
    /// its death is not a death on its side, neither its hit points nor its
    /// maximum count towards how healthy that side finished, and a side with
    /// nothing left but summons has lost. It goes when its summoner does.
    summoned_by: Option<(usize, usize)>,
    /// The once-a-turn extra damage rider's budget
    /// ([`crate::creature::Rider::OncePerTurnDamage`]), spent on the first
    /// hit it rides and back at the start of this creature's *own* turn -
    /// "once on each of your turns", unlike Sneak Attack's "once per turn",
    /// which `sneak_attack_spent` refreshes on everybody's.
    once_per_turn_damage_spent: bool,
    /// A reactive AC bonus still standing from a reaction already spent -
    /// [`crate::creature::Rider::ReactionOnTargeted`] with `lasting` - and
    /// what kind of attack it answers. Gone at the start of this creature's
    /// next turn, with the reaction itself.
    reactive_ac: Option<(AttackTrigger, i32)>,
    /// Killed outright by massive damage, so healing cannot bring it back.
    /// See [`Creature::player_character`].
    dead: bool,
    /// A spell slot has already been expended this turn - see
    /// [`Fighter::can_cast`]. Back at the start of this creature's turn.
    slot_spent_this_turn: bool,
    /// Damage taken this turn - whoever's turn it is - from creatures this
    /// one has swallowed, which is what
    /// [`crate::creature::Rider::Regurgitate`] measures.
    inside_damage: i32,
    /// Where this creature stands around each creature with a mouth, indexed
    /// by that creature's roster seat - see [`crate::creature::Zone`]. Read
    /// only for a seat that has one.
    zones: Vec<Zone>,
    /// Stood up from Prone as this turn began, which costs a move.
    stood_up: bool,
    dealt: i64,
    /// Resource points and limited uses burnt. This is the second column of the
    /// table `DESIGN.md` wants, because a win probability means nothing without
    /// what it cost to get.
    spent: u32,
}

/// How one active condition instance ends.
///
/// Splitting this out of [`crate::rules::Duration`] rather than storing the
/// duration itself keeps the roster indices - which only make sense once a
/// condition has actually been pinned to an applier and a victim - out of the
/// data an ability declares.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Expiry {
    /// Cleared at the start of this roster index's turn.
    TurnStart(usize),
    /// Cleared at the end of this roster index's turn, once `ends_left` of
    /// them have passed - see [`crate::rules::Duration::ApplierNextTurnEnd`].
    TurnEnd { who: usize, ends_left: u8 },
    /// Cleared once `left` of this roster index's turns have started - see
    /// [`crate::rules::Duration::Rounds`].
    Rounds { who: usize, left: u32 },
    /// A saving throw at the end of the victim's own turn, clearing the
    /// condition on a success. See [`crate::rules::Duration::SaveEndTurn`].
    SaveEachTurn {
        victim: usize,
        ability: Ability,
        dc: i32,
    },
    /// Held by this roster index - swallowed by it - until it lets go or
    /// dies. No clock touches it; see `swallow`.
    HeldBy(usize),
    /// Cleared once every creature has had its turn this round.
    RoundEnd,
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
    /// A per-target attack-roll and saving-throw modifier maintained by Bless
    /// or Bane - see [`crate::creature::Effect::Buff`] and
    /// [`crate::creature::Effect::SaveOrModifier`]. Ending concentration
    /// removes exactly these two modifiers from exactly these targets, the
    /// same "clear what this held" contract as
    /// [`ConcentrationEffect::Condition`] above.
    Modifiers {
        targets: Vec<usize>,
        attack_modifier: AttackModifier,
        save_modifier: SaveModifier,
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
    /// Where to stand around the target before acting, when the target is a
    /// creature with a mouth - see [`crate::creature::Zone`].
    pub zone: Option<Zone>,
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
    /// Whose turn is being resolved right now, if anyone's - a legendary
    /// action between turns has none. Decides whether "the end of the
    /// applier's next turn" is this turn's end or the one after.
    acting: Option<usize>,
    /// Set while an aura or a reaction resolves: everything the move does
    /// lands on this one creature - the enemy starting its turn, or whoever
    /// set the reaction off - rather than on every enemy an area would catch,
    /// and a strike whose target is gone is not redirected.
    sole_target: Option<usize>,
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

        let mut fight = Self {
            order: rolled.into_iter().map(|(_, _, i)| i).collect(),
            fighters,
            turns_lost: [0; 2],
            max_rounds,
            budget,
            rollout_plan: None,
            acting: None,
            sole_target: None,
        };
        // Out in front of anything with a mouth, one move from closing in.
        fight.place_everyone(Zone::Range);
        fight
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
            self.end_of_round();
        }
        self.unresolved(self.max_rounds)
    }

    /// Everything lasting until the end of the round ends.
    fn end_of_round(&mut self) {
        for f in self.fighters.iter_mut() {
            f.conditions
                .retain(|&(_, expiry)| expiry != Expiry::RoundEnd);
        }
    }

    fn phase(&mut self, round: u32, phase: usize, rng: &mut Rng, log: &mut Option<Vec<String>>) {
        let who = self.order[phase / 2];
        if phase.is_multiple_of(2) {
            self.take_turn(round, who, rng, log, None);
        } else {
            self.legendary_windows(round, who, rng, log);
        }
    }

    /// How many combatants `side` has left standing. A summon does not
    /// count: a side whose last creature has dropped has lost, however many
    /// doubles are still swirling about.
    fn living(&self, side: Side) -> u32 {
        self.fighters
            .iter()
            .filter(|f| f.side == side && f.alive() && !f.is_summon())
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
            // A destroyed summon is not a death, and its hit points are not
            // part of how healthy its side finished - it is spent kit, and
            // what it cost is already counted where it was paid for.
            if f.is_summon() {
                damage[s] += f.dealt;
                continue;
            }
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
}

/// The extra damage and side effects one attack roll will carry - see
/// [`Fight::extra_damage_plan`].
#[derive(Debug, Clone, Default)]
struct ExtraPlan {
    rolls: Vec<DamageRoll>,
    /// Sneak Attack qualified, so a hit spends its once-per-turn budget.
    sneak_attack: bool,
    /// A once-a-turn damage rider joined this blow, so a hit spends its
    /// budget for the turn - see
    /// [`crate::creature::Rider::OncePerTurnDamage`].
    once_per_turn_damage: bool,
    /// The Cunning Strike effect a Sneak Attack die was spent on.
    cunning: Option<Cunning>,
}

/// A Cunning Strike effect, at the rogue's Cunning Strike DC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cunning {
    Poison { dc: i32 },
    Trip { dc: i32 },
}

/// How a reaction answered a hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    /// [`crate::creature::Rider::ReduceDamage`], and by how much.
    Deflected(i32),
    /// [`crate::creature::Rider::HalveAttackDamage`].
    Halved,
}

/// Which of the three move lists is being read. Exists so the borrow of a list
/// and the borrow of its per-fight state cannot drift apart.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    Action,
    Bonus,
    Legendary,
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
    use crate::creature::{Effect, Strike};
    use crate::rules::DamageKind;
    use crate::sim::fight::test_support::{no_log, puncher};

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
}
