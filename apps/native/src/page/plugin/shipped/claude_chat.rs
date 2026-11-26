//! Claude chat's badges, plan-limit pause and `open_chat` tool.
use super::*;

#[test]
fn claude_chat_badges_claude_panes_and_its_button_moves_the_session_into_a_chat_tab() {
    let mut claude = pane("1", "/work");
    claude.session = Some(SessionEntry {
        session_id: "3c21b0c5-8113-4f55-9493-fa156c3fa369".into(),
        kind: "claude".into(),
        name: None,
        state: "idle".into(),
        started_at: 1_700_000_000_000,
        launcher: Some("claude-rtk".into()),
    });
    let mut working = claude.clone();
    working.session.as_mut().unwrap().state = "working".into();
    let rig = Rig::with(load("claude-chat"), false, vec![working, pane("2", "/work")]);
    let (on_event, main_errors) = errors();
    rig.host.start_main(&module("claude-chat", "main"), on_event).unwrap();
    let seen = rig.env.wait("the badges", |seen| seen.iter().any(|seen| matches!(seen, Seen::Badge(_, pane, Some(_)) if pane == "1")));
    assert!(!seen.iter().any(|seen| matches!(seen, Seen::Badge(_, pane, Some(_)) if pane == "2")), "{seen:?}");
    // Closing the terminal would end the turn Claude is on.
    assert_eq!(
        action(&rig, "1", "open"),
        Err("Claude is still working in this terminal. Open it as a chat once it has finished.".into())
    );
    assert!(!rig.env.seen().iter().any(|seen| matches!(seen, Seen::ClosePane(_))));
    rig.host.publish_panes(vec![claude, pane("2", "/work")]);

    assert_eq!(action(&rig, "1", "open"), Ok(()));
    let moved: Vec<Seen> = rig
        .env
        .seen()
        .into_iter()
        .filter(|seen| matches!(seen, Seen::ClosePane(_) | Seen::OpenView(_)))
        .collect();
    let [Seen::ClosePane(closed), Seen::OpenView(view)] = moved.as_slice() else { panic!("{moved:?}") };
    assert_eq!(closed, "1");
    assert_eq!((view.id.as_str(), view.place.as_str()), ("chat", "tab"));
    assert_eq!(
        view.args,
        Some(json!({ "resume": "3c21b0c5-8113-4f55-9493-fa156c3fa369", "cwd": "/work", "program": "claude-rtk" }))
    );
    assert!(main_errors.try_recv().is_err());
}

/// Near the plan's limit, Claude chat's main tells every Claude Code session to pause: with each message, and before a
/// run of commands once; under it, nothing.
#[test]
fn claude_chat_tells_claude_to_pause_near_the_plan_limit() {
    let rig = Rig::with(load("claude-chat"), false, vec![]);
    let (on_event, _errors) = errors();
    rig.host.start_main(&module("claude-chat", "main"), on_event).unwrap();
    let session = "3c21b0c5-8113-4f55-9493-fa156c3fa369";
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap();
    let report = |five: u64| {
        let status = json!({ "session_id": session, "rate_limits": {
            "five_hour": { "used_percentage": five, "resets_at": now.as_secs() + 3600 },
            "seven_day": { "used_percentage": 40, "resets_at": now.as_secs() + 86_400 },
        } });
        rig.env.statusline.lock().unwrap().record(&status, &serde_json::Value::Null, None, now.as_millis() as u64 + five);
    };
    let caller = super::super::ClaudeCaller { pane_id: Some("p1".into()), project: serde_json::Value::Null, session: json!({ "id": session }) };
    let hook = |event: &str, input: serde_json::Value| -> serde_json::Value {
        let (reply, answer) = crate::page::runtime::Reply::channel();
        rig.host.run_for_claude(event, input, &caller, true, reply);
        let said: serde_json::Value = serde_json::from_str(&answer.wait(WAIT).expect("an answer").expect("a result")).unwrap();
        serde_json::from_str(said["text"].as_str().unwrap()).unwrap()
    };
    let prompt = || hook("UserPromptSubmit", json!({ "session_id": session, "prompt": "go on" }));
    let command = || hook("PreToolUse", json!({ "session_id": session, "tool_name": "Bash", "tool_input": { "command": "ls" } }));
    report(50);
    assert_eq!(prompt(), json!({}));
    report(93);
    let told = prompt();
    let note = told["context"].as_str().unwrap_or_default();
    assert!(note.contains("5-hour limit is at 93%") && note.contains("Pause"), "{told}");
    // Before commands, once for this level, not before each one.
    assert!(command()["context"].as_str().is_some_and(|said| said.contains("Pause")));
    assert_eq!(command(), json!({}));
    report(100);
    assert!(command()["context"].as_str().is_some_and(|said| said.contains("used up") && said.contains("extra usage")));
}

/// An agent calls Claude chat's `open_chat`: the tool's schema is the manifest's, the chat tab opens in the project it names,
/// in the folder the session ran in, with the args the chat page reads, and an unknown account or session opens nothing.
#[test]
fn claude_chat_open_chat_opens_a_chat_tab_in_the_named_project_and_refuses_what_it_cannot_find() {
    let claude = claude_with_session();
    fs::write(
        claude.join(format!("projects/-x/{SESSION}.jsonl")),
        json!({ "type": "user", "uuid": "u1", "timestamp": iso(1000), "cwd": "/work/api/web", "message": { "role": "user", "content": "hi" } }).to_string(),
    )
    .unwrap();
    let rig = Rig::with(load("claude-chat"), false, vec![]);
    *rig.env.claude.lock().unwrap() = vec![claude.clone()];
    *rig.env.projects.lock().unwrap() = vec![PathBuf::from("/work/api")];
    rig.host.publish_projects(vec![Project { id: "/work/api".into(), path: "/work/api".into(), name: "Api".into(), parent_id: None, hidden_sections: Vec::new() }]);
    let (on_event, main_errors) = errors();
    rig.host.start_main(&module("claude-chat", "main"), on_event).unwrap();
    let caller = super::super::ClaudeCaller { pane_id: Some("p1".into()), project: json!({ "id": "/work/api", "name": "Api", "path": "/work/api" }), session: json!({ "id": SESSION }) };
    // A call that comes before main has set up its tools gets no answer: ask until it has.
    let call = |input: serde_json::Value| -> serde_json::Value {
        for _ in 0..40 {
            let (reply, answer) = crate::page::runtime::Reply::channel();
            rig.host.run_for_claude("open_chat", input.clone(), &caller, false, reply);
            if let Some(said) = answer.wait(std::time::Duration::from_millis(500)) {
                return serde_json::from_str(&said.expect("an answer")).unwrap();
            }
        }
        panic!("main never answered open_chat");
    };
    let opened = |rig: &Rig| rig.env.seen().into_iter().filter_map(|seen| if let Seen::OpenView(open) = seen { Some(open) } else { None }).collect::<Vec<_>>();

    let said = call(json!({ "prompt": "Take over the retry work" }));
    assert!(said["isError"] != true, "{said}");
    let open = opened(&rig);
    assert_eq!(open.len(), 1);
    assert_eq!((open[0].plugin.as_str(), open[0].id.as_str(), open[0].place.as_str(), open[0].new_tab), ("claude-chat", "chat", "tab", true));
    assert_eq!(open[0].project, Some(PathBuf::from("/work/api")));
    // The prompt is the calling agent's, so the chat sends it to Claude Code marked as not the user's.
    assert_eq!(open[0].args, Some(json!({ "cwd": "/work/api", "prompt": "Take over the retry work", "relayed": true })));

    // The caller's own session goes on in the folder it ran in, as a fork.
    let said = call(json!({ "resume": "current" }));
    assert!(said["isError"] != true, "{said}");
    assert_eq!(opened(&rig)[1].args, Some(json!({ "cwd": "/work/api/web", "resume": SESSION, "fork": true, "bring": false })));

    for (input, part) in [
        (json!({ "account": "nobody" }), "No Claude Code account \"nobody\""),
        (json!({ "project": "elsewhere" }), "No project \"elsewhere\" in Wings"),
        (json!({ "resume": "55555555-2222-4333-8444-555555555555" }), "No session 55555555-2222-4333-8444-555555555555"),
        (json!({ "resume": SESSION, "prompt": "hi" }), "resume or prompt, not both"),
        (json!({ "colour": "red" }), "colour"),
    ] {
        let said = call(input);
        assert!(said["isError"] == true && said["text"].as_str().unwrap().contains(part), "{part:?} in {said}");
    }
    assert_eq!(opened(&rig).len(), 2);
    assert!(main_errors.try_recv().is_err());
    let _ = fs::remove_dir_all(claude);
}
