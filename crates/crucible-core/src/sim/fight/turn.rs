//! A turn and the round around it: start- and end-of-turn bookkeeping, the
//! action and bonus action, repeated saves at the end of a turn, and
//! legendary actions between turns.

use crate::creature::{Effect, Move, Rider, Strike, Tactic};
use crate::prob::Rng;
use crate::rules::{Ability, Condition};
use crate::sim::fight::fighter::refresh;
use crate::sim::fight::value::{first_strike, Boost};
use crate::sim::fight::{Expiry, Fight, FrameKind, Slot};
use crate::sim::{Plan, Policy};

/// How far a move has to reach, in feet, before whoever is making it would
/// rather make it from a distance.
const RANGED: u32 = 30;

impl<'a> Fight<'a> {
    /// Every combatant's turn opens a legendary window for the other side, and
    /// **one** use may be spent per window - not the whole pool.
    ///
    /// That distinction is invisible in a duel and decisive out of it. A dragon
    /// with three legendary uses facing one enemy gets one window a round and so
    /// spends one; facing four enemies it gets four windows and spends all three.
    /// Its legendary output scales with the size of the party opposing it, which
    /// is the opposite of the intuition that more bodies is straightforwardly
    /// better.
    pub(super) fn legendary_windows(
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

    pub(super) fn take_turn(
        &mut self,
        round: u32,
        who: usize,
        rng: &mut Rng,
        log: &mut Option<Vec<String>>,
        plan: Option<Plan>,
    ) {
        self.start_of_turn(who);
        self.acting = Some(who);
        if self.fighters[who].alive() {
            if self.recording() {
                let name = self.fighters[who].creature.name.clone();
                self.beat(
                    FrameKind::Turn,
                    Some(who),
                    None,
                    format!("{name}'s turn"),
                    None,
                );
            }
            self.auras_on(round, who, rng, log);
            self.digest(round, who, rng, log);
        }
        if self.fighters[who].alive() {
            self.raise(who, log);
            self.break_out(round, who, rng, log);
        } else {
            self.death_save(round, who, rng, log);
        }
        if !self.fighters[who].alive() {
            // A creature that is down keeps its place in the order: whatever
            // was set to last until the start or end of its turn ends anyway.
            self.end_of_turn(who);
            self.check_regurgitation(round, rng, log);
            self.acting = None;
            return;
        }
        if !self.turn(round, who, rng, log, plan) {
            self.turns_lost[self.fighters[who].side.index()] += 1;
        }
        // Runs whether or not the creature actually got to act: a paralyzed
        // creature still reaches the end of its own turn, which is exactly
        // when its next chance to shake the condition off falls.
        self.end_of_turn_saves(round, who, rng, log);
        self.end_of_turn(who);
        self.check_regurgitation(round, rng, log);
        self.acting = None;
    }

    /// Whatever is holding `who` that it can break out of for a price - see
    /// [`crate::creature::Rider::EscapeFor`] - it breaks out of when the
    /// price is one it can afford: what it expects to pay leaves it above
    /// half its hit points. Paying ends the concentration holding it.
    fn break_out(&mut self, round: u32, who: usize, rng: &mut Rng, log: &mut Option<Vec<String>>) {
        let Some((holder, roll)) = self.fighters[who].escape else {
            return;
        };
        // Nothing holds it any longer: the spell ended some other way.
        let held = matches!(
            &self.fighters[holder].concentration,
            Some(crate::sim::fight::ActiveConcentration {
                effect: crate::sim::fight::ConcentrationEffect::Condition { targets, .. },
            }) if targets.contains(&who)
        );
        if !held || !self.fighters[holder].alive() {
            self.fighters[who].escape = None;
            return;
        }
        let f = &self.fighters[who];
        let reduction = f.creature.reduction(roll.kind);
        let price = reduction.apply(roll.mean().round() as i32);
        if (f.hp - price) * 2 <= f.creature.hp {
            return;
        }
        let paid = roll.sample(rng, false, reduction);
        self.fighters[who].escape = None;
        self.fighters[holder].dealt += i64::from(paid);
        self.apply_damage(rng, who, paid);
        self.end_concentration(holder);
        let text = format!(
            "{} breaks out of {}'s hold, for {paid}",
            self.fighters[who].creature.name, self.fighters[holder].creature.name
        );
        if let Some(l) = log.as_mut() {
            l.push(format!("r{round} {text}"));
        }
        self.beat(FrameKind::Other, Some(who), Some(holder), text, None);
    }

    /// A player character that is down and not yet stable rolls for its
    /// life as its turn comes round: three successes leave it stable, three
    /// failures dead, a 1 is two failures and a 20 puts it back on its feet
    /// with one hit point - unless nothing can give it one.
    fn death_save(&mut self, round: u32, who: usize, rng: &mut Rng, log: &mut Option<Vec<String>>) {
        let f = &self.fighters[who];
        if !f.can_revive() || f.stable {
            return;
        }
        let mode = match f.has(|c| c == Condition::Doomed) {
            true => crate::rules::RollMode::Disadvantage,
            false => crate::rules::RollMode::Normal,
        };
        let unhealable = f.has(Condition::blocks_healing);
        let die = mode.roll(rng);
        let total = die - 2 * i32::from(f.exhaustion);
        let f = &mut self.fighters[who];
        let how = if die == 20 && !unhealable {
            f.hp = 1;
            f.death_successes = 0;
            f.death_failures = 0;
            "is back up on a 20".to_string()
        } else if die == 1 {
            f.death_failures += 2;
            format!(
                "fails a death save twice over ({} of 3)",
                f.death_failures.min(3)
            )
        } else if total < 10 {
            f.death_failures += 1;
            format!("fails a death save ({} of 3)", f.death_failures)
        } else {
            f.death_successes += 1;
            format!("makes a death save ({} of 3)", f.death_successes)
        };
        let end = if f.death_failures >= 3 {
            f.dead = true;
            " and dies"
        } else if f.death_successes >= 3 {
            f.stable = true;
            " and is stable"
        } else {
            ""
        };
        let text = format!("{} {how}{end}", f.creature.name);
        if let Some(l) = log.as_mut() {
            l.push(format!("r{round} {text}"));
        }
        self.beat(FrameKind::Other, Some(who), None, text, None);
    }

    /// The blow `who` makes at something walking out of its reach: its first
    /// action that is a melee attack and costs it nothing to make. One swing
    /// of it, whatever else that action does.
    fn opportunity_strike(&self, who: usize) -> Option<(&'a Move, &'a Strike)> {
        let f = &self.fighters[who];
        let creature = f.creature;
        creature.actions.iter().find_map(|m| {
            let strike = first_strike(&m.effect)?;
            let free = m.cost.is_none()
                && m.spell_slot_level.is_none()
                && matches!(m.uses, crate::creature::Uses::Unlimited);
            (!strike.kind.ranged && free && f.move_allowed(m)).then_some((m, strike))
        })
    }

    /// How far `who` can strike something leaving its reach right now, in
    /// feet - when it can at all: on its feet, with a reaction, and with
    /// something to swing.
    fn threat(&self, who: usize) -> Option<u32> {
        let f = &self.fighters[who];
        if !f.alive()
            || !f.reaction
            || f.incapacitated()
            || f.swallowed_by().is_some()
            || f.has(Condition::no_opportunity_attacks)
        {
            return None;
        }
        let (m, _) = self.opportunity_strike(who)?;
        Some(m.range.unwrap_or(crate::sim::arena::SQUARE))
    }

    /// Is `me` standing in the reach of an enemy that would strike it for
    /// walking out of it?
    pub(super) fn pinned(&self, me: usize) -> bool {
        let (side, here) = (self.fighters[me].side, self.fighters[me].pos);
        (0..self.fighters.len()).any(|e| {
            self.fighters[e].side != side
                && self.reaches(e, me)
                && self
                    .threat(e)
                    .is_some_and(|reach| self.feet_from(me, here, e) <= reach)
                && self.takes_opportunity(e, me)
        })
    }

    /// Would `me`, by how it is played, rather fight from where it stands
    /// than take a blow for leaving?
    ///
    /// Not something spending everything it has as fast as it can, nor
    /// something set on finishing one enemy: both go where they mean to and
    /// take what that costs. Something playing it safe never walks out of a
    /// reach once it is bloodied, and the rest stay as long as there is
    /// anything they can hit from here.
    fn holds_its_ground(&self, me: usize) -> bool {
        !matches!(self.fighters[me].policy, Policy::Nova | Policy::FocusFire)
    }

    /// Does `who` spend its reaction on a blow at `mover` as it leaves?
    ///
    /// By how it is played. Something working down a list, or spending
    /// everything as it comes, always does. Something playing it safe keeps
    /// its reaction, once bloodied, for whatever it has that turns a blow
    /// aside. Anything else weighs the blow against the best other use it
    /// has for a reaction - a move of its own waiting on a trigger, at what
    /// that would do and what its author says it is worth - and takes
    /// whichever is worth more.
    fn takes_opportunity(&self, who: usize, mover: usize) -> bool {
        let f = &self.fighters[who];
        let Some((m, strike)) = self.opportunity_strike(who) else {
            return false;
        };
        match f.policy {
            Policy::Nova | Policy::InOrder => true,
            Policy::Defensive
                if f.bloodied() && f.creature.riders.iter().any(|r| r.is_reaction()) =>
            {
                false
            }
            _ => {
                let blow =
                    self.expected_strikes(who, mover, strike, 1, &m.riders, Boost::default());
                let victim = self.fighters[mover].creature;
                let kept = f
                    .creature
                    .reactions
                    .iter()
                    .zip(&f.reactions)
                    .filter(|(r, state)| {
                        state.available()
                            && f.affords(&r.action)
                            && f.can_cast(r.action.spell_slot_level)
                            && f.move_allowed(&r.action)
                    })
                    .map(|(r, _)| r.action.effect.mean_damage(victim) + f64::from(r.action.worth))
                    .fold(0.0, f64::max);
                blow >= kept
            }
        }
    }

    /// `me` has a way to walk, from `start` along `path`: every enemy whose
    /// reach a step of it leaves gets its chance at a blow as that step is
    /// taken. Dropped, or stopped in its tracks, `me` goes no further - and
    /// `path` is cut to as far as it got.
    fn gauntlet(
        &mut self,
        rng: &mut Rng,
        me: usize,
        start: (i32, i32),
        path: &mut Vec<(i32, i32)>,
        record: bool,
        notes: &mut Vec<String>,
    ) {
        if path.is_empty() {
            return;
        }
        let side = self.fighters[me].side;
        let mut at = start;
        self.fighters[me].pos = start;
        for (step, next) in path.clone().into_iter().enumerate() {
            let leaving: Vec<usize> = (0..self.fighters.len())
                .filter(|&e| self.fighters[e].side != side && self.reaches(e, me))
                .filter(|&e| {
                    self.threat(e).is_some_and(|reach| {
                        self.feet_from(me, at, e) <= reach && self.feet_from(me, next, e) > reach
                    })
                })
                .collect();
            for e in leaving {
                // Asked again each time: a reaction spent on the last blow
                // is not there for this one.
                if self.threat(e).is_none() || !self.takes_opportunity(e, me) {
                    continue;
                }
                let (m, strike) = self.opportunity_strike(e).expect("it threatens");
                let mut blow = Move::new(
                    format!("{} (opportunity attack)", m.name),
                    Effect::Strikes {
                        strike: strike.clone(),
                        count: 1,
                    },
                );
                blow.riders = m.riders.clone();
                blow.range = m.range;
                blow.kind = m.kind;
                self.fighters[e].spend_reaction_budget();
                let previous = self.sole_target.replace(me);
                let slot = std::mem::replace(&mut self.cue.slot, "reaction");
                let mut said = String::new();
                self.apply(&blow, rng, e, me, record, &mut said);
                self.cue.slot = slot;
                self.sole_target = previous;
                if record {
                    notes.push(format!("{}: {said}", self.fighters[e].creature.name));
                }
                if !self.fighters[me].alive() {
                    break;
                }
            }
            if !self.fighters[me].alive() || self.fighters[me].has(Condition::zeroes_speed) {
                path.truncate(step);
                self.fighters[me].pos = at;
                return;
            }
            at = next;
            self.fighters[me].pos = next;
        }
    }

    /// How far the widest emanation `me` raised and is still holding up
    /// reaches, in feet. Not an aura it simply has: that is part of what it
    /// is, not something it cast to walk onto people.
    fn aura_reach(&self, me: usize) -> Option<u32> {
        self.fighters[me]
            .creature
            .lasting_auras
            .iter()
            .enumerate()
            .filter(|(i, m)| self.has_aura(me, *i) && !crate::sim::fight::grid::anchored(m))
            .map(|(_, m)| m.range.unwrap_or(crate::sim::fight::grid::AURA))
            .max()
    }

    /// Where `me` would rather stand than the nearest square it can act
    /// from, and the way there - or `None`, for a creature with no reason to
    /// stand anywhere in particular, which walks up to its target as ever.
    ///
    /// Two reasons. Holding an emanation up, it stands where the most
    /// enemies are inside it. And aiming from range with nobody yet upon it, it stands
    /// as far from its nearest enemy as it can and still reach its target -
    /// though not out of something's reach once that has closed with it,
    /// which would be turning its back on it.
    fn footwork(
        &mut self,
        me: usize,
        target: usize,
        plan: &Plan,
        slips: bool,
    ) -> Option<Vec<(i32, i32)>> {
        let side = self.fighters[me].side;
        let enemies: Vec<usize> = (0..self.fighters.len())
            .filter(|&i| self.fighters[i].side != side && self.fighters[i].alive())
            .filter(|&i| self.reaches(me, i))
            .collect();
        // What it means to do has to still reach whoever it is meant for.
        let wanted = self.wanted_range(me, plan);
        let acts = plan.action.is_some() || plan.bonus.is_some();
        let in_range = move |fight: &Self, cell: (i32, i32)| match wanted {
            Some(feet) if acts => fight.feet_from(me, cell, target) <= feet,
            _ => true,
        };
        if let Some(reach) = self.aura_reach(me) {
            let caught = |fight: &Self, cell: (i32, i32)| {
                enemies
                    .iter()
                    .filter(|&&e| fight.feet_from(me, cell, e) <= reach)
                    .count() as i64
            };
            let way = self.roam(me, |fight, cell| {
                in_range(fight, cell).then(|| caught(fight, cell))
            });
            // With nobody it can get inside it this turn, it closes on its
            // target until somebody is.
            let here = self.fighters[me].pos;
            return match way {
                Some(path) if path.is_empty() && caught(self, here) == 0 => {
                    Some(self.advance(me, target, reach))
                }
                other => other,
            };
        }
        // Something that would rather be in the press walks into it.
        if self.fighters[me].creature.riders.contains(&Rider::ClosesIn) && wanted.is_some() {
            return Some(self.advance(me, target, crate::sim::arena::SQUARE));
        }
        // Something that fights hand to hand closes even on a turn it
        // spends on something else - putting a form on, marking its prey:
        // what it is doing this turn reaches from anywhere, and next turn's
        // blow will not.
        if self.fighters[me].melee && wanted.is_some_and(|feet| feet >= RANGED) {
            let creature = self.fighters[me].creature;
            let reach = creature
                .actions
                .iter()
                .filter(|m| first_strike(&m.effect).is_some_and(|s| !s.kind.ranged))
                .map(|m| m.range.unwrap_or(crate::sim::arena::SQUARE))
                .min()
                .unwrap_or(crate::sim::arena::SQUARE);
            return Some(self.advance(me, target, reach));
        }
        // Something that would strike it for leaving: it does not turn its
        // back on that to gain a few yards.
        let beset = !slips && self.pinned(me);
        match wanted {
            Some(feet) if feet >= RANGED && plan.action.is_some() && !beset => {
                // With as many of them still in range as it can keep there -
                // whoever it turns to next, or whatever else its area takes
                // in - and, among squares that keep as many, the one furthest
                // from the nearest. No further than it can shoot: past that,
                // distance buys nothing.
                let far = i64::from(feet / crate::sim::arena::SQUARE);
                self.roam(me, |fight, cell| {
                    in_range(fight, cell).then(|| {
                        let gaps = enemies.iter().map(|&e| {
                            i64::from(fight.feet_from(me, cell, e) / crate::sim::arena::SQUARE)
                        });
                        let kept = gaps.clone().filter(|&gap| gap <= far).count() as i64;
                        // Out of whatever its enemies are holding up, first;
                        // then far from the nearest of them; then not in a
                        // knot with its own side, for whatever is thrown at
                        // one of them.
                        kept * 100_000 - fight.hazards_at(me, cell) * 10_000
                            + gaps.min().unwrap_or(far).min(far) * 10
                            - fight.crowd_at(me, cell)
                    })
                })
            }
            _ => None,
        }
    }

    /// Every emanation `me` is standing in, and everybody standing in one of
    /// its own: who holds it, which of its raised auras it is, and who is in
    /// it. Only the ones that were raised - a spell's emanation - which are
    /// the ones that catch what they pass over.
    fn washed_by(&self, me: usize) -> Vec<(usize, usize, usize)> {
        let mut out = Vec::new();
        if !self.on_grid() {
            return out;
        }
        for owner in 0..self.fighters.len() {
            let holder = &self.fighters[owner];
            if !holder.alive() {
                continue;
            }
            for (which, aura) in holder.creature.lasting_auras.iter().enumerate() {
                // One that was put down catches what starts a turn in it,
                // not what walks in.
                if !self.has_aura(owner, which) || crate::sim::fight::grid::anchored(aura) {
                    continue;
                }
                for victim in 0..self.fighters.len() {
                    let involved = owner == me || victim == me;
                    if involved
                        && self.fighters[victim].side != holder.side
                        && self.fighters[victim].alive()
                        && self.reaches(owner, victim)
                        && self.aura_reaches(owner, victim, aura)
                    {
                        out.push((owner, which, victim));
                    }
                }
            }
        }
        out
    }

    /// `me` has moved: whoever is inside an emanation now that was not
    /// `before` - carried over them by whoever holds it, or walked into -
    /// meets it at once, as it would at the start of its own turn.
    fn wash(
        &mut self,
        round: u32,
        me: usize,
        before: &[(usize, usize, usize)],
        rng: &mut Rng,
        log: &mut Option<Vec<String>>,
    ) {
        for (owner, which, victim) in self.washed_by(me) {
            if before.contains(&(owner, which, victim))
                || !self.fighters[victim].alive()
                || !self.fighters[owner].alive()
            {
                continue;
            }
            let creature = self.fighters[owner].creature;
            let m = &creature.lasting_auras[which];
            let mut line = String::new();
            let previous = self.sole_target.replace(victim);
            let slot = std::mem::replace(&mut self.cue.slot, "aura");
            self.apply(m, rng, owner, victim, log.is_some(), &mut line);
            self.cue.slot = slot;
            self.sole_target = previous;
            if let Some(l) = log.as_mut() {
                l.push(format!(
                    "r{round} {} (aura, on the move): {line}  [{} {} hp]",
                    creature.name,
                    self.fighters[victim].creature.name,
                    self.fighters[victim].hp.max(0)
                ));
            }
        }
    }

    /// Every aura on the other side washes over `who` as its turn starts,
    /// each resolved against `who` alone - and whatever it lands can set off
    /// its owner's reaction.
    fn auras_on(&mut self, round: u32, who: usize, rng: &mut Rng, log: &mut Option<Vec<String>>) {
        let side = self.fighters[who].side;
        for owner in 0..self.fighters.len() {
            let creature = self.fighters[owner].creature;
            if (creature.auras.is_empty() && creature.lasting_auras.is_empty())
                || self.fighters[owner].side == side
                || !self.fighters[owner].alive()
            {
                continue;
            }
            // Always-on ones, then whichever raised ones are still up. Both
            // resolve identically from here: the only thing being asked is
            // whether this one is switched on.
            let live: Vec<&Move> = creature
                .auras
                .iter()
                .chain(
                    creature
                        .lasting_auras
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| self.has_aura(owner, *i))
                        .map(|(_, m)| m),
                )
                .collect();
            for m in live {
                if !self.fighters[who].alive() || !self.reaches(owner, who) {
                    break;
                }
                if !self.aura_reaches(owner, who, m) {
                    continue;
                }
                let mut line = String::new();
                let previous = self.sole_target.replace(who);
                let slot = std::mem::replace(&mut self.cue.slot, "aura");
                self.pull_to = self.anchor_of(owner, m).map(|middle| (owner, middle));
                self.apply(m, rng, owner, who, log.is_some(), &mut line);
                self.pull_to = None;
                self.cue.slot = slot;
                self.sole_target = previous;
                if let Some(l) = log.as_mut() {
                    l.push(format!(
                        "r{round} {} (aura): {line}  [{} {} hp]",
                        creature.name,
                        self.fighters[who].creature.name,
                        self.fighters[who].hp.max(0)
                    ));
                }
            }
        }
    }

    /// Everything that happens as `who`'s turn starts, whether or not it is
    /// able to act: conditions lasting until then end - the stun a monk
    /// landed ends on the monk's turn, not the dragon's - one more round
    /// comes off anything timed in `who`'s rounds, and every creature's
    /// once-per-turn Sneak Attack budget comes back.
    pub(super) fn start_of_turn(&mut self, who: usize) {
        let was_prone = self.fighters[who].has(|c| c == Condition::Prone);
        for f in self.fighters.iter_mut() {
            f.sneak_attack_spent = false;
            f.redirect_spent = false;
            f.turned_from = None;
            f.inside_damage = 0;
            // A creature with reactions to spare gets one for this turn -
            // which for everything with the usual single reaction means the
            // one it has not spent yet, unchanged.
            f.offer_reaction();
            f.conditions.retain_mut(|(_, expiry)| match expiry {
                Expiry::TurnStart(x) => *x != who,
                Expiry::Rounds { who: w, left } | Expiry::RoundsOrDamaged { who: w, left }
                    if *w == who =>
                {
                    *left = left.saturating_sub(1);
                    *left > 0
                }
                _ => true,
            });
        }
        // Getting up is what ended it, and getting up costs a move.
        let f = &mut self.fighters[who];
        f.stood_up = was_prone && !f.has(|c| c == Condition::Prone);
    }

    /// Conditions lasting until the end of `who`'s turn, counted down.
    pub(super) fn end_of_turn(&mut self, who: usize) {
        for f in self.fighters.iter_mut() {
            f.conditions.retain_mut(|(_, expiry)| match expiry {
                Expiry::TurnEnd { who: w, ends_left } if *w == who => {
                    *ends_left = ends_left.saturating_sub(1);
                    *ends_left > 0
                }
                _ => true,
            });
        }
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
        let creature = self.fighters[me].creature;
        // Recharge is rolled at the start of the creature's turn; reactions and
        // legendary actions come back then too.
        refresh(&mut self.fighters[me], rng);

        if self.fighters[me].loses_turn() {
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
            self.frame(
                round,
                FrameKind::Lost,
                Some(me),
                None,
                format!("{} loses its turn", creature.name),
                Vec::new(),
            );
            return false;
        }

        let record = log.is_some();
        let mut line = String::new();

        // A creature with a mouth swims first, by its tactic.
        if let Some((prey, from)) = self.close_in(me) {
            if record {
                line.push_str(&format!(
                    "closes on {} from {}",
                    self.fighters[prey].creature.name,
                    from.name()
                ));
            }
        }

        // On a grid it has not moved yet, and what it can reach is judged
        // as if it already had.
        if self.on_grid() {
            self.stride = Some((me, self.squares(me)));
        }
        let Some(mut target) = self.aim(me) else {
            self.stride = None;
            return true; // nothing left to hit
        };

        let mut plan = match plan {
            Some(p) => p,
            None => self.decide(round, me, target, rng),
        };
        // Whoever it means to heal, judged while what it can reach is still
        // judged as if it had walked: where its walk is headed, if that is
        // not somebody it can already touch.
        let mending = [
            plan.action.and_then(|i| creature.actions.get(i)),
            plan.bonus.and_then(|i| creature.bonus_actions.get(i)),
        ]
        .into_iter()
        .flatten()
        .find(|m| matches!(m.effect, Effect::Heal(_)))
        .map(|m| {
            let patient = self.with_cast(m, target, || self.heal_target(me));
            (patient, m.range.unwrap_or(crate::sim::arena::SQUARE))
        });
        self.stride = None;
        let errand = mending.filter(|&(patient, feet)| {
            patient != me && self.on_grid() && !self.within(me, patient, feet)
        });
        // Taking aim means standing still: it aims if what it means to do
        // reaches from where it stands, and walks without aiming if not.
        let aims = plan
            .bonus
            .and_then(|i| creature.bonus_actions.get(i))
            .is_some_and(|m| {
                m.before_action && m.effect.stance().is_some_and(Condition::zeroes_speed)
            });
        let rooted = aims
            && self.on_grid()
            && match plan.action.and_then(|i| creature.actions.get(i)) {
                Some(m) if errand.is_none() && self.move_lands(me, target, m) => true,
                _ => {
                    plan.bonus = None;
                    false
                }
            };
        // An action that is a bonus action's worth as well leaves none.
        if plan
            .action
            .and_then(|i| creature.actions.get(i))
            .is_some_and(|m| m.carries(&Rider::TakesBonusAction))
        {
            plan.bonus = None;
        }

        // Move, then act: as far as the chosen action needs, and twice over
        // - a Dash - when nothing it has reaches from here.
        let walks = self.on_grid()
            && !rooted
            && !self.has_mouth(me)
            && !self.has_mouth(target)
            && self.fighters[me].swallowed_by().is_none();
        // Whose emanations it is standing in, or holding over whom, before
        // anybody moves - so that what the move changes can be told.
        let washed = self.washed_by(me);
        // Somewhere it would rather be than the nearest square it can act
        // from: clear of its enemies, or with as many of them as it can get
        // inside what it is holding up.
        // Held where it is by something that would strike it for leaving,
        // it fights from there when its playstyle would rather not take the
        // blow - turning on whoever it can reach, if not the one it meant.
        // Slipping away rather than walking away - when that is what its
        // bonus action is for, what its action lets it do, or simply how
        // it moves: nothing gets a swing at it.
        let chosen = plan.action.and_then(|i| creature.actions.get(i));
        // An action that carries its user its speed again.
        let surges = chosen.is_some_and(|m| m.carries(&Rider::NeverProvokes));
        let disengages = |m: Option<&Move>| {
            m.is_some_and(|m| m.name.to_ascii_lowercase().starts_with("disengage"))
        };
        let slips = surges
            || creature.never_provokes()
            || disengages(chosen)
            || disengages(plan.bonus.and_then(|i| creature.bonus_actions.get(i)));
        let mut walks = walks;
        if walks && !slips && self.pinned(me) && self.holds_its_ground(me) {
            let lands = |fight: &Self, at: usize| match plan.action {
                Some(i) => creature
                    .actions
                    .get(i)
                    .is_some_and(|m| fight.move_lands(me, at, m)),
                None => false,
            };
            if lands(self, target) {
                walks = false;
            } else if let Some(other) = self.pick_target_in(me, |f, i| lands(f, i)) {
                target = other;
                walks = false;
            } else if self.fighters[me].policy == Policy::Defensive && self.fighters[me].bloodied()
            {
                walks = false;
            }
        }
        let start = self.fighters[me].pos;
        let mut struck: Vec<String> = Vec::new();
        let mut ground = match (walks, errand) {
            // To whoever it means to heal, before anything else.
            (true, Some((patient, feet))) => Some(self.advance(me, patient, feet)),
            (true, None) => self.footwork(me, target, &plan, slips),
            (false, _) => None,
        };
        // Having Disengaged, it gets as far from all of them as it can.
        if walks && errand.is_none() && disengages(chosen) {
            let side = self.fighters[me].side;
            ground = self.roam(me, |fight, cell| {
                (0..fight.fighters.len())
                    .filter(|&i| fight.fighters[i].side != side && fight.fighters[i].alive())
                    .map(|i| i64::from(fight.feet_from(me, cell, i)))
                    .min()
            });
        }
        // Closing to strike, something that walks through bodies goes
        // through as many of them as it can on the way.
        if walks && ground.is_none() && chosen.is_some() {
            if let Some(feet) = self.wanted_range(me, &plan).filter(|&feet| feet < RANGED) {
                ground = self.phase_walk(me, target, feet);
            }
        }
        // The way it went, whichever way that was.
        let mut trail: Vec<(i32, i32)> = Vec::new();
        if let Some(path) = ground.as_mut() {
            if !slips {
                self.gauntlet(rng, me, start, path, record, &mut struck);
            }
        }
        if let Some(path) = &ground {
            trail.clone_from(path);
            if !path.is_empty() {
                let text = format!(
                    "moves {} ft",
                    path.len() * crate::sim::arena::SQUARE as usize
                );
                if record {
                    line.push_str(&text);
                }
                self.frame(
                    round,
                    FrameKind::Move,
                    Some(me),
                    Some(target),
                    format!("{} {text}", creature.name),
                    path.clone(),
                );
            }
        }
        if let Some(feet) = self
            .wanted_range(me, &plan)
            .filter(|_| walks && ground.is_none())
        {
            let mut path = self.advance(me, target, feet);
            let walked = path.len();
            if plan.action.is_none() || surges {
                path.extend(self.advance(me, target, feet));
            }
            // And a third time, with a bonus action to Dash with as well.
            let sprints = plan
                .bonus
                .and_then(|i| creature.bonus_actions.get(i))
                .is_some_and(|m| m.name.to_ascii_lowercase().starts_with("dash"));
            if plan.action.is_none() && sprints {
                path.extend(self.advance(me, target, feet));
            }
            if !slips {
                self.gauntlet(rng, me, start, &mut path, record, &mut struck);
            }
            trail.clone_from(&path);
            if !path.is_empty() {
                let text = format!(
                    "{} {} ft toward {}",
                    if path.len() > walked {
                        "dashes"
                    } else {
                        "moves"
                    },
                    path.len() * crate::sim::arena::SQUARE as usize,
                    self.fighters[target].creature.name
                );
                if record {
                    line.push_str(&text);
                }
                self.frame(
                    round,
                    FrameKind::Move,
                    Some(me),
                    Some(target),
                    format!("{} {text}", creature.name),
                    path,
                );
            }
        }

        if let Some(roll) = creature.phases_through() {
            self.pass_through(rng, me, &trail, roll, record, &mut struck);
        }
        if record && !struck.is_empty() {
            line.push_str(&format!(" ({})", struck.join(", ")));
        }
        if !self.fighters[me].alive() {
            if let Some(l) = log.as_mut() {
                l.push(format!("r{round} {}: {line}", creature.name));
            }
            return true;
        }

        // Whatever emanation that walk carried over somebody, or carried
        // it into, washes over them now.
        if walks {
            self.wash(round, me, &washed, rng, log);
            if !self.fighters[me].alive() {
                return true;
            }
        }

        // Around a creature with a mouth, its enemies move before acting.
        if let Some(zone) = plan.zone.filter(|_| self.zone_choice_applies(me, target)) {
            if let Some((_, to)) = self.step_to(me, target, zone) {
                if record {
                    if !line.is_empty() {
                        line.push_str(" | ");
                    }
                    line.push_str(&format!("moves to {}", to.name()));
                }
            }
        }

        // Withdrawing takes the bonus action: getting clear without being hit
        // on the way out is what Primordial-Surge-style movement buys.
        let withdrawing = self.has_mouth(me) && creature.tactic == Tactic::HitAndRun;
        if withdrawing {
            plan.bonus = None;
        }

        // Almost every bonus action follows the action; one that sets the
        // action up - Steady Aim - has to come first.
        let bonus_first = plan
            .bonus
            .and_then(|i| creature.bonus_actions.get(i))
            .is_some_and(|m| m.before_action);
        let order = if bonus_first {
            [(Slot::Bonus, plan.bonus), (Slot::Action, plan.action)]
        } else {
            [(Slot::Action, plan.action), (Slot::Bonus, plan.bonus)]
        };

        for (slot, pick) in order {
            let Some(pick) = pick else { continue };
            if !self.fighters[target].alive() || !self.reaches(me, target) {
                let Some(new_target) = self.aim(me) else {
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
                || !self.fighters[me].affords(chosen)
                || !self.fighters[me].can_cast(chosen.spell_slot_level)
                || !self.can_take(me, chosen)
            {
                continue;
            }
            // The walk fell short of this target - the way was blocked - so
            // it goes to whoever it does reach, or to nobody.
            if self.on_grid() && !self.move_lands(me, target, chosen) {
                match self.pick_target_in(me, |f, i| f.move_lands(me, i, chosen)) {
                    Some(other) => target = other,
                    None => continue,
                }
            }
            // Stopped as it is cast: the action is gone, and what the spell
            // would have cost is not.
            if self.countered(rng, me, target, chosen, record, &mut line) {
                continue;
            }
            self.fighters[me].charge(chosen);
            self.fighters[me].cast_spell_slot(chosen.spell_slot_level);
            self.fighters[me].spend_move(slot, pick, chosen.uses);
            if let Some(spend) = chosen.spends {
                self.spend(me, spend);
            }
            self.cue.slot = match slot {
                Slot::Bonus => "bonus action",
                _ => "action",
            };
            self.apply(chosen, rng, me, target, record, &mut line);
            self.cue.slot = "action";
        }

        if withdrawing && self.withdraw(me) && record {
            if !line.is_empty() {
                line.push_str(" | ");
            }
            line.push_str("withdraws");
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
                _ => None,
            })
            .collect();

        for (condition, ability, dc) in pending {
            let (saved, resisted) = self.saving_throw(rng, me, ability, dc, false);
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
            self.frame(
                round,
                FrameKind::Recover,
                Some(me),
                None,
                format!(
                    "{} shakes off {}",
                    self.fighters[me].creature.name,
                    condition.name()
                ),
                Vec::new(),
            );
        }
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
        let Some(target) = self.aim(me) else {
            return;
        };

        let record = log.is_some();
        let mut line = String::new();
        let pick = {
            let f = &self.fighters[me];
            // Legendary actions are chosen by policy even for the search, which
            // only plans whole turns. Searching them too would multiply the
            // rollout count by the number of windows. One costing more than
            // is left this round is not a choice.
            f.policy.choose(&creature.legendary, &f.legendary, f, |m| {
                if m.legendary_cost > f.legendary_left {
                    f64::NEG_INFINITY
                } else {
                    self.move_value(me, target, m, Boost::default())
                }
            })
        };
        let Some(pick) = pick else { return };
        let chosen = &creature.legendary[pick];

        self.fighters[me].legendary_left -= chosen.legendary_cost;
        self.fighters[me].charge(chosen);
        self.fighters[me].cast_spell_slot(chosen.spell_slot_level);
        self.fighters[me].spend_move(Slot::Legendary, pick, chosen.uses);
        if let Some(spend) = chosen.spends {
            self.spend(me, spend);
        }
        // Any legendary action opens a sealed mouth, attack or not.
        self.lapse_on_attack(me);
        self.cue.slot = "legendary action";
        self.apply(chosen, rng, me, target, record, &mut line);
        self.cue.slot = "action";

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::{Cost, Creature, Effect, Move, Resource, Rider, Strike, Uses};
    use crate::rules::{DamageKind, DamageRoll, Duration, RollMode};
    use crate::sim::fight::test_support::{bow, fight_of, no_log, puncher, sneak_attacker};
    use crate::sim::{run, run_teams, Budget, Policy, Side};

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
            conditions: vec![Condition::Stunned],
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
            d.riders.push(Rider::AlwaysSucceed {
                uses: 3,
                ability: None,
                reaction: false,
            });
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
                conditions: vec![Condition::Paralyzed],
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

    /// Cunning Strike's Poison option (ROG-03) needs no engine changes of its
    /// own: it is `Rider::SaveOrCondition` plus `Condition::Poisoned` and
    /// `Duration::SaveEndTurn`, three mechanisms that already exist
    /// independently of each other and of Cunning Strike. This gives
    /// Poisoned the same check the test above already gives Paralyzed - the
    /// repeat-save loop generalizes rather than needing a re-test written
    /// specifically for it - measured through what Poisoned actually does
    /// (disadvantage on its own attack rolls) rather than `turns_lost`,
    /// since unlike Paralyzed, Poisoned never takes the turn away.
    #[test]
    fn poison_from_cunning_strike_is_cleared_by_the_existing_repeat_save_loop() {
        let cost = Cost {
            resource: 0,
            amount: 1,
        };
        let poisoner = |ability: Ability, dc: i32| {
            let mut c = puncher("poisoner", 10, 200, 20, 0);
            c.initiative = 100;
            // One shot only, so this is about how long a single application
            // of Poisoned lasts, not how often it lands.
            c.actions[0].uses = Uses::Limited(1);
            c.resources.push(Resource {
                name: "focus".into(),
                max: 1,
            });
            c.actions[0].riders.push(Rider::SaveOrCondition {
                ability: Ability::Con,
                dc: 99, // the one hit always lands Poisoned
                conditions: vec![Condition::Poisoned],
                duration: Duration::SaveEndTurn { ability, dc },
                cost: Some(cost),
                once_per_turn: true,
            });
            c
        };
        // Poisoned's whole effect is disadvantage on its own attack rolls,
        // so give the victim a strike of its own and measure what it deals
        // instead of turns lost.
        let mut victim = puncher("victim", 10, 200, 0, 0);
        victim.initiative = -100;

        let dealt = |monster: &Creature| {
            let mut rng = Rng::new(41);
            let mut total = 0i64;
            for _ in 0..300 {
                let mut log = no_log();
                let o = run(
                    &mut rng,
                    [monster, &victim],
                    [Policy::Greedy; 2],
                    6,
                    &mut log,
                );
                total += o.damage_dealt[1];
            }
            total
        };

        // Con save DC 99 against Poisoned's repeat: never clears.
        let unbeatable = dealt(&poisoner(Ability::Con, 99));
        // Con save DC 1: clears at the end of the very turn it landed.
        let easy = dealt(&poisoner(Ability::Con, 1));

        assert!(
            easy > unbeatable,
            "a near-certain repeat save should clear Poisoned quickly, costing the victim \
             less accuracy over the fight than a save it can never make: easy {easy} vs \
             unbeatable {unbeatable}"
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

    /// Steady Aim goes before the attack it sets up, grants that attack
    /// advantage (and with it Sneak Attack), and is gone after one roll. A
    /// ranking policy takes it when advantage is worth more than any other
    /// bonus action - here, the only other one does nothing.
    #[test]
    fn steady_aim_is_chosen_resolved_first_and_used_up_by_the_attack() {
        let mut rogue = sneak_attacker("rogue").with_action(bow(5, RollMode::Normal));
        rogue.bonus_actions.push(
            Move::new(
                "Steady Aim",
                Effect::Stance {
                    condition: Condition::SteadyAim,
                },
            )
            .with_before_action(),
        );
        rogue
            .bonus_actions
            .push(Move::new("Dash", Effect::Sequence(Vec::new())));
        let target = Creature::new("target", 12, 1_000);
        let roster = [(&rogue, Side::A), (&target, Side::B)];
        let (mut fight, mut rng) = fight_of(&roster, 11);

        let plan = fight.decide(1, 0, 1, &mut rng);
        assert_eq!(
            plan,
            Plan {
                action: Some(0),
                bonus: Some(0),
                zone: None,
            }
        );

        let mut log = Some(Vec::new());
        fight.acting = Some(0);
        fight.turn(1, 0, &mut rng, &mut log, Some(plan));
        let line = log.unwrap().join("\n");
        assert!(
            line.find("steady_aim").unwrap() < line.find("Bow").unwrap(),
            "Steady Aim resolves before the attack: {line}"
        );
        assert!(
            !fight.fighters[0].has(|c| c == Condition::SteadyAim),
            "used up by the attack roll"
        );
    }

    /// A legendary action costing two takes two of the round's three, and
    /// one costing more than is left is passed over for one that fits.
    #[test]
    fn a_legendary_action_takes_what_it_costs() {
        let zap = |name: &str, damage: i32, cost: u32| {
            Move::new(
                name,
                Effect::AutoHit {
                    damage: vec![DamageRoll::new(0, 1, damage, DamageKind::Force)],
                },
            )
            .with_legendary_cost(cost)
        };
        let mut boss = Creature::new("boss", 10, 1_000);
        boss.legendary_uses = 3;
        boss.legendary.push(zap("Big", 10, 2));
        boss.legendary.push(zap("Small", 1, 1));
        let hero = Creature::new("hero", 10, 1_000);
        let (mut fight, mut rng) = fight_of(&[(&boss, Side::B), (&hero, Side::A)], 1);

        for _ in 0..3 {
            fight.legendary(1, 0, &mut rng, &mut no_log());
        }
        assert_eq!(fight.fighters[0].legendary_left, 0);
        assert_eq!(fight.fighters[1].hp, 1_000 - 10 - 1, "Big, then Small");
    }
}
