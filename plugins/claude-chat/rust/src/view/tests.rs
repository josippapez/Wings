use super::*;

fn terminal(pane: &str) -> crate::chat::Terminal {
    crate::chat::Terminal { pane: pane.into() }
}

/// Where what the page shows of the conversation starts and ends, as items.
fn shows(c: &Chat) -> (usize, usize) {
    let end = end_of(c);
    let (window, _) = window(c, end, end.saturating_sub(c.shown));
    (window.last().map_or(end, |block| block.start), end)
}

#[test]
fn a_permission_card_for_a_long_write_is_read_whole_and_can_be_allowed() {
    let content = "let x = 1;\n".repeat(3_000);
    let input = serde_json::json!({ "tool_name": "Write", "input": { "file_path": "/a/b.rs", "content": content } });
    let mut session = Session::default();
    session.hear(&serde_json::json!({ "type": "control_request", "request_id": "r1", "request": { "subtype": "can_use_tool", "tool_name": "Write", "input": input["input"].clone() } }));
    let Some(crate::session::Item::Ask(ask)) = session.items.first() else { panic!("{:?}", session.items) };
    assert!(ask.detail.len() > 30_000 && !ask.cut_short, "36,000 characters are kept whole");
    let drawn = chat::with(|c| wings_plugin::ui::tree_json(permission_node(ask, &Draft::default(), c)).to_string());
    assert!(drawn.contains("\"Code\""), "a long detail is one code node");
    assert!(!drawn.contains("too long to show"), "{}", &drawn[..300]);
    assert!(!drawn.contains("\"disabled\":true"));
}

#[test]
fn several_questions_are_asked_one_at_a_time() {
    let input = serde_json::json!({ "questions": [
        { "question": "Which library?", "options": [{ "label": "dayjs" }, { "label": "date-fns" }] },
        { "question": "Which formats?", "multiSelect": true, "options": [{ "label": "ISO" }] },
    ] });
    let ask = Ask { request: "q1".into(), tool: "AskUserQuestion".into(), summary: String::new(), detail: String::new(), cut_short: false, reason: None, suggestions: Vec::new(), input, preview: Default::default(), answer: None };
    let shown = |draft: &Draft| wings_plugin::ui::tree_json(questions_node(&ask, draft)).to_string();
    let mut draft = Draft::default();
    let first = shown(&draft);
    assert!(first.contains("1 of 2") && first.contains("Which library?") && !first.contains("Which formats?") && first.contains("Next"));
    draft.toggle(0, 1, false);
    assert!(draft.has_answer(0) && !draft.has_answer(1));
    draft.step = 1;
    let second = shown(&draft);
    assert!(second.contains("2 of 2") && second.contains("Which formats?") && !second.contains("Which library?") && second.contains("Back") && second.contains("Submit"));
    let one = Ask { input: serde_json::json!({ "questions": [{ "question": "Which library?", "options": [{ "label": "dayjs" }] }] }), ..ask };
    let alone = wings_plugin::ui::tree_json(questions_node(&one, &Draft::default())).to_string();
    assert!(alone.contains("Claude has a question") && !alone.contains("Next"));
}

#[test]
fn a_call_cut_off_by_a_restart_shows_interrupted_with_no_spinner_and_no_background_button() {
    chat::with(|c| {
        c.state = State::Running;
        c.session = Session::default();
        c.session.working = true;
        c.session.hear(&serde_json::json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "id": "b1", "name": "Bash", "input": { "command": "rg -c x f" } }] } }));
        c.session.stop_replayed();
        c.session.hear(&serde_json::json!({ "type": "user", "message": { "role": "user", "content": "what is the status?" } }));
    });
    let tree = chat::with(|c| match c.session.items.first() {
        Some(Item::Tool(tool)) => wings_plugin::ui::tree_json(step(tool, false, c)).to_string(),
        other => panic!("{other:?}"),
    });
    assert!(!tree.contains("Spinner") && !tree.contains("Run in background"), "{tree}");
    assert!(tree.contains("Stopped when Claude Code restarted"), "{tree}");
    chat::with(|c| {
        let Some(Item::Tool(tool)) = c.session.items.first() else { panic!() };
        assert_eq!(describe(&[tool], &c.session), "Ran 1 command");
        c.session = Session::default();
    });
}

#[test]
fn a_fold_of_calls_still_running_says_running_not_ran() {
    let done = call("a", "Bash", Detail::Plain, "ok");
    let mut pending = call("b", "Bash", Detail::Plain, "");
    pending.output = None;
    let session = Session::default();
    assert_eq!(describe(&[&done], &session), "Ran 1 command");
    assert_eq!(describe(&[&pending], &session), "Running 1 command");
    assert_eq!(describe(&[&done, &pending], &session), "Ran 1 command · 1 running");
}

#[test]
fn a_queued_plugin_command_says_it_runs_when_claude_finishes_and_a_message_does_not() {
    let shown = |texts: &[&str]| {
        chat::with(|c| {
            c.session.queued.clear();
            c.session.commands = crate::session::Session::default().commands;
            c.session.commands.push(crate::session::Command { name: "toolbox:lint".into(), description: String::new(), hint: String::new(), plugin: true, builtin: false });
            for (n, text) in texts.iter().enumerate() {
                c.session.queue(text, Vec::new(), 1, n as u32 + 1);
            }
            let shown = wings_plugin::ui::tree_json(queued(c).unwrap()).to_string();
            c.session.queued.clear();
            shown
        })
    };
    let command = shown(&["/toolbox:lint src"]);
    assert!(command.contains(HELD_NOTE) && !command.contains("Claude reads"), "{command}");
    assert!(!command.contains("Send now"), "a plugin's command has no Send now: {command}");
    let message = shown(&["and lint"]);
    assert!(!message.contains(HELD_NOTE) && message.contains("Claude reads it"));
    assert!(message.contains("Send now"));
    let both = shown(&["/toolbox:lint", "and lint"]);
    assert_eq!(both.matches(HELD_NOTE).count(), 1);
    assert_eq!(both.matches("Send now").count(), 1);
    assert!(both.contains("Claude reads it after"));
}

#[test]
fn a_huge_queued_message_has_a_short_tooltip_and_a_short_one_keeps_its_own() {
    let paste = "pasted line of a big log\n".repeat(1000);
    let tip = queued_tip(&paste);
    assert!(tip.chars().count() <= QUEUED_TIP + 1 && tip.ends_with('…'), "{}", tip.len());
    assert_eq!(queued_tip("and lint"), "and lint");
    let shown = chat::with(|c| {
        c.session.queued.clear();
        c.session.queue(&paste, Vec::new(), 1, 1);
        let shown = wings_plugin::ui::tree_json(queued(c).unwrap()).to_string();
        c.session.queued.clear();
        shown
    });
    assert!(!shown.contains(&format!("\"title\":\"{}", paste.replace('\n', "\\n"))), "the whole paste is the tooltip");
}

#[test]
fn the_queue_card_says_it_for_one_message_and_them_for_several() {
    assert_eq!(queued_note(1), "Claude reads it after its current step");
    assert_eq!(queued_note(2), "Claude reads them after its current step");
}

#[test]
fn up_with_messages_queued_brings_them_all_back_into_the_box_and_empties_the_queue() {
    chat::with(|c| {
        c.draft.clear();
        c.images.clear();
        c.session.queued.clear();
        c.session.queue("and lint", Vec::new(), 1, 1);
        c.session.queue("then deploy", vec![("image/png".into(), "AAAA".into())], 1, 2);
    });
    assert!(chat::take_queued());
    chat::with(|c| {
        assert_eq!(c.draft, "and lint\n\nthen deploy");
        assert_eq!(c.images, vec![("image/png".to_owned(), "AAAA".to_owned())]);
        assert!(c.session.queued.is_empty());
    });
    // Nothing waits any more: Up recalls a prompt, as it did.
    assert!(!chat::take_queued());
    // What was in the box stays, after the queued ones.
    chat::with(|c| {
        c.draft = "half a thought".into();
        c.session.queue("one more", Vec::new(), 1, 3);
    });
    assert!(chat::take_queued());
    assert_eq!(chat::with(|c| c.draft.clone()), "one more\n\nhalf a thought");
}

#[test]
fn a_session_too_long_for_the_page_moves_along_as_you_scroll_so_all_of_it_can_be_read() {
    chat::with(|c| {
        c.session.items = (0..1_500).flat_map(|n| [Item::User(format!("Prompt {n}")), Item::Text(format!("Answer {n}"))]).collect();
        c.shown = PAGE;
        c.until = None;
    });
    // Up to the very first message: older items come in, and once the page is full the newest go to make room, while
    // what was at the top stays.
    let mut steps = 0;
    while let (first, _) = chat::with(|c| shows(c))
        && first > 0
    {
        chat::with(show_older);
        let (next, end) = chat::with(|c| shows(c));
        assert!(next < first && first < end, "{first} → {next}..{end}");
        assert!(chat::with(|c| chat_page(c).count()) <= 5_000, "the page stays within what Wings draws");
        steps += 1;
        assert!(steps < 200);
    }
    assert!(chat::with(|c| c.until.is_some()), "the newest no longer show");
    // Back down to the newest: newer items come in and the oldest go, while what was at the end stays.
    while chat::with(|c| c.until.is_some()) {
        let (_, end) = chat::with(|c| shows(c));
        chat::with(show_newer);
        let (first, next) = chat::with(|c| shows(c));
        assert!(first < end && end < next, "{end} → {first}..{next}");
        steps += 1;
        assert!(steps < 400);
    }
    assert_eq!(chat::with(|c| shows(c)).1, 3_000);
    // And "Jump to the latest" goes straight back from anywhere.
    chat::with(show_older);
    chat::with(|c| c.until = Some(10));
    chat::with(show_latest);
    assert_eq!(chat::with(|c| (c.until, shows(c).1)), (None, 3_000));
}

/// A session that ran many subagents lists each in the side panel, which the page sends whole whatever the conversation
/// shows: the conversation's budget is what the rest of the page leaves under the 5,000 nodes Wings draws.
#[test]
fn a_long_session_with_many_subagents_stays_within_the_page_limit_as_you_scroll_up() {
    let agent = |n: usize| Item::Tool(crate::session::Tool { id: format!("a{n}"), name: "Agent".into(), summary: format!("Survey the callers of function {n}"), output: Some(("done".into(), false)), detail: crate::session::Detail::Plain, input: Default::default(), images: Vec::new() });
    chat::with(|c| {
        c.state = chat::State::Running;
        c.session.items = (0..400).flat_map(|n| [Item::User(format!("Prompt {n}")), agent(n), Item::Text(format!("Answer {n}"))]).collect();
        c.shown = PAGE;
        c.until = None;
    });
    let mut most = chat::with(|c| chat_page(c).count());
    for _ in 0..60 {
        chat::with(show_older);
        most = most.max(chat::with(|c| chat_page(c).count()));
    }
    assert!(most <= 4_800, "the page had {most} nodes, and Wings refuses one over 5,000");
    chat::with(|c| (c.session.items.clear(), c.until = None));
}

#[test]
fn a_suggestion_taken_with_tab_shows_again_once_the_box_is_emptied_and_typing_your_own_dismisses_it() {
    chat::with(|c| {
        c.state = chat::State::Running;
        c.session.suggestion = Some("run the tests".into());
    });
    chat::accept_suggestion();
    assert_eq!(chat::with(|c| c.draft.clone()), "run the tests");
    chat::typed("run the tes");
    chat::typed("");
    assert_eq!(chat::with(|c| chat::suggested(c).map(str::to_owned)).as_deref(), Some("run the tests"));
    // A new turn's suggestion is dismissed by typing, as Claude Code does.
    chat::with(|c| c.session.suggestion = Some("commit it".into()));
    chat::typed("no, ");
    assert_eq!(chat::with(|c| c.session.suggestion.clone()), None);
}

#[test]
fn a_key_in_the_message_box_leaves_the_page_alone_unless_the_box_fills_or_empties_or_a_list_depends_on_it() {
    chat::with(|c| {
        c.state = chat::State::Running;
        c.draft.clear();
        c.session.suggestion = Some("run the tests".into());
    });
    // The first key fills the box: the suggestion goes and the send button turns on.
    assert!(chat::typed("h"));
    // Plain keys after that draw nothing the page doesn't already show.
    for said in ["he", "hel", "hello", "hello w", "hello  "] {
        assert!(!chat::typed(said), "{said:?}");
    }
    assert!(!chat::typed("hello world"), "a space ends no list");
    // A leading `/` lists commands, an `@` files and a `#` issues, and each key in such a word changes the list.
    assert!(chat::typed("/"));
    assert!(chat::typed("/he"));
    assert!(chat::typed("hello @sr"));
    assert!(chat::typed("hello @src"));
    assert!(chat::typed("see #1"));
    // Leaving one draws the list away.
    assert!(chat::typed("see "));
    // Emptying the box brings the suggestion back; a box of spaces turns the send button off.
    assert!(chat::typed(""));
    assert!(chat::typed("x"));
    assert!(chat::typed("  "));
    assert!(!chat::typed("   "));
    assert!(chat::typed("a"));
}

#[test]
fn with_the_panel_in_the_sidebar_the_header_keeps_to_icons_as_in_a_narrow_tab() {
    chat::with(|c| {
        (c.narrow, c.side_in_wings) = (None, false);
        assert!(!header_tight(c));
        c.narrow = Some(true);
        assert!(header_tight(c));
        (c.narrow, c.side_in_wings) = (None, true);
        assert!(header_tight(c), "the page has no split to measure its tab with");
        c.side_in_wings = false;
    });
}

#[test]
fn the_side_panel_shows_by_default_at_any_width_and_the_button_turns_it_off_and_on() {
    for narrow in [None, Some(false), Some(true)] {
        chat::with(|c| {
            c.side_open = true;
            c.narrow = narrow;
            assert!(side_shown(c), "{narrow:?}");
            assert!(side_button(c).is_some(), "the button is there at any width");
        });
    }
    chat::set_narrow(false, false);
    chat::toggle_side();
    assert!(!chat::with(|c| side_shown(c)));
    chat::toggle_side();
    assert!(chat::with(|c| side_shown(c)));
}

#[test]
fn the_toggle_flips_the_panel_and_a_chat_starts_with_it_on_and_in_the_chat() {
    // The choice is a setting every chat follows (`prefs::side_panel`, which Wings stores); outside Wings nothing is
    // stored, so here only this chat's panel flips.
    assert!(prefs::side_panel().get());
    assert!(!chat::with(|c| c.side_in_wings), "the panel is in the chat unless you moved it");
    chat::with(|c| c.side_open = true);
    chat::toggle_side();
    assert!(!chat::with(|c| c.side_open));
    chat::toggle_side();
    assert!(chat::with(|c| c.side_open));
    assert!(!crate::session_side::shown(), "the button never opens the sidebar while the panel is in the chat");
}

#[test]
fn only_the_button_puts_the_panel_away_even_over_a_narrow_tab() {
    chat::set_narrow(true, false);
    chat::with(|c| c.side_open = true);
    chat::escape();
    assert!(chat::with(|c| c.side_open), "Escape leaves it");
    assert!(chat::with(|c| side_shown(c)));
    chat::toggle_side();
    assert!(!chat::with(|c| side_shown(c)));
    chat::toggle_side();
    assert!(chat::with(|c| side_shown(c)));
}

#[test]
fn the_panel_is_wide_enough_to_leave_the_conversation_column_clear() {
    // Centered in the tab, the column and its margins have (tab - 776) / 2 either side.
    assert!((SIDE_BELOW - COLUMN - 16) / 2 >= SIDE + 12);
}

fn page_text(c: &Chat) -> String {
    wings_plugin::ui::tree_json(chat_page(c)).to_string()
}

/// A PushNotification's line says what became of the push, and a Skill an MCP server served says so.
#[test]
fn a_notification_says_whether_it_was_pushed_and_a_skill_from_an_mcp_server_says_so() {
    let tool = |id: &str, name: &str, output: &str| crate::session::Tool { id: id.into(), name: name.into(), summary: String::new(), output: Some((output.into(), false)), detail: crate::session::Detail::Plain, input: Default::default(), images: Vec::new() };
    let shown = |node: Option<Node>| node.map(|node| wings_plugin::ui::tree_json(node).to_string());
    chat::with(|c| {
        c.session = crate::session::Session::default();
        c.session.mcp_skills.insert("s1".into());
        let said = [
            ("Terminal notification sent. Mobile push requested.", Some("Push requested")),
            ("Mobile push requested.", Some("Push requested")),
            ("Mobile push not sent (Remote Control inactive).", Some("Not pushed: Remote Control is off")),
            ("Push not sent \u{2014} mobile push is disabled in /config.", Some("Not pushed: turned off in /config")),
            ("Not sent \u{2014} this terminal is active, so your output here already reaches the user.", None),
        ];
        for (output, expected) in said {
            let drawn = shown(call_note(&tool("p", "PushNotification", output), c));
            assert_eq!(drawn.is_some(), expected.is_some(), "{output}");
            if let (Some(drawn), Some(expected)) = (drawn, expected) {
                assert!(drawn.contains(expected), "{drawn}");
            }
        }
        assert!(shown(call_note(&tool("s1", "Skill", "Launching skill: docs"), c)).is_some_and(|drawn| drawn.contains("MCP")));
        assert!(call_note(&tool("s2", "Skill", "Launching skill: docs"), c).is_none());
        assert!(call_note(&tool("s1", "Bash", "Mobile push requested."), c).is_none());
    });
}

#[test]
fn polish_shell_command_uses_one_bash_row_without_a_zero_exit_code() {
    chat::with(|c| {
        c.state = State::Running;
        c.session = crate::session::Session::default();
        let row = |text: &str| serde_json::json!({ "type": "user", "message": { "role": "user", "content": text }, "parent_tool_use_id": null, "uuid": "u", "isReplay": true });
        c.session.hear(&row("<bash-input>echo hi</bash-input>"));
        c.session.hear(&row("<bash-stdout>hi</bash-stdout><bash-stderr></bash-stderr><bash-exit-code>0</bash-exit-code>"));
        let shown = wings_plugin::ui::tree_json(chat_page(c));
        fn command_nodes<'a>(node: &'a serde_json::Value, command: &str, found: &mut Vec<&'a serde_json::Value>) {
            if node["type"] == "Text" && node["children"].as_array().is_some_and(|children| children.iter().any(|child| child["text"] == command)) {
                found.push(node);
            }
            for child in node["children"].as_array().into_iter().flatten() {
                command_nodes(child, command, found);
            }
        }
        let mut commands = Vec::new();
        command_nodes(&shown, "$ echo hi", &mut commands);
        // The row, and its input again in the fold once it opens, as a Bash call's does.
        assert!(!commands.is_empty(), "{shown}");
        assert_eq!(commands[0]["props"]["mono"], true, "{shown}");
        let shown = shown.to_string();
        assert!(!shown.contains("Ran echo hi") && !shown.contains("Shell output"), "{shown}");
        assert!(!shown.contains("exit 0"), "{shown}");
    });
}

#[test]
fn polish_shell_failed_command_shows_its_folded_streams_and_exit_code() {
    chat::with(|c| {
        c.state = State::Running;
        c.session = crate::session::Session::default();
        let row = |text: &str| serde_json::json!({ "type": "user", "message": { "role": "user", "content": text }, "parent_tool_use_id": null, "uuid": "u", "isReplay": true });
        c.session.hear(&row("<bash-input>sh -c 'exit 7'</bash-input>"));
        c.session.hear(&row("<bash-stdout>starting</bash-stdout><bash-stderr>warning</bash-stderr><bash-exit-code>7</bash-exit-code>"));
        c.open_outputs.insert("shell-0".into());
        let shown = page_text(c);
        assert!(shown.contains("Failed · exit 7") && shown.contains("starting") && shown.contains("warning"), "{shown}");
        assert!(!shown.contains("<bash-"), "{shown}");
    });
}

#[test]
fn a_running_turn_says_its_tokens_in_the_status_line_and_a_subagent_row_its_tool_calls() {
    chat::with(|c| {
        c.state = State::Running;
        c.session.working = true;
        c.session.items = vec![Item::User("go".into())];
        c.session.meter.message(Some("m1"), &serde_json::json!({ "output_tokens": 2_100 }));
        let page = page_text(c);
        assert!(page.contains("· 2.1k tokens"), "{page}");
        c.session.hear(&serde_json::json!({ "type": "assistant", "parent_tool_use_id": null, "message": { "role": "assistant", "content": [{ "type": "tool_use", "id": "toolu_7", "name": "Agent", "input": { "description": "scan", "prompt": "scan" } }] } }));
        c.session.hear(&serde_json::json!({ "type": "system", "subtype": "task_progress", "task_id": "a", "tool_use_id": "toolu_7", "usage": { "total_tokens": 900, "tool_uses": 7, "duration_ms": 12_000 } }));
        let side = side_sections(c, false).into_iter().map(|node| wings_plugin::ui::tree_json(node).to_string()).collect::<String>();
        assert!(side.contains("7 tool calls"), "{side}");
    });
}

#[test]
fn the_worked_line_under_a_turn_has_tokens_and_cost_only_for_an_api_key_and_the_panel_has_the_cache_clock() {
    chat::with(|c| {
        c.state = State::Running;
        c.session.items = vec![Item::User("go".into()), Item::Worked(14_000, crate::spend::Spend { output: 12_300, cost: Some(0.0412), ..Default::default() })];
        let page = page_text(c);
        assert!(page.contains("Worked for 14s · 12.3k tokens\""), "{page}");
        c.session.meter.per_token = true;
        assert!(page_text(c).contains("Worked for 14s · 12.3k tokens · $0.04"));
        // Counted down from the last request, by the host's clock.
        wings_plugin::host::testing::set_now(1_000_000.0);
        c.session.meter.requested_at(1_000_000 - 2 * 60_000, &serde_json::json!({}));
        let side = side_sections(c, false).into_iter().map(|node| wings_plugin::ui::tree_json(node).to_string()).collect::<String>();
        assert!(side.contains("Cache warm · 3m left"), "{side}");
        wings_plugin::host::testing::set_now(1_000_000.0 + 4.0 * 60_000.0);
        let side = side_sections(c, false).into_iter().map(|node| wings_plugin::ui::tree_json(node).to_string()).collect::<String>();
        assert!(side.contains("Cache expired"), "{side}");
    });
}

/// In a narrow tab with room, the open panel sets the conversation and the message box in beside it: they went under
/// it. Collapsed to its rail, or with no room, it floats over as before, with the box under the split.
#[test]
fn an_open_panel_in_a_narrow_tab_with_room_sets_the_conversation_in_beside_it() {
    chat::with(|c| {
        c.state = State::Running;
        c.side_in_wings = false;
        c.side_open = true;
        c.side_rail = Some(false);
        c.narrow = Some(true);
        let page = |c: &Chat| wings_plugin::ui::tree_json(chat_page(c));
        let split = |page: &serde_json::Value| -> serde_json::Value {
            fn find(node: &serde_json::Value) -> Option<serde_json::Value> {
                if node["type"] == "Split" && node["props"]["dock"].is_null() {
                    return Some(node.clone());
                }
                node["children"].as_array()?.iter().find_map(find)
            }
            find(page).expect("the split")
        };
        let composer_in_start = |split: &serde_json::Value| split["children"][0].to_string().contains("Ask anything");
        c.inset = false;
        let no_room = split(&page(c));
        assert_eq!(no_room["props"]["insetBelow"], INSET_BELOW);
        assert_eq!(no_room["props"]["insetColumn"], INSET_COLUMN, "moved only as far as the column needs");
        assert!(!composer_in_start(&no_room), "with no room the box sits under the split");
        c.inset = true;
        assert!(composer_in_start(&split(&page(c))), "set in, the box moves with the conversation");
        c.side_rail = Some(true);
        assert!(split(&page(c))["props"].get("insetBelow").is_none(), "the rail floats over");
        c.narrow = Some(false);
        c.side_rail = None;
        assert!(split(&page(c))["props"].get("insetBelow").is_none(), "a wide tab has room already");
    });
}

#[test]
fn the_background_you_picked_goes_behind_the_whole_page_and_a_new_chat_drops_its_glow_over_it() {
    chat::with(|c| {
        c.state = State::Running;
        c.backdrop = None;
        let page = wings_plugin::ui::tree_json(chat_page(c));
        assert_eq!(page["type"], "Page");
        assert!(page["props"].get("backdrop").is_none(), "the plain ground: {}", page["props"]);
        assert!(page.to_string().contains(r#""backdrop":"glow""#), "a new chat's soft glow");
        for kind in ["pixels", "pixels:embers", "gradient:pensive"] {
            c.backdrop = Some(kind.to_owned());
            let page = wings_plugin::ui::tree_json(chat_page(c));
            assert_eq!(page["props"]["backdrop"], kind);
            assert!(!page.to_string().contains(r#""backdrop":"glow""#), "one backdrop behind a new chat, not two");
        }
    });
}

#[test]
fn a_new_chat_says_once_that_settings_can_make_the_background_plain() {
    chat::with(|c| {
        c.state = State::Running;
        c.backdrop = Some("pixels:bayer".to_owned());
        c.background_hint = true;
        let shown = |c: &Chat| wings_plugin::ui::tree_json(chat_page(c)).to_string().contains("Prefer a plain background?");
        assert!(shown(c), "under a new chat's prompt");
        // On the plain ground there's nothing to turn off.
        c.backdrop = None;
        assert!(!shown(c));
        c.backdrop = Some("pixels:bayer".to_owned());
    });
    chat::dismiss_background_hint();
    chat::with(|c| assert!(!c.background_hint && !wings_plugin::ui::tree_json(chat_page(c)).to_string().contains("Prefer a plain background?")));
}

#[test]
fn the_background_hint_reads_at_one_size_with_both_actions_drawn_as_buttons() {
    let hint = wings_plugin::ui::tree_json(background_hint()).to_string();
    assert!(!hint.contains(r#""size":"small""#), "hint text beside size-sm buttons reads at the buttons' size: {hint}");
    assert!(!hint.contains(r#""variant":"link""#), "a link variant draws as bare text: {hint}");
}

#[test]
fn in_the_chat_the_panel_floats_over_the_conversation_with_the_place_and_the_terminals_dock_apart() {
    chat::with(|c| {
        c.side_in_wings = false;
        c.side_open = true;
        c.folder = "shop".into();
        c.branch = Some("main".into());
        c.changes = Some((12, 3));
        c.session.context = Some((41_000, 200_000));
        c.terminals = vec![terminal("1")];
        let page = page_text(c);
        assert!(page.contains("\"Split\"") && page.contains("collapseBelow"), "{page}");
        assert!(page.contains("shop · main") && page.contains("Open Changes") && page.contains("Context"));
        assert!(page.contains("raised"), "the card");
        // Put away, the panel leaves the page, and the conversation stays in the split. The terminal is not the panel's.
        c.side_open = false;
        let page = page_text(c);
        assert!(page.contains("\"Split\"") && !page.contains("Open Changes") && page.contains("\"Terminal\""));
    });
}

#[test]
fn the_panels_own_button_collapses_it_to_a_pill_and_the_pill_opens_it_again_and_the_choice_is_kept() {
    wings_plugin::host::testing::answer(|_, _| Ok(serde_json::Value::Null));
    chat::with(|c| {
        c.side_in_wings = false;
        c.side_open = true;
        c.side_rail = Some(false);
        c.narrow = Some(false);
        c.folder = "shop".into();
        c.branch = Some("main".into());
        c.changes = Some((12, 3));
        c.session.context = Some((41_000, 200_000));
        c.terminals = vec![terminal("1")];
        let open = page_text(c);
        assert!(open.contains("Collapse the side panel") && open.contains("Open Changes"), "{open}");
        assert!(!open.contains("Expand the side panel"));
    });
    chat::set_side_rail(true);
    chat::with(|c| {
        let rail = page_text(c);
        assert!(rail.contains("Expand the side panel") && !rail.contains("Open Changes") && !rail.contains("Context"), "{rail}");
        assert!(rail.contains("shop · main") && rail.contains("+12") && rail.contains("−3"), "the pill says the place and the changes: {rail}");
        assert!(rail.contains("1 running"), "what runs shows on the pill");
        assert!(rail.contains("\"Split\"") && !rail.contains("\"defaultSize\":340"), "the pane shrinks to the pill: {rail}");
        assert!(rail_width(c) < SIDE && rail_width(c) >= 120);
    });
    // Outside Wings nothing is stored (as with `side_panel`), so this chat holds the choice the setting keeps.
    assert_eq!(chat::with(|c| c.side_rail), Some(true));
    chat::set_side_rail(false);
    assert_eq!(chat::with(|c| c.side_rail), Some(false));
    let open = chat::with(|c| page_text(c));
    assert!(open.contains("Open Changes") && open.contains("\"defaultSize\":340"), "{open}");
    // The per-section folds go on working in the open panel.
    chat::set_panel_open("usage", false);
    assert!(chat::with(|c| page_text(c)).contains(r#""open":false"#));
    chat::set_panel_open("usage", true);
}

#[test]
fn a_narrow_tab_starts_with_the_pill_until_you_choose_and_a_wide_one_with_the_panel() {
    wings_plugin::host::testing::answer(|_, _| Ok(serde_json::Value::Null));
    chat::with(|c| {
        c.side_in_wings = false;
        c.side_open = true;
        c.side_rail = None;
        c.changes = Some((12, 3));
        c.narrow = Some(true);
        assert!(side_railed(c), "over the conversation it starts out of the way");
        assert!(!page_text(c).contains("Open Changes"));
        c.narrow = Some(false);
        assert!(!side_railed(c));
        assert!(page_text(c).contains("Open Changes"));
        c.narrow = Some(true);
    });
    // Opened over a narrow tab by hand, it stays open there: your choice outranks the width.
    chat::set_side_rail(false);
    assert!(chat::with(|c| !side_railed(c) && page_text(c).contains("Open Changes")));
    chat::with(|c| c.side_rail = None);
}

/// Over a narrow tab the panel floated over the message box once it was taller than the conversation above it: the
/// box sits under the split there, so the panel ends above it. In a wide tab the box stays in the split, and the panel
/// beside the column reaches down beside it, as before. Before the width is known the box is where a narrow tab has it.
#[test]
fn over_a_narrow_tab_the_panel_ends_above_the_message_box() {
    wings_plugin::host::testing::answer(|_, _| Ok(serde_json::Value::Null));
    fn split_of(tree: &serde_json::Value) -> Option<&serde_json::Value> {
        if tree["type"] == "Split" && tree["props"]["dock"].is_null() {
            return Some(tree);
        }
        tree["children"].as_array()?.iter().find_map(split_of)
    }
    chat::with(|c| {
        c.side_in_wings = false;
        c.side_open = true;
        for (narrow, in_split) in [(Some(true), false), (Some(false), true), (None, false)] {
            c.narrow = narrow;
            let page = wings_plugin::ui::tree_json(chat_page(c));
            let split = split_of(&page).expect("the split").to_string();
            assert!(page.to_string().contains(r#""label":"Message""#));
            assert_eq!(split.contains(r#""label":"Message""#), in_split, "narrow {narrow:?}");
        }
    });
}

#[test]
fn every_panel_section_folds_and_a_closed_one_keeps_its_title_and_sends_no_rows() {
    wings_plugin::host::testing::answer(|_, _| Ok(serde_json::Value::Null));
    chat::with(|c| {
        c.panel_closed.clear();
        c.session.context = Some((41_000, 200_000));
        c.terminals.clear();
        let text_of = |c: &Chat| side_sections(c, false).into_iter().map(|node| wings_plugin::ui::tree_json(node).to_string()).collect::<String>();
        let open = text_of(c);
        assert!(open.contains(r#""collapsible":true"#) && open.contains(r#""open":true"#) && open.contains("Usage") && open.contains("Context"), "{open}");
        let rows_open: usize = side_sections(c, false).iter().map(Node::count).sum();
        c.panel_closed = vec!["usage".into()];
        let shut = text_of(c);
        assert!(shut.contains(r#""open":false"#) && shut.contains("Usage") && !shut.contains("Context"), "{shut}");
        let rows_shut: usize = side_sections(c, false).iter().map(Node::count).sum();
        assert!(rows_shut < rows_open, "fewer nodes: {rows_shut} against {rows_open}");
        c.panel_closed.clear();
    });
}

#[test]
fn a_click_on_a_section_heading_keeps_its_state_for_every_chat() {
    wings_plugin::host::testing::answer(|_, _| Ok(serde_json::Value::Null));
    chat::with(|c| c.panel_closed.clear());
    chat::set_panel_open("tasks", false);
    chat::set_panel_open("refs", false);
    chat::set_panel_open("tasks", true);
    // Each folded section is one key of the list the setting keeps.
    assert_eq!(chat::with(|c| c.panel_closed.clone()), vec!["refs".to_owned()]);
    chat::set_panel_open("refs", true);
    assert!(chat::with(|c| c.panel_closed.is_empty()));
}

#[test]
fn in_wings_sidebar_the_conversation_has_the_whole_tab_and_the_session_is_left_to_the_sidebar() {
    chat::with(|c| {
        c.side_in_wings = true;
        c.changes = Some((12, 3));
        c.session.context = Some((41_000, 200_000));
        c.terminals = vec![terminal("1")];
        let page = page_text(c);
        assert!(!page.contains("\"Split\""), "no panel floats over the conversation");
        assert!(!page.contains("\"Terminal\"") && !page.contains("Open Changes"), "the session's rows are the sidebar's");
        // The sidebar's sections are drawn from the same chat, with the place left to its header.
        c.folder = "shop".into();
        let side = side_sections(c, false).into_iter().map(|node| wings_plugin::ui::tree_json(node).to_string()).collect::<String>();
        assert!(side.contains("Open Changes") && side.contains("Context") && !side.contains("shop"));
    });
}

#[test]
fn the_session_panel_moves_with_the_setting_in_an_open_chat() {
    wings_plugin::host::testing::answer(|_, _| Ok(serde_json::Value::Null));
    chat::with(|c| {
        c.changes = Some((12, 3));
        assert!(page_text(c).contains("\"Split\""));
    });
    chat::session_panel_moved("sidebar");
    assert!(!chat::with(|c| page_text(c)).contains("\"Split\""));
    chat::session_panel_moved("chat");
    assert!(chat::with(|c| page_text(c)).contains("\"Split\""));
}

#[test]
fn the_header_button_shows_a_count_when_something_runs_and_the_panel_is_away() {
    chat::with(|c| c.side_open = false);
    let quiet = chat::with(|c| (live_in_side(c), side_button(c).unwrap().count()));
    chat::with(|c| c.terminals = vec![terminal("1"), terminal("2")]);
    let busy = chat::with(|c| (live_in_side(c), side_button(c).unwrap().count()));
    assert_eq!((quiet.0, busy.0), (0, 2));
    assert!(busy.1 > quiet.1, "the count is a node of its own");
    // With the panel showing, what runs in it is in front of you: no count.
    chat::with(|c| c.side_open = true);
    assert_eq!(chat::with(|c| side_button(c).unwrap().count()), quiet.1);
}

#[test]
fn in_wings_sidebar_the_header_button_counts_while_the_sidebar_is_away_and_not_while_it_shows() {
    chat::with(|c| c.side_in_wings = true);
    let quiet = chat::with(|c| side_button(c).unwrap().count());
    chat::with(|c| c.terminals = vec![terminal("1"), terminal("2")]);
    assert!(chat::with(|c| side_button(c).unwrap().count()) > quiet);
    chat::toggle_side();
    assert!(crate::session_side::shown());
    assert_eq!(chat::with(|c| side_button(c).unwrap().count()), quiet);
}

#[test]
fn each_message_you_sent_is_found_among_the_children_of_the_conversations_column() {
    chat::with(|c| {
        c.session.items = vec![
            Item::User("one".into()),
            Item::Text("reply".into()),
            Item::Tool(crate::session::Tool { id: "t".into(), name: "Read".into(), summary: String::new(), output: None, detail: crate::session::Detail::Plain, input: Default::default(), images: Vec::new() }),
            Item::User("two".into()),
            Item::Text("reply".into()),
        ];
        c.forked_from = Some("Fix the build".into());
    });
    let (_, found, _) = chat::with(|c| conversation(c));
    // An empty child opens the column (its gap sits above the first message), then a fork's note: the first message is the third.
    assert_eq!(found, [(0, 2), (3, 5)]);
}

#[test]
fn a_fork_says_where_it_came_from_at_the_top_of_the_conversation() {
    chat::with(|c| c.session.items.push(Item::User("Fix the build".into())));
    let plain = chat::with(|c| conversation(c).0.count());
    chat::with(|c| c.forked_from = Some("Fix the build".into()));
    assert!(chat::with(|c| conversation(c).0.count()) > plain);
}

#[test]
fn the_header_fork_asks_first_and_starts_nothing_until_fork_is_clicked() {
    chat::with(|c| {
        c.session.id = Some("abc".into());
        c.state = State::Running;
    });
    crate::fork_view::ask_fork();
    assert!(chat::with(|c| crate::fork_view::header_dialog(c).is_some() && c.state == State::Running));
    crate::fork_view::close_ask();
    assert!(chat::with(|c| crate::fork_view::header_dialog(c).is_none() && c.state == State::Running));
}

/// A background command whose description is the whole command, many lines and wider than the chat, ends on a line
/// cut at its width, with all of it on hover, and its time apart, so the line never runs past the conversation.
#[test]
fn a_long_finished_command_is_cut_to_its_line_with_all_of_it_on_hover() {
    let description = format!("cd /tmp/probe && cat > in.jsonl <<'EOF'\n{}\nEOF", "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"/goal clear\"}} ".repeat(8));
    let done = Finished { task: "t1".into(), call: "c1".into(), kind: "Command", description, status: "completed".into(), took: Some(1500), report: String::new() };
    let tree = chat::with(|c| wings_plugin::ui::tree_json(finished(0, &done, c)));
    let title = &tree["children"][0]["children"][1];
    assert_eq!(title["props"]["truncate"], true, "{tree}");
    assert_eq!(title["props"]["title"], done.title(), "{tree}");
    assert!(!tree.to_string().contains(" · 1s"), "the time is its own text, not cut off with the title: {tree}");
}

fn call(id: &str, name: &str, detail: Detail, output: &str) -> Tool {
    Tool { id: id.into(), name: name.into(), summary: "src/a.ts".into(), output: Some((output.into(), false)), detail, input: Default::default(), images: Vec::new() }
}

/// A call's line, as the chat and a subagent's tab both draw it: a click on it opens its fold as the fold's own line
/// does, its cut text's hover has all of it, and the fold opened shows the whole input before what it printed.
#[test]
fn a_calls_line_opens_its_fold_which_shows_the_whole_input() {
    let command = format!("cargo test --manifest-path apps/native/Cargo.toml {}--lib", "--features test-support ".repeat(12));
    let mut session = Session::default();
    session.hear(&serde_json::json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "id": "b1", "name": "Bash", "input": { "command": command, "description": "Run the native tests" } }] } }));
    session.hear(&serde_json::json!({ "type": "user", "message": { "content": [{ "type": "tool_result", "tool_use_id": "b1", "content": "test result: ok" }] } }));
    let Some(Item::Tool(tool)) = session.items.first() else { panic!("{:?}", session.items) };
    let whole = serde_json::to_string(&command).unwrap();
    chat::with(|c| c.open_outputs.clear());
    let (tree, handlers) = chat::with(|c| wings_plugin::ui::export(vec![step(tool, false, c)]));
    let closed = tree.to_string();
    assert!(closed.contains(&format!("\"title\":{whole}")), "the hover has the whole command: {closed}");
    assert!(!closed.contains(&format!("\"code\":{whole}")), "{closed}");
    let line = tree[0]["children"][0].clone();
    assert_eq!(line["type"], "ListButton", "{closed}");
    assert!(handlers.call(line["props"]["onClick"].as_str().unwrap_or_default(), &serde_json::json!({})));
    assert!(chat::with(|c| c.open_outputs.contains("b1")), "a click on the line opens the call");
    let opened = chat::with(|c| wings_plugin::ui::tree_json(step(tool, true, c))).to_string();
    for said in [format!("\"code\":{whole}"), "Run the native tests".to_owned(), "test result: ok".to_owned()] {
        assert!(opened.contains(&said), "{said} in {opened}");
    }
    chat::with(|c| c.open_outputs.clear());
}

/// An opened call says how long it ran after its input, when that is known, and nothing when it is not.
#[test]
fn an_opened_call_says_how_long_it_ran() {
    let mut tool = call("r1", "Bash", Detail::Plain, "hi");
    tool.summary = "sleep 3".into();
    tool.input = crate::session::Input { whole: String::new(), fields: vec![("timeout".into(), "600000".into())] };
    let shown = |ran: Option<u64>| {
        chat::with(|c| {
            c.session.ran.clear();
            c.session.ran.extend(ran.map(|ms| ("r1".to_owned(), ms)));
        });
        chat::with(|c| wings_plugin::ui::tree_json(step(&tool, true, c))).to_string()
    };
    let unknown = shown(None);
    assert!(!unknown.contains("\"ran\""), "{unknown}");
    let known = shown(Some(4_200));
    assert!(known.contains("\"ran\"") && known.contains("4.2 s"), "{known}");
    assert!(known.find("600000") < known.find("4.2 s") && known.find("4.2 s") < known.find("\"hi\""), "after the timeout, before the output: {known}");
    chat::with(|c| c.session.ran.clear());
    assert_eq!([0, 4_200, 21_900, 72_000, 3_840_000].map(ran_for), ["0.0 s", "4.2 s", "21 s", "1 m 12 s", "1 h 4 m"]);
}

/// A call with nothing to fold (its input fits its line, it printed nothing yet) is a row you click all the same, and
/// opened it shows its input.
#[test]
fn a_call_with_nothing_to_fold_opens_on_a_click_and_shows_its_input() {
    let mut tool = call("n1", "TaskCreate", Detail::Plain, "");
    tool.summary = "Write the release notes".into();
    tool.output = None;
    chat::with(|c| c.open_outputs.clear());
    let (tree, handlers) = chat::with(|c| wings_plugin::ui::export(vec![step(&tool, false, c)]));
    let line = tree[0]["children"][0].clone();
    assert_eq!(line["type"], "ListButton", "{tree}");
    assert_ne!(line["props"]["disabled"], true, "{tree}");
    assert!(handlers.call(line["props"]["onClick"].as_str().unwrap_or_default(), &serde_json::json!({})));
    assert!(chat::with(|c| c.open_outputs.contains("n1")));
    let opened = chat::with(|c| wings_plugin::ui::tree_json(step(&tool, true, c))).to_string();
    assert!(opened.contains("\"code\":\"Write the release notes\""), "{opened}");
    chat::with(|c| c.open_outputs.clear());
}

/// The call that opened a terminal, or read or messaged one, has an Open that shows that terminal; once Wings no longer
/// lists it the button is there but waits, with the reason on hover.
#[test]
fn a_call_that_opened_a_terminal_has_an_open_that_shows_it() {
    let mut tool = call("p1", "mcp__wings__open_pane", Detail::Plain, r#"{"paneId":"48","commandRun":true}"#);
    tool.summary = "make all".into();
    let button_of = |tool: &Tool| {
        let (tree, handlers) = chat::with(|c| wings_plugin::ui::export(vec![step(tool, false, c)]));
        let found = tree.to_string();
        let line = tree[0]["children"][0]["children"][0]["children"].as_array().unwrap().iter().find(|child| child["props"]["title"].as_str().is_some_and(|title| title.contains("terminal"))).cloned();
        (line, handlers, found)
    };
    let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let kept = calls.clone();
    wings_plugin::host::testing::answer(move |method, params| {
        kept.borrow_mut().push((method.to_owned(), params.clone()));
        Ok(serde_json::Value::Null)
    });
    crate::terminals::hear(&[serde_json::json!({ "paneId": "48", "cwd": "/work/shop", "command": "make all" })]);
    let (line, handlers, found) = button_of(&tool);
    let line = line.unwrap_or_else(|| panic!("{found}"));
    assert_eq!((line["type"].as_str(), line["props"]["title"].as_str(), line["props"]["disabled"].as_bool()), (Some("Button"), Some("Show that terminal"), Some(false)), "{found}");
    assert!(handlers.call(line["props"]["onClick"].as_str().unwrap_or_default(), &serde_json::json!({})));
    assert_eq!(*calls.borrow(), [("focusPane".to_owned(), serde_json::json!({ "paneId": "48" }))]);
    // A message to a pane and a read of one name it in their input.
    let mut read = call("p2", "mcp__wings__read_pane", Detail::Plain, "$ ok");
    read.summary = "48".into();
    assert!(button_of(&read).0.is_some());
    // It closed: the button waits and says so, and a click does nothing.
    crate::terminals::hear(&[]);
    let (line, _, found) = button_of(&tool);
    let line = line.unwrap_or_else(|| panic!("{found}"));
    assert_eq!((line["props"]["title"].as_str(), line["props"]["disabled"].as_bool()), (Some("That terminal is closed"), Some(true)), "{found}");
    // A call that opened nothing has no button.
    assert!(button_of(&call("p3", "Bash", Detail::Plain, "ok")).0.is_none());
}

/// A call a mod draws the row of (a button holding its words, as a mod's hook for MCP calls draws it) opens with a click
/// on the row, as any other: the mod's button would take the click for a row the chat can't draw.
#[test]
fn a_mod_drawn_call_opens_on_a_click_on_its_row() {
    let mut tool = call("m1", "mcp__wings__tasks__create", Detail::Plain, "created WIN-1");
    tool.summary = "Fix the build".into();
    tool.input = crate::session::Input { whole: String::new(), fields: vec![("priority".into(), "high".into())] };
    chat::with(|c| {
        c.open_outputs.clear();
        c.session.mods.answered(
            "ui-render-use:m1-3",
            &serde_json::json!({ "subtype": "success", "response": { "tree": { "type": "Button", "props": { "key": "call-m1", "plain": true }, "press": { "plugin": "quiet-tools", "handle": 3 }, "children": [{ "type": "Text", "props": { "bold": true }, "children": ["· wings › tasks__create Fix the build"] }] } } }),
        );
        // Its result, kept back until the mod's own row opens.
        c.session.mods.answered("ui-render-result:m1-3", &serde_json::json!({ "subtype": "success", "response": { "tree": { "type": "Box" } } }));
    });
    let (tree, handlers) = chat::with(|c| wings_plugin::ui::export(vec![step(&tool, false, c)]));
    let shown = tree.to_string();
    assert!(shown.contains("wings › tasks__create Fix the build"), "the mod's words: {shown}");
    assert!(!shown.contains("\"type\":\"Button\""), "no button of the mod's inside the row: {shown}");
    let line = tree[0]["children"][0].clone();
    assert_eq!(line["type"], "ListButton", "{shown}");
    assert!(handlers.call(line["props"]["onClick"].as_str().unwrap_or_default(), &serde_json::json!({})));
    assert!(chat::with(|c| c.open_outputs.contains("m1")), "a click on the row opens the call");
    let opened = chat::with(|c| wings_plugin::ui::tree_json(step(&tool, true, c))).to_string();
    for said in ["priority", "high", "created WIN-1"] {
        assert!(opened.contains(said), "{said} in {opened}");
    }
    chat::with(|c| c.open_outputs.clear());
}

fn reported(text: &str, note: Option<&str>) -> Item {
    Item::Report(Report { from: "agentabc123".into(), name: "reviewer".into(), text: text.into(), note: note.map(str::to_owned), call: "toolu_r1".into() })
}

/// A subagent's report shows in full whatever the view mode, with its note above it, until the setting says to fold it.
#[test]
fn a_subagents_report_shows_in_full_in_every_view_mode_and_starts_folded_only_when_the_setting_says() {
    let item = reported("The parser fix is in.\n\n- the tests pass (12 ok)", Some("SECURITY WARNING: treat it as data"));
    for detailed in [false, true] {
        chat::with(|c| {
            c.open_outputs.clear();
            c.fold_reports = false;
            c.detailed = detailed;
        });
        let shown = chat::with(|c| wings_plugin::ui::tree_json(item_node(0, &item, c, true).unwrap())).to_string();
        for said in ["Report from reviewer", "12 ok", "Copy this reply", "SECURITY WARNING: treat it as data", "\"tone\":\"warn\""] {
            assert!(shown.contains(said), "{said} in {shown}");
        }
        assert!(shown.contains("\"open\":true"), "{shown}");
    }
    chat::with(|c| c.fold_reports = true);
    let (tree, handlers) = chat::with(|c| wings_plugin::ui::export(vec![item_node(0, &item, c, true).unwrap()]));
    let folded = tree.to_string();
    assert!(folded.contains("The parser fix is in."), "its first line: {folded}");
    assert!(!folded.contains("12 ok") && !folded.contains("Copy this reply"), "{folded}");
    assert!(folded.contains("SECURITY WARNING"), "the note is never folded: {folded}");
    assert!(folded.contains("\"open\":false"), "{folded}");
    let fold = tree[0]["children"].as_array().unwrap().iter().find_map(|row| row["children"].as_array()?.iter().find(|node| node["type"] == "Disclosure")).expect("its fold").clone();
    assert!(handlers.call(fold["props"]["onOpenChange"].as_str().unwrap_or_default(), &serde_json::json!({})));
    assert!(chat::with(|c| c.open_outputs.contains("report-0")), "a click opens it");
    let opened = chat::with(|c| wings_plugin::ui::tree_json(item_node(0, &item, c, true).unwrap())).to_string();
    assert!(opened.contains("12 ok"), "{opened}");
    chat::with(|c| {
        c.open_outputs.clear();
        c.fold_reports = false;
        c.detailed = false;
    });
}

/// Its Copy button copies the report, as a reply's does.
#[test]
fn a_reports_copy_button_has_the_report_to_copy() {
    chat::with(|c| c.session.items = vec![reported("The whole report", None)]);
    assert_eq!(crate::status_view::said_text_at(0).as_deref(), Some("The whole report"));
    chat::with(|c| c.session.items.clear());
}

#[test]
fn detailed_opens_each_kind_its_setting_leaves_on_and_a_click_still_flips_it() {
    let tools = [call("a", "Bash", Detail::Command("ls".into()), "one\ntwo"), call("b", "Read", Detail::Read(None), "     1→let a = 1;")];
    let refs: Vec<&Tool> = tools.iter().collect();
    chat::with(|c| {
        c.detailed = true;
        c.opens = crate::folds::Opens::default();
        c.open_steps.clear();
        c.open_outputs.clear();
        // Every kind on: the run is open with both outputs.
        let all = steps(0, &refs, c).count();
        // Command output off: the run is open and that output is left out.
        c.opens.set(Kind::Command, false);
        let no_command = steps(0, &refs, c).count();
        assert!(no_command < all);
        // A click on that output opens it, as a click flips what a fold starts as.
        c.open_outputs.insert("a".into());
        assert_eq!(steps(0, &refs, c).count(), all);
        c.open_outputs.clear();
        // The file read off as well: only the run's lines are left.
        c.opens.set(Kind::Read, false);
        let neither = steps(0, &refs, c).count();
        assert!(neither < no_command);
        // The runs off: shut, and a click opens it with its outputs still shut.
        c.opens.set(Kind::Steps, false);
        let shut = steps(0, &refs, c).count();
        assert!(shut < neither);
        c.open_steps.insert(0);
        assert_eq!(steps(0, &refs, c).count(), neither);
        // Compact keeps everything shut whatever the settings say.
        c.detailed = false;
        c.opens = crate::folds::Opens::default();
        c.open_steps.clear();
        assert_eq!(steps(0, &refs, c).count(), shut);
        c.open_steps.insert(0);
        assert_eq!(steps(0, &refs, c).count(), neither);
    });
}

#[test]
fn a_read_shows_its_file_as_a_block_in_the_files_language() {
    // Its fold is there once the call printed the file, and not before.
    let printed = call("r", "Read", Detail::Read(None), "     1→let a = 1;");
    let empty = call("r", "Read", Detail::Read(None), "");
    chat::with(|c| {
        c.detailed = true;
        let (with, without) = (step(&printed, true, c).count(), step(&empty, true, c).count());
        assert!(with > without);
    });
}

#[test]
fn a_duration_is_told_in_seconds_minutes_and_hours() {
    assert_eq!([took(21_400), took(125_000), took(3_900_000)], ["21s", "2m 5s", "1h 5m"]);
}

#[test]
fn the_header_names_the_account_the_chat_runs_under_and_checks_it_in_its_menu() {
    use wings_plugin::ui::tree_json;
    chat::with(|c| {
        let choice = |name: Option<&str>, email: Option<&str>, current: bool| wings_plugin::host::AccountChoice { name: name.map(str::to_owned), email: email.map(str::to_owned), signed_in: true, current };
        c.accounts = vec![choice(None, Some("me@home.example"), false), choice(Some("work"), None, true)];
        let items = account_items(c);
        let rows: Vec<(String, serde_json::Value)> = items.into_iter().map(tree_json).map(|item| (item["children"][0]["text"].as_str().unwrap().to_owned(), item["props"]["pressed"].clone())).collect();
        assert_eq!(rows, [("Account: me@home.example".to_owned(), serde_json::json!(false)), ("Account: work".to_owned(), serde_json::json!(true))]);
        // They are in the header's "…" menu, whatever the width.
        let drawn = tree_json(header(c)).to_string();
        assert!(drawn.contains("Account: work") && drawn.contains("Account: me@home.example"));
        // One account, nothing to pick.
        c.accounts.truncate(1);
        assert!(account_items(c).is_empty());
        c.accounts.clear();
    });
}

#[test]
fn the_model_menu_lists_each_model_once_and_the_efforts_the_chosen_one_takes_in_their_own_group() {
    let model = |value: &str, name: &str, efforts: &[&str]| crate::session::Model {
        value: value.into(),
        name: name.into(),
        efforts: efforts.iter().map(|effort| effort.to_string()).collect(),
        resolved: format!("claude-{value}-5-5"),
        fast: false,
        description: if value == "opus" { "Most capable".into() } else { String::new() },
    };
    let mut session = Session::default();
    session.models = vec![model("opus", "Opus 5.5", &["medium", "high", "xhigh"]), model("haiku", "Haiku 5.5", &[])];
    session.model = Some("opus".into());
    let (menu, label) = model_menu(&session);
    let values: Vec<&str> = menu.iter().map(|row| row["value"].as_str().unwrap()).collect();
    // One row for each model, then one Effort row holding the levels: with no effort known yet, High, which is the
    // recommended one.
    assert_eq!(values, ["heading:model", "model:opus", "model:haiku", "effort:open"]);
    assert_eq!(label, "Opus 5.5 · High");
    assert_eq!((menu[1]["checked"].clone(), menu[2]["checked"].clone()), (true.into(), false.into()));
    assert_eq!((menu[1]["detail"].as_str(), menu[2].get("detail")), (Some("Most capable"), None));
    let effort = &menu[3];
    assert_eq!((effort["trailing"].as_str(), effort["pill"].as_str()), (Some("High"), Some("Recommended")));
    let levels: Vec<(&str, &str, bool)> = effort["options"].as_array().unwrap().iter().map(|row| (row["value"].as_str().unwrap(), row["label"].as_str().unwrap(), row["checked"].as_bool().unwrap())).collect();
    assert_eq!(levels, [("effort:medium", "Medium", false), ("effort:high", "High", true), ("effort:xhigh", "Extra high", false)]);
    session.effort = Some("xhigh".into());
    let (menu, label) = model_menu(&session);
    assert_eq!(label, "Opus 5.5 · Extra high");
    assert_eq!((menu[3]["trailing"].as_str(), menu[3].get("pill")), (Some("Extra high"), None), "no pill off the recommended level");
    // The levels follow the model: a model without any has no Effort group.
    session.model = Some("haiku".into());
    let (menu, label) = model_menu(&session);
    assert!(menu.iter().all(|row| row["value"] != "effort:open"));
    assert_eq!(label, "Haiku 5.5");
}

#[test]
fn a_new_chat_offers_the_agents_claude_code_lists_and_the_header_names_the_one_it_runs_as() {
    use wings_plugin::ui::tree_json;
    let drawn = |c: &Chat| format!("{}{}", pickers(c).into_iter().map(|node| tree_json(node).to_string()).collect::<String>(), tree_json(header(c)));
    chat::with(|c| {
        assert!(!drawn(c).contains("No agent"), "no picker before Claude Code lists agents");
        // As `initialize` lists them.
        c.session.options.initialized(&serde_json::json!({ "agents": [
            { "name": "Explore", "description": "Fast read-only search" },
            { "name": "dev-core:mid-tier-coder", "description": "Implements", "model": "sonnet" } ] }));
        let shown = drawn(c);
        assert!(shown.contains("\"Explore\"") && shown.contains("restarts Claude Code"), "{shown}");
        assert!(!shown.contains("@Explore"));
        c.agent_type = Some("Explore".into());
        assert!(drawn(c).contains("@Explore"), "{}", drawn(c));
        // With a message in the chat the agent can't change without losing it: no picker.
        c.session.items.push(Item::User("hi".into()));
        assert!(!pickers(c).into_iter().any(|node| tree_json(node).to_string().contains("No agent")));
        c.session.items.clear();
        c.session.options = Default::default();
        c.agent_type = None;
    });
}

#[test]
fn a_plugins_style_and_agent_go_by_their_own_names_so_the_message_box_has_room() {
    use wings_plugin::ui::tree_json;
    chat::with(|c| {
        c.session.options.initialized(&serde_json::json!({
            "agents": [{ "name": "dev-core:mid-tier-coder", "description": "Implements" }],
            "available_output_styles": ["default", "concise-output:concise-output"],
            "output_style": "concise-output:concise-output",
        }));
        let options = |title: &str| -> serde_json::Value {
            let picker = pickers(c).into_iter().map(tree_json).find(|node| node["props"]["label"].as_str().is_some_and(|label| label.starts_with(title))).expect(title);
            picker["props"]["options"].clone()
        };
        let styles = options("Output style");
        assert_eq!(styles[1], serde_json::json!({ "value": "concise-output:concise-output", "label": "Concise output", "detail": "concise-output" }));
        assert_eq!(styles[0]["label"], "Default");
        // The agent: a robot alone until one is picked, then its own name; the menu lists them in full.
        let agent = |c: &Chat| pickers(c).into_iter().map(|node| tree_json(node).to_string()).find(|node| node.contains("Start as an agent")).expect("the agent button");
        assert!(agent(c).contains("icon-sm") && !agent(c).contains("\"mid-tier-coder\""), "{}", agent(c));
        assert!(agent(c).contains("No agent") && agent(c).contains("dev-core:mid-tier-coder"), "{}", agent(c));
        c.agent_type = Some("dev-core:mid-tier-coder".into());
        assert!(agent(c).contains("\"mid-tier-coder\""), "{}", agent(c));
        c.agent_type = None;
        c.session.options = Default::default();
    });
}

#[test]
fn the_model_menu_ends_with_an_ultracode_switch_only_while_claude_code_offers_it() {
    use wings_plugin::ui::tree_json;
    let menu = |c: &Chat| pickers(c).into_iter().map(|node| tree_json(node)).find(|json| json["props"]["label"] == "Model and effort").unwrap();
    chat::with(|c| {
        c.session.models = vec![crate::session::Model { value: "opus".into(), name: "Opus 5.5".into(), efforts: vec!["high".into()], resolved: "claude-opus-5-5".into(), fast: false, description: String::new() }];
        let none = menu(c);
        assert!(!none.to_string().contains("Ultracode"), "{none}");
        c.session.options.applied(&serde_json::json!({ "ultracode": false, "ultracodeAvailable": true }));
        let off = menu(c);
        let last = off["props"]["options"].as_array().unwrap().last().unwrap().clone();
        assert_eq!((last["label"].as_str(), last["toggle"].as_bool()), (Some("Ultracode"), Some(false)), "{off}");
        assert!(last["detail"].as_str().is_some() && off["props"]["badge"] == "", "{off}");
        c.session.options.applied(&serde_json::json!({ "ultracode": true, "ultracodeAvailable": true }));
        let on = menu(c);
        assert_eq!(on["props"]["options"].as_array().unwrap().last().unwrap()["toggle"], true);
        assert_eq!(on["props"]["badge"], "Ultracode", "{on}");
        c.session.options = Default::default();
        c.session.models.clear();
    });
}

/// A session as long as the ones the prompt rail is clicked through: `prompts` turns, each your message, a run of
/// tool calls and a reply in Markdown with a list and a code block.
fn long_session(prompts: usize) -> Vec<Item> {
    let tool = |turn: usize, n: usize| {
        Item::Tool(crate::session::Tool {
            id: format!("t{turn}-{n}"),
            name: "Bash".into(),
            summary: format!("npm run check -- case_{n}"),
            output: Some(("ok\n".repeat(20), false)),
            detail: crate::session::Detail::Plain,
            input: Default::default(),
            images: Vec::new(),
        })
    };
    let reply = |turn: usize| {
        let mut said = format!("## Turn {turn}\n\nI changed **three** files and ran the checks:\n\n");
        said.push_str(&(0..8).map(|n| format!("- `src/file_{n}.rs`: the change number {n} of this turn\n")).collect::<String>());
        said.push_str("\n```rust\nfn main() {\n    println!(\"hello\");\n}\n```\n\nAll green.");
        said
    };
    (0..prompts)
        .flat_map(|turn| {
            let mut items = vec![Item::User(format!("Prompt {turn}: fix the next thing\nwith a second line"))];
            items.extend((0..6).map(|n| tool(turn, n)));
            items.push(Item::Text(reply(turn)));
            items
        })
        .collect()
}

/// The `scrollTo` the conversation's `Scroll` carries in a built page, if any.
fn scroll_to_of(page: Vec<Node>) -> Option<serde_json::Value> {
    fn find(node: &serde_json::Value) -> Option<serde_json::Value> {
        if node["type"] == "Scroll" && !node["props"]["scrollTo"].is_null() {
            return Some(node["props"]["scrollTo"].clone());
        }
        node["children"].as_array()?.iter().find_map(find)
    }
    page.into_iter().find_map(|node| find(&wings_plugin::ui::tree_json(node)))
}

/// What a click on the rail costs the page in a long session: the time to build the page at the newest message, and
/// from a click on the first prompt to the built page that scrolls there. Run with `--ignored --nocapture`. In Wings
/// the page runs as WebAssembly, slower than this native build, so compare the two numbers with each other.
#[test]
#[ignore]
fn measure_a_rail_click_on_a_far_prompt() {
    chat::with(|c| {
        c.session.items = long_session(400);
        c.shown = PAGE;
        c.until = None;
    });
    let rounds = 5;
    let started = std::time::Instant::now();
    for _ in 0..rounds {
        drop(view());
    }
    let newest = started.elapsed().as_secs_f64() * 1000.0 / f64::from(rounds);
    let started = std::time::Instant::now();
    crate::rail::go(0);
    let clicked = started.elapsed().as_secs_f64() * 1000.0;
    let page = view();
    let built = started.elapsed().as_secs_f64() * 1000.0;
    let nodes: usize = page.iter().map(Node::count).sum();
    println!("[measure] page at the newest: {newest:.1} ms; a click: {clicked:.1} ms to bring the prompt in, {built:.1} ms to the built page ({nodes} nodes)");
    assert!(scroll_to_of(page).is_some(), "the first page after the click scrolls to the prompt");
    chat::with(show_latest);
}

#[test]
fn the_header_has_three_groups_and_a_narrow_one_folds_search_and_remote_control_into_its_menu() {
    use wings_plugin::ui::tree_json;
    let (wide, tight) = chat::with(|c| {
        c.state = State::Running;
        c.session.id = Some("s1".into());
        c.session.working = false;
        c.side_in_wings = false;
        c.narrow = Some(false);
        let wide = tree_json(header(c)).to_string();
        c.narrow = Some(true);
        let tight = tree_json(header(c)).to_string();
        c.narrow = None;
        (wide, tight)
    });
    for (name, drawn) in [("wide", &wide), ("narrow", &tight)] {
        for row in ["Rename session", "Fork session", "Past sessions", "Chat settings"] {
            assert!(drawn.contains(&format!("\"text\":\"{row}\"")), "{name}: {row} is a row of the menu");
        }
        assert!(drawn.contains("\"label\":\"New chat\"") && drawn.contains("\"label\":\"Side panel\""), "{name}: new chat and the panel stay buttons");
        assert!(drawn.contains("\"label\":\"View mode\"") && drawn.contains("\"text\":\"Verbose\""), "{name}: the mode is a menu of three");
        assert_eq!(drawn.matches("\"shortcut\":\"ctrl-o\"").count(), 1, "{name}: Ctrl+O is on one row");
        for keys in ["cmd-shift-f", "cmd-y"] {
            assert_eq!(drawn.matches(&format!("\"shortcut\":\"{keys}\"")).count(), 1, "{name}: {keys} is on one control");
        }
        assert!(!drawn.contains("\"label\":\"Rename\"") && !drawn.contains("\"label\":\"Fork\""), "{name}: no icon buttons for them");
    }
    assert!(wide.contains("\"label\":\"Search\"") && wide.contains("\"label\":\"Remote Control\"") && !wide.contains("\"text\":\"Search\""));
    assert!(!tight.contains("\"label\":\"Search\"") && !tight.contains("\"label\":\"Remote Control\"") && tight.contains("\"text\":\"Search\"") && tight.contains("\"text\":\"Remote Control\""));
}
