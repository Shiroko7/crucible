//! Replayable example fight narrations.

use crucible_core::creature::Creature;
use crucible_core::duel::{run_teams, Outcome, Policy, Side};
use crucible_core::Rng;

use crate::args::Args;

/// Replayable example fights, chosen from a probe run rather than cherry-picked by
/// hand: the worst case for the first side, the median, and the best.
pub fn print_fights(args: &Args, roster: &[&Creature]) {
    if args.fights == 0 {
        return;
    }
    let policies = [Policy::Greedy; 2];
    // Enough fights to have a genuine worst and best case to choose between,
    // without paying for a second full evaluation.
    let probe = args.samples.min(2_000);

    let mut runs: Vec<(u64, Outcome)> = (0..probe as u64)
        .map(|stream| {
            let mut rng = Rng::with_stream(args.seed, stream);
            let mut log = None;
            (
                stream,
                run_teams(&mut rng, roster, policies, args.cap, args.budget, &mut log),
            )
        })
        .collect();

    // Rank by how the first side did: winning dominates, then its own HP, then how
    // far into the opponent it got. That last term is what separates the runs in a
    // matchup it never wins, where every fight leaves it on zero.
    runs.sort_by_key(|(_, o)| {
        (
            o.winner == Some(Side::A),
            o.hp_left[0].max(0),
            -o.hp_left[1],
        )
    });

    let picks: Vec<(&str, usize)> = [
        ("worst for the first side", 0),
        ("median", runs.len() / 2),
        ("best for the first side", runs.len() - 1),
    ]
    .into_iter()
    .take(args.fights)
    .collect();

    for (label, at) in picks {
        let (stream, _) = runs[at];
        let mut rng = Rng::with_stream(args.seed, stream);
        let mut log = Some(Vec::new());
        let outcome = run_teams(&mut rng, roster, policies, args.cap, args.budget, &mut log);
        println!(
            "--- {label} (seed {} stream {stream}, of {probe} probed) ---",
            args.seed
        );
        for line in log.unwrap_or_default() {
            println!("{line}");
        }
        match outcome.winner {
            Some(side) => println!(
                "side {:?} wins in {} with {} standing on {} hp\n",
                side,
                rounds(outcome.rounds),
                outcome.survivors[side.index()],
                outcome.hp_left[side.index()]
            ),
            None => println!(
                "unresolved after {}: {} standing on {} hp against {} on {} hp\n",
                rounds(outcome.rounds),
                outcome.survivors[0],
                outcome.hp_left[0],
                outcome.survivors[1],
                outcome.hp_left[1]
            ),
        }
    }
}

pub fn rounds(n: u32) -> String {
    if n == 1 {
        "1 round".to_string()
    } else {
        format!("{n} rounds")
    }
}
