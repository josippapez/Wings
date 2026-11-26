//! Issues: its board, list and Issue tab, through its main.
use super::*;

/// Issues with its real main, in the project `/work/web`.
fn tasks_rig() -> Rig {
    let rig = Rig::with(load("tasks"), true, vec![]);
    *rig.env.projects.lock().unwrap() = vec![PathBuf::from("/work/web")];
    rig.host.publish_projects(vec![Project {
        id: "/work/web".into(),
        path: "/work/web".into(),
        name: "web".into(),
        parent_id: None, hidden_sections: Vec::new(),
    }]);
    // The project on screen, which the app always says and the pages follow with `onProject`.
    rig.host.publish_project(Some(Path::new("/work/web")));
    let (on_event, _errors) = errors();
    rig.host
        .start_main(&module("tasks", "main"), on_event)
        .unwrap();
    rig
}

/// Types `title` into the board's New issue field and presses Add, and returns the board that shows it.
fn add_issue(page: &PageRuntime, events: &mpsc::Receiver<Event>, title: &str) -> Tree {
    // The header's New issue opens the line to type it in.
    let board = tree(events, "the board", |tree| {
        let mut new = Vec::new();
        buttons_named(&tree.children, "New issue", &mut new);
        !new.is_empty() || input_named(&tree.children, "New issue").is_some()
    });
    // It stays open after an issue is added, for the next.
    let board = match input_named(&board.children, "New issue") {
        Some(_) => board,
        None => {
            let mut new = Vec::new();
            buttons_named(&board.children, "New issue", &mut new);
            assert!(page.dispatch(&new[0], None));
            tree(events, "the line to add an issue", |tree| input_named(&tree.children, "New issue").is_some())
        }
    };
    let field = input_named(&board.children, "New issue").unwrap();
    assert!(page.dispatch(&field, Some(&json!({ "value": title }))));
    let typed = tree(events, "the typed title", |tree| {
        let mut add = Vec::new();
        buttons_named(&tree.children, "Add", &mut add);
        !add.is_empty()
    });
    let mut add = Vec::new();
    buttons_named(&typed.children, "Add", &mut add);
    assert!(page.dispatch(&add[0], None));
    tree(events, "the new issue", |tree| tree.text().contains(title))
}

#[test]
fn tasks_board_adds_an_issue_through_its_main() {
    let rig = tasks_rig();
    let (page, events, _saved) = open_with_state(
        &rig.host,
        &module("tasks", "board"),
        None,
        super::super::StatePlace::Tab,
        None,
    );
    let board = add_issue(&page, &events, "Fix the login redirect");
    assert!(board.text().contains("1 to do"), "{}", board.text());
}

/// Typing in the board's search narrows it to the issues that match.
#[test]
fn tasks_board_search_narrows_the_issues_as_you_type() {
    let rig = tasks_rig();
    let (page, events, _saved) = open_with_state(
        &rig.host,
        &module("tasks", "board"),
        None,
        super::super::StatePlace::Tab,
        None,
    );
    add_issue(&page, &events, "Fix the login redirect");
    let board = add_issue(&page, &events, "Upgrade the date picker");
    fn search(nodes: &[Node]) -> Option<String> {
        nodes.iter().find_map(|node| match node {
            Node::SearchField(field) if field.label == "Search issues" => field.on_input.clone(),
            other => other.parts().into_iter().find_map(search),
        })
    }
    let typed = search(&board.children).expect("the search field takes typing");
    assert!(page.dispatch(&typed, Some(&json!({ "value": "login" }))));
    tree(&events, "the searched board", |tree| {
        let text = tree.text();
        text.contains("Fix the login redirect") && !text.contains("Upgrade the date picker")
    });
}

/// 500 issues in storage (the plugin refuses to make more than 300, but storage can hold what an older build left): the
/// board draws within the node budget of a page, and a keystroke in the search narrows it in a turn. The timings are the
/// time to the tree the app gets, in a debug build of the app on a busy machine.
#[test]
fn tasks_board_with_500_issues_draws_and_searches_within_a_turn() {
    let rig = tasks_rig();
    /// A page that stores 500 issues and the project's counter, as an older build left them, and then says so.
    struct Seeds;
    impl Guest for Seeds {
        fn start(&mut self, cx: &mut Cx<'_>) {
            let slot = "c10b05ffeab5d23e"; // the slot of /work/web
            let now = now_ms();
            let statuses = ["todo", "in_progress", "in_review", "done"];
            let set = |key: String, value: serde_json::Value| {
                cx.link()
                    .storage(Store::Set(key, value.to_string()))
                    .unwrap();
            };
            for i in 1..=500u64 {
                set(
                    format!("issues/{slot}/WEB-{i}"),
                    json!({ "key": format!("WEB-{i}"), "title": format!("Item {i} of the made-up backlog"), "status": statuses[i as usize % 4],
                        "type": "task", "sessions": [], "log": [{ "at": now, "by": "you", "kind": "created", "text": "Created" }],
                        "createdAt": now, "updatedAt": now - i * 1000 }),
                );
            }
            set(
                format!("issues/{slot}"),
                json!({ "v": 3, "path": "/work/web", "prefix": "WEB", "next": 501 }),
            );
            cx.render(&json!({ "id": 0, "type": "#root", "props": {}, "children": [{ "id": 1, "type": "#text", "text": "Seeded" }] }));
        }
    }
    let (_seeding, seeded) = open_page(&rig.host, &fake::source("seed", || Box::new(Seeds)));
    tree(&seeded, "the seeded issues", |tree| {
        tree.text().contains("Seeded")
    });
    let started = std::time::Instant::now();
    let (page, events, _saved) = open_with_state(
        &rig.host,
        &module("tasks", "board"),
        None,
        super::super::StatePlace::Tab,
        None,
    );
    let board = tree(&events, "the first board", |tree| tree.text().contains("Item 4 of the made-up backlog"));
    let first = started.elapsed();
    fn search(nodes: &[Node]) -> Option<String> {
        nodes.iter().find_map(|node| match node {
            Node::SearchField(field) if field.label == "Search issues" => field.on_input.clone(),
            other => other.parts().into_iter().find_map(search),
        })
    }
    let typed = search(&board.children).expect("the search field takes typing");
    let keystroke = std::time::Instant::now();
    assert!(page.dispatch(&typed, Some(&json!({ "value": "item 42" }))));
    tree(&events, "the searched board", |tree| {
        let text = tree.text();
        text.contains("Item 42 of the made-up backlog") && !text.contains("Item 4 of the made-up backlog")
    });
    let searched = keystroke.elapsed();
    // A second tab, which has the module compiled already.
    let again = std::time::Instant::now();
    let (_second, second_events, _saved) = open_with_state(&rig.host, &module("tasks", "board"), None, super::super::StatePlace::Tab, None);
    tree(&second_events, "the second board", |tree| tree.text().contains("Item 4 of the made-up backlog"));
    let second = again.elapsed();
    println!("500 issues: the board's first tree after {first:?} (a second tab after {second:?}), one search keystroke to its tree after {searched:?}");
    assert!(first < std::time::Duration::from_secs(10) && searched < std::time::Duration::from_secs(5));
}

#[test]
fn tasks_list_starts_with_the_groups_its_tab_saved_as_folded_and_saves_an_unfold() {
    let rig = tasks_rig();
    let (page, events, saved) = open_with_state(
        &rig.host,
        &module("tasks", "board"),
        None,
        super::super::StatePlace::Tab,
        Some(json!({ "folded": ["status:todo"] })),
    );
    let board = add_issue(&page, &events, "Fix the login redirect");
    let view = chips_named(&board.children, "View").unwrap();
    assert!(page.dispatch(&view, Some(&json!({ "value": "list" }))));
    // A group's bar is a row that folds it.
    fn group(nodes: &[Node]) -> Option<(Option<bool>, Option<String>)> {
        nodes.iter().find_map(|node| match node {
            Node::ListButton(group)
                if group.expanded.is_some()
                    && plain_text(&group.button.children).contains("To do") =>
            {
                Some((group.expanded, group.button.on_click.clone()))
            }
            other => other.parts().into_iter().find_map(group),
        })
    }
    let list = tree(&events, "the list", |tree| group(&tree.children).is_some());
    let (open, toggle) = group(&list.children).unwrap();
    assert_eq!(open, Some(false));
    assert!(page.dispatch(&toggle.unwrap(), None));
    tree(&events, "the unfolded group", |tree| {
        group(&tree.children).is_some_and(|(open, _)| open == Some(true))
    });
    assert_eq!(saved.lock().unwrap().last(), Some(&json!({ "folded": [] })));
}

/// The buttons named "Edit the title".
fn title_buttons(nodes: &[Node]) -> Vec<&crate::page::tree::Button> {
    let mut found = Vec::new();
    for node in nodes {
        match node {
            Node::Button(button) if button.label.as_deref() == Some("Edit the title") => {
                found.push(button)
            }
            other => {
                for part in other.parts() {
                    found.extend(title_buttons(part));
                }
            }
        }
    }
    found
}

/// The Issue tab's title is a button that wraps it and opens the title's field on a click or F2, as the desktop's is.
/// The native page had its own Edit after the title, right over the description's Edit.
#[test]
fn tasks_issue_tab_edits_the_title_from_a_click_on_it_with_no_edit_beside_it() {
    let rig = tasks_rig();
    {
        let (page, events, _saved) = open_with_state(
            &rig.host,
            &module("tasks", "board"),
            None,
            super::super::StatePlace::Tab,
            None,
        );
        add_issue(&page, &events, "Fix the login redirect");
    }
    let args = json!({ "key": "WEB-1", "projectId": "/work/web" });
    let (page, events, _saved) = open_with_state(
        &rig.host,
        &module("tasks", "issue"),
        Some(&args),
        super::super::StatePlace::Tab,
        None,
    );
    let issue = tree(&events, "the issue", |tree| {
        tree.text().contains("Fix the login redirect") && !title_buttons(&tree.children).is_empty()
    });
    // One button edits the title, and it is the title, in a ghost button that wraps it.
    let buttons = title_buttons(&issue.children);
    assert_eq!(buttons.len(), 1);
    let title = buttons[0];
    assert_eq!(plain_text(&title.children), "Fix the login redirect");
    assert!(title.wrap);
    assert_eq!(title.variant, crate::page::tree::Variant::Ghost);
    fn title_field(nodes: &[Node]) -> bool {
        nodes.iter().any(|node| match node {
            Node::Input(input) => input.label == "Title",
            other => other.parts().into_iter().any(title_field),
        })
    }
    // F2 on it opens the field, as a click does in tasks_issue_tab_saves_a_title_edited_in_place.
    assert!(page.dispatch(
        title.on_key_down.as_ref().unwrap(),
        Some(&json!({ "type": "keydown", "key": "F2" }))
    ));
    tree(&events, "the title field", |tree| {
        title_field(&tree.children)
    });
}

#[test]
fn tasks_issue_tab_saves_a_title_edited_in_place() {
    let rig = tasks_rig();
    {
        let (page, events, _saved) = open_with_state(
            &rig.host,
            &module("tasks", "board"),
            None,
            super::super::StatePlace::Tab,
            None,
        );
        add_issue(&page, &events, "Fix the login redirect");
    }
    let args = json!({ "key": "WEB-1", "projectId": "/work/web" });
    let (page, events, _saved) = open_with_state(
        &rig.host,
        &module("tasks", "issue"),
        Some(&args),
        super::super::StatePlace::Tab,
        None,
    );
    // The title is the button that edits it, as on the desktop.
    let issue = tree(&events, "the issue", |tree| {
        title_buttons(&tree.children).len() == 1
    });
    let title = title_buttons(&issue.children)[0];
    assert_eq!(plain_text(&title.children), "Fix the login redirect");
    assert!(page.dispatch(title.on_click.as_ref().unwrap(), None));
    fn title_input(nodes: &[Node]) -> Option<(Option<String>, Option<String>)> {
        nodes.iter().find_map(|node| match node {
            Node::Input(input) if input.label == "Title" => {
                Some((input.on_change.clone(), input.on_key_down.clone()))
            }
            other => other.parts().into_iter().find_map(title_input),
        })
    }
    let editing = tree(&events, "the title field", |tree| {
        title_input(&tree.children).is_some()
    });
    let (change, key) = title_input(&editing.children).unwrap();
    assert!(page.dispatch(
        &change.unwrap(),
        Some(&json!({ "value": "Fix the sign-in redirect" }))
    ));
    fn typed(nodes: &[Node]) -> bool {
        nodes.iter().any(|node| match node {
            Node::Input(input) => {
                input.label == "Title" && input.value == "Fix the sign-in redirect"
            }
            other => other.parts().into_iter().any(typed),
        })
    }
    tree(&events, "the typed title", |tree| typed(&tree.children));
    assert!(page.dispatch(
        &key.unwrap(),
        Some(&json!({ "type": "keydown", "key": "Enter" }))
    ));
    tree(&events, "the saved title", |tree| {
        tree.text().contains("Fix the sign-in redirect") && title_input(&tree.children).is_none()
    });
}

/// The Issue tab's title row starts with the issue's type icon, the desktop's checked square for a task, at the
/// title row's 18 points and in the task's muted colour. It was a dot.
#[test]
fn tasks_issue_tab_draws_the_type_icon_by_its_title() {
    let rig = tasks_rig();
    {
        let (page, events, _saved) = open_with_state(
            &rig.host,
            &module("tasks", "board"),
            None,
            super::super::StatePlace::Tab,
            None,
        );
        add_issue(&page, &events, "Fix the login redirect");
    }
    let args = json!({ "key": "WEB-1", "projectId": "/work/web" });
    let (_page, events, _saved) = open_with_state(
        &rig.host,
        &module("tasks", "issue"),
        Some(&args),
        super::super::StatePlace::Tab,
        None,
    );
    fn type_icon(nodes: &[Node]) -> Option<&crate::page::tree::Svg> {
        nodes.iter().find_map(|node| match node {
            Node::Svg(icon) if icon.label.as_deref() == Some("Task") => Some(icon),
            other => other.parts().into_iter().find_map(type_icon),
        })
    }
    let issue = tree(&events, "the issue's type icon", |tree| {
        tree.text().contains("Fix the login redirect") && type_icon(&tree.children).is_some()
    });
    let icon = type_icon(&issue.children).unwrap();
    assert_eq!(
        (icon.size, icon.tone),
        (18.0, Some(crate::page::tree::Tone::Muted))
    );
    // A Rust page writes a shape's attributes in key order, so the square's are checked one by one.
    for part in [
        r#"x="2.5""#,
        r#"y="2.5""#,
        r#"width="11""#,
        r#"height="11""#,
        r#"rx="2.5""#,
        r#"<path d="m5.5 8.2 1.8 1.8 3.2-3.6"/>"#,
    ] {
        assert!(icon.markup.contains(part), "{part} in {}", icon.markup);
    }
}

/// Claude Code calls Issues' tools and hooks, as the app's socket hands them to the plugin's main.
#[test]
fn tasks_main_answers_claude_codes_tools_and_hooks() {
    let rig = tasks_rig();
    let caller = super::super::ClaudeCaller {
        pane_id: Some("p1".into()),
        project: json!({ "id": "/work/web", "name": "web", "path": "/work/web" }),
        session: json!({ "id": "s-1", "name": "agent one" }),
    };
    let call = |name: &str, input: serde_json::Value, hook: bool| -> serde_json::Value {
        let (reply, answer) = crate::page::runtime::Reply::channel();
        rig.host.run_for_claude(name, input, &caller, hook, reply);
        serde_json::from_str(&answer.wait(WAIT).expect("an answer").expect("a result")).unwrap()
    };
    let created = call("create", json!({ "title": "Do it", "status": "in_progress" }), false);
    assert_eq!(created["isError"], false);
    assert!(created["text"].as_str().unwrap().starts_with("Created WEB-1 in web."), "{created}");
    assert_eq!(call("get", json!({ "key": "WEB-1" }), false)["text"].as_str().unwrap().lines().next(), Some("WEB-1 [in_progress] (task) Do it"));
    let hook = |event: &str, input: serde_json::Value| -> serde_json::Value { serde_json::from_str(call(event, input, true)["text"].as_str().unwrap()).unwrap() };
    let start = hook("SessionStart", json!({ "session_id": "s-1", "source": "startup" }));
    let context = start["context"].as_str().unwrap_or_default();
    assert!(context.contains("Wings Issues is the local tracker for web") && context.contains("This session is on WEB-1 \"Do it\""), "{start}");
    let denied = hook("PreToolUse", json!({ "session_id": "s-1", "tool_name": "Bash", "tool_input": { "command": "git commit -m \"WEB-1 done\"" } }));
    assert!(denied["deny"].as_str().unwrap_or_default().starts_with("WEB-1 is a local Wings issue"), "{denied}");
    assert_eq!(hook("PreToolUse", json!({ "session_id": "s-1", "tool_name": "Bash", "tool_input": { "command": "ls" } })), json!({}));
}

/// A note typed on the Issue tab is saved through main and shows in the activity.
#[test]
fn tasks_issue_tab_adds_a_note_through_its_main() {
    let rig = tasks_rig();
    {
        let (page, events, _saved) = open_with_state(&rig.host, &module("tasks", "board"), None, super::super::StatePlace::Tab, None);
        add_issue(&page, &events, "Fix the login redirect");
    }
    let args = json!({ "key": "WEB-1", "projectId": "/work/web" });
    let (page, events, _saved) = open_with_state(&rig.host, &module("tasks", "issue"), Some(&args), super::super::StatePlace::Tab, None);
    let issue = tree(&events, "the issue", |tree| input_named(&tree.children, "Note").is_some());
    let note = input_named(&issue.children, "Note").unwrap();
    assert!(page.dispatch(&note, Some(&json!({ "value": "Checked the redirect" }))));
    let typed = tree(&events, "the typed note", |tree| {
        let mut add = Vec::new();
        buttons_named(&tree.children, "Add note", &mut add);
        !add.is_empty()
    });
    let mut add = Vec::new();
    buttons_named(&typed.children, "Add note", &mut add);
    assert!(page.dispatch(&add[0], None));
    tree(&events, "the new note", |tree| tree.text().contains("Checked the redirect") && tree.text().contains("2 entries"));
}

/// The board asks before it deletes an issue, and deleting takes it off the board.
#[test]
fn tasks_board_deletes_an_issue_after_asking() {
    let rig = tasks_rig();
    let (page, events, _saved) = open_with_state(&rig.host, &module("tasks", "board"), None, super::super::StatePlace::Tab, None);
    let board = add_issue(&page, &events, "Fix the login redirect");
    let trash = handler_of(&board.children, &|node| match node {
        Node::Button(button) if button.label.as_deref() == Some("Delete WEB-1") => button.on_click.clone(),
        _ => None,
    })
    .expect("a card has its delete button");
    assert!(page.dispatch(&trash, None));
    let asking = tree(&events, "the question", |tree| tree.text().contains("Fix the login redirectThis removes the issue and its log."));
    let mut delete = Vec::new();
    buttons_named(&asking.children, "Delete WEB-1", &mut delete);
    assert!(page.dispatch(&delete[0], None));
    tree(&events, "the board without it", |tree| tree.text().contains("0 to do") && !tree.text().contains("Fix the login redirect"));
}
