//! Managing plugins from the window, as the desktop's Plugins section does, and the agent features that press Enter or
//! ask before a setup command is saved: real packages, real plugin mains, real shells, in temp folders only.

use std::{
    io::Write,
    sync::{Arc, Mutex},
    time::Duration,
};

use gpui::{TestApp, TestAppWindow};
use serde_json::{Value, json};
use wings_plugins::store::{Grant, Source};

use super::{
    Workspace,
    plugin_tests::{open_plugins, until},
};
use crate::installed::fixture::Data;

/// A `.wings-plugin` of plugin `id` whose native main saves `version` under `started`, asking to run `exec`.
fn package(id: &str, version: &str, exec: &[&str]) -> Vec<u8> {
    let manifest = json!({ "id": id, "name": format!("Plugin {id}"), "version": version, "api": 1,
        "main": "dist/main.js", "permissions": { "exec": exec } });
    let main = format!("wings.storage.set('started', '{version}');");
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, body) in [
        ("wings-plugin.json", manifest.to_string()),
        ("dist/main.js", String::new()),
        ("dist/native/main.js", main),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn github() -> Source {
    Source::Github {
        repo: "owner/demo".into(),
        folder: None,
    }
}

fn runs(window: &TestAppWindow<Workspace>, id: &str) -> bool {
    window.read(|w, _| w.plugin_runs(id) && w.plugin_env.running.builtin(id).is_some())
}

/// What the plugin's main saved, read where the desktop reads it.
fn started(data: &Data, w: &Workspace, id: &str) -> Option<Value> {
    let scope = w
        .plugin_env
        .store
        .as_ref()?
        .lock()
        .unwrap()
        .known_storage_scope(id)?;
    data.saved(id, &scope, "started")
}

fn state(data: &Data) -> Value {
    serde_json::from_slice(&std::fs::read(data.paths().state).unwrap()).unwrap()
}

/// Installs `bytes` from `source` as the window would after a download.
fn install(
    window: &mut TestAppWindow<Workspace>,
    bytes: Vec<u8>,
    source: Source,
    expected: Option<&str>,
) -> Result<wings_plugins::store::PluginView, String> {
    window.update(|w, window, cx| w.install_package(&bytes, source, expected, window, cx))
}

/// Approves what the plugin's manifest asks for now, as the review's Turn on does.
fn approve(window: &mut TestAppWindow<Workspace>, id: &str) {
    window.update(|w, window, cx| {
        let view = w
            .plugin_views()
            .into_iter()
            .find(|v| v.manifest.id == id)
            .unwrap();
        w.set_plugin_enabled(id, true, Some(Grant::of(&view.manifest)), window, cx)
            .unwrap();
    });
}

#[test]
fn a_package_installs_off_asks_for_review_and_runs_once_approved() {
    let data = Data::temp();
    let file = data.path().join("demo.wings-plugin");
    std::fs::write(&file, package("demo", "1.0.0", &["git status"])).unwrap();
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    window.update(|w, window, cx| w.install_file(file.clone(), window, cx));
    until(&app, &mut window, "the review never opened", |w| {
        !w.manage.reviews.is_empty()
    });
    // Installed, but off and unapproved: nothing of it runs until you turn it on.
    assert!(!runs(&window, "demo"));
    assert_eq!(state(&data)["demo"]["enabled"], false);
    assert_eq!(state(&data)["demo"]["approved"], Value::Null);
    app.advance_clock(Duration::from_secs(1));
    app.run_until_parked();
    window.update(|w, window, cx| w.approve_review(window, cx));
    assert!(window.read(|w, _| w.manage.reviews.is_empty()));
    assert!(runs(&window, "demo"));
    until(&app, &mut window, "the main never ran", |w| {
        started(&data, w, "demo").is_some()
    });
    assert_eq!(
        window.read(|w, _| started(&data, w, "demo")),
        Some(json!("1.0.0"))
    );
    assert_eq!(state(&data)["demo"]["enabled"], true);
    assert_eq!(
        state(&data)["demo"]["approved"]["permissions"]["exec"],
        json!(["git status"])
    );
}

#[test]
fn turn_on_does_nothing_until_each_review_has_been_up_a_moment() {
    use crate::installed::{WINGS_VERSION, fixture::Approval::Never};
    let data = Data::temp();
    data.install("one", WINGS_VERSION, true, false, Never, "s1");
    data.install("two", WINGS_VERSION, true, false, Never, "s2");
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    window.update(|w, window, cx| {
        w.review_plugin("one", window, cx);
        w.review_plugin("two", window, cx);
        // A click that lands as the review opens approves nothing.
        w.approve_review(window, cx);
    });
    assert_eq!(window.read(|w, _| w.manage.reviews.len()), 2);
    assert!(!runs(&window, "one"));
    app.advance_clock(Duration::from_secs(1));
    app.run_until_parked();
    window.update(|w, window, cx| {
        w.approve_review(window, cx);
        // The next review shows, and a second click meant for the first lands on nothing.
        w.approve_review(window, cx);
    });
    assert!(runs(&window, "one") && !runs(&window, "two"));
    assert_eq!(window.read(|w, _| w.manage.reviews.len()), 1);
    app.advance_clock(Duration::from_secs(1));
    app.run_until_parked();
    window.update(|w, window, cx| w.approve_review(window, cx));
    assert!(runs(&window, "two"));
}

#[test]
fn an_update_restarts_the_plugin_with_its_new_version_and_keeps_its_approval() {
    let data = Data::temp();
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    install(
        &mut window,
        package("demo", "1.0.0", &["git status"]),
        github(),
        None,
    )
    .unwrap();
    approve(&mut window, "demo");
    until(&app, &mut window, "v1 never ran", |w| {
        started(&data, w, "demo").is_some()
    });
    let view = install(
        &mut window,
        package("demo", "1.1.0", &["git status"]),
        github(),
        Some("demo"),
    )
    .unwrap();
    assert!(view.enabled && view.approved && runs(&window, "demo"));
    until(&app, &mut window, "v1.1 never ran", |w| {
        started(&data, w, "demo") == Some(json!("1.1.0"))
    });
    // An update that turns out to be another plugin is refused.
    let other = install(
        &mut window,
        package("other", "1.0.0", &[]),
        github(),
        Some("demo"),
    );
    assert!(other.unwrap_err().contains("different plugin"));
}

#[test]
fn an_update_that_asks_for_more_access_stops_the_plugin_until_it_is_approved_again() {
    let data = Data::temp();
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    install(
        &mut window,
        package("demo", "1.0.0", &["git status"]),
        github(),
        None,
    )
    .unwrap();
    approve(&mut window, "demo");
    assert!(runs(&window, "demo"));
    let shown = window.read(|w, _| {
        w.plugin_views()
            .into_iter()
            .find(|v| v.manifest.id == "demo")
            .unwrap()
    });
    let grown = install(
        &mut window,
        package("demo", "1.1.0", &["git status", "gh api"]),
        github(),
        Some("demo"),
    )
    .unwrap();
    assert!(!grown.approved && !runs(&window, "demo"));
    // What the dialog showed before the update doesn't approve the new access.
    let stale = window.update(|w, window, cx| {
        w.set_plugin_enabled("demo", true, Some(Grant::of(&shown.manifest)), window, cx)
    });
    assert!(stale.unwrap_err().contains("changed since"));
    assert!(!runs(&window, "demo"));
    // The switch asks for a review instead of turning it on.
    window.update(|w, window, cx| w.toggle_plugin(grown.clone(), true, window, cx));
    assert_eq!(window.read(|w, _| w.manage.reviews.len()), 1);
    app.advance_clock(Duration::from_secs(1));
    app.run_until_parked();
    window.update(|w, window, cx| w.approve_review(window, cx));
    assert!(runs(&window, "demo"));
}

#[test]
fn turning_a_plugin_off_or_removing_it_stops_its_main_and_closes_its_tabs() {
    let data = Data::temp();
    data.install(
        "demo",
        crate::installed::WINGS_VERSION,
        true,
        true,
        crate::installed::fixture::Approval::Current,
        "scope",
    );
    let (_app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    assert!(runs(&window, "demo"));
    window.update(|w, window, cx| assert!(w.open_plugin_view("demo", "fixture", None, window, cx)));
    let host = Arc::downgrade(&window.read(|w, _| w.plugins[0].host.clone()));
    window.update(|w, window, cx| {
        w.set_plugin_enabled("demo", false, None, window, cx)
            .unwrap()
    });
    assert!(!runs(&window, "demo"));
    assert!(window.read(|w, _| w.plugin_tabs.is_empty()));
    assert!(
        host.upgrade().is_none(),
        "the stopped plugin's host is still alive"
    );
    assert_eq!(state(&data)["demo"]["enabled"], false);
    // On again, with what it was approved for: it runs again.
    window.update(|w, window, cx| {
        let grant = w
            .plugin_env
            .store
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .current_grant("demo")
            .unwrap();
        w.set_plugin_enabled("demo", true, grant, window, cx)
            .unwrap();
    });
    assert!(runs(&window, "demo"));
    window.update(|w, window, cx| w.remove_plugin("demo", window, cx).unwrap());
    assert!(!runs(&window, "demo"));
    assert!(window.read(|w, _| w.plugin_views().is_empty()));
    assert!(!data.paths().dir.join("demo").exists());
    assert!(state(&data).get("demo").is_none());
}

fn agent(pane: usize, state: crate::agents::AgentState) -> crate::agents::Agent {
    crate::agents::Agent {
        pane_id: pane.to_string(),
        space_id: String::new(),
        pid: 1,
        kind: crate::agents::AgentKind::Claude,
        session_id: None,
        name: None,
        state,
        waiting_for: Some("permission to run Bash".into()),
        args: Vec::new(),
        started_at: 0,
        state_since: None,
        direct: true,
        summary: None,
        weak_name: false,
    }
}

/// Sends `text` to the agent in pane 0 as a plugin's `sendToAgent` does, and the answer.
fn send(app: &TestApp, window: &mut TestAppWindow<Workspace>, text: &str) -> Result<Value, String> {
    let answer = paste(window, text);
    answered(app, &answer)
}

/// Starts sending `text` to the agent in pane 0; the answer comes once Enter was pressed or skipped.
fn paste(window: &mut TestAppWindow<Workspace>, text: &str) -> crate::page::runtime::Answer {
    let (reply, answer) = crate::page::runtime::Reply::channel();
    window.update(|w, _, cx| w.send_to_agent("0", text, reply, cx));
    answer
}

/// Runs the clock past the moment Enter waits for, until the answer is there.
fn answered(app: &TestApp, answer: &crate::page::runtime::Answer) -> Result<Value, String> {
    for _ in 0..50 {
        if let Some(answer) = answer.wait(Duration::ZERO) {
            return Ok(serde_json::from_str(&answer?).unwrap());
        }
        app.advance_clock(Duration::from_millis(50));
        app.run_until_parked();
    }
    panic!("never answered");
}

fn output(window: &TestAppWindow<Workspace>) -> String {
    window.read(|w, cx| {
        String::from_utf8_lossy(&w.terminal(0).unwrap().read(cx).recent_output()).into_owned()
    })
}

#[test]
fn enter_is_pressed_for_an_agent_only_with_the_setting_on_and_never_over_a_prompt_or_your_typing() {
    use crate::agents::AgentState;
    let (app, mut window) = super::tests::open_config(|_| {});
    super::tests::press(&mut window, "d");
    window.update(|w, _, _| {
        w.agents.insert(0, agent(0, AgentState::Idle));
    });
    // Off, as on the desktop by default: the message is pasted and waits for you.
    assert_eq!(
        send(&app, &mut window, "echo off-$((1+1))").unwrap()["submitted"],
        false
    );
    // On: Enter follows a moment later, so the shell standing in for the agent runs it.
    window.update(|w, _, cx| w.set_prefs(|prefs| prefs.agents_message = true, cx));
    assert_eq!(
        send(&app, &mut window, "; echo on-$((2+2))").unwrap()["submitted"],
        true
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !output(&window).contains("on-4") {
        assert!(
            std::time::Instant::now() < deadline,
            "Enter was never pressed: {}",
            output(&window)
        );
        app.advance_clock(Duration::from_millis(200));
        app.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
    // An agent waiting on a permission prompt would take the text for its answer.
    window.update(|w, _, _| {
        w.agents.insert(0, agent(0, AgentState::Blocked));
    });
    assert!(
        send(&app, &mut window, "hi")
            .unwrap_err()
            .contains("waiting for permission to run Bash")
    );
    // Text you typed and haven't sent stays yours.
    window.update(|w, window, cx| {
        w.agents.insert(0, agent(0, AgentState::Idle));
        w.terminal(0).unwrap().update(cx, |view, cx| {
            gpui::EntityInputHandler::replace_text_in_range(view, None, "draft", window, cx)
        });
    });
    assert!(
        send(&app, &mut window, "hi")
            .unwrap_err()
            .contains("hasn't sent it")
    );
}

#[test]
fn enter_is_skipped_when_you_type_or_the_agent_comes_to_a_prompt_before_it_is_pressed() {
    use crate::agents::AgentState;
    let (app, mut window) = super::tests::open_config(|_| {});
    super::tests::press(&mut window, "d");
    window.update(|w, _, cx| {
        w.agents.insert(0, agent(0, AgentState::Idle));
        w.set_prefs(|prefs| prefs.agents_message = true, cx);
    });
    // You type in the moment between the paste and Enter.
    let answer = paste(&mut window, "echo typed-$((3+3))");
    window.update(|w, window, cx| {
        w.terminal(0).unwrap().update(cx, |view, cx| {
            gpui::EntityInputHandler::replace_text_in_range(view, None, "x", window, cx)
        });
    });
    assert_eq!(answered(&app, &answer).unwrap()["submitted"], false);
    // The agent comes to a permission prompt in that moment. Ctrl-C clears the prompt and your draft first.
    window.update(|w, window, cx| {
        w.terminal(0).unwrap().update(cx, |view, cx| {
            gpui::EntityInputHandler::replace_text_in_range(view, None, "\u{3}", window, cx)
        });
    });
    let answer = paste(&mut window, "echo blocked-$((4+4))");
    window.update(|w, _, _| {
        w.agents.insert(0, agent(0, AgentState::Blocked));
    });
    assert_eq!(answered(&app, &answer).unwrap()["submitted"], false);
    // Neither was run: no Enter reached the shell.
    for _ in 0..20 {
        app.advance_clock(Duration::from_millis(100));
        app.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
    let shown = output(&window);
    assert!(
        !shown.contains("typed-6") && !shown.contains("blocked-8"),
        "{shown}"
    );
}

#[test]
fn open_pane_runs_the_command_only_with_the_setting_on() {
    let (_app, mut window) = super::tests::open_config(|_| {});
    let plan = |run| wings_connect::panes::OpenPlan {
        space_id: std::env::temp_dir().to_string_lossy().into_owned(),
        cwd: None,
        input: Some("echo hi".into()),
        run,
        next_to: None,
    };
    let ran = |window: &mut TestAppWindow<Workspace>, run| {
        window.update(|w, window, cx| w.claude_open_pane(plan(run), window, cx).unwrap().ran)
    };
    assert!(
        !ran(&mut window, true),
        "pressed Enter with the setting off"
    );
    window.update(|w, _, cx| w.set_prefs(|prefs| prefs.agents_run_commands = true, cx));
    assert!(ran(&mut window, true));
    assert!(
        !ran(&mut window, false),
        "pressed Enter without being asked to run"
    );
}

/// Calls `worktree_setup` on the app's socket from another thread, and the place its answer lands.
fn propose(socket: &std::path::Path, setup: &str) -> Arc<Mutex<Option<Value>>> {
    use std::io::{BufRead, BufReader};
    let answer = Arc::new(Mutex::new(None));
    let (found, socket) = (answer.clone(), socket.to_owned());
    let request = json!({ "id": 1, "op": "call", "name": "worktree_setup", "arguments": { "project": "alpha", "setup": setup } });
    std::thread::spawn(move || {
        let stream = std::os::unix::net::UnixStream::connect(socket).unwrap();
        writeln!(&stream, "{request}").unwrap();
        for line in BufReader::new(&stream).lines() {
            let message: Value = serde_json::from_str(&line.unwrap()).unwrap_or(Value::Null);
            if message.get("id") == Some(&json!(1)) {
                *found.lock().unwrap() = Some(message);
                return;
            }
        }
    });
    answer
}

/// Waits for the prompt, checks Allow does nothing before it is armed, answers, and gives the tool's reply.
fn answer(
    app: &TestApp,
    window: &mut TestAppWindow<Workspace>,
    asked: &Arc<Mutex<Option<Value>>>,
    allow: bool,
) -> Value {
    until(app, window, "the prompt never showed", |w| {
        !w.setup_asks.queue.is_empty()
    });
    let shown = window.read(|w, _| {
        let ask = &w.setup_asks.queue[0];
        (ask.project.clone(), ask.proposed.clone())
    });
    assert_eq!(shown.0, "alpha");
    if allow {
        window.update(|w, _, cx| w.answer_setup(true, cx));
        assert_eq!(
            window.read(|w, _| w.setup_asks.queue.len()),
            1,
            "Allow answered before the prompt was armed"
        );
        app.advance_clock(Duration::from_secs(1));
        app.run_until_parked();
    }
    window.update(|w, _, cx| w.answer_setup(allow, cx));
    until(app, window, "the tool never answered", |_| {
        asked.lock().unwrap().is_some()
    });
    asked.lock().unwrap().take().unwrap()
}

#[test]
fn a_setup_command_an_agent_proposes_is_saved_only_once_you_allow_it() {
    let data = Data::temp();
    let alpha = data.path().canonicalize().unwrap().join("alpha");
    std::fs::create_dir_all(&alpha).unwrap();
    let list = data.path().join("spaces.json");
    std::fs::write(
        &list,
        json!([{ "id": "a", "name": "alpha", "path": alpha }]).to_string(),
    )
    .unwrap();
    let socket = data.path().join("mcp.sock");
    let (app, mut window) = open_plugins(&data, &Arc::default(), |config| {
        config.project = alpha.clone();
        config.projects = Some(list.clone());
        config.socket = Some(socket.clone());
    });
    let setup = |window: &TestAppWindow<Workspace>| {
        window.read(|w, _| {
            w.spaces
                .worktree_setup(&alpha.to_string_lossy())
                .unwrap()
                .setup
        })
    };

    let asked = propose(&socket, "make bootstrap");
    let reply = answer(&app, &mut window, &asked, false);
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    assert!(
        reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("declined")
    );
    assert_eq!(setup(&window), None);

    let asked = propose(&socket, "pnpm install");
    let reply = answer(&app, &mut window, &asked, true);
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    assert_eq!(setup(&window).as_deref(), Some("pnpm install"));
}
