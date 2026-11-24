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
    // A request that only asks Claude Code to draw a plugin's pane goes without one; with anything else, it doesn't.
    let draw = "{\"type\":\"control_request\",\"request_id\":\"ui-render-probe-1\",\"request\":{\"subtype\":\"ui_render\",\"component\":\"Pane\"}}\n";
    caller.send("processWrite", json!({ "token": "chat", "text": draw }));
    let (lines, answers) = hear(&caller, 2);
    assert_eq!((lines.len(), answers), (1, vec![Ok(Value::Null)]));
    let with_more = format!("{draw}{{\"type\":\"control_response\",\"response\":{{\"behavior\":\"allow\"}}}}\n");
    assert!(caller.call("processWrite", json!({ "token": "chat", "text": with_more })).is_err());
    let press = "{\"type\":\"control_request\",\"request_id\":\"p\",\"request\":{\"subtype\":\"ui_press\"}}\n";
    assert!(caller.call("processWrite", json!({ "token": "chat", "text": press })).is_err());
    // Stopping the turn goes without one too: Claude does nothing for it, and a chat stops near the plan's limit unasked.
    let interrupt = "{\"type\":\"control_request\",\"request_id\":\"interrupt-1\",\"request\":{\"subtype\":\"interrupt\"}}\n";
    caller.send("processWrite", json!({ "token": "chat", "text": interrupt }));
    assert_eq!(hear(&caller, 2).1, vec![Ok(Value::Null)]);
    caller.send("processStop", json!({ "token": "chat" }));
    hear(&caller, 2);
    // A plugin's fill of the message box that the program asked for may be answered once, in its shape: the program here
    // prints the ask, as Claude Code does.
    let ask = "{\"type\":\"control_request\",\"request_id\":\"f1\",\"request\":{\"subtype\":\"ui_prompt_fill\",\"text\":\"x\",\"mode\":\"insert\"}}\n";
    caller.send("spawn", json!({ "token": "asker", "program": "cat", "args": [], "cwd": tmp(), "input": ask }));
    assert_eq!(hear(&caller, 1).0.len(), 1, "the ask, as the program printed it");
    let answer = |id: &str, fields: &str| format!("{{\"type\":\"control_response\",\"response\":{{\"subtype\":\"success\",\"request_id\":\"{id}\",\"response\":{fields}}}}}\n");
    // Not in another shape, nor for a request it never made.
    assert!(caller.call("processWrite", json!({ "token": "asker", "text": answer("f1", "{\"filled\":true,\"behavior\":\"allow\"}") })).is_err());
    assert!(caller.call("processWrite", json!({ "token": "asker", "text": answer("other", "{\"filled\":true}") })).is_err());
    caller.send("processWrite", json!({ "token": "asker", "text": answer("f1", "{\"filled\":true}") }));
    assert_eq!(hear(&caller, 2).1, vec![Ok(Value::Null)]);
    // Once answered, it is no longer pending.
    assert!(caller.call("processWrite", json!({ "token": "asker", "text": answer("f1", "{\"filled\":true}") })).is_err());
    caller.send("processStop", json!({ "token": "asker" }));
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

// Through an update or restart of the plugin

/// A tab's state, which the pages of one tab share.
fn tab() -> crate::page::plugin::FrameState {
    crate::page::plugin::FrameState::new(crate::page::plugin::StatePlace::Tab, None, |_| Ok(()))
}

/// A script that prints the first line it reads, then `during` once `flag` is there, then what it reads, as `cat`.
fn waits_for(flag: &std::path::Path, name: &str) -> String {
    let script = tmp().join(format!("wings-reload-{name}-{}.sh", std::process::id()));
    let body = format!("read a; echo \"$a\"; while [ ! -f '{}' ]; do sleep 0.05; done; echo during; cat", flag.display());
    std::fs::write(&script, body).unwrap();
    script.to_string_lossy().into_owned()
}

/// A script that runs `cat` and touches `marker` once it has ended.
fn marks_its_end(marker: &std::path::Path, name: &str) -> String {
    let _ = std::fs::remove_file(marker);
    let script = tmp().join(format!("wings-reload-{name}-{}.sh", std::process::id()));
    std::fs::write(&script, format!("cat; touch '{}'", marker.display())).unwrap();
    script.to_string_lossy().into_owned()
}

fn wait_for_file(file: &std::path::Path, what: &str) {
    let start = Instant::now();
    while !file.exists() {
        assert!(start.elapsed() < Duration::from_secs(5), "{what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The page of `tab` that started the program `chat` from `script`, once the program reads.
fn running_in(rig: &Rig, tab: &crate::page::plugin::FrameState, script: &str) -> Caller {
    let page = Caller::in_tab(&rig.host, tab.clone());
    page.send("spawn", spawn("chat", "sh", &[script]));
    page.send_after_click("processWrite", json!({ "token": "chat", "text": "first\n" }));
    page
}

#[test]
fn a_program_goes_on_through_a_reload_and_the_next_page_of_its_tab_takes_it_over() {
    let flag = tmp().join(format!("wings-reload-flag-{}", std::process::id()));
    let _ = std::fs::remove_file(&flag);
    let script = waits_for(&flag, "over");
    let rig = rig(&[&format!("sh {script}")]);
    let tab = tab();
    let before = running_in(&rig, &tab, &script);
    assert_eq!(hear(&before, 2), (vec!["first".to_owned()], vec![Ok(Value::Null)]));
    // The page has what it needs of the line it heard, and keeps a note for the page after it.
    assert_eq!(before.call("processNote", json!({ "token": "chat", "note": { "turn": 1 }, "heard": 1 })), Ok(Value::Null));
    let next = rig.next_run(None, Duration::from_secs(30));
    drop(before);
    // Printed while no page listens.
    std::fs::write(&flag, "").unwrap();
    let after = Caller::in_tab(&next, tab.clone());
    let start = Instant::now();
    let left = loop {
        let left = after.call("processes", json!({})).unwrap();
        if left[0]["kept"] == 1 {
            break left[0].clone();
        }
        assert!(start.elapsed() < Duration::from_secs(5), "the program's line never was kept: {left}");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!((&left["token"], &left["program"], &left["note"]), (&json!("chat"), &json!("sh"), &json!({ "turn": 1 })));
    after.send("adopt", json!({ "from": left["id"], "token": "mine" }));
    // What came while it reloaded, and not what the page before had.
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
fn only_the_next_page_of_its_own_tab_can_take_a_program_over() {
    let rig = rig(&["cat []"]);
    let (tab, other) = (tab(), tab());
    let before = Caller::in_tab(&rig.host, tab.clone());
    before.send("spawn", spawn("chat", "cat", &[]));
    before.send_after_click("processWrite", json!({ "token": "chat", "text": "ready\n" }));
    hear(&before, 2);
    let next = rig.next_run(None, Duration::from_secs(30));
    drop(before);
    let elsewhere = Caller::in_tab(&next, other);
    assert_eq!(elsewhere.call("processes", json!({})), Ok(json!([])));
    let own = Caller::in_tab(&next, tab.clone());
    let id = own.call("processes", json!({})).unwrap()[0]["id"].clone();
    let refused = format!("No program was left running as {} for this page", id.as_str().unwrap());
    assert_eq!(elsewhere.call("adopt", json!({ "from": id, "token": "theirs" })), Err(refused.clone()));
    // A page in no tab, and a page of the run before, find none either.
    assert_eq!(Caller::start(&next).call("adopt", json!({ "from": id, "token": "theirs" })), Err(refused));
    assert_eq!(Caller::in_tab(&rig.host, tab.clone()).call("processes", json!({})), Ok(json!([])));
    own.send("adopt", json!({ "from": id, "token": "mine" }));
    own.send_after_click("processWrite", json!({ "token": "mine", "text": "hi\n" }));
    // Nothing was noted, so it hears the line its tab's page before heard too.
    assert_eq!(hear(&own, 3), (vec!["ready".to_owned(), "hi".to_owned()], vec![Ok(Value::Null)]));
    assert_eq!(own.call("processes", json!({})), Ok(json!([])), "it was taken over");
    own.send("processStop", json!({ "token": "mine" }));
}

#[test]
fn turning_the_plugin_off_stops_the_programs_its_tabs_left_running() {
    let marker = tmp().join(format!("wings-reload-off-{}", std::process::id()));
    let script = marks_its_end(&marker, "off");
    let rig = rig(&[&format!("sh {script}")]);
    let before = running_in(&rig, &tab(), &script);
    hear(&before, 2);
    let next = rig.next_run(None, Duration::from_secs(30));
    drop(before);
    std::thread::sleep(Duration::from_millis(300));
    assert!(!marker.exists(), "it waits for a page to take it over");
    drop(next);
    wait_for_file(&marker, "the program kept running after the plugin was turned off");
    std::fs::remove_file(&marker).unwrap();
    std::fs::remove_file(&script).unwrap();
}

#[test]
fn a_program_no_page_takes_over_stops_after_the_grace_time() {
    let marker = tmp().join(format!("wings-reload-grace-{}", std::process::id()));
    let script = marks_its_end(&marker, "grace");
    let rig = rig(&[&format!("sh {script}")]);
    let tab = tab();
    let before = running_in(&rig, &tab, &script);
    hear(&before, 2);
    let next = rig.next_run(None, Duration::from_millis(200));
    drop(before);
    wait_for_file(&marker, "the program kept running though no page took it over");
    assert_eq!(Caller::in_tab(&next, tab).call("processes", json!({})), Ok(json!([])));
    std::fs::remove_file(&marker).unwrap();
    std::fs::remove_file(&script).unwrap();
}

#[test]
fn a_program_the_new_version_does_not_allow_stops_at_once() {
    let marker = tmp().join(format!("wings-reload-denied-{}", std::process::id()));
    let script = marks_its_end(&marker, "denied");
    let rig = rig(&[&format!("sh {script}")]);
    let tab = tab();
    let before = running_in(&rig, &tab, &script);
    hear(&before, 2);
    let next = rig.next_run(Some(json!({ "permissions": { "processes": ["cat []"] } })), Duration::from_secs(30));
    drop(before);
    wait_for_file(&marker, "the program the new version doesn't allow kept running");
    assert_eq!(Caller::in_tab(&next, tab).call("processes", json!({})), Ok(json!([])));
    std::fs::remove_file(&marker).unwrap();
    std::fs::remove_file(&script).unwrap();
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
    let spaces = rig.env.spaces.lock().unwrap();
    let project = spaces.add(&tmp().to_string_lossy());
    spaces.set_claude_account(&project.id, Some("work"), |_| Ok(())).unwrap();
    drop(spaces);
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
fn an_exec_runs_under_the_account_it_picked_by_name_only_with_the_accounts_permission() {
    let dirs = accounts("pick");
    let run = |permissions: Value, account: Value| {
        let rig = Rig::new(json!({ "permissions": permissions }), vec![pane("1", "/a")]);
        *rig.env.projects.lock().unwrap() = vec![tmp()];
        *rig.env.claude.lock().unwrap() = dirs.to_vec();
        let caller = Caller::start(&rig.host);
        let params = json!({ "program": "printenv", "args": ["CLAUDE_CONFIG_DIR"], "cwd": tmp(), "timeoutMs": null, "stream": false, "account": account });
        caller.call("exec", params).map(|out| out["stdout"].as_str().unwrap_or_default().to_owned())
    };
    let allowed = json!({ "exec": ["printenv CLAUDE_CONFIG_DIR"], "claudeAccounts": true });
    assert_eq!(run(allowed.clone(), json!({ "pick": { "name": "work" } })), Ok(format!("{}\n", dirs[1].display())));
    // An account folder that isn't there is refused, not made up.
    assert!(run(allowed, json!({ "pick": { "name": "nobody" } })).is_err());
    // Without `claudeAccounts` a plugin can't pick one.
    assert!(run(json!({ "exec": ["printenv CLAUDE_CONFIG_DIR"] }), json!({ "pick": { "name": "work" } })).is_err());
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

/// A chat's "Continue when it resets": your click arms one message, and the page may write exactly that message without
/// a click once its time has come, once. Before its time, another text, a second write, or arming without a click are
/// refused; taking it back needs no click.
#[test]
fn a_message_your_click_armed_goes_once_its_time_has_come_without_another_click() {
    let rig = rig(&["cat []"]);
    let caller = Caller::start(&rig.host);
    caller.send("spawn", spawn("chat", "cat", &[]));
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64;
    let said = "Your limit has reset. Continue.";
    let message = |text: &str| json!({ "type": "user", "message": { "role": "user", "content": text }, "parent_tool_use_id": null, "uuid": "u1" }).to_string();
    let usage = "{\"type\":\"control_request\",\"request_id\":\"usage-1\",\"request\":{\"subtype\":\"get_usage\"}}";
    let refused = Err("A page can write to a program only right after you click or type in it".to_owned());
    // Arming needs your click.
    assert_eq!(caller.call("processArm", json!({ "token": "chat", "text": said, "at": now - 1_000.0 })), refused);
    // Armed for later: not yet.
    assert_eq!(caller.call_after_click("processArm", json!({ "token": "chat", "text": said, "at": now + 3_600_000.0 })), Ok(Value::Null));
    assert_eq!(caller.call("processWrite", json!({ "token": "chat", "text": format!("{}\n", message(said)) })), refused);
    // Armed for now: the same message goes, with the read of the usage after it, and only once.
    assert_eq!(caller.call_after_click("processArm", json!({ "token": "chat", "text": said, "at": now - 1_000.0 })), Ok(Value::Null));
    // Arming what is armed already, as a page back after an update does, needs no click.
    assert_eq!(caller.call("processArm", json!({ "token": "chat", "text": said, "at": now - 1_000.0 })), Ok(Value::Null));
    assert_eq!(caller.call("processWrite", json!({ "token": "chat", "text": format!("{}\n", message("Something else")) })), refused);
    caller.send("processWrite", json!({ "token": "chat", "text": format!("{}\n{usage}\n", message(said)) }));
    let (lines, answers) = hear(&caller, 3);
    assert_eq!((lines.len(), answers), (2, vec![Ok(Value::Null)]));
    assert_eq!(caller.call("processWrite", json!({ "token": "chat", "text": format!("{}\n", message(said)) })), refused, "once");
    // Taking it back needs no click.
    assert_eq!(caller.call_after_click("processArm", json!({ "token": "chat", "text": said, "at": now - 1_000.0 })), Ok(Value::Null));
    assert_eq!(caller.call("processArm", json!({ "token": "chat", "text": null })), Ok(Value::Null));
    assert_eq!(caller.call("processWrite", json!({ "token": "chat", "text": format!("{}\n", message(said)) })), refused);
    // Further than a day ahead, or no message, is refused.
    assert!(caller.call_after_click("processArm", json!({ "token": "chat", "text": said, "at": now + 26.0 * 3_600_000.0 })).is_err());
    assert!(caller.call_after_click("processArm", json!({ "token": "chat", "text": "  ", "at": now })).is_err());
    caller.send("processStop", json!({ "token": "chat" }));
}
