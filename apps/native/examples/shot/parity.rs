//! Settings' sections and themes, the parity pages, made-up tools and harnesses, and terminal images.
use super::*;

/// Settings' sections, the project bar's Open in editor with its menu, the quick terminal floating, minimised and filling
/// the window, and the welcome once the last project is removed. Made-up editors and the fixture plugin only.
pub(super) fn themes(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
    use wings_native::theme::Theme;
    let mut config = base.clone();
    config.projects = None;
    let cases = Theme::ALL
        .into_iter()
        .map(|theme| (theme, true))
        .chain([(Theme::Wings, false), (Theme::Nord, false)]);
    for (theme, transparent) in cases {
        for width in [1000.0, 640.0] {
            let config = config.clone();
            let handle = cx
                .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                    cx.new(|cx| Workspace::new(config, window, cx))
                })
                .expect("open an offscreen window");
            let window: AnyWindowHandle = handle.into();
            handle
                .update(cx, |workspace, _, cx| workspace.set_appearance(theme, transparent, cx))
                .expect("choose the theme");
            let state = format!("{}{}", theme.key(), if transparent { "" } else { "-solid" });
            settle(cx, window, Duration::from_secs(2));
            save(cx, window, &out.join(format!("{state}-{width}.png")));
            cx.simulate_keystrokes(window, "cmd-,");
            wait(cx, window, Duration::from_millis(300));
            let left = (width - 820.0f32.min(width - 64.0)) / 2.0;
            // The second section, Appearance: in the list of icons below 800 points, a row 68 apart.
            let row = if width < 800.0 { point(px(left + 28.0), px(98.0)) } else { point(px(left + 70.0), px(133.0 + 34.0)) };
            click(cx, window, row);
            settle(cx, window, Duration::from_millis(200));
            save(cx, window, &out.join(format!("{state}-settings-{width}.png")));
            if theme == Theme::Wings && transparent {
                // Terminal background, Custom colour: the swatches and hex row, and the terminal behind in that colour.
                handle
                    .update(cx, |workspace, _, cx| workspace.set_terminal_background(Some(0x1e1e2e), cx))
                    .expect("choose a custom terminal background");
                settle(cx, window, Duration::from_millis(300));
                save(cx, window, &out.join(format!("{state}-settings-custom-bg-{width}.png")));
            }
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
    }
    wings_native::theme::set(Default::default(), true);
}

pub(super) fn parity(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
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
        settle(cx, window, Duration::from_secs(2));
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
                let (tools, changelog) = made_up_tools();
                workspace.show_tools(tools, changelog, "2.1.329", cx);
                workspace.show_ai_status(
                    wings_agents::ai::Status {
                        engine: wings_agents::ai::Engine::Ollama,
                        ollama_model: Some("qwen3:4b".into()),
                        available: true,
                        reason: None,
                        models: vec!["llama3.2:latest".into(), "qwen3:4b".into()],
                    },
                    cx,
                );
            })
            .expect("show the made-up accounts");
        // Each section of Settings, by its row in the list: 32px rows, 2px apart, the first at 117 as in the desktop.
        cx.simulate_keystrokes(window, "cmd-,");
        wait(cx, window, Duration::from_millis(300));
        let left = (width - 820.0f32.min(width - 64.0)) / 2.0;
        let sections = [
            "general",
            "appearance",
            "text",
            "cursor",
            "history",
            "keyboard",
            "editor",
            "agents",
            "tools",
            "notifications",
            "plugins",
        ];
        for (index, section) in sections.into_iter().enumerate() {
            // Below 800 points the list shrinks to its icons, 56 wide.
            let (nav_x, first_row) = if width < 800.0 { (28.0, 65.0) } else { (70.0, 133.0) };
            let row = point(px(left + nav_x), px(first_row + 34.0 * index as f32));
            click(cx, window, row);
            settle(cx, window, Duration::from_millis(200));
            save(cx, window, &name(&format!("settings-{section}")));
            // Agents runs past the first screen: its names, summaries and sleep settings are further down.
            if section == "agents" || section == "tools" {
                cx.simulate_event(
                    window,
                    gpui::ScrollEvent {
                        position: point(px(width / 2.0 + 100.0), px(400.0)),
                        delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(if section == "tools" { -1400.0 } else { -560.0 }))),
                        modifiers: Modifiers::default(),
                        touch_phase: gpui::TouchPhase::Moved,
                        source: gpui::ScrollSource::Pointer,
                    },
                );
                settle(cx, window, Duration::from_millis(200));
                save(cx, window, &name(&format!("settings-{section}-scrolled")));
                // The next section starts at its top.
                cx.simulate_event(
                    window,
                    gpui::ScrollEvent {
                        position: point(px(width / 2.0 + 100.0), px(400.0)),
                        delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(2000.0))),
                        modifiers: Modifiers::default(),
                        touch_phase: gpui::TouchPhase::Moved,
                        source: gpui::ScrollSource::Pointer,
                    },
                );
                wait(cx, window, Duration::from_millis(100));
            }
            if section == "tools" {
                handle
                    .update(cx, |workspace, _, cx| {
                        workspace.show_tool_runs(
                            "claude",
                            "az",
                            "Error: sudo: a terminal is required to read the password\nError: azure-cli: failed to upgrade the cask",
                            cx,
                        )
                    })
                    .expect("show the made-up updates");
                wait(cx, window, Duration::from_millis(300));
                save(cx, window, &name("settings-tools-updating"));
                cx.simulate_event(
                    window,
                    gpui::ScrollEvent {
                        position: point(px(width / 2.0 + 100.0), px(400.0)),
                        delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(-380.0))),
                        modifiers: Modifiers::default(),
                        touch_phase: gpui::TouchPhase::Moved,
                        source: gpui::ScrollSource::Pointer,
                    },
                );
                wait(cx, window, Duration::from_millis(200));
                save(cx, window, &name("settings-tools-failed"));
            }
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
        settle(cx, window, Duration::from_secs(2));
        save(cx, window, &name("quick"));
        timed(cx, "hide");
        timed(cx, "show again");
        settle(cx, window, Duration::from_millis(300));
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
        settle(cx, window, Duration::from_millis(500));
        save(cx, window, &name("quick-full"));
        timed(cx, "hide");

        // The only project's remove button, at the right of its row while the pointer is over it, then Remove. The row is
        // under the nav's four rows and the quick terminal's row in the Terminals list, each 28px with 8px below, and
        // the 32px Projects heading.
        let project = 40.0 + 4.0 * 29.0 - 1.0 + 8.0 + 28.0 + 8.0 + 32.0 + 20.0;
        mouse_move(cx, window, point(px(60.0), px(project)), false);
        wait(cx, window, Duration::from_millis(200));
        click(cx, window, point(px(248.0 - 8.0 - 6.0 - 12.0), px(project)));
        settle(cx, window, Duration::from_millis(200));
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

/// Made-up command-line tools and Claude Code changelog for Settings, Tools: current, with an update by each way of
/// installing, one that isn't installed and one Wings doesn't know the source of.
pub(super) fn made_up_tools() -> (Vec<wings_native::tools::ToolStatus>, Vec<wings_native::tools::Entry>) {
    use wings_native::tools::{Entry, Source, ToolStatus};
    let tool = |program: &str, name: &str, uses: &str, installed: Option<&str>, latest: Option<&str>, source| ToolStatus {
        program: program.into(),
        name: name.into(),
        uses: uses.into(),
        found: installed.is_some(),
        installed: installed.map(Into::into),
        latest: latest.map(Into::into),
        source,
    };
    let tools = vec![
        tool("claude", "Claude Code", "Claude chat", Some("2.1.329"), Some("2.1.331"), Source::ClaudeInstaller),
        tool("gh", "GitHub CLI", "PR tracker, Changes, Tasks", Some("2.105.0"), Some("2.105.0"), Source::Homebrew("gh".into())),
        tool("opencode", "OpenCode", "Connecting OpenCode to Wings", Some("2.3.0"), Some("2.4.0"), Source::Homebrew("anomalyco/tap/opencode-v2".into())),
        tool("glab", "GitLab CLI", "PR tracker", None, None, Source::Unknown),
        tool("az", "Azure CLI", "PR tracker, Schedules, Work item", Some("2.92.0"), Some("2.93.0"), Source::HomebrewCask("azure-cli".into())),
        tool("git", "Git", "Branches and changes, PR tracker, Worktrees, and 4 more", Some("2.39.5"), None, Source::Unknown),
    ];
    let entry = |version: &str, notes: &[&str]| Entry {
        version: version.into(),
        notes: notes.iter().map(|n| (*n).into()).collect(),
    };
    let changelog = vec![
        entry("2.1.331", &["Added `/rewind` to step a session back to an earlier message", "Fixed a crash when resuming a session that had a very long tool result in its last message", "Plugins can now ship hooks"]),
        entry("2.1.330", &["Added the `--max-budget` flag", "Improved how permission prompts show long commands", "Fixed the status line flickering", "Fixed `claude update` on slow networks", "Added a Windows installer", "Improved startup time", "Fixed MCP servers not reconnecting"]),
        entry("2.1.329", &["Older note"]),
    ];
    (tools, changelog)
}

/// Made-up agent CLIs for Settings, Agents: one of each state the rows show.
pub(super) fn made_up_harnesses() -> Vec<wings_connect::harnesses::HarnessStatus> {
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
pub(super) fn images(cx: &mut VisualTestAppContext, config: &Config, fake: &Path, out: &Path) {
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
        settle(cx, window, Duration::from_secs(3));
        cx.simulate_input(window, &format!("clear; cat {}", fixture.display()));
        cx.simulate_keystrokes(window, "enter");
        settle(cx, window, Duration::from_secs(2));
        save(
            cx,
            window,
            &out.join(format!("terminal-images-{width}.png")),
        );
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
}
