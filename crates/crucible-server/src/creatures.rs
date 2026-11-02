//! The roster: every creature file on disk, read, checked, saved and
//! removed. A creature is a TOML file holding one `[pc]` or `[monster]` -
//! see `crucible_core::dsl::config` - and the text of that file is what the
//! browser edits; this only says whether it loads and what it loads as.

use std::path::Path;

use crucible_core::creature::Creature;
use crucible_core::dsl::config::load_creature_from_str;
use crucible_core::features::FeatureRegistry;
use serde::{Deserialize, Serialize};

use crate::files::{self, CREATURES};
use crate::form::{self, Form};

/// A creature as its file loads: the numbers a stat block opens with, and
/// the names of what it can do.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub(crate) struct Card {
    name: String,
    /// `character` for a `[pc]`, `monster` for anything else.
    kind: &'static str,
    ac: i32,
    pub(crate) hp: i32,
    size: String,
    /// Squares along each side of its space.
    span: i32,
    creature_type: Option<String>,
    initiative: i32,
    speed: u32,
    /// Strength through Charisma.
    saves: [i32; 6],
    resources: Vec<Pool>,
    moves: Vec<Listed>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct Pool {
    name: String,
    max: u32,
}

/// One thing a creature can do, and what it costs to do it.
#[derive(Debug, Serialize, PartialEq, Eq)]
struct Listed {
    /// `action`, `bonus`, `reaction`, `legendary` or `aura`.
    slot: &'static str,
    name: String,
}

fn listed(creature: &Creature) -> Vec<Listed> {
    let of = |slot: &'static str, name: &str| Listed {
        slot,
        name: name.to_string(),
    };
    let mut out = Vec::new();
    // What anybody can do is not part of what this creature is.
    let own = creature.actions.iter().filter(|m| {
        !m.riders
            .contains(&crucible_core::creature::Rider::Universal)
    });
    out.extend(own.map(|m| of("action", &m.name)));
    out.extend(creature.bonus_actions.iter().map(|m| of("bonus", &m.name)));
    out.extend(
        creature
            .reactions
            .iter()
            .map(|r| of("reaction", &r.action.name)),
    );
    out.extend(creature.legendary.iter().map(|m| of("legendary", &m.name)));
    let auras = creature.auras.iter().chain(&creature.lasting_auras);
    out.extend(auras.map(|m| of("aura", &m.name)));
    out
}

/// What a creature file loads as, or why it does not.
pub(crate) fn card(text: &str) -> Result<Card, String> {
    let creature =
        load_creature_from_str(text, &FeatureRegistry::new()).map_err(|e| e.to_string())?;
    let character = text.lines().any(|line| line.trim() == "[pc]");
    // By name: the loader keeps pools in no order of its own, and a card
    // that reshuffled itself on every read would look like a change.
    let mut resources: Vec<Pool> = creature
        .resources
        .iter()
        .map(|r| Pool {
            name: r.name.clone(),
            max: r.max,
        })
        .collect();
    resources.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Card {
        kind: if character { "character" } else { "monster" },
        ac: creature.ac,
        hp: creature.hp,
        size: format!("{:?}", creature.size),
        span: crucible_core::sim::arena::span(creature.size),
        creature_type: creature.creature_type.map(|t| format!("{t:?}")),
        initiative: creature.initiative,
        speed: creature.speed,
        saves: creature.saves,
        resources,
        moves: listed(&creature),
        name: creature.name,
    })
}

/// A creature file in the roster.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub(crate) struct CreatureInfo {
    /// Relative to the root, with forward slashes - what a scenario's
    /// `source:` line says.
    path: String,
    /// The folder it is in.
    group: String,
    /// Nothing, when the file does not load.
    card: Option<Card>,
    /// Why it does not load, when it does not.
    error: Option<String>,
}

fn info(root: &Path, file: &Path) -> CreatureInfo {
    let parts = files::named(root, file);
    let loaded = std::fs::read_to_string(file)
        .map_err(|e| e.to_string())
        .and_then(|text| card(&text));
    let (card, error) = match loaded {
        Ok(card) => (Some(card), None),
        Err(e) => (None, Some(e)),
    };
    CreatureInfo {
        card,
        error,
        group: parts[..parts.len() - 1].join("/"),
        path: parts.join("/"),
    }
}

/// Every creature file under the folders creatures live in.
pub(crate) fn roster(root: &Path) -> Vec<CreatureInfo> {
    let mut found = Vec::new();
    for top in CREATURES {
        files::walk(&root.join(top), "toml", &mut found);
    }
    let mut out: Vec<CreatureInfo> = found.iter().map(|file| info(root, file)).collect();
    out.sort_by(|a, b| (&a.group, &a.path).cmp(&(&b.group, &b.path)));
    out
}

/// The scenarios whose `source:` lines name this creature file.
fn used_by(root: &Path, path: &str) -> Vec<String> {
    let mut scenarios = Vec::new();
    files::walk(&root.join("scenarios"), "crucible", &mut scenarios);
    let names = |text: &str| {
        text.lines().any(|line| {
            line.split_once(':').is_some_and(|(key, value)| {
                matches!(key.trim(), "source" | "import" | "file") && value.trim() == path
            })
        })
    };
    let mut out: Vec<String> = scenarios
        .iter()
        .filter(|file| std::fs::read_to_string(file).is_ok_and(|text| names(&text)))
        .map(|file| files::named(root, file).join("/"))
        .collect();
    out.sort();
    out
}

/// A creature file opened for editing.
#[derive(Debug, Serialize)]
pub(crate) struct Sheet {
    path: String,
    text: String,
    card: Option<Card>,
    error: Option<String>,
    /// The scenarios that would lose a combatant if this file went away.
    used_by: Vec<String>,
}

pub(crate) fn open(root: &Path, path: &str) -> Result<Sheet, String> {
    let file = files::existing(root, path, CREATURES, "toml", "creature")?;
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{path}: {e}"))?;
    let (card, error) = match card(&text) {
        Ok(card) => (Some(card), None),
        Err(e) => (None, Some(e)),
    };
    Ok(Sheet {
        card,
        error,
        text,
        used_by: used_by(root, path),
        path: path.to_string(),
    })
}

/// What a draft loads as, without saving it, and the sheet it fills in.
#[derive(Debug, Serialize)]
pub(crate) struct Checked {
    card: Option<Card>,
    error: Option<String>,
    /// The file as fields. Nothing, for text that is not one creature's
    /// table at all.
    form: Option<Form>,
    /// Comments inside the file that saving from the sheet would drop.
    comments: usize,
}

pub(crate) fn check(text: &str) -> Checked {
    let (card, error) = match card(text) {
        Ok(card) => (Some(card), None),
        Err(e) => (None, Some(e)),
    };
    let (form, comments) = match form::read(text) {
        Ok((form, comments)) => (Some(form), comments),
        Err(_) => (None, 0),
    };
    Checked {
        card,
        error,
        form,
        comments,
    }
}

/// A sheet as the file it would be saved as, and what that loads as.
#[derive(Debug, Serialize)]
pub(crate) struct Rendered {
    text: String,
    card: Option<Card>,
    error: Option<String>,
}

pub(crate) fn render(sheet: &Form) -> Result<Rendered, String> {
    let text = form::write(sheet)?;
    let (card, error) = match card(&text) {
        Ok(card) => (Some(card), None),
        Err(e) => (None, Some(e)),
    };
    Ok(Rendered { text, card, error })
}

#[derive(Debug, Deserialize)]
pub(crate) struct Draft {
    path: String,
    text: String,
    /// A new file, which must not replace one already there.
    #[serde(default)]
    fresh: bool,
}

/// Save a creature file - only one that loads, so nothing in the roster is
/// ever broken by an edit made here.
pub(crate) fn save(root: &Path, draft: &Draft) -> Result<CreatureInfo, String> {
    let file = files::place(root, &draft.path, CREATURES, "toml", "creature")?;
    card(&draft.text)?;
    files::write(&file, &draft.path, &draft.text, draft.fresh)?;
    Ok(info(root, &file))
}

/// Remove a creature file, unless a scenario still fields it.
pub(crate) fn remove(root: &Path, path: &str) -> Result<(), String> {
    let file = files::existing(root, path, CREATURES, "toml", "creature")?;
    let users = used_by(root, path);
    if !users.is_empty() {
        return Err(format!(
            "`{path}` is still in {} - take it out of there first",
            users.join(", ")
        ));
    }
    std::fs::remove_file(&file).map_err(|e| format!("{path}: {e}"))
}

#[cfg(test)]
mod tests {
    use crate::testing::{call, root, send, KNIGHT};
    use axum::http::StatusCode;
    use serde_json::json;

    #[tokio::test]
    async fn the_roster_lists_every_creature_file_with_what_it_loads_as() {
        let root = root("roster");
        std::fs::write(root.join("content/monsters/broken.toml"), "[monster]\n").unwrap();
        let (status, body) = call(root, send("GET", "/api/creatures", None)).await;
        assert_eq!(status, StatusCode::OK);
        let paths: Vec<&str> = body
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["path"].as_str().unwrap())
            .collect();
        assert_eq!(
            paths,
            [
                "content/characters/knight.toml",
                "content/monsters/broken.toml",
                "content/monsters/ogre.toml",
            ]
        );
        assert_eq!(body[0]["group"], "content/characters");
        assert_eq!(
            body[0]["card"],
            json!({
                "name": "Knight", "kind": "character", "ac": 18, "hp": 52, "size": "Medium",
                "span": 1, "creature_type": null, "initiative": 0, "speed": 30,
                "saves": [5, 0, 4, 0, 1, 2], "resources": [],
                "moves": [{ "slot": "action", "name": "Longsword" }],
            })
        );
        assert_eq!(body[2]["card"]["kind"], "monster");
        assert_eq!(body[2]["card"]["span"], 2);
        assert_eq!(body[2]["card"]["creature_type"], "Giant");
        // One that does not load is still listed, with why.
        assert_eq!(body[1]["card"], json!(null));
        assert!(
            body[1]["error"].as_str().unwrap().contains("name"),
            "{body}"
        );
    }

    /// The app's own starting points are text in the front end, so nothing
    /// but this says they still mean something to the engine.
    #[test]
    fn what_the_app_starts_a_creature_from_and_teaches_loads() {
        let source = include_str!("../../../web/src/lib/templates.ts");
        let templates: Vec<&str> = source.split('`').skip(1).step_by(2).collect();
        assert_eq!(templates.len(), 2, "a character and a monster");
        for template in templates {
            let card = super::card(template).unwrap_or_else(|e| {
                panic!(
                    "{e}:
{template}"
                )
            });
            assert_eq!(card.moves.len(), 1, "{template}");
        }
        let phrases: Vec<&str> = source
            .split("say: '")
            .skip(1)
            .filter_map(|rest| rest.split("'").next())
            .collect();
        assert!(phrases.len() >= 6, "{phrases:?}");
        for phrase in phrases {
            let text = format!(
                "[monster]
name = \"Example\"
ac = 10
hp = 10

                 [monster.resources]
focus = 3

[monster.resources.slots]
2 = 2

                 [[monster.actions]]
name = \"Example\"
effect = \"{phrase}\"
"
            );
            if let Err(e) = super::card(&text) {
                panic!("`{phrase}` does not load: {e}");
            }
        }
    }

    #[tokio::test]
    async fn a_draft_is_checked_without_being_saved() {
        let root = root("check");
        let request = send(
            "POST",
            "/api/creature/check",
            Some(json!({ "text": KNIGHT })),
        );
        let (status, body) = call(root.clone(), request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["card"]["name"], "Knight");
        assert_eq!(body["error"], json!(null));

        let bad = KNIGHT.replace("1d8+4 slashing", "1d8+4 sharp");
        let request = send("POST", "/api/creature/check", Some(json!({ "text": bad })));
        let (status, body) = call(root, request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["card"], json!(null));
        assert!(body["error"].as_str().unwrap().contains("sharp"), "{body}");
    }

    #[tokio::test]
    async fn a_file_is_read_into_a_sheet_and_a_sheet_written_back_as_a_file() {
        let root = root("sheet");
        let request = send(
            "POST",
            "/api/creature/check",
            Some(json!({ "text": KNIGHT })),
        );
        let (_, checked) = call(root.clone(), request).await;
        assert_eq!(checked["comments"], 0);
        assert_eq!(checked["form"]["kind"], "character");
        assert_eq!(checked["form"]["table"]["saves"]["str"], 5);

        // The sheet with one field changed is the file with one line changed.
        let mut sheet = checked["form"].clone();
        sheet["table"]["hp"] = json!(60);
        sheet["notes"] = json!("Promoted.");
        let (status, rendered) = call(
            root.clone(),
            send("POST", "/api/creature/render", Some(sheet)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{rendered}");
        assert_eq!(rendered["card"]["hp"], 60);
        let expected = format!("# Promoted.\n\n{}", KNIGHT.replace("hp = 52", "hp = 60"));
        assert_eq!(rendered["text"].as_str().unwrap(), expected);

        // One that would not load still comes back as a file, with why.
        let mut sheet = checked["form"].clone();
        sheet["table"]["actions"][0]["effect"] = json!("strikes 1 | hit +7 | 1d8 sharp");
        let (status, rendered) =
            call(root, send("POST", "/api/creature/render", Some(sheet))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(rendered["card"], json!(null));
        assert!(
            rendered["error"].as_str().unwrap().contains("sharp"),
            "{rendered}"
        );
        assert!(rendered["text"].as_str().unwrap().contains("1d8 sharp"));
    }

    #[tokio::test]
    async fn the_features_a_file_can_name_are_listed() {
        let (status, body) = call(root("features"), send("GET", "/api/features", None)).await;
        assert_eq!(status, StatusCode::OK);
        let ids: Vec<&str> = body
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i.as_str().unwrap())
            .collect();
        assert!(
            ids.contains(&"bless") && ids.contains(&"spirit_guardians"),
            "{ids:?}"
        );
        assert!(ids.windows(2).all(|pair| pair[0] < pair[1]), "in order");
    }

    #[tokio::test]
    async fn a_creature_is_saved_opened_and_removed() {
        let root = root("save");
        let squire = KNIGHT
            .replace("Knight", "Squire")
            .replace("hp = 52", "hp = 20");
        let path = "scenarios/local/party/squire.toml";
        let draft = json!({ "path": path, "text": squire, "fresh": true });
        let (status, body) = call(
            root.clone(),
            send("PUT", "/api/creature", Some(draft.clone())),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["group"], "scenarios/local/party");
        assert_eq!(body["card"]["hp"], 20);
        assert_eq!(std::fs::read_to_string(root.join(path)).unwrap(), squire);

        // A new file does not replace one that is there; an edit does.
        let (status, body) = call(root.clone(), send("PUT", "/api/creature", Some(draft))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body["error"].as_str().unwrap().contains("already"),
            "{body}"
        );
        let edit = json!({ "path": path, "text": squire.replace("hp = 20", "hp = 25") });
        let (status, body) = call(root.clone(), send("PUT", "/api/creature", Some(edit))).await;
        assert_eq!(status, StatusCode::OK, "{body}");

        let uri = format!("/api/creature?path={path}");
        let (status, body) = call(root.clone(), send("GET", &uri, None)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["card"]["hp"], 25);
        assert!(body["text"].as_str().unwrap().contains("hp = 25"));
        assert_eq!(body["used_by"], json!([]));

        let (status, _) = call(root.clone(), send("DELETE", &uri, None)).await;
        assert_eq!(status, StatusCode::OK);
        assert!(!root.join(path).exists());
    }

    #[tokio::test]
    async fn a_creature_that_does_not_load_is_not_saved() {
        let root = root("unsaved");
        let draft =
            json!({ "path": "content/monsters/ogre.toml", "text": "[monster]\nname = 1\n" });
        let (status, body) = call(root.clone(), send("PUT", "/api/creature", Some(draft))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        let kept = std::fs::read_to_string(root.join("content/monsters/ogre.toml")).unwrap();
        assert!(kept.contains("Greatclub"));
    }

    #[tokio::test]
    async fn a_creature_a_scenario_fields_is_not_removed() {
        let root = root("used");
        let uri = "/api/creature?path=content/monsters/ogre.toml";
        let (_, sheet) = call(root.clone(), send("GET", uri, None)).await;
        assert_eq!(sheet["used_by"], json!(["scenarios/camp/ambush.crucible"]));
        let (status, body) = call(root.clone(), send("DELETE", uri, None)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["error"].as_str().unwrap().contains("ambush"), "{body}");
        assert!(root.join("content/monsters/ogre.toml").is_file());
    }

    #[tokio::test]
    async fn only_a_creature_file_in_the_library_can_be_written() {
        let root = root("creature-guard");
        for path in [
            "knight.toml",
            "web/knight.toml",
            "content/../knight.toml",
            "content/knight.txt",
            "content/.hidden/knight.toml",
            "/etc/knight.toml",
        ] {
            let draft = json!({ "path": path, "text": KNIGHT });
            let (status, body) =
                call(root.clone(), send("PUT", "/api/creature", Some(draft))).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{path}");
            assert!(
                body["error"].as_str().unwrap().contains("creature"),
                "{body}"
            );
        }
        assert!(!root.join("knight.toml").exists());
    }
}
