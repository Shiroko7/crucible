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

Early, but it runs. The probability layer is solid, and there is now one thin
vertical slice on top of it: a single creature against a single creature, many
times, reported.

| component | subsystem | state |
|---|---|---|
| `rng` | `prob::rng` | PCG32, seedable, independent streams for parallel rollouts |
| `dice` | `prob::dice` | exact PMFs by convolution: pools, mixtures, flooring, halving |
| `combat` | `rules::combat` | attack resolution, both exact and sampled: crits, advantage, resistance, composable `AttackModifier`/`DamageRider` lists (Bless, Bane, extra damage dice) |
| `exact` | `prob::exact` | closed-form kill curves and expected attacks, by dynamic programming |
| `creature` | `rules::creature` | modular combatant model: multi-type damage, saves, recharge, resource pools, riders |
| `dsl` | `dsl` | Monad Plugin Architecture (`FeaturePlugin`, `CreatureBuilder`, `FeatureRegistry`), PC & Monster abstractions, TOML config loaders |
| `scenario` | `dsl::scenario` | scenario parser supporting both external creature configs (`source:`) and inline declarations |
| `duel` | `sim::duel` | team combat rounds: initiative, action economy, reactions, condition lifetimes, legendary actions between turns |
| `analysis` | `sim::analysis` | win and death probability, CVaR of the bad tail, exact pacing check |
| riders | `rules::creature::rider` | Evasion, Legendary Resistance, Stunning Strike, Deflect Attacks - as general mechanisms |
| policies | `sim::duel::policy` | eight of the nine from `DESIGN.md`: `solver`, `nova`, `greedy`, `focus-fire`, `scattered`, `in-order`, `defensive`, `attrition`, `thrifty` |
| search | `sim::duel` | flat Monte Carlo over one turn, to a depth budget |
| content | `content/` | data-driven PC (`content/characters/`) and Monster (`content/monsters/`) formatted configurations |
| event pipeline | `sim::duel` | riders fire at four fixed points; they do not yet subscribe to hooks |
| `fitted` policy | | blocked: it means fitting parameters to logged play, and there are no logs |

### What is and is not modelled

A triggered modifier is data, not an engine branch, which is the property that
has to hold for the full ruleset to be reachable. Four mechanisms cover a lot:

| mechanism | features it carries |
|---|---|
| `NothingOnSuccess` | Evasion, Danger Sense |
| `AlwaysSucceed` | Legendary Resistance, Indomitable |
| `SaveOrCondition` | Stunning Strike, knockdowns, on-hit poisons, breath weapon riders |
| `ReduceDamage` | Deflect Attacks, Uncanny Dodge, Heavy Armor Master |

**Positioning is the gap that matters.** There is no movement, reach, or flight,
so a dragon with an 80-foot fly speed stands still and trades hits. Anything
whose point is where the combatants are - Wings Unfurled, a 60-foot cone, Shell
Defense as a way to survive a round - is therefore out of scope until there is a
movement model. So is everything non-combat: languages, tool proficiencies,
Hold Breath.

One deliberate simplification: a creature gets one once-per-turn rider trigger
per turn in total rather than one per rider. That is exact for Stunning Strike
and understates anything with two, which is the safe direction here.

### The objective needs a margin term

`DESIGN.md` states the objective as `P(win) - lambda * resources spent`. That is
degenerate in a position that cannot be won. Every line scores zero on the first
term, so the only term left is the penalty, and a search maximising it correctly
concludes that the best available play is **to do nothing at all** - it stands
still saving its focus points while it is eaten. The first solver row printed
was exactly that: 2.7 damage a round and nothing spent.

So the value function also carries a margin term - how much healthier a side
finished than the other - counted on every rollout, not only on truncated ones.
A win still dominates it outright. Without it a hopeless row stops describing a
fight, and a search can never prefer the line that nearly won.

`lambda` has to stay far below what a resource buys, for the same reason: one
focus point spent on a Flurry of Blows moves the health margin by about 0.015,
so any penalty near that turns "spend it" into "hoard it".

### What the search is, and is not

`solver` is flat Monte Carlo: it enumerates this turn's legal plans, plays each
out to a depth budget many times, and keeps the best. One ply of real choice. It
is **not** the UCT tree search `DESIGN.md` asks for, and it shows - it cannot
plan a sequence, and at a small budget it is noisy enough that its row is not
reliably the best one. Against the ogre it matches greedy's survival and lands
50% more stuns while taking half a round longer, because its objective rewards
finishing healthy rather than finishing fast.

Its rollouts also repeat the plan under test rather than falling back to greedy
play. Without that, a plan whose whole value is in being repeated - a stun lock -
scores the same as the move it is meant to beat.

### Trying it

```bash
cargo run --release -p crucible-cli -- scenarios/gio-vs-adult-red-dragon.crucible
cargo run --release -p crucible-cli -- scenarios/gio-vs-ogre.crucible
```

Each prints what it read — including traits and resource pools, since
transcription is the likeliest thing to be wrong — then a row per playstyle, the
same fight swept over how well the monster is run, and three replayable example
fights chosen by how the first side did: worst, median, best.

The `solver` row runs a fight inside every fight, so it gets its own sample count
(`--solver-n`, default 1000) and the table prints `n` per row rather than hiding
the difference in confidence.

Ingestion is designed so the agent is a **compile step, not a runtime one**: it
checks each ability against the registry, writes only what is genuinely
missing, and the result is hashed and reused. A seeded simulation whose rules
get re-derived on every run is not reproducible, and reproducibility is the
whole premise. The model emits data in a constrained DSL and never does
arithmetic — so the DSL is the sandbox, and a wrong ability is wrong in a
bounded way. Anything synthesised stays marked unreviewed, and every run
reports how much of it was guessed.

Nothing here plays D&D well. What there is is a probability engine that is
checked rather than trusted, and a simulator thin enough that every gap in it
is written down.

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
cargo test                                # 86 tests: unit, agreement, property
cargo clippy --all-targets -- -D warnings
cargo fmt
```

On Windows with the GNU toolchain, `proptest` reaches `windows-sys` through
`getrandom`, which needs mingw-w64 **binutils** on `PATH` — `rustup` ships a
linker but not `dlltool`. `winget install BrechtSanders.WinLibs.POSIX.MSVCRT`
supplies it.

## Content and Configuration

Creatures are formatted data configurations rather than hardcoded simulation logic:
- `content/characters/`: Player Characters (e.g. `gio.toml`), defining class, level, stats, resources, and known feature plugins.
- `content/monsters/`: Monster statblocks (e.g. `adult-red-dragon.toml`, `ogre.toml`).
- `scenarios/`: Encounters referencing combatants via `source:` (e.g. `source: content/characters/gio.toml`).

SRD material is CC-BY-4.0 and ships with attribution. Non-SRD material is loaded from local
data files and never committed — `scenarios/local/` is gitignored for that. The Monad Plugin
Architecture ensures that new or missing mechanics are implemented as reusable plugins,
while PCs and monsters are expressed purely as structured data.
