//! Plugin icons, plugin settings pages and the agent terminals.
use super::*;

/// Settings, Plugins with made-up plugins (one running, one from GitHub with an update, one waiting for approval), the
/// review before a plugin runs, a setup command an agent proposes, and Settings, Agents. Temp folders only.
/// A plugin's icons where the desktop draws them: its sidebar open beside its pages under Plugins in the sidebar, its
/// row in Settings, Plugins, and its block on the project page. It names `kanban` for its sidebar and block and
/// `git-compare` for one page, and its own SVG (the fixture's gauge) stands for the other.
pub(super) fn icons(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
    let data = Data::temp();
    let (side, pr, list, sprint) = (
        words("The board"),
        words("A pull request"),
        words("A list"),
        words("This sprint"),
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
        &fake::idle("main"),
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
        settle(cx, window, Duration::from_secs(1));
        handle
            .update(cx, |w, _, cx| w.shot_sidebar("board", "side", cx))
            .unwrap();
        settle(cx, window, Duration::from_millis(500));
        save(cx, window, &name("sidebar"));
        handle
            .update(cx, |w, window, cx| w.show_plugins("board", &[], window, cx))
            .unwrap();
        settle(cx, window, Duration::from_millis(300));
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
pub(super) fn plugin_settings(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
    let data = Data::temp();
    let settings = serde_json::json!({ "settings": { "page": "dist/settings.html" } });
    data.install_script(
        "issues",
        serde_json::json!({ "name": "Issues", "contributes": settings }),
        &fake::idle("main"),
        &[("settings", &built("tasks", "settings"))],
        "shot",
    );
    let pr: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../plugins/pr-tracker/wings-plugin.json"
    ))
    .expect("read PR tracker's manifest");
    data.install_script(
        "pr-tracker",
        serde_json::json!({ "name": "PR tracker", "permissions": { "secrets": pr["permissions"]["secrets"] },
                            "contributes": settings }),
        &fake::idle("main"),
        &[("settings", &built("pr-tracker", "settings"))],
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
        settle(cx, window, Duration::from_secs(1));
        cx.simulate_keystrokes(window, "cmd-,");
        wait(cx, window, Duration::from_millis(300));
        for (state, row) in [
            ("plugin-issues", "Issues"),
            ("plugin-pr-tracker", "PR tracker"),
        ] {
            click(cx, window, spot(&format!("settings-nav:{row}")));
            // The page's module is compiled the first time it starts, which takes seconds in a debug build.
            settle(cx, window, Duration::from_millis(5000));
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
/// Enter. Then the buttons on its terminals float them over it, pick the other one in the panel's menu, minimise and dock
/// the panel, hide them and move them to its right; and Settings, Agents with where they open.
pub(super) fn agent_terminals(
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
        settle(cx, window, Duration::from_secs(3));
        cx.simulate_input(window, &format!("exec '{}' 131", fake_claude.display()));
        cx.simulate_keystrokes(window, "enter");
        settle(cx, window, Duration::from_secs(1));
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
        settle(cx, window, Duration::from_secs(2));
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
        settle(cx, window, Duration::from_secs(8));
        save(cx, window, &name("below"));
        // The dev server keeps it busy, so the test run gets a terminal of its own, beside it.
        open(cx, "pnpm test --watch", false);
        settle(cx, window, Duration::from_secs(2));
        save(cx, window, &name("two"));
        // At 640 the terminal's buttons are in its "…" menu, whose items can't be clicked here: no shots of them there.
        if wings_native::ui::spot_at("agent-float:1").is_some() {
            click(cx, window, spot("agent-float:1"));
            settle(cx, window, Duration::from_secs(1));
            save(cx, window, &name("floated"));
            click(cx, window, spot("float-picker:0"));
            settle(cx, window, Duration::from_millis(500));
            save(cx, window, &name("floated-menu"));
            click(cx, window, spot("float-pick:1"));
            settle(cx, window, Duration::from_millis(500));
            save(cx, window, &name("floated-picked"));
            click(cx, window, spot("float-minimise:0"));
            settle(cx, window, Duration::from_millis(500));
            save(cx, window, &name("minimised"));
            click(cx, window, spot("float-dock:0"));
            settle(cx, window, Duration::from_millis(500));
            click(cx, window, spot("agent-hide:1"));
            settle(cx, window, Duration::from_millis(500));
            save(cx, window, &name("hidden"));
            click(cx, window, spot("agent-terminals:0"));
            settle(cx, window, Duration::from_millis(500));
            click(cx, window, spot("agent-move:1"));
            settle(cx, window, Duration::from_secs(1));
            save(cx, window, &name("right"));
        }
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
        settle(cx, window, Duration::from_millis(300));
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
