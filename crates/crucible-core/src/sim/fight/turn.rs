//! A turn and the round around it: start- and end-of-turn bookkeeping, the
//! action and bonus action, repeated saves at the end of a turn, and
//! legendary actions between turns.

use crate::creature::{Move, Tactic};
use crate::prob::Rng;
use crate::rules::{Ability, Condition};
use crate::sim::fight::fighter::refresh;
use crate::sim::fight::saves::saving_throw;
use crate::sim::fight::value::Boost;
use crate::sim::fight::{Expiry, Fight, Slot};
use crate::sim::Plan;

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
            self.auras_on(round, who, rng, log);
            self.digest(round, who, rng, log);
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
                let mut line = String::new();
                let previous = self.sole_target.replace(who);
                self.apply(m, rng, owner, who, log.is_some(), &mut line);
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

        let Some(mut target) = self.aim(me) else {
            return true; // nothing left to hit
        };

        let mut plan = match plan {
            Some(p) => p,
            None => self.decide(round, me, target, rng),
        };

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
                || !self.fighters[me].can_pay(chosen.cost)
                || !self.fighters[me].can_cast(chosen.spell_slot_level)
                || !self.can_take(me, chosen)
            {
                continue;
            }
            self.fighters[me].pay(chosen.cost);
            self.fighters[me].cast_spell_slot(chosen.spell_slot_level);
            self.fighters[me].spend_move(slot, pick, chosen.uses);
            if let Some(spend) = chosen.spends {
                self.spend(me, spend);
            }
            self.apply(chosen, rng, me, target, record, &mut line);
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
            let (saved, resisted) = saving_throw(&mut self.fighters, rng, me, ability, dc, false);
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
        self.fighters[me].pay(chosen.cost);
        self.fighters[me].cast_spell_slot(chosen.spell_slot_level);
        self.fighters[me].spend_move(Slot::Legendary, pick, chosen.uses);
        if let Some(spend) = chosen.spends {
            self.spend(me, spend);
        }
        // Any legendary action opens a sealed mouth, attack or not.
        self.lapse_on_attack(me);
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
