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
    // `-- <out-dir> sidebar` renders only the sidebar's states.
    if std::env::args().nth(2).as_deref() == Some("sidebar") {
        return sidebar(&mut cx, &config, &out);
    }
    // `-- <out-dir> parity` renders only Settings' sections, Open in editor, the welcome and the quick terminal.
    if std::env::args().nth(2).as_deref() == Some("parity") {
        return parity(&mut cx, &config, &out);
    }
    // `-- <out-dir> finder` renders only the command palette and the history sheet.
    if std::env::args().nth(2).as_deref() == Some("finder") {
        return finder(&mut cx, &config, &out);
    }
    // `-- <out-dir> chrome` renders only the Inbox, the right-click menus and the tooltips.
    if std::env::args().nth(2).as_deref() == Some("chrome") {
        return chrome(&mut cx, &config, &out);
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
/// group, hovering a row, the sort menu, the question before a project with tabs is removed, dragging the edge to both
/// limits, and the peek of a closed sidebar.
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
        // Pointing at the third row, the shown project: its buttons replace the pin.
        let third = point(px(120.0), px(78.0 + 40.0 + 2.0 + 40.0 + 2.0 + 20.0));
        mouse_move(cx, window, third, false);
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("hover"));
        // Its X asks first, as it has a tab open.
        click(cx, window, point(px(222.0), third.y));
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("remove"));
        cx.simulate_keystrokes(window, "escape");
        mouse_move(cx, window, point(px(500.0), px(300.0)), false);
        // The sort menu, from the header's middle button, then Name, its last item.
        click(cx, window, point(px(204.0), px(62.0)));
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("sort"));
        click(cx, window, point(px(220.0), px(194.0)));
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("sorted"));
        // "Older" is closed under the list; open it.
        click(cx, window, point(px(60.0), px(288.0)));
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
        // Closed, and the pointer at the window's edge.
        cx.simulate_keystrokes(window, "cmd-b");
        wait(cx, window, Duration::from_millis(200));
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
    // The first project: under the 28px Inbox row and its 8px of padding, the 32px heading, and half a 26px row.
    let project = point(px(60.0), px(40.0 + 28.0 + 8.0 + 32.0 + 13.0));
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
        let inbox = point(px(60.0), px(40.0 + 14.0));
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
            save(cx, window, &name(state));
        }
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
    let claude = dir.join("claude");
    // Id, folder, branch, first prompt, reply, and how many minutes ago it was last active.
    let sessions = [
        (
            "0b8e7c1a-3d2f-4a5b-9c6d-7e8f9a0b1c2d",
            web.clone(),
            "main",
            "Fix the login redirect after sign-in",
            "I changed the redirect handler so a signed-in user lands on the dashboard.",
            5,
        ),
        (
            "1c9f8d2b-4e3a-4b6c-8d7e-8f9a0b1c2d3e",
            web.clone(),
            "fix-login",
            "Add rate limiting to the login endpoint",
            "I added a token bucket in front of the login handler, with a test for the limit.",
            120,
        ),
        (
            "2d0a9e3c-5f4b-4c7d-9e8f-9a0b1c2d3e4f",
            api.clone(),
            "release-2",
            "Write the release notes for 2.0",
            "Here are the notes, grouped by what changed for API users.",
            24 * 60,
        ),
        (
            "3e1b0f4d-6a5c-4d8e-8f9a-0b1c2d3e4f5a",
            web.join("packages/ui"),
            "main",
            "Make the Button component accept an icon",
            "The Button now takes an icon prop and keeps its label centred.",
            3 * 24 * 60,
        ),
        (
            "4f2c1a5e-7b6d-4e9f-9a0b-1c2d3e4f5a6b",
            web.clone(),
            "main",
            "Why does the sidebar flicker when I resize the window?",
            "The flicker comes from measuring the width on every frame; I memoised it.",
            10 * 24 * 60,
        ),
    ];
    let now = std::time::SystemTime::now();
    for (id, cwd, branch, prompt, reply, minutes) in sessions {
        let named: String = cwd
            .to_string_lossy()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let folder = claude.join("projects").join(named);
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
    config.sessions = Some(SessionSource::Folders(vec![claude]));
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
                workspace.show_claude_accounts(accounts, cx)
            })
            .expect("show the made-up accounts");
        // Each section of Settings, by its row in the list: 32px rows, 2px apart, the first at 117 as in the desktop.
        cx.simulate_keystrokes(window, "cmd-,");
        wait(cx, window, Duration::from_millis(300));
        let left = (width - 820.0f32.min(width - 64.0)) / 2.0;
        let sections = [
            "general", "text", "cursor", "history", "keyboard", "editor", "agents", "plugins",
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

        // The only project's remove button, at the right of its row while the pointer is over it, then Remove.
        mouse_move(cx, window, point(px(60.0), px(90.0)), false);
        wait(cx, window, Duration::from_millis(200));
        click(cx, window, point(px(248.0 - 8.0 - 6.0 - 12.0), px(90.0)));
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
