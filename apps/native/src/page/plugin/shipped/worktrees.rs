//! Changes and Worktrees, as the app runs them.
use super::*;

#[test]
fn changes_lists_what_changed_opens_a_files_diff_through_main_and_searches_the_history() {
    let rig = Rig::with(load("changes"), false, vec![]);
    let (root, _) = worktree_repo(&rig);
    fs::write(root.join("README.md"), "fixture\nmore\n").unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/new.txt"), "one\ntwo\n").unwrap();
    *rig.env.projects.lock().unwrap() = vec![root.clone()];
    rig.host.publish_projects(vec![Project {
        id: root.to_string_lossy().into(),
        path: root.to_string_lossy().into(),
        name: "Fixture".into(),
        parent_id: None, hidden_sections: Vec::new(),
    }]);
    rig.host.publish_panes(vec![pane("1", &root.to_string_lossy())]);
    let (on_event, main_errors) = errors();
    rig.host.start_main(&module("changes", "main"), on_event).unwrap();
    // Main badges the pane with what changed in its repo.
    rig.env.wait("the badge", |seen| seen.iter().any(|seen| matches!(seen, Seen::Badge(_, _, Some(_)))));

    let (page, events) = open_page(&rig.host, &module("changes", "sidebar"));
    let files = tree(&events, "the changed files", |tree| tree.text().contains("README.md") && tree.text().contains("new.txt"));
    assert!(files.text().contains("2 files") && files.text().contains("+3"), "{}", files.text());
    // A file's row asks main for its diff, and main fills the viewer with the patch.
    let readme = handler_of(&files.children, &|node| match node {
        Node::ListButton(row) if plain_text(&row.button.children).contains("README.md") => row.button.on_click.clone(),
        _ => None,
    });
    assert!(page.dispatch(&readme.expect("README.md's row"), None));
    rig.env.wait("the diff viewer", |seen| seen.iter().any(|seen| matches!(seen, Seen::DiffUpdate(_, _, update) if update.patch.is_some())));

    // The History tab, and a search that matches nothing. The header's Files and Graph are chips too.
    let chips = handler_of(&files.children, &|node| match node {
        Node::Chips(chips) if chips.options.iter().any(|option| option.value == "history") => chips.on_change.clone(),
        _ => None,
    });
    assert!(page.dispatch(&chips.expect("the chips"), Some(&json!({ "value": "history" }))));
    let history = tree(&events, "the history", |tree| tree.text().contains("fixture") && tree.text().contains("All branches"));
    let search = handler_of(&history.children, &|node| match node {
        Node::SearchField(field) => field.on_input.clone(),
        _ => None,
    });
    assert!(page.dispatch(&search.expect("the search box"), Some(&json!({ "value": "not-a-commit" }))));
    tree(&events, "no match", |tree| tree.text().contains("No commits match."));

    // The graph view asks main which repo it shows, and reads every branch of it.
    let (_graph, events) = open_page(&rig.host, &module("changes", "graph"));
    tree(&events, "the graph", |tree| tree.text().contains("1 commits on every branch"));
    assert!(main_errors.try_recv().is_err());
}

#[test]
fn changes_shows_its_sidebar_when_the_claude_chat_asks_and_not_for_another_sender() {
    let rig = Rig::with(load("changes"), true, vec![]);
    let (on_event, main_errors) = errors();
    rig.host.start_main(&module("changes", "main"), on_event).unwrap();
    let opened = |rig: &Rig| rig.env.seen().iter().filter(|seen| matches!(seen, Seen::OpenView(open) if open.id == "changes")).count();
    rig.host.hear_event(&json!({ "from": "other", "name": "open-changes", "data": null }).to_string());
    rig.host.hear_event(&json!({ "from": "claude-chat", "name": "open-changes", "data": null }).to_string());
    rig.env.wait("the sidebar", |seen| seen.iter().any(|seen| matches!(seen, Seen::OpenView(open) if open.id == "changes")));
    assert_eq!(opened(&rig), 1);
    assert!(main_errors.try_recv().is_err());
}

#[test]
fn worktrees_sidebar_loads_the_actual_native_bundle_refreshes_on_git_and_opens_a_linked_worktree() {
    let rig = Rig::with(load("worktrees"), true, vec![]);
    let (root, linked) = worktree_repo(&rig);
    *rig.env.projects.lock().unwrap() = vec![root.clone()];
    rig.host.publish_projects(vec![Project {
        id: root.to_string_lossy().into(),
        path: root.to_string_lossy().into(),
        name: "Fixture worktrees".into(),
        parent_id: None, hidden_sections: Vec::new(),
    }]);
    rig.host
        .publish_panes(vec![pane("1", &root.to_string_lossy())]);

    let (page, events) = open_page(&rig.host, &module("worktrees", "sidebar"));
    tree(&events, "the initial main worktree", |tree| {
        let text = tree.text();
        let mut open = Vec::new();
        buttons_named(&tree.children, "Open in new pane", &mut open);
        text.contains("Worktrees") && text.contains("main") && !open.is_empty()
    });

    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feature",
            linked.to_str().unwrap(),
        ],
    );
    *rig.env.projects.lock().unwrap() = vec![root.clone(), linked.clone()];
    rig.host.publish_projects(vec![
        Project {
            id: root.to_string_lossy().into(),
            path: root.to_string_lossy().into(),
            name: "Fixture worktrees".into(),
            parent_id: None, hidden_sections: Vec::new(),
        },
        Project {
            id: linked.to_string_lossy().into(),
            path: linked.to_string_lossy().into(),
            name: "Fixture feature".into(),
            parent_id: None, hidden_sections: Vec::new(),
        },
    ]);
    rig.host.publish_git(&[root.to_string_lossy().into()]);
    let refreshed = tree(&events, "the git refresh", |tree| {
        tree.text().contains("feature")
    });
    let mut open = Vec::new();
    buttons_named(&refreshed.children, "Open in new pane", &mut open);
    assert_eq!(open.len(), 2, "{}", refreshed.text());
    assert!(page.dispatch(&open[1], None));
    let seen = rig.env.wait("the linked worktree pane", |seen| {
        matches!(seen.last(), Some(Seen::OpenPane(_)))
    });
    let Seen::OpenPane(plan) = seen.last().unwrap() else {
        unreachable!()
    };
    assert_eq!(plan.cwd, linked.canonicalize().unwrap());
    assert_eq!(plan.project, linked);
    assert_eq!(plan.input, None);
    assert_eq!(plan.placement, "tab");
}

/// With one project the page's lookups never wait on each other. With many they do, and a page that wakes a waiting
/// task once for every time it was polled runs out of its turn.
#[test]
fn worktrees_sidebar_loads_forty_projects_within_its_turns() {
    let rig = Rig::with(load("worktrees"), true, vec![]);
    let base = rig.root.config("worktrees").root.join("many");
    let roots: Vec<PathBuf> = (0..40)
        .map(|n| {
            let root = base.join(format!("repo-{n:02}"));
            fs::create_dir_all(&root).unwrap();
            git(&root, &["init", "-q", "-b", "main"]);
            root
        })
        .collect();
    *rig.env.projects.lock().unwrap() = roots.clone();
    rig.host.publish_projects(
        roots
            .iter()
            .map(|root| Project {
                id: root.to_string_lossy().into(),
                path: root.to_string_lossy().into(),
                name: root.file_name().unwrap().to_string_lossy().into(),
                parent_id: None, hidden_sections: Vec::new(),
            })
            .collect(),
    );
    rig.host.publish_panes(roots.iter().take(10).enumerate().map(|(n, root)| pane(&n.to_string(), &root.to_string_lossy())).collect());
    let (_page, events) = open_page(&rig.host, &module("worktrees", "sidebar"));
    let loaded = tree(&events, "forty repos", |tree| tree.text().contains("repo-39"));
    assert!(loaded.text().contains("repo-00"));
}

#[test]
fn worktrees_sidebar_shows_the_native_empty_state_without_repos() {
    let rig = Rig::with(load("worktrees"), true, vec![]);
    let (_page, events) = open_page(&rig.host, &module("worktrees", "sidebar"));
    let empty = tree(&events, "the empty state", |tree| {
        tree.text().contains("None of your projects is a git repo.")
    });
    assert!(empty.text().contains("Scan this Mac"));
}

#[test]
fn worktrees_sidebar_reports_a_denied_scan_without_walking_any_disk() {
    let mut plugin = load("worktrees");
    plugin.manifest.permissions.disk_usage = false;
    let rig = Rig::with(plugin, true, vec![]);
    let (page, events) = open_page(&rig.host, &module("worktrees", "sidebar"));
    let initial = tree(&events, "the scan action", |tree| {
        tree.text().contains("Scan this Mac")
    });
    let mut scan = Vec::new();
    buttons_named(&initial.children, "Scan this Mac", &mut scan);
    assert_eq!(scan.len(), 1, "{}", initial.text());
    assert!(page.dispatch(&scan[0], None));
    let denied = tree(&events, "the refused scan", |tree| {
        tree.text().contains("worktrees may not read disk usage")
    });
    assert!(denied.text().contains("Elsewhere on this Mac"));
}

#[cfg(feature = "test-support")]
#[test]
fn worktrees_native_sidebar_bundle_draws_the_empty_state_in_a_native_window() {
    use crate::page::PageView;
    use gpui::{TestApp, TestAppWindow};
    use std::{
        sync::Arc,
        thread,
        time::{Duration, Instant},
    };

    let rig = Rig::with(load("worktrees"), true, vec![]);
    let host = std::sync::Arc::new(rig.host);
    let source: &'static str = Box::leak(module("worktrees", "sidebar").into_boxed_str());
    let mut app = TestApp::new();
    app.background_executor().allow_parking();
    let page_host = Arc::clone(&host);
    let mut window: TestAppWindow<PageView> =
        app.open_window(move |_, cx| PageView::in_plugin(&page_host, source, cx));
    window.update(|view, _, _| view.set_visible(true));
    let deadline = Instant::now() + WAIT;
    loop {
        app.run_until_parked();
        window.draw();
        if window.read(|view, _| view.text().contains("None of your projects is a git repo.")) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the window shows {:?}",
            window.read(|view, _| view.text())
        );
        thread::sleep(Duration::from_millis(5));
    }
}
