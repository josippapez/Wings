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

const SESSION: &str = "11111111-2222-4333-8444-555555555555";

/// Prints the account folder the program runs under.
fn prints_account(name: &str) -> String {
    let script = tmp().join(format!("wings-account-{name}-{}.sh", std::process::id()));
    std::fs::write(&script, "echo \"dir:$CLAUDE_CONFIG_DIR\"").unwrap();
    script.to_string_lossy().into_owned()
}

/// The default account's folder and a named one's holding `SESSION`, in a scratch folder.
fn accounts(name: &str) -> [std::path::PathBuf; 2] {
    let base = tmp().join(format!("wings-spawn-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let dirs = [base.join(".claude"), base.join(".claude-work")];
    std::fs::create_dir_all(dirs[0].join("projects/p")).unwrap();
    std::fs::create_dir_all(dirs[1].join("projects/p")).unwrap();
    std::fs::write(dirs[1].join(format!("projects/p/{SESSION}.jsonl")), "{}\n").unwrap();
    dirs
}

/// What the program printed when spawned in the scratch project with `account`.
fn account_seen(rig: &Rig, script: &str, account: Value) -> String {
    let caller = Caller::start(&rig.host);
    let mut start = spawn("account", "sh", &[script]);
    start["account"] = account;
    caller.send("spawn", start);
    let (lines, _) = hear(&caller, 2);
    lines.into_iter().next().unwrap_or_default()
}

#[test]
fn a_program_runs_under_the_account_of_the_session_it_resumes() {
    let dirs = accounts("session");
    let script = prints_account("session");
    let rig = rig(&[&format!("sh {script}")]);
    *rig.env.claude.lock().unwrap() = dirs.to_vec();
    assert_eq!(account_seen(&rig, &script, json!({ "session": SESSION })), format!("dir:{}", dirs[1].display()));
    std::fs::remove_file(&script).unwrap();
}

#[test]
fn a_new_chat_runs_under_the_account_of_its_project_and_keeps_wings_own_without_one() {
    let dirs = accounts("project");
    let script = prints_account("project");
    let rig = rig(&[&format!("sh {script}")]);
    *rig.env.claude.lock().unwrap() = dirs.to_vec();
    let own = std::env::var("CLAUDE_CONFIG_DIR").unwrap_or_default();
    assert_eq!(account_seen(&rig, &script, json!({})), format!("dir:{own}"), "the project has no account");
    // No `account` at all leaves the environment alone too.
    assert_eq!(account_seen_without(&rig, &script), format!("dir:{own}"));
    rig.env.spaces.lock().unwrap().set_claude_account(&tmp().to_string_lossy(), Some("work"), |_| Ok(())).unwrap();
    let work = wings_spaces::claude_accounts::dir(Some("work"));
    assert_eq!(account_seen(&rig, &script, json!({})), format!("dir:{}", work.display()));
    // A session that has no transcript is a new one.
    assert_eq!(account_seen(&rig, &script, json!({ "session": "99999999-2222-4333-8444-555555555555" })), format!("dir:{}", work.display()));
    std::fs::remove_file(&script).unwrap();
}

fn account_seen_without(rig: &Rig, script: &str) -> String {
    let caller = Caller::start(&rig.host);
    caller.send("spawn", spawn("account", "sh", &[script]));
    hear(&caller, 2).0.into_iter().next().unwrap_or_default()
}

#[test]
fn an_exec_runs_under_the_tabs_account_too() {
    let dirs = accounts("exec");
    let rig = Rig::new(json!({ "permissions": { "exec": ["printenv CLAUDE_CONFIG_DIR"] } }), vec![pane("1", "/a")]);
    *rig.env.projects.lock().unwrap() = vec![tmp()];
    *rig.env.claude.lock().unwrap() = dirs.to_vec();
    let caller = Caller::start(&rig.host);
    let run = |account: Value| {
        let params = json!({ "program": "printenv", "args": ["CLAUDE_CONFIG_DIR"], "cwd": tmp(), "timeoutMs": null, "stream": false, "account": account });
        caller.call("exec", params).unwrap()["stdout"].as_str().unwrap().to_owned()
    };
    assert_eq!(run(json!({ "session": SESSION })), format!("{}\n", dirs[1].display()));
    let raw = json!({ "program": "printenv", "args": ["CLAUDE_CONFIG_DIR"], "cwd": tmp(), "timeoutMs": null, "stream": false, "account": { "dir": "/etc" } });
    assert!(caller.call("exec", raw).is_err());
}

#[test]
fn a_page_cannot_name_an_account_folder_of_its_own() {
    let script = prints_account("raw");
    let rig = rig(&[&format!("sh {script}")]);
    let caller = Caller::start(&rig.host);
    for account in [json!({ "dir": "/etc" }), json!({ "session": null, "configDir": "/etc" }), json!("/etc")] {
        let mut start = spawn("raw", "sh", &[&script]);
        start["account"] = account.clone();
        assert!(caller.call("spawn", start).is_err(), "{account}");
    }
    std::fs::remove_file(&script).unwrap();
}
