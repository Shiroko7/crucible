//! The solver: a plan found by playing the fight forward from here, rather
//! than ranked by a heuristic.

use crate::creature::Zone;
use crate::prob::Rng;
use crate::sim::fight::{Fight, Slot};
use crate::sim::{Outcome, Plan, Policy, Side};

/// How much a spent resource counts against a win in the search's scoring.
///
/// The Lagrange multiplier from `DESIGN.md`, pinned very small rather than swept.
/// It has to stay well below the value of what a resource buys, or the search
/// hoards: one focus point spent on a Flurry moves the health margin by about
/// 0.015, so a penalty anywhere near that turns "spend it" into "do not".
/// Sweeping it to draw the Pareto frontier is a separate job, and this is the end
/// of the sweep where winning is all that matters.
const LAMBDA: f64 = 0.0005;

impl<'a> Fight<'a> {
    /// Flat Monte Carlo over this turn's legal plans.
    ///
    /// For each candidate, clone the fight, play that exact turn, then let
    /// everyone play on to the depth budget, and average the value of where it
    /// lands. One ply of real choice; see [`Policy::Solver`] for what that does
    /// and does not buy.
    pub(super) fn search(&self, round: u32, me: usize, rng: &mut Rng) -> Plan {
        let side = self.fighters[me].side;
        let max_hp = self.side_max_hp();
        let seat = self
            .order
            .iter()
            .position(|&i| i == me)
            .expect("every fighter has a place in the order");
        let resume = seat * 2 + 1;
        let depth_limit = self.max_rounds.min(round + self.budget.depth);

        // Around a creature with a mouth, where to stand is part of the plan:
        // whether the mouth is worth the risk is exactly what rollouts weigh.
        let zones: Vec<Option<Zone>> = match self.aim(me) {
            Some(t) if self.zone_choice_applies(me, t) => {
                self.zone_options(me, t).into_iter().map(Some).collect()
            }
            _ => vec![None],
        };

        let mut best = (Plan::default(), f64::NEG_INFINITY);
        for zone in zones {
            for action in self.legal(me, Slot::Action) {
                for bonus in self.legal(me, Slot::Bonus) {
                    let plan = Plan {
                        action,
                        bonus,
                        zone,
                    };
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
        }
        best.0
    }

    /// What each side's health margin is measured against: its combatants'
    /// hit point maximums. A summon is left out, the same way its hit points
    /// are left out of what is still standing - see [`Fight::unresolved`].
    fn side_max_hp(&self) -> [i32; 2] {
        let mut out = [0i32; 2];
        for f in self.fighters.iter().filter(|f| !f.is_summon()) {
            out[f.side.index()] += f.creature.hp;
        }
        out
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::{Creature, Effect, Move, Rider, Strike};
    use crate::rules::{Ability, Condition, DamageKind, DamageRoll, Duration};
    use crate::sim::fight::test_support::{no_log, puncher};
    use crate::sim::{run_teams, Budget};

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
                conditions: vec![Condition::Stunned],
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
}
