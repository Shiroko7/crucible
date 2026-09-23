//! How long a condition lasts: `until victim`, `until applier`, `until end`,
//! `until save`, or `for N rounds|minutes|hours`.

use crate::dsl::grammar::count;
use crate::dsl::grammar::lex::arg;
use crate::rules::{Ability, Duration};

/// How many rounds a minute of game time is, which is the clock every "for 1
/// minute" effect is written against.
const A_MINUTE: u32 = 10;

/// A duration as written, before the save it might refer back to is known -
/// `until save` repeats "the" save, and in a move whose clauses come in any
/// order that save may not have been read yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurationSpec {
    Fixed(Duration),
    /// Repeat the triggering save at the end of each of the victim's turns.
    UntilSave,
}

impl DurationSpec {
    /// Pin down `until save` against the save it repeats, if there is one.
    pub fn resolve(self, save: Option<(Ability, i32)>, context: &str) -> Result<Duration, String> {
        match self {
            DurationSpec::Fixed(d) => Ok(d),
            DurationSpec::UntilSave => match save {
                Some((ability, dc)) => Ok(Duration::SaveEndTurn { ability, dc }),
                None => Err(format!("`until save` in `{context}` has no save to repeat")),
            },
        }
    }
}

/// A condition's lifetime starting at word `at`, and how many words it took:
///
/// - nothing at all: [`Duration::ApplierTurn`];
/// - `until victim|theirs|their`: [`Duration::VictimTurn`];
/// - `until applier|mine|my`: [`Duration::ApplierTurn`];
/// - `until end`: [`Duration::ApplierNextTurnEnd`];
/// - `until save`: [`Duration::SaveEndTurn`], against the triggering save;
/// - `until damaged`: [`Duration::RoundsOrDamaged`] for a minute - a turned
///   Undead snapping out of it the moment anything hits it;
/// - `for N round(s)|minute(s)|hour(s)`: [`Duration::Rounds`], a minute being
///   ten rounds - followed by `or damaged` for the same clock with the early
///   exit ([`Duration::RoundsOrDamaged`]).
///
/// Anything else is left for the caller, which is why the word count comes
/// back rather than the rest being rejected here.
pub fn parse_duration(
    words: &[&str],
    at: usize,
    clause: &str,
) -> Result<(DurationSpec, usize), String> {
    match words.get(at).map(|w| w.to_ascii_lowercase()) {
        Some(w) if w == "until" => {
            let who = arg(words, at + 1, clause)?.to_ascii_lowercase();
            let spec = match who.as_str() {
                "victim" | "theirs" | "their" => DurationSpec::Fixed(Duration::VictimTurn),
                "applier" | "mine" | "my" => DurationSpec::Fixed(Duration::ApplierTurn),
                "end" => DurationSpec::Fixed(Duration::ApplierNextTurnEnd),
                "save" => DurationSpec::UntilSave,
                // "for 1 minute, or until it takes any damage", written as
                // its short half: the minute is implied.
                "damaged" | "damage" => DurationSpec::Fixed(Duration::RoundsOrDamaged(A_MINUTE)),
                other => return Err(format!("`until {other}` is not a duration")),
            };
            Ok((spec, 2))
        }
        Some(w) if w == "for" => {
            let n = count(arg(words, at + 1, clause)?)?;
            let unit = arg(words, at + 2, clause)?.to_ascii_lowercase();
            let rounds = match unit.trim_end_matches('s') {
                "round" => n,
                "minute" => n * A_MINUTE,
                "hour" => n * 60 * A_MINUTE,
                other => return Err(format!("`{other}` is not a unit of time in `{clause}`")),
            };
            // `for 1 minute or damaged` / `... or until damaged`: the same
            // clock, ended early by any damage its holder takes.
            let tail: Vec<String> = words[at + 3..]
                .iter()
                .take(3)
                .map(|w| w.to_ascii_lowercase())
                .collect();
            let ends_on_damage = match tail.iter().map(String::as_str).collect::<Vec<_>>()[..] {
                ["or", "damaged", ..] | ["or", "damage", ..] => Some(2),
                ["or", "until", "damaged"] | ["or", "until", "damage"] => Some(3),
                _ => None,
            };
            Ok(match ends_on_damage {
                Some(used) => (
                    DurationSpec::Fixed(Duration::RoundsOrDamaged(rounds)),
                    3 + used,
                ),
                None => (DurationSpec::Fixed(Duration::Rounds(rounds)), 3),
            })
        }
        _ => Ok((DurationSpec::Fixed(Duration::ApplierTurn), 0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::{Effect, Rider};
    use crate::dsl::scenario::parse;

    /// "For 1 minute, or until it takes any damage" - written either way
    /// round, and on either kind of clause.
    #[test]
    fn a_condition_can_end_on_damage_taken() {
        let text = "
creature: x
hp: 20
action: A | save wis dc 15 | 0 force | on fail frightened until damaged
action: B | save wis dc 15 | 0 force | on fail incapacitated for 2 rounds or damaged
action: C | save wis dc 15 | 0 force | on fail prone for 1 minute or until damaged
action: D | hit +5 | 1d6 fire | on hit save con dc 12 stunned until damaged
";
        let c = &parse(text).unwrap()[0];
        let fail = |i: usize| match &c.actions[i].effect {
            Effect::Save(save) => save.on_failure[0].1,
            other => panic!("expected a save, got {other:?}"),
        };
        assert_eq!(fail(0), Duration::RoundsOrDamaged(10));
        assert_eq!(fail(1), Duration::RoundsOrDamaged(2));
        assert_eq!(fail(2), Duration::RoundsOrDamaged(10));
        let Rider::SaveOrCondition { duration, .. } = &c.actions[3].riders[0] else {
            panic!("expected a save-or-condition rider");
        };
        assert_eq!(*duration, Duration::RoundsOrDamaged(10));

        // The plain clock still reads as itself.
        let plain = &parse(
            "creature: x\nhp: 4\naction: E | save wis dc 15 | 0 force | on fail prone for 3 rounds\n",
        )
        .unwrap()[0];
        let Effect::Save(save) = &plain.actions[0].effect else {
            unreachable!()
        };
        assert_eq!(save.on_failure[0].1, Duration::Rounds(3));
    }

    /// `until save` refers back to the move's own save wherever the clauses
    /// fall, and every other duration phrase reads as written.
    #[test]
    fn durations_parse_in_every_form() {
        let text = "
creature: x
hp: 20
resource: ki 3
action: A | on fail stunned until save | save con dc 14 | 0 force
action: B | save wis dc 13 | 0 force | on fail suppressed for 1 minute
action: C | save wis dc 13 | 0 force | on fail prone until end
action: D | hit +5 | 1d6 bludgeoning | on hit save con dc 12 poisoned until save
action: E | hit +5 | 1d6 bludgeoning | on hit save con dc 12 stunned once cost ki 1 for 2 rounds
";
        let c = &parse(text).unwrap()[0];
        let fail = |i: usize| match &c.actions[i].effect {
            Effect::Save(save) => save.on_failure[0].1,
            other => panic!("expected a save, got {other:?}"),
        };
        assert_eq!(
            fail(0),
            Duration::SaveEndTurn {
                ability: Ability::Con,
                dc: 14
            }
        );
        assert_eq!(fail(1), Duration::Rounds(10));
        assert_eq!(fail(2), Duration::ApplierNextTurnEnd);
        let rider = |i: usize| match &c.actions[i].riders[0] {
            Rider::SaveOrCondition { duration, .. } => *duration,
            other => panic!("expected a save-or-condition rider, got {other:?}"),
        };
        assert_eq!(
            rider(3),
            Duration::SaveEndTurn {
                ability: Ability::Con,
                dc: 12
            }
        );
        assert_eq!(rider(4), Duration::Rounds(2));

        let orphan = parse("creature: x\nhp: 4\naction: X | hit +5 | 1d6 fire | stance dodging\naction: Y | stance prone | on fail prone until save\n");
        assert!(orphan.is_err(), "`until save` with no save to repeat");
        let bad_unit = parse_duration(&["for", "2", "fortnights"], 0, "for 2 fortnights");
        assert!(bad_unit.is_err());
    }
}
