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
        Workspace,
        tests::{open_config, press},
    },
};

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
    assert!(
        halfway < docked - 30.0 && halfway > 30.0,
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
