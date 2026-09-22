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
| `exact` | `prob::exact` | closed-form kill curves and expected attacks, by dynamic programming |
| rules | `rules` | core 5e rules, one concept per file: abilities, sizes, creature types, conditions and their lifetimes, damage and reduction, healing, spell slots, saves, checks |
| attacks | `rules::attack` | attack resolution, both exact and sampled: crits, advantage, resistance, composable `AttackModifier`/`DamageRider` lists (Bless, Bane, extra damage dice) |
| `creature` | `creature` | modular combatant model: moves and their effects, multi-type damage (either of two types, chosen per target), saves, recharge, resource pools, riders, reactions that are moves of their own, auras, lasting boons, summons |
| riders | `creature::rider` | Evasion, Legendary Resistance (and a ring that rescues one kind of save for a reaction), Stunning Strike, Sneak Attack and Cunning Strike, bonus dice against a creature type or against a marked quarry, dice on the first hit of each of its turns, weapon buffs armed by a condition, injury poisons, immunity downgrades, a damage threshold with a weak spot, swallowing, and reactions (Deflect Attacks, Uncanny Dodge, a reactive AC boost that can stay up) - as general mechanisms, all applied in live fights |
| features | `features` | the ruleset as plugins (`FeaturePlugin`, `CreatureBuilder`, `FeatureRegistry`), one file per feature: `classes/` (a folder per class and subclass), `spells/`, `monsters/`, `spellcasting/`, `items/`, `summons/`, and a lasting boon directly under `features/`; each feature registers its own TOML factory |
| grammar | `dsl::grammar` | the phrase grammar moves and traits are written in, shared by every creature format and by features that take a move as a parameter |
| `scenario` | `dsl::scenario` | scenario parser supporting both external creature configs (`source:`) and inline declarations |
| configs | `dsl::config` | TOML PC and Monster loaders |
| fight | `sim::fight` | team combat rounds, one file per stage of a turn: initiative, action economy, auras, reactions, condition lifetimes, concentration, a damage threshold, swallowing, summons and boons, where enemies stand around a creature with a mouth, legendary actions of any cost between turns |
| `analysis` | `sim::analysis` | win and death probability, CVaR of the bad tail, exact pacing check |
| policies | `sim::policy` | eight of the nine from `DESIGN.md`: `solver`, `nova`, `greedy`, `focus-fire`, `scattered`, `in-order`, `defensive`, `attrition`, `thrifty` |
| search | `sim::fight` | flat Monte Carlo over one turn, to a depth budget |
| content | `content/` | data-driven PC (`content/characters/`) and Monster (`content/monsters/`) formatted configurations |
| event pipeline | `sim::fight` | riders fire at fixed points (at the start of a turn, as an attack is rolled, on being targeted, on a hit's damage, as damage lands, on a hit, as a condition lands, on a failed save, on a save for half, at the end of a turn); they do not yet subscribe to hooks |
| `fitted` policy | | blocked: it means fitting parameters to logged play, and there are no logs |

### What is and is not modelled

A triggered modifier is data, not an engine branch, which is the property that
has to hold for the full ruleset to be reachable. A handful of mechanisms cover a
lot:

| mechanism | features it carries |
|---|---|
| `NothingOnSuccess` | Evasion, Danger Sense |
| `AlwaysSucceed` | Legendary Resistance, Indomitable |
| `SaveOrCondition` | Stunning Strike, knockdowns, on-hit poisons, breath weapon riders |
| `ConditionalExtraDamage` | Sneak Attack, with Cunning Strike spending its dice |
| `BonusDamageVsCreatureType` | a slaying weapon, Favored Enemy damage |
| `ReduceDamage` | Deflect Attacks, Heavy Armor Master |
| `HalveAttackDamage` | Uncanny Dodge |
| `ReactionOnTargeted` | the Shield spell, an item raised against ranged weapon attacks, a parry that stays up until its next turn |
| `OncePerTurnDamage` | a swarm that joins one blow a turn, a once-a-turn elemental strike |
| `BonusDamageVsQuarry` with `Afflict` | Hunter's Mark, Hex: a mark the caster concentrates on and pays out against |
| `Boon` | a card that turns its bearer into something with tougher hide and heavier blows, a spell laid on one blade, a stance of ice |
| `Summon` with `Requirement` | a double called up beside its summoner, commanded out of its bonus action and spent in a burst |
| damage of either type | a blade that can deal cold "instead of the weapon's normal damage type" |
| `DamageThreshold` | an armoured shell or hull; a mouth or a crack as its weak spot |
| `Swallow`, `Digestion`, `Regurgitate` | a purple worm's, a behir's or a tarrasque's gullet |
| a `Reaction` move with a trigger | a snap at whoever is dragged into reach, a spray from a breached shell |
| `mouth`, `reach`, `tactic` | a whirlpool, a kraken or a dragon turtle its enemies circle, and how it swims at them |

A creature has one reaction a round, shared by every reaction it knows, and
none while Incapacitated - unless its stat block grants more (`reactions N`),
in which case it still takes at most one per turn, so the budget buys it
answers to several *different* creatures rather than a second answer to one. An aura is a move every enemy meets at the start of
its turn, and a legendary action costs as many of the round's uses as it says. Heals go to the healer's own side - a downed player
character first - and a player character dropped by less than massive damage
is down, not dead, until healed or the fight ends. Under the 2024 rules at most
one spell slot is spent per turn.

**Positioning is the gap that matters.** There is no map, so for most creatures
there is no movement, reach, or flight: a dragon with an 80-foot fly speed
stands still and trades hits. Anything whose point is where the combatants are -
Wings Unfurled, a 60-foot cone, Shell Defense as a way to survive a round - is
out of scope for them. So is everything non-combat: languages, tool
proficiencies, Hold Breath.

The exception is a creature with a `mouth`. Its enemies stand in one of four
places around it: at its mouth, beside its body, at range in front of it, or far
off in front. That is not a map, but it is enough for what such a creature is
about:

- Its moves reach only where they say (`reach mouth`, `near`, `front`): a bite
  reaches the mouth, a slam anywhere beside it, a breath the whole front.
- A melee attack reaches it only from beside it. Its mouth - its weak spot -
  takes a melee attack at the mouth or a ranged one from in front, unless it has
  sealed it.
- An enemy moves one place a turn, or two outside difficult terrain, and loses a
  move to standing up. A pull drags it a place closer, a push throws it a place
  out.
- The creature swims by its `tactic`: `charge` brings the nearest enemy to its
  mouth every turn, `hit and run` does that and then withdraws everyone far off,
  and `hold` stays put. The report adds a row for each.
- Each character stands wherever its best action is worth the most, and of
  equally good places the one furthest from the mouth. The solver searches
  where to stand along with what to do.

The other kind of reach comes from being swallowed: a swallowed creature can
reach nothing but its swallower's insides, and nothing outside can reach it - not
an attack, an area, or a heal.

A damage threshold ignores any single hit, save or dart below it. An attack
roll can go through an open weak spot instead - a mouth, a crack a breach leaves
until the round ends - trading the threshold for the weak spot's resistances. A
sealed mouth opens again the moment its owner attacks or takes a legendary
action. An attacker takes whichever it expects to do more against, so a
hit big enough to breach goes to the shell, where it lands whole. From inside,
everything reaches the weak spot.

"An ally within 5 feet of the target", which Sneak Attack asks about, has no
geometry to read, so it is read off who is fighting what: an ally counts if it is
up, not Incapacitated, leads with a melee attack, and is going after the same
target. A party's archers never count for each other; its front line counts for
everyone.

A summoned double is kit rather than a combatant. It has hit points, it can be
attacked and destroyed, and it goes when its summoner does - but it takes no
turn of its own (everything it does costs its summoner a move, and commanding
several of them together hits harder than any one), its destruction is not a
death on its side, its hit points are not part of how healthy that side
finished, and a side with nothing left but doubles has lost.

Something that lasts - a mark, a form, an enchantment on a blade - is worth
what it adds to this creature's best blow, counted over the fight's own
planning horizon (the search's depth budget) or the effect's duration,
whichever is shorter, and nothing at all once it is already up. So a playstyle
that ranks on damage puts a mark up once and then swings, one that hoards never
bothers, and the solver finds the rest by playing it out. Nothing counts what a
boon is worth *defensively*: resistances have no attack value, so a purely
defensive form scores zero.

One deliberate simplification: a creature gets one once-per-turn trigger for
Stunning-Strike-style riders per turn in total rather than one per rider. That is
exact for Stunning Strike and understates anything with two, which is the safe
direction here. Sneak Attack keeps its own once-per-turn budget, and so does a
rider that joins the first hit of each of the creature's own turns.

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
cargo test                                # unit, agreement, property
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
while PCs and monsters are expressed purely as structured data. Each plugin lives in its own
file under `crates/crucible-core/src/features/`, in the narrowest folder that covers every
class (or spell list, or item) that has it, together with its TOML factory and its tests.

`crates/crucible-core/tests/multiclass_rogue_caster.rs` is the worked example of a whole
multiclass build written that way - a rogue with a secondary spellcasting grant, magic
weapons, items used through Fast Hands and reactions - with every plugin and trait keyword
it needs, and fights proving each one is live. `tests/swallowing_titan.rs` does the same for
a monster: a shell, a gullet, an aura, reactions and legendary actions of several costs.
`tests/positions_around_a_maw.rs` does it for a creature with a mouth and the places its
enemies stand around it, under each tactic. `tests/dual_wielding_summoner.rs` does it for a
two-weapon build: a marked quarry, a swarm joining one blow a turn, an enchantment on one of
its two blades, a form it puts on for a minute, a parry that stays up, a ring that rescues a
failed save, and doubles it calls up and commands. `dsl::scenario`'s module docs list the
move and trait syntax.
