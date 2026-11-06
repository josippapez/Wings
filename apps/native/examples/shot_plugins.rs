//! Renders Worktrees' installed native sidebar offscreen at 1000 and 640 points wide. The plugin installation and
//! project are made up in a temporary desktop data folder, never the user's.
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
            Access, Env, PanePort, PluginConfig, PluginHost,
            ui::{Badge, PaneStatus},
        },
    },
    workspace::Workspace,
};

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

struct ChangesEnv(PathBuf);
impl Env for ChangesEnv {
    fn projects(&self) -> Vec<PathBuf> {
        vec![self.0.clone()]
    }
    fn claude_dir(&self) -> PathBuf {
        self.0.clone()
    }
    fn ports(&self) -> Result<Vec<PanePort>, String> {
        Ok(vec![])
    }
    fn open_url(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn notify(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn set_badge(&self, _: &str, _: &str, _: Option<Badge>) {}
    fn set_pane_status(&self, _: &str, _: &str, _: Option<PaneStatus>) {}
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
            env: std::sync::Arc::new(ChangesEnv(root.clone())),
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
