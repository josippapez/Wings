//! `spawn`, `processWrite` and `processStop`: a page keeps a program running and talks to it.

use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::page::plugin::testing::{Caller, Heard, Rig, pane};

fn tmp() -> std::path::PathBuf {
    std::env::temp_dir().canonicalize().unwrap()
}

fn rig(processes: &[&str]) -> Rig {
    let rig = Rig::new(json!({ "permissions": { "processes": processes } }), vec![pane("1", "/a")]);
    *rig.env.projects.lock().unwrap() = vec![tmp()];
    rig
}

fn spawn(token: &str, program: &str, args: &[&str]) -> Value {
    json!({ "token": token, "program": program, "args": args, "cwd": tmp() })
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

#[test]
fn a_page_writes_to_a_program_hears_its_lines_and_hears_when_it_ends() {
    let rig = rig(&["cat []"]);
    let caller = Caller::start(&rig.host);
    caller.send("spawn", spawn("chat", "cat", &[]));
    caller.send_after_click("processWrite", json!({ "token": "chat", "text": "{\"type\":\"user\"}\nplain\n" }));
    let (lines, answers) = hear(&caller, 3);
    assert_eq!(lines, ["{\"type\":\"user\"}", "plain"]);
    assert_eq!(answers, [Ok(Value::Null)]);
    caller.send("processStop", json!({ "token": "chat" }));
    let (lines, answers) = hear(&caller, 2);
    assert!(lines.is_empty());
    assert_eq!(answers, [Ok(Value::Null), Ok(json!({ "code": 0, "stderr": "" }))]);
    assert_eq!(
        caller.call_after_click("processWrite", json!({ "token": "chat", "text": "late\n" })),
        Err("No program is running as chat".into())
    );
}

#[test]
fn a_program_reads_what_it_was_started_with_before_anything_written_later() {
    let rig = rig(&["cat []"]);
    let caller = Caller::start(&rig.host);
    let mut start = spawn("chat", "cat", &[]);
    start["input"] = json!("{\"subtype\":\"initialize\"}\n");
    caller.send("spawn", start);
    caller.send_after_click("processWrite", json!({ "token": "chat", "text": "after\n" }));
    let (lines, _) = hear(&caller, 3);
    assert_eq!(lines, ["{\"subtype\":\"initialize\"}", "after"]);
    caller.send("processStop", json!({ "token": "chat" }));
}

#[test]
fn a_page_writes_to_a_program_only_right_after_you_click_or_type_in_it() {
    let rig = rig(&["cat []"]);
    let caller = Caller::start(&rig.host);
    caller.send("spawn", spawn("chat", "cat", &[]));
    let unasked = json!({ "token": "chat", "text": "{\"behavior\":\"allow\"}\n" });
    assert_eq!(caller.call("processWrite", unasked.clone()), Err("A page can write to a program only right after you click or type in it".into()));
    // One click lets one write through.
    caller.send_after_click("processWrite", json!({ "token": "chat", "text": "yes\n" }));
    let (lines, answers) = hear(&caller, 2);
    assert_eq!((lines, answers), (vec!["yes".to_owned()], vec![Ok(Value::Null)]));
    assert!(caller.call("processWrite", unasked).is_err());
    caller.send("processStop", json!({ "token": "chat" }));
}

#[test]
fn every_line_reaches_the_page_in_order_however_many_come_at_once() {
    let rig = rig(&["seq 1 5000"]);
    let caller = Caller::start(&rig.host);
    // Far more than the frame's queue holds.
    caller.send("spawn", spawn("count", "seq", &["1", "5000"]));
    let (lines, answers) = hear(&caller, 5001);
    let expected: Vec<String> = (1..=5000).map(|n| n.to_string()).collect();
    assert_eq!(lines, expected);
    assert_eq!(answers[0].as_ref().unwrap()["code"], 0);
}

#[test]
fn a_program_stops_when_its_page_closes() {
    let marker = tmp().join(format!("wings-process-ended-{}", std::process::id()));
    let _ = std::fs::remove_file(&marker);
    let script = tmp().join(format!("wings-process-script-{}.sh", std::process::id()));
    std::fs::write(&script, format!("cat; touch '{}'", marker.display())).unwrap();
    let script = script.to_string_lossy().into_owned();
    let rig = rig(&[&format!("sh {script}")]);
    let caller = Caller::start(&rig.host);
    caller.send("spawn", spawn("long", "sh", &[&script]));
    // Answered once the program reads its input, so it is running.
    caller.send_after_click("processWrite", json!({ "token": "long", "text": "x\n" }));
    let (_, answers) = hear(&caller, 2);
    assert_eq!(answers, [Ok(Value::Null)]);
    drop(caller);
    let start = Instant::now();
    while !marker.exists() {
        assert!(start.elapsed() < Duration::from_secs(5), "the program kept running after its page closed");
        std::thread::sleep(Duration::from_millis(20));
    }
    std::fs::remove_file(&marker).unwrap();
    std::fs::remove_file(&script).unwrap();
}

#[test]
fn a_program_runs_only_as_the_manifest_and_the_token_allow() {
    let rig = rig(&["cat []"]);
    let caller = Caller::start(&rig.host);
    assert_eq!(caller.call("spawn", spawn("x", "sh", &["-c", "echo hi"])), Err("probe may not run sh ".into()));
    assert_eq!(caller.call("spawn", spawn("x", "cat", &["-u"])), Err("probe may not run cat ".into()));
    let mut outside = spawn("x", "cat", &[]);
    outside["cwd"] = json!("/");
    assert!(caller.call("spawn", outside).unwrap_err().contains("isn't inside any of your Wings projects"));
    let mut no_token = spawn("x", "cat", &[]);
    no_token["token"] = json!("");
    assert_eq!(caller.call("spawn", no_token), Err("A program needs a token of 1 to 64 characters".into()));
    // Not listed under processes: exec's list doesn't count.
    let exec_only = Rig::new(json!({ "permissions": { "exec": ["cat"] } }), vec![]);
    *exec_only.env.projects.lock().unwrap() = vec![tmp()];
    assert_eq!(Caller::start(&exec_only.host).call("spawn", spawn("x", "cat", &[])), Err("probe may not run cat ".into()));

    caller.send("spawn", spawn("chat", "cat", &[]));
    caller.send_after_click("processWrite", json!({ "token": "chat", "text": "ready\n" }));
    hear(&caller, 2);
    assert_eq!(caller.call("spawn", spawn("chat", "cat", &[])), Err("A program is already running as chat".into()));
    // Another page's program of the same name is its own.
    let other = Caller::start(&rig.host);
    assert_eq!(other.call_after_click("processWrite", json!({ "token": "chat", "text": "x\n" })), Err("No program is running as chat".into()));
    caller.send("processStop", json!({ "token": "chat" }));
}
