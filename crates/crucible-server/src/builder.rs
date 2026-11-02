//! Putting a scenario together: who is on each side, as creature files and
//! how many of each, written out as the `.crucible` file the rest of the
//! tool already reads.
//!
//! A scenario built here is nothing but `team:`, `source:` and `count:`
//! lines under a few lines of notes. One written by hand can say more - a
//! creature spelled out in place - and that one is edited as text instead.

use std::path::Path;

use crucible_core::dsl::scenario;
use serde::{Deserialize, Serialize};

use crate::files::{self, CREATURES, SCENARIOS};

/// One creature file on a side, and how many copies of it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Entry {
    /// 0 for the party, 1 for what it faces.
    team: u8,
    /// The creature file, relative to the root.
    source: String,
    #[serde(default = "one")]
    count: u32,
}

fn one() -> u32 {
    1
}

/// The notes a scenario opens with and the entries under them, if that is
/// all the file is.
fn entries(text: &str) -> Option<(String, Vec<Entry>)> {
    let mut notes: Vec<&str> = Vec::new();
    let mut out: Vec<Entry> = Vec::new();
    let mut team: Option<u8> = None;
    let mut begun = false;
    for line in text.lines().map(str::trim) {
        if line.is_empty() {
            continue;
        }
        if let Some(note) = line.strip_prefix('#') {
            // A comment further down belongs to a line a rewrite would drop.
            if begun {
                return None;
            }
            notes.push(note.strip_prefix(' ').unwrap_or(note));
            continue;
        }
        begun = true;
        let (key, value) = line.split_once(':')?;
        let value = value.trim();
        match key.trim().to_ascii_lowercase().as_str() {
            "team" if team.is_none() => {
                team = Some(match value.to_ascii_lowercase().as_str() {
                    "a" | "0" | "party" => 0,
                    "b" | "1" | "monsters" => 1,
                    _ => return None,
                });
            }
            "source" => out.push(Entry {
                team: team.take()?,
                source: value.to_string(),
                count: 1,
            }),
            "count" if team.is_none() => out.last_mut()?.count = value.parse().ok()?,
            _ => return None,
        }
    }
    team.is_none().then(|| (notes.join("\n"), out))
}

/// The file a set of notes and entries is saved as.
fn written(notes: &str, entries: &[Entry]) -> String {
    let mut out = String::new();
    for line in notes.trim().lines() {
        out.push_str(format!("# {line}").trim_end());
        out.push('\n');
    }
    for entry in entries {
        if !out.is_empty() {
            out.push('\n');
        }
        let team = if entry.team == 0 { 'a' } else { 'b' };
        out.push_str(&format!("team: {team}\nsource: {}\n", entry.source));
        if entry.count > 1 {
            out.push_str(&format!("count: {}\n", entry.count));
        }
    }
    out
}

/// A scenario opened for editing.
#[derive(Debug, Serialize)]
pub(crate) struct Sheet {
    path: String,
    text: String,
    notes: String,
    /// Nothing, for a file that says more than a list of creature files
    /// can: that one is edited as text.
    entries: Option<Vec<Entry>>,
}

pub(crate) fn open(root: &Path, path: &str) -> Result<Sheet, String> {
    let file = files::existing(root, path, SCENARIOS, "crucible", "scenario")?;
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{path}: {e}"))?;
    let (notes, entries) = match entries(&text) {
        Some((notes, entries)) => (notes, Some(entries)),
        None => (String::new(), None),
    };
    Ok(Sheet {
        notes,
        entries,
        text,
        path: path.to_string(),
    })
}

#[derive(Debug, Deserialize)]
pub(crate) struct Plan {
    path: String,
    /// A new file, which must not replace one already there.
    #[serde(default)]
    fresh: bool,
    /// The whole file, for one edited as text...
    text: Option<String>,
    /// ...or what it is built from.
    #[serde(default)]
    notes: String,
    #[serde(default)]
    entries: Vec<Entry>,
}

/// Save a scenario - only one that loads and has somebody on each side, so
/// whatever is saved here can be run.
pub(crate) fn save(root: &Path, plan: &Plan) -> Result<crate::ScenarioInfo, String> {
    let file = files::place(root, &plan.path, SCENARIOS, "crucible", "scenario")?;
    let text = match &plan.text {
        Some(text) => text.clone(),
        None => {
            for entry in &plan.entries {
                files::existing(root, &entry.source, CREATURES, "toml", "creature")?;
                if entry.count == 0 {
                    return Err("a count of zero leaves nothing to fight".to_string());
                }
            }
            written(&plan.notes, &plan.entries)
        }
    };
    let creatures = scenario::parse_under(&text, root).map_err(|e| e.to_string())?;
    for (team, who) in [(0, "the party"), (1, "the other side")] {
        if !creatures.iter().any(|c| c.team == team) {
            return Err(format!("there is nobody on {who} - a fight needs both"));
        }
    }
    files::write(&file, &plan.path, &text, plan.fresh)?;
    Ok(crate::scenario_info(root, &file))
}

pub(crate) fn remove(root: &Path, path: &str) -> Result<(), String> {
    let file = files::existing(root, path, SCENARIOS, "crucible", "scenario")?;
    std::fs::remove_file(&file).map_err(|e| format!("{path}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{call, root, send};
    use axum::http::StatusCode;
    use serde_json::json;

    const KNIGHT: &str = "content/characters/knight.toml";
    const OGRE: &str = "content/monsters/ogre.toml";

    fn entry(team: u8, source: &str, count: u32) -> Entry {
        Entry {
            team,
            source: source.to_string(),
            count,
        }
    }

    #[test]
    fn a_built_scenario_reads_back_as_what_it_was_built_from() {
        let built = [entry(0, KNIGHT, 1), entry(1, OGRE, 3)];
        let text = written("An ambush.\nThree of them.", &built);
        assert_eq!(
            text,
            "# An ambush.\n# Three of them.\n\nteam: a\nsource: content/characters/knight.toml\n\n\
             team: b\nsource: content/monsters/ogre.toml\ncount: 3\n"
        );
        let (notes, read) = entries(&text).unwrap();
        assert_eq!(notes, "An ambush.\nThree of them.");
        assert_eq!(read, built);
    }

    #[test]
    fn a_scenario_that_says_more_is_not_one_to_rebuild() {
        for text in [
            "creature: Knight\nac: 16\nhp: 40\n",
            // No side given: which one is the file's own rule to apply.
            "source: content/monsters/ogre.toml\n",
            "team: a\nsource: a.toml\n# why this one\nteam: b\nsource: b.toml\n",
            "team: a\n",
        ] {
            assert_eq!(entries(text), None, "{text}");
        }
    }

    #[tokio::test]
    async fn a_scenario_is_built_opened_and_removed() {
        let root = root("build");
        let path = "scenarios/camp/the-bridge.crucible";
        let plan = json!({
            "path": path, "fresh": true, "notes": "Held by one knight.",
            "entries": [
                { "team": 0, "source": KNIGHT },
                { "team": 1, "source": OGRE, "count": 2 },
            ],
        });
        let (status, body) = call(
            root.clone(),
            send("PUT", "/api/scenario", Some(plan.clone())),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["name"], "the bridge");
        assert_eq!(body["group"], "camp");
        assert_eq!(body["sides"][1][0]["count"], 2);

        let (status, body) = call(root.clone(), send("PUT", "/api/scenario", Some(plan))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body["error"].as_str().unwrap().contains("already"),
            "{body}"
        );

        let uri = format!("/api/scenario?path={path}");
        let (status, body) = call(root.clone(), send("GET", &uri, None)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["notes"], "Held by one knight.");
        assert_eq!(
            body["entries"][1],
            json!({ "team": 1, "source": OGRE, "count": 2 })
        );

        // What was built is a scenario like any other: it runs.
        let run = json!({ "path": path, "samples": 20 });
        let (status, body) = call(root.clone(), send("POST", "/api/showcase", Some(run))).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body["entries"][0]["replay"]["units"]
                .as_array()
                .unwrap()
                .len(),
            3
        );

        let (status, _) = call(root.clone(), send("DELETE", &uri, None)).await;
        assert_eq!(status, StatusCode::OK);
        assert!(!root.join(path).exists());
    }

    #[tokio::test]
    async fn a_scenario_written_by_hand_is_opened_and_saved_as_text() {
        let root = root("by-hand");
        let uri = "/api/scenario?path=scenarios/knight-vs-ogre.crucible";
        let (_, body) = call(root.clone(), send("GET", uri, None)).await;
        assert_eq!(body["entries"], json!(null));
        let text = body["text"].as_str().unwrap().replace("hp: 40", "hp: 45");
        let plan = json!({ "path": "scenarios/knight-vs-ogre.crucible", "text": text });
        let (status, body) = call(root.clone(), send("PUT", "/api/scenario", Some(plan))).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["sides"][0][0]["hp"], 45);
    }

    #[tokio::test]
    async fn a_scenario_nobody_could_run_is_not_saved() {
        let root = root("unbuilt");
        let path = "scenarios/empty.crucible";
        for (entries, why) in [
            (
                json!([{ "team": 0, "source": KNIGHT }]),
                "nobody on the other side",
            ),
            (
                json!([{ "team": 1, "source": OGRE }]),
                "nobody on the party",
            ),
            (
                json!([{ "team": 0, "source": KNIGHT }, { "team": 1, "source": "content/monsters/troll.toml" }]),
                "no creature",
            ),
            (
                json!([{ "team": 0, "source": KNIGHT }, { "team": 1, "source": "../secret.toml" }]),
                "not a creature",
            ),
        ] {
            let plan = json!({ "path": path, "entries": entries });
            let (status, body) = call(root.clone(), send("PUT", "/api/scenario", Some(plan))).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{why}");
            assert!(body["error"].as_str().unwrap().contains(why), "{body}");
        }
        let plan = json!({ "path": "content/x.crucible", "text": "" });
        let (status, _) = call(root.clone(), send("PUT", "/api/scenario", Some(plan))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(!root.join(path).exists());
    }
}
