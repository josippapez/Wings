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
    runtime::{Event, PageRuntime},
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

/// A file of the plugin's native build. It is built by `wings-plugin build` in the plugin's folder.
fn native(name: &str, file: &str) -> String {
    let path = repo_plugin(name).join("dist/native").join(file);
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "{}: {error}. Build the plugin: cd plugins/{name} && pnpm build",
            path.display()
        )
    })
}

/// A frame of a plugin written in Rust, as the app runs it: its committed module, which `sh build-rust.sh` in `plugins/`
/// builds.
fn module(name: &str, frame: &str) -> String {
    let path = repo_plugin(name).join("dist/native").join(format!("{frame}.wasm"));
    assert!(path.is_file(), "{}: build the plugin: cd plugins && sh build-rust.sh {name}", path.display());
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

#[test]
fn schedules_main_serves_its_native_page_and_a_schedule_added_in_the_form_is_listed() {
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
        .start_main(&native("schedules", "main.js"), on_event)
        .unwrap();

    let (page, events) = open_page(&rig.host, &native("schedules", "schedules.js"));
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
    assert!(
        main_errors.try_recv().is_err(),
        "main reported {:?}",
        main_errors.try_recv()
    );
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
        .start_main(&native("disk", "main.js"), on_event)
        .unwrap();

    let (page, events) = open_page(&rig.host, &native("disk", "map.js"));
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

// PR tracker: its real native bundles, over a fixture main that answers what the pages ask with made-up data.

/// The project's folder the fixture's pull requests belong to.
const PR_PROJECT: &str = "/work/web";

fn pr_rig() -> Rig {
    let rig = Rig::with(load("pr-tracker"), true, vec![]);
    *rig.env.projects.lock().unwrap() = vec![PathBuf::from(PR_PROJECT)];
    rig.host.publish_projects(vec![Project {
        id: PR_PROJECT.into(),
        path: PR_PROJECT.into(),
        name: "web".into(),
        parent_id: None,
    }]);
    // As the window does: the list's `useProject` waits to hear the project on screen.
    rig.host
        .publish_project(Some(std::path::Path::new(PR_PROJECT)));
    let fixture = fs::read_to_string(repo_plugin("pr-tracker").join("preview/native-host.js"))
        .unwrap()
        .replace("__ROOT__", PR_PROJECT);
    let (on_event, _errors) = errors();
    rig.host.start_main(&fixture, on_event).unwrap();
    rig
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
fn pr_tracker_tab_lists_the_projects_pull_requests_and_opens_one_in_the_same_tab() {
    let rig = pr_rig();
    let (page, events) = open_page(&rig.host, &native("pr-tracker", "pr.js"));
    let list = tree(&events, "the project's list", |tree| {
        tree.text().contains("Migrate the settings page")
    });
    let text = list.text();
    for shown in ["web · Pull requests", "Agent working", "2 new"] {
        assert!(text.contains(shown), "{shown:?} in {text:?}");
    }
    assert!(count_nodes(&list.children, &|node| matches!(node, Node::Avatar(_))) >= 2);
    let open = list_button_showing(&list.children, "Add retry with backoff").expect("a row");
    assert!(page.dispatch(&open, None));

    let pull = tree(&events, "the pull request", |tree| {
        tree.text().contains("Conversation") && tree.text().contains("Alan Turing")
    });
    let text = pull.text();
    for shown in [
        "All pull requests",
        "acme/web #482",
        "1 approved, changes requested",
        "1 failed · 1 running · 1 passed ›",
        "exponential backoff",
        "const delay = Math.min(base * 2 ** attempt, 30_000);",
        "Setting",
        "Equal jitter keeps a floor on the delay.",
        "ci-bot[bot]",
    ] {
        assert!(text.contains(shown), "{shown:?} in {text:?}");
    }
    // The bold word and the link are spans of their paragraph, and the link opens in the browser.
    let spans = count_nodes(
        &pull.children,
        &|node| matches!(node, Node::Text(span) if span.on_click.is_some() || span.weight != crate::page::tree::Weight::Normal),
    );
    assert!(spans >= 2, "{spans} spans");
    // Going back shows the list again.
    let back = {
        let mut found = Vec::new();
        buttons_named(&pull.children, "← All pull requests", &mut found);
        found.remove(0)
    };
    assert!(page.dispatch(&back, None));
    tree(&events, "the list again", |tree| {
        tree.text().contains("Migrate the settings page")
    });
}

#[test]
fn pr_tracker_tab_opened_with_args_shows_that_pull_request_and_its_checks() {
    let rig = pr_rig();
    let args = json!({
        "provider": "github", "url": "https://github.com/acme/web/pull/482", "number": 482,
        "repo": "acme/web", "title": "Add retry with backoff to the sync client", "cwd": PR_PROJECT,
    });
    let (sent, events) = mpsc::channel();
    let page = rig
        .host
        .open_hidden_page_with_args(&native("pr-tracker", "pr.js"), Some(&args), move |event| {
            let _ = sent.send(event);
        })
        .unwrap();
    page.set_visible(true);
    let pull = tree(&events, "the pull request", |tree| {
        tree.text().contains("Conversation")
    });
    assert!(
        !pull.text().contains("All pull requests"),
        "a tab opened on a pull request has no list to go back to"
    );
    let tabs = chips_named(&pull.children, "Pull request").expect("the tabs");
    assert!(page.dispatch(&tabs, Some(&json!({ "value": "checks" }))));
    let checks = tree(&events, "the checks", |tree| {
        tree.text().contains("Unit tests")
    });
    let text = checks.text();
    for shown in [
        "Failed",
        "3 tests failed in src/sync/client.test.ts",
        "Required",
        "Running and queued",
        "Optional and not run",
    ] {
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
    let rig = pr_rig();
    let args = json!({
        "provider": "github", "url": "https://github.com/acme/web/pull/482", "number": 482,
        "repo": "acme/web", "title": "Add retry with backoff to the sync client", "cwd": PR_PROJECT,
    });
    let (sent, events) = mpsc::channel();
    let page = rig
        .host
        .open_hidden_page_with_args(&native("pr-tracker", "pr.js"), Some(&args), move |event| {
            let _ = sent.send(event);
        })
        .unwrap();
    page.set_visible(true);
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
    // 1 and 9 open folded groups; 2 and 10 to 103 are folded in them.
    let shown: Vec<String> = [1, 3, 4, 5, 6, 7, 8, 9]
        .into_iter()
        .chain(104..=124)
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
    let rig = pr_rig();
    let args = json!({
        "provider": "azure", "url": "https://dev.azure.com/acme/Web/_git/web/pullrequest/482", "number": 482,
        "repo": "Web/web", "title": "Add retry with backoff to the sync client", "cwd": PR_PROJECT,
        "remote": { "kind": "azure", "org": "https://dev.azure.com/acme", "project": "Web", "repo": "web" },
    });
    let (sent, events) = mpsc::channel();
    let page = rig
        .host
        .open_hidden_page_with_args(&native("pr-tracker", "pr.js"), Some(&args), move |event| {
            let _ = sent.send(event);
        })
        .unwrap();
    page.set_visible(true);
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
fn pr_tracker_main_starts_in_the_native_host_and_its_settings_page_renders() {
    let mut plugin = load("pr-tracker");
    // No program runs: the CLIs a real main asks for are refused, as for a plugin that may not run them.
    plugin.manifest.permissions.exec.clear();
    let rig = Rig::with(plugin, true, vec![pane("1", "/work/web")]);
    let (on_event, main_errors) = errors();
    rig.host
        .start_main(&native("pr-tracker", "main.js"), on_event)
        .unwrap();
    assert!(
        main_errors
            .recv_timeout(std::time::Duration::from_millis(800))
            .is_err(),
        "main reported an error"
    );
    let (_page, events) = open_page(&rig.host, &native("pr-tracker", "settings.js"));
    let settings = tree(&events, "the settings", |tree| {
        // The token rows fill in once `wings.secrets.has` answers, a turn after the first tree.
        tree.text().contains("Codeberg") && tree.text().contains("No token saved.")
    });
    let text = settings.text();
    for shown in [
        "Sidebar",
        "New pull requests",
        "Pull request hosts",
        "No token saved.",
    ] {
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

/// Work item with the fixture main that answers the Sprint block's query, in the project `/work/web`.
fn work_item_rig() -> Rig {
    let rig = Rig::with(load("work-item"), true, vec![]);
    *rig.env.projects.lock().unwrap() = vec![PathBuf::from("/work/web")];
    rig.host.publish_projects(vec![Project {
        id: "/work/web".into(),
        path: "/work/web".into(),
        name: "web".into(),
        parent_id: None,
    }]);
    // The project on screen, which the app always says and the pages follow with `onProject`.
    rig.host.publish_project(Some(Path::new("/work/web")));
    let fixture =
        fs::read_to_string(repo_plugin("work-item").join("preview/native-host.js")).unwrap();
    let (on_event, _errors) = errors();
    rig.host.start_main(&fixture, on_event).unwrap();
    rig
}

#[test]
fn work_item_sprint_block_draws_a_column_per_state_and_saves_its_filter_as_block_state() {
    let rig = work_item_rig();
    let (page, events, saved) = open_with_state(
        &rig.host,
        &native("work-item", "sprint.js"),
        None,
        super::StatePlace::Block,
        None,
    );
    let mine = tree(&events, "the sprint", |tree| {
        tree.text().contains("Add retry with backoff")
    });
    assert_eq!(chips_value(&mine.children, "Show").as_deref(), Some("mine"));
    assert!(!mine.text().contains("Show a notice while retrying"));
    assert_eq!(
        count_nodes(&mine.children, &|node| matches!(node, Node::BoardColumn(_))),
        2,
        "Active and Closed: {}",
        mine.text()
    );

    let show = chips_named(&mine.children, "Show").unwrap();
    assert!(page.dispatch(&show, Some(&json!({ "value": "all" }))));
    tree(&events, "the whole sprint", |tree| {
        tree.text().contains("Show a notice while retrying")
    });
    assert_eq!(*saved.lock().unwrap(), [json!({ "mine": false })]);
}

#[test]
fn work_item_sprint_block_starts_from_its_saved_filter() {
    let rig = work_item_rig();
    let (_page, events, _saved) = open_with_state(
        &rig.host,
        &native("work-item", "sprint.js"),
        None,
        super::StatePlace::Block,
        Some(json!({ "mine": false })),
    );
    let all = tree(&events, "the whole sprint", |tree| {
        tree.text().contains("Show a notice while retrying")
    });
    assert_eq!(chips_value(&all.children, "Show").as_deref(), Some("all"));
}

#[test]
fn work_item_tab_without_a_work_item_says_where_to_open_one() {
    let rig = work_item_rig();
    let (_page, events) = open_page(&rig.host, &native("work-item", "item.js"));
    tree(&events, "the empty tab", |tree| {
        tree.text().contains("Open a work item from a pane's badge")
    });
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
        .start_main(&native("tasks", "main.js"), on_event)
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
        &native("tasks", "board.js"),
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
        &native("tasks", "board.js"),
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

#[test]
fn tasks_list_starts_with_the_groups_its_tab_saved_as_folded_and_saves_an_unfold() {
    let rig = tasks_rig();
    let (page, events, saved) = open_with_state(
        &rig.host,
        &native("tasks", "board.js"),
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
            &native("tasks", "board.js"),
            None,
            super::StatePlace::Tab,
            None,
        );
        add_issue(&page, &events, "Fix the login redirect");
    }
    let args = json!({ "key": "WEB-1", "projectId": "/work/web" });
    let (page, events, _saved) = open_with_state(
        &rig.host,
        &native("tasks", "issue.js"),
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
            &native("tasks", "board.js"),
            None,
            super::StatePlace::Tab,
            None,
        );
        add_issue(&page, &events, "Fix the login redirect");
    }
    let args = json!({ "key": "WEB-1", "projectId": "/work/web" });
    let (page, events, _saved) = open_with_state(
        &rig.host,
        &native("tasks", "issue.js"),
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
            &native("tasks", "board.js"),
            None,
            super::StatePlace::Tab,
            None,
        );
        add_issue(&page, &events, "Fix the login redirect");
    }
    let args = json!({ "key": "WEB-1", "projectId": "/work/web" });
    let (_page, events, _saved) = open_with_state(
        &rig.host,
        &native("tasks", "issue.js"),
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
    assert!(
        icon.markup
            .contains(r#"<rect x="2.5" y="2.5" width="11" height="11" rx="2.5"/><path d="m5.5 8.2 1.8 1.8 3.2-3.6"/>"#),
        "{}",
        icon.markup
    );
}

/// Every page of the repo's plugins, built to share React (`dist/native/shared/<page>.js`), draws what its build that
/// brings its own React draws, side by side on one host, and the app picks it.
#[test]
fn each_page_built_to_share_react_draws_what_its_own_build_draws() {
    let mut checked = 0;
    for entry in fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins")).unwrap() {
        let dir = entry.unwrap().path();
        let Some(library) = crate::installed::library(&dir) else {
            continue;
        };
        let name = dir.file_name().unwrap().to_str().unwrap().to_owned();
        let root = super::tests::Root::new();
        let host = super::PluginHost::new(root.config(&name));
        host.set_library(Some(library.clone()));
        for page in fs::read_dir(dir.join("dist/native/shared")).unwrap() {
            let stem = page.unwrap().path().file_stem().unwrap().to_str().unwrap().to_owned();
            let bundle = dir.join("dist/native").join(format!("{stem}.js"));
            let lean = crate::installed::page_source(&dir, &bundle, Some(&library)).unwrap();
            assert!(lean.starts_with(crate::page::runtime::SHARED_MARKER), "{name}/{stem}");
            let first = |source: &str| {
                let (_page, received) = open_page(&host, source);
                tree(&received, &format!("{name}/{stem}"), |_| true)
            };
            assert_eq!(first(&lean), first(&native(&name, &format!("{stem}.js"))), "{name}/{stem}");
            checked += 1;
        }
    }
    assert!(checked >= 13, "only {checked} pages");
}
