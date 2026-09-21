//! Formats and prints the minimax party size sweep table.

use crate::args::{Args, MONSTER_STYLES, PARTY_STYLES};
use crate::report::sheet::strip_number;
use crucible_core::creature::Creature;
use crucible_core::sim::{evaluate_teams, Policy, Summary};
/// Rebuild the roster with `copies` of each creature on the first side, or leave
/// it alone when `copies` is zero.
pub fn resize(parsed: &[Creature], copies: usize) -> Vec<Creature> {
    if copies == 0 {
        return parsed.to_vec();
    }
    let mut out = Vec::new();
    for c in parsed.iter().filter(|c| c.team == 0) {
        let stem = strip_number(&c.name).to_string();
        for i in 0..copies {
            let mut copy = c.clone();
            copy.name = if copies > 1 {
                format!("{stem} {}", i + 1)
            } else {
                stem.clone()
            };
            out.push(copy);
        }
    }
    out.extend(parsed.iter().filter(|c| c.team == 1).cloned());
    out
}

/// "How many does it take" as a minimax, because the honest answer depends on who
/// is playing badly.
pub fn print_party_sweep(args: &Args, parsed: &[Creature], max: usize) {
    println!(
        "How many it takes. `both greedy` is the naive headline; `minimax` is what\n\
         the party can guarantee against the best-played monster. {} fights a cell,\n\
         {} cells a row.\n",
        args.sweep_samples,
        PARTY_STYLES.len() * MONSTER_STYLES.len() + 1
    );
    println!(
        "{:>6}  {:>11}  {:>8}  {:>11}  {:>13}  {:>7}  {:>7}  {:>6}",
        "size",
        "both greedy",
        "minimax",
        "best party",
        "worst monster",
        "deaths",
        "rounds",
        "spent"
    );

    for size in 1..=max {
        let creatures = resize(parsed, size);
        let roster: Vec<&Creature> = creatures.iter().collect();
        let mut seed = args.seed.wrapping_add(1_000 * size as u64);

        let greedy = evaluate_teams(
            seed,
            &roster,
            [Policy::Greedy, Policy::Greedy],
            args.sweep_samples,
            args.cap,
            args.budget,
        );

        // max over the party's options of the min over the monster's.
        let mut best: Option<(f64, Policy, Policy, Summary)> = None;
        for party in PARTY_STYLES {
            let mut worst: Option<(f64, Policy, Summary)> = None;
            for monster in MONSTER_STYLES {
                seed = seed.wrapping_add(1);
                let s = evaluate_teams(
                    seed,
                    &roster,
                    [party, monster],
                    args.sweep_samples,
                    args.cap,
                    args.budget,
                );
                if worst.as_ref().is_none_or(|(w, _, _)| s.wins[0] < *w) {
                    worst = Some((s.wins[0], monster, s));
                }
            }
            let (value, monster, s) = worst.expect("at least one monster style");
            if best.as_ref().is_none_or(|(b, _, _, _)| value > *b) {
                best = Some((value, party, monster, s));
            }
        }
        let (value, party, monster, s) = best.expect("at least one party style");

        println!(
            "{size:>6}  {:>11.3}  {:>8.3}  {:>11}  {:>13}  {:>7.2}  {:>7.1}  {:>6.1}",
            greedy.wins[0],
            value,
            party.name(),
            monster.name(),
            s.mean_deaths[0],
            s.mean_rounds,
            s.mean_spent[0],
        );
    }
    println!();
}
