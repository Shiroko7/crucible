//! A local server for the web app: it lists the scenarios and creatures on
//! disk, saves the ones the app builds or edits, runs a scenario on request
//! and hands back the fights worth watching, and serves the built front end
//! from `web/build`.
//!
//! Everything is read relative to one root folder - the repo - which is also
//! what a scenario's `source:` lines are relative to.

mod agent;
mod builder;
mod creatures;
mod draft;
mod files;
mod form;

pub use agent::{Agent, Cli};

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::{FromRef, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use crucible_core::dsl::scenario;
use crucible_core::sim::showcase::{self, Bundle};
use serde::{Deserialize, Serialize};
use tower_http::services::{ServeDir, ServeFile};

/// Fights probed per playstyle at most: a request is answered in one go, so
/// it has to stay something a browser will wait for.
const MAX_SAMPLES: usize = 5_000;

/// A scenario file in the library.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub(crate) struct ScenarioInfo {
    /// Relative to the root, with forward slashes - what a run asks for.
    path: String,
    name: String,
    /// The folder it is in under `scenarios/`, or `scenarios` itself.
    group: String,
    /// Who is on each side, or nobody when the file does not parse.
    sides: [Vec<Member>; 2],
    /// Why it does not parse, when it does not.
    error: Option<String>,
}

/// One kind of creature on a side, and how many of it.
#[derive(Debug, Serialize, PartialEq, Eq)]
struct Member {
    name: String,
    count: u32,
    /// Squares along each side of its space.
    span: i32,
    hp: i32,
}

/// Who a scenario puts on each side: copies of one creature - `Ogre 1`,
/// `Ogre 2` - counted together.
fn sides(root: &Path, file: &Path) -> Result<[Vec<Member>; 2], String> {
    let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
    let creatures = scenario::parse_under(&text, root).map_err(|e| e.to_string())?;
    let mut out: [Vec<Member>; 2] = [Vec::new(), Vec::new()];
    for c in &creatures {
        let stem = c
            .name
            .trim_end_matches(|ch: char| ch.is_ascii_digit())
            .trim_end();
        let name = if stem.is_empty() {
            c.name.as_str()
        } else {
            stem
        };
        let side = &mut out[usize::from(c.team.min(1))];
        match side.iter_mut().find(|m| m.name == name && m.hp == c.hp) {
            Some(member) => member.count += 1,
            None => side.push(Member {
                name: name.to_string(),
                count: 1,
                span: crucible_core::sim::arena::span(c.size),
                hp: c.hp,
            }),
        }
    }
    Ok(out)
}

#[derive(Debug, Deserialize)]
struct RunRequest {
    path: String,
    #[serde(default = "one")]
    seed: u64,
    #[serde(default = "five_hundred")]
    samples: usize,
    #[serde(default = "thirty")]
    apart: u32,
    /// The map in squares, across and down; the least there is when left
    /// out.
    #[serde(default)]
    map: Option<(i32, i32)>,
}

/// The biggest map a request can ask for, a side: a mile of it.
const MAX_MAP: i32 = 1000;

fn one() -> u64 {
    1
}

fn five_hundred() -> usize {
    500
}

fn thirty() -> u32 {
    30
}

#[derive(Debug, Serialize)]
struct Failure {
    error: String,
}

type Rejection = (StatusCode, Json<Failure>);

fn rejected(error: String) -> Rejection {
    (StatusCode::BAD_REQUEST, Json(Failure { error }))
}

/// A scenario file as the library lists it.
pub(crate) fn scenario_info(root: &Path, file: &Path) -> ScenarioInfo {
    let parts = files::named(root, file);
    let folders = &parts[1..parts.len() - 1];
    let (sides, error) = match sides(root, file) {
        Ok(sides) => (sides, None),
        Err(e) => ([Vec::new(), Vec::new()], Some(e)),
    };
    ScenarioInfo {
        sides,
        error,
        path: parts.join("/"),
        name: file
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .replace('-', " "),
        group: if folders.is_empty() {
            "scenarios".to_string()
        } else {
            folders.join("/")
        },
    }
}

/// Every `.crucible` file under `scenarios/`, folder by folder.
fn library(root: &Path) -> Vec<ScenarioInfo> {
    let mut found = Vec::new();
    files::walk(&root.join("scenarios"), "crucible", &mut found);
    let mut out: Vec<ScenarioInfo> = found.iter().map(|f| scenario_info(root, f)).collect();
    out.sort_by(|a, b| (&a.group, &a.name).cmp(&(&b.group, &b.name)));
    out
}

/// Run a scenario and record the fights worth watching.
fn run(root: &Path, request: &RunRequest) -> Result<Bundle, String> {
    let file = files::existing(
        root,
        &request.path,
        files::SCENARIOS,
        "crucible",
        "scenario",
    )?;
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", request.path))?;
    let creatures =
        scenario::parse_under(&text, root).map_err(|e| format!("{}: {e}", request.path))?;
    showcase::of_scenario(
        &request.path,
        creatures,
        request.seed,
        request.samples.clamp(1, MAX_SAMPLES),
        request.apart,
        request
            .map
            .map(|(across, down)| (across.clamp(1, MAX_MAP), down.clamp(1, MAX_MAP))),
    )
}

async fn list_scenarios(State(root): State<Arc<PathBuf>>) -> Json<Vec<ScenarioInfo>> {
    Json(library(&root))
}

async fn run_scenario(
    State(root): State<Arc<PathBuf>>,
    Json(request): Json<RunRequest>,
) -> Result<Json<Bundle>, Rejection> {
    // A few thousand fights is a second or two of solid arithmetic: off the
    // threads that answer requests.
    tokio::task::spawn_blocking(move || run(&root, &request))
        .await
        .map_err(|e| rejected(format!("the simulation stopped: {e}")))?
        .map(Json)
        .map_err(rejected)
}

/// The file a request is about.
#[derive(Debug, Deserialize)]
struct Named {
    path: String,
}

/// What a request that only does something answers with.
#[derive(Debug, Serialize)]
struct Done {
    done: bool,
}

fn done(outcome: Result<(), String>) -> Result<Json<Done>, Rejection> {
    outcome
        .map(|()| Json(Done { done: true }))
        .map_err(rejected)
}

type Root = State<Arc<PathBuf>>;

/// What every request can reach: where the files are, and the model that
/// drafts new ones.
#[derive(Clone)]
struct Shared {
    root: Arc<PathBuf>,
    agent: Arc<dyn Agent>,
}

impl FromRef<Shared> for Arc<PathBuf> {
    fn from_ref(shared: &Shared) -> Self {
        shared.root.clone()
    }
}

impl FromRef<Shared> for Arc<dyn Agent> {
    fn from_ref(shared: &Shared) -> Self {
        shared.agent.clone()
    }
}

async fn list_creatures(State(root): Root) -> Json<Vec<creatures::CreatureInfo>> {
    Json(creatures::roster(&root))
}

async fn open_creature(
    State(root): Root,
    Query(named): Query<Named>,
) -> Result<Json<creatures::Sheet>, Rejection> {
    creatures::open(&root, &named.path)
        .map(Json)
        .map_err(rejected)
}

/// A draft's text, to be read without being saved.
#[derive(Debug, Deserialize)]
struct Text {
    text: String,
}

async fn check_creature(Json(draft): Json<Text>) -> Json<creatures::Checked> {
    Json(creatures::check(&draft.text))
}

/// A sheet written out as the file it would be saved as, and read.
async fn render_creature(
    Json(sheet): Json<form::Form>,
) -> Result<Json<creatures::Rendered>, Rejection> {
    creatures::render(&sheet).map(Json).map_err(rejected)
}

/// A creature drafted by the model from pasted text. It can take a minute,
/// so it is off the threads that answer requests.
async fn draft_creature(
    State(agent): State<Arc<dyn Agent>>,
    Json(request): Json<draft::Request>,
) -> Result<Json<draft::Drafted>, Rejection> {
    tokio::task::spawn_blocking(move || draft::draft(agent.as_ref(), &request))
        .await
        .map_err(|e| rejected(format!("the draft stopped: {e}")))?
        .map(Json)
        .map_err(rejected)
}

/// The features a creature file can name.
async fn list_features() -> Json<Vec<String>> {
    Json(crucible_core::features::FeatureRegistry::new().ids())
}

async fn save_creature(
    State(root): Root,
    Json(draft): Json<creatures::Draft>,
) -> Result<Json<creatures::CreatureInfo>, Rejection> {
    creatures::save(&root, &draft).map(Json).map_err(rejected)
}

async fn remove_creature(
    State(root): Root,
    Query(named): Query<Named>,
) -> Result<Json<Done>, Rejection> {
    done(creatures::remove(&root, &named.path))
}

async fn open_scenario(
    State(root): Root,
    Query(named): Query<Named>,
) -> Result<Json<builder::Sheet>, Rejection> {
    builder::open(&root, &named.path)
        .map(Json)
        .map_err(rejected)
}

async fn save_scenario(
    State(root): Root,
    Json(plan): Json<builder::Plan>,
) -> Result<Json<ScenarioInfo>, Rejection> {
    builder::save(&root, &plan).map(Json).map_err(rejected)
}

async fn remove_scenario(
    State(root): Root,
    Query(named): Query<Named>,
) -> Result<Json<Done>, Rejection> {
    done(builder::remove(&root, &named.path))
}

/// The whole server, rooted at `root`: the API, and the built front end.
pub fn app(root: PathBuf) -> Router {
    app_with(root, Arc::new(Cli::from_env()))
}

/// [`app`], drafting creatures with a model of the caller's choosing.
pub fn app_with(root: PathBuf, agent: Arc<dyn Agent>) -> Router {
    let site = root.join("web").join("build");
    // Anything that is not a file is a page of the app, which finds its own
    // way from the address.
    let pages = ServeDir::new(&site).fallback(ServeFile::new(site.join("index.html")));
    Router::new()
        .route("/api/scenarios", get(list_scenarios))
        .route("/api/showcase", post(run_scenario))
        .route(
            "/api/scenario",
            get(open_scenario)
                .put(save_scenario)
                .delete(remove_scenario),
        )
        .route("/api/creatures", get(list_creatures))
        .route("/api/creature/check", post(check_creature))
        .route("/api/creature/render", post(render_creature))
        .route("/api/creature/draft", post(draft_creature))
        .route("/api/features", get(list_features))
        .route(
            "/api/creature",
            get(open_creature)
                .put(save_creature)
                .delete(remove_creature),
        )
        .fallback_service(pages)
        .with_state(Shared {
            root: Arc::new(root),
            agent,
        })
}

/// What every test of the server starts from.
#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use tower::ServiceExt;

    pub(crate) const DUEL: &str = "\
creature: Knight
ac: 16
hp: 40
action: Sword | hit +6 | 1d8+4 slashing
team: b
creature: Ogre
size: large
ac: 11
hp: 59
action: Club | hit +6 | 2d8+4 bludgeoning
";

    pub(crate) const KNIGHT: &str = "\
[pc]
name = \"Knight\"
ac = 18
hp = 52

[pc.saves]
str = 5
con = 4
wis = 1
cha = 2

[[pc.actions]]
name = \"Longsword\"
effect = \"strikes 1 | hit +7 | 1d8+4 slashing\"
";

    const OGRE: &str = "\
[monster]
name = \"Ogre\"
size = \"Large\"
creature_type = \"Giant\"
ac = 11
hp = 59

[[monster.actions]]
name = \"Greatclub\"
effect = \"strikes 1 | hit +6 | 2d8+4 bludgeoning\"
";

    const AMBUSH: &str = "\
team: a
source: content/characters/knight.toml

team: b
source: content/monsters/ogre.toml
count: 2
";

    /// A root with three scenarios in it, two in a folder of their own, and
    /// the two creature files one of them fields.
    pub(crate) fn root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("crucible-server-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        for folder in ["scenarios/camp", "content/characters", "content/monsters"] {
            std::fs::create_dir_all(root.join(folder)).unwrap();
        }
        std::fs::write(root.join("scenarios/knight-vs-ogre.crucible"), DUEL).unwrap();
        let three_ogres = format!("{DUEL}count: 3\n");
        std::fs::write(root.join("scenarios/camp/rematch.crucible"), three_ogres).unwrap();
        std::fs::write(root.join("scenarios/camp/ambush.crucible"), AMBUSH).unwrap();
        std::fs::write(root.join("scenarios/notes.txt"), "not a scenario").unwrap();
        std::fs::write(root.join("secret.crucible"), DUEL).unwrap();
        std::fs::write(root.join("content/characters/knight.toml"), KNIGHT).unwrap();
        std::fs::write(root.join("content/monsters/ogre.toml"), OGRE).unwrap();
        root
    }

    pub(crate) async fn call(
        root: PathBuf,
        request: Request<Body>,
    ) -> (StatusCode, serde_json::Value) {
        let response = app(root).oneshot(request).await.unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }

    /// A request, with a JSON body when it has one.
    pub(crate) fn send(method: &str, uri: &str, body: Option<serde_json::Value>) -> Request<Body> {
        let request = Request::builder().method(method).uri(uri);
        match body {
            Some(body) => request
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
            None => request.body(Body::empty()).unwrap(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{call, root, send};
    use super::*;
    use axum::body::Body;
    use axum::http::Request;

    fn post(body: serde_json::Value) -> Request<Body> {
        send("POST", "/api/showcase", Some(body))
    }

    #[tokio::test]
    async fn the_library_lists_every_scenario_by_folder() {
        let request = Request::get("/api/scenarios").body(Body::empty()).unwrap();
        let root = root("library");
        std::fs::write(
            root.join("scenarios/camp/broken.crucible"),
            "creature: X\nwat: 1\n",
        )
        .unwrap();
        let (status, body) = call(root, request).await;
        assert_eq!(status, StatusCode::OK);
        let knight = serde_json::json!([{ "name": "Knight", "count": 1, "span": 1, "hp": 40 }]);
        let sworn = serde_json::json!([{ "name": "Knight", "count": 1, "span": 1, "hp": 52 }]);
        let ogres = |count: u32| serde_json::json!([{ "name": "Ogre", "count": count, "span": 2, "hp": 59 }]);
        assert_eq!(
            body,
            serde_json::json!([
                // One made of creature files reads them from under the root.
                { "path": "scenarios/camp/ambush.crucible", "name": "ambush", "group": "camp",
                  "sides": [sworn, ogres(2)], "error": null },
                // One that does not parse is still listed, with why.
                { "path": "scenarios/camp/broken.crucible", "name": "broken", "group": "camp",
                  "sides": [[], []], "error": "line 2: unknown key `wat`" },
                // Copies of one creature are counted together.
                { "path": "scenarios/camp/rematch.crucible", "name": "rematch", "group": "camp",
                  "sides": [knight, ogres(3)], "error": null },
                { "path": "scenarios/knight-vs-ogre.crucible", "name": "knight vs ogre",
                  "group": "scenarios", "sides": [knight, ogres(1)], "error": null },
            ])
        );
    }

    #[tokio::test]
    async fn a_scenario_comes_back_as_recorded_fights() {
        let request = post(serde_json::json!({
            "path": "scenarios/knight-vs-ogre.crucible", "samples": 40
        }));
        let (status, body) = call(root("run"), request).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["entries"].as_array().unwrap().len(), 15);
        let replay = &body["entries"][0]["replay"];
        assert_eq!(replay["units"][1]["name"], "Ogre");
        assert_eq!(replay["units"][1]["span"], 2);
        assert_eq!(replay["frames"][0]["kind"], "start");
        assert_eq!(replay["order"].as_array().unwrap().len(), 2);
        // Never less than 200 feet by 150, whatever it needs.
        assert_eq!(
            (&replay["width"], &replay["height"]),
            (&serde_json::json!(40), &serde_json::json!(30))
        );
        let styles = body["styles"].as_array().unwrap();
        assert_eq!(styles.len(), 5, "one summary a playstyle");
        assert_eq!(styles[0]["policy"], "greedy");
        let shares = ["win", "loss", "draw"].map(|k| styles[0][k].as_f64().unwrap());
        assert!(
            (shares.iter().sum::<f64>() - 1.0).abs() < 1e-9,
            "{shares:?}"
        );
    }

    #[tokio::test]
    async fn a_fight_is_run_on_as_big_a_map_as_is_asked_for() {
        let request = post(serde_json::json!({
            "path": "scenarios/knight-vs-ogre.crucible", "samples": 5, "map": [70, 55]
        }));
        let (status, body) = call(root("map"), request).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let replay = &body["entries"][0]["replay"];
        assert_eq!(replay["width"], 70);
        assert_eq!(replay["height"], 55);
    }

    #[tokio::test]
    async fn only_a_scenario_in_the_library_can_be_run() {
        for path in [
            "secret.crucible",
            "scenarios/../secret.crucible",
            "scenarios/notes.txt",
            "scenarios/missing.crucible",
            "/etc/passwd",
        ] {
            let request = post(serde_json::json!({ "path": path }));
            let (status, body) = call(root("guard"), request).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{path}");
            assert!(
                body["error"].as_str().unwrap().contains("scenario"),
                "{body}"
            );
        }
    }

    #[tokio::test]
    async fn a_scenario_that_does_not_parse_says_why() {
        let root = root("broken");
        std::fs::write(
            root.join("scenarios/broken.crucible"),
            "creature: X\nwat: 1\n",
        )
        .unwrap();
        let request = post(serde_json::json!({ "path": "scenarios/broken.crucible" }));
        let (status, body) = call(root, request).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body["error"].as_str().unwrap().contains("unknown key"),
            "{body}"
        );
    }
}
