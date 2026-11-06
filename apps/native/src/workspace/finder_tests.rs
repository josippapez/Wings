//! Headless window tests for the command palette and the history sheet, over made-up sessions in a folder of their own:
//! never the sessions of whoever runs the tests.
//!
//! A made-up session's folder is one that is not there, so the `cd` in front of the resume command fails and the
//! `claude` after it never runs.

use super::*;
use crate::{
    host::SessionSource,
    workspace::{
        history::SessionRef,
        rank::Kind,
        tests::{open_config, press, state, wait_for},
    },
};
use gpui::{TestApp, TestAppWindow};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const LOGIN: &str = "0b8e7c1a-3d2f-4a5b-9c6d-7e8f9a0b1c2d";
const DARK: &str = "1c9f8d2b-4e3a-4b6c-8d7e-8f9a0b1c2d3e";

/// Short, so a typed line doesn't wrap in the terminal.
fn folder(name: &str) -> PathBuf {
    let dir = PathBuf::from(format!("/tmp/wnf-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A Claude Code folder holding `sessions`: id, folder they ran in, branch, first prompt and reply.
fn made_up_claude(dir: &Path, sessions: &[(&str, &Path, &str, &str, &str)]) -> PathBuf {
    let claude = dir.join("claude");
    for (id, cwd, branch, prompt, reply) in sessions {
        let name: String = cwd
            .to_string_lossy()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let projects = claude.join("projects").join(name);
        std::fs::create_dir_all(&projects).unwrap();
        let user = serde_json::json!({ "type": "user", "cwd": cwd, "gitBranch": branch,
            "timestamp": "2026-10-01T10:00:00.000Z",
            "message": { "role": "user", "content": prompt } });
        let assistant = serde_json::json!({ "type": "assistant", "timestamp": "2026-10-01T10:00:05.000Z",
            "message": { "role": "assistant", "content": [{ "type": "text", "text": reply }] } });
        std::fs::write(
            projects.join(format!("{id}.jsonl")),
            format!("{user}\n{assistant}\n"),
        )
        .unwrap();
    }
    claude
}

/// A window on `project` whose past sessions are the ones in `claude`.
fn open_over(project: &Path, claude: &Path) -> (TestApp, TestAppWindow<Workspace>) {
    open_config(|config| {
        config.project = project.to_path_buf();
        config.sessions = Some(SessionSource::Folders(vec![claude.to_path_buf()]));
    })
}

fn titles(window: &TestAppWindow<Workspace>) -> Vec<String> {
    window.read(|w, cx| {
        let entries = w.palette_entries(cx);
        w.palette_shown(cx)
            .into_iter()
            .map(|i| entries[i].title.clone())
            .collect()
    })
}

fn active(window: &TestAppWindow<Workspace>) -> usize {
    window.read(|w, _| w.palette.as_ref().unwrap().active)
}

/// Waits until the focused pane has echoed `needle`.
fn echoes(app: &TestApp, window: &TestAppWindow<Workspace>, needle: &str) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let output = window.read(|w, cx| {
            let view = w.focused_pane().and_then(|pane| w.terminal(pane));
            view.map(|view| String::from_utf8_lossy(&view.read(cx).recent_output()).into_owned())
                .unwrap_or_default()
        });
        // zsh says "no such file", bash "No such file".
        if output.to_lowercase().contains(&needle.to_lowercase()) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "never echoed {needle:?}: {output:?}"
        );
        app.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn session(id: &str, cwd: &Path) -> SessionRef {
    SessionRef {
        id: id.into(),
        cwd: cwd.to_string_lossy().into_owned(),
        agent: wings_agents::history::Agent::Claude,
    }
}

#[test]
fn the_palette_opens_by_its_shortcut_filters_and_runs_a_command_by_keyboard() {
    let (app, mut window) = open_config(|_| {});
    press(&mut window, "k");
    assert!(window.read(|w, _| w.palette.is_some()));
    // Typed words reach the search field, which has the keys.
    window.simulate_input("sidebar");
    assert_eq!(
        titles(&window).first().map(String::as_str),
        Some("Toggle Sidebar")
    );
    assert!(window.read(|w, _| w.sidebar));
    window.simulate_keystroke("enter");
    app.run_until_parked();
    assert!(window.read(|w, _| w.palette.is_none()));
    assert!(!window.read(|w, _| w.sidebar), "Toggle Sidebar ran");

    // Again by the same shortcut: it closes, and Escape closes too, with the keys back in the pane.
    press(&mut window, "k");
    assert!(window.read(|w, _| w.palette.is_some()));
    press(&mut window, "k");
    assert!(window.read(|w, _| w.palette.is_none()));
    press(&mut window, "k");
    window.simulate_keystroke("escape");
    assert!(window.read(|w, _| w.palette.is_none()));
    let focused = window.update(|w, window, cx| {
        let pane = w.focused_pane().unwrap();
        w.terminal(pane)
            .unwrap()
            .focus_handle(cx)
            .is_focused(window)
    });
    assert!(focused);
}

#[test]
fn the_arrow_keys_and_ctrl_n_and_p_move_through_the_list_and_enter_runs_the_highlighted_line() {
    let (app, mut window) = open_config(|_| {});
    press(&mut window, "k");
    let count = titles(&window).len();
    assert_eq!(active(&window), 0);
    window.simulate_keystroke("down");
    window.simulate_keystroke("ctrl-n");
    assert_eq!(active(&window), 2);
    window.simulate_keystroke("up");
    window.simulate_keystroke("ctrl-p");
    window.simulate_keystroke("up");
    assert_eq!(active(&window), count - 1, "it wraps round");
    window.simulate_keystroke("home");
    assert_eq!(active(&window), 0);
    window.simulate_keystroke("end");
    assert_eq!(active(&window), count - 1);

    // A second tab, then the palette's first Tab line goes back to the first.
    window.simulate_keystroke("escape");
    press(&mut window, "t");
    assert_eq!(state(&window).1, Active::Terminal(1));
    press(&mut window, "k");
    let first_tab = window.read(|w, cx| {
        let entries = w.palette_entries(cx);
        w.palette_shown(cx)
            .iter()
            .position(|&i| entries[i].kind == Kind::Tab)
    });
    for _ in 0..first_tab.expect("a Tab line") {
        window.simulate_keystroke("down");
    }
    window.simulate_keystroke("enter");
    app.run_until_parked();
    assert_eq!(state(&window).1, Active::Terminal(0));
}

#[test]
fn a_command_that_cannot_run_does_nothing_and_the_palette_stays() {
    let (_app, mut window) = open_config(|_| {});
    press(&mut window, "k");
    // One terminal tab: there is nothing to move it past.
    window.simulate_input("move tab left");
    assert_eq!(
        titles(&window).first().map(String::as_str),
        Some("Move Tab Left")
    );
    window.simulate_keystroke("enter");
    assert!(window.read(|w, _| w.palette.is_some()));
}

#[test]
fn the_palette_lists_the_commands_with_their_keys_and_none_the_native_app_lacks() {
    let (_app, mut window) = open_config(|_| {});
    press(&mut window, "k");
    window.read(|w, cx| {
        let entries = w.palette_entries(cx);
        let shortcut = |title: &str| {
            entries
                .iter()
                .find(|e| e.title == title)
                .unwrap_or_else(|| panic!("no {title}"))
                .shortcut
                .clone()
        };
        let mac = cfg!(target_os = "macos");
        let keys =
            |mac_keys: &str, other: &str| Some(if mac { mac_keys } else { other }.to_owned());
        assert_eq!(shortcut("New Tab"), keys("⌘T", "Ctrl+Shift+T"));
        assert_eq!(shortcut("Split Down"), keys("⇧⌘D", "Ctrl+Shift+Alt+D"));
        assert_eq!(shortcut("Focus Pane Left"), keys("⌥⌘←", "Ctrl+Shift+Alt+←"));
        assert_eq!(shortcut("Increase Font Size"), keys("⌘+", "Ctrl+Shift++"));
        assert_eq!(shortcut("Settings"), keys("⌘,", "Ctrl+,"));
        assert_eq!(shortcut("Close Tab"), None);
        assert_eq!(shortcut("Quick Terminal"), keys("⌃`", "Ctrl+`"));
        // The desktop's palette doesn't list itself, nor what the native app can't do.
        for missing in ["Command Palette", "Go Back"] {
            assert!(entries.iter().all(|e| e.title != missing), "{missing}");
        }
        assert!(entries.iter().any(|e| e.title == "Past Sessions"));
        assert!(entries.iter().any(|e| e.title == "Check for Updates"));
    });
}

#[test]
fn a_recent_session_is_found_in_the_palette_and_enter_resumes_it_in_a_new_tab() {
    let project = folder("palette");
    let gone = project.join("g/s");
    let claude = made_up_claude(
        &folder("palette-claude"),
        &[
            (
                LOGIN,
                &gone,
                "main",
                "Fix the login redirect",
                "I updated the redirect handler",
            ),
            (
                DARK,
                &gone,
                "main",
                "Add dark mode",
                "Done with the dark mode toggle",
            ),
        ],
    );
    let (app, mut window) = open_over(&project, &claude);
    press(&mut window, "k");
    window.simulate_input("login redirect");
    wait_for(
        &app,
        &window,
        |w| w.palette.as_ref().is_some_and(|p| p.sessions.len() == 1),
        "the palette never listed the session",
    );
    assert_eq!(
        titles(&window).first().map(String::as_str),
        Some("Fix the login redirect")
    );
    let tabs = window.read(|w, _| w.terminals.len());
    window.simulate_keystroke("enter");
    app.run_until_parked();
    assert!(window.read(|w, _| w.palette.is_none()));
    assert_eq!(window.read(|w, _| w.terminals.len()), tabs + 1, "a new tab");
    assert_eq!(state(&window).1, Active::Terminal(tabs));
    echoes(&app, &window, &format!("claude --resume {LOGIN}"));
    echoes(&app, &window, "no such file or directory");
}

#[test]
fn the_history_sheet_lists_this_projects_sessions_then_everyones_and_narrows_by_words_and_branch() {
    let project = folder("sheet");
    let elsewhere = folder("elsewhere");
    let claude = made_up_claude(
        &folder("sheet-claude"),
        &[
            (
                LOGIN,
                &project.join("g/s"),
                "main",
                "Fix the login redirect",
                "I updated the redirect handler",
            ),
            (
                DARK,
                &elsewhere,
                "fix-login",
                "Add dark mode",
                "Done with the dark mode toggle",
            ),
        ],
    );
    let (app, mut window) = open_over(&project, &claude);
    let ids = |window: &TestAppWindow<Workspace>| -> Vec<String> {
        window.read(|w, _| {
            let results = w.sheet.as_ref().unwrap().results.as_ref();
            results.map_or_else(Vec::new, |r| {
                r.sessions.iter().map(|h| h.id.clone()).collect()
            })
        })
    };
    window.update(|w, window, cx| w.open_history(window, cx));
    wait_for(
        &app,
        &window,
        |w| w.sheet.as_ref().is_some_and(|s| s.results.is_some()),
        "no results",
    );
    assert_eq!(ids(&window), [LOGIN], "this project's only");

    window.update(|w, _, cx| w.refine_history(|sheet| sheet.all = true, cx));
    wait_for(
        &app,
        &window,
        |w| {
            w.sheet
                .as_ref()
                .unwrap()
                .results
                .as_ref()
                .is_some_and(|r| r.total == 2)
        },
        "every project's sessions never came",
    );
    window.simulate_input("redirect");
    wait_for(
        &app,
        &window,
        |w| {
            w.sheet
                .as_ref()
                .unwrap()
                .results
                .as_ref()
                .is_some_and(|r| r.total == 1)
        },
        "the words never narrowed it",
    );
    assert_eq!(ids(&window), [LOGIN]);
    let snippet = window.read(|w, _| {
        w.sheet.as_ref().unwrap().results.as_ref().unwrap().sessions[0].snippets[0]
            .text
            .clone()
    });
    assert!(snippet.contains("redirect"), "{snippet}");

    window.simulate_input(" nothing-like-it");
    wait_for(
        &app,
        &window,
        |w| {
            w.sheet
                .as_ref()
                .unwrap()
                .results
                .as_ref()
                .is_some_and(|r| r.total == 0)
        },
        "a word that isn't there still matched",
    );
    window.simulate_keystroke("escape");
    assert!(window.read(|w, _| w.sheet.is_none()));

    window.update(|w, window, cx| w.open_history(window, cx));
    window.update(|w, _, cx| {
        w.refine_history(
            |sheet| (sheet.all, sheet.branch) = (true, Some("fix-login".into())),
            cx,
        )
    });
    wait_for(
        &app,
        &window,
        |w| {
            w.sheet
                .as_ref()
                .unwrap()
                .results
                .as_ref()
                .is_some_and(|r| r.total == 1)
        },
        "the branch never narrowed it",
    );
    assert_eq!(ids(&window), [DARK]);
    let branches = window.read(|w, _| {
        w.sheet
            .as_ref()
            .unwrap()
            .results
            .as_ref()
            .unwrap()
            .branches
            .clone()
    });
    assert!(branches.contains(&"main".into()) && branches.contains(&"fix-login".into()));
}

#[test]
fn picking_a_session_opens_it_in_a_new_tab_where_it_started_and_resume_here_types_it_in_the_pane() {
    let project = folder("pick");
    let gone = project.join("g/s");
    let claude = made_up_claude(
        &folder("pick-claude"),
        &[(
            LOGIN,
            &gone,
            "main",
            "Fix the login redirect",
            "I updated the redirect handler",
        )],
    );
    let (app, mut window) = open_over(&project, &claude);

    // Resume starts a tab of its own, with the folder to go to first; a fork starts a copy.
    let tabs = window.read(|w, _| w.terminals.len());
    window.update(|w, window, cx| w.open_history(window, cx));
    window.update(|w, window, cx| w.start_session(&session(LOGIN, &gone), true, window, cx));
    assert!(window.read(|w, _| w.sheet.is_none()));
    assert_eq!(window.read(|w, _| w.terminals.len()), tabs + 1);
    echoes(
        &app,
        &window,
        &format!("claude --resume {LOGIN} --fork-session"),
    );

    // In this tab only at a prompt: a pane running a program keeps the sheet and gets nothing.
    window.update(|w, window, cx| w.open_history(window, cx));
    window.update(|w, window, cx| {
        let pane = w.focused_pane().unwrap();
        w.pane_info.insert(
            pane,
            PaneInfo {
                command: "claude".into(),
                cwd: None,
                busy: true,
            },
        );
        w.resume_here(&session(DARK, &gone), window, cx)
    });
    assert!(window.read(|w, _| w.sheet.is_some()));
    window.update(|w, window, cx| {
        let pane = w.focused_pane().unwrap();
        w.pane_info.insert(
            pane,
            PaneInfo {
                command: "zsh".into(),
                cwd: None,
                busy: false,
            },
        );
        w.resume_here(&session(DARK, &gone), window, cx)
    });
    assert!(window.read(|w, _| w.sheet.is_none()));
    echoes(&app, &window, &format!("claude --resume {DARK}"));
}

#[test]
fn a_session_opens_in_the_project_that_holds_its_folder_or_in_one_made_of_it() {
    let outer = folder("outer");
    let inner = outer.join("inner");
    std::fs::create_dir_all(inner.join("deep")).unwrap();
    let stranger = folder("stranger");
    let list = outer.join("spaces.json");
    std::fs::write(
        &list,
        serde_json::json!([
            { "id": "o", "name": "outer", "path": outer },
            { "id": "i", "name": "inner", "path": inner },
        ])
        .to_string(),
    )
    .unwrap();
    let (_app, mut window) = open_config(|config| {
        config.project = outer.clone();
        config.projects = Some(list.clone());
    });
    let plan = |window: &mut TestAppWindow<Workspace>, cwd: &Path, fork: bool| {
        window.update(|w, _, _| w.launch_plan(&session(LOGIN, cwd), fork))
    };
    // The folder of a project: no `cd`. A folder inside two projects: the deepest one, and a `cd` to get there.
    assert_eq!(
        plan(&mut window, &outer, false),
        Ok((outer.clone(), format!("claude --resume {LOGIN}\r")))
    );
    assert_eq!(
        plan(&mut window, &inner.join("deep"), false),
        Ok((
            inner.clone(),
            format!("cd {}/deep && claude --resume {LOGIN}\r", inner.display())
        ))
    );
    // A folder no project holds becomes one.
    assert_eq!(
        plan(&mut window, &stranger, true),
        Ok((
            stranger.clone(),
            format!("claude --resume {LOGIN} --fork-session\r")
        ))
    );
    assert!(window.read(|w, _| w.projects.iter().any(|p| p.path == stranger)));
    // Nothing is made of a folder that is gone, and what could run something else isn't typed.
    let nowhere = PathBuf::from(format!("/tmp/wnf-{}-nowhere/x", std::process::id()));
    assert!(plan(&mut window, &nowhere, false).is_err());
    assert!(plan(&mut window, Path::new("/tmp/a\nrm -rf ~"), false).is_err());
    window.update(|w, _, _| {
        assert!(
            w.launch_plan(&session("x; rm -rf ~", &outer), false)
                .is_err()
        )
    });
}

#[test]
fn the_resume_command_can_be_copied() {
    let project = folder("copy");
    let (app, mut window) = open_over(&project, &folder("copy-claude"));
    let gone = project.join("g/s");
    window.update(|w, window, cx| w.open_history(window, cx));
    window.update(|w, _, cx| w.copy_resume_command(&session(LOGIN, &gone), cx));
    assert_eq!(
        app.read_from_clipboard().and_then(|item| item.text()),
        Some(format!("cd {} && claude --resume {LOGIN}", gone.display()))
    );
}

#[test]
fn the_palette_and_the_sheet_wait_while_settings_has_the_keys() {
    let (_app, mut window) = open_config(|_| {});
    press(&mut window, ",");
    assert!(window.read(|w, _| w.settings.is_some()));
    press(&mut window, "k");
    window.update(|w, window, cx| w.open_history(window, cx));
    assert!(window.read(|w, _| w.palette.is_none() && w.sheet.is_none()));
}
