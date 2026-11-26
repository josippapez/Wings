//! Renders Worktrees' sidebar, which is written in Rust, offscreen at 1000 and 640 points wide, over a made-up repo in
//! the output folder, never the user's. With `--changes` it renders Changes' built
//! pages, with `--schedules` Schedules' list and forms, and with
//! `--share` Share's modal (a port asked about, what is shared, nothing shared), `--hud` the HUD's sidebar, with
//! `--disk` Disk usage's overview and the map of a folder of made-up files it scans, with `--work-item` Work item's tab
//! and Sprint block, and with `--tasks` the Issues board, list, issue and block, each over made-up data. With
//! `--external <folder>` it renders the separately shipped Sprint board and Cyclops plugins from their repos in that
//! folder, popovers too (`shot_plugins/external.rs`).
//! `--claude-chat` renders Claude chat's page over a fake `claude` that prints made-up sessions.
//! `--tooltips` points at a titled Button and Text of a made-up page until their tooltips show.
//! `--ui-gallery` renders the pages the Rust SDK's newer `ui` builders draw (`ui_gallery.json`, kept in step by a test in
//! `packages/plugin-rs/src/ui/more.rs`).
//! `--prompt-rail` renders a `PromptRail` of 40 prompts with one pointed at, and one of 300 that has scrolled.
//! Nothing shows on screen and focus doesn't move. macOS only (real Metal rendering).
//! `--list` prints every flag and what it renders (`SCENES`). Each screen is checked for text and controls cut off at its
//! width (`ui::cutoff`): a `CUT OFF` line under its PNG for each, all said again at the end, and the run exits with 1.
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
    ui::cutoff,
};

#[path = "shot_plugins/external.rs"]
mod external;
#[path = "shot_plugins/typing.rs"]
mod typing;
#[path = "shot_plugins/browser.rs"]
mod browser;
#[path = "shot_plugins/claude_chat.rs"]
mod claude_chat;
#[path = "shot_plugins/disk.rs"]
mod disk;
#[path = "shot_plugins/pr_tracker.rs"]
mod pr_tracker;
#[path = "shot_plugins/schedules.rs"]
mod schedules;
#[path = "shot_plugins/share.rs"]
mod share;
#[path = "shot_plugins/small_scenes.rs"]
mod small_scenes;
#[path = "shot_plugins/tasks.rs"]
mod tasks;
#[path = "shot_plugins/work_item.rs"]
mod work_item;
#[path = "shot_plugins/worktrees_hud.rs"]
mod worktrees_hud;
use claude_chat::*;
use disk::*;
use pr_tracker::*;
use schedules::*;
use share::*;
use small_scenes::*;
use tasks::*;
use work_item::*;
use worktrees_hud::*;

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
    // What this screen cuts off at its width, said under its PNG and again at the end of the run (`ui::cutoff`).
    let cuts = cx.update_window(window, |_, window, cx| cutoff::check(window, cx)).unwrap_or_default();
    let _ = cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx));
    // `SHOT_FRAMES=<n>` draws each screen n more times from scratch and prints the median ms of a draw and of a frame
    // rendered on the GPU, for what a redraw of that screen costs.
    if let Some(frames) = std::env::var("SHOT_FRAMES").ok().and_then(|n| n.parse::<usize>().ok()) {
        let (mut drawn, mut rendered) = (Vec::new(), Vec::new());
        for _ in 0..frames {
            let start = Instant::now();
            let _ = cx.update_window(window, |_, window, cx| {
                window.refresh();
                window.draw(cx).clear(cx)
            });
            drawn.push(start.elapsed().as_secs_f64() * 1000.0);
            let start = Instant::now();
            let _ = cx.capture_screenshot(window);
            rendered.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        let median = |times: &mut Vec<f64>| {
            times.sort_by(f64::total_cmp);
            times[times.len() / 2]
        };
        println!("[measure] {}: draw {:.2} ms, GPU frame {:.2} ms", path.display(), median(&mut drawn), median(&mut rendered));
    }
    cx.capture_screenshot(window)
        .expect("render the window")
        .save(&path)
        .expect("save the PNG");
    println!("{}", path.display());
    cutoff::report(&path, &cuts);
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

/// A set of screens this example renders: the flag that picks it, what it shows, and the function that renders it. The
/// first scene whose flag is given runs, and with none, Worktrees' sidebar. `--list` prints them.
struct Scene {
    flag: &'static str,
    help: &'static str,
    run: fn(PathBuf),
}

const SCENES: &[Scene] = &[
    Scene { flag: "--browser", help: "the Browser's tab (its web page is WebKit's, an empty place here) and its settings", run: browser::browser },
    Scene { flag: "--changes", help: "Changes' built pages", run: changes },
    Scene { flag: "--stopped", help: "a page that stopped, with its error and Reload, in a tab and a popover", run: stopped },
    Scene { flag: "--schedules", help: "Schedules' list and forms, empty too, and the modals that ask to allow a schedule", run: schedules },
    Scene { flag: "--share-missing", help: "Share's modal when `opentunnel` isn't installed: it offers Install", run: share_missing },
    Scene { flag: "--share", help: "Share's modal: a port asked about, what is shared, nothing shared", run: share },
    Scene { flag: "--pr-tracker", help: "PR tracker's list, a pull request and its tabs on GitHub and Azure DevOps, settings, host states", run: pr_tracker },
    Scene { flag: "--claude-accounts", help: "Claude accounts' sidebar and popover with one account signed out", run: claude_accounts },
    Scene { flag: "--hud", help: "the HUD's sidebar: two sessions on two accounts, then none", run: hud },
    Scene { flag: "--disk", help: "Disk usage's overview and the map of a folder of made-up files", run: disk },
    Scene { flag: "--work-item", help: "Work item's tab and Sprint block over a fake `az`", run: work_item },
    Scene { flag: "--external", help: "<folder>: the separately shipped Cyclops plugin, from its repo in <folder>, popovers too", run: external_plugins },
    Scene { flag: "--tooltips", help: "a titled Button and Text, each pointed at until its tooltip shows", run: tooltips },
    Scene { flag: "--prompt-rail", help: "a PromptRail of 40 prompts with one pointed at, and one of 300 that has scrolled", run: prompt_rail },
    Scene { flag: "--ui-gallery", help: "the pages the Rust SDK's `ui` builders draw (shot_plugins/ui_gallery.json)", run: ui_gallery },
    Scene { flag: "--claude-chat", help: "Claude chat over a fake `claude`; CHAT_SCENE=<name>[,...] renders those alone, CHAT_SKIP=<name>[,...] all but those", run: claude_chat },
    Scene { flag: "--tasks", help: "the Issues board, list, issue tab and block", run: tasks },
    Scene { flag: "--worktrees", help: "Worktrees' sidebar over a repo it makes (the scene with no flag)", run: worktrees },
];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--list") {
        for scene in SCENES {
            println!("{:<16} {}", scene.flag, scene.help);
        }
        return;
    }
    let out = PathBuf::from(args.get(1).cloned().unwrap_or_else(|| "shots".into()));
    std::fs::create_dir_all(&out).expect("create the output folder");
    let scene = SCENES
        .iter()
        .find(|scene| args.iter().any(|arg| arg == scene.flag))
        .unwrap_or(&SCENES[SCENES.len() - 1]);
    // `--theme <name>` paints every scene in that colour theme, as Settings, Appearance would.
    if let Some(name) = args.iter().skip_while(|arg| *arg != "--theme").nth(1) {
        wings_native::theme::set(wings_native::theme::Theme::from_key(Some(name)), true);
    }
    // The modules' compiled code is kept for the next run, which then loads it in place of compiling: Claude chat's took
    // 26 s to compile with the debug build's Cranelift, and 5 s with the optimised one.
    wings_native::page::runtime::wasm::keep_compiled_in(std::env::temp_dir().join("wings-test-cwasm"));
    (scene.run)(out);
    cutoff::end_run();
}

/// `--external <folder>`: the plugins in the repos in that folder (`shot_plugins/external.rs`).
fn external_plugins(out: PathBuf) {
    let root = std::env::args()
        .skip_while(|arg| arg != "--external")
        .nth(1)
        .expect("--external <folder of the plugin repos>");
    external::external(out, root.into());
}

/// An app with one project, which does nothing for a plugin.
/// What Claude Code told `wings statusline`, for the HUD.
static STATUSLINE: std::sync::Mutex<Option<serde_json::Value>> = std::sync::Mutex::new(None);

struct FixtureEnv(PathBuf);
impl Env for FixtureEnv {
    // This Mac has WebKit's extensions, and nothing else WebKit does reaches a screenshot.
    fn web(&self, job: wings_native::web::Job, done: Box<dyn FnOnce(Result<String, String>) + Send>) {
        let supported = matches!(job, wings_native::web::Job::Supported);
        done(Ok(if supported { "true" } else { "null" }.into()))
    }
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
    // Two Claude accounts, personal and work, so Claude chat shows its account menu.
    fn claude_accounts(&self) -> Result<wings_spaces::claude_accounts::Accounts, String> {
        use wings_spaces::claude_accounts::{Account, Accounts};
        // `$SHOT_SIGNED_OUT` names the account the Claude accounts scene shows as signed out.
        let signed_out = std::env::var("SHOT_SIGNED_OUT").ok();
        let account = |name: Option<&str>, email: &str| Account { name: name.map(String::from), dir: String::new(), email: Some(email.into()), signed_in: signed_out.as_deref() != name, auth_method: None, shares_setup: false };
        Ok(Accounts { accounts: vec![account(None, "me@example.com"), account(Some("work"), "me@work.example.com")], projects: Default::default() })
    }
    // Two dev servers in the pane the Browser scene publishes, which no other scene has.
    fn ports(&self) -> Result<Vec<PanePort>, String> {
        let server = |name: &str, port| PanePort { pane_id: "shot-web".into(), pid: 1, name: name.into(), port, started_at: 0 };
        Ok(vec![server("storybook", 6006), server("node", 5173)])
    }
    fn open_url(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn notify(&self, _: wings_native::page::plugin::Notification) -> Result<(), String> {
        Ok(())
    }
    // Schedules' "Allow this?" modal: the scene shows it itself, so opening it succeeds. A host that refuses makes Schedules
    // answer the ask at once as not shown, which empties the modal. Every other view stays unavailable, as before.
    fn open_view(&self, plan: wings_native::page::plugin::OpenView, reply: wings_native::page::runtime::Reply) {
        if (plan.plugin == "schedules" && plan.id == "approve") || (plan.plugin == "share" && plan.id == "share") {
            reply.ok("null".into());
        } else {
            reply.err("Opening views isn't available".into());
        }
    }
    // Issues' list, made up, for the one scene that sets `$CHAT_ISSUES` (the folder holding `tasks.txt`): the chat's host there is
    // one of Wings' own plugins, which may call it. Everywhere else no plugin runs, as before.
    fn builtin(&self, plugin: &str) -> Option<bool> {
        let set = |name: &str| std::env::var_os(name).is_some_and(|dir| !dir.is_empty());
        ((plugin == "tasks" && set("CHAT_ISSUES")) || (plugin == "pr-tracker" && set("CHAT_PR"))).then_some(true)
    }
    fn call_tool(&self, call: wings_native::page::plugin::ToolCall, reply: wings_native::page::runtime::Reply) {
        // The PR tracker's answer for the side panel's Pull request section: the text in the file `$CHAT_PR` names.
        if let Some(file) = std::env::var_os("CHAT_PR").filter(|file| !file.is_empty()).filter(|_| call.target == "pr-tracker" && call.tool == "pull_request") {
            let text = std::fs::read_to_string(PathBuf::from(file)).unwrap_or_default();
            return reply.ok(json!({ "text": text, "isError": false }).to_string());
        }
        match std::env::var_os("CHAT_ISSUES").filter(|dir| !dir.is_empty()) {
            Some(dir) if call.target == "tasks" && call.tool == "list" => {
                let text = std::fs::read_to_string(PathBuf::from(dir).join("tasks.txt")).unwrap_or_default();
                reply.ok(json!({ "text": text, "isError": false }).to_string());
            }
            _ => reply.err(format!("{} is turned off or not installed", call.target)),
        }
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
                // Each chip shows once the page has loaded what it needs. `menu:<name>` opens the menu of the button
                // named so, and a label no chip shows presses the button that shows it.
                let deadline = Instant::now() + Duration::from_secs(10);
                while !match label.strip_prefix("menu:") {
                    Some(menu) => handle.update(&mut cx, |view, _, cx| view.open_menu(menu, 0, cx)).unwrap(),
                    None => handle
                        .read_with(&cx, |view, _| {
                            view.press_chip(label) || view.turn_on_chip(label) || view.press(label, 0)
                        })
                        .unwrap(),
                } {
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
