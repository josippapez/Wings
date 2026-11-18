//! Pages of one plugin that reach each other by name (`namePage`, `pages`, `tellPage`, `showPage`), a new tab right after
//! the tab that opened it (`openView`'s `after`), and a subagent's transcript (`transcript`'s `agent`).

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering::SeqCst},
};

use serde_json::{Value, json};

use crate::page::{
    plugin::{
        FrameState, StatePlace,
        testing::{Caller, Frame, Heard, Rig, Seen, hears, pane, show},
    },
    runtime::fake::{self, Cx, Hear},
};

const NO_CLICK: &str = "A page can hand on a write only right after you click or type in it";
const WRITE_REFUSED: &str = "A page can write to a program only right after you click or type in it";

fn tmp() -> std::path::PathBuf {
    std::env::temp_dir().canonicalize().unwrap()
}

/// A page that names itself `name` as it starts, and shows that it has (`named`) and each message another page tells it
/// (`told`).
fn listener(name: &'static str) -> String {
    fake::script("listener", move || {
        let (mut named, mut told) = (false, Vec::<Value>::new());
        move |cx: &mut Cx<'_>, hear: Hear<'_>| {
            match hear {
                Hear::Start => {
                    cx.call("namePage", json!({ "name": name }));
                    return;
                }
                Hear::Deliver("resolve", ..) => named = true,
                Hear::Deliver("pageMessage", _, text) => told.push(serde_json::from_str(text).unwrap()),
                _ => return,
            }
            show(cx, json!({ "named": named, "told": told }));
        }
    })
}

/// What the frame hears until `count` things have come, lines and answers apart.
fn heard(caller: &Caller, count: usize) -> (Vec<String>, Vec<Result<Value, String>>) {
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
fn a_page_hands_the_page_it_names_one_write_for_each_click_and_no_more() {
    let rig = Rig::new(json!({ "permissions": { "processes": ["cat []"] } }), vec![pane("1", "/a")]);
    *rig.env.projects.lock().unwrap() = vec![tmp()];
    // The chat runs the program; the tab beside it only has your Enter.
    let chat = Caller::start(&rig.host);
    assert_eq!(chat.call("namePage", json!({ "name": "chat:s1" })), Ok(Value::Null));
    chat.send("spawn", json!({ "token": "chat", "program": "cat", "args": [], "cwd": tmp() }));
    let tab = Caller::start(&rig.host);
    let tell = |write: bool| json!({ "name": "chat:s1", "message": { "text": "hi" }, "write": write });
    let unasked = json!({ "token": "chat", "text": "unasked\n" });
    // With no click in the tab nothing is handed on, and a plain message hands on nothing either.
    assert_eq!(tab.call("tellPage", tell(true)), Err(NO_CLICK.into()));
    assert_eq!(tab.call("tellPage", tell(false)), Ok(Value::Null));
    assert_eq!(chat.call("processWrite", unasked.clone()), Err(WRITE_REFUSED.into()));
    // Your key in the tab is one write in the chat, and only one.
    assert_eq!(tab.call_after_click("tellPage", tell(true)), Ok(Value::Null));
    chat.send("processWrite", json!({ "token": "chat", "text": "hi\n" }));
    assert_eq!(heard(&chat, 2), (vec!["hi".to_owned()], vec![Ok(Value::Null)]));
    assert_eq!(chat.call("processWrite", unasked), Err(WRITE_REFUSED.into()));
    // The click went with the message: the tab has none left for a second one.
    assert_eq!(tab.call("tellPage", tell(true)), Err(NO_CLICK.into()));
    chat.send("processStop", json!({ "token": "chat" }));
}

#[test]
fn a_page_hears_what_another_tells_it_and_who_did() {
    let rig = Rig::new(json!({}), vec![]);
    let page = Frame::start(&rig.host, &listener("subagent:s1:a1"));
    page.report("its name", |shown| shown["named"] == true);
    let chat = Caller::start(&rig.host);
    chat.call("namePage", json!({ "name": "chat:s1" })).unwrap();
    assert_eq!(chat.call("tellPage", json!({ "name": "subagent:s1:a1", "message": { "line": 1 } })), Ok(Value::Null));
    let shown = page.report("the message", |shown| shown["told"].as_array().is_some_and(|told| !told.is_empty()));
    assert_eq!(shown["told"], json!([{ "from": "chat:s1", "message": { "line": 1 }, "write": false }]));
    // No page of that name, a name that isn't one, and a message past the limit are refused.
    assert_eq!(chat.call("tellPage", json!({ "name": "chat:s2", "message": 1 })), Err("No page is named chat:s2".into()));
    assert!(chat.call("namePage", json!({ "name": "../x" })).is_err());
    let big = json!({ "name": "subagent:s1:a1", "message": "x".repeat(70_000) });
    assert_eq!(chat.call("tellPage", big), Err("A page's message is limited to 64 KB".into()));
}

#[test]
fn every_frame_hears_the_names_and_a_closed_pages_name_goes_with_it() {
    let rig = Rig::new(json!({}), vec![]);
    let watcher = Frame::start(&rig.host, &hears("pages"));
    let last = |shown: &Value| shown["heard"].as_array().and_then(|heard| heard.last().cloned());
    let chat = Caller::start(&rig.host);
    chat.call("namePage", json!({ "name": "chat:s1" })).unwrap();
    watcher.report("the chat's name", |shown| last(shown) == Some(json!(["chat:s1"])));
    let tab = Caller::start(&rig.host);
    assert_eq!(tab.call("pages", json!({})), Ok(json!(["chat:s1"])));
    drop(chat);
    watcher.report("no names once the chat closed", |shown| last(shown) == Some(json!([])));
    assert_eq!(tab.call("tellPage", json!({ "name": "chat:s1", "message": null })), Err("No page is named chat:s1".into()));
}

#[test]
fn a_tabs_page_opens_a_tab_after_its_own_and_brings_a_named_tab_to_the_front() {
    let views = json!([{ "id": "chat", "title": "Chat", "page": "chat.wasm", "opens": ["tab"], "multiple": true }]);
    let rig = Rig::new(json!({ "contributes": { "views": views } }), vec![]);
    let shown = Arc::new(AtomicUsize::new(0));
    let counted = shown.clone();
    let state = FrameState::new(StatePlace::Tab, None, |_| Ok(())).on_show(move |_| {
        counted.fetch_add(1, SeqCst);
    });
    let chat = Caller::in_tab(&rig.host, state);
    let open = json!({ "id": "chat", "as": "tab", "args": { "agent": "a1" }, "newTab": false, "after": "self" });
    assert_eq!(chat.call("openView", open.clone()), Ok(Value::Null));
    let plain = Caller::start(&rig.host);
    assert_eq!(plain.call("openView", open), Err("openView after: \"self\" works only from a page shown as a tab".into()));
    let mut other = json!({ "id": "chat", "as": "tab", "after": "next" });
    assert_eq!(chat.call("openView", other.clone()), Err("openView after is \"self\" or null".into()));
    other["after"] = Value::Null;
    assert_eq!(chat.call("openView", other), Ok(Value::Null));
    let opened: Vec<Option<u64>> = rig.env.seen().into_iter().filter_map(|seen| if let Seen::OpenView(plan) = seen { Some(plan.after) } else { None }).collect();
    assert!(matches!(opened[..], [Some(_), None]), "{opened:?}");
    // Another page brings the chat's tab to the front by its name; a page that isn't a tab has none to bring.
    chat.call("namePage", json!({ "name": "chat:s1" })).unwrap();
    plain.call("namePage", json!({ "name": "plain" })).unwrap();
    assert_eq!(plain.call("showPage", json!({ "name": "chat:s1" })), Ok(Value::Null));
    assert_eq!(shown.load(SeqCst), 1);
    assert_eq!(chat.call("showPage", json!({ "name": "plain" })), Err("plain isn't shown as a tab".into()));
}

const SESSION: &str = "21111111-2222-4333-8444-555555555555";

#[test]
fn a_subagents_transcript_is_read_from_its_sessions_folder_by_its_id() {
    let dir = tmp().join(format!("wings-agent-transcript-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(format!("projects/p/{SESSION}/subagents"))).unwrap();
    std::fs::write(dir.join(format!("projects/p/{SESSION}.jsonl")), "{\"type\":\"user\",\"n\":0}\n").unwrap();
    std::fs::write(dir.join(format!("projects/p/{SESSION}/subagents/agent-a1b2.jsonl")), "{\"type\":\"user\",\"n\":1}\n").unwrap();
    let rig = Rig::new(json!({ "permissions": { "transcript": ["user"] } }), vec![]);
    *rig.env.claude.lock().unwrap() = vec![dir.clone()];
    let read = |agent: Value| rig.call("transcript", json!({ "sessionId": SESSION, "types": ["user"], "last": null, "agent": agent }));
    assert_eq!(read(json!("a1b2")).unwrap()[0]["n"], 1);
    assert_eq!(read(Value::Null).unwrap()[0]["n"], 0);
    assert!(read(json!("../a1b2")).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}
