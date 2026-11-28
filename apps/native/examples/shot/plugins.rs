//! Installing plugins, dropping one, a plugin's modal and its terminals.
use super::*;

pub(super) fn plugins_install(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
    let data = Data::temp();
    data.install("demo", WINGS_VERSION, true, true, Approval::Current, "shot");
    data.install_script(
        "tracker",
        serde_json::json!({ "name": "Pull requests", "version": "1.2.0", "description": "Shows each pane's pull request and its checks.",
            "permissions": { "exec": ["gh pr view", "gh pr checks"], "notify": true, "fetch": ["https://api.github.com/"] },
            "contributes": { "ui": ["badges"], "mcpTools": [{ "name": "pr_status" }] } }),
        &fake::idle("main"),
        &[],
        "tracker-scope",
    );
    data.install_script(
        "notes",
        serde_json::json!({ "name": "Notes", "version": "0.3.0", "description": "Keeps notes beside each project.",
            "permissions": { "panes": [], "sendToAgents": true, "fetch": ["https://notes.example.com/", "http://127.0.0.1:4096/"],
                "secrets": { "notes-token": "https://notes.example.com/" } },
            "contributes": { "sidebars": [{ "id": "notes", "title": "Notes", "page": "dist/notes.html" }] } }),
        &fake::idle("main"),
        &[("notes", &fake::idle("notes"))],
        "notes-scope",
    );
    // Built for JavaScript: a main and a page that are scripts, which Wings no longer runs.
    let old = data.install_script(
        "old-board",
        serde_json::json!({ "name": "Old board", "version": "0.4.0", "description": "A board written in JavaScript.",
            "contributes": { "sidebars": [{ "id": "side", "title": "Board", "page": "dist/side.html" }] } }),
        &fake::idle("main"),
        &[("side", &fake::idle("side"))],
        "old-board-scope",
    );
    for stem in ["main", "side"] {
        let _ = std::fs::remove_file(old.join(format!("dist/native/{stem}.wasm")));
        std::fs::write(old.join(format!("dist/native/{stem}.js")), "wings.render('{}');").unwrap();
    }
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
        settle(cx, window, Duration::from_secs(1));
        let show = |cx: &mut VisualTestAppContext, selected: &'static str| {
            handle
                .update(cx, |w, window, cx| {
                    w.close_dialogs(cx);
                    w.show_plugins(selected, &[("tracker", "1.3.0")], window, cx)
                })
                .unwrap();
            settle(cx, window, Duration::from_millis(300));
        };
        let list = |cx: &mut VisualTestAppContext, tab: &'static str, query: &'static str, menu: Option<&'static str>| {
            let entry = |id: &str, name: &str, description: &str| wings_plugins::store::Available {
                id: id.into(),
                name: name.into(),
                description: description.into(),
                version: "1.0.0".into(),
                link: format!("https://github.com/example/{id}"),
            };
            let available = vec![
                entry("issues", "Issues", "A board of the project's issues, with the agent's work on each."),
                entry("changes", "Changes", "What changed in the repo, file by file, with line counts."),
                entry("disk", "Disk usage", "What takes space on a disk or in a folder."),
            ];
            handle
                .update(cx, |w, window, cx| {
                    w.close_dialogs(cx);
                    w.show_plugins_list(tab, query, menu, available, &[("tracker", "1.3.0")], window, cx)
                })
                .unwrap();
            wait(cx, window, Duration::from_millis(400));
        };
        list(cx, "installed", "", None);
        save(cx, window, &name("plugins-list"));
        list(cx, "installed", "board", None);
        save(cx, window, &name("plugins-search"));
        list(cx, "installed", "", Some("tracker"));
        save(cx, window, &name("plugins-row-menu"));
        list(cx, "installed", "", Some("add"));
        save(cx, window, &name("plugins-add-menu"));
        list(cx, "browse", "", None);
        save(cx, window, &name("plugins-browse"));
        list(cx, "updates", "", None);
        save(cx, window, &name("plugins-updates-tab"));
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
        settle(cx, window, Duration::from_millis(300));
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
        show(cx, "old-board");
        save(cx, window, &name("plugins-javascript"));
        handle.update(cx, |w, _, cx| w.close_dialogs(cx)).unwrap();
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("plugins-javascript-sidebar"));
        show(cx, "demo");
        let left = (width - 820.0f32.min(width * 0.92)) / 2.0;
        click(cx, window, point(px(left + 70.0), px(150.0 + 34.0 * 6.0)));
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("settings-agents"));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
        wait(cx, window, Duration::from_millis(300));
    }
}

pub(super) fn setup_existing_worktree(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
    for width in [1000.0, 640.0] {
        let config = base.clone();
        let handle = cx
            .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                cx.new(|cx| Workspace::new(config, window, cx))
            })
            .expect("open worktree setup fixture");
        let window: AnyWindowHandle = handle.into();
        settle(cx, window, Duration::from_secs(1));
        handle
            .update(cx, |w, _, cx| {
                w.show_existing_worktree_setup_approval(
                    "acme-web",
                    "/work/acme-web-worktrees/fix-login",
                    &[".claude/rules/project.md", ".mcp.json", ".env"],
                    Some("pnpm install"),
                    cx,
                )
            })
            .unwrap();
        wait(cx, window, Duration::from_millis(1300));
        wait_fake(cx, window, Duration::from_secs(1));
        save(cx, window, &out.join(format!("setup-existing-worktree-{width}.png")));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
}

/// A `.wings-plugin` file dragged over the window, the review it opens once dropped, and an installed plugin's page with
/// its README. Temp folders only.
/// The sidebar's Agents with a made-up chat tab's agent working beside the terminals.
pub(super) fn plugins_agent(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
    use serde_json::json;
    let data = Data::temp();
    let main = fake::script("main", || {
        let mut opened = false;
        move |cx: &mut Cx<'_>, hear: Hear<'_>| {
            if let Hear::Deliver("panes", _, _) = hear
                && !std::mem::replace(&mut opened, true)
            {
                cx.call("openView", json!({ "id": "chat", "as": "tab", "newTab": true }));
            }
        }
    });
    let page = fake::script("page", || {
        let mut said = false;
        move |cx: &mut Cx<'_>, _: Hear<'_>| {
            if !std::mem::replace(&mut said, true) {
                cx.call("setAgent", json!({ "agent": { "state": "working", "name": "Fix the rounding in checkout", "summary": "Running pnpm test discount" } }));
            }
        }
    });
    data.install_script(
        "chat",
        json!({ "name": "Claude chat", "contributes": { "ui": ["agents"], "views": [{ "id": "chat", "title": "Claude",
            "page": "dist/chat.html", "opens": ["tab"], "newTab": true, "button": false }] } }),
        &main,
        &[("chat", &page)],
        "chat-scope",
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
        wait(cx, window, Duration::from_secs(3));
        save(cx, window, &out.join(format!("plugins-agent-{width}.png")));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
}

/// A made-up chat in a tab, laid out as Claude chat's: its message on the left, and on the right the terminal its agent
/// opened with Wings' `open_pane`, which Wings draws live there. An issue tracker that is on makes the key in the message a
/// link.
pub(super) fn plugins_terminals(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
    use serde_json::json;
    let data = Data::temp();
    let main = fake::script("main", || {
        let mut opened = false;
        move |cx: &mut Cx<'_>, hear: Hear<'_>| {
            if let Hear::Deliver("panes", _, _) = hear
                && !std::mem::replace(&mut opened, true)
            {
                cx.call("openView", json!({ "id": "chat", "as": "tab", "newTab": true }));
            }
        }
    });
    let page = fake::script("page", || {
        move |cx: &mut Cx<'_>, hear: Hear<'_>| {
            let panes: Vec<String> = match hear {
                Hear::Start => Vec::new(),
                Hear::Deliver("terminals", _, text) => serde_json::from_str::<serde_json::Value>(text)
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|terminal| terminal["paneId"].as_str().unwrap().to_owned())
                    .collect(),
                _ => return,
            };
            let terminals: Vec<serde_json::Value> = panes
                .iter()
                .enumerate()
                .map(|(at, pane)| json!({ "id": 100 + at, "type": "Terminal", "props": { "pane": pane, "height": 240 } }))
                .collect();
            let said = "The build for ABC-12 runs in the terminal beside the chat. I'll tell you when it finishes.";
            cx.render(&json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [
                { "id": 1, "type": "Page", "props": { "flush": true }, "children": [
                    { "id": 2, "type": "Slot", "props": { "name": "body" }, "children": [
                        { "id": 3, "type": "Split", "props": { "side": "end", "defaultSize": 260, "collapseBelow": 1100, "pinned": !panes.is_empty() }, "children": [
                            { "id": 4, "type": "Slot", "props": { "name": "start" }, "children": [
                                { "id": 5, "type": "Scroll", "props": {}, "children": [
                                    { "id": 6, "type": "Stack", "props": { "gap": "l", "maxWidth": 760, "links": true }, "children": [
                                        { "id": 7, "type": "Text", "props": { "grow": true }, "children": [{ "id": 8, "type": "#text", "text": said }] }
                                    ] }
                                ] }
                            ] },
                            { "id": 9, "type": "Slot", "props": { "name": "end" }, "children": terminals }
                        ] }
                    ] }
                ] }
            ] }));
        }
    });
    data.install_script(
        "chat",
        json!({ "name": "Claude chat", "contributes": { "ui": ["agents", "terminals"], "views": [{ "id": "chat", "title": "Claude",
            "page": "dist/chat.html", "opens": ["tab"], "newTab": true, "button": false }] } }),
        &main,
        &[("chat", &page)],
        "chat-scope",
    );
    data.install_script(
        "tracker",
        json!({ "name": "Tracker", "contributes": {
            "views": [{ "id": "issue", "title": "Issue", "page": "dist/issue.html", "opens": ["tab"], "button": false }],
            "links": [{ "id": "issue", "title": "Issue", "pattern": "[A-Z]{1,3}-[0-9]{1,9}", "view": "issue", "arg": "key" }] } }),
        &fake::idle("main"),
        &[("issue", &fake::idle("page"))],
        "tracker-scope",
    );
    let mut config = base.clone();
    config.plugins = Some(data.paths());
    for width in [1280.0, 640.0] {
        let config = config.clone();
        let handle = cx
            .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                cx.new(|cx| Workspace::new(config, window, cx))
            })
            .expect("open an offscreen window");
        let window: AnyWindowHandle = handle.into();
        wait(cx, window, Duration::from_secs(2));
        handle
            .update(cx, |workspace, window, cx| {
                let tab = workspace.plugin_tab_ids()[0];
                workspace.sample_page_terminal(tab, "for step in fetch compile link; do echo $step; done", window, cx);
            })
            .expect("the chat's agent opens a terminal");
        settle(cx, window, Duration::from_secs(2));
        save(cx, window, &out.join(format!("plugins-terminals-{width}.png")));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
}

/// A plugin's view that opens as a modal, over the window: a made-up "Allow this?" page, as Schedules asks before an agent
/// adds a schedule.
pub(super) fn plugins_modal(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
    use serde_json::json;
    let data = Data::temp();
    let main = fake::script("main", || {
        let mut asked = false;
        move |cx: &mut Cx<'_>, hear: Hear<'_>| {
            if let Hear::Deliver("panes", _, _) = hear
                && !std::mem::replace(&mut asked, true)
            {
                cx.call("openView", json!({ "id": "approve", "as": "modal", "newTab": false }));
            }
        }
    });
    let page = fake::page("page", |_| {
        let text = |id: u64, text: &str, props: serde_json::Value| {
            json!({ "id": id, "type": "Text", "props": props, "children": [{ "id": id + 1, "type": "#text", "text": text }] })
        };
        let button = |id: u64, label: &str, variant: &str| {
            json!({ "id": id, "type": "Button", "props": { "variant": variant }, "children": [{ "id": id + 1, "type": "#text", "text": label }] })
        };
        json!({ "id": 0, "type": "#root", "props": {}, "children": [
            { "id": 1, "type": "Stack", "props": { "gap": "m" }, "children": [
                text(2, "An agent asks to add a schedule", json!({ "weight": "strong" })),
                text(4, "Claude in shop · Once, 16 Dec 2026 at 12:00", json!({ "tone": "muted" })),
                { "id": 6, "type": "Card", "props": {}, "children": [
                    text(7, "Check the Ministry's answers to the consultation comments on the price list rules.", json!({})) ] },
                { "id": 9, "type": "Row", "props": { "gap": "s", "justify": "end" }, "children": [
                    button(10, "Deny", "outline"), button(12, "Allow", "default") ] } ] } ] })
    });
    data.install_script(
        "asker",
        json!({ "name": "Schedules", "contributes": { "views": [{ "id": "approve", "title": "Allow this?",
            "page": "dist/approve.html", "opens": ["modal"], "width": 560, "height": 420, "button": false }] } }),
        &main,
        &[("approve", &page)],
        "asker-scope",
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
        settle(cx, window, Duration::from_secs(4));
        save(cx, window, &out.join(format!("plugins-modal-{width}.png")));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
}

/// The plugins store of one pass: a pass drops the same package, so each starts from a store that hasn't installed it, as
/// the first does. Installed already, the drop replaces it and forgets its web data, which asks WebKit for its stores and
/// crashes in a harness that never started WebKit's run loop (it does in the app).
fn drop_fixture() -> (Data, std::path::PathBuf) {
    use std::io::Write;
    let data = Data::temp();
    data.install_script(
        "tracker",
        serde_json::json!({ "name": "Pull requests", "version": "1.2.0", "description": "Shows each pane's pull request and its checks.",
            "permissions": { "exec": ["gh pr view", "gh pr checks"], "notify": true },
            "contributes": { "ui": ["badges"] } }),
        &fake::idle("main"),
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
    (data, package)
}

pub(super) fn plugins_drop(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
    for width in [1000.0, 640.0] {
        let (data, package) = drop_fixture();
        let mut config = base.clone();
        config.plugins = Some(data.paths());
        let handle = cx
            .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                cx.new(|cx| Workspace::new(config, window, cx))
            })
            .expect("open an offscreen window");
        let window: AnyWindowHandle = handle.into();
        let name = |state: &str| out.join(format!("{state}-{width}.png"));
        settle(cx, window, Duration::from_secs(1));
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
