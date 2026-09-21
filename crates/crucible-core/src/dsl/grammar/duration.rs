//! How long a condition lasts: `until victim`, `until applier`, `until end`,
//! `until save`, or `for N rounds|minutes|hours`.

use crate::dsl::grammar::count;
use crate::dsl::grammar::lex::arg;
use crate::rules::{Ability, Duration};

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
/// - `for N round(s)|minute(s)|hour(s)`: [`Duration::Rounds`], a minute being
///   ten rounds.
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
                other => return Err(format!("`until {other}` is not a duration")),
            };
            Ok((spec, 2))
        }
        Some(w) if w == "for" => {
            let n = count(arg(words, at + 1, clause)?)?;
            let unit = arg(words, at + 2, clause)?.to_ascii_lowercase();
            let rounds = match unit.trim_end_matches('s') {
                "round" => n,
                "minute" => n * 10,
                "hour" => n * 600,
                other => return Err(format!("`{other}` is not a unit of time in `{clause}`")),
            };
            Ok((DurationSpec::Fixed(Duration::Rounds(rounds)), 3))
        }
        _ => Ok((DurationSpec::Fixed(Duration::ApplierTurn), 0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::Effect;
    use crate::creature::Rider;
    use crate::dsl::scenario::parse;

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
