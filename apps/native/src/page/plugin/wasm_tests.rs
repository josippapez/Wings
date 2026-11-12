//! A plugin whose main is written in Rust and built to WebAssembly (`pages/fixture-wasm`), run as the app runs it.

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, OnceLock, mpsc},
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use super::testing::{Rig, Seen, WAIT, pane};
use crate::page::runtime::{Event, Reply, wasm::MARKER};

/// The fixture main's module, built once for the tests that run it. It needs the `wasm32-unknown-unknown` target
/// (`rustup target add wasm32-unknown-unknown`).
fn fixture() -> &'static Path {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT.get_or_init(|| {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("pages/fixture-wasm");
        let built = Command::new("cargo")
            .args(["build", "--release", "--target", "wasm32-unknown-unknown"])
            .current_dir(&dir)
            .output()
            .expect("run cargo");
        assert!(
            built.status.success(),
            "pages/fixture-wasm didn't build:\n{}",
            String::from_utf8_lossy(&built.stderr)
        );
        dir.join("target/wasm32-unknown-unknown/release/fixture_main.wasm")
    })
}

fn greet(host: &super::PluginHost, name: &str) -> Value {
    let (reply, answer) = Reply::channel();
    host.run_tool("greet", json!({ "name": name }), None, reply);
    let answer = answer.wait(WAIT).expect("the tool's answer").expect("the tool to succeed");
    serde_json::from_str(&answer).unwrap()
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
        format!("{MARKER}{}", fixture().display()).into(),
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

    let said = greet(&host, "Ada");
    assert_eq!((&said["text"], &said["isError"]), (&json!("Hello, Ada"), &json!(false)));

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
    let deadline = Instant::now() + WAIT;
    while root.saved("probe", "starts") != Some(json!(2)) {
        assert!(Instant::now() < deadline, "the main didn't start again");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(greet(&host, "Grace")["text"], "Hello, Grace");
}

#[test]
fn a_main_built_to_webassembly_that_renders_is_stopped() {
    let Rig { host, .. } = Rig::new(json!({}), vec![]);
    let (errors, heard) = mpsc::channel();
    host.start_main(&format!("{MARKER}{}", fixture().display()), move |event| {
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
