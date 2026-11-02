//! Drafting a creature from whatever somebody has: a stat block copied out
//! of a book, a character sheet, a paragraph - or an instruction to change
//! the one already there.
//!
//! The model writes the file; the loader says whether it loads; and when it
//! does not, the model is told why and asked again. What comes back is a
//! draft for a person to check against their source, never something saved
//! on its own.

use crucible_core::features::FeatureRegistry;
use serde::{Deserialize, Serialize};

use crate::agent::Agent;
use crate::creatures::{card, Card};

/// How many times the model is asked before its last answer is handed back
/// with what is wrong with it.
const ATTEMPTS: u32 = 3;

/// The most text a draft is made from: a few pages of a book.
const MAX_SOURCE: usize = 40_000;

const CHARACTER: &str = include_str!("../../../content/characters/cleric.toml");
const MONSTER: &str = include_str!("../../../content/monsters/brute.toml");

/// The move grammar as its own parser documents it: the comment that file
/// opens with, so what the model is told can never fall behind the parser.
fn grammar() -> String {
    include_str!("../../crucible-core/src/dsl/scenario.rs")
        .lines()
        .map_while(|line| line.strip_prefix("//!"))
        .map(|line| line.strip_prefix(' ').unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// What the model is told it is doing.
fn instructions() -> String {
    format!(
        r#"You convert tabletop RPG (D&D 5e) creatures into the TOML file format of a combat simulator called crucible. You are given a stat block, a character sheet or a description - in any layout, possibly messy - and you write the one file that describes that creature for combat.

# How to answer

Answer with exactly one fenced ```toml block holding the whole file, and after it zero or more lines that each start with `NOTE: `, one for each thing in the source that matters in a fight and that you left out, simplified or had to guess. Say nothing else.

# The file

A file holds one table: `[pc]` for a player character, `[monster]` for anything else.

Fields of both: `name` (required), `ac` (required), `hp` (required), `initiative` (the modifier, not a roll), `speed` (walking, in feet), `resist`, `immune`, `vulnerable` (lists of damage types), `condition_immune` (list of conditions), `traits` (list of trait phrases, see the grammar).
Only `[pc]`: `class`, `subclass`, `species`, `level`, `equipment` (list of names, descriptive only).
Only `[monster]`: `minions` (creatures it can raise, each a stat block like its own, written `[[monster.minions]]`), `cr`, `size` (Tiny, Small, Medium, Large, Huge, Gargantuan), `creature_type` (Aberration, Beast, Celestial, Construct, Dragon, Elemental, Fey, Fiend, Giant, Humanoid, Monstrosity, Ooze, Plant, Undead), `legendary_uses`.

Sub-tables, written `[pc.saves]` or `[monster.saves]` and so on:
- `saves`: the total saving throw bonus for each of `str`, `dex`, `con`, `int`, `wis`, `cha` - the ability modifier, plus proficiency where the creature is proficient.
- `abilities` (`[pc]` only): the raw ability scores.
- `spellcasting`: `ability` ("wis"), `ability_modifier`, `proficiency_bonus`, for a creature that casts spells.
- `resources`: named pools, `focus = 8`; and `resources.slots`: spell slots by level, `1 = 4`.

Lists of moves, each entry a `name` and an `effect`: `[[pc.actions]]`, `[[pc.bonus]]`, `[[pc.reactions]]`, `[[pc.auras]]`, and the same under `monster`, plus `[[monster.legendary]]`.

An `effect` is written in the move grammar below. The grammar is documented for an older line format, `action: Name | phrase | phrase`; in TOML the same move is `name = "Name"` and `effect = "phrase | phrase"`. Likewise `bonus:` is an entry of `bonus`, `reaction:` of `reactions` (its effect must carry a `when ...` clause), `legendary:` of `legendary`, `aura:` of `auras`, `trait: X` is the string "X" in `traits`, and `resource: focus 8` is `focus = 8` under `resources`.

`[[pc.features]]` or `[[monster.features]]` entries name something the simulator already implements, by `plugin = "id"`. The ids that exist are:
{plugins}
Use one only when the source names that exact spell or feature. Give it no other settings unless an example below shows them. For everything else, write the move out with the grammar.

# Rules

- Copy numbers from the source: attack bonuses, save DCs, damage dice, hit points. Do not recompute them, and do not invent any that are missing - leave the field out and say so in a NOTE.
- Use only phrases the grammar documents. If something cannot be written with them, leave it out and add a NOTE; never make up syntax.
- A Multiattack becomes one move: `strikes N` for N of the same attack, or effects joined by `&&` for different ones. Do not also list the single attack as a separate action unless the creature would choose it on its own.
- A hit that deals several kinds of damage lists them with commas: `1d6+4 piercing, 1d6 poison`.
- Order the actions from the one it should prefer to the one it should fall back on.
- Things that do not matter in a fight - skills, languages, senses, lore - are left out without a NOTE.
- Keep any comment lines the file you are given already opens with.

# The move grammar

{grammar}

# Example: a character

```toml
{CHARACTER}```

# Example: a monster

```toml
{MONSTER}```
"#,
        plugins = FeatureRegistry::new().ids().join(", "),
        grammar = grammar(),
    )
}

/// What a draft is made from.
#[derive(Debug, Deserialize)]
pub(crate) struct Request {
    /// The pasted text, or what to change.
    source: String,
    /// `character` or `monster`, when the person has said which.
    #[serde(default)]
    kind: Option<String>,
    /// The file being changed, when this is a change to one.
    #[serde(default)]
    current: Option<String>,
}

/// What the model made of it.
#[derive(Debug, Serialize)]
pub(crate) struct Drafted {
    /// The file - its last attempt at one, when none of them loads.
    text: String,
    card: Option<Card>,
    /// Why it does not load, when it does not.
    error: Option<String>,
    /// What the model says it left out or guessed.
    notes: Vec<String>,
    /// How many times it was asked.
    attempts: u32,
}

/// The file in an answer, and the notes after it.
fn answer(said: &str) -> (String, Vec<String>) {
    let file = said
        .split_once("```toml")
        .or_else(|| said.split_once("```"))
        .and_then(|(_, rest)| rest.split_once("```"))
        .map_or(said, |(file, _)| file);
    let notes = said
        .lines()
        .filter_map(|line| line.trim().strip_prefix("NOTE:"))
        .map(|note| note.trim().to_string())
        .filter(|note| !note.is_empty())
        .collect();
    (format!("{}\n", file.trim()), notes)
}

const UNREVIEWED: &str =
    "# Drafted by a model from pasted text - not yet checked against its source.";

pub(crate) fn draft(agent: &dyn Agent, request: &Request) -> Result<Drafted, String> {
    let source = request.source.trim();
    if source.is_empty() {
        return Err("there is nothing to make a creature from - paste or describe one".to_string());
    }
    if source.len() > MAX_SOURCE {
        return Err(format!(
            "that is {} characters, and a creature is drafted from {MAX_SOURCE} at most - \
             paste only its own pages",
            source.len()
        ));
    }
    let current = request.current.as_deref().filter(|c| !c.trim().is_empty());
    let mut prompt = match current {
        Some(file) => format!(
            "Here is a creature's file:\n\n```toml\n{file}```\n\nChange it as follows, keeping \
             everything else as it is, and answer with the whole file:\n\n{source}\n"
        ),
        None => {
            let which = match request.kind.as_deref() {
                Some("character") => "It is a player character: write a `[pc]`.\n\n",
                Some("monster") => "It is a monster: write a `[monster]`.\n\n",
                _ => "",
            };
            format!("{which}Write the file for this creature:\n\n{source}\n")
        }
    };
    let system = instructions();
    let asked = prompt.clone();
    let mut last = None;
    for attempt in 1..=ATTEMPTS {
        let (mut text, notes) = answer(&agent.ask(&system, &prompt)?);
        if current.is_none() && !text.starts_with(UNREVIEWED) {
            text = format!("{UNREVIEWED}\n\n{text}");
        }
        match card(&text) {
            Ok(card) => {
                return Ok(Drafted {
                    text,
                    card: Some(card),
                    error: None,
                    notes,
                    attempts: attempt,
                })
            }
            Err(error) => {
                prompt = format!(
                    "{asked}\nYour last answer was this file:\n\n```toml\n{text}```\n\nThe \
                     simulator refused it: {error}\n\nAnswer again with the whole file, fixed. \
                     If what it refused cannot be written in the grammar, leave it out and add \
                     a NOTE.\n"
                );
                last = Some(Drafted {
                    text,
                    card: None,
                    error: Some(error),
                    notes,
                    attempts: attempt,
                });
            }
        }
    }
    last.ok_or_else(|| "the model was never asked".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A model that says what it is told to, and keeps what it was asked.
    pub(crate) struct Scripted {
        answers: Mutex<Vec<String>>,
        pub(crate) asked: Mutex<Vec<(String, String)>>,
    }

    impl Scripted {
        pub(crate) fn new(answers: &[&str]) -> Self {
            Self {
                answers: Mutex::new(answers.iter().rev().map(|a| a.to_string()).collect()),
                asked: Mutex::new(Vec::new()),
            }
        }
    }

    impl Agent for Scripted {
        fn ask(&self, system: &str, prompt: &str) -> Result<String, String> {
            self.asked
                .lock()
                .unwrap()
                .push((system.to_string(), prompt.to_string()));
            self.answers
                .lock()
                .unwrap()
                .pop()
                .ok_or_else(|| "nothing left to say".to_string())
        }
    }

    const WOLF: &str = "Here it is.\n\n```toml\n[monster]\nname = \"Wolf\"\nsize = \"Medium\"\n\
        creature_type = \"Beast\"\nac = 12\nhp = 11\n\n[[monster.actions]]\nname = \"Bite\"\n\
        effect = \"strikes 1 | hit +4 | 1d6+2 piercing | on hit save str dc 11 prone\"\n```\n\
        NOTE: Pack Tactics is left out: nothing in the grammar gives advantage for an ally nearby.\n";

    fn request(source: &str, current: Option<&str>) -> Request {
        Request {
            source: source.to_string(),
            kind: Some("monster".to_string()),
            current: current.map(str::to_string),
        }
    }

    #[test]
    fn the_model_is_told_the_format_as_the_parser_documents_it() {
        let system = instructions();
        // The grammar, out of the parser's own file.
        assert!(system.contains("half on success"), "{system}");
        assert!(!system.contains("use crate::"), "only the comment");
        // The features that exist, and the two examples the repo commits.
        assert!(system.contains("spirit_guardians"));
        // What it is told beyond the grammar has to be true of the loader.
        let two_kinds = "[monster]
name = \"Asp\"
ac = 12
hp = 9

[[monster.actions]]
            name = \"Bite\"
effect = \"strikes 1 | hit +7 | 1d6+4 piercing, 1d6 poison\"
";
        assert!(system.contains("1d6+4 piercing, 1d6 poison"));
        card(two_kinds).expect("damage of two kinds loads");
        assert!(system.contains("name = \"Cleric\"") && system.contains("name = \"Brute\""));
    }

    #[test]
    fn a_pasted_stat_block_comes_back_as_a_file_that_loads() {
        let model = Scripted::new(&[WOLF]);
        let drafted = draft(
            &model,
            &request("Wolf. AC 12, HP 11. Bite +4, 1d6+2.", None),
        )
        .unwrap();
        assert_eq!(drafted.attempts, 1);
        assert_eq!(drafted.error, None);
        assert_eq!(drafted.card.unwrap().hp, 11);
        // Marked as nobody's work yet, and with what the model owned up to.
        assert!(drafted.text.starts_with(UNREVIEWED), "{}", drafted.text);
        assert!(drafted.text.contains("[monster]\nname = \"Wolf\""));
        assert_eq!(drafted.notes.len(), 1);
        assert!(drafted.notes[0].starts_with("Pack Tactics"));
        let asked = model.asked.lock().unwrap();
        assert!(asked[0].1.contains("write a `[monster]`"));
        assert!(asked[0].1.contains("Bite +4, 1d6+2."));
    }

    #[test]
    fn a_file_that_does_not_load_is_sent_back_with_why() {
        let broken = WOLF.replace("piercing", "toothy");
        let model = Scripted::new(&[&broken, WOLF]);
        let drafted = draft(&model, &request("a wolf", None)).unwrap();
        assert_eq!(drafted.attempts, 2);
        assert_eq!(drafted.error, None);
        let asked = model.asked.lock().unwrap();
        assert!(asked[1].1.contains("refused it"), "{}", asked[1].1);
        assert!(asked[1].1.contains("toothy"), "{}", asked[1].1);
        assert!(asked[1].1.contains("a wolf"), "the request is still there");
    }

    #[test]
    fn a_model_that_never_gets_it_right_hands_back_its_last_try_and_why() {
        let broken = WOLF.replace("piercing", "toothy");
        let model = Scripted::new(&[&broken, &broken, &broken, WOLF]);
        let drafted = draft(&model, &request("a wolf", None)).unwrap();
        assert_eq!(drafted.attempts, ATTEMPTS);
        assert!(drafted.card.is_none());
        assert!(drafted.error.unwrap().contains("toothy"));
        assert!(drafted.text.contains("toothy"));
    }

    #[test]
    fn a_change_is_asked_for_against_the_file_as_it_stands() {
        let model = Scripted::new(&[&WOLF.replace("hp = 11", "hp = 30")]);
        let current = "# Mine.\n[monster]\nname = \"Wolf\"\nac = 12\nhp = 11\n";
        let drafted = draft(&model, &request("make it tougher", Some(current))).unwrap();
        assert_eq!(drafted.card.unwrap().hp, 30);
        assert!(
            !drafted.text.contains("Drafted by a model"),
            "it was somebody's already"
        );
        let asked = model.asked.lock().unwrap();
        assert!(asked[0].1.contains("# Mine.") && asked[0].1.contains("make it tougher"));
    }

    #[test]
    fn nothing_and_too_much_are_both_refused_before_the_model_is_asked() {
        let model = Scripted::new(&[]);
        assert!(draft(&model, &request("  ", None))
            .unwrap_err()
            .contains("nothing"));
        let book = "x".repeat(MAX_SOURCE + 1);
        assert!(draft(&model, &request(&book, None))
            .unwrap_err()
            .contains("at most"));
        assert!(model.asked.lock().unwrap().is_empty());
    }
}
