//! The plugins of the repo's `plugins/` that ship a native build, run as the app runs them: their `dist/native` bundles
//! against a fake app and the manifest they declare, so a call a plugin makes meets the permission its manifest grants.

mod claude_chat;
mod dev_servers_share;
mod disk;
mod hud;
mod pr_tracker;
mod schedules;
mod tasks;
mod work_item;
mod worktrees;

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc,
};

use serde_json::json;
use wings_plugins::{loading, manifest::Plugin};

use super::{
    PanePort, Project, SessionEntry,
    testing::{Rig, Seen, WAIT, pane},
    ui::Tone,
};
use crate::page::{
    runtime::{
        Event, PageRuntime, Reply, Store,
        fake::{self, Cx, Guest},
    },
    tree::{Node, Tree, plain_text},
};

fn repo_plugin(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins")
        .join(name)
}

/// The plugin as the desktop loads it from the repo.
fn load(name: &str) -> Plugin {
    wings_plugins::set_wings_version(env!("CARGO_PKG_VERSION"));
    loading::load(&repo_plugin(name)).unwrap_or_else(|error| panic!("{name}: {error}"))
}

/// A frame of a plugin written in Rust, as the app runs it: its committed module, which `sh build-rust.sh` in `plugins/`
/// builds.
fn module(name: &str, frame: &str) -> String {
    // The test build compiles a module slowly (PR tracker's pages take minutes), so the compiled code is kept for the
    // next test and the next run.
    crate::page::runtime::wasm::keep_compiled_in(std::env::temp_dir().join("wings-test-cwasm"));
    let path = repo_plugin(name).join("dist/native").join(format!("{frame}.wasm"));
    assert!(path.is_file(), "{}: build the plugin: cd plugins && sh build-rust.sh {name}", path.display());
    // Before any page is opened: a page that is still compiling when its test stops waiting is never kept.
    crate::page::runtime::wasm::compile_now(&path.to_string_lossy()).unwrap();
    format!("{}{}", crate::page::runtime::wasm::MARKER, path.display())
}

/// What a plugin's main reported: its errors, which a test of a working plugin expects none of.
fn errors() -> (impl FnMut(Event) + Send + 'static, mpsc::Receiver<String>) {
    let (sender, received) = mpsc::channel();
    (
        move |event| {
            if let Event::Error { message, .. } = event {
                let _ = sender.send(message);
            }
        },
        received,
    )
}

fn action(rig: &Rig, pane: &str, id: &str) -> Result<(), String> {
    let (done, finished) = mpsc::channel();
    rig.host.send_action(pane, id, move |result| {
        let _ = done.send(result);
    });
    finished.recv_timeout(WAIT).expect("the action to settle")
}

fn node(pane: &str, port: u16) -> PanePort {
    PanePort {
        pane_id: pane.into(),
        pid: 4242,
        name: "node".into(),
        port,
        started_at: 0,
    }
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("git {args:?}: {error}"));
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A committed main checkout and a linked `feature` worktree. It lives under the test storage root, which `Root` removes.
fn worktree_repo(rig: &Rig) -> (PathBuf, PathBuf) {
    let root = rig.root.config("worktrees").root.join("worktrees-repo");
    fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "test@example.com"]);
    git(&root, &["config", "user.name", "Wings test"]);
    fs::write(root.join("README.md"), "fixture\n").unwrap();
    git(&root, &["add", "README.md"]);
    git(&root, &["commit", "-qm", "fixture"]);
    git(&root, &["branch", "-M", "main"]);
    (root.clone(), root.with_file_name("worktrees-feature"))
}

fn open_page(host: &super::PluginHost, source: &str) -> (PageRuntime, mpsc::Receiver<Event>) {
    let (sent, received) = mpsc::channel();
    let runtime = host
        .open_page(source, move |event| {
            let _ = sent.send(event);
        })
        .unwrap();
    (runtime, received)
}

fn tree(received: &mpsc::Receiver<Event>, what: &str, done: impl Fn(&Tree) -> bool) -> Tree {
    let mut last = String::new();
    loop {
        match received.recv_timeout(WAIT) {
            Ok(Event::Tree(tree)) if done(&tree) => return tree,
            Ok(Event::Tree(tree)) => last = tree.text(),
            Ok(Event::Error { message, .. }) => panic!("waiting for {what}: {message}"),
            Err(_) => panic!("waiting for {what}: the page last showed {last:?}"),
        }
    }
}

/// The handler of the text input named `label`.
fn input_named(nodes: &[Node], label: &str) -> Option<String> {
    nodes.iter().find_map(|node| match node {
        Node::Input(input) if input.label == label => input.on_change.clone(),
        other => other
            .parts()
            .into_iter()
            .find_map(|part| input_named(part, label)),
    })
}

fn buttons_named(nodes: &[Node], label: &str, found: &mut Vec<String>) {
    for node in nodes {
        // By its words, or by the name an icon button is read out by.
        if let Node::Button(button) = node
            && (plain_text(&button.children) == label || button.label.as_deref() == Some(label))
            && let Some(handler) = &button.on_click
        {
            found.push(handler.clone());
        }
        for part in node.parts() {
            buttons_named(part, label, found);
        }
    }
}

/// The handler of the first node `pick` gives one for, wherever it is in the tree.
fn handler_of(nodes: &[Node], pick: &impl Fn(&Node) -> Option<String>) -> Option<String> {
    nodes.iter().find_map(|node| pick(node).or_else(|| node.parts().into_iter().find_map(|part| handler_of(part, pick))))
}

const SESSION: &str = "3c21b0c5-8113-4f55-9493-fa156c3fa369";

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// An ISO time for a transcript entry, `ago_ms` before now.
fn iso(ago_ms: u64) -> String {
    let secs = (now_ms() - ago_ms) / 1000;
    let (days, rest) = (secs / 86_400, secs % 86_400);
    // Days since 1970 to a civil date (Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.000Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// A Claude folder with one session's transcript: its model, an MCP server that connected and one that failed, and a
/// reply that used 100,000 tokens of context and a skill.
fn claude_with_session() -> PathBuf {
    // Its own folder each time: the tests run side by side.
    static COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let claude = std::env::temp_dir().canonicalize().unwrap().join(format!(
        "wings-native-hud-claude-{}-{}",
        std::process::id(),
        COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir_all(claude.join("projects/-x")).unwrap();
    let at = iso(1000);
    let lines = [
        json!({ "type": "attachment", "uuid": "a1", "timestamp": at, "attachment": { "type": "model", "identity": { "modelId": "claude-opus-5-5[1m]" } } }),
        json!({ "type": "attachment", "uuid": "a2", "timestamp": at, "attachment": { "type": "deferred_tools_delta",
            "addedNames": ["mcp__plugin_srv_files__read", "mcp__plugin_srv_files__write"], "removedNames": [],
            "failedMcpServers": [{ "name": "broken", "error": "timed out" }], "pendingMcpServers": [] } }),
        json!({ "type": "attachment", "uuid": "a3", "timestamp": at, "attachment": { "type": "invoked_skills", "skills": [{ "name": "simplify" }] } }),
        json!({ "type": "assistant", "uuid": "r1", "timestamp": at, "message": {
            "usage": { "input_tokens": 1000, "cache_creation_input_tokens": 2000, "cache_read_input_tokens": 97000 },
            "content": [{ "type": "text", "text": "done" }] } }),
    ];
    let text: Vec<String> = lines.iter().map(ToString::to_string).collect();
    fs::write(
        claude.join(format!("projects/-x/{SESSION}.jsonl")),
        text.join("\n"),
    )
    .unwrap();
    claude
}

fn claude_pane() -> super::PaneEntry {
    let mut claude = pane("1", "/work");
    claude.command = "claude".into();
    claude.session = Some(SessionEntry {
        session_id: SESSION.into(),
        kind: "claude".into(),
        name: Some("fix tests".into()),
        state: "idle".into(),
        started_at: now_ms() - 60_000,
        launcher: None,
    });
    claude
}

/// What a tool answered: its text, and whether it failed.
fn run_tool(rig: &Rig, name: &str, input: serde_json::Value) -> crate::page::runtime::Answer {
    let (reply, answer) = Reply::channel();
    rig.host.run_tool(name, input, None, reply);
    answer
}

fn tool_result(answer: crate::page::runtime::Answer) -> (String, bool) {
    let answer: serde_json::Value = serde_json::from_str(&answer.wait(super::testing::WAIT).expect("the tool's answer").expect("an answer")).unwrap();
    (answer["text"].as_str().unwrap().to_owned(), answer["isError"] == true)
}

/// The handler of the first `ListButton` that shows `text`.
fn list_button_showing(nodes: &[Node], text: &str) -> Option<String> {
    nodes.iter().find_map(|node| match node {
        Node::ListButton(row) if plain_text(&row.button.children).contains(text) => {
            row.button.on_click.clone()
        }
        other => other
            .parts()
            .into_iter()
            .find_map(|part| list_button_showing(part, text)),
    })
}

fn count_nodes(nodes: &[Node], is: &dyn Fn(&Node) -> bool) -> usize {
    nodes
        .iter()
        .map(|node| {
            usize::from(is(node))
                + node
                    .parts()
                    .into_iter()
                    .map(|part| count_nodes(part, is))
                    .sum::<usize>()
        })
        .sum()
}

fn chips_named(nodes: &[Node], label: &str) -> Option<String> {
    nodes.iter().find_map(|node| match node {
        Node::Chips(chips) if chips.label == label => chips.on_change.clone(),
        other => other
            .parts()
            .into_iter()
            .find_map(|part| chips_named(part, label)),
    })
}

/// A page of the plugin `host` runs, shown as a tab or block whose saved state starts as `initial`. What the page saves
/// lands in the returned list.
fn open_with_state(
    host: &super::PluginHost,
    source: &str,
    args: Option<&serde_json::Value>,
    place: super::StatePlace,
    initial: Option<serde_json::Value>,
) -> (
    PageRuntime,
    mpsc::Receiver<Event>,
    std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
) {
    let saved = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let state = super::FrameState::new(place, initial, {
        let saved = saved.clone();
        move |value| {
            saved.lock().unwrap().push(value);
            Ok(())
        }
    });
    let (sent, received) = mpsc::channel();
    let runtime = host
        .open_hidden_page_with_state(source, args, state, move |event| {
            let _ = sent.send(event);
        })
        .unwrap();
    (runtime, received, saved)
}

fn chips_value(nodes: &[Node], label: &str) -> Option<String> {
    nodes.iter().find_map(|node| match node {
        Node::Chips(chips) if chips.label == label => chips.value.clone(),
        other => other
            .parts()
            .into_iter()
            .find_map(|part| chips_value(part, label)),
    })
}
