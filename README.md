# crucible

Answering one question about a D&D 5e encounter:

> Is this actually difficult **for my party**?

Not its CR. The encounter-building rules reduce a fight to a sum of XP values,
which ignores tactics, terrain, action economy, party composition, and whatever
the party has left in the tank. It produces a label that most DMs have learned
to distrust.

The intended answer is not a label and not an optimal line of play. It is a
profile across the ways a table might actually play:

```
                       P(win)   P(a PC dies)   slots spent   rounds
Solver                  0.97        0.04           1.2         3.1
Nova                    0.91        0.11           4.0         2.4
FocusFire               0.88        0.14           2.1         3.6
Attrition               0.52        0.44           0.3         6.8
```

The spread between those rows is the point. If every playstyle wins it is
filler; if none do it is a wall; if the gap is wide it is a skill check, and
that distinction does not survive being compressed into "Hard."

[`DESIGN.md`](DESIGN.md) has the reasoning: why this is a two-player stochastic
game rather than an optimisation problem, why "minimise rounds" is a trap, and
why the whole ruleset forces an event-driven architecture rather than a set of
procedures.

## Status

Early. What exists is the probability layer everything else sits on.

| component | state |
|---|---|
| `rng` | PCG32, seedable, independent streams for parallel rollouts |
| `dice` | exact PMFs by convolution: pools, mixtures, flooring, halving |
| `combat` | attack resolution, both exact and sampled: crits, advantage, resistance |
| `exact` | closed-form kill curves and expected attacks, by dynamic programming |
| event pipeline | not started |
| ability DSL | not started |
| policies | not started |
| MCTS | not started |
| stat block ingestion | not started |

There is no encounter simulator yet, and nothing here plays D&D. What there is
is a probability engine that is checked rather than trusted.

## The thing worth looking at

Every rule is implemented twice: once exactly, by convolution and dynamic
programming, and once by sampling. The exact path is obviously correct and far
too slow to scale. The sampled path is the one a real encounter will use, and
it is the one that can be quietly wrong — a biased `d20`, a crit that doubles
the modifier, resistance applied before the damage floor instead of after. None
of those produce implausible numbers. They produce slightly wrong ones, forever.

So the two are required to agree, per outcome, with tolerances derived from the
standard error of the estimate rather than tuned until they passed:

```rust
let tol = 5.0 * (p * (1.0 - p) / n as f64).sqrt() + 1e-4;
```

Same idea as a chess engine's perft suite. The cheap path proves the fast path
before anything interesting is built on top of it.

`proptest` covers the rest: mass is conserved, a kill curve never decreases,
advantage is never worse than disadvantage, sampled damage is never something
the exact distribution calls impossible.

## Working on it

```bash
cargo test                                # 42 tests: unit, agreement, property
cargo clippy --all-targets -- -D warnings
cargo fmt
```

On Windows with the GNU toolchain, `proptest` reaches `windows-sys` through
`getrandom`, which needs mingw-w64 **binutils** on `PATH` — `rustup` ships a
linker but not `dlltool`. `winget install BrechtSanders.WinLibs.POSIX.MSVCRT`
supplies it.

## Content

SRD 5.1 material is CC-BY-4.0 and can ship here with attribution. Anything
outside the SRD is loaded from local data files and never committed — which
doubles as a useful constraint, since a monster that cannot be expressed as
data means the ability DSL is missing something.
