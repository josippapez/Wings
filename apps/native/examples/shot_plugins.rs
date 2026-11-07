//! Renders Worktrees' installed native sidebar offscreen at 1000 and 640 points wide. The plugin installation and
//! project are made up in a temporary desktop data folder, never the user's. With `--changes` it renders Changes' built
//! pages, with `--schedules` Schedules' list and forms, with `--session-detail` Session detail's sidebar, and with
//! `--disk` Disk usage's overview and the map of a folder of made-up files it scans, with `--work-item` Work item's tab
//! and Sprint block, and with `--tasks` the Issues board, list, issue and block, each over made-up data. With
//! `--external <folder>` it renders the separately shipped Sprint board, Cyclops and Claude accounts plugins from their
//! repos in that folder, popovers too (`shot_plugins/external.rs`).
//! Nothing shows on screen and focus doesn't move. macOS only (real Metal rendering).
//!
//! cargo run --manifest-path apps/native/Cargo.toml --example shot_plugins --features test-support -- <out-dir>

use std::{
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use gpui::{AnyWindowHandle, AppContext, VisualTestAppContext, px, size};
use serde_json::json;
use wings_native::{
    actions,
    host::Config,
    icons::Assets,
    installed::fixture::Data,
    page::{
        PageView,
        plugin::{
            Access, Env, PaneEntry, PanePort, PluginConfig, PluginHost, SessionEntry,
            ui::{Badge, PaneStatus},
        },
    },
    workspace::Workspace,
};

#[path = "shot_plugins/external.rs"]
mod external;

const WORKTREES_MAIN: &str = "wings.openView('worktrees', { as: 'sidebar' });";
const WORKTREES_PAGE: &str = include_str!("../../../plugins/worktrees/dist/native/sidebar.js");

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

fn main() {
    let out = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "shots".into()));
    std::fs::create_dir_all(&out).expect("create the output folder");
    if std::env::args().any(|arg| arg == "--changes") {
        changes(out);
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
    if std::env::args().any(|arg| arg == "--session-detail") {
        session_detail(out);
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
    if std::env::args().any(|arg| arg == "--tasks") {
        tasks(out);
        return;
    }
    let data = Data::temp();
    data.install_script(
        "worktrees",
        json!({
            "icon": "icon.svg",
            "permissions": {
                "exec": [
                    "git rev-parse [--show-toplevel --abbrev-ref --verify --quiet]",
                    "git worktree list [--porcelain]",
                    "git status [--porcelain]",
                    "git log [--max-count --format]",
                    "git branch --list [--list --merged --format]"
                ],
                "panes": [],
                "diskUsage": true
            },
            "contributes": { "sidebars": [{ "id": "worktrees", "title": "Worktrees", "page": "dist/sidebar.html" }] },
        }),
        WORKTREES_MAIN,
        &[("sidebar", WORKTREES_PAGE)],
        "shot",
    );
    let project = std::env::temp_dir().join("wings-native-shot-plugins-project");
    std::fs::create_dir_all(&project).expect("make a project folder");
    let args = ["--project".into(), project.clone().into_os_string()];
    let mut config = Config::parse(args, &project)
        .expect("parse the options")
        .expect("options, not --help");
    config.state = None;
    config.projects = None;
    config.agents = None;
    config.plugins = Some(data.paths());
    let mut cx = VisualTestAppContext::with_asset_source(
        gpui_platform::current_platform(false),
        std::sync::Arc::new(Assets),
    );
    cx.update(actions::bind);
    for width in [1000.0, 640.0] {
        let config = config.clone();
        let handle = cx
            .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                cx.new(|cx| Workspace::new(config, window, cx))
            })
            .expect("open an offscreen window");
        let window: AnyWindowHandle = handle.into();
        wait(&mut cx, window, Duration::from_secs(3));
        save(
            &mut cx,
            window,
            out.join(format!("worktrees-sidebar-{width}.png")),
        );
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
}

/// An app with one project, which does nothing for a plugin.
struct FixtureEnv(PathBuf);
impl Env for FixtureEnv {
    fn projects(&self) -> Vec<PathBuf> {
        vec![self.0.clone()]
    }
    fn current_project(&self) -> Option<PathBuf> {
        Some(self.0.clone())
    }
    fn claude_dirs(&self) -> Vec<PathBuf> {
        vec![self.0.clone()]
    }
    /// The OpenCode session of `session_detail`.
    fn agent_activity(&self, _: &str) -> Result<serde_json::Value, String> {
        let info = json!({ "location": { "directory": "/work/api" }, "model": { "id": "gpt-5.1-codex" }, "cost": 0.42,
            "tokens": { "input": 12_000, "output": 3_100, "reasoning": 900, "cache": { "read": 88_000, "write": 4_000 } } });
        let now = now_ms();
        // A call that is still running has no time it completed.
        let tool = |id: &str, name: &str, ago: u64, took: Option<u64>, state: serde_json::Value| {
            let mut time = json!({ "created": now - ago });
            if let Some(took) = took {
                time["completed"] = (now - ago + took).into();
            }
            json!({ "type": "tool", "id": id, "name": name, "state": state, "time": time })
        };
        // The service sends the newest message first.
        let messages = [
            json!({ "id": "m3", "type": "assistant", "model": { "id": "gpt-5.1-codex" },
                "tokens": { "input": 2_000, "output": 300, "cache": { "read": 60_000, "write": 0 } }, "content": [
                tool("c3", "shell", 20_000, None, json!({ "status": "running", "input": { "command": "cargo test" }, "metadata": {} })),
                tool("c2", "shell", 90_000, Some(1_200), json!({ "status": "completed", "input": { "command": "git commit -m 'Cache the shipping rates'" },
                    "content": [{ "type": "text", "text": "[main 4f2a9c1] Cache the shipping rates\n 2 files changed" }], "metadata": {} })),
                tool("c1", "patch", 300_000, Some(80), json!({ "status": "completed", "input": { "patchText": "*** Update File: src/rates.rs" },
                    "content": [{ "type": "text", "text": "Success. Updated the following files:\nM src/rates.rs" }],
                    "metadata": { "files": [{ "file": "src/rates.rs" }, { "file": "src/cache.rs" }] } })),
            ] }),
            json!({ "id": "m1", "type": "user", "text": "Cache the shipping rates so the cart stops asking for them twice", "time": { "created": now - 330_000 } }),
        ];
        serde_json::to_value(wings_agents::agent_activity::normalize(&info, &messages))
            .map_err(|error| error.to_string())
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

/// Actual built pages in a fixture host. This does not exercise workspace openView/window routing.
fn changes(out: PathBuf) {
    let root = out.join("fixture-repo");
    std::fs::create_dir_all(&root).unwrap();
    let git = |args: &[&str]| {
        let result = std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .env("GIT_AUTHOR_NAME", "Ada Lovelace")
            .env("GIT_AUTHOR_EMAIL", "ada@example.invalid")
            .env("GIT_COMMITTER_NAME", "Ada Lovelace")
            .env("GIT_COMMITTER_EMAIL", "ada@example.invalid")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    };
    if !root.join(".git").exists() {
        git(&["init", "-q", "-b", "main"]);
        git(&["commit", "-q", "--allow-empty", "-m", "Initial checkout"]);
        git(&["checkout", "-q", "-b", "feature/coupon"]);
        git(&[
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "Add a coupon field to checkout",
        ]);
        git(&[
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "Validate coupon codes",
        ]);
        git(&["checkout", "-q", "main"]);
        git(&[
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "Round shipping rates",
        ]);
        git(&[
            "merge",
            "-q",
            "--no-ff",
            "feature/coupon",
            "-m",
            "Merge checkout coupon field",
        ]);
        git(&["tag", "v2.14.0"]);
    }
    let manifest =
        serde_json::from_str(include_str!("../../../plugins/changes/wings-plugin.json")).unwrap();
    let host = PluginHost::with_access(
        PluginConfig {
            id: "changes".into(),
            root: out.join("fixture-storage"),
            scope: "shot".into(),
        },
        Access {
            plugin: wings_plugins::manifest::Plugin {
                manifest,
                dir: root.clone(),
                icon_svg: None,
            },
            builtin: false,
            env: std::sync::Arc::new(FixtureEnv(root.clone())),
        },
    );
    let fixture = include_str!("../../../plugins/changes/preview/native-host.js")
        .replace("__ROOT__", &root.to_string_lossy());
    host.start_main(&fixture, |event| {
        if let wings_native::page::runtime::Event::Error { message, .. } = event {
            panic!("{message}");
        }
    })
    .unwrap();
    let mut cx = VisualTestAppContext::with_asset_source(
        gpui_platform::current_platform(false),
        std::sync::Arc::new(Assets),
    );
    cx.update(actions::bind);
    for (name, source) in [
        (
            "sidebar",
            include_str!("../../../plugins/changes/dist/native/sidebar.js"),
        ),
        (
            "graph",
            include_str!("../../../plugins/changes/dist/native/graph.js"),
        ),
    ] {
        for width in [1000.0, 640.0] {
            let handle = cx
                .open_offscreen_window(size(px(width), px(640.0)), |_, cx| {
                    cx.new(|cx| PageView::in_plugin(&host, source, cx))
                })
                .unwrap();
            let window = handle.into();
            let expected = if name == "graph" {
                "commits on every branch"
            } else {
                "Coupon.tsx"
            };
            let deadline = Instant::now() + Duration::from_secs(10);
            while !handle
                .read_with(&cx, |view, _| view.text().contains(expected))
                .unwrap()
            {
                assert!(
                    Instant::now() < deadline,
                    "page did not load: {}",
                    handle.read_with(&cx, |view, _| view.text()).unwrap()
                );
                wait(&mut cx, window, Duration::from_millis(20));
            }
            save(
                &mut cx,
                window,
                out.join(format!("changes-{name}-{width}.png")),
            );
            if name == "sidebar" {
                let position = gpui::point(px(205.0), px(78.0));
                cx.simulate_event(
                    window,
                    gpui::MouseDownEvent {
                        position,
                        button: gpui::MouseButton::Left,
                        modifiers: gpui::Modifiers::default(),
                        click_count: 1,
                        first_mouse: false,
                    },
                );
                cx.simulate_event(
                    window,
                    gpui::MouseUpEvent {
                        position,
                        button: gpui::MouseButton::Left,
                        modifiers: gpui::Modifiers::default(),
                        click_count: 1,
                    },
                );
                let deadline = Instant::now() + Duration::from_secs(10);
                while !handle
                    .read_with(&cx, |view, _| view.text().contains("Add coupon validation"))
                    .unwrap()
                {
                    assert!(Instant::now() < deadline, "history did not load");
                    wait(&mut cx, window, Duration::from_millis(20));
                }
                save(
                    &mut cx,
                    window,
                    out.join(format!("changes-history-{width}.png")),
                );
            }
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
    }
}

/// Schedules' real main over made-up schedules and one paused "assigned" entry with tickets, and its built page. The
/// entries are kept in a storage folder of their own, so nothing runs: no schedule is due, and the entry is paused.
fn schedules(out: PathBuf) {
    let project = out.join("fixture-project");
    std::fs::create_dir_all(&project).unwrap();
    let id = project.to_string_lossy().into_owned();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let hour = 3_600_000;
    let storage = out.join("fixture-storage-schedules");
    let _ = std::fs::remove_dir_all(&storage);
    let store = wings_plugin_storage::Storage::new(storage.clone());
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
    store
        .set(
            "schedules",
            "shot",
            "assigned",
            json!([{
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
            }]),
        )
        .unwrap();
    let manifest: wings_plugins::manifest::Manifest =
        serde_json::from_str(include_str!("../../../plugins/schedules/wings-plugin.json")).unwrap();
    let host = PluginHost::with_access(
        PluginConfig {
            id: "schedules".into(),
            root: storage,
            scope: "shot".into(),
        },
        Access {
            plugin: wings_plugins::manifest::Plugin {
                manifest,
                dir: project.clone(),
                icon_svg: None,
            },
            builtin: true,
            env: std::sync::Arc::new(FixtureEnv(project.clone())),
        },
    );
    host.publish_projects(vec![wings_native::page::plugin::Project {
        id: id.clone(),
        name: "shop".into(),
        parent_id: None,
        path: id,
    }]);
    host.start_main(
        include_str!("../../../plugins/schedules/dist/native/main.js"),
        |event| {
            if let wings_native::page::runtime::Event::Error { message, .. } = event {
                panic!("{message}");
            }
        },
    )
    .unwrap();
    let mut cx = VisualTestAppContext::with_asset_source(
        gpui_platform::current_platform(false),
        std::sync::Arc::new(Assets),
    );
    cx.update(actions::bind);
    let source = include_str!("../../../plugins/schedules/dist/native/schedules.js");
    // What to press to reach each view, and a text it shows.
    let views = [
        ("list", None, "Frontend tickets"),
        ("new", Some(("Add entry", 0)), "Start an agent"),
        ("edit-entry", Some(("Edit", 2)), "Keep free"),
    ];
    for width in [1000.0, 640.0] {
        for (name, press, expected) in views {
            let handle = cx
                .open_offscreen_window(size(px(width), px(900.0)), |_, cx| {
                    cx.new(|cx| PageView::in_plugin(&host, source, cx))
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
            until(&mut cx, "Frontend tickets");
            if let Some((label, nth)) = press {
                let pressed = handle
                    .read_with(&cx, |view, _| view.press(label, nth))
                    .unwrap();
                assert!(pressed, "no button {label}");
            }
            until(&mut cx, expected);
            wait(&mut cx, window, Duration::from_millis(300));
            save(
                &mut cx,
                window,
                out.join(format!("schedules-{name}-{width}.png")),
            );
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
    }
}

/// Session detail's real main and built sidebar over a made-up Claude Code session, whose transcript is written to a
/// folder of its own, and a made-up OpenCode one. It shows the session working, with rows opened, then the OpenCode
/// session, then no session at all.
fn session_detail(out: PathBuf) {
    const SESSION: &str = "3c21b0c5-8113-4f55-9493-fa156c3fa369";
    let root = out.join("fixture-session-detail");
    let minute = 60_000;
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("projects/-work-web")).unwrap();
    let read = |id: &str, name: &str, ago: u64, input: serde_json::Value| {
        json!({ "type": "assistant", "uuid": format!("a-{id}"), "timestamp": iso(ago), "message": { "id": format!("m-{id}"), "model": "claude-sonnet-5-5",
            "usage": { "input_tokens": 4_200, "output_tokens": 800, "cache_read_input_tokens": 110_000, "cache_creation_input_tokens": 1_800 },
            "content": [{ "type": "tool_use", "id": id, "name": name, "input": input }] } })
    };
    let result = |id: &str, ago: u64, text: &str, error: bool| {
        json!({ "type": "user", "uuid": format!("r-{id}"), "timestamp": iso(ago), "message": { "content": [
            { "type": "tool_result", "tool_use_id": id, "content": text, "is_error": error }] } })
    };
    let lines = [
        json!({ "type": "user", "uuid": "p1", "timestamp": iso(42 * minute), "message": { "content":
            "Rework the checkout page so the coupon field sits under the order summary, and keep the totals in sync with the cart." } }),
        read(
            "t1",
            "Read",
            41 * minute,
            json!({ "file_path": "/work/web/src/features/checkout/CheckoutPage.tsx" }),
        ),
        result(
            "t1",
            41 * minute - 120,
            "import { useCoupon } from '../coupons';",
            false,
        ),
        read(
            "t2",
            "Grep",
            40 * minute,
            json!({ "pattern": "useCoupon", "path": "src/" }),
        ),
        result(
            "t2",
            40 * minute - 340,
            "src/features/checkout/CheckoutPage.tsx:3",
            false,
        ),
        json!({ "type": "assistant", "uuid": "a-t3", "timestamp": iso(38 * minute), "message": { "id": "m-t3", "model": "claude-sonnet-5-5",
            "usage": { "input_tokens": 4_200, "output_tokens": 1_600, "cache_read_input_tokens": 110_000, "cache_creation_input_tokens": 1_800 },
            "content": [
                { "type": "tool_use", "id": "t3", "name": "Edit", "input": { "file_path": "/work/web/src/features/checkout/CheckoutSummary.tsx",
                    "old_string": "<Total value={subtotal} />", "new_string": "<Total value={subtotal - discount} />" } },
                { "type": "tool_use", "id": "w1", "name": "TodoWrite", "input": { "todos": [
                    { "content": "Move the coupon field under the summary", "activeForm": "Moving the coupon field", "status": "completed" },
                    { "content": "Subtract the discount from the total", "activeForm": "Subtracting the discount", "status": "completed" },
                    { "content": "Show the discount in the cart", "activeForm": "Showing the discount in the cart", "status": "in_progress" },
                    { "content": "Update the checkout snapshots", "activeForm": "Updating the snapshots", "status": "pending" }] } }] } }),
        result(
            "t3",
            38 * minute - 42,
            "The file src/features/checkout/CheckoutSummary.tsx has been updated.",
            false,
        ),
        read(
            "t4",
            "Bash",
            30 * minute,
            json!({ "command": "git commit -m 'Add a coupon field to the checkout summary'" }),
        ),
        result(
            "t4",
            30 * minute - 880,
            "[feature/checkout 9b1c2d3] Add a coupon field to the checkout summary",
            false,
        ),
        read(
            "t5",
            "Bash",
            29 * minute,
            json!({ "command": "git push origin feature/checkout-redesign" }),
        ),
        result(
            "t5",
            29 * minute - 2_400,
            "To github.com:example/shop.git",
            false,
        ),
        read(
            "t6",
            "Bash",
            12 * minute,
            json!({ "command": "pnpm test -- checkout" }),
        ),
        result(
            "t6",
            12 * minute - 18_400,
            "FAIL src/features/checkout/CheckoutSummary.test.tsx\n  ● coupon › keeps the totals in sync\n\n    Expected: 41.2\n    Received: 45",
            true,
        ),
        json!({ "type": "attachment", "uuid": "q1", "timestamp": iso(5 * minute), "attachment": { "type": "queued_command", "humanTurn": true,
            "prompt": "Also make the coupon discount show in the cart" } }),
        read(
            "t7",
            "Edit",
            minute,
            json!({ "file_path": "/work/web/src/routes/cart.tsx", "old_string": "<Total value={subtotal} />", "new_string": "<Total value={subtotal - discount} />" }),
        ),
        read(
            "t8",
            "Bash",
            20_000,
            json!({ "command": "pnpm test -- cart" }),
        ),
    ];
    let text: Vec<String> = lines.iter().map(ToString::to_string).collect();
    std::fs::write(
        root.join(format!("projects/-work-web/{SESSION}.jsonl")),
        text.join("\n"),
    )
    .unwrap();

    let manifest = serde_json::from_str(include_str!(
        "../../../plugins/session-detail/wings-plugin.json"
    ))
    .unwrap();
    let storage = out.join("fixture-storage-session-detail");
    let _ = std::fs::remove_dir_all(&storage);
    let host = PluginHost::with_access(
        PluginConfig {
            id: "session-detail".into(),
            root: storage,
            scope: "shot".into(),
        },
        Access {
            plugin: wings_plugins::manifest::Plugin {
                manifest,
                dir: root.clone(),
                icon_svg: None,
            },
            builtin: false,
            env: std::sync::Arc::new(FixtureEnv(root.clone())),
        },
    );
    let started_at = now_ms() - 60 * minute;
    let pane = |id: &str, kind: &str, session: &str, name: &str, state: &str| PaneEntry {
        pane_id: id.into(),
        cwd: "/work/web".into(),
        command: kind.into(),
        project: "web".into(),
        project_id: "/work/web".into(),
        session: Some(SessionEntry {
            session_id: session.into(),
            kind: kind.into(),
            name: Some(name.into()),
            state: state.into(),
            started_at,
        }),
        focused: false,
    };
    let claude = pane("1", "claude", SESSION, "checkout redesign", "working");
    let opencode = pane("2", "opencode", "ses_shot1", "shipping rates", "working");
    host.publish_panes(vec![claude, opencode.clone()]);
    host.start_main(
        include_str!("../../../plugins/session-detail/dist/native/main.js"),
        |event| {
            if let wings_native::page::runtime::Event::Error { message, .. } = event {
                panic!("{message}");
            }
        },
    )
    .unwrap();
    let mut cx = VisualTestAppContext::with_asset_source(
        gpui_platform::current_platform(false),
        std::sync::Arc::new(Assets),
    );
    cx.update(actions::bind);
    let source = include_str!("../../../plugins/session-detail/dist/native/sidebar.js");
    // What the panes are, the rows to open, and a text the page shows once it has drawn it. 360 is the sidebar's own width.
    let views: [(&str, Vec<PaneEntry>, &[&str], &str); 4] = [
        (
            "claude",
            vec![
                pane("1", "claude", SESSION, "checkout redesign", "working"),
                opencode.clone(),
            ],
            &[],
            "Adding",
        ),
        (
            "opened",
            vec![
                pane("1", "claude", SESSION, "checkout redesign", "working"),
                opencode.clone(),
            ],
            &[
                "Rework the checkout",
                "CheckoutSummary.tsx",
                "pnpm test -- checkout",
            ],
            "Expected: 41.2",
        ),
        (
            "opencode",
            vec![opencode],
            &[],
            "Cache the shipping rates so",
        ),
        ("none", vec![], &[], "No agent session in your panes."),
    ];
    for width in [1000.0, 640.0, 360.0] {
        for (name, panes, rows, expected) in &views {
            host.publish_panes(panes.clone());
            let handle = cx
                .open_offscreen_window(size(px(width), px(900.0)), |_, cx| {
                    cx.new(|cx| PageView::in_plugin(&host, source, cx))
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
            for row in rows.iter() {
                until(&mut cx, row);
                let pressed = handle
                    .read_with(&cx, |view, _| view.press_row(row, 0))
                    .unwrap();
                assert!(pressed, "no row {row}");
                wait(&mut cx, window, Duration::from_millis(300));
            }
            until(&mut cx, expected);
            wait(&mut cx, window, Duration::from_millis(300));
            save(
                &mut cx,
                window,
                out.join(format!("session-detail-{name}-{width}.png")),
            );
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
    }
}

/// Disk usage's real main and built page: the disks are this Mac's, and the map is of a folder of made-up files, which it
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
    let storage = out.join("fixture-storage-disk");
    let _ = std::fs::remove_dir_all(&storage);
    let manifest: wings_plugins::manifest::Manifest =
        serde_json::from_str(include_str!("../../../plugins/disk/wings-plugin.json")).unwrap();
    let host = PluginHost::with_access(
        PluginConfig {
            id: "disk".into(),
            root: storage,
            scope: "shot".into(),
        },
        Access {
            plugin: wings_plugins::manifest::Plugin {
                manifest,
                dir: folder.clone(),
                icon_svg: None,
            },
            builtin: true,
            env: std::sync::Arc::new(FixtureEnv(folder.clone())),
        },
    );
    host.publish_projects(vec![wings_native::page::plugin::Project {
        id: id.clone(),
        name: "Fixture".into(),
        parent_id: None,
        path: id,
    }]);
    host.start_main(
        include_str!("../../../plugins/disk/dist/native/main.js"),
        |event| {
            if let wings_native::page::runtime::Event::Error { message, .. } = event {
                panic!("{message}");
            }
        },
    )
    .unwrap();
    let mut cx = VisualTestAppContext::with_asset_source(
        gpui_platform::current_platform(false),
        std::sync::Arc::new(Assets),
    );
    cx.update(actions::bind);
    let source = include_str!("../../../plugins/disk/dist/native/map.js");
    // The Scan buttons come in the page's order: a disk's each, then the home folder's, then the project's, which is the folder.
    let scan = wings_disk_usage::disks().len() + 1;
    // What to press to reach each view, one after the other with the text each shows, and what undoes it: main keeps
    // the Collector between windows.
    type Steps<'a> = Vec<(&'a str, usize, &'a str)>;
    let views: [(&str, Steps, Steps); 3] = [
        ("overview", vec![], vec![]),
        ("map", vec![("Scan", scan, "Downloads")], vec![]),
        (
            "collected",
            vec![
                ("Scan", scan, "Downloads"),
                ("⋯ ▾", 0, "Add to Collector"),
                ("Add to Collector", 0, "1 item"),
            ],
            vec![
                ("⋯ ▾", 0, "Remove from Collector"),
                ("Remove from Collector", 0, "Drag folders"),
            ],
        ),
    ];
    for width in [1000.0, 640.0] {
        for (name, steps, undo) in &views {
            let handle = cx
                .open_offscreen_window(size(px(width), px(900.0)), |_, cx| {
                    cx.new(|cx| PageView::in_plugin(&host, source, cx))
                })
                .unwrap();
            let window = handle.into();
            handle
                .read_with(&cx, |view, _| view.set_visible(true))
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
            until(&mut cx, "Folders");
            // The disks come from main, which reads them with `df`.
            let deadline = Instant::now() + Duration::from_secs(20);
            while handle
                .read_with(&cx, |view, _| view.text().contains("Looking for disks"))
                .unwrap()
            {
                assert!(Instant::now() < deadline, "the disks never came");
                wait(&mut cx, window, Duration::from_millis(20));
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
                press(&mut cx, step);
            }
            wait(&mut cx, window, Duration::from_millis(500));
            save(
                &mut cx,
                window,
                out.join(format!("disk-{name}-{width}.png")),
            );
            for step in undo {
                press(&mut cx, step);
            }
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
    }
    let _ = std::fs::remove_dir_all(&folder);
}

/// A page, what it opens with, what to press to reach a view of it, and a text it shows.
type View<'a> = (
    &'a str,
    &'a str,
    Option<&'a serde_json::Value>,
    &'a [&'a str],
    &'a str,
);

/// PR tracker's built pages over a fixture main (`plugins/pr-tracker/preview/native-host.js`): the project's list, a pull
/// request opened from it with its description and conversation, its Checks tab, a check's run, and the settings.
fn pr_tracker(out: PathBuf) {
    let project = out.join("fixture-project");
    std::fs::create_dir_all(&project).unwrap();
    let id = project.to_string_lossy().into_owned();
    let manifest: wings_plugins::manifest::Manifest = serde_json::from_str(include_str!(
        "../../../plugins/pr-tracker/wings-plugin.json"
    ))
    .unwrap();
    let host = PluginHost::with_access(
        PluginConfig {
            id: "pr-tracker".into(),
            root: out.join("fixture-storage-pr"),
            scope: "shot".into(),
        },
        Access {
            plugin: wings_plugins::manifest::Plugin {
                manifest,
                dir: project.clone(),
                icon_svg: None,
            },
            builtin: true,
            env: std::sync::Arc::new(FixtureEnv(project.clone())),
        },
    );
    host.publish_projects(vec![wings_native::page::plugin::Project {
        id: id.clone(),
        name: "web".into(),
        parent_id: None,
        path: id.clone(),
    }]);
    let fixture_main = include_str!("../../../plugins/pr-tracker/preview/native-host.js").replace(
        r#"- `SyncClient.send` retries up to 5 times\n- Delay doubles from 200 ms\n"#,
        r#"- SyncClient retries failed requests with exponential backoff, caps the delay, and reports each retry so operators can diagnose a slow upstream without losing the pull request's complete status history\n1. The retry budget applies to every request in this pull request, including the request that loads the final status after the retry loop has reached its configured limit\n- [ ] Add a metric that records the retry count for this request and explains why it reached the final retry budget before the worker returned\n  - Keep the nested diagnostic note with the parent item so the rendered description still explains the retry decision on a narrow pull request page\n"#,
    );
    host.start_main(&fixture_main.replace("__ROOT__", &id), |event| {
        if let wings_native::page::runtime::Event::Error { message, .. } = event {
            panic!("{message}");
        }
    })
    .unwrap();
    let mut cx = VisualTestAppContext::with_asset_source(
        gpui_platform::current_platform(false),
        std::sync::Arc::new(Assets),
    );
    cx.update(actions::bind);
    let tab = include_str!("../../../plugins/pr-tracker/dist/native/pr.js");
    let settings = include_str!("../../../plugins/pr-tracker/dist/native/settings.js");
    let args = json!({
        "provider": "github", "url": "https://github.com/acme/web/pull/482", "number": 482,
        "repo": "acme/web", "title": "Add retry with backoff to the sync client", "cwd": id,
    });
    // The page, what it opens with, what to press to reach each view, and a text it shows.
    let views: [View; 5] = [
        ("list", tab, None, &[], "Migrate the settings page"),
        ("pull", tab, Some(&args), &[], "Conversation"),
        ("checks", tab, Some(&args), &["Checks"], "Unit tests"),
        (
            "run",
            tab,
            Some(&args),
            &["Checks", "Unit tests"],
            "Install dependencies",
        ),
        ("settings", settings, None, &[], "Bitbucket Cloud"),
    ];
    for width in [1000.0, 640.0] {
        for (name, source, args, presses, expected) in views {
            let handle = cx
                .open_offscreen_window(size(px(width), px(1500.0)), |_, cx| {
                    cx.new(|cx| PageView::in_plugin_with_args(&host, source, args, cx))
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
                        "page did not show {text:?}: {}",
                        handle.read_with(cx, |view, _| view.text()).unwrap()
                    );
                    wait(cx, window, Duration::from_millis(20));
                }
            };
            until(
                &mut cx,
                if name == "list" {
                    "Migrate the settings page"
                } else if name == "settings" {
                    "Bitbucket Cloud"
                } else {
                    "Conversation"
                },
            );
            for (step, label) in presses.iter().enumerate() {
                let pressed = handle
                    .read_with(&cx, |view, _| {
                        if step == 0 {
                            view.press_chip(label)
                        } else {
                            view.press_list_button(label)
                        }
                    })
                    .unwrap();
                assert!(pressed, "nothing to press for {label}");
                until(&mut cx, if step == 0 { "Unit tests" } else { expected });
            }
            wait(&mut cx, window, Duration::from_millis(300));
            save(
                &mut cx,
                window,
                out.join(format!("pr-tracker-{name}-{width}.png")),
            );
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
    }
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
    let mut cx = VisualTestAppContext::with_asset_source(
        gpui_platform::current_platform(false),
        std::sync::Arc::new(Assets),
    );
    cx.update(actions::bind);
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
                    .read_with(&cx, |view, _| view.press_chip(label))
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
    host.publish_projects(vec![wings_native::page::plugin::Project {
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

/// Work item's built tab and Sprint block. The tab calls `az` itself, so the shots run an `az` of their own, first on
/// PATH, that answers from `plugins/work-item/preview/native-az.json`; the block's main is the fixture main
/// `plugins/work-item/preview/native-host.js`.
fn work_item(out: PathBuf) {
    let answers: serde_json::Map<String, serde_json::Value> = serde_json::from_str(include_str!(
        "../../../plugins/work-item/preview/native-az.json"
    ))
    .unwrap();
    let bin = out.join("fake-az");
    std::fs::create_dir_all(&bin).unwrap();
    let mut script = String::from("#!/bin/sh\ncase \"$*\" in\n");
    for (index, (words, answer)) in answers
        .iter()
        .filter(|(words, _)| !words.starts_with('_'))
        .enumerate()
    {
        let file = bin.join(format!("{index}.json"));
        std::fs::write(&file, answer.to_string()).unwrap();
        script += &format!("  *'{words}'*) cat '{}' ;;\n", file.display());
    }
    script += "  *) echo \"no made-up answer for az $*\" >&2; exit 1 ;;\nesac\n";
    let az = bin.join("az");
    std::fs::write(&az, script).unwrap();
    std::process::Command::new("chmod")
        .arg("+x")
        .arg(&az)
        .status()
        .unwrap();
    // Programs are found on the login shell's PATH, then this process's: no login shell leaves this one.
    unsafe {
        std::env::set_var("SHELL", "/nonexistent-shell");
        std::env::set_var(
            "PATH",
            format!(
                "{}:{}",
                bin.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        );
    }
    let (host, _) = repo_host(
        &out,
        "work-item",
        include_str!("../../../plugins/work-item/wings-plugin.json"),
    );
    host.start_main(
        include_str!("../../../plugins/work-item/preview/native-host.js"),
        fail_on_error,
    )
    .unwrap();
    let item = include_str!("../../../plugins/work-item/dist/native/item.js");
    let sprint = include_str!("../../../plugins/work-item/dist/native/sprint.js");
    let args = json!({ "id": 1432, "org": "https://dev.azure.com/acme" });
    use wings_native::page::plugin::StatePlace::{Block, Tab};
    shoot_pages(
        &out,
        &host,
        "work-item",
        &[
            (
                "overview",
                item,
                Some(&args),
                Tab,
                &[],
                "Pull request is up",
            ),
            (
                "links",
                item,
                Some(&args),
                Tab,
                &["Links"],
                "Make the sync client reliable",
            ),
            (
                "history",
                item,
                Some(&args),
                Tab,
                &["History"],
                "Remaining work",
            ),
            ("sprint", sprint, None, Block, &[], "Add retry with backoff"),
        ],
    );
}

/// The Issues plugin's built board, list, issue tab and block with its real main, over the made-up issues
/// `plugins/tasks/preview/native-seed.js` stores first.
fn tasks(out: PathBuf) {
    let _ = std::fs::remove_dir_all(out.join("fixture-storage-tasks"));
    let (host, project) = repo_host(
        &out,
        "tasks",
        include_str!("../../../plugins/tasks/wings-plugin.json"),
    );
    let (seeded, done) = std::sync::mpsc::channel();
    let seed = host
        .open_page(
            &include_str!("../../../plugins/tasks/preview/native-seed.js")
                .replace("__PROJECT__", &project),
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
    host.start_main(
        include_str!("../../../plugins/tasks/dist/native/main.js"),
        fail_on_error,
    )
    .unwrap();
    let board = include_str!("../../../plugins/tasks/dist/native/board.js");
    let issue = include_str!("../../../plugins/tasks/dist/native/issue.js");
    let block = include_str!("../../../plugins/tasks/dist/native/block.js");
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
        ],
    );
}
