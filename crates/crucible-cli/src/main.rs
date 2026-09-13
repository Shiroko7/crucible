//! Read a scenario or formatted creature configuration, fight it many times, and report results.

use std::process::ExitCode;

use crucible_core::creature::Creature;
use crucible_core::scenario;

mod args;
mod report;

use args::{parse_args, USAGE};
use report::{
    print_fights, print_monster_sweep, print_party_sweep, print_profile, print_sheet, resize,
};

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(message) => {
            if !message.is_empty() {
                eprintln!("crucible: {message}\n");
            }
            eprint!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };

    let text = match std::fs::read_to_string(&args.path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("crucible: cannot read {}: {e}", args.path);
            return ExitCode::FAILURE;
        }
    };

    let parsed = match scenario::parse(&text) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("crucible: {}: {e}", args.path);
            return ExitCode::FAILURE;
        }
    };
    for team in [0u8, 1] {
        if !parsed.iter().any(|c| c.team == team) {
            eprintln!("crucible: {} has nobody on one of the two sides", args.path);
            return ExitCode::FAILURE;
        }
    }

    if let Some(max) = args.sweep {
        print_party_sweep(&args, &parsed, max);
        return ExitCode::SUCCESS;
    }

    let creatures = resize(&parsed, args.copies.unwrap_or(0));
    let roster: Vec<&Creature> = creatures.iter().collect();
    print_sheet(&roster);
    print_profile(&args, &roster);
    print_monster_sweep(&args, &roster);
    print_fights(&args, &roster);
    ExitCode::SUCCESS
}
