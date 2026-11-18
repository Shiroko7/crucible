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
| features | `features` | the ruleset as plugins (`FeaturePlugin`, `CreatureBuilder`, `FeatureRegistry`), one file per feature: `classes/` (a folder per class and subclass - Cleric and Rogue so far), `spells/`, `monsters/`, `spellcasting/`, `items/`, `summons/`, and the mechanisms no one class owns directly under `features/` - a lasting boon, a raised aura, a reaction that answers a blow, a cast taken at its maximum; each feature registers its own TOML factory |
| grammar | `dsl::grammar` | the phrase grammar moves and traits are written in, shared by every creature format and by features that take a move as a parameter |
| `scenario` | `dsl::scenario` | scenario parser supporting both external creature configs (`source:`) and inline declarations |
| configs | `dsl::config` | TOML PC and Monster loaders |
| fight | `sim::fight` | team combat rounds, one file per stage of a turn: initiative, action economy, auras, reactions, condition lifetimes, concentration, a damage threshold, swallowing, summons and boons, where enemies stand around a creature with a mouth, legendary actions of any cost between turns |
| `analysis` | `sim::analysis` | win and death probability, CVaR of the bad tail, exact pacing check |
| policies | `sim::policy` | eight of the nine from `DESIGN.md`: `solver`, `nova`, `greedy`, `focus-fire`, `scattered`, `in-order`, `defensive`, `attrition`, `thrifty` |
| search | `sim::fight` | flat Monte Carlo over one turn, to a depth budget |
| content | `content/` | one example character (`content/characters/`) and one example monster (`content/monsters/`) as data; your own go in the gitignored `content/local/` |
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
| `Boon` with `ac` | a ward held around somebody: Armor Class and nothing else, for as long as it is up |
| `TempHp` | a ward that soaks the next blows and is then gone - spent ahead of hit points, and never healing anybody back up |
| `Banished` | sent elsewhere for the duration: it acts on nothing and nothing reaches it, and it comes back when the spell ends |
| `Components` | V/S/M on a cast, so Silence stops what has to be spoken rather than every spell |
| `Aura` raised and held | a ring of spirits, a burning field: a cast that every enemy answers at the start of its turn, for as long as concentration holds it |
| a reaction `when hit` | a storm answering whoever closes with its priest, a mantle of thorns: it does not stop the blow, it makes landing one cost something |
| a move taken at its maximum | a charge spent to stop rolling and take what the dice could never beat - a domain's wrath, a metamagic that empowers a blast |
| `PushOnDamage` | damage of one type also shoves what it lands on: a thunderbolt that throws a Large or smaller creature back |
| `RoundsOrDamaged` | "for a minute, or until it takes any damage" - a turned Undead snapping out of it the moment anything hits it |
| a save `only` one type is caught by | turning the Undead, Hold Person's humanoid: anything else is not caught at all rather than saving against it |
| `Summon` with `Requirement` | a double called up beside its summoner, commanded out of its bonus action and spent in a burst |
| damage of either type | a blade that can deal cold "instead of the weapon's normal damage type" |
| `DamageThreshold` | an armoured shell or hull; a mouth or a crack as its weak spot |
| `Swallow`, `Digestion`, `Regurgitate` | a purple worm's, a behir's or a tarrasque's gullet |
| a `Reaction` move with a trigger | a snap at whoever is dragged into reach, a spray from a breached shell |
| `mouth`, `reach`, `tactic` | a whirlpool, a kraken or a dragon turtle its enemies circle, and how it swims at them |
| `ResistNonmagical`, `MagicWeapons` | "Bludgeoning, Piercing and Slashing from nonmagical attacks": halved unless the attack is a spell's, an enchanted weapon's, or made by something whose weapons all are |
| `MirrorDamage` | a mind that takes no psychic damage and hands it to every creature within ten feet instead |
| `SwapWhenHit` | a reaction that trades places with one of its own kind as the blow lands, which is hit in its stead |
| `PhaseThrough`, `NeverProvokes` | something that walks through its enemies, hurting each one it passes through, and that nothing gets a swing at for leaving |
| `Displace` | space folded around one creature: thrown somewhere else on a failed save, with a burst where it stood |
| `Hide`, `Hidden`, `Lit` | a Stealth check against passive Perception, allowed only in dim light or darkness; unseen until the first attack roll |
| `BonusDamageWithAdvantage`, `AllHit`, `EachInReach` | claws that hurt more on a roll with advantage, what a Multiattack adds when every blow lands on one target, one swing at each creature in reach |
| `InitiativeAdvantage` | an ambusher's two dice for its place in the order |
| `Chains` | a bolt that leaps from each creature that fails its save to the next one near it |
| `RideLine` | a caster who becomes a line of lightning and steps out at the far end of it |
| `Anchored`, `Hinders` | a whirlpool or a storm put down at a distance and left there: it drags at what starts a turn in it, and is difficult terrain |
| `HalfOnMiss`, `SparesTarget`, `TakesBonusAction` | an enchanted arrow: half its damage on a miss, a burst around the target that leaves the target out, cast and loosed as one action and bonus action |
| `AlsoCosts` | one cast paid out of two pools at once - an essence spell taken at its maximum for a charge |
| `SaveAdvantage`, `Shocked` | reflexes quick enough for advantage on one kind of save; a jolt that stops opportunity attacks |

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
cargo run --release -p crucible-cli -- scenarios/cleric-vs-brute.crucible
cargo run --release -p crucible-cli -- scenarios/two-clerics-vs-four-brutes.crucible
```

Each prints what it read — including traits and resource pools, since
transcription is the likeliest thing to be wrong — then a row per playstyle, the
same fight swept over how well the monster is run, and three replayable example
fights chosen by how the first side did: worst, median, best.

The `solver` row runs a fight inside every fight, so it gets its own sample count
(`--solver-n`, default 1000) and the table prints `n` per row rather than hiding
the difference in confidence.

### The grid

A scenario is played on a grid of five-foot squares, never smaller than 40 by
30 of them - 200 feet by 150 - and as big as `--map WxH` (or the web app's map
setting) asks. The two sides line up 30 feet apart (`--apart`) in the middle of
it. A creature covers `speed:` feet a turn (30 unless it says) and starts on
`at: x, y` if the scenario names a square. A move reaches `range N` feet - 5 for
a melee attack and 60 for anything else unless it says - and `area N` makes an
area a radius around its target rather than everything in range.

Everyone chooses its ground before it acts. Something that fights hand to hand
walks up to its target, and Dashes when nothing it has reaches. Something that
fights from range backs away instead: to the square furthest from its nearest
enemy that still keeps as many of them in range as it can - though not once an
enemy is already beside it. And whoever is holding up an emanation it cast - a
ring of spirits - walks to where the most enemies are inside it. An emanation
catches what it is carried over, and what walks into it, there and then, as
well as whoever starts a turn inside.

`pushed` and `pulled` move their victim ten feet on the grid, a rout runs it
its speed away, and a teleport puts its user somewhere else.

Walking out of an enemy's reach is an opening for it: one swing of its first
melee attack, for its reaction. Being shoved, routed or teleported is not, nor
is leaving with a Disengage. Both sides of that are decided by how a creature
is played. Whoever would have to take the blow stays and fights what it can
reach from where it stands - unless it is set on finishing one enemy
(focus-fire) or spending everything as it comes (nova), which go regardless,
or playing it safe and bloodied (defensive), which never goes. And whoever
could deal it weighs it against the best other use it has for a reaction - a
move of its own waiting on a trigger, at its damage and its `worth` - and
takes whichever is worth more; except that nova and in-order always swing, and
defensive, once bloodied, keeps its reaction for whatever it has that turns a
blow aside.

Something that walks through other creatures does so on purpose when it is closing to
strike: through as many enemies as its speed allows, a square somebody stands on costing
two, and out to a free one. What is thrown by folded space, or trades places with an ally,
provokes nothing either.

Light is where a creature stands, not a map of it: a trait on the creature, which a
scenario can override. It gates the moves that need the dark and nothing else - nobody is
blinded by darkness, and darkvision changes nothing.

An area save is worth, to whatever ranks moves, what it does to everyone it would catch -
not only to whoever it is aimed at.

A heal reaches as far as it says - a touch when it says nothing - and whoever means to
heal walks to its patient first. A heal that can go on the bonus action goes there, and
the action is kept for something else. Whoever holds an emanation closes on its target
until somebody is inside it, and raises it from where its target will be. Taking aim is
standing still. And something with a blow that cannot reach yet runs at its target
rather than casting a spell that would do nothing anybody here can count.

Who a creature goes for is whoever is holding a spell up, then whoever is nearest, then
whoever is nearest to falling - except for `focus-fire` (the most wounded), `scattered`
(a different one each) and `in-order` (as they are listed). A ranged attack is at
disadvantage with an enemy at its maker's elbow or at long range (`long N`), a prone
target is easier to hit only from beside it, and somebody standing in the way is half
cover. Something aiming from range stands clear of what its enemies are holding up and,
where it can, out of a knot with its own side.

Everything loaded from a file can Dodge, Disengage, Help, Shove and Grapple, each taken
only when its moment has come: Dodge by something playing it safe once it is bloodied
and beset, Disengage by something that fights from range and is held in a reach, Help
when the advantage is worth more to an ally than its own action is, Shove for the same
opening against a target that can be put on the ground, Grapple against something that
fights from a distance and can still walk away. `in-order` never reaches for any of them.

This is a first cut. Not on the grid yet: walls, objects and cover from them, flying,
cones, lines other than one its caster rides, buff ranges, the Ready action, and Sneak
Attack's "ally within 5 feet", which is still read off who is fighting what. A creature
with a `mouth` keeps its zones. `--no-grid` runs the old model, where everyone
reaches everyone, and the library runs that way whenever a creature has nowhere
to stand - which is what the exact-agreement tests rely on.

### Watching a fight

```bash
make            # builds the web app, starts the server; open http://127.0.0.1:8000
```

The library shows every scenario under `scenarios/` as a card: who, against
whom. Opening one runs it and gives a prediction - how often the party wins
under each playstyle, how long it takes and how many of them go down - with the
worst, median and best fight of each to watch.

A fight plays beat by beat. Initiative is rolled one creature at a time, each
joining the turn order as it does; whoever's turn it is lights up; and every
roll lands in a dice tray with what it had to beat - an attack against an armor
class, a saving throw against a DC - stamped hit, miss, saved or failed. On the
grid a spell is cast in a circle of light and a blade lunges, a miss is called
out, damage bursts in the colour of its type, healing rises green, and a potion
bubbles. Left and right step through it, space plays it, up and down move to the
next fight, escape goes back. The address keeps up, so
`/scenario?path=scenarios/cleric-vs-brute.crucible&fight=2&frame=5` reopens the same
moment.

The dice are the ones the fight actually rolled: a recorded fight reads them off
the generator as they fall, so nothing about how a fight resolves changes when
it is being watched.

The same page is where a fight is set up. **Creatures** lists every character
and monster on disk, by which of the two it is. A new one is made by pasting
whatever there is of it - a stat block out of a book, a character sheet, a few
lines - and a model writes its file, once: the loader checks what it wrote and
sends back what it refuses, and the draft arrives marked unreviewed with a list
of what the model left out or guessed. From there it is a sheet of fields with
the stat block it loads as beside it, changed by typing or by saying what to
change; the file itself is one tab away, for what the sheet has no field for.
**New scenario** puts creatures from that roster on two sides, however many of
each, and saves the result as a `.crucible` file - in a collection, which is a
folder under `scenarios/`, so a campaign is `local/its-name`. Nothing that
would not load or run is saved.

The model is the Claude Code command line, run without tools: drafting needs
`claude` installed and signed in (`CRUCIBLE_AGENT` names another program,
`CRUCIBLE_AGENT_MODEL` another model). It is asked only when a creature is
drafted or changed that way - never when a fight is run.

What you make is yours and stays on your machine. The repo commits one example
character, one example monster and two scenarios made of them, and nothing
else: everything the app creates goes under `content/local/` and
`scenarios/local/` unless you say otherwise, and both are gitignored. The app
marks each creature *private* or *example* and warns before saving anything
where it would be committed.

It is two parts. `crates/crucible-server` is a local server that reads and
writes those files, runs the simulations natively and serves the page; `web/` is the
page, a Svelte app built to static files (`make web` needs Node). `crucible
<scenario> --export replays.json` writes the same recorded fights to a file.

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

`make check` runs all of that and type-checks the web app. `make e2e` builds the
web app and drives it in a real browser from a Rust test
(`crates/crucible-server/tests/web.rs`): it needs Chrome or Edge, so it is not
part of `cargo test`. `make dev-web` serves the page with hot reload against a
server started with `make server`.

On Windows with the GNU toolchain, `proptest` reaches `windows-sys` through
`getrandom`, which needs mingw-w64 **binutils** on `PATH` — `rustup` ships a
linker but not `dlltool`. `winget install BrechtSanders.WinLibs.POSIX.MSVCRT`
supplies it.

## Content and Configuration

Creatures are formatted data configurations rather than hardcoded simulation logic:
- `content/characters/`: one example Player Character (`cleric.toml`), defining class, level, stats, resources, and known feature plugins.
- `content/monsters/`: one example Monster statblock (`brute.toml`).
- `scenarios/`: Encounters referencing combatants via `source:` (e.g. `source: content/characters/cleric.toml`).
- `content/local/characters/`, `content/local/monsters/`, `scenarios/local/`: your own, gitignored.

SRD material is CC-BY-4.0 and ships with attribution. Characters, monsters and scenarios of
your own - whatever they are drawn from - are loaded from local data files and never
committed: `content/local/` and `scenarios/local/` are gitignored for that. The Monad Plugin
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
failed save, and doubles it calls up and commands. `tests/storm_priest.rs` does it for a
divine caster: a pool of charges spent on a burst, on a dying ally or on turning the Undead,
a storm that answers whoever hits it in melee, a cast taken at its maximum for one of those
charges, spirits raised in a ring, and lightning that shoves what it hits.
`tests/warped_ambushers.rs` does it for creatures that are not quite where they seem: a hide
only magic bites, a walk through its enemies, places traded as a blow lands, space folded
around a victim, a mind that hands its hurt to whoever is near, and an ambush out of the dark.
`tests/storm_thrown_by_hand.rs` does it for a caster's harder shapes: a bolt that chains, a
line that is ridden, a whirlpool placed at a distance, and a thrown blade that bursts.
`dsl::scenario`'s module docs list the move and trait syntax.
