//! Choosing what to do: a target, then a plan for the action and bonus
//! action, under the side's [`Policy`].

use crate::creature::{Move, Uses};
use crate::prob::Rng;
use crate::rules::Condition;
use crate::sim::fight::{Fight, Fighter, MoveState, Slot};
use crate::sim::{Plan, Policy};

impl<'a> Fight<'a> {
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
    ///
    /// Only an enemy it can reach counts: nothing outside a swallowed
    /// creature, and nothing but its swallower from inside one - see
    /// [`Fight::reaches`].
    pub(super) fn pick_target(&self, me: usize) -> Option<usize> {
        self.pick_target_in(me, |f, i| f.reaches(me, i))
    }

    /// [`Fight::pick_target`] among the living enemies `eligible` allows. A
    /// creature with a mouth goes after the nearest of them first - see
    /// [`crate::creature::Zone::distance`] - and applies its rule among
    /// those.
    pub(super) fn pick_target_in(
        &self,
        me: usize,
        eligible: impl Fn(&Self, usize) -> bool,
    ) -> Option<usize> {
        let side = self.fighters[me].side;
        let mut enemies: Vec<usize> = (0..self.fighters.len())
            .filter(|&i| self.fighters[i].side != side && self.fighters[i].alive())
            .filter(|&i| eligible(self, i))
            .collect();
        if self.has_mouth(me) {
            let nearest = enemies.iter().map(|&i| self.zone(i, me).distance()).min();
            enemies.retain(|&i| Some(self.zone(i, me).distance()) == nearest);
        }
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

    /// Who `me` plans its turn against: [`Fight::pick_target`] - or, when
    /// every enemy left is out of its reach because it has swallowed them
    /// all, the first of them anyway. Nothing it aims at them will land, but
    /// a squeeze of its gullet or a stance still needs a turn to be planned.
    pub(super) fn aim(&self, me: usize) -> Option<usize> {
        let side = self.fighters[me].side;
        self.pick_target(me).or_else(|| {
            (0..self.fighters.len())
                .find(|&i| self.fighters[i].side != side && self.fighters[i].alive())
        })
    }

    /// What to do this turn: ask the search if this creature searches, otherwise
    /// ask the policy for each slot.
    ///
    /// The action is chosen first. A bonus action that sets the action up -
    /// a stance granting advantage on this creature's own next attack, taken
    /// before it ([`Move::before_action`]) - is then worth exactly what that
    /// advantage adds to the chosen action, Sneak Attack it unlocks included,
    /// so it competes on the same footing as a bonus action that simply
    /// deals damage.
    ///
    /// Going after a creature with a mouth, where to stand comes before
    /// either (see [`Fight::zone_choice`]), and both are chosen as if already
    /// standing there.
    pub(super) fn decide(&mut self, round: u32, me: usize, target: usize, rng: &mut Rng) -> Plan {
        if let Some((who, plan)) = self.rollout_plan {
            if who == me {
                return self.repair(me, target, plan);
            }
        }
        if self.fighters[me].policy == Policy::Solver {
            return self.search(round, me, rng);
        }
        let zone = self.zone_choice(me, target);
        let here = zone.map(|_| self.zone(me, target));
        if let Some(z) = zone {
            self.set_zone(me, target, z);
        }
        let (action, bonus) = self.choose_moves(me, target);
        if let Some(back) = here {
            self.set_zone(me, target, back);
        }
        Plan {
            action,
            bonus,
            zone,
        }
    }

    /// The action, then the bonus action, the policy picks against `target`.
    fn choose_moves(&self, me: usize, target: usize) -> (Option<usize>, Option<usize>) {
        let f = &self.fighters[me];
        let action = f
            .policy
            .choose(Slot::Action.moves(f.creature), &f.actions, f, |m| {
                self.move_value(me, target, m, false)
            });
        let lead = action.map(|i| &f.creature.actions[i]);
        let bonus = f
            .policy
            .choose(Slot::Bonus.moves(f.creature), &f.bonus_actions, f, |m| {
                self.bonus_value(me, target, m, lead)
            });
        (action, bonus)
    }

    /// Keep a rollout on the plan under test, falling back slot by slot to greedy
    /// play when a pick is no longer legal - a pool that has run dry, a breath
    /// weapon that has not recharged.
    ///
    /// A deliberate `None` stays `None`, so "do nothing with the bonus action" is
    /// still evaluated as itself.
    fn repair(&self, me: usize, target: usize, plan: Plan) -> Plan {
        let f = &self.fighters[me];
        let fix = |slot: Slot, pick: Option<usize>| -> Option<usize> {
            let still_legal = pick.is_some_and(|i| {
                slot.moves(f.creature).get(i).is_some_and(|m| {
                    slot.states(f)[i].available()
                        && f.can_pay(m.cost)
                        && f.can_cast(m.spell_slot_level)
                        && f.move_allowed(m)
                })
            });
            if pick.is_none() || still_legal {
                pick
            } else {
                Policy::Greedy.choose(slot.moves(f.creature), slot.states(f), f, |m| {
                    self.move_value(me, target, m, false)
                })
            }
        };
        Plan {
            action: fix(Slot::Action, plan.action),
            bonus: fix(Slot::Bonus, plan.bonus),
            // Heads for the same place every turn, as far as it can get.
            zone: plan.zone,
        }
    }

    /// Every move that could legally be taken in this slot, plus doing nothing.
    ///
    /// Doing nothing goes last so the strict comparison in [`Fight::search`]
    /// breaks ties towards acting. A search that idles because idling scored
    /// equal-worst looks broken and is hard to tell from one that is.
    pub(super) fn legal(&self, me: usize, slot: Slot) -> Vec<Option<usize>> {
        let f = &self.fighters[me];
        let mut out = Vec::new();
        for (i, (m, state)) in slot
            .moves(f.creature)
            .iter()
            .zip(slot.states(f))
            .enumerate()
        {
            if state.available()
                && f.can_pay(m.cost)
                && f.can_cast(m.spell_slot_level)
                && f.move_allowed(m)
            {
                out.push(Some(i));
            }
        }
        out.push(None);
        out
    }

    /// Who on `me`'s side a heal goes to: a downed ally that can still be
    /// brought back first, otherwise whoever alive is missing the most hit
    /// points - `me` itself when nobody is hurt. Only an ally it can reach:
    /// not one swallowed, and none at all from inside a swallower.
    pub(super) fn heal_target(&self, me: usize) -> usize {
        let side = self.fighters[me].side;
        let allies = || {
            (0..self.fighters.len())
                .filter(move |&i| self.fighters[i].side == side && self.reaches(me, i))
        };
        if let Some(i) = allies().find(|&i| self.fighters[i].can_revive()) {
            return i;
        }
        let missing = |i: usize| self.fighters[i].creature.hp - self.fighters[i].hp;
        allies()
            .filter(|&i| self.fighters[i].alive())
            .fold(None, |best: Option<usize>, i| match best {
                Some(b) if missing(b) >= missing(i) => Some(b),
                _ => Some(i),
            })
            .unwrap_or(me)
    }
}

impl Policy {
    /// Index of the chosen move, or `None` if nothing is available. `value`
    /// is what a move is worth right now - [`Fight::move_value`] - which the
    /// ranking policies compare.
    ///
    /// Ties go to the earlier move, so the choice is a function of the scenario
    /// file and not of floating-point noise.
    pub(super) fn choose(
        self,
        moves: &[Move],
        states: &[MoveState],
        f: &Fighter<'_>,
        value: impl Fn(&Move) -> f64,
    ) -> Option<usize> {
        let bloodied = f.bloodied();
        let mut best: Option<(usize, (f64, f64))> = None;
        for (i, (m, state)) in moves.iter().zip(states).enumerate() {
            if !state.available()
                || !f.can_pay(m.cost)
                || !f.can_cast(m.spell_slot_level)
                || !f.move_allowed(m)
            {
                continue;
            }
            // A hoarding policy passes over anything with a price on it, not just
            // anything drawn from a pool.
            if !m.is_free() && !f.will_spend() {
                continue;
            }
            // A move that cannot do anything here - aimed at a creature type
            // the target is not, a second slot this turn - is not a choice.
            let v = value(m);
            if v == f64::NEG_INFINITY {
                continue;
            }
            if self == Policy::InOrder {
                return Some(i);
            }
            let rank = self.rank(m, v, bloodied);
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
    fn rank(self, m: &Move, value: f64, bloodied: bool) -> (f64, f64) {
        match self {
            Policy::Nova => (f64::from(spend_weight(m)), value),
            // Once things are going badly a defensive stance beats any amount
            // of damage, and before that it is worth nothing.
            Policy::Defensive
                if bloodied
                    && m.effect
                        .stance()
                        .is_some_and(Condition::disadvantage_to_attackers) =>
            {
                (f64::INFINITY, 0.0)
            }
            _ => (value, 0.0),
        }
    }
}

/// How much of a finite resource a move burns, for a policy that wants to burn
/// them. A pool cost counts by its size; any private budget counts as one; a
/// spell slot counts by its level, since a Nova table reaches for its highest
/// slot first.
fn spend_weight(m: &Move) -> u32 {
    m.cost.map_or(0, |c| c.amount)
        + u32::from(!matches!(m.uses, Uses::Unlimited))
        + m.spell_slot_level.unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::{Cost, Creature, Effect, Resource, SaveEffect, Strike};
    use crate::rules::{Ability, DamageKind, DamageRoll, Reduction};
    use crate::sim::fight::test_support::{no_log, puncher};
    use crate::sim::{run, run_teams, Budget, Side};

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
                on_failure: vec![],
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

    /// Defensive means a defensive stance: Steady Aim, a stance too, is not
    /// what a bloodied Defensive creature reaches for.
    #[test]
    fn a_defensive_policy_does_not_mistake_steady_aim_for_a_defence() {
        let steady = Move::new(
            "Steady Aim",
            Effect::Stance {
                condition: Condition::SteadyAim,
            },
        );
        let dodge = Move::new(
            "Dodge",
            Effect::Stance {
                condition: Condition::Dodging,
            },
        );
        assert_eq!(Policy::Defensive.rank(&steady, 3.0, true), (3.0, 0.0));
        assert_eq!(
            Policy::Defensive.rank(&dodge, 0.0, true),
            (f64::INFINITY, 0.0)
        );
    }
}
