use super::*;
use crate::plugins::Show;

/// A turn as Claude Code 2.1.327 printed it, trimmed to the lines the chat reads and a few it skips.
const TURN: &str = include_str!("../../tests/turn.jsonl");

fn heard(lines: &str) -> Session {
    let mut session = Session::default();
    for line in lines.lines().filter(|line| !line.trim().is_empty()) {
        session.hear(&serde_json::from_str(line).unwrap());
    }
    session
}

#[test]
fn a_turn_becomes_the_tool_call_with_its_output_then_the_answer() {
    let mut session = Session::default();
    session.send("How many files?", &[], 1, 1);
    for line in TURN.lines().filter(|line| !line.trim().is_empty()) {
        session.hear(&serde_json::from_str(line).unwrap());
    }
    assert_eq!(session.id.as_deref(), Some("1fccd556-e115-4d47-9a08-f291a83a5457"));
    assert_eq!(session.model.as_deref(), Some("claude-haiku-5-5"));
    assert_eq!(
        session.items,
        [
            Item::User("How many files?".into()),
            Item::Tool(Tool {
                id: "toolu_01PQFGJCuZRyDiWjCATE1ktf".into(),
                name: "Bash".into(),
                summary: "ls".into(),
                output: Some(("err.txt\nout.jsonl\nprobe-file.txt".into(), false)),
                detail: Detail::Command("ls".into()),
                input: Input { whole: String::new(), fields: vec![("description".into(), "List files in current directory".into())] },
                images: Vec::new(),
            }),
            Item::Text("There are 3 files:\n\n- `err.txt`\n- `out.jsonl`\n- `probe-file.txt`".into()),
        ]
    );
    assert!(!session.working);
    assert!(session.live.is_empty());
    assert_eq!(session.cost, 0.00596854);
}

/// The line Claude Code 2.1.327 prints after a turn's result with `--prompt-suggestions`.
const SUGGESTION: &str = r#"{"type":"prompt_suggestion","suggestion":"run it","uuid":"cdab3e06-e878-423a-add5-9ed9174c610e","session_id":"4414f6b3-7ed9-4059-bbb1-974fbcaf0b9b"}"#;

#[test]
fn the_suggested_next_message_comes_after_the_turn_and_goes_with_the_next_one() {
    let mut session = heard(TURN);
    assert_eq!(session.suggestion, None);
    session.hear(&serde_json::from_str(SUGGESTION).unwrap());
    assert_eq!(session.suggestion.as_deref(), Some("run it"));
    assert!(session.items.iter().all(|item| !matches!(item, Item::User(_))));

    session.send("run it", &[], 1, 1);
    assert_eq!(session.suggestion, None);

    // A turn that starts on its own, or a message queued while Claude works, clears it as well.
    session.hear(&serde_json::from_str(SUGGESTION).unwrap());
    session.queue("and lint", Vec::new(), 1, 1);
    assert_eq!(session.suggestion, None);
    session.hear(&serde_json::from_str(SUGGESTION).unwrap());
    session.hear(&serde_json::from_str(r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Running."}]}}"#).unwrap());
    assert_eq!(session.suggestion, None);

    session.hear(&serde_json::from_str(r#"{"type":"prompt_suggestion","suggestion":"  "}"#).unwrap());
    assert_eq!(session.suggestion, None);
}

#[test]
fn text_shows_as_it_streams_in_before_the_message_is_whole() {
    // Up to the delta that writes the third file's name.
    let partial = TURN.lines().take_while(|line| !line.contains("l`\\n- `probe-")).collect::<Vec<_>>().join("\n");
    assert_eq!(heard(&partial).live, "There are 3 files:\n\n- `err.txt`\n- `out.json");
}

#[test]
fn a_permission_question_is_answered_once_with_the_input_it_asked_about() {
    let mut session = heard(
        r#"{"type":"control_request","request_id":"r1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"touch a\nb","description":"Make a"}}}"#,
    );
    let Item::Ask(ask) = &session.items[0] else { panic!("{:?}", session.items) };
    assert_eq!((ask.tool.as_str(), ask.summary.as_str()), ("Bash", "touch a"));
    let allow: Value = serde_json::from_str(&session.allow("r1", false).unwrap()).unwrap();
    assert_eq!(
        allow,
        json!({ "type": "control_response", "response": { "subtype": "success", "request_id": "r1", "response": { "behavior": "allow", "updatedInput": { "command": "touch a\nb", "description": "Make a" } } } })
    );
    assert_eq!(session.deny("r1", ""), None, "answered already");
    assert_eq!(session.allow("r2", false), None, "never asked");
    let mut denied = heard(r#"{"type":"control_request","request_id":"r3","request":{"subtype":"can_use_tool","tool_name":"Write","input":{}}}"#);
    let deny: Value = serde_json::from_str(&denied.deny("r3", "  ").unwrap()).unwrap();
    assert_eq!(deny["response"]["response"], json!({ "behavior": "deny", "message": "The user said no to this in the chat." }));
}

#[test]
fn a_subagents_lines_are_kept_under_its_call_and_stay_out_of_the_conversation() {
    let mut session = Session::default();
    session.now = 1_000;
    for line in [
        r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Task","input":{"description":"Survey"}}]}}"#,
        r#"{"type":"assistant","parent_tool_use_id":"toolu_1","message":{"model":"claude-haiku-5-5","content":[{"type":"tool_use","id":"toolu_2","name":"Bash","input":{"command":"ls"}}]}}"#,
        r#"{"type":"user","parent_tool_use_id":"toolu_1","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_2","content":"a.txt"}]}}"#,
        r#"{"type":"assistant","parent_tool_use_id":"toolu_1","message":{"content":[{"type":"text","text":"Found a.txt"}]}}"#,
    ] {
        session.hear(&serde_json::from_str(line).unwrap());
        session.now += 500;
    }
    assert_eq!(session.items.len(), 1);
    let sub = &session.subagents["toolu_1"];
    assert_eq!(sub.items.len(), 2);
    assert!(matches!(&sub.items[0], Item::Tool(Tool { output: Some((out, false)), .. }) if out == "a.txt"));
    assert_eq!(sub.items[1], Item::Text("Found a.txt".into()));
    assert_eq!((sub.started, sub.last), (1_500, 2_500));
    assert_eq!(sub.reported_model.as_deref(), Some("claude-haiku-5-5"));
    // Its words never reach the conversation's own state.
    assert!(session.live.is_empty() && session.reported_model.is_none());
}

#[test]
fn a_call_keeps_the_input_its_one_line_leaves_out() {
    let long = format!("rg -n pattern {}", "src/a.rs ".repeat(30));
    let called = [
        ("Bash", json!({ "command": long, "description": "Search the sources" })),
        ("Bash", json!({ "command": "echo one\necho two" })),
        ("Read", json!({ "file_path": "/repo/a.txt", "offset": 10 })),
        ("mcp__wings__notify", json!({ "title": "Done", "urgent": true })),
        ("Bash", json!({ "command": "ls" })),
    ];
    let inputs: Vec<Input> = called.iter().map(|(tool, input)| input_of(tool, input)).collect();
    // A command past the line's width, whole, and what else it was given.
    assert_eq!(inputs[0], Input { whole: long.trim().to_owned(), fields: vec![("description".into(), "Search the sources".into())] });
    // One of several lines, whole.
    assert_eq!(inputs[1].whole, "echo one\necho two");
    // A read's fold is its file, and its range is on its line.
    assert_eq!(inputs[2], Input::default());
    // Anything that isn't text, as JSON.
    assert_eq!(inputs[3], Input { whole: String::new(), fields: vec![("urgent".into(), "true".into())] });
    // A line that says it all keeps nothing more.
    assert!(!inputs[4].says_more());
}

#[test]
fn a_subagents_list_keeps_only_its_newest_items() {
    let mut session = Session::default();
    for n in 0..SUBAGENT_KEPT + 30 {
        let line = json!({ "type": "assistant", "parent_tool_use_id": "toolu_1", "message": { "content": [{ "type": "text", "text": format!("step {n}") }] } });
        session.hear(&line);
    }
    let sub = &session.subagents["toolu_1"];
    assert_eq!(sub.items.len(), SUBAGENT_KEPT);
    assert_eq!(sub.items[0], Item::Text("step 30".into()));
    assert_eq!(sub.items.last(), Some(&Item::Text(format!("step {}", SUBAGENT_KEPT + 29))));
}

#[test]
fn a_model_is_named_as_a_person_says_it() {
    assert_eq!(model_name("claude-haiku-5-5"), "Haiku 5.5");
    assert_eq!(model_name("claude-opus-4-1-20250805"), "Opus 4.1");
    assert_eq!(model_name("claude-sonnet-5"), "Sonnet 5");
    assert_eq!(model_name("gpt-4o"), "gpt-4o");
}

#[test]
fn a_resumed_sessions_transcript_reads_like_the_stream() {
    let session = heard(concat!(
        r#"{"type":"user","message":{"role":"user","content":"Fix the build"}}"#,
        "\n",
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<local-command-caveat>"}}"#,
        "\n",
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Done."}]}}"#,
        "\n",
        r#"{"type":"assistant","parent_tool_use_id":"toolu_1","message":{"content":[{"type":"text","text":"a subagent's"}]}}"#,
    ));
    assert_eq!(session.items, [Item::User("Fix the build".into()), Item::Text("Done.".into())]);
    // As this very session's transcript has them: what Claude Code wrote in your name is a note, not yours.
    let added = heard(concat!(
        r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>b1</task-id>\n<summary>Survey stopped</summary>\n</task-notification>"}}"#,
        "\n",
        r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"<local-command-caveat>The command below was run</local-command-caveat>"}]}}"#,
        "\n",
        r#"{"type":"user","message":{"role":"user","content":"<command-name>/reload-plugins</command-name>\n<command-args></command-args>"}}"#,
        "\n",
        r#"{"type":"user","message":{"role":"user","content":"<b>bold</b> is mine"}}"#,
    ));
    assert_eq!(
        added.items,
        [
            Item::Note("Survey stopped".into(), "b1\nSurvey stopped".into()),
            Item::Note("Ran /reload-plugins".into(), "/reload-plugins".into()),
            Item::User("<b>bold</b> is mine".into()),
        ]
    );
}

#[test]
fn pictures_go_as_image_blocks_before_the_text_and_show_by_their_ids() {
    images::reset();
    let pictures = [("image/png".to_owned(), "AAAA".to_owned()), ("image/jpeg".to_owned(), "BBBB".to_owned())];
    let mut session = Session::default();
    let sent: Value = serde_json::from_str(session.send("what is this?", &pictures, 1, 1).lines().next().unwrap()).unwrap();
    assert_eq!(
        sent["message"]["content"],
        json!([
            { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "AAAA" } },
            { "type": "image", "source": { "type": "base64", "media_type": "image/jpeg", "data": "BBBB" } },
            { "type": "text", "text": "what is this?" },
        ])
    );
    // Without text there is no empty text block, which the API refuses.
    let alone: Value = serde_json::from_str(session.send("", &pictures[..1], 1, 1).lines().next().unwrap()).unwrap();
    assert_eq!(alone["message"]["content"], json!([{ "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "AAAA" } }]));
    let [Item::UserImages(text, two), Item::UserImages(none, one)] = &session.items[..] else {
        panic!("{:?}", session.items)
    };
    assert_eq!((text.as_str(), two.len(), none.as_str(), one.len()), ("what is this?", 2, "", 1));
    // Each picture is kept for Wings under its id, once, and the id is what the item holds.
    let kept = images::pending();
    assert_eq!(kept.iter().map(|(_, data)| data.as_str()).collect::<Vec<_>>(), ["AAAA", "BBBB", "AAAA"]);
    assert_eq!(kept.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>(), [two.clone(), one.clone()].concat());
}

#[test]
fn a_queued_message_keeps_its_pictures_for_send_now() {
    let mut session = Session::default();
    let sent = parsed(&session.queue("look", vec![("image/gif".into(), "CCCC".into())], 1, 1));
    assert_eq!(sent[0]["message"]["content"][0]["source"]["media_type"], "image/gif");
    assert_eq!(sent[0]["message"]["content"][1], json!({ "type": "text", "text": "look" }));
    let uuid = session.queued[0].uuid.clone();
    let again = parsed(&session.send_now(&uuid, 2, 2).unwrap());
    assert_eq!(again[1]["message"]["content"][0]["source"]["data"], "CCCC");
}

#[test]
fn a_transcript_message_with_pictures_shows_them() {
    images::reset();
    let session = heard(concat!(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":"AAAA"}},{"type":"text","text":"what is this?"}]}}"#,
        "\n",
        r#"{"type":"user","message":{"role":"user","content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":"BBBB"}}]}}"#,
    ));
    let [Item::UserImages(text, first), Item::UserImages(none, second)] = &session.items[..] else {
        panic!("{:?}", session.items)
    };
    assert_eq!((text.as_str(), first.len(), none.as_str(), second.len()), ("what is this?", 1, "", 1));
    assert_ne!(first, second);
    assert!(images::is_shown(&first[0]) && images::is_shown(&second[0]));
}

#[test]
fn a_failed_turn_says_why_and_ends() {
    let mut session = Session::default();
    session.send("hi", &[], 1, 1);
    session.hear(&json!({ "type": "result", "subtype": "error_during_execution", "is_error": true, "errors": ["Credit balance is too low"] }));
    assert!(!session.working);
    assert_eq!(session.items[1], Item::Error("Credit balance is too low".into()));
}

#[test]
fn a_hit_limit_shows_at_once_in_the_limits_and_once_in_the_conversation() {
    let answer = |response: Value| json!({ "type": "control_response", "response": { "subtype": "success", "request_id": USAGE, "response": response } });
    let mut session = Session::default();
    session.send("go on", &[], 1, 1);
    session.hear(&answer(json!({ "rate_limits": {
        "five_hour": { "utilization": 67, "resets_at": "2026-11-18T19:30:00.262659+00:00" },
        "seven_day": { "utilization": 97, "resets_at": "2026-11-19T10:59:59+00:00" } } })));
    // As Claude Code 2.1.327 says it once the 5-hour limit is hit, past the last message's answer.
    let limited = "You've hit your 5-hour limit · resets 8:30pm (Europe/Zagreb)";
    session.hear(&json!({ "type": "rate_limit_event", "rate_limit_info": { "status": "rejected", "resetsAt": 1795030200,
        "rateLimitType": "five_hour", "unifiedWindows": {
            "five_hour": { "utilization": 1, "resetsAt": 1795030200, "status": "rejected", "surpassedThreshold": 1 },
            "seven_day": { "utilization": 0.98, "resetsAt": 1795086000, "status": "allowed_warning", "surpassedThreshold": 0.75 } } } }));
    session.hear(&json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": limited }] }, "error": "rate_limit" }));
    session.hear(&json!({ "type": "result", "subtype": "success", "is_error": true, "result": limited }));
    assert_eq!(session.limits, [("5h", 100, Some(1_795_030_200_000.0)), ("week", 98, Some(1_795_086_000_000.0))]);
    assert_eq!(session.items, [Item::User("go on".into()), Item::Error(limited.into())]);
    // An answer that comes without them keeps the limits shown.
    session.hear(&answer(json!({ "rate_limits": null })));
    assert_eq!(session.limits.len(), 2);
}

#[test]
fn long_output_is_cut_at_a_character() {
    assert_eq!(cut("ééé", 3), "é…");
    assert_eq!(cut("abc", 3), "abc");
    assert_eq!(summary("Mystery", &json!({ "n": 1, "path": "/a/b" })), "/a/b");
}

#[test]
fn a_stopped_turn_shows_as_stopped_not_as_an_error() {
    let mut session = Session::default();
    session.send("count to 400", &[], 1, 1);
    let stop: Value = serde_json::from_str(&session.interrupt(4)).unwrap();
    assert_eq!(stop, json!({ "type": "control_request", "request_id": "interrupt-4", "request": { "subtype": "interrupt" } }));
    // As Claude Code 2.1.327 ended a stopped turn.
    session.hear(&json!({ "type": "user", "message": { "role": "user", "content": [{ "type": "text", "text": "[Request interrupted by user]" }] } }));
    session.hear(&json!({ "type": "result", "subtype": "error_during_execution", "is_error": true,
        "errors": ["[ede_diagnostic] result_type=user last_content_type=n/a stop_reason=null"] }));
    assert_eq!(session.items, [Item::User("count to 400".into()), Item::Stopped]);
    assert!(!session.working);
    // The next turn's error is an error again.
    session.hear(&json!({ "type": "result", "subtype": "error_during_execution", "is_error": true, "errors": ["Overloaded"] }));
    assert_eq!(session.items[2], Item::Error("Overloaded".into()));
}

fn parsed(lines: &str) -> Vec<Value> {
    lines.lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}

/// Agent SDK docs, `SDKUserMessage`: `isSynthetic` and `client_composed` sit on the user message itself. Claude Code
/// 2.1.332 answered such a message with `origin: { kind: "unclassified" }` and kept it in the transcript as `isMeta`,
/// its text after `[MESSAGE FROM NON-USER SOURCE - NOT USER INPUT]`.
#[test]
fn text_another_agent_wrote_goes_to_claude_code_as_not_yours_and_what_you_typed_does_not() {
    let mut session = Session::default();
    let typed = parsed(&session.send("Run the tests", &[], 1, 1));
    assert!(typed[0].get("isSynthetic").is_none() && typed[0].get("client_composed").is_none(), "{}", typed[0]);
    let mut session = Session::default();
    let relayed = parsed(&session.send_as("(Message from the agent in Wings tab t1, sent through Wings.)\nRun the tests.", &[], 1, 1, true));
    assert_eq!((relayed[0]["isSynthetic"].clone(), relayed[0]["client_composed"].clone()), (json!(true), json!(true)), "{}", relayed[0]);
    assert_eq!(relayed[0]["message"]["role"], "user");
    // Queued while Claude works, it is marked the same, and so is it when sent at once: but never as a person's.
    let queued = parsed(&session.queue_as("Then this.", Vec::new(), 2, 2, true));
    assert_eq!(queued[0]["isSynthetic"], json!(true));
    let again = parsed(&session.send_now(&session.queued[0].uuid.clone(), 3, 3).unwrap());
    let sent = again.iter().find(|line| line["type"] == "user").unwrap();
    assert_eq!((sent["isSynthetic"].clone(), sent["priority"].clone(), sent.get("origin").is_none()), (json!(true), json!("now"), true), "{sent}");
    // Your own queued message still counts as a person's.
    let mut session = Session::default();
    session.queue("and stop", Vec::new(), 1, 1);
    let again = parsed(&session.send_now(&session.queued[0].uuid.clone(), 2, 2).unwrap());
    let sent = again.iter().find(|line| line["type"] == "user").unwrap();
    assert_eq!((sent.get("isSynthetic").is_none(), sent["origin"].clone()), (true, json!({ "kind": "human" })));
}

#[test]
fn a_resumed_chat_shows_relayed_text_as_a_message_without_claude_codes_marker_and_still_hides_other_meta_entries() {
    let mut session = Session::default();
    session.hear(&json!({ "type": "user", "isMeta": true, "uuid": "u1", "origin": { "kind": "unclassified" },
        "message": { "role": "user", "content": "[MESSAGE FROM NON-USER SOURCE - NOT USER INPUT]\n(Message from the agent in Wings tab t1, sent through Wings.)\nRun the tests." } }));
    session.hear(&json!({ "type": "user", "isMeta": true, "message": { "role": "user", "content": [{ "type": "text", "text": "# Keybindings Skill" }] } }));
    assert_eq!(session.items, [Item::User("(Message from the agent in Wings tab t1, sent through Wings.)\nRun the tests.".into())]);
    assert_eq!(session.prompts.len(), 1);
}

/// `initialize`'s answer carries an MCP server's skill under the name Claude Code gives it, `<server>:<skill>`
/// (docs: code.claude.com/docs/en/mcp.md#find-and-call-mcp-skills-by-name), like a plugin's. No MCP server with skills was
/// available to capture one from, so the entry is built from the shape `initialize` has: name, description, argumentHint.
#[test]
fn an_mcp_servers_skill_is_listed_as_server_skill_and_picking_it_sends_its_command() {
    let mut session = Session::default();
    session.hear(&json!({ "type": "control_response", "response": { "subtype": "success", "request_id": "initialize", "response": { "commands": [
        { "name": "compact", "description": "Compact", "argumentHint": "", "builtin": true },
        { "name": "claude_ai_Docs:pdf", "description": "Fill in PDF forms", "argumentHint": "<file>" },
        { "name": "deploy", "description": "Ship it", "argumentHint": "" }] } } }));
    let names: Vec<String> = crate::commands::menu(&session.commands, "pdf").into_iter().map(|command| command.name).collect();
    assert_eq!(names, ["claude_ai_Docs:pdf"]);
    let mid: Vec<String> = crate::commands::mid_menu(&session.commands, "pdf", false).into_iter().map(|command| command.name).collect();
    assert_eq!(mid, ["claude_ai_Docs:pdf"], "its part after the colon completes it in the middle of a message");
    assert_eq!(crate::builtin::find("/claude_ai_Docs:pdf", &session.commands), None);
    let sent = parsed(&session.send("/claude_ai_Docs:pdf report.pdf", &[], 1, 1));
    assert_eq!(sent[0]["message"]["content"], "/claude_ai_Docs:pdf report.pdf");
    assert!(sent[0].get("isSynthetic").is_none(), "a command you typed must run as one");
}

#[test]
fn a_message_sent_while_claude_works_is_queued_for_claudes_next_step_with_a_uuid() {
    let mut session = Session::default();
    session.send("count to 400", &[], 1, 1);
    let sent = parsed(&session.queue("and then stop", Vec::new(), 1_700_000_000_123, 5));
    assert_eq!(sent.len(), 1);
    let uuid = sent[0]["uuid"].as_str().unwrap().to_owned();
    assert_eq!(uuid.len(), 36);
    assert_eq!(uuid.matches('-').count(), 4);
    assert_eq!(
        sent[0],
        json!({ "type": "user", "message": { "role": "user", "content": "and then stop" }, "parent_tool_use_id": null, "priority": "next", "uuid": uuid })
    );
    assert_eq!(session.queued, [Queued { uuid: uuid.clone(), text: "and then stop".into(), images: Vec::new(), relayed: false }]);
    // Another write, even in the same millisecond, has another uuid.
    session.queue("more", Vec::new(), 1_700_000_000_123, 6);
    assert_ne!(session.queued[0].uuid, session.queued[1].uuid);
}

#[test]
fn taking_a_queued_message_back_cancels_it_by_uuid() {
    let mut session = Session::default();
    session.queue("one", Vec::new(), 1, 1);
    let uuid = session.queued[0].uuid.clone();
    let cancelled: Value = serde_json::from_str(&session.unqueue(&uuid, 2).unwrap()).unwrap();
    assert_eq!(cancelled, json!({ "type": "control_request", "request_id": "cancel-2", "request": { "subtype": "cancel_async_message", "message_uuid": uuid } }));
    assert!(session.queued.is_empty());
    assert_eq!(session.unqueue(&uuid, 3), None, "gone already");
}

#[test]
fn send_now_cancels_and_sends_again_to_be_read_at_once_in_one_write() {
    let mut session = Session::default();
    session.queue("first", Vec::new(), 1, 1);
    session.queue("second", Vec::new(), 1, 2);
    let old = session.queued[1].uuid.clone();
    let written = session.send_now(&old, 2, 3).unwrap();
    let lines = parsed(&written);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["request"], json!({ "subtype": "cancel_async_message", "message_uuid": old }));
    assert_eq!((lines[1]["priority"].as_str(), &lines[1]["origin"]), (Some("now"), &json!({ "kind": "human" })));
    assert_eq!(lines[1]["message"]["content"], "second");
    let fresh = lines[1]["uuid"].as_str().unwrap();
    assert_ne!(fresh, old);
    // Still queued, in its place, under the new uuid.
    assert_eq!((session.queued[1].uuid.as_str(), session.queued[1].text.as_str()), (fresh, "second"));
    assert_eq!(session.send_now("nope", 2, 4), None);
}

#[test]
fn a_queued_message_moves_into_the_conversation_when_claude_echoes_it_and_only_once() {
    let mut session = Session::default();
    session.send("count to 400", &[], 1, 1);
    session.queue("then say done", Vec::new(), 1, 1);
    let uuid = session.queued[0].uuid.clone();
    // The echo of the first message, sent while Claude was idle, is on the page already.
    session.hear(&json!({ "type": "user", "isReplay": true, "uuid": "x", "message": { "role": "user", "content": "count to 400" } }));
    assert_eq!(session.items, [Item::User("count to 400".into())]);
    let echo = json!({ "type": "user", "isReplay": true, "uuid": uuid, "session_id": "s", "message": { "role": "user", "content": "then say done" } });
    session.hear(&echo);
    session.hear(&echo);
    assert_eq!(session.items, [Item::User("count to 400".into()), Item::User("then say done".into())]);
    assert!(session.queued.is_empty());
}

#[test]
fn stop_with_a_message_queued_only_interrupts_and_leaves_it_queued_to_run_next() {
    let mut session = Session::default();
    session.send("count to 400", &[], 1, 1);
    let first = uuid(1, 1);
    session.hear(&lifecycle(&first, "started"));
    session.hear(&turn_start());
    session.queue("one", Vec::new(), 1, 2);
    let one = session.queued[0].uuid.clone();
    session.hear(&lifecycle(&one, "queued"));
    let written = session.interrupt(3);
    // One plain interrupt: no `cancel_queued`, and the message isn't written again.
    assert_eq!(parsed(&written), [json!({ "type": "control_request", "request_id": "interrupt-3", "request": { "subtype": "interrupt" } })]);
    assert_eq!(session.queued.len(), 1, "still waiting, shown as queued");
    // The stopped turn ends; Claude Code goes on with the queued message as the next turn.
    session.hear(&json!({ "type": "result", "subtype": "error_during_execution", "is_error": true, "user_message_uuids": [first], "errors": ["x"] }));
    session.hear(&lifecycle(&first, "completed"));
    assert_eq!(session.items, [Item::User("count to 400".into()), Item::Stopped]);
    assert!(session.working, "its message is next");
    assert_eq!(session.queued.len(), 1);
    session.hear(&lifecycle(&one, "started"));
    session.hear(&turn_start());
    let echo = json!({ "type": "user", "isReplay": true, "uuid": one, "message": { "role": "user", "content": "one" } });
    session.hear(&echo);
    session.hear(&echo);
    assert_eq!(session.items, [Item::User("count to 400".into()), Item::Stopped, Item::User("one".into())]);
    assert!(session.queued.is_empty());
    session.hear(&turn_end(&[&one], None));
    session.hear(&lifecycle(&one, "completed"));
    assert!(!session.working);
    assert_eq!(session.items.iter().filter(|item| **item == Item::Stopped).count(), 1, "the next turn isn't a stopped one");
}

#[test]
fn stop_with_nothing_queued_just_stops_the_turn() {
    let mut session = Session::default();
    session.send("count to 400", &[], 1, 1);
    let first = uuid(1, 1);
    session.hear(&lifecycle(&first, "started"));
    session.hear(&turn_start());
    assert_eq!(parsed(&session.interrupt(2)).len(), 1);
    session.hear(&json!({ "type": "result", "subtype": "error_during_execution", "is_error": true, "user_message_uuids": [first], "errors": ["x"] }));
    session.hear(&lifecycle(&first, "completed"));
    assert_eq!(session.items, [Item::User("count to 400".into()), Item::Stopped]);
    assert!(!session.working);
    assert!(session.queued.is_empty());
}

#[test]
fn the_working_state_follows_session_state_changed_once_claude_code_prints_it() {
    let state = |state: &str| json!({ "type": "system", "subtype": "session_state_changed", "state": state });
    let result = json!({ "type": "result", "subtype": "success", "is_error": false });
    // Without the events, the result ends the turn.
    let mut plain = Session::default();
    plain.send("hi", &[], 1, 1);
    plain.hear(&result);
    assert!(!plain.working);
    // With them, a turn's result isn't the end: the next queued turn goes on, and `idle` ends it.
    let mut session = Session::default();
    session.hear(&state("running"));
    session.queue("next", Vec::new(), 1, 1);
    let uuid = session.queued[0].uuid.clone();
    session.hear(&result);
    assert!(session.working);
    session.hear(&json!({ "type": "user", "isReplay": true, "uuid": uuid, "message": { "role": "user", "content": "next" } }));
    session.hear(&state("requires_action"));
    assert!(session.working);
    session.hear(&result);
    session.hear(&state("idle"));
    assert!(!session.working);
    // A message nothing echoed is shown once Claude is idle, not left in the queue.
    session.hear(&state("running"));
    session.queue("lost", Vec::new(), 1, 2);
    session.hear(&state("idle"));
    assert!(session.queued.is_empty());
    assert_eq!(session.items.last(), Some(&Item::User("lost".into())));
}

#[test]
fn you_are_told_once_a_turn_when_claude_needs_you_and_not_when_it_finishes() {
    let ask = |id: &str| json!({ "type": "control_request", "request_id": id, "request": { "subtype": "can_use_tool", "tool_name": "Bash", "input": { "command": "rm x" } } });
    let mut session = Session::default();
    session.send("clean up", &[], 1, 1);
    session.hear(&ask("r1"));
    session.hear(&ask("r2"));
    assert_eq!(session.alerts, [("Claude needs you: Bash".to_owned(), "rm x".to_owned())]);
    session.alerts.clear();
    session.hear(&json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": "\nDone, x is gone.\nAnything else?" }] } }));
    session.hear(&json!({ "type": "result", "subtype": "success", "is_error": false }));
    session.hear(&json!({ "type": "result", "subtype": "success", "is_error": false }));
    assert!(session.alerts.is_empty(), "a turn that ends needs nothing of you");
    // The next turn's question tells you again.
    session.hear(&ask("r3"));
    assert_eq!(session.alerts.len(), 1);
}

fn asked(extra: Value) -> Session {
    let mut request = json!({ "subtype": "can_use_tool", "tool_name": "Bash", "input": { "command": "npm test" } });
    request.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    heard(&json!({ "type": "control_request", "request_id": "r1", "request": request }).to_string())
}

fn sent(line: &str) -> Value {
    serde_json::from_str(line).unwrap()
}

#[test]
fn the_opening_input_asks_for_a_start_mode_only_when_given_one() {
    let lines = |mode| Session::opening(mode, None).lines().map(sent).collect::<Vec<_>>();
    assert!(lines(None).iter().all(|line| line["request"]["subtype"] != "set_permission_mode"));
    let asked = lines(Some("auto"));
    assert_eq!(asked.len(), 8);
    let last = asked.last().unwrap();
    assert_eq!(last["request_id"], START_MODE);
    assert_eq!(last["request"], json!({ "subtype": "set_permission_mode", "mode": "auto" }));
    // Once accepted the picker shows it, though Claude Code's own answers before it say it started in `default`.
    let answer = |id: &str, response: Value| json!({ "type": "control_response", "response": { "subtype": "success", "request_id": id, "response": response } });
    let mut session = Session { start_mode: Some("auto".into()), ..Session::default() };
    session.hear(&answer(INITIALIZE, json!({ "current_permission_mode": "default" })));
    assert_eq!(session.mode.as_deref(), Some("default"));
    session.hear(&answer(START_MODE, json!({})));
    assert_eq!(session.mode.as_deref(), Some("auto"));
    // Refused, it stays as Claude Code has it.
    let mut refused = Session { start_mode: Some("auto".into()), ..Session::default() };
    refused.hear(&answer(INITIALIZE, json!({ "current_permission_mode": "default" })));
    refused.hear(&json!({ "type": "control_response", "response": { "subtype": "error", "request_id": START_MODE, "error": "no" } }));
    assert_eq!(refused.mode.as_deref(), Some("default"));
}

#[test]
fn an_output_shows_its_first_lines_and_counts_the_rest() {
    assert_eq!(first_lines("a\nb\nc", 2), ("a\nb", 1));
    assert_eq!(first_lines("a\nb\nc", 3), ("a\nb\nc", 0));
    assert_eq!(first_lines("a\nb", 5), ("a\nb", 0));
    assert_eq!(first_lines("a\nb\nc\nd\n", 1), ("a", 3));
}

#[test]
fn always_allow_gives_the_suggested_rules_back() {
    let rules = json!([{ "type": "addRules", "rules": [{ "toolName": "Bash", "ruleContent": "npm test:*" }], "behavior": "allow", "destination": "localSettings" }]);
    let mut session = asked(json!({ "permission_suggestions": rules, "title": "Run the tests", "decision_reason": "\u{1b}[31mNot allowed\u{1b}[0m yet" }));
    let Item::Ask(ask) = &session.items[0] else { panic!() };
    assert_eq!(ask.reason.as_deref(), Some("Run the tests · Not allowed yet"));
    assert_eq!(ask.suggestions.len(), 1);
    let line = sent(&session.allow("r1", true).unwrap());
    assert_eq!(line["response"]["response"], json!({ "behavior": "allow", "updatedInput": { "command": "npm test" }, "updatedPermissions": rules }));
    // Allow once carries no rules, and a request that suppresses them offers none.
    let mut once = asked(json!({ "permission_suggestions": rules }));
    assert!(sent(&once.allow("r1", false).unwrap())["response"]["response"].get("updatedPermissions").is_none());
    let Item::Ask(ask) = &asked(json!({ "permission_suggestions": rules, "suppress_always_allow_rule": true })).items[0] else { panic!() };
    assert!(ask.suggestions.is_empty());
}

#[test]
fn deny_tells_claude_what_you_wrote() {
    let mut session = asked(json!({}));
    let line = sent(&session.deny("r1", " use pnpm instead ").unwrap());
    assert_eq!(line["response"]["response"], json!({ "behavior": "deny", "message": "use pnpm instead" }));
    let Item::Ask(ask) = &session.items[0] else { panic!() };
    assert_eq!(ask.answer, Some(false));
}

#[test]
fn the_chat_starts_on_your_settings_model_and_effort_and_shows_your_limits() {
    let answer = |id: &str, response: Value| json!({ "type": "control_response", "response": { "subtype": "success", "request_id": id, "response": response } });
    let efforts = json!(["low", "medium", "high", "xhigh", "max"]);
    let mut session = Session::default();
    session.hear(&answer(INITIALIZE, json!({ "models": [
        { "value": "default", "displayName": "Default", "supportedEffortLevels": efforts, "resolvedModel": "claude-opus-5-5" },
        { "value": "opus", "displayName": "Opus 5.5", "supportedEffortLevels": efforts, "resolvedModel": "claude-opus-5-5" },
        { "value": "sonnet", "displayName": "Sonnet 5.5", "supportedEffortLevels": efforts, "resolvedModel": "claude-sonnet-5-5" },
    ] })));
    // As a terminal's Claude: `model` and `effortLevel`, and `modelSettings` over them for one model.
    session.hear(&answer(SETTINGS, json!({ "effective": { "model": "opus", "effortLevel": "high",
        "modelSettings": { "claude-opus-5-5": { "effortLevel": "xhigh" } } } })));
    assert_eq!(session.current_model().map(|model| model.name.as_str()), Some("Opus 5.5"));
    assert_eq!(session.current_effort().as_deref(), Some("xhigh"));
    session.set_model("sonnet", 1);
    assert_eq!(session.current_effort().as_deref(), Some("high"));
    session.set_effort("low", 2);
    assert_eq!(session.current_effort().as_deref(), Some("low"));
    session.hear(&answer(USAGE, json!({ "rate_limits": {
        "five_hour": { "utilization": 96.4, "resets_at": "2026-11-18T14:29:59.562698+00:00" },
        "seven_day": { "utilization": 91, "resets_at": "2026-11-19T10:59:59+00:00" }, "seven_day_opus": null } })));
    assert_eq!(session.limits.iter().map(|(label, used, _)| (*label, *used)).collect::<Vec<_>>(), [("5h", 96), ("week", 91)]);
    // Each message asks for them again.
    assert!(session.send("hi", &[], 1, 1).contains(r#""request_id":"usage""#));
}

#[test]
fn the_model_stays_as_picked_once_claude_code_names_it_in_full_and_a_skills_text_isnt_yours() {
    let answer = |id: &str, response: Value| json!({ "type": "control_response", "response": { "subtype": "success", "request_id": id, "response": response } });
    let mut session = Session::default();
    session.hear(&answer(INITIALIZE, json!({ "models": [
        { "value": "default", "displayName": "Default (recommended)", "resolvedModel": "claude-opus-5-5" },
        { "value": "opus", "displayName": "Opus 5.5", "resolvedModel": "claude-opus-5-5" },
        { "value": "sonnet", "displayName": "Sonnet 5.5", "resolvedModel": "claude-sonnet-5-5" },
    ] })));
    session.hear(&answer(SETTINGS, json!({ "effective": { "model": "opus" } })));
    session.send("go", &[], 1, 1);
    session.hear(&json!({ "type": "system", "subtype": "init", "session_id": "s", "model": "claude-opus-5-5" }));
    assert_eq!(session.current_model().map(|model| model.name.as_str()), Some("Opus 5.5"));
    // Started with no model of its own: the full name reads as the model, not as "Default".
    let mut fresh = Session::default();
    fresh.hear(&json!({ "type": "system", "subtype": "init", "session_id": "s", "model": "claude-opus-5-5" }));
    fresh.hear(&answer(INITIALIZE, json!({ "models": [
        { "value": "default", "displayName": "Default (recommended)", "resolvedModel": "claude-opus-5-5" },
        { "value": "opus", "displayName": "Opus 5.5", "resolvedModel": "claude-opus-5-5" } ] })));
    assert_eq!(fresh.current_model().map(|model| model.name.as_str()), Some("Opus 5.5"));
    // A skill's text, as Claude Code prints it once the Skill tool loaded it.
    let shown = session.items.len();
    session.hear(&json!({ "type": "user", "isSynthetic": true, "message": { "role": "user", "content": [{ "type": "text", "text": "# Keybindings Skill" }] } }));
    assert_eq!(session.items.len(), shown);
}

#[test]
fn the_status_line_names_the_tool_that_runs() {
    let mut session = Session::default();
    session.send("go", &[], 1, 1);
    assert_eq!(session.activity(), "Working");
    tool_call(&mut session, "a", "mcp__wings__tasks__list", json!({}));
    assert_eq!(session.activity(), "Calling wings");
    tool_result(&mut session, "a", "[]", false);
    tool_call(&mut session, "b", "mcp__plugin_repo-docs_repo-docs__find_docs", json!({ "query": "x" }));
    assert_eq!(session.activity(), "Calling repo-docs");
    tool_result(&mut session, "b", "", false);
    tool_call(&mut session, "c", "Read", json!({ "file_path": "/work/shop/src/checkout.ts" }));
    assert_eq!(session.activity(), "Reading checkout.ts");
    tool_result(&mut session, "c", "", false);
    tool_call(&mut session, "d", "Bash", json!({ "command": "pnpm test\necho done" }));
    assert_eq!(session.activity(), "Running pnpm test");
}

#[test]
fn a_transcript_read_back_at_the_hosts_limit_says_earlier_messages_are_not_loaded() {
    let entry = json!({ "type": "user", "message": { "role": "user", "content": "hi" } });
    let mut whole = Session::default();
    let few = vec![entry.clone(); HISTORY_ALL as usize - 1];
    whole.read_back(&few);
    assert!(whole.items.is_empty());
    let mut cut = Session::default();
    let most = vec![entry; HISTORY_ALL as usize];
    cut.read_back(&most);
    assert_eq!(cut.items, [Item::Divider(EARLIER_NOT_LOADED.into())]);
    // The rebuild of a chat Wings kept running says it too.
    let mut rebuilt = Session::default();
    crate::reattach::rebuild(&mut rebuilt, &most, None, &[], false);
    assert_eq!(rebuilt.items.first(), Some(&Item::Divider(EARLIER_NOT_LOADED.into())));
}

#[test]
fn the_card_shows_all_a_tool_would_do_and_the_characters_that_hide() {
    let detail = |extra: Value| {
        let Item::Ask(ask) = asked(extra).items.remove(0) else { panic!() };
        ask
    };
    let ask = detail(json!({ "input": { "command": "a\nb\nc\nd\ne\nf\ng\nh" } }));
    assert_eq!((ask.summary.as_str(), ask.detail.as_str()), ("a", "a\nb\nc\nd\ne\nf\ng\nh"));
    // A right-to-left mark, a zero-width space, a Hangul filler or a tag would make the command read other than it
    // runs. Letters stay as they are.
    let tricky = detail(json!({ "input": { "command": "ls \u{202e}fdp.exe\u{200b} \u{3164} \u{e0041} café" } })).detail;
    assert_eq!(tricky, "ls \\u{202e}fdp.exe\\u{200b} \\u{3164} \\u{e0041} café");
    // A path is one line: a line break in it is written out, not drawn.
    let edit = json!({ "file_path": "/a.rs\n/b.rs", "old_string": "one\ntwo", "new_string": "one\n2" });
    assert_eq!(detail(json!({ "tool_name": "Edit", "input": edit })).detail, "/a.rs\\u{a}/b.rs\n\n− one\n− two\n+ one\n+ 2");
    assert_eq!(detail(json!({ "tool_name": "Write", "input": { "file_path": "/b", "content": "x\ny" } })).detail, "/b\n\nx\ny");
    assert_eq!(detail(json!({ "tool_name": "mcp__db__drop", "input": { "table": "users" } })).detail, "{\n  \"table\": \"users\"\n}");
    // What else the input holds shows too, and leaving the sandbox says so first.
    let outside = detail(json!({ "input": { "command": "make", "description": "Build", "dangerouslyDisableSandbox": true, "timeout": 600000 } })).detail;
    assert_eq!(outside, "Runs outside the sandbox.\n\nmake\n\n{\n  \"dangerouslyDisableSandbox\": true,\n  \"timeout\": 600000\n}");
    // Past what a card can show, it says so.
    let long = detail(json!({ "input": { "command": "x".repeat(ASK_KEPT + 1) } }));
    assert!(long.cut_short);
    assert!(!ask.cut_short);
}

#[test]
fn the_questions_are_answered_by_text_with_labels_or_other_words() {
    let questions = json!([
        { "question": "Which library?", "header": "Library", "multiSelect": false, "options": [{ "label": "date-fns", "description": "" }, { "label": "dayjs", "description": "" }] },
        { "question": "Which features?", "header": "Features", "multiSelect": true, "options": [{ "label": "Parse", "description": "" }, { "label": "Format", "description": "" }] },
    ]);
    let mut session = heard(&json!({ "type": "control_request", "request_id": "q1", "request": { "subtype": "can_use_tool", "tool_name": "AskUserQuestion", "input": { "questions": questions } } }).to_string());
    let mut draft = Draft::default();
    assert_eq!(session.submit("q1", &draft), None, "nothing picked");
    draft.toggle(0, 0, false);
    draft.toggle(0, 1, false);
    draft.toggle(1, 0, true);
    draft.toggle(1, 1, true);
    draft.set_other(1, " and Diff ".into(), true);
    let line = sent(&session.submit("q1", &draft).unwrap());
    assert_eq!(
        line["response"]["response"],
        json!({ "behavior": "allow", "updatedInput": { "questions": questions, "answers": { "Which library?": "dayjs", "Which features?": "Parse, Format, and Diff" } } })
    );
}

#[test]
fn approving_a_plan_allows_it_and_switches_the_mode_in_one_write() {
    let mut session = heard(r##"{"type":"control_request","request_id":"p1","request":{"subtype":"can_use_tool","tool_name":"ExitPlanMode","input":{"plan":"# Plan"}}}"##);
    let written = session.approve_plan("p1", "acceptEdits", 7).unwrap();
    let lines: Vec<Value> = written.lines().map(sent).collect();
    assert_eq!(lines.len(), 2);
    assert!(written.ends_with('\n'));
    assert_eq!(lines[0]["response"]["response"], json!({ "behavior": "allow", "updatedInput": { "plan": "# Plan" } }));
    assert_eq!(lines[1], json!({ "type": "control_request", "request_id": "mode-7", "request": { "subtype": "set_permission_mode", "mode": "acceptEdits" } }));
    assert_eq!(session.mode.as_deref(), Some("acceptEdits"));
    assert_eq!(session.approve_plan("p1", "default", 8), None, "answered already");
}

#[test]
fn a_plan_can_be_approved_into_auto_mode_first() {
    assert_eq!(PLAN_APPROVALS[0].1, "auto", "Claude Code lists \"Yes, and use auto mode\" first");
    assert!(PLAN_APPROVALS.iter().all(|(_, mode, _)| MODES.iter().any(|(known, _)| known == mode)));
    let mut session = heard(r##"{"type":"control_request","request_id":"p1","request":{"subtype":"can_use_tool","tool_name":"ExitPlanMode","input":{"plan":"# Plan"}}}"##);
    let written = session.approve_plan("p1", PLAN_APPROVALS[0].1, 3).unwrap();
    let lines: Vec<Value> = written.lines().map(sent).collect();
    assert_eq!(lines[0]["response"]["response"]["behavior"], "allow");
    assert_eq!(lines[1]["request"], json!({ "subtype": "set_permission_mode", "mode": "auto" }));
    assert_eq!(session.mode.as_deref(), Some("auto"));
}

#[test]
fn a_cancel_request_withdraws_the_card() {
    let mut session = asked(json!({}));
    session.hear(&json!({ "type": "control_cancel_request", "request_id": "other" }));
    assert_eq!(session.pending("r1").is_some(), true);
    session.hear(&json!({ "type": "control_cancel_request", "request_id": "r1" }));
    let Item::Ask(ask) = &session.items[0] else { panic!() };
    assert_eq!(ask.answer, Some(false));
    assert_eq!(session.allow("r1", false), None, "nothing left to answer");
}

fn tool_call(session: &mut Session, id: &str, name: &str, input: Value) {
    session.hear(&json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "id": id, "name": name, "input": input }] } }));
}

fn tool_result(session: &mut Session, id: &str, content: &str, error: bool) {
    session.hear(&json!({ "type": "user", "message": { "content": [{ "type": "tool_result", "tool_use_id": id, "content": content, "is_error": error }] } }));
}

/// How long a call ran is the time between its message and its result, which Claude Code 2.1.332 stamps on the
/// stream (`timestamp`) and in the transcript alike. Unknown, a call to ask you, or a command in the background says nothing.
#[test]
fn a_call_knows_how_long_it_ran_from_the_timestamps_of_its_message_and_result() {
    let call = |session: &mut Session, id: &str, at: &str, input: Value| {
        session.hear(&json!({ "type": "assistant", "timestamp": at, "message": { "content": [{ "type": "tool_use", "id": id, "name": "Bash", "input": input }] } }));
    };
    let result = |session: &mut Session, id: &str, at: &str, key: &str| {
        session.hear(&json!({ "type": "user", "timestamp": at, "message": { "content": [{ "type": "tool_result", "tool_use_id": id, "content": "hi" }] }, key: { "stdout": "hi" } }));
    };
    // Live, and as a resumed transcript reads back, a call and its result a few seconds apart.
    for key in ["tool_use_result", "toolUseResult"] {
        let mut session = Session::default();
        call(&mut session, "b1", "2026-11-25T06:29:54.668Z", json!({ "command": "sleep 3" }));
        result(&mut session, "b1", "2026-11-25T06:29:58.248Z", key);
        assert_eq!(session.ran.get("b1"), Some(&3_580), "{key}");
    }
    // Two calls at once each run from their own message to their own result.
    let mut session = Session::default();
    call(&mut session, "a", "2026-11-25T06:00:00.000Z", json!({ "command": "one" }));
    call(&mut session, "b", "2026-11-25T06:00:00.100Z", json!({ "command": "two" }));
    result(&mut session, "b", "2026-11-25T06:00:01.100Z", "tool_use_result");
    result(&mut session, "a", "2026-11-25T06:01:12.000Z", "tool_use_result");
    assert_eq!((session.ran.get("b"), session.ran.get("a")), (Some(&1_000), Some(&72_000)));
    // No timestamps, nothing to say.
    let mut session = Session::default();
    tool_call(&mut session, "n1", "Bash", json!({ "command": "ls" }));
    tool_result(&mut session, "n1", "a", false);
    assert!(session.ran.is_empty());
    // A command in the background answers at once with its launch notice.
    let mut session = Session::default();
    call(&mut session, "g1", "2026-11-25T06:00:00.000Z", json!({ "command": "make", "run_in_background": true }));
    result(&mut session, "g1", "2026-11-25T06:00:00.300Z", "tool_use_result");
    assert!(session.ran.is_empty());
    // A call you were asked about includes the time you took.
    let mut session = Session::default();
    call(&mut session, "p1", "2026-11-25T06:00:00.000Z", json!({ "command": "rm x" }));
    session.hear(&json!({ "type": "control_request", "request_id": "r1", "request": { "subtype": "can_use_tool", "tool_name": "Bash", "tool_use_id": "p1", "input": { "command": "rm x" } } }));
    result(&mut session, "p1", "2026-11-25T06:00:40.000Z", "tool_use_result");
    assert!(session.ran.is_empty());
}

/// Claude Code 2.1.331 answers a gateway's call with no name with an error, where it crashed before.
#[test]
fn a_tool_call_with_no_name_has_one_to_show() {
    for (id, call) in [("t1", json!({ "type": "tool_use", "id": "t1", "name": "", "input": {} })), ("t2", json!({ "type": "tool_use", "id": "t2", "input": {} }))] {
        let mut session = Session::default();
        session.hear(&json!({ "type": "assistant", "message": { "content": [call] } }));
        let Some(Item::Tool(tool)) = session.items.last() else { panic!("{:?}", session.items) };
        assert_eq!((tool.id.as_str(), tool.name.as_str()), (id, UNNAMED));
        assert_eq!(session.activity(), "Using a tool with no name");
        session.hear(&json!({ "type": "user", "message": { "content": [{ "type": "tool_result", "tool_use_id": id, "content": "<tool_use_error>No such tool available: </tool_use_error>", "is_error": true }] } }));
        let Some(Item::Tool(tool)) = session.items.last() else { panic!() };
        assert_eq!(tool.output, Some(("No such tool available:".to_owned(), true)), "the error without its tags");
    }
}

/// A Skill result says when an MCP server served the skill: `tool_use_result` as Claude Code prints it,
/// `toolUseResult` in a transcript.
#[test]
fn a_skill_an_mcp_server_served_is_noted_from_its_result() {
    let mut session = Session::default();
    for (id, key, mcp) in [("s1", "tool_use_result", true), ("s2", "toolUseResult", true), ("s3", "tool_use_result", false)] {
        tool_call(&mut session, id, "Skill", json!({ "skill": "docs:release" }));
        let result = json!({ "success": true, "commandName": "docs:release", "fromMcpServer": mcp });
        session.hear(&json!({ "type": "user", "message": { "content": [{ "type": "tool_result", "tool_use_id": id, "content": "Launching skill: docs:release" }] }, key: result }));
    }
    // Another tool's result with the flag marks nothing.
    tool_call(&mut session, "b1", "Bash", json!({ "command": "ls" }));
    session.hear(&json!({ "type": "user", "message": { "content": [{ "type": "tool_result", "tool_use_id": "b1", "content": "a" }] }, "tool_use_result": { "fromMcpServer": true } }));
    let mut marked: Vec<&str> = session.mcp_skills.iter().map(String::as_str).collect();
    marked.sort_unstable();
    assert_eq!(marked, ["s1", "s2"]);
}

/// A session read back: what you sent while Claude worked is an attachment of the turn that read it, with text or
/// with pictures; another agent's message and a task's notice are not yours.
#[test]
fn a_session_read_back_is_named_as_claude_code_titled_it_and_your_own_title_wins() {
    let mut session = Session::default();
    session.hear(&json!({ "type": "ai-title", "aiTitle": "HUD plugin limits display" }));
    assert_eq!(session.title.as_deref(), Some("HUD plugin limits display"));
    session.hear(&json!({ "type": "custom-title", "customTitle": "Wings chat work" }));
    session.hear(&json!({ "type": "ai-title", "aiTitle": "A later made-up title" }));
    assert_eq!(session.title.as_deref(), Some("Wings chat work"));
}

#[test]
fn messages_you_sent_while_claude_worked_come_back_from_the_transcript() {
    let mut session = Session::default();
    let queued = |prompt: Value, mode: &str, origin: Value| {
        json!({ "type": "attachment", "uuid": "u", "attachment": { "type": "queued_command", "prompt": prompt, "commandMode": mode, "origin": origin } })
    };
    session.hear(&queued(json!("ANd we are missing when the subagents finish"), "prompt", Value::Null));
    session.hear(&queued(json!("see this"), "prompt", json!({ "kind": "human" })));
    session.hear(&queued(json!([{ "type": "text", "text": "it's too narrow" }]), "prompt", Value::Null));
    session.hear(&queued(json!("Message from a teammate"), "prompt", json!({ "kind": "peer" })));
    session.hear(&queued(
        json!("<task-notification>\n<task-id>b1</task-id>\n<status>completed</status>\n<summary>Background command \"Build\" completed (exit code 0)</summary>\n</task-notification>"),
        "task-notification",
        json!({ "kind": "task-notification" }),
    ));
    assert_eq!(
        session.items[..3],
        [Item::User("ANd we are missing when the subagents finish".into()), Item::User("see this".into()), Item::User("it's too narrow".into())]
    );
    assert_eq!(session.items.len(), 4, "{:?}", session.items);
    assert!(matches!(&session.items[3], Item::Finished(done) if done.description == "Build"));
}

#[test]
fn a_call_that_can_change_files_asks_for_the_changes_to_be_counted_again_as_it_ends() {
    let mut session = Session::default();
    tool_call(&mut session, "r1", "Read", json!({ "file_path": "/a/b.rs" }));
    tool_result(&mut session, "r1", "fn main() {}", false);
    assert!(!session.files_touched, "a read changes nothing");
    for (id, name) in [("e1", "Edit"), ("w1", "Write"), ("b1", "Bash"), ("t1", "Agent")] {
        tool_call(&mut session, id, name, json!({}));
        assert!(!session.files_touched, "not before {name} has ended");
        tool_result(&mut session, id, "done", false);
        assert!(std::mem::take(&mut session.files_touched), "{name}");
    }
}

#[test]
fn a_summary_says_a_command_on_one_line_without_its_runs_of_spaces() {
    let command = json!({ "command": "sed -i '' '206s/                        .line_clamp(3)/x/' src/a.rs\nmore" });
    assert_eq!(summary("Bash", &command), "sed -i '' '206s/ .line_clamp(3)/x/' src/a.rs");
}

#[test]
fn an_edit_counts_the_lines_it_takes_out_and_puts_in() {
    let mut session = Session::default();
    tool_call(&mut session, "e1", "Edit", json!({ "file_path": "/a/b.rs", "old_string": "one\ntwo", "new_string": "one\n2\nthree" }));
    let Item::Tool(tool) = &session.items[0] else { panic!("{:?}", session.items) };
    assert_eq!(
        tool.detail,
        Detail::Diff(Diff {
            added: 3,
            removed: 2,
            lines: vec![(false, "one".into()), (false, "two".into()), (true, "one".into()), (true, "2".into()), (true, "three".into())],
            more: 0,
        })
    );
    // A multi-edit adds its edits up, and a long one keeps only its first lines.
    let many = "x\n".repeat(30);
    tool_call(&mut session, "e2", "MultiEdit", json!({ "file_path": "/a", "edits": [{ "old_string": many, "new_string": "y" }, { "old_string": "", "new_string": many }] }));
    let Item::Tool(tool) = &session.items[1] else { panic!() };
    let Detail::Diff(diff) = &tool.detail else { panic!("{:?}", tool.detail) };
    assert_eq!((diff.added, diff.removed, diff.lines.len(), diff.more), (31, 30, 40, 21));
}

#[test]
fn the_head_of_an_edit_ask_is_its_path_without_the_lines_the_diff_draws() {
    let input = json!({ "file_path": "/a/b.rs", "old_string": "x", "new_string": "y", "extra": 1 });
    assert_eq!(ask_head("Edit", &input), "/a/b.rs\n\n{\n  \"extra\": 1\n}");
    assert!(ask_detail("Edit", &input).contains("− x\n+ y"));
    assert_eq!(ask_head("Write", &json!({ "file_path": "/a", "content": "c" })), "/a");
    assert_eq!(ask_head("MultiEdit", &json!({ "file_path": "/a", "edits": [] })), "/a");
    assert_eq!(ask_head("Bash", &json!({ "command": "ls" })), ask_detail("Bash", &json!({ "command": "ls" })));
}

#[test]
fn a_write_a_read_and_a_command_keep_what_the_view_draws() {
    let mut session = Session::default();
    tool_call(&mut session, "w", "Write", json!({ "file_path": "/a", "content": "a\nb\nc\n" }));
    tool_call(&mut session, "r", "Read", json!({ "file_path": "/a", "offset": 10, "limit": 50 }));
    tool_call(&mut session, "r2", "Read", json!({ "file_path": "/a" }));
    let details: Vec<&Detail> = session.items.iter().filter_map(|item| if let Item::Tool(tool) = item { Some(&tool.detail) } else { None }).collect();
    assert_eq!(details[0], &Detail::Write { lines: 3, head: vec!["a".into(), "b".into(), "c".into()] });
    assert_eq!(details[1], &Detail::Read(Some("lines 10–59".into())));
    assert_eq!(details[2], &Detail::Read(None));
}

#[test]
fn failed_commands_and_searches_say_what_they_printed() {
    assert_eq!(exit_code("Exit code 127\nzsh: command not found"), Some(127));
    assert_eq!(exit_code("fine"), None);
    assert_eq!(files_found("Grep", "Found 12 files\na\nb"), Some(12));
    assert_eq!(files_found("Grep", "No files found"), Some(0));
    assert_eq!(files_found("Grep", "12:let x = 1"), None);
    assert_eq!(files_found("Glob", "a.rs\nb.rs"), Some(2));
}

#[test]
fn todo_write_replaces_the_checklist() {
    let mut session = Session::default();
    tool_call(
        &mut session,
        "t1",
        "TodoWrite",
        json!({ "todos": [
            { "content": "Write it", "status": "completed", "activeForm": "Writing it" },
            { "content": "Test it", "status": "in_progress", "activeForm": "Testing it" },
            { "content": "Ship it", "status": "pending" },
        ] }),
    );
    let got: Vec<(&str, &str, Progress)> = session.todos.iter().map(|t| (t.text.as_str(), t.active.as_str(), t.progress)).collect();
    assert_eq!(got, [("Write it", "Writing it", Progress::Done), ("Test it", "Testing it", Progress::Active), ("Ship it", "Ship it", Progress::Pending)]);
    tool_call(&mut session, "t2", "TodoWrite", json!({ "todos": [{ "content": "Only", "status": "pending" }] }));
    assert_eq!(session.todos.len(), 1);
}

#[test]
fn task_tools_build_the_checklist_from_their_results() {
    let mut session = Session::default();
    tool_call(&mut session, "c1", "TaskCreate", json!({ "subject": "Entities", "description": "d", "activeForm": "Building entities" }));
    assert!(session.todos.is_empty(), "the number comes with the result");
    tool_result(&mut session, "c1", "Task #1 created successfully: Entities", false);
    tool_call(&mut session, "c2", "TaskCreate", json!({ "subject": "Maps" }));
    tool_result(&mut session, "c2", "Task #2 created successfully: Maps", false);
    tool_call(&mut session, "c3", "TaskCreate", json!({ "subject": "Refused" }));
    tool_result(&mut session, "c3", "no", true);
    tool_call(&mut session, "u1", "TaskUpdate", json!({ "taskId": "1", "status": "in_progress" }));
    tool_call(&mut session, "u2", "TaskUpdate", json!({ "taskId": "2", "status": "completed", "subject": "EF maps" }));
    let got: Vec<(&str, &str, Progress)> = session.todos.iter().map(|t| (t.id.as_str(), t.text.as_str(), t.progress)).collect();
    assert_eq!(got, [("1", "Entities", Progress::Active), ("2", "EF maps", Progress::Done)]);
    assert_eq!(session.todos[0].active, "Building entities");
    tool_call(&mut session, "u3", "TaskUpdate", json!({ "taskId": "1", "status": "deleted" }));
    assert_eq!(session.todos.len(), 1);
}

#[test]
fn the_usage_answer_becomes_the_windows_credits_contributors_and_totals_the_view_shows() {
    let mut session = Session::default();
    session.hear(&serde_json::from_str(include_str!("../../tests/usage.json")).unwrap());
    let usage = session.usage_view.expect("the answer to the usage view's request");
    let windows: Vec<(&str, u64)> = usage.windows.iter().map(|window| (window.name.as_str(), window.used)).collect();
    assert_eq!(windows, [("Current session", 3), ("Current week (all models)", 13), ("Current week (Fable)", 0)]);
    assert_eq!(usage.windows[1].resets, "2026-11-22T15:59:59.562716+00:00");
    assert_eq!(usage.plan.as_deref(), Some("max"));
    let credits = usage.credits.unwrap();
    assert_eq!((credits.spent, credits.limit, credits.currency.as_str(), credits.used), (11.74, Some(200.0), "EUR", Some(6)));
    let day = usage.day.unwrap();
    assert_eq!(day.tips[0], (70, "See what fills your context".into(), "run /context".into()));
    assert_eq!((day.skills, day.plugins, day.mcp_servers), (vec![("release-notes".into(), 4)], vec![("toolbox".into(), 11)], vec![("files".into(), 6)]));
    assert_eq!(usage.week.unwrap().skills, [("release-notes".to_owned(), 7)]);
    let totals = usage.totals;
    assert_eq!((totals.api_ms, totals.wall_ms, totals.added, totals.removed), (84_200, 612_000, 120, 31));
    // Summed over the models the session used.
    assert_eq!((totals.input, totals.output, totals.cache_read, totals.cache_write), (1500, 10_500, 410_000, 57_000));
    assert!((totals.cost - 1.2345).abs() < 1e-9);
}

#[test]
fn a_usage_answer_without_credits_or_a_scan_shows_neither_and_an_error_says_why() {
    let answer = |id: &str, response: Value| json!({ "type": "control_response", "response": { "subtype": "success", "request_id": id, "response": response } });
    let mut session = Session::default();
    session.hear(&answer(USAGE_VIEW, json!({ "session": {}, "rate_limits": {
        "five_hour": { "utilization": 40, "resets_at": "2026-11-18T14:29:59+00:00" },
        "seven_day_opus": { "utilization": 7, "resets_at": "2026-11-19T10:59:59+00:00" },
        "extra_usage": { "is_enabled": false } }, "behaviors": null })));
    let usage = session.usage_view.take().unwrap();
    // The side panel takes the same fresh numbers: it showed what the last reply said beside the dialog's newer ones.
    assert_eq!(session.limits.iter().map(|(label, used, _)| (*label, *used)).collect::<Vec<_>>(), [("5h", 40)]);
    // No server-listed per-model windows: the fixed Opus one stands in.
    assert_eq!(usage.windows.iter().map(|window| window.name.as_str()).collect::<Vec<_>>(), ["Current session", "Current week (Opus)"]);
    assert!(usage.credits.is_none() && usage.day.is_none() && usage.week.is_none());
    // Turned off once the monthly limit is reached, as Claude Code answers then: what was spent still shows.
    session.hear(&answer(USAGE_VIEW, json!({ "session": {}, "rate_limits": {
        "extra_usage": { "is_enabled": false, "monthly_limit": 5000, "used_credits": 5000, "utilization": 100, "currency": "USD" } } })));
    let credits = session.usage_view.take().unwrap().credits.expect("the spend, though they are off");
    assert_eq!((credits.spent, credits.limit, credits.used, credits.on), (50.0, Some(50.0), Some(100), false));
    session.hear(&json!({ "type": "control_response", "response": { "subtype": "error", "request_id": USAGE_VIEW, "error": "Not signed in" } }));
    assert_eq!(session.view_error.as_deref(), Some("Not signed in"));
}

#[test]
fn the_context_answer_becomes_its_categories_without_the_deferred_ones() {
    let answer = json!({ "type": "control_response", "response": { "subtype": "success", "request_id": CONTEXT_VIEW, "response": {
        "model": "claude-opus-5-5", "totalTokens": 41_000, "maxTokens": 200_000,
        "categories": [
            { "name": "System prompt", "tokens": 6000, "color": "x", "kind": "used" },
            { "name": "Messages", "tokens": 35_000, "color": "x", "kind": "used" },
            { "name": "Deferred tools", "tokens": 9000, "color": "x", "kind": "deferred", "isDeferred": true },
            { "name": "Free space", "tokens": 120_000, "color": "x", "kind": "free" },
            { "name": "Autocompact buffer", "tokens": 39_000, "color": "x", "kind": "buffer" } ] } } });
    let mut session = Session::default();
    session.hear(&answer);
    let context = session.context_view.unwrap();
    assert_eq!((context.used, context.most, context.model.as_str()), (41_000, 200_000, "claude-opus-5-5"));
    let names: Vec<&str> = context.categories.iter().map(|(name, ..)| name.as_str()).collect();
    assert_eq!(names, ["System prompt", "Messages", "Free space", "Autocompact buffer"]);
}

#[test]
fn usage_cost_and_context_open_a_view_and_are_not_sent_as_messages() {
    for (text, panel, subtype) in [("/usage", Panel::Usage, "get_usage"), (" /cost\n", Panel::Usage, "get_usage"), ("/context", Panel::Context, "get_context_usage")] {
        let mut session = Session::default();
        let Some(crate::builtin::Local::Panel(opened)) = crate::builtin::find(text, &[]) else { panic!("{text}") };
        let line = session.open_view(opened);
        assert_eq!(opened, panel);
        // One control request, no message, and nothing in the conversation.
        assert_eq!(line.lines().count(), 1);
        let sent: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(sent["type"], "control_request");
        assert_eq!(sent["request"]["subtype"], subtype);
        assert!(sent["request"].get("skip_behaviors").is_none());
        assert!(session.items.is_empty() && !session.working);
    }
}

#[test]
fn a_click_on_a_side_panel_row_opens_the_dialog_with_one_read() {
    for (panel, subtype) in [(Panel::Usage, "get_usage"), (Panel::Context, "get_context_usage")] {
        let mut session = Session::default();
        session.view_error = Some("Not signed in".into());
        let line = session.open_view(panel);
        assert_eq!(line.lines().count(), 1);
        let sent: Value = serde_json::from_str(&line).unwrap();
        assert_eq!((sent["type"].as_str(), sent["request"]["subtype"].as_str()), (Some("control_request"), Some(subtype)));
        assert_eq!(session.view_error, None, "the last answer's error goes with the new read");
        assert!(session.items.is_empty() && !session.working);
    }
}

// As a rewound session's transcript has them, ids made up: what the rewind dropped stays in the file, and a
// `last-prompt` line with `rewound` comes after it.
const FIRST_USER: &str = r#"{"type":"user","uuid":"11111111-1111-4111-8111-111111111111","parentUuid":"b70db500-0000-4000-8000-000000000001","message":{"role":"user","content":"Say only: one"}}"#;
const FIRST_REPLY: &str = r#"{"type":"assistant","uuid":"33469fd0-0000-4000-8000-000000000002","parentUuid":"bbc4a9ba-0000-4000-8000-000000000003","message":{"role":"assistant","content":[{"type":"text","text":"one"}]}}"#;
const SECOND_USER: &str = r#"{"type":"user","uuid":"22222222-2222-4222-8222-222222222222","parentUuid":"9936c70a-0000-4000-8000-000000000004","message":{"role":"user","content":"Say only: two"}}"#;
const SECOND_REPLY: &str = r#"{"type":"assistant","uuid":"f66a3fb3-0000-4000-8000-000000000005","parentUuid":"9aeba6c8-0000-4000-8000-000000000006","message":{"role":"assistant","content":[{"type":"text","text":"two"}]}}"#;
const PLAIN_LAST_PROMPT: &str = r#"{"type":"last-prompt","lastPrompt":"Say only: two","leafUuid":"9aeba6c8-0000-4000-8000-000000000006","sessionId":"c05c0eb5-0000-4000-8000-000000000007"}"#;

#[test]
fn a_transcript_leaves_out_what_a_rewind_dropped() {
    let rewound = r#"{"type":"last-prompt","lastPrompt":"Say only: two","leafUuid":"33469fd0-0000-4000-8000-000000000002","explicit":true,"rewound":true,"cutUuid":"22222222-2222-4222-8222-222222222222","sessionId":"c05c0eb5-0000-4000-8000-000000000007"}"#;
    let mut session = heard(&[FIRST_USER, FIRST_REPLY, PLAIN_LAST_PROMPT, SECOND_USER, SECOND_REPLY, rewound, PLAIN_LAST_PROMPT].join("\n"));
    assert_eq!(session.items, [Item::User("Say only: one".into()), Item::Text("one".into())]);
    assert_eq!(session.prompts.len(), 1);
    // What is said after the rewind goes on from the reply it kept.
    let next = r#"{"type":"user","uuid":"44444444-4444-4444-8444-444444444444","parentUuid":"33469fd0-0000-4000-8000-000000000002","message":{"role":"user","content":"Say only: three"}}"#;
    session.hear(&serde_json::from_str(next).unwrap());
    assert_eq!(session.items, [Item::User("Say only: one".into()), Item::Text("one".into()), Item::User("Say only: three".into())]);
}

#[test]
fn a_transcript_rewound_to_its_first_message_is_empty() {
    // Claude Code writes a null `leafUuid` when nothing is left.
    let rewound = r#"{"type":"last-prompt","lastPrompt":"Say only: two","leafUuid":null,"explicit":true,"rewound":true,"cutUuid":"11111111-1111-4111-8111-111111111111","sessionId":"c05c0eb5-0000-4000-8000-000000000007"}"#;
    let bare = r#"{"type":"last-prompt","lastPrompt":"Say only: two","sessionId":"c05c0eb5-0000-4000-8000-000000000007"}"#;
    let session = heard(&[FIRST_USER, FIRST_REPLY, SECOND_USER, SECOND_REPLY, rewound, bare].join("\n"));
    assert!(session.items.is_empty() && session.prompts.is_empty());
    // A last-prompt line that isn't a rewind drops nothing.
    assert_eq!(heard(&[FIRST_USER, FIRST_REPLY, PLAIN_LAST_PROMPT].join("\n")).items.len(), 2);
}

#[test]
fn the_counters_follow_usage_lines_and_the_finished_turn_keeps_its_tokens_and_its_share_of_the_cost() {
    let mut session = Session::default();
    session.now = 1_000;
    session.hear(&json!({ "type": "system", "subtype": "init", "session_id": "s", "apiKeySource": "ANTHROPIC_API_KEY" }));
    assert!(session.meter.per_token);
    session.send("go", &[], 1, 1);
    session.hear(&json!({ "type": "stream_event", "event": { "type": "message_start", "message": { "id": "m1", "usage": { "input_tokens": 9, "output_tokens": 1, "cache_creation": { "ephemeral_1h_input_tokens": 0, "ephemeral_5m_input_tokens": 30 } } } } }));
    session.hear(&json!({ "type": "system", "subtype": "thinking_tokens", "estimated_tokens": 600, "estimated_tokens_delta": 600 }));
    assert_eq!(session.meter.tokens(), 600);
    session.hear(&json!({ "type": "stream_event", "event": { "type": "message_delta", "usage": { "output_tokens": 1_500 } } }));
    session.hear(&json!({ "type": "assistant", "parent_tool_use_id": null, "message": { "id": "m1", "role": "assistant", "content": [{ "type": "text", "text": "hi" }], "usage": { "output_tokens": 1_500 } } }));
    assert_eq!(session.meter.tokens(), 1_500);
    assert_eq!(session.meter.cache(1_000 + 60_000).as_deref(), Some("Cache warm · 4m left"));
    session.hear(&json!({ "type": "system", "subtype": "task_progress", "task_id": "a", "tool_use_id": "toolu_9", "usage": { "total_tokens": 10, "tool_uses": 3, "duration_ms": 4_000 } }));
    assert_eq!(session.meter.agents["toolu_9"].tool_uses, 3);
    session.cost = 1.0;
    session.hear(&json!({ "type": "result", "subtype": "success", "is_error": false, "duration_ms": 14_000, "num_turns": 1, "total_cost_usd": 1.04, "usage": { "input_tokens": 9, "output_tokens": 1_500, "cache_read_input_tokens": 0 } }));
    let Some(Item::Worked(14_000, spend)) = session.items.last() else { panic!("{:?}", session.items) };
    assert_eq!(spend.output, 1_500);
    assert!((spend.cost.unwrap() - 0.04).abs() < 1e-9);
    assert_eq!(session.meter.tokens(), 0, "the next turn counts from nothing");
}

#[test]
fn a_resumed_transcript_times_the_cache_from_its_last_message_not_from_when_it_was_read() {
    let mut session = Session::default();
    session.now = 1_800_000_000_000;
    session.hear(&json!({ "type": "assistant", "timestamp": "2026-11-18T18:42:42.764Z", "message": { "id": "m1", "role": "assistant", "content": [], "usage": { "output_tokens": 5, "cache_creation": { "ephemeral_1h_input_tokens": 40, "ephemeral_5m_input_tokens": 0 } } } }));
    assert!(session.meter.hour);
    assert_eq!(session.meter.cache(session.now).as_deref(), Some("Cache expired"));
}

#[test]
fn a_finished_turn_says_how_long_it_worked() {
    let mut session = Session::default();
    session.send("go", &[], 1, 1);
    // Fields as the Agent SDK's SDKResultMessage names them.
    session.hear(&json!({ "type": "result", "subtype": "success", "is_error": false, "duration_ms": 20_400, "num_turns": 3 }));
    // One that answers a background task along with another did no turns of its own.
    session.hear(&json!({ "type": "result", "subtype": "success", "is_error": false, "duration_ms": 12, "num_turns": 0 }));
    // A failed turn says so instead.
    session.hear(&json!({ "type": "result", "subtype": "error_during_execution", "is_error": true, "duration_ms": 900, "num_turns": 1, "errors": ["Overloaded"] }));
    assert_eq!(session.items, [Item::User("go".into()), Item::Worked(20_400, Default::default()), Item::Error("Overloaded".into())]);
}

#[test]
fn the_turn_counts_its_subagents_and_waits_on_them_once_nothing_else_runs() {
    let mut session = Session::default();
    session.send("survey", &[], 1, 1);
    tool_call(&mut session, "s1", "Task", json!({ "description": "Callers" }));
    tool_call(&mut session, "s2", "Agent", json!({ "description": "Rounding" }));
    tool_call(&mut session, "b1", "Bash", json!({ "command": "ls" }));
    assert_eq!((session.turn_subagents(), session.waiting_on_subagents()), ((2, 2), None), "it runs a command too");
    tool_result(&mut session, "b1", "a.txt", false);
    tool_result(&mut session, "s1", "Three callers", false);
    assert_eq!((session.turn_subagents(), session.waiting_on_subagents()), ((2, 1), Some(1)));
    session.thinking = true;
    assert_eq!(session.waiting_on_subagents(), None, "Claude thinks");
    // The next message starts a turn with none, even with one left running.
    session.thinking = false;
    session.send("next", &[], 1, 1);
    assert_eq!((session.turn_subagents(), session.waiting_on_subagents()), ((0, 0), None));
}

#[test]
fn a_model_pick_keeps_the_effort_an_effort_pick_keeps_the_model_and_the_levels_follow_the_model() {
    let answer = |id: &str, response: Value| json!({ "type": "control_response", "response": { "subtype": "success", "request_id": id, "response": response } });
    let mut session = Session::default();
    session.hear(&answer(INITIALIZE, json!({ "models": [
        { "value": "opus", "displayName": "Opus 5.5", "supportedEffortLevels": ["high", "max"], "resolvedModel": "claude-opus-5-5" },
        { "value": "sonnet", "displayName": "Sonnet 5.5", "supportedEffortLevels": ["low", "high"], "resolvedModel": "claude-sonnet-5-5" },
        { "value": "haiku", "displayName": "Haiku 5.5", "resolvedModel": "claude-haiku-5-5" },
    ] })));
    session.hear(&answer(SETTINGS, json!({ "effective": { "model": "opus", "effortLevel": "high" } })));
    let requests = |lines: String| parsed(&lines).iter().map(|line| line["request"].clone()).collect::<Vec<_>>();
    let effort = |level: &str| json!({ "subtype": "apply_flag_settings", "settings": { "effortLevel": level } });
    let model = |name: &str| json!({ "subtype": "set_model", "model": name });
    // The one in use, and one not listed, change nothing.
    assert_eq!(session.pick_model("opus", 1), None);
    assert_eq!(session.pick_model("nope", 1), None);
    // An effort alone leaves the model; one the model doesn't list, or the one shown, sends nothing.
    assert_eq!(requests(session.pick_effort("max", 2).unwrap()), [effort("max")]);
    assert_eq!(session.current_model().map(|m| m.value.as_str()), Some("opus"));
    assert_eq!(session.pick_effort("max", 3), None);
    assert_eq!(session.pick_effort("low", 3), None, "opus has no low");
    // A model that takes the effort in use keeps it: only the model is sent.
    session.pick_effort("high", 4);
    assert_eq!(requests(session.pick_model("sonnet", 5).unwrap()), [model("sonnet")]);
    assert_eq!(session.shown_effort().as_deref(), Some("high"));
    // One that doesn't list it gets High, else its first.
    session.pick_effort("low", 6);
    assert_eq!(requests(session.pick_model("opus", 7).unwrap()), [model("opus"), effort("high")]);
    // A model without effort has no levels, and the effort comes back when another takes it.
    assert_eq!(requests(session.pick_model("haiku", 8).unwrap()), [model("haiku")]);
    assert_eq!(session.shown_effort(), None);
    assert_eq!(session.pick_effort("high", 9), None);
    assert_eq!(requests(session.pick_model("sonnet", 10).unwrap()), [model("sonnet"), effort("high")]);
}

/// Claude Code 2.1.327's answers to `get_settings` and `mcp_status`, with made-up plugins and servers.
const PLUGINS: &str = include_str!("../../tests/plugins.jsonl");

fn with_plugins() -> Session {
    heard(PLUGINS)
}

#[test]
fn plugins_plugin_and_mcp_open_the_dialog_ask_only_to_read_and_are_not_sent_as_messages() {
    for (text, show) in [("/plugins", Show::Plugins), (" /plugin\n", Show::Plugins), ("/mcp", Show::Mcp)] {
        let mut session = Session::default();
        let Some(crate::builtin::Local::Dialog(opened)) = crate::builtin::find(text, &[]) else { panic!("{text}") };
        let line = session.plugins.ask();
        assert_eq!(opened, show);
        // One write of two requests that read, no message, and nothing in the conversation.
        let subtypes: Vec<Value> = parsed(&line).iter().map(|sent| sent["request"]["subtype"].clone()).collect();
        assert_eq!(subtypes, [json!("get_settings"), json!("mcp_status")]);
        assert!(parsed(&line).iter().all(|sent| sent["type"] == "control_request"));
        assert!(session.items.is_empty() && !session.working);
    }
}

#[test]
fn the_settings_answer_lists_each_plugin_on_or_off_where_your_settings_decide_it() {
    let session = with_plugins();
    let list = session.plugins.list.as_ref().expect("the plugins");
    let shown: Vec<(&str, &str, bool, bool, &str)> = list.iter().map(|p| (p.name.as_str(), p.marketplace.as_str(), p.on, p.saved, p.scope)).collect();
    assert_eq!(
        shown,
        [
            // Only your organization's settings name it, so the chat can't turn it off.
            ("audit-log", "company-policy", true, true, "Managed"),
            // The project's settings come after yours and turn it on.
            ("code-search", "claude-plugins-official", true, true, "Project"),
            ("lint-guard", "team-tools", false, false, "User"),
            // On in your settings, off in this session's flags.
            ("pixel-perfect", "claude-plugins-official", false, true, "User"),
            ("release-notes", "team-tools", true, true, "User"),
            ("toolbox", "team-tools", true, true, "User"),
        ]
    );
    assert!(list[0].locked() && !list[1].locked());
}

#[test]
fn the_status_answer_lists_each_server_with_its_status_and_a_plugins_under_it() {
    let session = with_plugins();
    let servers = session.plugins.servers.as_ref().expect("the servers");
    let files = &servers[0];
    assert_eq!((files.of_plugin(), files.short_name(), files.status.as_str(), files.tools), (Some(("toolbox", "files")), "files", "connected", Some(3)));
    let index = servers.iter().find(|server| server.short_name() == "index").unwrap();
    assert_eq!((index.status.as_str(), index.error.as_deref(), index.tools), ("failed", Some("Connection closed"), None));
    let own: Vec<(&str, &str, &str)> = servers.iter().filter(|server| server.of_plugin().is_none()).map(|s| (s.name.as_str(), s.status.as_str(), s.scope.as_str())).collect();
    assert_eq!(
        own,
        [
            ("notes", "connected", "user"),
            ("design-files", "disabled", "user"),
            ("tickets", "connected", "project"),
            ("claude.ai Calendar", "needs-auth", "claudeai"),
            ("claude.ai Drive", "pending", "claudeai"),
        ]
    );
}

#[test]
fn a_command_with_a_plugin_key_waits_for_the_turns_end_and_no_other_does() {
    let mut session = Session::default();
    session.commands = commands(&json!([
        { "name": "toolbox:lint", "description": "", "argumentHint": "", "plugin": "toolbox" },
        { "name": "review", "description": "", "argumentHint": "" },
        { "name": "plain", "description": "", "argumentHint": "", "plugin": null },
    ]));
    assert!(session.runs_after_turn("/toolbox:lint src"));
    assert!(session.runs_after_turn(" /toolbox:lint"));
    assert!(!session.runs_after_turn("/review"));
    assert!(!session.runs_after_turn("/plain"));
    assert!(!session.runs_after_turn("toolbox:lint"), "a message that only names it");
    assert!(!session.runs_after_turn("/toolbox:lint-all"));
}

#[test]
fn a_plugins_skills_are_the_commands_named_after_it() {
    let commands = |names: &[&str]| names.iter().map(|name| Command { name: (*name).into(), ..Command::default() }).collect::<Vec<_>>();
    let all = commands(&["toolbox:lint", "toolbox:format", "toolbox-extra:x", "review", "release-notes:write"]);
    assert_eq!([plugins::skills(&all, "toolbox"), plugins::skills(&all, "release-notes"), plugins::skills(&all, "lint-guard")], [2, 1, 0]);
}

#[test]
fn a_plugin_toggle_is_one_flag_settings_request_with_every_plugin_this_chat_set_and_never_a_settings_file() {
    let mut session = with_plugins();
    let line = session.plugins.set_plugin("toolbox@team-tools", false, 7).expect("a toggle");
    assert_eq!(line.lines().count(), 1);
    let sent = sent(&line);
    assert_eq!(sent["request_id"], "plugin-toggle-7");
    // The flag settings take a second `enabledPlugins` in place of the first, so the one already there goes too.
    assert_eq!(
        sent["request"],
        json!({ "subtype": "apply_flag_settings", "settings": { "enabledPlugins": { "pixel-perfect@claude-plugins-official": false, "toolbox@team-tools": false } } })
    );
    let toolbox = session.plugins.list.as_ref().unwrap().iter().find(|p| p.name == "toolbox").unwrap();
    assert!(!toolbox.on && toolbox.saved && session.plugins.unapplied);
    // Off already, so nothing to send; and your organization's plugin can't be turned off here.
    assert_eq!(session.plugins.set_plugin("toolbox@team-tools", false, 8), None);
    assert_eq!(session.plugins.set_plugin("audit-log@company-policy", false, 9), None);
    assert_eq!(session.plugins.set_plugin("unknown@nowhere", true, 10), None);
}

#[test]
fn a_server_toggle_is_one_mcp_toggle_request_by_its_full_name() {
    let mut session = with_plugins();
    let line = session.plugins.set_server("plugin:toolbox:files", false, 3).expect("a toggle");
    assert_eq!(line.lines().count(), 1);
    assert_eq!(sent(&line)["request"], json!({ "subtype": "mcp_toggle", "serverName": "plugin:toolbox:files", "enabled": false }));
    assert_eq!(session.plugins.servers.as_ref().unwrap()[0].status, "disabled");
    assert_eq!(session.plugins.set_server("plugin:toolbox:files", false, 4), None, "off already");
    // Turned on, it connects anew, which only a new read shows.
    let on = session.plugins.set_server("design-files", true, 5).expect("a toggle");
    assert_eq!(sent(&on)["request"]["enabled"], true);
    assert!(!session.plugins.stale);
    session.hear(&json!({ "type": "control_response", "response": { "subtype": "success", "request_id": "server-toggle-5" } }));
    assert!(session.plugins.stale);
}

#[test]
fn a_refused_toggle_is_put_back_and_says_why() {
    let mut session = with_plugins();
    session.plugins.set_plugin("toolbox@team-tools", false, 1).unwrap();
    session.plugins.set_server("notes", false, 2).unwrap();
    let refused = |id: &str, why: &str| json!({ "type": "control_response", "response": { "subtype": "error", "request_id": id, "error": why } });
    session.hear(&refused("plugin-toggle-1", "apply_flag_settings failed"));
    let toolbox = session.plugins.list.as_ref().unwrap().iter().find(|p| p.name == "toolbox").unwrap();
    assert!(toolbox.on && !session.plugins.unapplied);
    session.hear(&refused("server-toggle-2", "Server not found: notes"));
    assert_eq!(session.plugins.servers.as_ref().unwrap()[3].status, "connected");
    assert_eq!(session.plugins.error.as_deref(), Some("Server not found: notes"));
    // The next toggle sends only what this chat set and Claude Code kept.
    let line = session.plugins.set_plugin("lint-guard@team-tools", true, 3).unwrap();
    assert_eq!(sent(&line)["request"]["settings"]["enabledPlugins"], json!({ "pixel-perfect@claude-plugins-official": false, "lint-guard@team-tools": true }));
}

/// Claude Code 2.1.330 lists its own `/mcp`, `/usage` and `/context` in `initialize`'s answer: they still open here,
/// and a plugin's command of the same name still goes to Claude Code.
#[test]
fn claude_codes_own_listed_built_ins_open_here_and_a_plugins_namesake_does_not() {
    use crate::builtin::{Local, find};
    let mut session = Session::default();
    session.hear(&json!({ "type": "control_response", "response": { "subtype": "success", "request_id": "initialize", "response": { "commands": [
        { "name": "mcp", "description": "Manage MCP servers", "argumentHint": "[reconnect|enable|disable [<server>|all]]", "builtin": true },
        { "name": "usage", "description": "Show session cost, plan usage, and what's contributing to your limits", "argumentHint": "", "aliases": ["cost", "stats"], "builtin": true },
        { "name": "context", "description": "Show current context usage", "argumentHint": "", "builtin": true },
        { "name": "permissions", "description": "Review the rules", "argumentHint": "" }] } } }));
    assert!(session.commands.iter().take(3).all(|command| command.builtin));
    assert_eq!(find("/mcp", &session.commands), Some(Local::Dialog(Show::Mcp)));
    assert_eq!(find("/usage", &session.commands), Some(Local::Panel(crate::usage::Panel::Usage)));
    assert_eq!(find("/cost", &session.commands), Some(Local::Panel(crate::usage::Panel::Usage)));
    assert_eq!(find("/context", &session.commands), Some(Local::Panel(crate::usage::Panel::Context)));
    assert_eq!(find("/permissions", &session.commands), None);
}

#[test]
fn a_reload_applies_the_toggles_and_brings_the_servers_and_commands_as_they_are_now() {
    let mut session = with_plugins();
    session.plugins.set_plugin("toolbox@team-tools", false, 1).unwrap();
    let reload = session.plugins.reload();
    assert_eq!(reload.lines().count(), 1);
    assert_eq!(sent(&reload)["request"], json!({ "subtype": "reload_plugins", "refuse_if_busy": true }));
    session.hear(&json!({ "type": "control_response", "response": { "subtype": "success", "request_id": "plugins-reload", "response": {
        "commands": [{ "name": "release-notes:write", "description": "Write the release notes", "argumentHint": "" }],
        "agents": [], "plugins": [{ "name": "release-notes", "path": "/plugins/release-notes", "source": "release-notes@team-tools" }],
        "mcpServers": [{ "name": "notes", "status": "connected", "scope": "user", "source": "user" }], "error_count": 0 } } }));
    assert!(!session.plugins.unapplied);
    assert_eq!(session.commands.iter().map(|command| command.name.as_str()).collect::<Vec<_>>(), ["release-notes:write"]);
    assert_eq!(session.plugins.servers.as_ref().unwrap().len(), 1);
    // A busy session reloads nothing, and says so.
    session.plugins.reload();
    session.hear(&json!({ "type": "control_response", "response": { "subtype": "error", "request_id": "plugins-reload", "error": "The session is busy" } }));
    assert_eq!(session.plugins.error.as_deref(), Some("The session is busy"));
    assert_eq!(session.commands.len(), 1);
}

/// What Claude Code 2.1.327 printed while two commands ran with `run_in_background`, one finishing on its own, with
/// made-up ids and descriptions.
fn live(tasks: &[(&str, &str)]) -> Value {
    let tasks: Vec<Value> = tasks.iter().map(|(id, description)| json!({ "task_id": id, "run_id": format!("run-{id}"), "task_type": "local_bash", "description": description })).collect();
    json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": tasks, "uuid": "7a0e5b52-0c1f-4d39-8f0b-2b3c4d5e6f70", "session_id": "s" })
}

fn started(id: &str, description: &str) -> Value {
    json!({ "type": "system", "subtype": "task_started", "task_id": id, "run_id": format!("run-{id}"), "tool_use_id": format!("toolu_{id}"),
        "description": description, "is_backgrounded": true, "task_type": "local_bash", "uuid": "8b1f6c63-1d20-4e4a-9a1c-3c4d5e6f7081", "session_id": "s" })
}

fn task_end(id: &str, updated: &str, notified: &str) -> [Value; 2] {
    [
        json!({ "type": "system", "subtype": "task_updated", "task_id": id, "run_id": format!("run-{id}"), "patch": { "status": updated, "end_time": 1_795_027_323_607u64 }, "uuid": "9c2a7d74-2e31-4f5b-8b2d-4d5e6f708192", "session_id": "s" }),
        json!({ "type": "system", "subtype": "task_notification", "task_id": id, "run_id": format!("run-{id}"), "tool_use_id": format!("toolu_{id}"), "status": notified,
            "output_file": format!("/tmp/claude/tasks/{id}.output"), "summary": "Background command \"Run the tests\" completed (exit code 0)", "uuid": "ad3b8e85-3f42-405c-9c3e-5e6f708192a3", "session_id": "s" }),
    ]
}

fn answered_with(id: &str, response: Value) -> Value {
    json!({ "type": "control_response", "response": { "subtype": "success", "request_id": id, "response": response } })
}

#[test]
fn background_tasks_follow_the_live_set_and_say_how_each_ended() {
    let mut session = Session::default();
    session.now = 1_000;
    session.hear(&live(&[("bk1", "Run the tests")]));
    session.hear(&started("bk1", "Run the tests"));
    session.now = 1_070;
    session.hear(&live(&[("bk1", "Run the tests"), ("bk2", "Watch the build")]));
    session.hear(&started("bk2", "Watch the build"));
    let running: Vec<(&str, &str, &str, u64)> = session.tasks.iter().map(|t| (t.id.as_str(), t.kind.as_str(), t.description.as_str(), t.started)).collect();
    assert_eq!(running, [("bk1", "local_bash", "Run the tests", 1_000), ("bk2", "local_bash", "Watch the build", 1_070)]);
    assert!(session.tasks.iter().all(|task| task.ended.is_none() && task.has_output()));
    // The first ends on its own: its update and notice come before the set it leaves.
    session.now = 6_000;
    for line in task_end("bk1", "completed", "completed") {
        session.hear(&line);
    }
    session.hear(&live(&[("bk2", "Watch the build")]));
    assert_eq!(session.tasks[0].ended.as_deref(), Some("completed"));
    assert_eq!((session.tasks[1].ended.as_deref(), session.tasks[1].started), (None, 1_070));
    // One that leaves the set with no notice has ended all the same.
    session.hear(&live(&[]));
    assert_eq!(session.tasks[1].ended.as_deref(), Some("ended"));
    // Listed again, it is a new run, timed from now.
    session.hear(&live(&[("bk2", "Watch the build")]));
    assert_eq!((session.tasks[1].ended.as_deref(), session.tasks[1].started), (None, 6_000));
    assert_eq!(session.tasks.len(), 2);
}

#[test]
fn a_stop_click_is_one_stop_task_request_and_the_task_ends_as_claude_code_says() {
    let mut session = heard(&live(&[("bk1", "Sleep then say done")]).to_string());
    let line = session.stop_task("bk1").expect("a stop");
    assert_eq!(parsed(&line), [json!({ "type": "control_request", "request_id": "task-stop-bk1", "request": { "subtype": "stop_task", "task_id": "bk1" } })]);
    assert!(session.tasks[0].stopping);
    assert_eq!(session.stop_task("bk1"), None, "stopping already");
    assert_eq!(session.stop_task("nope"), None);
    // As Claude Code 2.1.327 answered and ended it.
    session.hear(&answered_with("task-stop-bk1", json!({})));
    for line in task_end("bk1", "killed", "stopped") {
        session.hear(&line);
    }
    session.hear(&live(&[]));
    assert_eq!((session.tasks[0].ended.as_deref(), session.tasks[0].stopping), (Some("stopped"), false));
    assert_eq!(session.stop_task("bk1"), None, "ended");
    // A refused stop lets you try again, and says why.
    let mut refused = heard(&live(&[("bk2", "Watch the build")]).to_string());
    refused.stop_task("bk2").unwrap();
    refused.hear(&json!({ "type": "control_response", "response": { "subtype": "error", "request_id": "task-stop-bk2", "error": "No task found with ID: bk2" } }));
    assert_eq!((refused.tasks[0].stopping, refused.tasks[0].error.as_deref()), (false, Some("No task found with ID: bk2")));
    assert!(refused.stop_task("bk2").is_some());
}

#[test]
fn opening_a_tasks_output_is_one_read_and_its_answer_shows_the_end_of_what_it_printed() {
    let mut session = heard(&live(&[("bk1", "Watch the build")]).to_string());
    let line = session.read_task("bk1").expect("a read");
    assert_eq!(parsed(&line), [json!({ "type": "control_request", "request_id": "task-output-bk1", "request": { "subtype": "get_task_output", "task_id": "bk1" } })]);
    // As Claude Code 2.1.327 answered while it ran, and once it had ended.
    session.hear(&answered_with("task-output-bk1", json!({ "output": "tock 1\ntock 2\ntock 3\n", "total_bytes": 21, "truncated": false })));
    assert_eq!(session.tasks[0].output, Some(Output { text: "tock 1\ntock 2\ntock 3\n".into(), bytes: 21, truncated: false }));
    session.hear(&answered_with("task-output-bk1", json!({ "output": "\n[killed]\n", "total_bytes": 10, "truncated": false })));
    assert_eq!(session.tasks[0].output.as_ref().map(|output| output.text.as_str()), Some("\n[killed]\n"));
    // A subagent sent to the background has no output Claude Code reads.
    let mut agent = heard(&json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": [{ "task_id": "a1", "task_type": "local_agent", "description": "Survey" }] }).to_string());
    assert_eq!(agent.read_task("a1"), None);
    assert_eq!(agent.read_task("nope"), None);
}

const ONE: &str = "11111111-1111-4111-8111-111111111111";

/// Two messages of yours sent from the chat, the second at `stamp` 2, each answered.
fn two_turns() -> (Session, String, String) {
    let mut session = Session::default();
    let first: Value = serde_json::from_str(session.send("Write notes.txt with one", &[], 1, 1).lines().next().unwrap()).unwrap();
    session.hear(&json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": "OK" }] } }));
    session.hear(&json!({ "type": "result", "subtype": "success", "is_error": false }));
    let second: Value = serde_json::from_str(session.send("Change one to two", &[], 2, 3).lines().next().unwrap()).unwrap();
    session.hear(&json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": "Done" }] } }));
    session.hear(&json!({ "type": "result", "subtype": "success", "is_error": false }));
    let uuid = |sent: &Value| sent["uuid"].as_str().unwrap().to_owned();
    (session, uuid(&first), uuid(&second))
}

#[test]
fn a_message_sent_while_claude_is_idle_goes_under_a_uuid_a_rewind_names() {
    let (session, first, second) = two_turns();
    assert_ne!(first, second);
    assert_eq!(first.len(), 36);
    let prompts: Vec<(usize, &str)> = session.prompts.iter().map(|prompt| (prompt.at, prompt.uuid.as_str())).collect();
    assert_eq!(prompts, [(0, first.as_str()), (2, second.as_str())]);
    // A queued one keeps its own once it is read.
    let mut queued = Session::default();
    queued.hear(&json!({ "type": "system", "subtype": "session_state_changed", "state": "running" }));
    queued.queue("and lint", Vec::new(), 5, 6);
    let uuid = queued.queued[0].uuid.clone();
    queued.hear(&json!({ "type": "user", "isReplay": true, "uuid": uuid, "message": { "role": "user", "content": "and lint" } }));
    assert_eq!(queued.prompts, [Prompt { at: 0, uuid, parent: None }]);
}

#[test]
fn the_rewind_dialog_opens_with_a_dry_run_that_lists_what_restoring_the_code_changes() {
    let (mut session, first, second) = two_turns();
    let line = session.open_rewind(&second).expect("a dry run");
    assert_eq!(
        parsed(&line),
        [json!({ "type": "control_request", "request_id": format!("rewind-check-{second}"), "request": { "subtype": "rewind_files", "user_message_id": second, "dry_run": true } })]
    );
    let rewind = session.rewind.as_ref().unwrap();
    assert_eq!((rewind.text.as_str(), &rewind.check), ("Change one to two", &None));
    // Another message's dry run doesn't fill this one.
    session.hear(&answered_with(&format!("rewind-check-{first}"), json!({ "canRewind": false, "error": "No file checkpoint found for this message." })));
    assert_eq!(session.rewind.as_ref().unwrap().check, None);
    // As Claude Code 2.1.327 answered it, with a made-up path.
    session.hear(&answered_with(&format!("rewind-check-{second}"), json!({ "canRewind": true, "filesChanged": ["/work/shop/notes.txt"], "insertions": 1, "deletions": 1 })));
    assert_eq!(session.rewind.as_ref().unwrap().check, Some(Ok(Check { files: vec!["/work/shop/notes.txt".into()], added: 1, removed: 1 })));
    // Without checkpoints it says why, as Claude Code does.
    session.open_rewind(&first).unwrap();
    session.hear(&answered_with(&format!("rewind-check-{first}"), json!({ "canRewind": false, "error": "File rewinding is not enabled." })));
    assert_eq!(session.rewind.as_ref().unwrap().check, Some(Err("File rewinding is not enabled.".into())));
    assert_eq!(session.open_rewind(ONE), None, "not a message of this chat");
}

#[test]
fn each_rewind_choice_is_one_write_of_what_it_restores() {
    let (mut session, first, second) = two_turns();
    session.open_rewind(&first).unwrap();
    // Code isn't restored before the dry run has listed what it changes.
    assert_eq!(session.restore(Restore::Code), None);
    assert_eq!(session.restore(Restore::Both), None);
    session.hear(&answered_with(&format!("rewind-check-{first}"), json!({ "canRewind": true, "filesChanged": ["/work/shop/notes.txt"], "insertions": 0, "deletions": 1 })));
    let files = json!({ "type": "control_request", "request_id": "rewind-files", "request": { "subtype": "rewind_files", "user_message_id": first } });
    let conversation = json!({ "type": "control_request", "request_id": "rewind-conversation",
        "request": { "subtype": "rewind_conversation", "target_message_uuid": first, "last_seen_user_message_uuid": second } });
    assert_eq!(parsed(&session.restore(Restore::Both).unwrap()), [files.clone(), conversation.clone()]);
    assert_eq!(session.restore(Restore::Conversation), None, "waiting on the first choice");
    for (how, sent) in [(Restore::Code, vec![files]), (Restore::Conversation, vec![conversation])] {
        session.rewind.as_mut().unwrap().waiting.clear();
        assert_eq!(parsed(&session.restore(how).unwrap()), sent);
    }
    // With no files changed there is no code to restore; the conversation still can be.
    let (mut quiet, first, _) = two_turns();
    quiet.open_rewind(&first).unwrap();
    quiet.hear(&answered_with(&format!("rewind-check-{first}"), json!({ "canRewind": true, "filesChanged": [], "insertions": 0, "deletions": 0 })));
    assert_eq!(quiet.restore(Restore::Code), None);
    assert!(quiet.restore(Restore::Conversation).is_some());
}

#[test]
fn a_restored_conversation_drops_the_message_and_after_and_gives_it_back_to_send_again() {
    let (mut session, _, second) = two_turns();
    session.open_rewind(&second).unwrap();
    session.hear(&answered_with(&format!("rewind-check-{second}"), json!({ "canRewind": true, "filesChanged": ["/work/shop/notes.txt"], "insertions": 1, "deletions": 1 })));
    session.restore(Restore::Both).unwrap();
    // As Claude Code 2.1.327 answered the two, in the order sent.
    session.hear(&answered_with("rewind-files", json!({ "canRewind": true, "skippedLinks": 0 })));
    assert_eq!(session.rewind.as_ref().unwrap().waiting, [REWIND_CONVERSATION]);
    session.hear(&answered_with("rewind-conversation", json!({ "rewound": true, "targetMessageUuid": second,
        "prefillText": "Change one to two", "precedingAssistantUuid": "5cc4bf2b-6279-4085-9a7b-75fa1de128b9" })));
    assert_eq!(session.items, [Item::User("Write notes.txt with one".into()), Item::Text("OK".into())]);
    assert_eq!(session.prompts.len(), 1);
    assert_eq!(session.restored.as_deref(), Some("Change one to two"));
    assert!(session.rewind.is_none(), "done, so closed");
}

#[test]
fn a_refused_restore_says_why_and_keeps_the_dialog_open() {
    let (mut session, first, _) = two_turns();
    session.open_rewind(&first).unwrap();
    session.restore(Restore::Conversation).unwrap();
    // As Claude Code 2.1.327 refused one that named no message as seen.
    session.hear(&answered_with("rewind-conversation", json!({ "rewound": false, "prefillText": null, "precedingAssistantUuid": null, "error": "stale target", "reason": "stale_target" })));
    let rewind = session.rewind.as_ref().unwrap();
    assert_eq!((rewind.error.as_deref(), rewind.waiting.is_empty()), (Some("Couldn't restore the conversation: stale target"), true));
    assert_eq!(session.items.len(), 4);
    assert_eq!(session.restored, None);
    // Closed before the answer, a failure shows in the conversation.
    session.rewind = None;
    session.hear(&json!({ "type": "control_response", "response": { "subtype": "error", "request_id": "rewind-files", "error": "Failed to rewind: EACCES" } }));
    assert_eq!(session.items.last(), Some(&Item::Error("Couldn't restore the code: Failed to rewind: EACCES".into())));
}

#[test]
fn a_transcript_read_again_leaves_out_what_a_rewind_dropped() {
    // Entries as Claude Code 2.1.327 wrote them, with made-up uuids: the second message was rewound, and the next one
    // follows the entry it followed, which the chat can't read.
    let entry = |kind: &str, uuid: &str, parent: &str, content: Value| {
        json!({ "parentUuid": parent, "isSidechain": false, "type": kind, "message": { "role": kind, "content": content }, "uuid": uuid, "timestamp": "2026-11-18T18:42:42.764Z" })
    };
    let said = |text: &str| json!([{ "type": "text", "text": text }]);
    let mut session = Session::default();
    for line in [
        entry("user", "u1", "a0", json!("Write notes.txt with one")),
        entry("assistant", "r1", "a1", said("OK")),
        entry("user", "u2", "s1", json!("Start two commands in the background")),
        entry("assistant", "r2", "a2", said("Both run")),
        entry("user", "u3", "s1", json!("List my messages")),
        entry("assistant", "r3", "a3", said("One, and this one")),
    ] {
        session.hear(&line);
    }
    assert_eq!(
        session.items,
        [Item::User("Write notes.txt with one".into()), Item::Text("OK".into()), Item::User("List my messages".into()), Item::Text("One, and this one".into())]
    );
    let prompts: Vec<(usize, &str)> = session.prompts.iter().map(|prompt| (prompt.at, prompt.uuid.as_str())).collect();
    assert_eq!(prompts, [(0, "u1"), (2, "u3")]);
}

#[test]
fn the_opening_input_has_claude_code_keep_the_checkpoints_a_code_rewind_restores() {
    let lines = parsed(&Session::opening(None, None));
    assert!(lines.contains(&json!({ "type": "control_request", "request_id": "checkpoints",
        "request": { "subtype": "apply_flag_settings", "settings": { "env": { "CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING": "1" } } } })));
}

/// A Read of a small generated png as Claude Code 2.1.327 printed it: the tool result is an image block, and the line
/// carries the picture a second time as `tool_use_result`.
const IMAGE_TURN: &str = include_str!("../../tests/image_turn.jsonl");

fn tool_images(session: &Session) -> Vec<Vec<String>> {
    session.items.iter().filter_map(|item| if let Item::Tool(tool) = item { Some(tool.images.clone()) } else { None }).collect()
}

#[test]
fn a_picture_a_tool_gives_back_shows_in_its_step_and_goes_to_wings_once() {
    images::reset();
    let session = heard(IMAGE_TURN);
    let shown = tool_images(&session);
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].len(), 1);
    let Some(Item::Tool(read)) = session.items.first() else {
        panic!("{:?}", session.items)
    };
    // It printed no text, and the picture is the id of what Wings was given.
    assert_eq!(read.output, Some((String::new(), false)));
    let kept = images::pending();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].0, shown[0][0]);
    assert!(kept[0].1.starts_with("iVBORw0KGgo"));
    assert!(images::is_shown(&kept[0].0));
}

#[test]
fn a_transcripts_pictures_come_as_ids_wings_registered() {
    images::reset();
    let session = heard(concat!(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"image","source":{"type":"registered","id":"tabc"}},{"type":"text","text":"look"}]}}"#,
        "\n",
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"c1","name":"Read","input":{"file_path":"/tmp/a.png"}}]}}"#,
        "\n",
        r#"{"type":"user","message":{"role":"user","content":[{"tool_use_id":"c1","type":"tool_result","content":[{"type":"image","source":{"type":"registered","id":"tdef"}},{"type":"image","source":{"type":"unavailable"}}]}]}}"#,
    ));
    assert_eq!(session.items[0], Item::UserImages("look".into(), vec!["tabc".into()]));
    assert_eq!(tool_images(&session), [["tdef".to_owned(), "gone".to_owned()]]);
    // Nothing is sent to Wings: it kept them as it read the transcript.
    assert!(images::pending().is_empty());
    assert!(images::is_shown("tabc") && images::is_shown("tdef") && !images::is_shown("gone"));
}

#[test]
fn every_picture_shows_however_many_came_before_and_an_oversize_one_is_not_sent() {
    images::reset();
    let mut session = Session::default();
    let pictures: Vec<(String, String)> = (0..40).map(|n| ("image/png".to_owned(), format!("A{n:03}"))).collect();
    session.send("many", &pictures, 1, 1);
    let Some(Item::UserImages(_, ids)) = session.items.first() else {
        panic!("{:?}", session.items)
    };
    assert_eq!(ids.len(), 40);
    assert!(ids.iter().all(|id| images::is_shown(id)), "the oldest pictures show too");
    // Beyond what Wings keeps, the picture isn't copied into the page's queue at all.
    images::reset();
    let big = "A".repeat(5 * 1024 * 1024 / 3 * 4 + 8);
    assert_eq!(images::keep(&big), "gone");
    assert!(images::pending().is_empty());
}

#[test]
fn a_queued_message_shows_its_pictures_once_claude_code_reads_it() {
    images::reset();
    let mut session = Session::default();
    session.queue("look", vec![("image/gif".into(), "CCCC".into())], 1, 1);
    assert!(images::pending().is_empty());
    let uuid = session.queued[0].uuid.clone();
    session.hear(&json!({ "type": "user", "isReplay": true, "uuid": uuid, "message": { "role": "user", "content": "look" } }));
    let Some(Item::UserImages(text, ids)) = session.items.first() else {
        panic!("{:?}", session.items)
    };
    assert_eq!((text.as_str(), ids.len()), ("look", 1));
    assert_eq!(images::pending(), [(ids[0].clone(), "CCCC".to_owned())]);
}

#[test]
fn a_tool_result_shows_at_most_a_few_pictures() {
    images::reset();
    let shot = json!({ "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "AAAA" } });
    let session = heard(&[
        json!({ "type": "assistant", "message": { "role": "assistant", "content": [{ "type": "tool_use", "id": "c1", "name": "mcp__shots__all", "input": {} }] } }),
        json!({ "type": "user", "message": { "role": "user", "content": [{ "tool_use_id": "c1", "type": "tool_result", "content": vec![shot; 10] }] } }),
    ].map(|line| line.to_string()).join("\n"));
    assert_eq!(tool_images(&session)[0].len(), images::PER_RESULT);
    assert_eq!(images::pending().len(), images::PER_RESULT);
}

#[test]
fn the_opening_input_asks_for_the_text_of_thinking_and_leaves_the_budget_alone() {
    let lines = parsed(&Session::opening(None, None));
    // Probed with Claude Code 2.1.327: with this alone the thinking blocks carry a summary, and the request is accepted.
    let asked = lines.iter().find(|line| line["request"]["subtype"] == "set_max_thinking_tokens").expect("a thinking request");
    assert_eq!(asked["request"], json!({ "subtype": "set_max_thinking_tokens", "thinking_display": "summarized" }));
}

#[test]
fn a_thought_is_kept_as_its_summary_with_how_long_it_took_and_one_without_text_is_left_out() {
    let mut session = Session::default();
    session.now = 1_000;
    session.hear(&json!({ "type": "stream_event", "event": { "type": "content_block_start", "index": 0, "content_block": { "type": "thinking", "thinking": "", "signature": "" } } }));
    assert!(session.thinking);
    session.now = 13_000;
    session.hear(&json!({ "type": "assistant", "message": { "content": [{ "type": "thinking", "thinking": "I'm checking the riddle.\n\n", "signature": "sig" }] } }));
    assert_eq!(session.items, [Item::Thought("I'm checking the riddle.".into(), 12_000)]);
    // Without a summary asked for, Claude Code sends the block with an empty text.
    session.hear(&json!({ "type": "assistant", "message": { "content": [{ "type": "thinking", "thinking": "", "signature": "sig" }] } }));
    assert_eq!(session.items.len(), 1);
}

// What Claude Code 2.1.327 printed, which has no `session_state_changed`: a turn starts with a `status` of `requesting` and ends
// with its `result`, and `command_lifecycle` follows each message of yours.
fn turn_start() -> Value {
    json!({ "type": "system", "subtype": "status", "status": "requesting", "session_id": "s" })
}

fn turn_end(answered: &[&String], origin: Option<&str>) -> Value {
    json!({ "type": "result", "subtype": "success", "is_error": false, "num_turns": 1, "user_message_uuids": answered, "origin": origin.map(|kind| json!({ "kind": kind })) })
}

fn lifecycle(uuid: &str, state: &str) -> Value {
    json!({ "type": "command_lifecycle", "command_uuid": uuid, "state": state, "session_id": "s" })
}

fn thinks() -> Value {
    json!({ "type": "assistant", "message": { "content": [{ "type": "thinking", "thinking": "Reading the request.", "signature": "sig" }] } })
}

#[test]
fn a_turn_a_finished_task_starts_on_its_own_is_working_from_its_first_line_to_its_result() {
    // No message of yours began it, as when a background command ends: its notice, then `init`, thinking, one command.
    let mut session = Session::default();
    session.now = 1_000;
    session.hear(&live(&[("bk1", "Run the tests")]));
    session.hear(&started("bk1", "Run the tests"));
    for line in task_end("bk1", "completed", "completed") {
        session.hear(&line);
    }
    session.hear(&live(&[]));
    assert!(!session.working);
    session.hear(&turn_start());
    assert!(session.working);
    session.hear(&thinks());
    session.hear(&json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "id": "toolu_1", "name": "Bash", "input": { "command": "ls" } }] } }));
    assert!(session.working, "while it thinks and runs a command");
    session.hear(&json!({ "type": "user", "message": { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": "toolu_1", "content": "a" }] } }));
    assert!(session.working);
    session.hear(&turn_end(&[], Some("task-notification")));
    assert!(!session.working);
}

#[test]
fn a_long_command_that_goes_on_in_the_background_leaves_the_turn_ended_and_its_end_starts_another() {
    let mut session = Session::default();
    session.send("Run the slow job", &[], 1, 1);
    let sent = uuid(1, 1);
    session.hear(&lifecycle(&sent, "queued"));
    session.hear(&lifecycle(&sent, "started"));
    session.hear(&turn_start());
    session.hear(&live(&[("bk1", "Slow job")]));
    session.hear(&started("bk1", "Slow job"));
    assert!(session.working);
    session.hear(&turn_end(&[&sent], None));
    session.hear(&lifecycle(&sent, "completed"));
    assert!(!session.working, "the turn is over while the job runs on");
    for line in task_end("bk1", "completed", "completed") {
        session.hear(&line);
    }
    session.hear(&turn_start());
    assert!(session.working);
    session.hear(&turn_end(&[], Some("task-notification")));
    assert!(!session.working);
}

#[test]
fn a_message_sent_while_claude_works_keeps_it_working_across_the_turn_that_read_it() {
    let mut session = Session::default();
    session.send("Run sleep 8", &[], 1, 1);
    let (first, next) = (uuid(1, 1), uuid(2, 2));
    session.hear(&lifecycle(&first, "queued"));
    session.hear(&lifecycle(&first, "started"));
    session.hear(&turn_start());
    session.queue("Also say banana", Vec::new(), 2, 2);
    session.hear(&lifecycle(&next, "queued"));
    // Claude Code stops the command for it: the first turn ends with its result, and the next starts at once.
    session.hear(&turn_end(&[&first], None));
    assert!(session.working, "its message is next");
    session.hear(&lifecycle(&first, "cancelled"));
    session.hear(&lifecycle(&next, "started"));
    session.hear(&turn_start());
    session.hear(&json!({ "type": "user", "isReplay": true, "uuid": next, "message": { "role": "user", "content": "Also say banana" } }));
    assert!(session.working);
    assert_eq!(session.items.last(), Some(&Item::User("Also say banana".into())));
    session.hear(&thinks());
    assert!(session.working);
    session.hear(&turn_end(&[&next], None));
    session.hear(&lifecycle(&next, "completed"));
    assert!(!session.working);
}

#[test]
fn a_finished_message_is_not_waited_for_when_its_completed_line_never_comes() {
    let mut session = Session::default();
    session.send("hi", &[], 1, 1);
    let sent = uuid(1, 1);
    session.hear(&lifecycle(&sent, "queued"));
    session.hear(&turn_start());
    session.hear(&turn_end(&[&sent], None));
    assert!(!session.working);
    // The process ending ends it too.
    session.hear(&turn_start());
    session.stopped_running();
    assert!(!session.working);
}

fn agent_live(id: &str, description: &str) -> [Value; 2] {
    [
        json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": [{ "task_id": id, "run_id": format!("run-{id}"), "task_type": "local_agent", "subagent_type": "general-purpose", "description": description }] }),
        json!({ "type": "system", "subtype": "task_started", "task_id": id, "run_id": format!("run-{id}"), "tool_use_id": format!("toolu_{id}"), "description": description,
            "subagent_type": "general-purpose", "is_backgrounded": true, "task_type": "local_agent" }),
    ]
}

#[test]
fn a_background_agent_woken_by_send_message_runs_again_under_the_call_that_started_it() {
    let mut session = Session::default();
    session.now = 10_000;
    // Claude Code 2.1.329: the live set lists the agent again and `task_started` comes under the SendMessage's call.
    let lines = |call: &str, run: &str| {
        [
            json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": [{ "task_id": "ag1", "run_id": run, "task_type": "local_agent", "description": "tiny" }] }),
            json!({ "type": "system", "subtype": "task_started", "task_id": "ag1", "run_id": run, "tool_use_id": call, "description": "tiny", "is_backgrounded": true, "task_type": "local_agent" }),
        ]
    };
    lines("toolu_first", "run-1").iter().for_each(|line| session.hear(line));
    session.hear(&json!({ "type": "system", "subtype": "task_notification", "task_id": "ag1", "tool_use_id": "toolu_first", "status": "completed", "summary": "hi" }));
    session.hear(&json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": [] }));
    assert_eq!(session.tasks[0].ended.as_deref(), Some("completed"));
    session.now = 50_000;
    lines("toolu_send", "run-2").iter().for_each(|line| session.hear(line));
    assert_eq!((session.tasks[0].call.as_str(), session.tasks[0].ended.as_deref(), session.tasks[0].started), ("toolu_first", None, 50_000));
    session.hear(&json!({ "type": "system", "subtype": "task_progress", "task_id": "ag1", "tool_use_id": "toolu_send", "usage": { "total_tokens": 900, "tool_uses": 3, "duration_ms": 4_000 } }));
    assert_eq!(session.meter.agents["toolu_first"].tool_uses, 3);
}

#[test]
fn a_background_agent_woken_after_a_restart_runs_again_under_the_call_that_started_it() {
    let mut session = Session::default();
    // Read back from the transcript, which keeps the launch's result but none of the task lines.
    session.hear(&json!({ "type": "user", "message": { "content": [{ "type": "tool_result", "tool_use_id": "toolu_first", "content": "Async agent launched" }] },
        "toolUseResult": { "isAsync": true, "status": "async_launched", "agentId": "ag1" } }));
    session.hear(&json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": [{ "task_id": "ag1", "run_id": "run-2", "task_type": "local_agent", "description": "tiny" }] }));
    session.hear(&json!({ "type": "system", "subtype": "task_started", "task_id": "ag1", "run_id": "run-2", "tool_use_id": "toolu_send", "description": "tiny", "is_backgrounded": true, "task_type": "local_agent" }));
    assert_eq!(session.tasks[0].call, "toolu_first");
}

#[test]
fn a_background_agent_or_command_that_ends_makes_a_line_with_its_time_and_a_note_for_when_you_are_away() {
    let mut session = Session::default();
    session.now = 10_000;
    for line in agent_live("ag1", "Survey the parser") {
        session.hear(&line);
    }
    // The live set holds both now.
    let mut both = agent_live("ag1", "Survey the parser")[0].clone();
    both["tasks"].as_array_mut().unwrap().push(live(&[("bk1", "Run the tests")])["tasks"][0].clone());
    session.hear(&both);
    session.hear(&started("bk1", "Run the tests"));
    assert_eq!(session.tasks[0].call, "toolu_ag1");
    session.now = 10_000 + 452_000;
    session.hear(&json!({ "type": "system", "subtype": "task_notification", "task_id": "ag1", "run_id": "run-ag1", "tool_use_id": "toolu_ag1", "status": "completed",
        "output_file": "/tmp/claude/tasks/ag1.output", "summary": "Found two callers.\nBoth are tests.", "summary_is_report": true }));
    let Some(Item::Finished(done)) = session.items.last() else { panic!("a finished line") };
    assert_eq!((done.kind, done.description.as_str(), done.took, done.call.as_str()), ("Agent", "Survey the parser", Some(452_000), "toolu_ag1"));
    assert_eq!(done.title(), "Agent “Survey the parser” finished");
    assert_eq!(done.report, "Found two callers.\nBoth are tests.");
    assert!(session.alerts.is_empty(), "a finished task is a line in the chat, not a notification");
    // A command that fails says so, with Claude Code's words under it.
    session.now = 20_000;
    session.hear(&json!({ "type": "system", "subtype": "task_notification", "task_id": "bk1", "tool_use_id": "toolu_bk1", "status": "failed",
        "summary": "Background command \"Run the tests\" failed (exit code 1)" }));
    let Some(Item::Finished(done)) = session.items.last() else { panic!("a finished line") };
    assert_eq!((done.kind, done.failed(), done.title().as_str()), ("Command", true, "Command “Run the tests” failed"));
    assert_eq!(done.report, "Background command \"Run the tests\" failed (exit code 1)");
    assert_eq!(session.tasks[1].ended_at, 20_000);
    // Its call answered at once with its launch notice: opened, it says how long the command ran.
    assert_eq!(session.ran_for("toolu_bk1"), done.took);
    assert!(done.took.is_some());
    // A command that ran in the foreground sends a notice too, but was never one of the background tasks.
    let before = session.items.len();
    session.hear(&json!({ "type": "system", "subtype": "task_notification", "task_id": "fg1", "tool_use_id": "toolu_fg1", "status": "completed", "summary": "Ran it" }));
    assert_eq!(session.items.len(), before);
}

#[test]
fn a_resumed_sessions_task_notices_are_the_lines_where_tasks_ended_and_never_your_messages() {
    let text = |body: &str| json!({ "type": "user", "origin": { "kind": "task-notification" }, "message": { "role": "user", "content": body } }).to_string();
    let agent = "<task-notification>\n<task-id>ag1</task-id>\n<tool-use-id>toolu_ag1</tool-use-id>\n<output-file>/tmp/claude/tasks/ag1.output</output-file>\n<status>completed</status>\n<summary>Agent \"Survey the parser\" finished</summary>\n<note>A task-notification fires each time this agent stops.</note>\n<result>Found two callers.</result>\n<usage><subagent_tokens>89885</subagent_tokens><tool_uses>10</tool_uses><duration_ms>75250</duration_ms></usage>\n</task-notification>";
    let command = "<task-notification>\n<task-id>bk1</task-id>\n<tool-use-id>toolu_bk1</tool-use-id>\n<output-file>/tmp/claude/tasks/bk1.output</output-file>\n<status>completed</status>\n<summary>Background command \"Run the tests\" completed (exit code 0)</summary>\n</task-notification>";
    let event = "<task-notification>\n<task-id>mn1</task-id>\n<summary>Monitor event: \"Watch the build\"</summary>\n<event>build ended</event>\n</task-notification>";
    let session = heard(&[text(agent), text(command), text(event), text(&format!("{command}{agent}"))].join("\n"));
    let Item::Finished(first) = &session.items[0] else { panic!("an agent line") };
    assert_eq!((first.kind, first.description.as_str(), first.status.as_str(), first.took, first.report.as_str()), ("Agent", "Survey the parser", "completed", Some(75_250), "Found two callers."));
    let Item::Finished(second) = &session.items[1] else { panic!("a command line") };
    assert_eq!((second.kind, second.title().as_str(), second.took, second.report.as_str()), ("Command", "Command “Run the tests” finished", None, ""));
    // A notice with no status is a monitor's event, which stays a quiet note.
    assert!(matches!(&session.items[2], Item::Note(title, _) if title == "Monitor event: \"Watch the build\""));
    // One entry can hold several notices.
    assert_eq!(session.items.len(), 5);
    assert!(session.items.iter().all(|item| !matches!(item, Item::User(_))));
}

/// Two hand-backs as Claude Code 2.1.330 wrote them in a transcript: a `user` entry that is `isMeta`, and the same
/// message as a `queued_command` attachment when it came while Claude worked, with a note on the second.
const HANDBACKS: &str = include_str!("../../tests/handback.jsonl");

fn handbacks() -> Vec<Value> {
    HANDBACKS.lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}

#[test]
fn a_subagents_hand_back_is_a_report_item_with_the_whole_text_and_never_your_message() {
    let mut session = Session::default();
    let lines = handbacks();
    session.hear(&lines[0]);
    let [Item::Report(report)] = session.items.as_slice() else { panic!("{:?}", session.items) };
    assert_eq!((report.from.as_str(), report.name.as_str(), report.call.as_str()), ("agentabc123", "reviewer", "toolu_handback_1"));
    assert!(report.text.starts_with("The parser fix is in") && report.text.contains("```sh\ncargo test parser\n```"), "{}", report.text);
    assert!(!report.text.contains("[Subagent hand-back]"), "the report, not the frame around it");
    assert_eq!(report.note, None);
    assert!(session.items.iter().all(|item| !matches!(item, Item::User(_) | Item::UserImages(..))));
    assert!(session.prompts.is_empty(), "it is not one of your prompts, so a rewind never cuts at it");
}

#[test]
fn a_hand_back_that_came_while_claude_worked_is_a_report_too_with_its_note() {
    let mut session = Session::default();
    session.hear(&handbacks()[1]);
    let [Item::Report(report)] = session.items.as_slice() else { panic!("{:?}", session.items) };
    assert_eq!(report.from, "agentdef456");
    assert!(report.note.as_deref().is_some_and(|note| note.starts_with("SECURITY WARNING")), "{:?}", report.note);
    assert!(session.items.iter().all(|item| !matches!(item, Item::User(_))));
}

#[test]
fn a_hand_back_only_the_transcript_has_shows_once_when_a_turn_ends_however_often_it_is_read() {
    let mut session = Session::default();
    // The turn ended with nothing in the stream about it; the newest transcript entries hold the report, among others.
    let mut entries = vec![json!({ "type": "user", "message": { "role": "user", "content": "Audit it" } })];
    entries.extend(handbacks());
    entries.push(json!({ "type": "attachment", "attachment": { "type": "queued_command", "prompt": "later", "commandMode": "prompt" } }));
    session.hear_handbacks(&entries);
    session.hear_handbacks(&entries);
    let reports: Vec<&Report> = session.items.iter().filter_map(|item| if let Item::Report(report) = item { Some(report) } else { None }).collect();
    assert_eq!(reports.len(), 2, "{:?}", session.items);
    assert_eq!(session.items.len(), 2, "nothing else of the entries came in: {:?}", session.items);
    // A rewind that took one away doesn't bring it back with the next read.
    session.items.clear();
    session.hear_handbacks(&entries);
    assert!(session.items.is_empty());
}

#[test]
fn a_hand_back_heard_twice_is_one_report() {
    let mut session = Session::default();
    let lines = handbacks();
    // The live line and the transcript's entry for the same call.
    for line in [&lines[0], &lines[0], &lines[1], &lines[1]] {
        session.hear(line);
    }
    assert_eq!(session.items.iter().filter(|item| matches!(item, Item::Report(_))).count(), 2);
    // One from a Claude Code that gave no call id is told apart by its text.
    let mut old = lines[0].clone();
    old["origin"].as_object_mut().unwrap().remove("handbackToolUseId");
    old["origin"]["handbackReportText"] = "A different report".into();
    session.hear(&old);
    session.hear(&old);
    assert_eq!(session.items.iter().filter(|item| matches!(item, Item::Report(_))).count(), 3);
}

#[test]
fn a_call_the_transcript_holds_without_a_result_is_interrupted_once_the_chat_restarts_on_it() {
    let mut session = Session::default();
    session.hear(&json!({ "type": "user", "message": { "role": "user", "content": "count" } }));
    session.hear(&json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "id": "b1", "name": "Bash", "input": { "command": "sleep 99" } }] } }));
    session.stop_replayed();
    // The new process starts a turn of its own.
    session.hear(&json!({ "type": "user", "message": { "role": "user", "content": "what's the status?" } }));
    session.hear(&json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "id": "b2", "name": "Bash", "input": { "command": "ls" } }] } }));
    let tools: Vec<&Tool> = session.items.iter().filter_map(|item| if let Item::Tool(tool) = item { Some(tool) } else { None }).collect();
    assert!(session.was_interrupted(tools[0]) && !session.runs(tools[0]));
    assert!(session.runs(tools[1]) && !session.was_interrupted(tools[1]), "a call of the live process still runs");
    assert_eq!(session.activity(), "Running ls", "the live call is what Claude does, not the cut one");
    assert_eq!(session.turn_subagents(), (0, 0));
    assert!(session.background("b1").is_none());
}

#[test]
fn a_task_is_ambient_from_its_entry_and_can_change_while_it_runs() {
    let mut session = Session::default();
    session.hear(&json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": [{ "task_id": "w1", "task_type": "local_bash", "description": "Watch", "ambient": true }, { "task_id": "b1", "task_type": "local_bash", "description": "Build" }] }));
    assert_eq!(session.tasks.iter().map(|task| (task.ambient, task.active())).collect::<Vec<_>>(), [(true, false), (false, true)]);
    session.hear(&json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": [{ "task_id": "w1", "task_type": "local_bash", "description": "Watch" }, { "task_id": "b1", "task_type": "local_bash", "description": "Build", "ambient": true }] }));
    assert_eq!(session.tasks.iter().map(|task| task.active()).collect::<Vec<_>>(), [true, false]);
}

#[test]
fn a_chat_asks_claude_code_to_forward_what_subagents_say() {
    // `apply_flag_settings` with the env variable has no effect once Claude Code runs (measured on 2.1.332): `initialize` takes it.
    let first: Value = sent(Session::opening(None, None).lines().next().unwrap());
    assert_eq!(first["request"], json!({ "subtype": "initialize", "forwardSubagentText": true }));
}

#[test]
fn a_subagents_forwarded_text_and_thinking_are_kept_with_its_tool_calls() {
    let mut session = Session::default();
    for line in [
        json!({ "type": "assistant", "parent_tool_use_id": "toolu_1", "agent_id": "a1", "uuid": "u1", "message": { "role": "assistant", "content": [{ "type": "thinking", "thinking": "Count first", "signature": "" }] } }),
        json!({ "type": "assistant", "parent_tool_use_id": "toolu_1", "agent_id": "a1", "uuid": "u2", "message": { "role": "assistant", "content": [{ "type": "text", "text": "Counting the lines" }] } }),
    ] {
        session.hear(&line);
        assert!(crate::subagents::passed_line(&line).is_some(), "the tab gets the line as it is");
    }
    let sub = &session.subagents["toolu_1"];
    assert!(sub.items.iter().any(|item| matches!(item, Item::Text(said) if said == "Counting the lines")), "{:?}", sub.items);
    assert!(sub.items.iter().any(|item| matches!(item, Item::Thought(..))), "{:?}", sub.items);
    assert!(session.items.is_empty(), "and out of the conversation");
}
