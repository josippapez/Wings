//! Renders the native window offscreen at 1000 and 640 points wide and saves each state as a PNG:
//! tabs and dragging one, a terminal's colors and a selection, splits, renaming a pane, a zoomed pane, find, Issues, a
//! plugin page, agents and Settings, and the pages of `pages/` on their own: the pilot and the components page.
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
    host::{Config, SessionSource},
    icons::Assets,
    installed::{
        WINGS_VERSION,
        fixture::{Approval, Data},
    },
    page::PageView,
    workspace::{AccountStatus, Workspace},
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
    let _ = cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx));
    cx.capture_screenshot(window)
        .expect("render the window")
        .save(path)
        .expect("save the PNG");
    println!("{}", path.display());
}

fn main() {
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
                wait(&mut cx, window, Duration::from_millis(300));
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
                wait(&mut cx, window, Duration::from_millis(300));
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
    // `-- <out-dir> sidebar` renders only the sidebar's states.
    if std::env::args().nth(2).as_deref() == Some("sidebar") {
        return sidebar(&mut cx, &config, &out);
    }
    // `-- <out-dir> tab-drag` renders only dragging a tab along the title bar.
    if std::env::args().nth(2).as_deref() == Some("tab-drag") {
        return tab_drag(&mut cx, &config, &out);
    }
    // `-- <out-dir> parity` renders only Settings' sections, Open in editor, the welcome and the quick terminal.
    if std::env::args().nth(2).as_deref() == Some("parity") {
        return parity(&mut cx, &config, &out);
    }
    // `-- <out-dir> plugins-install` renders only managing plugins and the agent approvals.
    if std::env::args().nth(2).as_deref() == Some("plugins-install") {
        return plugins_install(&mut cx, &config, &out);
    }
    // `-- <out-dir> plugins-drop` renders only dropping a .wings-plugin file on the window and a plugin's README.
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
    // `-- <out-dir> disk-access` renders only the dialog that asks for Full Disk Access, and its row in Settings.
    if std::env::args().nth(2).as_deref() == Some("disk-access") {
        return disk_access(&mut cx, &config, &fake, &out);
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
    // `-- <out-dir> icons` renders only a plugin's icons: on its sidebar, its pages, its Settings row and its block.
    if std::env::args().nth(2).as_deref() == Some("icons") {
        return icons(&mut cx, &config, &out);
    }
    // `-- <out-dir> diff-dock` renders only a plugin's diff, docked beside its tab and filling the window.
    if std::env::args().nth(2).as_deref() == Some("diff-dock") {
        return diff_dock(&mut cx, &config, &out);
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
        // The sidebar's Terminal row, twice: the second fills the pane area, both listed under the nav, the first with
        // the pointer on it. Their close buttons then end both, and the keys go back to the tab.
        let terminal_row = |index: f32| {
            point(
                px(60.0),
                px(40.0 + 4.0 * 29.0 - 1.0 + 8.0 + 14.0 + index * 29.0),
            )
        };
        click(&mut cx, window, point(px(60.0), px(40.0 + 29.0 + 14.0)));
        wait(&mut cx, window, Duration::from_millis(500));
        click(&mut cx, window, point(px(60.0), px(40.0 + 29.0 + 14.0)));
        wait(&mut cx, window, Duration::from_secs(1));
        mouse_move(&mut cx, window, terminal_row(0.0), false);
        wait(&mut cx, window, Duration::from_millis(200));
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
        wait(&mut cx, window, Duration::from_secs(1));
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
        wait(&mut cx, window, Duration::from_secs(4));
        save(&mut cx, window, &name("tabs-overflow"));
        cx.simulate_keystrokes(window, "cmd-d cmd-shift-d");
        wait(&mut cx, window, Duration::from_secs(4));
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
        // The fixture plugin's page, opened from the palette, before and after a click on its button, which asks the
        // plugin's main to add one.
        cx.simulate_keystrokes(window, "cmd-k");
        wait(&mut cx, window, Duration::from_millis(200));
        cx.simulate_input(window, "New tab: Fixture");
        wait(&mut cx, window, Duration::from_millis(200));
        cx.simulate_keystrokes(window, "enter");
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
        wait(&mut cx, window, Duration::from_millis(200));
        save(&mut cx, window, &name("settings-selection"));
        // The Cursor section, third in the list, with its style menu open.
        click(&mut cx, window, point(px(left + 70.0), px(201.0)));
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
    sidebar(&mut cx, &config, &out);
    tab_drag(&mut cx, &config, &out);
}

/// Dragging a tab along the title bar: four terminals, the shown one, last, dragged most of the way onto its neighbour,
/// which has made room under it, then let go there. At 640 the strip is scrolled to it.
fn tab_drag(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
    for width in [1000.0, 640.0] {
        let config = base.clone();
        let handle = cx
            .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                cx.new(|cx| Workspace::new(config, window, cx))
            })
            .expect("open an offscreen window");
        let window: AnyWindowHandle = handle.into();
        let name = |state: &str| out.join(format!("tab-{state}-{width}.png"));
        wait(cx, window, Duration::from_secs(2));
        cx.simulate_keystrokes(window, "cmd-t cmd-t cmd-t");
        wait(cx, window, Duration::from_secs(2));
        let (from, neighbour) = (spot("tab:Terminal(3)"), spot("tab:Terminal(2)"));
        let to = point(neighbour.x - px(24.0), from.y);
        mouse_move(cx, window, from, false);
        wait(cx, window, Duration::from_millis(100));
        cx.simulate_event(
            window,
            MouseDownEvent {
                position: from,
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            },
        );
        for step in 1..=8 {
            let x = from.x + (to.x - from.x) * (step as f32 / 8.0);
            mouse_move(cx, window, point(x, from.y), true);
            wait(cx, window, Duration::from_millis(20));
        }
        save(cx, window, &name("dragging"));
        cx.simulate_event(
            window,
            gpui::MouseUpEvent {
                position: to,
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
                click_count: 1,
            },
        );
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("dragged"));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
}

fn git(dir: &Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("run git")
        .success();
    assert!(ok, "git {args:?} in {}", dir.display());
}

fn mouse_move(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    position: gpui::Point<gpui::Pixels>,
    pressed: bool,
) {
    cx.simulate_event(
        window,
        gpui::MouseMoveEvent {
            position,
            pressed_button: pressed.then_some(MouseButton::Left),
            modifiers: Modifiers::default(),
        },
    );
}

/// The sidebar: git counts from real temp repos (ahead, behind, changed, a worktree), a pinned project, the "Older"
/// group, hovering a row, dragging a project up the list and its Move up and Move down, the sort menu, the question
/// before a project with tabs is removed, dragging the edge to both limits, and the peek of a closed sidebar.
fn sidebar(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
    let day = 24 * 60 * 60 * 1000u64;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock")
        .as_millis() as u64;
    let dir = std::env::temp_dir().join("wings-native-shot-sidebar");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("make the folder");
    // A repo cloned from its own bare origin, so it can be ahead of or behind it.
    let clone = |name: &str, branch: &str| {
        let origin = dir.join(format!("{name}-origin.git"));
        std::fs::create_dir_all(&origin).unwrap();
        git(&origin, &["init", "--bare", "-b", "main"]);
        let repo = dir.join(name);
        git(&dir, &["clone", origin.to_str().unwrap(), name]);
        std::fs::write(repo.join("README.md"), "hello\n").unwrap();
        std::fs::write(repo.join("lib.rs"), "fn main() {}\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "first"]);
        git(&repo, &["push", "-u", "origin", "main"]);
        if branch != "main" {
            git(&repo, &["checkout", "-b", branch]);
            git(&repo, &["push", "-u", "origin", branch]);
        }
        (origin, repo)
    };
    // Ahead by one commit, four files changed.
    let (_, web) = clone("acme-web", "main");
    std::fs::write(web.join("lib.rs"), "fn main() { run(); }\n").unwrap();
    git(&web, &["commit", "-am", "run it"]);
    for name in ["a.txt", "b.txt", "c.txt"] {
        std::fs::write(web.join(name), "x").unwrap();
    }
    std::fs::write(web.join("README.md"), "changed\n").unwrap();
    // Behind by one commit as of the last fetch, two files changed.
    let (api_origin, api) = clone("acme-api", "release-2");
    let other = dir.join("api-other");
    git(
        &dir,
        &[
            "clone",
            "-b",
            "release-2",
            api_origin.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    std::fs::write(other.join("new.rs"), "x").unwrap();
    git(&other, &["add", "."]);
    git(&other, &["commit", "-m", "elsewhere"]);
    git(&other, &["push"]);
    git(&api, &["fetch"]);
    std::fs::write(api.join("lib.rs"), "changed\n").unwrap();
    std::fs::write(api.join("scratch.txt"), "x").unwrap();
    // A worktree of the first, titled by its branch because its folder is not the branch's name.
    let tree = dir.join("acme-web-login");
    git(
        &web,
        &["worktree", "add", tree.to_str().unwrap(), "-b", "fix-login"],
    );
    std::fs::write(tree.join("login.rs"), "x").unwrap();
    // A long name with many changes, to see them truncate, and a folder outside any repo.
    let long = dir.join("a-project-with-a-very-long-name-indeed");
    std::fs::create_dir_all(&long).unwrap();
    git(&long, &["init", "-b", "main"]);
    git(&long, &["commit", "--allow-empty", "-m", "first"]);
    for n in 0..128 {
        std::fs::write(long.join(format!("file-{n}.txt")), "x").unwrap();
    }
    let docs = dir.join("docs");
    std::fs::create_dir_all(&docs).unwrap();
    // Two projects nobody touched for five days.
    let mut stale = Vec::new();
    for name in ["old-notes", "legacy-site"] {
        let repo = dir.join(name);
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-b", "main"]);
        git(&repo, &["commit", "--allow-empty", "-m", "first"]);
        stale.push(repo);
    }
    let space = |path: &Path, extra: &str, added: u64| {
        format!(
            r#"{{"id":"{p}","name":"{n}","path":"{p}","addedAt":{added}{extra}}}"#,
            p = path.display(),
            n = path.file_name().unwrap().to_string_lossy()
        )
    };
    let spaces = [
        space(&api, "", now - 9 * day),
        space(&web, r#","pinned":true"#, now - 8 * day),
        space(
            &tree,
            &format!(r#","parent":"{}""#, web.display()),
            now - 2 * day,
        ),
        space(&docs, "", now - 7 * day),
        space(&long, "", now - 6 * day),
        space(&stale[0], "", now - 20 * day),
        space(&stale[1], "", now - 21 * day),
    ];
    std::fs::write(dir.join("spaces.json"), format!("[{}]", spaces.join(","))).unwrap();
    let times: Vec<String> = [&api, &web, &tree, &docs, &long]
        .iter()
        .map(|p| format!(r#""{}":{}"#, p.display(), now - day / 2))
        .chain(
            stale
                .iter()
                .map(|p| format!(r#""{}":{}"#, p.display(), now - 5 * day)),
        )
        .collect();
    std::fs::write(
        dir.join("workspace.json"),
        format!(
            r#"{{"version":1,"last_active":{{{}}},"projects":{{}}}}"#,
            times.join(",")
        ),
    )
    .unwrap();
    for width in [1000.0, 640.0] {
        let mut config = base.clone();
        config.project = api.clone();
        config.agents = None;
        config.plugins = None;
        // Each size starts from the same files.
        for file in ["workspace", "spaces"] {
            std::fs::copy(
                dir.join(format!("{file}.json")),
                dir.join(format!("{file}-{width}.json")),
            )
            .unwrap();
        }
        config.state = Some(dir.join(format!("workspace-{width}.json")));
        config.projects = Some(dir.join(format!("spaces-{width}.json")));
        let handle = cx
            .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                cx.new(|cx| Workspace::new(config, window, cx))
            })
            .expect("open an offscreen window");
        let window: AnyWindowHandle = handle.into();
        let name = |state: &str| out.join(format!("sidebar-{state}-{width}.png"));
        wait(cx, window, Duration::from_secs(3));
        save(cx, window, &name("counts"));
        // Pointing at the shown project's row: its buttons replace the pin. Where things are comes from where they were
        // last drawn, not from coordinates that go stale as the sidebar changes.
        let row = spot("project:acme-api");
        mouse_move(cx, window, point(px(120.0), row.y), false);
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("hover"));
        // Its X asks first, as it has a tab open; halfway through going away after Escape, then gone.
        click(cx, window, spot("project-remove:acme-api"));
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("remove"));
        cx.simulate_keystrokes(window, "escape");
        wait_fake(cx, window, Duration::from_millis(50));
        save(cx, window, &name("remove-closing"));
        mouse_move(cx, window, point(px(500.0), px(300.0)), false);
        wait(cx, window, Duration::from_millis(300));
        // In the Manual sort, docs dragged up most of the way to acme-api: it follows the pointer, past acme-api's
        // middle, and acme-api has made room under it. Then let go there.
        let (from, to) = (spot("project:docs"), spot("project:acme-api"));
        let to = point(from.x, to.y - px(6.0));
        mouse_move(cx, window, from, false);
        wait(cx, window, Duration::from_millis(100));
        cx.simulate_event(
            window,
            MouseDownEvent {
                position: from,
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            },
        );
        for step in 1..=8 {
            let y = from.y + (to.y - from.y) * (step as f32 / 8.0);
            mouse_move(cx, window, point(from.x, y), true);
            wait(cx, window, Duration::from_millis(20));
        }
        save(cx, window, &name("dragging"));
        cx.simulate_event(
            window,
            gpui::MouseUpEvent {
                position: to,
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
                click_count: 1,
            },
        );
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("dragged"));
        // Its menu: Move up stops at the first project that isn't pinned, Move down goes on.
        right_click(cx, window, spot("project:docs"));
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("menu-move"));
        click(cx, window, point(px(700.0), px(600.0)));
        mouse_move(cx, window, point(px(500.0), px(300.0)), false);
        wait(cx, window, Duration::from_millis(300));
        // The sort menu, from the header's middle button, then Name, its last item.
        click(cx, window, spot("sidebar-sort"));
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("sort"));
        click(cx, window, spot("sort:Name"));
        wait_fake(cx, window, Duration::from_millis(50));
        save(cx, window, &name("sort-closing"));
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("sorted"));
        // The project's menu, and its Worktree setup.
        right_click(cx, window, spot("project:acme-api"));
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("menu"));
        click(cx, window, spot("menu:Worktree setup…"));
        wait(cx, window, Duration::from_millis(400));
        save(cx, window, &name("worktree-setup"));
        cx.simulate_keystrokes(window, "escape");
        wait(cx, window, Duration::from_millis(300));
        // "Older" is closed under the list; open it.
        click(cx, window, spot("older"));
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("older"));
        // The edge dragged out past the widest, and in past the narrowest.
        for (state, to) in [("wide", 700.0), ("narrow", 100.0)] {
            let from = if state == "wide" { 247.0 } else { 419.0 };
            cx.simulate_event(
                window,
                MouseDownEvent {
                    position: point(px(from), px(300.0)),
                    button: MouseButton::Left,
                    modifiers: Modifiers::default(),
                    click_count: 1,
                    first_mouse: false,
                },
            );
            for step in 1..=10 {
                let x = from + (to - from) * step as f32 / 10.0;
                mouse_move(cx, window, point(px(x), px(300.0)), true);
                wait(cx, window, Duration::from_millis(20));
            }
            cx.simulate_event(
                window,
                gpui::MouseUpEvent {
                    position: point(px(to), px(300.0)),
                    button: MouseButton::Left,
                    modifiers: Modifiers::default(),
                    click_count: 1,
                },
            );
            wait(cx, window, Duration::from_millis(300));
            save(cx, window, &name(state));
        }
        // Closing, halfway through its slide, then closed, and the pointer at the window's edge.
        cx.simulate_keystrokes(window, "cmd-b");
        wait(cx, window, Duration::from_millis(100));
        save(cx, window, &name("sliding"));
        wait(cx, window, Duration::from_millis(400));
        mouse_move(cx, window, point(px(3.0), px(300.0)), false);
        wait(cx, window, Duration::from_millis(500));
        save(cx, window, &name("peek"));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
        wait(cx, window, Duration::from_millis(300));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The command palette and the history sheet, over made-up projects and sessions in a folder of their own, never the
/// user's: `Config::parse` points at the real Claude folders, so this sets them to the made-up one.
fn mouse(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    position: gpui::Point<gpui::Pixels>,
    button: Option<MouseButton>,
) {
    cx.simulate_event(
        window,
        gpui::MouseMoveEvent {
            position,
            pressed_button: None,
            modifiers: Modifiers::default(),
        },
    );
    if let Some(button) = button {
        cx.simulate_event(
            window,
            MouseDownEvent {
                position,
                button,
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            },
        );
        cx.simulate_event(
            window,
            gpui::MouseUpEvent {
                position,
                button,
                modifiers: Modifiers::default(),
                click_count: 1,
            },
        );
    }
}

/// The Inbox (empty, with made-up notifications, then its row with the count), each right-click menu, and the tooltips
/// of a title bar button, a tab and a pane's button.
fn chrome(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
    let mut config = base.clone();
    config.agents = None;
    config.plugins = None;
    // The 248 wide sidebar, then the tabs, which start 12 further; the panes start 8 in, right under the 76 of bars.
    let tab = point(px(248.0 + 12.0 + 60.0), px(20.0));
    let title = point(px(248.0 + 8.0 + 40.0), px(76.0 + 15.0));
    // The first project: under the nav's four 28px rows 1px apart and its 8px of padding, the 32px heading, and half a
    // 26px row.
    let project = point(px(60.0), px(40.0 + 4.0 * 28.0 + 3.0 + 8.0 + 32.0 + 13.0));
    for width in [1000.0, 640.0] {
        let config = config.clone();
        let handle = cx
            .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                cx.new(|cx| Workspace::new(config, window, cx))
            })
            .expect("open an offscreen window");
        let window: AnyWindowHandle = handle.into();
        let name = |state: &str| out.join(format!("{state}-{width}.png"));
        wait(cx, window, Duration::from_secs(2));
        cx.simulate_keystrokes(window, "cmd-t cmd-d");
        wait(cx, window, Duration::from_secs(2));
        // The nav's third row.
        let inbox = point(px(60.0), px(40.0 + 2.0 * 29.0 + 14.0));
        mouse(cx, window, inbox, Some(MouseButton::Left));
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("inbox-empty"));
        mouse(cx, window, inbox, Some(MouseButton::Left));
        let _ = handle.update(cx, |workspace, _, cx| workspace.sample_inbox(cx));
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("inbox-count"));
        let _ = handle.update(cx, |workspace, _, cx| workspace.show_inbox(cx));
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("inbox"));
        cx.simulate_keystrokes(window, "escape");
        // The new tab menu, from the 16px chevron at the title bar's right end, inside its 10px of padding.
        mouse(
            cx,
            window,
            point(px(width - 10.0 - 8.0), px(20.0)),
            Some(MouseButton::Left),
        );
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("menu-new-tab"));
        cx.simulate_keystrokes(window, "escape");
        for (state, at) in [
            ("menu-tab", tab),
            ("menu-pane", title),
            ("menu-project", project),
        ] {
            mouse(cx, window, at, Some(MouseButton::Right));
            wait(cx, window, Duration::from_millis(300));
            save(cx, window, &name(state));
            cx.simulate_keystrokes(window, "escape");
        }
        // The pane menu's "Move to tab" list: its third row, under 4 of padding and two 28px items and a hairline.
        mouse(cx, window, title, Some(MouseButton::Right));
        wait(cx, window, Duration::from_millis(200));
        mouse(
            cx,
            window,
            point(title.x + px(40.0), title.y + px(4.0 + 28.0 + 9.0 + 14.0)),
            None,
        );
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("menu-move"));
        cx.simulate_keystrokes(window, "escape");
        // A pane's 28px close button sits at the header's right end, 4 in from the card's 8px gutter and border.
        let close = point(px(width - 8.0 - 5.0 - 14.0), title.y);
        for (state, at) in [
            ("tooltip-sidebar", point(px(98.0), px(20.0))),
            ("tooltip-tab", tab),
            ("tooltip-pane", close),
        ] {
            mouse(cx, window, point(px(500.0), px(400.0)), None);
            wait(cx, window, Duration::from_millis(100));
            // The first move only finds what's under the pointer, on the next frame; the second starts the tooltip's wait.
            mouse(cx, window, at, None);
            wait(cx, window, Duration::from_millis(100));
            mouse(cx, window, point(at.x + px(1.0), at.y), None);
            wait(cx, window, Duration::from_millis(800));
            // `wait` moves the app's clock less than real time, so the tooltip may still be fading in.
            wait_fake(cx, window, Duration::from_millis(200));
            save(cx, window, &name(state));
        }
        // The pointer leaves the last one: halfway through fading and shrinking out.
        mouse(cx, window, point(px(500.0), px(400.0)), None);
        wait_fake(cx, window, Duration::from_millis(0));
        wait_fake(cx, window, Duration::from_millis(75));
        save(cx, window, &name("tooltip-closing"));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
        wait(cx, window, Duration::from_millis(300));
    }
}

fn finder(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
    let dir = std::env::temp_dir().join("wings-native-shot-finder");
    let _ = std::fs::remove_dir_all(&dir);
    let (web, api) = (dir.join("acme-web"), dir.join("acme-api"));
    for folder in [web.join("packages/ui"), api.clone()] {
        std::fs::create_dir_all(folder).expect("make a project folder");
    }
    std::fs::write(
        dir.join("spaces.json"),
        format!(
            r#"[{{"id":"w","name":"acme-web","path":"{}","pinned":true}},{{"id":"a","name":"acme-api","path":"{}"}}]"#,
            web.display(),
            api.display()
        ),
    )
    .expect("write the made-up project list");
    // The default account's folder, and two named accounts', whose sessions say so.
    let claude = dir.join("claude");
    let (work, client) = (
        dir.join(".claude-work"),
        dir.join(".claude-client-acme-consulting-eu"),
    );
    // Account, id, folder, branch, first prompt, reply, and how many minutes ago it was last active.
    let sessions = [
        (
            &claude,
            "0b8e7c1a-3d2f-4a5b-9c6d-7e8f9a0b1c2d",
            web.clone(),
            "main",
            "Fix the login redirect after sign-in",
            "I changed the redirect handler so a signed-in user lands on the dashboard.",
            5,
        ),
        (
            &work,
            "1c9f8d2b-4e3a-4b6c-8d7e-8f9a0b1c2d3e",
            web.clone(),
            "fix-login",
            "Add rate limiting to the login endpoint",
            "I added a token bucket in front of the login handler, with a test for the limit.",
            120,
        ),
        (
            &work,
            "2d0a9e3c-5f4b-4c7d-9e8f-9a0b1c2d3e4f",
            api.clone(),
            "release-2",
            "Write the release notes for 2.0",
            "Here are the notes, grouped by what changed for API users.",
            24 * 60,
        ),
        (
            &client,
            "3e1b0f4d-6a5c-4d8e-8f9a-0b1c2d3e4f5a",
            web.join("packages/ui"),
            "main",
            "Make the Button component accept an icon",
            "The Button now takes an icon prop and keeps its label centred.",
            3 * 24 * 60,
        ),
        (
            &claude,
            "4f2c1a5e-7b6d-4e9f-9a0b-1c2d3e4f5a6b",
            web.clone(),
            "main",
            "Why does the sidebar flicker when I resize the window?",
            "The flicker comes from measuring the width on every frame; I memoised it.",
            10 * 24 * 60,
        ),
    ];
    let now = std::time::SystemTime::now();
    for (account, id, cwd, branch, prompt, reply, minutes) in sessions {
        let named: String = cwd
            .to_string_lossy()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let folder = account.join("projects").join(named);
        std::fs::create_dir_all(&folder).expect("make a transcript folder");
        let user = serde_json::json!({ "type": "user", "cwd": cwd, "gitBranch": branch,
            "timestamp": "2026-10-01T10:00:00.000Z", "message": { "role": "user", "content": prompt } });
        let assistant = serde_json::json!({ "type": "assistant", "timestamp": "2026-10-01T10:00:05.000Z",
            "message": { "role": "assistant", "content": [{ "type": "text", "text": reply }] } });
        let path = folder.join(format!("{id}.jsonl"));
        std::fs::write(&path, format!("{user}\n{assistant}\n")).expect("write a transcript");
        std::fs::File::options()
            .write(true)
            .open(&path)
            .and_then(|file| file.set_modified(now - Duration::from_secs(minutes * 60)))
            .expect("date the transcript");
    }
    let mut config = base.clone();
    config.project = web.clone();
    config.projects = Some(dir.join("spaces.json"));
    config.agents = None;
    config.sessions = Some(SessionSource::Folders(vec![claude, work, client]));
    for width in [1000.0, 640.0] {
        let config = config.clone();
        let handle = cx
            .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                cx.new(|cx| Workspace::new(config, window, cx))
            })
            .expect("open an offscreen window");
        let window: AnyWindowHandle = handle.into();
        let name = |state: &str| out.join(format!("{state}-{width}.png"));
        wait(cx, window, Duration::from_secs(3));
        cx.simulate_keystrokes(window, "cmd-t");
        wait(cx, window, Duration::from_secs(1));
        // The palette with nothing typed, with the highlight moved down, and narrowed to the logins.
        cx.simulate_keystrokes(window, "cmd-k");
        wait(cx, window, Duration::from_millis(600));
        save(cx, window, &name("palette"));
        cx.simulate_keystrokes(window, "down down down");
        wait(cx, window, Duration::from_millis(200));
        save(cx, window, &name("palette-moved"));
        cx.simulate_input(window, "login");
        wait(cx, window, Duration::from_millis(700));
        save(cx, window, &name("palette-typed"));
        cx.simulate_keystrokes(window, "escape");
        // The sheet over this project's sessions, searched, with a row hovered and its menu open, over every project's, and
        // with nothing found.
        let _ = cx.update_window(window, |_, window, cx| {
            window.dispatch_action(Box::new(actions::OpenHistory), cx)
        });
        wait(cx, window, Duration::from_millis(800));
        save(cx, window, &name("history"));
        cx.simulate_input(window, "login");
        wait(cx, window, Duration::from_millis(800));
        save(cx, window, &name("history-search"));
        // The sheet is 480 wide at the right edge: 16 of padding, the heading, the two toggle rows, the search field and
        // the filters take about 200 before the first row.
        let sheet_left = width - 480.0;
        let row = point(px(sheet_left + 200.0), px(250.0));
        cx.simulate_event(
            window,
            gpui::MouseMoveEvent {
                position: row,
                pressed_button: None,
                modifiers: Modifiers::default(),
            },
        );
        wait(cx, window, Duration::from_millis(200));
        save(cx, window, &name("history-row"));
        click(cx, window, point(px(width - 32.0), px(233.0)));
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("history-menu"));
        click(cx, window, point(px(width - 32.0), px(233.0)));
        click(cx, window, point(px(sheet_left + 142.0), px(86.0)));
        wait(cx, window, Duration::from_millis(800));
        save(cx, window, &name("history-all"));
        cx.simulate_input(window, " zzzz");
        wait(cx, window, Duration::from_millis(800));
        save(cx, window, &name("history-empty"));
        cx.simulate_keystrokes(window, "escape");
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
        wait(cx, window, Duration::from_millis(300));
    }
}

/// Two agents in the sidebar, one under the default account and one under the named account `work` of the made-up home
/// `main` sets, which its row names; then with the sidebar dragged to its narrowest.
fn accounts(cx: &mut VisualTestAppContext, base: &Config, fake_claude: &Path, out: &Path) {
    let default = base.agents.clone().expect("the made-up default account");
    let work = std::env::home_dir()
        .expect("the made-up home")
        .join(".claude-work");
    let mut config = base.clone();
    config.plugins = None;
    config.sessions = None;
    for width in [1000.0, 640.0] {
        let config = config.clone();
        let handle = cx
            .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                cx.new(|cx| Workspace::new(config, window, cx))
            })
            .expect("open an offscreen window");
        let window: AnyWindowHandle = handle.into();
        let name = |state: &str| out.join(format!("accounts-{state}-{width}.png"));
        wait(cx, window, Duration::from_secs(3));
        // One agent in the first tab, one in a second; each a made-up `claude` told apart by how long it sleeps.
        let agents = [
            (
                &default,
                140,
                r#"{"status":"busy","name":"Refactor the API client"}"#,
            ),
            (
                &work,
                141,
                r#"{"status":"waiting","name":"Fix the login redirect after sign-in"}"#,
            ),
        ];
        for (at, (_, seconds, _)) in agents.iter().enumerate() {
            if at > 0 {
                cx.simulate_keystrokes(window, "cmd-t");
                wait(cx, window, Duration::from_secs(2));
            }
            cx.simulate_input(
                window,
                &format!("exec '{}' {seconds}", fake_claude.display()),
            );
            cx.simulate_keystrokes(window, "enter");
        }
        wait(cx, window, Duration::from_secs(1));
        for (account, seconds, session) in agents {
            let pids = std::process::Command::new("pgrep")
                .args(["-f", &format!("{} {seconds}", fake_claude.display())])
                .output()
                .expect("run pgrep");
            for pid in String::from_utf8_lossy(&pids.stdout).split_whitespace() {
                std::fs::write(account.join(format!("sessions/{pid}.json")), session)
                    .expect("write a session file");
            }
        }
        wait(cx, window, Duration::from_secs(2));
        save(cx, window, &name("sidebar"));
        // The sidebar's edge, at its 248 wide start, dragged in past the narrowest.
        cx.simulate_event(
            window,
            MouseDownEvent {
                position: point(px(247.0), px(300.0)),
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            },
        );
        for step in 1..=10 {
            let x = 247.0 - (247.0 - 100.0) * step as f32 / 10.0;
            mouse_move(cx, window, point(px(x), px(300.0)), true);
            wait(cx, window, Duration::from_millis(20));
        }
        cx.simulate_event(
            window,
            gpui::MouseUpEvent {
                position: point(px(100.0), px(300.0)),
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
                click_count: 1,
            },
        );
        wait(cx, window, Duration::from_millis(500));
        save(cx, window, &name("sidebar-narrow"));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
        let _ = std::process::Command::new("pkill")
            .args(["-f", &fake_claude.display().to_string()])
            .status();
        wait(cx, window, Duration::from_millis(300));
    }
}

/// The dialog that asks for Full Disk Access, as a launch that finds the terminals without it shows it, and the row in
/// Settings, General. The server is a made-up file in a temp folder that never starts.
fn disk_access(cx: &mut VisualTestAppContext, base: &Config, fake: &Path, out: &Path) {
    use wings_native::{
        host::TerminalServers,
        terminal_server::{Servers, Start},
    };
    let folder = fake.join("terminal-server");
    std::fs::create_dir_all(&folder).expect("make a server folder");
    let bundled = fake.join("wings-terminal-server");
    std::fs::write(&bundled, "not a server").expect("write a made-up server");
    let servers = Servers::connect_off(&folder, Some(&Start::Direct { bundled }));
    servers.set_on(true);
    let mut config = base.clone();
    config.plugins = None;
    config.sessions = None;
    config.terminal_server = Some(TerminalServers(std::sync::Arc::new(servers)));
    for width in [1000.0, 640.0] {
        let config = config.clone();
        let handle = cx
            .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                cx.new(|cx| Workspace::new(config, window, cx))
            })
            .expect("open an offscreen window");
        let window: AnyWindowHandle = handle.into();
        wait(cx, window, Duration::from_secs(2));
        handle
            .update(cx, |w, _, cx| w.disk_access_checked(Some(false), cx))
            .unwrap();
        wait(cx, window, Duration::from_millis(400));
        save(
            cx,
            window,
            &out.join(format!("disk-access-dialog-{width}.png")),
        );
        cx.simulate_keystrokes(window, "escape");
        wait(cx, window, Duration::from_millis(400));
        cx.simulate_keystrokes(window, "cmd-,");
        wait(cx, window, Duration::from_millis(600));
        // Settings, General, the first section.
        let left = (width - 820.0f32.min(width * 0.92)) / 2.0;
        click(cx, window, point(px(left + 70.0), px(150.0)));
        wait(cx, window, Duration::from_millis(300));
        save(
            cx,
            window,
            &out.join(format!("disk-access-settings-{width}.png")),
        );
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
        wait(cx, window, Duration::from_millis(300));
    }
}

/// The Agents list after a restart, from a made-up saved workspace: nothing it lists types into a shell, so no real
/// `claude` starts. Temp folders only.
fn restored(
    cx: &mut VisualTestAppContext,
    base: &Config,
    fake: &Path,
    fake_claude: &Path,
    out: &Path,
) {
    use wings_native::saved::{
        SavedLayout, SavedProject, SavedSession, SavedTab, SessionKind, Settings,
    };
    let volumes = fake.join("Volumes");
    std::fs::create_dir_all(&volumes).expect("make a drives folder");
    let pane = |cwd: PathBuf, name: Option<&str>| SavedTab {
        layout: SavedLayout::Pane {
            cwd: Some(cwd),
            history: None,
            name: None,
            claude_account: None,
            session: name.map(|name| SavedSession {
                kind: SessionKind::Claude,
                id: "0aa900ec-ed63-4df5-97c1-eecebeb8ef8d".into(),
                args: Vec::new(),
                name: Some(name.into()),
            }),
        },
        focused: 0,
        zoomed: None,
    };
    let state = fake.join("restored-workspace.json");
    let web = base.project.clone();
    wings_native::saved::save(
        &state,
        &web.to_string_lossy(),
        SavedProject {
            tabs: vec![
                pane(web.clone(), None),
                pane(
                    volumes.join("encrypted/acme-web"),
                    Some("Migrate the orders table"),
                ),
            ],
            active: Some(1),
            ..Default::default()
        },
        Settings::default(),
    )
    .expect("save the made-up workspace");
    let api = fake.join("acme-api");
    wings_native::saved::save(
        &state,
        &api.to_string_lossy(),
        SavedProject {
            tabs: vec![pane(api.clone(), Some("Fix the flaky upload test"))],
            active: Some(0),
            ..Default::default()
        },
        Settings::default(),
    )
    .expect("save the made-up workspace");
    let mut config = base.clone();
    config.plugins = None;
    config.sessions = None;
    config.state = Some(state);
    config.volumes = volumes;
    let claude_dir = base.agents.clone().expect("the made-up Claude folder");
    for width in [1000.0, 640.0] {
        let config = config.clone();
        let handle = cx
            .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                cx.new(|cx| Workspace::new(config, window, cx))
            })
            .expect("open an offscreen window");
        let window: AnyWindowHandle = handle.into();
        wait(cx, window, Duration::from_secs(3));
        // A live agent in the first tab, for the rows beside it.
        cx.simulate_keystrokes(window, "cmd-1");
        wait(cx, window, Duration::from_millis(300));
        cx.simulate_input(window, &format!("exec '{}' 142", fake_claude.display()));
        cx.simulate_keystrokes(window, "enter");
        wait(cx, window, Duration::from_secs(1));
        let pids = std::process::Command::new("pgrep")
            .args(["-f", &format!("{} 142", fake_claude.display())])
            .output()
            .expect("run pgrep");
        for pid in String::from_utf8_lossy(&pids.stdout).split_whitespace() {
            std::fs::write(
                claude_dir.join(format!("sessions/{pid}.json")),
                r#"{"status":"busy","name":"Refactor the API client"}"#,
            )
            .expect("write a session file");
        }
        wait(cx, window, Duration::from_secs(2));
        cx.simulate_keystrokes(window, "cmd-2");
        wait(cx, window, Duration::from_millis(500));
        save(cx, window, &out.join(format!("restored-{width}.png")));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
        let _ = std::process::Command::new("pkill")
            .args(["-f", &format!("{} 142", fake_claude.display())])
            .status();
        wait(cx, window, Duration::from_millis(300));
    }
}

/// Settings, Plugins with made-up plugins (one running, one from GitHub with an update, one waiting for approval), the
/// review before a plugin runs, a setup command an agent proposes, and Settings, Agents. Temp folders only.
/// A plugin's icons where the desktop draws them: its sidebar open beside its pages under Plugins in the sidebar, its
/// row in Settings, Plugins, and its block on the project page. It names `kanban` for its sidebar and block and
/// `git-compare` for one page, and its own SVG (the fixture's gauge) stands for the other.
fn icons(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
    let data = Data::temp();
    let page = |text: &str| {
        format!(
            "wings.render(JSON.stringify({{ id: 0, type: '#root', props: {{}}, children: [{{ id: 1, type: '#text', text: '{text}' }}] }}));"
        )
    };
    let (side, pr, list, sprint) = (
        page("The board"),
        page("A pull request"),
        page("A list"),
        page("This sprint"),
    );
    data.install_script(
        "board",
        serde_json::json!({ "name": "Board", "icon": "icon.svg", "contributes": {
            "sidebars": [{ "id": "side", "title": "Board", "page": "dist/side.html", "icon": "kanban" }],
            "views": [
                { "id": "pr", "title": "Pull request", "page": "dist/pr.html", "opens": ["tab"], "newTab": true, "icon": "git-compare" },
                { "id": "list", "title": "List", "page": "dist/list.html", "opens": ["tab"], "newTab": true },
            ],
            "blocks": [{ "id": "sprint", "title": "Sprint", "page": "dist/sprint.html", "icon": "kanban" }],
        } }),
        "",
        &[("side", &side), ("pr", &pr), ("list", &list), ("sprint", &sprint)],
        "board-scope",
    );
    // A made-up project page with the block on it, never the user's.
    let info = out.join("icons-project-info");
    let _ = std::fs::remove_dir_all(&info);
    wings_spaces::project_info::Store::new(info.join("projects"), info.join("plugin-storage"))
        .apply(
            &base.project.to_string_lossy(),
            wings_spaces::project_info::Op::AddBlock {
                plugin_id: "board".into(),
                block_id: "sprint".into(),
                at: wings_spaces::project_info::Place::End,
            },
        )
        .expect("place the block");
    let mut config = base.clone();
    config.plugins = Some(data.paths());
    config.project_info = Some(info);
    for width in [1000.0, 640.0] {
        let config = config.clone();
        let project = config.project.clone();
        let handle = cx
            .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                cx.new(|cx| Workspace::new(config, window, cx))
            })
            .expect("open an offscreen window");
        let window: AnyWindowHandle = handle.into();
        let name = |state: &str| out.join(format!("icons-{state}-{width}.png"));
        wait(cx, window, Duration::from_secs(1));
        handle
            .update(cx, |w, _, cx| w.shot_sidebar("board", "side", cx))
            .unwrap();
        wait(cx, window, Duration::from_millis(500));
        save(cx, window, &name("sidebar"));
        handle
            .update(cx, |w, window, cx| w.show_plugins("board", &[], window, cx))
            .unwrap();
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("settings"));
        handle
            .update(cx, |w, window, cx| {
                w.close_dialogs(cx);
                w.open_project_page(project.clone(), window, cx);
            })
            .unwrap();
        wait(cx, window, Duration::from_secs(1));
        save(cx, window, &name("block"));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
}

/// Settings on the Issues and PR tracker settings pages, each picked from its row under Plugin settings, then Plugins
/// at its larger size, halfway there and done. The pages are the plugins' built native ones, over a made-up data folder.
fn plugin_settings(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
    let data = Data::temp();
    let settings = serde_json::json!({ "settings": { "page": "dist/settings.html" } });
    data.install_script(
        "issues",
        serde_json::json!({ "name": "Issues", "contributes": settings }),
        "",
        &[(
            "settings",
            include_str!("../../../plugins/tasks/dist/native/settings.js"),
        )],
        "shot",
    );
    let pr: serde_json::Value = serde_json::from_str(include_str!(
        "../../../plugins/pr-tracker/wings-plugin.json"
    ))
    .expect("read PR tracker's manifest");
    data.install_script(
        "pr-tracker",
        serde_json::json!({ "name": "PR tracker", "permissions": { "secrets": pr["permissions"]["secrets"] },
                            "contributes": settings }),
        "",
        &[(
            "settings",
            include_str!("../../../plugins/pr-tracker/dist/native/settings.js"),
        )],
        "shot",
    );
    let mut config = base.clone();
    config.plugins = Some(data.paths());
    for width in [1000.0, 640.0] {
        let config = config.clone();
        let handle = cx
            .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                cx.new(|cx| Workspace::new(config, window, cx))
            })
            .expect("open an offscreen window");
        let window: AnyWindowHandle = handle.into();
        let name = |state: &str| out.join(format!("settings-{state}-{width}.png"));
        wait(cx, window, Duration::from_secs(1));
        cx.simulate_keystrokes(window, "cmd-,");
        wait(cx, window, Duration::from_millis(300));
        for (state, row) in [
            ("plugin-issues", "Issues"),
            ("plugin-pr-tracker", "PR tracker"),
        ] {
            click(cx, window, spot(&format!("settings-nav:{row}")));
            wait(cx, window, Duration::from_millis(800));
            save(cx, window, &name(state));
        }
        // The resize draws a frame every 16ms for its 200ms: the median of what each takes, against frames with nothing
        // changed.
        let frames = |cx: &mut VisualTestAppContext, count: usize, step: Duration| {
            let mut took: Vec<Duration> = (0..count)
                .map(|_| {
                    cx.advance_clock(step);
                    let started = Instant::now();
                    let _ = cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx));
                    started.elapsed()
                })
                .collect();
            took.sort();
            took[count / 2]
        };
        let still = frames(cx, 12, Duration::ZERO);
        click(cx, window, spot("settings-nav:Plugins"));
        let first = frames(cx, 6, Duration::from_millis(16));
        save(cx, window, &name("plugins-resizing"));
        let second = frames(cx, 6, Duration::from_millis(16));
        let settled = frames(cx, 12, Duration::ZERO);
        println!(
            "Settings at {width}: a frame with nothing changed {still:?}, resizing to Plugins {first:?} in its first \
             half and {second:?} in its second, on Plugins once resized {settled:?}"
        );
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("plugins-large"));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
        wait(cx, window, Duration::from_millis(300));
    }
}

/// A made-up plugin's diff of three files, opened from the terminal tab: docked beside it, filling the window, and docked
/// again with its file list hidden.
/// An agent (the made-up `claude`) in the first pane opens a dev server, which runs, and a test run, which waits for
/// Enter. Then the buttons on its terminals float them over it, minimise and dock the panel, hide them and move them
/// to its right; and Settings, Agents with where they open.
fn agent_terminals(
    cx: &mut VisualTestAppContext,
    config: &Config,
    fake_claude: &Path,
    claude_dir: &Path,
    out: &Path,
) {
    for width in [1000.0, 640.0] {
        let config = config.clone();
        let handle = cx
            .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                cx.new(|cx| Workspace::new(config, window, cx))
            })
            .expect("open an offscreen window");
        let window: AnyWindowHandle = handle.into();
        let name = |state: &str| out.join(format!("agent-terminals-{state}-{width}.png"));
        wait(cx, window, Duration::from_secs(3));
        cx.simulate_input(window, &format!("exec '{}' 131", fake_claude.display()));
        cx.simulate_keystrokes(window, "enter");
        wait(cx, window, Duration::from_secs(1));
        let pids = std::process::Command::new("pgrep")
            .args(["-f", &format!("{} 131", fake_claude.display())])
            .output()
            .expect("run pgrep");
        for pid in String::from_utf8_lossy(&pids.stdout).split_whitespace() {
            std::fs::write(
                claude_dir.join(format!("sessions/{pid}.json")),
                r#"{"status":"busy","name":"Fix the login redirect"}"#,
            )
            .expect("write a session file");
        }
        wait(cx, window, Duration::from_secs(2));
        let open = |cx: &mut VisualTestAppContext, command: &str, run: bool| {
            handle
                .update(cx, |workspace, window, cx| {
                    workspace.sample_agent_terminal(0, command, run, window, cx)
                })
                .expect("open the agent's terminal");
        };
        open(
            cx,
            "printf '\\n  VITE v6.0.0  ready in 412 ms\\n\\n  Local:   http://localhost:5173/\\n'; sleep 301",
            true,
        );
        // Long enough for a shell with a slow profile to start and run it, so it's busy.
        wait(cx, window, Duration::from_secs(8));
        save(cx, window, &name("below"));
        // The dev server keeps it busy, so the test run gets a terminal of its own, beside it.
        open(cx, "pnpm test --watch", false);
        wait(cx, window, Duration::from_secs(2));
        save(cx, window, &name("two"));
        click(cx, window, spot("agent-float:1"));
        wait(cx, window, Duration::from_secs(1));
        save(cx, window, &name("floated"));
        click(cx, window, spot("float-minimise:0"));
        wait(cx, window, Duration::from_millis(500));
        save(cx, window, &name("minimised"));
        click(cx, window, spot("float-dock:0"));
        wait(cx, window, Duration::from_millis(500));
        click(cx, window, spot("agent-hide:1"));
        wait(cx, window, Duration::from_millis(500));
        save(cx, window, &name("hidden"));
        click(cx, window, spot("agent-terminals:0"));
        wait(cx, window, Duration::from_millis(500));
        click(cx, window, spot("agent-move:1"));
        wait(cx, window, Duration::from_secs(1));
        save(cx, window, &name("right"));
        cx.simulate_keystrokes(window, "cmd-,");
        wait(cx, window, Duration::from_millis(300));
        click(cx, window, spot("settings-nav:Agents"));
        wait(cx, window, Duration::from_millis(300));
        // Down to its last card, where the choice is.
        cx.simulate_event(
            window,
            gpui::ScrollEvent {
                position: point(px(width / 2.0 + 80.0), px(400.0)),
                delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(-3000.0))),
                ..Default::default()
            },
        );
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("settings"));
        click(cx, window, spot("select:agent-terminals-open"));
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("settings-menu"));
        click(cx, window, spot("select-item:agent-terminals-open-2"));
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("settings-float"));
        cx.simulate_keystrokes(window, "escape");
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
        wait(cx, window, Duration::from_millis(300));
    }
    for pattern in [format!("{} 131", fake_claude.display()), "sleep 301".into()] {
        let _ = std::process::Command::new("pkill")
            .args(["-f", &pattern])
            .status();
    }
}

/// The made-up pull request of `diff-dock` and `diff-comments`: three files.
const DOCK_PATCH: &str = "diff --git a/src/config.rs b/src/config.rs\n--- a/src/config.rs\n+++ b/src/config.rs\n@@ -1,8 +1,10 @@\n use std::time::Duration;\n \n pub fn load() -> Config {\n-    let timeout = 10;\n+    // A slow network gets room to answer before the client gives up.\n+    let timeout = 30;\n+    validate(timeout);\n     Config {\n         timeout: Duration::from_secs(timeout),\n-        retries: 1,\n+        retries: 3,\n     }\n }\ndiff --git a/src/client/retry.rs b/src/client/retry.rs\nnew file mode 100644\n--- /dev/null\n+++ b/src/client/retry.rs\n@@ -0,0 +1,4 @@\n+pub fn backoff(attempt: u32) -> u64 {\n+    200 * 2u64.pow(attempt)\n+}\n+\ndiff --git a/README.md b/README.md\n--- a/README.md\n+++ b/README.md\n@@ -3,3 +3,3 @@\n ## Configuration\n-The client waits 10 seconds.\n+The client waits 30 seconds and retries three times.\n \n";

fn diff_dock(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
    let data = Data::temp();
    data.install_script(
        "differ",
        serde_json::json!({ "name": "Changes", "contributes": { "ui": ["diff"] } }),
        &format!(
            "let opened = false; wings.onPanes(async (list) => {{ if (!list.length || opened) return; opened = true; await wings.openDiff({{ title: 'Fix the client timeout', subtitle: 'acme-web #42 · fix-timeout into main', patch: {} }}); }});",
            serde_json::json!(DOCK_PATCH)
        ),
        &[],
        "differ-scope",
    );
    let mut config = base.clone();
    config.plugins = Some(data.paths());
    for width in [1000.0, 640.0] {
        let config = config.clone();
        let handle = cx
            .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                cx.new(|cx| Workspace::new(config, window, cx))
            })
            .expect("open an offscreen window");
        let window: AnyWindowHandle = handle.into();
        let name = |state: &str| out.join(format!("diff-{state}-{width}.png"));
        wait(cx, window, Duration::from_millis(1500));
        save(cx, window, &name("docked"));
        click(cx, window, spot("diff-expand"));
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("expanded"));
        click(cx, window, spot("diff-dock"));
        wait(cx, window, Duration::from_millis(300));
        click(cx, window, spot("diff-files-toggle"));
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("docked-no-files"));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
}

/// `diff-dock`'s pull request with made-up review threads: one on an added line with a reply, one on a removed line, one
/// on code that changed since, and one on the whole pull request.
fn diff_comments(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
    let at = |hours: i64| (chrono::Utc::now() - chrono::Duration::hours(hours)).to_rfc3339();
    let comment = |id: i64,
                   reply: Option<i64>,
                   path: Option<&str>,
                   line: Option<u32>,
                   side: &str,
                   author: &str,
                   body: &str,
                   hours: i64| {
        serde_json::json!({ "id": id, "replyTo": reply, "path": path, "line": line, "side": side, "author": author, "body": body,
            "createdAt": at(hours), "url": format!("https://github.com/acme/web/pull/42#discussion_r{id}"), "canReply": reply.is_none() })
    };
    let comments = serde_json::json!([
        comment(
            1,
            None,
            Some("src/config.rs"),
            Some(5),
            "additions",
            "maria",
            "Why 30? The load balancer drops idle connections after **25 s**, so the client would still see resets. See [the runbook](https://example.com/runbook).",
            50
        ),
        comment(
            2,
            Some(1),
            Some("src/config.rs"),
            Some(5),
            "additions",
            "tom",
            "Good catch, `20` it is. I'll change it.",
            3
        ),
        comment(
            3,
            None,
            Some("src/config.rs"),
            Some(7),
            "deletions",
            "maria",
            "Was one retry ever enough here?",
            49
        ),
        comment(
            4,
            None,
            Some("src/client/retry.rs"),
            None,
            "additions",
            "lee",
            "This doubles forever. Cap it at 30 s.",
            30
        ),
        comment(
            5,
            None,
            None,
            None,
            "additions",
            "lee",
            "Looks good once the timeout is settled. I ran it against staging.",
            2
        ),
    ]);
    let data = Data::temp();
    data.install_script(
        "differ",
        serde_json::json!({ "name": "Changes", "contributes": { "ui": ["diff"] } }),
        &format!(
            "let opened = false; wings.onReply(async () => {{ throw new Error('GitHub refused the reply: you are not signed in.'); }}); wings.onPanes(async (list) => {{ if (!list.length || opened) return; opened = true; await wings.openDiff({{ title: 'Fix the client timeout', subtitle: 'acme-web #42 · fix-timeout into main', patch: {}, comments: {} }}); }});",
            serde_json::json!(DOCK_PATCH),
            comments
        ),
        &[],
        "differ-scope",
    );
    let mut config = base.clone();
    config.plugins = Some(data.paths());
    for width in [1000.0, 640.0] {
        let config = config.clone();
        let handle = cx
            .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                cx.new(|cx| Workspace::new(config, window, cx))
            })
            .expect("open an offscreen window");
        let window: AnyWindowHandle = handle.into();
        let name = |state: &str| out.join(format!("diff-comments-{state}-{width}.png"));
        wait(cx, window, Duration::from_millis(1500));
        save(cx, window, &name("docked"));
        // At 640 there's no room to dock, so it already fills the window, with no file list toggle.
        let docks = width > 700.0;
        if docks {
            click(cx, window, spot("diff-files-toggle"));
            wait(cx, window, Duration::from_millis(300));
            save(cx, window, &name("docked-no-files"));
        }
        // A reply being written, which the plugin refuses.
        click(cx, window, spot("diff-reply-1"));
        cx.simulate_input(window, "20 s it is, thanks.");
        wait(cx, window, Duration::from_millis(300));
        click(cx, window, spot("diff-reply-send-1"));
        wait(cx, window, Duration::from_millis(500));
        save(cx, window, &name("reply"));
        if docks {
            click(cx, window, spot("diff-files-toggle"));
            click(cx, window, spot("diff-expand"));
            wait(cx, window, Duration::from_millis(300));
            save(cx, window, &name("expanded"));
        }
        click(cx, window, spot("diff-discussion"));
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("discussion"));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
}

fn plugins_install(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
    let data = Data::temp();
    data.install("demo", WINGS_VERSION, true, true, Approval::Current, "shot");
    data.install_script(
        "tracker",
        serde_json::json!({ "name": "Pull requests", "version": "1.2.0", "description": "Shows each pane's pull request and its checks.",
            "permissions": { "exec": ["gh pr view", "gh pr checks"], "notify": true, "fetch": ["https://api.github.com/"] },
            "contributes": { "ui": ["badges"], "mcpTools": [{ "name": "pr_status" }] } }),
        "",
        &[],
        "tracker-scope",
    );
    data.install_script(
        "notes",
        serde_json::json!({ "name": "Notes", "version": "0.3.0", "description": "Keeps notes beside each project.",
            "permissions": { "panes": [], "sendToAgents": true, "secrets": { "notes-token": "https://notes.example.com/" } },
            "contributes": { "sidebars": [{ "id": "notes", "title": "Notes", "page": "dist/notes.html" }] } }),
        "",
        &[("notes", "")],
        "notes-scope",
    );
    // The tracker came from GitHub, and Notes was never approved.
    let state = data.paths().state;
    let mut entries: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&state).unwrap()).unwrap();
    entries["tracker"]["source"] = serde_json::json!({ "kind": "github", "repo": "owner/tracker" });
    entries["notes"]["approved"] = serde_json::Value::Null;
    entries["notes"]["enabled"] = serde_json::json!(false);
    std::fs::write(&state, entries.to_string()).unwrap();
    let mut config = base.clone();
    config.plugins = Some(data.paths());
    for width in [1000.0, 640.0] {
        let config = config.clone();
        let handle = cx
            .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                cx.new(|cx| Workspace::new(config, window, cx))
            })
            .expect("open an offscreen window");
        let window: AnyWindowHandle = handle.into();
        let name = |state: &str| out.join(format!("{state}-{width}.png"));
        wait(cx, window, Duration::from_secs(1));
        let show = |cx: &mut VisualTestAppContext, selected: &'static str| {
            handle
                .update(cx, |w, window, cx| {
                    w.close_dialogs(cx);
                    w.show_plugins(selected, &[("tracker", "1.3.0")], window, cx)
                })
                .unwrap();
            wait(cx, window, Duration::from_millis(300));
        };
        show(cx, "tracker");
        save(cx, window, &name("plugins-update"));
        handle
            .update(cx, |w, _, cx| w.show_plugin_check(true, cx))
            .unwrap();
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("plugins-checking"));
        handle
            .update(cx, |w, _, cx| w.show_plugin_check(false, cx))
            .unwrap();
        show(cx, "notes");
        save(cx, window, &name("plugins-waiting"));
        handle
            .update(cx, |w, window, cx| {
                w.show_plugin_review("notes", window, cx)
            })
            .unwrap();
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("plugins-review"));
        handle
            .update(cx, |w, _, cx| {
                w.close_dialogs(cx);
                w.show_setup_approval(
                    "acme-web",
                    "Claude in acme-web",
                    Some("pnpm install"),
                    "pnpm install && pnpm run build:types",
                    cx,
                )
            })
            .unwrap();
        wait(cx, window, Duration::from_millis(1300));
        save(cx, window, &name("setup-approval"));
        show(cx, "demo");
        let left = (width - 820.0f32.min(width * 0.92)) / 2.0;
        click(cx, window, point(px(left + 70.0), px(150.0 + 34.0 * 6.0)));
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("settings-agents"));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
        wait(cx, window, Duration::from_millis(300));
    }
}

/// A `.wings-plugin` file dragged over the window, the review it opens once dropped, and an installed plugin's page with
/// its README. Temp folders only.
fn plugins_drop(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
    use std::io::Write;
    let data = Data::temp();
    data.install_script(
        "tracker",
        serde_json::json!({ "name": "Pull requests", "version": "1.2.0", "description": "Shows each pane's pull request and its checks.",
            "permissions": { "exec": ["gh pr view", "gh pr checks"], "notify": true },
            "contributes": { "ui": ["badges"] } }),
        "",
        &[],
        "tracker-scope",
    );
    std::fs::write(
        data.paths().dir.join("tracker/README.md"),
        "# Pull requests\n\nShows the pull request of each pane's branch on its badge, with its checks. Needs the \
         [GitHub CLI](https://cli.github.com) signed in.\n\n## Setup\n\n1. Install `gh`.\n2. Run `gh auth login`.\n\n\
         > Checks refresh every minute while a pane is open.\n\n```sh\ngh pr view --json number,state\n```\n",
    )
    .unwrap();
    let package = data.path().join("notes.wings-plugin");
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, body) in [
        ("wings-plugin.json", serde_json::json!({ "id": "notes", "name": "Notes", "version": "0.3.0", "api": 1, "main": "dist/main.js",
            "description": "Keeps notes beside each project.", "permissions": { "panes": [], "sendToAgents": true } }).to_string()),
        ("dist/main.js", String::new()),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    std::fs::write(&package, zip.finish().unwrap().into_inner()).unwrap();
    let mut config = base.clone();
    config.plugins = Some(data.paths());
    for width in [1000.0, 640.0] {
        let config = config.clone();
        let handle = cx
            .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                cx.new(|cx| Workspace::new(config, window, cx))
            })
            .expect("open an offscreen window");
        let window: AnyWindowHandle = handle.into();
        let name = |state: &str| out.join(format!("{state}-{width}.png"));
        wait(cx, window, Duration::from_secs(1));
        let position = point(px(width / 2.0), px(320.0));
        cx.simulate_event(
            window,
            gpui::FileDropEvent::Entered {
                position,
                paths: gpui::ExternalPaths([package.clone()].into_iter().collect()),
            },
        );
        cx.simulate_event(window, gpui::FileDropEvent::Pending { position });
        save(cx, window, &name("plugins-drop-over"));
        cx.simulate_event(window, gpui::FileDropEvent::Submit { position });
        cx.simulate_event(window, gpui::FileDropEvent::Ended);
        wait(cx, window, Duration::from_millis(2000));
        save(cx, window, &name("plugins-drop-review"));
        handle
            .update(cx, |w, window, cx| {
                w.close_dialogs(cx);
                w.show_plugins("tracker", &[], window, cx)
            })
            .unwrap();
        wait(cx, window, Duration::from_millis(300));
        // Down to the README, under what the plugin can access.
        cx.simulate_event(
            window,
            gpui::ScrollEvent {
                position: point(px(width / 2.0), px(400.0)),
                delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(-400.0))),
                ..Default::default()
            },
        );
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("plugins-readme"));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
        wait(cx, window, Duration::from_millis(300));
    }
}

/// Settings' sections, the project bar's Open in editor with its menu, the quick terminal floating, minimised and filling
/// the window, and the welcome once the last project is removed. Made-up editors and the fixture plugin only.
fn parity(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
    let mut config = base.clone();
    config.projects = None;
    config.editors = Some(
        [("vscode", "VS Code"), ("zed", "Zed"), ("xcode", "Xcode")]
            .map(|(id, name)| wings_editors::EditorView {
                id: id.into(),
                name: name.into(),
            })
            .into(),
    );
    for width in [1000.0, 640.0] {
        let config = config.clone();
        let handle = cx
            .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                cx.new(|cx| Workspace::new(config, window, cx))
            })
            .expect("open an offscreen window");
        let window: AnyWindowHandle = handle.into();
        let name = |state: &str| out.join(format!("{state}-{width}.png"));
        wait(cx, window, Duration::from_secs(2));
        save(cx, window, &name("editor"));
        // The chevron at the project bar's right end, under the 40px title bar.
        let chevron = point(px(width - 8.0 - 14.0), px(40.0 + 18.0));
        click(cx, window, chevron);
        wait(cx, window, Duration::from_millis(200));
        save(cx, window, &name("editor-menu"));
        click(cx, window, chevron);

        // Made-up Claude Code accounts for Agents: this window serves no socket, so it reads none of yours.
        let status = |connected, statusline| wings_connect::register::Status {
            claude: true,
            connected,
            statusline,
            always_load: false,
        };
        let accounts = vec![
            AccountStatus {
                name: None,
                status: status(true, true),
            },
            AccountStatus {
                name: Some("work".into()),
                status: status(true, false),
            },
        ];
        handle
            .update(cx, |workspace, _, cx| {
                workspace.show_claude_accounts(accounts, cx);
                workspace.show_opencode_status(
                    wings_connect::opencode::Status {
                        opencode: true,
                        connected: true,
                    },
                    cx,
                );
                workspace.show_harnesses(made_up_harnesses(), cx);
            })
            .expect("show the made-up accounts");
        // Each section of Settings, by its row in the list: 32px rows, 2px apart, the first at 117 as in the desktop.
        cx.simulate_keystrokes(window, "cmd-,");
        wait(cx, window, Duration::from_millis(300));
        let left = (width - 820.0f32.min(width - 64.0)) / 2.0;
        let sections = [
            "general",
            "text",
            "cursor",
            "history",
            "keyboard",
            "editor",
            "agents",
            "notifications",
            "plugins",
        ];
        for (index, section) in sections.into_iter().enumerate() {
            let row = point(px(left + 70.0), px(133.0 + 34.0 * index as f32));
            click(cx, window, row);
            wait(cx, window, Duration::from_millis(200));
            save(cx, window, &name(&format!("settings-{section}")));
        }
        cx.simulate_keystrokes(window, "escape");
        wait(cx, window, Duration::from_millis(200));

        // The quick terminal: its first show starts a shell, a second press hides it, a third shows the same one.
        let timed = |cx: &mut VisualTestAppContext, what: &str| {
            let started = Instant::now();
            let _ = cx.update_window(window, |_, window, cx| {
                window.dispatch_action(Box::new(actions::ToggleQuickTerminal), cx);
                window.draw(cx).clear(cx);
            });
            println!(
                "quick terminal {what} at {width}: {:?} to the next frame",
                started.elapsed()
            );
        };
        let started = Instant::now();
        let _ = cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx));
        println!(
            "a frame with nothing changed at {width}: {:?}",
            started.elapsed()
        );
        timed(cx, "first show");
        wait(cx, window, Duration::from_secs(2));
        save(cx, window, &name("quick"));
        timed(cx, "hide");
        timed(cx, "show again");
        wait(cx, window, Duration::from_millis(300));
        // The header's buttons from the right: close, minimise, fill. The panel sits 24px in from the area's corner.
        let top = 640.0 - 24.0 - 380.0;
        let button =
            |from_right: f32| point(px(width - 24.0 - 4.0 - 12.0 - from_right), px(top + 16.0));
        click(cx, window, button(30.0));
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("quick-minimised"));
        timed(cx, "hide");
        timed(cx, "show again");
        wait(cx, window, Duration::from_millis(300));
        click(cx, window, button(60.0));
        wait(cx, window, Duration::from_millis(500));
        save(cx, window, &name("quick-full"));
        timed(cx, "hide");

        // The only project's remove button, at the right of its row while the pointer is over it, then Remove. The row is
        // under the nav's four rows and the quick terminal's row in the Terminals list, each 28px with 8px below, and
        // the 32px Projects heading.
        let project = 40.0 + 4.0 * 29.0 - 1.0 + 8.0 + 28.0 + 8.0 + 32.0 + 20.0;
        mouse_move(cx, window, point(px(60.0), px(project)), false);
        wait(cx, window, Duration::from_millis(200));
        click(cx, window, point(px(248.0 - 8.0 - 6.0 - 12.0), px(project)));
        wait(cx, window, Duration::from_millis(200));
        save(cx, window, &name("welcome-remove"));
        // The dialog's Remove button: 400 x 176 in the middle, buttons at the bottom right inside 20px of padding.
        click(
            cx,
            window,
            point(
                px(width / 2.0 + 200.0 - 20.0 - 36.0),
                px(320.0 + 88.0 - 20.0 - 16.0),
            ),
        );
        wait(cx, window, Duration::from_millis(500));
        save(cx, window, &name("welcome"));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
        wait(cx, window, Duration::from_millis(300));
    }
}

/// Made-up agent CLIs for Settings, Agents: one of each state the rows show.
fn made_up_harnesses() -> Vec<wings_connect::harnesses::HarnessStatus> {
    use wings_connect::harnesses::{Auth, HarnessStatus};
    let row = |id: &str,
               name: &str,
               installed,
               version: Option<&str>,
               auth,
               account: Option<&str>,
               note: Option<&str>| HarnessStatus {
        id: id.into(),
        name: name.into(),
        installed,
        version: version.map(Into::into),
        auth,
        account: account.map(Into::into),
        note: note.map(Into::into),
        login: format!("{id} login"),
        install: format!("curl -fsSL https://example.com/{id}.sh | bash"),
        docs: "https://example.com".into(),
    };
    vec![
        row(
            "claude",
            "Claude Code",
            true,
            Some("2.1.312"),
            Auth::SignedIn,
            Some("me@example.com"),
            None,
        ),
        row(
            "codex",
            "Codex",
            true,
            Some("0.48.0"),
            Auth::SignedOut,
            None,
            None,
        ),
        row(
            "cursor",
            "Cursor CLI",
            true,
            Some("2025.10.1"),
            Auth::Unknown,
            None,
            Some("Installed. Wings can't check its sign-in yet"),
        ),
        row("grok", "Grok Build", false, None, Auth::Unknown, None, None),
        row(
            "opencode",
            "OpenCode",
            true,
            Some("1.0.4"),
            Auth::Unknown,
            None,
            Some("opencode didn't answer in 8s"),
        ),
    ]
}

/// A shell prints a red-and-blue SIXEL, a line of text, then a green iTerm2 PNG eight cells wide.
fn images(cx: &mut VisualTestAppContext, config: &Config, fake: &Path, out: &Path) {
    use base64::Engine;
    let band = |color: u8| format!("#{color}!80~");
    let sixel = format!(
        "\x1bP0;1q\"1;1;160;96#1;2;90;20;20#2;2;20;40;90{}\x1b\\",
        (0..16)
            .map(|_| format!("{}{}-", band(1), band(2)))
            .collect::<String>()
    );
    let png = image::RgbaImage::from_fn(64, 32, |x, y| {
        image::Rgba([40, 160 + (x % 64) as u8, 80 + y as u8 * 4, 255])
    });
    let mut file = std::io::Cursor::new(Vec::new());
    png.write_to(&mut file, image::ImageFormat::Png)
        .expect("encode a PNG");
    let file = file.into_inner();
    let iterm = format!(
        "\x1b]1337;File=size={};width=8;inline=1:{}\x07",
        file.len(),
        base64::engine::general_purpose::STANDARD.encode(&file)
    );
    let fixture = fake.join("images.txt");
    std::fs::write(
        &fixture,
        format!("sixel:\r\n{sixel}\r\nafter the sixel\r\niterm2: {iterm}\r\nafter the png\r\n"),
    )
    .expect("write the image fixture");
    for width in [1000.0, 640.0] {
        let config = config.clone();
        let handle = cx
            .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                cx.new(|cx| Workspace::new(config, window, cx))
            })
            .expect("open an offscreen window");
        let window: AnyWindowHandle = handle.into();
        wait(cx, window, Duration::from_secs(3));
        cx.simulate_input(window, &format!("clear; cat {}", fixture.display()));
        cx.simulate_keystrokes(window, "enter");
        wait(cx, window, Duration::from_secs(2));
        save(
            cx,
            window,
            &out.join(format!("terminal-images-{width}.png")),
        );
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
}
