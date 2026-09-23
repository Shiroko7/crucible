//! The smallest pieces of the grammar: numbers, dice, damage lists.

use crate::rules::{DamageKind, DamageRoll};

/// A comma-separated list of damage types: `fire, cold`. Blank entries - from
/// a trailing comma - are dropped rather than rejected.
pub(super) fn parse_damage_kinds(text: &str, context: &str) -> Result<Vec<DamageKind>, String> {
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
pub(super) fn trailing_number(words: &[&str], context: &str) -> Result<i32, String> {
    words
        .iter()
        .rev()
        .find_map(|w| number(w).ok())
        .ok_or_else(|| format!("expected a bonus number in `{context}`"))
}

/// The `i`th whitespace-separated word of a clause, or a message naming it.
pub(super) fn arg<'a>(words: &[&'a str], i: usize, clause: &str) -> Result<&'a str, String> {
    words
        .get(i)
        .copied()
        .ok_or_else(|| format!("`{clause}` is missing a value"))
}

/// `NdS`, `NdS+B`, `NdS-B`, or a flat `B`.
pub(super) fn parse_dice(expr: &str) -> Result<(u32, u32, i32), String> {
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

/// `1d10+8 slashing, 2d4 fire` - or a flat `5 fire`, or `1d6+7 slashing or
/// cold` for damage its dealer can choose the type of per attack.
pub fn parse_damage(clause: &str) -> Result<Vec<DamageRoll>, String> {
    let mut out = Vec::new();
    for term in clause.split(',') {
        let mut words = term.split_whitespace();
        let (Some(expr), Some(kind_word)) = (words.next(), words.next()) else {
            return Err(format!("expected `2d6+3 fire`, got `{}`", term.trim()));
        };
        let kind = DamageKind::parse(kind_word)
            .ok_or_else(|| format!("unknown damage type `{kind_word}`"))?;
        // `... or <type>`: the same damage, dealt as whichever of the two
        // suits its dealer - see `DamageRoll::alternative`.
        let alternative = match words.next() {
            None => None,
            Some(w) if w.eq_ignore_ascii_case("or") => {
                let alt_word = words.next().ok_or_else(|| {
                    format!("`{}` says `or` without a second damage type", term.trim())
                })?;
                Some(
                    DamageKind::parse(alt_word)
                        .ok_or_else(|| format!("unknown damage type `{alt_word}`"))?,
                )
            }
            Some(_) => {
                return Err(format!(
                    "`{}` has more than a roll and a type - `2d6+3 fire`, or `2d6+3 fire or cold`",
                    term.trim()
                ))
            }
        };
        if words.next().is_some() {
            return Err(format!("unexpected words at the end of `{}`", term.trim()));
        }
        let (dice, sides, bonus) = parse_dice(expr)?;
        let roll = DamageRoll::new(dice, sides, bonus, kind);
        out.push(match alternative {
            Some(alt) => roll.or(alt),
            None => roll,
        });
    }
    Ok(out)
}

/// A signed integer, tolerating the leading `+` that stat blocks always write.
pub(crate) fn number(s: &str) -> Result<i32, String> {
    let s = s.trim();
    s.strip_prefix('+')
        .unwrap_or(s)
        .parse()
        .map_err(|_| format!("`{s}` is not a number"))
}

pub(crate) fn count(s: &str) -> Result<u32, String> {
    let s = s.trim();
    s.parse()
        .map_err(|_| format!("`{s}` is not a non-negative number"))
}
