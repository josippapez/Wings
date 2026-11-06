//! The plugins of the repo's `plugins/` that ship a native build, run as the app runs them: their `dist/native` bundles
//! against a fake app and the manifest they declare, so a call a plugin makes meets the permission its manifest grants.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
};

use serde_json::json;
use wings_plugins::{loading, manifest::Plugin};

use super::{
    PanePort, SessionEntry,
    testing::{Frame, Rig, Seen, WAIT, pane},
    ui::Tone,
};
use crate::page::runtime::Event;

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
    *rig.env.claude.lock().unwrap() = claude.clone();
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
    *rig.env.claude.lock().unwrap() = claude.clone();
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
    *rig.env.claude.lock().unwrap() = claude.clone();
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
