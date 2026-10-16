//! The fights worth watching: for each playstyle, the worst, the median and
//! the best of a probe run, recorded so a viewer can play them back.
//!
//! Chosen from the run rather than cherry-picked by hand, and each one
//! reproducible from the seed and stream it carries.

use serde::{Deserialize, Serialize};

use crate::creature::Creature;
use crate::prob::Rng;
use crate::sim::{replay_teams, run_teams, Budget, Outcome, Policy, Replay, Side};

/// One recorded fight and how it was picked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// The first side's playstyle.
    pub policy: String,
    /// `worst`, `median` or `best`, for the first side.
    pub pick: String,
    pub seed: u64,
    pub stream: u64,
    pub replay: Replay,
}

/// How the first side did under one playstyle, over the probe run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Style {
    pub policy: String,
    /// One line on what the playstyle is.
    pub blurb: String,
    /// Shares of the probe run the first side won, lost, and neither.
    pub win: f64,
    pub loss: f64,
    pub draw: f64,
    /// Mean rounds a fight lasted.
    pub rounds: f64,
    /// Mean creatures the first side lost.
    pub deaths: f64,
}

/// Everything a viewer needs for one scenario.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bundle {
    pub title: String,
    /// Fights probed per playstyle to find the three it shows.
    pub probed: usize,
    /// One per playstyle, in the order their fights appear in `entries`.
    #[serde(default)]
    pub styles: Vec<Style>,
    pub entries: Vec<Entry>,
}

/// The playstyles a showcase covers when nobody says: the ones a party
/// actually plays, without the search.
pub const STYLES: [Policy; 5] = [
    Policy::Greedy,
    Policy::Nova,
    Policy::Defensive,
    Policy::Attrition,
    Policy::Thrifty,
];

/// [`showcase`] for a parsed scenario, under the defaults a viewer wants:
/// every creature given somewhere to stand, `apart` feet between the sides,
/// each of [`STYLES`] against a greedy opponent, fifty rounds at most - on a
/// map of `field` squares, or the least there is when it says none.
pub fn of_scenario(
    title: &str,
    mut creatures: Vec<Creature>,
    seed: u64,
    probe: usize,
    apart: u32,
    field: Option<(i32, i32)>,
) -> Result<Bundle, String> {
    for team in [0u8, 1] {
        if !creatures.iter().any(|c| c.team == team) {
            return Err("the scenario has nobody on one of the two sides".into());
        }
    }
    crate::sim::arena::place(&mut creatures, apart);
    let roster: Vec<&Creature> = creatures.iter().collect();
    Ok(showcase(
        title,
        seed,
        &roster,
        &STYLES,
        Policy::Greedy,
        probe,
        50,
        Budget {
            field,
            ..Budget::default()
        },
    ))
}

/// `probe` fights from `seed`, one per stream, ordered from the worst for the
/// first side to the best.
///
/// Winning dominates, then its own HP, then how far into the opponent it got.
/// That last term is what separates the runs in a matchup it never wins,
/// where every fight leaves it on zero.
pub fn ranked(
    seed: u64,
    roster: &[&Creature],
    policies: [Policy; 2],
    probe: usize,
    max_rounds: u32,
    budget: Budget,
) -> Vec<(u64, Outcome)> {
    let mut runs: Vec<(u64, Outcome)> = (0..probe as u64)
        .map(|stream| {
            let mut rng = Rng::with_stream(seed, stream);
            let mut log = None;
            (
                stream,
                run_teams(&mut rng, roster, policies, max_rounds, budget, &mut log),
            )
        })
        .collect();
    runs.sort_by_key(|(_, o)| {
        (
            o.winner == Some(Side::A),
            o.hp_left[0].max(0),
            -o.hp_left[1],
        )
    });
    runs
}

/// Where the worst, the median and the best sit in a [`ranked`] run.
pub fn picks(runs: usize) -> [(&'static str, usize); 3] {
    [
        ("worst", 0),
        ("median", runs / 2),
        ("best", runs.saturating_sub(1)),
    ]
}

/// The worst, median and best fight of each of `styles` for the first side,
/// against `opponent`, recorded.
#[allow(clippy::too_many_arguments)]
pub fn showcase(
    title: &str,
    seed: u64,
    roster: &[&Creature],
    styles: &[Policy],
    opponent: Policy,
    probe: usize,
    max_rounds: u32,
    budget: Budget,
) -> Bundle {
    let mut entries = Vec::new();
    let mut summaries = Vec::new();
    for &style in styles {
        let policies = [style, opponent];
        let runs = ranked(seed, roster, policies, probe, max_rounds, budget);
        if runs.is_empty() {
            continue;
        }
        let n = runs.len() as f64;
        let share =
            |side: Option<Side>| runs.iter().filter(|(_, o)| o.winner == side).count() as f64 / n;
        summaries.push(Style {
            policy: style.name().to_string(),
            blurb: style.blurb().to_string(),
            win: share(Some(Side::A)),
            loss: share(Some(Side::B)),
            draw: share(None),
            rounds: runs.iter().map(|(_, o)| f64::from(o.rounds)).sum::<f64>() / n,
            deaths: runs
                .iter()
                .map(|(_, o)| f64::from(o.deaths[0]))
                .sum::<f64>()
                / n,
        });
        for (pick, at) in picks(runs.len()) {
            let stream = runs[at].0;
            let mut rng = Rng::with_stream(seed, stream);
            let (_, replay) = replay_teams(&mut rng, roster, policies, max_rounds, budget);
            entries.push(Entry {
                policy: style.name().to_string(),
                pick: pick.to_string(),
                seed,
                stream,
                replay,
            });
        }
    }
    Bundle {
        title: title.to_string(),
        probed: probe,
        styles: summaries,
        entries,
    }
}
