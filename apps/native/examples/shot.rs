//! Renders the native window offscreen at 1000 and 640 points wide and saves each state as a PNG:
//! tabs and dragging one, a terminal's colors and a selection, splits, renaming a pane, a zoomed pane, find, Issues, a
//! plugin page, agents and Settings, and the pilot and components pages on their own, drawn from the trees React made of them.
//! Nothing shows on screen and focus doesn't move. macOS only (real Metal rendering).
//! Each screen is checked for controls cut off at its width (`ui::cutoff`): a `CUT OFF` line under its PNG for each, all
//! said again at the end, and the run exits with 1.
//!
//! cargo run --manifest-path apps/native/Cargo.toml --example shot --features test-support -- <out-dir>
#![recursion_limit = "256"]

#[path = "shot/chrome_scenes.rs"]
mod chrome_scenes;
#[path = "shot/diffs.rs"]
mod diffs;
#[path = "shot/launch_scenes.rs"]
mod launch_scenes;
#[path = "shot/parity.rs"]
mod parity;
#[path = "shot/plugin_pages.rs"]
mod plugin_pages;
#[path = "shot/plugins.rs"]
mod plugins;
use chrome_scenes::*;
use diffs::*;
use launch_scenes::*;
use parity::*;
use plugin_pages::*;
use plugins::*;

use std::{
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use gpui::{
    AnyWindowHandle, AppContext, Modifiers, MouseButton, MouseDownEvent, VisualTestAppContext,
    point, px, size,
};
use wings_native::{
    actions,
    host::{Config, SessionSource},
    icons::Assets,
    installed::{
        WINGS_VERSION,
        fixture::{Approval, Data},
    },
    page::{
        PageView,
        plugin::{PluginConfig, PluginHost},
        runtime::fake::{self, Cx, Guest, Hear},
    },
    workspace::{AccountStatus, Workspace},
};

/// The node `id` of `tree`.
fn node(tree: &mut serde_json::Value, id: u64) -> &mut serde_json::Value {
    fn find(tree: &mut serde_json::Value, id: u64) -> Option<&mut serde_json::Value> {
        if tree["id"] == id {
            return Some(tree);
        }
        tree.get_mut("children")?
            .as_array_mut()?
            .iter_mut()
            .find_map(|child| find(child, id))
    }
    find(tree, id).unwrap()
}

/// The pilot page as React drew it (`src/page/trees/pilot.json`): "Starting" at first, then "Ready" once it had mounted,
/// and a count that its button adds one to.
fn pilot() -> String {
    struct Pilot {
        ready: bool,
        count: u32,
    }
    impl Pilot {
        fn draw(&self, cx: &mut Cx<'_>) {
            let mut tree: serde_json::Value =
                serde_json::from_str(include_str!("../src/page/trees/pilot.json")).unwrap();
            node(&mut tree, 10)["text"] = format!("Count: {}", self.count).into();
            if self.ready {
                node(&mut tree, 3)["text"] = "Ready".into();
                node(&mut tree, 4)["props"]["tone"] = "success".into();
            }
            cx.render(&tree);
        }
    }
    impl Guest for Pilot {
        fn start(&mut self, cx: &mut Cx<'_>) {
            self.draw(cx);
            cx.later(1);
        }
        fn event(&mut self, cx: &mut Cx<'_>, _handler: &str, _payload: &serde_json::Value) {
            self.count += 1;
            self.draw(cx);
        }
        fn timer(&mut self, cx: &mut Cx<'_>, _id: u32) {
            self.ready = true;
            self.draw(cx);
        }
    }
    fake::source("page", || {
        Box::new(Pilot {
            ready: false,
            count: 0,
        })
    })
}

/// The components page as React drew it (`src/page/trees/components.json`), with what its two buttons have counted and
/// what its select was set to.
fn components() -> String {
    let (mut session, mut refreshed, mut retried) = ("web".to_owned(), 0, 0);
    fake::page("page", move |event| {
        match event {
            Some(("9:onClick", _)) => refreshed += 1,
            Some(("30:onClick", _)) => retried += 1,
            Some(("13:onChange", change)) => {
                session = change["value"].as_str().unwrap_or_default().to_owned()
            }
            _ => {}
        }
        let mut tree: serde_json::Value =
            serde_json::from_str(include_str!("../src/page/trees/components.json")).unwrap();
        node(&mut tree, 10)["text"] =
            format!("Session {session}, refreshed {refreshed}, retried {retried}").into();
        node(&mut tree, 13)["props"]["value"] = session.clone().into();
        tree
    })
}

/// A page that is the text `text` and nothing else.
fn words(text: &str) -> String {
    let text = text.to_owned();
    fake::page("page", move |_| {
        serde_json::json!({ "id": 0, "type": "#root", "props": {}, "children": [{ "id": 1, "type": "#text", "text": text }] })
    })
}

/// The page `frame` of this repo's plugin `plugin`, as its build left it (`sh build-rust.sh <plugin>`, in `plugins/`).
fn built(plugin: &str, frame: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../plugins/{plugin}/dist/native/{frame}.wasm"));
    let path = path
        .canonicalize()
        .unwrap_or_else(|_| panic!("sh build-rust.sh {plugin}, in plugins/"));
    format!("{}{}", wings_native::page::runtime::wasm::MARKER, path.display())
}

/// How far the app's clock moves between two frames.
const STEP: Duration = Duration::from_millis(20);
/// How long shells, git and plugin pages have been quiet when `settle` takes them as done.
const QUIET: Duration = Duration::from_millis(300);

/// One frame: the app's tasks run, its clock moves a step, and the window draws.
fn frame(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    cx.advance_clock(STEP);
    cx.run_until_parked();
    let _ = cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx));
}

/// Moves the app's clock through `duration` a frame at a time, without waiting in real time: its animations, timers
/// and debounces are done by then. A wait for what a shell, git or a plugin page does is `settle`.
fn wait(cx: &mut VisualTestAppContext, window: AnyWindowHandle, duration: Duration) {
    let mut moved = Duration::ZERO;
    while moved < duration {
        frame(cx, window);
        moved += STEP;
    }
}

/// Waits in real time for what shells, git and plugin pages do, a frame at a time: until none of them has woken a task
/// of the app for `QUIET`, `most` at the longest. Then the app's clock moves on to `most`, as `wait` moves it. A fixed
/// wait of `most` cost all of it, though they are mostly done in a fraction.
fn settle(cx: &mut VisualTestAppContext, window: AnyWindowHandle, most: Duration) {
    let start = Instant::now();
    let (mut heard, mut moved) = (start, Duration::ZERO);
    while start.elapsed() < most && heard.elapsed() < QUIET {
        frame(cx, window);
        moved += STEP;
        thread::sleep(STEP);
        let mut woken = false;
        while cx.executor().tick() {
            woken = true;
        }
        if woken {
            heard = Instant::now();
        }
    }
    wait(cx, window, most.saturating_sub(moved));
}

/// Waits `duration` in real time, a frame at a time, for a screen that measures real time: the performance readout
/// counts frames a second.
fn wait_real(cx: &mut VisualTestAppContext, window: AnyWindowHandle, duration: Duration) {
    let until = Instant::now() + duration;
    while Instant::now() < until {
        frame(cx, window);
        thread::sleep(STEP);
    }
}

/// Only the app's clock moves, so an animation can be caught partway.
fn wait_fake(cx: &mut VisualTestAppContext, window: AnyWindowHandle, duration: Duration) {
    cx.advance_clock(duration);
    cx.run_until_parked();
    let _ = cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx));
}

/// The middle of the element last drawn under `name` (`ui::spot`).
fn spot(name: &str) -> gpui::Point<gpui::Pixels> {
    wings_native::ui::spot_at(name)
        .unwrap_or_else(|| panic!("{name} wasn't drawn"))
        .center()
}

fn right_click(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    position: gpui::Point<gpui::Pixels>,
) {
    cx.simulate_event(
        window,
        MouseDownEvent {
            position,
            button: MouseButton::Right,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        },
    );
}

fn click(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    position: gpui::Point<gpui::Pixels>,
) {
    cx.simulate_event(
        window,
        MouseDownEvent {
            position,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        },
    );
    cx.simulate_event(
        window,
        gpui::MouseUpEvent {
            position,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: 1,
        },
    );
}

fn save(cx: &mut VisualTestAppContext, window: AnyWindowHandle, path: &Path) {
    // What this screen cuts off at its width, said under its PNG and again at the end of the run (`ui::cutoff`).
    let cuts = cx
        .update_window(window, |_, window, cx| wings_native::ui::cutoff::check(window, cx))
        .unwrap_or_default();
    let _ = cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx));
    cx.capture_screenshot(window)
        .expect("render the window")
        .save(path)
        .expect("save the PNG");
    println!("{}", path.display());
    wings_native::ui::cutoff::report(path, &cuts);
}

/// The desktop through the window's vibrancy, as the running app shows it under its glass: worked back from the
/// sidebar's 39,29,36 over this Mac's wallpaper. An offscreen window has none, and its glass lay on black.
const VIBRANCY: u32 = 0x39252f;

fn main() {
    wings_native::ui::set_screenshot_ground(VIBRANCY);
    scenes();
    wings_native::ui::cutoff::end_run();
}

fn scenes() {
    // `-- <out-dir> accounts` runs agents under a named account, whose folder is `~/.claude-<name>`: in a made-up home,
    // never the user's.
    if std::env::args().nth(2).as_deref() == Some("accounts") {
        let home = std::env::temp_dir().join("wings-native-shot-home");
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".claude-work/sessions")).expect("make a named account");
        // zsh asks a new user to set it up when it finds no startup file.
        std::fs::write(home.join(".zshrc"), "").expect("write an empty .zshrc");
        // SAFETY: nothing else runs yet to read the environment.
        unsafe { std::env::set_var("HOME", &home) };
    }
    let out = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "shots".into()));
    std::fs::create_dir_all(&out).expect("create the output folder");
    // Made-up projects for the sidebar, never the user's own list.
    let fake = std::env::temp_dir().join("wings-native-shot-projects");
    let _ = std::fs::remove_dir_all(&fake);
    for (folder, branch) in [
        ("acme-web", "main"),
        ("acme-api", "release-2"),
        ("acme-web-login", "fix-login"),
    ] {
        std::fs::create_dir_all(fake.join(folder).join(".git")).expect("make a fake project");
        std::fs::write(
            fake.join(folder).join(".git/HEAD"),
            format!("ref: refs/heads/{branch}\n"),
        )
        .expect("write a fake branch");
    }
    let spaces = format!(
        r#"[{{"id":"w","name":"acme-web","path":"{web}","pinned":true}},{{"id":"a","name":"acme-api","path":"{api}"}},{{"id":"l","name":"acme-web-login","path":"{login}","parent":"w"}}]"#,
        web = fake.join("acme-web").display(),
        api = fake.join("acme-api").display(),
        login = fake.join("acme-web-login").display(),
    );
    std::fs::write(fake.join("spaces.json"), spaces).expect("write the fake list");
    // A made-up `claude` (a renamed `sleep`) whose state comes from session files written below.
    let claude_dir = fake.join("claude");
    std::fs::create_dir_all(claude_dir.join("sessions")).expect("make a fake Claude folder");
    std::fs::create_dir_all(fake.join("bin")).expect("make a bin folder");
    let fake_claude = fake.join("bin/claude");
    std::os::unix::fs::symlink("/bin/sleep", &fake_claude).expect("make a fake claude");
    let project = fake.join("acme-web");
    let args = ["--project".into(), project.clone().into_os_string()];
    let mut config = Config::parse(args, &project)
        .expect("parse the options")
        .expect("options, not --help");
    // Renders never read or change the user's saved tabs, or ask the user's model for names and summaries.
    config.state = None;
    config.ai = None;
    config.semantic = None;
    config.projects = Some(fake.join("spaces.json"));
    config.agents = Some(claude_dir.clone());
    // The fixture plugin, installed in a made-up desktop data folder and turned on, never the user's plugins.
    let data = Data::temp();
    data.install("demo", WINGS_VERSION, true, true, Approval::Current, "shot");
    config.plugins = Some(data.paths());
    let mut cx = VisualTestAppContext::with_asset_source(
        gpui_platform::current_platform(false),
        std::sync::Arc::new(Assets),
    );
    cx.update(actions::bind);
    if std::env::args().nth(2).as_deref() == Some("plugin-updates") {
        for width in [1000.0, 640.0] {
            let config = config.clone();
            let handle = cx
                .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                    cx.new(|cx| Workspace::new(config, window, cx))
                })
                .expect("open plugin update fixture");
            let window: AnyWindowHandle = handle.into();
            for state in ["single", "stack", "available"] {
                handle
                    .update(&mut cx, |workspace, window, cx| {
                        workspace.sample_plugin_updates(state, window, cx)
                    })
                    .unwrap();
                settle(&mut cx, window, Duration::from_millis(300));
                save(
                    &mut cx,
                    window,
                    &out.join(format!("plugin-updates-{state}-{width}.png")),
                );
            }
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
        return;
    }
    if std::env::args().nth(2).as_deref() == Some("updater") {
        for width in [1000.0, 640.0] {
            let config = config.clone();
            let handle = cx
                .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                    cx.new(|cx| Workspace::new(config, window, cx))
                })
                .expect("open updater fixture");
            let window: AnyWindowHandle = handle.into();
            for state in [
                "checking",
                "current",
                "available",
                "server-note",
                "server-note-direct",
                "no-notes",
                "downloading",
                "verifying",
                "installing",
                "error",
            ] {
                handle
                    .update(&mut cx, |workspace, window, cx| {
                        workspace.sample_update(state, window, cx)
                    })
                    .unwrap();
                settle(&mut cx, window, Duration::from_millis(300));
                save(
                    &mut cx,
                    window,
                    &out.join(format!("updater-{state}-{width}.png")),
                );
            }
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
        return;
    }
    // `-- <out-dir> sidebar-update` renders only an update an automatic check found: its row above Settings and its
    // toast.
    if std::env::args().nth(2).as_deref() == Some("sidebar-update") {
        for width in [1000.0, 640.0] {
            let config = config.clone();
            let handle = cx
                .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                    cx.new(|cx| Workspace::new(config, window, cx))
                })
                .expect("open sidebar update fixture");
            let window: AnyWindowHandle = handle.into();
            handle
                .update(&mut cx, |workspace, window, cx| {
                    workspace.sample_update("found", window, cx)
                })
                .unwrap();
            wait(&mut cx, window, Duration::from_millis(300));
            save(
                &mut cx,
                window,
                &out.join(format!("sidebar-update-{width}.png")),
            );
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
        return;
    }
    // `-- <out-dir> semantic` renders the Settings row filling while past sessions are indexed (a third in, nearly done,
    // pointed at with its tooltip), and the index's switch in Settings, Agents.
    if std::env::args().nth(2).as_deref() == Some("semantic") {
        for width in [1000.0, 640.0] {
            let config = config.clone();
            let handle = cx
                .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                    cx.new(|cx| Workspace::new(config, window, cx))
                })
                .expect("open semantic fixture");
            let window: AnyWindowHandle = handle.into();
            settle(&mut cx, window, Duration::from_millis(300));
            for (state, done) in [("third", 310), ("nearly-done", 900)] {
                handle
                    .update(&mut cx, |workspace, _, cx| workspace.show_semantic_progress(done, 929, cx))
                    .unwrap();
                wait(&mut cx, window, Duration::from_millis(300));
                save(&mut cx, window, &out.join(format!("semantic-sidebar-{state}-{width}.png")));
            }
            // From elsewhere: the first move only finds what's under the pointer, on the next frame; the second starts
            // the tooltip's wait. On the label, which used to show nothing.
            let at = spot("open-settings");
            for to in [point(px(700.0), px(400.0)), at, point(at.x + px(1.0), at.y)] {
                mouse_move(&mut cx, window, to, false);
                wait(&mut cx, window, Duration::from_millis(100));
            }
            wait(&mut cx, window, Duration::from_millis(700));
            wait_fake(&mut cx, window, Duration::from_millis(200));
            save(&mut cx, window, &out.join(format!("semantic-sidebar-hover-{width}.png")));
            mouse_move(&mut cx, window, point(px(700.0), px(400.0)), false);
            handle
                .update(&mut cx, |workspace, window, cx| {
                    workspace.sample_semantic_settings(window, cx)
                })
                .unwrap();
            wait(&mut cx, window, Duration::from_millis(600));
            save(&mut cx, window, &out.join(format!("semantic-settings-{width}.png")));
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
        return;
    }
    // `-- <out-dir> sidebar` renders only the sidebar's states.
    if matches!(std::env::args().nth(2).as_deref(), Some("sidebar" | "sidebar-polish")) {
        return sidebar(&mut cx, &config, &out);
    }
    // `-- <out-dir> tab-drag` renders only dragging a tab along the title bar.
    if std::env::args().nth(2).as_deref() == Some("tab-drag") {
        return tab_drag(&mut cx, &config, &out);
    }
    // `-- <out-dir> themes` renders the window and Settings, Appearance in each theme, and with the glass off.
    if std::env::args().nth(2).as_deref() == Some("themes") {
        return themes(&mut cx, &config, &out);
    }
    // `-- <out-dir> parity` renders only Settings' sections, Open in editor, the welcome and the quick terminal.
    if std::env::args().nth(2).as_deref() == Some("parity") {
        return parity(&mut cx, &config, &out);
    }
    // `-- <out-dir> plugins-install` renders only managing plugins and the agent approvals.
    if std::env::args().nth(2).as_deref() == Some("plugins-install") {
        return plugins_install(&mut cx, &config, &out);
    }
    if std::env::args().nth(2).as_deref() == Some("--setup-existing-worktree") {
        return setup_existing_worktree(&mut cx, &config, &out);
    }
    // `-- <out-dir> plugins-drop` renders only dropping a .wings-plugin file on the window and a plugin's README.
    // `-- <out-dir> plugins-agent` renders only the sidebar's Agents with a plugin tab's agent, as Claude chat lists one.
    if std::env::args().nth(2).as_deref() == Some("plugins-agent") {
        return plugins_agent(&mut cx, &config, &out);
    }
    // `-- <out-dir> plugins-terminals` renders only a chat's page with the terminal its agent opened in its side panel, and
    // an issue key in its message as a link.
    if std::env::args().nth(2).as_deref() == Some("plugins-terminals") {
        return plugins_terminals(&mut cx, &config, &out);
    }
    // `-- <out-dir> plugins-modal` renders only a plugin's view that opens as a modal, over the window.
    if std::env::args().nth(2).as_deref() == Some("plugins-modal") {
        return plugins_modal(&mut cx, &config, &out);
    }
    if std::env::args().nth(2).as_deref() == Some("plugins-drop") {
        return plugins_drop(&mut cx, &config, &out);
    }
    // `-- <out-dir> finder` renders only the command palette and the history sheet.
    if std::env::args().nth(2).as_deref() == Some("finder") {
        return finder(&mut cx, &config, &out);
    }
    // `-- <out-dir> accounts` renders only the sidebar's agents under the default account and a named one.
    if std::env::args().nth(2).as_deref() == Some("accounts") {
        return accounts(&mut cx, &config, &fake_claude, &out);
    }
    // `-- <out-dir> disk-access` renders only the dialog that asks to turn on other apps' data, and its row in Settings.
    if std::env::args().nth(2).as_deref() == Some("disk-access") {
        return disk_access(&mut cx, &config, &fake, &out);
    }
    // `-- <out-dir> first-launch` renders only the first launch's question about the official plugins.
    if std::env::args().nth(2).as_deref() == Some("first-launch") {
        return first_launch(&mut cx, &config, &out);
    }
    // `-- <out-dir> older-servers` renders only a pane on a terminal server from before an update, the servers in
    // Settings, General, and the dialog that stops the older one.
    if std::env::args().nth(2).as_deref() == Some("older-servers") {
        return older_servers(&mut cx, &config, &fake, &out);
    }
    // `-- <out-dir> restored` renders only the Agents list after a restart: a live agent, one restored in a project not
    // opened yet, and one in a pane waiting for its drive to mount, with that pane.
    if std::env::args().nth(2).as_deref() == Some("restored") {
        return restored(&mut cx, &config, &fake, &fake_claude, &out);
    }
    // `-- <out-dir> images` renders only a pane with a SIXEL and an iTerm2 inline image a program printed.
    if std::env::args().nth(2).as_deref() == Some("images") {
        return images(&mut cx, &config, &fake, &out);
    }
    // `-- <out-dir> chrome` renders only the Inbox, the right-click menus and the tooltips.
    if std::env::args().nth(2).as_deref() == Some("chrome") {
        return chrome(&mut cx, &config, &out);
    }
    // `-- <out-dir> readout` renders only the performance readout, turned on from the command palette.
    if std::env::args().nth(2).as_deref() == Some("readout") {
        return readout(&mut cx, &config, &out);
    }
    // `-- <out-dir> icons` renders only a plugin's icons: on its sidebar, its pages, its Settings row and its block.
    if std::env::args().nth(2).as_deref() == Some("icons") {
        return icons(&mut cx, &config, &out);
    }
    // `-- <out-dir> diff-dock` renders only a plugin's diff, docked beside its tab and filling the window.
    if std::env::args().nth(2).as_deref() == Some("diff-dock") {
        return diff_dock(&mut cx, &config, &out);
    }
    // `-- <out-dir> diff-sidebar` renders only a diff opened from a plugin's sidebar: widened in the sidebar's place over
    // the panes at 1280, 1000 and 640 points, and the sidebar again after Back.
    if std::env::args().nth(2).as_deref() == Some("diff-sidebar") {
        return diff_sidebar(&mut cx, &config, &out);
    }
    // `-- <out-dir> right-splits` renders only two sidebar pages split in the right sidebar at 1280 and 640 points.
    if std::env::args().nth(2).as_deref() == Some("right-splits") {
        return right_splits(&mut cx, &config, &out);
    }
    // `-- <out-dir> pinned-bar` renders only the project bar with plugin buttons pinned: few, and many with long titles,
    // at 1600, 1000 and 640 points, with the right panel open.
    if std::env::args().nth(2).as_deref() == Some("pinned-bar") {
        return pinned_bar(&mut cx, &config, &out);
    }
    // `-- <out-dir> right-named` renders only a sidebar whose page names its own header (`setTabTitle`), at 1280 and 640.
    if std::env::args().nth(2).as_deref() == Some("right-named") {
        return right_named(&mut cx, &config, &out);
    }
    // `-- <out-dir> diff-comments` renders only a plugin's diff with review threads: under their lines docked and filling
    // the window, a reply being written, and the discussion.
    if std::env::args().nth(2).as_deref() == Some("diff-comments") {
        return diff_comments(&mut cx, &config, &out);
    }
    // `-- <out-dir> agent-terminals` renders only the terminals an agent opened: below it, floating over it, hidden,
    // to its right, and Settings, Agents.
    if std::env::args().nth(2).as_deref() == Some("agent-terminals") {
        return agent_terminals(&mut cx, &config, &fake_claude, &claude_dir, &out);
    }
    // `-- <out-dir> plugin-settings` renders only Settings showing plugins' own settings pages, and Plugins beside them.
    if std::env::args().nth(2).as_deref() == Some("plugin-settings") {
        return plugin_settings(&mut cx, &config, &out);
    }
    // A page runs in a plugin's host, which these two have none of: an empty one stands for it.
    let pages = PluginHost::new(PluginConfig {
        id: "pages".into(),
        root: out.join("fixture-storage-pages"),
        scope: "shot".into(),
    });
    // The pilot React page, drawn with the native controls, before and after a click on its button.
    for width in [1000.0, 640.0] {
        let handle = cx
            .open_offscreen_window(size(px(width), px(320.0)), |_, cx| {
                cx.new(|cx| PageView::in_plugin(&pages, &pilot(), cx))
            })
            .expect("open an offscreen window");
        let window: AnyWindowHandle = handle.into();
        wait(&mut cx, window, Duration::from_millis(500));
        save(&mut cx, window, &out.join(format!("pilot-{width}.png")));
        // Inside the button: under the 44px toolbar and 14px of padding.
        click(&mut cx, window, point(px(30.0), px(44.0 + 14.0 + 12.0)));
        wait(&mut cx, window, Duration::from_millis(300));
        save(
            &mut cx,
            window,
            &out.join(format!("pilot-clicked-{width}.png")),
        );
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
    // The components page: all of wings-plugin-ui/native's components, then with its disclosure open and with its select's
    // menu open. 260 is a sidebar's width, where the HUD's page lives.
    for width in [1000.0, 640.0, 260.0] {
        let handle = cx
            .open_offscreen_window(size(px(width), px(900.0)), |_, cx| {
                cx.new(|cx| PageView::in_plugin(&pages, &components(), cx))
            })
            .expect("open an offscreen window");
        let window: AnyWindowHandle = handle.into();
        settle(&mut cx, window, Duration::from_millis(500));
        save(
            &mut cx,
            window,
            &out.join(format!("components-{width}.png")),
        );
        // Under the 44px toolbar and 14px of padding: the refresh row, then the select, then the disclosure's line.
        let disclosure = point(px(40.0), px(44.0 + 14.0 + 32.0 + 10.0 + 32.0 + 10.0 + 8.0));
        click(&mut cx, window, disclosure);
        wait(&mut cx, window, Duration::from_millis(300));
        save(
            &mut cx,
            window,
            &out.join(format!("components-open-{width}.png")),
        );
        click(&mut cx, window, disclosure);
        click(
            &mut cx,
            window,
            point(px(60.0), px(44.0 + 14.0 + 32.0 + 10.0 + 16.0)),
        );
        wait(&mut cx, window, Duration::from_millis(300));
        save(
            &mut cx,
            window,
            &out.join(format!("components-select-{width}.png")),
        );
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
    for width in [1000.0, 640.0] {
        let config = config.clone();
        let handle = cx
            .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                cx.new(|cx| Workspace::new(config, window, cx))
            })
            .expect("open an offscreen window");
        let window: AnyWindowHandle = handle.into();
        let name = |state: &str| out.join(format!("{state}-{width}.png"));
        settle(&mut cx, window, Duration::from_secs(4));
        cx.simulate_input(
            window,
            "printf '\\033]0;build-server\\007'; seq 1 30 | sed s/^/needle-/",
        );
        cx.simulate_keystrokes(window, "enter");
        wait(&mut cx, window, Duration::from_secs(1));
        cx.simulate_keystrokes(window, "cmd-t cmd-t");
        settle(&mut cx, window, Duration::from_secs(4));
        save(&mut cx, window, &name("tabs"));
        // The sidebar's Terminal row, twice: the second fills the pane area, both listed under the nav, the first with
        // the pointer on it. Their close buttons then end both, and the keys go back to the tab.
        let terminal_row = |index: f32| {
            point(
                px(60.0),
                px(40.0 + 4.0 * 29.0 - 1.0 + 8.0 + 14.0 + index * 29.0),
            )
        };
        click(&mut cx, window, point(px(60.0), px(40.0 + 29.0 + 14.0)));
        settle(&mut cx, window, Duration::from_millis(500));
        click(&mut cx, window, point(px(60.0), px(40.0 + 29.0 + 14.0)));
        settle(&mut cx, window, Duration::from_secs(1));
        mouse_move(&mut cx, window, terminal_row(0.0), false);
        settle(&mut cx, window, Duration::from_millis(200));
        save(&mut cx, window, &name("sidebar-terminals"));
        for _ in 0..2 {
            click(&mut cx, window, point(px(226.0), terminal_row(0.0).y));
            wait(&mut cx, window, Duration::from_millis(200));
        }
        // Text styles, the palette, 256 and true colors, wide and combining characters, and a selection dragged from
        // the second row into the third.
        cx.simulate_input(
            window,
            "clear; printf '\\033[1mbold\\033[0m \\033[3mitalic\\033[0m \\033[4munder\\033[0m \\033[9mstrike\\033[0m \\033[7minverse\\033[0m \\033[2mdim\\033[0m \\033[31mred\\033[0m \\033[92mgreen\\033[0m\\n'; for i in $(seq 0 15); do printf \"\\033[48;5;${i}m  \"; done; printf '\\033[0m\\n\\033[38;2;255;120;0mtrue color\\033[0m \\033[38;5;75m256 color\\033[0m 漢字 e\\314\\201 ─┼─\\n'",
        );
        cx.simulate_keystrokes(window, "enter");
        settle(&mut cx, window, Duration::from_secs(1));
        cx.simulate_event(
            window,
            MouseDownEvent {
                position: point(px(300.0), px(125.0)),
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            },
        );
        mouse_move(&mut cx, window, point(px(420.0), px(143.0)), true);
        cx.simulate_event(
            window,
            gpui::MouseUpEvent {
                position: point(px(420.0), px(143.0)),
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
                click_count: 1,
            },
        );
        wait(&mut cx, window, Duration::from_millis(300));
        save(&mut cx, window, &name("terminal"));
        cx.simulate_keystrokes(window, "cmd-t cmd-t cmd-t cmd-t cmd-t cmd-t cmd-t");
        settle(&mut cx, window, Duration::from_secs(4));
        save(&mut cx, window, &name("tabs-overflow"));
        cx.simulate_keystrokes(window, "cmd-d cmd-shift-d");
        settle(&mut cx, window, Duration::from_secs(4));
        save(&mut cx, window, &name("splits"));
        // Double-click the focused pane's title (the bottom-right one) to rename it.
        let title = point(
            px(width / 2.0 + 40.0),
            // Below the 40px title bar and 36px project bar, above the 8px padding at the bottom.
            px(76.0 + (640.0 - 76.0 - 8.0) / 2.0 + 4.0 + 15.0),
        );
        cx.simulate_event(
            window,
            MouseDownEvent {
                position: title,
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
                click_count: 2,
                first_mouse: false,
            },
        );
        wait(&mut cx, window, Duration::from_millis(200));
        cx.simulate_input(window, "dev server");
        wait(&mut cx, window, Duration::from_millis(200));
        save(&mut cx, window, &name("rename"));
        cx.simulate_keystrokes(window, "enter");
        settle(&mut cx, window, Duration::from_millis(200));
        let _ = cx.update_window(window, |_, window, cx| {
            window.dispatch_action(Box::new(actions::ZoomPane), cx)
        });
        settle(&mut cx, window, Duration::from_millis(300));
        save(&mut cx, window, &name("zoom"));
        let _ = cx.update_window(window, |_, window, cx| {
            window.dispatch_action(Box::new(actions::ZoomPane), cx)
        });
        cx.simulate_keystrokes(window, "cmd-1 cmd-f");
        wait(&mut cx, window, Duration::from_millis(200));
        cx.simulate_input(window, "needle-2");
        wait(&mut cx, window, Duration::from_millis(500));
        save(&mut cx, window, &name("find"));
        cx.simulate_keystrokes(window, "escape cmd-1 cmd-[");
        wait(&mut cx, window, Duration::from_millis(300));
        save(&mut cx, window, &name("issues"));
        // The fixture plugin's page, opened from the palette, before and after a click on its button, which asks the
        // plugin's main to add one.
        cx.simulate_keystrokes(window, "cmd-k");
        wait(&mut cx, window, Duration::from_millis(200));
        cx.simulate_input(window, "New tab: Fixture");
        wait(&mut cx, window, Duration::from_millis(200));
        cx.simulate_keystrokes(window, "enter");
        settle(&mut cx, window, Duration::from_millis(800));
        save(&mut cx, window, &name("plugin-tab"));
        // Right of the 248 wide sidebar, under the title bar, project bar, the page's toolbar and its padding.
        click(
            &mut cx,
            window,
            point(px(270.0), px(40.0 + 36.0 + 44.0 + 14.0 + 16.0)),
        );
        wait(&mut cx, window, Duration::from_millis(500));
        save(&mut cx, window, &name("plugin-tab-clicked"));
        cx.simulate_keystrokes(window, "cmd-w");
        wait(&mut cx, window, Duration::from_millis(200));
        // One agent working in the first tab, one waiting for you in the split tab.
        let sessions = [
            (
                "cmd-1",
                120,
                r#"{"status":"busy","name":"Refactor the API client"}"#,
            ),
            (
                "cmd-3",
                121,
                r#"{"status":"waiting","name":"Fix the login redirect"}"#,
            ),
        ];
        for (tab, seconds, _) in sessions {
            cx.simulate_keystrokes(window, tab);
            settle(&mut cx, window, Duration::from_millis(200));
            cx.simulate_input(
                window,
                &format!("exec '{}' {seconds}", fake_claude.display()),
            );
            cx.simulate_keystrokes(window, "enter");
        }
        wait(&mut cx, window, Duration::from_secs(1));
        for (_, seconds, session) in sessions {
            let pids = std::process::Command::new("pgrep")
                .args(["-f", &format!("{} {seconds}", fake_claude.display())])
                .output()
                .expect("run pgrep");
            for pid in String::from_utf8_lossy(&pids.stdout).split_whitespace() {
                std::fs::write(claude_dir.join(format!("sessions/{pid}.json")), session)
                    .expect("write a session file");
            }
        }
        settle(&mut cx, window, Duration::from_secs(2));
        save(&mut cx, window, &name("agents"));
        cx.simulate_keystrokes(window, "cmd-,");
        wait(&mut cx, window, Duration::from_millis(300));
        save(&mut cx, window, &name("settings"));
        // The dialog is centred, 820 wide at most and 32px clear of each edge.
        let left = (width - 820.0f32.min(width - 64.0)) / 2.0;
        let right = left + 820.0f32.min(width - 64.0);
        // The font field: a click in it, a name typed, the caret moved back into the name and then a part of it selected
        // with Shift. It is at the card's right edge, and lower at 640, where the description wraps.
        let field = point(
            px(right - 131.0),
            px(if width < 800.0 { 154.0 } else { 125.0 }),
        );
        click(&mut cx, window, field);
        cx.simulate_input(window, "JetBrains Mono");
        cx.simulate_keystrokes(window, "left left left left left left");
        wait(&mut cx, window, Duration::from_millis(200));
        save(&mut cx, window, &name("settings-caret"));
        cx.simulate_keystrokes(
            window,
            "shift-left shift-left shift-left shift-left shift-left",
        );
        settle(&mut cx, window, Duration::from_millis(200));
        save(&mut cx, window, &name("settings-selection"));
        // The Cursor section, third in the list, with its style menu open.
        click(&mut cx, window, point(px(left + 70.0), px(201.0)));
        settle(&mut cx, window, Duration::from_millis(200));
        // The description wraps at 640, so the row and its select sit lower.
        let select_y = if width < 800.0 { 155.0 } else { 125.0 };
        click(&mut cx, window, point(px(right - 120.0), px(select_y)));
        wait(&mut cx, window, Duration::from_millis(200));
        save(&mut cx, window, &name("settings-cursor"));
        cx.simulate_keystrokes(window, "escape");
        let _ = std::process::Command::new("pkill")
            .args(["-f", &fake_claude.display().to_string()])
            .status();
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
        wait(&mut cx, window, Duration::from_millis(300));
    }
    sidebar(&mut cx, &config, &out);
    tab_drag(&mut cx, &config, &out);
}
