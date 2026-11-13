//! The plugins of the repo's `plugins/` that ship a native build, run as the app runs them: their `dist/native` bundles
//! against a fake app and the manifest they declare, so a call a plugin makes meets the permission its manifest grants.

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
    testing::{Frame, Rig, Seen, WAIT, pane},
    ui::Tone,
};
use crate::page::{
    runtime::{Event, PageRuntime, Reply},
    tree::{Node, Tree, plain_text},
};

fn repo_plugin(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins")
        .join(name)
}

/// The plugin as the desktop loads it from the repo.
fn load(name: &str) -> Plugin {
    wings_plugins::set_wings_version(env!("WINGS_DESKTOP_VERSION"));
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
        if let Node::Button(button) = node
            && plain_text(&button.children) == label
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

#[test]
fn changes_lists_what_changed_opens_a_files_diff_through_main_and_searches_the_history() {
    let rig = Rig::with(load("changes"), false, vec![]);
    let (root, _) = worktree_repo(&rig);
    fs::write(root.join("README.md"), "fixture\nmore\n").unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/new.txt"), "one\ntwo\n").unwrap();
    *rig.env.projects.lock().unwrap() = vec![root.clone()];
    rig.host.publish_projects(vec![Project {
        id: root.to_string_lossy().into(),
        path: root.to_string_lossy().into(),
        name: "Fixture".into(),
        parent_id: None,
    }]);
    rig.host.publish_panes(vec![pane("1", &root.to_string_lossy())]);
    let (on_event, main_errors) = errors();
    rig.host.start_main(&module("changes", "main"), on_event).unwrap();
    // Main badges the pane with what changed in its repo.
    rig.env.wait("the badge", |seen| seen.iter().any(|seen| matches!(seen, Seen::Badge(_, _, Some(_)))));

    let (page, events) = open_page(&rig.host, &module("changes", "sidebar"));
    let files = tree(&events, "the changed files", |tree| tree.text().contains("README.md") && tree.text().contains("new.txt"));
    assert!(files.text().contains("2 files") && files.text().contains("+3"), "{}", files.text());
    // A file's row asks main for its diff, and main fills the viewer with the patch.
    let readme = handler_of(&files.children, &|node| match node {
        Node::ListButton(row) if plain_text(&row.button.children).contains("README.md") => row.button.on_click.clone(),
        _ => None,
    });
    assert!(page.dispatch(&readme.expect("README.md's row"), None));
    rig.env.wait("the diff viewer", |seen| seen.iter().any(|seen| matches!(seen, Seen::DiffUpdate(_, _, update) if update.patch.is_some())));

    // The History tab, and a search that matches nothing.
    let chips = handler_of(&files.children, &|node| match node {
        Node::Chips(chips) => chips.on_change.clone(),
        _ => None,
    });
    assert!(page.dispatch(&chips.expect("the chips"), Some(&json!({ "value": "history" }))));
    let history = tree(&events, "the history", |tree| tree.text().contains("fixture") && tree.text().contains("All branches"));
    let search = handler_of(&history.children, &|node| match node {
        Node::SearchField(field) => field.on_input.clone(),
        _ => None,
    });
    assert!(page.dispatch(&search.expect("the search box"), Some(&json!({ "value": "not-a-commit" }))));
    tree(&events, "no match", |tree| tree.text().contains("No commits match."));

    // The graph view asks main which repo it shows, and reads every branch of it.
    let (_graph, events) = open_page(&rig.host, &module("changes", "graph"));
    tree(&events, "the graph", |tree| tree.text().contains("1 commits on every branch"));
    assert!(main_errors.try_recv().is_err());
}

#[test]
fn worktrees_sidebar_loads_the_actual_native_bundle_refreshes_on_git_and_opens_a_linked_worktree() {
    let rig = Rig::with(load("worktrees"), true, vec![]);
    let (root, linked) = worktree_repo(&rig);
    *rig.env.projects.lock().unwrap() = vec![root.clone()];
    rig.host.publish_projects(vec![Project {
        id: root.to_string_lossy().into(),
        path: root.to_string_lossy().into(),
        name: "Fixture worktrees".into(),
        parent_id: None,
    }]);
    rig.host
        .publish_panes(vec![pane("1", &root.to_string_lossy())]);

    let (page, events) = open_page(&rig.host, &module("worktrees", "sidebar"));
    tree(&events, "the initial main worktree", |tree| {
        let text = tree.text();
        text.contains("Worktrees") && text.contains("main") && text.contains("Open in new pane")
    });

    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feature",
            linked.to_str().unwrap(),
        ],
    );
    *rig.env.projects.lock().unwrap() = vec![root.clone(), linked.clone()];
    rig.host.publish_projects(vec![
        Project {
            id: root.to_string_lossy().into(),
            path: root.to_string_lossy().into(),
            name: "Fixture worktrees".into(),
            parent_id: None,
        },
        Project {
            id: linked.to_string_lossy().into(),
            path: linked.to_string_lossy().into(),
            name: "Fixture feature".into(),
            parent_id: None,
        },
    ]);
    rig.host.publish_git(&[root.to_string_lossy().into()]);
    let refreshed = tree(&events, "the git refresh", |tree| {
        tree.text().contains("feature")
    });
    let mut open = Vec::new();
    buttons_named(&refreshed.children, "Open in new pane", &mut open);
    assert_eq!(open.len(), 2, "{}", refreshed.text());
    assert!(page.dispatch(&open[1], None));
    let seen = rig.env.wait("the linked worktree pane", |seen| {
        matches!(seen.last(), Some(Seen::OpenPane(_)))
    });
    let Seen::OpenPane(plan) = seen.last().unwrap() else {
        unreachable!()
    };
    assert_eq!(plan.cwd, linked.canonicalize().unwrap());
    assert_eq!(plan.project, linked);
    assert_eq!(plan.input, None);
    assert_eq!(plan.placement, "tab");
}

/// With one project the page's lookups never wait on each other. With many they do, and a page that wakes a waiting
/// task once for every time it was polled runs out of its turn.
#[test]
fn worktrees_sidebar_loads_forty_projects_within_its_turns() {
    let rig = Rig::with(load("worktrees"), true, vec![]);
    let base = rig.root.config("worktrees").root.join("many");
    let roots: Vec<PathBuf> = (0..40)
        .map(|n| {
            let root = base.join(format!("repo-{n:02}"));
            fs::create_dir_all(&root).unwrap();
            git(&root, &["init", "-q", "-b", "main"]);
            root
        })
        .collect();
    *rig.env.projects.lock().unwrap() = roots.clone();
    rig.host.publish_projects(
        roots
            .iter()
            .map(|root| Project {
                id: root.to_string_lossy().into(),
                path: root.to_string_lossy().into(),
                name: root.file_name().unwrap().to_string_lossy().into(),
                parent_id: None,
            })
            .collect(),
    );
    rig.host.publish_panes(roots.iter().take(10).enumerate().map(|(n, root)| pane(&n.to_string(), &root.to_string_lossy())).collect());
    let (_page, events) = open_page(&rig.host, &module("worktrees", "sidebar"));
    let loaded = tree(&events, "forty repos", |tree| tree.text().contains("repo-39"));
    assert!(loaded.text().contains("repo-00"));
}

#[test]
fn worktrees_sidebar_shows_the_native_empty_state_without_repos() {
    let rig = Rig::with(load("worktrees"), true, vec![]);
    let (_page, events) = open_page(&rig.host, &module("worktrees", "sidebar"));
    let empty = tree(&events, "the empty state", |tree| {
        tree.text().contains("None of your projects is a git repo.")
    });
    assert!(empty.text().contains("Scan this Mac"));
}

#[test]
fn worktrees_sidebar_reports_a_denied_scan_without_walking_any_disk() {
    let mut plugin = load("worktrees");
    plugin.manifest.permissions.disk_usage = false;
    let rig = Rig::with(plugin, true, vec![]);
    let (page, events) = open_page(&rig.host, &module("worktrees", "sidebar"));
    let initial = tree(&events, "the scan action", |tree| {
        tree.text().contains("Scan this Mac")
    });
    let mut scan = Vec::new();
    buttons_named(&initial.children, "Scan this Mac", &mut scan);
    assert_eq!(scan.len(), 1, "{}", initial.text());
    assert!(page.dispatch(&scan[0], None));
    let denied = tree(&events, "the refused scan", |tree| {
        tree.text().contains("worktrees may not read disk usage")
    });
    assert!(denied.text().contains("Elsewhere on this Mac"));
}

#[cfg(feature = "test-support")]
#[test]
fn worktrees_native_sidebar_bundle_draws_the_empty_state_in_a_native_window() {
    use crate::page::PageView;
    use gpui::{TestApp, TestAppWindow};
    use std::{
        sync::Arc,
        thread,
        time::{Duration, Instant},
    };

    let rig = Rig::with(load("worktrees"), true, vec![]);
    let host = std::sync::Arc::new(rig.host);
    let source: &'static str = Box::leak(module("worktrees", "sidebar").into_boxed_str());
    let mut app = TestApp::new();
    app.background_executor().allow_parking();
    let page_host = Arc::clone(&host);
    let mut window: TestAppWindow<PageView> =
        app.open_window(move |_, cx| PageView::in_plugin(&page_host, source, cx));
    window.update(|view, _, _| view.set_visible(true));
    let deadline = Instant::now() + WAIT;
    loop {
        app.run_until_parked();
        window.draw();
        if window.read(|view, _| view.text().contains("None of your projects is a git repo.")) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the window shows {:?}",
            window.read(|view, _| view.text())
        );
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn dev_servers_badges_the_pane_with_a_listening_port_and_its_button_opens_the_browser() {
    let rig = Rig::with(
        load("dev-servers"),
        false,
        vec![pane("1", "/work"), pane("2", "/other")],
    );
    *rig.env.ports.lock().unwrap() = Ok(vec![node("1", 5173), node("1", 3000)]);
    let (on_event, main_errors) = errors();
    rig.host
        .start_main(&module("dev-servers", "main"), on_event)
        .unwrap();

    // main looks as soon as it hears the panes: pane 1 gets a badge, and pane 2, which listens to nothing, none.
    let seen = rig.env.wait("the badges", |seen| seen.len() >= 2);
    let badge = seen
        .iter()
        .find_map(|seen| match seen {
            Seen::Badge(plugin, pane, Some(badge)) if plugin == "dev-servers" && pane == "1" => {
                Some(badge.clone())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("{seen:?}"));
    assert_eq!(badge.label, "localhost:3000 +1");
    assert_eq!(badge.tone, Tone::Neutral);
    assert_eq!(badge.title.as_deref(), Some("Listening on 2 ports"));
    let rows: Vec<_> = badge
        .rows
        .iter()
        .map(|row| (row.label.as_str(), row.value.as_str()))
        .collect();
    assert_eq!(rows, [("node", ":3000"), ("node", ":5173")]);
    let buttons: Vec<_> = badge
        .actions
        .iter()
        .map(|action| (action.id.as_str(), action.primary))
        .collect();
    assert_eq!(
        buttons,
        [
            ("browser:3000", true),
            ("browser:5173", false),
            ("refresh", false)
        ]
    );
    assert!(
        seen.contains(&Seen::Badge("dev-servers".into(), "2".into(), None)),
        "{seen:?}"
    );

    // The button opens the page in the browser, through the permission the manifest declares.
    assert_eq!(action(&rig, "1", "browser:5173"), Ok(()));
    assert_eq!(
        rig.env.seen().last(),
        Some(&Seen::Url("http://localhost:5173/".into()))
    );
    // A click on a badge a few seconds old is for a port that closed.
    *rig.env.ports.lock().unwrap() = Ok(vec![node("1", 3000)]);
    assert_eq!(
        action(&rig, "1", "browser:9999"),
        Err("That port isn't open any more.".into())
    );
    assert_eq!(
        rig.env
            .seen()
            .iter()
            .filter(|seen| matches!(seen, Seen::Url(_)))
            .count(),
        1
    );

    // The badge goes when the port closes, which Refresh looks for.
    *rig.env.ports.lock().unwrap() = Ok(Vec::new());
    assert_eq!(action(&rig, "1", "refresh"), Ok(()));
    let seen = rig.env.wait("the badge to go", |seen| {
        seen.iter()
            .filter(|seen| matches!(seen, Seen::Badge(_, pane, None) if pane == "1"))
            .count()
            == 1
    });
    assert!(!seen.is_empty());
    assert!(
        main_errors.try_recv().is_err(),
        "main reported {:?}",
        main_errors.try_recv()
    );
}

#[test]
fn dev_servers_cannot_reach_what_its_manifest_does_not_grant() {
    // The same main without its `ports` and `openUrl` grants: every call it makes is refused, not run.
    let mut plugin = load("dev-servers");
    plugin.manifest.permissions.ports = false;
    plugin.manifest.permissions.open_url.clear();
    let rig = Rig::with(plugin, false, vec![pane("1", "/work")]);
    *rig.env.ports.lock().unwrap() = Ok(vec![node("1", 5173)]);
    let frame = Frame::start(
        &rig.host,
        r#"
        const results = [];
        const show = () => wings.render(JSON.stringify({ id: 0, type: '#root', props: {}, children: [{ id: 1, type: '#text', text: JSON.stringify({ results }) }] }));
        wings.ports().catch((error) => error.message).then((m) => { results.push(m); return wings.openUrl('http://localhost:5173/'); })
          .catch((error) => error.message).then((m) => { results.push(m); show(); });
        "#,
    );
    let report = frame.report("both refusals", |r| {
        r["results"].as_array().is_some_and(|r| r.len() == 2)
    });
    assert_eq!(
        report["results"],
        json!([
            "dev-servers may not read listening ports",
            "dev-servers may not open http://localhost:5173/"
        ])
    );
    assert!(rig.env.seen().is_empty());
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
    });
    claude
}

#[test]
fn hud_draws_a_strip_under_a_claude_session_from_its_transcript() {
    let claude = claude_with_session();
    let rig = Rig::with(load("hud"), false, vec![claude_pane(), pane("2", "/other")]);
    *rig.env.claude.lock().unwrap() = vec![claude.clone()];
    let (on_event, main_errors) = errors();
    rig.host
        .start_main(&module("hud", "main"), on_event)
        .unwrap();

    let seen = rig.env.wait("the strip", |seen| {
        seen.iter()
            .any(|seen| matches!(seen, Seen::Status(_, _, Some(_))))
    });
    let Some(Seen::Status(plugin, pane, Some(status))) = seen
        .iter()
        .find(|seen| matches!(seen, Seen::Status(_, _, Some(_))))
    else {
        unreachable!()
    };
    assert_eq!((plugin.as_str(), pane.as_str()), ("hud", "1"));
    let items: Vec<_> = status
        .items
        .iter()
        .map(|item| (item.label.as_deref().unwrap(), item.value.as_str()))
        .collect();
    // No limits until Claude Code reports them (native hears no status lines), 100k of the 1M window used, and the
    // server that failed to connect.
    assert_eq!(
        items,
        [
            ("5h", "–"),
            ("wk", "–"),
            ("ctx", "10%"),
            ("MCP", "1 failed")
        ]
    );
    assert_eq!(status.items[2].percent, Some(10.0));
    assert_eq!(status.items[2].tone, Some(Tone::Success));
    assert_eq!(status.open.as_deref(), Some("hud"));
    let rows: Vec<_> = status
        .rows
        .iter()
        .map(|row| (row.label.as_str(), row.value.as_str()))
        .collect();
    assert!(
        rows.contains(&("Context", "100k of 1M tokens (10%)")),
        "{rows:?}"
    );
    assert!(rows.contains(&("Failed", "broken: timed out")), "{rows:?}");
    assert!(rows.contains(&("Skills (1)", "simplify")), "{rows:?}");
    assert!(
        main_errors.try_recv().is_err(),
        "main reported {:?}",
        main_errors.try_recv()
    );
    let _ = fs::remove_dir_all(claude);
}

#[test]
fn hud_sidebar_page_shows_what_main_read_and_each_frame_is_checked_against_hud_s_manifest() {
    let claude = claude_with_session();
    let rig = Rig::with(load("hud"), false, vec![claude_pane()]);
    *rig.env.claude.lock().unwrap() = vec![claude.clone()];
    let (on_event, main_errors) = errors();
    rig.host
        .start_main(&module("hud", "main"), on_event)
        .unwrap();

    let (sender, events) = mpsc::channel();
    let page = rig
        .host
        .open_page(&module("hud", "sidebar"), move |event| {
            let _ = sender.send(event);
        })
        .unwrap();
    let mut last = String::new();
    let tree = loop {
        match events
            .recv_timeout(WAIT)
            .unwrap_or_else(|_| panic!("the sidebar last showed {last:?}"))
        {
            Event::Tree(tree) if tree.text().contains("simplify") => break tree,
            Event::Tree(tree) => last = tree.text(),
            Event::Error { message, .. } => panic!("the sidebar failed: {message}"),
        }
    };
    let text = tree.text();
    for shown in [
        "Usage",
        "Context",
        "100k of 1M tokens",
        "broken",
        "timed out",
        "plugin_srv_files",
        "simplify",
    ] {
        assert!(
            text.contains(shown),
            "the sidebar shows {text:?}, not {shown:?}"
        );
    }
    assert!(
        main_errors.try_recv().is_err(),
        "main reported {:?}",
        main_errors.try_recv()
    );
    drop(page);
    let _ = fs::remove_dir_all(claude);
}

#[test]
fn hud_reads_the_transcript_only_for_the_types_its_manifest_names() {
    let claude = claude_with_session();
    let rig = Rig::with(load("hud"), false, vec![claude_pane()]);
    *rig.env.claude.lock().unwrap() = vec![claude.clone()];
    let result = rig.allowed(&format!(
        "const read = (types) => wings.transcript('{SESSION}', types).then((entries) => entries.length, (error) => error.message);
         return {{ attachments: await read(['attachment:model']), assistant: await read(['assistant']),
                  system: await read(['system']), status: await wings.statusline() }};"
    ));
    assert_eq!(result["attachments"], 1);
    assert_eq!(result["assistant"], 1);
    assert_eq!(
        result["system"],
        "hud may not read \"system\" transcript entries"
    );
    assert_eq!(
        result["status"],
        json!({ "rateLimits": [], "sessions": {} })
    );
    let _ = fs::remove_dir_all(claude);
}

/// Schedules with its real main and the project `/work/api`, named Api.
fn schedules_rig() -> (Rig, mpsc::Receiver<String>) {
    // Needs Wings 0.2.21, for `host::view_args`: use `load` once the app's version is bumped.
    let rig = Rig::with(load("schedules"), true, vec![]);
    let work = PathBuf::from("/work/api");
    *rig.env.projects.lock().unwrap() = vec![work.clone()];
    rig.host.publish_projects(vec![Project {
        id: "/work/api".into(),
        path: "/work/api".into(),
        name: "Api".into(),
        parent_id: None,
    }]);
    let (on_event, main_errors) = errors();
    rig.host
        .start_main(&module("schedules", "main"), on_event)
        .unwrap();
    // A tool call that comes before main has set up what it listens to gets no answer: ask until it has.
    let started = (0..40).any(|_| {
        let (reply, answer) = Reply::channel();
        rig.host.run_tool("list", json!({}), None, reply);
        answer.wait(std::time::Duration::from_millis(500)).is_some()
    });
    assert!(started, "main never answered the list tool");
    (rig, main_errors)
}

#[test]
fn schedules_main_serves_its_native_page_and_a_schedule_added_in_the_form_is_listed() {
    let (rig, main_errors) = schedules_rig();

    let (page, events) = open_page(&rig.host, &module("schedules", "schedules"));
    let empty = tree(&events, "the empty list", |tree| {
        tree.text().contains("Nothing set up yet")
    });
    let mut add = Vec::new();
    buttons_named(&empty.children, "Add entry", &mut add);
    assert!(page.dispatch(&add[0], None));

    let form = tree(&events, "the form", |tree| {
        tree.text().contains("Start an agent") && input_named(&tree.children, "Prompt").is_some()
    });
    for (label, value) in [
        ("Name", "Morning review"),
        ("Prompt", "Check yesterday's CI"),
    ] {
        let handler = input_named(&form.children, label).expect(label);
        assert!(page.dispatch(&handler, Some(&json!({ "value": value }))));
    }
    let filled = tree(&events, "the typed text", |tree| {
        let value = |label: &str| {
            fn find(nodes: &[Node], label: &str) -> Option<String> {
                nodes.iter().find_map(|node| match node {
                    Node::Input(input) if input.label == label => Some(input.value.clone()),
                    other => other.parts().into_iter().find_map(|part| find(part, label)),
                })
            }
            find(&tree.children, label)
        };
        value("Name").as_deref() == Some("Morning review")
            && value("Prompt").as_deref() == Some("Check yesterday's CI")
    });
    let mut save = Vec::new();
    buttons_named(&filled.children, "Add entry", &mut save);
    assert!(page.dispatch(&save[0], None));

    let listed = tree(&events, "the saved schedule", |tree| {
        let text = tree.text();
        text.contains("Morning review") && text.contains("Next run")
    });
    assert!(
        listed.text().contains("Api · Claude Code"),
        "{}",
        listed.text()
    );
    // It is kept under the keys the TypeScript plugin used, which a saved schedule still reads.
    let saved = rig.root.saved("schedules", "schedules").unwrap();
    assert_eq!(saved[0]["name"], "Morning review");
    assert_eq!(saved[0]["when"], json!({ "kind": "daily", "time": "09:00", "days": [1, 2, 3, 4, 5] }));
    assert_eq!(saved[0]["agent"], "claude");
    assert!(
        main_errors.try_recv().is_err(),
        "main reported {:?}",
        main_errors.try_recv()
    );
}

#[test]
fn schedules_add_tool_waits_for_the_users_allow_in_the_modal_and_saves_nothing_when_denied() {
    let (rig, main_errors) = schedules_rig();
    let ask = |input: serde_json::Value| {
        let (reply, answer) = Reply::channel();
        rig.host.run_tool("add", input, None, reply);
        answer
    };
    let finished = |answer: crate::page::runtime::Answer| {
        let answer: serde_json::Value = serde_json::from_str(&answer.wait(super::testing::WAIT).expect("the tool's answer").expect("an answer")).unwrap();
        (answer["text"].as_str().unwrap().to_owned(), answer["isError"] == true)
    };
    // The tool's input is checked before anything is asked.
    let (bad, was_error) = finished(ask(json!({ "prompt": "Check CI", "project": "Api", "time": "9:00" })));
    assert!(was_error && bad.starts_with("time must match the pattern"), "{bad}");

    let first = ask(json!({ "prompt": "Check yesterday's CI", "project": "Api", "time": "08:30", "days": ["mon", "fri"] }));
    rig.env.wait("the modal", |seen| seen.iter().any(|seen| matches!(seen, Seen::OpenView(open) if open.id == "approve")));
    let (page, events) = open_page(&rig.host, &module("schedules", "approve"));
    let ask_shown = tree(&events, "the ask", |tree| tree.text().contains("asks to add a schedule in Api"));
    let text = ask_shown.text();
    for shown in ["Mon, Fri at 08:30", "Claude Code", "Check yesterday's CI"] {
        assert!(text.contains(shown), "{shown:?} in {text:?}");
    }
    // Allow waits a second after the ask shows, so a click meant for something else can't land on it.
    let armed = tree(&events, "Allow", |tree| {
        let mut allow = Vec::new();
        buttons_named(&tree.children, "Allow", &mut allow);
        !allow.is_empty()
    });
    let mut deny = Vec::new();
    buttons_named(&armed.children, "Deny", &mut deny);
    assert!(page.dispatch(&deny[0], None));
    let (denied, was_error) = finished(first);
    assert!(was_error && denied.starts_with("The user denied it in Wings"), "{denied}");
    rig.env.wait("the modal to close", |seen| seen.iter().any(|seen| matches!(seen, Seen::CloseView(_, view) if view == "approve")));
    assert_eq!(rig.root.saved("schedules", "schedules"), None);

    let second = ask(json!({ "prompt": "Check yesterday's CI", "project": "Api", "every_hours": 6 }));
    let again = tree(&events, "the second ask", |tree| {
        let mut allow = Vec::new();
        buttons_named(&tree.children, "Allow", &mut allow);
        tree.text().contains("Every 6 hours") && !allow.is_empty()
    });
    let mut allow = Vec::new();
    buttons_named(&again.children, "Allow", &mut allow);
    assert!(page.dispatch(&allow[0], None));
    let (added, was_error) = finished(second);
    assert!(!was_error && added.contains("in Api: Every 6 hours, next run"), "{added}");
    assert_eq!(rig.root.saved("schedules", "schedules").unwrap()[0]["when"], json!({ "kind": "every", "hours": 6 }));
    assert!(main_errors.try_recv().is_err(), "main reported {:?}", main_errors.try_recv());
}

/// Schedules with an assigned entry `w1` of the project Api, a real repo, that has the ticket 42 waiting. Its agent
/// starts only if `agents_start`, or if the user allows it.
fn schedules_start_rig(agents_start: bool) -> (Rig, mpsc::Receiver<String>) {
    let dir = crate::projects::test_dir("schedules-start");
    let repo = crate::projects::test_repo(&dir);
    let file = dir.join("spaces.json");
    std::fs::write(&file, serde_json::to_vec(&json!([{ "id": repo, "name": "Api", "path": repo }])).unwrap()).unwrap();
    let rig = Rig::with(load("schedules"), true, vec![]);
    *rig.env.spaces.lock().unwrap() = crate::projects::Projects::open(Some(file), repo.clone());
    // A pane opens only inside a project, and the app lists the worktrees a start makes by then.
    *rig.env.projects.lock().unwrap() = vec![repo.clone(), dir.join("app-worktrees")];
    let id: String = repo.to_string_lossy().into();
    rig.host.publish_projects(vec![Project { id: id.clone(), path: id.clone(), name: "Api".into(), parent_id: None }]);
    let mut entry = json!({
        "id": "w1", "projectId": id, "agent": "claude", "prompt": "Work on {id}: {title}", "minFreeGb": 0, "enabled": false, "primed": true, "seen": ["42"],
        "waiting": [{ "id": "42", "title": "Fix totals", "url": "https://example.test/42", "at": 1 }, { "id": "43", "title": "Add tax rows", "url": "https://example.test/43", "at": 2 }],
    });
    if agents_start {
        entry["agentsStart"] = json!(true);
    }
    rig.root.save("schedules", "assigned", json!([entry]));
    let (on_event, main_errors) = errors();
    rig.host.start_main(&module("schedules", "main"), on_event).unwrap();
    let started = (0..40).any(|_| {
        let (reply, answer) = Reply::channel();
        rig.host.run_tool("list", json!({}), None, reply);
        answer.wait(std::time::Duration::from_millis(500)).is_some()
    });
    assert!(started, "main never answered the list tool");
    (rig, main_errors)
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

fn agents_started(rig: &Rig) -> usize {
    rig.env.seen().iter().filter(|seen| matches!(seen, Seen::OpenPane(_))).count()
}

fn worktrees(rig: &Rig) -> usize {
    rig.env.spaces.lock().unwrap().entries().iter().filter(|p| p.child).count()
}

fn asked(rig: &Rig) -> bool {
    rig.env.seen().iter().any(|seen| matches!(seen, Seen::OpenView(open) if open.id == "approve"))
}

/// The Allow or Deny button of the modal, once it shows the ask for `ticket` and Allow is armed.
fn answer_button(events: &mpsc::Receiver<Event>, label: &str, ticket: &str) -> String {
    let armed = tree(events, label, |tree| {
        let mut allow = Vec::new();
        buttons_named(&tree.children, "Allow", &mut allow);
        tree.text().contains(&format!("asks to start an agent on {ticket}")) && !allow.is_empty()
    });
    let mut found = Vec::new();
    buttons_named(&armed.children, label, &mut found);
    found.remove(0)
}

#[test]
fn schedules_start_tool_asks_in_the_modal_and_starts_nothing_when_denied_and_starts_when_allowed() {
    let (rig, main_errors) = schedules_start_rig(false);
    // A start that would be refused isn't asked about.
    let (refused, was_error) = tool_result(run_tool(&rig, "start", json!({ "id": "w1", "ticket": "7" })));
    assert!(was_error && refused.contains("isn't waiting"), "{refused}");
    assert!(!asked(&rig));

    let first = run_tool(&rig, "start", json!({ "id": "w1", "ticket": "#42" }));
    rig.env.wait("the modal", |seen| seen.iter().any(|seen| matches!(seen, Seen::OpenView(open) if open.id == "approve")));
    let (page, events) = open_page(&rig.host, &module("schedules", "approve"));
    let shown = tree(&events, "the ask", |tree| tree.text().contains("asks to start an agent on #42 Fix totals in Api."));
    let text = shown.text();
    for part in ["Ticket", "#42 Fix totals", "Claude Code", "Work on 42: Fix totals", "It makes a worktree and opens the agent with this prompt.", "without asking, turn that on in the entry on the Schedules page"] {
        assert!(text.contains(part), "{part:?} in {text:?}");
    }
    // Nothing is made while the ask waits.
    assert_eq!((agents_started(&rig), worktrees(&rig)), (0, 0));
    assert!(page.dispatch(&answer_button(&events, "Deny", "#42"), None));
    let (denied, was_error) = tool_result(first);
    assert!(was_error && denied.starts_with("The user denied it in Wings") && denied.contains("Nothing started."), "{denied}");
    assert_eq!((agents_started(&rig), worktrees(&rig)), (0, 0));

    let second = run_tool(&rig, "start", json!({ "id": "w1", "ticket": "43" }));
    let button = answer_button(&events, "Allow", "#43");
    assert!(page.dispatch(&button, None));
    let (started, was_error) = tool_result(second);
    assert!(!was_error && started.starts_with("Started Add tax rows on "), "{started}");
    assert_eq!((agents_started(&rig), worktrees(&rig)), (1, 1));
    // A change by a tool leaves the entry asking.
    let (changed, was_error) = tool_result(run_tool(&rig, "update", json!({ "id": "w1", "name": "Totals" })));
    assert!(!was_error, "{changed}");
    assert!(rig.root.saved("schedules", "assigned").unwrap()[0].get("agentsStart").is_none());
    assert!(main_errors.try_recv().is_err(), "main reported {:?}", main_errors.try_recv());
}

#[test]
fn schedules_start_tool_starts_without_asking_for_an_entry_that_lets_agents_start_and_a_tool_keeps_that() {
    let (rig, main_errors) = schedules_start_rig(true);
    let (started, was_error) = tool_result(run_tool(&rig, "start", json!({ "id": "w1", "ticket": "42" })));
    assert!(!was_error && started.starts_with("Started Fix totals on "), "{started}");
    assert!(!asked(&rig));
    assert_eq!((agents_started(&rig), worktrees(&rig)), (1, 1));
    let (changed, was_error) = tool_result(run_tool(&rig, "update", json!({ "id": "w1", "prompt": "Do {id}" })));
    assert!(!was_error, "{changed}");
    assert_eq!(rig.root.saved("schedules", "assigned").unwrap()[0]["agentsStart"], json!(true));
    assert!(main_errors.try_recv().is_err(), "main reported {:?}", main_errors.try_recv());
}

#[test]
fn schedules_add_assigned_tool_makes_an_entry_whose_starts_ask() {
    let (rig, main_errors) = schedules_rig();
    let (added, was_error) = tool_result(run_tool(&rig, "add_assigned", json!({ "project": "Api" })));
    assert!(!was_error && added.starts_with("Added "), "{added}");
    assert!(rig.root.saved("schedules", "assigned").unwrap()[0].get("agentsStart").is_none());
    assert!(main_errors.try_recv().is_err(), "main reported {:?}", main_errors.try_recv());
}

#[test]
fn schedules_approve_modal_with_nothing_waiting_says_so() {
    let (rig, _) = schedules_rig();
    let (_page, events) = open_page(&rig.host, &module("schedules", "approve"));
    tree(&events, "the empty modal", |tree| tree.text().contains("Nothing is waiting"));
}

/// The handler of the button whose accessible name is `label`.
fn button_labelled(nodes: &[Node], label: &str) -> Option<String> {
    nodes.iter().find_map(|node| match node {
        Node::Button(button) if button.label.as_deref() == Some(label) => button.on_click.clone(),
        other => other
            .parts()
            .into_iter()
            .find_map(|part| button_labelled(part, label)),
    })
}

fn sunburst_in(nodes: &[Node]) -> Option<&crate::page::tree::Sunburst> {
    nodes.iter().find_map(|node| match node {
        Node::Sunburst(map) => Some(map),
        other => other.parts().into_iter().find_map(sunburst_in),
    })
}

#[test]
fn disk_scans_a_folder_in_its_main_and_its_native_page_draws_the_map_and_fills_the_collector() {
    let rig = Rig::with(load("disk"), true, vec![]);
    // Files of at least the 1 MiB a folder scan keeps, written out so that they take space on disk.
    let folder = rig.root.config("disk").root.join("scanned");
    for (path, mib) in [("Downloads/installer.dmg", 3), ("Code/app.bin", 2)] {
        let file = folder.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, vec![7u8; mib << 20]).unwrap();
    }
    let id: String = folder.to_string_lossy().into();
    rig.host.publish_projects(vec![Project {
        id: id.clone(),
        path: id,
        name: "Scanned".into(),
        parent_id: None,
    }]);
    let (on_event, main_errors) = errors();
    rig.host
        .start_main(&module("disk", "main"), on_event)
        .unwrap();

    let (page, events) = open_page(&rig.host, &module("disk", "map"));
    let overview = tree(&events, "the disks and folders", |tree| {
        let text = tree.text();
        text.contains("Disks") && text.contains("Scanned") && !text.contains("Looking for disks")
    });
    let scan = button_labelled(&overview.children, "Scan Scanned").expect("a Scan button");
    assert!(page.dispatch(&scan, None));

    let mapped = tree(&events, "the map of the folder", |tree| {
        sunburst_in(&tree.children).is_some() && tree.text().contains("Downloads")
    });
    let map = sunburst_in(&mapped.children).unwrap();
    // Two folders of one file each: a wedge for each folder and one for its file.
    assert!(map.wedges.len() >= 4, "{:?}", map.wedges);
    assert!(mapped.text().contains(" MB"), "{}", mapped.text());

    // Pointing at a wedge says what it is in the middle, and a row's menu puts its folder in the Collector.
    let on_point = map.on_point.clone().expect("a pointer handler");
    assert!(page.dispatch(&on_point, Some(&json!({ "index": 0 }))));
    tree(&events, "the wedge's name in the middle", |tree| {
        sunburst_in(&tree.children).is_some_and(|map| map.title != "scanned")
    });
    let mut menus = Vec::new();
    buttons_named(&mapped.children, "⋯", &mut menus);
    assert!(page.dispatch(&menus[0], None));
    let open = tree(&events, "the row's menu", |tree| {
        tree.text().contains("Add to Collector")
    });
    let mut add = Vec::new();
    buttons_named(&open.children, "Add to Collector", &mut add);
    assert!(page.dispatch(&add[0], None));
    tree(&events, "the Collector with the folder", |tree| {
        tree.text().contains("1 item,")
    });
    assert!(
        main_errors.try_recv().is_err(),
        "main reported {:?}",
        main_errors.try_recv()
    );
}

/// A folder of 5,000 files, each a block on disk, which the map asks Wings for in one answer: the page takes it in its
/// turn and draws the first rows of the list, with a button for the rest.
#[test]
fn disk_page_takes_a_folder_of_five_thousand_files_in_one_turn_and_draws_the_first_rows() {
    let rig = Rig::with(load("disk"), true, vec![]);
    let folder = rig.root.config("disk").root.join("many");
    fs::create_dir_all(&folder).unwrap();
    for n in 0..5000 {
        fs::write(folder.join(format!("file-{n:04}.txt")), "x").unwrap();
    }
    let id: String = folder.to_string_lossy().into();
    rig.host.publish_projects(vec![Project { id: id.clone(), path: id, name: "Many".into(), parent_id: None }]);
    let (on_event, main_errors) = errors();
    rig.host.start_main(&module("disk", "main"), on_event).unwrap();

    let (page, events) = open_page(&rig.host, &module("disk", "map"));
    let overview = tree(&events, "the disks and folders", |tree| tree.text().contains("Many") && !tree.text().contains("Looking for disks"));
    let scan = button_labelled(&overview.children, "Scan Many").expect("a Scan button");
    assert!(page.dispatch(&scan, None));
    let mapped = tree(&events, "the first rows of the folder", |tree| tree.text().contains("Show more (4880)"));
    assert!(mapped.text().contains("file-"), "{}", mapped.text());
    assert!(main_errors.try_recv().is_err(), "main reported {:?}", main_errors.try_recv());
}

/// 12,000 files of one block each are more than a folder's answer keeps, so they are all in its smaller items: opening
/// them asks Wings for every one, which the page takes in its turn and draws the first rows of.
#[test]
fn disk_page_opens_the_smaller_items_of_a_folder_of_twelve_thousand_files() {
    let rig = Rig::with(load("disk"), true, vec![]);
    let folder = rig.root.config("disk").root.join("most");
    fs::create_dir_all(&folder).unwrap();
    for n in 0..12_000 {
        fs::write(folder.join(format!("file-{n:05}.txt")), "x").unwrap();
    }
    let id: String = folder.to_string_lossy().into();
    rig.host.publish_projects(vec![Project { id: id.clone(), path: id, name: "Most".into(), parent_id: None }]);
    let (on_event, main_errors) = errors();
    rig.host.start_main(&module("disk", "main"), on_event).unwrap();

    let (page, events) = open_page(&rig.host, &module("disk", "map"));
    let overview = tree(&events, "the disks and folders", |tree| tree.text().contains("Most") && !tree.text().contains("Looking for disks"));
    assert!(page.dispatch(&button_labelled(&overview.children, "Scan Most").expect("a Scan button"), None));
    let mapped = tree(&events, "the folder with its smaller items", |tree| tree.text().contains("12,000 smaller items"));
    let open = list_button_showing(&mapped.children, "12,000 smaller items").expect("the smaller items row");
    assert!(page.dispatch(&open, None));
    tree(&events, "the first rows of the smaller items", |tree| tree.text().contains("Show more (11881)"));
    assert!(main_errors.try_recv().is_err(), "main reported {:?}", main_errors.try_recv());
}

// PR tracker: its Rust modules, over a fake gh and az that answer with made-up pull requests. Each test runs in a process
// of its own where those are first on PATH, so no test reaches a forge even when the Mac has the real CLIs.

/// The made-up answers of the fake `gh` and `az`, by the words in the call.
const FAKE_PR_GH: &str = include_str!("../../../examples/shot_plugins/pr_tracker_gh.json");
const FAKE_PR_AZ: &str = include_str!("../../../examples/shot_plugins/pr_tracker_az.json");
/// Set in the process where `gh` and `az` are the fakes.
const FAKE_PR: &str = "WINGS_TEST_FAKE_PR";

/// The folder of the fake `gh` and `az`: scripts that print the answer of the first key of their answers that the call has
/// the words of (a string as it is, anything else as JSON), log each call to the file `WINGS_FAKE_LOG` names when it is
/// set, and fail as a signed-out program does while `WINGS_FAKE_SIGNED_OUT` is set.
fn fake_pr_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wings-fake-pr-{name}-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    for (program, answers, signed_out) in [
        ("gh", FAKE_PR_GH, "To get started with GitHub CLI, please run:  gh auth login"),
        ("az", FAKE_PR_AZ, "ERROR: Please run 'az login' to setup account."),
    ] {
        let answers: serde_json::Map<String, serde_json::Value> = serde_json::from_str(answers).unwrap();
        let mut script = format!(
            "#!/bin/sh\n[ -n \"$WINGS_FAKE_LOG\" ] && echo \"{program} $*\" >> \"$WINGS_FAKE_LOG\"\n[ -n \"$WINGS_FAKE_SIGNED_OUT\" ] && {{ echo '{signed_out}' >&2; exit 1; }}\ncase \"$*\" in\n"
        );
        for (index, (words, answer)) in answers.iter().filter(|(words, _)| !words.starts_with('_')).enumerate() {
            let file = dir.join(format!("{program}-{index}.out"));
            fs::write(&file, answer.as_str().map_or_else(|| answer.to_string(), str::to_owned)).unwrap();
            script += &format!("  *'{words}'*) cat '{}' ;;\n", file.display());
        }
        script += "  *) echo \"no made-up answer for the call\" >&2; exit 1 ;;\nesac\n";
        let path = dir.join(program);
        fs::write(&path, script).unwrap();
        assert!(Command::new("chmod").arg("+x").arg(&path).status().unwrap().success());
    }
    dir
}

/// Runs the `#[ignore]`d test `name` of this file in a process whose `gh` and `az` are the fakes (see `with_fake_az` for why
/// a process of its own), with `env` set in it.
fn with_fake_pr_env(name: &str, env: &[(&str, &str)]) {
    let dir = fake_pr_dir(name);
    let test = format!("{}::{name}", module_path!().split_once("::").unwrap().1);
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--ignored", "--exact", &test, "--nocapture"])
        .env(FAKE_PR, &dir)
        .env("WINGS_FAKE_LOG", dir.join("calls.log"))
        .env("SHELL", "/nonexistent-shell")
        .env("PATH", format!("{}:{}", dir.display(), std::env::var("PATH").unwrap_or_default()));
    for (key, value) in env {
        command.env(key, value);
    }
    let output = command.output().unwrap();
    let calls = fs::read_to_string(dir.join("calls.log")).unwrap_or_default();
    let _ = fs::remove_dir_all(&dir);
    let said = format!("{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success() && said.contains("1 passed"), "{test}: {said}\nthe fake programs were called with:\n{calls}");
}

fn with_fake_pr(name: &str) {
    with_fake_pr_env(name, &[]);
}

/// PR tracker in a project that is a git repo on `ada/sync-retry` with a GitHub origin, with the fake `gh` and `az`.
fn pr_rig() -> (Rig, PathBuf) {
    assert!(std::env::var_os(FAKE_PR).is_some(), "run it through its test, which starts a process with the fake gh and az");
    let rig = Rig::with(load("pr-tracker"), true, vec![]);
    let repo = rig.root.config("pr-tracker").root.join("web");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Wings test"]);
    git(&repo, &["remote", "add", "origin", "git@github.com:acme/web.git"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "fixture"]);
    git(&repo, &["branch", "-M", "ada/sync-retry"]);
    let path = repo.to_string_lossy().to_string();
    *rig.env.projects.lock().unwrap() = vec![repo.clone()];
    rig.host.publish_projects(vec![Project { id: path.clone(), path, name: "web".into(), parent_id: None }]);
    // The project on screen, which the app always says and the pages follow.
    rig.host.publish_project(Some(&repo));
    (rig, repo)
}

/// What a "pr" tab is opened with for the fixture's GitHub pull request.
fn pr_args(repo: &Path) -> serde_json::Value {
    json!({
        "provider": "github", "url": "https://github.com/acme/web/pull/482", "number": 482,
        "repo": "acme/web", "title": "Add retry with backoff to the sync client", "cwd": repo.to_string_lossy(),
    })
}

/// Opens the "pr" tab with `args` as the app does.
fn open_pr_tab(rig: &Rig, args: &serde_json::Value) -> (PageRuntime, mpsc::Receiver<Event>) {
    let (page, events, _saved) = open_with_state(&rig.host, &module("pr-tracker", "pr"), Some(args), super::StatePlace::Tab, None);
    (page, events)
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

#[test]
fn pr_tracker_tab_opened_with_args_shows_that_pull_request_and_its_checks() {
    with_fake_pr("pr_tracker_tab_opened_with_args_shows_that_pull_request_and_its_checks_with_the_fakes");
}

#[test]
#[ignore = "started by its test without the _with_the_fakes, in a process with a fake gh and az"]
fn pr_tracker_tab_opened_with_args_shows_that_pull_request_and_its_checks_with_the_fakes() {
    let (rig, repo) = pr_rig();
    let (page, events) = open_pr_tab(&rig, &pr_args(&repo));
    let pull = tree(&events, "the pull request", |tree| tree.text().contains("Alan Turing"));
    let text = pull.text();
    for shown in ["acme/web #482", "exponential backoff", "const delay = Math.min(base * 2 ** attempt, 30_000);", "Setting", "Equal jitter keeps a floor on the delay.", "ci-bot[bot]"] {
        assert!(text.contains(shown), "{shown:?} in {text:?}");
    }
    assert!(!text.contains("All pull requests"), "a tab opened on a pull request has no list to go back to");
    let tabs = chips_named(&pull.children, "Pull request").expect("the tabs");
    assert!(page.dispatch(&tabs, Some(&json!({ "value": "checks" }))));
    let checks = tree(&events, "the checks", |tree| tree.text().contains("Unit tests"));
    let text = checks.text();
    // `gh` doesn't say which checks branch protection requires, so there are no Required and Optional groups.
    for shown in ["Failed", "Unit tests", "Running and queued", "End-to-end", "Passed"] {
        assert!(text.contains(shown), "{shown:?} in {text:?}");
    }
}

/// The width of the page's reading column.
fn column_width(nodes: &[Node]) -> Option<f32> {
    nodes.iter().find_map(|node| match node {
        Node::Stack(stack) if stack.max_width.is_some() => stack.max_width,
        other => other.parts().into_iter().find_map(column_width),
    })
}

/// The name of the step a run shows the log of: the one step whose name is bold.
fn picked_step(nodes: &[Node]) -> Option<String> {
    use crate::page::tree::Weight;
    nodes.iter().find_map(|node| match node {
        Node::ListButton(row) => row.button.children.iter().find_map(|child| match child {
            Node::Text(name) if name.truncate && name.weight == Weight::Strong => {
                Some(plain_text(&name.children))
            }
            _ => None,
        }),
        other => other.parts().into_iter().find_map(picked_step),
    })
}

/// A check's run: its log's numbers are one column, each as many characters as the last one's, with a folded group's
/// number in that column and not in the button that opens the group; the page is wider than its reading column while
/// the run is open; and a step picked says it's loading at once. It went on showing the step before's log until its
/// own came, so a click seemed to do nothing for as long as the CLI took.
#[test]
fn pr_tracker_run_numbers_its_lines_in_a_column_widens_the_page_and_loads_a_picked_step_at_once() {
    with_fake_pr("pr_tracker_run_numbers_its_lines_in_a_column_widens_the_page_and_loads_a_picked_step_at_once_with_the_fakes");
}

#[test]
#[ignore = "started by its test without the _with_the_fakes, in a process with a fake gh and az"]
fn pr_tracker_run_numbers_its_lines_in_a_column_widens_the_page_and_loads_a_picked_step_at_once_with_the_fakes() {
    let (rig, repo) = pr_rig();
    let (page, events) = open_pr_tab(&rig, &pr_args(&repo));
    let pull = tree(&events, "the pull request", |tree| {
        tree.text().contains("Conversation")
    });
    let tabs = chips_named(&pull.children, "Pull request").expect("the tabs");
    assert!(page.dispatch(&tabs, Some(&json!({ "value": "checks" }))));
    let checks = tree(&events, "the checks", |tree| {
        tree.text().contains("Unit tests")
    });
    assert_eq!(column_width(&checks.children), Some(860.0));
    let open = list_button_showing(&checks.children, "Unit tests").expect("the failed check");
    assert!(page.dispatch(&open, None));
    let run = tree(&events, "the failed step's log", |tree| {
        tree.text().contains("Process completed with exit code 1.")
    });
    assert_eq!(column_width(&run.children), Some(1100.0));

    fn numbers(nodes: &[Node], found: &mut Vec<String>) {
        for node in nodes {
            if let Node::Text(text) = node
                && text.mono
                && text.size == crate::page::tree::TextSize::Tiny
            {
                found.push(plain_text(&text.children));
            }
            for part in node.parts() {
                numbers(part, found);
            }
        }
    }
    let mut found = Vec::new();
    numbers(&run.children, &mut found);
    // 1 and 9 open folded groups; 2 and 10 to 103 are folded in them. The GitHub job's log has one more line after them
    // than the Azure DevOps log the fixture had before.
    let shown: Vec<String> = [1, 3, 4, 5, 6, 7, 8, 9]
        .into_iter()
        .chain(104..=125)
        .map(|n| format!("{n:>3}"))
        .collect();
    assert_eq!(found, shown);
    fn group_rows(nodes: &[Node], found: &mut Vec<(String, String)>) {
        for node in nodes {
            if let Node::Row(row) = node
                && let [Node::Text(number), Node::ListButton(button)] = row.children.as_slice()
            {
                found.push((
                    plain_text(&number.children),
                    plain_text(&button.button.children),
                ));
            }
            for part in node.parts() {
                group_rows(part, found);
            }
        }
    }
    let mut groups = Vec::new();
    group_rows(&run.children, &mut groups);
    assert_eq!(
        groups,
        [
            ("  1".to_owned(), "Run pnpm test".to_owned()),
            ("  9".to_owned(), "Install dependencies".to_owned())
        ]
    );

    assert_eq!(picked_step(&run.children).as_deref(), Some("Unit tests"));
    let install = list_button_showing(&run.children, "Install dependencies").expect("the step");
    assert!(page.dispatch(&install, None));
    let picked = tree(&events, "the step picked", |tree| {
        picked_step(&tree.children).as_deref() == Some("Install dependencies")
    });
    let text = picked.text();
    assert!(
        text.contains("Loading the log…") && !text.contains("Process completed with exit code 1."),
        "{text:?}"
    );
    tree(&events, "the picked step's log", |tree| {
        tree.text().contains("Packages: +812")
    });

    let mut back = Vec::new();
    buttons_named(&run.children, "← Checks", &mut back);
    assert!(page.dispatch(&back[0], None));
    let checks = tree(&events, "the checks again", |tree| {
        tree.text().contains("Running and queued")
    });
    assert_eq!(column_width(&checks.children), Some(860.0));
}

/// The Checks tab of the fixture's Azure DevOps pull request draws each check's mark as the desktop's: a red cross for
/// a failed one, an amber dot for one running, a ring for one queued. They were text glyphs, drawn thin. Every row
/// keeps an Open slot as wide as the button, so the times end in one column whether or not a row has a link.
#[test]
fn pr_tracker_checks_draw_each_checks_mark_and_line_their_times_up() {
    with_fake_pr("pr_tracker_checks_draw_each_checks_mark_and_line_their_times_up_with_the_fakes");
}

#[test]
#[ignore = "started by its test without the _with_the_fakes, in a process with a fake gh and az"]
fn pr_tracker_checks_draw_each_checks_mark_and_line_their_times_up_with_the_fakes() {
    let (rig, repo) = pr_rig();
    let args = json!({
        "provider": "azure", "url": "https://dev.azure.com/acme/Web/_git/web/pullrequest/482", "number": 482,
        "repo": "Web/web", "title": "Add retry with backoff to the sync client", "cwd": repo.to_string_lossy(),
        "remote": { "kind": "azure", "org": "https://dev.azure.com/acme", "project": "Web", "repo": "web" },
    });
    let (page, events) = open_pr_tab(&rig, &args);
    let pull = tree(&events, "the pull request", |tree| {
        tree.text().contains("Conversation")
    });
    let tabs = chips_named(&pull.children, "Pull request").expect("the tabs");
    assert!(page.dispatch(&tabs, Some(&json!({ "value": "checks" }))));
    let checks = tree(&events, "the checks", |tree| {
        tree.text().contains("web-storybook")
    });
    fn marks<'a>(nodes: &'a [Node], found: &mut Vec<&'a crate::page::tree::Svg>) {
        for node in nodes {
            match node {
                Node::Svg(mark)
                    if matches!(
                        mark.label.as_deref(),
                        Some("Failed" | "Running" | "Queued" | "Passed" | "Not run")
                    ) =>
                {
                    found.push(mark)
                }
                other => other
                    .parts()
                    .into_iter()
                    .for_each(|part| marks(part, found)),
            }
        }
    }
    let mut found = Vec::new();
    marks(&checks.children, &mut found);
    let shown: Vec<_> = found
        .iter()
        .map(|mark| (mark.label.as_deref().unwrap(), mark.tone))
        .collect();
    use crate::page::tree::Tone;
    // The open groups: the failed build, then what's running before what's queued.
    assert_eq!(
        shown,
        [
            ("Failed", Some(Tone::Danger)),
            ("Running", Some(Tone::Warn)),
            ("Running", Some(Tone::Warn)),
            ("Queued", Some(Tone::Muted)),
            ("Queued", Some(Tone::Muted)),
            ("Queued", Some(Tone::Muted)),
        ]
    );
    assert!(
        found[0]
            .markup
            .contains(r#"<path d="M3.5 3.5l9 9M12.5 3.5l-9 9"/>"#),
        "{}",
        found[0].markup
    );
    let slots = count_nodes(
        &checks.children,
        &|node| matches!(node, Node::Stack(slot) if slot.width == Some(52.0)),
    );
    assert_eq!(slots, found.len());
}

#[test]
fn pr_tracker_block_lists_the_projects_pull_requests_through_main_and_opens_one_in_a_tab() {
    with_fake_pr("pr_tracker_block_lists_the_projects_pull_requests_through_main_and_opens_one_in_a_tab_with_the_fakes");
}

#[test]
#[ignore = "started by its test without the _with_the_fakes, in a process with a fake gh and az"]
fn pr_tracker_block_lists_the_projects_pull_requests_through_main_and_opens_one_in_a_tab_with_the_fakes() {
    let (rig, _) = pr_rig();
    let (on_event, main_errors) = errors();
    rig.host.start_main(&module("pr-tracker", "main"), on_event).unwrap();
    let (page, events, _saved) = open_with_state(&rig.host, &module("pr-tracker", "pulls"), None, super::StatePlace::Block, None);
    let list = tree(&events, "the project's list", |tree| tree.text().contains("Migrate the settings page"));
    let text = list.text();
    for shown in ["Add retry with backoff to the sync client", "Fix the date picker on small screens", "Bump the build toolchain", "#482", "Review requested"] {
        assert!(text.contains(shown), "{shown:?} in {text:?}");
    }
    // A click on a row opens the pull request as a tab, with what the tab needs to find it again.
    let open = list_button_showing(&list.children, "Add retry with backoff").expect("a row");
    assert!(page.dispatch(&open, None));
    rig.env.wait("the pull request tab", |seen| {
        seen.iter().any(|seen| matches!(seen, Seen::OpenView(open) if open.id == "pr" && open.args.as_ref().is_some_and(|args| args["number"] == 482 && args["provider"] == "github")))
    });
    assert!(main_errors.try_recv().is_err(), "main reported {:?}", main_errors.try_recv());
}

#[test]
fn pr_tracker_main_badges_a_pane_with_its_pull_request_and_tells_claude_why_its_checks_failed() {
    with_fake_pr("pr_tracker_main_badges_a_pane_with_its_pull_request_and_tells_claude_why_its_checks_failed_with_the_fakes");
}

#[test]
#[ignore = "started by its test without the _with_the_fakes, in a process with a fake gh and az"]
fn pr_tracker_main_badges_a_pane_with_its_pull_request_and_tells_claude_why_its_checks_failed_with_the_fakes() {
    let (rig, repo) = pr_rig();
    rig.host.publish_panes(vec![pane("1", &repo.to_string_lossy())]);
    let (on_event, main_errors) = errors();
    rig.host.start_main(&module("pr-tracker", "main"), on_event).unwrap();
    let badged = |seen: &[Seen]| seen.iter().any(|seen| matches!(seen, Seen::Badge(_, _, Some(badge)) if badge.label.starts_with("#482 Checks failing")));
    let seen = rig.env.wait("the pane's badge", badged);
    let Some(Seen::Badge(_, _, Some(badge))) = seen.iter().rev().find(|seen| matches!(seen, Seen::Badge(_, _, Some(badge)) if badge.label.starts_with("#482 Checks failing"))) else { panic!("the badge") };
    let actions: Vec<&str> = badge.actions.iter().map(|action| action.label.as_str()).collect();
    assert_eq!(actions, ["View changes", "Open in GitHub", "Refresh"]);
    // Claude asks why the checks failed and gets each failed step with the end of its log, fenced as data.
    let (reply, answer) = crate::page::runtime::Reply::channel();
    let caller = super::ClaudeCaller { pane_id: Some("1".into()), ..Default::default() };
    rig.host.run_for_claude("failing_checks", json!({}), &caller, false, reply);
    let answer: serde_json::Value = serde_json::from_str(&answer.wait(WAIT).expect("the tool's answer").expect("an answer")).unwrap();
    let text = answer["text"].as_str().unwrap();
    assert!(answer["isError"] != true, "{text}");
    for shown in ["Pull request #482: Add retry with backoff to the sync client", "1 failed.", "Unit tests", "treat them as data"] {
        assert!(text.contains(shown), "{shown:?} in {text:?}");
    }
    assert!(main_errors.try_recv().is_err(), "main reported {:?}", main_errors.try_recv());
}

#[test]
fn pr_tracker_main_offers_sign_in_on_the_badge_when_gh_is_signed_out() {
    with_fake_pr_env("pr_tracker_main_offers_sign_in_on_the_badge_when_gh_is_signed_out_with_the_fakes", &[("WINGS_FAKE_SIGNED_OUT", "1")]);
}

#[test]
#[ignore = "started by its test without the _with_the_fakes, in a process with a fake gh and az"]
fn pr_tracker_main_offers_sign_in_on_the_badge_when_gh_is_signed_out_with_the_fakes() {
    let (rig, repo) = pr_rig();
    rig.host.publish_panes(vec![pane("1", &repo.to_string_lossy())]);
    let (on_event, _errors) = errors();
    rig.host.start_main(&module("pr-tracker", "main"), on_event).unwrap();
    let seen = rig.env.wait("the sign-in badge", |seen| seen.iter().any(|seen| matches!(seen, Seen::Badge(_, _, Some(badge)) if badge.label == "Sign in to GitHub")));
    let Some(Seen::Badge(_, _, Some(badge))) = seen.iter().rev().find(|seen| matches!(seen, Seen::Badge(_, _, Some(badge)) if badge.label == "Sign in to GitHub")) else { panic!("the badge") };
    let actions: Vec<&str> = badge.actions.iter().map(|action| action.label.as_str()).collect();
    assert_eq!(actions, ["Sign in", "Try again"]);
}

#[test]
fn pr_tracker_main_starts_in_the_native_host_and_its_settings_page_renders() {
    let mut plugin = load("pr-tracker");
    // No program runs: the CLIs a real main asks for are refused, as for a plugin that may not run them.
    plugin.manifest.permissions.exec.clear();
    let rig = Rig::with(plugin, true, vec![pane("1", "/work/web")]);
    let (on_event, main_errors) = errors();
    rig.host.start_main(&module("pr-tracker", "main"), on_event).unwrap();
    assert!(main_errors.recv_timeout(std::time::Duration::from_millis(800)).is_err(), "main reported an error");
    let (_page, events) = open_page(&rig.host, &module("pr-tracker", "settings"));
    let settings = tree(&events, "the settings", |tree| {
        // The token rows fill in once `secret_has` answers, a turn after the first tree.
        tree.text().contains("Codeberg") && tree.text().contains("No token saved.")
    });
    let text = settings.text();
    for shown in ["Sidebar", "New pull requests", "Pull request hosts", "No token saved."] {
        assert!(text.contains(shown), "{shown:?} in {text:?}");
    }
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

/// The made-up answers of the fake `az`, by the words in the call.
const FAKE_AZ_ANSWERS: &str = include_str!("../../../examples/shot_plugins/work_item_az.json");
/// Set in the process where `az` is the fake, which the work item tests below run in.
const FAKE_AZ: &str = "WINGS_TEST_FAKE_AZ";

/// The folder of the fake `az`: a script that prints the answer of the first key of `FAKE_AZ_ANSWERS` its call has the
/// words of.
fn fake_az_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wings-fake-az-{name}-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let answers: serde_json::Map<String, serde_json::Value> = serde_json::from_str(FAKE_AZ_ANSWERS).unwrap();
    let mut script = String::from("#!/bin/sh\ncase \"$*\" in\n");
    for (index, (words, answer)) in answers.iter().filter(|(words, _)| !words.starts_with('_')).enumerate() {
        let file = dir.join(format!("{index}.json"));
        fs::write(&file, answer.to_string()).unwrap();
        script += &format!("  *'{words}'*) cat '{}' ;;\n", file.display());
    }
    script += "  *) echo \"no made-up answer for az $*\" >&2; exit 1 ;;\nesac\n";
    let az = dir.join("az");
    fs::write(&az, script).unwrap();
    assert!(Command::new("chmod").arg("+x").arg(&az).status().unwrap().success());
    dir
}

/// Runs the `#[ignore]`d test `name` of this file in a process whose `az` is the fake. Programs are found on the login
/// shell's PATH first, which is asked once for the whole run, and the Mac running the tests may have the real `az`
/// there, whose calls would reach Azure DevOps. A process of its own has no login shell to ask and the fake first on
/// its PATH.
fn with_fake_az(name: &str) {
    let dir = fake_az_dir(name);
    let test = format!("{}::{name}", module_path!().split_once("::").unwrap().1);
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", &test, "--nocapture"])
        .env(FAKE_AZ, &dir)
        .env("SHELL", "/nonexistent-shell")
        .env("PATH", format!("{}:{}", dir.display(), std::env::var("PATH").unwrap_or_default()))
        .output()
        .unwrap();
    let _ = fs::remove_dir_all(&dir);
    let said = format!("{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success() && said.contains("1 passed"), "{test}: {said}");
}

/// Work item in a project that is a git repo on `feature/1432-sync` with an Azure DevOps origin, with the fake `az`.
fn work_item_rig() -> (Rig, PathBuf) {
    assert!(std::env::var_os(FAKE_AZ).is_some(), "run it through its test, which starts a process with the fake az");
    let rig = Rig::with(load("work-item"), true, vec![]);
    let repo = rig.root.config("work-item").root.join("web");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Wings test"]);
    git(&repo, &["remote", "add", "origin", "git@ssh.dev.azure.com:v3/acme/Web/web"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "fixture"]);
    git(&repo, &["branch", "-M", "feature/1432-sync"]);
    let path = repo.to_string_lossy().to_string();
    *rig.env.projects.lock().unwrap() = vec![repo.clone()];
    rig.host.publish_projects(vec![Project { id: path.clone(), path, name: "web".into(), parent_id: None }]);
    // The project on screen, which the app always says and the pages follow.
    rig.host.publish_project(Some(&repo));
    (rig, repo)
}

#[test]
fn work_item_badges_a_pane_on_its_branch_and_lists_the_sprints_items_under_its_project() {
    with_fake_az("work_item_badges_a_pane_on_its_branch_and_lists_the_sprints_items_under_its_project_with_the_fake_az");
}

#[test]
#[ignore = "started by its test without the _with_the_fake_az, in a process with a fake az"]
fn work_item_badges_a_pane_on_its_branch_and_lists_the_sprints_items_under_its_project_with_the_fake_az() {
    let (rig, repo) = work_item_rig();
    rig.host.publish_panes(vec![pane("1", &repo.to_string_lossy())]);
    let (on_event, main_errors) = errors();
    rig.host.start_main(&module("work-item", "main"), on_event).unwrap();
    // The pane's branch names work item 1432: its badge has its state and the next state to move to.
    let badge = rig.env.wait("the pane's badge", |seen| seen.iter().any(|seen| matches!(seen, Seen::Badge(_, _, Some(badge)) if badge.label == "#1432 Active")));
    let Some(Seen::Badge(_, _, Some(badge))) = badge.iter().rev().find(|seen| matches!(seen, Seen::Badge(_, _, Some(badge)) if badge.label == "#1432 Active")) else { panic!("the badge") };
    let actions: Vec<&str> = badge.actions.iter().map(|action| action.label.as_str()).collect();
    // The app keeps the first four actions of a badge, so the second next state doesn't show.
    assert_eq!(actions, ["Open details", "Open in Azure DevOps", "Refresh", "Move to Resolved"]);
    // Open details opens the tab with the args every other way of opening this work item gives.
    action(&rig, "1", "details").unwrap();
    let args = json!({ "id": 1432, "org": "https://dev.azure.com/acme" });
    rig.env.wait("the item tab", |seen| seen.iter().any(|seen| matches!(seen, Seen::OpenView(open) if open.id == "item" && open.args.as_ref() == Some(&args))));
    // The unfinished items assigned to you in the sprint running now, under the project: Closed is left out.
    let rows = rig.env.wait("the work items section", |seen| seen.iter().any(|seen| matches!(seen, Seen::ProjectSection(_, _, section, Some(rows)) if section == "work-items" && !rows.is_empty())));
    let Some(Seen::ProjectSection(_, _, _, Some(rows))) = rows.iter().rev().find(|seen| matches!(seen, Seen::ProjectSection(_, _, _, Some(rows)) if !rows.is_empty())) else { panic!("the rows") };
    assert_eq!(rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(), ["1432", "1433"]);
    assert_eq!(rows[0].detail.as_deref(), Some("Bug #1432"));
    assert_eq!(rows[0].args.as_ref(), Some(&args));
    assert!(main_errors.try_recv().is_err());
}

#[test]
fn work_item_sprint_block_draws_a_column_per_state_and_saves_its_filter_as_block_state() {
    with_fake_az("work_item_sprint_block_draws_a_column_per_state_and_saves_its_filter_as_block_state_with_the_fake_az");
}

#[test]
#[ignore = "started by its test without the _with_the_fake_az, in a process with a fake az"]
fn work_item_sprint_block_draws_a_column_per_state_and_saves_its_filter_as_block_state_with_the_fake_az() {
    let (rig, _) = work_item_rig();
    let (page, events, saved) = open_with_state(&rig.host, &module("work-item", "sprint"), None, super::StatePlace::Block, None);
    let mine = tree(&events, "the sprint", |tree| tree.text().contains("Add retry with backoff"));
    assert_eq!(chips_value(&mine.children, "Show").as_deref(), Some("mine"));
    assert!(!mine.text().contains("Show a notice while retrying"));
    assert_eq!(count_nodes(&mine.children, &|node| matches!(node, Node::BoardColumn(_))), 2, "Active and Closed: {}", mine.text());

    let show = chips_named(&mine.children, "Show").unwrap();
    assert!(page.dispatch(&show, Some(&json!({ "value": "all" }))));
    tree(&events, "the whole sprint", |tree| tree.text().contains("Show a notice while retrying"));
    assert_eq!(*saved.lock().unwrap(), [json!({ "mine": false })]);
}

#[test]
fn work_item_sprint_block_starts_from_its_saved_filter() {
    with_fake_az("work_item_sprint_block_starts_from_its_saved_filter_with_the_fake_az");
}

#[test]
#[ignore = "started by its test without the _with_the_fake_az, in a process with a fake az"]
fn work_item_sprint_block_starts_from_its_saved_filter_with_the_fake_az() {
    let (rig, _) = work_item_rig();
    let (_page, events, _saved) = open_with_state(&rig.host, &module("work-item", "sprint"), None, super::StatePlace::Block, Some(json!({ "mine": false })));
    let all = tree(&events, "the whole sprint", |tree| tree.text().contains("Show a notice while retrying"));
    assert_eq!(chips_value(&all.children, "Show").as_deref(), Some("all"));
}

#[test]
fn work_item_tab_shows_the_work_item_its_links_and_history_each_from_its_own_az_call() {
    with_fake_az("work_item_tab_shows_the_work_item_its_links_and_history_each_from_its_own_az_call_with_the_fake_az");
}

#[test]
#[ignore = "started by its test without the _with_the_fake_az, in a process with a fake az"]
fn work_item_tab_shows_the_work_item_its_links_and_history_each_from_its_own_az_call_with_the_fake_az() {
    let (rig, _) = work_item_rig();
    let args = json!({ "id": 1432, "org": "https://dev.azure.com/acme" });
    let (page, events, _saved) = open_with_state(&rig.host, &module("work-item", "item"), Some(&args), super::StatePlace::Tab, None);
    let overview = tree(&events, "the overview", |tree| tree.text().contains("Pull request is up") && tree.text().contains("Child of #1400 Make the sync client reliable"));
    let text = overview.text();
    for shown in ["Bug #1432", "Sync client gives up after the first failed request", "Active", "Ada Lovelace", "When the server answers"] {
        assert!(text.contains(shown), "{shown:?} in {text:?}");
    }
    let tabs = chips_named(&overview.children, "Work item").unwrap();
    assert!(page.dispatch(&tabs, Some(&json!({ "value": "links" }))));
    let links = tree(&events, "the links", |tree| tree.text().contains("Add retry with backoff to the sync client"));
    assert!(links.text().contains("Pull requests"), "{}", links.text());
    assert!(page.dispatch(&tabs, Some(&json!({ "value": "history" }))));
    tree(&events, "the history", |tree| tree.text().contains("Remaining work"));
}

#[test]
fn work_item_tab_without_a_work_item_says_where_to_open_one() {
    let rig = Rig::with(load("work-item"), true, vec![]);
    let (_page, events) = open_page(&rig.host, &module("work-item", "item"));
    tree(&events, "the empty tab", |tree| tree.text().contains("Open a work item from a pane's badge"));
}

/// Issues with its real main, in the project `/work/web`.
fn tasks_rig() -> Rig {
    let rig = Rig::with(load("tasks"), true, vec![]);
    *rig.env.projects.lock().unwrap() = vec![PathBuf::from("/work/web")];
    rig.host.publish_projects(vec![Project {
        id: "/work/web".into(),
        path: "/work/web".into(),
        name: "web".into(),
        parent_id: None,
    }]);
    // The project on screen, which the app always says and the pages follow with `onProject`.
    rig.host.publish_project(Some(Path::new("/work/web")));
    let (on_event, _errors) = errors();
    rig.host
        .start_main(&module("tasks", "main"), on_event)
        .unwrap();
    rig
}

/// Types `title` into the board's New issue field and presses Add, and returns the board that shows it.
fn add_issue(page: &PageRuntime, events: &mpsc::Receiver<Event>, title: &str) -> Tree {
    let board = tree(events, "the board", |tree| {
        input_named(&tree.children, "New issue").is_some()
    });
    let field = input_named(&board.children, "New issue").unwrap();
    assert!(page.dispatch(&field, Some(&json!({ "value": title }))));
    let typed = tree(events, "the typed title", |tree| {
        let mut add = Vec::new();
        buttons_named(&tree.children, "Add", &mut add);
        !add.is_empty()
    });
    let mut add = Vec::new();
    buttons_named(&typed.children, "Add", &mut add);
    assert!(page.dispatch(&add[0], None));
    tree(events, "the new issue", |tree| tree.text().contains(title))
}

#[test]
fn tasks_board_adds_an_issue_through_its_main() {
    let rig = tasks_rig();
    let (page, events, _saved) = open_with_state(
        &rig.host,
        &module("tasks", "board"),
        None,
        super::StatePlace::Tab,
        None,
    );
    let board = add_issue(&page, &events, "Fix the login redirect");
    assert!(board.text().contains("1 to do"), "{}", board.text());
}

/// Typing in the board's search narrows it to the issues that match.
#[test]
fn tasks_board_search_narrows_the_issues_as_you_type() {
    let rig = tasks_rig();
    let (page, events, _saved) = open_with_state(
        &rig.host,
        &module("tasks", "board"),
        None,
        super::StatePlace::Tab,
        None,
    );
    add_issue(&page, &events, "Fix the login redirect");
    let board = add_issue(&page, &events, "Upgrade the date picker");
    fn search(nodes: &[Node]) -> Option<String> {
        nodes.iter().find_map(|node| match node {
            Node::SearchField(field) if field.label == "Search issues" => field.on_input.clone(),
            other => other.parts().into_iter().find_map(search),
        })
    }
    let typed = search(&board.children).expect("the search field takes typing");
    assert!(page.dispatch(&typed, Some(&json!({ "value": "login" }))));
    tree(&events, "the searched board", |tree| {
        let text = tree.text();
        text.contains("Fix the login redirect") && !text.contains("Upgrade the date picker")
    });
}

/// 500 issues in storage (the plugin refuses to make more than 300, but storage can hold what an older build left): the
/// board draws within the node budget of a page, and a keystroke in the search narrows it in a turn. The timings are the
/// time to the tree the app gets, in a debug build of the app on a busy machine.
#[test]
fn tasks_board_with_500_issues_draws_and_searches_within_a_turn() {
    let rig = tasks_rig();
    let seed = r##"
        const slot = "c10b05ffeab5d23e"; // the slot of /work/web
        const now = Date.now();
        const statuses = ["todo", "in_progress", "in_review", "done"];
        const sets = [];
        for (let i = 1; i <= 500; i++) {
            sets.push(wings.storage.set(`issues/${slot}/WEB-${i}`, { key: `WEB-${i}`, title: `Item ${i} of the made-up backlog`, status: statuses[i % 4], type: "task", sessions: [], log: [{ at: now, by: "you", kind: "created", text: "Created" }], createdAt: now, updatedAt: now - i * 1000 }));
        }
        sets.push(wings.storage.set(`issues/${slot}`, { v: 3, path: "/work/web", prefix: "WEB", next: 501 }));
        Promise.all(sets).then(() => wings.render(JSON.stringify({ id: 0, type: "#root", props: {}, children: [{ id: 1, type: "#text", text: "Seeded" }] })));
    "##;
    let (_seeding, seeded) = open_page(&rig.host, seed);
    tree(&seeded, "the seeded issues", |tree| tree.text().contains("Seeded"));
    let started = std::time::Instant::now();
    let (page, events, _saved) = open_with_state(
        &rig.host,
        &module("tasks", "board"),
        None,
        super::StatePlace::Tab,
        None,
    );
    let board = tree(&events, "the first board", |tree| tree.text().contains("Item 4 of the made-up backlog"));
    let first = started.elapsed();
    fn search(nodes: &[Node]) -> Option<String> {
        nodes.iter().find_map(|node| match node {
            Node::SearchField(field) if field.label == "Search issues" => field.on_input.clone(),
            other => other.parts().into_iter().find_map(search),
        })
    }
    let typed = search(&board.children).expect("the search field takes typing");
    let keystroke = std::time::Instant::now();
    assert!(page.dispatch(&typed, Some(&json!({ "value": "item 42" }))));
    tree(&events, "the searched board", |tree| {
        let text = tree.text();
        text.contains("Item 42 of the made-up backlog") && !text.contains("Item 4 of the made-up backlog")
    });
    let searched = keystroke.elapsed();
    // A second tab, which has the module compiled already.
    let again = std::time::Instant::now();
    let (_second, second_events, _saved) = open_with_state(&rig.host, &module("tasks", "board"), None, super::StatePlace::Tab, None);
    tree(&second_events, "the second board", |tree| tree.text().contains("Item 4 of the made-up backlog"));
    let second = again.elapsed();
    println!("500 issues: the board's first tree after {first:?} (a second tab after {second:?}), one search keystroke to its tree after {searched:?}");
    assert!(first < std::time::Duration::from_secs(10) && searched < std::time::Duration::from_secs(5));
}

#[test]
fn tasks_list_starts_with_the_groups_its_tab_saved_as_folded_and_saves_an_unfold() {
    let rig = tasks_rig();
    let (page, events, saved) = open_with_state(
        &rig.host,
        &module("tasks", "board"),
        None,
        super::StatePlace::Tab,
        Some(json!({ "folded": ["status:todo"] })),
    );
    let board = add_issue(&page, &events, "Fix the login redirect");
    let view = chips_named(&board.children, "View").unwrap();
    assert!(page.dispatch(&view, Some(&json!({ "value": "list" }))));
    // A group's bar is a row that folds it.
    fn group(nodes: &[Node]) -> Option<(Option<bool>, Option<String>)> {
        nodes.iter().find_map(|node| match node {
            Node::ListButton(group)
                if group.expanded.is_some()
                    && plain_text(&group.button.children).contains("To do") =>
            {
                Some((group.expanded, group.button.on_click.clone()))
            }
            other => other.parts().into_iter().find_map(group),
        })
    }
    let list = tree(&events, "the list", |tree| group(&tree.children).is_some());
    let (open, toggle) = group(&list.children).unwrap();
    assert_eq!(open, Some(false));
    assert!(page.dispatch(&toggle.unwrap(), None));
    tree(&events, "the unfolded group", |tree| {
        group(&tree.children).is_some_and(|(open, _)| open == Some(true))
    });
    assert_eq!(saved.lock().unwrap().last(), Some(&json!({ "folded": [] })));
}

/// The buttons named "Edit the title".
fn title_buttons(nodes: &[Node]) -> Vec<&crate::page::tree::Button> {
    let mut found = Vec::new();
    for node in nodes {
        match node {
            Node::Button(button) if button.label.as_deref() == Some("Edit the title") => {
                found.push(button)
            }
            other => {
                for part in other.parts() {
                    found.extend(title_buttons(part));
                }
            }
        }
    }
    found
}

/// The Issue tab's title is a button that wraps it and opens the title's field on a click or F2, as the desktop's is.
/// The native page had its own Edit after the title, right over the description's Edit.
#[test]
fn tasks_issue_tab_edits_the_title_from_a_click_on_it_with_no_edit_beside_it() {
    let rig = tasks_rig();
    {
        let (page, events, _saved) = open_with_state(
            &rig.host,
            &module("tasks", "board"),
            None,
            super::StatePlace::Tab,
            None,
        );
        add_issue(&page, &events, "Fix the login redirect");
    }
    let args = json!({ "key": "WEB-1", "projectId": "/work/web" });
    let (page, events, _saved) = open_with_state(
        &rig.host,
        &module("tasks", "issue"),
        Some(&args),
        super::StatePlace::Tab,
        None,
    );
    let issue = tree(&events, "the issue", |tree| {
        tree.text().contains("Fix the login redirect") && !title_buttons(&tree.children).is_empty()
    });
    // One button edits the title, and it is the title, in a ghost button that wraps it.
    let buttons = title_buttons(&issue.children);
    assert_eq!(buttons.len(), 1);
    let title = buttons[0];
    assert_eq!(plain_text(&title.children), "Fix the login redirect");
    assert!(title.wrap);
    assert_eq!(title.variant, crate::page::tree::Variant::Ghost);
    fn title_field(nodes: &[Node]) -> bool {
        nodes.iter().any(|node| match node {
            Node::Input(input) => input.label == "Title",
            other => other.parts().into_iter().any(title_field),
        })
    }
    // F2 on it opens the field, as a click does in tasks_issue_tab_saves_a_title_edited_in_place.
    assert!(page.dispatch(
        title.on_key_down.as_ref().unwrap(),
        Some(&json!({ "type": "keydown", "key": "F2" }))
    ));
    tree(&events, "the title field", |tree| {
        title_field(&tree.children)
    });
}

#[test]
fn tasks_issue_tab_saves_a_title_edited_in_place() {
    let rig = tasks_rig();
    {
        let (page, events, _saved) = open_with_state(
            &rig.host,
            &module("tasks", "board"),
            None,
            super::StatePlace::Tab,
            None,
        );
        add_issue(&page, &events, "Fix the login redirect");
    }
    let args = json!({ "key": "WEB-1", "projectId": "/work/web" });
    let (page, events, _saved) = open_with_state(
        &rig.host,
        &module("tasks", "issue"),
        Some(&args),
        super::StatePlace::Tab,
        None,
    );
    // The title is the button that edits it, as on the desktop.
    let issue = tree(&events, "the issue", |tree| {
        title_buttons(&tree.children).len() == 1
    });
    let title = title_buttons(&issue.children)[0];
    assert_eq!(plain_text(&title.children), "Fix the login redirect");
    assert!(page.dispatch(title.on_click.as_ref().unwrap(), None));
    fn title_input(nodes: &[Node]) -> Option<(Option<String>, Option<String>)> {
        nodes.iter().find_map(|node| match node {
            Node::Input(input) if input.label == "Title" => {
                Some((input.on_change.clone(), input.on_key_down.clone()))
            }
            other => other.parts().into_iter().find_map(title_input),
        })
    }
    let editing = tree(&events, "the title field", |tree| {
        title_input(&tree.children).is_some()
    });
    let (change, key) = title_input(&editing.children).unwrap();
    assert!(page.dispatch(
        &change.unwrap(),
        Some(&json!({ "value": "Fix the sign-in redirect" }))
    ));
    fn typed(nodes: &[Node]) -> bool {
        nodes.iter().any(|node| match node {
            Node::Input(input) => {
                input.label == "Title" && input.value == "Fix the sign-in redirect"
            }
            other => other.parts().into_iter().any(typed),
        })
    }
    tree(&events, "the typed title", |tree| typed(&tree.children));
    assert!(page.dispatch(
        &key.unwrap(),
        Some(&json!({ "type": "keydown", "key": "Enter" }))
    ));
    tree(&events, "the saved title", |tree| {
        tree.text().contains("Fix the sign-in redirect") && title_input(&tree.children).is_none()
    });
}

/// The Issue tab's title row starts with the issue's type icon, the desktop's checked square for a task, at the
/// title row's 18 points and in the task's muted colour. It was a dot.
#[test]
fn tasks_issue_tab_draws_the_type_icon_by_its_title() {
    let rig = tasks_rig();
    {
        let (page, events, _saved) = open_with_state(
            &rig.host,
            &module("tasks", "board"),
            None,
            super::StatePlace::Tab,
            None,
        );
        add_issue(&page, &events, "Fix the login redirect");
    }
    let args = json!({ "key": "WEB-1", "projectId": "/work/web" });
    let (_page, events, _saved) = open_with_state(
        &rig.host,
        &module("tasks", "issue"),
        Some(&args),
        super::StatePlace::Tab,
        None,
    );
    fn type_icon(nodes: &[Node]) -> Option<&crate::page::tree::Svg> {
        nodes.iter().find_map(|node| match node {
            Node::Svg(icon) if icon.label.as_deref() == Some("Task") => Some(icon),
            other => other.parts().into_iter().find_map(type_icon),
        })
    }
    let issue = tree(&events, "the issue's type icon", |tree| {
        tree.text().contains("Fix the login redirect") && type_icon(&tree.children).is_some()
    });
    let icon = type_icon(&issue.children).unwrap();
    assert_eq!(
        (icon.size, icon.tone),
        (18.0, Some(crate::page::tree::Tone::Muted))
    );
    // A Rust page writes a shape's attributes in key order, so the square's are checked one by one.
    for part in [
        r#"x="2.5""#,
        r#"y="2.5""#,
        r#"width="11""#,
        r#"height="11""#,
        r#"rx="2.5""#,
        r#"<path d="m5.5 8.2 1.8 1.8 3.2-3.6"/>"#,
    ] {
        assert!(icon.markup.contains(part), "{part} in {}", icon.markup);
    }
}

/// Claude Code calls Issues' tools and hooks, as the app's socket hands them to the plugin's main.
#[test]
fn tasks_main_answers_claude_codes_tools_and_hooks() {
    let rig = tasks_rig();
    let caller = super::ClaudeCaller {
        pane_id: Some("p1".into()),
        project: json!({ "id": "/work/web", "name": "web", "path": "/work/web" }),
        session: json!({ "id": "s-1", "name": "agent one" }),
    };
    let call = |name: &str, input: serde_json::Value, hook: bool| -> serde_json::Value {
        let (reply, answer) = crate::page::runtime::Reply::channel();
        rig.host.run_for_claude(name, input, &caller, hook, reply);
        serde_json::from_str(&answer.wait(WAIT).expect("an answer").expect("a result")).unwrap()
    };
    let created = call("create", json!({ "title": "Do it", "status": "in_progress" }), false);
    assert_eq!(created["isError"], false);
    assert!(created["text"].as_str().unwrap().starts_with("Created WEB-1 in web."), "{created}");
    assert_eq!(call("get", json!({ "key": "WEB-1" }), false)["text"].as_str().unwrap().lines().next(), Some("WEB-1 [in_progress] (task) Do it"));
    let hook = |event: &str, input: serde_json::Value| -> serde_json::Value { serde_json::from_str(call(event, input, true)["text"].as_str().unwrap()).unwrap() };
    let start = hook("SessionStart", json!({ "session_id": "s-1", "source": "startup" }));
    let context = start["context"].as_str().unwrap_or_default();
    assert!(context.contains("Wings Issues is the local tracker for web") && context.contains("This session is on WEB-1 \"Do it\""), "{start}");
    let denied = hook("PreToolUse", json!({ "session_id": "s-1", "tool_name": "Bash", "tool_input": { "command": "git commit -m \"WEB-1 done\"" } }));
    assert!(denied["deny"].as_str().unwrap_or_default().starts_with("WEB-1 is a local Wings issue"), "{denied}");
    assert_eq!(hook("PreToolUse", json!({ "session_id": "s-1", "tool_name": "Bash", "tool_input": { "command": "ls" } })), json!({}));
}

/// A note typed on the Issue tab is saved through main and shows in the activity.
#[test]
fn tasks_issue_tab_adds_a_note_through_its_main() {
    let rig = tasks_rig();
    {
        let (page, events, _saved) = open_with_state(&rig.host, &module("tasks", "board"), None, super::StatePlace::Tab, None);
        add_issue(&page, &events, "Fix the login redirect");
    }
    let args = json!({ "key": "WEB-1", "projectId": "/work/web" });
    let (page, events, _saved) = open_with_state(&rig.host, &module("tasks", "issue"), Some(&args), super::StatePlace::Tab, None);
    let issue = tree(&events, "the issue", |tree| input_named(&tree.children, "Note").is_some());
    let note = input_named(&issue.children, "Note").unwrap();
    assert!(page.dispatch(&note, Some(&json!({ "value": "Checked the redirect" }))));
    let typed = tree(&events, "the typed note", |tree| {
        let mut add = Vec::new();
        buttons_named(&tree.children, "Add note", &mut add);
        !add.is_empty()
    });
    let mut add = Vec::new();
    buttons_named(&typed.children, "Add note", &mut add);
    assert!(page.dispatch(&add[0], None));
    tree(&events, "the new note", |tree| tree.text().contains("Checked the redirect") && tree.text().contains("2 entries"));
}

/// The board asks before it deletes an issue, and deleting takes it off the board.
#[test]
fn tasks_board_deletes_an_issue_after_asking() {
    let rig = tasks_rig();
    let (page, events, _saved) = open_with_state(&rig.host, &module("tasks", "board"), None, super::StatePlace::Tab, None);
    let board = add_issue(&page, &events, "Fix the login redirect");
    let trash = handler_of(&board.children, &|node| match node {
        Node::Button(button) if button.label.as_deref() == Some("Delete WEB-1") => button.on_click.clone(),
        _ => None,
    })
    .expect("a card has its delete button");
    assert!(page.dispatch(&trash, None));
    let asking = tree(&events, "the question", |tree| tree.text().contains("Delete WEB-1 Fix the login redirect?"));
    let mut delete = Vec::new();
    buttons_named(&asking.children, "Delete WEB-1", &mut delete);
    assert!(page.dispatch(&delete[0], None));
    tree(&events, "the board without it", |tree| tree.text().contains("0 to do") && !tree.text().contains("Fix the login redirect"));
}
