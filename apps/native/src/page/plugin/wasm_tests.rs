//! A plugin whose main is written in Rust and built to WebAssembly (`pages/fixture-wasm`), run as the app runs it.

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, OnceLock, mpsc},
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use super::{
    PluginHost,
    testing::{Rig, Seen, WAIT, pane},
};
use crate::page::{
    runtime::{Event, Reply, wasm::MARKER},
    tree::{Node, Tree},
};

/// The module `file` of the fixture crate `pages/<name>`, built once for the tests that run it, as a frame's source.
/// It needs the `wasm32-unknown-unknown` target (`rustup target add wasm32-unknown-unknown`).
pub(crate) fn module(name: &str, file: &str, built: &'static OnceLock<PathBuf>) -> String {
    let module = built.get_or_init(|| {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("pages").join(name);
        let built = Command::new("cargo")
            .args(["build", "--release", "--target", "wasm32-unknown-unknown"])
            .current_dir(&dir)
            .output()
            .expect("run cargo");
        assert!(
            built.status.success(),
            "pages/{name} didn't build:\n{}",
            String::from_utf8_lossy(&built.stderr)
        );
        dir.join("target/wasm32-unknown-unknown/release").join(file)
    });
    format!("{MARKER}{}", module.display())
}

/// The fixture main (`pages/fixture-wasm`).
fn fixture() -> String {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    module("fixture-wasm", "fixture_main.wasm", &BUILT)
}

/// What the main's tool `name` answered `input` with: `(text, isError)`.
fn tool(host: &PluginHost, name: &str, input: Value) -> (String, bool) {
    let (reply, answer) = Reply::channel();
    host.run_tool(name, input, None, reply);
    let answer = answer.wait(WAIT).expect("the tool's answer").expect("an answer");
    let answer: Value = serde_json::from_str(&answer).unwrap();
    (answer["text"].as_str().unwrap().to_owned(), answer["isError"] == true)
}

/// Waits until the main has counted `key` in storage `enough` times.
fn counted(rig_saved: impl Fn(&str) -> Option<Value>, key: &str, enough: u64) -> u64 {
    let deadline = Instant::now() + WAIT;
    loop {
        let so_far = rig_saved(key).and_then(|n| n.as_u64()).unwrap_or(0);
        if so_far >= enough {
            return so_far;
        }
        assert!(Instant::now() < deadline, "{key} stayed at {so_far}, short of {enough}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_main_built_to_webassembly_badges_panes_answers_a_tool_and_an_action_and_starts_again() {
    let Rig { host, env, root } = Rig::new(
        json!({ "contributes": { "ui": ["badges"] } }),
        vec![pane("1", "/work"), pane("2", "/other")],
    );
    let host = Arc::new(host);
    let (stops, stopped) = mpsc::channel();
    host.keep_main(
        fixture().into(),
        Arc::new(move |event| {
            if let Event::Error { message, fatal: true } = event {
                let _ = stops.send(message);
            }
        }),
    )
    .unwrap();

    let seen = env.wait("a badge on each pane", |seen| seen.len() >= 2);
    let mut badged: Vec<_> = seen
        .iter()
        .filter_map(|seen| match seen {
            Seen::Badge(plugin, pane, Some(badge)) if plugin == "probe" && badge.label == "rust" => Some(pane.as_str()),
            _ => None,
        })
        .collect();
    badged.sort_unstable();
    assert_eq!(badged, ["1", "2"], "{seen:?}");

    assert_eq!(tool(&host, "greet", json!({ "name": "Ada" })), ("Hello, Ada".into(), false));

    let (done, finished) = mpsc::channel();
    host.send_action("1", "open", move |result| {
        let _ = done.send(result);
    });
    assert_eq!(finished.recv_timeout(WAIT).expect("the action to settle"), Ok(()));
    assert_eq!(root.saved("probe", "action"), Some(json!("open")));
    assert_eq!(root.saved("probe", "starts"), Some(json!(1)));

    // A tool that never comes back: the main is stopped for running too long, and started again as a JS main is.
    let (reply, _answer) = Reply::channel();
    host.run_tool("spin", json!({}), None, reply);
    stopped.recv_timeout(WAIT).expect("the spinning main to be stopped");
    counted(|key| root.saved("probe", key), "starts", 2);
    assert_eq!(tool(&host, "greet", json!({ "name": "Grace" })).0, "Hello, Grace");
}

#[test]
fn a_main_built_to_webassembly_that_renders_is_stopped() {
    let Rig { host, .. } = Rig::new(json!({}), vec![]);
    let (errors, heard) = mpsc::channel();
    host.start_main(&fixture(), move |event| {
        if let Event::Error { message, fatal: true } = event {
            let _ = errors.send(message);
        }
    })
    .unwrap();
    let (reply, _answer) = Reply::channel();
    host.run_tool("draw", json!({}), None, reply);
    let message = heard.recv_timeout(WAIT).expect("the main to stop");
    assert!(message.contains("a plugin's main can't render"), "{message}");
}

/// The Rust SDK's `tools`: an input that doesn't fit the tool's schema goes back as the error the JS SDK words, and the
/// handler doesn't run.
#[test]
fn a_rust_mains_tools_check_their_input_and_say_what_to_fix() {
    let rig = Rig::new(json!({}), vec![]);
    rig.host.start_main(&fixture(), |_| {}).unwrap();
    let host = &rig.host;
    assert_eq!(tool(host, "greet", json!({})), ("name is required.".into(), true));
    assert_eq!(tool(host, "greet", json!({ "name": 5 })), ("name must be text.".into(), true));
    assert_eq!(
        tool(host, "greet", json!({ "name": "Ada", "age": 36 })),
        ("Unknown field: age. greet takes name.".into(), true)
    );
    assert_eq!(
        tool(host, "greet", json!({ "name": "Ada Lovelace" })),
        ("name is limited to 10 characters, this one has 12.".into(), true)
    );
    assert_eq!(tool(host, "greet", json!([])), ("The input must be an object.".into(), true));
    assert_eq!(tool(host, "nothing", json!({})), ("The plugin has no handler for nothing".into(), true));
    // A pattern can't be checked here, so the tool refuses instead of running on an unchecked input.
    assert_eq!(
        tool(host, "coded", json!({ "code": "abc" })),
        ("coded's schema has a pattern, which a plugin written in Rust can't check yet. Tell the plugin's author.".into(), true)
    );
    // What a handler returns that isn't text goes back as JSON, and nothing as "Done".
    assert_eq!(
        tool(host, "whoami", json!({})),
        ("{\n  \"paneId\": null,\n  \"project\": null,\n  \"session\": null\n}".into(), false)
    );
}

/// The Rust SDK's `host::set_sidebar_label`: the label and its rows reach the app, the same label isn't sent twice, and
/// none takes it away.
#[test]
fn a_rust_main_sets_its_sidebars_label_once_and_takes_it_away() {
    let rig = Rig::new(
        json!({ "contributes": { "sidebars": [{ "id": "side", "title": "Side", "icon": "clock", "page": "dist/side.html" }] } }),
        vec![],
    );
    rig.host.start_main(&fixture(), |_| {}).unwrap();
    let labels = |seen: &[Seen]| -> Vec<Option<(String, Vec<(String, String)>)>> {
        seen.iter()
            .filter_map(|seen| match seen {
                Seen::SidebarLabel(plugin, page, label) if plugin == "probe" && page == "side" => {
                    Some(label.as_ref().map(|label| (label.text.clone(), label.rows.clone())))
                }
                _ => None,
            })
            .collect()
    };
    assert_eq!(tool(&rig.host, "label", json!({ "text": "1:05" })), ("Done".into(), false));
    tool(&rig.host, "label", json!({ "text": "1:05" }));
    tool(&rig.host, "label", json!({}));
    let seen = rig.env.wait("the label taken away", |seen| labels(seen).last() == Some(&None));
    assert_eq!(labels(&seen), [Some(("1:05".into(), vec![("Task".into(), "Writing".into())])), None]);

    // `host::open_view` shows that sidebar.
    tool(&rig.host, "open", json!({}));
    let seen = rig.env.wait("the sidebar to open", |seen| seen.iter().any(|seen| matches!(seen, Seen::OpenView(_))));
    let opened = seen.iter().find_map(|seen| match seen {
        Seen::OpenView(view) => Some(format!("{view:?}")),
        _ => None,
    });
    assert!(opened.as_deref().is_some_and(|view| view.contains("side")), "{opened:?}");
}

/// The Rust SDK's `messages`: a tell from a TypeScript frame reaches the Rust main, whose own tell reaches that frame.
#[test]
fn a_rust_main_and_a_js_page_tell_each_other() {
    let rig = Rig::new(json!({}), vec![]);
    rig.host.start_main(&fixture(), |_| {}).unwrap();
    // The main is listening once it has answered a tool.
    tool(&rig.host, "greet", json!({ "name": "Ada" }));
    let heard = rig.run(
        "return await new Promise((resolve) => {
            wings.onBroadcast(resolve);
            wings.broadcast({ 'wings:sdk': 1, kind: 'tell', name: 'ping', payload: { n: 5 } });
        });",
    );
    assert_eq!(heard["ok"], json!({ "wings:sdk": 1, "kind": "tell", "name": "pong", "payload": { "n": 5 } }));
}

/// The Rust SDK's `poll::while_focused`: it ticks while Wings is in front, stops when it isn't, and goes on when it
/// is again.
#[test]
fn a_rust_mains_poll_runs_only_while_wings_is_focused() {
    let rig = Rig::new(json!({}), vec![]);
    rig.host.publish_focus(true);
    rig.host.start_main(&fixture(), |_| {}).unwrap();
    let saved = |key: &str| rig.root.saved("probe", key);
    counted(saved, "ticks", 2);
    rig.host.publish_focus(false);
    // A tick already on its way lands.
    std::thread::sleep(Duration::from_millis(200));
    let at_blur = saved("ticks");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(saved("ticks"), at_blur, "the poll went on while Wings was in the background");
    rig.host.publish_focus(true);
    counted(saved, "ticks", at_blur.unwrap().as_u64().unwrap() + 2);
}

/// The Rust SDK's `ui::chips`, `ui::chip` and a time field, in a page (`pages/fixture-wasm-page`): the app reads them
/// as it reads `wings-plugin-ui/native`'s, and a pick or a click comes back to the page, which shows it.
#[test]
fn a_rust_page_shows_chips_a_chip_and_a_time_field_and_hears_them() {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    let rig = Rig::new(json!({}), vec![]);
    let (trees, shown) = mpsc::channel();
    let page = rig
        .host
        .open_page(&module("fixture-wasm-page", "fixture_page.wasm", &BUILT), move |event| {
            let _ = trees.send(event);
        })
        .unwrap();
    let next = || match shown.recv_timeout(WAIT).expect("the page to render") {
        Event::Tree(tree) => tree,
        Event::Error { message, .. } => panic!("{message}"),
    };
    let parts = |tree: &Tree| match &tree.children[..] {
        [Node::Chips(chips), Node::Button(chip), Node::Input(start)] => (chips.clone(), chip.clone(), start.clone()),
        other => panic!("{other:?}"),
    };

    let (chips, chip, start) = parts(&next());
    assert_eq!(chips.label, "Range");
    let options: Vec<_> = chips.options.iter().map(|option| (option.value.as_str(), option.label.as_str())).collect();
    assert_eq!(options, [("week", "Week"), ("month", "Month")]);
    assert_eq!(chips.value.as_deref(), Some("week"));
    assert!(chip.chip && !chip.pressed);
    assert!(start.time && start.value == "08:30" && start.label == "Start");

    assert!(page.dispatch_gesture(&chips.on_change.unwrap(), Some(&json!({ "value": "month" }))));
    assert_eq!(parts(&next()).0.value.as_deref(), Some("month"));
    assert!(page.dispatch_gesture(&chip.on_click.unwrap(), None));
    assert!(parts(&next()).1.pressed);
}
