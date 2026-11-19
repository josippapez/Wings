//! Renders Worktrees' sidebar, which is written in Rust, offscreen at 1000 and 640 points wide, over a made-up repo in
//! the output folder, never the user's. With `--changes` it renders Changes' built
//! pages, with `--schedules` Schedules' list and forms, and with
//! `--hud` the HUD's sidebar, with
//! `--disk` Disk usage's overview and the map of a folder of made-up files it scans, with `--work-item` Work item's tab
//! and Sprint block, and with `--tasks` the Issues board, list, issue and block, each over made-up data. With
//! `--external <folder>` it renders the separately shipped Sprint board, Cyclops and Claude accounts plugins from their
//! repos in that folder, popovers too (`shot_plugins/external.rs`).
//! `--claude-chat` renders Claude chat's page over a fake `claude` that prints made-up sessions.
//! `--tooltips` points at a titled Button and Text of a made-up page until their tooltips show.
//! `--prompt-rail` renders a `PromptRail` of 40 prompts with one pointed at, and one of 300 that has scrolled.
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
    if std::env::args().any(|arg| arg == "--prompt-rail") {
        prompt_rail(out);
        return;
    }
    if std::env::args().any(|arg| arg == "--claude-chat") {
        claude_chat(out);
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
    fn past_sessions(&self, project: &std::path::Path, query: &str, _: usize) -> Vec<serde_json::Value> {
        let hours = |n: u64| now_ms() - n * 3_600_000;
        let query = query.trim().to_lowercase();
        // A search finds a session by its title here, and says some of its messages matched.
        let session = |id: &str, title: &str, cwd: &std::path::Path, branch: &str, ago: u64, messages: u64| {
            let matches = if query.is_empty() { 0 } else { messages / 8 };
            json!({ "id": id, "title": title, "firstPrompt": title, "cwd": cwd, "gitBranch": branch, "lastActiveMs": hours(ago), "messages": messages, "matches": matches })
        };
        vec![
            session("1d3f6a9e-0b2c-4f1d-8e7a-5c9b2d4e6f10", "Fix the flaky checkout test", project, "main", 1, 24),
            session("2e4a7b0f-1c3d-4e2f-9f8b-6d0c3e5f7a21", "Split checkout into its own package", project, "checkout-pkg", 5, 61),
            // One ran under a named account, which the sheet shows beside its title.
            {
                let mut limiter = session("3f5b8c1a-2d4e-4f3a-8a9c-7e1d4f6a8b32", "Why is the rate limiter returning 429s?", &project.join("api"), "main", 26, 12);
                limiter["account"] = json!("work");
                limiter
            },
            session("4a6c9d2b-3e5f-4a4b-9bad-8f2e5a7b9c43", "Add Stripe webhooks for refunds", project, "refunds", 72, 88),
        ]
        .into_iter()
        .filter(|session| session["title"].as_str().unwrap_or_default().to_lowercase().contains(&query))
        .collect()
    }
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

/// A `PromptRail` in a slim split beside some text: 40 prompts with the 13th in view and the 23rd pointed at, so its
/// card shows, and then 300 prompts with the 250th in view, so the rail has scrolled to keep it there.
fn prompt_rail(out: PathBuf) {
    use wings_native::page::tree::prompt_rail as rail;
    let host = PluginHost::new(PluginConfig { id: "prompt-rail".into(), root: out.join("fixture-storage-prompt-rail"), scope: "shot".into() });
    let height = 560.0;
    let mut cx = app();
    for (name, count, current, hover) in [("hover", 40_usize, 12_usize, Some(22_usize)), ("scrolled", 300, 249, None)] {
        let source = fake::page("page", move |_| {
            let marks: Vec<_> = (0..count)
                .map(|n| json!({
                    "title": format!("Prompt {} about the checkout flow", n + 1),
                    "preview": "I read the cart module first. The total is computed twice, once when a coupon applies and once on submit, and the two disagree by the tax. I would fix it in one place."
                }))
                .collect();
            json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [{ "id": 1, "type": "Split", "props": { "defaultSize": 64 }, "children": [
                { "id": 2, "type": "Slot", "props": { "name": "start" }, "children": [
                    { "id": 3, "type": "PromptRail", "props": { "marks": marks, "current": current, "onSelect": "3:onSelect" } }] },
                { "id": 4, "type": "Slot", "props": { "name": "end" }, "children": [
                    { "id": 5, "type": "Text", "children": [{ "id": 6, "type": "#text", "text": "The conversation sits here, to the right of the rail." }] }] }] }] })
        });
        let handle = cx.open_offscreen_window(size(px(640.0), px(height)), |_, cx| cx.new(|cx| PageView::in_plugin(&host, &source, cx))).unwrap();
        let window = handle.into();
        wait(&mut cx, window, Duration::from_millis(800));
        if let Some(at) = hover {
            // The rail fills the window's height, so its ticks are where the layout puts them.
            let pitch = rail::pitch(count, height);
            let y = rail::row_top(at, pitch) + pitch / 2.0;
            cx.simulate_mouse_move(window, point(px(rail::PAD_X + 4.0), px(y)), None, Modifiers::default());
            wait(&mut cx, window, Duration::from_millis(300));
        }
        save(&mut cx, window, out.join(format!("prompt-rail-{name}.png")));
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
        ("edit-schedule", &host, &source, Some(("Edit", 0)), "Prompt"),
        ("edit-once", &host, &source, Some(("Edit", 2)), "Year, month, day"),
        ("edit-entry", &host, &source, Some(("Edit", 4)), "Keep free"),
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
        session: Some(SessionEntry { session_id: session.into(), kind: "claude".into(), name: Some(name.into()), state: "idle".into(), started_at, launcher: None }),
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

/// Claude chat's real page, over a fake `claude` the scene puts first on the PATH, which prints a made-up session and
/// then waits: the real Claude Code never runs. Each scene is a conversation, a reply streaming in, a new chat and one
/// whose Claude Code couldn't start.
fn claude_chat(out: PathBuf) {
    let root = out.join("fixture-claude-chat");
    let _ = std::fs::remove_dir_all(&root);
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let script = |path: PathBuf, body: &str| {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    // `claude plugin ...` is what the plugins dialog's Discover view runs: the list prints a made-up one, an install says it went well.
    // `claude edit-...` is what a change in the /hooks, /memory and /permissions dialogs runs: it reads the edit to the end
    // of its input, and refuses it with the text of `$CHAT_ANSWERS/<subcommand>` when the scene has one, as Claude Code does.
    // A request the chat writes whose subtype names a file in `$CHAT_ANSWERS` gets that file as its answer, as Claude Code
    // answers a request made after it started.
    std::fs::write(root.join("available.json"), include_str!("../../../plugins/claude-chat/rust/tests/available.json")).unwrap();
    script(
        bin.join("claude"),
        r##"if [ "$1" = plugin ]; then case "$2" in list) cat "$CHAT_AVAILABLE";; install) echo '{"command":"install","outcome":"ok","scope":"user"}';; esac; exit 0; fi
case "$1" in edit-*) cat >/dev/null; if [ -f "$CHAT_ANSWERS/$1" ]; then cat "$CHAT_ANSWERS/$1" >&2; exit 1; fi; echo '{"ok":true}'; exit 0;; esac
cat "$CHAT_FIXTURE"; if [ -n "$CHAT_EXIT" ]; then echo "$CHAT_STDERR" >&2; exit "$CHAT_EXIT"; fi
while IFS= read -r line; do for answer in "$CHAT_ANSWERS"/*; do [ -f "$answer" ] && case "$line" in *"\"subtype\":\"${answer##*/}\""*) cat "$answer";; esac; done; done"##,
    );
    // `gh pr view <url> --json ...` is how the side panel reads a pull request the conversation names: made-up ones.
    script(
        bin.join("gh"),
        r##"case "$3" in
*/pull/42) echo '{"title":"Round the discount in cents","state":"OPEN","isDraft":false,"author":{"login":"ana"},"headRefName":"fix/discount","baseRefName":"main","reviewDecision":"REVIEW_REQUIRED","additions":12,"deletions":3,"statusCheckRollup":[{"status":"COMPLETED","conclusion":"SUCCESS"},{"status":"COMPLETED","conclusion":"FAILURE"},{"status":"IN_PROGRESS","conclusion":""}]}';;
*) echo '{"title":"Checkout: one test for the rounding","state":"MERGED","isDraft":false,"author":{"login":"ivo"},"headRefName":"test/rounding","baseRefName":"main","reviewDecision":"APPROVED","additions":40,"deletions":0,"statusCheckRollup":[{"status":"COMPLETED","conclusion":"SUCCESS"}]}';;
esac"##,
    );
    // Wings finds programs on the login shell's PATH, so the scene's shell names the fake's folder.
    script(root.join("shell"), &format!("printf '__WINGS_PATH__{}__WINGS_PATH__'", bin.display()));
    // SAFETY: set before anything in this process reads the environment from another thread.
    unsafe {
        std::env::set_var("SHELL", root.join("shell"));
        std::env::set_var("CHAT_AVAILABLE", root.join("available.json"));
    }

    let init = json!({ "type": "system", "subtype": "init", "session_id": "5b7e2c1a-0f3d-4c8e-9a61-2d4f8b3c7e90", "model": "claude-opus-5-5" });
    let user = |text: &str| json!({ "type": "user", "message": { "role": "user", "content": text } });
    let assistant = |content: serde_json::Value| json!({ "type": "assistant", "message": { "role": "assistant", "content": content } });
    let tool = |id: &str, name: &str, input: serde_json::Value| assistant(json!([{ "type": "tool_use", "id": id, "name": name, "input": input }]));
    let output = |id: &str, text: &str, failed: bool| {
        json!({ "type": "user", "message": { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": id, "content": text, "is_error": failed }] } })
    };
    let text = |text: &str| assistant(json!([{ "type": "text", "text": text }]));
    let delta = |text: &str| json!({ "type": "stream_event", "event": { "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": text } } });
    // What Claude Code answers the chat's opening questions with: its models, commands and mode, and the context.
    let answer = |id: &str, response: serde_json::Value| json!({ "type": "control_response", "response": { "subtype": "success", "request_id": id, "response": response } });
    let efforts = json!(["low", "medium", "high", "xhigh", "max"]);
    let ready = [
        answer("initialize", json!({
            "models": [
                { "value": "default", "displayName": "Default (recommended)", "supportedEffortLevels": efforts },
                { "value": "opus", "displayName": "Opus 5.5", "supportedEffortLevels": efforts, "resolvedModel": "claude-opus-5-5" },
                { "value": "sonnet", "displayName": "Sonnet 5.5", "supportedEffortLevels": efforts },
                { "value": "haiku", "displayName": "Haiku 5.5", "supportedEffortLevels": efforts },
            ],
            "commands": [
                { "name": "review", "description": "Review the changes on this branch", "argumentHint": "" },
                { "name": "compact", "description": "Clear the conversation but keep a summary in context", "argumentHint": "<instructions>" },
                { "name": "release", "description": "Bump the version, write the changelog and publish", "argumentHint": "" },
                { "name": "init", "description": "Write a CLAUDE.md for this project", "argumentHint": "" },
            ],
            "current_permission_mode": "default",
        })),
        answer("context", json!({ "totalTokens": 41_000, "maxTokens": 200_000 })),
        answer("settings", json!({ "effective": { "model": "opus", "effortLevel": "high",
            "modelSettings": { "claude-opus-5-5": { "effortLevel": "xhigh" } } } })),
        answer("usage", json!({ "rate_limits": {
            "five_hour": { "utilization": 42, "resets_at": "2026-11-18T17:00:00+00:00" },
            "seven_day": { "utilization": 86, "resets_at": "2026-11-21T09:00:00+00:00" } } })),
    ];
    let opened = |mut lines: Vec<serde_json::Value>| {
        lines.splice(1..1, ready.iter().cloned());
        lines
    };
    let fix = json!({ "file_path": "/work/shop/src/checkout.ts",
        "old_string": "  return total * (1 - rate);",
        "new_string": "  const cents = Math.round(total * 100);\n  return Math.round(cents * (1 - rate)) / 100;" });
    let conversation = opened(vec![
        init.clone(),
        user("Why does the checkout test fail on CI but pass here?"),
        tool("t1", "Bash", json!({ "command": "pnpm test checkout --reporter=dot", "description": "Run the checkout tests" })),
        output("t1", "FAIL  src/checkout.test.ts > applies the discount\n  Expected: 90\n  Received: 89.99999999999999\n\nTests  1 failed | 23 passed (24)", true),
        tool("t2", "Read", json!({ "file_path": "/work/shop/src/checkout.ts" })),
        output("t2", "", false),
        text("The discount is applied in floating point, so `100 * 0.9` comes out a hair under 90 on CI's Node version. The failing run's log is in `/private/var/folders/77/_cqqbnjn2dj0yzs68hlszmg40000gn/T/ci-logs/checkout-run-4211/report.txt`.\n\n- **Cause:** `total * (1 - rate)` in `applyDiscount`\n- **Fix:** work in cents, and round once at the end\n\n```ts\nconst cents = Math.round(total * 100);\nreturn Math.round(cents * (1 - rate)) / 100;\n```"),
        text("The screenshots are at `/private/tmp/project-renders-0f3c9a52/e78d6fcd-933c-466f-839f-fe4b99583cc2/scratchpad/shots/claude-chat-new-1000.png` and `/private/tmp/project-renders-0f3c9a52/e78d6fcd-933c-466f-839f-fe4b99583cc2/scratchpad/shots/claude-chat-new-640.png`."),
        user("Do it, and run the tests again."),
        tool("t3", "Edit", fix.clone()),
        json!({ "type": "control_request", "request_id": "r1", "request": { "subtype": "can_use_tool", "tool_name": "Edit", "input": fix } }),
    ]);
    // Claude at work on a checklist: an edit, a failed command, a file written and a read, with more to do.
    let work = opened(vec![
        init.clone(),
        user("Move the discount maths into its own module and test it."),
        tool("w0", "TodoWrite", json!({ "todos": [
            { "content": "Find every place that applies a discount", "status": "completed", "activeForm": "Finding the discounts" },
            { "content": "Move the maths into src/discount.ts", "status": "completed", "activeForm": "Moving the maths" },
            { "content": "Write tests for the rounding", "status": "in_progress", "activeForm": "Writing tests for the rounding" },
            { "content": "Run the whole suite", "status": "pending", "activeForm": "Running the suite" },
        ] })),
        output("w0", "Todos have been modified successfully.", false),
        tool("w1", "Read", json!({ "file_path": "/work/shop/src/checkout.ts", "offset": 40, "limit": 30 })),
        output("w1", "", false),
        tool("w2", "Edit", json!({ "file_path": "/work/shop/src/checkout.ts",
            "old_string": "export function applyDiscount(total: number, rate: number) {\n  return total * (1 - rate);\n}",
            "new_string": "export { applyDiscount } from './discount';" })),
        output("w2", "The file has been updated.", false),
        tool("w3", "Write", json!({ "file_path": "/work/shop/src/discount.ts",
            "content": "export function applyDiscount(total: number, rate: number) {\n  const cents = Math.round(total * 100);\n  return Math.round(cents * (1 - rate)) / 100;\n}\n" })),
        output("w3", "File created successfully.", false),
        tool("w4", "Bash", json!({ "command": "pnpm test discount" })),
        output("w4", "Exit code 1\nFAIL  src/discount.test.ts\n  No test suite found", true),
        tool("w5", "Grep", json!({ "pattern": "applyDiscount" })),
        output("w5", "Found 3 files\nsrc/checkout.ts\nsrc/discount.ts\nsrc/cart.ts", false),
        delta("The suite has no discount tests yet, so I'm writing them now."),
    ]);
    // The same turn finished: Claude's answer, then the result that ends the turn and says how long it took.
    let mut worked = work.clone();
    worked.pop();
    worked.extend([
        text("The discount maths is in `src/discount.ts` now, and `checkout.ts` re-exports it. The suite has no discount tests yet: shall I write them?"),
        json!({ "type": "result", "subtype": "success", "is_error": false, "duration_ms": 83_400, "num_turns": 7, "result": "", "total_cost_usd": 0.41 }),
    ]);
    let asked = |name: &str, input: serde_json::Value, extra: serde_json::Value| {
        let mut request = json!({ "subtype": "can_use_tool", "tool_name": name, "input": input });
        request.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        opened(vec![init.clone(), user("Clean up the old build output and rebuild."), json!({ "type": "control_request", "request_id": "r1", "request": request })])
    };
    let ask_bash = asked(
        "Bash",
        json!({ "command": "rm -rf dist .turbo\npnpm install --frozen-lockfile\npnpm build", "description": "Clear the build output and rebuild" }),
        json!({ "decision_reason": "rm -rf isn't in your allowed commands",
            "permission_suggestions": [{ "type": "addRules", "rules": [{ "toolName": "Bash", "ruleContent": "pnpm build" }], "behavior": "allow", "destination": "localSettings" }] }),
    );
    let question = asked(
        "AskUserQuestion",
        json!({ "questions": [
            { "question": "Which date library should the reports use?", "header": "Library", "multiSelect": false, "options": [
                { "label": "date-fns", "description": "Functions, tree-shakes well" }, { "label": "dayjs", "description": "Small, Moment-like API" }] },
            { "question": "Which formats do the reports need?", "header": "Formats", "multiSelect": true, "options": [
                { "label": "ISO", "description": "2026-11-18" }, { "label": "Long", "description": "18 November 2026" }, { "label": "Relative", "description": "3 days ago" }] },
        ] }),
        json!({}),
    );
    let plan = asked(
        "ExitPlanMode",
        json!({ "plan": "## Split the checkout into a package\n\n1. Move `src/checkout` to `packages/checkout`\n2. Give it its own `package.json` and tests\n3. Point the app at `@shop/checkout`\n\n- **Risk:** the cart imports private helpers; they move along" }),
        json!({}),
    );
    // Two subagents the session started: one still at work, one done, each with the lines it printed, which carry the id of
    // the call that started it and the subagent's own id, which its tab is opened by.
    let within = |parent: &str, mut line: serde_json::Value| {
        line["parent_tool_use_id"] = parent.into();
        line["agent_id"] = format!("a{parent}").into();
        line
    };
    let haiku = |content: serde_json::Value| json!({ "type": "assistant", "message": { "role": "assistant", "model": "claude-haiku-5-5", "content": content } });
    let subagent = opened(vec![
        init.clone(),
        user("Find every caller of applyDiscount and tell me which of them round."),
        tool("s1", "Task", json!({ "description": "Survey the callers of applyDiscount", "subagent_type": "Explore", "prompt": "List every caller of applyDiscount." })),
        tool("s2", "Task", json!({ "description": "Check the rounding in the cart", "subagent_type": "Explore", "prompt": "Does the cart round?" })),
        within("s1", haiku(json!([{ "type": "tool_use", "id": "g1", "name": "Grep", "input": { "pattern": "applyDiscount" } }]))),
        within("s1", output("g1", "Found 3 files\nsrc/checkout.ts\nsrc/discount.ts\nsrc/cart.ts", false)),
        within("s1", haiku(json!([{ "type": "text", "text": "Found three callers:\n\n- `src/checkout.ts` re-exports it\n- `src/cart.ts` calls it once per line item\n- `src/discount.ts` defines it" }]))),
        within("s1", haiku(json!([{ "type": "tool_use", "id": "g2", "name": "Read", "input": { "file_path": "/work/shop/src/cart.ts" } }]))),
        within("s2", haiku(json!([{ "type": "text", "text": "The cart rounds once, at the end." }]))),
        output("s2", "The cart rounds once, at the end.", false),
    ]);
    // Three subagents of a turn, two still at work and one done, while Claude waits on them alone. Claude Code says the
    // turn runs, as it does when asked to.
    let subagents = opened(vec![
        init.clone(),
        json!({ "type": "system", "subtype": "session_state_changed", "state": "running" }),
        user("Survey the discount code: who calls it, how the cart rounds, and which tests cover it."),
        tool("p1", "Task", json!({ "description": "Survey the callers of applyDiscount", "subagent_type": "Explore", "prompt": "List every caller." })),
        tool("p2", "Task", json!({ "description": "Check the rounding in the cart", "subagent_type": "Explore", "prompt": "Does the cart round?" })),
        tool("p3", "Task", json!({ "description": "List the discount tests", "subagent_type": "Explore", "prompt": "Which tests cover discounts?" })),
        within("p1", haiku(json!([{ "type": "tool_use", "id": "q1", "name": "Grep", "input": { "pattern": "applyDiscount" } }]))),
        within("p2", haiku(json!([{ "type": "tool_use", "id": "q2", "name": "Read", "input": { "file_path": "/work/shop/src/cart.ts" } }]))),
        within("p3", haiku(json!([{ "type": "text", "text": "Two tests cover discounts, both in checkout.test.ts." }]))),
        output("p3", "Two tests cover discounts, both in checkout.test.ts.", false),
    ]);
    let streaming = opened(vec![
        init.clone(),
        user("Explain what the rate limiter in api/limits.ts does."),
        tool("t1", "Read", json!({ "file_path": "/work/shop/api/limits.ts" })),
        output("t1", "export const RATE = 5;", false),
        tool("t2", "Grep", json!({ "pattern": "takeToken" })),
        output("t2", "api/limits.ts:12", false),
        delta("It keeps a **token bucket** per account: each request takes a token, and the bucket refills at "),
        delta("`RATE` tokens a second up to `BURST`.\n\n- A request with no token left gets a `429`"),
    ]);
    let thinking = opened(vec![
        init.clone(),
        user("Plan how to split the checkout into its own package."),
        json!({ "type": "stream_event", "event": { "type": "content_block_start", "index": 0, "content_block": { "type": "thinking", "thinking": "" } } }),
    ]);
    // A long session the chat goes on with, as a terminal's would be: far more than a page may draw.
    const LONG: &str = "6b8d0e3c-4f6a-4b5c-9cbe-0a3f6b8c0d54";
    std::fs::create_dir_all(root.join("projects/-work-shop")).unwrap();
    let code = (0..120).map(|n| format!("let step_{n} = compute({n});")).collect::<Vec<_>>().join("\n");
    let log = (0..200).map(|n| format!("test checkout::case_{n} ... ok")).collect::<Vec<_>>().join("\n");
    let history: Vec<serde_json::Value> = (0..100)
        .flat_map(|n| {
            let id = format!("long-{n}");
            [
                user(&format!("Step {n}: run the checkout suite and tidy what fails.")),
                tool(&id, "Bash", json!({ "command": format!("pnpm test checkout --run {n}") })),
                output(&id, &log, false),
                text(&format!("Step {n} passes. The loop now reads:\n\n```rs\n{code}\n```\n\n- **Kept:** the order of the steps\n- **Changed:** each step runs once")),
            ]
        })
        .collect();
    std::fs::write(root.join(format!("projects/-work-shop/{LONG}.jsonl")), history.iter().map(|line| format!("{line}\n")).collect::<String>()).unwrap();
    // Each: its name, what the fake prints, its exit code and stderr when it fails, what to do once the page is up (press a
    // button or type), and the text to wait for.
    type Scene<'a> = (&'a str, Vec<serde_json::Value>, Option<(&'a str, &'a str)>, Option<&'a str>, &'a str);
    // What `/usage` asks for in full, which the fake prints with the rest: the view fills from it once it opens.
    let full_usage = answer("usage-view", json!({
        "session": { "total_cost_usd": 1.2345, "total_api_duration_ms": 84200, "total_duration_ms": 612000, "total_lines_added": 120, "total_lines_removed": 31,
            "model_usage": { "claude-opus-5-5": { "inputTokens": 1200, "outputTokens": 9800, "cacheReadInputTokens": 410000, "cacheCreationInputTokens": 52000 } } },
        "subscription_type": "max",
        "rate_limits": {
            "five_hour": { "utilization": 42, "resets_at": "2026-11-18T17:00:00+00:00" },
            "seven_day": { "utilization": 13, "resets_at": "2026-11-22T15:59:00+00:00" },
            "model_scoped": [{ "display_name": "Fable", "utilization": 0, "resets_at": "2026-11-22T15:59:00+00:00" }],
            "extra_usage": { "is_enabled": true, "monthly_limit": 20000, "used_credits": 1174, "utilization": 6, "currency": "EUR", "decimal_places": 2 } },
        "behaviors": {
            "day": { "recommendations": [{ "key": "long_context", "pct": 70, "title": "See what fills your context", "detail": "run /context" }],
                "agents": [], "skills": [{ "name": "release-notes", "pct": 4 }], "plugins": [{ "name": "toolbox", "pct": 11 }], "mcp_servers": [{ "name": "files", "pct": 6 }] },
            "week": { "recommendations": [], "agents": [], "skills": [], "plugins": [], "mcp_servers": [] } } }));
    // What `/context` asks for in full: the categories the window is spent on.
    let full_context = answer("context-view", json!({
        "model": "claude-opus-5-5", "totalTokens": 41_000, "maxTokens": 200_000,
        "categories": [
            { "name": "System prompt", "tokens": 6000, "color": "x", "kind": "used" },
            { "name": "System tools", "tokens": 11_000, "color": "x", "kind": "used" },
            { "name": "Messages", "tokens": 24_000, "color": "x", "kind": "used" },
            { "name": "Free space", "tokens": 120_000, "color": "x", "kind": "free" },
            { "name": "Autocompact buffer", "tokens": 39_000, "color": "x", "kind": "buffer" } ] }));
    // Claude Code's guess at your next message, printed after the turn's result: grey in the empty box, with a Tab chip.
    let suggested = opened(vec![
        init.clone(),
        user("Add a function that sums two numbers to add.py, and don't run it."),
        text("`add.py` now has `add(a, b)`, which returns `a + b`. I did not run it."),
        json!({ "type": "result", "subtype": "success", "is_error": false, "total_cost_usd": 0.0068 }),
        json!({ "type": "prompt_suggestion", "suggestion": "run it", "uuid": "cdab3e06-e878-423a-add5-9ed9174c610e", "session_id": "5b7e2c1a-0f3d-4c8e-9a61-2d4f8b3c7e90" }),
    ]);
    // What `/plugins` and `/mcp` read (made-up plugins and servers, in Claude Code's shapes), and the plugins' skills among
    // the commands, which the dialog counts.
    let mut skilled = ready[0].clone();
    skilled["response"]["response"]["commands"]
        .as_array_mut()
        .unwrap()
        .extend(["toolbox:lint", "toolbox:format", "release-notes:write"].map(|name| json!({ "name": name, "description": "", "argumentHint": "" })));
    let installed: Vec<serde_json::Value> = include_str!("../../../plugins/claude-chat/rust/tests/plugins.jsonl").lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    let plugins = opened([init.clone(), skilled].into_iter().chain(installed).collect());
    // Two commands Claude started in the background, still running, as Claude Code lists them. The fake has already
    // answered the read of the second one's output, which a click on its row in the side panel shows.
    let server = json!({ "task_id": "bk3f9a2c1", "run_id": "run-1", "task_type": "local_bash", "description": "Run the dev server" });
    let watcher = json!({ "task_id": "bq7d2e8a4", "run_id": "run-2", "task_type": "local_bash", "description": "Watch the test suite" });
    let tasks = opened(vec![
        init.clone(),
        user("Start the dev server and the test watcher in the background, then check the cart page."),
        tool("b1", "Bash", json!({ "command": "pnpm dev", "description": "Run the dev server", "run_in_background": true })),
        json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": [server.clone()] }),
        output("b1", "Command running in background with ID: bk3f9a2c1.", false),
        tool("b2", "Bash", json!({ "command": "pnpm test --watch", "description": "Watch the test suite", "run_in_background": true })),
        json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": [server, watcher] }),
        output("b2", "Command running in background with ID: bq7d2e8a4.", false),
        text("Both run in the background: the dev server on port 5173, and the test watcher, which passes so far. The cart page loads and applies the discount."),
        json!({ "type": "result", "subtype": "success", "is_error": false, "duration_ms": 41_200, "num_turns": 4, "result": "", "total_cost_usd": 0.12 }),
        answer("task-output-bq7d2e8a4", json!({ "output": " RERUN  src/cart.test.ts\n\n ✓ src/cart.test.ts (12 tests) 38ms\n ✓ src/checkout.test.ts (9 tests) 22ms\n ✓ src/discount.test.ts (6 tests) 9ms\n\n Test Files  3 passed (3)\n      Tests  27 passed (27)\n   Duration  412ms\n\n Waiting for file changes...\n", "total_bytes": 248, "truncated": false })),
    ]);
    // Your messages as a transcript has them, each with the uuid Claude Code knows it by, which a rewind names. The fake
    // answers the dry run the Rewind button asks for.
    const FIRST: &str = "3e1b7c52-8a4d-4f6e-9b21-5c7d8e9f0a13";
    let said = |text: &str, uuid: &str| json!({ "type": "user", "message": { "role": "user", "content": text }, "uuid": uuid });
    let rewound = opened(vec![
        init.clone(),
        said("Why does the checkout test fail on CI but pass here?", FIRST),
        tool("t1", "Bash", json!({ "command": "pnpm test checkout --reporter=dot" })),
        output("t1", "FAIL  src/checkout.test.ts > applies the discount\n  Expected: 90\n  Received: 89.99999999999999", true),
        text("The discount is applied in floating point, so `100 * 0.9` comes out a hair under 90 on CI's Node version."),
        said("Do it, and run the tests again.", "4f2c8d63-9b5e-4a7f-8c32-6d8e9f0a1b24"),
        tool("t3", "Edit", fix.clone()),
        output("t3", "The file has been updated.", false),
        tool("t4", "Bash", json!({ "command": "pnpm test checkout" })),
        output("t4", "Tests  24 passed (24)", false),
        text("Fixed: the discount works in cents now, and all 24 checkout tests pass."),
        json!({ "type": "result", "subtype": "success", "is_error": false, "duration_ms": 52_800, "num_turns": 5, "result": "", "total_cost_usd": 0.2 }),
    ]);
    // A turn that edited one file and wrote another, with the results Claude Code prints (`tool_use_result`). The paths are
    // `/work/shop`, which the loop below swaps for the scene's folder so they show as the project's own.
    let edit_done = |id: &str, result: serde_json::Value| {
        json!({ "type": "user", "message": { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": id, "content": "done", "is_error": false }] }, "tool_use_result": result })
    };
    let edited = opened(vec![
        init.clone(),
        said("Add a discount to the cart, in its own file.", "6a2d9e41-3b7c-4f85-a1d2-9e0c4b6f8a37"),
        tool("e1", "Edit", json!({ "file_path": "/work/shop/src/cart.ts", "old_string": "return total;", "new_string": "return applyDiscount(total);" })),
        edit_done("e1", json!({ "filePath": "/work/shop/src/cart.ts", "oldString": "return total;", "newString": "return applyDiscount(total);", "originalFile": "export function cartTotal(items) {\n  const total = sum(items);\n  return total;\n}\n",
            "structuredPatch": [{ "oldStart": 1, "oldLines": 4, "newStart": 1, "newLines": 4, "lines": [" export function cartTotal(items) {", "   const total = sum(items);", "-  return total;", "+  return applyDiscount(total);", " }"] }], "userModified": false, "replaceAll": false })),
        tool("e2", "Write", json!({ "file_path": "/work/shop/src/discount.ts", "content": "" })),
        edit_done("e2", json!({ "type": "create", "filePath": "/work/shop/src/discount.ts", "content": "export function applyDiscount(total) {\n  return total * 0.9;\n}", "structuredPatch": [], "originalFile": null, "userModified": false })),
        text("I added `applyDiscount` in `src/discount.ts` and called it from `src/cart.ts:3`."),
        json!({ "type": "result", "subtype": "success", "is_error": false, "duration_ms": 14_000, "num_turns": 3, "result": "", "total_cost_usd": 0.05 }),
    ]);
    // Made-up pictures drawn in code: a bar chart you attach, and a window a Read of an image gives back.
    let png = |width: u32, height: u32, paint: &dyn Fn(u32, u32) -> [u8; 4]| {
        use base64::Engine;
        let mut bytes = Vec::new();
        image::RgbaImage::from_fn(width, height, |x, y| image::Rgba(paint(x, y))).write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).unwrap();
        base64::engine::general_purpose::STANDARD.encode(bytes)
    };
    let chart = png(480, 300, &|x, y| {
        let bar = [(60..140, 120, [217, 119, 87, 255]), (190..270, 200, [156, 192, 255, 255]), (320..400, 80, [125, 211, 167, 255])]
            .into_iter()
            .find(|(across, high, _)| across.contains(&x) && y >= 270 - high && y < 270);
        match bar {
            Some((.., color)) => color,
            None if (270..272).contains(&y) => [157, 151, 143, 255],
            None => [24, 22, 30, 255],
        }
    });
    let screen = png(800, 500, &|x, y| match (x, y) {
        (_, 0..36) if (16..64).contains(&x) && y > 12 && y < 24 && x % 16 < 10 => [217, 119, 87, 255],
        (_, 0..36) => [46, 42, 56, 255],
        (40..160, 400..440) => [217, 119, 87, 255],
        (40..720, 70..500) if (y - 70) % 28 < 10 && x < 40 + 640 - (y * 37 % 300) => [90, 86, 100, 255],
        _ => [30, 28, 38, 255],
    });
    // You attach a chart, Claude reads a screenshot, and both come back as thumbnails.
    let pictures = opened(vec![
        init.clone(),
        json!({ "type": "user", "message": { "role": "user", "content": [
            { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": chart } },
            { "type": "text", "text": "Why does the third bar look so small next to the others?" }] } }),
        tool("v1", "Read", json!({ "file_path": "/work/shop/shots/checkout.png" })),
        json!({ "type": "user", "message": { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": "v1",
            "content": [{ "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": screen } }] }] } }),
        text("The third bar is a fraction of the first because the cart reloads and its counter starts again, as the checkout screen shows."),
    ]);
    // Three messages sent, so Up has prompts to recall, and a model with fast mode, whose menu also lists the output styles.
    let recalled = opened(vec![
        init.clone(),
        user("Run the checkout tests."),
        text("All 24 pass."),
        user("Now bump the version."),
        text("Bumped to 0.2.40."),
        user("Write the changelog line."),
        text("Added."),
        json!({ "type": "result", "subtype": "success", "is_error": false, "duration_ms": 9_000, "num_turns": 3, "result": "", "total_cost_usd": 0.05 }),
    ]);
    let mut fast = opened(vec![init.clone()]);
    fast[1]["response"]["response"]["models"] = json!([
        { "value": "opus", "displayName": "Opus 5.5", "supportedEffortLevels": ["medium", "high"], "resolvedModel": "claude-opus-5-5", "supportsFastMode": true },
        { "value": "haiku", "displayName": "Haiku 5.5", "supportedEffortLevels": [] },
    ]);
    fast[1]["response"]["response"]["fast_mode_state"] = json!("off");
    fast[1]["response"]["response"]["fast_mode_disabled_reason"] = json!("sdk_opt_in_required");
    fast[1]["response"]["response"]["output_style"] = json!("default");
    fast[1]["response"]["response"]["available_output_styles"] = json!(["default", "Concise", "Explanatory", "Learning"]);
    // Claude Code's lines about the session itself: a compaction, then a retry under way, and the conversation cleared. The fake
    // prints a turn at once, so a thought shows no duration here (its unit test has one).
    let compacting = opened(vec![
        init.clone(),
        json!({ "type": "system", "subtype": "session_state_changed", "state": "running" }),
        user("Refactor the discount module, then update every caller."),
        text("The module is split in two. I'm updating the callers now."),
        json!({ "type": "system", "subtype": "compact_boundary", "compact_metadata": { "trigger": "manual", "pre_tokens": 120_000, "post_tokens": 18_000, "duration_ms": 3230 } }),
        json!({ "type": "system", "subtype": "api_retry", "attempt": 2, "max_retries": 10, "retry_delay_ms": 5000, "error_status": 529 }),
    ]);
    let cleared = opened(vec![
        init.clone(),
        user("Why does the checkout test fail on CI but pass here?"),
        text("The discount is applied in floating point."),
        json!({ "type": "conversation_reset", "new_conversation_id": "7c1d9e4a-2b6f-4a38-8d50-3e9f1a7b6c24", "trigger": "clear" }),
        json!({ "type": "system", "subtype": "init", "session_id": "7c1d9e4a-2b6f-4a38-8d50-3e9f1a7b6c24", "model": "claude-opus-5-5" }),
    ]);
    let thought = opened(vec![
        init.clone(),
        user("A bat and a ball cost 1.10 in total, and the bat costs 1 more than the ball. What does the ball cost?"),
        assistant(json!([{ "type": "thinking", "thinking": "If the ball cost 0.10 the bat would be 1.10 and the total 1.20, so the ball costs 0.05 and the bat 1.05.", "signature": "sig" }])),
        text("The ball costs **5 cents**, and the bat $1.05."),
        json!({ "type": "result", "subtype": "success", "is_error": false, "duration_ms": 4_200, "num_turns": 1, "result": "", "total_cost_usd": 0.01 }),
    ]);
    let copyable = opened(vec![
        init.clone(),
        user("Show me the rounding fix."),
        text("Work in cents and round once at the end:\n\n```ts\nconst cents = Math.round(total * 100);\nreturn Math.round(cents * (1 - rate)) / 100;\n```\n\nThat keeps `100 * 0.9` at exactly 90."),
    ]);
    // A long block in a reply shows its first 20 lines, coloured as Rust, and a row opens the other 30.
    let limiter = "use std::time::{Duration, Instant};\n\n/// A token bucket that refills at a steady rate.\npub struct Bucket {\n    capacity: u32,\n    tokens: f64,\n    rate: f64,\n    last: Instant,\n}\n\nimpl Bucket {\n    pub fn new(capacity: u32, per_second: f64) -> Self {\n        Self { capacity, tokens: f64::from(capacity), rate: per_second, last: Instant::now() }\n    }\n\n    fn refill(&mut self, now: Instant) {\n        let gained = now.duration_since(self.last).as_secs_f64() * self.rate;\n        self.tokens = (self.tokens + gained).min(f64::from(self.capacity));\n        self.last = now;\n    }\n\n    /// Takes one token, or says how long to wait for it.\n    pub fn take(&mut self) -> Result<(), Duration> {\n        self.refill(Instant::now());\n        if self.tokens >= 1.0 {\n            self.tokens -= 1.0;\n            Ok(())\n        } else {\n            let missing = 1.0 - self.tokens;\n            Err(Duration::from_secs_f64(missing / self.rate))\n        }\n    }\n}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n    #[test]\n    fn a_full_bucket_gives_its_capacity_then_asks_to_wait() {\n        let mut bucket = Bucket::new(3, 1.0);\n        for _ in 0..3 {\n            assert!(bucket.take().is_ok());\n        }\n        assert!(bucket.take().is_err());\n    }\n\n    #[test]\n    fn the_wait_is_what_one_token_takes_to_refill() {\n        let mut bucket = Bucket::new(1, 2.0);\n        bucket.take().unwrap();\n        let wait = bucket.take().unwrap_err();\n        assert!(wait <= Duration::from_millis(500));\n    }\n}";
    let long_block = opened(vec![
        init.clone(),
        user("Sketch the rate limiter for the API."),
        text(&format!("A token bucket does it. Here is the whole module with its tests:\n\n```rust\n{limiter}\n```\n\nEach request takes a token, and one that finds none is told how long to wait.")),
    ]);
    // A Read in Detailed shows the file it gave back, coloured as TypeScript, folded after 20 lines.
    let numbered = (1..=26)
        .zip([
            "import { applyDiscount } from './discount';", "import type { Cart } from './cart';", "", "export interface Receipt {", "  total: number;", "  discount: number;", "  lines: string[];", "}", "",
            "const TAX = 0.2; // VAT on every line", "", "export function checkout(cart: Cart, rate: number): Receipt {", "  const subtotal = cart.items.reduce((sum, item) => sum + item.price * item.qty, 0);",
            "  const total = applyDiscount(subtotal, rate);", "  const lines = cart.items.map((item) => `${item.qty} x ${item.name}`);", "  if (total < 0) {", "    throw new Error('negative total');", "  }",
            "  return { total: total * (1 + TAX), discount: subtotal - total, lines };", "}", "", "export async function pay(receipt: Receipt): Promise<boolean> {", "  const response = await fetch('/api/pay', { method: 'POST', body: JSON.stringify(receipt) });",
            "  return response.ok;", "}", "",
        ])
        .map(|(n, line)| format!("{n:>6}→{line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let read_ts = opened(vec![
        init.clone(),
        user("What does checkout do with the discount?"),
        tool("rt1", "Read", json!({ "file_path": "/work/shop/src/checkout.ts" })),
        output("rt1", &numbered, false),
        text("`checkout` takes the discount off the subtotal first, then adds the VAT on what is left."),
    ]);
    // Claude ran a build with Wings' open_pane: the terminal shows live in the side panel, which floats over the tab's right side
    // when the tab is wide and opens from the header's button when it is narrow.
    let built = opened(vec![
        init.clone(),
        user("Build the shop and tell me when it's done."),
        tool("o1", "mcp__wings__open_pane", json!({ "command": "make all", "run": true })),
        output("o1", "{\"paneId\":\"1\",\"commandTyped\":true,\"commandRun\":true}", false),
        text("The build runs in the terminal beside the chat. I'll tell you when it finishes."),
    ]);
    // Words that name something in Wings: two keys of issues there are, a dev server's address, and near misses.
    let linked = opened(vec![
        init.clone(),
        user("What's left on the checkout work?"),
        text("ABC-12 is done: the discount rounds in cents now. ABC-14 still needs its tests (`ABC-14` on the board), and the dev server runs at http://localhost:5173 if you want to try the cart.\n\n- UTF-8 handling is unchanged\n- ABC-99 was never filed"),
    ]);
    // /add-dir checks the folder with `test -d`, so the scene adds one that is there.
    let add_dir = format!("type:/add-dir {}/api;keys:enter", root.canonicalize().unwrap().display());
    // /mcp with a server that wants a sign-in in a browser, next to the one that failed and the one that is connected.
    let mut signed_out: Vec<serde_json::Value> = vec![init.clone()];
    signed_out.extend(include_str!("../../../plugins/claude-chat/rust/tests/plugins.jsonl").lines().map(|line| {
        let mut line: serde_json::Value = serde_json::from_str(line).unwrap();
        if line["response"]["request_id"] == "plugins-servers" {
            line["response"]["response"]["mcpServers"].as_array_mut().unwrap().push(json!({ "name": "issues", "status": "needs-auth", "scope": "user", "transport": "http" }));
        }
        line
    }));
    let signed_out = opened(signed_out);
    // A server asks for details in the middle of a turn: a form with a choice, a number, a text and a box.
    let elicited = opened(vec![
        init.clone(),
        user("Order me an ice cream from the shop."),
        json!({ "type": "system", "subtype": "session_state_changed", "state": "running" }),
        tool("o1", "mcp__shop__order", json!({ "item": "ice cream" })),
        json!({ "type": "control_request", "request_id": "e1", "request": { "subtype": "elicitation", "mcp_server_name": "shop", "message": "Pick a flavour and how many scoops.", "mode": "form",
            "requested_schema": { "type": "object", "required": ["flavour"], "properties": {
                "flavour": { "type": "string", "title": "Flavour", "enum": ["vanilla", "mint", "chocolate"] },
                "scoops": { "type": "integer", "title": "Scoops", "default": 2 },
                "note": { "type": "string", "title": "Note", "description": "Anything the shop should know." },
                "cone": { "type": "boolean", "title": "In a cone" } },
            }, "property_order": ["flavour", "scoops", "note", "cone"] } }),
    ]);
    // A command Claude waits on, which a button moves to the background.
    let running = opened(vec![
        init.clone(),
        json!({ "type": "system", "subtype": "session_state_changed", "state": "running" }),
        user("Run the whole test suite."),
        tool("rb1", "Bash", json!({ "command": "pnpm test --run", "description": "Run the whole test suite" })),
    ]);
    // A background agent that runs, which a Message button reaches through Claude.
    let agent = opened(vec![
        init.clone(),
        user("Have an agent audit the discount code in the background."),
        tool("a1", "Agent", json!({ "description": "Audit the discount code", "subagent_type": "general-purpose", "run_in_background": true, "prompt": "Audit the discount code." })),
        json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": [{ "task_id": "a4443e71dac390806", "run_id": "run-3", "task_type": "local_agent", "subagent_type": "general-purpose", "description": "Audit the discount code" }] }),
        output("a1", "Async agent launched successfully. The agent is working in the background.", false),
        text("The agent audits the discount code in the background."),
        json!({ "type": "result", "subtype": "success", "is_error": false, "duration_ms": 9_400, "num_turns": 2, "result": "", "total_cost_usd": 0.05 }),
    ]);
    // A background agent and a command Claude started, then both ended: each leaves a line where it happened, the agent's report
    // folded under its line, and the side panel lists them as done. The turn their notices start has no message of yours before it.
    let (helper_id, suite_id) = ("a4443e71dac390806", "bc91d0e5a");
    let helper = json!({ "task_id": helper_id, "run_id": "run-3", "task_type": "local_agent", "subagent_type": "general-purpose", "description": "Audit the discount code" });
    let suite = json!({ "task_id": suite_id, "run_id": "run-4", "task_type": "local_bash", "description": "Run the tests" });
    let started = |task: &serde_json::Value, call: &str| {
        json!({ "type": "system", "subtype": "task_started", "task_id": task["task_id"], "run_id": task["run_id"], "tool_use_id": call, "description": task["description"], "is_backgrounded": true, "task_type": task["task_type"] })
    };
    let live = |tasks: Vec<serde_json::Value>| json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": tasks });
    let ended = |task: &serde_json::Value, call: &str, status: &str, summary: &str, report: bool| {
        [
            json!({ "type": "system", "subtype": "task_updated", "task_id": task["task_id"], "run_id": task["run_id"], "patch": { "status": status, "end_time": 1_795_027_323_607u64 } }),
            json!({ "type": "system", "subtype": "task_notification", "task_id": task["task_id"], "run_id": task["run_id"], "tool_use_id": call, "status": status,
                "output_file": "/tmp/claude/tasks/out.output", "summary": summary, "summary_is_report": report }),
        ]
    };
    let finished = opened(
        [
            vec![
                init.clone(),
                user("Have an agent audit the discount code and run the tests, both in the background."),
                tool("a1", "Agent", json!({ "description": "Audit the discount code", "subagent_type": "general-purpose", "run_in_background": true, "prompt": "Audit the discount code." })),
                live(vec![helper.clone()]),
                started(&helper, "a1"),
                output("a1", "Async agent launched successfully. The agent is working in the background.", false),
                tool("b1", "Bash", json!({ "command": "pnpm test", "description": "Run the tests", "run_in_background": true })),
                live(vec![helper.clone(), suite.clone()]),
                started(&suite, "b1"),
                output("b1", "Command running in background with ID: bc91d0e5a.", false),
                within("a1", haiku(json!([{ "type": "text", "text": "Two callers round, and the cart does not." }]))),
                text("Both run in the background."),
                json!({ "type": "result", "subtype": "success", "is_error": false, "duration_ms": 6_000, "num_turns": 3, "result": "", "total_cost_usd": 0.04 }),
            ],
            ended(&helper, "a1", "completed", "The discount rounds in cents in two places, and the cart still rounds per line item.", true).to_vec(),
            vec![live(vec![suite.clone()])],
            ended(&suite, "b1", "failed", "Background command \"Run the tests\" failed (exit code 1)", false).to_vec(),
            vec![
                live(vec![]),
                json!({ "type": "system", "subtype": "status", "status": "requesting" }),
                text("The agent found one place that still rounds per line item, and the tests fail on it."),
                json!({ "type": "result", "subtype": "success", "is_error": false, "duration_ms": 3_000, "num_turns": 1, "result": "", "total_cost_usd": 0.02, "origin": { "kind": "task-notification" } }),
            ],
        ]
        .concat(),
    );
    let finished_panel = finished.clone();
    let (worked_again, worked_forking, rewound_again) = (worked.clone(), worked.clone(), rewound.clone());
    let edited_undo = edited.clone();
    let tasks_open = tasks.clone();
    // A long message of yours that wraps, with Rewind and Fork from here under it.
    let wrapped = opened(vec![
        init.clone(),
        said("Why does the checkout test fail on CI but pass here, and could you also look at whether the cart rounds the same way, and tell me which of the two places I should change first before the release goes out on Friday?", FIRST),
        text("The discount is applied in floating point, so `100 * 0.9` comes out a hair under 90 on CI's Node version."),
        json!({ "type": "result", "subtype": "success", "is_error": false, "duration_ms": 12_000, "num_turns": 2, "result": "", "total_cost_usd": 0.03 }),
    ]);
    // Eight prompts, each answered, for the rail at the left of the conversation.
    let railed = {
        let asks = [
            "Why does the checkout test fail on CI but pass here?",
            "Move the discount maths into its own module.",
            "Add a test for the rounding.",
            "Does the cart round the same way?",
            "Bump the plugin's version and the changelog.",
            "Run the whole suite again.",
            "Why is the dev server slow to start?",
            "Write the release notes for Friday.",
        ];
        let mut lines = vec![init.clone()];
        for (n, ask) in asks.iter().enumerate() {
            lines.push(said(ask, &format!("rail-{n}")));
            lines.push(text(&format!("Done with step {}: it is in src/discount.ts and its test passes.", n + 1)));
            lines.push(json!({ "type": "result", "subtype": "success", "is_error": false, "duration_ms": 9_000, "num_turns": 2, "result": "", "total_cost_usd": 0.02 }));
        }
        opened(lines)
    };
    let older = vec!["scroll:up"; 12].join(";");
    // A conversation that names two pull requests, which the side panel lists with a card of each.
    let mentioned = opened(vec![
        init.clone(),
        user("Review https://github.com/acme/shop/pull/42 before we merge it."),
        text("It rounds the discount in cents now, and one check fails. The test for it went in with https://github.com/acme/shop/pull/37."),
    ]);
    let scenes: [Scene; 73] = [
        ("long", opened(vec![json!({ "type": "system", "subtype": "init", "session_id": LONG, "model": "claude-opus-5-5" })]), None, None, "Step 99 passes"),
        // Scrolled up past what the page has room for: older steps come in, the newest go, and a button goes back to them.
        ("mentioned", mentioned.clone(), None, None, "Mentioned"),
        // The pointer on a pull request's row shows its card.
        ("mentioned-card", mentioned, None, Some("hover:800,267"), "Mentioned"),
        ("long-older", opened(vec![json!({ "type": "system", "subtype": "init", "session_id": LONG, "model": "claude-opus-5-5" })]), None, Some(older.as_str()), "Jump to the latest"),
        ("conversation", conversation, None, None, "Allow Edit?"),
        ("streaming", streaming, None, None, "token bucket"),
        ("thinking", thinking, None, None, "Thinking"),
        ("new", opened(vec![init.clone()]), None, None, "What should Claude do?"),
        // The model and effort menu open over the message box.
        ("models", opened(vec![init.clone()]), None, Some("select:Model and effort"), "resets"),
        // A model with fast mode has its button by the model menu, and the output styles have a menu of their own.
        ("models-fast", fast, None, None, "resets"),
        // Up twice from the empty box recalls the second newest of the prompts sent.
        ("recall", recalled, None, Some("keys:up;keys:up"), "Now bump the version."),
        ("slash", opened(vec![init.clone()]), None, Some("type:/re;keys:down"), "/review"),
        // The header's Past sessions button opens the sheet over the chat, and its search has the keyboard.
        ("sessions", opened(vec![init.clone()]), None, Some("press:Past sessions"), "Past sessions"),
        ("sessions-search", opened(vec![init.clone()]), None, Some("press:Past sessions;type:checkout"), "matching messages"),
        // /usage opens the usage dialog over the conversation instead of going to Claude as a message, and so does /context.
        ("usage", work.iter().cloned().chain([full_usage]).collect(), None, Some("type:/usage;keys:enter"), "Plan usage limits"),
        ("context", work.iter().cloned().chain([full_context]).collect(), None, Some("type:/context;keys:enter"), "41,000 of 200,000 tokens"),
        // /plugins and /mcp open the plugins dialog over the chat, each leading with its own list.
        ("plugins", plugins.clone(), None, Some("type:/plugins;keys:enter"), "toolbox"),
        ("mcp", plugins.clone(), None, Some("type:/mcp;keys:enter"), "Needs sign-in"),
        // The dialog's Discover view lists what the fake `claude plugin list` prints; three tabs reach its search field (the
        // close button and the two chips come first), and the filter leaves api-guard.
        ("discover", plugins, None, Some("type:/plugins;keys:enter;chip:Discover;keys:tab tab tab;type:guard"), "api-guard"),
        ("failed", vec![], Some(("1", "Error: Invalid API key · Please run /login")), None, "Invalid API key"),
        ("work", worked, None, None, "Worked for 1m 23s"),
        ("work-open", work.clone(), None, Some("disclose:Ran 1 command"), "pnpm test discount"),
        ("detailed", work.clone(), None, None, "No test suite found"),
        ("large-text", work.clone(), None, None, "of 4 done"),
        ("queue", work.clone(), None, Some("type:also bump the version in apps/native/Cargo.toml, the plugin's manifest and the changelog, then run the whole suite again and tell me what failed;keys:enter;type:and add a changelog line, mention the new queue buttons in it, and check that the screenshots of the chat still match the design;keys:enter"), "Claude reads these"),
        // The same two, each opened to its whole message by its More button, which reads Less once it is open.
        ("queue-open", work.clone(), None, Some("type:also bump the version in apps/native/Cargo.toml, the plugin's manifest and the changelog, then run the whole suite again and tell me what failed;keys:enter;type:and add a changelog line, mention the new queue buttons in it, and check that the screenshots of the chat still match the design;keys:enter;press:More"), "Less"),
        ("ask-bash", ask_bash, None, None, "Allow Bash?"),
        ("question", question, None, None, "Which formats"),
        ("plan", plan, None, None, "Keep planning"),
        ("mention", opened(vec![init.clone()]), None, Some("type:fix the rounding in @che"), "checkout.test.ts"),
        // Two pasted pictures wait in the message box as chips, with a message under them.
        ("paste", opened(vec![init.clone()]), None, Some("paste:;paste:;type:what is in these?"), "Image 2"),
        // Each subagent's call has Open, which opens its conversation in Wings' sidebar (the `subagent-sidebar` scene); the
        // sidebar's Pop out makes it a tab (the `subagent-tab` scenes).
        ("subagent", subagent, None, Some("disclose:Ran 2 subagents"), "Survey the callers of applyDiscount"),
        ("suggestion", suggested, None, None, "Tab"),
        ("subagents", subagents, None, None, "Waiting on 2 subagents"),
        // The prompt rail: a mark for each message, the one in view lit, and a tooltip on the one the pointer is over.
        ("prompt-rail", railed, None, Some("hover:14,130"), "Write the release notes"),
        // The side panel shows by default, and lists the background tasks that run, each with how long it has and Stop.
        ("tasks", tasks.clone(), None, None, "Background tasks"),
        // A click on one opens the end of what it printed in a dialog.
        ("task-output", tasks, None, Some("row:Watch the test suite"), "Waiting for file changes"),
        // The Rewind button beside your first message opens its dialog, with the files restoring the code writes back.
        ("rewind", rewound, None, Some("press:Rewind"), "src/checkout.ts"),
        // /hooks, /memory and /permissions open their dialog over the chat, filled by the reads the Enter sends.
        ("hooks", opened(vec![init.clone()]), None, Some("type:/hooks;keys:enter"), "Before tool execution"),
        ("memory", opened(vec![init.clone()]), None, Some("type:/memory;keys:enter"), "412 tokens"),
        ("permissions", opened(vec![init.clone()]), None, Some("type:/permissions;keys:enter"), "set in User settings"),
        // Remove on a hook in a settings file asks first, in its place: what it removes, and from which file.
        ("hooks-remove", opened(vec![init.clone()]), None, Some("type:/hooks;keys:enter;press:Remove"), "Remove this PreToolUse hook"),
        // A new rule's form, filled in, before it is reviewed.
        ("permissions-add", opened(vec![init.clone()]), None, Some("type:/permissions;keys:enter;press:Add rule;type:Bash(pnpm build)"), "what it may do in parentheses"),
        // Review asks before the rule is written: the rule as typed, what allowing means, and the file it goes in.
        ("permissions-confirm", opened(vec![init.clone()]), None, Some("type:/permissions;keys:enter;press:Add rule;type:Bash(pnpm build);press:Review"), "Bash(pnpm build)"),
        // Claude Code refuses the rule, and the dialog says why, with the form as it was.
        (
            "permissions-refused",
            opened(vec![init.clone()]),
            None,
            Some("type:/permissions;keys:enter;press:Add rule;type:Bash(curl -u admin:hunter2 https://ci.example.com);press:Review;press:Add allow rule"),
            "contains a secret",
        ),
        // The picture you attached shows in your message, and the one Read gave back under its line, once the run is opened.
        ("pictures", pictures.clone(), None, Some("disclose:Read 1 file"), "its counter starts again"),
        // A click on a thumbnail shows it large in a dialog, which fits the window.
        ("picture-open", pictures, None, Some("disclose:Read 1 file;image:Picture from the tool"), "its counter starts again"),
        // A compaction leaves a divider with the tokens before and after, and a request Claude Code retries is the status line.
        ("compaction", compacting, None, None, "Retrying in 5s, attempt 2 of 10"),
        // /clear starts a new session: what was said is gone.
        ("cleared", cleared, None, None, "What should Claude do?"),
        // Claude's thinking is folded under its turn, and opens on a click.
        ("thought", thought.clone(), None, None, "Thought"),
        ("thought-open", thought, None, Some("disclose:Thought"), "so the ball costs 0.05"),
        // /status, /btw and /export open a dialog over the chat that the conversation never gets.
        ("status", opened(vec![init.clone()]), None, Some("type:/status;keys:enter"), "Setting sources"),
        ("btw", work.clone(), None, Some("type:/btw what does the rounding fix change?;keys:enter"), "It rounds once, at the end"),
        ("export", work, None, Some("type:/export;keys:enter"), "Copy to clipboard"),
        // Each reply and each code block has a Copy button.
        ("copy", copyable, None, None, "at exactly 90"),
        // A block of more than 20 lines shows its first 20, coloured as Rust, with a row that opens the other 35.
        ("code-fold", long_block, None, None, "Show 35 more lines"),
        // A Read in Detailed shows its file coloured as TypeScript, folded after 20 lines.
        ("read-ts", read_ts, None, None, "Show 5 more lines"),
        // Issue keys Issues knows and a dev server's address are links; UTF-8 and a key never filed aren't.
        ("links", linked, None, None, "never filed"),
        // The terminal Claude opened, live in the side panel with what it runs and buttons to open it as a tab and close it.
        ("terminal", built, None, None, "beside the chat"),
        // The button beside your message forks before it, and asks first what that does.
        ("fork-from", rewound_again, None, Some("press:Fork from here"), "Start a new chat from this session"),
        // The header's Fork asks first, and says what a fork is.
        ("fork-ask", worked_forking, None, Some("press:Fork"), "A fork is a new session"),
        // A long message that wraps keeps Rewind and Fork from here clear of it, under it at the right.
        ("long-message", wrapped, None, None, "which of the two places"),
        // A turn that changed files ends with a card of them, and the files its reply names are links.
        ("edits", edited, None, None, "Edited 2 files"),
        // Undo asks first, and says what goes back.
        ("edits-undo", edited_undo, None, Some("press:Undo"), "Undo the changes to 2 files from this turn?"),
        // The header's Side panel button turns the panel off, at any width, and the conversation has the room.
        ("panel-off", tasks_open, None, Some("press:Side panel"), "Both run in the background"),
        // /add-dir restarts the chat with the folder, which the side panel lists with a Remove.
        ("folder", worked_again, None, Some(add_dir.as_str()), "Folders"),
        // /mcp offers Reconnect on the server that failed, Sign in on the one that wants it and Sign out on a signed-in one.
        ("mcp-actions", signed_out.clone(), None, Some("type:/mcp;keys:enter"), "Reconnect"),
        // Sign in has Claude Code start it, and its answer is the page to open: the site is shown before the button opens it.
        ("mcp-sign-in", signed_out, None, Some("type:/mcp;keys:enter;press:Sign in"), "Open sign-in page"),
        ("elicitation", elicited, None, None, "Pick a flavour and how many scoops"),
        ("run-background", running, None, None, "Run in background"),
        ("agent-message", agent, None, Some("press:Message"), "SendMessage tool"),
        // The agent and the command that ended each leave a line, the agent's report folded under its own.
        ("tasks-finished", finished, None, None, "Command “Run the tests” failed"),
        // The side panel lists them as done, the agent under Subagents and the command under Background tasks.
        ("tasks-done", finished_panel, None, None, "Background tasks · 1 done"),
    ];
    // Files for @-mentions, which the chat lists with git.
    for file in ["src/checkout.ts", "src/checkout.test.ts", "src/cart.ts", "api/limits.ts", "README.md"] {
        let path = root.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }
    // A repo with a commit and a change since, which the side panel's Changes line counts.
    let git = |args: &[&str]| assert!(std::process::Command::new("git").args(["-c", "user.name=Shop", "-c", "user.email=shop@example.com"]).args(args).current_dir(&root).status().unwrap().success());
    git(&["init", "-q", "-b", "main"]);
    std::fs::write(root.join("src/checkout.ts"), "export const a = 1;\nexport const b = 2;\nexport const c = 3;\n").unwrap();
    git(&["add", "src", "api", "README.md"]);
    git(&["commit", "-q", "-m", "Start"]);
    std::fs::write(root.join("src/checkout.ts"), "export const a = 1;\nexport const c = 30;\nexport const d = 4;\nexport const e = 5;\n").unwrap();

    let manifest: wings_plugins::manifest::Manifest = serde_json::from_str(include_str!("../../../plugins/claude-chat/wings-plugin.json")).unwrap();
    let project = root.canonicalize().unwrap().display().to_string();
    // A host over a storage folder of its own that holds `seed`, the settings of the Settings page: the page reads them as it builds.
    let host_over = |folder: &str, seed: &[(&str, serde_json::Value)]| {
        let storage = out.join(folder);
        let _ = std::fs::remove_dir_all(&storage);
        let store = wings_plugin_storage::Storage::new(storage.clone());
        for (key, value) in seed {
            store.set("claude-chat", "shot", key, value.clone()).unwrap();
        }
        let host = PluginHost::with_access(
            PluginConfig { id: "claude-chat".into(), root: storage, scope: "shot".into() },
            Access {
                plugin: wings_plugins::manifest::Plugin { manifest: manifest.clone(), dir: root.clone(), icon_svg: None },
                builtin: false,
                env: std::sync::Arc::new(FixtureEnv(root.canonicalize().unwrap())),
            },
        );
        host.publish_projects(vec![wings_native::page::plugin::Project { hidden_sections: Vec::new(), id: project.clone(), name: "shop".into(), parent_id: None, path: project.clone() }]);
        host
    };
    let host = host_over("fixture-storage-claude-chat", &[]);
    // Detailed output on: every run of calls open, with each command's output.
    let detailed = host_over("fixture-storage-claude-chat-detailed", &[("detailed-output", json!(true))]);
    // Large text: the conversation's text a fifth bigger.
    let large = host_over("fixture-storage-claude-chat-large", &[("text-size", json!("large"))]);
    let page = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/claude-chat/dist/native/chat.wasm");
    let page = page.canonicalize().expect("sh build-rust.sh claude-chat, in plugins/");
    // Compiled before the first scene waits on it: on a busy Mac a debug build's compile ran past the scene's wait.
    wings_native::page::runtime::wasm::compile_now(&page.to_string_lossy()).unwrap();
    let source = format!("{}{}", wings_native::page::runtime::wasm::MARKER, page.display());
    let mut cx = app();
    // The links Wings gives the pages with Issues and Dev servers on: the keys of the issues there are, and local addresses.
    let link = |id: &str, pattern: &str| wings_plugins::manifest::Link {
        id: id.into(),
        title: id.into(),
        pattern: pattern.into(),
        view: String::new(),
        arg: String::new(),
        url: true,
    };
    let keys = std::sync::Arc::new(wings_plugins::links::words(&["ABC-12".into(), "ABC-14".into()]).unwrap());
    let rules = vec![
        wings_plugins::links::Rule::new("tasks", &link("issue", "[A-Z]{1,3}-[0-9]{1,9}"), Some(keys)).unwrap(),
        wings_plugins::links::Rule::new("dev-servers", &link("local", "http://localhost:[0-9]{2,5}"), None).unwrap(),
    ];
    cx.update(|cx| {
        cx.set_global(wings_native::page::links::PageLinks {
            rules: std::sync::Arc::new(rules),
            generation: 1,
            open: std::rc::Rc::new(|_: wings_native::page::links::LinkTarget, _: &mut gpui::Window, _: &mut gpui::App| {}),
        })
    });
    // `CHAT_SCENE=<name>` renders that scene alone.
    let only = std::env::var("CHAT_SCENE").ok();
    for (name, lines, exit, act, expected) in scenes {
        if only.as_deref().is_some_and(|only| only != name) {
            continue;
        }
        let host = match name {
            "detailed" | "read-ts" => &detailed,
            "large-text" => &large,
            _ => &host,
        };
        let fixture = root.join(format!("{name}.jsonl"));
        let printed = lines.iter().map(|line| format!("{line}\n")).collect::<String>();
        let printed = if name.starts_with("edits") { printed.replace("/work/shop", &project) } else { printed };
        std::fs::write(&fixture, printed).unwrap();
        // What the fake answers a request with, by its subtype: the rewind dialog's dry run, with a path in the project.
        let answers = root.join(format!("answers-{name}"));
        let _ = std::fs::remove_dir_all(&answers);
        std::fs::create_dir_all(&answers).unwrap();
        if name == "rewind" {
            let check = answer(&format!("rewind-check-{FIRST}"), json!({ "canRewind": true, "filesChanged": [format!("{project}/src/checkout.ts")], "insertions": 2, "deletions": 1 }));
            std::fs::write(answers.join("rewind_files"), format!("{check}\n")).unwrap();
        }
        // Sign in is the chat's second request (the dialog's reads were the first), so Claude Code's answer is under its id.
        if name == "mcp-sign-in" {
            let page = answer("server-auth-2", json!({ "authUrl": "https://auth.issues.example.com/authorize?response_type=code&state=x", "requiresUserAction": true, "callbackExpected": true }));
            std::fs::write(answers.join("mcp_authenticate"), format!("{page}\n")).unwrap();
        }
        // Claude Code's answers to the reads of /hooks, /memory and /permissions, made up, each under the subtype it answers.
        if ["hooks", "memory", "permissions"].iter().any(|dialog| name.starts_with(dialog)) {
            for line in include_str!("../../../plugins/claude-chat/rust/tests/setup.jsonl").lines() {
                let id = serde_json::from_str::<serde_json::Value>(line).unwrap()["response"]["request_id"].as_str().unwrap().to_owned();
                let subtype = match id.as_str() {
                    "hooks-listing" => "get_hooks_listing",
                    "memory-dialog" => "get_memory_dialog",
                    "memory-sizes" => "get_context_usage",
                    "permission-rules" => "list_permission_rules",
                    _ => "get_settings",
                };
                std::fs::write(answers.join(subtype), format!("{line}\n")).unwrap();
            }
        }
        if name == "permissions-refused" {
            std::fs::write(answers.join("edit-permission-rules"), "the rule looks like it contains a secret, so it was not saved to settings\n").unwrap();
        }
        // Claude Code's answers to the reads of /status, /btw and /export, made up, each under the subtype it answers. A side
        // question's id ends with the number of the write that asked it, which is the first.
        if name == "status" {
            let status = answer("status-dialog", json!({ "sections": [
                { "title": "Session", "rows": [
                    { "label": "Version", "value": "2.1.327" },
                    { "label": "Session ID", "value": "5b7e2c1a-0f3d-4c8e-9a61-2d4f8b3c7e90" },
                    { "label": "cwd", "value": project.clone() },
                    { "label": "Login method", "value": "Claude Max account" } ] },
                { "title": "Environment", "rows": [
                    { "label": "Model", "value": "claude-opus-5-5" },
                    { "label": "MCP servers", "value": "2 connected, 1 needs auth · /mcp" },
                    { "label": "Setting sources", "value": "User settings, Project settings" },
                    { "value": "Your organization may apply global configuration to Claude Code." } ] } ] }));
            std::fs::write(answers.join("get_status"), format!("{status}\n")).unwrap();
        }
        if name == "btw" {
            let side = answer("side-question-1", json!({ "response": "It rounds once, at the end, in cents, so `100 * 0.9` is exactly 90 instead of 89.99999999999999.", "synthetic": false }));
            std::fs::write(answers.join("side_question"), format!("{side}\n")).unwrap();
        }
        if name == "export" {
            let export = answer("export-dialog", json!({ "text": "User: Move the discount maths into its own module and test it.\n\nClaude: The discount maths is in src/discount.ts now, and checkout.ts re-exports it.\n", "default_filename": "conversation-2026-11-18-101500.txt" }));
            std::fs::write(answers.join("export_conversation"), format!("{export}\n")).unwrap();
        }
        // SAFETY: the page from the scene before is closed, so nothing reads these while they change.
        unsafe {
            std::env::set_var("CHAT_FIXTURE", &fixture);
            std::env::set_var("CHAT_ANSWERS", &answers);
            std::env::set_var("CHAT_EXIT", exit.map_or("", |(code, _)| code));
            std::env::set_var("CHAT_STDERR", exit.map_or("", |(_, said)| said));
        }
        // The side panel shows by default and floats over a tab's right side, beside the conversation while the tab is wide
        // enough to leave it clear (1,480 points) and over it in a narrower one, so a few scenes are drawn at 1,600 too and at
        // 1,280 just under it.
        let widths: &[f32] = if ["conversation", "work", "ask-bash", "tasks", "terminal", "folder", "long-message", "subagents", "subagent"].contains(&name) {
            &[1600.0, 1280.0, 1000.0, 640.0]
        } else {
            &[1000.0, 640.0]
        };
        for &width in widths {
            // The shell of the terminal the workspace draws over the page, opened before the page so the fake `claude` the
            // page starts is found under the scene's own shell.
            let shot_terminal = (name == "terminal").then(|| {
                let printed = b"$ make all\r\n  Compiling shop v0.4.0 (/work/shop)\r\n  Compiling checkout v0.4.0\r\n  Compiling cart v0.4.0\r\n    Building [=========>       ] 41/63: cart\r\n".to_vec();
                // SAFETY: nothing else reads the environment while the shell starts.
                unsafe { std::env::set_var("SHELL", "/bin/sh") };
                let terminal = cx.update(|cx| wings_native::terminal_view::TerminalView::open(root.clone(), printed, None, cx).unwrap());
                unsafe { std::env::set_var("SHELL", root.join("shell")) };
                terminal
            });
            let args = matches!(name, "long" | "long-older").then(|| json!({ "resume": LONG, "cwd": root.canonicalize().unwrap() }));
            let handle = cx
                .open_offscreen_window(size(px(width), px(760.0)), |_, cx| cx.new(|cx| PageView::in_plugin_with_args(&host, &source, args.as_ref(), cx)))
                .unwrap();
            let window = handle.into();
            handle.read_with(&cx, |view, _| view.set_visible(true)).unwrap();
            if let Some(terminal) = shot_terminal {
                // What the workspace does over the page: it hands the tab the terminal its agent opened, and draws it there,
                // here over made-up output of a build, in a shell of its own rather than the scene's fake one.
                handle
                    .update(&mut cx, |view, _, cx| {
                        let listed = json!([{ "paneId": "1", "title": "make all", "running": true }]).to_string();
                        view.set_terminals(vec![1], listed, cx);
                        view.draw_terminals_for_shots(move |_| {
                            let card = wings_native::page::terminal_card(1, "make all".into(), true, terminal.clone(), |_, _| {}, |_, _| {});
                            vec![(wings_native::page::terminal_slot(1), card)]
                        });
                    })
                    .unwrap();
            }
            let until = |cx: &mut VisualTestAppContext, expected: &str| {
                let deadline = Instant::now() + Duration::from_secs(30);
                while !handle.read_with(cx, |view, _| view.text().contains(expected)).unwrap() {
                    assert!(Instant::now() < deadline, "page did not show {expected:?}: {}", handle.read_with(cx, |view, _| view.text()).unwrap());
                    wait(cx, window, Duration::from_millis(20));
                }
            };
            if let Some(act) = act {
                // Once Claude Code has answered, the model picker shows.
                let deadline = Instant::now() + Duration::from_secs(30);
                while handle.read_with(&cx, |view, _| view.select_named("Model and effort")).unwrap().is_none() {
                    assert!(Instant::now() < deadline, "the model picker never showed: {}", handle.read_with(&cx, |view, _| view.text()).unwrap());
                    wait(&mut cx, window, Duration::from_millis(20));
                }
                for act in act.split(';') {
                    match act.split_once(':') {
                        Some(("type", typed)) => cx.simulate_input(window, typed),
                        Some(("keys", keys)) => cx.simulate_keystrokes(window, keys),
                        // The clipboard holds a picture, which Cmd-V gives the message box. The bytes needn't decode.
                        Some(("paste", _)) => {
                            let picture = gpui::Image::from_bytes(gpui::ImageFormat::Png, vec![0x89, b'P', b'N', b'G']);
                            cx.update(|cx| cx.write_to_clipboard(gpui::ClipboardItem::new_image(&picture)));
                            cx.simulate_keystrokes(window, "cmd-v");
                        }
                        Some(("wait", ms)) => wait(&mut cx, window, Duration::from_millis(ms.parse().unwrap())),
                        // The pointer over a point of the page, which shows the tooltip of what is there once it has rested.
                        Some(("hover", at)) => {
                            let (x, y) = at.split_once(',').unwrap();
                            let position = point(px(x.parse().unwrap()), px(y.parse().unwrap()));
                            cx.simulate_event(window, gpui::MouseMoveEvent { position, pressed_button: None, modifiers: gpui::Modifiers::default() });
                            wait(&mut cx, window, Duration::from_millis(1200));
                        }
                        // The wheel all the way up or down the conversation.
                        Some(("scroll", way)) => cx.simulate_event(window, gpui::ScrollEvent {
                            position: point(px(width / 2.0), px(300.0)),
                            delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(if way == "up" { 100_000.0 } else { -100_000.0 }))),
                            modifiers: gpui::Modifiers::default(),
                            touch_phase: gpui::TouchPhase::Moved,
                            source: Default::default(),
                        }),
                        Some(("chip", label)) => assert!(handle.read_with(&cx, |view, _| view.press_chip(label)).unwrap()),
                        Some(("press", label)) => assert!(handle.read_with(&cx, |view, _| view.press(label, 0)).unwrap()),
                        // A row of a list that is a button, like a task in the side panel.
                        Some(("row", text)) => assert!(handle.read_with(&cx, |view, _| view.press_list_button(text)).unwrap()),
                        Some(("image", label)) => assert!(handle.read_with(&cx, |view, _| view.press_image(label)).unwrap()),
                        Some(("disclose", summary)) => assert!(handle.update(&mut cx, |view, _, cx| view.press_disclosure(summary, cx)).unwrap()),
                        Some(("select", label)) => assert!(handle.update(&mut cx, |view, _, cx| view.open_select(label, cx)).unwrap()),
                        _ => panic!("{act}"),
                    }
                    // The page hears what was typed before the next key.
                    wait(&mut cx, window, Duration::from_millis(300));
                }
            }
            until(&mut cx, expected);
            wait(&mut cx, window, Duration::from_millis(300));
            save(&mut cx, window, out.join(format!("claude-chat-{name}-{width}.png")));
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
    }
    // A subagent's tab: the subagent's transcript, read from its session's folder, how it stands as its chat tells it, and the
    // message box, which sends through the chat. The chat runs in a window behind, as in a tab beside; without it the box
    // says why it can't send. The sidebar scene is the same page as Wings' sidebar shows it, in a window of the sidebar's
    // width of its own: the harness draws one page to a window, so Wings' window around it, with the Changes sidebar beside
    // it, isn't in the picture. The chat's own Open button, pressed here, is what tells the sidebar which subagent to show.
    if only.as_deref().is_none_or(|only| only.starts_with("subagent-")) {
        let side_page = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/claude-chat/dist/native/subagent.wasm");
        let side_page = side_page.canonicalize().expect("sh build-rust.sh claude-chat, in plugins/");
        wings_native::page::runtime::wasm::compile_now(&side_page.to_string_lossy()).unwrap();
        let side_source = format!("{}{}", wings_native::page::runtime::wasm::MARKER, side_page.display());
        const SUB_SESSION: &str = "8c2e4a6b-1d3f-4a5c-9e7b-0f2d4c6e8a13";
        let (call, agent, title) = ("sa1", "asa1", "Survey the callers of applyDiscount");
        let prompt = "List every caller of applyDiscount, and say which of them round.";
        let printed = [
            haiku(json!([{ "type": "tool_use", "id": "sg1", "name": "Grep", "input": { "pattern": "applyDiscount" } }])),
            output("sg1", "Found 3 files\nsrc/checkout.ts\nsrc/discount.ts\nsrc/cart.ts", false),
            haiku(json!([{ "type": "text", "text": "Found three callers:\n\n- `src/checkout.ts` re-exports it\n- `src/cart.ts` calls it once per line item, and rounds the total\n- `src/discount.ts` defines it" }])),
            haiku(json!([{ "type": "tool_use", "id": "sg2", "name": "Read", "input": { "file_path": "/work/shop/src/cart.ts" } }])),
        ];
        // The subagent's own transcript, in a folder named after its session beside the session's, as Claude Code keeps it.
        let folder = root.join(format!("projects/-work-shop/{SUB_SESSION}/subagents"));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(root.join(format!("projects/-work-shop/{SUB_SESSION}.jsonl")), "").unwrap();
        let entries: String = [user(prompt)]
            .into_iter()
            .chain(printed.iter().cloned())
            .enumerate()
            .map(|(n, mut entry)| {
                entry["uuid"] = format!("su{n}").into();
                entry["isSidechain"] = true.into();
                entry["agentId"] = agent.into();
                format!("{entry}\n")
            })
            .collect();
        std::fs::write(folder.join(format!("agent-{agent}.jsonl")), entries).unwrap();
        // The chat that runs it: its call, Claude Code's word that it started, and the lines it printed so far.
        let started = [
            json!({ "type": "system", "subtype": "init", "session_id": SUB_SESSION, "model": "claude-opus-5-5" }),
            user("Find every caller of applyDiscount and tell me which of them round."),
            tool(call, "Agent", json!({ "description": title, "subagent_type": "Explore", "prompt": prompt })),
            json!({ "type": "system", "subtype": "task_started", "task_id": agent, "tool_use_id": call, "description": title, "task_type": "local_agent" }),
        ];
        let chat_lines = opened(started.into_iter().chain(printed.iter().cloned().map(|line| within(call, line))).collect());
        let fixture = root.join("subagent-chat.jsonl");
        std::fs::write(&fixture, chat_lines.iter().map(|line| format!("{line}\n")).collect::<String>()).unwrap();
        let answers = root.join("answers-subagent-tab");
        let _ = std::fs::remove_dir_all(&answers);
        std::fs::create_dir_all(&answers).unwrap();
        // SAFETY: the pages of the scenes before are closed, so nothing reads these while they change.
        unsafe {
            std::env::set_var("CHAT_FIXTURE", &fixture);
            std::env::set_var("CHAT_ANSWERS", &answers);
            std::env::set_var("CHAT_EXIT", "");
            std::env::set_var("CHAT_STDERR", "");
        }
        let args = json!({ "subagent": { "session": SUB_SESSION, "agent": agent, "title": title } });
        for (state, chat_open, sidebar) in [("subagent-tab", true, false), ("subagent-tab-closed", false, false), ("subagent-sidebar", true, true)] {
            if only.as_deref().is_some_and(|only| only != state) {
                continue;
            }
            let widths: &[f32] = if sidebar { &[420.0] } else { &[1000.0, 640.0] };
            for &width in widths {
                let chat_view = chat_open.then(|| {
                    let handle = cx
                        .open_offscreen_window(size(px(1000.0), px(760.0)), |_, cx| cx.new(|cx| PageView::in_plugin_with_args(&host, &source, None, cx)))
                        .unwrap();
                    handle.read_with(&cx, |view, _| view.set_visible(true)).unwrap();
                    let window = handle.into();
                    let deadline = Instant::now() + Duration::from_secs(30);
                    while !handle.read_with(&cx, |view, _| view.text().contains("Find every caller")).unwrap() {
                        assert!(Instant::now() < deadline, "the chat did not start: {}", handle.read_with(&cx, |view, _| view.text()).unwrap());
                        wait(&mut cx, window, Duration::from_millis(20));
                    }
                    handle
                });
                let chat: Option<AnyWindowHandle> = chat_view.map(Into::into);
                let (page, page_args) = if sidebar { (&side_source, None) } else { (&source, Some(&args)) };
                let handle = cx
                    .open_offscreen_window(size(px(width), px(760.0)), |_, cx| cx.new(|cx| PageView::in_plugin_with_args(&host, page, page_args, cx)))
                    .unwrap();
                handle.read_with(&cx, |view, _| view.set_visible(true)).unwrap();
                let window = handle.into();
                if let Some(chat_view) = chat_view.filter(|_| sidebar) {
                    // The sidebar is empty until a click in the chat says which subagent: the call's Open button.
                    let deadline = Instant::now() + Duration::from_secs(30);
                    while !handle.read_with(&cx, |view, _| view.text().contains("No subagent open")).unwrap() {
                        assert!(Instant::now() < deadline, "the sidebar did not start: {}", handle.read_with(&cx, |view, _| view.text()).unwrap());
                        wait(&mut cx, window, Duration::from_millis(20));
                    }
                    assert!(chat_view.update(&mut cx, |view, _, cx| view.press_disclosure("Ran 1 subagent", cx)).unwrap());
                    wait(&mut cx, chat_view.into(), Duration::from_millis(300));
                    assert!(chat_view.read_with(&cx, |view, _| view.press("Open", 0)).unwrap());
                }
                // Open, the box says how a message goes once the chat has said it can pass one on; closed, it says why not.
                let shown = match (chat_open, sidebar) {
                    (true, true) => ["Found three callers", "Pop out"],
                    (true, false) => ["Found three callers", "Goes through the chat"],
                    (false, _) => ["Found three callers", "is closed"],
                };
                for expected in shown {
                    let deadline = Instant::now() + Duration::from_secs(30);
                    while !handle.read_with(&cx, |view, _| view.text().contains(expected)).unwrap() {
                        assert!(Instant::now() < deadline, "the subagent's tab did not show {expected:?}: {}", handle.read_with(&cx, |view, _| view.text()).unwrap());
                        wait(&mut cx, window, Duration::from_millis(20));
                    }
                }
                wait(&mut cx, window, Duration::from_millis(300));
                save(&mut cx, window, out.join(format!("claude-chat-{state}-{width}.png")));
                let _ = cx.update_window(window, |_, window, _| window.remove_window());
                if let Some(chat) = chat {
                    let _ = cx.update_window(chat, |_, window, _| window.remove_window());
                }
            }
        }
    }
    // The page in Wings Settings, with a mode, Detailed and Large text picked.
    if only.as_deref().is_none_or(|only| only == "settings") {
        let settings = host_over("fixture-storage-claude-chat-settings", &[("default-mode", json!("auto")), ("detailed-output", json!(true)), ("open-command", json!(false)), ("open-thought", json!(false)), ("text-size", json!("large"))]);
        let page = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/claude-chat/dist/native/settings.wasm");
        let page = page.canonicalize().expect("the claude-chat settings page is built, in plugins/");
        wings_native::page::runtime::wasm::compile_now(&page.to_string_lossy()).unwrap();
        let source = format!("{}{}", wings_native::page::runtime::wasm::MARKER, page.display());
        for width in [1000.0, 640.0] {
            let handle = cx
                .open_offscreen_window(size(px(width), px(900.0)), |_, cx| cx.new(|cx| PageView::in_plugin(&settings, &source, cx)))
                .unwrap();
            let window = handle.into();
            handle.read_with(&cx, |view, _| view.set_visible(true)).unwrap();
            let deadline = Instant::now() + Duration::from_secs(30);
            while !handle.read_with(&cx, |view, _| view.text().contains("Text size")).unwrap() {
                assert!(Instant::now() < deadline, "page did not show the settings: {}", handle.read_with(&cx, |view, _| view.text()).unwrap());
                wait(&mut cx, window, Duration::from_millis(20));
            }
            wait(&mut cx, window, Duration::from_millis(300));
            save(&mut cx, window, out.join(format!("claude-chat-settings-{width}.png")));
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
    let views: [(&str, Steps, Steps); 4] = [
        ("overview", vec![], vec![]),
        ("map", vec![("Scan", scan, "Downloads")], vec![]),
        ("menu", vec![("Scan", scan, "Downloads"), ("⋯", 0, "Add to Collector")], vec![]),
        (
            "collected",
            vec![
                ("Scan", scan, "Downloads"),
                ("Add to Collector", 0, "1 item"),
            ],
            vec![("Remove from Collector", 0, "Drag folders")],
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
                // A menu's button has no click of its own for the page: the app opens the menu.
                let pressed = handle
                    .read_with(cx, |view, _| view.press(label, *nth))
                    .unwrap()
                    || handle
                        .update(cx, |view, _, cx| view.open_menu(label, *nth, cx))
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
