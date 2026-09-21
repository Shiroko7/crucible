//! Formats and prints the playstyle profile and monster sweep tables.

use crate::args::Args;
use crucible_core::creature::Creature;
use crucible_core::sim::{evaluate_teams, Policy, Summary};
pub fn print_profile(args: &Args, roster: &[&Creature]) {
    let monster = Policy::Greedy;
    let names = side_names(args, roster);
    println!(
        "{} against {}, which is played {} throughout",
        names[0],
        names[1],
        monster.name()
    );
    print_header();
    let searchers = roster.iter().filter(|c| c.team == 0).count();
    for (i, policy) in Policy::ALL.into_iter().enumerate() {
        let samples = args.samples_for(policy, searchers);
        let s = evaluate_teams(
            args.seed.wrapping_add(i as u64),
            roster,
            [policy, monster],
            samples,
            args.cap,
            args.budget,
        );
        print_row(policy.name(), policy.blurb(), &s, 0, samples);
    }
    println!();
}

/// The other axis, and the one no published statistics cover: how much the answer
/// moves with how well the monster is run.
pub fn print_monster_sweep(args: &Args, roster: &[&Creature]) {
    let party = Policy::Greedy;
    let names = side_names(args, roster);
    println!(
        "The same fight by how {} is run, with {} played {}",
        names[1],
        names[0],
        party.name()
    );
    print_header();
    for (i, policy) in Policy::CHEAP.into_iter().enumerate() {
        let s = evaluate_teams(
            args.seed.wrapping_add(100 + i as u64),
            roster,
            [party, policy],
            args.samples,
            args.cap,
            args.budget,
        );
        print_row(policy.name(), policy.blurb(), &s, 1, args.samples);
    }
    println!();
}

/// One cheap evaluation purely to read the side labels back out.
pub fn side_names(args: &Args, roster: &[&Creature]) -> [String; 2] {
    evaluate_teams(
        args.seed,
        roster,
        [Policy::Greedy; 2],
        1,
        args.cap,
        args.budget,
    )
    .names
}

pub fn print_header() {
    println!(
        "{:>11}  {:>6}  {:>7}  {:>6}  {:>6}  {:>5}  {:>7}  {:>7}  {:>6}",
        "playstyle", "P(win)", "P(loss)", "deaths", "rounds", "spent", "dmg/rnd", "worst10", "n"
    );
}

pub fn print_row(name: &str, blurb: &str, s: &Summary, side: usize, samples: usize) {
    println!(
        "{name:>11}  {:>6.3}  {:>7.3}  {:>6.2}  {:>6.1}  {:>5.1}  {:>7.1}  {:>7.1}  {:>6}   {blurb}",
        s.wins[side],
        s.wins[1 - side],
        s.mean_deaths[side],
        s.mean_rounds,
        s.mean_spent[side],
        s.mean_damage_per_round[side],
        s.cvar_hp_left[side],
        samples,
    );
}
