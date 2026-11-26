//! Schedules' list, forms and modals.
use super::*;

/// Schedules' real main (`plugins/schedules/rust`) over made-up schedules and one paused "assigned" entry with tickets,
/// and its pages. The entries are kept in a storage folder of their own, so nothing runs: no schedule is due, and the
/// entry is paused. Then the same pages over nothing at all, and the modals that ask to allow a schedule an agent adds, and a start it asks for.
pub(super) fn schedules(out: PathBuf) {
    let project = out.join("fixture-project");
    std::fs::create_dir_all(&project).unwrap();
    let id = project.to_string_lossy().into_owned();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let hour = 3_600_000;
    let manifest: wings_plugins::manifest::Manifest =
        serde_json::from_str(include_str!("../../../../plugins/schedules/wings-plugin.json")).unwrap();
    let module = |frame: &str| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../plugins/schedules/dist/native/{frame}.wasm"));
        format!("{}{}", wings_native::page::runtime::wasm::MARKER, path.canonicalize().expect("sh build-rust.sh schedules, in plugins/").display())
    };
    // A host over its own storage folder, with the project published.
    let host_over = |folder: &str| {
        let storage = out.join(folder);
        let _ = std::fs::remove_dir_all(&storage);
        let host = PluginHost::with_access(
            PluginConfig { id: "schedules".into(), root: storage.clone(), scope: "shot".into() },
            Access {
                plugin: wings_plugins::manifest::Plugin { manifest: manifest.clone(), dir: project.clone(), icon_svg: None },
                builtin: true,
                env: std::sync::Arc::new(FixtureEnv(project.clone())),
            },
        );
        host.publish_projects(vec![wings_native::page::plugin::Project { hidden_sections: Vec::new(), id: id.clone(), name: "shop".into(), parent_id: None, path: id.clone() }]);
        (host, wings_plugin_storage::Storage::new(storage))
    };
    let (host, store) = host_over("fixture-storage-schedules");
    store
        .set(
            "schedules",
            "shot",
            "schedules",
            json!([
                { "id": "a1", "name": "Morning review", "projectId": id, "agent": "claude", "runIn": "chat",
                  "prompt": "Check yesterday's failed CI runs and open a fix for the flaky test.",
                  "when": { "kind": "daily", "time": "09:00", "days": [1, 2, 3, 4, 5] }, "enabled": true, "anchor": now,
                  "lastRun": { "at": now - 2 * hour, "paneId": "3", "ok": true } },
                { "id": "a2", "name": "", "projectId": id, "agent": "opencode",
                  "prompt": "Update the dependencies that have a patch release and run the tests.",
                  "when": { "kind": "every", "hours": 6 }, "enabled": false, "anchor": now,
                  "lastRun": { "at": now - 26 * hour, "ok": false, "error": "The project is no longer in Wings." } },
                { "id": "a3", "name": "Year-end check", "projectId": id, "agent": "claude",
                  "prompt": "Check that the release notes list every change since the last tag.",
                  "when": { "kind": "once", "date": "2099-12-16", "time": "09:00" }, "enabled": true, "anchor": now },
                { "id": "a4", "name": "Migration dry run", "projectId": id, "agent": "claude",
                  "prompt": "Dry-run the pending migrations against the staging copy.",
                  "when": { "kind": "once", "date": "2026-11-03", "time": "09:00" }, "enabled": true, "anchor": now,
                  "lastRun": { "at": now - 6 * 24 * hour, "paneId": "4", "ok": true } },
            ]),
        )
        .unwrap();
    let assigned = json!([{
                "id": "w1", "name": "Frontend tickets", "projectId": id, "agent": "claude",
                "prompt": "Work on the ticket at {url}, titled \"{title}\".", "minFreeGb": 10, "enabled": false,
                "primed": true, "seen": ["41", "42", "43"], "states": ["Open"], "prefixes": ["FE"],
                "waiting": [
                    { "id": "42", "title": "FE: Show the coupon field on mobile", "url": "https://github.com/example/shop/issues/42", "at": now - hour },
                    { "id": "43", "title": "FE: Round shipping rates in the cart", "url": "https://github.com/example/shop/issues/43", "at": now - hour / 2 },
                ],
                "source": { "kind": "github", "host": "github.com", "path": "example/shop" },
                "tickets": [
                    { "id": "42", "title": "FE: Show the coupon field on mobile", "url": "https://github.com/example/shop/issues/42", "state": "Open", "type": "bug" },
                    { "id": "40", "title": "BE: Cache the rates", "url": "https://github.com/example/shop/issues/40", "state": "In review" },
                ],
                "lastCheck": { "at": now - 20 * 60_000, "ok": true, "count": 2 },
            }]);
    store.set("schedules", "shot", "assigned", assigned.clone()).unwrap();
    host.start_main(&module("main"), |event| {
        if let wings_native::page::runtime::Event::Error { message, .. } = event {
            panic!("{message}");
        }
    })
    .unwrap();
    let (empty_host, _) = host_over("fixture-storage-schedules-empty");
    // A host of its own for the ask to start, so the modal shows it first.
    let (start_host, start_store) = host_over("fixture-storage-schedules-start");
    start_store.set("schedules", "shot", "assigned", assigned).unwrap();
    start_host
        .start_main(&module("main"), |event| {
            if let wings_native::page::runtime::Event::Error { message, .. } = event {
                panic!("{message}");
            }
        })
        .unwrap();
    let mut cx = app();
    let source = module("schedules");
    // What to press to reach each view, and a text it shows.
    let views: [(&str, &PluginHost, &str, Option<(&str, usize)>, &str); 8] = [
        ("list", &host, &source, None, "Frontend tickets"),
        ("new", &host, &source, Some(("Add entry", 0)), "Start an agent"),
        ("edit-schedule", &host, &source, Some(("Edit Morning review", 0)), "Prompt"),
        ("edit-once", &host, &source, Some(("Edit Year-end check", 0)), "Year, month, day"),
        ("edit-entry", &host, &source, Some(("Edit Frontend tickets in shop", 0)), "Keep free"),
        ("empty", &empty_host, &source, None, "Nothing set up yet"),
        ("approve", &host, &module("approve"), None, "asks to add a schedule"),
        ("approve-start", &start_host, &module("approve"), None, "asks to start an agent"),
    ];
    // An agent asks to add a schedule, which waits for the user in the modal.
    let (reply, _answer) = wings_native::page::runtime::Reply::channel();
    host.run_tool("add", json!({ "prompt": "Summarize last week's merged pull requests.", "project": "shop", "time": "08:30", "days": ["mon", "fri"] }), None, reply);
    let (reply, _start_answer) = wings_native::page::runtime::Reply::channel();
    start_host.run_tool("start", json!({ "id": "w1", "ticket": "42" }), None, reply);
    for width in [1000.0, 640.0] {
        for (name, host, source, press, expected) in views {
            let handle = cx
                .open_offscreen_window(size(px(width), px(if name.starts_with("approve") { 420.0 } else { 900.0 })), |_, cx| {
                    cx.new(|cx| PageView::in_plugin(host, source, cx))
                })
                .unwrap();
            let window = handle.into();
            // A page starts hidden and loads once its tab is shown.
            handle
                .read_with(&cx, |view, _| view.set_visible(true))
                .unwrap();
            let until = |cx: &mut VisualTestAppContext, text: &str| {
                let deadline = Instant::now() + Duration::from_secs(10);
                while !handle
                    .read_with(cx, |view, _| view.text().contains(text))
                    .unwrap()
                {
                    assert!(
                        Instant::now() < deadline,
                        "page did not show {text:?}: {}",
                        handle.read_with(cx, |view, _| view.text()).unwrap()
                    );
                    wait(cx, window, Duration::from_millis(20));
                }
            };
            until(&mut cx, if name.starts_with("approve") || name == "empty" { expected } else { "Frontend tickets" });
            if let Some((label, nth)) = press {
                let pressed = handle
                    .read_with(&cx, |view, _| view.press(label, nth))
                    .unwrap();
                assert!(pressed, "no button {label}");
            }
            until(&mut cx, expected);
            // Allow waits a second after the ask shows.
            wait(&mut cx, window, Duration::from_millis(if name.starts_with("approve") { 1300 } else { 300 }));
            save(
                &mut cx,
                window,
                out.join(format!("schedules-{name}-{width}.png")),
            );
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
    }
}
