# Design

## The question

> Is this encounter actually difficult **for my party**?

Not "what is its CR." Not "what would a solved agent do." The 5e encounter
budget is a static formula over creature XP that ignores tactics, terrain,
action economy, party composition, and the resources the party actually has
left. It answers a question nobody asked.

The question a DM has at 11pm the night before a session is whether *these six
people, playing the way they actually play, with the slots they actually have
left*, are in trouble.

That reframes everything. The deliverable is not a number and not an optimal
line. It is a **profile across playstyles**.

## Why this is a game, not an optimisation problem

It is tempting to state this as "find the action sequence minimising rounds to
end the encounter." That framing fails three ways.

**It is not continuous.** Actions are discrete. There is no feasible region to
walk and no gradient to follow, so linear and convex programming do not apply
at the top level.

**It is not single-agent.** The monsters act, and adversarially. This is a
two-player zero-sum stochastic game with perfect information, not an MDP.
Perfect information is a gift: expectimax and MCTS with chance nodes are
sufficient, and the machinery for hidden information (CFR, belief states) is
not needed.

**"Minimise rounds" is a degenerate objective.** Optimise it literally and the
solver novas every slot on round one, ignores defence, and accepts a 40% chance
of a dead PC, because a corpse does not lengthen the fight. It produces advice
no table would follow.

### The objective

Maximise the probability the party wins, penalised by what it costs:

```
maximise   P(win) - lambda * (slots + charges + HP + consumables spent)
```

`lambda` is a Lagrange multiplier on the resource budget. Sweeping it does not
produce an answer, it produces a **Pareto frontier** - win probability against
resources spent - and the player picks a point on it. That is the honest
output, because "should I burn the 5th-level slot" has no context-free answer.

Aggregate rollouts by **CVaR** rather than expectation: the mean of the worst
10% of outcomes. Tables do not care about the average result, they care whether
this can go badly. It is a one-line change in how rollouts are reduced and it
matches how people actually reason about risk.

### The monsters must be modelled too

If the party plays well and the monsters play randomly, the resulting number
means nothing. Difficulty is the value of the game **under both sides playing
to a stated standard**, which is exactly why this is a game and not an MDP.

DMs do not play monsters optimally. So the monster policy is a parameter, and
the spread across monster skill is itself an output:

> Medium if you run the dragon like a brute. Deadly if you fly it and open with
> Frightful Presence.

That information exists nowhere in the DMG.

## Playstyles are the product

The party policy is not one thing to be solved for. It is a library, because
the question is about a specific table.

| policy | behaviour | models |
|---|---|---|
| `Solver` | MCTS to a depth budget | the ceiling - what was theoretically available |
| `Nova` | highest-level resources first, front-loaded | the table that ends fights in two rounds |
| `Attrition` | never spend a limited resource above a HP threshold | "I might need it later" - the most common real player |
| `FocusFire` | coordinate on lowest effective HP | a party that communicates |
| `Scattered` | each PC picks its own preferred target | a party that does not |
| `Defensive` | heal and disengage when bloodied | a spooked table |
| `Fitted` | parameters fit to logged actual play | **this specific table** |

The headline output is not a scalar:

```
                       P(win)   P(a PC dies)   slots spent   rounds
Solver                  0.97        0.04           1.2         3.1
Nova                    0.91        0.11           4.0         2.4
FocusFire               0.88        0.14           2.1         3.6
Attrition               0.52        0.44           0.3         6.8
```

**The gap between rows is the interesting metric.** If every policy wins, the
encounter is filler. If none do, it is a wall. If the spread is wide, it is a
skill check, and that is a genuinely different kind of encounter that the CR
system cannot express. Two axes fall out of this: difficulty (how hard under
best play) and punishment (how much worse play costs you).

`Fitted` is the endgame. Given logged sessions, fit the policy parameters to
what the table actually did, then report that row. That is the literal answer
to the question.

## Full 5e

This targets the full ruleset, not a reduced kernel. That decision has a direct
architectural consequence: **full 5e cannot be written as procedures.**
Hardcoding attack resolution means every feature that modifies it - Bless,
Sneak Attack, Lucky, Great Weapon Master, Sharpshooter, Elven Accuracy, cover,
half cover from a familiar - becomes a branch inside that procedure, and the
procedure becomes unmaintainable somewhere around feature thirty.

5e is structurally a large pile of **triggered modifiers over a small set of
events**. So the engine is an event pipeline, and abilities are handlers on it.

```
TurnStart -> ActionDeclared -> AttackRollBuilt -> AttackRolled
  -> HitDetermined -> DamageRolled -> DamageModified -> DamageApplied
  -> ConcentrationChecked -> Died -> TurnEnd
```

Every ability registers against a hook and returns a modification. Bless adds a
d4 at `AttackRollBuilt`. Sneak Attack adds dice at `DamageRolled` when its
condition holds. Resistance halves at `DamageModified`. Absorb Elements is a
reaction on `DamageApplied`. None of them touch the engine.

This is more work than a reduced kernel up front and dramatically less work per
ability afterwards, which is the right trade when the target is the whole game.
**Adding a spell must never be a commit to the engine.** If it is, the ability
DSL is missing something, and that is a bug in the DSL.

Consequences worth stating now, because they are expensive to retrofit:

- **Reactions break turn atomicity.** A turn is not a unit. Any event that
  offers a choice to any combatant is a decision node, so the search operates
  over decision points rather than turns.
- **Action economy is a first-class resource** - action, bonus action,
  reaction, movement, free interaction, plus legendary actions between turns
  and lair actions on initiative 20.
- **Conditions are a system**, not flags, with their exact interactions with
  attack rolls, saves, movement, and each other.
- **Concentration** couples incoming damage to a CON save and to effects
  ending, which means effects need ownership and lifetimes.

### Content and licensing

The SRD is CC-BY-4.0 and ships in-repo with attribution. Non-SRD material is
loaded from user-supplied data files and is never committed. That is both the
licensing answer and a useful forcing function: if a monster cannot be
expressed as data, the DSL is missing something.

## Why Rust

The workload is millions of node visits over small structs in an arena, with
root-parallel MCTS that `rayon` provides nearly free, and ablation multiplying
the rollout count by the ability count. That is the canonical profile for a
compiled language with no GC pauses.

Separately, the domain is algebraic. Actions, conditions, damage types, and the
ability AST are sum types, and exhaustive `match` means the compiler enumerates
every site a newly added condition must be handled. Without that, adding
Frightened is a search-and-hope.

## Verification

The engine is a probability calculator, so it can be checked against closed
forms rather than against itself.

For any scenario simple enough to solve analytically, the exact answer is
computable by convolution and dynamic programming, and the Monte Carlo path
must converge to it. `tests/exact_vs_sampled.rs` asserts exactly that: the
exact survival curve for a target under repeated attacks, against sampled
rollouts, compared at multiple points with a tolerance derived from the
standard error rather than from a number that happened to pass.

This is the same trick as a chess engine's perft suite. The simulator proves
itself before any of the interesting machinery exists, and stays honest
afterwards: the sampled path is the one that scales, the exact path is the one
that is obviously correct, and they are required to agree.

`proptest` then searches for counterexamples to the invariants - a PMF sums to
one, HP never goes below zero, a survival curve is monotone non-decreasing -
and shrinks any failure to a minimal case.

## Build order

1. **Dice and probability.** Exact PMF by convolution, sampling, and the d20
   outcome distribution under advantage and disadvantage. *(first slice)*
2. **Attack resolution, and the exact/sampled agreement test.** *(first slice)*
3. **Event pipeline and the ability DSL.** The spine everything hangs on.
4. **Evaluator.** Fixed policies, many rollouts, report the table above.
   Useful and testable before any search exists.
5. **Search.** MCTS as a drop-in replacement for one policy slot.
6. **Ablation.** Re-run with each ability disabled, diff win probability.
7. **Ingestion.** An LLM turns a pasted stat block into schema-valid JSON. It
   never does arithmetic; it fills in a schema, which is validated before
   anything is rolled. Last, because it is the least risky part and useless
   without something to feed.
