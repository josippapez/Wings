//! Renders the native window offscreen at 1000 and 640 points wide and saves each state as a PNG:
//! tabs, splits, renaming a pane, a zoomed pane, find, Issues, a plugin page, agents and Settings, and the pages of
//! `pages/` on their own: the pilot and the components page.
//! Nothing shows on screen and focus doesn't move. macOS only (real Metal rendering).
//!
//! cargo run --manifest-path apps/native/Cargo.toml --example shot --features test-support -- <out-dir>

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
    host::Config,
    icons::Assets,
    installed::{
        WINGS_VERSION,
        fixture::{Approval, Data},
    },
    page::PageView,
    workspace::Workspace,
};

/// Real shells print on their own time, so this waits in real time while the test clock keeps up.
fn wait(cx: &mut VisualTestAppContext, window: AnyWindowHandle, duration: Duration) {
    let until = Instant::now() + duration;
    while Instant::now() < until {
        cx.run_until_parked();
        cx.advance_clock(Duration::from_millis(20));
        let _ = cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx));
        thread::sleep(Duration::from_millis(20));
    }
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
    let _ = cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx));
    cx.capture_screenshot(window)
        .expect("render the window")
        .save(path)
        .expect("save the PNG");
    println!("{}", path.display());
}

fn main() {
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
    // Renders never read or change the user's saved tabs.
    config.state = None;
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
    // The pilot React page, drawn with the native controls, before and after a click on its button.
    for width in [1000.0, 640.0] {
        let handle = cx
            .open_offscreen_window(size(px(width), px(320.0)), |_, cx| {
                cx.new(|cx| PageView::new(include_str!("../pages/dist/pilot.js"), cx))
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
                cx.new(|cx| PageView::new(include_str!("../pages/dist/components-page.js"), cx))
            })
            .expect("open an offscreen window");
        let window: AnyWindowHandle = handle.into();
        wait(&mut cx, window, Duration::from_millis(500));
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
        wait(&mut cx, window, Duration::from_secs(4));
        cx.simulate_input(
            window,
            "printf '\\033]0;build-server\\007'; seq 1 30 | sed s/^/needle-/",
        );
        cx.simulate_keystrokes(window, "enter");
        wait(&mut cx, window, Duration::from_secs(1));
        cx.simulate_keystrokes(window, "cmd-t cmd-t");
        wait(&mut cx, window, Duration::from_secs(4));
        save(&mut cx, window, &name("tabs"));
        cx.simulate_keystrokes(window, "cmd-d cmd-shift-d");
        wait(&mut cx, window, Duration::from_secs(4));
        save(&mut cx, window, &name("splits"));
        // Double-click the focused pane's title (the bottom-right one) to rename it.
        let title = point(
            px(width / 2.0 + 40.0),
            // Below the 40px title bar and 36px project bar, inside the 8px padding.
            px(76.0 + 8.0 + (640.0 - 76.0 - 16.0) / 2.0 + 4.0 + 15.0),
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
        wait(&mut cx, window, Duration::from_millis(200));
        let _ = cx.update_window(window, |_, window, cx| {
            window.dispatch_action(Box::new(actions::ZoomPane), cx)
        });
        wait(&mut cx, window, Duration::from_millis(300));
        save(&mut cx, window, &name("zoom"));
        let _ = cx.update_window(window, |_, window, cx| {
            window.dispatch_action(Box::new(actions::ZoomPane), cx)
        });
        cx.simulate_keystrokes(window, "cmd-1 cmd-f");
        wait(&mut cx, window, Duration::from_millis(200));
        cx.simulate_input(window, "needle-2");
        wait(&mut cx, window, Duration::from_millis(500));
        save(&mut cx, window, &name("find"));
        cx.simulate_keystrokes(window, "escape cmd-4");
        wait(&mut cx, window, Duration::from_millis(300));
        save(&mut cx, window, &name("issues"));
        // The fixture plugin's page from the sidebar's Plugins list (above Settings in the 640 high window), before and after a
        // click on its button, which asks the plugin's main to add one.
        click(&mut cx, window, point(px(60.0), px(580.0)));
        wait(&mut cx, window, Duration::from_millis(800));
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
            wait(&mut cx, window, Duration::from_millis(200));
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
        wait(&mut cx, window, Duration::from_secs(2));
        save(&mut cx, window, &name("agents"));
        cx.simulate_keystrokes(window, "cmd-,");
        wait(&mut cx, window, Duration::from_millis(300));
        save(&mut cx, window, &name("settings"));
        // The Cursor section, with its style menu open. The dialog is centred, 820 wide at most.
        let left = (width - 820.0f32.min(width * 0.92)) / 2.0;
        let right = left + 820.0f32.min(width * 0.92);
        click(&mut cx, window, point(px(left + 70.0), px(184.0)));
        wait(&mut cx, window, Duration::from_millis(200));
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
}
