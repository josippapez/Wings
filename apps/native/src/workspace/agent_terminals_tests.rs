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
