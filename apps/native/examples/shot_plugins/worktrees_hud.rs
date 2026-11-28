//! The Worktrees sidebar, Claude accounts and the HUD.
use super::*;

/// The Worktrees sidebar, which is written in Rust (`plugins/worktrees/rust`), over a repo made for it: worktrees that
/// are merged, changed, locked, detached and gone. Then the card that asks before the merged one is removed.
pub(super) fn worktrees(out: PathBuf) {
    let dev = out.join("fixture-worktrees");
    let _ = std::fs::remove_dir_all(&dev);
    let root = dev.join("acme-web");
    std::fs::create_dir_all(&root).unwrap();
    let dev = dev.canonicalize().unwrap();
    let root = root.canonicalize().unwrap();
    let git = |dir: &std::path::Path, days_ago: u64, args: &[&str]| {
        let date = format!("{} +0000", now_ms() / 1000 - days_ago * 86_400);
        let done = std::process::Command::new("git")
            .args(["-c", "user.email=test@example.com", "-c", "user.name=Wings", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .envs([("GIT_AUTHOR_DATE", &date), ("GIT_COMMITTER_DATE", &date)])
            .output()
            .unwrap();
        assert!(done.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&done.stderr));
    };
    let commit = |dir: &std::path::Path, days_ago: u64, file: &str| {
        std::fs::write(dir.join(file), file).unwrap();
        git(dir, days_ago, &["add", file]);
        git(dir, days_ago, &["commit", "-qm", file]);
    };
    git(&root, 0, &["init", "-q", "-b", "main"]);
    commit(&root, 60, "README.md");
    let linked = |name: &str, branch: &str| {
        let path = dev.join(name);
        git(&root, 0, &["worktree", "add", "-q", "-b", branch, path.to_str().unwrap()]);
        path
    };
    let search = linked("acme-web-search", "feature/search");
    commit(&search, 5, "search.ts");
    git(&root, 5, &["merge", "-q", "feature/search"]);
    commit(&root, 1, "CHANGELOG.md");
    let checkout = linked("acme-web-checkout", "feature/checkout");
    commit(&checkout, 2, "checkout.ts");
    std::fs::write(checkout.join("cart.ts"), "not committed").unwrap();
    let hotfix = linked("acme-web-hotfix", "fix/cart-total");
    commit(&hotfix, 12, "cart.ts");
    git(&root, 0, &["worktree", "lock", "--reason", "release freeze", hotfix.to_str().unwrap()]);
    let bisect = dev.join("acme-web-bisect");
    git(&root, 0, &["worktree", "add", "-q", "--detach", bisect.to_str().unwrap()]);
    std::fs::remove_dir_all(linked("acme-web-old", "experiment/old")).unwrap();

    let manifest = serde_json::from_str(include_str!("../../../../plugins/worktrees/wings-plugin.json")).unwrap();
    let storage = out.join("fixture-storage-worktrees");
    let _ = std::fs::remove_dir_all(&storage);
    let host = PluginHost::with_access(
        PluginConfig { id: "worktrees".into(), root: storage, scope: "shot".into() },
        Access {
            plugin: wings_plugins::manifest::Plugin { manifest, dir: root.clone(), icon_svg: None },
            builtin: true,
            env: std::sync::Arc::new(FixtureEnv(dev.clone())),
        },
    );
    host.publish_projects(vec![wings_native::page::plugin::Project { hidden_sections: Vec::new(),
        id: root.to_string_lossy().into(),
        path: root.to_string_lossy().into(),
        name: "acme-web".into(),
        parent_id: None,
    }]);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/worktrees/dist/native/sidebar.wasm");
    let source = format!("{}{}", wings_native::page::runtime::wasm::MARKER, path.canonicalize().expect("sh build-rust.sh worktrees, in plugins/").display());
    let mut cx = app();
    for width in [1000.0, 640.0, 360.0] {
        let handle = cx
            .open_offscreen_window(size(px(width), px(1100.0)), |_, cx| cx.new(|cx| PageView::in_plugin(&host, &source, cx)))
            .unwrap();
        let window = handle.into();
        handle.read_with(&cx, |view, _| view.set_visible(true)).unwrap();
        let shown = |cx: &mut VisualTestAppContext, expected: &str, name: &str| {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !handle.read_with(cx, |view, _| view.text().contains(expected)).unwrap() {
                assert!(Instant::now() < deadline, "page did not show {expected:?}: {}", handle.read_with(cx, |view, _| view.text()).unwrap());
                wait(cx, window, Duration::from_millis(20));
            }
            wait(cx, window, Duration::from_millis(300));
            save(cx, window, out.join(format!("worktrees-{name}-{width}.png")));
        };
        shown(&mut cx, "uncommitted", "list");
        assert!(handle.read_with(&cx, |view, _| view.press("Remove 1 merged & clean", 0)).unwrap());
        shown(&mut cx, "Also delete its branch", "confirm");
        assert!(handle.read_with(&cx, |view, _| view.text().contains("Remove 1 merged & clean")).unwrap());
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
}

/// The HUD's sidebar, which is written in Rust (`plugins/hud/rust`): two Claude sessions on two accounts, with the
/// second one's pane selected, then no session at all.
/// Claude accounts' sidebar and popover over two accounts, the second signed out.
pub(super) fn claude_accounts(out: PathBuf) {
    unsafe { std::env::set_var("SHOT_SIGNED_OUT", "work") };
    let manifest = serde_json::from_str(include_str!("../../../../plugins/claude-accounts/wings-plugin.json")).unwrap();
    let root = out.join("fixture-claude-accounts");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let host = PluginHost::with_access(
        PluginConfig { id: "claude-accounts".into(), root: out.join("fixture-storage-claude-accounts"), scope: "shot".into() },
        Access { plugin: wings_plugins::manifest::Plugin { manifest, dir: root.clone(), icon_svg: None }, builtin: false, env: std::sync::Arc::new(FixtureEnv(root.clone())) },
    );
    let project = root.to_string_lossy().into_owned();
    host.publish_projects(vec![wings_native::page::plugin::Project { hidden_sections: Vec::new(), id: project.clone(), name: "shop".into(), parent_id: None, path: project }]);
    host.publish_project(Some(&root));
    let mut cx = app();
    for width in [1000.0, 640.0] {
        for (frame, expected) in [("sidebar", "Not signed in"), ("popover", "is not signed in")] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../plugins/claude-accounts/dist/native/{frame}.wasm"));
            let source = format!("{}{}", wings_native::page::runtime::wasm::MARKER, path.canonicalize().expect("build the claude-accounts plugin first").display());
            let handle = cx.open_offscreen_window(size(px(width), px(700.0)), |_, cx| cx.new(|cx| PageView::in_plugin(&host, &source, cx))).unwrap();
            let window = handle.into();
            handle.read_with(&cx, |view, _| view.set_visible(true)).unwrap();
            let deadline = Instant::now() + Duration::from_secs(30);
            while !handle.read_with(&cx, |view, _| view.text().contains(expected)).unwrap() {
                assert!(Instant::now() < deadline, "page did not show {expected:?}: {}", handle.read_with(&cx, |view, _| view.text()).unwrap());
                wait(&mut cx, window, Duration::from_millis(20));
            }
            wait(&mut cx, window, Duration::from_millis(300));
            save(&mut cx, window, out.join(format!("claude-accounts-{frame}-{width}.png")));
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
    }
}

pub(super) fn hud(out: PathBuf) {
    const FIRST: &str = "3c21b0c5-8113-4f55-9493-fa156c3fa369";
    const SECOND: &str = "7d0e4a11-52c6-4b7e-9d57-0c2f6a1b9e44";
    let root = out.join("fixture-hud");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("projects/-work-web")).unwrap();
    let transcript = |tokens: u64, servers: &[&str], failed: serde_json::Value, skills: &[&str]| {
        let at = iso(60_000);
        let lines = [
            json!({ "type": "attachment", "uuid": "a1", "timestamp": at, "attachment": { "type": "model", "identity": { "modelId": "claude-opus-5-5" } } }),
            json!({ "type": "attachment", "uuid": "a2", "timestamp": at, "attachment": { "type": "deferred_tools_delta",
                "addedNames": servers.iter().flat_map(|server| [format!("mcp__{server}__read"), format!("mcp__{server}__write")]).collect::<Vec<_>>(),
                "removedNames": [], "failedMcpServers": failed, "pendingMcpServers": ["sentry"] } }),
            json!({ "type": "attachment", "uuid": "a3", "timestamp": at, "attachment": { "type": "invoked_skills",
                "skills": skills.iter().map(|name| json!({ "name": name })).collect::<Vec<_>>() } }),
            json!({ "type": "assistant", "uuid": "r1", "timestamp": at, "message": {
                "usage": { "input_tokens": 1000, "cache_creation_input_tokens": 2000, "cache_read_input_tokens": tokens },
                "content": [{ "type": "tool_use", "id": "t1", "name": format!("mcp__{}__read", servers[0]), "input": {} }] } }),
        ];
        lines.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n")
    };
    std::fs::write(
        root.join(format!("projects/-work-web/{FIRST}.jsonl")),
        transcript(113_000, &["github", "linear"], json!([{ "name": "postgres", "error": "connection refused: 127.0.0.1:5432" }]), &["wings-page-design", "wings-plugin"]),
    )
    .unwrap();
    std::fs::write(
        root.join(format!("projects/-work-web/{SECOND}.jsonl")),
        transcript(43_000, &["github", "playwright-browser-automation-with-a-long-name"], json!([]), &["dev-core:opensrc", "verified:verified-replay-with-a-very-long-skill-name"]),
    )
    .unwrap();
    let hours = |n: u64| (now_ms() / 1000 + n * 3600) as f64;
    let limits = |account: serde_json::Value, five: u64, week: u64| {
        json!({ "account": account, "at": now_ms(),
            "fiveHour": { "usedPercentage": five, "resetsAt": hours(2) }, "sevenDay": { "usedPercentage": week, "resetsAt": hours(60) } })
    };
    *STATUSLINE.lock().unwrap() = Some(json!({
        "rateLimits": [limits(serde_json::Value::Null, 12, 61), limits("work".into(), 88, 34)],
        "sessions": { SECOND: { "account": "work", "model": "Opus 5.5", "contextWindowSize": 200_000, "usedPercentage": 23,
            "totalInputTokens": null, "totalOutputTokens": null, "currentUsage": null,
            "promptCache": { "warm": true, "ttl": "1h", "expiresAt": hours(1) }, "at": now_ms() } },
    }));

    let manifest = serde_json::from_str(include_str!("../../../../plugins/hud/wings-plugin.json")).unwrap();
    let storage = out.join("fixture-storage-hud");
    let _ = std::fs::remove_dir_all(&storage);
    let host = PluginHost::with_access(
        PluginConfig { id: "hud".into(), root: storage, scope: "shot".into() },
        Access {
            plugin: wings_plugins::manifest::Plugin { manifest, dir: root.clone(), icon_svg: None },
            builtin: false,
            env: std::sync::Arc::new(FixtureEnv(root.clone())),
        },
    );
    let started_at = now_ms() - 10 * 60_000;
    let pane = |id: &str, session: &str, project: &str, name: &str, focused: bool| PaneEntry {
        pane_id: id.into(),
        cwd: "/work/web".into(),
        command: "claude".into(),
        project: project.into(),
        project_id: "/work/web".into(),
        session: Some(SessionEntry { session_id: session.into(), kind: "claude".into(), name: Some(name.into()), state: "idle".into(), started_at, launcher: None }),
        focused,
    };
    let both = vec![pane("1", FIRST, "web", "checkout redesign", false), pane("2", SECOND, "api", "invoice export", true)];
    let module = |frame: &str| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../plugins/hud/dist/native/{frame}.wasm"));
        format!("{}{}", wings_native::page::runtime::wasm::MARKER, path.canonicalize().expect("sh build-rust.sh hud, in plugins/").display())
    };
    host.publish_panes(both.clone());
    host.start_main(&module("main"), |event| {
        if let wings_native::page::runtime::Event::Error { message, .. } = event {
            panic!("{message}");
        }
    })
    .unwrap();
    let mut cx = app();
    let source = module("sidebar");
    // The second session's pane is selected, so its session shows and its account's limits come first.
    // Last, the first session's pane: Claude Code never reported that session, which the page says once it has had
    // the time to.
    let unreported = vec![pane("1", FIRST, "web", "checkout redesign", true), pane("2", SECOND, "api", "invoice export", false)];
    let views: [(&str, Vec<PaneEntry>, &str); 3] =
        [("selected", both, "46k of 200k tokens"), ("none", vec![], "No Claude session in your panes."), ("unreported", unreported, "isn't connected to Wings")];
    for width in [1000.0, 640.0, 360.0] {
        for (name, panes, expected) in &views {
            host.publish_panes(panes.clone());
            let handle = cx
                .open_offscreen_window(size(px(width), px(900.0)), |_, cx| cx.new(|cx| PageView::in_plugin(&host, &source, cx)))
                .unwrap();
            let window = handle.into();
            handle.read_with(&cx, |view, _| view.set_visible(true)).unwrap();
            let deadline = Instant::now() + Duration::from_secs(30);
            while !handle.read_with(&cx, |view, _| view.text().contains(expected)).unwrap() {
                assert!(Instant::now() < deadline, "page did not show {expected:?}: {}", handle.read_with(&cx, |view, _| view.text()).unwrap());
                wait(&mut cx, window, Duration::from_millis(20));
            }
            wait(&mut cx, window, Duration::from_millis(300));
            save(&mut cx, window, out.join(format!("hud-{name}-{width}.png")));
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
    }
}
