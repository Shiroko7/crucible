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
//! trait: ac 2
//! trait: saves 1
//! trait: spell 1
//! trait: resistance fire, cold
//! action: Greatclub | strikes 1 | hit +6 | 2d8+4 bludgeoning
//! action: Staff | strikes 2 | hit +9 | 1d8+6 bludgeoning
//!              | on hit save con dc 16 stunned once cost focus 1
//! bonus: Patient Defense | cost focus 1 | stance dodging
//! action: Breath | uses 3 | save dex dc 16 | 2d8 cold | half on success
//!               && strikes 1 | hit +9 | 1d8+6 bludgeoning
//! ```
//!
//! `&&` joins a move out of several effects, for a Multiattack that is not all
//! the same attack. Move order is meaningful: it is what
//! [`crate::duel::Policy::InOrder`] reads.

use std::fmt;

use crate::rules::combat::{Reduction, RollMode};
use crate::rules::creature::{
    Ability, Condition, Cost, Creature, DamageKind, DamageRoll, Duration, Effect, Move, Resource,
    Rider, SaveEffect, Strike, Uses,
};

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
            let registry = crate::dsl::plugin::FeatureRegistry::new();
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
            "action" | "bonus" | "legendary" => {
                let m = parse_move(&value, current).map_err(fail)?;
                match key.as_str() {
                    "action" => current.actions.push(m),
                    "bonus" => current.bonus_actions.push(m),
                    _ => current.legendary.push(m),
                }
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
/// [`crate::duel::Policy::Scattered`] needs them to be distinguishable.
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

/// The direct effect of one `trait:` line.
///
/// Most traits so far - Evasion, Legendary Resistance, Deflect Attacks - are
/// [`Rider`]s: they only matter at a specific point the combat engine already
/// hooks (a save, a reaction). A magic item's passive stat boost is not that
/// shape - a flat bonus to AC, to every saving throw, or to a spell attack/DC
/// is just a number the engine already reads straight off the creature, so it
/// is applied directly rather than routed through a rider that would have
/// nowhere new to fire. Keeping both kinds behind one `TraitEffect` is what
/// lets `traits = [...]` stay one flat list of independent, composable
/// keyword phrases regardless of which shape a given trait turns out to be.
#[derive(Debug, Clone, PartialEq)]
pub enum TraitEffect {
    Rider(Rider),
    /// A flat, always-on bonus to Armor Class - distinct from
    /// [`Rider::ReactionOnTargeted`], which is a *reactive* AC bonus spent
    /// against one attack rather than always active.
    AcBonus(i32),
    /// A flat, always-on bonus to every saving throw.
    SavesBonus(i32),
    /// A flat, always-on bonus to spell attack rolls and spell save DC,
    /// added to the creature's existing [`crate::rules::creature::SpellCastingProfile::item_bonus`].
    SpellBonus(i32),
    /// Resistance to one or more damage types.
    Resistance(Vec<DamageKind>),
}

impl TraitEffect {
    /// Apply this effect directly onto `creature`.
    ///
    /// Every variant but [`TraitEffect::SpellBonus`] always succeeds: there is
    /// nothing to validate about adding a number to an AC, a save array, or a
    /// reduction list. A spell attack/DC bonus needs a caster to add itself
    /// to, and a creature with no `spellcasting` profile at all is the one
    /// case this cannot silently do something reasonable with, so it is
    /// reported rather than dropped.
    pub fn apply(self, creature: &mut Creature) -> Result<(), String> {
        match self {
            Self::Rider(rider) => creature.riders.push(rider),
            Self::AcBonus(n) => creature.ac += n,
            Self::SavesBonus(n) => {
                for save in creature.saves.iter_mut() {
                    *save += n;
                }
            }
            Self::SpellBonus(n) => {
                let profile = creature.spellcasting.as_mut().ok_or_else(|| {
                    "a spell attack/DC bonus trait needs this creature to already have a \
                     `spellcasting` profile"
                        .to_string()
                })?;
                profile.item_bonus += n;
            }
            Self::Resistance(kinds) => {
                for kind in kinds {
                    creature.reductions.push((kind, Reduction::Resistant));
                }
            }
        }
        Ok(())
    }
}

/// An always-on or reactive modifier, or a flat passive stat bonus.
pub fn parse_trait_external(value: &str) -> Result<TraitEffect, String> {
    parse_trait(value)
}

/// Parse a move string against a creature owner.
pub fn parse_move_external(value: &str, owner: &Creature) -> Result<Move, String> {
    parse_move(value, owner)
}

/// A comma-separated list of damage types: `fire, cold`. Blank entries - from
/// a trailing comma - are dropped rather than rejected.
fn parse_damage_kinds(text: &str, context: &str) -> Result<Vec<DamageKind>, String> {
    let mut kinds = Vec::new();
    for word in text.split(',') {
        let word = word.trim();
        if word.is_empty() {
            continue;
        }
        kinds.push(
            DamageKind::parse(word)
                .ok_or_else(|| format!("unknown damage type `{word}` in `{context}`"))?,
        );
    }
    Ok(kinds)
}

/// The first word, scanning from the end, that parses as a signed number -
/// tolerating a filler word in between, so `ac 2` and the more readable
/// `ac bonus 2` both work.
fn trailing_number(words: &[&str], context: &str) -> Result<i32, String> {
    words
        .iter()
        .rev()
        .find_map(|w| number(w).ok())
        .ok_or_else(|| format!("expected a bonus number in `{context}`"))
}

/// An always-on or reactive modifier, or a flat passive stat bonus.
fn parse_trait(value: &str) -> Result<TraitEffect, String> {
    let words: Vec<&str> = value.split_whitespace().collect();
    let head = words
        .first()
        .ok_or("a trait needs a name")?
        .to_ascii_lowercase();
    match head.as_str() {
        "evasion" => {
            let name = arg(&words, 1, value)?;
            let ability = Ability::parse(name)
                .ok_or_else(|| format!("unknown ability `{name}` in `{value}`"))?;
            Ok(TraitEffect::Rider(Rider::NothingOnSuccess { ability }))
        }
        // `legendary resistance 3`, or the mechanism's own name.
        "legendary" | "always" => {
            let n = words
                .iter()
                .rev()
                .find_map(|w| count(w).ok())
                .ok_or_else(|| format!("expected a number of uses in `{value}`"))?;
            Ok(TraitEffect::Rider(Rider::AlwaysSucceed { uses: n }))
        }
        // `deflect 1d10+7 bludgeoning, piercing, slashing [per round 2]`
        "deflect" | "reduce" => {
            let (dice, sides, bonus) = parse_dice(arg(&words, 1, value)?)?;
            let rest = value
                .split_once(arg(&words, 1, value)?)
                .map(|(_, r)| r)
                .unwrap_or("");
            let (kinds_text, per_round) = match rest.to_ascii_lowercase().find("per round") {
                Some(at) => {
                    let n = count(rest[at + "per round".len()..].trim())?;
                    (&rest[..at], n)
                }
                None => (rest, 1),
            };
            let kinds = parse_damage_kinds(kinds_text, value)?;
            if kinds.is_empty() {
                return Err(format!("`{value}` needs the damage types it applies to"));
            }
            Ok(TraitEffect::Rider(Rider::ReduceDamage {
                // The type on the reduction roll is never read; only its dice.
                roll: DamageRoll::new(dice, sides, bonus, DamageKind::Force),
                kinds,
                per_round,
            }))
        }
        // `ac 2`, or the more readable `ac bonus 2` - a passive item's flat
        // Armor Class bonus.
        "ac" => Ok(TraitEffect::AcBonus(trailing_number(&words, value)?)),
        // `saves 1`, or `saves bonus 1` - a passive item's flat bonus to
        // every saving throw, as opposed to one ability at a time the way
        // the stat block's own `saves:` table works.
        "saves" | "save" => Ok(TraitEffect::SavesBonus(trailing_number(&words, value)?)),
        // `spell 1`, or `spell bonus 1` - a passive item's flat bonus to
        // spell attack rolls and spell save DC, added to the caster's own
        // `SpellCastingProfile::item_bonus`.
        "spell" => Ok(TraitEffect::SpellBonus(trailing_number(&words, value)?)),
        // `resistance fire`, or `resistance fire, cold` - a passive item's
        // grant of resistance to one or more damage types, alongside
        // whatever the stat block's own `resist:` list already carries.
        "resistance" | "resist" => {
            let kinds = parse_damage_kinds(&words[1..].join(" "), value)?;
            if kinds.is_empty() {
                return Err(format!("`{value}` needs at least one damage type"));
            }
            Ok(TraitEffect::Resistance(kinds))
        }
        other => Err(format!("unknown trait `{other}`")),
    }
}

/// `Name | clause | ... && clause | ...`
///
/// Clauses are order-independent on purpose: a stat block does not put them in
/// a reliable order either.
fn parse_move(value: &str, owner: &Creature) -> Result<Move, String> {
    let mut segments = value.split("&&");
    let head = segments.next().unwrap_or("");
    let (name, mut built) = {
        let mut parts = head.split('|');
        let name = parts.next().unwrap_or("").trim().to_string();
        if name.is_empty() {
            return Err("a move needs a name before the first `|`".into());
        }
        (name, parse_body(parts, owner)?)
    };

    let mut effects = vec![built
        .effect
        .take()
        .ok_or_else(|| format!("`{name}` does nothing - give it damage, a save, or a stance"))?];
    for segment in segments {
        let tail = parse_body(segment.split('|'), owner)?;
        effects.push(
            tail.effect
                .ok_or_else(|| format!("the `&&` part of `{name}` does nothing"))?,
        );
        built.riders.extend(tail.riders);
    }

    Ok(Move {
        name,
        uses: built.uses,
        cost: built.cost,
        riders: built.riders,
        effect: if effects.len() == 1 {
            effects.pop().unwrap()
        } else {
            Effect::Sequence(effects)
        },
    })
}

#[derive(Default)]
struct Body {
    uses: Uses,
    cost: Option<Cost>,
    riders: Vec<Rider>,
    effect: Option<Effect>,
}

fn parse_body<'a>(
    clauses: impl Iterator<Item = &'a str>,
    owner: &Creature,
) -> Result<Body, String> {
    let mut to_hit: Option<i32> = None;
    let mut strikes: u32 = 1;
    let mut mode = RollMode::Normal;
    let mut save: Option<(Ability, i32)> = None;
    let mut half_on_success = false;
    let mut stance: Option<Condition> = None;
    let mut on_failure: Option<(Condition, Duration)> = None;
    let mut max_targets: Option<u32> = None;
    let mut damage: Vec<DamageRoll> = Vec::new();
    let mut out = Body::default();

    for clause in clauses {
        let clause = clause.trim();
        if clause.is_empty() {
            continue;
        }
        let words: Vec<&str> = clause.split_whitespace().collect();
        match words[0].to_ascii_lowercase().as_str() {
            "strikes" => strikes = count(arg(&words, 1, clause)?)?,
            "hit" => to_hit = Some(number(arg(&words, 1, clause)?)?),
            "recharge" => out.uses = Uses::Recharge(count(arg(&words, 1, clause)?)?),
            "uses" => out.uses = Uses::Limited(count(arg(&words, 1, clause)?)?),
            "advantage" => mode = RollMode::Advantage,
            "disadvantage" => mode = RollMode::Disadvantage,
            "half" => half_on_success = true,
            "targets" => max_targets = Some(count(arg(&words, 1, clause)?)?),
            "cost" => out.cost = Some(parse_cost(&words, 1, clause, owner)?),
            "stance" => {
                let name = arg(&words, 1, clause)?;
                stance = Some(
                    Condition::parse(name).ok_or_else(|| format!("unknown condition `{name}`"))?,
                );
            }
            "on" if arg(&words, 1, clause)?.eq_ignore_ascii_case("fail") => {
                on_failure = Some(parse_on_fail(&words, clause)?);
            }
            "on" => out.riders.push(parse_on_hit(&words, clause, owner)?),
            "save" => {
                let ability = Ability::parse(arg(&words, 1, clause)?)
                    .ok_or_else(|| format!("unknown ability in `{clause}`"))?;
                if !arg(&words, 2, clause)?.eq_ignore_ascii_case("dc") {
                    return Err(format!("expected `save <ability> dc <n>`, got `{clause}`"));
                }
                save = Some((ability, number(arg(&words, 3, clause)?)?));
            }
            _ => damage.extend(parse_damage(clause)?),
        }
    }

    out.effect = if let Some(condition) = stance {
        Some(Effect::Stance { condition })
    } else if let Some((ability, dc)) = save {
        Some(Effect::Save(SaveEffect {
            ability,
            dc,
            damage,
            half_on_success,
            on_failure,
            max_targets,
        }))
    } else if !damage.is_empty() {
        let to_hit = to_hit.ok_or("a damaging move needs a `hit +N` clause or a `save`")?;
        Some(Effect::Strikes {
            strike: Strike {
                to_hit,
                mode,
                damage,
            },
            count: strikes,
        })
    } else {
        None
    };
    Ok(out)
}

/// `on fail prone [until victim]`, the condition half of a saving throw.
fn parse_on_fail(words: &[&str], clause: &str) -> Result<(Condition, Duration), String> {
    let name = arg(words, 2, clause)?;
    let condition = Condition::parse(name).ok_or_else(|| format!("unknown condition `{name}`"))?;
    let duration = match words.get(3).map(|w| w.to_ascii_lowercase()) {
        None => Duration::ApplierTurn,
        Some(w) if w == "until" => match arg(words, 4, clause)?.to_ascii_lowercase().as_str() {
            "victim" | "theirs" | "their" => Duration::VictimTurn,
            "applier" | "mine" | "my" => Duration::ApplierTurn,
            other => return Err(format!("`until {other}` is not a duration")),
        },
        Some(other) => return Err(format!("unexpected `{other}` in `{clause}`")),
    };
    Ok((condition, duration))
}

/// `on hit save con dc 16 stunned [once] [cost focus 1] [until victim]`
fn parse_on_hit(words: &[&str], clause: &str, owner: &Creature) -> Result<Rider, String> {
    if !arg(words, 1, clause)?.eq_ignore_ascii_case("hit") {
        return Err(format!("expected `on hit ...`, got `{clause}`"));
    }
    if !arg(words, 2, clause)?.eq_ignore_ascii_case("save") {
        return Err(format!("expected `on hit save ...`, got `{clause}`"));
    }
    let ability = Ability::parse(arg(words, 3, clause)?)
        .ok_or_else(|| format!("unknown ability in `{clause}`"))?;
    if !arg(words, 4, clause)?.eq_ignore_ascii_case("dc") {
        return Err(format!("expected `save <ability> dc <n>` in `{clause}`"));
    }
    let dc = number(arg(words, 5, clause)?)?;
    let name = arg(words, 6, clause)?;
    let condition = Condition::parse(name).ok_or_else(|| format!("unknown condition `{name}`"))?;

    let mut once_per_turn = false;
    let mut cost = None;
    let mut duration = Duration::ApplierTurn;
    let mut i = 7;
    while i < words.len() {
        match words[i].to_ascii_lowercase().as_str() {
            "once" => once_per_turn = true,
            "cost" => {
                cost = Some(parse_cost(words, i + 1, clause, owner)?);
                i += 2;
            }
            "until" => {
                let who = arg(words, i + 1, clause)?;
                duration = match who.to_ascii_lowercase().as_str() {
                    "victim" | "theirs" | "their" => Duration::VictimTurn,
                    "applier" | "mine" | "my" => Duration::ApplierTurn,
                    other => return Err(format!("`until {other}` is not a duration")),
                };
                i += 1;
            }
            // Words like "per" and "turn" in "once per turn" read as noise.
            "per" | "turn" => {}
            other => return Err(format!("unexpected `{other}` in `{clause}`")),
        }
        i += 1;
    }

    Ok(Rider::SaveOrCondition {
        ability,
        dc,
        condition,
        duration,
        cost,
        once_per_turn,
    })
}

fn parse_cost(words: &[&str], at: usize, clause: &str, owner: &Creature) -> Result<Cost, String> {
    let name = arg(words, at, clause)?;
    let amount = count(arg(words, at + 1, clause)?)?;
    let resource = owner.resource_index(name).ok_or_else(|| {
        format!("`{name}` is not a declared resource - add a `resource: {name} N` line above")
    })?;
    Ok(Cost { resource, amount })
}

/// The `i`th whitespace-separated word of a clause, or a message naming it.
fn arg<'a>(words: &[&'a str], i: usize, clause: &str) -> Result<&'a str, String> {
    words
        .get(i)
        .copied()
        .ok_or_else(|| format!("`{clause}` is missing a value"))
}

/// `NdS`, `NdS+B`, `NdS-B`, or a flat `B`.
fn parse_dice(expr: &str) -> Result<(u32, u32, i32), String> {
    match expr.split_once('d') {
        Some((n, rest)) => {
            let dice = count(n)?;
            let (sides, bonus) = match rest.find(['+', '-']) {
                Some(i) => (count(&rest[..i])?, number(&rest[i..])?),
                None => (count(rest)?, 0),
            };
            if sides == 0 {
                return Err(format!("`{expr}` has a zero-sided die"));
            }
            Ok((dice, sides, bonus))
        }
        // A flat amount, which `0d1 + n` expresses without a special case.
        None => Ok((0, 1, number(expr)?)),
    }
}

/// `1d10+8 slashing, 2d4 fire` - or a flat `5 fire`.
fn parse_damage(clause: &str) -> Result<Vec<DamageRoll>, String> {
    let mut out = Vec::new();
    for term in clause.split(',') {
        let mut words = term.split_whitespace();
        let (Some(expr), Some(kind_word)) = (words.next(), words.next()) else {
            return Err(format!("expected `2d6+3 fire`, got `{}`", term.trim()));
        };
        if words.next().is_some() {
            return Err(format!("`{}` has more than a roll and a type", term.trim()));
        }
        let kind = DamageKind::parse(kind_word)
            .ok_or_else(|| format!("unknown damage type `{kind_word}`"))?;
        let (dice, sides, bonus) = parse_dice(expr)?;
        out.push(DamageRoll::new(dice, sides, bonus, kind));
    }
    Ok(out)
}

/// A signed integer, tolerating the leading `+` that stat blocks always write.
fn number(s: &str) -> Result<i32, String> {
    let s = s.trim();
    s.strip_prefix('+')
        .unwrap_or(s)
        .parse()
        .map_err(|_| format!("`{s}` is not a number"))
}

fn count(s: &str) -> Result<u32, String> {
    let s = s.trim();
    s.parse()
        .map_err(|_| format!("`{s}` is not a non-negative number"))
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// The case the multi-component damage model exists for.
    #[test]
    fn a_strike_can_deal_two_damage_types_at_once() {
        let text = "creature: x\nhp: 1\naction: Rend | hit +14 | 1d10+8 slashing, 2d4 fire\n";
        let c = &parse(text).unwrap()[0];
        let Effect::Strikes { strike, .. } = &c.actions[0].effect else {
            panic!("expected strikes")
        };
        assert_eq!(
            strike.damage,
            vec![
                DamageRoll::new(1, 10, 8, DamageKind::Slashing),
                DamageRoll::new(2, 4, 0, DamageKind::Fire),
            ]
        );
    }

    #[test]
    fn a_breath_weapon_parses_as_a_save() {
        let text = "creature: x\nhp: 1\naction: Fire Breath | recharge 5 | save dex dc 21 | 17d6 fire | half on success\n";
        let m = &parse(text).unwrap()[0].actions[0];
        assert_eq!(m.uses, Uses::Recharge(5));
        match &m.effect {
            Effect::Save(save) => {
                assert_eq!((save.ability, save.dc), (Ability::Dex, 21));
                assert!(save.half_on_success);
                assert_eq!(
                    save.damage,
                    vec![DamageRoll::new(17, 6, 0, DamageKind::Fire)]
                );
            }
            other => panic!("expected a save, got {other:?}"),
        }
    }

    #[test]
    fn traits_become_riders() {
        let text = "
creature: x
hp: 10
trait: evasion dex
trait: legendary resistance 3
trait: deflect 1d10+7 bludgeoning, piercing, slashing
";
        let c = &parse(text).unwrap()[0];
        assert!(c.has_evasion(Ability::Dex));
        assert!(!c.has_evasion(Ability::Con));
        assert!(c
            .riders
            .iter()
            .any(|r| matches!(r, Rider::AlwaysSucceed { uses: 3 })));
        let deflect = c
            .riders
            .iter()
            .find_map(|r| match r {
                Rider::ReduceDamage {
                    kinds,
                    roll,
                    per_round,
                } => Some((kinds, roll, per_round)),
                _ => None,
            })
            .expect("deflect parsed");
        assert_eq!(deflect.0.len(), 3);
        assert_eq!(
            (deflect.1.count, deflect.1.sides, deflect.1.bonus),
            (1, 10, 7)
        );
        assert_eq!(*deflect.2, 1);
    }

    /// The four generic item-passive traits ARCH-05's spellcasting profile and
    /// the existing AC/saves/resistance fields make possible: none of them
    /// are riders, and a creature can carry any subset of them at once,
    /// exactly the way real items grant different subsets of these.
    #[test]
    fn stat_boost_traits_stack_directly_onto_the_creature_and_combine() {
        let text = "
creature: x
hp: 10
ac: 14
saves: str +1, dex +1, con +1, int +1, wis +1, cha +1
resist: cold
trait: ac 2
trait: ac bonus 1
trait: saves 1
trait: resistance fire, radiant
";
        let c = &parse(text).unwrap()[0];
        // Two separate AC-granting items stack: 14 base + 2 + 1.
        assert_eq!(c.ac, 17);
        // The flat saves trait adds on top of the stat block's own +1 to
        // every ability, not just one.
        for ability in [
            Ability::Str,
            Ability::Dex,
            Ability::Con,
            Ability::Int,
            Ability::Wis,
            Ability::Cha,
        ] {
            assert_eq!(c.save(ability), 2, "{ability:?} should be +1 base, +1 item");
        }
        // The trait-granted resistances sit alongside the stat block's own
        // `resist:` list rather than replacing it.
        assert_eq!(c.reduction(DamageKind::Cold), Reduction::Resistant);
        assert_eq!(c.reduction(DamageKind::Fire), Reduction::Resistant);
        assert_eq!(c.reduction(DamageKind::Radiant), Reduction::Resistant);
        assert_eq!(c.reduction(DamageKind::Acid), Reduction::Normal);
    }

    /// A spell attack/DC bonus needs an existing spellcasting profile to add
    /// itself to - there is nothing reasonable to do with "add 2 to a spell
    /// attack bonus" on a creature that does not cast spells, so this is
    /// reported rather than silently dropped.
    #[test]
    fn a_spell_bonus_trait_without_a_spellcasting_profile_is_an_error() {
        let text = "
creature: x
hp: 10
trait: spell 2
";
        let err = parse(text).expect_err("no spellcasting profile to add the bonus to");
        assert!(err.message.contains("spellcasting"), "{}", err.message);
    }

    /// A malformed or unrecognised trait keyword is a parse error rather than
    /// something silently ignored.
    #[test]
    fn an_unknown_trait_keyword_is_rejected() {
        let text = "
creature: x
hp: 10
trait: flight 60
";
        assert!(parse(text).is_err());
    }

    /// A monk's turn, which is what forced resource pools and on-hit riders to
    /// exist. Also checks that a line ending in `|` continues.
    #[test]
    fn a_monk_turn_parses_including_its_pool_and_riders() {
        let text = "
creature: Gio
hp: 69
ac: 20
resource: focus 8
action: Staff x2 | strikes 2 | hit +9 | 1d8+6 bludgeoning |
        on hit save con dc 16 stunned once per turn cost focus 1
bonus: Flurry of Blows | cost focus 1 | strikes 2 | hit +7 | 1d8+4 force
bonus: Patient Defense | cost focus 1 | stance dodging
";
        let c = &parse(text).unwrap()[0];
        assert_eq!(c.resources[0].name, "focus");
        assert_eq!(c.resources[0].max, 8);

        let staff = &c.actions[0];
        assert!(staff.cost.is_none(), "the staff itself is free");
        match &staff.riders[0] {
            Rider::SaveOrCondition {
                ability,
                dc,
                condition,
                duration,
                cost,
                once_per_turn,
            } => {
                assert_eq!((*ability, *dc), (Ability::Con, 16));
                assert_eq!(*condition, Condition::Stunned);
                assert_eq!(*duration, Duration::ApplierTurn);
                assert_eq!(
                    *cost,
                    Some(Cost {
                        resource: 0,
                        amount: 1
                    })
                );
                assert!(*once_per_turn);
            }
            other => panic!("expected a save-or-condition rider, got {other:?}"),
        }

        assert_eq!(
            c.bonus_actions[0].cost,
            Some(Cost {
                resource: 0,
                amount: 1
            })
        );
        assert!(!c.bonus_actions[0].is_free());
        assert_eq!(
            c.bonus_actions[1].effect,
            Effect::Stance {
                condition: Condition::Dodging
            }
        );
    }

    /// `&&` is how a Multiattack that is not all the same attack gets written.
    #[test]
    fn a_move_can_be_built_from_several_effects() {
        let text = "
creature: x
hp: 10
action: Breath and a swing | uses 3 | save dex dc 16 | 2d8 cold | half on success
                          && strikes 1 | hit +9 | 1d8+6 bludgeoning
";
        let m = &parse(text).unwrap()[0].actions[0];
        assert_eq!(m.uses, Uses::Limited(3));
        let Effect::Sequence(parts) = &m.effect else {
            panic!("expected a sequence, got {:?}", m.effect)
        };
        assert_eq!(parts.len(), 2);
        assert!(matches!(parts[0], Effect::Save(_)));
        assert!(matches!(parts[1], Effect::Strikes { count: 1, .. }));
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
