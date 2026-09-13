//! Many rollouts, reduced to something a DM can read.
//!
//! The headline is not a mean. `DESIGN.md` argues that tables do not care what
//! happens on average, they care whether the fight can go badly, so the risk
//! figure here is **CVaR**: the mean of the worst tenth of outcomes. A matchup
//! the party wins 85% of the time but ends at 2 HP in the bad tail is a
//! different encounter from one it wins 85% of the time comfortably, and an
//! expectation cannot tell them apart.
//!
//! The closed-form layer earns its keep again here.
//! [`sustained_rounds_to_kill`] answers "how long would this take if nothing
//! ran out" exactly, by dynamic programming, which turns the simulated result
//! into something checkable by hand: if a side needs eleven rounds of output
//! to drop the other and the other needs two, no amount of sampling should
//! report a close fight.

use crate::prob::dice::Pmf;
use crate::prob::exact::expected_attacks_to_kill;
use crate::prob::rng::Rng;
use crate::rules::creature::{Creature, Move};
use crate::sim::duel::{run_teams, run_with, Budget, Outcome, Policy, Side};

/// The fraction of outcomes CVaR averages over.
const TAIL: f64 = 0.10;

#[derive(Debug, Clone)]
pub struct Summary {
    pub names: [String; 2],
    pub policies: [Policy; 2],
    pub samples: usize,
    /// Probability each side is the last one standing.
    pub wins: [f64; 2],
    /// Neither side down when the round cap arrived.
    pub unresolved: f64,
    pub mean_rounds: f64,
    pub median_rounds: u32,
    /// HP remaining, counting a downed creature as zero.
    pub mean_hp_left: [f64; 2],
    /// Mean HP left across the worst tenth of fights for that side.
    pub cvar_hp_left: [f64; 2],
    pub mean_damage_per_round: [f64; 2],
    /// Resource points and limited uses burnt. The column that makes a win
    /// probability mean something: 0.9 for nothing spent and 0.9 for everything
    /// are different answers.
    pub mean_spent: [f64; 2],
    /// Turns each side never got to take, having been incapacitated. A monk's
    /// entire plan against a dragon, so it is worth a column.
    pub mean_turns_lost: [f64; 2],
    /// Creatures dropped per side. For a party this matters more than the win
    /// probability: winning with two of four dead is not the same answer.
    pub mean_deaths: [f64; 2],
    pub mean_survivors: [f64; 2],
    /// Rounds each side would need to drop the other at sustained output,
    /// computed exactly. `None` when it cannot get there at all.
    pub sustained_rounds: [Option<f64>; 2],
}

/// Run `samples` fights and reduce them.
///
/// One generator threaded through every fight rather than one per fight, so a
/// run is reproducible from `seed` and consecutive fights cannot accidentally
/// share a sequence.
pub fn evaluate(
    seed: u64,
    sides: [&Creature; 2],
    policies: [Policy; 2],
    samples: usize,
    max_rounds: u32,
) -> Summary {
    evaluate_with(
        seed,
        sides,
        policies,
        samples,
        max_rounds,
        Budget::default(),
    )
}

/// [`evaluate`], with the search budget spelled out.
pub fn evaluate_with(
    seed: u64,
    sides: [&Creature; 2],
    policies: [Policy; 2],
    samples: usize,
    max_rounds: u32,
    budget: Budget,
) -> Summary {
    tally(
        seed,
        samples,
        [sides[0].name.clone(), sides[1].name.clone()],
        policies,
        [
            sustained_rounds_to_kill(sides[0], sides[1]),
            sustained_rounds_to_kill(sides[1], sides[0]),
        ],
        |rng| run_with(rng, sides, policies, max_rounds, budget, &mut None),
    )
}

/// Evaluate a fight between two sides of any size.
///
/// Each creature's `team` says which side it is on. The sustained-output figures
/// assume nobody dies, so for a party they are an optimistic bound on pace
/// rather than a prediction: a dead character stops contributing and this does
/// not know that.
pub fn evaluate_teams(
    seed: u64,
    roster: &[&Creature],
    policies: [Policy; 2],
    samples: usize,
    max_rounds: u32,
    budget: Budget,
) -> Summary {
    let names = [label(roster, 0), label(roster, 1)];
    let sustained = [team_sustained(roster, 0, 1), team_sustained(roster, 1, 0)];
    tally(seed, samples, names, policies, sustained, |rng| {
        run_teams(rng, roster, policies, max_rounds, budget, &mut None)
    })
}

/// "4x Gio", or just the name when there is one of it.
fn label(roster: &[&Creature], team: u8) -> String {
    let members: Vec<&Creature> = roster.iter().copied().filter(|c| c.team == team).collect();
    match members.split_first() {
        None => "nobody".to_string(),
        Some((first, rest)) if rest.iter().all(|c| c.hp == first.hp && c.ac == first.ac) => {
            let stem = first
                .name
                .rsplit_once(' ')
                .map_or(first.name.as_str(), |(head, tail)| {
                    if tail.chars().all(|c| c.is_ascii_digit()) {
                        head
                    } else {
                        first.name.as_str()
                    }
                });
            if rest.is_empty() {
                stem.to_string()
            } else {
                format!("{}x {stem}", members.len())
            }
        }
        Some(_) => format!("{} creatures", members.len()),
    }
}

/// Rounds for one whole side to drop the other at sustained output, summing the
/// side's per-round damage distributions. Assumes nobody dies, so it is a bound.
fn team_sustained(roster: &[&Creature], attackers: u8, defenders: u8) -> Option<f64> {
    let target = roster.iter().copied().find(|c| c.team == defenders)?;
    let hp: i32 = roster
        .iter()
        .filter(|c| c.team == defenders)
        .map(|c| c.hp)
        .sum();
    let mut pmf = Pmf::constant(0);
    for c in roster.iter().filter(|c| c.team == attackers) {
        pmf = pmf.convolve(&sustained_round_pmf(c, target));
    }
    expected_attacks_to_kill(hp, &pmf)
}

/// Run `samples` fights and reduce them.
///
/// One generator threaded through every fight rather than one per fight, so a run
/// is reproducible from `seed` and consecutive fights cannot accidentally share a
/// sequence.
fn tally(
    seed: u64,
    samples: usize,
    names: [String; 2],
    policies: [Policy; 2],
    sustained: [Option<f64>; 2],
    mut fight: impl FnMut(&mut Rng) -> Outcome,
) -> Summary {
    let mut rng = Rng::new(seed);
    let mut wins = [0usize; 2];
    let mut unresolved = 0usize;
    let mut total_rounds = 0u64;
    let mut total_damage = [0i64; 2];
    let mut total_spent = [0u64; 2];
    let mut total_turns_lost = [0u64; 2];
    let mut total_deaths = [0u64; 2];
    let mut total_survivors = [0u64; 2];
    let mut rounds = Vec::with_capacity(samples);
    let mut hp_left = [Vec::with_capacity(samples), Vec::with_capacity(samples)];

    for _ in 0..samples {
        let o = fight(&mut rng);
        match o.winner {
            Some(Side::A) => wins[0] += 1,
            Some(Side::B) => wins[1] += 1,
            None => unresolved += 1,
        }
        total_rounds += u64::from(o.rounds);
        rounds.push(o.rounds);
        for i in 0..2 {
            total_damage[i] += o.damage_dealt[i];
            total_spent[i] += u64::from(o.resources_spent[i]);
            total_turns_lost[i] += u64::from(o.turns_lost[i]);
            total_deaths[i] += u64::from(o.deaths[i]);
            total_survivors[i] += u64::from(o.survivors[i]);
            hp_left[i].push(o.hp_left[i].max(0));
        }
    }

    for side in hp_left.iter_mut() {
        side.sort_unstable();
    }
    rounds.sort_unstable();

    let n = samples.max(1) as f64;
    let rounds_total = total_rounds.max(1) as f64;
    Summary {
        names,
        policies,
        samples,
        wins: [wins[0] as f64 / n, wins[1] as f64 / n],
        unresolved: unresolved as f64 / n,
        mean_rounds: total_rounds as f64 / n,
        median_rounds: rounds.get(samples / 2).copied().unwrap_or(0),
        mean_hp_left: [mean(&hp_left[0]), mean(&hp_left[1])],
        cvar_hp_left: [cvar(&hp_left[0]), cvar(&hp_left[1])],
        mean_damage_per_round: [
            total_damage[0] as f64 / rounds_total,
            total_damage[1] as f64 / rounds_total,
        ],
        mean_spent: [total_spent[0] as f64 / n, total_spent[1] as f64 / n],
        mean_turns_lost: [
            total_turns_lost[0] as f64 / n,
            total_turns_lost[1] as f64 / n,
        ],
        mean_deaths: [total_deaths[0] as f64 / n, total_deaths[1] as f64 / n],
        mean_survivors: [total_survivors[0] as f64 / n, total_survivors[1] as f64 / n],
        sustained_rounds: sustained,
    }
}

fn mean(sorted: &[i32]) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted.iter().map(|&v| f64::from(v)).sum::<f64>() / sorted.len() as f64
}

/// Mean of the worst [`TAIL`] of a sorted ascending sample.
///
/// At least one element, so a handful of rollouts still produces a number
/// rather than a division by zero.
fn cvar(sorted: &[i32]) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let take = ((sorted.len() as f64 * TAIL).round() as usize).max(1);
    mean(&sorted[..take])
}

/// The best move a side can repeat forever, ignoring anything with a budget.
fn best_unlimited<'a>(moves: &'a [Move], target: &Creature) -> Option<&'a Move> {
    moves.iter().filter(|m| m.is_free()).max_by(|a, b| {
        a.effect
            .mean_damage(target)
            .total_cmp(&b.effect.mean_damage(target))
    })
}

/// Exact damage distribution for one round of sustained output: the best
/// unlimited action, bonus action, and every legendary action it gets.
///
/// Limited and recharge moves are excluded on purpose. This is the floor a
/// side settles into once the interesting resources are gone, and it is the
/// part that can be solved in closed form.
pub fn sustained_round_pmf(attacker: &Creature, target: &Creature) -> Pmf {
    let mut pmf = Pmf::constant(0);
    for moves in [&attacker.actions, &attacker.bonus_actions] {
        if let Some(m) = best_unlimited(moves, target) {
            pmf = pmf.convolve(&m.effect.damage_pmf(target));
        }
    }
    if let Some(m) = best_unlimited(&attacker.legendary, target) {
        let one = m.effect.damage_pmf(target);
        for _ in 0..attacker.legendary_uses {
            pmf = pmf.convolve(&one);
        }
    }
    pmf
}

/// Expected rounds for `attacker` to drop `target` at sustained output.
///
/// `None` when it cannot: a creature whose every attack is absorbed by
/// resistance never gets there, and reporting an infinity that quietly
/// propagates into an average would be worse than saying so.
pub fn sustained_rounds_to_kill(attacker: &Creature, target: &Creature) -> Option<f64> {
    expected_attacks_to_kill(target.hp, &sustained_round_pmf(attacker, target))
}

impl Summary {
    /// P(the first side is the one left standing).
    pub fn win_rate(&self) -> f64 {
        self.wins[0]
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        let rounds = |r: Option<f64>| match r {
            Some(v) => format!("{v:.1}"),
            None => "never".to_string(),
        };
        out.push_str(&format!(
            "{:>28}   {:>7}   {:>7}   {:>8}   {:>9}   {:>8}   {:>6}   {:>6}\n",
            "", "P(win)", "P(dies)", "mean hp", "worst 10%", "dmg/rnd", "spent", "denied"
        ));
        for i in 0..2 {
            out.push_str(&format!(
                "{:>28}   {:>7.3}   {:>7.3}   {:>8.1}   {:>9.1}   {:>8.1}   {:>6.1}   {:>6.2}\n",
                format!("{} [{}]", self.names[i], self.policies[i].name()),
                self.wins[i],
                self.wins[1 - i],
                self.mean_hp_left[i],
                self.cvar_hp_left[i],
                self.mean_damage_per_round[i],
                self.mean_spent[i],
                self.mean_turns_lost[1 - i],
            ));
        }
        out.push_str(&format!(
            "\n{} fights, {:.1} rounds on average (median {}), {:.1}% unresolved at the cap\n",
            self.samples,
            self.mean_rounds,
            self.median_rounds,
            self.unresolved * 100.0,
        ));
        out.push_str(&format!(
            "sustained output (exact): {} needs {} rounds to drop {}; {} needs {}\n",
            self.names[0],
            rounds(self.sustained_rounds[0]),
            self.names[1],
            self.names[1],
            rounds(self.sustained_rounds[1]),
        ));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::creature::{DamageKind, DamageRoll, Effect, Strike};

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

    #[test]
    fn win_probabilities_and_the_unresolved_share_partition_the_runs() {
        let a = puncher("a", 14, 40, 6, 3);
        let b = puncher("b", 12, 55, 4, 4);
        let s = evaluate(1, [&a, &b], [Policy::Greedy; 2], 2_000, 100);
        assert!((s.wins[0] + s.wins[1] + s.unresolved - 1.0).abs() < 1e-9);
        assert!(s.mean_rounds > 0.0);
    }

    /// Two identical creatures differ in exactly one thing, so this is the one
    /// matchup whose shape is known without simulating.
    ///
    /// Note that the initiative *roll* is almost useless as a probe here: two
    /// creatures with the same modifier tie only 5% of the time, so the edge a
    /// fair roll confers is about a third of a point and vanishes into the
    /// noise. Forcing the order is what makes the effect measurable.
    #[test]
    fn between_identical_creatures_only_the_turn_order_matters() {
        let quick = {
            let mut c = puncher("quick", 13, 45, 6, 3);
            c.initiative = 100;
            c
        };
        let slow = {
            let mut c = puncher("slow", 13, 45, 6, 3);
            c.initiative = -100;
            c
        };

        let s = evaluate(7, [&quick, &slow], [Policy::Greedy; 2], 20_000, 100);
        assert!(
            s.wins[0] > s.wins[1] + 0.05,
            "a free turn has to be worth something: {:.4} vs {:.4}",
            s.wins[0],
            s.wins[1]
        );

        // Nothing about the engine may depend on which slot a creature is in,
        // so swapping them has to mirror the result exactly, not merely
        // closely: the same seed drives the same rolls in the same order.
        let swapped = evaluate(7, [&slow, &quick], [Policy::Greedy; 2], 20_000, 100);
        assert_eq!(
            (swapped.wins[1], swapped.wins[0]),
            (s.wins[0], s.wins[1]),
            "side A and side B are not interchangeable"
        );
    }

    #[test]
    fn the_worst_tenth_is_never_kinder_than_the_average() {
        let a = puncher("a", 15, 60, 7, 4);
        let b = puncher("b", 13, 40, 5, 3);
        let s = evaluate(3, [&a, &b], [Policy::Greedy; 2], 2_000, 100);
        for i in 0..2 {
            assert!(s.cvar_hp_left[i] <= s.mean_hp_left[i] + 1e-9);
        }
    }

    /// Sustained output is a closed form, so it can be checked against a hand
    /// calculation: 1d6+3 against AC 10 at +6 hits on anything but a 1.
    #[test]
    fn sustained_output_matches_the_simulated_pace() {
        let a = puncher("a", 10, 500, 6, 3);
        let b = puncher("b", 10, 60, 6, 3);
        let exact = sustained_rounds_to_kill(&a, &b).expect("this kills");
        let s = evaluate(5, [&a, &b], [Policy::Greedy; 2], 20_000, 200);
        // `a` acts first about half the time, so the fight ends a little sooner
        // than the number of rounds `a` alone would need.
        assert!(
            (s.mean_rounds - exact).abs() < 1.5,
            "simulated {:.2} rounds against an exact {exact:.2}",
            s.mean_rounds
        );
    }
}
