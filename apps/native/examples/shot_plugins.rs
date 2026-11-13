//! Renders Worktrees' sidebar, which is written in Rust, offscreen at 1000 and 640 points wide, over a made-up repo in
//! the output folder, never the user's. With `--changes` it renders Changes' built
//! pages, with `--schedules` Schedules' list and forms, and with
//! `--hud` the HUD's sidebar, with
//! `--disk` Disk usage's overview and the map of a folder of made-up files it scans, with `--work-item` Work item's tab
//! and Sprint block, and with `--tasks` the Issues board, list, issue and block, each over made-up data. With
//! `--external <folder>` it renders the separately shipped Sprint board, Cyclops and Claude accounts plugins from their
//! repos in that folder, popovers too (`shot_plugins/external.rs`).
//! `--tooltips` points at a titled Button and Text of a made-up page until their tooltips show.
//! Nothing shows on screen and focus doesn't move. macOS only (real Metal rendering).
//!
//! cargo run --manifest-path apps/native/Cargo.toml --example shot_plugins --features test-support -- <out-dir>

use std::{
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use gpui::{AnyWindowHandle, AppContext, Modifiers, VisualTestAppContext, point, px, size};
use serde_json::json;
use wings_native::{
    actions,
    icons::Assets,
    page::{
        PageView,
        plugin::{
            Access, Env, PaneEntry, PanePort, PluginConfig, PluginHost, SessionEntry,
            ui::{Badge, PaneStatus},
        },
        runtime::fake::{self, Cx, Guest, Hear},
    },
};

#[path = "shot_plugins/external.rs"]
mod external;

fn wait(cx: &mut VisualTestAppContext, window: AnyWindowHandle, duration: Duration) {
    let until = Instant::now() + duration;
    while Instant::now() < until {
        cx.run_until_parked();
        cx.advance_clock(Duration::from_millis(20));
        let _ = cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx));
        thread::sleep(Duration::from_millis(20));
    }
}

fn save(cx: &mut VisualTestAppContext, window: AnyWindowHandle, path: PathBuf) {
    let _ = cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx));
    cx.capture_screenshot(window)
        .expect("render the window")
        .save(&path)
        .expect("save the PNG");
    println!("{}", path.display());
}

/// An offscreen app with real rendering. Plugin pages run on threads of their own and wake their views from there,
/// which the app's test scheduler records as nondeterminism by capturing a symbolized backtrace on the waking thread.
/// The first one takes about a second in a debug build, inside the page's turn, so the page was stopped for running too
/// long and stayed empty. Allowing parking turns that check off, as the app's own window tests do.
fn app() -> VisualTestAppContext {
    let mut cx = VisualTestAppContext::with_asset_source(
        gpui_platform::current_platform(false),
        std::sync::Arc::new(Assets),
    );
    cx.executor().allow_parking();
    cx.update(actions::bind);
    cx
}

fn main() {
    let out = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "shots".into()));
    std::fs::create_dir_all(&out).expect("create the output folder");
    if std::env::args().any(|arg| arg == "--changes") {
        changes(out);
        return;
    }
    if std::env::args().any(|arg| arg == "--stopped") {
        stopped(out);
        return;
    }
    if std::env::args().any(|arg| arg == "--schedules") {
        schedules(out);
        return;
    }
    if std::env::args().any(|arg| arg == "--pr-tracker") {
        pr_tracker(out);
        return;
    }
    if std::env::args().any(|arg| arg == "--hud") {
        hud(out);
        return;
    }
    if std::env::args().any(|arg| arg == "--disk") {
        disk(out);
        return;
    }
    if std::env::args().any(|arg| arg == "--work-item") {
        work_item(out);
        return;
    }
    if let Some(at) = std::env::args().position(|arg| arg == "--external") {
        let root = std::env::args()
            .nth(at + 1)
            .expect("--external <folder of the plugin repos>");
        external::external(out, root.into());
        return;
    }
    if std::env::args().any(|arg| arg == "--tooltips") {
        tooltips(out);
        return;
    }
    if std::env::args().any(|arg| arg == "--tasks") {
        tasks(out);
        return;
    }
    worktrees(out);
}

/// An app with one project, which does nothing for a plugin.
/// What Claude Code told `wings statusline`, for the HUD.
static STATUSLINE: std::sync::Mutex<Option<serde_json::Value>> = std::sync::Mutex::new(None);

struct FixtureEnv(PathBuf);
impl Env for FixtureEnv {
    fn statusline(&self) -> serde_json::Value {
        STATUSLINE.lock().unwrap().clone().unwrap_or_else(|| json!({ "rateLimits": [], "sessions": {} }))
    }
    fn projects(&self) -> Vec<PathBuf> {
        vec![self.0.clone()]
    }
    fn current_project(&self) -> Option<PathBuf> {
        Some(self.0.clone())
    }
    fn claude_dirs(&self) -> Vec<PathBuf> {
        vec![self.0.clone()]
    }
    fn ports(&self) -> Result<Vec<PanePort>, String> {
        Ok(vec![])
    }
    fn open_url(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn notify(&self, _: wings_native::page::plugin::Notification) -> Result<(), String> {
        Ok(())
    }
    fn set_badge(&self, _: &str, _: &str, _: Option<Badge>) {}
    fn set_pane_status(&self, _: &str, _: &str, _: Option<PaneStatus>) {}
}

/// The made-up data's "now": when the run started, so data made up later in the run lines up with data made earlier.
fn now_ms() -> u64 {
    static START: std::sync::LazyLock<u64> = std::sync::LazyLock::new(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    });
    *START
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

/// Actual built pages in a fixture host. This does not exercise workspace openView/window routing.
fn changes(out: PathBuf) {
    let dev = out.join("fixture-changes");
    let _ = std::fs::remove_dir_all(&dev);
    let root = dev.join("acme-web");
    std::fs::create_dir_all(root.join("src/cart")).unwrap();
    let root = root.canonicalize().unwrap();
    let git = |days_ago: u64, args: &[&str]| {
        let date = format!("{} +0000", now_ms() / 1000 - days_ago * 86_400);
        let done = std::process::Command::new("git")
            .args(["-c", "user.email=ana@example.com", "-c", "user.name=Ana Kovač", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(&root)
            .envs([("GIT_AUTHOR_DATE", &date), ("GIT_COMMITTER_DATE", &date)])
            .output()
            .unwrap();
        assert!(done.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&done.stderr));
    };
    let commit = |days_ago: u64, file: &str, text: &str, subject: &str| {
        std::fs::write(root.join(file), text).unwrap();
        git(days_ago, &["add", file]);
        git(days_ago, &["commit", "-qm", subject]);
    };
    git(0, &["init", "-q", "-b", "main"]);
    commit(30, "README.md", "acme-web\n", "Start the web shop");
    commit(12, "src/cart/total.ts", "export const total = 0;\n", "Add the cart total");
    git(0, &["tag", "v1.0"]);
    git(0, &["checkout", "-q", "-b", "feature/coupons"]);
    commit(3, "src/cart/coupon.ts", "export const coupon = null;\n", "Add coupon validation");
    git(0, &["checkout", "-q", "main"]);
    commit(2, "src/cart/total.ts", "export const total = 1;\n", "Count the first item");
    git(1, &["merge", "-q", "--no-ff", "-m", "Merge feature/coupons", "feature/coupons"]);
    // What isn't committed: a changed file, a deleted one, a new one and a binary.
    std::fs::write(root.join("src/cart/total.ts"), "export const total = 2;\nexport const tax = 0.25;\n").unwrap();
    std::fs::remove_file(root.join("README.md")).unwrap();
    std::fs::write(root.join("src/cart/shipping.ts"), "export const shipping = 5;\n").unwrap();
    std::fs::write(root.join("logo.png"), [0u8, 159, 146, 150, 0, 1, 2]).unwrap();

    let manifest = serde_json::from_str(include_str!("../../../plugins/changes/wings-plugin.json")).unwrap();
    let storage = out.join("fixture-storage-changes");
    let _ = std::fs::remove_dir_all(&storage);
    let host = PluginHost::with_access(
        PluginConfig { id: "changes".into(), root: storage, scope: "shot".into() },
        Access {
            plugin: wings_plugins::manifest::Plugin { manifest, dir: root.clone(), icon_svg: None },
            builtin: false,
            env: std::sync::Arc::new(FixtureEnv(root.clone())),
        },
    );
    host.publish_panes(vec![PaneEntry {
        pane_id: "1".into(),
        cwd: root.to_string_lossy().into(),
        command: "zsh".into(),
        project: "acme-web".into(),
        project_id: root.to_string_lossy().into(),
        session: None,
        focused: true,
    }]);
    let module = |frame: &str| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../plugins/changes/dist/native/{frame}.wasm"));
        format!("{}{}", wings_native::page::runtime::wasm::MARKER, path.canonicalize().expect("sh build-rust.sh changes, in plugins/").display())
    };
    host.start_main(&module("main"), |event| {
        if let wings_native::page::runtime::Event::Error { message, .. } = event {
            panic!("{message}");
        }
    })
    .unwrap();
    let mut cx = app();
    for (frame, width, height) in [("sidebar", 1000.0, 760.0), ("sidebar", 640.0, 760.0), ("sidebar", 360.0, 760.0), ("sidebar", 250.0, 760.0), ("graph", 1100.0, 420.0)] {
        let source = module(frame);
        let handle = cx
            .open_offscreen_window(size(px(width), px(height)), |_, cx| cx.new(|cx| PageView::in_plugin(&host, &source, cx)))
            .unwrap();
        let window = handle.into();
        handle.read_with(&cx, |view, _| view.set_visible(true)).unwrap();
        let shown = |cx: &mut VisualTestAppContext, expected: &str, name: &str| {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !handle.read_with(cx, |view, _| view.text().contains(expected)).unwrap() {
                assert!(Instant::now() < deadline, "page did not show {expected:?}: {}", handle.read_with(cx, |view, _| view.text()).unwrap());
                wait(cx, window, Duration::from_millis(20));
            }
            wait(cx, window, Duration::from_millis(300));
            save(cx, window, out.join(format!("changes-{name}-{width}.png")));
        };
        if frame == "graph" {
            shown(&mut cx, "5 commits on every branch", "graph");
        } else {
            shown(&mut cx, "shipping.ts", "files");
            assert!(handle.read_with(&cx, |view, _| view.press_chip("History")).unwrap());
            shown(&mut cx, "Add coupon validation", "history");
            assert!(handle.read_with(&cx, |view, _| view.press_chip("Graph")).unwrap());
            // The graph's rows aren't text, so this waits for its git log to be read.
            wait(&mut cx, window, Duration::from_millis(700));
            shown(&mut cx, "acme-web", "sidebar-graph");
            // The pick is kept per repo, and the next width starts on Files again.
            assert!(handle.read_with(&cx, |view, _| view.press_chip("Files")).unwrap());
            wait(&mut cx, window, Duration::from_millis(300));
        }
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
}

/// A page with a titled Button and a titled Text, each pointed at until its tooltip shows.
fn tooltips(out: PathBuf) {
    let words = |id: u64, text: &str| json!([{ "id": id, "type": "#text", "text": text }]);
    let source = fake::page("page", move |_| {
        json!({ "id": 0, "type": "#root", "children": [{ "id": 1, "type": "Page", "children": [
            { "id": 2, "type": "Slot", "props": { "name": "body" }, "children": [
                { "id": 3, "type": "Button", "props": { "variant": "outline", "title": "Opens a terminal as this account running claude auth login" }, "children": words(4, "Sign in") },
                { "id": 5, "type": "Text", "props": { "tone": "muted", "title": "/Users/ada/.claude-work" }, "children": words(6, "ada@example.com") }] }] }] })
    });
    let host = PluginHost::new(PluginConfig { id: "tooltips".into(), root: out.join("fixture-storage-tooltips"), scope: "shot".into() });
    let mut cx = app();
    for width in [1000.0, 640.0] {
        let handle = cx.open_offscreen_window(size(px(width), px(240.0)), |_, cx| cx.new(|cx| PageView::in_plugin(&host, &source, cx))).unwrap();
        let window = handle.into();
        wait(&mut cx, window, Duration::from_millis(800));
        for (name, at) in [("button", point(px(40.0), px(44.0))), ("text", point(px(40.0), px(80.0)))] {
            cx.simulate_mouse_move(window, at, None, Modifiers::default());
            wait(&mut cx, window, Duration::from_millis(100));
            cx.advance_clock(Duration::from_millis(600));
            wait(&mut cx, window, Duration::from_millis(100));
            save(&mut cx, window, out.join(format!("tooltip-{name}-{width}.png")));
            cx.simulate_mouse_move(window, point(px(width - 5.0), px(230.0)), None, Modifiers::default());
            wait(&mut cx, window, Duration::from_millis(100));
        }
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
}

/// A page that stopped, with its error and Reload, at a tab's widths and a popover's.
fn stopped(out: PathBuf) {
    // Shows "Up", then stops a moment later as a page that never gives the turn back is stopped.
    struct Hangs;
    impl Guest for Hangs {
        fn start(&mut self, cx: &mut Cx<'_>) {
            cx.render(&json!({ "id": 0, "type": "#root", "children": [{ "id": 1, "type": "Text", "children": [{ "id": 2, "type": "#text", "text": "Up" }] }] }));
            cx.later(100);
        }
        fn timer(&mut self, cx: &mut Cx<'_>, _id: u32) {
            cx.stop("the page ran for too long and was stopped");
        }
    }
    let source = fake::source("page", || Box::new(Hangs));
    let host = PluginHost::new(PluginConfig { id: "stopped".into(), root: out.join("fixture-storage-stopped"), scope: "shot".into() });
    let mut cx = app();
    for width in [1000.0, 640.0, 320.0] {
        let handle = cx.open_offscreen_window(size(px(width), px(240.0)), |_, cx| cx.new(|cx| PageView::in_plugin(&host, &source, cx))).unwrap();
        let window = handle.into();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !handle.read_with(&cx, |view, _| view.stopped()).unwrap() {
            assert!(Instant::now() < deadline, "the page never stopped");
            wait(&mut cx, window, Duration::from_millis(20));
        }
        save(&mut cx, window, out.join(format!("stopped-{width}.png")));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
}

/// Schedules' real main (`plugins/schedules/rust`) over made-up schedules and one paused "assigned" entry with tickets,
/// and its pages. The entries are kept in a storage folder of their own, so nothing runs: no schedule is due, and the
/// entry is paused. Then the same pages over nothing at all, and the modals that ask to allow a schedule an agent adds, and a start it asks for.
fn schedules(out: PathBuf) {
    let project = out.join("fixture-project");
    std::fs::create_dir_all(&project).unwrap();
    let id = project.to_string_lossy().into_owned();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let hour = 3_600_000;
    let manifest: wings_plugins::manifest::Manifest =
        serde_json::from_str(include_str!("../../../plugins/schedules/wings-plugin.json")).unwrap();
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
                { "id": "a1", "name": "Morning review", "projectId": id, "agent": "claude",
                  "prompt": "Check yesterday's failed CI runs and open a fix for the flaky test.",
                  "when": { "kind": "daily", "time": "09:00", "days": [1, 2, 3, 4, 5] }, "enabled": true, "anchor": now,
                  "lastRun": { "at": now - 2 * hour, "paneId": "3", "ok": true } },
                { "id": "a2", "name": "", "projectId": id, "agent": "opencode",
                  "prompt": "Update the dependencies that have a patch release and run the tests.",
                  "when": { "kind": "every", "hours": 6 }, "enabled": false, "anchor": now,
                  "lastRun": { "at": now - 26 * hour, "ok": false, "error": "The project is no longer in Wings." } },
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
    let views: [(&str, &PluginHost, &str, Option<(&str, usize)>, &str); 7] = [
        ("list", &host, &source, None, "Frontend tickets"),
        ("new", &host, &source, Some(("Add entry", 0)), "Start an agent"),
        ("edit-schedule", &host, &source, Some(("Edit", 0)), "Prompt"),
        ("edit-entry", &host, &source, Some(("Edit", 2)), "Keep free"),
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

/// The Worktrees sidebar, which is written in Rust (`plugins/worktrees/rust`), over a repo made for it: worktrees that
/// are merged, changed, locked, detached and gone. Then the card that asks before the merged one is removed.
fn worktrees(out: PathBuf) {
    let dev = out.join("fixture-worktrees");
    let _ = std::fs::remove_dir_all(&dev);
    let root = dev.join("acme-web");
    std::fs::create_dir_all(&root).unwrap();
    let dev = dev.canonicalize().unwrap();
    let root = root.canonicalize().unwrap();
    let git = |dir: &std::path::Path, days_ago: u64, args: &[&str]| {
        let date = format!("{} +0000", now_ms() / 1000 - days_ago * 86_400);
        let done = std::process::Command::new("git")
            .args(["-c", "user.email=test@example.com", "-c", "user.name=Wings", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .envs([("GIT_AUTHOR_DATE", &date), ("GIT_COMMITTER_DATE", &date)])
            .output()
            .unwrap();
        assert!(done.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&done.stderr));
    };
    let commit = |dir: &std::path::Path, days_ago: u64, file: &str| {
        std::fs::write(dir.join(file), file).unwrap();
        git(dir, days_ago, &["add", file]);
        git(dir, days_ago, &["commit", "-qm", file]);
    };
    git(&root, 0, &["init", "-q", "-b", "main"]);
    commit(&root, 60, "README.md");
    let linked = |name: &str, branch: &str| {
        let path = dev.join(name);
        git(&root, 0, &["worktree", "add", "-q", "-b", branch, path.to_str().unwrap()]);
        path
    };
    let search = linked("acme-web-search", "feature/search");
    commit(&search, 5, "search.ts");
    git(&root, 5, &["merge", "-q", "feature/search"]);
    commit(&root, 1, "CHANGELOG.md");
    let checkout = linked("acme-web-checkout", "feature/checkout");
    commit(&checkout, 2, "checkout.ts");
    std::fs::write(checkout.join("cart.ts"), "not committed").unwrap();
    let hotfix = linked("acme-web-hotfix", "fix/cart-total");
    commit(&hotfix, 12, "cart.ts");
    git(&root, 0, &["worktree", "lock", "--reason", "release freeze", hotfix.to_str().unwrap()]);
    let bisect = dev.join("acme-web-bisect");
    git(&root, 0, &["worktree", "add", "-q", "--detach", bisect.to_str().unwrap()]);
    std::fs::remove_dir_all(linked("acme-web-old", "experiment/old")).unwrap();

    let manifest = serde_json::from_str(include_str!("../../../plugins/worktrees/wings-plugin.json")).unwrap();
    let storage = out.join("fixture-storage-worktrees");
    let _ = std::fs::remove_dir_all(&storage);
    let host = PluginHost::with_access(
        PluginConfig { id: "worktrees".into(), root: storage, scope: "shot".into() },
        Access {
            plugin: wings_plugins::manifest::Plugin { manifest, dir: root.clone(), icon_svg: None },
            builtin: true,
            env: std::sync::Arc::new(FixtureEnv(dev.clone())),
        },
    );
    host.publish_projects(vec![wings_native::page::plugin::Project { hidden_sections: Vec::new(),
        id: root.to_string_lossy().into(),
        path: root.to_string_lossy().into(),
        name: "acme-web".into(),
        parent_id: None,
    }]);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/worktrees/dist/native/sidebar.wasm");
    let source = format!("{}{}", wings_native::page::runtime::wasm::MARKER, path.canonicalize().expect("sh build-rust.sh worktrees, in plugins/").display());
    let mut cx = app();
    for width in [1000.0, 640.0, 360.0] {
        let handle = cx
            .open_offscreen_window(size(px(width), px(1100.0)), |_, cx| cx.new(|cx| PageView::in_plugin(&host, &source, cx)))
            .unwrap();
        let window = handle.into();
        handle.read_with(&cx, |view, _| view.set_visible(true)).unwrap();
        let shown = |cx: &mut VisualTestAppContext, expected: &str, name: &str| {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !handle.read_with(cx, |view, _| view.text().contains(expected)).unwrap() {
                assert!(Instant::now() < deadline, "page did not show {expected:?}: {}", handle.read_with(cx, |view, _| view.text()).unwrap());
                wait(cx, window, Duration::from_millis(20));
            }
            wait(cx, window, Duration::from_millis(300));
            save(cx, window, out.join(format!("worktrees-{name}-{width}.png")));
        };
        shown(&mut cx, "uncommitted", "list");
        assert!(handle.read_with(&cx, |view, _| view.press("Remove 1 merged & clean", 0)).unwrap());
        shown(&mut cx, "Also delete their branches", "confirm");
        assert!(handle.read_with(&cx, |view, _| view.text().contains("1 worktree merged and clean")).unwrap());
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
}

/// The HUD's sidebar, which is written in Rust (`plugins/hud/rust`): two Claude sessions on two accounts, with the
/// second one's pane selected, then no session at all.
fn hud(out: PathBuf) {
    const FIRST: &str = "3c21b0c5-8113-4f55-9493-fa156c3fa369";
    const SECOND: &str = "7d0e4a11-52c6-4b7e-9d57-0c2f6a1b9e44";
    let root = out.join("fixture-hud");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("projects/-work-web")).unwrap();
    let transcript = |tokens: u64, servers: &[&str], failed: serde_json::Value, skills: &[&str]| {
        let at = iso(60_000);
        let lines = [
            json!({ "type": "attachment", "uuid": "a1", "timestamp": at, "attachment": { "type": "model", "identity": { "modelId": "claude-opus-5-5" } } }),
            json!({ "type": "attachment", "uuid": "a2", "timestamp": at, "attachment": { "type": "deferred_tools_delta",
                "addedNames": servers.iter().flat_map(|server| [format!("mcp__{server}__read"), format!("mcp__{server}__write")]).collect::<Vec<_>>(),
                "removedNames": [], "failedMcpServers": failed, "pendingMcpServers": ["sentry"] } }),
            json!({ "type": "attachment", "uuid": "a3", "timestamp": at, "attachment": { "type": "invoked_skills",
                "skills": skills.iter().map(|name| json!({ "name": name })).collect::<Vec<_>>() } }),
            json!({ "type": "assistant", "uuid": "r1", "timestamp": at, "message": {
                "usage": { "input_tokens": 1000, "cache_creation_input_tokens": 2000, "cache_read_input_tokens": tokens },
                "content": [{ "type": "tool_use", "id": "t1", "name": format!("mcp__{}__read", servers[0]), "input": {} }] } }),
        ];
        lines.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n")
    };
    std::fs::write(
        root.join(format!("projects/-work-web/{FIRST}.jsonl")),
        transcript(113_000, &["github", "linear"], json!([{ "name": "postgres", "error": "connection refused: 127.0.0.1:5432" }]), &["wings-page-design", "wings-plugin"]),
    )
    .unwrap();
    std::fs::write(
        root.join(format!("projects/-work-web/{SECOND}.jsonl")),
        transcript(43_000, &["github", "playwright-browser-automation-with-a-long-name"], json!([]), &["dev-core:opensrc", "verified:verified-replay-with-a-very-long-skill-name"]),
    )
    .unwrap();
    let hours = |n: u64| (now_ms() / 1000 + n * 3600) as f64;
    let limits = |account: serde_json::Value, five: u64, week: u64| {
        json!({ "account": account, "at": now_ms(),
            "fiveHour": { "usedPercentage": five, "resetsAt": hours(2) }, "sevenDay": { "usedPercentage": week, "resetsAt": hours(60) } })
    };
    *STATUSLINE.lock().unwrap() = Some(json!({
        "rateLimits": [limits(serde_json::Value::Null, 12, 61), limits("work".into(), 88, 34)],
        "sessions": { SECOND: { "account": "work", "model": "Opus 5.5", "contextWindowSize": 200_000, "usedPercentage": 23,
            "totalInputTokens": null, "totalOutputTokens": null, "currentUsage": null,
            "promptCache": { "warm": true, "ttl": "1h", "expiresAt": hours(1) }, "at": now_ms() } },
    }));

    let manifest = serde_json::from_str(include_str!("../../../plugins/hud/wings-plugin.json")).unwrap();
    let storage = out.join("fixture-storage-hud");
    let _ = std::fs::remove_dir_all(&storage);
    let host = PluginHost::with_access(
        PluginConfig { id: "hud".into(), root: storage, scope: "shot".into() },
        Access {
            plugin: wings_plugins::manifest::Plugin { manifest, dir: root.clone(), icon_svg: None },
            builtin: false,
            env: std::sync::Arc::new(FixtureEnv(root.clone())),
        },
    );
    let started_at = now_ms() - 10 * 60_000;
    let pane = |id: &str, session: &str, project: &str, name: &str, focused: bool| PaneEntry {
        pane_id: id.into(),
        cwd: "/work/web".into(),
        command: "claude".into(),
        project: project.into(),
        project_id: "/work/web".into(),
        session: Some(SessionEntry { session_id: session.into(), kind: "claude".into(), name: Some(name.into()), state: "idle".into(), started_at }),
        focused,
    };
    let both = vec![pane("1", FIRST, "web", "checkout redesign", false), pane("2", SECOND, "api", "invoice export", true)];
    let module = |frame: &str| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../plugins/hud/dist/native/{frame}.wasm"));
        format!("{}{}", wings_native::page::runtime::wasm::MARKER, path.canonicalize().expect("sh build-rust.sh hud, in plugins/").display())
    };
    host.publish_panes(both.clone());
    host.start_main(&module("main"), |event| {
        if let wings_native::page::runtime::Event::Error { message, .. } = event {
            panic!("{message}");
        }
    })
    .unwrap();
    let mut cx = app();
    let source = module("sidebar");
    // The second session's pane is selected, so its session shows and its account's limits come first.
    // Last, the first session's pane: Claude Code never reported that session, which the page says once it has had
    // the time to.
    let unreported = vec![pane("1", FIRST, "web", "checkout redesign", true), pane("2", SECOND, "api", "invoice export", false)];
    let views: [(&str, Vec<PaneEntry>, &str); 3] =
        [("selected", both, "46k of 200k tokens"), ("none", vec![], "No Claude session in your panes."), ("unreported", unreported, "isn't connected to Wings")];
    for width in [1000.0, 640.0, 360.0] {
        for (name, panes, expected) in &views {
            host.publish_panes(panes.clone());
            let handle = cx
                .open_offscreen_window(size(px(width), px(900.0)), |_, cx| cx.new(|cx| PageView::in_plugin(&host, &source, cx)))
                .unwrap();
            let window = handle.into();
            handle.read_with(&cx, |view, _| view.set_visible(true)).unwrap();
            let deadline = Instant::now() + Duration::from_secs(30);
            while !handle.read_with(&cx, |view, _| view.text().contains(expected)).unwrap() {
                assert!(Instant::now() < deadline, "page did not show {expected:?}: {}", handle.read_with(&cx, |view, _| view.text()).unwrap());
                wait(&mut cx, window, Duration::from_millis(20));
            }
            wait(&mut cx, window, Duration::from_millis(300));
            save(&mut cx, window, out.join(format!("hud-{name}-{width}.png")));
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
    }
}

/// Disk usage's real main and page modules: the disks are this Mac's, and the map is of a folder of made-up files, which it
/// scans for real. The folder is removed afterwards.
fn disk(out: PathBuf) {
    let folder = out.join("fixture-disk");
    let _ = std::fs::remove_dir_all(&folder);
    // Files of the sizes (in MiB) that give the map a few rings, written out so that they take space on disk.
    for (path, mib) in [
        ("Downloads/installer.dmg", 11),
        ("Downloads/archive.zip", 6),
        ("Downloads/old/backup.tar", 4),
        ("Code/app/target/debug.bin", 9),
        ("Code/app/node_modules/react.bin", 3),
        ("Code/app/node_modules/vite.bin", 2),
        ("Photos/2025/trip.raw", 5),
        ("Photos/2025/party.raw", 3),
        ("Movies/clip.mov", 7),
    ] {
        let file = folder.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let mut data = std::fs::File::create(file).unwrap();
        for _ in 0..mib {
            std::io::Write::write_all(&mut data, &vec![7u8; 1 << 20]).unwrap();
        }
    }
    std::fs::write(folder.join("notes.txt"), "made up\n").unwrap();
    let id = folder.to_string_lossy().into_owned();
    let module = |frame: &str| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../plugins/disk/dist/native/{frame}.wasm"));
        format!("{}{}", wings_native::page::runtime::wasm::MARKER, path.canonicalize().expect("sh build-rust.sh disk, in plugins/").display())
    };
    let fail = |event: wings_native::page::runtime::Event| {
        if let wings_native::page::runtime::Event::Error { message, .. } = event {
            panic!("{message}");
        }
    };
    let project = |name: &str, path: String| wings_native::page::plugin::Project { hidden_sections: Vec::new(), id: path.clone(), name: name.into(), parent_id: None, path };
    let host = disk_host(&out, "fixture-storage-disk", &folder, vec![project("Fixture", id)]);
    host.start_main(&module("main"), fail).unwrap();
    let mut cx = app();
    let source = module("map");
    // The Scan buttons come in the page's order: a disk's each, then the home folder's, then the projects'.
    let scan = wings_disk_usage::disks().len() + 1;
    // What to press to reach each view, one after the other with the text each shows, and what undoes it: main keeps
    // the Collector between windows.
    let views: [(&str, Steps, Steps); 3] = [
        ("overview", vec![], vec![]),
        ("map", vec![("Scan", scan, "Downloads")], vec![]),
        (
            "collected",
            vec![
                ("Scan", scan, "Downloads"),
                ("⋯", 0, "Add to Collector"),
                ("Add to Collector", 0, "1 item"),
            ],
            vec![
                ("⋯", 0, "Remove from Collector"),
                ("Remove from Collector", 0, "Drag folders"),
            ],
        ),
    ];
    shoot_disk(&mut cx, &host, &source, &out, &views);
    // The other states of the tab, which a scan of a few files is too quick to show: a main that says what the page
    // hears, as the real one does, for a scan in progress, one that failed and one that has not begun.
    let states = disk_host(&out, "fixture-storage-disk-states", &folder, vec![project("Slow", "/fixture/slow".into()), project("Broken", "/fixture/broken".into()), project("Quiet", "/fixture/quiet".into())]);
    states.start_main(&disk_states_main(), fail).unwrap();
    let views: [(&str, Steps, Steps); 3] = [
        ("scanning", vec![("Scan", scan, "Found so far")], vec![]),
        ("error", vec![("Scan", scan + 1, "Try again")], vec![]),
        ("starting", vec![("Scan", scan + 2, "Starting the scan")], vec![]),
    ];
    shoot_disk(&mut cx, &states, &source, &out, &views);
    let _ = std::fs::remove_dir_all(&folder);
}

/// A stand-in for Disk usage's main that tells its page what the real one tells it, for the states a scan of a few files
/// is too quick to show: a scan in progress (the "Slow" project), one that failed ("Broken"), and one that is asked for and
/// not begun ("Quiet"), which main neither shows nor answers.
fn disk_states_main() -> String {
    fn tell(cx: &mut Cx<'_>, name: &str, payload: serde_json::Value) {
        let message = json!({ "wings:sdk": 1, "kind": "tell", "name": name, "payload": payload });
        cx.link().broadcast(message.to_string()).unwrap();
    }
    fake::script("main", || {
        let mut scans = json!([]);
        move |cx: &mut Cx<'_>, hear: Hear<'_>| {
            let Hear::Deliver("broadcast", _, text) = hear else { return };
            let message: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
            if message["wings:sdk"] != 1 || message["kind"] != "tell" {
                return;
            }
            let state = |scans: &serde_json::Value| json!({ "scans": scans, "collector": { "items": [], "total": 0 } });
            if message["name"] == "disk-want" {
                tell(cx, "disk-state", state(&scans));
            }
            if message["name"] != "disk-ask" || message["payload"]["op"] != "scan" {
                return;
            }
            let (id, root) = (&message["payload"]["id"], message["payload"]["root"].as_str().unwrap_or_default());
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
            scans = if root.ends_with("/slow") {
                json!([{ "root": root, "state": "scanning", "expected": 8_000_000_000u64, "startedAt": now - 83_000, "stamp": now,
                         "progress": { "entries": 48213, "bytes": 3_100_000_000u64, "path": format!("{root}/node_modules/react/cjs") } }])
            } else if root.ends_with("/broken") {
                json!([{ "root": root, "state": "error", "message": format!("There's no file or folder at {root}"), "stamp": now }])
            } else {
                return;
            };
            tell(cx, "disk-state", state(&scans));
            tell(cx, "disk-done", json!({ "id": id, "result": null }));
        }
    })
}

/// What to press to reach a view of Disk usage, with the text each press shows.
type Steps<'a> = Vec<(&'a str, usize, &'a str)>;

/// Disk usage's plugin host over a storage folder of its own, its projects as given and its permissions its manifest's.
fn disk_host(out: &std::path::Path, storage: &str, folder: &std::path::Path, projects: Vec<wings_native::page::plugin::Project>) -> PluginHost {
    let storage = out.join(storage);
    let _ = std::fs::remove_dir_all(&storage);
    let manifest: wings_plugins::manifest::Manifest = serde_json::from_str(include_str!("../../../plugins/disk/wings-plugin.json")).unwrap();
    let host = PluginHost::with_access(
        PluginConfig { id: "disk".into(), root: storage, scope: "shot".into() },
        Access {
            plugin: wings_plugins::manifest::Plugin { manifest, dir: folder.to_path_buf(), icon_svg: None },
            builtin: true,
            env: std::sync::Arc::new(FixtureEnv(folder.to_path_buf())),
        },
    );
    host.publish_projects(projects);
    host
}

/// Renders each of `views` of Disk usage's page at 1000 and 640 points wide.
fn shoot_disk(cx: &mut VisualTestAppContext, host: &PluginHost, source: &str, out: &std::path::Path, views: &[(&str, Steps, Steps)]) {
    for width in [1000.0, 640.0] {
        for (name, steps, undo) in views {
            let handle = cx
                .open_offscreen_window(size(px(width), px(900.0)), |_, cx| {
                    cx.new(|cx| PageView::in_plugin(host, source, cx))
                })
                .unwrap();
            let window = handle.into();
            handle
                .read_with(cx, |view, _| view.set_visible(true))
                .unwrap();
            let until = |cx: &mut VisualTestAppContext, text: &str| {
                let deadline = Instant::now() + Duration::from_secs(20);
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
            until(cx, "Folders");
            // The disks come from the page's own calls.
            let deadline = Instant::now() + Duration::from_secs(20);
            while handle
                .read_with(cx, |view, _| view.text().contains("Looking for disks"))
                .unwrap()
            {
                assert!(Instant::now() < deadline, "the disks never came");
                wait(cx, window, Duration::from_millis(20));
            }
            let press = |cx: &mut VisualTestAppContext,
                         (label, nth, shows): &(&str, usize, &str)| {
                let pressed = handle
                    .read_with(cx, |view, _| view.press(label, *nth))
                    .unwrap();
                assert!(pressed, "no button {label} number {nth}");
                until(cx, shows);
            };
            for step in steps {
                press(cx, step);
            }
            wait(cx, window, Duration::from_millis(500));
            save(cx, window, out.join(format!("disk-{name}-{width}.png")));
            for step in undo {
                press(cx, step);
            }
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
    }
}

/// A page, what it opens with, what to press to reach a view of it, and a text it shows.
type View<'a> = (
    &'a str,
    &'a str,
    Option<&'a serde_json::Value>,
    wings_native::page::plugin::StatePlace,
    &'a [&'a str],
    &'a str,
);

/// The fake `gh` and `az` of PR tracker's shots: scripts that answer from `shot_plugins/pr_tracker_gh.json` and
/// `pr_tracker_az.json` by the words in the call, and act on the word in the file `mode` beside them: `slow` waits before
/// it answers, `error` fails as a forge that is down does, `signed-out` fails as a program with no sign-in does. A project
/// whose repository is called `empty` has no pull requests.
fn pr_tracker_programs(bin: &std::path::Path) {
    std::fs::create_dir_all(bin).unwrap();
    for (program, answers, signed_out) in [
        ("gh", include_str!("shot_plugins/pr_tracker_gh.json"), "To get started with GitHub CLI, please run:  gh auth login"),
        ("az", include_str!("shot_plugins/pr_tracker_az.json"), "ERROR: Please run 'az login' to setup account."),
    ] {
        let answers: serde_json::Map<String, serde_json::Value> = serde_json::from_str(answers).unwrap();
        let mut script = format!(
            "#!/bin/sh\nmode=$(cat \"$(dirname \"$0\")/mode\" 2>/dev/null)\ncase \"$mode\" in\n  signed-out) echo '{signed_out}' >&2; exit 1 ;;\n  error) echo 'HTTP 502: Bad Gateway' >&2; exit 1 ;;\n  slow) sleep 6 ;;\nesac\ncase \"$*\" in\n"
        );
        if program == "gh" {
            script += "  *'name: \"empty\"'*) echo '{\"data\":{\"r0\":{\"pullRequests\":{\"nodes\":[]}},\"asked\":{\"nodes\":[]}}}' ;;\n";
        }
        for (index, (words, answer)) in answers.iter().filter(|(words, _)| !words.starts_with('_')).enumerate() {
            let file = bin.join(format!("{program}-{index}.out"));
            std::fs::write(&file, answer.as_str().map_or_else(|| answer.to_string(), str::to_owned)).unwrap();
            script += &format!("  *'{words}'*) cat '{}' ;;\n", file.display());
        }
        script += "  *) echo \"no made-up answer for $*\" >&2; exit 1 ;;\nesac\n";
        let path = bin.join(program);
        std::fs::write(&path, script).unwrap();
        std::process::Command::new("chmod").arg("+x").arg(&path).status().unwrap();
    }
    // Programs are found on the login shell's PATH, then this process's: no login shell leaves this one.
    unsafe {
        std::env::set_var("SHELL", "/nonexistent-shell");
        std::env::set_var("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default()));
    }
}

/// A folder that is a git repo on `branch` whose origin is `origin`, which is how a page knows which forge to ask.
fn pr_tracker_repo(folder: &std::path::Path, origin: &str, branch: &str) {
    std::fs::create_dir_all(folder).unwrap();
    let git = |args: &[&str]| {
        let done = std::process::Command::new("git").args(args).current_dir(folder).output().unwrap();
        assert!(done.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&done.stderr));
    };
    git(&["init", "-q"]);
    let _ = std::process::Command::new("git").args(["remote", "remove", "origin"]).current_dir(folder).output();
    git(&["remote", "add", "origin", origin]);
    git(&["-c", "user.email=shot@example.com", "-c", "user.name=Shot", "commit", "-q", "--allow-empty", "-m", "fixture"]);
    git(&["branch", "-M", branch]);
}

/// PR tracker's Rust modules over a fake `gh` and `az`: the project's list (the Pull requests block, and the picker a tab
/// with no pull request shows), a pull request opened from it with its description and conversation, its Checks and
/// Files tabs, a check's run, the same pull request on Azure DevOps with its pushes and its policies as checks, the
/// settings, and the states a host can be in: loading, an error, a CLI that is signed out and a project with no remote. The
/// pages run the forges themselves and ask the main script, which runs here too, for the lists.
fn pr_tracker(out: PathBuf) {
    use wings_native::page::plugin::StatePlace::{Block, Tab};
    // The compiled code of the modules is kept for the next run: compiling them takes minutes in this build.
    wings_native::page::runtime::wasm::keep_compiled_in(std::env::temp_dir().join("wings-test-cwasm"));
    let bin = out.join("fake-pr-programs");
    pr_tracker_programs(&bin);
    let mode = |mode: &str| std::fs::write(bin.join("mode"), mode).unwrap();
    mode("");
    let (host, project) = repo_host(&out, "pr-tracker", include_str!("../../../plugins/pr-tracker/wings-plugin.json"));
    pr_tracker_repo(std::path::Path::new(&project), "git@github.com:acme/web.git", "ada/sync-retry");
    let empty = out.join("fixture-project-pr-tracker-empty");
    pr_tracker_repo(&empty, "git@github.com:acme/empty.git", "main");
    let empty_path = empty.to_string_lossy().into_owned();
    host.publish_projects(vec![
        wings_native::page::plugin::Project { hidden_sections: Vec::new(), id: project.clone(), name: "web".into(), parent_id: None, path: project.clone() },
        wings_native::page::plugin::Project { hidden_sections: Vec::new(), id: empty_path.clone(), name: "empty".into(), parent_id: None, path: empty_path.clone() },
    ]);
    // The project on screen, which the app always says and the block and the picker follow.
    host.publish_project(Some(std::path::Path::new(&project)));
    let module = |frame: &str| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../plugins/pr-tracker/dist/native/{frame}.wasm"));
        format!("{}{}", wings_native::page::runtime::wasm::MARKER, path.canonicalize().expect("sh build-rust.sh pr-tracker, in plugins/").display())
    };
    host.start_main(&module("main"), fail_on_error).unwrap();
    let (tab, block, settings) = (module("pr"), module("pulls"), module("settings"));
    let args = json!({
        "provider": "github", "url": "https://github.com/acme/web/pull/482", "number": 482,
        "repo": "acme/web", "title": "Add retry with backoff to the sync client", "cwd": project,
    });
    let azure = json!({
        "provider": "azure", "url": "https://dev.azure.com/acme/Web/_git/web/pullrequest/482", "number": 482,
        "repo": "Web/web", "title": "Add retry with backoff to the sync client", "cwd": project,
        "remote": { "kind": "azure", "org": "https://dev.azure.com/acme", "project": "Web", "repo": "web" },
    });
    // The page, what it opens with, what to press to reach each view, and a text it shows.
    let views: [View; 7] = [
        ("pull", &tab, Some(&args), Tab, &[], "Alan Turing"),
        ("checks", &tab, Some(&args), Tab, &["Checks"], "Unit tests"),
        ("files", &tab, Some(&args), Tab, &["Files"], "diff viewer"),
        ("azure", &tab, Some(&azure), Tab, &[], "Alan Turing"),
        ("azure-checks", &tab, Some(&azure), Tab, &["Checks"], "web-storybook"),
        ("run", &tab, Some(&args), Tab, &["Checks", "Unit tests"], "Install dependencies"),
        ("settings", &settings, None, Tab, &[], "Bitbucket Cloud"),
    ];
    let mut cx = app();
    let shoot = |cx: &mut VisualTestAppContext, name: &str, source: &str, args: Option<&serde_json::Value>, place, presses: &[&str], expected: &str| {
        for width in [1000.0, 640.0] {
            let state = wings_native::page::plugin::FrameState::new(place, None, |_| Ok(()));
            let handle = cx
                .open_offscreen_window(size(px(width), px(1500.0)), |_, cx| cx.new(|cx| PageView::in_plugin_with_state(&host, source, args, state, cx)))
                .unwrap();
            let window = handle.into();
            handle.read_with(cx, |view, _| view.set_visible(true)).unwrap();
            let until = |cx: &mut VisualTestAppContext, text: &str| {
                let deadline = Instant::now() + Duration::from_secs(30);
                while !handle.read_with(cx, |view, _| view.text().contains(text)).unwrap() {
                    assert!(Instant::now() < deadline, "pr-tracker {name} did not show {text:?}: {}", handle.read_with(cx, |view, _| view.text()).unwrap());
                    wait(cx, window, Duration::from_millis(20));
                }
            };
            if presses.is_empty() {
                until(cx, expected);
            }
            for (step, label) in presses.iter().enumerate() {
                let deadline = Instant::now() + Duration::from_secs(10);
                while !handle.read_with(cx, |view, _| if step == 0 { view.press_chip(label) } else { view.press_list_button(label) }).unwrap() {
                    assert!(Instant::now() < deadline, "nothing to press for {label}: {}", handle.read_with(cx, |view, _| view.text()).unwrap());
                    wait(cx, window, Duration::from_millis(20));
                }
                until(cx, if step + 1 == presses.len() { expected } else { "Unit tests" });
            }
            wait(cx, window, Duration::from_millis(300));
            save(cx, window, out.join(format!("pr-tracker-{name}-{width}.png")));
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
    };
    for (name, source, args, place, presses, expected) in views {
        shoot(&mut cx, name, source, args, place, presses, expected);
    }
    // What a host being slow, down or signed out looks like, and a project with no pull requests.
    mode("slow");
    shoot(&mut cx, "loading", &tab, Some(&args), Tab, &[], "");
    mode("error");
    shoot(&mut cx, "error", &tab, Some(&args), Tab, &[], "Couldn't load this pull request");
    mode("signed-out");
    shoot(&mut cx, "signed-out", &tab, Some(&args), Tab, &[], "Sign in");
    mode("");
    // Last, since they all read the project's list from main.
    shoot(&mut cx, "list", &block, None, Block, &[], "Migrate the settings page");
    shoot(&mut cx, "picker", &tab, None, Tab, &[], "Migrate the settings page");
    // A project whose folder the fixture's app doesn't hold: its remote can't be read.
    host.publish_project(Some(&empty));
    shoot(&mut cx, "no-remote", &block, None, Block, &[], "No ");
}

/// A page shown as a tab or block: its name in the PNG, the bundle, what it opens with, its saved state's place, the
/// chips to press to reach the view, and a text it shows once there.
type StatePage<'a> = (
    &'a str,
    &'a str,
    Option<&'a serde_json::Value>,
    wings_native::page::plugin::StatePlace,
    &'a [&'a str],
    &'a str,
);

/// Renders each of `pages` of the plugin `host` runs at 1000 and 640 points wide, into `<prefix>-<name>-<width>.png`.
fn shoot_pages(out: &std::path::Path, host: &PluginHost, prefix: &str, pages: &[StatePage]) {
    let mut cx = app();
    for width in [1000.0, 640.0] {
        for &(name, source, args, place, presses, expected) in pages {
            let state = wings_native::page::plugin::FrameState::new(place, None, |_| Ok(()));
            let handle = cx
                .open_offscreen_window(size(px(width), px(900.0)), |_, cx| {
                    cx.new(|cx| PageView::in_plugin_with_state(host, source, args, state, cx))
                })
                .unwrap();
            let window = handle.into();
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
                        "{prefix} {name} did not show {text:?}: {}",
                        handle.read_with(cx, |view, _| view.text()).unwrap()
                    );
                    wait(cx, window, Duration::from_millis(20));
                }
            };
            for label in presses {
                // Each chip shows once the page has loaded what it needs.
                let deadline = Instant::now() + Duration::from_secs(10);
                while !handle
                    .read_with(&cx, |view, _| {
                        view.press_chip(label) || view.turn_on_chip(label)
                    })
                    .unwrap()
                {
                    assert!(
                        Instant::now() < deadline,
                        "nothing to press for {label}: {}",
                        handle.read_with(&cx, |view, _| view.text()).unwrap()
                    );
                    wait(&mut cx, window, Duration::from_millis(20));
                }
            }
            until(&mut cx, expected);
            wait(&mut cx, window, Duration::from_millis(300));
            save(
                &mut cx,
                window,
                out.join(format!("{prefix}-{name}-{width}.png")),
            );
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
    }
}

/// A host for the repo's plugin `id`, as the app runs it, in a project folder of the shots.
fn repo_host(out: &std::path::Path, id: &str, manifest: &str) -> (PluginHost, String) {
    let project = out.join(format!("fixture-project-{id}"));
    std::fs::create_dir_all(&project).unwrap();
    let path = project.to_string_lossy().into_owned();
    let host = PluginHost::with_access(
        PluginConfig {
            id: id.into(),
            root: out.join(format!("fixture-storage-{id}")),
            scope: "shot".into(),
        },
        Access {
            plugin: wings_plugins::manifest::Plugin {
                manifest: serde_json::from_str(manifest).unwrap(),
                dir: project.clone(),
                icon_svg: None,
            },
            builtin: true,
            env: std::sync::Arc::new(FixtureEnv(project)),
        },
    );
    host.publish_projects(vec![wings_native::page::plugin::Project { hidden_sections: Vec::new(),
        id: path.clone(),
        name: "web".into(),
        parent_id: None,
        path: path.clone(),
    }]);
    (host, path)
}

fn fail_on_error(event: wings_native::page::runtime::Event) {
    if let wings_native::page::runtime::Event::Error { message, .. } = event {
        panic!("{message}");
    }
}

/// Work item's built tab and Sprint block. The pages call `az` themselves, so the shots run an `az` of their own, first on
/// PATH, that answers from `shot_plugins/work_item_az.json`, in a project folder that is a git repo with an Azure DevOps
/// origin, which is how a page knows what to ask for.
fn work_item(out: PathBuf) {
    let answers: serde_json::Map<String, serde_json::Value> = serde_json::from_str(include_str!("shot_plugins/work_item_az.json")).unwrap();
    let bin = out.join("fake-az");
    std::fs::create_dir_all(&bin).unwrap();
    let mut script = String::from("#!/bin/sh\ncase \"$*\" in\n");
    for (index, (words, answer)) in answers.iter().filter(|(words, _)| !words.starts_with('_')).enumerate() {
        let file = bin.join(format!("{index}.json"));
        std::fs::write(&file, answer.to_string()).unwrap();
        script += &format!("  *'{words}'*) cat '{}' ;;\n", file.display());
    }
    script += "  *) echo \"no made-up answer for az $*\" >&2; exit 1 ;;\nesac\n";
    let az = bin.join("az");
    std::fs::write(&az, script).unwrap();
    std::process::Command::new("chmod").arg("+x").arg(&az).status().unwrap();
    // Programs are found on the login shell's PATH, then this process's: no login shell leaves this one.
    unsafe {
        std::env::set_var("SHELL", "/nonexistent-shell");
        std::env::set_var("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default()));
    }
    let (host, project) = repo_host(&out, "work-item", include_str!("../../../plugins/work-item/wings-plugin.json"));
    let git = |args: &[&str]| {
        let done = std::process::Command::new("git").args(args).current_dir(&project).output().unwrap();
        assert!(done.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&done.stderr));
    };
    git(&["init", "-q"]);
    let _ = std::process::Command::new("git").args(["remote", "remove", "origin"]).current_dir(&project).output();
    git(&["remote", "add", "origin", "git@ssh.dev.azure.com:v3/acme/Web/web"]);
    // The project on screen, which the app always says and the Sprint block follows.
    host.publish_project(Some(std::path::Path::new(&project)));
    let module = |frame: &str| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../plugins/work-item/dist/native/{frame}.wasm"));
        format!("{}{}", wings_native::page::runtime::wasm::MARKER, path.canonicalize().expect("sh build-rust.sh work-item, in plugins/").display())
    };
    let (item, sprint) = (module("item"), module("sprint"));
    let args = json!({ "id": 1432, "org": "https://dev.azure.com/acme" });
    // The fake `az` has no work item 9999.
    let missing = json!({ "id": 9999, "org": "https://dev.azure.com/acme" });
    use wings_native::page::plugin::StatePlace::{Block, Tab};
    shoot_pages(
        &out,
        &host,
        "work-item",
        &[
            ("no-item", &item, None, Tab, &[], "Open a work item from a pane's badge"),
            ("missing", &item, Some(&missing), Tab, &[], "doesn't exist, or you can't see it"),
            ("overview", &item, Some(&args), Tab, &[], "Pull request is up"),
            ("links", &item, Some(&args), Tab, &["Links"], "Make the sync client reliable"),
            ("history", &item, Some(&args), Tab, &["History"], "Remaining work"),
            ("sprint", &sprint, None, Block, &[], "Add retry with backoff"),
            ("sprint-all", &sprint, None, Block, &["Whole sprint"], "Show a notice while retrying"),
        ],
    );
}

/// The Issues plugin's board, list, issue tab and block with its real main, over the made-up issues
/// `tasks_seed` stores first.
fn tasks(out: PathBuf) {
    let _ = std::fs::remove_dir_all(out.join("fixture-storage-tasks"));
    let (host, project) = repo_host(
        &out,
        "tasks",
        include_str!("../../../plugins/tasks/wings-plugin.json"),
    );
    // The project on screen, which the app always says and the pages follow.
    host.publish_project(Some(std::path::Path::new(&project)));
    let (seeded, done) = std::sync::mpsc::channel();
    let seed = host
        .open_page(
            &tasks_seed(&project),
            move |event| match event {
                wings_native::page::runtime::Event::Tree(tree)
                    if tree.text().contains("Seeded") =>
                {
                    let _ = seeded.send(());
                }
                other => fail_on_error(other),
            },
        )
        .unwrap();
    done.recv_timeout(Duration::from_secs(10))
        .expect("the issues to be stored");
    drop(seed);
    let module = |frame: &str| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../plugins/tasks/dist/native/{frame}.wasm"));
        format!("{}{}", wings_native::page::runtime::wasm::MARKER, path.canonicalize().expect("sh build-rust.sh tasks, in plugins/").display())
    };
    host.start_main(&module("main"), fail_on_error).unwrap();
    let (board, issue, block) = (module("board"), module("issue"), module("block"));
    let (board, issue, block) = (board.as_str(), issue.as_str(), block.as_str());
    let args = json!({ "key": "WEB-4", "projectId": project });
    use wings_native::page::plugin::StatePlace::{Block, Tab};
    shoot_pages(
        &out,
        &host,
        "tasks",
        &[
            (
                "board",
                board,
                None,
                Tab,
                &["Board"],
                "Checkout summary card",
            ),
            ("list", board, None, Tab, &["List"], "Checkout redesign"),
            (
                "issue",
                issue,
                Some(&args),
                Tab,
                &[],
                "Waiting on the release branch",
            ),
            ("block", block, None, Block, &[], "Open board"),
            (
                "issue-missing",
                issue,
                Some(&json!({ "key": "WEB-99", "projectId": project })),
                Tab,
                &[],
                "may have been removed",
            ),
            ("settings", &module("settings"), None, Tab, &[], "Keep issues local"),
        ],
    );
    // A project with no issues: the board's empty columns and the block's empty state.
    let (empty_host, empty_project) = repo_host(&out, "tasks-empty", include_str!("../../../plugins/tasks/wings-plugin.json"));
    empty_host.publish_project(Some(std::path::Path::new(&empty_project)));
    shoot_pages(
        &out,
        &empty_host,
        "tasks-empty",
        &[
            ("board", board, None, Tab, &["Board"], "Nothing waiting"),
            ("block", block, None, Block, &[], "No issues yet"),
        ],
    );
    // On its own, after the others: Group by parent stays on for every Issues tab once it's turned on.
    shoot_pages(
        &out,
        &host,
        "tasks",
        &[(
            "list-parent",
            board,
            None,
            Tab,
            &["List", "Group by parent"],
            "No parent",
        )],
    );
}

/// The slot the plugin keeps a project under (`plugins/tasks/rust/src/storage.rs` `slot_of`).
fn slot_of(project_id: &str) -> String {
    let (mut h1, mut h2) = (0xdead_beef_u32, 0x41c6_ce57_u32);
    for unit in project_id.encode_utf16() {
        let c = u32::from(unit);
        h1 = (h1 ^ c).wrapping_mul(2_654_435_761);
        h2 = (h2 ^ c).wrapping_mul(1_597_334_677);
    }
    h1 = (h1 ^ (h1 >> 16)).wrapping_mul(2_246_822_507) ^ (h2 ^ (h2 >> 13)).wrapping_mul(3_266_489_909);
    h2 = (h2 ^ (h2 >> 16)).wrapping_mul(2_246_822_507) ^ (h1 ^ (h1 >> 13)).wrapping_mul(3_266_489_909);
    format!("{h2:08x}{h1:08x}")
}

/// Made-up issues for the screenshots of Issues: run as a page of the plugin before its main starts, it writes them to
/// storage the way `plugins/tasks/rust/src/storage.rs` keeps them, for the project `project`, then shows "Seeded".
fn tasks_seed(project: &str) -> String {
    struct Seed(String);
    impl Guest for Seed {
        fn start(&mut self, cx: &mut Cx<'_>) {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
            let ago = |hours: u64| now - hours * 3_600_000;
            let agent = |seen: u64| json!({ "id": "6f1c2a90-3b7e-4d51-9a02-5c8e1d4b7a11", "name": "checkout summary", "seen": seen });
            let created = |hours: u64| json!({ "at": ago(hours), "by": "you", "kind": "created", "text": "Created" });
            let issue = |key: &str, title: &str, status: &str, kind: &str, hours: u64, more: serde_json::Value| {
                let mut issue = json!({ "key": key, "title": title, "status": status, "type": kind, "sessions": [], "log": [created(hours)],
                                        "createdAt": ago(hours), "updatedAt": ago(hours / 2) });
                issue.as_object_mut().unwrap().extend(more.as_object().unwrap().clone());
                issue
            };
            let by = json!({ "id": "6f1c2a90-3b7e-4d51-9a02-5c8e1d4b7a11", "name": "checkout summary" });
            let issues = [
                issue("WEB-1", "Show the delivery estimate under the cart total", "todo", "task", 30, json!({ "parent": "WEB-10", "priority": "medium", "description": "## Goal\nShow when the order arrives, under the total.\n\n## Done when\n- [ ] The estimate shows for a saved address\n- [ ] A cart with no address shows the default" })),
                issue("WEB-2", "Fix the focus ring on the promo code field", "todo", "bug", 28, json!({ "priority": "high", "links": { "branch": "fix/promo-focus" } })),
                issue("WEB-3", "Backfill order totals for rows created before the currency column existed", "in_progress", "task", 20, json!({ "parent": "WEB-10", "sessions": [agent(ago(2))] })),
                issue("WEB-4", "Checkout summary card", "in_review", "feature", 52, json!({
                    "parent": "WEB-10",
                    "priority": "urgent",
                    "description": "## Goal\nShow the buyer what they are about to pay for in one place, right above the pay button.\n\n## Scope\n- One card built from `useCart()`, no new data fetching\n- Rounding happens once, on the sum\n\n## Done when\n- [x] The card shows items, shipping, tax and total\n- [ ] An empty cart hides the card",
                    "related": ["WEB-8"],
                    "blockedBy": ["WEB-3"],
                    "links": { "branch": "feature/checkout-summary", "pr": "https://example.com/acme/web/pull/412", "commits": ["3fa91c2", "8b27de0"] },
                    "sessions": [agent(ago(3))],
                    "log": [
                        created(52),
                        { "at": ago(50), "by": by, "kind": "note", "text": "Read the cart code. Plan: one card with items, shipping, tax and a total, no new data fetching." },
                        { "at": ago(26), "by": by, "kind": "status", "text": "In progress → In review" },
                        { "at": ago(5), "by": "you", "kind": "note", "text": "Approved. Waiting on the release branch." },
                    ],
                })),
                issue("WEB-5", "Rename the cart store to match the checkout naming", "done", "task", 60, json!({})),
                issue("WEB-7", "Hide the summary card for an empty cart", "todo", "task", 20, json!({ "parent": "WEB-4", "priority": "low" })),
                issue("WEB-8", "Order history landing page: tabs, design alignment, page and tab events", "todo", "feature", 18, json!({ "parent": "WEB-10" })),
                issue("WEB-6", "Upgrade the date picker", "done", "task", 70, json!({ "parent": "WEB-10" })),
                issue("WEB-10", "Checkout redesign", "in_progress", "epic", 80, json!({ "description": "The checkout pages on the new design system." })),
            ];
            let slot = slot_of(&self.0);
            for issue in &issues {
                cx.save(&format!("issues/{slot}/{}", issue["key"].as_str().unwrap()), issue);
            }
            cx.save(&format!("issues/{slot}"), &json!({ "v": 3, "path": self.0, "prefix": "WEB", "next": 11 }));
            cx.render(&json!({ "id": 0, "type": "#root", "props": {}, "children": [{ "id": 1, "type": "#text", "text": "Seeded" }] }));
        }
    }
    let project = project.to_owned();
    fake::source("page", move || Box::new(Seed(project.clone())))
}
