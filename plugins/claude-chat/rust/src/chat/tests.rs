use super::{changes_counted, review_event, shortstat};
use super::{State, first_prompt, new_chat, resume, typed, with};
use crate::{commands, session::Command};
use crate::fork::Launch;
use serde_json::{Value, json};
use std::{cell::RefCell, rc::Rc};
use wings_plugin::host;

/// Wings, as far as the chat asks it, keeping every `openView` it is asked for.
fn opened() -> Rc<RefCell<Vec<Value>>> {
    let opened = Rc::new(RefCell::new(Vec::new()));
    let kept = opened.clone();
    host::testing::answer(move |method, params| {
        if method == "openView" {
            kept.borrow_mut().push(params.clone());
        }
        Ok(Value::Null)
    });
    opened
}

fn running(working: bool) -> u32 {
    with(|c| {
        c.cwd = "/work/shop".into();
        c.program = "claude".into();
        c.picked = Some(Some("work".into()));
        c.state = State::Running;
        c.session.working = working;
        c.run
    })
}

/// Claude Code writes the name it makes up for a session (`ai-title`) to the transcript and doesn't print it: once a turn
/// ends the chat reads it there, and its Agents row and tab drop the first message they were named after.
#[test]
fn a_live_chat_takes_the_name_claude_code_wrote_in_its_transcript() {
    let told = Rc::new(RefCell::new(Vec::new()));
    let kept = told.clone();
    host::testing::answer(move |method, params| match method {
        "transcript" => Ok(json!([{ "type": "ai-title", "aiTitle": "Rust code optimization", "sessionId": "s1" }])),
        "setAgent" => {
            kept.borrow_mut().push(params["agent"]["name"].clone());
            Ok(Value::Null)
        }
        _ => Ok(Value::Null),
    });
    let run = running(false);
    with(|c| {
        c.session = Default::default();
        c.session.id = Some("s1".into());
        c.session.items.push(crate::session::Item::User("(Message from the agent in Wings tab tab-30, sent through Wings.) Handoff".into()));
    });
    assert!(with(|c| super::name(c)).is_some_and(|name| name.starts_with("(Message from the agent")), "named after the first message");
    super::read_title(run);
    wings_plugin::task::block_on(async {});
    assert_eq!(with(|c| super::name(c)).as_deref(), Some("Rust code optimization"));
    assert_eq!(told.borrow().last(), Some(&json!("Rust code optimization")));
    with(|c| c.session = Default::default());
}

/// A sidebar running the chat's code is no tab: Wings refuses its save, and that is no failure to show.
#[test]
fn a_page_that_is_no_tab_shows_no_failed_save() {
    host::testing::answer(|method, _| if method == "setTabState" { Err(super::NOT_A_TAB.into()) } else { Ok(Value::Null) });
    with(|c| c.save_error = Some("old".into()));
    super::save_tab();
    wings_plugin::task::block_on(async {});
    assert_eq!(with(|c| c.save_error.clone()), None);
}

#[test]
fn a_comment_that_does_not_fit_is_said_so_and_a_refused_tab_save_is_kept_to_show() {
    host::testing::answer(|method, _| if method == "setTabState" { Err("Tab state is limited to 64 KB".into()) } else { Ok(Value::Null) });
    with(|c| c.review.clear());
    let event = |id: &str, body: &str| json!({ "chat": crate::review::key(), "path": "src/a.rs", "line": 4, "quote": "q", "body": body, "id": id });
    let long = "x".repeat(12_000);
    for n in 0..4 {
        super::review_comment(&event(&format!("1:{n}"), &long));
    }
    wings_plugin::task::block_on(async {});
    assert_eq!(with(|c| c.review.len()), 3, "the fourth was refused");
    assert_eq!(with(|c| c.review_note.clone()).as_deref(), Some(crate::review::FULL));
    assert_eq!(with(|c| c.save_error.clone()).as_deref(), Some("Tab state is limited to 64 KB"));
    super::clear_review();
    assert!(with(|c| c.review_note.is_none()));
    with(|c| c.save_error = None);
}

#[test]
fn opening_a_built_in_shows_its_view_and_asks_only_to_read_without_a_message() {
    use crate::{builtin::Local, plugins::Show, usage::Panel};
    let line = with(|c| {
        c.session.items.clear();
        let line = super::open_builtin(c, Local::Panel(Panel::Usage));
        assert_eq!(c.panel, Some(Panel::Usage));
        assert!(c.session.items.is_empty() && !c.session.working);
        line
    });
    let sent: Value = serde_json::from_str(&line).unwrap();
    assert_eq!((line.lines().count(), sent["type"].as_str(), sent["request"]["subtype"].as_str()), (1, Some("control_request"), Some("get_usage")));
    with(|c| {
        super::open_builtin(c, Local::Dialog(Show::Mcp));
        assert!(c.plugins.is_some());
    });
}

#[test]
fn picking_a_built_in_from_the_slash_list_opens_it_and_any_other_command_is_filled_in_for_its_arguments() {
    let _opened = opened();
    running(false);
    let process = host::spawn("claude", &[], "/work/shop", "", |_| {});
    with(|c| (c.input, c.plugins, c.draft) = (Some(process.input()), None, "/m".into()));
    super::insert_command("mcp");
    with(|c| assert!(c.plugins.is_some() && c.draft.is_empty(), "{:?}", c.draft));
    with(|c| c.plugins = None);
    super::insert_command("review");
    with(|c| assert!(c.plugins.is_none() && c.draft == "/review "));
}

fn command(name: &str) -> Command {
    Command { name: name.into(), description: String::new(), hint: String::new(), plugin: false, builtin: false }
}

fn mid_names() -> Vec<String> {
    with(|c| super::mid_matches(c).map(|(_, found)| found.into_iter().map(|command| command.name).collect()).unwrap_or_default())
}

fn with_commands(draft: &str) {
    with(|c| {
        c.session.commands = vec![command("compact"), command("commit"), command("myplugin:deploy-app")];
        c.session.suggestion = None;
    });
    typed(draft);
}

#[test]
fn a_slash_after_a_space_lists_the_commands_by_prefix_or_bare_name_and_a_path_closes_the_list() {
    with_commands("run the tests, then /com");
    assert_eq!(mid_names(), ["compact", "commit"]);
    typed("run /deploy");
    assert_eq!(mid_names(), ["myplugin:deploy-app"]);
    typed("see /tmp/notes.md");
    assert!(mid_names().is_empty());
    // A leading `/` keeps its own list, and a bare mid `/` lists nothing until Tab.
    typed("/com");
    assert!(mid_names().is_empty() && !with(|c| super::command_matches(c).is_empty()));
    typed("run /");
    assert!(mid_names().is_empty());
    assert!(with(|c| super::takes_tab(c)));
}

#[test]
fn tab_inserts_the_top_match_with_its_full_name_and_enter_sends_the_message_as_typed() {
    running(false);
    with_commands("then /deploy");
    super::tab();
    assert_eq!(with(|c| c.draft.clone()), "then /myplugin:deploy-app ");
}

#[test]
fn tab_on_a_bare_slash_lists_every_command_and_inserting_only_writes_the_text() {
    with_commands("then /");
    super::tab();
    assert_eq!(with(|c| c.draft.clone()), "then /");
    assert_eq!(mid_names().len(), 3 + commands::own_commands(&with(|c| c.session.commands.clone())).count());
    super::insert_mid_command("commit");
    assert_eq!(with(|c| c.draft.clone()), "then /commit ");
    assert!(with(|c| c.plugins.is_none() && c.panel.is_none()));
}

#[test]
fn a_key_in_a_mid_message_slash_word_redraws_the_page_and_other_words_do_not() {
    with_commands("hello world");
    assert!(typed("hello /c"));
    assert!(typed("hello /co"));
    assert!(typed("hello "));
    assert!(!typed("hello wor"));
}

#[test]
fn the_failed_banner_is_one_sentence_without_a_doubled_error() {
    let said = super::why(&json!({ "code": 1, "stderr": "Error: Invalid API key · Please run /login\n" }));
    assert_eq!(said, "Claude Code stopped: Invalid API key. Please run /login");
}

#[test]
fn a_new_chat_while_a_turn_runs_opens_in_a_new_tab_and_leaves_this_chat_running() {
    let opened = opened();
    let run = running(true);
    new_chat();
    assert_eq!(with(|c| (c.run, c.state == State::Running)), (run, true));
    assert_eq!(
        *opened.borrow(),
        [json!({ "id": "chat", "as": "tab", "args": { "cwd": "/work/shop", "program": "claude", "account": { "name": "work" } }, "newTab": true, "after": "self" })]
    );
}

#[test]
fn a_resume_while_a_turn_runs_opens_in_a_new_tab_too() {
    let opened = opened();
    let run = running(true);
    resume("s1");
    assert_eq!(with(|c| c.run), run);
    assert_eq!(opened.borrow()[0]["args"]["resume"], "s1");
    assert_eq!(opened.borrow()[0]["newTab"], true);
}

/// Wings, answering `openHistory` with `picked` and keeping every call.
fn wings_picks(picked: Value) -> Rc<RefCell<Vec<(String, Value)>>> {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let kept = calls.clone();
    host::testing::answer(move |method, params| {
        kept.borrow_mut().push((method.to_owned(), params.clone()));
        Ok(if method == "openHistory" { picked.clone() } else { Value::Null })
    });
    calls
}

fn asked(calls: &Rc<RefCell<Vec<(String, Value)>>>, method: &str) -> Vec<Value> {
    calls.borrow().iter().filter(|(called, _)| called == method).map(|(_, params)| params.clone()).collect()
}

#[test]
fn past_sessions_opens_wings_own_sheet_and_a_session_picked_there_goes_on_in_this_chat() {
    use super::open_past;
    let calls = wings_picks(json!({ "resume": "s2", "cwd": "/work/shop" }));
    let run = running(false);
    open_past();
    wings_plugin::task::block_on(async {});
    assert_eq!(asked(&calls, "openHistory"), [json!({ "resumeIn": "this tab" })]);
    // Here, as the chat is idle.
    assert_eq!(with(|c| c.run), run + 1);
    assert!(asked(&calls, "openView").is_empty());
    // While a turn runs, beside this chat.
    let calls = wings_picks(json!({ "resume": "s2", "cwd": "/work/shop" }));
    let run = running(true);
    open_past();
    wings_plugin::task::block_on(async {});
    assert_eq!(with(|c| c.run), run);
    assert_eq!(asked(&calls, "openView")[0]["args"]["resume"], "s2");
}

#[test]
fn a_past_session_of_another_folder_goes_on_in_a_new_chat_in_that_folder() {
    use super::open_past;
    let calls = wings_picks(json!({ "resume": "s3", "cwd": "/work/shop/api" }));
    let run = running(false);
    open_past();
    wings_plugin::task::block_on(async {});
    assert_eq!(with(|c| c.run), run);
    let opened = asked(&calls, "openView");
    assert_eq!((&opened[0]["args"]["resume"], &opened[0]["args"]["cwd"]), (&json!("s3"), &json!("/work/shop/api")));
}

#[test]
fn a_new_chat_while_idle_starts_here_and_opens_no_tab() {
    let opened = opened();
    let run = running(false);
    new_chat();
    assert_eq!(with(|c| c.run), run + 1);
    assert!(opened.borrow().is_empty());
}

#[test]
fn a_message_from_another_agent_fills_the_message_box_and_is_sent_only_when_wings_says_so() {
    use super::take_agent_message;
    let calls = Rc::new(RefCell::new(Vec::new()));
    let kept = calls.clone();
    host::testing::answer(move |method, params| {
        kept.borrow_mut().push((method.to_owned(), params.clone()));
        Ok(Value::Null)
    });
    running(false);
    with(|c| (c.input, c.draft) = (None, String::new()));
    assert!(take_agent_message("hi", true).unwrap_err().contains("isn't running"));
    let process = host::spawn("claude", &[], "/work/shop", "", |_| {});
    with(|c| c.input = Some(process.input()));
    // Without "Let agents message each other" it waits in the message box for you.
    assert_eq!(take_agent_message("(Message from another agent)\nReview it.", false), Ok(false));
    assert_eq!(with(|c| c.draft.clone()), "(Message from another agent)\nReview it.");
    // What is in the box stays yours.
    assert!(take_agent_message("Run the tests.", true).unwrap_err().contains("hasn't sent it"));
    with(|c| c.draft.clear());
    // With it, it is sent as your message, in one write.
    assert_eq!(take_agent_message("Run the tests.", true), Ok(true));
    wings_plugin::task::block_on(async {});
    let writes: Vec<String> = calls.borrow().iter().filter(|(method, _)| method == "processWrite").map(|(_, params)| params["text"].as_str().unwrap().to_owned()).collect();
    assert_eq!(writes.len(), 1, "{writes:?}");
    assert!(writes[0].contains("Run the tests."), "{}", writes[0]);
    let relayed: Value = serde_json::from_str(writes[0].lines().next().unwrap()).unwrap();
    assert_eq!((relayed["isSynthetic"].clone(), relayed["client_composed"].clone()), (json!(true), json!(true)), "{}", writes[0]);
    // The flag is for that message only: what you type next is yours.
    with(|c| (c.session.working, c.draft) = (false, "And now this".into()));
    super::send();
    wings_plugin::task::block_on(async {});
    let typed = calls.borrow().iter().filter(|(method, _)| method == "processWrite").map(|(_, params)| params["text"].as_str().unwrap().to_owned()).last().unwrap();
    assert!(typed.contains("And now this") && !typed.contains("isSynthetic"), "{typed}");
    assert!(with(|c| c.draft.is_empty() && c.session.working));
}

/// Comments from the diff viewer for this chat wait above the message box, are kept with the tab, go out with the next
/// message once, and a message with none carries none.
#[test]
fn review_comments_wait_for_the_next_message_which_carries_them_once() {
    use super::{clear_review, remove_review, review_comment, send};
    let calls = Rc::new(RefCell::new(Vec::new()));
    let kept = calls.clone();
    host::testing::answer(move |method, params| {
        kept.borrow_mut().push((method.to_owned(), params.clone()));
        Ok(Value::Null)
    });
    running(false);
    let process = host::spawn("claude", &[], "/work/shop", "", |_| {});
    with(|c| (c.input, c.draft, c.review) = (Some(process.input()), String::new(), Vec::new()));
    let key = crate::review::key();
    let event = |chat: &str, id: &str, line: u64, body: &str| json!({ "chat": chat, "path": "src/a.rs", "side": "new", "line": line, "quote": "let a = 1;", "body": body, "id": id });
    review_comment(&event(&key, "1:-1", 12, "Why one?"));
    review_comment(&event(&key, "1:-2", 14, "And this?"));
    // Another chat's comment isn't ours.
    review_comment(&event("chat-other", "1:-3", 1, "Not for this chat"));
    assert_eq!(with(|c| c.review.len()), 2);
    // The tab keeps them.
    let saved = asked(&calls, "setTabState");
    assert_eq!(saved.last().unwrap()["state"]["reviewComments"].as_array().unwrap().len(), 2);
    // One is taken off, a taken-back draft goes too.
    remove_review(0);
    assert_eq!(with(|c| c.review.iter().map(|c| c.body.clone()).collect::<Vec<_>>()), ["And this?"]);
    review_comment(&event(&key, "1:-4", 20, "Third"));
    review_comment(&json!({ "chat": key, "delete": true, "id": "1:-4" }));
    assert_eq!(with(|c| c.review.len()), 1);
    // With a message typed, it goes out with the comment after it, and the comments are spent.
    with(|c| c.draft = "Fix these".into());
    send();
    wings_plugin::task::block_on(async {});
    let writes: Vec<String> = calls.borrow().iter().filter(|(method, _)| method == "processWrite").map(|(_, params)| params["text"].as_str().unwrap().to_owned()).collect();
    assert_eq!(writes.len(), 1, "{writes:?}");
    assert!(writes[0].contains("Fix these") && writes[0].contains("src/a.rs:14") && writes[0].contains("let a = 1;") && writes[0].contains("And this?"), "{}", writes[0]);
    assert!(with(|c| c.review.is_empty()));
    let saved = asked(&calls, "setTabState");
    assert_eq!(saved.last().unwrap()["state"]["reviewComments"], json!([]));
    // The next message has none of them; and Clear takes them all off.
    review_comment(&event(&key, "2:-1", 1, "Later"));
    clear_review();
    assert!(with(|c| c.review.is_empty()));
    with(|c| c.draft = "Plain".into());
    send();
    wings_plugin::task::block_on(async {});
    let writes: Vec<String> = calls.borrow().iter().filter(|(method, _)| method == "processWrite").map(|(_, params)| params["text"].as_str().unwrap().to_owned()).collect();
    assert_eq!(writes.len(), 2);
    assert!(writes[1].contains("Plain") && !writes[1].contains("Review comments"), "{}", writes[1]);
}

/// A tab that kept comments shows them again after a restart.
#[test]
fn a_restarted_tab_gets_its_unsent_review_comments_back() {
    let saved = json!({ "reviewComments": [{ "id": "1:-1", "path": "src/a.rs", "old": false, "line": 3, "quote": "x", "body": "Why?" }] });
    wings_with_tab(saved.clone());
    wings_plugin::task::block_on(super::open_with(json!({ "cwd": "/work/shop" }), saved));
    assert_eq!(with(|c| c.review.len()), 1);
    assert_eq!(with(|c| c.review[0].place()), "src/a.rs:3");
}

/// Wings answering `tabState` with `saved`, keeping every call.
fn wings_with_tab(saved: Value) -> Rc<RefCell<Vec<(String, Value)>>> {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let kept = calls.clone();
    host::testing::answer(move |method, params| {
        kept.borrow_mut().push((method.to_owned(), params.clone()));
        Ok(if method == "tabState" { saved.clone() } else { Value::Null })
    });
    calls
}

/// What the start of the tab sent Claude Code with it: the `input` of each `spawn`.
fn started_with(calls: &Rc<RefCell<Vec<(String, Value)>>>) -> Vec<String> {
    asked(calls, "spawn").iter().map(|params| params["input"].as_str().unwrap_or_default().to_owned()).collect()
}

fn user_lines(input: &str) -> Vec<Value> {
    input.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()).filter(|line| line["type"] == "user").collect()
}

/// Wings answering `project` with `/work/shown`, `transcript` with entries that ran in `/work/ran`, and `tabState` with `saved`.
fn wings_showing_another_project(saved: Value) -> Rc<RefCell<Vec<(String, Value)>>> {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let kept = calls.clone();
    host::testing::answer(move |method, params| {
        kept.borrow_mut().push((method.to_owned(), params.clone()));
        Ok(match method {
            "tabState" => saved.clone(),
            "project" => json!({ "path": "/work/shown" }),
            "transcript" => json!([{ "type": "user", "message": {} }, { "type": "user", "cwd": "/work/ran", "message": {} }]),
            _ => Value::Null,
        })
    });
    calls
}

#[test]
fn a_restored_tab_without_a_saved_folder_starts_where_its_session_ran_not_in_the_project_on_screen() {
    let saved = json!({ "sessionId": "s1", "dirs": [] });
    let calls = wings_showing_another_project(saved.clone());
    wings_plugin::task::block_on(super::open_with(json!({}), saved));
    assert_eq!(asked(&calls, "spawn")[0]["cwd"], "/work/ran");
    assert_eq!(with(|c| c.cwd.clone()), "/work/ran");
}

#[test]
fn a_saved_folder_wins_over_the_transcript_and_a_new_chat_without_one_starts_in_the_project() {
    let saved = json!({ "sessionId": "s1", "cwd": "/work/saved" });
    let calls = wings_showing_another_project(saved.clone());
    wings_plugin::task::block_on(super::open_with(json!({}), saved));
    assert_eq!(asked(&calls, "spawn")[0]["cwd"], "/work/saved");
    let calls = wings_showing_another_project(json!({}));
    wings_plugin::task::block_on(super::open_with(json!({}), json!({})));
    assert_eq!(asked(&calls, "spawn")[0]["cwd"], "/work/shown");
}

#[test]
fn a_tab_saves_the_folder_it_runs_in() {
    let calls = wings_with_tab(json!({}));
    wings_plugin::task::block_on(super::open_with(json!({ "cwd": "/work/shop" }), json!({})));
    assert_eq!(asked(&calls, "setTabState").last().unwrap()["state"]["cwd"], "/work/shop");
}

/// The dock `Split` of the chat's page and the handlers it came with.
fn dock_export() -> (Value, wings_plugin::ui::Handlers) {
    fn dock_of(node: &Value) -> Option<Value> {
        if node["type"] == "Split" && node["props"]["dock"] == true {
            return Some(node.clone());
        }
        node["children"].as_array()?.iter().find_map(dock_of)
    }
    let (tree, handlers) = wings_plugin::ui::export(with(|c| {
        c.side_in_wings = false;
        vec![crate::view::chat_page(c)]
    }));
    (tree.as_array().unwrap().iter().find_map(dock_of).expect("the dock"), handlers)
}

#[test]
fn dragging_the_docks_edge_keeps_the_width_in_the_tab_and_the_page_draws_it() {
    let calls = wings_with_tab(json!({}));
    with(|c| c.dock_width = None);
    let (dock, handlers) = dock_export();
    assert_eq!((dock["props"]["defaultSize"].as_u64(), dock["props"]["minSize"].as_u64(), dock["props"]["minRest"].as_u64()), (Some(420), Some(280), Some(480)));
    assert!(handlers.call(dock["props"]["onResize"].as_str().unwrap(), &json!({ "size": 560 })));
    assert_eq!(asked(&calls, "setTabState").last().unwrap()["state"]["dockWidth"], 560);
    let (dock, _) = dock_export();
    // The tab folds the column under the conversation where the conversation would have less than its 480.
    assert_eq!((dock["props"]["defaultSize"].as_u64(), dock["props"]["collapseBelow"].as_u64()), (Some(560), Some(1040)));
}

#[test]
fn a_restored_tab_has_its_dock_width_back_within_bounds() {
    for (saved, wanted) in [(json!(640), Some(640)), (json!(50), Some(280)), (json!(9000), Some(1000)), (Value::Null, None)] {
        let state = json!({ "dockWidth": saved });
        wings_with_tab(state.clone());
        wings_plugin::task::block_on(super::open_with(json!({ "cwd": "/work/shop" }), state));
        assert_eq!(with(|c| c.dock_width), wanted, "{saved}");
    }
}

#[test]
fn picking_the_ultracode_row_of_the_model_menu_sends_the_setting_and_shows_it_on() {
    let calls = wings_with_tab(json!({}));
    let process = host::spawn("claude", &[], "/work/shop", "", |_| {});
    with(|c| {
        c.input = Some(process.input());
        c.session.models = vec![crate::session::Model { value: "opus".into(), name: "Opus 5.5".into(), efforts: vec!["high".into()], resolved: "claude-opus-5-5".into(), fast: false, description: String::new() }];
        c.session.options.applied(&json!({ "ultracode": false, "ultracodeAvailable": true }));
    });
    let (tree, handlers) = wings_plugin::ui::export(with(|c| crate::view::pickers(c)));
    let menu = tree.as_array().unwrap().iter().find(|node| node["props"]["label"] == "Model and effort").unwrap();
    assert!(handlers.call(menu["props"]["onChange"].as_str().unwrap(), &json!({ "value": crate::view::ULTRACODE })));
    wings_plugin::task::block_on(async {});
    assert!(with(|c| c.session.options.ultracode));
    let writes: Vec<String> = asked(&calls, "processWrite").iter().map(|params| params["text"].as_str().unwrap_or_default().to_owned()).collect();
    assert!(writes.iter().any(|text| text.contains("apply_flag_settings") && text.contains("\"ultracode\":true")), "{writes:?}");
    let after = wings_plugin::ui::export(with(|c| crate::view::pickers(c))).0;
    assert_eq!(after[0]["props"]["badge"], "Ultracode");
}

#[test]
fn a_prompt_a_coding_agent_opened_the_chat_with_goes_to_claude_code_as_not_yours() {
    let calls = wings_with_tab(json!({}));
    wings_plugin::task::block_on(super::open_with(json!({ "cwd": "/work/shop", "prompt": "Take over the retry work", "relayed": true }), json!({})));
    let inputs = started_with(&calls);
    let sent = user_lines(&inputs[0]);
    assert_eq!(sent.len(), 1, "{}", inputs[0]);
    assert_eq!((sent[0]["isSynthetic"].clone(), sent[0]["client_composed"].clone()), (json!(true), json!(true)), "{}", inputs[0]);
    assert!(with(|c| !c.first_relayed), "only that first message is marked");
}

#[test]
fn a_new_chat_opened_with_a_prompt_sends_it_once_as_the_first_message_and_shows_it_as_yours() {
    let calls = wings_with_tab(json!({}));
    wings_plugin::task::block_on(super::open_with(json!({ "cwd": "/work/shop", "prompt": "Run the tests" }), json!({})));
    let inputs = started_with(&calls);
    assert_eq!(inputs.len(), 1);
    let sent = user_lines(&inputs[0]);
    assert_eq!(sent.len(), 1, "{}", inputs[0]);
    assert_eq!(sent[0]["message"]["content"], json!("Run the tests"));
    assert!(sent[0].get("isSynthetic").is_none(), "a prompt another plugin passes is not marked");
    // The fake Claude Code ends at once, so the turn isn't running any more; the message stays shown as yours.
    assert!(with(|c| c.session.items.contains(&crate::session::Item::User("Run the tests".into())) && c.session.prompts.len() == 1 && c.prompted && c.first_prompt.is_none()));
    // The tab keeps that it was sent, so a restart goes on without it.
    let saved = asked(&calls, "setTabState");
    assert_eq!(saved.last().unwrap()["state"]["prompted"], true);
}

#[test]
fn a_session_brought_to_the_picked_account_asks_wings_to_copy_its_transcript_and_a_plain_resume_does_not() {
    let id = "3c21b0c5-8113-4f55-9493-fa156c3fa369";
    let account = |args: Value| {
        let calls = wings_with_tab(json!({}));
        wings_plugin::task::block_on(super::open_with(args, json!({})));
        asked(&calls, "spawn")[0]["account"].clone()
    };
    let picked = json!({ "name": "personal" });
    assert_eq!(account(json!({ "cwd": "/work/shop", "resume": id, "fork": true, "account": picked, "bring": true })), json!({ "session": id, "pick": { "name": "personal" }, "bring": true }));
    // A resume that only names the account still runs where the session's transcript is.
    assert_eq!(account(json!({ "cwd": "/work/shop", "resume": id, "account": picked })), json!({ "session": id, "pick": { "name": "personal" } }));
}

#[test]
fn a_reloaded_tab_does_not_send_the_prompt_again() {
    // Restarted before Claude Code named its session: the state holds no session, only that the prompt went.
    let calls = wings_with_tab(json!({}));
    wings_plugin::task::block_on(super::open_with(
        json!({ "cwd": "/work/shop", "prompt": "Run the tests" }),
        json!({ "sessionId": null, "dirs": [], "account": null, "prompted": true }),
    ));
    assert_eq!(started_with(&calls).len(), 1);
    assert!(user_lines(&started_with(&calls)[0]).is_empty());
    assert!(with(|c| c.prompted && !c.session.working));
    // Restarted with a session saved.
    let calls = wings_with_tab(json!({}));
    wings_plugin::task::block_on(super::open_with(json!({ "cwd": "/work/shop", "prompt": "Run the tests" }), json!({ "sessionId": "s1" })));
    assert!(user_lines(&started_with(&calls)[0]).is_empty());
    assert!(asked(&calls, "spawn")[0]["args"].as_array().unwrap().contains(&json!("--resume=s1")));
}

#[test]
fn a_prompt_is_ignored_when_the_chat_resumes_a_session_or_forks_one() {
    let new = Launch::default();
    let fresh = |args: Value| first_prompt(&json!({}), &args, &new);
    assert_eq!(fresh(json!({ "prompt": "Go" })), Some("Go".into()));
    assert_eq!(fresh(json!({ "prompt": "Go", "resume": "s1" })), None);
    assert_eq!(first_prompt(&json!({}), &json!({ "prompt": "Go" }), &Launch { fork: true, before: None }), None);
    assert_eq!(fresh(json!({ "prompt": "  " })), None);
    assert_eq!(fresh(json!({ "prompt": 3 })), None);
    assert_eq!(fresh(json!({})), None);
    let calls = wings_with_tab(json!({}));
    wings_plugin::task::block_on(super::open_with(json!({ "cwd": "/work/shop", "prompt": "Go", "resume": "s1" }), json!({})));
    assert!(user_lines(&started_with(&calls)[0]).is_empty());
    assert!(with(|c| !c.prompted && c.first_prompt.is_none()));
}

#[test]
fn the_changes_plugins_totals_are_the_count() {
    let answer = serde_json::to_string_pretty(&serde_json::json!({ "repo": "/r", "added": 120, "removed": 7, "total": 3, "files": [] })).unwrap();
    assert_eq!(changes_counted(&answer), Some((120, 7)));
    assert_eq!(changes_counted("wings is not inside a git repo."), None);
    assert_eq!(changes_counted(r#"{ "added": 1 }"#), None);
}

#[test]
fn the_review_event_asks_the_changes_plugin_for_the_viewer_on_this_chats_folder() {
    let key = crate::review::key();
    assert_eq!(review_event("/shop"), serde_json::json!({ "name": "open-changes", "data": { "review": true, "cwd": "/shop", "chat": key } }));
}

#[test]
fn a_shortstat_gives_the_lines_added_and_removed() {
    assert_eq!(shortstat(" 3 files changed, 590 insertions(+), 277 deletions(-)"), (590, 277));
    assert_eq!(shortstat(" 1 file changed, 1 insertion(+)"), (1, 0));
    assert_eq!(shortstat(" 2 files changed, 4 deletions(-)"), (0, 4));
    assert_eq!(shortstat(""), (0, 0));
}

/// A bare /model, /tasks, /copy and the rest open the chat's own screen and send nothing to Claude Code; with words after the
/// name they are sent as before.
#[test]
fn a_bare_slash_command_opens_the_chats_screen_and_sends_nothing() {
    use super::send;
    let calls = Rc::new(RefCell::new(Vec::new()));
    let kept = calls.clone();
    host::testing::answer(move |method, params| {
        kept.borrow_mut().push((method.to_owned(), params.clone()));
        Ok(Value::Null)
    });
    running(false);
    let process = host::spawn("claude", &[], "/work/shop", "", |_| {});
    with(|c| {
        c.input = Some(process.input());
        c.model_dialog = false;
        c.session.items = vec![crate::session::Item::User("hi".into()), crate::session::Item::Text("Hello there".into())];
    });
    let writes = || calls.borrow().iter().filter(|(method, _)| method == "processWrite").count();
    for typed in ["/model", "/effort", "/fork", "/copy", "/copy 1", "/tasks", "/bookmarks"] {
        with(|c| c.draft = typed.into());
        send();
        wings_plugin::task::block_on(async {});
        assert!(with(|c| c.draft.is_empty()), "{typed}");
    }
    assert_eq!(writes(), 0, "none of them went to Claude Code");
    assert!(with(|c| c.model_dialog && c.fork_ask && c.session.cmds.copy == Some(1)));
    // Nothing is running or bookmarked: the panel groups say so in the conversation.
    let notices = with(|c| c.session.items.iter().filter(|item| matches!(item, crate::session::Item::Notice(_))).count());
    assert_eq!(notices, 2);
    let shown = with(|c| wings_plugin::ui::tree_json(crate::screens::model_dialog(c).unwrap()).to_string());
    assert!(shown.contains("Model and effort"));
    with(|c| (c.model_dialog, c.fork_ask, c.session.cmds.copy) = (false, false, None));
    // With words after it, a command is a message.
    with(|c| c.draft = "/model opus".into());
    send();
    wings_plugin::task::block_on(async {});
    assert_eq!(writes(), 1);
    assert!(!with(|c| c.model_dialog));
}
