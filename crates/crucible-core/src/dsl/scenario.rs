//! A text format for combatants, and a parser for it.
//!
//! Two reasons this exists rather than a pair of hardcoded constants.
//!
//! The licensing one: non-SRD stat blocks cannot be committed, so the tool has
//! to be able to read a creature it has never seen from a file the user owns.
//! The design one is the forcing function `DESIGN.md` describes - if a creature
//! cannot be written down as data, something is missing, and finding that out
//! now is cheaper than finding it out after the ability DSL is built. Writing
//! a level-8 monk in this format is what turned up the need for shared resource
//! pools, on-hit riders and conditions with lifetimes.
//!
//! This is still not that DSL: no triggers of its own, no arithmetic, no
//! references between abilities. It will be replaced.
//!
//! ```text
//! creature: Ogre
//! ac: 11
//! hp: 68
//! initiative: -1
//! saves: str +4, dex -1, con +3, int -3, wis -2, cha -2
//! immune: fire
//! resource: focus 8
//! trait: evasion dex
//! trait: legendary resistance 3
//! trait: deflect 1d10+7 bludgeoning, piercing, slashing
//! trait: halve attack damage
//! trait: extra damage applies to spell attacks
//! trait: reaction ac 5 vs ranged weapon
//! trait: bonus 3d6 piercing vs dragon
//! trait: ac 2
//! trait: saves 1
//! trait: spell 1
//! trait: resistance fire, cold
//! trait: downgrade immunity poison damage, poisoned condition
//! trait: empower weapon 2d6 poison on poisoned
//! trait: injury poison con dc 13 poisoned disadvantage str for 1 hour
//! trait: once per turn 1d6 piercing
//! trait: quarry 1d6 force
//! trait: always succeed dex 3 reaction
//! trait: reaction ac 4 vs melee until next turn
//! condition immune: poisoned
//! action: Greatclub | strikes 1 | hit +6 | 2d8+4 bludgeoning
//! action: Longbow | ranged | hit +8 | 1d8+4 piercing | bonus 3d6 piercing vs dragon
//! action: Shortsword | weapon shortsword | finesse | hit +10 | 1d6+6 slashing
//!                    | on hit vexed until end
//! action: Icebrand | weapon icebrand | hit +11 | 1d6+7 slashing or cold
//! action: Guiding Bolt | spell | slot 1 | ranged | hit +7 | 4d6 radiant
//! action: Hold | spell | slot 2 | concentration | save wis dc 15 | on fail paralyzed until save
//! bonus: Potion of Healing | object | cost potions 1 | heal 2d4+2
//! action: Staff | strikes 2 | hit +9 | 1d8+6 bludgeoning
//!              | on hit save con dc 16 stunned once cost focus 1
//! bonus: Patient Defense | cost focus 1 | stance dodging
//! action: Breath | uses 3 | save dex dc 16 | 2d8 cold | half on success
//!               && strikes 1 | hit +9 | 1d8+6 bludgeoning
//! trait: damage threshold 30 cracks weak spot resists bludgeoning, piercing
//! trait: digest 6d6 acid
//! trait: regurgitate 30 con dc 21
//! condition immune: prone, frightened
//! action: Bite | hit +9 | 3d8+5 piercing | on hit swallow large
//!              && stance exposed
//! bonus: Clamp Shut | stance sealed
//! legendary: Squeeze | points 2 | swallowed | 4d10 bludgeoning
//! reaction: Snap | when enemy pulled | hit +9 | 3d8+5 piercing
//! reaction: Spray | when breached | save dex dc 18 | 4d10 piercing | half on success
//! aura: Undertow | save str dc 15 | on fail pulled
//! trait: mouth
//! trait: difficult terrain
//! tactic: hit and run
//! action: Maw | reach mouth | hit +9 | 3d8+5 piercing
//!             && reach near | strikes 2 | hit +9 | 2d8+5 bludgeoning
//!             | on hit save str dc 17 prone and pushed until victim
//! action: Gush | recharge 5 | reach front | save con dc 17 | 8d8 cold | on fail pushed
//! legendary: Chill | reach near | save con dc 17 | 4d6 cold | on fail slowed for 1 round
//! ```
//!
//! `&&` joins a move out of several effects, for a Multiattack that is not all
//! the same attack. Move order is meaningful: it is what
//! [`crate::sim::Policy::InOrder`] reads.
//!
//! A strike is a melee weapon attack unless its move says otherwise: `ranged`,
//! `finesse`, `spell` (a spell attack; also marks the move as casting a spell),
//! and `weapon` (a weapon attack made as part of a spell, alongside `spell`).
//! `weapon <name>` also names the blade the swing is made with, which is what
//! an enchantment laid on one weapon rides (see [`crate::creature::Boon`]).
//! `item` and `object` mark a move as activating a magic item or using an
//! object. Damage written `1d6+7 slashing or cold` is dealt as whichever of
//! the two its target reduces least, chosen per attack.
//!
//! A condition's lifetime, after `on fail` or an `on hit save`, is
//! `until victim`, `until applier` (the default), `until end` (the end of the
//! applier's next turn), `until save` (repeat the save at the end of each of
//! the victim's turns), or `for N rounds|minutes|hours`. `on hit <condition>
//! [lifetime]`, with no `save`, lands one with nothing to resist it - what a
//! weapon mastery like Vex does (`on hit vexed until end`: advantage on that
//! attacker's own next roll against the target).
//!
//! A shell and a gullet: `damage threshold N` ignores any single instance of
//! damage below N, and a weak spot skips it - reached from inside, or by an
//! attack roll while the creature is `exposed` and not `sealed`, or while a
//! breach has left it `cracked`. `on hit swallow <size>` swallows a target
//! that size or smaller; `digest` is what it takes each turn inside, a
//! `swallowed` move hurts everything inside on demand, and `regurgitate` is
//! the damage from inside in one turn that forces a save to keep it down.
//! `points N` is what a legendary action costs. A `reaction:` is a move with a
//! `when enemy <condition>` or `when breached` clause, aimed at whoever set it
//! off; an `aura:` is a move every enemy is subject to at the start of its
//! turns, aimed at that enemy alone.
//!
//! A creature with a `mouth` has its enemies stand around it - at its mouth,
//! beside its body, at range or far off in front (see
//! [`crate::creature::Zone`]). Its moves reach only what `reach mouth`,
//! `reach near` or `reach front` names (anywhere, without one), its mouth is
//! its weak spot, open unless `sealed`, and it swims by its `tactic` - `hold`,
//! `charge` or `hit and run`. `difficult terrain` slows its enemies to one
//! place a turn; `pulled` and `pushed` move them a place in or out, and
//! `slowed` halves their moves. An on-hit save can land several conditions:
//! `prone and pushed`.
//!
//! Some mechanisms are only reachable through a feature plugin, because they
//! need more than a phrase: a mark maintained by concentration that a rider
//! pays out against (`hunters_mark`), a lasting boon a creature puts on
//! itself (`lasting_boon` - extra damage on its hits, resistances, or both,
//! for a number of rounds), and a double it calls up and then commands
//! (`commanded_double`). See [`crate::features`].

use crate::creature::{Creature, Resource, Tactic};
use crate::dsl::grammar::{count, number, parse_move, parse_reaction, parse_trait};
use crate::rules::{Ability, Condition, DamageKind, Reduction};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

/// Parse a whole scenario. Creatures come back in the order they were written.
///
/// A line ending in `|` or `&&` continues onto the next, so a monk's action can
/// be written over several lines without becoming unreadable.
pub fn parse(text: &str) -> Result<Vec<Creature>, ParseError> {
    let mut creatures: Vec<Creature> = Vec::new();
    let mut pending_team: Option<u8> = None;

    for (line_no, line) in logical_lines(text) {
        let fail = |message: String| ParseError {
            line: line_no,
            message,
        };

        let Some((key, value)) = line.split_once(':') else {
            return Err(fail(format!("expected `key: value`, got `{line}`")));
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim().to_string();

        if key == "team" && creatures.is_empty()
            || (key == "team"
                && pending_team.is_none()
                && creatures.last().is_some_and(|c| c.hp > 0))
        {
            let team_val = match value.to_ascii_lowercase().as_str() {
                "a" | "0" | "party" => 0,
                "b" | "1" | "monsters" => 1,
                other => return Err(fail(format!("`{other}` is not a team - use a or b"))),
            };
            pending_team = Some(team_val);
            continue;
        }

        if key == "source" || key == "import" || key == "file" {
            let registry = crate::features::FeatureRegistry::new();
            let mut creature = crate::dsl::config::load_creature_from_file(&value, &registry)
                .map_err(|e| fail(format!("failed to load creature from '{value}': {e}")))?;
            if let Some(team) = pending_team.take() {
                creature.team = team;
            }
            creatures.push(creature);
            continue;
        }

        if key == "creature" {
            if value.is_empty() {
                return Err(fail("a creature needs a name".into()));
            }
            let mut c = Creature::new(value, 0, 0);
            if let Some(team) = pending_team.take() {
                c.team = team;
            }
            creatures.push(c);
            continue;
        }

        let Some(current) = creatures.last_mut() else {
            return Err(fail(format!(
                "`{key}` before any `creature:` or `source:` line"
            )));
        };

        match key.as_str() {
            "ac" => current.ac = number(&value).map_err(fail)?,
            "hp" => current.hp = number(&value).map_err(fail)?,
            "initiative" | "init" => current.initiative = number(&value).map_err(fail)?,
            "legendary uses" => current.legendary_uses = count(&value).map_err(fail)?,
            "team" => {
                current.team = match value.to_ascii_lowercase().as_str() {
                    "a" | "0" | "party" => 0,
                    "b" | "1" | "monsters" => 1,
                    other => return Err(fail(format!("`{other}` is not a team - use a or b"))),
                }
            }
            "count" => {
                let n = count(&value).map_err(fail)?;
                if n == 0 {
                    return Err(fail("a count of zero leaves nothing to fight".into()));
                }
                current.count = n;
            }
            "saves" => {
                for entry in value.split(',') {
                    let mut words = entry.split_whitespace();
                    let (Some(name), Some(bonus)) = (words.next(), words.next()) else {
                        return Err(fail(format!("expected `str +3` in saves, got `{entry}`")));
                    };
                    let ability = Ability::parse(name)
                        .ok_or_else(|| fail(format!("unknown ability `{name}`")))?;
                    current.saves[ability.index()] = number(bonus).map_err(fail)?;
                }
            }
            "immune" | "resist" | "vulnerable" => {
                let reduction = match key.as_str() {
                    "immune" => Reduction::Immune,
                    "resist" => Reduction::Resistant,
                    _ => Reduction::Vulnerable,
                };
                for word in value.split(',') {
                    let word = word.trim();
                    let kind = DamageKind::parse(word)
                        .ok_or_else(|| fail(format!("unknown damage type `{word}`")))?;
                    current.reductions.push((kind, reduction));
                }
            }
            "condition immune" => {
                for word in value.split(',') {
                    let word = word.trim();
                    let condition = Condition::parse(word)
                        .ok_or_else(|| fail(format!("unknown condition `{word}`")))?;
                    current.condition_immunities.push(condition);
                }
            }
            "resource" => {
                let mut words = value.split_whitespace();
                let (Some(name), Some(max)) = (words.next(), words.next()) else {
                    return Err(fail(format!("expected `resource: focus 8`, got `{value}`")));
                };
                let max = count(max).map_err(fail)?;
                current.resources.push(Resource {
                    name: name.to_string(),
                    max,
                });
            }
            "trait" => {
                let effect = parse_trait(&value).map_err(fail)?;
                effect.apply(current).map_err(fail)?;
            }
            "action" | "bonus" | "legendary" | "aura" => {
                let m = parse_move(&value, current).map_err(fail)?;
                match key.as_str() {
                    "action" => current.actions.push(m),
                    "bonus" => current.bonus_actions.push(m),
                    "aura" => current.auras.push(m),
                    _ => current.legendary.push(m),
                }
            }
            "reaction" => {
                let r = parse_reaction(&value, current).map_err(fail)?;
                current.reactions.push(r);
            }
            "tactic" => {
                current.tactic = Tactic::parse(&value).ok_or_else(|| {
                    fail(format!(
                        "`{value}` is not a tactic - use hold, charge or hit and run"
                    ))
                })?;
            }
            other => return Err(fail(format!("unknown key `{other}`"))),
        }
    }

    for (i, c) in creatures.iter().enumerate() {
        if c.hp <= 0 {
            return Err(ParseError {
                line: 0,
                message: format!("creature {} (`{}`) needs a positive `hp`", i + 1, c.name),
            });
        }
    }

    // A file that never mentions teams reads as the first creature against
    // everything else, which is what every one-against-one scenario meant before
    // teams existed.
    if creatures.iter().all(|c| c.team == 0) && creatures.len() > 1 {
        for c in creatures.iter_mut().skip(1) {
            c.team = 1;
        }
    }
    Ok(expand(creatures))
}

/// Turn `count: 4` into four separately numbered creatures.
///
/// Numbered because the narration is unreadable otherwise, and because
/// [`crate::sim::Policy::Scattered`] needs them to be distinguishable.
fn expand(creatures: Vec<Creature>) -> Vec<Creature> {
    let mut out = Vec::new();
    for c in creatures {
        let n = c.count.max(1);
        for i in 0..n {
            let mut copy = c.clone();
            copy.count = 1;
            if n > 1 {
                copy.name = format!("{} {}", c.name, i + 1);
            }
            out.push(copy);
        }
    }
    out
}

/// Strip comments, drop blanks, and glue continuation lines together. The line
/// number reported is the one the logical line started on.
fn logical_lines(text: &str) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        // Either end of the join may carry the marker: a line may trail off
        // with `|`, or the next may pick up with `&&`. Both read naturally, so
        // both work.
        let picks_up = line.starts_with("&&") || line.starts_with('|');
        let trails_off = out
            .last()
            .is_some_and(|(_, acc)| acc.ends_with('|') || acc.ends_with("&&"));
        match out.last_mut() {
            Some((_, acc)) if picks_up || trails_off => {
                acc.push(' ');
                acc.push_str(line);
            }
            _ => out.push((n + 1, line.to_string())),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::Effect;
    use crate::rules::DamageRoll;

    #[test]
    fn a_whole_creature_round_trips_into_the_right_numbers() {
        let text = "
# a comment, and a blank line above
creature: Ogre
ac: 11
hp: 68
initiative: -1
saves: str +4, dex -1, con +3
action: Greatclub | strikes 1 | hit +6 | 2d8+4 bludgeoning
";
        let c = &parse(text).expect("parses")[0];
        assert_eq!(c.name, "Ogre");
        assert_eq!((c.ac, c.hp, c.initiative), (11, 68, -1));
        assert_eq!(c.save(Ability::Str), 4);
        assert_eq!(c.save(Ability::Cha), 0, "unset saves stay at zero");
        match &c.actions[0].effect {
            Effect::Strikes { strike, count } => {
                assert_eq!(*count, 1);
                assert_eq!(strike.to_hit, 6);
                assert_eq!(
                    strike.damage,
                    vec![DamageRoll::new(2, 8, 4, DamageKind::Bludgeoning)]
                );
            }
            other => panic!("expected strikes, got {other:?}"),
        }
    }

    /// Errors carry a line number, because a scenario file is hand-written and
    /// "parse error" with no location is the least useful message there is.
    #[test]
    fn mistakes_are_reported_where_they_happen() {
        let missing_creature = parse("ac: 10\n").unwrap_err();
        assert_eq!(missing_creature.line, 1);

        let bad_type =
            parse("creature: x\nhp: 4\naction: Hit | hit +2 | 1d6 sparkly\n").unwrap_err();
        assert_eq!(bad_type.line, 3);
        assert!(bad_type.message.contains("sparkly"), "{bad_type}");

        let no_hit = parse("creature: x\nhp: 4\naction: Hit | 1d6 fire\n").unwrap_err();
        assert!(no_hit.message.contains("hit +N"), "{no_hit}");

        let no_hp = parse("creature: x\nac: 4\n").unwrap_err();
        assert!(no_hp.message.contains("positive `hp`"), "{no_hp}");

        // Spending from a pool nobody declared is the easy mistake to make.
        let no_pool = parse("creature: x\nhp: 4\nbonus: Flurry | cost focus 1 | stance dodging\n")
            .unwrap_err();
        assert!(
            no_pool.message.contains("not a declared resource"),
            "{no_pool}"
        );

        let empty_move = parse("creature: x\nhp: 4\naction: Ponder | uses 2\n").unwrap_err();
        assert!(empty_move.message.contains("does nothing"), "{empty_move}");
    }

    #[test]
    fn scenarios_can_reference_external_config_sources() {
        let text = "
team: a
source: content/characters/gio.toml

team: b
source: content/monsters/adult-red-dragon.toml
";
        let combatants = parse(text).expect("parses external config sources");
        assert_eq!(combatants.len(), 2);
        assert_eq!(combatants[0].name, "Gio");
        assert_eq!(combatants[0].team, 0);
        assert_eq!(combatants[1].name, "Adult Red Dragon");
        assert_eq!(combatants[1].team, 1);
    }
}
