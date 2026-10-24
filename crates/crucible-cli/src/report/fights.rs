//! Replayable example fight narrations.

use crate::args::{Args, PARTY_STYLES};
use crucible_core::creature::Creature;
use crucible_core::prob::Rng;
use crucible_core::sim::showcase::{picks, ranked, showcase};
use crucible_core::sim::{run_teams, Policy};

/// Enough fights to have a genuine worst and best case to choose between,
/// without paying for a second full evaluation.
fn probe(args: &Args) -> usize {
    args.samples.min(2_000)
}

/// Replayable example fights, chosen from a probe run rather than cherry-picked by
/// hand: the worst case for the first side, the median, and the best.
pub fn print_fights(args: &Args, roster: &[&Creature]) {
    if args.fights == 0 {
        return;
    }
    let policies = [Policy::Greedy; 2];
    let probe = probe(args);
    let runs = ranked(args.seed, roster, policies, probe, args.cap, args.budget);

    for (label, at) in picks(runs.len()).into_iter().take(args.fights) {
        let (stream, _) = runs[at];
        let mut rng = Rng::with_stream(args.seed, stream);
        let mut log = Some(Vec::new());
        let outcome = run_teams(&mut rng, roster, policies, args.cap, args.budget, &mut log);
        let label = match label {
            "median" => label.to_string(),
            other => format!("{other} for the first side"),
        };
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

/// The worst, median and best fight of each party playstyle against a greedy
/// monster, recorded frame by frame and written to `file` as JSON - what the
/// viewers play back.
pub fn export_fights(args: &Args, roster: &[&Creature], file: &str) -> std::io::Result<()> {
    let bundle = showcase(
        &args.path,
        args.seed,
        roster,
        &PARTY_STYLES,
        Policy::Greedy,
        probe(args),
        args.cap,
        args.budget,
    );
    std::fs::write(file, serde_json::to_string(&bundle)?)?;
    println!(
        "wrote {} fights ({} playstyles, {} probed each) to {file}",
        bundle.entries.len(),
        PARTY_STYLES.len(),
        bundle.probed
    );
    Ok(())
}

pub fn rounds(n: u32) -> String {
    if n == 1 {
        "1 round".to_string()
    } else {
        format!("{n} rounds")
    }
}
