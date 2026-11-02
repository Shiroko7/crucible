//! A creature file as fields, and fields as a creature file.
//!
//! The file on disk is the TOML the engine reads. The app shows it as a
//! sheet to fill in, so it has to go both ways: read into the one table the
//! file holds, and written back out of it. Whatever the sheet has no field
//! for - a feature's own settings, a key added by hand - rides along in the
//! table untouched.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A creature file taken apart.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct Form {
    /// `character` for a `[pc]`, `monster` for a `[monster]`.
    kind: String,
    /// The comment the file opens with, a line a line.
    #[serde(default)]
    notes: String,
    /// Everything under `[pc]` or `[monster]`.
    table: Value,
}

/// Whether a line of TOML carries a comment: a `#` outside any string.
fn commented(line: &str) -> bool {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for c in line.chars() {
        match quote {
            Some(q) => {
                if escaped {
                    escaped = false;
                } else if c == '\\' && q == '"' {
                    escaped = true;
                } else if c == q {
                    quote = None;
                }
            }
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '#' => return true,
            None => {}
        }
    }
    false
}

/// The comment a file opens with, and how many lines further down carry one
/// - the ones a rewrite from the sheet cannot keep.
fn comments(text: &str) -> (String, usize) {
    let mut notes: Vec<&str> = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.peek().map(|l| l.trim()) {
        if let Some(note) = line.strip_prefix('#') {
            notes.push(note.strip_prefix(' ').unwrap_or(note));
        } else if !line.is_empty() {
            break;
        } else if !notes.is_empty() {
            notes.push("");
        }
        lines.next();
    }
    while notes.last() == Some(&"") {
        notes.pop();
    }
    (notes.join("\n"), lines.filter(|l| commented(l)).count())
}

/// A creature file as a sheet, and how many comments inside it the sheet
/// has nowhere to keep.
pub(crate) fn read(text: &str) -> Result<(Form, usize), String> {
    let root: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
    let (kind, table) = match (root.get("pc"), root.get("monster")) {
        (Some(table), _) => ("character", table),
        (None, Some(table)) => ("monster", table),
        (None, None) => return Err("expected a [pc] or [monster] table".to_string()),
    };
    let table = serde_json::to_value(table).map_err(|e| e.to_string())?;
    let (notes, lost) = comments(text);
    Ok((
        Form {
            kind: kind.to_string(),
            notes,
            table,
        },
        lost,
    ))
}

/// The order a file is written in: what a stat block opens with, then its
/// lists, the way a person would write one.
const FIELDS: &[&str] = &[
    "name",
    "class",
    "subclass",
    "species",
    "level",
    "cr",
    "size",
    "creature_type",
    "ac",
    "hp",
    "initiative",
    "speed",
    "resist",
    "immune",
    "vulnerable",
    "condition_immune",
    "legendary_uses",
    "tactic",
    "equipment",
    "traits",
];
const TABLES: &[&str] = &["abilities", "saves", "spellcasting", "resources"];
const LISTS: &[&str] = &[
    "features",
    "actions",
    "bonus",
    "reactions",
    "legendary",
    "auras",
];
const ABILITIES: &[&str] = &["str", "dex", "con", "int", "wis", "cha"];

/// `keys`, with the ones `first` names in front and in that order.
fn ordered<'a>(map: &'a Map<String, Value>, first: &[&str]) -> Vec<&'a String> {
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort_by_key(|key| {
        let at = first.iter().position(|f| f == key);
        (at.is_none(), at, key.to_string())
    });
    keys
}

/// Nothing to write: a field left empty on the sheet.
fn blank(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(s) => s.trim().is_empty(),
        Value::Array(items) => items.iter().all(blank),
        Value::Object(map) => map.values().all(blank),
        _ => false,
    }
}

fn key(name: &str) -> String {
    let bare = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if bare {
        name.to_string()
    } else {
        toml::Value::String(name.to_string()).to_string()
    }
}

/// A value written on one line.
fn inline(value: &Value) -> Result<String, String> {
    Ok(match value {
        Value::Null => return Err("an empty value has no place in a file".to_string()),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => match (n.as_i64(), n.as_f64()) {
            (Some(whole), _) => whole.to_string(),
            (None, Some(float)) => toml::Value::Float(float).to_string(),
            (None, None) => return Err(format!("`{n}` is not a number a file can hold")),
        },
        Value::String(s) => toml::Value::String(s.clone()).to_string(),
        Value::Array(items) => {
            let items: Result<Vec<String>, String> =
                items.iter().filter(|i| !blank(i)).map(inline).collect();
            format!("[{}]", items?.join(", "))
        }
        Value::Object(map) => {
            let mut fields = Vec::new();
            for name in ordered(map, &[]) {
                if !blank(&map[name]) {
                    fields.push(format!("{} = {}", key(name), inline(&map[name])?));
                }
            }
            format!("{{ {} }}", fields.join(", "))
        }
    })
}

/// A list of short things, one a line when there are several.
fn listed(name: &str, items: &[Value]) -> Result<String, String> {
    let items: Result<Vec<String>, String> =
        items.iter().filter(|i| !blank(i)).map(inline).collect();
    let items = items?;
    let one_line = format!("{} = [{}]\n", key(name), items.join(", "));
    if items.len() < 2 || one_line.len() <= 72 {
        return Ok(one_line);
    }
    let mut out = format!("{} = [\n", key(name));
    for item in items {
        out.push_str(&format!("    {item},\n"));
    }
    out.push_str("]\n");
    Ok(out)
}

/// A table under its own heading: its plain values, then the tables inside
/// it under headings of their own.
fn section(
    out: &mut String,
    path: &str,
    map: &Map<String, Value>,
    first: &[&str],
) -> Result<(), String> {
    let (inner, plain): (Vec<&String>, Vec<&String>) = ordered(map, first)
        .into_iter()
        .filter(|name| !blank(&map[*name]))
        .partition(|name| map[*name].is_object());
    if !plain.is_empty() {
        out.push_str(&format!("\n[{path}]\n"));
        for name in plain {
            out.push_str(&format!("{} = {}\n", key(name), inline(&map[name])?));
        }
    }
    for name in inner {
        if let Value::Object(table) = &map[name] {
            section(out, &format!("{path}.{}", key(name)), table, &[])?;
        }
    }
    Ok(())
}

/// The file a sheet is saved as.
pub(crate) fn write(form: &Form) -> Result<String, String> {
    let head = match form.kind.as_str() {
        "character" => "pc",
        "monster" => "monster",
        other => return Err(format!("`{other}` is neither a character nor a monster")),
    };
    let Value::Object(table) = &form.table else {
        return Err("a creature is a table of fields".to_string());
    };
    let mut out = String::new();
    for line in form.notes.trim().lines() {
        out.push_str(format!("# {line}").trim_end());
        out.push('\n');
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&format!("[{head}]\n"));

    let mut tables = Vec::new();
    let mut lists = Vec::new();
    for name in ordered(table, FIELDS) {
        let value = &table[name];
        if blank(value) {
            continue;
        }
        match value {
            Value::Object(_) => tables.push(name),
            Value::Array(items) if items.iter().all(Value::is_object) => lists.push(name),
            Value::Array(items) => out.push_str(&listed(name, items)?),
            _ => out.push_str(&format!("{} = {}\n", key(name), inline(value)?)),
        }
    }
    tables.sort_by_key(|name| {
        TABLES
            .iter()
            .position(|t| t == name)
            .unwrap_or(TABLES.len())
    });
    for name in tables {
        if let Value::Object(map) = &table[name] {
            section(&mut out, &format!("{head}.{}", key(name)), map, ABILITIES)?;
        }
    }
    lists.sort_by_key(|name| LISTS.iter().position(|l| l == name).unwrap_or(LISTS.len()));
    for name in lists {
        let Value::Array(items) = &table[name] else {
            continue;
        };
        for item in items.iter().filter(|i| !blank(i)) {
            let Value::Object(map) = item else { continue };
            out.push_str(&format!("\n[[{head}.{}]]\n", key(name)));
            for field in ordered(map, &["plugin", "name", "effect"]) {
                if !blank(&map[field]) {
                    out.push_str(&format!("{} = {}\n", key(field), inline(&map[field])?));
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creatures::card;

    const SHEET: &str = r#"# A knight of the old road.
# Sworn, and tired of it.

[pc]
name = "Knight"
class = "Fighter"
level = 5
ac = 18          # plate, no shield
hp = 52
resist = ["fire"]
traits = [
    "evasion dex",
    "ac 1",
]

[pc.saves]
str = 5
con = 4

[pc.spellcasting]
ability = "wis"
ability_modifier = 3
proficiency_bonus = 3

[pc.resources]
focus = 4

[pc.resources.slots]
1 = 2

# Its one spell.
[[pc.features]]
plugin = "sacred_flame"
dice = 2

[[pc.actions]]
name = "Longsword"
effect = "strikes 2 | hit +7 | 1d8+4 slashing"

[[pc.bonus]]
name = "Second Wind"
effect = "uses 1 | heal 1d10+5"
"#;

    #[test]
    fn a_file_reads_into_a_sheet_and_says_what_a_rewrite_would_drop() {
        let (form, lost) = read(SHEET).unwrap();
        assert_eq!(form.kind, "character");
        assert_eq!(
            form.notes,
            "A knight of the old road.\nSworn, and tired of it."
        );
        // The note beside the armor class, and the one above the feature.
        assert_eq!(lost, 2);
        assert_eq!(form.table["name"], "Knight");
        assert_eq!(form.table["ac"], 18);
        assert_eq!(form.table["saves"]["con"], 4);
        assert_eq!(form.table["resources"]["slots"]["1"], 2);
        assert_eq!(form.table["features"][0]["dice"], 2);
        assert_eq!(form.table["actions"][0]["name"], "Longsword");
    }

    #[test]
    fn a_sheet_is_written_as_a_file_that_loads_the_same() {
        let (form, _) = read(SHEET).unwrap();
        let text = write(&form).unwrap();
        assert_eq!(
            text,
            r#"# A knight of the old road.
# Sworn, and tired of it.

[pc]
name = "Knight"
class = "Fighter"
level = 5
ac = 18
hp = 52
resist = ["fire"]
traits = ["evasion dex", "ac 1"]

[pc.saves]
str = 5
con = 4

[pc.spellcasting]
ability = "wis"
ability_modifier = 3
proficiency_bonus = 3

[pc.resources]
focus = 4

[pc.resources.slots]
1 = 2

[[pc.features]]
plugin = "sacred_flame"
dice = 2

[[pc.actions]]
name = "Longsword"
effect = "strikes 2 | hit +7 | 1d8+4 slashing"

[[pc.bonus]]
name = "Second Wind"
effect = "uses 1 | heal 1d10+5"
"#
        );
        assert_eq!(card(&text).unwrap(), card(SHEET).unwrap());
        // And it reads back as the sheet it was written from.
        assert_eq!(read(&text).unwrap(), (form, 0));
    }

    #[test]
    fn what_a_sheet_leaves_empty_is_left_out_of_the_file() {
        let form: Form = serde_json::from_value(serde_json::json!({
            "kind": "monster",
            "table": {
                "name": "Rat", "ac": 10, "hp": 3, "size": "", "initiative": null,
                "immune": [], "traits": ["", "  "], "saves": { "str": null },
                "resources": { "slots": {} },
                "actions": [{ "name": "Bite", "effect": "hit +2 | 1d4 piercing" }, { "name": "", "effect": "" }],
                "it's odd": "kept \"as is\"",
            },
        }))
        .unwrap();
        assert_eq!(
            write(&form).unwrap(),
            "[monster]\nname = \"Rat\"\nac = 10\nhp = 3\n\"it's odd\" = 'kept \"as is\"'\n\n\
             [[monster.actions]]\nname = \"Bite\"\neffect = \"hit +2 | 1d4 piercing\"\n"
        );
    }

    #[test]
    fn a_file_that_is_not_one_creature_is_not_a_sheet() {
        assert!(read("name = \"Loose\"\n")
            .unwrap_err()
            .contains("[pc] or [monster]"));
        assert!(read("[pc\n").is_err());
    }
}
