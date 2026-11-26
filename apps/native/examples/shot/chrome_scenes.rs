//! The window's chrome: tab dragging, the sidebar, the palette and history, the Inbox, right-click menus and the finder.
use super::*;

/// Dragging a tab along the title bar: four terminals, the shown one, last, dragged most of the way onto its neighbour,
/// which has made room under it, then let go there. At 640 the strip is scrolled to it.
pub(super) fn tab_drag(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
    for width in [1000.0, 640.0] {
        let config = base.clone();
        let handle = cx
            .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                cx.new(|cx| Workspace::new(config, window, cx))
            })
            .expect("open an offscreen window");
        let window: AnyWindowHandle = handle.into();
        let name = |state: &str| out.join(format!("tab-{state}-{width}.png"));
        settle(cx, window, Duration::from_secs(2));
        cx.simulate_keystrokes(window, "cmd-t cmd-t cmd-t");
        settle(cx, window, Duration::from_secs(2));
        let (from, neighbour) = (spot("tab:Terminal(3)"), spot("tab:Terminal(2)"));
        let to = point(neighbour.x - px(24.0), from.y);
        mouse_move(cx, window, from, false);
        settle(cx, window, Duration::from_millis(100));
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
        // A program in the shown terminal: its tab gets the green dot once detection sees it.
        cx.simulate_input(window, "sleep 600");
        cx.simulate_keystrokes(window, "enter");
        settle(cx, window, Duration::from_secs(2));
        save(cx, window, &name("running"));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
}

pub(super) fn git(dir: &Path, args: &[&str]) {
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

pub(super) fn mouse_move(
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
pub(super) fn sidebar(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
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
    // A plugin that lists a section under each project, so the project's menu shows it with its check. Its main lists
    // no rows, so the sidebar looks as it did.
    let sections = Data::temp();
    sections.install_script(
        "tracker",
        serde_json::json!({ "contributes": { "projectSections": [{ "id": "pulls", "title": "Pull requests" }] } }),
        &fake::idle("main"),
        &[],
        "shot",
    );
    for width in [1000.0, 640.0] {
        let mut config = base.clone();
        config.project = api.clone();
        config.agents = None;
        config.plugins = Some(sections.paths());
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
        settle(cx, window, Duration::from_secs(3));
        if std::env::args().nth(2).as_deref() == Some("sidebar-polish") {
            handle.update(cx, |workspace, window, cx| workspace.sample_sidebar_polish(window, cx)).unwrap();
            settle(cx, window, Duration::from_millis(300));
            save(cx, window, &name("unread-card"));
            click(cx, window, spot("sidebar-filter"));
            wait(cx, window, Duration::from_millis(300));
            save(cx, window, &name("filters"));
            click(cx, window, spot("menu:Show idle projects"));
            wait(cx, window, Duration::from_millis(300));
            save(cx, window, &name("active-only"));
            click(cx, window, spot("sidebar-filter"));
            wait(cx, window, Duration::from_millis(300));
            click(cx, window, spot("menu:Show worktrees"));
            wait(cx, window, Duration::from_millis(300));
            save(cx, window, &name("no-worktrees"));
            click(cx, window, spot("agent-row:900"));
            wait(cx, window, Duration::from_millis(300));
            save(cx, window, &name("read"));
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
            continue;
        }
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
                settle(cx, window, Duration::from_millis(20));
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
            settle(cx, window, Duration::from_millis(300));
            save(cx, window, &name(state));
        }
        // Closing, halfway through its slide, then closed, and the pointer at the window's edge.
        cx.simulate_keystrokes(window, "cmd-b");
        wait(cx, window, Duration::from_millis(100));
        save(cx, window, &name("sliding"));
        settle(cx, window, Duration::from_millis(400));
        mouse_move(cx, window, point(px(3.0), px(300.0)), false);
        settle(cx, window, Duration::from_millis(500));
        save(cx, window, &name("peek"));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
        wait(cx, window, Duration::from_millis(300));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The command palette and the history sheet, over made-up projects and sessions in a folder of their own, never the
/// user's: `Config::parse` points at the real Claude folders, so this sets them to the made-up one.
pub(super) fn mouse(
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
pub(super) fn readout(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
    let mut config = base.clone();
    config.agents = None;
    config.plugins = None;
    for width in [1000.0, 640.0] {
        let config = config.clone();
        let handle = cx
            .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                cx.new(|cx| Workspace::new(config, window, cx))
            })
            .expect("open an offscreen window");
        let window: AnyWindowHandle = handle.into();
        settle(cx, window, Duration::from_secs(2));
        cx.simulate_keystrokes(window, "cmd-t cmd-d");
        cx.simulate_keystrokes(window, "cmd-k");
        cx.simulate_input(window, "Performance Readout");
        cx.simulate_keystrokes(window, "enter");
        // Past two updates, so the card shows numbers.
        wait_real(cx, window, Duration::from_millis(2500));
        save(cx, window, &out.join(format!("readout-{width}.png")));
    }
}

pub(super) fn chrome(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
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
        settle(cx, window, Duration::from_secs(2));
        cx.simulate_keystrokes(window, "cmd-t cmd-d");
        settle(cx, window, Duration::from_secs(2));
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

pub(super) fn finder(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
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
        settle(cx, window, Duration::from_secs(3));
        cx.simulate_keystrokes(window, "cmd-t");
        settle(cx, window, Duration::from_secs(1));
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
