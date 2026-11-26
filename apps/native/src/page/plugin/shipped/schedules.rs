//! Schedules, its page and its tools.
use super::*;

/// Schedules with its real main and the project `/work/api`, named Api.
fn schedules_rig() -> (Rig, mpsc::Receiver<String>) {
    // Needs Wings 0.2.21, for `host::view_args`: use `load` once the app's version is bumped.
    let rig = Rig::with(load("schedules"), true, vec![]);
    let work = PathBuf::from("/work/api");
    *rig.env.projects.lock().unwrap() = vec![work.clone()];
    rig.host.publish_projects(vec![Project {
        id: "/work/api".into(),
        path: "/work/api".into(),
        name: "Api".into(),
        parent_id: None, hidden_sections: Vec::new(),
    }]);
    let (on_event, main_errors) = errors();
    rig.host
        .start_main(&module("schedules", "main"), on_event)
        .unwrap();
    // A tool call that comes before main has set up what it listens to gets no answer: ask until it has.
    let started = (0..40).any(|_| {
        let (reply, answer) = Reply::channel();
        rig.host.run_tool("list", json!({}), None, reply);
        answer.wait(std::time::Duration::from_millis(500)).is_some()
    });
    assert!(started, "main never answered the list tool");
    (rig, main_errors)
}

#[test]
fn schedules_main_serves_its_native_page_and_a_schedule_added_in_the_form_is_listed() {
    let (rig, main_errors) = schedules_rig();

    let (page, events) = open_page(&rig.host, &module("schedules", "schedules"));
    let empty = tree(&events, "the empty list", |tree| {
        tree.text().contains("Nothing set up yet")
    });
    let mut add = Vec::new();
    buttons_named(&empty.children, "Add entry", &mut add);
    assert!(page.dispatch(&add[0], None));

    let form = tree(&events, "the form", |tree| {
        tree.text().contains("Start an agent") && input_named(&tree.children, "Prompt").is_some()
    });
    for (label, value) in [
        ("Name", "Morning review"),
        ("Prompt", "Check yesterday's CI"),
    ] {
        let handler = input_named(&form.children, label).expect(label);
        assert!(page.dispatch(&handler, Some(&json!({ "value": value }))));
    }
    let filled = tree(&events, "the typed text", |tree| {
        let value = |label: &str| {
            fn find(nodes: &[Node], label: &str) -> Option<String> {
                nodes.iter().find_map(|node| match node {
                    Node::Input(input) if input.label == label => Some(input.value.clone()),
                    other => other.parts().into_iter().find_map(|part| find(part, label)),
                })
            }
            find(&tree.children, label)
        };
        value("Name").as_deref() == Some("Morning review")
            && value("Prompt").as_deref() == Some("Check yesterday's CI")
    });
    let mut save = Vec::new();
    buttons_named(&filled.children, "Add entry", &mut save);
    assert!(page.dispatch(&save[0], None));

    let listed = tree(&events, "the saved schedule", |tree| {
        let text = tree.text();
        text.contains("Morning review") && text.contains("Next run")
    });
    assert!(
        listed.text().contains("Api · Claude Code"),
        "{}",
        listed.text()
    );
    // It is kept under the keys the TypeScript plugin used, which a saved schedule still reads.
    let saved = rig.root.saved("schedules", "schedules").unwrap();
    assert_eq!(saved[0]["name"], "Morning review");
    assert_eq!(saved[0]["when"], json!({ "kind": "daily", "time": "09:00", "days": [1, 2, 3, 4, 5] }));
    assert_eq!(saved[0]["agent"], "claude");
    assert!(
        main_errors.try_recv().is_err(),
        "main reported {:?}",
        main_errors.try_recv()
    );
}

#[test]
fn schedules_add_tool_waits_for_the_users_allow_in_the_modal_and_saves_nothing_when_denied() {
    let (rig, main_errors) = schedules_rig();
    let ask = |input: serde_json::Value| {
        let (reply, answer) = Reply::channel();
        rig.host.run_tool("add", input, None, reply);
        answer
    };
    let finished = |answer: crate::page::runtime::Answer| {
        let answer: serde_json::Value = serde_json::from_str(&answer.wait(super::super::testing::WAIT).expect("the tool's answer").expect("an answer")).unwrap();
        (answer["text"].as_str().unwrap().to_owned(), answer["isError"] == true)
    };
    // The tool's input is checked before anything is asked.
    let (bad, was_error) = finished(ask(json!({ "prompt": "Check CI", "project": "Api", "time": "9:00" })));
    assert!(was_error && bad.starts_with("time must match the pattern"), "{bad}");

    let first = ask(json!({ "prompt": "Check yesterday's CI", "project": "Api", "time": "08:30", "days": ["mon", "fri"] }));
    rig.env.wait("the modal", |seen| seen.iter().any(|seen| matches!(seen, Seen::OpenView(open) if open.id == "approve")));
    let (page, events) = open_page(&rig.host, &module("schedules", "approve"));
    let ask_shown = tree(&events, "the ask", |tree| tree.text().contains("asks to add a schedule in Api"));
    let text = ask_shown.text();
    for shown in ["Mon, Fri at 08:30", "Claude Code", "Check yesterday's CI"] {
        assert!(text.contains(shown), "{shown:?} in {text:?}");
    }
    // Allow waits a second after the ask shows, so a click meant for something else can't land on it.
    let armed = tree(&events, "Allow", |tree| {
        let mut allow = Vec::new();
        buttons_named(&tree.children, "Allow", &mut allow);
        !allow.is_empty()
    });
    let mut deny = Vec::new();
    buttons_named(&armed.children, "Deny", &mut deny);
    assert!(page.dispatch(&deny[0], None));
    let (denied, was_error) = finished(first);
    assert!(was_error && denied.starts_with("The user denied it in Wings"), "{denied}");
    rig.env.wait("the modal to close", |seen| seen.iter().any(|seen| matches!(seen, Seen::CloseView(_, view) if view == "approve")));
    assert_eq!(rig.root.saved("schedules", "schedules"), None);

    let second = ask(json!({ "prompt": "Check yesterday's CI", "project": "Api", "every_hours": 6 }));
    let again = tree(&events, "the second ask", |tree| {
        let mut allow = Vec::new();
        buttons_named(&tree.children, "Allow", &mut allow);
        tree.text().contains("Every 6 hours") && !allow.is_empty()
    });
    let mut allow = Vec::new();
    buttons_named(&again.children, "Allow", &mut allow);
    assert!(page.dispatch(&allow[0], None));
    let (added, was_error) = finished(second);
    assert!(!was_error && added.contains("in Api: Every 6 hours, next run"), "{added}");
    assert_eq!(rig.root.saved("schedules", "schedules").unwrap()[0]["when"], json!({ "kind": "every", "hours": 6 }));
    assert!(main_errors.try_recv().is_err(), "main reported {:?}", main_errors.try_recv());
}

/// Schedules with an assigned entry `w1` of the project Api, a real repo, that has the ticket 42 waiting. Its agent
/// starts only if `agents_start`, or if the user allows it.
fn schedules_start_rig(agents_start: bool) -> (Rig, mpsc::Receiver<String>) {
    let dir = crate::projects::test_dir("schedules-start");
    let repo = crate::projects::test_repo(&dir);
    let file = dir.join("spaces.json");
    std::fs::write(&file, serde_json::to_vec(&json!([{ "id": repo, "name": "Api", "path": repo }])).unwrap()).unwrap();
    let rig = Rig::with(load("schedules"), true, vec![]);
    *rig.env.spaces.lock().unwrap() = crate::projects::Projects::open(Some(file), repo.clone());
    // A pane opens only inside a project, and the app lists the worktrees a start makes by then.
    *rig.env.projects.lock().unwrap() = vec![repo.clone(), dir.join("app-worktrees")];
    let id: String = repo.to_string_lossy().into();
    rig.host.publish_projects(vec![Project { id: id.clone(), path: id.clone(), name: "Api".into(), parent_id: None, hidden_sections: Vec::new() }]);
    let mut entry = json!({
        "id": "w1", "projectId": id, "agent": "claude", "prompt": "Work on {id}: {title}", "minFreeGb": 0, "enabled": false, "primed": true, "seen": ["42"],
        "waiting": [{ "id": "42", "title": "Fix totals", "url": "https://example.test/42", "at": 1 }, { "id": "43", "title": "Add tax rows", "url": "https://example.test/43", "at": 2 }],
    });
    if agents_start {
        entry["agentsStart"] = json!(true);
    }
    rig.root.save("schedules", "assigned", json!([entry]));
    let (on_event, main_errors) = errors();
    rig.host.start_main(&module("schedules", "main"), on_event).unwrap();
    let started = (0..40).any(|_| {
        let (reply, answer) = Reply::channel();
        rig.host.run_tool("list", json!({}), None, reply);
        answer.wait(std::time::Duration::from_millis(500)).is_some()
    });
    assert!(started, "main never answered the list tool");
    (rig, main_errors)
}

fn agents_started(rig: &Rig) -> usize {
    rig.env.seen().iter().filter(|seen| matches!(seen, Seen::OpenPane(_))).count()
}

fn worktrees(rig: &Rig) -> usize {
    rig.env.spaces.lock().unwrap().entries().iter().filter(|p| p.child).count()
}

fn asked(rig: &Rig) -> bool {
    rig.env.seen().iter().any(|seen| matches!(seen, Seen::OpenView(open) if open.id == "approve"))
}

/// The Allow or Deny button of the modal, once it shows the ask for `ticket` and Allow is armed.
fn answer_button(events: &mpsc::Receiver<Event>, label: &str, ticket: &str) -> String {
    let armed = tree(events, label, |tree| {
        let mut allow = Vec::new();
        buttons_named(&tree.children, "Allow", &mut allow);
        tree.text().contains(&format!("asks to start an agent on {ticket}")) && !allow.is_empty()
    });
    let mut found = Vec::new();
    buttons_named(&armed.children, label, &mut found);
    found.remove(0)
}

#[test]
fn schedules_start_tool_asks_in_the_modal_and_starts_nothing_when_denied_and_starts_when_allowed() {
    let (rig, main_errors) = schedules_start_rig(false);
    // A start that would be refused isn't asked about.
    let (refused, was_error) = tool_result(run_tool(&rig, "start", json!({ "id": "w1", "ticket": "7" })));
    assert!(was_error && refused.contains("isn't waiting"), "{refused}");
    assert!(!asked(&rig));

    let first = run_tool(&rig, "start", json!({ "id": "w1", "ticket": "#42" }));
    rig.env.wait("the modal", |seen| seen.iter().any(|seen| matches!(seen, Seen::OpenView(open) if open.id == "approve")));
    let (page, events) = open_page(&rig.host, &module("schedules", "approve"));
    let shown = tree(&events, "the ask", |tree| tree.text().contains("asks to start an agent on #42 Fix totals in Api."));
    let text = shown.text();
    for part in ["Ticket", "#42 Fix totals", "Claude Code", "Work on 42: Fix totals", "It makes a worktree and opens the agent with this prompt.", "without asking, turn that on in the entry on the Schedules page"] {
        assert!(text.contains(part), "{part:?} in {text:?}");
    }
    // Nothing is made while the ask waits.
    assert_eq!((agents_started(&rig), worktrees(&rig)), (0, 0));
    assert!(page.dispatch(&answer_button(&events, "Deny", "#42"), None));
    let (denied, was_error) = tool_result(first);
    assert!(was_error && denied.starts_with("The user denied it in Wings") && denied.contains("Nothing started."), "{denied}");
    assert_eq!((agents_started(&rig), worktrees(&rig)), (0, 0));

    let second = run_tool(&rig, "start", json!({ "id": "w1", "ticket": "43" }));
    let button = answer_button(&events, "Allow", "#43");
    assert!(page.dispatch(&button, None));
    let (started, was_error) = tool_result(second);
    assert!(!was_error && started.starts_with("Started Add tax rows on "), "{started}");
    assert_eq!((agents_started(&rig), worktrees(&rig)), (1, 1));
    // A change by a tool leaves the entry asking.
    let (changed, was_error) = tool_result(run_tool(&rig, "update", json!({ "id": "w1", "name": "Totals" })));
    assert!(!was_error, "{changed}");
    assert!(rig.root.saved("schedules", "assigned").unwrap()[0].get("agentsStart").is_none());
    assert!(main_errors.try_recv().is_err(), "main reported {:?}", main_errors.try_recv());
}

#[test]
fn schedules_start_tool_starts_without_asking_for_an_entry_that_lets_agents_start_and_a_tool_keeps_that() {
    let (rig, main_errors) = schedules_start_rig(true);
    let (started, was_error) = tool_result(run_tool(&rig, "start", json!({ "id": "w1", "ticket": "42" })));
    assert!(!was_error && started.starts_with("Started Fix totals on "), "{started}");
    assert!(!asked(&rig));
    assert_eq!((agents_started(&rig), worktrees(&rig)), (1, 1));
    let (changed, was_error) = tool_result(run_tool(&rig, "update", json!({ "id": "w1", "prompt": "Do {id}" })));
    assert!(!was_error, "{changed}");
    assert_eq!(rig.root.saved("schedules", "assigned").unwrap()[0]["agentsStart"], json!(true));
    assert!(main_errors.try_recv().is_err(), "main reported {:?}", main_errors.try_recv());
}

#[test]
fn schedules_add_assigned_tool_makes_an_entry_whose_starts_ask() {
    let (rig, main_errors) = schedules_rig();
    let (added, was_error) = tool_result(run_tool(&rig, "add_assigned", json!({ "project": "Api" })));
    assert!(!was_error && added.starts_with("Added "), "{added}");
    assert!(rig.root.saved("schedules", "assigned").unwrap()[0].get("agentsStart").is_none());
    assert!(main_errors.try_recv().is_err(), "main reported {:?}", main_errors.try_recv());
}

#[test]
fn schedules_approve_modal_with_nothing_waiting_says_so() {
    let (rig, _) = schedules_rig();
    let (_page, events) = open_page(&rig.host, &module("schedules", "approve"));
    tree(&events, "the empty modal", |tree| tree.text().contains("Nothing is waiting"));
}
