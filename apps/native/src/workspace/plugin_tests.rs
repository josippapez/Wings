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
    /// The named accounts Wings was connected into, never in the user's home.
    connected: Mutex<Vec<String>>,
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

    fn connect_claude_account(&self, name: &str) {
        self.connected.lock().unwrap().push(name.to_owned());
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
    // It opens docked beside the terminal; the clicks below are where it is when it fills the window.
    window.update(|w, window, cx| w.expand_diff(true, window, cx));
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

/// The desktop's review threads: a plugin's diff with comments opens with them, and a reply typed under one reaches the
/// plugin's `onReply`, whose error shows under the box with the text kept, and whose `updateDiff` adds the reply.
#[test]
fn a_reply_typed_under_a_review_comment_reaches_the_plugin() {
    let data = Data::temp();
    let patch = "diff --git a/one b/one\n--- a/one\n+++ b/one\n@@ -1 +1 @@\n-old\n+new\n";
    data.install_script("differ", json!({ "contributes": { "ui": ["diff"] } }), &format!(r#"
        const comments = [{{ id: 1, replyTo: null, path: 'one', line: 1, side: 'additions', author: 'maria', body: 'Why new?', createdAt: '2026-11-01T10:00:00Z', url: '', canReply: true }}];
        let started = false;
        wings.onReply(async ({{ diffId, replyTo, body }}) => {{
            if (body === 'fail') throw new Error('GitHub refused it');
            await wings.storage.set('reply', {{ replyTo, body }});
            comments.push({{ id: 2, replyTo, path: 'one', line: 1, side: 'additions', author: 'you', body, createdAt: '2026-11-01T11:00:00Z', url: '', canReply: false }});
            await wings.updateDiff(diffId, {{ comments }});
        }});
        wings.onPanes(async list => {{ if (!list.length || started) return; started = true; await wings.openDiff({{ title: 'PR', patch: {}, comments }}); }});
    "#, json!(patch)), &[], "diff-scope");
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    until(
        &app,
        &mut window,
        "the diff with comments never opened",
        |w| w.plugin_ui.diff.is_some(),
    );
    window.update(|w, window, cx| w.expand_diff(true, window, cx));
    window.draw();
    let viewer = window.read(|w, _| w.plugin_ui.diff.as_ref().unwrap().0.clone());
    let reply = crate::ui::spot_at("diff-reply-1").expect("the thread's Reply button was drawn");
    window.simulate_click(reply.center(), MouseButton::Left);
    window.draw();
    window.simulate_input("fail");
    window.simulate_keystroke("cmd-enter");
    until(&app, &mut window, "the plugin's error never showed", |_| {
        app.read(|cx| viewer.read(cx).reply_error(1)).as_deref() == Some("GitHub refused it")
    });
    window.simulate_keystroke("cmd-a");
    window.simulate_input("Thanks, fixed.");
    window.simulate_keystroke("cmd-enter");
    until(
        &app,
        &mut window,
        "the reply never reached the plugin",
        |_| {
            data.saved("differ", "diff-scope", "reply")
                == Some(json!({ "replyTo": 1, "body": "Thanks, fixed." }))
        },
    );
    until(&app, &mut window, "the posted reply never showed", |_| {
        app.read(|cx| {
            let viewer = viewer.read(cx);
            !viewer.reply_open(1)
                && viewer
                    .view
                    .update
                    .comments
                    .as_ref()
                    .is_some_and(|c| c.len() == 2)
        })
    });
}

/// A plugin that may open diffs and does nothing of its own, so a test opens one from the tab it wants.
fn install_differ(data: &Data) {
    data.install_script(
        "differ",
        json!({ "contributes": { "ui": ["diff"] } }),
        "",
        &[],
        "differ-scope",
    );
}

/// Opens a diff of two files from the tab on screen, as the plugin's `openDiff` does.
fn open_diff(window: &mut TestAppWindow<Workspace>) {
    let patch = "diff --git a/one b/one\n--- a/one\n+++ b/one\n@@ -1 +1 @@\n-old\n+new\ndiff --git a/two b/two\n--- a/two\n+++ b/two\n@@ -1 +1 @@\n-a\n+b\n";
    window.update(|w, window, cx| {
        let view = crate::diff::View {
            id: "differ:1".into(),
            plugin_id: "differ".into(),
            title: "Changes".into(),
            subtitle: String::new(),
            cwd: String::new(),
            refresh: false,
            update: crate::diff::clean_update(&json!({ "patch": patch })).unwrap(),
        };
        w.open_diff_view(view, window, cx);
    });
    window.draw();
}

/// Where the diff was last drawn, inside its card's border when docked.
fn diff_at() -> gpui::Bounds<gpui::Pixels> {
    crate::ui::spot_at("diff").expect("the diff was drawn")
}

/// The right edge of the terminal of `pane`, as last drawn.
fn terminal_right(window: &TestAppWindow<Workspace>, pane: usize) -> f32 {
    window.read(|w, cx| {
        let drawn = w.terminal(pane).unwrap().read(cx).drawn_at().unwrap();
        drawn.right().as_f32()
    })
}

/// The desktop's `diffDocked`: the diff opens beside the tab on screen, which gives it its room, and shows only while
/// that tab does, kept while another is shown. Expand fills the window and Dock beside the tab goes back. Closing the
/// tab closes the diff.
#[test]
fn a_diff_docks_beside_the_tab_it_opened_from_and_shows_only_on_that_tab() {
    let data = Data::temp();
    install_differ(&data);
    let (_app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    let full = terminal_right(&window, 0);
    let first = window.read(|w, _| w.active());
    open_diff(&mut window);
    // In the 1000 window the tab keeps its 320 beside the 248 sidebar, and the dock has what's left, under its 620.
    let width = 1000.0 - 248.0 - 320.0;
    let docked = (1000.0 - width + 1.0, 1000.0 - 8.0 - 1.0);
    let at = diff_at();
    assert_eq!((at.left().as_f32(), at.right().as_f32()), docked);
    assert_eq!(
        full - terminal_right(&window, 0),
        width,
        "the tab gives the dock its room"
    );
    // The keys stay with the terminal: the dock is no dialog.
    assert!(window.update(|w, window, cx| {
        gpui::Focusable::focus_handle(&w.terminal(0).unwrap(), cx).is_focused(window)
    }));

    // On another tab it's out of sight but kept, and it's back with its tab.
    press(&mut window, "t");
    window.draw();
    let second = window.read(|w, _| w.terminals[1].layout.panes()[0]);
    assert_eq!(terminal_right(&window, second), full);
    assert!(window.read(|w, _| w.plugin_ui.diff.is_some()));
    window.update(|w, window, cx| w.show(first, window, cx));
    window.draw();
    assert_eq!(full - terminal_right(&window, 0), width);

    // Expand fills the window, and Dock beside the tab goes back.
    let expand = crate::ui::spot_at("diff-expand").unwrap().center();
    window.simulate_click(expand, MouseButton::Left);
    window.draw();
    assert_eq!(
        diff_at(),
        gpui::Bounds::new(point(px(0.0), px(0.0)), size(px(1000.0), px(640.0)))
    );
    let dock = crate::ui::spot_at("diff-dock").unwrap().center();
    window.simulate_click(dock, MouseButton::Left);
    window.draw();
    let at = diff_at();
    assert_eq!((at.left().as_f32(), at.right().as_f32()), docked);

    // Its file list hides, and stays hidden for the next diff.
    let toggle = crate::ui::spot_at("diff-files-toggle").unwrap().center();
    window.simulate_click(toggle, MouseButton::Left);
    window.draw();
    assert!(!window.read(|w, _| w.prefs.diff_dock_files));
    assert_eq!(crate::ui::spot_at("diff-code").unwrap().left(), at.left());

    // Closing the tab it docks beside closes it.
    window.update(|w, window, cx| w.close_tab_of(first, window, cx));
    window.draw();
    assert!(window.read(|w, _| w.plugin_ui.diff.is_none()));
}

/// A window with no room for the dock beside the tab's 320 shows the diff filling it, with no button to dock, rather than
/// squeezing the terminal to a sliver; given room, the diff docks again.
#[test]
fn a_window_too_narrow_for_the_dock_shows_the_diff_filling_it() {
    let data = Data::temp();
    install_differ(&data);
    let (_app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    window.simulate_resize(size(px(800.0), px(640.0)));
    window.draw();
    open_diff(&mut window);
    window.draw();
    // 800 less the 248 sidebar leaves the tab's 320 and only 232 for the dock, under its 360.
    assert_eq!(
        diff_at(),
        gpui::Bounds::new(point(px(0.0), px(0.0)), size(px(800.0), px(640.0)))
    );
    window.read(|w, cx| {
        let viewer = w.plugin_ui.diff.as_ref().unwrap().0.read(cx);
        assert!(
            !viewer.docked && !viewer.can_dock,
            "no Expand or Dock button"
        );
    });

    // With the sidebar hidden there is room again, and it docks.
    window.update(|w, _, cx| w.toggle_docked(cx));
    window.draw();
    window.draw();
    window.read(|w, cx| {
        assert_eq!(w.diff_shown(), Some(super::diff::Shown::Docked));
        assert!(w.plugin_ui.diff.as_ref().unwrap().0.read(cx).docked);
    });
}

/// The dock's left edge drags within the desktop's `SIDES.diff`, leaving the tab its 320, and the width is written once
/// the pointer is released and comes back on the next launch, as the sidebars' do.
#[test]
fn dragging_the_docked_diffs_edge_resizes_it_within_its_limits_and_saves_when_it_is_released() {
    use super::diff::{INITIAL_WIDTH, MAX_WIDTH, MIN_WIDTH};
    let data = Data::temp();
    install_differ(&data);
    let dir = crate::projects::test_dir("diff-resize");
    let file = dir.join("workspace.json");
    // Wide enough for the widest dock beside the sidebar and the tab's 320.
    let wide = 2400.0;
    let open = || {
        let (app, mut window) = open_sized(Some(size(px(wide), px(640.0))), |config| {
            config.plugins = Some(data.paths());
            config.state = Some(file.clone());
        });
        window.draw();
        open_diff(&mut window);
        (app, window)
    };
    let width = |window: &TestAppWindow<Workspace>| window.read(|w, _| w.plugin_ui.diff_dock.width);
    let saved = || crate::saved::load_settings(&file).diff_width;
    // From the gap left of the dock drawn `drawn` wide, to `to`.
    let drag = |window: &mut TestAppWindow<Workspace>, drawn: f32, to: f32| {
        window.simulate_mouse_down(point(px(wide - drawn - 4.0), px(300.0)), MouseButton::Left);
        window.draw();
        window.simulate_event(gpui::MouseMoveEvent {
            position: point(px(to), px(300.0)),
            pressed_button: Some(MouseButton::Left),
            modifiers: gpui::Modifiers::default(),
        });
    };
    let release = |window: &mut TestAppWindow<Workspace>, at: f32| {
        window.simulate_mouse_up(point(px(at), px(300.0)), MouseButton::Left);
        window.draw();
    };

    let (_app, mut window) = open();
    assert_eq!(width(&window), INITIAL_WIDTH);
    let before = saved();
    // It follows the pointer by how far it moved, and nothing is written until the pointer is released.
    let from = wide - INITIAL_WIDTH - 4.0;
    drag(&mut window, INITIAL_WIDTH, from - 100.0);
    assert_eq!((width(&window), saved()), (INITIAL_WIDTH + 100.0, before));
    release(&mut window, from - 100.0);
    assert_eq!(saved(), Some(INITIAL_WIDTH + 100.0));
    let at = diff_at();
    assert_eq!(at.left().as_f32(), wide - INITIAL_WIDTH - 100.0 + 1.0);

    drag(&mut window, INITIAL_WIDTH + 100.0, 0.0);
    release(&mut window, 0.0);
    assert_eq!((width(&window), saved()), (MAX_WIDTH, Some(MAX_WIDTH)));
    drag(&mut window, MAX_WIDTH, wide);
    release(&mut window, wide);
    assert_eq!((width(&window), saved()), (MIN_WIDTH, Some(MIN_WIDTH)));

    // The next launch starts at the saved width, and a width outside the limits starts over.
    drop((window, _app));
    {
        let (_app, window) = open();
        assert_eq!(width(&window), MIN_WIDTH);
    }
    std::fs::write(&file, r#"{"version":1,"diff_width":20.0,"projects":{}}"#).unwrap();
    let (_app, window) = open();
    assert_eq!(width(&window), INITIAL_WIDTH);
    drop((window, _app));
    std::fs::remove_dir_all(dir).unwrap();
}

/// Dragging the dock's edge changes the size of the pane area on every frame of the drag, but the shell is given its
/// new size once, when the panes hold still, as with the sidebars' edges.
#[test]
fn dragging_the_docked_diffs_edge_resizes_the_terminal_once_when_it_settles() {
    let data = Data::temp();
    install_differ(&data);
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    let grid = |window: &TestAppWindow<Workspace>| {
        window.read(|w, cx| w.terminal(0).unwrap().read(cx).grid())
    };
    // Real time for the shell's thread to take each size, fake time for the app's timers.
    let settle = |window: &mut TestAppWindow<Workspace>| {
        for _ in 0..15 {
            app.advance_clock(Duration::from_millis(100));
            app.run_until_parked();
            window.draw();
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    open_diff(&mut window);
    settle(&mut window);
    let (before, start) = (grid(&window), terminal_right(&window, 0));
    // The dock has the 432 the 1000 window leaves it, so it can only get narrower: the pointer goes right, from the
    // gap left of the dock.
    let mut x = 1000.0 - 432.0 - 4.0;
    window.simulate_mouse_down(point(px(x), px(300.0)), MouseButton::Left);
    let (mut grids, mut frames, mut drawn, mut slowest) =
        (vec![before], 0, Duration::ZERO, Duration::ZERO);
    // 30 frames of 16ms, the pointer 2 points further right each.
    for _ in 0..30 {
        x += 2.0;
        window.simulate_event(gpui::MouseMoveEvent {
            position: point(px(x), px(300.0)),
            pressed_button: Some(MouseButton::Left),
            modifiers: gpui::Modifiers::default(),
        });
        app.advance_clock(Duration::from_millis(16));
        app.run_until_parked();
        let begun = Instant::now();
        window.draw();
        let took = begun.elapsed();
        frames += 1;
        drawn += took;
        slowest = slowest.max(took);
        grids.push(grid(&window));
    }
    window.simulate_mouse_up(point(px(x), px(300.0)), MouseButton::Left);
    settle(&mut window);
    grids.push(grid(&window));
    grids.dedup();
    let gained = terminal_right(&window, 0) - start;
    eprintln!(
        "[measure] docked diff drag: {frames} frames, {:.2} ms a frame on average, {:.2} ms at most, the panes gained {gained} points, grids {grids:?}",
        drawn.as_secs_f64() * 1000.0 / frames as f64,
        slowest.as_secs_f64() * 1000.0,
    );
    assert_eq!(gained, 60.0, "the panes follow the pointer");
    assert_eq!(
        grids.len(),
        2,
        "one resize of the shell, not one a frame: {grids:?}"
    );
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
    window.update(|w, window, cx| w.hover_popover(key.clone(), false, true, window, cx));
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
    window.update(|w, window, cx| {
        w.hover_popover(key.clone(), false, false, window, cx);
        w.hover_popover(key.clone(), true, true, window, cx);
    });
    app.advance_clock(Duration::from_millis(300));
    window.read(|w, _| assert_eq!(w.plugin_ui.popover.open.as_ref(), Some(&key)));
    window.update(|w, window, cx| w.hover_popover(key.clone(), true, false, window, cx));
    app.advance_clock(Duration::from_millis(210));
    window.read(|w, _| assert_eq!(w.plugin_ui.popover.open, None));
    window.read(|w, _| assert!(w.plugin_ui.popover.pages.contains_key(&key)));
    // Not over its own open sidebar.
    window.update(|w, _, cx| w.show_sidebar("timer", "tracker", cx));
    window.update(|w, window, cx| w.hover_popover(key.clone(), false, true, window, cx));
    app.advance_clock(Duration::from_millis(400));
    window.read(|w, _| assert_eq!(w.plugin_ui.popover.open, None));
    // Two minutes out of view, its page is dropped, and the next show loads it again.
    app.advance_clock(Duration::from_secs(121));
    window.read(|w, _| assert!(!w.plugin_ui.popover.pages.contains_key(&key)));
}

/// A sidebar's page is kept for as long as the app runs, so one that stopped stayed stopped, its buttons dead, as
/// Cyclops' did. Its error now comes with Reload, which loads the page again in the panel.
#[test]
fn a_sidebar_page_that_stopped_is_loaded_again_by_its_reload_button() {
    let data = Data::temp();
    // Draws, then loops until it is stopped.
    let sidebar = "wings.render(JSON.stringify({id:0,type:'#root',children:[{id:1,type:'Text',children:[{id:2,type:'#text',text:'Up'}]}]}));
        setTimeout(() => { for (;;) {} }, 300);";
    data.install_script(
        "timer",
        json!({ "contributes": { "sidebars": [{ "id": "tracker", "title": "Cyclops", "page": "dist/sidebar.html" }] } }),
        "",
        &[("sidebar", sidebar)],
        "timer-scope",
    );
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    let key = ("timer".to_owned(), "tracker".to_owned());
    window.update(|w, _, cx| w.show_sidebar("timer", "tracker", cx));
    let first = window.read(|w, _| w.sidebar_pages[&key].clone());
    let wait = |window: &mut TestAppWindow<Workspace>, what: &str, done: &dyn Fn(&crate::page::PageView) -> bool| {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !window.read(|w, cx| done(w.sidebar_pages[&key].read(cx))) {
            assert!(Instant::now() < deadline, "{what}");
            app.run_until_parked();
            window.draw();
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    // A page that runs has no Reload to press.
    wait(&mut window, "the page never drew", &|page| page.text().contains("Up"));
    assert!(window.read(|_, cx| first.read(cx).reload_button().is_none()));
    wait(&mut window, "the page was never stopped", &|page| page.stopped());
    let reload = window.read(|_, cx| first.read(cx).reload_button()).expect("Reload by the error");
    window.update(|_, window, cx| reload(window, cx));
    app.run_until_parked();
    // Another page in the same place, drawn again and running.
    let second = window.read(|w, _| w.sidebar_pages[&key].clone());
    assert_ne!(first.entity_id(), second.entity_id());
    window.read(|w, _| assert_eq!(w.right.as_ref(), Some(&key)));
    wait(&mut window, "the reloaded page never drew", &|page| page.text().contains("Up") && !page.stopped());
}

/// Typing in Cyclops' popover, or picking from a list or menu in it, keeps it open: gpui reads a key press as the
/// pointer leaving, and a field or menu in use holds it even once the pointer goes, as does a press on a list drawn past
/// the card. It goes when the keyboard leaves it with the pointer away, or on a press elsewhere.
#[test]
fn a_popover_stays_open_while_you_type_in_it_or_use_its_menu() {
    let data = Data::temp();
    let page = r#"
let text = '';
function render(){
  wings.render(JSON.stringify({id:0,type:'#root',children:[
    {id:9,type:'Text',children:[{id:10,type:'#text',text:'No timer running'}]},
    {id:4,type:'Select',props:{label:'Project',value:'a',options:[{value:'a',label:'Alpha'},{value:'b',label:'Beta'}]}},
    {id:1,type:'Stack',props:{gap:'s'},children:[{id:3,type:'TextInput',props:{label:'Task',value:text,onChange:'3:onChange'}},
      {id:5,type:'Row',props:{gap:'s',wrap:true},children:['Code review','Checkout redesign'].map(t=>({id:t.length,type:'Button',props:{chip:true,pressed:false,onClick:t.length+':onClick'},children:[{id:100+t.length,type:'#text',text:t}]}))}]},
    {id:7,type:'Text',children:[{id:8,type:'#text',text:'Task='+text+'.'}]}]}));
}
globalThis.__wingsEvent=(h,json)=>{ text = h==='3:onChange' ? JSON.parse(json).value : 'Checkout redesign'; render(); return true; };
render();"#;
    data.install_script(
        "timer",
        json!({ "contributes": { "sidebars": [{ "id": "tracker", "title": "Cyclops", "page": "dist/sidebar.html",
            "popover": { "page": "dist/popover.html", "width": 360, "height": 160 } }] } }),
        "",
        &[("sidebar", page), ("popover", page)],
        "timer-scope",
    );
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    // gpui tells focus listeners only about an active window's focus.
    window.update(|_, window, _| window.activate_window());
    let key = ("timer".to_owned(), "tracker".to_owned());
    let open = |window: &mut TestAppWindow<Workspace>| {
        window.read(|w, _| w.plugin_ui.popover.open.is_some())
    };
    let point_at_button = |app: &TestApp, window: &mut TestAppWindow<Workspace>| {
        window.update(|w, window, cx| w.hover_popover(key.clone(), false, true, window, cx));
        app.advance_clock(Duration::from_millis(310));
        assert!(open(window), "pointing at the button never showed it");
    };
    point_at_button(&app, &mut window);
    let popover = window.read(|w, _| w.popover_page("timer", "tracker").unwrap());
    let deadline = Instant::now() + Duration::from_secs(10);
    let field = loop {
        app.run_until_parked();
        window.draw();
        if let Some(focus) = window.read(|_, cx| popover.read(cx).first_input_focus(cx)) {
            break focus;
        }
        assert!(
            Instant::now() < deadline,
            "the popover never drew its field"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    // The pointer moves from the button onto the card, and you type in its field.
    let card = crate::ui::spot_at("plugin-popover").expect("the card was drawn");
    window.update(|w, window, cx| w.hover_popover(key.clone(), false, false, window, cx));
    window.simulate_mouse_move(card.center());
    window.draw();
    window.update(|_, window, cx| field.focus(window, cx));
    window.draw();
    window.simulate_input("re");
    let shows = |app: &TestApp, window: &mut TestAppWindow<Workspace>, text: &str| {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !window.read(|_, cx| popover.read(cx).text().contains(text)) {
            assert!(Instant::now() < deadline, "the page never showed {text}");
            app.run_until_parked();
            window.draw();
            std::thread::sleep(Duration::from_millis(10));
        }
        window.draw();
    };
    shows(&app, &mut window, "Task=re.");
    app.advance_clock(Duration::from_millis(300));
    assert!(open(&mut window), "typing in it closed it");
    // The keyboard leaves it with the pointer still on it, which typing didn't move: it stays.
    window.update(|_, window, cx| window.blur(cx));
    window.draw();
    app.advance_clock(Duration::from_millis(300));
    assert!(open(&mut window), "it closed under the pointer");
    window.update(|_, window, cx| field.focus(window, cx));
    window.draw();
    // The pointer leaves while the field has the keyboard: it stays.
    let away = point(card.left() - px(40.0), card.bottom() + px(40.0));
    window.simulate_mouse_move(away);
    window.draw();
    app.advance_clock(Duration::from_millis(300));
    assert!(open(&mut window), "leaving it while typing closed it");
    // A press on the field's list, drawn past the card, picks from it and keeps it.
    let row = crate::ui::spot_at("combobox-option:Checkout redesign").expect("the list was drawn");
    assert!(!card.contains(&row.center()), "the list ends in the card");
    window.simulate_click(row.center(), MouseButton::Left);
    shows(&app, &mut window, "Task=Checkout redesign.");
    assert!(open(&mut window), "a press on its list closed it");
    window.simulate_mouse_move(away);
    window.draw();
    // The keyboard leaves it with the pointer away: it goes.
    window.update(|_, window, cx| window.blur(cx));
    window.draw();
    app.advance_clock(Duration::from_millis(210));
    assert!(!open(&mut window), "it stayed once the keyboard left");

    // A menu of its page is open, which may reach past the card: the pointer leaving keeps it.
    point_at_button(&app, &mut window);
    window
        .update(|_, _, cx| popover.update(cx, |page, cx| assert!(page.open_select("Project", cx))));
    window.draw();
    window.update(|w, window, cx| w.hover_popover(key.clone(), false, false, window, cx));
    app.advance_clock(Duration::from_millis(300));
    assert!(open(&mut window), "leaving it with its menu open closed it");
    // A press on the card keeps it; one elsewhere closes it.
    window.simulate_click(
        point(card.right() - px(20.0), card.top() + px(8.0)),
        MouseButton::Left,
    );
    window.draw();
    assert!(open(&mut window), "a press on the card closed it");
    window.simulate_click(away, MouseButton::Left);
    window.draw();
    assert!(!open(&mut window), "a press elsewhere left it open");
}

/// Cyclops' task field, which lists recent tasks over the page as you type. In the popover under the title bar button,
/// itself a card drawn over the window, the list is drawn over the card and a click on it picks a task. On the right,
/// where a page is drawn again only when it changes, Escape takes the list away and Down and Enter pick.
#[test]
fn a_combobox_lists_its_options_over_the_popover_and_the_sidebar_page() {
    let data = Data::temp();
    // What wings-plugin-ui/native's `Combobox` renders: the field, and the tasks that match it as chips.
    let page = r#"
let text = '';
function render(){
  const tasks = ['Code review', 'Checkout redesign'].filter(t => t.toLowerCase().includes(text.toLowerCase()));
  wings.render(JSON.stringify({id:0,type:'#root',children:[
    {id:1,type:'Stack',props:{gap:'s'},children:[{id:3,type:'TextInput',props:{label:'Task',value:text,onChange:'3:onChange'}},
      {id:4,type:'Row',props:{gap:'s',wrap:true},children:tasks.map(t=>({id:t.length,type:'Button',props:{chip:true,pressed:false,onClick:t.length+':onClick'},children:[{id:100+t.length,type:'#text',text:t}]}))}]},
    {id:7,type:'Text',children:[{id:8,type:'#text',text:'Task='+text+'.'}]}]}));
}
globalThis.__wingsEvent=(h,json)=>{ text = h==='3:onChange' ? JSON.parse(json).value : (h.startsWith('11:') ? 'Code review' : 'Checkout redesign'); render(); return true; };
render();"#;
    data.install_script(
        "timer",
        json!({ "contributes": { "sidebars": [{ "id": "tracker", "title": "Cyclops", "page": "dist/sidebar.html",
            "popover": { "page": "dist/popover.html", "width": 360, "height": 300 } }] } }),
        "",
        &[("sidebar", page), ("popover", page)],
        "timer-scope",
    );
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    let shows = |app: &TestApp,
                 window: &mut TestAppWindow<Workspace>,
                 page: &gpui::Entity<crate::page::PageView>,
                 text: &str| {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !window.read(|_, cx| page.read(cx).text().contains(text)) {
            assert!(Instant::now() < deadline, "the page never showed {text}");
            app.run_until_parked();
            window.draw();
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    let type_in = |app: &TestApp,
                   window: &mut TestAppWindow<Workspace>,
                   page: &gpui::Entity<crate::page::PageView>| {
        let focus = window
            .read(|_, cx| page.read(cx).first_input_focus(cx))
            .expect("a field");
        window.update(|_, window, cx| focus.focus(window, cx));
        window.draw();
        window.simulate_input("re");
        shows(app, window, page, "Task=re.");
        window.draw();
    };

    window.update(|w, _, cx| w.shot_popover("timer", "tracker", cx));
    until(&app, &mut window, "the popover never opened", |w| {
        w.popover_page("timer", "tracker").is_some()
    });
    let popover = window.read(|w, _| w.popover_page("timer", "tracker").unwrap());
    shows(&app, &mut window, &popover, "Task=.");
    type_in(&app, &mut window, &popover);
    let row = crate::ui::spot_at("combobox-option:Checkout redesign").expect("the list was drawn");
    window.simulate_click(row.center(), MouseButton::Left);
    shows(&app, &mut window, &popover, "Task=Checkout redesign.");
    window.read(|w, _| assert!(w.plugin_ui.popover.open.is_some()));

    window.update(|w, _, cx| w.shot_sidebar("timer", "tracker", cx));
    until(&app, &mut window, "the sidebar never opened", |w| {
        w.sidebar_page("timer", "tracker").is_some()
    });
    let sidebar = window.read(|w, _| w.sidebar_page("timer", "tracker").unwrap());
    shows(&app, &mut window, &sidebar, "Task=.");
    type_in(&app, &mut window, &sidebar);
    let row = crate::ui::spot_at("combobox-option:Code review").expect("the list was drawn");
    window.simulate_keystroke("escape");
    window.draw();
    // The list is gone: a press where its first task was picks nothing.
    window.simulate_click(row.center(), MouseButton::Left);
    std::thread::sleep(Duration::from_millis(100));
    app.run_until_parked();
    window.draw();
    window.read(|_, cx| assert!(sidebar.read(cx).text().contains("Task=re.")));
    // Typing brings it back, and the keyboard picks.
    window.simulate_input("v");
    shows(&app, &mut window, &sidebar, "Task=rev.");
    window.simulate_keystroke("down");
    window.simulate_keystroke("enter");
    shows(&app, &mut window, &sidebar, "Task=Code review.");
}

/// Cyclops' popover shares the window with the panes behind it: Tab goes round its fields and never out to them.
#[test]
fn tab_goes_round_a_popovers_fields_and_stays_in_it() {
    let data = Data::temp();
    let page = r#"wings.render(JSON.stringify({id:0,type:'#root',children:[
      {id:3,type:'TextInput',props:{label:'Start',type:'time',value:''}},
      {id:4,type:'TextInput',props:{label:'End',type:'time',value:''}}]}));"#;
    data.install_script(
        "timer",
        json!({ "contributes": { "sidebars": [{ "id": "tracker", "title": "Cyclops", "page": "dist/sidebar.html",
            "popover": { "page": "dist/popover.html", "width": 360, "height": 300 } }] } }),
        "",
        &[("sidebar", page), ("popover", page)],
        "timer-scope",
    );
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    window.update(|w, _, cx| w.shot_popover("timer", "tracker", cx));
    until(&app, &mut window, "the popover never opened", |w| {
        w.popover_page("timer", "tracker").is_some()
    });
    let popover = window.read(|w, _| w.popover_page("timer", "tracker").unwrap());
    let deadline = Instant::now() + Duration::from_secs(10);
    let first = loop {
        app.run_until_parked();
        window.draw();
        if let Some(focus) = window.read(|_, cx| popover.read(cx).first_input_focus(cx)) {
            break focus;
        }
        assert!(
            Instant::now() < deadline,
            "the popover never drew its fields"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    window.update(|_, window, cx| first.focus(window, cx));
    window.draw();
    let page = popover.entity_id().to_string();
    for key in ["tab", "tab", "tab", "shift-tab", "shift-tab", "shift-tab"] {
        window.simulate_keystroke(key);
        window.draw();
        window.update(|_, window, _| {
            let inside = window
                .context_stack()
                .iter()
                .any(|context| context.get("plugin-page").is_some_and(|id| **id == page));
            assert!(inside, "{key} took focus out of the popover");
        });
    }
}

/// As on the desktop, Wings goes into a named account's Claude Code when a plugin picks the account for a project, opens
/// a pane in it, or turns its shared setup on or off. Whether Wings may (you didn't disconnect it) is the system's.
#[test]
fn picking_a_claude_account_connects_wings_into_it() {
    let data = Data::temp();
    let project = data.root.join("project");
    fs::create_dir_all(&project).unwrap();
    let main = r#"
        wings.onAction(async () => {
            try {
                await wings.setClaudeAccount(PROJECT, 'wings-picked');
                await wings.shareClaudeSetup('wings-shared', false);
                await wings.openPane({ claudeAccount: 'wings-started' });
                await wings.storage.set('done', true);
            } catch (error) { await wings.storage.set('failed', error.message); }
        });
    "#
    .replace("PROJECT", &json!(project).to_string());
    data.install_script(
        "picker",
        json!({ "permissions": { "panes": [], "claudeAccounts": true } }),
        &main,
        &[],
        "picker-scope",
    );
    let system = Arc::new(FakeSystem::default());
    let (app, mut window) = open_plugins(&data, &system, |config| config.project = project.clone());
    let host = window.read(|w, _| w.plugins[0].host.clone());
    host.send_action("0", "pick", |_| {});
    until(&app, &mut window, "the plugin never finished", |_| {
        data.saved("picker", "picker-scope", "done").is_some()
            || data.saved("picker", "picker-scope", "failed").is_some()
    });
    assert_eq!(data.saved("picker", "picker-scope", "failed"), None);
    assert_eq!(
        *system.connected.lock().unwrap(),
        ["wings-picked", "wings-shared", "wings-started"]
    );
    // Each after its folder and shared setup, which the fake made instead of your home folder.
    assert_eq!(
        *system.prepared.lock().unwrap(),
        ["wings-picked", "wings-started"]
    );
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
    let state = data.root.join("workspace.json");
    let (app, mut window) = open_plugins(&data, &system, |config| {
        config.project = project.clone();
        config.state = Some(state.clone());
    });
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
    drop((app, window));

    // Relaunched, the pane comes back under that account, and so does its shell, not under the project's.
    let (app, mut window) = open_plugins(&data, &system, |config| {
        config.project = project.clone();
        config.state = Some(state.clone());
    });
    let restored = window.read(|w, _| {
        let account = |pane: &super::Pane| pane.claude_account.clone();
        w.panes.values().filter_map(account).collect::<Vec<_>>()
    });
    assert_eq!(
        restored,
        [wings_spaces::claude_accounts::AccountPick {
            name: Some("wings-native-test".into())
        }]
    );
    let pane = window.read(|w, _| {
        *w.panes
            .iter()
            .find(|(_, pane)| pane.claude_account.is_some())
            .unwrap()
            .0
    });
    let terminal = window.read(|w, _| w.terminal(pane).unwrap());
    window.update(|_, _, cx| {
        terminal.update(cx, |view, cx| {
            view.write_input("echo account-''dir:$CLAUDE_CONFIG_DIR\r", cx)
        })
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while !String::from_utf8_lossy(&window.read(|_, cx| terminal.read(cx).recent_output()))
        .contains(&expected)
    {
        assert!(
            Instant::now() < deadline,
            "the restored shell never printed {expected}"
        );
        app.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Plugins the desktop installed from a release built before native pages existed (Changes 0.7.0 and Cyclops 0.8.3 from
/// GitHub): on and approved, with no `dist/native`. The sidebar lists them as needing an update instead of hiding them.
#[test]
fn a_plugin_on_without_a_native_build_is_listed_in_the_sidebar_as_needing_an_update() {
    let data = Data::temp();
    data.install_script(
        "desktop-only",
        json!({ "contributes": { "sidebars": [{ "id": "s", "title": "S", "page": "dist/s.html" }] } }),
        "",
        &[("s", "")],
        "desktop-only-scope",
    );
    fs::remove_dir_all(data.paths().dir.join("desktop-only/dist/native")).unwrap();
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    app.run_until_parked();
    window.update(|w, _, cx| {
        assert!(w.plugins.is_empty(), "nothing to run here");
        let waiting: Vec<_> = w
            .plugins_without_native_build()
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert_eq!(waiting, ["desktop-only"]);
        assert!(
            w.plugin_list(cx).is_some(),
            "the Plugins section shows for it"
        );
    });
    window.draw();
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
        point(px(978.0 - super::right_edge::INITIAL_WIDTH), px(58.0)),
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

/// The right-hand panel's edge drags within the desktop's `SIDES.right`, leaving the panes their 320, and the width is
/// written once the pointer is released and comes back on the next launch, as the project sidebar's does.
#[test]
fn dragging_the_right_sidebars_edge_resizes_it_within_its_limits_and_saves_when_it_is_released() {
    use super::right_edge::{INITIAL_WIDTH, MAX_WIDTH, MIN_WIDTH};
    let data = Data::temp();
    install_sider(&data);
    let dir = crate::projects::test_dir("right-resize");
    let file = dir.join("workspace.json");
    let open = |wide: f32| {
        let (app, mut window) = open_sized(Some(size(px(wide), px(640.0))), |config| {
            config.plugins = Some(data.paths());
            config.state = Some(file.clone());
        });
        window.update(|w, _, cx| w.show_sidebar("sider", "side", cx));
        window.draw();
        app.advance_clock(crate::ui::motion::SLIDE * 2);
        app.run_until_parked();
        window.draw();
        (app, window)
    };
    let width =
        |window: &TestAppWindow<Workspace>| window.read(|w, _| w.plugin_ui.right_size.width);
    let saved = || crate::saved::load_settings(&file).right_width;
    let right_edge = |window: &TestAppWindow<Workspace>| {
        window.read(|w, cx| {
            let drawn = w.terminal(0).unwrap().read(cx).drawn_at().unwrap();
            drawn.right().as_f32()
        })
    };
    // From the edge of the panel drawn `drawn` wide in a window `wide` across, to `to`.
    let drag = |window: &mut TestAppWindow<Workspace>, wide: f32, drawn: f32, to: f32| {
        window.simulate_mouse_down(point(px(wide - drawn + 4.0), px(300.0)), MouseButton::Left);
        window.draw();
        window.simulate_event(gpui::MouseMoveEvent {
            position: point(px(to), px(300.0)),
            pressed_button: Some(MouseButton::Left),
            modifiers: gpui::Modifiers::default(),
        });
    };
    let release = |window: &mut TestAppWindow<Workspace>, at: f32| {
        window.simulate_mouse_up(point(px(at), px(300.0)), MouseButton::Left);
        window.draw();
    };

    let (_app, mut window) = open(1600.0);
    assert_eq!(width(&window), INITIAL_WIDTH);
    let before = saved();
    // It follows the pointer by how far it moved, and nothing is written until the pointer is released.
    let from = 1600.0 - INITIAL_WIDTH + 4.0;
    drag(&mut window, 1600.0, INITIAL_WIDTH, from - 100.0);
    assert_eq!((width(&window), saved()), (INITIAL_WIDTH + 100.0, before));
    release(&mut window, from - 100.0);
    assert_eq!(saved(), Some(INITIAL_WIDTH + 100.0));

    drag(&mut window, 1600.0, INITIAL_WIDTH + 100.0, 0.0);
    release(&mut window, 0.0);
    assert_eq!((width(&window), saved()), (MAX_WIDTH, Some(MAX_WIDTH)));
    drag(&mut window, 1600.0, MAX_WIDTH, 1600.0);
    release(&mut window, 1600.0);
    assert_eq!((width(&window), saved()), (MIN_WIDTH, Some(MIN_WIDTH)));

    // In a 1000 window, the widest it draws is what leaves the panes 320 beside the 248 sidebar, and a drag starts from
    // there.
    drag(&mut window, 1600.0, MIN_WIDTH, 0.0);
    release(&mut window, 0.0);
    let gap = 1600.0 - MAX_WIDTH - right_edge(&window);
    // The test window takes the new size without telling the window, which reads it again here.
    window.simulate_resize(size(px(1000.0), px(640.0)));
    window.update(|_, window, cx| window.bounds_changed(cx));
    window.draw();
    let room = 1000.0 - 248.0 - 320.0;
    assert_eq!(right_edge(&window), 1000.0 - room - gap);
    drag(&mut window, 1000.0, room, 1000.0 - room + 4.0 + 10.0);
    release(&mut window, 1000.0 - room + 4.0 + 10.0);
    assert_eq!((width(&window), saved()), (room - 10.0, Some(room - 10.0)));

    // The next launch starts at the saved width, and a width outside the limits starts over.
    drop((window, _app));
    {
        let (_app, window) = open(1600.0);
        assert_eq!(width(&window), room - 10.0);
    }
    std::fs::write(&file, r#"{"version":1,"right_width":9999.0,"projects":{}}"#).unwrap();
    let (_app, window) = open(1600.0);
    assert_eq!(width(&window), INITIAL_WIDTH);
    drop((window, _app));
    std::fs::remove_dir_all(dir).unwrap();
}

/// Dragging the right-hand panel's edge changes the size of the pane area on every frame of the drag, but the shell is
/// given its new size once, when the panes hold still, as with the project sidebar's edge.
#[test]
fn dragging_the_right_sidebars_edge_resizes_the_terminal_once_when_it_settles() {
    let data = Data::temp();
    install_sider(&data);
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    window.update(|w, _, cx| w.show_sidebar("sider", "side", cx));
    let grid = |window: &TestAppWindow<Workspace>| {
        window.read(|w, cx| w.terminal(0).unwrap().read(cx).grid())
    };
    let right_edge = |window: &TestAppWindow<Workspace>| {
        window.read(|w, cx| {
            let drawn = w.terminal(0).unwrap().read(cx).drawn_at().unwrap();
            drawn.right().as_f32()
        })
    };
    // Real time for the shell's thread to take each size, fake time for the app's timers.
    let settle = |window: &mut TestAppWindow<Workspace>| {
        for _ in 0..15 {
            app.advance_clock(Duration::from_millis(100));
            app.run_until_parked();
            window.draw();
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    window.draw();
    settle(&mut window);
    let (before, start) = (grid(&window), right_edge(&window));
    // The panel starts 360 wide in the 1000 window; its edge is the first points of it.
    let mut x = 1000.0 - 360.0 + 4.0;
    window.simulate_mouse_down(point(px(x), px(300.0)), MouseButton::Left);
    let (mut grids, mut frames, mut drawn, mut slowest) =
        (vec![before], 0, Duration::ZERO, Duration::ZERO);
    // 30 frames of 16ms, the pointer 2 points further left each.
    for _ in 0..30 {
        x -= 2.0;
        window.simulate_event(gpui::MouseMoveEvent {
            position: point(px(x), px(300.0)),
            pressed_button: Some(MouseButton::Left),
            modifiers: gpui::Modifiers::default(),
        });
        app.advance_clock(Duration::from_millis(16));
        app.run_until_parked();
        let begun = Instant::now();
        window.draw();
        let took = begun.elapsed();
        frames += 1;
        drawn += took;
        slowest = slowest.max(took);
        grids.push(grid(&window));
    }
    window.simulate_mouse_up(point(px(x), px(300.0)), MouseButton::Left);
    settle(&mut window);
    grids.push(grid(&window));
    grids.dedup();
    let moved = start - right_edge(&window);
    eprintln!(
        "[measure] right sidebar drag: {frames} frames, {:.2} ms a frame on average, {:.2} ms at most, the panes gave up {moved} points, grids {grids:?}",
        drawn.as_secs_f64() * 1000.0 / frames as f64,
        slowest.as_secs_f64() * 1000.0,
    );
    assert_eq!(moved, 60.0, "the panes follow the pointer");
    assert_eq!(
        grids.len(),
        2,
        "one resize of the shell, not one a frame: {grids:?}"
    );
}

// Plugin icons, drawn as the desktop's `PluginGlyph`.

/// Where the plugin icon `name` was last drawn: one its manifest names, or `own` for the plugin's own SVG.
fn glyph_at(name: &str) -> Option<gpui::Bounds<gpui::Pixels>> {
    crate::ui::spot_at(&format!("plugin-glyph:{name}"))
}

/// Cyclops names `clock` for its sidebar, which its button and the panel's title showed on the desktop.
#[test]
fn a_sidebar_shows_the_icon_its_manifest_names_on_its_button_and_over_its_panel() {
    let data = Data::temp();
    data.install_script(
        "timer",
        json!({ "contributes": { "sidebars": [{ "id": "side", "title": "Side", "page": "dist/side.html", "icon": "clock" }] } }),
        "",
        &[("side", SIDEBAR_PAGE)],
        "timer-scope",
    );
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    window.draw();
    // The button is the last thing in the project bar under the 40px title bar.
    let button = glyph_at("clock").expect("the button shows the sidebar's icon");
    assert!(
        button.center().x > px(950.0)
            && button.center().y > px(40.0)
            && button.center().y < px(76.0),
        "{button:?}"
    );
    window.simulate_click(point(px(978.0), px(58.0)), MouseButton::Left);
    app.advance_clock(crate::ui::motion::SLIDE * 2);
    app.run_until_parked();
    window.draw();
    // Then before the sidebar's title over the 360 panel, after its 4, 4 and 8 of padding.
    let title = glyph_at("clock").unwrap();
    assert_eq!(
        (title.left(), title.center().y),
        (px(1000.0 - 360.0 + 16.0), px(40.0 + 16.0))
    );
}

/// Claude accounts ships its own `icon.svg`, which its row in Settings, Plugins showed on the desktop.
#[test]
fn settings_shows_a_plugins_own_icon_on_its_row() {
    let data = Data::temp();
    data.install_script(
        "drawn",
        json!({ "icon": "icon.svg" }),
        "",
        &[],
        "drawn-scope",
    );
    let (_app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    window.draw();
    assert!(glyph_at("own").is_none());
    window.update(|w, window, cx| {
        w.open_settings(window, cx);
        w.settings.as_mut().unwrap().section = super::settings::Section::Plugins;
        cx.notify();
    });
    window.draw();
    // Half the row's 32 tile.
    let tile = glyph_at("own").expect("the row shows the plugin's icon");
    assert_eq!(tile.size, size(px(16.0), px(16.0)));
}

/// A settings page that counts its loads in the plugin's storage, and keeps whether it is focused there.
const SETTINGS_PAGE: &str = r#"
wings.storage.get('loads').then((n) => wings.storage.set('loads', (n ?? 0) + 1));
wings.onFocus(({ focused }) => wings.storage.set('focused', focused));
wings.render(JSON.stringify({ id: 0, type: '#root', props: {}, children: [{ id: 1, type: '#text', text: 'Tuner options' }] }));
"#;

/// "Plugin tuner", with `main` and a settings page.
pub(super) fn install_tuner(data: &Data, main: &str) {
    data.install_script(
        "tuner",
        json!({ "contributes": { "settings": { "page": "dist/settings.html" } } }),
        main,
        &[("settings", SETTINGS_PAGE)],
        "tuner-scope",
    );
}

/// Clicks the middle of the element last drawn as `name` (`ui::spot`).
pub(super) fn click_spot(window: &mut TestAppWindow<Workspace>, name: &str) {
    window.draw();
    let at = crate::ui::spot_at(name)
        .unwrap_or_else(|| panic!("{name} wasn't drawn"))
        .center();
    window.simulate_click(at, MouseButton::Left);
}

/// As on the desktop, a plugin's row under Plugin settings shows its page in Settings rather than in a window of its
/// own, and the page's frame runs only while that section shows.
#[test]
fn a_plugins_settings_page_shows_in_settings_and_stops_when_left() {
    let data = Data::temp();
    install_tuner(&data, "");
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    window.update(|_, window, _| window.activate_window());
    let frames = |w: &Workspace| w.plugins[0].host.frames();
    let loads = || data.saved("tuner", "tuner-scope", "loads");
    assert_eq!(window.read(|w, _| frames(w)), 1, "only its main runs");
    press(&mut window, ",");
    click_spot(&mut window, "settings-nav:Plugin tuner");
    until(&app, &mut window, "the page never showed, focused", |_| {
        data.saved("tuner", "tuner-scope", "focused") == Some(json!(true))
    });
    assert_eq!(app.windows().len(), 1, "no window of its own");
    assert!(window.read(|w, _| w.plugin_ui.windows.is_empty()));
    assert_eq!(
        window
            .read(|w, _| w.settings_plugin().map(str::to_owned))
            .as_deref(),
        Some("tuner")
    );
    assert_eq!(window.read(|w, _| frames(w)), 2);
    assert_eq!(loads(), Some(json!(1)));
    // Its row again keeps the page it shows.
    let page = |w: &Workspace| match &w.settings.as_ref().unwrap().section {
        super::settings::Section::Plugin(settings) => Some(settings.page.entity_id()),
        _ => None,
    };
    let shown = window.read(|w, _| page(w));
    click_spot(&mut window, "settings-nav:Plugin tuner");
    assert_eq!(window.read(|w, _| page(w)), shown);

    click_spot(&mut window, "settings-nav:Text");
    until(
        &app,
        &mut window,
        "the page's frame outlived its section",
        |w| frames(w) == 1,
    );
    // Shown again it loads again, and closing Settings stops it once Settings has gone out.
    click_spot(&mut window, "settings-nav:Plugin tuner");
    until(&app, &mut window, "the page didn't load again", |_| {
        loads() == Some(json!(2))
    });
    window.simulate_keystroke("escape");
    assert!(window.read(|w, _| w.settings.is_none()));
    app.advance_clock(crate::ui::motion::POPUP_IN * 2);
    until(
        &app,
        &mut window,
        "the page's frame outlived Settings",
        |w| frames(w) == 1,
    );
    assert_eq!(app.windows().len(), 1);
}

/// `wings.openSettings` opens Settings on the plugin's own section, as the desktop's does, rather than a window.
#[test]
fn open_settings_from_a_plugin_shows_its_section_in_settings() {
    let data = Data::temp();
    install_tuner(&data, "wings.onAction(() => wings.openSettings());");
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    window.update(|_, window, _| window.activate_window());
    let host = window.read(|w, _| w.plugins[0].host.clone());
    host.send_action("0", "settings", |_| {});
    until(&app, &mut window, "Settings never opened", |w| {
        w.settings.is_some()
    });
    assert_eq!(
        window
            .read(|w, _| w.settings_plugin().map(str::to_owned))
            .as_deref(),
        Some("tuner")
    );
    until(&app, &mut window, "the page never showed, focused", |_| {
        data.saved("tuner", "tuner-scope", "focused") == Some(json!(true))
    });
    assert_eq!(app.windows().len(), 1, "no window of its own");
}

/// Restarted while its page shows, as an update does, the plugin's settings page loads again from the new run, as the
/// desktop's remounts; stopped, Settings shows Plugins instead.
#[test]
fn a_shown_settings_page_loads_again_when_its_plugin_restarts() {
    let data = Data::temp();
    install_tuner(&data, "");
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    window.update(|w, window, cx| w.open_plugin_settings("tuner", window, cx));
    until(&app, &mut window, "the page never loaded", |_| {
        data.saved("tuner", "tuner-scope", "loads") == Some(json!(1))
    });
    window.update(|w, window, cx| w.restart_plugin("tuner", window, cx));
    until(&app, &mut window, "the page didn't load again", |_| {
        data.saved("tuner", "tuner-scope", "loads") == Some(json!(2))
    });
    assert_eq!(
        window
            .read(|w, _| w.settings_plugin().map(str::to_owned))
            .as_deref(),
        Some("tuner")
    );
    assert_eq!(window.read(|w, _| w.plugins[0].host.frames()), 2);
    // Turned off, Settings shows Plugins instead.
    window.update(|w, window, cx| {
        w.set_plugin_enabled("tuner", false, None, window, cx)
            .unwrap();
    });
    assert!(
        window
            .read(|w, _| w.settings.as_ref().unwrap().section == super::settings::Section::Plugins)
    );
}

/// As in the desktop, a plugin's pages open from the + menu and the project's sections, not from a list in the sidebar.
#[test]
fn the_sidebar_lists_no_plugin_pages() {
    let data = Data::temp();
    data.install_script(
        "reviews",
        json!({ "icon": "icon.svg", "contributes": { "views": [
            { "id": "pr", "title": "Pull request", "page": "dist/pr.html", "opens": ["tab"], "newTab": true, "icon": "git-compare" },
            { "id": "list", "title": "List", "page": "dist/list.html", "opens": ["tab"], "newTab": true },
        ] } }),
        "",
        &[("pr", SIDEBAR_PAGE), ("list", SIDEBAR_PAGE)],
        "reviews-scope",
    );
    let (_app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    window.update(|w, _, cx| {
        assert_eq!(w.plugins.len(), 1, "the plugin runs");
        assert!(w.plugin_list(cx).is_none());
    });
    window.draw();
    // Their icons show only on their buttons in the project bar, right of the 248 sidebar.
    for glyph in ["git-compare", "own"] {
        assert!(
            glyph_at(glyph).is_none_or(|at| at.left() >= px(248.0)),
            "{glyph} in the sidebar"
        );
    }
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
        json!({ "id": 1, "op": "statusline", "status": status, "account": "work" }),
    );
    let view = window.read(|w, _| crate::page::plugin::Env::statusline(&*w.plugin_env));
    assert_eq!(view["rateLimits"][0]["fiveHour"]["usedPercentage"], 40.0);
    assert_eq!(view["rateLimits"][0]["account"], "work");
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
