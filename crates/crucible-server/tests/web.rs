//! The web app, end to end: the real server, the built front end, a real
//! browser, and what ends up on the screen.
//!
//! Ignored by default because it needs two things `cargo test` cannot
//! supply: the front end built into `web/build` (`npm run build` in `web/`)
//! and a Chromium browser - Chrome, or Edge, or whatever `CHROME` points at.
//! Run it with `cargo test -p crucible-server -- --ignored`, or `make e2e`.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crucible_core::dsl::scenario;
use crucible_core::sim::showcase;
use crucible_server::Agent;
use headless_chrome::{Browser, LaunchOptions, Tab};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repo root")
}

/// A model that says what it is told to, in order: the browser tests do
/// not ask a real one.
struct Scripted(Mutex<Vec<String>>);

impl Scripted {
    fn saying(answers: &[String]) -> Arc<Self> {
        Arc::new(Self(Mutex::new(answers.iter().rev().cloned().collect())))
    }
}

impl Agent for Scripted {
    fn ask(&self, _system: &str, _prompt: &str) -> Result<String, String> {
        let mut left = self.0.lock().expect("the answers");
        left.pop().ok_or_else(|| "nothing left to say".to_string())
    }
}

/// The server on a port of its own, for as long as the test runs.
fn serve(root: PathBuf) -> SocketAddr {
    serve_with(root, Scripted::saying(&[]))
}

/// [`serve`], drafting creatures with the model given.
fn serve_with(root: PathBuf, agent: Arc<dyn Agent>) -> SocketAddr {
    assert!(
        root.join("web/build/index.html").is_file(),
        "the front end is not built: run `npm install && npm run build` in web/"
    );
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().expect("a runtime");
        runtime.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("a free port");
            send.send(listener.local_addr().expect("an address"))
                .expect("the test is waiting");
            axum::serve(listener, crucible_server::app_with(root, agent))
                .await
                .expect("the server runs");
        });
    });
    receive.recv().expect("the server started")
}

/// Chrome if the machine has one, otherwise Edge, which is the same engine.
fn browser() -> Browser {
    let edge = [
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        "/usr/bin/microsoft-edge",
    ];
    let path = headless_chrome::browser::default_executable()
        .ok()
        .or_else(|| edge.iter().map(PathBuf::from).find(|p| p.is_file()))
        .expect("a Chromium browser: install Chrome or Edge, or set CHROME to one");
    let options = LaunchOptions::default_builder()
        .path(Some(path))
        .window_size(Some((1400, 900)))
        .idle_browser_timeout(Duration::from_secs(120))
        .build()
        .expect("launch options");
    Browser::new(options).expect("the browser starts")
}

/// The text of everything matching `selector`.
fn texts(tab: &Tab, selector: &str) -> Vec<String> {
    tab.find_elements(selector)
        .unwrap_or_default()
        .iter()
        .map(|e| e.get_inner_text().expect("text"))
        .collect()
}

fn text(tab: &Tab, selector: &str) -> String {
    tab.wait_for_element(selector)
        .unwrap_or_else(|e| panic!("`{selector}` never appeared: {e}"))
        .get_inner_text()
        .expect("text")
}

/// Wait until `selector`'s text satisfies `ready`, and return it.
fn eventually(tab: &Tab, selector: &str, ready: impl Fn(&str) -> bool) -> String {
    let mut seen = String::new();
    for _ in 0..100 {
        // Lowercased: a heading drawn in capitals reports itself that way.
        seen = texts(tab, selector).join("\n").to_lowercase();
        if ready(&seen) {
            return seen;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("`{selector}` never got there; it says: {seen}");
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("a folder");
    for entry in std::fs::read_dir(from).expect("a folder to copy").flatten() {
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        if source.is_dir() {
            copy(&source, &target);
        } else {
            std::fs::copy(&source, &target).expect("a file copied");
        }
    }
}

/// A root of its own for a test: the built front end, and the examples the
/// repo commits - one character, one monster, the scenarios made of them -
/// without whatever else is on this machine, so a test sees the same library
/// everywhere and saves nothing into the repo.
fn sandbox(name: &str) -> PathBuf {
    let (repo, root) = (
        repo(),
        std::env::temp_dir().join(format!("crucible-web-{name}")),
    );
    let _ = std::fs::remove_dir_all(&root);
    copy(&repo.join("web/build"), &root.join("web/build"));
    for folder in ["content/characters", "content/monsters"] {
        copy(&repo.join(folder), &root.join(folder));
    }
    std::fs::create_dir_all(root.join("scenarios")).expect("a folder");
    for entry in std::fs::read_dir(repo.join("scenarios"))
        .expect("the scenarios")
        .flatten()
    {
        if entry.path().is_file() {
            std::fs::copy(entry.path(), root.join("scenarios").join(entry.file_name()))
                .expect("a file copied");
        }
    }
    root
}

/// Run a line of script in the page: for what a click cannot say.
fn script(tab: &Tab, code: &str) {
    tab.evaluate(code, false)
        .unwrap_or_else(|e| panic!("`{code}` failed: {e}"));
}

/// Rewrite the text in the box being edited, as typing over it would.
fn retype(tab: &Tab, from: &str, to: &str) {
    script(
        tab,
        &format!(
            "(() => {{ const box = document.querySelector('[data-test=source]'); \
             box.value = box.value.replace('{from}', '{to}'); \
             box.dispatchEvent(new Event('input', {{ bubbles: true }})); }})()"
        ),
    );
}

/// Put `value` in the box `selector` names, as typing it would.
fn fill(tab: &Tab, selector: &str, value: &str) {
    script(
        tab,
        &format!(
            "(() => {{ const box = document.querySelector('{selector}'); \
             box.value = '{value}'; \
             box.dispatchEvent(new Event('input', {{ bubbles: true }})); }})()"
        ),
    );
}

fn press(tab: &Tab, selector: &str) {
    tab.wait_for_element(selector)
        .unwrap_or_else(|e| panic!("`{selector}` never appeared: {e}"))
        .click()
        .expect("a click");
}

/// Press a button in the roster row of the creature called `name`.
fn pick(tab: &Tab, name: &str, button: &str) {
    script(
        tab,
        &format!(
            "[...document.querySelectorAll('[data-test=pick]')] \
             .find((row) => row.innerText.includes('{name}')) \
             .querySelector('[data-test={button}]').click()"
        ),
    );
}

/// A picture of the page into the folder `CRUCIBLE_SHOTS` names, if it
/// names one.
fn picture(tab: &Tab, name: &str) {
    let Ok(folder) = std::env::var("CRUCIBLE_SHOTS") else {
        return;
    };
    std::thread::sleep(Duration::from_millis(600));
    let png = tab
        .capture_screenshot(
            headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption::Png,
            None,
            None,
            true,
        )
        .expect("a picture");
    std::fs::write(format!("{folder}/{name}.png"), png).expect("the folder exists");
}

#[test]
#[ignore = "needs web/build and a Chromium browser; run with --ignored"]
fn a_creature_is_written_and_a_scenario_built_from_the_roster_in_the_browser() {
    let root = sandbox("builder");
    // What the model will say: a file with a mistake in it, then the same
    // one put right, as it would after being told what was refused.
    let captain = "```toml\n[monster]\nname = \"Bandit Captain\"\nsize = \"Medium\"\n\
        creature_type = \"Humanoid\"\nac = 15\nhp = 65\n\n[[monster.actions]]\n\
        name = \"Scimitar\"\neffect = \"strikes 2 | hit +5 | 1d6+3 slashing\"\n```\n\
        NOTE: Parry is left out: nothing in the grammar raises AC against one attack.\n";
    let answers = [captain.replace("slashing", "sharp"), captain.to_string()];
    let address = serve_with(root.clone(), Scripted::saying(&answers));
    let browser = browser();
    let tab = browser.new_tab().expect("a tab");

    // The roster lists what is on disk.
    tab.navigate_to(&format!("http://{address}/creatures"))
        .expect("the page loads");
    let roster = eventually(&tab, "[data-test=creature]", |cards| {
        cards.contains("brute")
    });
    assert!(roster.contains("cleric"), "{roster}");
    // By what they are, not by where their files happen to be.
    let shelves = texts(&tab, "[data-test=kind] h2").join("|").to_lowercase();
    assert!(
        shelves.starts_with("characters") && shelves.contains("|monsters"),
        "{shelves}"
    );
    picture(&tab, "roster");

    // A new creature is made by pasting whatever there is of it. Nothing
    // else is on the page until there is one.
    press(&tab, "[data-test=new-creature]");
    tab.wait_for_element("[data-test=ask]")
        .expect("a place to paste");
    assert!(tab.find_element("[data-test=sheet]").is_err());
    assert!(tab.find_element("[data-test=save]").is_err());
    picture(&tab, "creature-new");
    fill(
        &tab,
        "[data-test=ask]",
        "Bandit Captain. AC 15, 65 hp. Two scimitar attacks.",
    );
    press(&tab, "[data-test=draft]");

    // The model's first file did not load, so it was sent back; what comes
    // out is a creature on a sheet, with what the model owned up to.
    let report = eventually(&tab, "[data-test=drafted]", |report| {
        report.contains("2 tries")
    });
    assert!(report.contains("parry is left out"), "{report}");
    eventually(&tab, "[data-test=stat-name]", |name| {
        name == "bandit captain"
    });
    eventually(&tab, "[data-test=stat-hp]", |hp| hp == "65");
    eventually(&tab, "[data-test=target]", |file| {
        file == "content/local/monsters/bandit-captain.toml"
    });
    assert_eq!(
        texts(&tab, "[data-test=moves-actions] [data-test=move-name]").len(),
        1
    );
    picture(&tab, "creature-sheet");

    // A field changed on the sheet is the creature changed.
    fill(&tab, "[data-test=field-hp]", "70");
    eventually(&tab, "[data-test=stat-hp]", |hp| hp == "70");
    fill(
        &tab,
        "[data-test=move-effect]",
        "strikes 2 | hit +5 | 1d6+3 sharp",
    );
    let problem = eventually(&tab, "[data-test=problem]", |why| why.contains("sharp"));
    assert!(problem.contains("does not load"), "{problem}");
    let save = tab.wait_for_element("[data-test=save]").expect("a button");
    assert!(
        save.get_attribute_value("disabled").expect("it").is_some(),
        "a broken creature can be saved"
    );

    // The file is the same creature, there to be edited too.
    press(&tab, "[data-test=tab-file]");
    let file = tab
        .wait_for_element("[data-test=source]")
        .expect("the file");
    let shown = file
        .call_js_fn("function() { return this.value; }", vec![], false)
        .expect("its text")
        .value
        .expect("a value");
    let shown = shown.as_str().expect("text");
    assert!(
        shown.contains("hp = 70") && shown.contains("1d6+3 sharp"),
        "{shown}"
    );
    assert!(shown.starts_with("# Drafted by a model"), "{shown}");
    picture(&tab, "creature-file");
    retype(&tab, "sharp", "slashing");
    eventually(&tab, "[data-test=loads]", |note| note == "loads.");
    press(&tab, "[data-test=tab-sheet]");
    let effect = tab
        .wait_for_element("[data-test=move-effect]")
        .expect("the move")
        .call_js_fn("function() { return this.value; }", vec![], false)
        .expect("its text")
        .value
        .expect("a value");
    assert_eq!(effect.as_str(), Some("strikes 2 | hit +5 | 1d6+3 slashing"));

    press(&tab, "[data-test=save]");
    eventually(&tab, "[data-test=creature-path]", |path| {
        path == "content/local/monsters/bandit-captain.toml"
    });
    eventually(&tab, "[data-test=saved]", |note| note == "saved.");
    let written = std::fs::read_to_string(root.join("content/local/monsters/bandit-captain.toml"))
        .expect("the creature was saved");
    assert!(written.contains("name = \"Bandit Captain\""), "{written}");
    assert!(written.contains("hp = 70"), "{written}");

    // A scenario is built by putting creatures of the roster on two sides.
    tab.navigate_to(&format!("http://{address}/"))
        .expect("the page loads");
    tab.wait_for_element("[data-test=new-scenario]")
        .expect("a way to build one")
        .click()
        .expect("a click");
    eventually(&tab, "[data-test=pick]", |rows| {
        rows.contains("bandit captain")
    });
    let run = tab
        .wait_for_element("[data-test=save-run]")
        .expect("a button");
    assert!(
        run.get_attribute_value("disabled").expect("it").is_some(),
        "an empty scenario can be saved"
    );
    pick(&tab, "Cleric", "add-0");
    pick(&tab, "Bandit Captain", "add-1");
    pick(&tab, "Bandit Captain", "add-1");
    eventually(&tab, "[data-test=built-0] [data-test=fielded]", |side| {
        side.contains("cleric")
    });
    eventually(&tab, "[data-test=built-1] [data-test=count]", |count| {
        count == "2"
    });
    tab.wait_for_element("[data-test=collection]")
        .expect("a box")
        .click()
        .expect("a click");
    // A new one is private until somebody says otherwise.
    tab.press_key("End").expect("a key");
    tab.type_str("/raid").expect("typing");
    let file = "scenarios/local/raid/cleric-vs-2-bandit-captain.crucible";
    eventually(&tab, "[data-test=target]", |target| target == file);
    picture(&tab, "builder");

    // Saved, it is a scenario like any other: it runs.
    tab.wait_for_element("[data-test=save-run]")
        .expect("a button")
        .click()
        .expect("a click");
    eventually(&tab, "[data-test=matchup]", |title| {
        title.contains("cleric") && title.contains("2 bandit captains")
    });
    tab.wait_for_element("[data-test=verdict] h2")
        .expect("a verdict");
    assert_eq!(
        std::fs::read_to_string(root.join(file)).expect("the scenario was saved"),
        "team: a\nsource: content/characters/cleric.toml\n\n\
         team: b\nsource: content/local/monsters/bandit-captain.toml\ncount: 2\n"
    );

    // The library lists it under its collection, and it opens to be changed.
    tab.navigate_to(&format!("http://{address}/"))
        .expect("the page loads");
    eventually(&tab, "[data-test=group] h2", |headings| {
        headings.contains("local / raid")
    });
    tab.wait_for_element("[data-test=edit]")
        .expect("a way to change it")
        .click()
        .expect("a click");
    eventually(&tab, "[data-test=build-title]", |title| title == file);
    eventually(&tab, "[data-test=built-1] [data-test=fielded]", |side| {
        side.contains("bandit captain")
    });
    eventually(&tab, "[data-test=built-1] [data-test=count]", |count| {
        count == "2"
    });

    // The creature's own page says where it is fielded.
    tab.navigate_to(&format!(
        "http://{address}/creature?path=content/local/monsters/bandit-captain.toml"
    ))
    .expect("the page loads");
    eventually(&tab, "[data-test=stat-card]", |card| {
        card.contains("scimitar")
    });
    eventually(&tab, "[data-test=creature-title]", |title| {
        title == "bandit captain"
    });
    let page = text(&tab, "main").to_lowercase();
    assert!(page.contains("fielded in"), "{page}");
    assert!(page.contains(file), "{page}");
}

const ENDED: [&str; 2] = ["wins", "unresolved"];

fn ended(text: &str) -> bool {
    ENDED.iter().any(|word| text.contains(word))
}

#[test]
#[ignore = "needs web/build and a Chromium browser; run with --ignored"]
fn a_scenario_is_picked_predicted_and_played_back_in_the_browser() {
    let address = serve(sandbox("playback"));
    let browser = browser();
    let tab = browser.new_tab().expect("a tab");
    tab.set_default_timeout(Duration::from_secs(30));

    // The library shows what is on disk as cards: who, against whom.
    tab.navigate_to(&format!("http://{address}/"))
        .expect("the page loads");
    tab.wait_for_element("[data-test=card]").expect("a library");
    let cards = texts(&tab, "[data-test=card]");
    assert!(
        cards
            .iter()
            .any(|card| card.contains("2 Clerics") && card.contains("against 4 Brutes")),
        "{cards:?}"
    );
    eventually(&tab, "[data-test=group] h2", |headings| {
        headings.contains("examples")
    });

    // Opening one runs it: a verdict, and a row for each of five playstyles
    // with its worst, median and best fight.
    tab.wait_for_element("a[data-test=card][href*='cleric-vs-brute.crucible']")
        .expect("the card")
        .click()
        .expect("a click");
    eventually(&tab, "[data-test=matchup]", |title| {
        title.contains("cleric") && title.contains("against brute")
    });
    tab.wait_for_element("[data-test=verdict] h2")
        .expect("a verdict");
    let styles = texts(&tab, "[data-test=style]");
    assert_eq!(styles.len(), 5, "{styles:?}");
    assert!(styles[0].starts_with("Greedy"), "{styles:?}");
    assert!(styles[0].contains('%'), "a win rate: {styles:?}");
    assert_eq!(texts(&tab, "[data-test=pick]").len(), 15);
    assert!(text(&tab, "[data-test=basis]").contains("500 fights"));

    // A fight opens on the stage at its first frame: both sides listed, at
    // full health, initiative rolled.
    tab.wait_for_element("[data-test=pick]")
        .expect("a fight")
        .click()
        .expect("a click");
    eventually(&tab, "[data-test=fight-title]", |title| {
        title.contains("greedy") && title.contains("worst")
    });
    let party = texts(&tab, "[data-test=side-0] [data-test=plate]");
    let foes = texts(&tab, "[data-test=side-1] [data-test=plate]");
    assert_eq!((party.len(), foes.len()), (1, 1), "{party:?} {foes:?}");
    assert!(party[0].contains("Cleric") && foes[0].contains("Brute"));
    assert!(text(&tab, "[data-test=narration]").contains("Combat begins"));
    assert!(text(&tab, "[data-test=counter]").starts_with("1 /"));
    assert!(
        tab.wait_for_element(".field canvas").is_ok(),
        "a board is drawn"
    );
    let in_order = |tab: &Tab| {
        tab.find_elements("[data-test=order] .token")
            .map_or(0, |tokens| tokens.len())
    };
    assert_eq!(in_order(&tab), 0, "nobody has rolled yet");

    // Initiative is rolled one creature at a time, each joining the turn
    // order with the die it rolled in the tray; then the first turn starts.
    tab.press_key("ArrowRight").expect("a key");
    eventually(&tab, "[data-test=tray]", |tray| tray.contains("initiative"));
    assert_eq!(in_order(&tab), 1);
    assert_eq!(
        tab.find_elements("[data-test=tray] .die")
            .expect("the die")
            .len(),
        1
    );
    tab.press_key("ArrowRight").expect("a key");
    eventually(&tab, "[data-test=counter]", |c| c.starts_with("3 /"));
    assert_eq!(in_order(&tab), 2);
    tab.press_key("ArrowRight").expect("a key");
    let whose = eventually(&tab, "[data-test=turn]", |turn| turn.ends_with("'s turn"));
    assert!(
        text(&tab, "[data-test=narration]")
            .to_lowercase()
            .contains(&whose),
        "the narration and the turn order agree on {whose}"
    );

    // An attack roll shows its die, its total and the armor class it had to
    // beat, and is stamped with how it went.
    let mut stamp = String::new();
    for _ in 0..60 {
        tab.press_key("ArrowRight").expect("a key");
        std::thread::sleep(Duration::from_millis(60));
        if text(&tab, "[data-test=tray]").contains("ATTACK ROLL") {
            stamp = text(&tab, "[data-test=stamp]");
            break;
        }
    }
    assert!(
        ["HIT", "MISS", "CRITICAL HIT!"].contains(&stamp.as_str()),
        "an attack was rolled and judged: `{stamp}`"
    );
    let tray = text(&tab, "[data-test=tray]");
    assert!(tray.contains("AC") && tray.contains('='), "{tray}");
    assert!(text(&tab, "[data-test=narration]").contains("against AC"));
    tab.press_key("Home").expect("a key");
    eventually(&tab, "[data-test=counter]", |c| c.starts_with("1 /"));

    // The arrow keys step through it.
    tab.press_key("ArrowRight").expect("a key");
    eventually(&tab, "[data-test=counter]", |c| c.starts_with("2 /"));
    tab.press_key("End").expect("a key");
    eventually(&tab, "[data-test=narration]", ended);
    tab.press_key("Home").expect("a key");
    eventually(&tab, "[data-test=counter]", |c| c.starts_with("1 /"));

    // Down picks the next fight, and the address follows so it can be reopened.
    tab.press_key("ArrowDown").expect("a key");
    eventually(&tab, "[data-test=fight-title]", |title| {
        title.contains("greedy") && title.contains("median")
    });
    let address_bar = tab.get_url();
    assert!(address_bar.contains("fight=2"), "{address_bar}");
    assert!(
        address_bar.contains("cleric-vs-brute.crucible"),
        "{address_bar}"
    );

    // Play runs to the end on its own and stops there. Using the speed
    // control must not keep the keyboard to itself.
    tab.wait_for_element("footer select")
        .expect("the speed")
        .click()
        .expect("a click");
    tab.evaluate(
        "const s = document.querySelector('footer select'); s.value = '140'; \
         s.dispatchEvent(new Event('change', { bubbles: true }))",
        false,
    )
    .expect("the speed is set");
    tab.press_key(" ").expect("a key");
    eventually(&tab, "[data-test=narration]", ended);
    eventually(&tab, "[data-test=play]", |c| c.contains("play"));

    // The log lists every beat of the fight.
    tab.wait_for_element("[data-test=log]")
        .expect("the log button")
        .click()
        .expect("a click");
    let lines = texts(&tab, "[data-test=log-panel] .line");
    let counter = text(&tab, "[data-test=counter]");
    assert!(
        counter.ends_with(&format!("/ {}", lines.len())),
        "{counter} for {} lines",
        lines.len()
    );
    assert!(ended(&lines.last().expect("an ending").to_lowercase()));

    // Escape goes back to the prediction.
    tab.press_key("Escape").expect("a key");
    tab.wait_for_element("[data-test=verdict]")
        .expect("the prediction again");
    assert!(!tab.get_url().contains("fight="));
}

#[test]
#[ignore = "needs web/build and a Chromium browser; run with --ignored"]
fn an_address_opens_straight_on_a_frame_of_a_fight_and_a_bad_one_says_so() {
    let root = sandbox("address");
    let address = serve(root.clone());
    let browser = browser();
    let tab = browser.new_tab().expect("a tab");
    tab.set_default_timeout(Duration::from_secs(30));

    let scenario = "scenarios/two-clerics-vs-four-brutes.crucible";
    tab.navigate_to(&format!(
        "http://{address}/scenario?path={scenario}&fight=14&frame=3"
    ))
    .expect("the page loads");
    eventually(&tab, "[data-test=fight-title]", |title| {
        title.contains("thrifty") && title.contains("median")
    });
    // Both sides' frames are up: one line of text a creature, at the least.
    eventually(&tab, "[data-test=side-0] [data-test=plate]", |frames| {
        frames.contains("cleric 1") && frames.contains("cleric 2")
    });
    eventually(&tab, "[data-test=side-1] [data-test=plate]", |frames| {
        (1..=4).all(|n| frames.contains(&format!("brute {n}")))
    });
    assert!(text(&tab, "[data-test=counter]").starts_with("3 /"));
    // Each frame says how hard its creature is to hit.
    eventually(&tab, "[data-test=side-1] [data-test=ac]", |acs| {
        acs.matches("11").count() == 4
    });

    // What everyone is under shows beside them. The fights are the ones the
    // page is sent, so the first beat where somebody is holding a spell up
    // and a brute is under something is read off the same recordings.
    let text = std::fs::read_to_string(root.join(scenario)).expect("the scenario");
    let creatures = scenario::parse_under(&text, &root).expect("it parses");
    let bundle = showcase::of_scenario(scenario, creatures, 1, 500, 30, None).expect("it runs");
    let held = |replay: &crucible_core::sim::Replay| {
        replay.frames.iter().enumerate().find_map(|(at, frame)| {
            let spell = frame.units.iter().find_map(|u| u.concentrating.clone())?;
            let condition = frame
                .units
                .iter()
                .zip(&replay.units)
                .filter(|(_, unit)| unit.team == 1)
                .find_map(|(state, _)| state.conditions.first().cloned())?;
            Some((at + 1, spell, condition))
        })
    };
    let (fight, (beat, spell, condition)) = bundle
        .entries
        .iter()
        .enumerate()
        .find_map(|(at, entry)| Some((at + 1, held(&entry.replay)?)))
        .expect("a beat with a spell held and a brute under something");
    let replay = &bundle.entries[0].replay;
    tab.navigate_to(&format!(
        "http://{address}/scenario?path={scenario}&fight={fight}&frame={beat}"
    ))
    .expect("the page loads");
    eventually(&tab, "[data-test=concentrating]", |chip| {
        chip.contains(&spell.to_lowercase())
    });
    eventually(&tab, "[data-test=side-1] [data-test=condition]", |chips| {
        chips.contains(&condition.to_lowercase())
    });

    // With CRUCIBLE_SHOTS set to a folder, leave pictures of a few beats of
    // the first fight there, caught mid-animation: for looking at.
    if let Ok(folder) = std::env::var("CRUCIBLE_SHOTS") {
        let shoot = |name: &str| {
            std::thread::sleep(Duration::from_millis(1200));
            let png = tab
                .capture_screenshot(
                    headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption::Png,
                    None,
                    None,
                    true,
                )
                .expect("a picture");
            std::fs::write(format!("{folder}/{name}.png"), png).expect("the folder exists");
        };
        // And of the two screens that lead to it.
        tab.navigate_to(&format!("http://{address}/"))
            .expect("the page loads");
        tab.wait_for_element("[data-test=card]").expect("a library");
        shoot("library");
        tab.navigate_to(&format!("http://{address}/scenario?path={scenario}"))
            .expect("the page loads");
        tab.wait_for_element("[data-test=verdict] h2")
            .expect("a verdict");
        shoot("prediction");
        let beats = [3, 10, 14, 27, 28, 30, 42, 108];
        for frame in beats.into_iter().filter(|&n| n <= replay.frames.len()) {
            tab.navigate_to(&format!(
                "http://{address}/scenario?path={scenario}&fight=1&frame={frame}&still=0.55"
            ))
            .expect("the page loads");
            eventually(&tab, "[data-test=counter]", |c| {
                c.starts_with(&format!("{frame} /"))
            });
            shoot(&format!("beat-{frame}"));
        }
        tab.press_key("m").expect("a key");
        shoot("whole-map");
        tab.press_key("End").expect("a key");
        shoot("ending");
    }

    tab.navigate_to(&format!(
        "http://{address}/scenario?path=scenarios/no-such-fight.crucible"
    ))
    .expect("the page loads");
    let failure = eventually(&tab, "[data-test=failure]", |note| {
        note.contains("no scenario")
    });
    assert!(failure.contains("no-such-fight"), "{failure}");
}
