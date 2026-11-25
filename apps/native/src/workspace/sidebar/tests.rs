//! Headless window tests of the sidebar: git counts, dragging its edge, pinning, adding and removing projects, its
//! choices coming back after a relaunch, and the peek of a closed sidebar.

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use gpui::{Modifiers, MouseButton, MouseMoveEvent, TestApp, TestAppWindow, point, px};
use wings_spaces::spaces::GitStatus;

use super::{INITIAL_WIDTH, MAX_WIDTH, MIN_WIDTH, State, order::Sort};
use crate::{
    projects::{test_dir, test_repo},
    saved,
    workspace::{
        Workspace, NotifyWorkspace,
        tests::{drag_to, moves, open_config, press, release},
    },
};

fn sidebar_agent(pane: usize, project: &Path, state: crate::agents::AgentState) -> crate::agents::Agent {
    crate::agents::Agent {
        pane_id: pane.to_string(), space_id: project.to_string_lossy().into_owned(),
        pid: 1, kind: crate::agents::AgentKind::Claude, session_id: None,
        name: Some("Fix checkout".into()), state, waiting_for: None,
        args: vec!["--model".into(), "opus".into()], launcher: None,
        started_at: super::now_ms() - 240_000, state_since: Some(super::now_ms() - 240_000),
        direct: true, summary: None, weak_name: false, claude_dir: None,
    }
}

#[test]
fn sidebar_unread_completion_is_bold_until_its_project_and_agent_are_opened() {
    use crate::agents::{AgentState, Scan};
    use super::AgentKey;
    let dir = test_dir("sidebar-unread-completion");
    let (_app, mut window, folders) = open(&dir, &["alpha", "beta"]);
    // Make a real pane in beta, then leave it while its agent works.
    let pane = window.update(|w, window, cx| {
        w.switch_project(folders[1].clone(), window, cx);
        let pane = w.focused_pane().unwrap();
        w.switch_project(folders[0].clone(), window, cx);
        pane
    });
    let apply = |window: &mut TestAppWindow<Workspace>, state| window.update(|w, _, cx| {
        w.apply(Scan { agents: vec![sidebar_agent(pane, &folders[1], state)], panes: Default::default() }, cx)
    });
    apply(&mut window, AgentState::Working);
    apply(&mut window, AgentState::Done);
    window.draw();
    assert!(crate::ui::spot_at("project-unread:beta").is_some(), "the completed project's name is not bold");
    assert!(crate::ui::spot_at(&format!("agent-unread:{pane}")).is_some(), "the completed agent's name is not bold");
    window.simulate_click(row("beta"), MouseButton::Left);
    window.read(|w, _| {
        assert!(!w.project_unread(&folders[1]));
        assert!(!w.agent_unread(AgentKey::Pane(pane), AgentState::Done));
    });
    // A new turn must be unread again even if the same session and pane are reused.
    window.update(|w, window, cx| w.switch_project(folders[0].clone(), window, cx));
    apply(&mut window, AgentState::Working);
    apply(&mut window, AgentState::Done);
    window.read(|w, _| {
        assert!(w.project_unread(&folders[1]));
        assert!(w.agent_unread(AgentKey::Pane(pane), AgentState::Done));
    });
    window.draw();
    window.simulate_click(crate::ui::spot_at(&format!("agent-row:{pane}")).unwrap().center(), MouseButton::Left);
    window.read(|w, _| {
        assert_eq!(w.project(), folders[1]);
        assert!(!w.project_unread(&folders[1]));
        assert!(!w.agent_unread(AgentKey::Pane(pane), AgentState::Done));
    });
}

#[test]
fn sidebar_current_project_and_worktrees_share_one_card() {
    let dir = test_dir("sidebar-project-card");
    let (_app, mut window, folders) = open(&dir, &["alpha", "alpha-fix", "beta"]);
    window.update(|w, _, cx| {
        w.projects[1].child = true;
        cx.notify_workspace();
    });
    window.draw();
    let card = crate::ui::spot_at("current-project-card").expect("the project has no shared card");
    assert!(card.contains(&row("alpha")) && card.contains(&row("alpha-fix")));
    assert!(!card.contains(&row("beta")));
    assert_eq!(crate::ui::spot_at("project:alpha").unwrap().origin.x,
        crate::ui::spot_at("project:beta").unwrap().origin.x, "the card shifted its rows sideways");
    assert_eq!(row("alpha-fix").y - row("alpha").y, row("beta").y - row("alpha-fix").y,
        "the card changed the spacing between rows");
    window.update(|w, window, cx| w.switch_project(folders[1].clone(), window, cx));
    window.draw();
    let card = crate::ui::spot_at("current-project-card").unwrap();
    assert!(card.contains(&row("alpha")) && card.contains(&row("alpha-fix")));
}

#[test]
fn sidebar_filter_menu_hides_idle_projects_and_worktrees_and_keeps_its_choices() {
    let dir = test_dir("sidebar-project-filters");
    let (app, mut window, folders) = open(&dir, &["alpha", "beta", "beta-fix", "gamma"]);
    window.update(|w, _, cx| w.refresh_git(None, true, cx));
    until(&app, &window, "git did not settle", |w| !w.side.git.checking);
    window.update(|w, _, cx| {
        w.projects[2].child = true;
        w.side.git.status.insert(folders[1].clone(), GitStatus { branch: None, changed: 1, ahead: 0, behind: 0 });
        cx.notify_workspace();
    });
    window.draw();
    let filter = crate::ui::spot_at("sidebar-filter").expect("the header still has Refresh instead of Filter").center();
    window.simulate_click(filter, MouseButton::Left);
    window.draw();
    let pick = |window: &mut TestAppWindow<Workspace>, label: &str| {
        window.simulate_click(crate::ui::spot_at(&format!("menu:{label}")).unwrap().center(), MouseButton::Left);
        window.draw();
    };
    pick(&mut window, "Show idle projects");
    assert_eq!(window.read(|w, _| w.project_order().shown), [0, 1, 2]);
    window.simulate_click(filter, MouseButton::Left);
    window.draw();
    pick(&mut window, "Show worktrees");
    assert_eq!(window.read(|w, _| w.project_order().shown), [0, 1]);
    // A working child keeps its idle parent visible, even when worktrees are hidden.
    window.update(|w, _, cx| {
        w.side.git.status.remove(&folders[1]);
        w.agents.insert(76, sidebar_agent(76, &folders[2], crate::agents::AgentState::Working));
        w.agents.insert(77, sidebar_agent(77, &folders[3], crate::agents::AgentState::Working));
        cx.notify_workspace();
    });
    assert_eq!(window.read(|w, _| w.project_order().shown), [0, 1, 3]);
    window.simulate_click(filter, MouseButton::Left);
    window.draw();
    window.simulate_keystroke("escape");
    assert!(window.read(|w, _| w.side.filter_menu.is_none()));
    let saved = saved::load_settings(&dir.join("workspace.json"));
    assert_eq!((saved.sidebar_show_idle, saved.sidebar_show_worktrees), (Some(false), Some(false)));
    drop((window, app));
    let (_app, window, _) = open(&dir, &["alpha", "beta", "beta-fix", "gamma"]);
    assert!(window.read(|w, _| !w.side.filters.idle && !w.side.filters.worktrees));
}

#[test]
fn sidebar_agent_age_steps_in_a_live_slot_without_redrawing_the_sidebar() {
    let dir = test_dir("sidebar-agent-age");
    let (app, mut window, folders) = open(&dir, &["alpha"]);
    window.update(|w, _, cx| w.refresh_git(None, true, cx));
    until(&app, &window, "git did not settle", |w| !w.side.git.checking);
    window.update(|w, _, cx| {
        w.agents.insert(77, sidebar_agent(77, &folders[0], crate::agents::AgentState::Working));
        cx.notify_workspace();
    });
    window.draw();
    app.run_until_parked();
    window.draw();
    assert!(crate::ui::live::placed("agent-age-77").is_some(), "the agent's age isn't in a live slot");
    let drawn = |window: &TestAppWindow<Workspace>| window.read(|w, cx| {
        let parts = w.parts.as_ref().unwrap();
        (parts.sidebar.read(cx).drawn, parts.main.read(cx).drawn, parts.title_bar.read(cx).drawn)
    });
    let before = drawn(&window);
    for _ in 0..3 {
        app.advance_clock(Duration::from_secs(1));
        app.run_until_parked();
        window.draw();
    }
    let after = drawn(&window);
    eprintln!("[measure] sidebar agent age, 3 clock steps: sidebar {}, main {}, title bar {}", after.0 - before.0, after.1 - before.1, after.2 - before.2);
    assert_eq!(after, before, "an age step redrew the window's pieces");
}

#[test]
fn sidebar_filter_menu_uses_keyboard_dismissal_and_trigger_toggle() {
    let dir = test_dir("sidebar-filter-keys");
    let (_app, mut window, _) = open(&dir, &["alpha"]);
    window.draw();
    let trigger = crate::ui::spot_at("sidebar-filter").unwrap().center();
    window.simulate_click(trigger, MouseButton::Left);
    window.draw();
    assert!(window.read(|w, _| w.side.filter_menu.is_some()));
    window.simulate_click(trigger, MouseButton::Left);
    assert!(window.read(|w, _| w.side.filter_menu.is_none()), "the trigger reopened its own menu");
    window.draw();
    window.simulate_click(trigger, MouseButton::Left);
    window.draw();
    window.simulate_keystroke("home");
    window.simulate_keystroke("enter");
    assert!(window.read(|w, _| !w.side.filters.idle && w.side.filter_menu.is_none()));
    window.draw();
    window.simulate_click(trigger, MouseButton::Left);
    window.draw();
    window.simulate_keystroke("escape");
    assert!(window.read(|w, _| w.side.filter_menu.is_none()));
    window.draw();
    window.simulate_click(trigger, MouseButton::Left);
    window.draw();
    window.simulate_mouse_down(point(px(500.0), px(400.0)), MouseButton::Left);
    assert!(window.read(|w, _| w.side.filter_menu.is_none()));
}

/// The project list of folders `names` in order, made unless it is there already, so a relaunch finds what was
/// changed. The first folder is the project shown.
fn list(dir: &Path, names: &[&str]) -> (PathBuf, Vec<PathBuf>) {
    let folders: Vec<PathBuf> = names.iter().map(|name| dir.join(name)).collect();
    for folder in &folders {
        std::fs::create_dir_all(folder).unwrap();
    }
    let spaces: Vec<_> = folders
        .iter()
        .map(|folder| serde_json::json!({ "id": folder, "name": folder.file_name().unwrap().to_string_lossy(), "path": folder }))
        .collect();
    let file = dir.join("spaces.json");
    if !file.exists() {
        std::fs::write(&file, serde_json::to_vec(&spaces).unwrap()).unwrap();
    }
    (file, folders)
}

fn open(dir: &Path, names: &[&str]) -> (TestApp, TestAppWindow<Workspace>, Vec<PathBuf>) {
    let (projects, folders) = list(dir, names);
    let state = dir.join("workspace.json");
    let first = folders[0].clone();
    let (app, window) = open_config(|config| {
        config.project = first;
        config.projects = Some(projects);
        config.state = Some(state);
    });
    (app, window, folders)
}

/// Runs the executors until `done` holds, or fails after ten seconds.
fn until(
    app: &TestApp,
    window: &TestAppWindow<Workspace>,
    what: &str,
    done: impl Fn(&Workspace) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !window.read(|w, _| done(w)) {
        assert!(Instant::now() < deadline, "{what}");
        app.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn running(pid: u32) -> bool {
    std::process::Command::new("ps")
        .args(["-p", &pid.to_string()])
        .output()
        .unwrap()
        .status
        .success()
}

fn git(dir: &Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .current_dir(dir)
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

#[test]
fn a_project_shows_its_changed_files_and_the_commits_it_is_ahead_by() {
    if wings_plugins::exec::find_program("git").is_none() {
        return;
    }
    let dir = test_dir("sidebar-counts");
    let repo = test_repo(&dir);
    let origin = dir.join("origin.git");
    std::fs::create_dir_all(&origin).unwrap();
    git(&origin, &["init", "--bare", "-b", "main"]);
    git(
        &repo,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(&repo, &["push", "-u", "origin", "main"]);
    git(&repo, &["commit", "--allow-empty", "-m", "second"]);
    std::fs::write(repo.join("a.txt"), "a").unwrap();
    std::fs::write(repo.join("b.txt"), "b").unwrap();

    // The list's one project is `dir/app`, which is the repo.
    let (app, mut window, _) = open(&dir, &["app"]);
    let counts = |w: &Workspace| w.side.git.status.get(&repo).cloned();
    until(&app, &window, "the project never got its counts", |w| {
        counts(w).is_some()
    });
    assert_eq!(
        window.read(|w, _| counts(w)),
        Some(GitStatus {
            branch: Some("main".into()),
            ahead: 1,
            behind: 0,
            changed: 2
        })
    );
    // The Refresh button looks again. The test app can finish the sweep inside `update`, so the
    // "checking" state in between isn't observable here.
    std::fs::write(repo.join("c.txt"), "c").unwrap();
    window.update(|w, _, cx| w.refresh_git(None, true, cx));
    until(&app, &window, "the refresh never finished", |w| {
        !w.side.git.checking
    });
    assert_eq!(
        window.read(|w, _| counts(w)).map(|status| status.changed),
        Some(3)
    );
    window.draw();
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn dragging_the_edge_resizes_the_sidebar_within_its_limits_and_saves_when_it_is_released() {
    let dir = test_dir("sidebar-resize");
    let file = dir.join("workspace.json");
    let width = |window: &TestAppWindow<Workspace>| window.read(|w, _| w.side.width);
    let saved = || saved::load_settings(&file).sidebar_width;
    let (_app, mut window, _) = open(&dir, &["alpha"]);
    window.draw();
    assert_eq!(
        (width(&window), saved()),
        (INITIAL_WIDTH, Some(INITIAL_WIDTH))
    );

    let drag = |window: &mut TestAppWindow<Workspace>, to: f32| {
        let from = width(window) - 4.0;
        window.simulate_mouse_down(point(px(from), px(300.0)), MouseButton::Left);
        window.draw();
        window.simulate_event(MouseMoveEvent {
            position: point(px(to), px(300.0)),
            pressed_button: Some(MouseButton::Left),
            modifiers: Modifiers::default(),
        });
    };
    drag(&mut window, 330.0);
    // It follows the pointer by how far it moved, and nothing is written until the pointer is released.
    assert_eq!(
        (width(&window), saved()),
        (
            INITIAL_WIDTH + 330.0 - (INITIAL_WIDTH - 4.0),
            Some(INITIAL_WIDTH)
        )
    );
    window.simulate_mouse_up(point(px(330.0), px(300.0)), MouseButton::Left);
    assert_eq!(saved(), Some(width(&window)));

    drag(&mut window, 2000.0);
    window.simulate_mouse_up(point(px(2000.0), px(300.0)), MouseButton::Left);
    assert_eq!((width(&window), saved()), (MAX_WIDTH, Some(MAX_WIDTH)));
    window.draw();
    drag(&mut window, 0.0);
    window.simulate_mouse_up(point(px(0.0), px(300.0)), MouseButton::Left);
    assert_eq!((width(&window), saved()), (MIN_WIDTH, Some(MIN_WIDTH)));

    // The next launch starts at the saved width, and a width outside the limits starts over.
    drop((window, _app));
    {
        let (_app, window, _) = open(&dir, &["alpha"]);
        assert_eq!(width(&window), MIN_WIDTH);
    }
    std::fs::write(
        &file,
        r#"{"version":1,"sidebar_width":9999.0,"projects":{}}"#,
    )
    .unwrap();
    let (_app, window, _) = open(&dir, &["alpha"]);
    assert_eq!(width(&window), INITIAL_WIDTH);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn pinning_a_project_moves_it_to_the_top_and_keeps_it_there_after_a_relaunch() {
    let dir = test_dir("sidebar-pin");
    let (_app, mut window, folders) = open(&dir, &["alpha", "beta"]);
    let names = |window: &TestAppWindow<Workspace>| {
        window.read(|w, _| {
            w.projects
                .iter()
                .map(|p| p.name.clone())
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(names(&window), ["alpha", "beta"]);
    window.draw();
    // Below the 40px title bar, the nav's four 28px rows 1px apart, 8px of padding and the 28px header: alpha's row, then
    // beta's, 24px and 2px apart.
    let beta = point(
        px(100.0),
        px(40.0 + 4.0 * 28.0 + 3.0 + 8.0 + 28.0 + 2.0 + 24.0 + 2.0 + 12.0),
    );
    window.simulate_mouse_move(beta);
    // One frame notices the pointer, the next draws the buttons.
    window.draw();
    window.draw();
    // Its buttons are at the row's right end: pin, then remove.
    window.simulate_click(point(px(198.0), beta.y), MouseButton::Left);
    assert_eq!(names(&window), ["beta", "alpha"]);
    assert_eq!(window.read(|w, _| w.project().to_path_buf()), folders[0]);

    let listed: Vec<wings_spaces::spaces::Space> =
        serde_json::from_slice(&std::fs::read(dir.join("spaces.json")).unwrap()).unwrap();
    assert_eq!(
        listed.iter().map(|space| space.pinned).collect::<Vec<_>>(),
        [false, true]
    );
    drop((window, _app));
    let (_app, window, _) = open(&dir, &["alpha", "beta"]);
    assert_eq!(names(&window), ["beta", "alpha"]);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn adding_a_project_shows_it_and_removing_one_asks_when_it_has_tabs_then_closes_them() {
    let dir = test_dir("sidebar-add-remove");
    let (app, mut window, folders) = open(&dir, &["alpha", "beta"]);
    let names = |window: &TestAppWindow<Workspace>| {
        window.read(|w, _| {
            w.projects
                .iter()
                .map(|p| p.name.clone())
                .collect::<Vec<_>>()
        })
    };
    let gamma = dir.join("gamma");
    std::fs::create_dir_all(&gamma).unwrap();
    window.update(|w, window, cx| w.add_project(&gamma, window, cx));
    assert_eq!(names(&window), ["alpha", "beta", "gamma"]);
    assert_eq!(window.read(|w, _| w.project().to_path_buf()), gamma);
    let on_disk = || std::fs::read_to_string(dir.join("spaces.json")).unwrap();
    assert!(on_disk().contains("gamma"));

    // Beta was never shown, so it has no tabs and goes without asking.
    window.update(|w, window, cx| w.request_remove(folders[1].clone(), window, cx));
    assert_eq!(names(&window), ["alpha", "gamma"]);
    assert!(!on_disk().contains("beta"));

    // Gamma is shown with a shell running, so it asks first, and No leaves everything as it was.
    let shell = window.read(|w, cx| w.terminal(1).and_then(|t| t.read(cx).process_id()).unwrap());
    assert!(running(shell));
    window.update(|w, window, cx| w.request_remove(gamma.clone(), window, cx));
    assert_eq!(names(&window), ["alpha", "gamma"]);
    window.update(|w, window, cx| w.cancel_remove(window, cx));
    assert_eq!(window.read(|w, _| w.project().to_path_buf()), gamma);

    // Yes shows the first other project, closes gamma's shell and takes it off the list.
    window.update(|w, window, cx| w.remove_project(gamma.clone(), window, cx));
    assert_eq!(names(&window), ["alpha"]);
    assert_eq!(window.read(|w, _| w.project().to_path_buf()), folders[0]);
    assert!(window.read(|w, _| w.parked.is_empty()));
    assert!(!on_disk().contains("gamma"));
    let deadline = Instant::now() + Duration::from_secs(10);
    while running(shell) {
        assert!(
            Instant::now() < deadline,
            "the removed project's shell is still running"
        );
        app.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
    // Removing the last one closes its tabs and shows the welcome, as the desktop does with no project.
    window.update(|w, window, cx| w.remove_project(folders[0].clone(), window, cx));
    assert!(names(&window).is_empty());
    window.read(|w, _| assert!(w.welcome && w.terminals.is_empty() && w.panes.is_empty()));
    window.update(|w, window, cx| w.new_terminal(window, cx));
    assert!(window.read(|w, _| w.terminals.is_empty()));
    // Adding a project from there opens it with a terminal.
    window.update(|w, window, cx| w.add_project(&gamma, window, cx));
    window.read(|w, _| {
        assert!(!w.welcome);
        assert_eq!(w.project(), gamma);
        assert_eq!(w.terminals.len(), 1);
    });
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_sort_the_older_group_and_the_times_come_back_after_a_relaunch() {
    let dir = test_dir("sidebar-keeps");
    let (app, mut window, folders) = open(&dir, &["alpha", "beta"]);
    window.update(|w, _, cx| {
        w.set_sort(Sort::Name, cx);
        w.toggle_older(cx);
        w.side
            .last_active
            .insert(folders[1].to_string_lossy().into_owned(), 7);
        w.side.last_active.insert("/gone".into(), 9);
        w.save(cx);
    });
    drop((window, app));
    let (_app, window, _) = open(&dir, &["alpha", "beta"]);
    window.read(|w, _| {
        assert_eq!((w.side.sort, w.side.older_open), (Sort::Name, true));
        assert_eq!(w.side.last_active[&*folders[1].to_string_lossy()], 7);
        assert!(!w.side.last_active.contains_key("/gone"));
    });
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn pointing_at_the_edge_of_a_closed_sidebar_floats_it_over_the_panes_until_the_pointer_leaves() {
    let dir = test_dir("sidebar-peek");
    let (app, mut window, _) = open(&dir, &["alpha"]);
    press(&mut window, "b");
    window.draw();
    let peek = |window: &TestAppWindow<Workspace>| {
        window.read(|w, _| (w.side.peek.open, w.side.peek.leaving))
    };
    let at = |window: &mut TestAppWindow<Workspace>, x: f32| {
        window.simulate_mouse_move(point(px(x), px(300.0)));
        window.draw();
    };
    let wait = |ms: u64| {
        app.advance_clock(Duration::from_millis(ms));
        app.run_until_parked();
    };
    at(&mut window, 3.0);
    wait(30);
    assert_eq!(peek(&window), (false, false), "it waits a moment first");
    wait(40);
    assert_eq!(peek(&window), (true, false));
    // On the floating sidebar it stays; off it, it goes after a moment, sliding out.
    window.draw();
    at(&mut window, 100.0);
    wait(100);
    assert_eq!(peek(&window), (true, false));
    at(&mut window, 600.0);
    wait(210);
    assert_eq!(peek(&window), (false, true));
    wait(260);
    assert_eq!(peek(&window), (false, false));
    // Docking it again drops a peek that is showing.
    at(&mut window, 3.0);
    wait(100);
    assert!(peek(&window).0);
    press(&mut window, "b");
    assert_eq!(peek(&window), (false, false));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_project_is_touched_at_most_once_a_minute() {
    let mut state = State::default();
    let project = Path::new("/code/app");
    let key = project.to_string_lossy().into_owned();
    let now = super::now_ms();
    state.last_active.insert(key.clone(), now - 10_000);
    state.touch(project);
    assert_eq!(state.last_active[&key], now - 10_000);
    state.last_active.insert(key.clone(), now - 120_000);
    state.touch(project);
    assert!(state.last_active[&key] >= now);
}

#[test]
fn a_worktree_is_titled_by_its_branch_when_its_folder_only_repeats_it() {
    use super::view::worktree_title;
    let same = worktree_title("wings-fix-login", Some("wings/fix-login".into()));
    assert_eq!(same, ("wings/fix-login".into(), None));
    let other = worktree_title("login", Some("wings/fix-login".into()));
    assert_eq!(other, ("login".into(), Some("wings/fix-login".into())));
    assert_eq!(worktree_title("login", None), ("login".into(), None));
}

/// Docking the sidebar shut and open again changes the size of the pane area on every frame of the slide, but the
/// shell is given its new size once, when the panes hold still, as the desktop refits after its slide.
#[test]
fn sliding_the_sidebar_resizes_the_terminal_once_when_it_settles() {
    let dir = test_dir("sidebar-slide");
    let (app, mut window, _) = open(&dir, &["alpha"]);
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
    window.draw();
    settle(&mut window);
    for closing in [true, false] {
        let before = grid(&window);
        press(&mut window, "b");
        let (mut grids, mut frames, mut drawn, mut slowest) =
            (vec![before], 0, Duration::ZERO, Duration::ZERO);
        // 16ms frames through the slide and past the terminal's wait for the size to hold.
        for _ in 0..30 {
            app.advance_clock(Duration::from_millis(16));
            app.run_until_parked();
            let start = Instant::now();
            window.draw();
            let took = start.elapsed();
            frames += 1;
            drawn += took;
            slowest = slowest.max(took);
            grids.push(grid(&window));
        }
        settle(&mut window);
        grids.push(grid(&window));
        grids.dedup();
        eprintln!(
            "[measure] sidebar {}: {frames} frames, {:.2} ms a frame on average, {:.2} ms at most, grids {grids:?}",
            if closing { "closing" } else { "opening" },
            drawn.as_secs_f64() * 1000.0 / frames as f64,
            slowest.as_secs_f64() * 1000.0,
        );
        assert_eq!(
            grids.len(),
            2,
            "one resize of the shell, not one a frame: {grids:?}"
        );
        assert_eq!(window.read(|w, _| w.sidebar), !closing);
    }
    std::fs::remove_dir_all(dir).unwrap();
}

/// Docking it shut, the sidebar slides away over the desktop's 320ms, its contents fading out, and the panes take the
/// room; docking it again slides it back to its full width.
#[test]
fn the_docked_sidebar_slides_shut_and_open_over_320ms() {
    let dir = test_dir("sidebar-slide-width");
    let (app, mut window, _) = open(&dir, &["alpha"]);
    let left = |window: &TestAppWindow<Workspace>| {
        window.read(|w, cx| {
            w.terminal(0)
                .unwrap()
                .read(cx)
                .drawn_at()
                .unwrap()
                .left()
                .as_f32()
        })
    };
    let sliding = |window: &TestAppWindow<Workspace>| window.read(|w, _| w.side.slide.running);
    let at = |window: &mut TestAppWindow<Workspace>, ms: u64| {
        app.advance_clock(Duration::from_millis(ms));
        app.run_until_parked();
        window.draw();
    };
    window.draw();
    let docked = left(&window);
    press(&mut window, "b");
    window.draw();
    assert!(sliding(&window));
    assert!(left(&window) > docked - 30.0, "it starts where it was");
    at(&mut window, 160);
    let halfway = left(&window);
    // The panes sit flush at the window's edge once shut, so partly over is a few points in from it.
    assert!(
        halfway < docked - 30.0 && halfway > 10.0,
        "halfway, the panes are partly over: {halfway}"
    );
    at(&mut window, 170);
    assert!(!sliding(&window));
    let shut = left(&window);
    assert!(shut < 30.0, "{shut}");
    press(&mut window, "b");
    window.draw();
    at(&mut window, 160);
    let opening = left(&window);
    assert!(opening > shut + 30.0 && opening < docked, "{opening}");
    at(&mut window, 170);
    assert!(!sliding(&window));
    assert_eq!(left(&window), docked);
    std::fs::remove_dir_all(dir).unwrap();
}

/// Where the first terminal's left edge is drawn: the docked sidebar's edge, through a slide.
fn terminal_left(window: &TestAppWindow<Workspace>) -> f32 {
    window.read(|w, cx| {
        w.terminal(0)
            .unwrap()
            .read(cx)
            .drawn_at()
            .unwrap()
            .left()
            .as_f32()
    })
}

/// Toggled again mid-slide, the sidebar turns around from where it is, as the desktop's CSS transition does, rather
/// than starting over from an edge, and gets back sooner than a whole slide.
#[test]
fn toggling_the_sidebar_mid_slide_turns_it_around_where_it_is() {
    let dir = test_dir("sidebar-slide-reverse");
    let (app, mut window, _) = open(&dir, &["alpha"]);
    let at = |window: &mut TestAppWindow<Workspace>, ms: u64| {
        app.advance_clock(Duration::from_millis(ms));
        app.run_until_parked();
        window.draw();
    };
    window.draw();
    let docked = terminal_left(&window);
    press(&mut window, "b");
    window.draw();
    at(&mut window, 60);
    let reached = terminal_left(&window);
    assert!(reached < docked - 30.0 && reached > 30.0, "{reached}");
    press(&mut window, "b");
    window.draw();
    let turned = terminal_left(&window);
    assert!(
        (turned - reached).abs() < 2.0,
        "it turns where it was, not at an edge: {turned} vs {reached}"
    );
    at(&mut window, 40);
    assert!(terminal_left(&window) > turned, "and heads back open");
    let (sliding, left) = window.read(|w, _| (w.side.slide.running, w.side.slide.motion));
    assert!(sliding);
    assert!(
        left.duration() < crate::ui::motion::SLIDE,
        "back sooner than a whole slide"
    );
    at(&mut window, 320);
    assert!(!window.read(|w, _| w.side.slide.running));
    assert_eq!(terminal_left(&window), docked);
    std::fs::remove_dir_all(dir).unwrap();
}

/// Under Reduce motion the sidebar takes or gives up its width at once, as the desktop's `[data-sliding]` rule turns
/// its transition off, but its contents still fade in over 200ms, as the desktop's `transition-opacity` does.
#[test]
fn under_reduce_motion_the_sidebar_doesnt_slide_but_still_fades() {
    let dir = test_dir("sidebar-slide-reduced");
    let (mut app, mut window, _) = open(&dir, &["alpha"]);
    app.update(|cx| cx.set_reduce_motion(true));
    let at = |window: &mut TestAppWindow<Workspace>, ms: u64| {
        app.advance_clock(Duration::from_millis(ms));
        app.run_until_parked();
        window.draw();
    };
    window.draw();
    let docked = terminal_left(&window);
    press(&mut window, "b");
    window.draw();
    assert!(terminal_left(&window) < 30.0, "shut at once");
    at(&mut window, 250);
    press(&mut window, "b");
    window.draw();
    assert_eq!(terminal_left(&window), docked, "open at once");
    let fading = window.read(|w, cx| {
        let slide = &w.side.slide;
        (
            slide.running,
            slide.motion.now(cx.background_executor().now()),
        )
    });
    assert!(fading.0, "its contents still fading in");
    assert_eq!(fading.1, (1.0, 0.0));
    at(&mut window, 100);
    let half = window.read(|w, cx| w.side.slide.motion.now(cx.background_executor().now()).1);
    assert!(half > 0.0 && half < 1.0, "{half}");
    at(&mut window, 110);
    assert!(!window.read(|w, _| w.side.slide.running));
    std::fs::remove_dir_all(dir).unwrap();
}

/// The middle of the row of project `name`, where it was last drawn.
fn row(name: &str) -> gpui::Point<gpui::Pixels> {
    crate::ui::spot_at(&format!("project:{name}"))
        .unwrap_or_else(|| panic!("{name}'s row wasn't drawn"))
        .center()
}

fn names(window: &TestAppWindow<Workspace>) -> Vec<String> {
    window.read(|w, _| w.projects.iter().map(|p| p.name.clone()).collect())
}

/// The desktop's `dragSpace`: in the Manual sort a project's row follows the pointer, the others make room as it passes
/// them, and the list is written once, when it is let go, without selecting the project it was let go on. Each frame
/// of the drag draws the sidebar alone.
#[test]
fn dragging_a_project_past_others_moves_it_there_and_saves_the_list_once_it_is_let_go() {
    let dir = test_dir("sidebar-drag");
    let (_app, mut window, folders) = open(&dir, &["alpha", "beta", "gamma"]);
    window.draw();
    let file = dir.join("spaces.json");
    let before = std::fs::read(&file).unwrap();
    let drawn = |window: &TestAppWindow<Workspace>| {
        window.read(|w, cx| {
            let parts = w.parts.as_ref().expect("the pieces");
            (
                parts.main.read(cx).drawn,
                parts.title_bar.read(cx).drawn,
                parts.sidebar.read(cx).drawn,
            )
        })
    };
    let (alpha, beta, gamma) = (row("alpha"), row("beta"), row("gamma"));
    // Pressing draws the window again, as any click does.
    window.simulate_mouse_down(gamma, MouseButton::Left);
    window.draw();
    let (main, title_bar, sidebar) = drawn(&window);

    // Gamma up past beta's middle, not yet alpha's: it takes beta's place and beta makes room under it.
    let mid = point(gamma.x, beta.y - px(4.0));
    // The rows are a little shorter at the default text size, so its first moves stay within the drag's start distance.
    let frames = moves(&mut window, gamma, mid, 16);
    assert!(
        (row("gamma").y - mid.y).abs() < px(0.5),
        "it follows the pointer: {:?} vs {mid:?}",
        row("gamma")
    );
    assert_eq!(row("beta").y, gamma.y, "beta moved down into gamma's place");
    assert_eq!(row("alpha").y, alpha.y);
    assert_eq!(
        std::fs::read(&file).unwrap(),
        before,
        "nothing is written while it moves"
    );
    let (main_after, title_bar_after, sidebar_after) = drawn(&window);
    assert_eq!(
        (main_after, title_bar_after),
        (main, title_bar),
        "the panes and title bar kept their drawing"
    );
    assert!(
        sidebar_after >= sidebar + 12,
        "the sidebar drew each move: {sidebar} to {sidebar_after}"
    );
    let total: Duration = frames.iter().sum();
    eprintln!(
        "[measure] project drag: {} frames, {:.2} ms a frame on average, {:.2} ms at most",
        frames.len(),
        total.as_secs_f64() * 1000.0 / frames.len() as f64,
        frames.iter().max().unwrap().as_secs_f64() * 1000.0,
    );

    // On up past alpha, and let go on alpha's old place: first in the list, written once, and alpha still shown.
    let top = point(gamma.x, alpha.y - px(4.0));
    moves(&mut window, mid, top, 6);
    assert_eq!(
        names(&window),
        ["alpha", "beta", "gamma"],
        "the list itself waits for the release"
    );
    release(&mut window, top);
    assert_eq!(names(&window), ["gamma", "alpha", "beta"]);
    assert_eq!(
        window.read(|w, _| w.project().to_path_buf()),
        folders[0],
        "the release selects nothing"
    );
    let listed: Vec<wings_spaces::spaces::Space> =
        serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|space| space.name.as_str())
            .collect::<Vec<_>>(),
        ["gamma", "alpha", "beta"]
    );
    assert_eq!(row("gamma").y, alpha.y, "it sits in its new place");

    // The order comes back after a relaunch.
    drop((window, _app));
    let (_app, window, _) = open(&dir, &["alpha", "beta", "gamma"]);
    assert_eq!(names(&window), ["gamma", "alpha", "beta"]);
    std::fs::remove_dir_all(dir).unwrap();
}

/// `trackDrag`'s threshold: a press that moves less than 5px is still a click and selects the project; one that moves
/// further is a drag, which selects nothing, and Escape puts a dragged project back with nothing written.
#[test]
fn a_press_that_hardly_moves_still_selects_and_escape_puts_a_dragged_project_back() {
    let dir = test_dir("sidebar-drag-click");
    let (_app, mut window, folders) = open(&dir, &["alpha", "beta", "gamma"]);
    window.draw();
    let file = dir.join("spaces.json");
    let before = std::fs::read(&file).unwrap();
    let shown = |window: &TestAppWindow<Workspace>| window.read(|w, _| w.project().to_path_buf());

    let beta = row("beta");
    let nudged = point(beta.x, beta.y + px(4.0));
    drag_to(&mut window, beta, nudged, 2);
    release(&mut window, nudged);
    assert_eq!(shown(&window), folders[1], "a click selects");

    let alpha = row("alpha");
    let moved = point(alpha.x, alpha.y + px(6.0));
    drag_to(&mut window, alpha, moved, 3);
    release(&mut window, moved);
    assert_eq!(shown(&window), folders[1], "a drag selects nothing");
    assert_eq!(names(&window), ["alpha", "beta", "gamma"]);

    // Gamma dragged to the top, then Escape: back where it was, and the release after it is no click either.
    let gamma = row("gamma");
    let top = point(gamma.x, alpha.y - px(4.0));
    drag_to(&mut window, gamma, top, 10);
    assert_eq!(row("alpha").y, beta.y, "alpha made room");
    window.simulate_keystroke("escape");
    window.draw();
    assert_eq!(row("gamma").y, gamma.y);
    assert_eq!(row("alpha").y, alpha.y);
    // The pointer is on alpha's row now, where it was let go.
    release(&mut window, top);
    assert_eq!(shown(&window), folders[1]);
    assert_eq!(names(&window), ["alpha", "beta", "gamma"]);
    assert_eq!(std::fs::read(&file).unwrap(), before, "nothing was written");
    std::fs::remove_dir_all(dir).unwrap();
}

/// The desktop's Move up and Move down in a project's menu: one place at a time, disabled at the ends, and among the
/// projects of its kind, since pinned ones always lead.
#[test]
fn move_up_and_move_down_in_a_projects_menu_move_it_one_place() {
    let dir = test_dir("sidebar-move-menu");
    let (_app, mut window, _) = open(&dir, &["alpha", "beta", "gamma"]);
    window.draw();
    let choose = |window: &mut TestAppWindow<Workspace>, name: &str, item: &str| {
        window.simulate_mouse_down(row(name), MouseButton::Right);
        window.draw();
        let at = crate::ui::spot_at(&format!("menu:{item}"))
            .unwrap_or_else(|| panic!("no {item} in {name}'s menu"))
            .center();
        window.simulate_click(at, MouseButton::Left);
        window.draw();
        window.update(|w, _, cx| w.close_menu(cx));
        window.draw();
    };
    choose(&mut window, "gamma", "Move up");
    assert_eq!(names(&window), ["alpha", "gamma", "beta"]);
    choose(&mut window, "alpha", "Move down");
    assert_eq!(names(&window), ["gamma", "alpha", "beta"]);
    // At the ends they are disabled.
    choose(&mut window, "gamma", "Move up");
    choose(&mut window, "beta", "Move down");
    assert_eq!(names(&window), ["gamma", "alpha", "beta"]);
    let listed: Vec<wings_spaces::spaces::Space> =
        serde_json::from_slice(&std::fs::read(dir.join("spaces.json")).unwrap()).unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|space| space.name.as_str())
            .collect::<Vec<_>>(),
        ["gamma", "alpha", "beta"]
    );
    // Beta pinned leads, and alpha, the first of the others, goes no higher.
    window.update(|w, _, cx| w.pin_project(&dir.join("beta"), true, cx));
    window.draw();
    assert_eq!(names(&window), ["beta", "gamma", "alpha"]);
    choose(&mut window, "gamma", "Move up");
    assert_eq!(names(&window), ["beta", "gamma", "alpha"]);
    choose(&mut window, "alpha", "Move up");
    assert_eq!(names(&window), ["beta", "alpha", "gamma"]);
    std::fs::remove_dir_all(dir).unwrap();
}

/// Only a listed project moves, and only in the Manual sort: not in another sort, not a worktree, not one under "Older",
/// as the desktop's rows with no `onPointerDown`. Their menus have no Move up or Move down either.
#[test]
fn only_a_listed_project_in_the_manual_sort_can_be_dragged() {
    let dir = test_dir("sidebar-drag-only");
    let folders: Vec<PathBuf> = ["alpha", "alpha-fix", "beta", "gamma", "old"]
        .iter()
        .map(|name| dir.join(name))
        .collect();
    for folder in &folders {
        std::fs::create_dir_all(folder).unwrap();
    }
    let space = |at: usize| serde_json::json!({ "id": folders[at], "name": folders[at].file_name().unwrap().to_string_lossy(), "path": folders[at] });
    let mut spaces: Vec<_> = (0..folders.len()).map(space).collect();
    spaces[1]["parent"] = serde_json::json!(folders[0]);
    std::fs::write(
        dir.join("spaces.json"),
        serde_json::to_vec(&spaces).unwrap(),
    )
    .unwrap();
    let (_app, mut window, _) = open(&dir, &["alpha", "alpha-fix", "beta", "gamma", "old"]);
    window.update(|w, _, cx| {
        w.side
            .last_active
            .insert(folders[4].to_string_lossy().into_owned(), 1);
        w.toggle_older(cx);
    });
    window.draw();
    let file = dir.join("spaces.json");
    let before = std::fs::read(&file).unwrap();
    let still = |window: &mut TestAppWindow<Workspace>, name: &str, to: &str| {
        let (from, to) = (row(name), row(to));
        let to = point(from.x, to.y - px(4.0));
        drag_to(window, from, to, 10);
        assert_eq!(row(name), from, "{name}'s row stays put");
        release(window, to);
        assert_eq!(std::fs::read(&file).unwrap(), before, "{name} didn't move");
    };
    // Their menus have no Move up or Move down.
    let steps = |window: &TestAppWindow<Workspace>, name: &str| {
        window.read(|w, _| w.project_steps(&dir.join(name)))
    };
    still(&mut window, "alpha-fix", "alpha");
    still(&mut window, "old", "alpha");
    assert_eq!(
        (steps(&window, "alpha-fix"), steps(&window, "old")),
        (None, None)
    );
    window.update(|w, _, cx| w.set_sort(Sort::Name, cx));
    window.draw();
    still(&mut window, "gamma", "alpha");
    assert_eq!(steps(&window, "gamma"), None);
    // In the Manual sort the same drag moves gamma to the top.
    window.update(|w, _, cx| w.set_sort(Sort::Manual, cx));
    window.draw();
    let (from, to) = (row("gamma"), row("alpha"));
    drag_to(&mut window, from, point(from.x, to.y - px(4.0)), 10);
    release(&mut window, point(from.x, to.y - px(4.0)));
    assert_eq!(
        names(&window),
        ["gamma", "alpha", "alpha-fix", "beta", "old"]
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn going_to_an_agent_scrolls_the_project_list_to_its_project() {
    let dir = test_dir("sidebar-reveal");
    let names: Vec<String> = (0..40).map(|i| format!("p{i:02}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let (_app, mut window, folders) = open(&dir, &names);
    let last = folders.last().unwrap().clone();
    // Whether the row of `last` is inside the list's visible part.
    let shows_last = |window: &TestAppWindow<Workspace>| {
        window.read(|w, _| {
            let shown = w.project_order().shown;
            let row = shown
                .iter()
                .position(|&index| w.projects[index].path == last)
                .unwrap();
            let (list, offset) = (w.side.scroll.bounds(), w.side.scroll.offset());
            let bounds = w.side.scroll.bounds_for_item(row).unwrap();
            bounds.top() + offset.y >= list.top() && bounds.bottom() + offset.y <= list.bottom()
        })
    };
    window.draw();
    assert!(!shows_last(&window), "40 projects overflow the list");

    window.update(|w, window, cx| w.go_to_pane(last.clone(), usize::MAX, window, cx));
    window.draw();
    assert!(shows_last(&window));
    std::fs::remove_dir_all(dir).unwrap();
}

/// The height of a project's row, which follows its name's size: the line, plus the row's padding.
fn row_height(name: &str) -> f32 {
    crate::ui::spot_at(&format!("project:{name}"))
        .unwrap_or_else(|| panic!("{name}'s row wasn't drawn"))
        .size
        .height
        .into()
}

/// The sidebar's text has a size of its own: smaller than it was by default, the Settings choice moves every row's
/// text with it, and what was chosen comes back after a relaunch.
#[test]
fn the_sidebar_text_size_is_its_own_setting_and_comes_back_after_a_relaunch() {
    let dir = test_dir("sidebar-text-size");
    let height_after = |window: &mut TestAppWindow<Workspace>, size: f32| {
        window.update(|w, _, cx| w.set_sidebar_text(size, cx));
        window.draw();
        row_height("beta")
    };
    let small = {
        let (_app, mut window, _) = open(&dir, &["alpha", "beta"]);
        window.draw();
        assert_eq!(window.read(|_, cx| super::text_size(cx)), 12.0, "not one step below the old 13");
        let default = row_height("beta");
        let small = height_after(&mut window, 11.0);
        let large = height_after(&mut window, 13.0);
        assert!(small < default && default < large, "{small} {default} {large}");
        // The terminals' font size is a different setting, and stays where it was.
        assert_eq!(window.read(|_, cx| crate::terminal_view::font_size(cx)), 13.0);
        height_after(&mut window, 11.0)
    };
    let (_app, mut window, _) = open(&dir, &["alpha", "beta"]);
    window.draw();
    assert_eq!(window.read(|_, cx| super::text_size(cx)), 11.0);
    assert_eq!(row_height("beta"), small);
    std::fs::remove_dir_all(dir).unwrap();
}

/// An agent's row in the Agents list is drawn outside the sidebar's own files, and its text follows the setting too: the
/// name, the project and state line and the summary, with the row's height.
#[test]
fn an_agents_row_follows_the_sidebar_text_size() {
    let dir = test_dir("sidebar-text-agent-row");
    let (_app, mut window, folders) = open(&dir, &["alpha"]);
    window.update(|w, _, cx| {
        let mut agent = sidebar_agent(77, &folders[0], crate::agents::AgentState::Working);
        agent.summary = Some("Reading the tests".into());
        w.agents.insert(77, agent);
        cx.notify_workspace();
    });
    let height_after = |window: &mut TestAppWindow<Workspace>, size: f32| {
        window.update(|w, _, cx| w.set_sidebar_text(size, cx));
        window.draw();
        let height: f32 = crate::ui::spot_at("agent-row:77").expect("the agent's row wasn't drawn").size.height.into();
        height
    };
    let (small, default, large) = (height_after(&mut window, 11.0), height_after(&mut window, 12.0), height_after(&mut window, 13.0));
    assert!(small < default && default < large, "{small} {default} {large}");
    std::fs::remove_dir_all(dir).unwrap();
}

/// A file saved before the setting existed has no size for it: the sidebar takes the new default, not the old size.
#[test]
fn a_file_saved_before_the_sidebar_text_setting_gets_the_default_size() {
    let dir = test_dir("sidebar-text-old-file");
    let (_, folders) = list(&dir, &["alpha"]);
    let file = dir.join("workspace.json");
    let settings = saved::Settings { font_size: Some(14.0), ..Default::default() };
    saved::save(&file, &folders[0].to_string_lossy(), saved::SavedProject::default(), settings).unwrap();
    assert_eq!(saved::load_settings(&file).sidebar_text, None);
    let (_app, window, _) = open(&dir, &["alpha"]);
    assert_eq!(window.read(|_, cx| super::text_size(cx)), 12.0);
    assert_eq!(window.read(|_, cx| crate::terminal_view::font_size(cx)), 14.0);
    std::fs::remove_dir_all(dir).unwrap();
}
