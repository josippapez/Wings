//! Headless window tests for the terminals an agent opens with `open_pane`: where Settings says they open, reusing an
//! idle one, the desktop's buttons that hide, move, float and dock them, and a floating one's drag. That the choice in
//! Settings is saved is in `desktop_parity_tests`.

use std::time::{Duration, Instant};

use gpui::{Modifiers, MouseButton, MouseMoveEvent, TestApp, TestAppWindow, point, px, size};
use wings_connect::panes::{OpenPlan, Opened};

use super::{agent_terminals::Opens, plugin_tests::click_spot, tests::open_sized, *};
use crate::{agents::PaneInfo, layout::Layout};

fn open() -> (TestApp, TestAppWindow<Workspace>) {
    open_sized(Some(size(px(1280.0), px(900.0))), |_| {})
}

/// Claude in pane `caller` asks for a terminal for `command`.
fn open_from(window: &mut TestAppWindow<Workspace>, caller: usize, command: &str) -> Opened {
    let plan = OpenPlan {
        space_id: std::env::temp_dir().to_string_lossy().into_owned(),
        cwd: None,
        input: Some(command.into()),
        run: true,
        next_to: Some(caller.to_string()),
    };
    window.update(|w, window, cx| w.claude_open_pane(plan, window, cx).unwrap())
}

fn layout(window: &TestAppWindow<Workspace>) -> Layout {
    window.read(|w, _| w.terminals[0].layout.clone())
}

fn split(dir: Dir, ratio: f32, a: Layout, b: Layout) -> Layout {
    Layout::Split {
        dir,
        ratio,
        a: Box::new(a),
        b: Box::new(b),
    }
}

/// The panes drawn as cards, those that paint, and whether the agent in pane 0 has its terminals floating.
fn shown(window: &TestAppWindow<Workspace>) -> (Vec<usize>, Vec<usize>, bool) {
    window.read(|w, cx| {
        let painting = w.terminals[0]
            .layout
            .panes()
            .into_iter()
            .filter(|pane| w.terminal(*pane).unwrap().read(cx).is_visible())
            .collect();
        (
            w.card_panes(&w.terminals[0]),
            painting,
            w.agent_terminals.floats(0),
        )
    })
}

/// Lets the shell take its size and the screen redraw, both of which wait on the clock.
fn settle(app: &TestApp, window: &mut TestAppWindow<Workspace>) {
    for _ in 0..15 {
        app.advance_clock(Duration::from_millis(100));
        app.run_until_parked();
        window.draw();
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn an_agents_first_terminal_opens_where_settings_say_and_its_next_ones_beside_it() {
    use Layout::Pane;
    for (opens, dir, floats) in [
        (Opens::Below, Dir::Column, false),
        (Opens::Right, Dir::Row, false),
        (Opens::Float, Dir::Column, true),
    ] {
        let (_app, mut window) = open();
        window.update(|w, _, cx| w.set_prefs(|prefs| prefs.agent_terminals_open = opens, cx));
        let first = open_from(&mut window, 0, "echo first");
        assert_eq!((first.pane_id.as_str(), first.reused), ("1", false));
        // The agent keeps two thirds of its place, and the keys.
        assert_eq!(
            layout(&window),
            split(dir, 0.65, Pane(0), Pane(1)),
            "{opens:?}"
        );
        assert_eq!(window.read(|w, _| w.focused_pane()), Some(0), "{opens:?}");
        // The next goes beside the last, so the agent's pane doesn't shrink again.
        open_from(&mut window, 0, "echo second");
        assert_eq!(
            layout(&window),
            split(dir, 0.65, Pane(0), split(Dir::Row, 0.5, Pane(1), Pane(2))),
            "{opens:?}"
        );
        let cards = if floats { vec![0] } else { vec![0, 1, 2] };
        let (shown_cards, _, floating) = shown(&window);
        assert_eq!((shown_cards, floating), (cards, floats), "{opens:?}");
    }
}

/// An agent's terminal opens out of sight: you stay on the project, tab and pane you were on, whether the agent is in
/// another tab or another project, and so does a tab it opens.
#[test]
fn an_agents_terminal_opens_without_taking_you_from_what_you_look_at() {
    let dir = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("wings-quiet-open-{}", std::process::id()));
    let (alpha, beta) = (dir.join("alpha"), dir.join("beta"));
    std::fs::create_dir_all(&alpha).unwrap();
    std::fs::create_dir_all(&beta).unwrap();
    let spaces = serde_json::json!([
        { "id": "a", "name": "alpha", "path": alpha },
        { "id": "b", "name": "beta", "path": beta },
    ]);
    std::fs::write(dir.join("spaces.json"), spaces.to_string()).unwrap();
    let (_app, mut window) = open_sized(Some(size(px(1280.0), px(900.0))), |config| {
        config.project = alpha.clone();
        config.projects = Some(dir.join("spaces.json"));
    });
    let open_in = |window: &mut TestAppWindow<Workspace>, next_to: Option<usize>| {
        let plan = OpenPlan {
            space_id: alpha.to_string_lossy().into_owned(),
            cwd: None,
            input: Some("echo hi".into()),
            run: false,
            next_to: next_to.map(|pane| pane.to_string()),
        };
        let opened = window.update(|w, window, cx| w.claude_open_pane(plan, window, cx).unwrap());
        opened.pane_id.parse::<usize>().unwrap()
    };
    // What's on screen, and the pane with the keys.
    let on_screen = |window: &mut TestAppWindow<Workspace>| {
        window.update(|w, window, cx| {
            let keys = w.panes.iter().find(|(_, pane)| {
                pane.view
                    .as_ref()
                    .is_ok_and(|view| view.focus_handle(cx).is_focused(window))
            });
            (w.config.project.clone(), w.active, keys.map(|(id, _)| *id))
        })
    };

    // The agent in alpha's first tab, you in its second.
    let agent = window.read(|w, _| w.terminals[0].focused);
    super::tests::press(&mut window, "t");
    let before = on_screen(&mut window);
    assert_ne!(
        before.1,
        Active::Terminal(window.read(|w, _| w.terminals[0].id))
    );
    let split = open_in(&mut window, Some(agent));
    assert_eq!(on_screen(&mut window), before);
    assert!(window.read(|w, _| w.terminals[0].layout.panes().contains(&split)));

    // You in beta, alpha out of sight.
    window.update(|w, window, cx| w.switch_project(beta.clone(), window, cx));
    let before = on_screen(&mut window);
    let split = open_in(&mut window, Some(agent));
    let tab = open_in(&mut window, None);
    assert_eq!(on_screen(&mut window), before);
    window.read(|w, _| {
        let alpha = &w.parked[&alpha];
        assert!(alpha.terminals[0].layout.panes().contains(&split));
        assert!(
            alpha
                .terminals
                .iter()
                .any(|t| t.layout.panes() == vec![tab])
        );
        assert!(!w.terminals.iter().any(|t| t.layout.panes().contains(&tab)));
    });
    let _ = std::fs::remove_dir_all(dir);
}

/// An agent opens a terminal in a project nobody entered yet: the project gets its tabs and the terminal, and you stay on
/// the project, tab and pane you were on.
#[test]
fn an_agents_terminal_in_a_project_not_entered_yet_leaves_you_where_you_are() {
    let dir = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("wings-quiet-enter-{}", std::process::id()));
    let (alpha, gamma) = (dir.join("alpha"), dir.join("gamma"));
    std::fs::create_dir_all(&alpha).unwrap();
    std::fs::create_dir_all(&gamma).unwrap();
    let spaces = serde_json::json!([
        { "id": "a", "name": "alpha", "path": alpha },
        { "id": "g", "name": "gamma", "path": gamma },
    ]);
    std::fs::write(dir.join("spaces.json"), spaces.to_string()).unwrap();
    let (_app, mut window) = open_sized(Some(size(px(1280.0), px(900.0))), |config| {
        config.project = alpha.clone();
        config.projects = Some(dir.join("spaces.json"));
    });
    let agent = window.read(|w, _| w.terminals[0].focused);
    window.update(|w, window, cx| w.focus_pane(agent, window, cx));
    let on_screen = |window: &mut TestAppWindow<Workspace>| {
        window.update(|w, window, cx| {
            let keys = w.panes.iter().find(|(_, pane)| {
                pane.view.as_ref().is_ok_and(|view| view.focus_handle(cx).is_focused(window))
            });
            (w.config.project.clone(), w.active, keys.map(|(id, _)| *id))
        })
    };
    let before = on_screen(&mut window);
    assert_eq!(before.2, Some(agent));
    let pane = window.update(|w, window, cx| {
        let plan = OpenPlan {
            space_id: gamma.to_string_lossy().into_owned(),
            cwd: None,
            input: None,
            run: false,
            next_to: None,
        };
        w.claude_open_pane(plan, window, cx).unwrap().pane_id.parse::<usize>().unwrap()
    });
    window.draw();
    assert_eq!(on_screen(&mut window), before);
    window.read(|w, _| {
        let gamma = &w.parked[&gamma];
        assert!(gamma.terminals.iter().any(|tab| tab.layout.panes() == vec![pane]));
    });
    let _ = std::fs::remove_dir_all(dir);
}

/// You look at the agent's own tab with the keys in its pane, and it opens a split beside it: the keys stay in the agent.
#[test]
fn an_agents_split_beside_the_pane_you_type_in_leaves_the_keys_there() {
    let (_app, mut window) = open_sized(Some(size(px(1280.0), px(900.0))), |_| {});
    let agent = window.read(|w, _| w.terminals[0].focused);
    window.update(|w, window, cx| w.focus_pane(agent, window, cx));
    let keys = |window: &mut TestAppWindow<Workspace>| {
        window.update(|w, window, cx| {
            let held = w.panes.iter().find(|(_, pane)| {
                pane.view.as_ref().is_ok_and(|view| view.focus_handle(cx).is_focused(window))
            });
            (w.active, held.map(|(id, _)| *id), w.terminals[0].focused)
        })
    };
    let before = keys(&mut window);
    assert_eq!(before.1, Some(agent));
    let split = window.update(|w, window, cx| {
        let plan = OpenPlan {
            space_id: w.config.project.to_string_lossy().into_owned(),
            cwd: None,
            input: Some("echo hi".into()),
            run: false,
            next_to: Some(agent.to_string()),
        };
        w.claude_open_pane(plan, window, cx).unwrap().pane_id.parse::<usize>().unwrap()
    });
    window.draw();
    assert!(window.read(|w, _| w.terminals[0].layout.panes().contains(&split)));
    assert_eq!(keys(&mut window), before);
}

/// Claude's `close_pane` closes a terminal in a project out of sight there, and you stay where you were. When it was that
/// project's front tab, the tab after it is in front when you come back.
#[test]
fn an_agents_terminal_in_a_project_out_of_sight_closes_there() {
    let dir = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("wings-quiet-close-{}", std::process::id()));
    let (alpha, beta) = (dir.join("alpha"), dir.join("beta"));
    std::fs::create_dir_all(&alpha).unwrap();
    std::fs::create_dir_all(&beta).unwrap();
    let spaces = serde_json::json!([
        { "id": "a", "name": "alpha", "path": alpha },
        { "id": "b", "name": "beta", "path": beta },
    ]);
    std::fs::write(dir.join("spaces.json"), spaces.to_string()).unwrap();
    let (_app, mut window) = open_sized(Some(size(px(1280.0), px(900.0))), |config| {
        config.project = alpha.clone();
        config.projects = Some(dir.join("spaces.json"));
    });
    let open_in = |window: &mut TestAppWindow<Workspace>, next_to: Option<usize>| {
        let plan = OpenPlan {
            space_id: alpha.to_string_lossy().into_owned(),
            cwd: None,
            input: Some("echo hi".into()),
            run: false,
            next_to: next_to.map(|pane| pane.to_string()),
        };
        let opened = window.update(|w, window, cx| w.claude_open_pane(plan, window, cx).unwrap());
        opened.pane_id.parse::<usize>().unwrap()
    };
    let on_screen = |window: &mut TestAppWindow<Workspace>| {
        window.update(|w, window, cx| {
            let keys = w.panes.iter().find(|(_, pane)| {
                pane.view
                    .as_ref()
                    .is_ok_and(|view| view.focus_handle(cx).is_focused(window))
            });
            (w.config.project.clone(), w.active, keys.map(|(id, _)| *id))
        })
    };
    let tab_of = |window: &TestAppWindow<Workspace>, pane: usize| {
        window.read(|w, _| w.terminals.iter().find(|t| t.layout.panes() == vec![pane]).unwrap().id)
    };

    // In alpha: a split beside the agent and two tabs, the first of them in front.
    let agent = window.read(|w, _| w.terminals[0].focused);
    let split = open_in(&mut window, Some(agent));
    let (first, second) = (open_in(&mut window, None), open_in(&mut window, None));
    let (first_tab, second_tab) = (tab_of(&window, first), tab_of(&window, second));
    window.update(|w, window, cx| w.show(Active::Terminal(first_tab), window, cx));

    // You in beta.
    window.update(|w, window, cx| w.switch_project(beta.clone(), window, cx));
    let before = on_screen(&mut window);
    for pane in [split, first] {
        window
            .update(|w, window, cx| w.close_named_pane(&pane.to_string(), window, cx))
            .unwrap();
    }
    assert_eq!(on_screen(&mut window), before);
    window.read(|w, _| {
        assert!(!w.panes.contains_key(&split) && !w.panes.contains_key(&first));
        let alpha = &w.parked[&alpha];
        assert_eq!(alpha.terminals[0].layout.panes(), vec![agent]);
        assert!(!alpha.terminals.iter().any(|t| t.id == first_tab));
        assert_eq!(alpha.active, Active::Terminal(second_tab));
    });
    window.update(|w, window, cx| w.switch_project(alpha.clone(), window, cx));
    assert_eq!(window.read(|w, _| w.active), Active::Terminal(second_tab));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_command_goes_to_a_terminal_the_agent_opened_that_sits_idle() {
    let (app, mut window) = open();
    window.update(|w, _, cx| w.set_prefs(|prefs| prefs.agents_run_commands = true, cx));
    open_from(&mut window, 0, "true");
    // Its shell at its prompt in another folder, and nothing else opened by the agent.
    let idle = |busy| PaneInfo {
        command: "zsh".into(),
        cwd: Some("/".into()),
        busy,
    };
    window.update(|w, _, _| {
        w.pane_info.insert(1, idle(false));
    });
    let again = open_from(&mut window, 0, "echo reused-$((3+4))");
    assert_eq!((again.pane_id.as_str(), again.reused), ("1", true));
    assert_eq!(layout(&window).panes(), vec![0, 1]);
    let output = |window: &TestAppWindow<Workspace>| {
        window.read(|w, cx| {
            String::from_utf8_lossy(&w.terminal(1).unwrap().read(cx).recent_output()).into_owned()
        })
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    while !output(&window).contains("reused-7") {
        assert!(
            Instant::now() < deadline,
            "it never ran: {}",
            output(&window)
        );
        app.advance_clock(Duration::from_millis(200));
        app.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        output(&window).contains("cd "),
        "it didn't go to the project's folder first: {}",
        output(&window)
    );
    // Running something, it's left alone and the command gets a terminal of its own.
    window.update(|w, _, _| {
        w.pane_info.insert(1, idle(true));
    });
    let third = open_from(&mut window, 0, "echo third");
    assert_eq!((third.pane_id.as_str(), third.reused), ("2", false));
}

#[test]
fn hide_move_float_and_dock_change_the_layout_as_the_desktops_buttons_do() {
    use Layout::Pane;
    let (app, mut window) = open();
    open_from(&mut window, 0, "echo hi");
    settle(&app, &mut window);
    assert_eq!(
        window.read(|w, _| w.focused_pane()),
        Some(0),
        "the keys left the agent"
    );
    assert_eq!(shown(&window), (vec![0, 1], vec![0, 1], false));

    // Move to the right, then below again.
    click_spot(&mut window, "agent-move:1");
    assert_eq!(layout(&window), split(Dir::Row, 0.65, Pane(0), Pane(1)));
    click_spot(&mut window, "agent-move:1");
    assert_eq!(layout(&window), split(Dir::Column, 0.65, Pane(0), Pane(1)));

    // Hide: the agent's pane fills the tab, the terminal keeps its shell out of sight, and the agent's button shows it.
    let shell = window.read(|w, cx| w.terminal(1).unwrap().read(cx).process_id());
    click_spot(&mut window, "agent-hide:1");
    assert_eq!(window.read(|w, _| w.terminals[0].zoomed), Some(0));
    assert_eq!(shown(&window), (vec![0], vec![0], false));
    click_spot(&mut window, "agent-terminals:0");
    assert_eq!(window.read(|w, _| w.terminals[0].zoomed), None);
    assert_eq!(shown(&window), (vec![0, 1], vec![0, 1], false));
    assert_eq!(
        window.read(|w, cx| w.terminal(1).unwrap().read(cx).process_id()),
        shell
    );

    // Float: out of the split but still in the tree, painted in a panel over the agent's terminal.
    click_spot(&mut window, "agent-float:1");
    assert_eq!(layout(&window), split(Dir::Column, 0.65, Pane(0), Pane(1)));
    assert_eq!(shown(&window), (vec![0], vec![0, 1], true));
    settle(&app, &mut window);
    let (agent, floated) = window.read(|w, cx| {
        let at = |pane| w.terminal(pane).unwrap().read(cx).drawn_at().unwrap();
        (at(0), at(1))
    });
    assert!(
        agent.contains(&floated.origin) && agent.contains(&floated.bottom_right()),
        "the panel isn't over the agent: {floated:?} in {agent:?}"
    );
    let panel = crate::ui::spot_at("agent-float-0").unwrap();
    // The desktop's 520 x 300, inside its 1px border.
    assert_eq!(
        (panel.size.width, panel.size.height),
        (px(518.0), px(298.0))
    );

    // Minimised to a bar it stops painting; docked, it's back in the split.
    click_spot(&mut window, "float-minimise:0");
    assert_eq!(shown(&window), (vec![0], vec![0], true));
    click_spot(&mut window, "float-dock:0");
    assert_eq!(shown(&window), (vec![0, 1], vec![0, 1], false));
}

/// Dragging the floating panel's corner changes its size on every frame, but its shell is given the new size once,
/// when the panel holds still, as the desktop refits once the gesture ends. Moving it never resizes the shell.
#[test]
fn dragging_a_floating_agent_terminal_resizes_its_shell_once_when_it_settles() {
    let (app, mut window) = open();
    window.update(|w, _, cx| w.set_prefs(|prefs| prefs.agent_terminals_open = Opens::Float, cx));
    open_from(&mut window, 0, "echo hi");
    settle(&app, &mut window);
    let grid = |window: &TestAppWindow<Workspace>| {
        window.read(|w, cx| w.terminal(1).unwrap().read(cx).grid())
    };
    let panel = || crate::ui::spot_at("agent-float-0").unwrap();
    let drag = |window: &mut TestAppWindow<Workspace>, from: gpui::Point<gpui::Pixels>| {
        let (before, start) = (grid(window), panel());
        window.simulate_mouse_down(from, MouseButton::Left);
        let (mut grids, mut frames, mut drawn, mut slowest) =
            (vec![before], 0, Duration::ZERO, Duration::ZERO);
        let mut at = from;
        // 30 frames of 16ms, the pointer 3 points left and 2 up each.
        for _ in 0..30 {
            at = point(at.x - px(3.0), at.y - px(2.0));
            window.simulate_event(MouseMoveEvent {
                position: at,
                pressed_button: Some(MouseButton::Left),
                modifiers: Modifiers::default(),
            });
            app.advance_clock(Duration::from_millis(16));
            app.run_until_parked();
            let begun = Instant::now();
            window.draw();
            let took = begun.elapsed();
            frames += 1;
            drawn += took;
            slowest = slowest.max(took);
            grids.push(grid(window));
        }
        window.simulate_mouse_up(at, MouseButton::Left);
        settle(&app, window);
        grids.push(grid(window));
        grids.dedup();
        eprintln!(
            "[measure] floating agent terminal drag: {frames} frames, {:.2} ms a frame on average, {:.2} ms at most, grids {grids:?}",
            drawn.as_secs_f64() * 1000.0 / frames as f64,
            slowest.as_secs_f64() * 1000.0,
        );
        (start, panel(), grids)
    };
    // The corner is the panel's top-left 16 points.
    let corner = panel().origin + point(px(6.0), px(6.0));
    let (start, end, grids) = drag(&mut window, corner);
    assert_eq!(
        (
            end.size.width - start.size.width,
            end.size.height - start.size.height
        ),
        (px(90.0), px(60.0)),
        "the panel follows the pointer"
    );
    assert_eq!(
        grids.len(),
        2,
        "one resize of the shell, not one a frame: {grids:?}"
    );
    // The header, right of its title.
    let header = point(end.center().x, end.origin.y + px(16.0));
    let (start, end, grids) = drag(&mut window, header);
    assert_eq!(end.origin - start.origin, point(px(-90.0), px(-60.0)));
    assert_eq!(end.size, start.size);
    assert_eq!(grids.len(), 1, "moving it resized the shell: {grids:?}");
}

/// Each edge and corner of a floating panel resizes it, the edges across from it staying where they were.
#[test]
fn every_edge_and_corner_of_a_floating_panel_resizes_it() {
    let (app, mut window) = open();
    window.update(|w, _, cx| w.set_prefs(|prefs| prefs.agent_terminals_open = Opens::Float, cx));
    open_from(&mut window, 0, "echo hi");
    settle(&app, &mut window);
    let panel = || crate::ui::spot_at("agent-float-0").unwrap();
    // Each grip dragged 30 points left and 20 up: how far its left, top, right and bottom edges go.
    for (grip, edges) in [
        ("left", (-30.0, 0.0, 0.0, 0.0)),
        ("right", (0.0, 0.0, -30.0, 0.0)),
        ("top", (0.0, -20.0, 0.0, 0.0)),
        ("bottom", (0.0, 0.0, 0.0, -20.0)),
        ("top-left", (-30.0, -20.0, 0.0, 0.0)),
        ("top-right", (0.0, -20.0, -30.0, 0.0)),
        ("bottom-left", (-30.0, 0.0, 0.0, -20.0)),
        ("bottom-right", (0.0, 0.0, -30.0, -20.0)),
    ] {
        window.draw();
        let start = panel();
        let from = crate::ui::spot_at(&format!("agent-float-0-resize-{grip}"))
            .unwrap_or_else(|| panic!("no {grip} grip"))
            .center();
        let to = point(from.x - px(30.0), from.y - px(20.0));
        window.simulate_mouse_down(from, MouseButton::Left);
        window.simulate_event(MouseMoveEvent {
            position: to,
            pressed_button: Some(MouseButton::Left),
            modifiers: Modifiers::default(),
        });
        window.simulate_mouse_up(to, MouseButton::Left);
        window.draw();
        let end = panel();
        assert_eq!(
            (
                (end.left() - start.left()).as_f32(),
                (end.top() - start.top()).as_f32(),
                (end.right() - start.right()).as_f32(),
                (end.bottom() - start.bottom()).as_f32(),
            ),
            edges,
            "{grip}"
        );
    }
}

/// With several terminals in the panel, the menu in its header picks the one it shows, and a second press on its
/// button closes it.
#[test]
fn the_floating_panels_menu_picks_the_terminal_it_shows() {
    let (app, mut window) = open();
    window.update(|w, _, cx| w.set_prefs(|prefs| prefs.agent_terminals_open = Opens::Float, cx));
    open_from(&mut window, 0, "echo first");
    open_from(&mut window, 0, "echo second");
    settle(&app, &mut window);
    let menu =
        |window: &TestAppWindow<Workspace>| window.read(|w, _| w.agent_terminals.menu_open(0));
    let shown =
        |window: &TestAppWindow<Workspace>| window.read(|w, _| w.shown_panes(&w.terminals[0]));
    // The newest shows.
    assert_eq!(shown(&window), vec![0, 2]);
    click_spot(&mut window, "float-picker:0");
    assert!(menu(&window), "the menu didn't open");
    settle(&app, &mut window);
    click_spot(&mut window, "float-pick:1");
    assert!(!menu(&window), "picking one left the menu open");
    assert_eq!(shown(&window), vec![0, 1]);
    settle(&app, &mut window);
    click_spot(&mut window, "float-picker:0");
    assert!(menu(&window));
    click_spot(&mut window, "float-picker:0");
    assert!(!menu(&window), "a press on its button left it open");
    assert_eq!(shown(&window), vec![0, 1]);
}

/// The scan that marks a terminal idle is up to half a second old. A program started since holds the terminal, so the
/// agent's command gets a terminal of its own rather than being typed into that program.
#[test]
fn a_command_is_not_typed_into_a_program_started_since_the_terminal_was_seen_idle() {
    let (app, mut window) = open();
    window.update(|w, _, cx| w.set_prefs(|prefs| prefs.agents_run_commands = true, cx));
    open_from(&mut window, 0, "true");
    window.update(|w, _, cx| {
        w.terminal(1)
            .unwrap()
            .update(cx, |view, cx| view.write_input("sleep 30\r", cx));
    });
    let held = |window: &TestAppWindow<Workspace>| {
        window.read(|w, cx| {
            let view = w.terminal(1).unwrap().read(cx);
            // The `true` the pane opened with is a program in front too, for as long as the loaded machine takes to run it,
            // so it is the `sleep` that has to be in front, not any program.
            view.foreground_pid().is_some_and(|pid| {
                Some(pid) != view.process_id()
                    && std::process::Command::new("ps")
                        .args(["-o", "command=", "-p", &pid.to_string()])
                        .output()
                        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains("sleep 30"))
            })
        })
    };
    let deadline = Instant::now() + super::tests::THREAD_BOUND;
    while !held(&window) {
        assert!(Instant::now() < deadline, "sleep never took the terminal");
        app.advance_clock(Duration::from_millis(100));
        app.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
    // The last scan still says it sits at its prompt.
    window.update(|w, _, _| {
        w.pane_info.insert(
            1,
            PaneInfo {
                command: "zsh".into(),
                cwd: Some("/".into()),
                busy: false,
            },
        );
    });
    let next = open_from(&mut window, 0, "echo next");
    assert_eq!((next.pane_id.as_str(), next.reused), ("2", false));
}
