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
    made_up_account(&dir.join("claude"), sessions)
}

/// The folder Claude Code keeps the transcripts of sessions in `cwd` in, under `projects/`.
fn slug(cwd: &Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// `made_up_claude` in the account folder `claude`.
fn made_up_account(claude: &Path, sessions: &[(&str, &Path, &str, &str, &str)]) -> PathBuf {
    let claude = claude.to_path_buf();
    for (id, cwd, branch, prompt, reply) in sessions {
        let projects = claude.join("projects").join(slug(cwd));
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
        transcript: None,
    }
}

/// Each key typed in the palette, and each line the arrows moved to, drew the sidebar, the title bar and the main column
/// again, as the palette's list is the workspace's to draw. The palette is drawn over them, and only the window draws
/// again for it.
#[test]
fn typing_in_the_palette_draws_the_window_and_none_of_its_pieces() {
    let (_app, mut window) = open_config(|_| {});
    press(&mut window, "k");
    window.draw();
    let mut counts = crate::ui::readout::Readout::new();
    counts.begin_measure().unwrap();
    counts.restart_measure().unwrap();
    window.simulate_input("sidebar");
    window.simulate_keystroke("down");
    window.simulate_keystroke("up");
    let renders = counts.end_measure().unwrap().renders;
    let count = |view: &str| renders.iter().find(|(name, _)| *name == view).map_or(0, |(_, count)| *count);
    assert_eq!(titles(&window).first().map(String::as_str), Some("Toggle Sidebar"));
    assert!(count("workspace") >= 9, "the window drew each key: {renders:?}");
    assert_eq!((count("sidebar"), count("title-bar"), count("main")), (0, 0, 0), "{renders:?}");
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
fn a_click_on_the_branch_filter_while_its_menu_is_open_closes_it() {
    let project = folder("toggle");
    let claude = made_up_claude(
        &folder("toggle-claude"),
        &[(LOGIN, &project, "main", "Fix the login redirect", "I updated the redirect handler")],
    );
    let (app, mut window) = open_over(&project, &claude);
    window.update(|w, window, cx| w.open_history(window, cx));
    wait_for(&app, &window, |w| w.sheet.as_ref().is_some_and(|s| s.results.is_some()), "no results");
    let shows = |app: &TestApp, window: &mut TestAppWindow<Workspace>| {
        app.advance_clock(crate::ui::motion::POPUP_IN * 2);
        app.run_until_parked();
        crate::ui::forget_spots();
        window.draw();
        crate::ui::spot_at("history-branch-menu").is_some()
    };
    for _ in 0..2 {
        super::plugin_tests::click_spot(&mut window, "history-branch");
        assert!(shows(&app, &mut window), "the branch menu opened");
        super::plugin_tests::click_spot(&mut window, "history-branch");
        assert!(!shows(&app, &mut window), "the branch menu closed by its own button");
    }
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

/// The default account's folder, and a named account's beside it holding `LOGIN`, with a subagent's transcript in the
/// folder beside its own; `DARK` is the default account's. The window lists both accounts' sessions.
fn two_accounts(name: &str) -> (TestApp, TestAppWindow<Workspace>, PathBuf, PathBuf, PathBuf) {
    two_accounts_with(name, None)
}

/// `two_accounts`, with the plugins in `data` running.
fn two_accounts_with(
    name: &str,
    data: Option<&crate::installed::fixture::Data>,
) -> (TestApp, TestAppWindow<Workspace>, PathBuf, PathBuf, PathBuf) {
    let project = folder(name);
    let gone = project.join("g/s");
    let accounts = folder(&format!("{name}-home"));
    let (default, work) = (accounts.join(".claude"), accounts.join(".claude-work"));
    made_up_account(
        &default,
        &[(DARK, &gone, "main", "Add dark mode", "Done with the toggle")],
    );
    made_up_account(
        &work,
        &[(
            LOGIN,
            &gone,
            "main",
            "Fix the login redirect",
            "I updated the redirect handler",
        )],
    );
    let extras = work.join("projects").join(slug(&gone)).join(LOGIN);
    std::fs::create_dir_all(extras.join("subagents")).unwrap();
    std::fs::write(extras.join("subagents/agent-1.jsonl"), "{}\n").unwrap();
    let (app, window) = open_config(|config| {
        config.project = project.clone();
        config.sessions = Some(SessionSource::Folders(vec![default.clone(), work.clone()]));
        config.plugins = data.map(|data| data.paths());
    });
    (app, window, gone, default, work)
}

/// Opens the sheet over this project's sessions and waits for them: their sessions, and the account each shows.
fn listed(
    app: &TestApp,
    window: &mut TestAppWindow<Workspace>,
) -> Vec<(SessionRef, Option<String>)> {
    window.update(|w, window, cx| w.open_history(window, cx));
    wait_for(
        app,
        window,
        |w| w.sheet.as_ref().is_some_and(|s| s.results.is_some()),
        "no results",
    );
    window.read(|w, _| {
        let sheet = w.sheet.as_ref().unwrap();
        let hits = &sheet.results.as_ref().unwrap().sessions;
        hits.iter()
            .zip(&sheet.accounts)
            .map(|(hit, account)| {
                let label = account.as_ref().map(ToString::to_string);
                (w.session_of(hit).unwrap(), label)
            })
            .collect()
    })
}

#[test]
fn a_session_shows_the_account_it_ran_under_unless_it_is_the_default_one() {
    let (app, mut window, _, _, _) = two_accounts("label");
    let mut accounts: Vec<(String, Option<String>)> = listed(&app, &mut window)
        .into_iter()
        .map(|(session, label)| (session.id, label))
        .collect();
    accounts.sort();
    assert_eq!(
        accounts,
        [(LOGIN.into(), Some("work".into())), (DARK.into(), None)]
    );
}

#[test]
fn resuming_another_accounts_session_copies_its_transcript_into_the_terminals_account() {
    let (app, mut window, gone, default, work) = two_accounts("bring");
    let sessions = listed(&app, &mut window);
    let login = sessions
        .iter()
        .find(|(s, _)| s.id == LOGIN)
        .unwrap()
        .0
        .clone();
    let (from, to) = (
        work.join("projects").join(slug(&gone)),
        default.join("projects").join(slug(&gone)),
    );
    // The project has no account of its own, so its terminals run the default one.
    window.update(|w, window, cx| w.start_session(&login, false, window, cx));
    let file = format!("{LOGIN}.jsonl");
    assert_eq!(
        std::fs::read(to.join(&file)).unwrap(),
        std::fs::read(from.join(&file)).unwrap()
    );
    assert!(to.join(LOGIN).join("subagents/agent-1.jsonl").is_file());
    assert!(from.join(&file).is_file(), "the original stays");
    echoes(&app, &window, &format!("claude --resume {LOGIN}"));
}

#[test]
fn a_transcript_already_in_the_terminals_account_is_kept() {
    let (app, mut window, gone, default, _) = two_accounts("keep");
    let sessions = listed(&app, &mut window);
    let login = sessions
        .iter()
        .find(|(s, _)| s.id == LOGIN)
        .unwrap()
        .0
        .clone();
    // Resumed there before and carried on: the continuation is kept.
    let there = default
        .join("projects")
        .join(slug(&gone))
        .join(format!("{LOGIN}.jsonl"));
    std::fs::write(&there, "carried on\n").unwrap();
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
        w.resume_here(&login, window, cx)
    });
    assert_eq!(std::fs::read_to_string(&there).unwrap(), "carried on\n");
    echoes(&app, &window, &format!("claude --resume {LOGIN}"));
}

#[test]
fn a_transcript_that_cant_be_copied_stops_the_resume_and_says_why() {
    let (app, mut window, _, default, _) = two_accounts("fail");
    let sessions = listed(&app, &mut window);
    let login = sessions
        .iter()
        .find(|(s, _)| s.id == LOGIN)
        .unwrap()
        .0
        .clone();
    // The default account's `projects` is a file, so nothing can be copied into it.
    std::fs::remove_dir_all(default.join("projects")).unwrap();
    std::fs::write(default.join("projects"), "").unwrap();
    let tabs = window.read(|w, _| w.terminals.len());
    window.update(|w, window, cx| w.start_session(&login, true, window, cx));
    assert_eq!(window.read(|w, _| w.terminals.len()), tabs, "no tab opened");
    let notice = window.read(|w, _| {
        w.plugin_ui
            .notices
            .last()
            .map(|n| (n.title.to_string(), n.body.to_string()))
    });
    assert!(
        notice
            .as_ref()
            .is_some_and(|(title, body)| title == "Couldn't fork the session"
                && body.starts_with("Its transcript couldn't be copied")),
        "{notice:?}"
    );
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
    // A session from another account copies with that account's folder, so it resumes there.
    let work = folder("copy-work");
    let mut other = session(LOGIN, &gone);
    other.transcript = Some(work.join(format!("projects/-g-s/{LOGIN}.jsonl")));
    window.update(|w, _, cx| w.copy_resume_command(&other, cx));
    assert_eq!(
        app.read_from_clipboard().and_then(|item| item.text()),
        Some(format!(
            "cd {} && CLAUDE_CONFIG_DIR={} claude --resume {LOGIN}",
            gone.display(),
            work.display()
        ))
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

/// A session with no title of its own shows the name the model made for it, as on the desktop.
#[test]
fn an_untitled_session_shows_its_generated_name() {
    let project = folder("named");
    let gone = project.join("g/s");
    let claude = made_up_claude(
        &folder("named-claude"),
        &[(
            LOGIN,
            &gone,
            "main",
            "Fix the login redirect",
            "I updated the redirect handler",
        )],
    );
    // Names for every session, so nothing is asked of a model.
    let ai = folder("named-ai");
    std::fs::write(
        ai.join("ai.json"),
        r#"{"engine":"ollama","ollamaModel":"m"}"#,
    )
    .unwrap();
    std::fs::write(
        ai.join("ai-names.json"),
        format!(r#"{{"{LOGIN}":"Login redirect fix"}}"#),
    )
    .unwrap();
    let (app, mut window) = open_config(|config| {
        config.project = project.clone();
        config.sessions = Some(SessionSource::Folders(vec![claude.clone()]));
        config.ai = Some(ai.clone());
    });
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
        Some("Login redirect fix")
    );
}

/// Two plugins with a page each: `chatty`'s view says it resumes Claude sessions, `plain`'s doesn't.
fn resuming_plugins() -> crate::installed::fixture::Data {
    use crate::page::runtime::fake;
    let data = crate::installed::fixture::Data::temp();
    let page = |id: &str, extra: serde_json::Value| {
        let mut view = serde_json::json!({ "id": id, "title": format!("View {id}"), "page": format!("dist/{id}.html"), "opens": ["tab"], "multiple": true });
        view.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        view
    };
    let views = serde_json::json!([page("chat", serde_json::json!({ "title": "Claude chat", "resumes": ["claude"] })), page("plain", serde_json::json!({}))]);
    data.install_script(
        "chatty",
        serde_json::json!({ "contributes": { "views": views } }),
        &fake::idle("main"),
        &[("chat", &fake::idle("page")), ("plain", &fake::idle("page"))],
        "chatty-scope",
    );
    data
}

#[test]
fn only_a_view_that_declares_it_is_offered_to_resume_a_session() {
    let data = resuming_plugins();
    let (_app, window, _, _, _) = two_accounts_with("offer", Some(&data));
    let offered = |agent| window.read(|w, _| w.resume_views(agent));
    assert_eq!(offered(wings_agents::history::Agent::Claude), [("chatty".into(), "chat".into(), "Claude chat".into())]);
    assert!(offered(wings_agents::history::Agent::OpenCode).is_empty());
    // With no plugin running there is nothing to offer.
    let (_app, window, _, _, _) = two_accounts("offer-none");
    assert!(window.read(|w, _| w.resume_views(wings_agents::history::Agent::Claude)).is_empty());
}

#[test]
fn a_work_account_session_resumes_in_the_view_with_the_args_the_badge_gives() {
    let data = resuming_plugins();
    let (app, mut window, gone, _, _) = two_accounts_with("view", Some(&data));
    let login = listed(&app, &mut window).into_iter().find(|(s, _)| s.id == LOGIN).unwrap().0;
    window.update(|w, window, cx| w.resume_in_view(&login, "chatty", "chat", window, cx));
    window.read(|w, _| {
        assert!(w.sheet.is_none(), "the sheet closed");
        let tab = w.plugin_tabs.last().expect("a tab opened");
        assert_eq!((tab.plugin.as_str(), tab.view.as_str()), ("chatty", "chat"));
        assert_eq!(tab.args, Some(serde_json::json!({ "resume": LOGIN, "cwd": gone, "program": "claude" })));
    });
}

/// A chat page that opens Past sessions to resume in its own tab as it starts, and again after each answer, which it
/// keeps under `answers`.
fn resuming_here() -> crate::installed::fixture::Data {
    use crate::page::runtime::fake::{self, Cx, Hear, Pending};
    let data = crate::installed::fixture::Data::temp();
    let page = fake::script("page", || {
        let mut wait = Pending::default();
        let mut answers = Vec::new();
        move |cx: &mut Cx<'_>, hear: Hear<'_>| {
            match hear {
                Hear::Start => {}
                Hear::Deliver(kind, id, text) => {
                    let Some(((), answer)) = wait.answered(kind, id, text) else { return };
                    answers.push(answer.unwrap_or_else(serde_json::Value::String));
                    cx.save("answers", &serde_json::json!(answers));
                }
                Hear::Event(..) => return,
            }
            wait.call(cx, "openHistory", serde_json::json!({ "resumeIn": "this tab" }), ());
        }
    });
    let views = serde_json::json!([{ "id": "chat", "title": "Claude chat", "page": "dist/chat.html", "opens": ["tab"], "resumes": ["claude"] }]);
    data.install_script("chatty", serde_json::json!({ "contributes": { "views": views } }), &fake::idle("main"), &[("chat", &page)], "chatty-scope");
    data
}

#[test]
fn a_page_opens_past_sessions_to_resume_its_agents_sessions_of_this_project_in_its_own_tab() {
    use super::plugin_tests::until;
    let data = &resuming_here();
    let (app, mut window, gone, _, _) = two_accounts_with("here", Some(data));
    window.update(|w, window, cx| assert!(w.open_plugin_view("chatty", "chat", None, window, cx)));
    until(&app, &mut window, "the page didn't open Past sessions", |w| w.sheet.is_some());
    let login = listed(&app, &mut window).into_iter().find(|(s, _)| s.id == LOGIN).unwrap().0;
    window.read(|w, _| {
        assert!(w.page_resumes(&login));
        let opencode = SessionRef { agent: wings_agents::history::Agent::OpenCode, ..login.clone() };
        assert!(!w.page_resumes(&opencode), "the page doesn't resume OpenCode's sessions");
        assert!(!w.page_resumes(&session(LOGIN, Path::new("/elsewhere"))), "nor sessions of another project");
    });
    window.update(|w, window, cx| w.resume_in_page(&login, window, cx));
    let answers = |n: usize| move |_: &Workspace| data.saved("chatty", "chatty-scope", "answers").is_some_and(|a| a.as_array().unwrap().len() == n);
    until(&app, &mut window, "the page wasn't told the session", answers(1));
    assert_eq!(
        data.saved("chatty", "chatty-scope", "answers"),
        Some(serde_json::json!([{ "resume": LOGIN, "cwd": gone }]))
    );
    // Closing the sheet without resuming one tells the page so.
    until(&app, &mut window, "the page didn't open Past sessions again", |w| w.sheet.is_some());
    window.update(|w, window, cx| w.close_history(window, cx));
    until(&app, &mut window, "the page wasn't told the sheet closed", answers(2));
    assert_eq!(data.saved("chatty", "chatty-scope", "answers").unwrap()[1], serde_json::Value::Null);
}

#[test]
fn a_view_that_is_not_there_says_so_instead_of_opening_nothing() {
    let data = resuming_plugins();
    let (app, mut window, _, _, _) = two_accounts_with("missing", Some(&data));
    let login = listed(&app, &mut window).into_iter().find(|(s, _)| s.id == LOGIN).unwrap().0;
    window.update(|w, window, cx| w.resume_in_view(&login, "chatty", "gone", window, cx));
    let notice = window.read(|w, _| w.plugin_ui.notices.last().map(|n| (n.title.to_string(), n.body.to_string())));
    let (title, body) = notice.expect("a notice");
    assert_eq!(title, "Couldn't resume the session");
    assert!(body.contains("chatty isn't running a page named gone"), "{body}");
}

#[test]
fn past_sessions_for_a_plugin_find_every_account_and_name_the_one_that_is_not_the_default() {
    use crate::page::plugin::Env;
    let (_app, window, gone, _, _) = two_accounts("plugin-list");
    let project = gone.ancestors().nth(2).unwrap().to_path_buf();
    let mut listed: Vec<(String, Option<String>)> = window
        .read(|w, _| w.plugin_env.past_sessions(&project, "", 50))
        .iter()
        .map(|s| (s["id"].as_str().unwrap().to_owned(), s["account"].as_str().map(str::to_owned)))
        .collect();
    listed.sort();
    assert_eq!(listed, [(LOGIN.into(), Some("work".into())), (DARK.into(), None)]);
}

#[test]
fn a_recent_session_in_the_palette_says_when_it_was_in_words_not_as_a_timestamp() {
    use wings_agents::history::Hit;
    let hit = Hit {
        id: LOGIN.into(),
        agent: wings_agents::history::Agent::Claude,
        cwd: Some("/work/acme-web".into()),
        title: Some("Fix the login redirect".into()),
        first_prompt: None,
        git_branch: Some("main".into()),
        last_active_ms: super::history::now_ms() - 5 * 60_000,
        last_active: Some("2026-10-01T10:00:00.000Z".into()),
        messages: 2,
        matches: 0,
        snippets: Vec::new(),
        name: None,
        transcript: None,
    };
    let entry = super::palette::session_entry(&hit).unwrap();
    assert_eq!(entry.subtitle.as_deref(), Some("acme-web · main · 5 minutes ago"));
}

#[test]
fn a_long_session_title_runs_to_the_end_of_its_row_not_to_where_the_hidden_buttons_would_start() {
    let project = folder("long-title");
    let claude = made_up_claude(
        &folder("long-title-claude"),
        &[(
            LOGIN,
            &project,
            "main",
            &"Why does the sidebar flicker when I resize the window ".repeat(8),
            "I looked at it",
        )],
    );
    let (app, mut window) = open_over(&project, &claude);
    window.update(|w, window, cx| w.open_history(window, cx));
    wait_for(&app, &window, |w| w.sheet.as_ref().is_some_and(|s| s.results.is_some()), "no results");
    app.advance_clock(crate::ui::motion::POPUP_IN * 2);
    app.run_until_parked();
    crate::ui::forget_spots();
    window.draw();
    let row = crate::ui::spot_at("history-row-0").expect("the row is drawn");
    let title = crate::ui::spot_at("history-title-0").expect("the title is drawn");
    // The Resume and more buttons show on hover only, so they must not hold space the title could use.
    assert!(
        row.right() - title.right() < gpui::px(40.0),
        "the title stops {:?} short of its row",
        row.right() - title.right()
    );
}

#[test]
fn the_history_sheet_is_full_height_its_meta_lines_up_with_the_title_and_its_scope_toggle_hugs_its_buttons() {
    let project = folder("layout");
    let claude = made_up_claude(
        &folder("layout-claude"),
        &[(LOGIN, &project.join("g/s"), "main", "Fix the login redirect", "I updated the redirect handler")],
    );
    let (app, mut window) = open_over(&project, &claude);
    window.update(|w, window, cx| w.open_history(window, cx));
    wait_for(&app, &window, |w| w.sheet.as_ref().is_some_and(|s| s.results.is_some()), "no results");
    app.advance_clock(crate::ui::motion::POPUP_IN * 2);
    app.run_until_parked();
    crate::ui::forget_spots();
    window.draw();
    let title = crate::ui::spot_at("history-title-0").expect("the title is drawn");
    let meta = crate::ui::spot_at("history-meta-0").expect("the meta line is drawn");
    // The meta box starts under the agent mark and pads past it: its text starts where the title's does.
    assert_eq!(meta.left() + gpui::px(18.0), title.left());
    let scope = crate::ui::spot_at("history-scope").expect("the scope toggle is drawn");
    assert!(scope.size.width < gpui::px(300.0), "the toggle is {:?} wide, as wide as the sheet", scope.size.width);
    let sheet = crate::ui::spot_at("history-sheet").expect("the sheet is drawn");
    let height = window.update(|_, window, _| window.viewport_size().height);
    assert_eq!((sheet.origin.y, sheet.size.height), (gpui::px(0.0), height), "the sheet covers the window's height");
}
