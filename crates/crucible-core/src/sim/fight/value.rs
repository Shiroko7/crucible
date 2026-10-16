//! What a move is worth right now, in expected damage or healing - the
//! number every ranking policy compares.

use crate::creature::{Boon, Creature, Effect, Move, Reach, Rider, Strike, Zone};
use crate::prob::Pmf;
use crate::rules::{
    hit_outcomes_with, AttackModifier, Condition, DamageKind, DamageRoll, Duration, HealRoll,
    Reduction, RollMode,
};
use crate::sim::fight::attack::attack_mode_at;
use crate::sim::fight::reactions::standing_ac_boost;
use crate::sim::fight::threshold::reducer;
use crate::sim::fight::{Fight, Fighter, Slot};
use crate::sim::{Budget, Policy, Side};

/// What a move would be worth *if* something else were true - the question a
/// setup move is scored by.
///
/// A move that only helps the next blow is worth exactly what it adds to that
/// blow, so scoring it means valuing an attack under a hypothetical: with
/// advantage on the roll (Steady Aim), or with extra dice riding on every hit
/// (a mark on the target, an enchantment on the blade). One turn's worth of
/// it, which for anything lasting longer than a turn is a floor rather than
/// the whole story - see [`Fight::boost_gain`].
#[derive(Clone, Copy, Default)]
pub(super) struct Boost<'b> {
    /// ... the attacker rolled with advantage.
    pub advantage: bool,
    /// ... every hit carried something more.
    pub extra: Extra<'b>,
}

/// The extra damage half of a [`Boost`].
#[derive(Clone, Copy, Default)]
pub(super) enum Extra<'b> {
    #[default]
    None,
    /// Dice on every hit against a marked target - see
    /// [`Rider::BonusDamageVsQuarry`].
    Quarry(DamageRoll),
    /// A boon's dice, on the swings that boon actually rides - which is the
    /// boon's own question to answer, not this one's.
    Boon(&'b Boon),
}

impl<'b> Boost<'b> {
    fn with_advantage() -> Self {
        Self {
            advantage: true,
            extra: Extra::None,
        }
    }

    fn of(extra: Extra<'b>) -> Self {
        Self {
            advantage: false,
            extra,
        }
    }

    /// What this hypothetical adds to one swing, if anything.
    fn rolls_on(&self, strike: &Strike) -> Option<DamageRoll> {
        match self.extra {
            Extra::None => None,
            Extra::Quarry(roll) => Some(roll),
            Extra::Boon(boon) => boon.damage_on(strike),
        }
    }
}

impl<'a> Fight<'a> {
    /// What `m` is worth to `me` against `target` right now, in expected hit
    /// points: the damage it deals - every damage rider the attack would
    /// actually carry included, so Sneak Attack and a slaying weapon's dice
    /// count towards choosing the move that earns them - or, for a heal, the
    /// downed ally it would bring back. `boost` scores it under a
    /// hypothetical; see [`Boost`].
    ///
    /// A move none of which can land on `target` from here - out of reach
    /// around a creature with a mouth, or swallowed - is not a choice at all,
    /// and neither is one whose double is dead or whose blade has gone out
    /// ([`Fight::requirement_met`]).
    pub(super) fn move_value(&self, me: usize, target: usize, m: &Move, boost: Boost<'_>) -> f64 {
        if !self.requirement_met(me, m) || !self.move_lands(me, target, m) {
            return f64::NEG_INFINITY;
        }
        if m.riders.contains(&Rider::Universal) {
            return self.with_cast(m, target, || self.universal_value(me, target, m));
        }
        let value = self.with_cast(m, target, || {
            self.effect_value(me, target, &m.effect, &m.riders, m.reach, boost)
        });
        // Letting go of a spell it is holding for one that would do nothing
        // anybody here can count is no choice at all.
        if m.concentration
            && self.fighters[me].concentration.is_some()
            && value <= 0.0
            && m.worth == 0
        {
            return f64::NEG_INFINITY;
        }
        // What its author says it is worth beyond its damage, while it
        // would still change something.
        if value.is_finite() && m.worth != 0 && self.would_change(me, target, &m.effect) {
            value + f64::from(m.worth)
        } else {
            value
        }
    }

    /// Would `effect` leave `target`, or `me`, under something it is not
    /// under already?
    fn would_change(&self, me: usize, target: usize, effect: &Effect) -> bool {
        match effect {
            Effect::Save(save) if !save.on_failure.is_empty() => save
                .on_failure
                .iter()
                .any(|&(landed, _)| !self.fighters[target].has(|c| c == landed)),
            Effect::Stance { condition } => !self.fighters[me].has(|c| c == *condition),
            Effect::Sequence(parts) => parts.iter().any(|p| self.would_change(me, target, p)),
            Effect::Part { effect, .. } => self.would_change(me, target, effect),
            _ => true,
        }
    }

    /// What being up to `feet` away is worth to `me` right now: half of what
    /// each enemy standing next to it could expect to do to it with its best
    /// action - counting only those that could not simply walk after it. An
    /// enemy with the speed to close the distance again loses nothing by
    /// it, and a turn spent leaving one is a turn wasted.
    fn worth_of_leaving(&self, me: usize, feet: u32) -> f64 {
        if !self.on_grid() {
            return 0.0;
        }
        let (mine, side) = (self.fighters[me].creature, self.fighters[me].side);
        self.fighters
            .iter()
            .enumerate()
            .filter(|&(i, f)| {
                f.side != side
                    && f.alive()
                    && self.within(i, me, crate::sim::arena::SQUARE)
                    && (self.squares(i) as u32) * crate::sim::arena::SQUARE < feet
            })
            .map(|(_, f)| {
                let best = f
                    .creature
                    .actions
                    .iter()
                    .map(|m| m.effect.mean_damage(mine))
                    .fold(0.0, f64::max);
                best / 2.0
            })
            .sum()
    }

    fn effect_value(
        &self,
        me: usize,
        target: usize,
        effect: &Effect,
        riders: &[Rider],
        reach: Reach,
        boost: Boost<'_>,
    ) -> f64 {
        let attacker = self.fighters[me].creature;
        let victim = self.fighters[target].creature;
        match effect {
            // A part aimed at a creature out of its reach - swallowed, out of
            // a bite's reach, behind a cone - lands nowhere.
            Effect::Strikes { strike, .. }
                if !self.in_reach(me, target, reach, Some(strike.kind)) =>
            {
                0.0
            }
            Effect::Save(_) | Effect::AutoHit { .. } | Effect::Displace(_)
                if !self.in_reach(me, target, reach, None) =>
            {
                0.0
            }
            // One swing at everything in reach: what it does to the other
            // side, less what it does to its own.
            Effect::Strikes { strike, .. } if riders.contains(&Rider::EachInReach) => {
                let rest: Vec<Rider> = riders
                    .iter()
                    .filter(|r| **r != Rider::EachInReach)
                    .cloned()
                    .collect();
                let side = self.fighters[me].side;
                self.swept(me, target, reach, strike.kind)
                    .into_iter()
                    .map(|i| {
                        let blow = self.expected_strikes(me, i, strike, 1, &rest, boost);
                        match self.fighters[i].side == side {
                            true => -blow,
                            false => blow,
                        }
                    })
                    .sum()
            }
            Effect::Strikes { strike, count } => {
                self.expected_strikes(me, target, strike, *count, riders, boost)
            }
            Effect::Displace(d) => self.fold_value(me, target, d),
            // Hiding is worth what striking unseen adds to its best blow,
            // as often as it works - and nothing to something hidden
            // already.
            Effect::Hide { bonus } => match self.fighters[me].has(Condition::unseen) {
                true => f64::NEG_INFINITY,
                false => {
                    let needed = self.keenest(me) - bonus;
                    let chance = f64::from((21 - needed).clamp(0, 20)) / 20.0;
                    chance * self.first_blow_gain(me, target)
                }
            },
            Effect::Heal(roll) => self.heal_value(me, roll),
            Effect::Sequence(parts) => parts
                .iter()
                .map(|p| self.effect_value(me, target, p, riders, reach, boost))
                .sum(),
            Effect::Part {
                effect,
                riders: own,
                reach: own_reach,
            } => {
                let riders: Vec<Rider> = riders.iter().chain(own).cloned().collect();
                self.effect_value(me, target, effect, &riders, own_reach.within(reach), boost)
            }
            // A save only some creature types are subject to, against a
            // target that is not one: it catches nobody.
            Effect::Save(save)
                if save
                    .requires_type
                    .as_ref()
                    .is_some_and(|t| !victim.is_creature_type(t)) =>
            {
                f64::NEG_INFINITY
            }
            // Passed on while they keep failing: each one in turn, as
            // likely as everyone before it was to fail.
            Effect::Save(save) if chain_of(riders).is_some() => {
                let feet = chain_of(riders).unwrap_or(0);
                let (mut total, mut reaching, mut struck) = (0.0, 1.0, Vec::new());
                let mut next = Some(target);
                while let Some(i) = next {
                    struck.push(i);
                    total += reaching * self.save_value(me, i, save, riders);
                    reaching *= save.failure_chance(self.fighters[i].creature);
                    next = match reaching > 0.01 {
                        true => self.arc_from(me, i, feet, &struck),
                        false => None,
                    };
                }
                total
            }
            // Everyone on the line it would ride, its own side counted
            // against it.
            Effect::Save(save) if line_of(riders).is_some() => {
                let side = self.fighters[me].side;
                let (_, caught) = self.line_from(me, target, line_of(riders).unwrap_or(0));
                if caught.is_empty() {
                    return f64::NEG_INFINITY;
                }
                caught
                    .into_iter()
                    .map(|i| match self.fighters[i].side == side {
                        true => -self.save_value(me, i, save, riders),
                        false => self.save_value(me, i, save, riders),
                    })
                    .sum()
            }
            // A burst around its target that leaves the target out: worth
            // what it does to everyone else it catches.
            Effect::Save(save) if riders.contains(&Rider::SparesTarget) => self
                .caught(me, reach)
                .into_iter()
                .filter(|&i| i != target)
                .map(|i| self.save_value(me, i, save, riders))
                .sum(),
            // Everyone it would catch, as it catches them when it lands:
            // whoever it was aimed at first, and no more than it can hold.
            Effect::Save(save) => {
                let mut caught: Vec<usize> = self
                    .caught(me, reach)
                    .into_iter()
                    .filter(|&i| match &save.requires_type {
                        None => true,
                        Some(t) => self.fighters[i].creature.is_creature_type(t),
                    })
                    .collect();
                if let Some(max) = save.max_targets {
                    if let Some(at) = caught.iter().position(|&i| i == target) {
                        caught[..=at].rotate_right(1);
                    }
                    caught.truncate(max as usize);
                }
                if !caught.contains(&target) {
                    caught.push(target);
                }
                caught
                    .into_iter()
                    .map(|i| self.save_value(me, i, save, riders))
                    .sum()
            }
            Effect::AutoHit { damage } => {
                let weak = self.through_weak_spot(me, target, None);
                let defending = self.boon_resistances(target);
                let reduce = reducer(attacker, victim, weak, &defending);
                let reduce = unmirrored(victim, &reduce);
                damage
                    .iter()
                    .map(|r| {
                        self.past_threshold(
                            target,
                            weak,
                            r.pmf(false, r.reduction_against(&reduce)),
                        )
                        .mean()
                    })
                    .sum()
            }
            // Going nowhere for no reason is not a choice at all.
            Effect::Teleport { feet } => match self.worth_of_leaving(me, *feet) {
                worth if worth > 0.0 => worth,
                _ => f64::NEG_INFINITY,
            },
            // A mark is worth what it will add to this creature's blows over
            // the rounds it lasts - and nothing at all if no rider is waiting
            // on it. Marking what is already marked is not a worse choice
            // than swinging, it is no choice at all: it would change nothing
            // whatsoever, so it is ruled out the same way a move that cannot
            // reach is, and no playstyle - not even one reaching for the most
            // expensive thing it can find - spends a turn on it.
            Effect::Afflict {
                condition,
                duration,
            } => match condition {
                _ if self.fighters[target].has(|c| c == *condition) => f64::NEG_INFINITY,
                Condition::Quarry => self.fighters[me]
                    .creature
                    .riders
                    .iter()
                    .find_map(quarry_dice)
                    .map_or(0.0, |roll| {
                        self.lasting_value(me, target, *duration, Boost::of(Extra::Quarry(roll)))
                    }),
                _ => 0.0,
            },
            // A boon likewise: putting up one already up is no choice at all,
            // so nothing ever spends a turn on the same form twice.
            Effect::Boon { which, duration } => match self.has_boon(me, *which) {
                true => f64::NEG_INFINITY,
                false => self.fighters[me]
                    .creature
                    .boons
                    .get(*which)
                    .map_or(0.0, |boon| {
                        self.lasting_value(me, target, *duration, Boost::of(Extra::Boon(boon)))
                    }),
            },
            // An aura is worth what it collects from whoever has to stand in
            // it, once per turn for as long as it is up - not what it adds to
            // this creature's own blow, which is the boon's question. Raising
            // one already up is no choice at all, exactly as with a boon.
            // Nor is dropping one to raise another: holding any of them up
            // already, it keeps the one it has.
            Effect::Aura { which, duration } => match (0..self.fighters[me]
                .creature
                .lasting_auras
                .len())
                .any(|i| self.has_aura(me, i))
                || self.has_aura(me, *which)
            {
                true => f64::NEG_INFINITY,
                false => self.fighters[me]
                    .creature
                    .lasting_auras
                    .get(*which)
                    .map_or(0.0, |aura| {
                        let horizon = match duration {
                            Duration::Rounds(rounds) | Duration::RoundsOrDamaged(rounds) => {
                                (*rounds).min(self.budget.depth.max(1))
                            }
                            _ => 1,
                        };
                        self.rounds_inside(target, aura, horizon)
                            * aura.effect.mean_damage(self.fighters[target].creature)
                    }),
            },
            // A double is worth the blow it will strike once it is standing.
            Effect::Summon { which } => self.summon_value(target, me, *which),
            // Whoever is inside, not `target`, takes it.
            Effect::HarmSwallowed { damage } => self
                .held_by(me)
                .into_iter()
                .map(|i| {
                    let defending = self.boon_resistances(i);
                    let reduce = reducer(attacker, self.fighters[i].creature, false, &defending);
                    damage
                        .iter()
                        .map(|r| r.pmf(false, r.reduction_against(&reduce)).mean())
                        .sum::<f64>()
                })
                .sum(),
            other => other.mean_damage(victim),
        }
    }

    /// What the saving throw `save` is worth against `target` alone.
    fn save_value(
        &self,
        me: usize,
        target: usize,
        save: &crate::creature::SaveEffect,
        riders: &[Rider],
    ) -> f64 {
        let attacker = self.fighters[me].creature;
        let victim = self.fighters[target].creature;
        // What the move adds against whatever this target is under
        // is part of its damage, as it is when it lands.
        let bonus = self.bonus_vs_condition(riders.iter(), target);
        let boosted;
        let save = if bonus.is_empty() {
            save
        } else {
            boosted = crate::creature::SaveEffect {
                damage: save.damage.iter().copied().chain(bonus).collect(),
                ..save.clone()
            };
            &boosted
        };
        let weak = self.through_weak_spot(me, target, None);
        let defending = self.boon_resistances(target);
        let reduce = reducer(attacker, victim, weak, &defending);
        let pmf = save.damage_pmf_by(victim, &unmirrored(victim, &reduce));
        self.past_threshold(target, weak, pmf).mean()
    }

    /// A bonus action's worth, given the action this turn already leads
    /// with: its own value, or - for one that sets that action up
    /// ([`Move::before_action`] with a stance granting advantage on the
    /// attacker's own roll, Steady Aim) - exactly what the advantage adds to
    /// it.
    ///
    /// Two things the action already settles are worth nothing again: a
    /// second spell slot this turn (the action spent the one allowed), and a
    /// second heal for the same downed ally the action is already bringing
    /// back.
    pub(super) fn bonus_value(
        &self,
        me: usize,
        target: usize,
        m: &Move,
        lead: Option<&Move>,
    ) -> f64 {
        if lead.is_some_and(|a| a.carries(&Rider::TakesBonusAction)) {
            return f64::NEG_INFINITY;
        }
        // Nor is a second spell to concentrate on in the one turn: it would
        // end the first as it was cast.
        if m.concentration && lead.is_some_and(|a| a.concentration) {
            return f64::NEG_INFINITY;
        }
        if let Some(a) = lead {
            let second_slot = a.spell_slot_level.is_some() && m.spell_slot_level.is_some();
            let second_heal =
                matches!(a.effect, Effect::Heal(_)) && matches!(m.effect, Effect::Heal(_));
            if second_slot || second_heal {
                return f64::NEG_INFINITY;
            }
        }
        // A bonus action that is nothing but footwork is taken for the
        // footwork: a Dash by something spending its whole turn running, a
        // Disengage by something that fights from range and is held in a
        // reach. Otherwise it is no choice at all.
        if matches!(&m.effect, Effect::Sequence(parts) if parts.is_empty()) {
            let name = m.name.to_ascii_lowercase();
            let wanted = match () {
                _ if name.starts_with("dash") => lead.is_none() && self.on_grid(),
                _ if name.starts_with("disengage") => !self.fighters[me].melee && self.pinned(me),
                _ => true,
            };
            return match wanted {
                true => 0.0,
                false => f64::NEG_INFINITY,
            };
        }
        let sets_up = m.before_action
            && m.effect
                .stance()
                .is_some_and(Condition::advantage_on_attacks);
        if !sets_up {
            return self.move_value(me, target, m, Boost::default());
        }
        // Aiming at nothing it will roll to hit is no choice at all.
        let gain = lead.map_or(0.0, |a| {
            let with = self.move_value(me, target, a, Boost::with_advantage());
            let without = self.move_value(me, target, a, Boost::default());
            with - without
        });
        match gain > 0.0 {
            true => gain,
            false => f64::NEG_INFINITY,
        }
    }

    /// What something that lasts is worth: what it adds to this creature's
    /// best blow, over however many rounds are worth counting.
    ///
    /// The horizon is the search's own depth budget ([`Budget::depth`]) - how
    /// far ahead this fight plans - capped by how long the effect actually
    /// lasts. That is a judgement, and it is the same judgement the solver
    /// makes when it stops playing a rollout out and scores the position;
    /// borrowing it rather than inventing a second number is the point. A
    /// mark worth `g` a turn for the next four rounds scores `4g`, so a
    /// ranking playstyle puts it up once and then swings, while one that
    /// spends nothing still never bothers.
    ///
    /// Nothing here counts what a boon is worth *defensively* - resistances
    /// have no attack value to gain - so a purely defensive form scores zero
    /// and is taken only by a playstyle that reaches for expensive things, or
    /// by the solver, which plays it out and sees the difference.
    fn lasting_value(&self, me: usize, target: usize, duration: Duration, boost: Boost<'_>) -> f64 {
        let horizon = match duration {
            Duration::Rounds(rounds) | Duration::RoundsOrDamaged(rounds) => {
                rounds.min(self.budget.depth.max(1))
            }
            // Anything ending at a turn boundary is one turn of it.
            _ => 1,
        };
        f64::from(horizon) * self.boost_gain(me, target, boost)
    }

    /// What a hypothetical is worth on this creature's own best blow: the
    /// most any attacking action it could take right now gains from it.
    fn boost_gain(&self, me: usize, target: usize, boost: Boost<'_>) -> f64 {
        let f = &self.fighters[me];
        Slot::Action
            .moves(f.creature)
            .iter()
            .zip(Slot::Action.states(f))
            .filter(|(m, state)| {
                first_strike(&m.effect).is_some()
                    && state.available()
                    && f.affords(m)
                    && f.can_cast(m.spell_slot_level)
                    && self.can_take(me, m)
            })
            .map(|(m, _)| {
                let with = self.move_value(me, target, m, boost);
                let without = self.move_value(me, target, m, Boost::default());
                match with.is_finite() && without.is_finite() {
                    true => with - without,
                    false => 0.0,
                }
            })
            .fold(0.0, f64::max)
    }

    /// How many of the next `horizon` rounds `target` can be counted on to
    /// spend inside `aura`, were it raised now.
    ///
    /// The first, always: it is put where the target is. After that only
    /// something that cannot or will not leave stays - one held where it
    /// stands, or one that fights hand to hand against an emanation its
    /// enemy carries with it. Anything else walks out of a pool on the
    /// ground, or backs away from a ring, and is counted for a quarter of
    /// each later round. And a save that does nothing when it is made is
    /// worth half as much against something that can simply decide to make
    /// it, for as long as it has such decisions left.
    fn rounds_inside(&self, target: usize, aura: &Move, horizon: u32) -> f64 {
        let victim = &self.fighters[target];
        let held = victim.has(Condition::zeroes_speed) || victim.creature.speed == 0;
        let placed = crate::sim::fight::grid::anchored(aura);
        let stays = held || (!placed && victim.melee);
        let later = f64::from(horizon.saturating_sub(1));
        let rounds = 1.0 + later * if stays || !self.on_grid() { 1.0 } else { 0.25 };
        let all_or_nothing = matches!(&aura.effect, Effect::Save(save) if !save.half_on_success);
        let refusals: u32 = victim
            .creature
            .riders
            .iter()
            .zip(&victim.rider_uses)
            .filter(|(r, _)| matches!(r, Rider::AlwaysSucceed { ability: None, .. }))
            .map(|(_, &left)| left)
            .sum();
        match all_or_nothing && refusals > 0 {
            true => rounds - 0.5 * rounds.min(f64::from(refusals)),
            false => rounds,
        }
    }

    /// What advantage on one attack roll - the first it makes, and no more -
    /// would add to the best of `me`'s attacking actions: what striking
    /// from hiding is worth.
    fn first_blow_gain(&self, me: usize, target: usize) -> f64 {
        let f = &self.fighters[me];
        f.creature
            .actions
            .iter()
            .filter_map(|m| {
                let strike = first_strike(&m.effect)?;
                let blow = |boost| self.expected_strikes(me, target, strike, 1, &m.riders, boost);
                Some(blow(Boost::with_advantage()) - blow(Boost::default()))
            })
            .fold(0.0, f64::max)
    }

    /// What one of the things anybody can do is worth to `me` right now -
    /// see [`Rider::Universal`]. Each has its moment, and outside it is no
    /// choice at all, so that a creature with nothing better does not Dodge
    /// when it should be running at its enemy:
    ///
    /// - Dodge, once bloodied and playing it safe, with an enemy upon it;
    /// - Disengage, held in somebody's reach, for what getting clear saves;
    /// - Help, for what advantage adds to an ally's first blow at the
    ///   target;
    /// - Shove, for the same thing, as often as the target goes down;
    /// - Grapple, against something that fights from a distance and can
    ///   still walk away: half of this creature's own best blow, as often
    ///   as the hold takes.
    ///
    /// Something working down its own list as written never reaches for
    /// any of them.
    fn universal_value(&self, me: usize, target: usize, m: &Move) -> f64 {
        let f = &self.fighters[me];
        if f.policy == Policy::InOrder {
            return f64::NEG_INFINITY;
        }
        let side = f.side;
        let victim = &self.fighters[target];
        // What advantage on the first blow at `target` is worth to the
        // ally it is worth most to - one that fights hand to hand, for a
        // target on the ground.
        let opening = |melee_only: bool| {
            (0..self.fighters.len())
                .filter(|&i| {
                    let ally = &self.fighters[i];
                    i != me
                        && ally.side == side
                        && ally.alive()
                        && !ally.incapacitated()
                        && (ally.melee || !melee_only)
                })
                .map(|i| self.first_blow_gain(i, target))
                .fold(0.0, f64::max)
        };
        let fails = |save: &crate::creature::SaveEffect| save.failure_chance(victim.creature);
        let worth = match &m.effect {
            Effect::Stance { .. } => {
                let beset = self.fighters.iter().enumerate().any(|(i, e)| {
                    e.side != side && e.alive() && e.melee && self.feet_between(me, i) <= 5
                });
                match f.policy == Policy::Defensive && f.bloodied() && beset {
                    true => return 0.0,
                    false => 0.0,
                }
            }
            Effect::Sequence(_) => match self.pinned(me) && !f.melee {
                true => self.worth_of_leaving(me, f.creature.speed),
                false => 0.0,
            },
            Effect::Afflict { condition, .. } if !victim.has(|c| c == *condition) => opening(false),
            Effect::Save(save) => match save.on_failure.first() {
                Some((Condition::Prone, _))
                    if !victim.has(|c| c == Condition::Prone)
                        && !victim.creature.immune_to_condition(Condition::Prone) =>
                {
                    fails(save) * opening(true)
                }
                Some((Condition::Grappled, _))
                    if !victim.melee
                        && !victim.has(Condition::zeroes_speed)
                        && !victim.creature.immune_to_condition(Condition::Grappled) =>
                {
                    let blow = f
                        .creature
                        .actions
                        .iter()
                        .filter(|a| !a.riders.contains(&Rider::Universal))
                        .map(|a| a.effect.mean_damage(victim.creature))
                        .fold(0.0, f64::max);
                    fails(save) * blow / 2.0
                }
                _ => 0.0,
            },
            _ => 0.0,
        };
        match worth > 0.0 {
            true => worth,
            false => f64::NEG_INFINITY,
        }
    }

    /// What calling up `me`'s `which`th summon is worth: one turn of the best
    /// blow it can be commanded to strike against `target`.
    ///
    /// Its body - something else for an enemy to hit - is worth more than
    /// that, and is not counted: there is no threat model here to say whether
    /// anything would bother attacking it.
    fn summon_value(&self, target: usize, me: usize, which: usize) -> f64 {
        let victim = self.fighters[target].creature;
        self.fighters[me]
            .creature
            .summons
            .get(which)
            .map_or(0.0, |summon| {
                summon
                    .actions
                    .iter()
                    .map(|m| m.effect.mean_damage(victim))
                    .fold(0.0, f64::max)
            })
    }

    /// Expected damage of `count` strikes, the first with every rider that
    /// would apply to it; later swings without the once-per-turn Sneak
    /// Attack the first one would already have claimed.
    pub(super) fn expected_strikes(
        &self,
        me: usize,
        target: usize,
        strike: &Strike,
        count: u32,
        riders: &[Rider],
        boost: Boost<'_>,
    ) -> f64 {
        let attacker = &self.fighters[me];
        let victim = &self.fighters[target];
        let mode = attack_mode_at(
            strike.mode,
            attacker,
            victim,
            boost.advantage,
            self.spot(me, target, strike.kind),
        );
        let force_crit = victim.has(Condition::auto_crits);
        // What the hypothetical would add to each of these swings, on top of
        // every rider they already carry.
        let hypothetical = boost.rolls_on(strike);
        let with_boost = |mut rolls: Vec<DamageRoll>| {
            rolls.extend(hypothetical);
            rolls
        };
        let first = self.extra_damage_plan(me, target, strike, riders, mode, true);
        let mut total = self
            .aimed_hit(
                me,
                target,
                strike,
                mode,
                force_crit,
                &with_boost(first.rolls),
                riders,
            )
            .0;
        if count > 1 {
            let rest = self.extra_damage_plan(me, target, strike, riders, mode, false);
            total += f64::from(count - 1)
                * self
                    .aimed_hit(
                        me,
                        target,
                        strike,
                        mode,
                        force_crit,
                        &with_boost(rest.rolls),
                        riders,
                    )
                    .0;
        }
        if riders.contains(&Rider::HalfOnMiss) {
            let ac = victim.creature.ac + standing_ac_boost(victim, strike.kind);
            let o = hit_outcomes_with(strike.to_hit, mode, ac, &attacker.attack_modifiers);
            let defending = self.defending(me, target, Some(strike), riders);
            let reduce = reducer(attacker.creature, victim.creature, false, &defending);
            let whole = crate::creature::Displace::mean_of(&strike.damage, &reduce);
            total += f64::from(count) * (1.0 - o.hit - o.crit) * whole / 2.0;
        }
        // What every one of them landing adds - see `Rider::AllHit`.
        if count >= 2 {
            let ac = victim.creature.ac + standing_ac_boost(victim, strike.kind);
            let o = hit_outcomes_with(strike.to_hit, mode, ac, &attacker.attack_modifiers);
            let every = (o.hit + o.crit).powi(count as i32);
            let reduce = |k| match victim.creature.mirrors(k) {
                Some(_) => Reduction::Immune,
                None => victim.creature.reduction(k),
            };
            for rider in riders {
                if let Rider::AllHit { damage, .. } = rider {
                    total += every * crate::creature::Displace::mean_of(damage, &reduce);
                }
            }
        }
        total
    }

    /// Expected damage of one attack roll from `me` at `target` in `mode`,
    /// with `extra` riding on a hit - and whether it is aimed at `target`'s
    /// weak spot.
    ///
    /// From inside there is nothing else to aim at. From outside, an open
    /// weak spot is taken when it is worth more than the shell: it skips the
    /// threshold but resists some damage, so a hit big enough to breach can
    /// do better against the shell, where it lands whole.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn aimed_hit(
        &self,
        me: usize,
        target: usize,
        strike: &Strike,
        mode: RollMode,
        force_crit: bool,
        extra: &[DamageRoll],
        riders: &[Rider],
    ) -> (f64, bool) {
        let attacker = &self.fighters[me];
        let victim = &self.fighters[target];
        let defending = self.defending(me, target, Some(strike), riders);
        // A blade the defender already has raised is part of how hard it is
        // to hit - see `reactions::standing_ac_boost` - and so is whoever
        // is standing in the way.
        let ac = victim.creature.ac
            + standing_ac_boost(victim, strike.kind)
            + self.spot(me, target, strike.kind).cover;
        let hit = |weak: bool| {
            let threshold = match weak {
                true => None,
                false => victim.creature.damage_threshold().map(|(t, _)| t),
            };
            let reduce = reducer(attacker.creature, victim.creature, weak, &defending);
            let reduce = unmirrored(victim.creature, &reduce);
            expected_hit(
                &reduce,
                threshold,
                strike,
                ac,
                mode,
                force_crit,
                &attacker.attack_modifiers,
                extra,
            )
        };
        if !self.through_weak_spot(me, target, Some(strike.kind)) {
            return (hit(false), false);
        }
        if attacker.swallowed_by() == Some(target) {
            return (hit(true), true);
        }
        let (through, shell) = (hit(true), hit(false));
        if through >= shell {
            (through, true)
        } else {
            (shell, false)
        }
    }

    /// A heal is worth a whole ally back in the fight when one is down - its
    /// hit point maximum - and nothing otherwise: topping up someone still
    /// standing is almost never worth a turn in a fight this short.
    fn heal_value(&self, me: usize, _roll: &HealRoll) -> f64 {
        let side = self.fighters[me].side;
        self.fighters
            .iter()
            .enumerate()
            .filter(|&(i, f)| {
                f.side == side
                    && f.can_revive()
                    && self.reaches(me, i)
                    && (i == me || self.within(me, i, self.heal_reach()))
            })
            .map(|(_, f)| f64::from(f.creature.hp))
            .fold(0.0, f64::max)
    }
}

/// The dice a [`Rider::BonusDamageVsQuarry`] would put on every hit against
/// a marked target - what marking one is worth, before it is marked.
fn quarry_dice(rider: &Rider) -> Option<DamageRoll> {
    match rider {
        Rider::BonusDamageVsQuarry {
            dice_count,
            dice_sides,
            bonus,
            damage_kind,
        } => Some(DamageRoll::new(
            *dice_count,
            *dice_sides,
            *bonus,
            *damage_kind,
        )),
        _ => None,
    }
}

/// `reduce`, for weighing a blow at `victim`: what it would hand on to
/// those around it (see [`Rider::MirrorDamage`]) is worth nothing against
/// it, whatever that then does to anybody else.
fn unmirrored<'r>(
    victim: &'r Creature,
    reduce: &'r dyn Fn(DamageKind) -> Reduction,
) -> impl Fn(DamageKind) -> Reduction + 'r {
    move |kind| match victim.mirrors(kind) {
        Some(_) => Reduction::Immune,
        None => reduce(kind),
    }
}

fn chain_of(riders: &[Rider]) -> Option<u32> {
    riders.iter().find_map(|r| match r {
        Rider::Chains { feet } => Some(*feet),
        _ => None,
    })
}

fn line_of(riders: &[Rider]) -> Option<u32> {
    riders.iter().find_map(|r| match r {
        Rider::RideLine { feet } => Some(*feet),
        _ => None,
    })
}

pub(super) fn first_strike(effect: &Effect) -> Option<&Strike> {
    match effect {
        Effect::Strikes { strike, .. } => Some(strike),
        Effect::Sequence(parts) => parts.iter().find_map(first_strike),
        Effect::Part { effect, .. } => first_strike(effect),
        _ => None,
    }
}

/// Expected damage of one attack roll in `mode` against `ac`, with `extra`
/// riding on a hit: the probability of each outcome times the mean of each
/// damage component, reduced by `reduce`. A `force_crit` target turns every
/// hit into a critical one.
///
/// Against a damage `threshold` the components no longer simply add - a hit
/// either clears it whole or lands nothing - so the hit's full distribution
/// is built and cut at the threshold instead.
#[allow(clippy::too_many_arguments)]
fn expected_hit(
    reduce: &dyn Fn(DamageKind) -> Reduction,
    threshold: Option<i32>,
    strike: &Strike,
    ac: i32,
    mode: RollMode,
    force_crit: bool,
    modifiers: &[AttackModifier],
    extra: &[DamageRoll],
) -> f64 {
    let o = hit_outcomes_with(strike.to_hit, mode, ac, modifiers);
    let rolls = || strike.damage.iter().chain(extra);
    let landed = |crit: bool| -> f64 {
        match threshold {
            None => rolls()
                .map(|r| r.pmf(crit, r.reduction_against(reduce)).mean())
                .sum(),
            Some(t) => rolls()
                .fold(Pmf::constant(0), |acc, r| {
                    acc.convolve(&r.pmf(crit, r.reduction_against(reduce)))
                })
                .map_values(move |d| if d >= t { d } else { 0 })
                .mean(),
        }
    };
    let (hit, crit) = if force_crit {
        (0.0, o.hit + o.crit)
    } else {
        (o.hit, o.crit)
    };
    hit * landed(false) + crit * landed(true)
}

/// What `m` is worth when `attacker` uses it on `target` at the start of a
/// fight, in expected damage - the number the ranking policies compare, with
/// every rider that would apply to the attack included (a slaying weapon's
/// dice, say) and none that needs a situation the fight has not produced yet
/// (Sneak Attack without advantage or an ally beside the target). Around a
/// creature with a mouth, the two are face to face at its mouth, where
/// everything reaches.
pub fn expected_damage(attacker: &Creature, m: &Move, target: &Creature) -> f64 {
    let mut fight = Fight {
        fighters: vec![
            Fighter::new(attacker, Side::A, Policy::Greedy, 0),
            Fighter::new(target, Side::B, Policy::Greedy, 1),
        ],
        order: vec![0, 1],
        turns_lost: [0; 2],
        max_rounds: 1,
        budget: Budget::default(),
        rollout_plan: None,
        acting: None,
        sole_target: None,
        grid: None,
        stride: None,
        pull_to: None,
        cast: Default::default(),
        tape: Default::default(),
        round: 1,
        cue: Default::default(),
        hurt: None,
        held: Vec::new(),
    };
    fight.place_everyone(Zone::Mouth);
    // It is the attacker's own turn, which is what a once-a-turn rider needs
    // to be true before it counts.
    fight.acting = Some(0);
    fight.move_value(0, 1, m, Boost::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::SaveEffect;
    use crate::prob::Rng;
    use crate::rules::{Ability, DamageKind, Duration};
    use crate::sim::fight::test_support::{fight_of, puncher};

    /// A spell only one creature type is subject to is never taken against
    /// anything else - not even by a policy that ranks on what it spends.
    #[test]
    fn a_type_restricted_save_is_never_chosen_against_the_wrong_type() {
        let mut caster = puncher("caster", 10, 30, 5, 2);
        caster.spell_slots.set_max(2, 3);
        caster.actions.insert(
            0,
            Move::new(
                "Hold",
                Effect::Save(SaveEffect {
                    ability: Ability::Wis,
                    dc: 15,
                    damage: Vec::new(),
                    half_on_success: false,
                    on_failure: vec![(Condition::Paralyzed, Duration::VictimTurn)],
                    max_targets: Some(1),
                    requires_type: Some("humanoid".to_string()),
                }),
            )
            .with_spell_slot(2),
        );
        let dragon =
            puncher("dragon", 10, 100, 5, 2).with_creature_type(crate::rules::CreatureType::Dragon);
        let bandit = puncher("bandit", 10, 100, 5, 2)
            .with_creature_type(crate::rules::CreatureType::Humanoid);
        for (foe, expected) in [(&dragon, Some(1)), (&bandit, Some(0))] {
            let roster = [(&caster, Side::A), (foe, Side::B)];
            let mut rng = Rng::new(1);
            let mut fight = Fight::new(
                &mut rng,
                &roster,
                [Policy::Nova; 2],
                5,
                Budget::default(),
                &mut None,
            );
            assert_eq!(fight.decide(1, 0, 1, &mut rng).action, expected);
            fight.fighters[0].policy = Policy::InOrder;
            assert_eq!(fight.decide(1, 0, 1, &mut rng).action, expected);
        }
    }

    /// A ranking policy spends its bonus action bringing a downed ally back
    /// rather than on a little damage.
    #[test]
    fn a_heal_is_worth_a_downed_ally() {
        let mut healer = puncher("healer", 10, 20, 5, 2);
        healer.bonus_actions.push(Move::new(
            "Jab",
            Effect::Strikes {
                strike: Strike::new(5, vec![DamageRoll::new(1, 4, 0, DamageKind::Piercing)]),
                count: 1,
            },
        ));
        healer.bonus_actions.push(Move::new(
            "Healing Word",
            Effect::Heal(HealRoll::new(1, 4, 3)),
        ));
        let mut friend = puncher("friend", 10, 30, 5, 2);
        friend.player_character = true;
        let enemy = puncher("enemy", 10, 30, 5, 2);
        let roster = [(&healer, Side::A), (&friend, Side::A), (&enemy, Side::B)];
        let (mut fight, mut rng) = fight_of(&roster, 13);

        assert_eq!(
            fight.decide(1, 0, 2, &mut rng).bonus,
            Some(0),
            "nobody down: jab"
        );
        fight.fighters[1].hp = 0;
        assert_eq!(
            fight.decide(1, 0, 2, &mut rng).bonus,
            Some(1),
            "friend down: heal"
        );
    }
}
