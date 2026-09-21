//! CLI argument parsing and configuration flags.

use crucible_core::sim::{Budget, Policy};
pub const USAGE: &str = "\
usage: crucible <scenario file> [options]

  --samples N   fights per policy row (default 10000)
  --seed N      base seed; a run is reproducible from it (default 1)
  --cap N       give up after this many rounds (default 50)
  --fights N    narrated example fights (default 3)
  --copies N    put N copies of the first side in the fight
  --sweep N     sweep the first side's size from 1 to N and minimax it
  --sweep-n N   fights per cell inside a sweep (default 2000)
  --rollouts N  solver rollouts per candidate turn (default 16)
  --depth N     solver lookahead in rounds (default 4)
  --solver-n N  fights for the solver row (default 1000; it costs ~200x a row)
";

/// Playstyles swept for the party in a minimax. `Scattered` and `FocusFire` are
/// the same thing when there is one monster to hit, so only one of them is here.
pub const PARTY_STYLES: [Policy; 5] = [
    Policy::Greedy,
    Policy::Nova,
    Policy::Defensive,
    Policy::Attrition,
    Policy::Thrifty,
];

/// Playstyles swept for the monster. Targeting is where a monster's skill mostly
/// lives once it faces more than one enemy, so both targeting rules are here.
pub const MONSTER_STYLES: [Policy; 6] = [
    Policy::FocusFire,
    Policy::Greedy,
    Policy::Scattered,
    Policy::Nova,
    Policy::InOrder,
    Policy::Thrifty,
];

#[derive(Debug, Clone)]
pub struct Args {
    pub path: String,
    pub samples: usize,
    pub seed: u64,
    pub cap: u32,
    pub fights: usize,
    pub copies: Option<usize>,
    pub sweep: Option<usize>,
    pub sweep_samples: usize,
    pub budget: Budget,
    pub solver_samples: usize,
}

impl Args {
    pub fn samples_for(&self, policy: Policy, searchers: usize) -> usize {
        if policy == Policy::Solver {
            let scaled = self.solver_samples / searchers.max(1).pow(2);
            scaled.clamp(20.min(self.samples), self.samples)
        } else {
            self.samples
        }
    }
}

pub fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let mut path = None;
    let mut out = Args {
        path: String::new(),
        samples: 10_000,
        seed: 1,
        cap: 50,
        fights: 3,
        copies: None,
        sweep: None,
        sweep_samples: 2_000,
        budget: Budget {
            rollouts: 16,
            depth: 4,
        },
        solver_samples: 1_000,
    };

    while let Some(arg) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("`{arg}` needs a value"))?
                .parse::<u64>()
                .map_err(|_| format!("`{arg}` needs a number"))
        };
        match arg.as_str() {
            "-h" | "--help" => return Err(String::new()),
            "--samples" => out.samples = value()? as usize,
            "--seed" => out.seed = value()?,
            "--cap" => out.cap = value()? as u32,
            "--fights" => out.fights = value()? as usize,
            "--copies" => out.copies = Some(value()? as usize),
            "--sweep" => out.sweep = Some(value()? as usize),
            "--sweep-n" => out.sweep_samples = value()? as usize,
            "--rollouts" => out.budget.rollouts = value()? as u32,
            "--depth" => out.budget.depth = value()? as u32,
            "--solver-n" => out.solver_samples = value()? as usize,
            other if other.starts_with('-') => return Err(format!("unknown option `{other}`")),
            other => path = Some(other.to_string()),
        }
    }

    out.path = path.ok_or("no scenario file given")?;
    if out.samples == 0 {
        return Err("--samples must be at least 1".into());
    }
    Ok(out)
}
