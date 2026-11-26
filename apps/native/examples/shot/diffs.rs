//! The diff viewer: docked, stacked in the sidebar, and with review threads.
use super::*;

/// A main that opens the diff viewer with `diff` the first time it hears of a pane, and refuses each reply typed in it
/// with `refusal`.
pub(super) fn differ(diff: serde_json::Value, refusal: &'static str) -> String {
    fake::script("main", move || {
        let (diff, mut opened) = (diff.clone(), false);
        move |cx: &mut Cx<'_>, hear: Hear<'_>| match hear {
            Hear::Deliver("panes", _, panes) if !opened && panes != "[]" => {
                opened = true;
                cx.call("openDiff", diff.clone());
            }
            Hear::Deliver("reply", id, _) => {
                cx.call("actionDone", serde_json::json!({ "token": id, "error": refusal }));
            }
            _ => {}
        }
    })
}

/// The made-up pull request of `diff-dock` and `diff-comments`: three files.
pub(super) const DOCK_PATCH: &str = "diff --git a/src/config.rs b/src/config.rs\n--- a/src/config.rs\n+++ b/src/config.rs\n@@ -1,8 +1,10 @@\n use std::time::Duration;\n \n pub fn load() -> Config {\n-    let timeout = 10;\n+    // A slow network gets room to answer before the client gives up.\n+    let timeout = 30;\n+    validate(timeout);\n     Config {\n         timeout: Duration::from_secs(timeout),\n-        retries: 1,\n+        retries: 3,\n     }\n }\ndiff --git a/src/client/retry.rs b/src/client/retry.rs\nnew file mode 100644\n--- /dev/null\n+++ b/src/client/retry.rs\n@@ -0,0 +1,4 @@\n+pub fn backoff(attempt: u32) -> u64 {\n+    200 * 2u64.pow(attempt)\n+}\n+\ndiff --git a/README.md b/README.md\n--- a/README.md\n+++ b/README.md\n@@ -3,3 +3,3 @@\n ## Configuration\n-The client waits 10 seconds.\n+The client waits 30 seconds and retries three times.\n \n";

pub(super) fn diff_dock(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
    let data = Data::temp();
    data.install_script(
        "differ",
        serde_json::json!({ "name": "Changes", "contributes": { "ui": ["diff"] } }),
        &differ(
            serde_json::json!({ "title": "Fix the client timeout", "subtitle": "acme-web #42 · fix-timeout into main", "patch": DOCK_PATCH }),
            "This plugin doesn't take replies",
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
        settle(cx, window, Duration::from_millis(1500));
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

/// One file's part of a made-up change: lines both sides have, lines removed, and lines added.
pub(super) enum Part {
    Same(Vec<String>),
    Removed(Vec<String>),
    Added(Vec<String>),
}

/// The file `parts` make, and a git patch of them with 3 lines of context, as `git diff` writes one. `new` makes it an
/// added file.
pub(super) fn file_patch(path: &str, parts: &[Part], new: bool) -> (String, String) {
    let lines: Vec<(char, &str)> = parts
        .iter()
        .flat_map(|part| {
            let (mark, lines) = match part {
                Part::Same(lines) => (' ', lines),
                Part::Removed(lines) => ('-', lines),
                Part::Added(lines) => ('+', lines),
            };
            lines.iter().map(move |line| (mark, line.as_str()))
        })
        .collect();
    let text: String = lines
        .iter()
        .filter(|(mark, _)| *mark != '-')
        .map(|(_, line)| format!("{line}\n"))
        .collect();
    let mut patch = if new {
        format!("diff --git a/{path} b/{path}\nnew file mode 100644\n--- /dev/null\n+++ b/{path}\n")
    } else {
        format!("diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n")
    };
    let changed: Vec<usize> = (0..lines.len()).filter(|&at| lines[at].0 != ' ').collect();
    let mut at = 0;
    while at < changed.len() {
        // Changes 6 or fewer lines apart share a hunk.
        let mut last = at;
        while last + 1 < changed.len() && changed[last + 1] - changed[last] <= 7 {
            last += 1;
        }
        let (from, to) = (changed[at].saturating_sub(3), (changed[last] + 4).min(lines.len()));
        let before = &lines[..from];
        let number = |side: char| before.iter().filter(|(mark, _)| *mark != side).count();
        let count = |side: char| lines[from..to].iter().filter(|(mark, _)| *mark != side).count();
        let (old, new) = (count('+'), count('-'));
        let start = |len: usize, side: char| if len == 0 { 0 } else { number(side) + 1 };
        patch += &format!("@@ -{},{old} +{},{new} @@\n", start(old, '+'), start(new, '-'));
        for (mark, line) in &lines[from..to] {
            patch += &format!("{mark}{line}\n");
        }
        at = last + 1;
    }
    (text, patch)
}

/// `diff-sidebar`'s stacked change, in a made-up repository folder: `src/config.rs` changed in two places with
/// unchanged lines between and after them, `src/client/retry.rs` new, and `README.md` changed in its middle. Gives the
/// folder and the patch.
pub(super) fn stack_repo() -> (PathBuf, String) {
    let lines = |list: &[&str]| list.iter().map(|line| line.to_string()).collect::<Vec<_>>();
    let limits = |from: usize, to: usize| {
        (from..to)
            .flat_map(|n| [format!("pub fn limit_{n}() -> u64 {{"), format!("    {n} * 1_000"), "}".into()])
            .collect::<Vec<_>>()
    };
    let config = [
        Part::Same(lines(&["use std::time::Duration;", "", "pub fn load() -> Config {"])),
        Part::Removed(lines(&["    let timeout = 10;"])),
        Part::Added(lines(&[
            "    // A slow network gets room to answer before the client gives up.",
            "    let timeout = 30;",
            "    validate(timeout);",
        ])),
        Part::Same(lines(&["    Config {", "        timeout: Duration::from_secs(timeout),"])),
        Part::Removed(lines(&["        retries: 1,"])),
        Part::Added(lines(&["        retries: 3,"])),
        Part::Same([lines(&["    }", "}", ""]), limits(0, 10), lines(&["pub fn validate(timeout: u64) {"])].concat()),
        Part::Added(lines(&["    assert!(timeout > 0, \"a timeout of zero never answers\");"])),
        Part::Same([lines(&["}", ""]), limits(10, 14)].concat()),
    ];
    let retry = [Part::Added(lines(&[
        "pub fn backoff(attempt: u32) -> u64 {",
        "    200 * 2u64.pow(attempt)",
        "}",
    ]))];
    let readme_lines = |from: usize, to: usize| (from..to).map(|n| format!("Step {n} of the setup.")).collect::<Vec<_>>();
    let readme = [
        Part::Same([lines(&["# acme-web", ""]), readme_lines(1, 11), lines(&["", "## Configuration"])].concat()),
        Part::Removed(lines(&["The client waits 10 seconds."])),
        Part::Added(lines(&["The client waits 30 seconds and retries three times."])),
        Part::Same([lines(&[""]), readme_lines(11, 20)].concat()),
    ];
    let dir = std::env::temp_dir().join("wings-shot-stack");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".git")).expect("make the made-up repository");
    std::fs::create_dir_all(dir.join("src/client")).expect("make its folders");
    let mut patch = String::new();
    for (path, parts, new) in [
        ("src/config.rs", &config[..], false),
        ("src/client/retry.rs", &retry[..], true),
        ("README.md", &readme[..], false),
    ] {
        let (text, part) = file_patch(path, parts, new);
        std::fs::write(dir.join(path), text).expect("write a made-up file");
        patch += &part;
    }
    (dir, patch)
}

/// A diff opened from the Changes sidebar: every file stacked in the sidebar's place, widened over the panes, with the
/// unchanged lines between the hunks folded, and one fold opened.
pub(super) fn right_splits(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
    let data = Data::temp();
    data.install_script(
        "changes",
        serde_json::json!({ "name": "Changes", "contributes": {
            "sidebars": [{ "id": "side", "title": "Changes", "page": "dist/side.html" }],
        } }),
        &fake::idle("main"),
        &[("side", &words("3 changed files: src/config.rs, src/client/retry.rs, README.md"))],
        "changes-scope",
    );
    data.install_script(
        "agents",
        serde_json::json!({ "name": "Subagent", "contributes": {
            "sidebars": [{ "id": "agent", "title": "Subagent", "page": "dist/agent.html", "icon": "message" }],
        } }),
        &fake::idle("main"),
        &[("agent", &words("Reading src/client/retry.rs"))],
        "agents-scope",
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
        let name = |state: &str| out.join(format!("right-splits-{state}-{width}.png"));
        settle(cx, window, Duration::from_secs(1));
        handle
            .update(cx, |w, _, cx| w.shot_sidebar("changes", "side", cx))
            .unwrap();
        settle(cx, window, Duration::from_millis(500));
        save(cx, window, &name("one"));
        handle
            .update(cx, |w, _, cx| w.shot_split_sidebar("agents", "agent", cx))
            .unwrap();
        settle(cx, window, Duration::from_millis(500));
        save(cx, window, &name("two"));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
}

pub(super) fn pinned_bar(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
    let data = Data::temp();
    let titles = ["Changes", "Subagent", "Cyclops tracker", "Disk usage", "Pull requests", "Sprint board", "Claude chat"];
    for (n, title) in titles.iter().enumerate() {
        data.install_script(
            &format!("p{n}"),
            serde_json::json!({ "name": title, "contributes": {
                "sidebars": [{ "id": "s", "title": title, "page": "dist/s.html" }],
            } }),
            &fake::idle("main"),
            &[("s", &words(&format!("{title} page")))],
            &format!("p{n}-scope"),
        );
    }
    let mut config = base.clone();
    config.plugins = Some(data.paths());
    for (label, pins) in [("few", 2), ("many", 7)] {
        for width in [1600.0, 1000.0, 640.0] {
            let config = config.clone();
            let handle = cx
                .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                    cx.new(|cx| Workspace::new(config, window, cx))
                })
                .expect("open an offscreen window");
            let window: AnyWindowHandle = handle.into();
            settle(cx, window, Duration::from_secs(1));
            handle
                .update(cx, |w, _, cx| {
                    for n in 0..pins {
                        w.shot_pin(&format!("p{n}:s"), cx);
                    }
                    w.shot_sidebar("p0", "s", cx);
                })
                .unwrap();
            settle(cx, window, Duration::from_millis(800));
            save(cx, window, &out.join(format!("pinned-{label}-{width}.png")));
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
    }
}

pub(super) fn right_named(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
    let data = Data::temp();
    // A page that names its header "Feedback" as it starts, under the manifest's "Claude Code pane".
    let page = fake::script("page", || {
        let mut named = false;
        move |cx: &mut Cx<'_>, _: Hear<'_>| {
            if !std::mem::replace(&mut named, true) {
                cx.call("setTabTitle", serde_json::json!({ "title": "Feedback" }));
                cx.render(&serde_json::json!({ "id": 0, "type": "#root", "props": {}, "children": [{ "id": 1, "type": "#text", "text": "88 open · 11 high" }] }));
            }
        }
    });
    data.install_script(
        "claude-chat",
        serde_json::json!({ "name": "Claude chat", "contributes": {
            "sidebars": [{ "id": "pane", "title": "Claude Code pane", "page": "dist/pane.html" }],
        } }),
        &fake::idle("main"),
        &[("pane", &page)],
        "claude-chat-scope",
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
        settle(cx, window, Duration::from_secs(1));
        handle.update(cx, |w, _, cx| w.shot_sidebar("claude-chat", "pane", cx)).unwrap();
        settle(cx, window, Duration::from_millis(800));
        save(cx, window, &out.join(format!("right-named-{width}.png")));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
}

pub(super) fn diff_sidebar(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
    let (repo, patch) = stack_repo();
    let data = Data::temp();
    data.install_script(
        "changes",
        serde_json::json!({ "name": "Changes", "contributes": {
            "sidebars": [{ "id": "side", "title": "Changes", "page": "dist/side.html" }],
        } }),
        &fake::idle("main"),
        &[("side", &words("3 changed files: src/config.rs, src/client/retry.rs, README.md"))],
        "changes-scope",
    );
    let mut config = base.clone();
    config.plugins = Some(data.paths());
    for width in [1280.0, 1000.0, 640.0] {
        let config = config.clone();
        let handle = cx
            .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                cx.new(|cx| Workspace::new(config, window, cx))
            })
            .expect("open an offscreen window");
        let window: AnyWindowHandle = handle.into();
        let name = |state: &str| out.join(format!("diff-sidebar-{state}-{width}.png"));
        settle(cx, window, Duration::from_secs(1));
        handle
            .update(cx, |w, _, cx| w.shot_sidebar("changes", "side", cx))
            .unwrap();
        settle(cx, window, Duration::from_millis(500));
        save(cx, window, &name("list"));
        let payload = serde_json::json!({
            "patch": patch,
            "worktree": true,
            "scopes": [{ "id": "uncommitted", "label": "Uncommitted" }, { "id": "branch", "label": "Branch" }],
            "scope": "uncommitted",
        });
        let cwd = repo.to_string_lossy().into_owned();
        handle
            .update(cx, |w, window, cx| {
                w.shot_sidebar_stack("changes", "side", payload, &cwd, window, cx)
            })
            .unwrap();
        // The widening, and the files read for the folds.
        settle(cx, window, Duration::from_millis(2000));
        save(cx, window, &name("open"));
        // The fold between config.rs's hunks opens; README.md's stay closed.
        click(cx, window, spot("diff-fold-0-0"));
        settle(cx, window, Duration::from_millis(500));
        save(cx, window, &name("fold"));
        click(cx, window, spot("diff-scope"));
        settle(cx, window, Duration::from_millis(300));
        save(cx, window, &name("scopes"));
        cx.simulate_keystrokes(window, "escape");
        wait(cx, window, Duration::from_millis(300));
        click(cx, window, spot("diff-back"));
        wait(cx, window, Duration::from_millis(300));
        save(cx, window, &name("back"));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
}

/// `diff-dock`'s pull request with made-up review threads: one on an added line with a reply, one on a removed line, one
/// on code that changed since, and one on the whole pull request.
pub(super) fn diff_comments(cx: &mut VisualTestAppContext, base: &Config, out: &Path) {
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
        &differ(
            serde_json::json!({ "title": "Fix the client timeout", "subtitle": "acme-web #42 · fix-timeout into main", "patch": DOCK_PATCH, "comments": comments }),
            "GitHub refused the reply: you are not signed in.",
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
        settle(cx, window, Duration::from_millis(1500));
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
