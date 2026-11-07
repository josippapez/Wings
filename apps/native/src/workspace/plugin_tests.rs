//! Plugins in the window: what they are told of the panes, the badges and strips they draw on them, the sidebars they show
//! on the right, what they ask the system for, and the transcripts they read, all with real plugin mains in real frames.

use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use gpui::{MouseButton, TestApp, TestAppWindow, point, px, size};
use serde_json::{Value, json};
use wings_plugins::transcript::Line;

use super::{
    Workspace,
    plugin_env::System,
    tests::{open_sized, press, wait_for},
};
use crate::{host::Config, installed::fixture::Data};

/// What the system was asked to do, instead of doing it.
#[derive(Default)]
pub(super) struct FakeSystem {
    opened: Mutex<Vec<String>>,
    notified: Mutex<Vec<(String, String)>>,
    /// The system can't show notifications.
    no_notifications: bool,
    /// The named Claude Code accounts made ready for a terminal, never in the user's home.
    prepared: Mutex<Vec<String>>,
}

impl System for FakeSystem {
    fn open_url(&self, url: &str) -> Result<(), String> {
        self.opened.lock().unwrap().push(url.to_owned());
        Ok(())
    }

    fn notify(&self, title: &str, body: &str) -> Result<(), String> {
        if self.no_notifications {
            return Err("no notification centre".into());
        }
        self.notified
            .lock()
            .unwrap()
            .push((title.to_owned(), body.to_owned()));
        Ok(())
    }

    fn prepare_claude_account(&self, name: &str) -> Result<(), String> {
        self.prepared.lock().unwrap().push(name.to_owned());
        Ok(())
    }

    fn share_claude_setup(&self, _name: &str, _on: bool) -> Result<(), String> {
        Ok(())
    }

    fn claude_auth(&self, _name: Option<&str>) -> wings_spaces::claude_accounts::AuthStatus {
        Default::default()
    }
}

/// A 1000 x 640 window of the plugins in `data`, with `system` standing in for the system before anything is drawn.
pub(super) fn open_plugins(
    data: &Data,
    system: &Arc<FakeSystem>,
    change: impl FnOnce(&mut Config),
) -> (TestApp, TestAppWindow<Workspace>) {
    let (app, mut window) = open_sized(Some(size(px(1000.0), px(640.0))), |config| {
        config.plugins = Some(data.paths());
        change(config);
    });
    let system: Arc<dyn System> = system.clone();
    window.read(|w, _| w.plugin_env.use_system(system));
    window.draw();
    (app, window)
}

/// Runs the window until `done`, redrawing so the plugins hear of what changed.
pub(super) fn until(
    app: &TestApp,
    window: &mut TestAppWindow<Workspace>,
    what: &str,
    done: impl Fn(&Workspace) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !window.read(|w, _| done(w)) {
        assert!(Instant::now() < deadline, "{what}");
        app.run_until_parked();
        window.draw();
        std::thread::sleep(Duration::from_millis(10));
    }
}

const BADGER: &str = r#"
wings.onPanes(async (panes) => {
  for (const p of panes) {
    await wings.setBadge(p.paneId, { label: 'hello', tone: 'success', title: 'Hi', actions: [{ id: 'go', label: 'Go', primary: true }, { id: 'bad', label: 'Bad' }] });
    await wings.setPaneStatus(p.paneId, { items: [{ label: 'cpu', value: '12%', percent: 12, tone: 'warning' }], rows: [{ label: 'Load', value: 'low' }] });
  }
});
wings.onAction(async ({ paneId, actionId }) => {
  await wings.storage.set('clicked', { paneId, actionId });
  if (actionId === 'bad') throw new Error('It broke');
});
"#;

fn install_badger(data: &Data) {
    data.install_script(
        "badger",
        json!({ "contributes": { "ui": ["badges", "status"] } }),
        BADGER,
        &[],
        "badger-scope",
    );
}

#[test]
fn graph_default_open_view_uses_an_owned_native_window_with_local_focus_and_cleanup() {
    let data = Data::temp();
    data.install_script("graph-window", json!({ "contributes": {
        "views": [{ "id":"graph", "title":"Commit graph", "page":"dist/graph.html", "opens":["window","modal"], "width":1100, "height":760 }],
        "sidebars": [{"id":"changes","title":"Changes","page":"dist/sidebar.html"}]
    } }), "let opened=0; wings.onFocus(({focused})=>wings.storage.set('main',focused)); wings.onAction(async()=>{await wings.openView('graph'); wings.storage.set('opened',++opened);});",
        &[("graph", "wings.onFocus(({focused})=>wings.storage.set('graph',focused));"), ("sidebar", "wings.onFocus(({focused})=>wings.storage.set('sidebar',focused));")], "graph-scope");
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    window.update(|w, window, cx| {
        window.activate_window();
        w.show_sidebar("graph-window", "changes", cx);
    });
    until(&app, &mut window, "sidebar not focused", |_| {
        data.saved("graph-window", "graph-scope", "sidebar") == Some(json!(true))
    });
    let host = window.read(|w, _| w.plugins[0].host.clone());
    host.send_action("0", "graph", |_| {});
    until(&app, &mut window, "graph did not open", |w| {
        !w.plugin_ui.windows.is_empty()
    });
    let handle = window.read(|w, _| *w.plugin_ui.windows.values().next().unwrap());
    window.update(|_, _, cx| {
        handle
            .update(cx, |_, window, _| window.activate_window())
            .unwrap();
    });
    until(&app, &mut window, "graph page not focused", |_| {
        data.saved("graph-window", "graph-scope", "graph") == Some(json!(true))
    });
    until(
        &app,
        &mut window,
        "hidden primary sidebar still focused",
        |_| data.saved("graph-window", "graph-scope", "sidebar") == Some(json!(false)),
    );
    assert_eq!(
        data.saved("graph-window", "graph-scope", "main"),
        Some(json!(true))
    );
    assert_eq!(app.windows().len(), 2);
    host.send_action("0", "graph", |_| {});
    until(
        &app,
        &mut window,
        "second graph request did not complete",
        |_| data.saved("graph-window", "graph-scope", "opened") == Some(json!(2)),
    );
    assert_eq!(app.windows().len(), 2);
    window.update(|_, _, cx| {
        handle
            .update(cx, |_, window, _| window.remove_window())
            .unwrap();
    });
    until(&app, &mut window, "graph frame survived its window", |w| {
        w.plugins[0].host.frames() == 2 && w.plugin_ui.windows.is_empty()
    });
    window.update(|_, window, _| window.activate_window());
    until(
        &app,
        &mut window,
        "primary sidebar didn't regain focus",
        |_| data.saved("graph-window", "graph-scope", "sidebar") == Some(json!(true)),
    );
}

#[test]
fn central_diff_loads_selects_files_refreshes_and_closes_through_the_plugin_api() {
    let data = Data::temp();
    let patch = "diff --git a/one b/one\n--- a/one\n+++ b/one\n@@ -1 +1 @@\n-old\n+new\ndiff --git a/two b/two\nnew file mode 100644\n";
    data.install_script("differ", json!({ "contributes": { "ui": ["diff"] } }), &format!(r#"
        let id, started = false;
        wings.onDiffRefresh(async ({{diffId}}) => {{ await wings.updateDiff(diffId, {{patch:{}}}); await wings.storage.set('refreshed', true); }});
        wings.onDiffClosed(async value => {{ await wings.storage.set('closed', value.diffId); }});
        wings.onAction(async () => {{ await wings.updateDiff(id, {{patch:{}}}); await wings.storage.set('filled',true); }});
        wings.onPanes(async list => {{ if (!list.length || started) return; started=true; id=(await wings.openDiff({{title:'Changes',subtitle:'Working tree',refresh:true}})).id; }});
    "#, json!(patch), json!(patch)), &[], "diff-scope");
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    until(&app, &mut window, "loading diff never opened", |w| {
        w.plugin_ui.diff.is_some()
    });
    assert!(window.read(|w, cx| {
        w.plugin_ui
            .diff
            .as_ref()
            .unwrap()
            .0
            .read(cx)
            .view
            .update
            .patch
            .is_none()
    }));
    let host = window.read(|w, _| w.plugins[0].host.clone());
    host.send_action("0", "fill", |_| {});
    until(&app, &mut window, "diff never filled", |_| {
        data.saved("differ", "diff-scope", "filled") == Some(json!(true))
    });
    app.run_until_parked();
    window.draw();
    // The folder tree, which is kept for the next diff. Both files are at the top, so their rows stay put.
    window.simulate_click(point(px(263.0), px(95.0)), MouseButton::Left);
    assert!(window.read(|w, _| w.prefs.diff_tree));
    window.draw();
    window.simulate_click(point(px(80.0), px(162.0)), MouseButton::Left);
    assert_eq!(
        window.read(|w, cx| w
            .plugin_ui
            .diff
            .as_ref()
            .unwrap()
            .0
            .read(cx)
            .selected_file()),
        1
    );
    // Refresh, by its shortcut: the button's place depends on the toggles' label widths, which the test text system
    // doesn't measure like the real one.
    window.simulate_keystroke("cmd-r");
    until(&app, &mut window, "refresh did not reach main", |_| {
        data.saved("differ", "diff-scope", "refreshed") == Some(json!(true))
    });
    window.simulate_keystroke("escape");
    until(&app, &mut window, "diff stayed open", |w| {
        w.plugin_ui.diff.is_none()
    });
    until(&app, &mut window, "close event did not reach main", |_| {
        data.saved("differ", "diff-scope", "closed").is_some()
    });
    // The next diff opens with the kept choices.
    window.update(|w, window, cx| {
        w.prefs.diff_wrap = true;
        let view = crate::diff::View {
            id: "differ:again".into(),
            plugin_id: "differ".into(),
            title: "Again".into(),
            subtitle: String::new(),
            cwd: String::new(),
            refresh: false,
            update: Default::default(),
        };
        w.open_diff_view(view, window, cx);
    });
    assert!(window.read(|w, cx| {
        let viewer = w.plugin_ui.diff.as_ref().unwrap().0.read(cx);
        viewer.tree && viewer.wrap
    }));
}

#[test]
fn plugin_frame_focus_follows_tabs_sidebars_projects_and_window_activation() {
    let data = Data::temp();
    let alpha = data.root.join("focus-alpha");
    let beta = data.root.join("focus-beta");
    fs::create_dir_all(&alpha).unwrap();
    fs::create_dir_all(&beta).unwrap();
    let projects = data.root.join("focus-projects.json");
    fs::write(
        &projects,
        json!([
            { "id": alpha, "name": "Alpha", "path": alpha },
            { "id": beta, "name": "Beta", "path": beta }
        ])
        .to_string(),
    )
    .unwrap();
    let source = |key: &str| {
        format!(
            "const states = []; const initial = wings.focused; wings.onFocus(({{focused}}) => {{ states.push(focused); wings.storage.set('{key}', {{initial, focused, states}}); }});"
        )
    };
    let (one, two, side, other, main) = (
        source("one"),
        source("two"),
        source("side"),
        source("other"),
        source("main"),
    );
    data.install_script("visibility", json!({ "contributes": {
        "views": [
            { "id": "one", "title": "One", "page": "dist/one.html", "opens": ["tab"], "newTab": true },
            { "id": "two", "title": "Two", "page": "dist/two.html", "opens": ["tab"], "newTab": true }
        ],
        "sidebars": [
            { "id": "side", "title": "Side", "page": "dist/side.html" },
            { "id": "other", "title": "Other", "page": "dist/other.html" }
        ]
    } }), &main, &[("one", &one), ("two", &two), ("side", &side), ("other", &other)], "visibility-scope");
    let (mut app, mut window) = open_plugins(&data, &Arc::default(), |config| {
        config.project = alpha.clone();
        config.projects = Some(projects);
    });
    window.update(|_, window, _| window.activate_window());
    let focused =
        |app: &TestApp, window: &mut TestAppWindow<Workspace>, key: &str, expected: bool| {
            until(
                app,
                window,
                &format!("{key} never became focused={expected}"),
                |_| {
                    data.saved("visibility", "visibility-scope", key)
                        .is_some_and(|value| value["focused"] == expected)
                },
            );
        };
    focused(&app, &mut window, "main", true);
    window.update(|w, window, cx| {
        assert!(w.open_plugin_view("visibility", "one", None, window, cx));
    });
    focused(&app, &mut window, "one", true);
    assert_eq!(
        data.saved("visibility", "visibility-scope", "one").unwrap()["initial"],
        false
    );
    window.update(|w, window, cx| {
        assert!(w.open_plugin_view("visibility", "two", None, window, cx));
    });
    focused(&app, &mut window, "one", false);
    focused(&app, &mut window, "two", true);
    window.update(|w, _, cx| w.show_sidebar("visibility", "side", cx));
    focused(&app, &mut window, "side", true);
    window.update(|w, _, cx| w.show_sidebar("visibility", "other", cx));
    focused(&app, &mut window, "side", false);
    focused(&app, &mut window, "other", true);
    window.update(|w, window, cx| w.switch_project(beta.clone(), window, cx));
    focused(&app, &mut window, "two", false);
    focused(&app, &mut window, "other", true);
    window.update(|w, window, cx| w.switch_project(alpha, window, cx));
    focused(&app, &mut window, "two", true);
    struct Elsewhere;
    impl gpui::Render for Elsewhere {
        fn render(
            &mut self,
            _: &mut gpui::Window,
            _: &mut gpui::Context<Self>,
        ) -> impl gpui::IntoElement {
            gpui::div()
        }
    }
    let mut elsewhere = app.open_window(|_, _| Elsewhere);
    elsewhere.update(|_, window, _| window.activate_window());
    for key in ["main", "two", "other"] {
        focused(&app, &mut window, key, false);
    }
    window.update(|_, window, _| window.activate_window());
    for key in ["main", "two", "other"] {
        focused(&app, &mut window, key, true);
    }
    for key in ["one", "side"] {
        focused(&app, &mut window, key, false);
    }
    window.update(|w, _, cx| {
        w.right = None;
        w.sync_plugin_visibility(cx);
    });
    focused(&app, &mut window, "other", false);
    assert_eq!(window.read(|w, _| w.plugins[0].host.frames()), 5);
    window.update(|w, window, cx| w.close_plugin_tab(1, window, cx));
    until(&app, &mut window, "closed page retained its frame", |w| {
        w.plugins[0].host.frames() == 4
    });
    focused(&app, &mut window, "one", true);
}

/// Cyclops' title bar button: its running timer as a label by the button, and its popover page while you point at it.
#[test]
fn a_sidebar_shows_its_label_and_its_popover_page_while_you_point_at_its_button() {
    let data = Data::temp();
    let page = |text: &str| {
        format!(
            "wings.render(JSON.stringify({{id:0,type:'#root',children:[{{id:1,type:'Text',children:[{{id:2,type:'#text',text:'{text}'}}]}}]}}));"
        )
    };
    let (sidebar, popover) = (page("In the sidebar"), page("In the popover"));
    data.install_script(
        "timer",
        json!({ "contributes": { "sidebars": [{ "id": "tracker", "title": "Cyclops", "page": "dist/sidebar.html",
            "popover": { "page": "dist/popover.html", "width": 360, "height": 240 } }] } }),
        "wings.setSidebarLabel('tracker', '1:05:00', { tone: 'warning' });",
        &[("sidebar", &sidebar), ("popover", &popover)],
        "timer-scope",
    );
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    let key = ("timer".to_owned(), "tracker".to_owned());
    until(&app, &mut window, "the label never came", |w| {
        w.plugin_ui.labels.contains_key(&key)
    });
    window.read(|w, _| {
        let label = &w.plugin_ui.labels[&key];
        assert_eq!(label.text, "1:05:00");
        assert_eq!(
            super::plugin_ui::tone_dot(label.tone),
            crate::theme::WORKING
        );
    });
    // Pointing at the button shows the popover after its delay, with the popover's own page.
    window.update(|w, _, cx| w.hover_popover(key.clone(), false, true, cx));
    app.advance_clock(Duration::from_millis(250));
    window.read(|w, _| assert_eq!(w.plugin_ui.popover.open, None, "not at once"));
    app.advance_clock(Duration::from_millis(60));
    window.read(|w, _| assert_eq!(w.plugin_ui.popover.open.as_ref(), Some(&key)));
    let shown = window.read(|w, _| w.plugin_ui.popover.pages[&key].0.clone());
    let deadline = Instant::now() + Duration::from_secs(10);
    while !window.read(|_, cx| shown.read(cx).text().contains("In the popover")) {
        assert!(Instant::now() < deadline, "the popover page never drew");
        app.run_until_parked();
        window.draw();
        std::thread::sleep(Duration::from_millis(10));
    }
    // Moving from the button onto the popover keeps it; leaving both closes it, and its page stays loaded, hidden.
    window.update(|w, _, cx| {
        w.hover_popover(key.clone(), false, false, cx);
        w.hover_popover(key.clone(), true, true, cx);
    });
    app.advance_clock(Duration::from_millis(300));
    window.read(|w, _| assert_eq!(w.plugin_ui.popover.open.as_ref(), Some(&key)));
    window.update(|w, _, cx| w.hover_popover(key.clone(), true, false, cx));
    app.advance_clock(Duration::from_millis(210));
    window.read(|w, _| assert_eq!(w.plugin_ui.popover.open, None));
    window.read(|w, _| assert!(w.plugin_ui.popover.pages.contains_key(&key)));
    // Not over its own open sidebar.
    window.update(|w, _, cx| w.show_sidebar("timer", "tracker", cx));
    window.update(|w, _, cx| w.hover_popover(key.clone(), false, true, cx));
    app.advance_clock(Duration::from_millis(400));
    window.read(|w, _| assert_eq!(w.plugin_ui.popover.open, None));
    // Two minutes out of view, its page is dropped, and the next show loads it again.
    app.advance_clock(Duration::from_secs(121));
    window.read(|w, _| assert!(!w.plugin_ui.popover.pages.contains_key(&key)));
}

/// The Claude accounts plugin's "Start Claude as": the pane's shell runs under the account picked, not the project's.
#[test]
fn a_pane_opened_with_a_claude_account_runs_its_shell_under_that_account() {
    let data = Data::temp();
    let project = data.root.join("project");
    fs::create_dir_all(&project).unwrap();
    data.install_script(
        "starter",
        json!({ "permissions": { "panes": [], "claudeAccounts": true } }),
        // On an action, which the test sends once the fake system is in place: never the real one, which makes the folder.
        r#"
            wings.onAction(async () => {
                try {
                    await wings.storage.set('opened', await wings.openPane({ claudeAccount: 'wings-native-test' }));
                } catch (error) { await wings.storage.set('failed', error.message); }
            });
        "#,
        &[],
        "starter-scope",
    );
    let system = Arc::new(FakeSystem::default());
    let (app, mut window) = open_plugins(&data, &system, |config| config.project = project.clone());
    let host = window.read(|w, _| w.plugins[0].host.clone());
    host.send_action("0", "start", |_| {});
    until(&app, &mut window, "the pane never opened", |_| {
        data.saved("starter", "starter-scope", "opened").is_some()
            || data.saved("starter", "starter-scope", "failed").is_some()
    });
    assert_eq!(data.saved("starter", "starter-scope", "failed"), None);
    // Made ready by the fake system, so nothing was made in your home folder.
    assert_eq!(*system.prepared.lock().unwrap(), ["wings-native-test"]);
    let pane: usize = data.saved("starter", "starter-scope", "opened").unwrap()["paneId"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let terminal = window.read(|w, _| w.terminal(pane).unwrap());
    window.update(|_, _, cx| {
        terminal.update(cx, |view, cx| {
            view.write_input("echo account-''dir:$CLAUDE_CONFIG_DIR\r", cx)
        })
    });
    let expected = format!(
        "account-dir:{}",
        wings_spaces::claude_accounts::dir(Some("wings-native-test")).display()
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let output = window.read(|_, cx| terminal.read(cx).recent_output());
        if String::from_utf8_lossy(&output).contains(&expected) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the shell never printed {expected}"
        );
        app.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_plugin_opens_a_real_pane_and_hears_the_workspace_projects() {
    let data = Data::temp();
    let project = data.root.join("project");
    fs::create_dir_all(project.join("sub")).unwrap();
    let cwd = project.join("sub").canonicalize().unwrap();
    data.install_script(
        "opener",
        json!({ "permissions": { "panes": [] }, "contributes": { "ui": ["badges"] } }),
        &format!(
            r#"
            let opened = false;
            wings.onProjects(list => wings.storage.set('projects', list));
            wings.onPanes(async list => {{
                if (!list.length || opened) return;
                opened = true;
                try {{
                    const pane = await wings.openPane({{ cwd: {}, placement: 'right' }});
                    await wings.setBadge(pane.paneId, {{ label: 'Opened' }});
                    await wings.storage.set('opened', {{ pane }});
                }} catch (error) {{ await wings.storage.set('failed', error.message); }}
            }});
        "#,
            json!(cwd)
        ),
        &[],
        "opener-scope",
    );
    let (app, mut window) = open_plugins(&data, &Arc::default(), |config| {
        config.project = project.clone()
    });
    until(&app, &mut window, "the pane never opened", |_| {
        data.saved("opener", "opener-scope", "opened").is_some()
            || data.saved("opener", "opener-scope", "failed").is_some()
    });
    assert_eq!(data.saved("opener", "opener-scope", "failed"), None);
    let result = data.saved("opener", "opener-scope", "opened").unwrap();
    let pane: usize = result["pane"]["paneId"].as_str().unwrap().parse().unwrap();
    // setBadge resolves before the window has applied the badge.
    until(&app, &mut window, "the badge never showed", |w| {
        w.plugin_ui
            .badges
            .get(&pane)
            .is_some_and(|badges| badges.contains_key("opener"))
    });
    window.read(|w, cx| {
        assert_eq!(w.panes.len(), 2);
        assert_eq!(w.terminals[0].focused, pane);
        assert_eq!(w.plugin_ui.badges[&pane]["opener"].label, "Opened");
        assert_eq!(w.terminal(pane).unwrap().read(cx).cwd(), Some(cwd));
    });
    assert_eq!(
        data.saved("opener", "opener-scope", "projects").unwrap()[0]["path"],
        json!(project)
    );
}

#[test]
fn a_worktree_a_plugin_makes_is_listed_under_its_project_and_is_a_project_the_next_pane_can_open_in()
 {
    let data = Data::temp();
    let dir = crate::projects::test_dir("window-worktree");
    let repo = crate::projects::test_repo(&dir);
    let list = dir.join("spaces.json");
    let spaces = json!([{ "id": repo, "name": "app", "path": repo, "worktreeSetup": "true" }]);
    fs::write(&list, spaces.to_string()).unwrap();
    data.install_script(
        "maker",
        json!({ "permissions": { "panes": [], "worktrees": true } }),
        r#"
        let started = false;
        wings.onPanes(async list => {
            if (!list.length || started) return;
            started = true;
            try {
                const made = await wings.createWorktree({ branch: 'wings/one' });
                const pane = await wings.openPane({ cwd: made.path });
                await wings.storage.set('made', { made, pane });
            } catch (error) { await wings.storage.set('failed', error.message); }
        });
        "#,
        &[],
        "maker-scope",
    );
    let (app, mut window) = open_plugins(&data, &Arc::default(), |config| {
        config.project = repo.clone();
        config.projects = Some(list.clone());
    });
    until(&app, &mut window, "the worktree was never made", |_| {
        data.saved("maker", "maker-scope", "made").is_some()
            || data.saved("maker", "maker-scope", "failed").is_some()
    });
    assert_eq!(data.saved("maker", "maker-scope", "failed"), None);
    let made = data.saved("maker", "maker-scope", "made").unwrap();
    let tree = dir.join("app-worktrees/wings-one");
    assert_eq!(made["made"], json!({ "projectId": tree, "path": tree }));
    window.read(|w, _| {
        let rows: Vec<_> = w
            .projects
            .iter()
            .map(|p| (p.name.as_str(), p.child))
            .collect();
        assert_eq!(rows, [("app", false), ("wings-one", true)]);
        // The pane opened in the worktree, which is now the project on screen, and took its setup line.
        assert_eq!(w.project(), tree);
        assert_eq!(w.spaces.take_setup(&tree), None);
    });
    // Saved in the native list; the plugin's pane is the one the worktree's project shows.
    let saved: Vec<serde_json::Value> = serde_json::from_slice(&fs::read(&list).unwrap()).unwrap();
    assert_eq!(saved[1]["parent"], json!(repo));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_plugins_badge_and_status_show_on_the_pane_and_a_click_on_a_button_reaches_the_plugin() {
    let data = Data::temp();
    install_badger(&data);
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    until(
        &app,
        &mut window,
        "the badge and the strip never showed",
        |w| w.plugin_ui.badges.contains_key(&0) && w.plugin_ui.statuses.contains_key(&0),
    );
    window.read(|w, _| {
        let badge = &w.plugin_ui.badges[&0]["badger"];
        assert_eq!(
            (badge.label.as_str(), badge.title.as_deref()),
            ("hello", Some("Hi"))
        );
        let status = &w.plugin_ui.statuses[&0]["badger"];
        assert_eq!(status.items[0].value, "12%");
        assert!(w.badge_card.is_none());
    });

    // The pill is the last thing before the pane's four 28px buttons, on the right of a 30px header 76px down.
    window.draw();

    window.simulate_click(point(px(854.0), px(91.0)), MouseButton::Left);
    window.draw();
    assert_eq!(
        window.read(|w, _| w.badge_card.as_ref().map(|c| (c.pane, c.plugin.clone()))),
        Some((0, "badger".into()))
    );
    // The card's first button is the primary one: a click asks the plugin's main, which saves what it heard.
    window.simulate_click(point(px(572.0), px(172.0)), MouseButton::Left);
    until(
        &app,
        &mut window,
        "the plugin never heard the click",
        |_| data.saved("badger", "badger-scope", "clicked").is_some(),
    );
    assert_eq!(
        data.saved("badger", "badger-scope", "clicked"),
        Some(json!({ "paneId": "0", "actionId": "go" }))
    );
    until(&app, &mut window, "the button stayed busy", |w| {
        w.badge_card
            .as_ref()
            .is_some_and(|card| card.pending.is_none())
    });
    assert!(window.read(|w, _| w.badge_card.as_ref().unwrap().error.is_none()));

    // A click on the pill again closes the card; one outside it does too.
    window.simulate_click(point(px(854.0), px(91.0)), MouseButton::Left);
    assert!(window.read(|w, _| w.badge_card.is_none()));
    window.simulate_click(point(px(854.0), px(91.0)), MouseButton::Left);
    assert!(window.read(|w, _| w.badge_card.is_some()));
    window.simulate_click(point(px(500.0), px(400.0)), MouseButton::Left);
    assert!(window.read(|w, _| w.badge_card.is_none()));
}

#[test]
fn a_button_whose_listener_throws_shows_its_message_in_the_card() {
    let data = Data::temp();
    install_badger(&data);
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    until(&app, &mut window, "the badge never showed", |w| {
        w.plugin_ui.badges.contains_key(&0)
    });
    window.update(|w, _, cx| {
        w.toggle_badge_card(0, "badger".into(), cx);
        w.run_badge_action(0, "badger".into(), "bad".into(), cx);
    });
    until(&app, &mut window, "the error never showed", |w| {
        w.badge_card
            .as_ref()
            .is_some_and(|card| card.error.is_some())
    });
    window.read(|w, _| {
        let card = w.badge_card.as_ref().unwrap();
        assert_eq!(card.error.as_deref(), Some("It broke"));
        assert_eq!(card.pending, None);
    });
    // The next click clears it while it waits.
    window.update(|w, _, cx| w.run_badge_action(0, "badger".into(), "go".into(), cx));
    until(&app, &mut window, "the second click never settled", |w| {
        w.badge_card
            .as_ref()
            .is_some_and(|card| card.pending.is_none() && card.error.is_none())
    });
    window.draw();
}

#[test]
fn a_closed_panes_badge_strip_and_card_go_with_it() {
    let data = Data::temp();
    install_badger(&data);
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    press(&mut window, "d");
    until(&app, &mut window, "both panes never got a badge", |w| {
        w.plugin_ui.badges.len() == 2 && w.plugin_ui.statuses.len() == 2
    });
    window.update(|w, _, cx| w.toggle_badge_card(1, "badger".into(), cx));
    press(&mut window, "w");
    window.draw();
    window.read(|w, _| {
        assert_eq!(w.plugin_ui.badges.keys().copied().collect::<Vec<_>>(), [0]);
        assert_eq!(
            w.plugin_ui.statuses.keys().copied().collect::<Vec<_>>(),
            [0]
        );
        assert!(w.badge_card.is_none());
    });
}

const SHELL: &str = r#"
const heard = [];
wings.onPanes((panes) => { heard.push(panes); wings.storage.set('panes', heard); });
"#;

#[test]
fn plugins_are_told_the_panes_as_the_desktop_lists_them_when_they_change() {
    let data = Data::temp();
    data.install_script("watcher", json!({}), SHELL, &[], "watcher-scope");
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    let saved = || {
        data.saved("watcher", "watcher-scope", "panes")
            .unwrap_or(json!([]))
    };
    until(
        &app,
        &mut window,
        "the plugin was never told of the pane",
        |_| !saved().as_array().unwrap().is_empty(),
    );
    let project = window.read(|w, _| w.config.project.clone());
    let name = project.file_name().unwrap().to_string_lossy().into_owned();
    let first = saved();
    assert_eq!(
        first[0],
        json!([{ "paneId": "0", "cwd": project, "command": "", "project": name, "projectId": project, "session": null, "focused": true }])
    );

    // A split is a new pane, and the focus moved to it. Nothing else changed, so there is one list per change.
    press(&mut window, "d");
    until(&app, &mut window, "the split was never heard", |_| {
        saved().as_array().unwrap().len() >= 2
    });
    let second = &saved()[1];
    assert_eq!(second.as_array().unwrap().len(), 2);
    assert_eq!(
        (second[0]["paneId"].clone(), second[0]["focused"].clone()),
        (json!("0"), json!(false))
    );
    assert_eq!(
        (second[1]["paneId"].clone(), second[1]["focused"].clone()),
        (json!("1"), json!(true))
    );
    for _ in 0..5 {
        app.run_until_parked();
        window.draw();
    }
    assert_eq!(
        saved().as_array().unwrap().len(),
        2,
        "a redraw told the plugin again"
    );
}

// Notifications, URLs.

const REACTOR: &str = r#"
wings.onPanes(async (panes) => { for (const p of panes) await wings.setBadge(p.paneId, { label: 'x', actions: [{ id: 'notify', label: 'Notify' }, { id: 'open', label: 'Open' }, { id: 'refused', label: 'Refused' }] }); });
wings.onAction(async ({ actionId }) => {
  if (actionId === 'notify') await wings.notify({ title: ' Done ', body: 'all green' });
  if (actionId === 'open') await wings.openUrl('http://localhost:5173/');
  if (actionId === 'refused') await wings.openUrl('https://evil.example/');
});
"#;

fn install_reactor(data: &Data) {
    data.install_script(
        "reactor",
        json!({ "contributes": { "ui": ["badges"] }, "permissions": { "notify": true, "openUrl": ["http://localhost:"] } }),
        REACTOR,
        &[],
        "reactor-scope",
    );
}

fn click_action(
    app: &TestApp,
    window: &mut TestAppWindow<Workspace>,
    action: &str,
) -> Option<String> {
    until(app, window, "the badge never showed", |w| {
        w.plugin_ui.badges.contains_key(&0)
    });
    window.update(|w, _, cx| {
        if w.badge_card.is_none() {
            w.toggle_badge_card(0, "reactor".into(), cx);
        }
        w.run_badge_action(0, "reactor".into(), action.into(), cx);
    });
    until(app, window, "the click never settled", |w| {
        w.badge_card.as_ref().is_some_and(|c| c.pending.is_none())
    });
    window.read(|w, _| {
        w.badge_card
            .as_ref()
            .unwrap()
            .error
            .as_ref()
            .map(|e| e.to_string())
    })
}

#[test]
fn a_notification_goes_to_the_system_with_the_plugins_name_and_a_url_opens_in_the_browser() {
    let data = Data::temp();
    install_reactor(&data);
    let system = Arc::new(FakeSystem::default());
    let (app, mut window) = open_plugins(&data, &system, |_| {});
    assert_eq!(click_action(&app, &mut window, "notify"), None);
    assert_eq!(
        *system.notified.lock().unwrap(),
        [("Plugin reactor: Done".to_owned(), "all green".to_owned())]
    );
    assert!(window.read(|w, _| w.plugin_ui.notices.is_empty()));

    assert_eq!(click_action(&app, &mut window, "open"), None);
    assert_eq!(*system.opened.lock().unwrap(), ["http://localhost:5173/"]);
    // What the manifest doesn't name never reaches the system.
    assert_eq!(
        click_action(&app, &mut window, "refused").as_deref(),
        Some("reactor may not open https://evil.example/")
    );
    assert_eq!(system.opened.lock().unwrap().len(), 1);
}

const NOTIFIER: &str = r#"
wings.onPanes(async (panes) => { for (const p of panes) await wings.setBadge(p.paneId, { label: 'x', actions: [{ id: 'notify', label: 'Notify' }] }); });
wings.onAction(async ({ actionId }) => {
  if (actionId === 'notify') await wings.notify({ title: 'Build passed', body: 'main is green', actions: [{ label: 'Open run', url: 'https://ci.example/run/1' }, { label: 'Elsewhere', url: 'https://evil.example/' }] });
});
"#;

fn install_notifier(data: &Data) {
    data.install_script(
        "reactor",
        json!({ "contributes": { "ui": ["badges"] }, "permissions": { "notify": true, "openUrl": ["https://ci.example/"] } }),
        NOTIFIER,
        &[],
        "reactor-scope",
    );
}

#[test]
fn a_plugins_notification_waits_unread_in_the_inbox_and_its_button_opens_a_link_the_plugin_may_open()
 {
    let data = Data::temp();
    install_notifier(&data);
    let system = Arc::new(FakeSystem::default());
    let (app, mut window) = open_plugins(&data, &system, |_| {});
    assert_eq!(click_action(&app, &mut window, "notify"), None);
    until(&app, &mut window, "the inbox never heard of it", |w| {
        !w.inbox.events.is_empty()
    });
    window.read(|w, _| {
        let event = &w.inbox.events[0];
        assert_eq!(
            (event.title.as_ref(), event.detail.as_str(), event.read),
            ("Build passed", "main is green", false)
        );
        assert_eq!(w.inbox.unread(), 1);
    });
    // Opening the list reads everything, and keeps what was new marked while it stays open.
    window.update(|w, _, cx| w.open_inbox(cx));
    window.read(|w, _| {
        assert_eq!(w.inbox.unread(), 0);
        assert_eq!(w.inbox.open.as_ref().map(|fresh| fresh.len()), Some(1));
    });
    let notification = window.read(|w, _| w.inbox.events[0].plugin.clone().unwrap());
    for button in &notification.buttons {
        window.update(|w, window, cx| w.inbox_button(&notification, button, window, cx));
    }
    // The second link isn't one the manifest names, so it never reaches the browser.
    assert_eq!(*system.opened.lock().unwrap(), ["https://ci.example/run/1"]);
    window.read(|w, _| {
        assert!(w.inbox.open.is_none(), "a button closes the list");
        assert_eq!(
            w.plugin_ui.notices.last().map(|n| n.body.to_string()),
            Some("reactor may not open https://evil.example/".into())
        );
    });
}

#[test]
fn what_a_page_copies_reaches_the_clipboard() {
    let data = Data::temp();
    let system = Arc::new(FakeSystem::default());
    let (app, mut window) = open_plugins(&data, &system, |_| {});
    // The host has checked the page may (`page::plugin::calls`): this is the window's part.
    let env = window.read(|w, _| w.plugin_env.clone());
    assert_eq!(
        crate::page::plugin::Env::copy_text(&*env, "fix/login".into()),
        Ok(())
    );
    until(&app, &mut window, "nothing was copied", |_| {
        app.read_from_clipboard().is_some()
    });
    assert_eq!(
        app.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("fix/login")
    );
}

#[test]
fn a_notification_the_system_cannot_show_is_shown_in_the_window_and_goes_away() {
    let data = Data::temp();
    install_reactor(&data);
    let system = Arc::new(FakeSystem {
        no_notifications: true,
        ..Default::default()
    });
    let (app, mut window) = open_plugins(&data, &system, |_| {});
    assert_eq!(click_action(&app, &mut window, "notify"), None);
    until(&app, &mut window, "the notice never showed", |w| {
        !w.plugin_ui.notices.is_empty()
    });
    window.read(|w, _| {
        assert_eq!(w.plugin_ui.notices.len(), 1);
        assert_eq!(
            w.plugin_ui.notices[0].title.as_ref(),
            "Plugin reactor: Done"
        );
        assert_eq!(w.plugin_ui.notices[0].body.as_ref(), "all green");
    });
    // Clicking it puts it away, at the bottom right of the window.
    window.draw();
    window.simulate_click(point(px(850.0), px(620.0)), MouseButton::Left);
    assert!(window.read(|w, _| w.plugin_ui.notices.is_empty()));
}

// Sidebars.

const SIDEBAR_PAGE: &str = r#"
wings.render(JSON.stringify({ id: 0, type: '#root', props: {}, children: [{ id: 1, type: '#text', text: 'Side page' }] }));
"#;

const SIDEBAR_MAIN: &str = r#"
wings.onPanes(async (panes) => {
  for (const p of panes) await wings.setPaneStatus(p.paneId, { items: [{ label: 'ctx', value: '10%' }], open: 'side' });
});
"#;

fn install_sider(data: &Data) {
    data.install_script(
        "sider",
        json!({ "contributes": { "ui": ["status"], "sidebars": [{ "id": "side", "title": "Side", "page": "dist/side.html" }] } }),
        SIDEBAR_MAIN,
        &[("side", SIDEBAR_PAGE)],
        "sider-scope",
    );
}

#[test]
fn a_plugins_sidebar_opens_in_the_right_hand_panel_from_its_project_bar_button_and_keeps_running() {
    let data = Data::temp();
    install_sider(&data);
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    assert_eq!(window.read(|w, _| w.plugins[0].sidebars.len()), 1);
    assert!(window.read(|w, _| w.right.is_none() && w.sidebar_pages.is_empty()));

    let right_edge = |window: &TestAppWindow<Workspace>| {
        window.read(|w, cx| {
            w.terminal(0)
                .unwrap()
                .read(cx)
                .drawn_at()
                .unwrap()
                .right()
                .as_f32()
        })
    };
    window.draw();
    let before = right_edge(&window);
    // The button is the last thing in the project bar under the 40px title bar, 8px from the right edge.
    window.simulate_click(point(px(978.0), px(58.0)), MouseButton::Left);
    assert_eq!(
        window.read(|w, _| w.right.clone()),
        Some(("sider".into(), "side".into()))
    );
    let text = |window: &TestAppWindow<Workspace>| {
        window.read(|w, cx| {
            w.sidebar_pages[&("sider".to_owned(), "side".to_owned())]
                .read(cx)
                .text()
        })
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    while !text(&window).contains("Side page") {
        assert!(
            Instant::now() < deadline,
            "the sidebar shows {:?}",
            text(&window)
        );
        app.run_until_parked();
        window.draw();
        std::thread::sleep(Duration::from_millis(10));
    }

    // It slides open over the desktop's 320ms, as the desktop's right panel does: halfway, the panes have given up
    // part of its width, and all of it at the end.
    app.advance_clock(crate::ui::motion::SLIDE / 2);
    app.run_until_parked();
    window.draw();
    let halfway = right_edge(&window);
    app.advance_clock(crate::ui::motion::SLIDE);
    app.run_until_parked();
    window.draw();
    let open = right_edge(&window);
    // At half time, along the slide's curve: the share of its width the panel has taken.
    let share = (before - halfway) / (before - open);
    let expected = crate::ui::motion::cubic_bezier(crate::ui::motion::EASE_SLIDE)(0.5);
    assert!(
        halfway > open && (share - expected).abs() < 0.02,
        "partway at half time: {open} < {halfway} < {before}, {share} vs {expected}"
    );

    // Closed, the panel goes and the page's frame stays. Opened again, it is the same page. The open panel narrows
    // the project bar, so the button sits its width further left.
    window.simulate_click(
        point(px(978.0 - super::plugin_ui::RIGHT_WIDTH), px(58.0)),
        MouseButton::Left,
    );
    assert!(window.read(|w, _| w.right.is_none()));
    assert_eq!(window.read(|w, _| w.plugins[0].host.frames()), 2);
    window.update(|w, _, cx| w.show_sidebar("sider", "side", cx));
    assert!(window.read(|w, _| w.right.is_some() && w.sidebar_pages.len() == 1));
}

#[test]
fn a_click_on_a_status_strip_that_names_a_sidebar_opens_it() {
    let data = Data::temp();
    install_sider(&data);
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    until(&app, &mut window, "the strip never showed", |w| {
        w.plugin_ui.statuses.contains_key(&0)
    });
    assert_eq!(
        window.read(|w, _| w.plugin_ui.statuses[&0]["sider"].open.clone()),
        Some("side".into())
    );
    // The strip is the card's last 22px: the card ends 8px above the window's bottom, and the project bar is 36px.
    window.draw();
    window.simulate_click(point(px(290.0), px(620.0)), MouseButton::Left);
    assert_eq!(
        window.read(|w, _| w.right.clone()),
        Some(("sider".into(), "side".into()))
    );
    // The close button shuts the panel.
    window.draw();
    window.simulate_click(
        point(px(1000.0 - 8.0 - 12.0 - 8.0), px(40.0 + 16.0)),
        MouseButton::Left,
    );
    assert!(window.read(|w, _| w.right.is_none()));
}

// Transcripts.

const READER: &str = r#"
wings.onPanes((panes) => wings.storage.set('session', panes[0]?.session ?? null));
wings.onTranscript((event) => wings.storage.set(`heard-${event.entry.n}`, event));
"#;

const DEAF: &str = r#"
wings.onTranscript((event) => wings.storage.set('deaf', event));
"#;

const SESSION: &str = "3c21b0c5-8113-4f55-9493-fa156c3fa369";

#[test]
fn claude_writing_a_transcript_in_a_pane_reaches_only_the_plugins_that_may_read_its_type() {
    let data = Data::temp();
    data.install_script(
        "reader",
        json!({ "permissions": { "transcript": ["assistant"] } }),
        READER,
        &[],
        "reader-scope",
    );
    data.install_script("deaf", json!({}), DEAF, &[], "deaf-scope");
    let dir = std::env::temp_dir().join(format!("wings-native-transcripts-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let claude = dir.join("claude");
    fs::create_dir_all(claude.join("sessions")).unwrap();
    fs::create_dir_all(claude.join("projects/-x")).unwrap();
    fs::create_dir_all(dir.join("bin")).unwrap();
    let fake = dir.join("bin/claude");
    std::os::unix::fs::symlink("/bin/sleep", &fake).unwrap();
    let transcript = claude.join(format!("projects/-x/{SESSION}.jsonl"));
    fs::write(&transcript, "{\"type\":\"assistant\",\"n\":0}\n").unwrap();

    let (app, mut window) = open_plugins(&data, &Arc::default(), |config| {
        config.agents = Some(claude.clone())
    });
    wait_for(
        &app,
        &window,
        |w| w.pane_info.contains_key(&0),
        "the shell was never detected",
    );
    window.simulate_input(&format!("exec '{}' 60", fake.display()));
    window.simulate_keystroke("enter");
    wait_for(
        &app,
        &window,
        |w| w.agents.contains_key(&0),
        "the agent was never detected",
    );
    let pid = window.read(|w, _| w.agents[&0].pid);
    fs::write(
        claude.join(format!("sessions/{pid}.json")),
        format!(r#"{{"sessionId":"{SESSION}","status":"idle","name":"fix it"}}"#),
    )
    .unwrap();
    wait_for(
        &app,
        &window,
        |w| w.agents[&0].session_id.is_some(),
        "the session was never read",
    );
    window.draw();

    // The plugin sees the session in the pane, as `WingsPane` has it.
    let saved = |plugin: &str, key: &str| data.saved(plugin, &format!("{plugin}-scope"), key);
    let deadline = Instant::now() + Duration::from_secs(15);
    while saved("reader", "session").is_none_or(|s| s.is_null()) {
        assert!(
            Instant::now() < deadline,
            "the plugin never saw the session"
        );
        app.run_until_parked();
        window.draw();
        std::thread::sleep(Duration::from_millis(10));
    }
    let session = saved("reader", "session").unwrap();
    assert_eq!(session["sessionId"], SESSION);
    assert_eq!(
        (session["kind"].clone(), session["name"].clone()),
        (json!("claude"), json!("fix it"))
    );
    assert!(session["startedAt"].as_u64().unwrap() > 0, "{session}");

    // Lines Claude appends: the first poll binds the session at the transcript's end, so append until one is heard.
    let append = |n: u32, kind: &str| {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&transcript)
            .unwrap();
        writeln!(file, "{}", json!({ "type": kind, "n": n })).unwrap();
    };
    let mut n = 1;
    while saved("reader", &format!("heard-{n}")).is_none() {
        assert!(Instant::now() < deadline, "no transcript entry was heard");
        append(n, "user");
        n += 1;
        append(n, "assistant");
        for _ in 0..4 {
            app.run_until_parked();
            app.advance_clock(crate::agents::EVERY);
            window.draw();
            std::thread::sleep(Duration::from_millis(10));
        }
        if saved("reader", &format!("heard-{n}")).is_some() {
            break;
        }
        n += 1;
    }
    let heard = saved("reader", &format!("heard-{n}")).unwrap();
    assert_eq!(
        (heard["sessionId"].clone(), heard["paneId"].clone()),
        (json!(SESSION), json!("0"))
    );
    assert_eq!(heard["entry"], json!({ "type": "assistant", "n": n }));
    // A user entry isn't one the reader may read, and the plugin that may read nothing heard nothing.
    assert!(saved("reader", &format!("heard-{}", n - 1)).is_none());
    assert!(saved("deaf", "deaf").is_none());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn transcript_entries_are_handed_only_to_plugins_whose_manifest_names_the_type() {
    let data = Data::temp();
    data.install_script(
        "reader",
        json!({ "permissions": { "transcript": ["assistant"] } }),
        READER,
        &[],
        "reader-scope",
    );
    data.install_script("deaf", json!({}), DEAF, &[], "deaf-scope");
    let (app, window) = open_plugins(&data, &Arc::default(), |_| {});
    let line = |entry: Value| Line {
        pane_id: "0".into(),
        session_id: SESSION.into(),
        bytes: entry.to_string().into_bytes(),
    };
    window.read(|w, _| {
        w.publish_transcripts(vec![
            line(json!({ "type": "user", "n": 1 })),
            line(json!({ "type": "assistant", "n": 2 })),
            Line {
                pane_id: "0".into(),
                session_id: SESSION.into(),
                bytes: b"not json".to_vec(),
            },
        ])
    });
    let heard = |key: &str| data.saved("reader", "reader-scope", key);
    let deadline = Instant::now() + Duration::from_secs(10);
    while heard("heard-2").is_none() {
        assert!(Instant::now() < deadline, "the entry never arrived");
        app.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        heard("heard-2"),
        Some(
            json!({ "sessionId": SESSION, "paneId": "0", "entry": { "type": "assistant", "n": 2 } })
        )
    );
    assert!(heard("heard-1").is_none());
    assert!(data.saved("deaf", "deaf-scope", "deaf").is_none());
}

// A debug build's dev root, with the repo's own plugins.

fn repo_plugins() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins")
        .canonicalize()
        .unwrap()
}

/// Whether `python3` can listen, for the test that starts a server in a pane.
fn has_python() -> bool {
    std::process::Command::new("python3")
        .arg("-c")
        .arg("import socket")
        .status()
        .is_ok_and(|s| s.success())
}

#[test]
fn dev_servers_from_the_dev_root_badges_a_pane_that_listens_and_its_button_opens_the_browser() {
    if !has_python() {
        return;
    }
    let data = Data::temp();
    let system = Arc::new(FakeSystem::default());
    let (app, mut window) = open_sized(Some(size(px(1000.0), px(640.0))), |config| {
        config.plugins = Some(crate::host::PluginPaths {
            dev: Some(repo_plugins()),
            ..data.paths()
        });
    });
    let fake: Arc<dyn System> = system.clone();
    window.read(|w, _| w.plugin_env.use_system(fake));
    window.draw();
    // The repo's plugins run with no `plugins.json` entry: always on, approved, and with a scope of their own.
    let ids = window.read(|w, _| w.plugins.iter().map(|p| p.id.clone()).collect::<Vec<_>>());
    assert!(
        ids.contains(&"dev-servers".to_owned()) && ids.contains(&"hud".to_owned()),
        "{ids:?}"
    );
    assert!(
        !data.paths().state.exists(),
        "the dev root wrote plugins.json"
    );

    // A program in the pane starts listening on a port.
    wait_for(
        &app,
        &window,
        |w| w.panes[&0].view.as_ref().is_ok_and(|_| true),
        "no pane",
    );
    window.simulate_input(
        "python3 -c \"import socket,time;s=socket.socket();s.bind(('127.0.0.1',0));s.listen(1);print('LISTENING',s.getsockname()[1],flush=True);time.sleep(120)\"",
    );
    window.simulate_keystroke("enter");
    let output = |window: &TestAppWindow<Workspace>| {
        String::from_utf8_lossy(
            &window.read(|w, cx| w.terminal(0).unwrap().read(cx).recent_output()),
        )
        .into_owned()
    };
    let deadline = Instant::now() + Duration::from_secs(30);
    let port = loop {
        let text = output(&window);
        // The program's output follows an escape sequence on its line; the typed command has no space after the word.
        if let Some(port) = text.split("LISTENING ").skip(1).find_map(|after| {
            let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
            digits.parse::<u16>().ok()
        }) {
            break port;
        }
        assert!(
            Instant::now() < deadline,
            "the server never started: {text:?}"
        );
        app.run_until_parked();
        app.advance_clock(Duration::from_millis(50));
        window.draw();
        std::thread::sleep(Duration::from_millis(20));
    };

    // A new pane is a change the plugin looks at, and the port is found in the first pane's process tree.
    press(&mut window, "d");
    let label = format!("localhost:{port}");
    until(&app, &mut window, "the pane never got its badge", |w| {
        w.plugin_ui
            .badges
            .get(&0)
            .and_then(|b| b.get("dev-servers"))
            .is_some_and(|badge| badge.label == label)
    });
    assert!(window.read(|w, _| !w.plugin_ui.badges.contains_key(&1)));
    window.update(|w, _, cx| {
        w.toggle_badge_card(0, "dev-servers".into(), cx);
        w.run_badge_action(0, "dev-servers".into(), format!("browser:{port}"), cx);
    });
    until(&app, &mut window, "the browser never opened", |_| {
        !system.opened.lock().unwrap().is_empty()
    });
    assert_eq!(
        *system.opened.lock().unwrap(),
        [format!("http://localhost:{port}/")]
    );
    // Stop the server, so the shell isn't left holding it.
    window.update(|w, window, cx| w.focus_pane(0, window, cx));
    window.simulate_keystroke("ctrl-c");
}

#[test]
fn a_tab_opened_with_args_reads_them_and_takes_new_ones_by_starting_over() {
    let data = Data::temp();
    let page = |key: &str| format!("wings.storage.set('{key}', {{ seen: wings.viewArgs() }});");
    let (one, many) = (page("one"), page("many"));
    data.install_script(
        "argsy",
        json!({ "contributes": { "views": [
            { "id": "one", "title": "One", "page": "dist/one.html", "opens": ["tab"], "newTab": true },
            { "id": "many", "title": "Many", "page": "dist/many.html", "opens": ["tab"], "newTab": true, "multiple": true }
        ] } }),
        "",
        &[("one", &one), ("many", &many)],
        "args-scope",
    );
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    let seen =
        |app: &TestApp, window: &mut TestAppWindow<Workspace>, key: &str, expected: Value| {
            until(app, window, &format!("{key} never read {expected}"), |_| {
                data.saved("argsy", "args-scope", key)
                    .is_some_and(|value| value["seen"] == expected)
            });
        };
    let tabs = |window: &mut TestAppWindow<Workspace>, view: &str| {
        window.read(|w, _| w.plugin_tabs.iter().filter(|tab| tab.view == view).count())
    };
    let frame = |window: &mut TestAppWindow<Workspace>| {
        window.read(|w, _| {
            w.plugin_tabs
                .iter()
                .find(|tab| tab.view == "one")
                .map(|tab| tab.page.entity_id())
        })
    };
    let open = |window: &mut TestAppWindow<Workspace>, view: &str, args: Option<Value>| {
        window.update(|w, window, cx| {
            assert!(w.open_plugin_view("argsy", view, args, window, cx));
        });
    };

    open(&mut window, "one", None);
    seen(&app, &mut window, "one", Value::Null);
    let first = frame(&mut window);

    // A tab that is open with no args takes the first ones by starting over, and the same ones again change nothing.
    open(&mut window, "one", Some(json!({ "n": 1 })));
    seen(&app, &mut window, "one", json!({ "n": 1 }));
    let second = frame(&mut window);
    assert_ne!(first, second);
    open(&mut window, "one", Some(json!({ "n": 1 })));
    open(&mut window, "one", None);
    assert_eq!(frame(&mut window), second);
    open(&mut window, "one", Some(json!({ "n": 2 })));
    seen(&app, &mut window, "one", json!({ "n": 2 }));
    assert_ne!(frame(&mut window), second);
    assert_eq!(tabs(&mut window, "one"), 1);

    // A `multiple` page opens a tab for each args, and shows the one that has them already.
    for (args, count) in [(1, 1), (2, 2), (1, 2)] {
        open(&mut window, "many", Some(json!({ "n": args })));
        assert_eq!(tabs(&mut window, "many"), count, "args {args}");
    }
}

#[test]
fn a_plugin_tabs_saved_state_comes_back_after_a_relaunch() {
    let dir = std::env::temp_dir().join(format!("wings-native-tab-state-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let file = dir.join("workspace.json");
    let data = Data::temp();
    data.install_script(
        "keeper",
        json!({ "contributes": { "views": [{ "id": "page", "title": "Keeper", "page": "dist/page.html", "opens": ["tab"], "newTab": true }] } }),
        "",
        &[("page", "wings.tabState().then(async (s) => { await wings.storage.set('seen', s); if (s == null) await wings.setTabState({ n: 1 }); await wings.storage.set('set', true); });")],
        "keeper-scope",
    );
    {
        let (app, mut window) = open_plugins(&data, &Arc::default(), |config| {
            config.state = Some(file.clone())
        });
        window.update(|w, window, cx| {
            assert!(w.open_plugin_view("keeper", "page", None, window, cx))
        });
        until(&app, &mut window, "the page never saved its state", |_| {
            data.saved("keeper", "keeper-scope", "set") == Some(json!(true))
        });
        app.run_until_parked();
    }
    let (app, mut window) = open_plugins(&data, &Arc::default(), |config| {
        config.state = Some(file.clone())
    });
    until(
        &app,
        &mut window,
        "the relaunched tab never read its saved state",
        |_| data.saved("keeper", "keeper-scope", "seen") == Some(json!({ "n": 1 })),
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_plugin_focusing_a_pane_shows_the_tab_it_is_in() {
    use super::Active::Terminal;
    let data = Data::temp();
    data.install_script(
        "focuser",
        json!({ "permissions": { "panes": [] } }),
        "let done = false; wings.onPanes(async (panes) => { if (panes.length < 2 || done) return; done = true; await wings.focusPane(panes[0].paneId); await wings.storage.set('focused', panes[0].paneId); });",
        &[],
        "focuser-scope",
    );
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    // A new tab is shown, and the plugin hears of its pane.
    press(&mut window, "t");
    until(
        &app,
        &mut window,
        "the plugin never focused the first pane",
        |_| data.saved("focuser", "focuser-scope", "focused").is_some(),
    );
    app.run_until_parked();
    assert_eq!(window.read(|w, _| w.active()), Terminal(0));
}

/// One request on the app's socket, as the `wings` command sends it, answered while the window runs.
fn ask_socket(
    app: &TestApp,
    window: &mut TestAppWindow<Workspace>,
    socket: &std::path::Path,
    request: Value,
) -> Value {
    use std::io::{BufRead, BufReader, Write};
    let answer = Arc::new(Mutex::new(None));
    let (found, socket) = (answer.clone(), socket.to_owned());
    std::thread::spawn(move || {
        let stream = std::os::unix::net::UnixStream::connect(socket).unwrap();
        writeln!(&stream, "{request}").unwrap();
        // The app also writes empty lines and events on the connection; the reply has the request's id.
        for line in BufReader::new(&stream).lines() {
            let message: Value = serde_json::from_str(&line.unwrap()).unwrap_or(Value::Null);
            if message.get("id") == Some(&json!(1)) {
                *found.lock().unwrap() = Some(message);
                return;
            }
        }
    });
    until(app, window, "the app never answered on the socket", |_| {
        answer.lock().unwrap().is_some()
    });
    answer.lock().unwrap().take().unwrap()
}

#[test]
fn claude_code_reaches_the_app_through_its_socket_and_a_hook_reaches_the_plugin() {
    let data = Data::temp();
    data.install_script(
        "hooker",
        json!({ "contributes": { "hooks": ["SessionStart"], "mcpTools": [{ "name": "hi", "description": "Says hi", "inputSchema": { "type": "object" } }], "mcpInstructions": "Hooker (hooker__*): says hi." } }),
        "wings.onHook('SessionStart', (input, c) => `hello ${input.source} in ${c.project?.name ?? 'no project'}`);
         wings.onTool('hi', (input) => `hi ${input.name}`);",
        &[],
        "hooker-scope",
    );
    // What a crash left behind: a socket file nothing listens on, which the app replaces.
    let socket = data.root.join("app-data/mcp.sock");
    fs::create_dir_all(socket.parent().unwrap()).unwrap();
    drop(std::os::unix::net::UnixListener::bind(&socket).unwrap());
    let (app, mut window) = open_plugins(&data, &Arc::default(), |config| {
        config.socket = Some(socket.clone())
    });

    let list = ask_socket(&app, &mut window, &socket, json!({ "id": 1, "op": "list" }));
    let names: Vec<&str> = list["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    for name in [
        "search_history",
        "list_sessions",
        "read_pane",
        "open_pane",
        "hooker__hi",
    ] {
        assert!(names.contains(&name), "{name} in {names:?}");
    }
    assert_eq!(
        list["instructions"],
        json!(["Hooker (hooker__*): says hi."])
    );

    // The main may still be starting; Claude Code's next call finds it.
    let started = Instant::now();
    let call = loop {
        let call = ask_socket(
            &app,
            &mut window,
            &socket,
            json!({ "id": 1, "op": "call", "name": "hooker__hi", "arguments": { "name": "Claude" } }),
        );
        let text = call["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        if !text.contains("still starting") || started.elapsed() > Duration::from_secs(10) {
            break call;
        }
    };
    assert_eq!(
        call["result"],
        json!({ "content": [{ "type": "text", "text": "hi Claude" }], "isError": false })
    );

    let hook = ask_socket(
        &app,
        &mut window,
        &socket,
        json!({ "id": 1, "op": "hook", "input": { "hook_event_name": "SessionStart", "source": "startup" } }),
    );
    assert_eq!(
        hook["result"],
        json!({ "context": "hello startup in no project" })
    );

    let sessions = ask_socket(
        &app,
        &mut window,
        &socket,
        json!({ "id": 1, "op": "call", "name": "list_sessions", "arguments": {} }),
    );
    assert_eq!(sessions["result"]["isError"], false, "{sessions}");

    let status = json!({ "session_id": "3c21b0c5-8113-4f55-9493-fa156c3fa369", "rate_limits": { "five_hour": { "used_percentage": 40 } } });
    ask_socket(
        &app,
        &mut window,
        &socket,
        json!({ "id": 1, "op": "statusline", "status": status }),
    );
    let view = window.read(|w, _| crate::page::plugin::Env::statusline(&*w.plugin_env));
    assert_eq!(view["rateLimits"]["fiveHour"]["usedPercentage"], 40.0);
}

/// A page with a button in its bottom right corner that counts its presses. `PROPS` is what `mount` puts on the root.
macro_rules! corner_page {
    ($props:literal) => {
        concat!(
            "let n = 0;\n",
            "function render(){wings.render(JSON.stringify({id:0,type:'#root',props:",
            $props,
            ",children:[{id:1,type:'Stack',props:{grow:true},children:[{id:2,type:'#text',text:'Pressed '+n+'.'}]},",
            "{id:3,type:'Row',props:{justify:'end'},children:[",
            "{id:4,type:'Button',props:{onClick:'4:click'},children:[{id:5,type:'#text',text:'Corner'}]}]}]}));}\n",
            "globalThis.__wingsEvent=()=>{n++;render();return true;};render();",
        )
    };
}

#[test]
fn a_fill_tab_page_reaches_its_cards_edges_and_one_that_does_not_fill_keeps_its_padding() {
    let data = Data::temp();
    data.install_script(
        "framed",
        json!({ "contributes": { "views": [
            { "id": "filled", "title": "Filled", "page": "dist/filled.html", "opens": ["tab"], "newTab": true },
            { "id": "padded", "title": "Padded", "page": "dist/padded.html", "opens": ["tab"], "newTab": true }
        ] } }),
        "",
        &[
            ("filled", corner_page!("{fill:true}")),
            ("padded", corner_page!("{}")),
        ],
        "framed-scope",
    );
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    let shows = |app: &TestApp, window: &mut TestAppWindow<Workspace>, view: &str, text: &str| {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            app.run_until_parked();
            window.draw();
            let page = window.read(|w, cx| {
                w.plugin_tabs
                    .iter()
                    .find(|tab| tab.view == view)
                    .map(|tab| tab.page.read(cx).text())
                    .unwrap_or_default()
            });
            if page.contains(text) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "{view} shows {page:?}, not {text:?}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    let viewport = window.update(|_, window, _| window.viewport_size());
    // `inset` points in from the bottom right corner of the tab's card, which sits 8pt in from the window's right and
    // bottom edges (the desktop's `p-2 pt-0`) inside a 1pt border.
    let click = |app: &TestApp, window: &mut TestAppWindow<Workspace>, inset: f32| {
        let at = point(
            viewport.width - px(9.0 + inset),
            viewport.height - px(9.0 + inset),
        );
        window.simulate_click(at, MouseButton::Left);
        std::thread::sleep(Duration::from_millis(100));
        app.run_until_parked();
        window.draw();
    };

    // A fill page's button is right in the card's corner.
    window
        .update(|w, window, cx| assert!(w.open_plugin_view("framed", "filled", None, window, cx)));
    shows(&app, &mut window, "filled", "Pressed 0.");
    click(&app, &mut window, 2.0);
    shows(&app, &mut window, "filled", "Pressed 1.");

    // Any other page sits inside plugin-ui.css's 14px of `.w-shell` padding: the corner is empty, and the button is
    // just inside the padding.
    window
        .update(|w, window, cx| assert!(w.open_plugin_view("framed", "padded", None, window, cx)));
    shows(&app, &mut window, "padded", "Pressed 0.");
    click(&app, &mut window, 2.0);
    shows(&app, &mut window, "padded", "Pressed 0.");
    click(&app, &mut window, 16.0);
    shows(&app, &mut window, "padded", "Pressed 1.");
}
