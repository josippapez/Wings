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
    }]);
    rig.host
        .publish_panes(vec![pane("1", &root.to_string_lossy())]);

    let (page, events) = open_page(&rig.host, &native("worktrees", "sidebar.js"));
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
        },
        Project {
            id: linked.to_string_lossy().into(),
            path: linked.to_string_lossy().into(),
            name: "Fixture feature".into(),
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

#[test]
fn worktrees_sidebar_shows_the_native_empty_state_without_repos() {
    let rig = Rig::with(load("worktrees"), true, vec![]);
    let (_page, events) = open_page(&rig.host, &native("worktrees", "sidebar.js"));
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
    let (page, events) = open_page(&rig.host, &native("worktrees", "sidebar.js"));
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
    let source: &'static str = Box::leak(native("worktrees", "sidebar.js").into_boxed_str());
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
        .start_main(&native("dev-servers", "main.js"), on_event)
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
        .start_main(&native("hud", "main.js"), on_event)
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
        .start_main(&native("hud", "main.js"), on_event)
        .unwrap();

    let (sender, events) = mpsc::channel();
    let page = rig
        .host
        .open_page(&native("hud", "sidebar.js"), move |event| {
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
        json!({ "rateLimits": null, "sessions": {} })
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
