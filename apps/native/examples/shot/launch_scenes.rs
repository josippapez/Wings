//! Agents and accounts: two accounts, first launch, other apps' data, older servers and a restored workspace.
use super::*;

/// Two agents in the sidebar, one under the default account and one under the named account `work` of the made-up home
/// `main` sets, which its row names; then with the sidebar dragged to its narrowest.
pub(super) fn accounts(cx: &mut VisualTestAppContext, base: &Config, fake_claude: &Path, out: &Path) {
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
        settle(cx, window, Duration::from_secs(3));
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
                settle(cx, window, Duration::from_secs(2));
            }
            cx.simulate_input(
                window,
                &format!("exec '{}' {seconds}", fake_claude.display()),
            );
            cx.simulate_keystrokes(window, "enter");
        }
        settle(cx, window, Duration::from_secs(1));
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
        settle(cx, window, Duration::from_secs(2));
        save(cx, window, &name("sidebar"));
        // What each agent just did, as the model sums it up, drawn before the next scan clears it.
        handle
            .update(cx, |workspace, _, cx| {
                workspace.show_agent_summaries(
                    &[
                        "Moved the retry logic into the client and added tests",
                        "Waiting for you to approve the redirect fix",
                    ],
                    cx,
                )
            })
            .expect("show made-up summaries");
        save(cx, window, &name("sidebar-summaries"));
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
            settle(cx, window, Duration::from_millis(20));
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
        settle(cx, window, Duration::from_millis(500));
        save(cx, window, &name("sidebar-narrow"));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
        let _ = std::process::Command::new("pkill")
            .args(["-f", &fake_claude.display().to_string()])
            .status();
        wait(cx, window, Duration::from_millis(300));
    }
}

/// The question a first launch asks, over a made-up list of the official plugins: never the real catalogue.
pub(super) fn first_launch(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
    let plugin = |id: &str, name: &str, description: &str| wings_plugins::store::Available {
        id: id.into(),
        name: name.into(),
        description: description.into(),
        version: "1.0.0".into(),
        link: format!("https://github.com/example/{id}"),
    };
    let list = vec![
        plugin("issues", "Issues", "A board of the project's issues, with the agent's work on each."),
        plugin("changes", "Changes", "What changed in the repo, file by file, with line counts."),
        plugin("pr-tracker", "PR tracker", "Open pull requests and whether their checks pass."),
        plugin("disk", "Disk usage", "What takes space on a disk or in a folder."),
        plugin("hud", "HUD", "Agent status in the title bar."),
    ];
    let mut config = base.clone();
    config.plugins = None;
    config.sessions = None;
    for width in [1000.0, 640.0] {
        let config = config.clone();
        let list = list.clone();
        let handle = cx
            .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                cx.new(|cx| Workspace::new(config, window, cx))
            })
            .expect("open an offscreen window");
        let window: AnyWindowHandle = handle.into();
        settle(cx, window, Duration::from_secs(1));
        handle
            .update(cx, |w, _, cx| w.official_plugins_listed(list, cx))
            .unwrap();
        wait(cx, window, Duration::from_millis(500));
        save(cx, window, &out.join(format!("first-launch-{width}.png")));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
        wait(cx, window, Duration::from_millis(300));
    }
}

/// The dialog that asks to turn on other apps' data, as a launch that finds the terminals refused it shows it, and the
/// row in Settings, General. The server is a made-up helper app in a temp folder that never starts.
pub(super) fn disk_access(cx: &mut VisualTestAppContext, base: &Config, fake: &Path, out: &Path) {
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
        settle(cx, window, Duration::from_secs(2));
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
        // Settings opens on General, the first section.
        settle(cx, window, Duration::from_millis(300));
        save(
            cx,
            window,
            &out.join(format!("disk-access-settings-{width}.png")),
        );
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
        wait(cx, window, Duration::from_millis(300));
    }
}

/// A server from before an update, still running a program in a pane of the project and one no pane shows, beside the
/// current server: in-process servers in a temp folder, which end with the renders.
pub(super) fn older_servers(cx: &mut VisualTestAppContext, base: &Config, fake: &Path, out: &Path) {
    use wings_native::{
        host::TerminalServers,
        terminal_server::{self, Servers},
    };
    use wings_ptyd::{SERVER_VERSION, client::Pane, host, proto::Spawn};
    for width in [1000.0, 640.0] {
        // The app drains the older server and closing the window ends its panes, so each width has servers of its own.
        let folder = PathBuf::from("/tmp").join(format!("wshot-{}-{width}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("make a server folder");
        for version in [SERVER_VERSION, SERVER_VERSION + 1] {
            let socket = terminal_server::socket(&folder, version);
            let listener = host::bind(&socket).expect("bind a server");
            let config = host::Config {
                version,
                ..host::Config::default()
            };
            thread::spawn(move || host::serve(listener, &socket, config));
        }
        let older = terminal_server::socket(&folder, SERVER_VERSION);
        for (space, history) in [
            (base.project.to_string_lossy().into_owned(), "shot-older"),
            ("/elsewhere".to_owned(), "shot-hidden"),
        ] {
            let spawn = Spawn {
                space_id: space,
                cwd: fake.to_owned(),
                cols: 80,
                rows: 24,
                history_id: Some(history.into()),
                replay_bytes: None,
                env: std::env::vars().collect(),
            };
            let (pane, _events) =
                Pane::spawn(&older, spawn, b"").expect("a pane on the older server");
            // A program runs, so nothing moves it to the current server.
            pane.write(b"sleep 600\r").unwrap();
            pane.detach();
        }
        thread::sleep(Duration::from_millis(500));
        let servers = Servers::connect_as(&folder, None, SERVER_VERSION + 1);
        let mut config = base.clone();
        config.plugins = None;
        config.sessions = None;
        config.terminal_server = Some(TerminalServers(std::sync::Arc::new(servers)));
        let handle = cx
            .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                cx.new(|cx| Workspace::new(config, window, cx))
            })
            .expect("open an offscreen window");
        let window: AnyWindowHandle = handle.into();
        settle(cx, window, Duration::from_secs(2));
        save(
            cx,
            window,
            &out.join(format!("older-servers-pane-{width}.png")),
        );
        cx.simulate_keystrokes(window, "cmd-,");
        wait(cx, window, Duration::from_millis(600));
        // Settings opens on General, the first section.
        wait(cx, window, Duration::from_millis(300));
        // Down to the terminal server's rows, at the end of the section.
        cx.simulate_event(
            window,
            gpui::ScrollEvent {
                position: point(px(width - 120.0), px(400.0)),
                delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(-3000.0))),
                ..Default::default()
            },
        );
        wait(cx, window, Duration::from_millis(300));
        save(
            cx,
            window,
            &out.join(format!("older-servers-settings-{width}.png")),
        );
        handle
            .update(cx, |w, window, cx| w.sample_stop_server(window, cx))
            .unwrap();
        settle(cx, window, Duration::from_millis(400));
        save(
            cx,
            window,
            &out.join(format!("older-servers-stop-{width}.png")),
        );
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
        wait(cx, window, Duration::from_millis(300));
        terminal_server::end_all(&folder);
        let _ = std::fs::remove_dir_all(&folder);
    }
}

/// The Agents list after a restart, from a made-up saved workspace: nothing it lists types into a shell, so no real
/// `claude` starts. Temp folders only.
pub(super) fn restored(
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
            asleep: false,
            claude_account: None,
            session: name.map(|name| SavedSession {
                kind: SessionKind::Claude,
                id: "0aa900ec-ed63-4df5-97c1-eecebeb8ef8d".into(),
                args: Vec::new(),
                name: Some(name.into()),
                launcher: None,
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
        settle(cx, window, Duration::from_secs(3));
        // A live agent in the first tab, for the rows beside it.
        cx.simulate_keystrokes(window, "cmd-1");
        settle(cx, window, Duration::from_millis(300));
        cx.simulate_input(window, &format!("exec '{}' 142", fake_claude.display()));
        cx.simulate_keystrokes(window, "enter");
        settle(cx, window, Duration::from_secs(1));
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
        settle(cx, window, Duration::from_secs(2));
        cx.simulate_keystrokes(window, "cmd-2");
        settle(cx, window, Duration::from_millis(500));
        save(cx, window, &out.join(format!("restored-{width}.png")));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
        let _ = std::process::Command::new("pkill")
            .args(["-f", &format!("{} 142", fake_claude.display())])
            .status();
        wait(cx, window, Duration::from_millis(300));
    }
}
