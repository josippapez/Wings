//! `spawn` with `survive`: the terminal server runs a tab's program, so it goes on while Wings quits, restarts or
//! updates, and the page of the same tab in Wings' next run takes it over. And no two programs on one Claude Code session.

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use wings_ptyd::{
    client::Control,
    host::{Config, test_host},
};

use crate::page::plugin::{
    FrameState, StatePlace,
    testing::{Caller, Heard, Rig, pane, plugin},
};

const SESSION: &str = "11111111-2222-4333-8444-555555555555";
const TAKEN: &str = "Claude Code is already running this session elsewhere in Wings. Open it again here once it has finished.";

fn tmp() -> PathBuf {
    std::env::temp_dir().canonicalize().unwrap()
}

/// One of Wings' own plugins that may run `script` with `sh`, in a fake app whose terminal server listens at `server`.
fn rig(script: &str, server: &Path) -> Rig {
    rig_of(script, server, true)
}

fn rig_of(script: &str, server: &Path, builtin: bool) -> Rig {
    let manifest = json!({ "permissions": { "processes": [format!("sh {script} [--resume --fork-session]")] } });
    let rig = Rig::with(plugin(manifest), builtin, vec![pane("1", "/a")]);
    *rig.env.projects.lock().unwrap() = vec![tmp()];
    *rig.env.job_server.lock().unwrap() = Some(server.to_owned());
    rig
}

fn new_tab() -> FrameState {
    FrameState::new(StatePlace::Tab, None, |_| Ok(()))
}

/// The tab `tab` as Wings restores it in its next run.
fn restored(tab: &FrameState) -> FrameState {
    new_tab().with_key(Some(tab.key()))
}

fn spawn(token: &str, script: &str, flags: &[&str]) -> Value {
    let args: Vec<&str> = std::iter::once(script).chain(flags.iter().copied()).collect();
    json!({ "token": token, "program": "sh", "args": args, "cwd": tmp(), "survive": true })
}

/// What the frame hears until `count` things have come, lines and answers apart, each in the order it came.
fn hear(caller: &Caller, count: usize) -> (Vec<String>, Vec<Result<Value, String>>) {
    let (mut lines, mut answers) = (Vec::new(), Vec::new());
    for _ in 0..count {
        match caller.next() {
            Heard::Line(line) => lines.push(line),
            Heard::Done(answer) => answers.push(answer),
        }
    }
    (lines, answers)
}

/// A script that prints the first line it reads, then `during` once `flag` is there, then what it reads, as `cat`.
fn waits_for(flag: &Path, name: &str) -> String {
    let _ = std::fs::remove_file(flag);
    let script = tmp().join(format!("wings-job-{name}-{}.sh", std::process::id()));
    std::fs::write(&script, format!("read a; echo \"$a\"; while [ ! -f '{}' ]; do sleep 0.05; done; echo during; cat", flag.display())).unwrap();
    script.to_string_lossy().into_owned()
}

/// A script that runs `cat` and touches `marker` once it has ended.
fn marks_its_end(marker: &Path, name: &str) -> String {
    let _ = std::fs::remove_file(marker);
    let script = tmp().join(format!("wings-job-{name}-{}.sh", std::process::id()));
    std::fs::write(&script, format!("cat; touch '{}'", marker.display())).unwrap();
    script.to_string_lossy().into_owned()
}

fn wait_for_file(file: &Path, what: &str) {
    let start = Instant::now();
    while !file.exists() {
        assert!(start.elapsed() < Duration::from_secs(10), "{what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The page `caller` starts the program `chat` from `script` with `flags`, and it reads its first line.
fn running(caller: &Caller, script: &str, flags: &[&str]) {
    caller.send("spawn", spawn("chat", script, flags));
    caller.send_after_click("processWrite", json!({ "token": "chat", "text": "first\n" }));
}

#[test]
fn a_tabs_program_runs_on_through_a_restart_of_wings_and_the_restored_tab_takes_it_over() {
    let (server, _host) = test_host(Config::default());
    let flag = tmp().join(format!("wings-job-flag-{}", std::process::id()));
    let script = waits_for(&flag, "restart");
    let rig = rig(&script, &server);
    let chat_tab = new_tab();
    let before = Caller::in_tab(&rig.host, chat_tab.clone());
    running(&before, &script, &[&format!("--resume={SESSION}")]);
    assert_eq!(hear(&before, 2), (vec!["first".to_owned()], vec![Ok(Value::Null)]));
    // The terminal server runs it, for this tab.
    let jobs = Control::connect(&server).unwrap().jobs().unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!((&jobs[0].owner["plugin"], &jobs[0].owner["tab"], &jobs[0].owner["session"]), (&json!("probe"), &json!(chat_tab.key()), &json!(SESSION)));
    assert_eq!(before.call("processNote", json!({ "token": "chat", "note": { "turn": 1 }, "heard": 1 })), Ok(Value::Null));
    // Wings quits and starts again; the program prints while no Wings hears it.
    let next = rig.next_app(Duration::from_secs(30));
    drop(before);
    std::fs::write(&flag, "").unwrap();
    let after = Caller::in_tab(&next, restored(&chat_tab));
    let start = Instant::now();
    let left = loop {
        let left = after.call("processes", json!({})).unwrap();
        if left[0]["kept"] == 1 {
            break left[0].clone();
        }
        assert!(start.elapsed() < Duration::from_secs(10), "the line printed during the restart never was kept: {left}");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!((&left["token"], &left["program"], &left["note"], &left["session"]), (&json!("chat"), &json!("sh"), &json!({ "turn": 1 }), &json!(SESSION)));
    // Another tab finds nothing to take over, can't take it, and can't start a second Claude on its session.
    let elsewhere = Caller::in_tab(&next, new_tab());
    assert_eq!(elsewhere.call("processes", json!({})), Ok(json!([])));
    assert_eq!(elsewhere.call("processes", json!({ "session": SESSION })), Ok(json!([{ "elsewhere": true, "session": SESSION }])));
    assert!(elsewhere.call("adopt", json!({ "from": left["id"], "token": "theirs" })).is_err());
    assert_eq!(elsewhere.call("spawn", spawn("chat", &script, &[&format!("--resume={SESSION}")])), Err(TAKEN.into()));
    after.send("adopt", json!({ "from": left["id"], "token": "mine" }));
    // What came while Wings restarted, and not what the page before had.
    assert_eq!(hear(&after, 1).0, ["during"]);
    // Taking it over is no write: writing still needs your click.
    let unasked = json!({ "token": "mine", "text": "x\n" });
    assert_eq!(after.call("processWrite", unasked), Err("A page can write to a program only right after you click or type in it".into()));
    after.send_after_click("processWrite", json!({ "token": "mine", "text": "again\n" }));
    assert_eq!(hear(&after, 2), (vec!["again".to_owned()], vec![Ok(Value::Null)]));
    after.send("processStop", json!({ "token": "mine" }));
    assert_eq!(hear(&after, 2).1, [Ok(Value::Null), Ok(json!({ "code": 0, "stderr": "" }))]);
    std::fs::remove_file(&flag).unwrap();
    std::fs::remove_file(&script).unwrap();
}

#[test]
fn a_program_no_restored_tab_takes_over_is_stopped_after_the_wait() {
    let (server, _host) = test_host(Config::default());
    let marker = tmp().join(format!("wings-job-unclaimed-{}", std::process::id()));
    let script = marks_its_end(&marker, "unclaimed");
    let rig = rig(&script, &server);
    let page = Caller::in_tab(&rig.host, new_tab());
    running(&page, &script, &[]);
    hear(&page, 2);
    let _next = rig.next_app(Duration::from_millis(300));
    drop(page);
    std::thread::sleep(Duration::from_millis(100));
    assert!(!marker.exists(), "it waits for its tab");
    wait_for_file(&marker, "the program kept running though no tab took it over");
    std::fs::remove_file(&marker).unwrap();
    std::fs::remove_file(&script).unwrap();
}

#[test]
fn closing_the_tab_stops_its_program_in_the_terminal_server() {
    let (server, _host) = test_host(Config::default());
    let marker = tmp().join(format!("wings-job-closed-{}", std::process::id()));
    let script = marks_its_end(&marker, "closed");
    let rig = rig(&script, &server);
    let page = Caller::in_tab(&rig.host, new_tab());
    running(&page, &script, &[]);
    hear(&page, 2);
    assert_eq!(Control::connect(&server).unwrap().jobs().unwrap().len(), 1, "the terminal server runs it");
    drop(page);
    wait_for_file(&marker, "the program kept running after its tab closed");
    std::fs::remove_file(&marker).unwrap();
    std::fs::remove_file(&script).unwrap();
}

#[test]
fn a_second_program_on_a_session_that_runs_is_refused_until_the_first_stops() {
    let (server, _host) = test_host(Config::default());
    let marker = tmp().join(format!("wings-job-session-{}", std::process::id()));
    let script = marks_its_end(&marker, "session");
    let rig = rig(&script, &server);
    let resume = format!("--resume={SESSION}");
    let first = Caller::in_tab(&rig.host, new_tab());
    running(&first, &script, &[&resume]);
    hear(&first, 2);
    let second = Caller::in_tab(&rig.host, new_tab());
    assert_eq!(second.call("processes", json!({ "session": SESSION })), Ok(json!([{ "elsewhere": true, "session": SESSION }])));
    assert_eq!(second.call("spawn", spawn("chat", &script, &[&resume])), Err(TAKEN.into()));
    // A fork starts a session of its own.
    second.send("spawn", spawn("fork", &script, &[&resume, "--fork-session"]));
    second.send("processStop", json!({ "token": "fork" }));
    assert!(hear(&second, 2).1.iter().all(Result::is_ok));
    // Once the first is told to stop, the session is free.
    first.send("processStop", json!({ "token": "chat" }));
    hear(&first, 2);
    running(&second, &script, &[&resume]);
    assert_eq!(hear(&second, 2), (vec!["first".to_owned()], vec![Ok(Value::Null)]));
    // A new chat's session is known once its page says it.
    let other = "22222222-2222-4333-8444-555555555555";
    let third = Caller::in_tab(&rig.host, new_tab());
    running(&third, &script, &[]);
    hear(&third, 2);
    assert_eq!(third.call("processNote", json!({ "token": "chat", "note": null, "session": other })), Ok(Value::Null));
    assert_eq!(Caller::in_tab(&rig.host, new_tab()).call("spawn", spawn("chat", &script, &[&format!("--resume={other}")])), Err(TAKEN.into()));
    second.send("processStop", json!({ "token": "chat" }));
    third.send("processStop", json!({ "token": "chat" }));
    hear(&second, 2);
    hear(&third, 2);
    let _ = std::fs::remove_file(&marker);
    std::fs::remove_file(&script).unwrap();
}

#[test]
fn a_program_runs_in_the_app_when_no_terminal_server_can_run_it_its_page_is_in_no_tab_or_its_plugin_isnt_wings_own() {
    let (server, _host) = test_host(Config::default());
    let marker = tmp().join(format!("wings-job-here-{}", std::process::id()));
    let script = marks_its_end(&marker, "here");
    let rig = rig(&script, &server);
    // A page in no tab has nothing to come back to.
    let loose = Caller::start(&rig.host);
    running(&loose, &script, &[]);
    assert_eq!(hear(&loose, 2), (vec!["first".to_owned()], vec![Ok(Value::Null)]));
    assert!(Control::connect(&server).unwrap().jobs().unwrap().is_empty());
    // Another plugin's program ends with Wings.
    let other = rig_of(&script, &server, false);
    let theirs = Caller::in_tab(&other.host, new_tab());
    running(&theirs, &script, &[]);
    assert_eq!(hear(&theirs, 2), (vec!["first".to_owned()], vec![Ok(Value::Null)]));
    assert!(Control::connect(&server).unwrap().jobs().unwrap().is_empty());
    theirs.send("processStop", json!({ "token": "chat" }));
    hear(&theirs, 2);
    // No server answers: the app runs it, as it did before.
    *rig.env.job_server.lock().unwrap() = Some(tmp().join("no-such-server.sock"));
    let page = Caller::in_tab(&rig.host, new_tab());
    running(&page, &script, &[]);
    assert_eq!(hear(&page, 2), (vec!["first".to_owned()], vec![Ok(Value::Null)]));
    assert!(Control::connect(&server).unwrap().jobs().unwrap().is_empty());
    loose.send("processStop", json!({ "token": "chat" }));
    page.send("processStop", json!({ "token": "chat" }));
    hear(&loose, 2);
    hear(&page, 2);
    let _ = std::fs::remove_file(&marker);
    std::fs::remove_file(&script).unwrap();
}

#[test]
fn a_tabs_program_runs_in_a_new_terminal_server_once_the_one_it_had_stopped_answering() {
    // A server that was killed leaves its socket behind with nobody listening, as it did on 2026-11-23.
    let gone = tmp().join(format!("wings-job-gone-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&gone);
    drop(std::os::unix::net::UnixListener::bind(&gone).unwrap());
    let (server, _host) = test_host(Config::default());
    let marker = tmp().join(format!("wings-job-replaced-{}", std::process::id()));
    let script = marks_its_end(&marker, "replaced");
    let rig = rig(&script, &gone);
    *rig.env.job_server_after.lock().unwrap() = Some(server.clone());
    let page = Caller::in_tab(&rig.host, new_tab());
    running(&page, &script, &[]);
    assert_eq!(hear(&page, 2), (vec!["first".to_owned()], vec![Ok(Value::Null)]));
    // The app was told that server was gone, and the new one runs the program, so it outlives Wings.
    assert_eq!(*rig.env.job_server_gone.lock().unwrap(), vec![gone.clone()]);
    assert_eq!(Control::connect(&server).unwrap().jobs().unwrap().len(), 1, "the new terminal server runs it");
    page.send("processStop", json!({ "token": "chat" }));
    hear(&page, 2);
    let _ = std::fs::remove_file(&marker);
    let _ = std::fs::remove_file(&gone);
    std::fs::remove_file(&script).unwrap();
}
