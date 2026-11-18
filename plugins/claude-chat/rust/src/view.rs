//! What the chat page shows: the conversation in a reading column, newest at the bottom and kept there while you read
//! the end, and a floating message box with the model, effort and mode pickers in it. Your messages are bubbles on the
//! right; Claude's replies are plain text; its tool calls fold into one line per run of them; a question it asks is a
//! card: Allow, Always allow and Deny with a reason; its questions with options to pick; its plan to approve. The project's past sessions open in place of the conversation.

use serde_json::Value;
use wings_plugin::{
    format::{ago, clock, parse_time, plural, truncate},
    local_time::local,
    host,
    markdown::{code_block, markdown},
    ui::{
        Node, badge, button, card, chip, confirm, disclosure, empty, gauge, icon, list, list_button, loading, muted, notice, page,
        row, search_field, select, spinner, stack, text, text_input, toolbar_with,
    },
};

use crate::{
    chat::{self, Chat, PAGE, Past, State},
    session::{Ask, Detail, Diff, Draft, Item, MODES, Progress, Todo, Tool, exit_code, files_found},
};

/// An arrow up, a square and a cross, in a 14-point box.
const SEND: &str = "M7 12V2M2.5 6.5 7 2l4.5 4.5";
const STOP: &str = "M3.5 3.5h7v7h-7z";
const CLOSE: &str = "M3.5 3.5l7 7M10.5 3.5l-7 7";

/// The conversation's column, as wide as a page meant for reading.
const COLUMN: u32 = 760;
/// The most nodes the conversation sends. The app draws a page of at most 5,000, and a long session's whole history
/// made more: the page was refused and stayed on Loading. The toolbar and the message box take the rest.
const BUDGET: usize = 3_500;

pub fn view() -> Vec<Node> {
    chat::with(|c| vec![chat_page(c)])
}

fn chat_page(c: &Chat) -> Node {
    let facts = row().prop("gap", "s").child(muted(c.folder.as_str()));
    let sessions = button("Sessions", if c.past.is_some() { "secondary" } else { "ghost" }, false, chat::toggle_past);
    let terminal = (c.session.id.is_some() && c.state != State::Moved).then(|| button("Open in terminal", "ghost", false, chat::to_terminal));
    let actions = row().prop("gap", "s").maybe(terminal).child(sessions).child(button("New chat", "outline", false, chat::new_chat));
    let toolbar = toolbar_with("Claude", facts, Some(actions));
    let body = match &c.past {
        Some(past) => past_sessions(past),
        None => conversation(c),
    };
    // Flush, so the conversation scrolls on its own and the checklist and the message box stay at the foot.
    page(toolbar, [body].into_iter().chain(todo_card(c)).chain([composer(c)])).prop("flush", true)
}

fn column() -> Node {
    stack().prop("gap", "l").prop("maxWidth", COLUMN)
}

/// The conversation, or what shows before it has any.
fn conversation(c: &Chat) -> Node {
    if c.session.items.is_empty() && c.session.live.is_empty() {
        let body = match &c.state {
            State::Starting => loading(),
            State::Failed(why) => notice("danger", why.as_str()),
            State::Moved => muted("This session goes on in a terminal."),
            _ => empty("What should Claude do?", "It works in this folder with your Claude Code setup, and asks here before it does anything you haven't allowed."),
        };
        return Node::new("Scroll").child(column().child(body));
    }
    let items = &c.session.items;
    let from = items.len().saturating_sub(c.shown);
    // Built from the newest back until the budget is spent, so a long session's oldest items stay out of the page.
    let (mut blocks, mut spent, mut cut) = (Vec::new(), 0, false);
    let mut end = items.len();
    while end > from {
        let mut start = end - 1;
        while folds(&items[start]) && start > from && folds(&items[start - 1]) {
            start -= 1;
        }
        let block = match folds(&items[start]) {
            true => {
                let tools: Vec<&Tool> = items[start..end].iter().filter_map(|item| if let Item::Tool(tool) = item { Some(tool) } else { None }).collect();
                (!tools.is_empty()).then(|| steps(start, &tools, c))
            }
            false => item_node(start, &items[start], c),
        };
        end = start;
        let Some(block) = block else { continue };
        let size = block.count();
        if spent + size > BUDGET {
            cut = true;
            // The newest item always shows, as plain text when it is too big to lay out.
            if blocks.is_empty() {
                blocks.push(plain(start, &items[start]));
            }
            break;
        }
        spent += size;
        blocks.push(block);
    }
    blocks.reverse();
    let earlier = if cut {
        Some(muted("Older messages aren't shown here. Open the session in a terminal to see all of it.").key("earlier"))
    } else {
        (from > 0).then(|| row().key("earlier").child(button(format!("Show earlier ({from})"), "ghost", false, || chat::with(|c| c.shown += PAGE))))
    };
    let mut nodes: Vec<Node> = earlier.into_iter().chain(blocks).collect();
    if !c.session.live.is_empty() {
        let live = markdown(&c.session.live);
        let live = if live.count() > BUDGET / 4 { text(truncate(&c.session.live, 20_000)) } else { live };
        nodes.push(live.key("live"));
    } else if c.session.thinking || (c.session.working && !asking(c)) {
        // The app adds how long it has been and counts it up: "Calling wings · 16s".
        let what = if c.doing.0.is_empty() { "Working" } else { c.doing.0.as_str() };
        nodes.push(row().key("working").child(text(what).prop("tone", "muted").prop("shimmer", true).prop("since", c.doing.1)));
    }
    match &c.state {
        State::Failed(why) => nodes.push(notice("danger", why.as_str()).key("ended")),
        State::Ended => nodes.push(muted("Claude Code ended. Start a new chat, or pick a past session, to go on.").key("ended")),
        State::Moved => nodes.push(muted("This session goes on in a terminal.").key("ended")),
        _ => {}
    }
    Node::new("Scroll").prop("follow", "end").child(column().prop("enter", "rise").children(nodes))
}

/// An item that folds into the line of the tool calls around it: a call, or a question you answered about one.
fn folds(item: &Item) -> bool {
    matches!(item, Item::Tool(_) | Item::Ask(Ask { answer: Some(_), .. }))
}

/// An item too big to lay out, as one plain text.
fn plain(at: usize, item: &Item) -> Node {
    let said = match item {
        Item::Text(said) | Item::User(said) | Item::UserImages(said, _) | Item::Error(said) => truncate(said, 20_000),
        _ => "This is too long to show here.".to_owned(),
    };
    text(said).key(at.to_string())
}

/// Claude is waiting on you, or on a tool it started.
/// A card waits for your answer, so the status line would only repeat it.
fn asking(c: &Chat) -> bool {
    c.session.items.iter().any(|item| matches!(item, Item::Ask(Ask { answer: None, .. })))
}

/// Your message on the right, as chat apps show it, and never wider than the column less a margin on its left: a long one
/// spread over the whole width read like Claude's.
fn bubble(card: Node) -> Node {
    row().prop("justify", "end").child(stack().prop("width", 64)).child(card)
}

fn item_node(at: usize, item: &Item, c: &Chat) -> Option<Node> {
    let node = match item {
        Item::User(said) => bubble(card().child(text(said.as_str()))),
        Item::UserImages(said, images) => {
            let card = card().child(muted(plural(*images, "image")));
            bubble(if said.is_empty() { card } else { card.child(text(said.as_str())) })
        }
        Item::Text(said) => markdown(said),
        Item::Ask(ask) if ask.answer.is_none() => ask_node(ask, c),
        Item::Error(said) => notice("danger", said.as_str()),
        Item::Stopped => muted("Stopped"),
        Item::Note(title, body) => note(at, title, body, c.open_outputs.contains(&format!("note-{at}"))),
        Item::Tool(_) | Item::Ask(_) => return None,
    };
    Some(node.key(at.to_string()))
}

/// A run of tool calls as one line that says what they did, open only when you open it: one that opened while a call ran
/// and shut once it ended showed the calls for a moment and hid them again. The line under the conversation says what
/// runs. Closed, its calls aren't built at all.
fn steps(first: usize, tools: &[&Tool], c: &Chat) -> Node {
    let open = c.open_steps.contains(&first);
    let failed = tools.iter().filter(|tool| matches!(tool.output, Some((_, true)))).count();
    let mut said = describe(tools);
    if failed > 0 {
        said.push_str(&format!(" · {failed} failed"));
    }
    let rows = stack().prop("gap", "s");
    let rows = if open { rows.children(tools.iter().map(|tool| step(tool, c.open_outputs.contains(&tool.id)))) } else { rows };
    disclosure(said, rows).prop("open", open).on("onOpenChange", move |_| chat::toggle_steps(first)).key(format!("steps-{first}"))
}

/// What a run of tool calls did, by kind: "Ran 2 commands · Read 3 files".
fn describe(tools: &[&Tool]) -> String {
    let kinds: [(&[&str], &str, &str); 8] = [
        (&["Bash"], "Ran", "command"),
        (&["Read"], "Read", "file"),
        (&["Edit", "MultiEdit", "Write", "NotebookEdit"], "Edited", "file"),
        (&["Grep", "Glob"], "Searched", "time"),
        (&["WebFetch", "WebSearch"], "Looked up", "page"),
        (&["Task", "Agent"], "Ran", "subagent"),
        (&["Skill"], "Loaded", "skill"),
        (&["TodoWrite", "TaskCreate", "TaskUpdate"], "Updated the checklist", ""),
    ];
    let known = |tool: &&&Tool| kinds.iter().any(|(names, ..)| names.contains(&tool.name.as_str()));
    let mut parts: Vec<String> = kinds
        .iter()
        .filter_map(|(names, verb, noun)| {
            let n = tools.iter().filter(|tool| names.contains(&tool.name.as_str())).count();
            // The checklist shows above the message box: how often it changed says nothing.
            (n > 0).then(|| if noun.is_empty() { (*verb).to_owned() } else { format!("{verb} {}", plural(n, noun)) })
        })
        .collect();
    let others = tools.iter().filter(|tool| !known(tool)).count();
    if others > 0 {
        parts.push(format!("Used {}", plural(others, "tool")));
    }
    parts.join(" · ")
}

/// One tool call: what it is, and what it did or printed, folded. The fold is built only once you open it.
fn step(tool: &Tool, opened: bool) -> Node {
    let output = tool.output.as_ref().map(|(output, failed)| (output.as_str(), *failed)).filter(|(output, _)| !output.trim().is_empty());
    let mark = match &tool.output {
        None => spinner(),
        Some((output, true)) => badge(exit_code(output).map_or("Failed".to_owned(), |code| format!("Failed · exit {code}")), Some("danger")),
        Some(_) => text("✓").prop("tone", "success"),
    };
    let head = row()
        .prop("gap", "s")
        .prop("align", "baseline")
        .child(mark)
        .child(text(tool.name.as_str()).prop("weight", "medium"))
        .child(text(tool.summary.as_str()).prop("mono", true).prop("size", "small").prop("tone", "muted").prop("truncate", true).prop("grow", true))
        .children(facts(tool));
    // What an edit or a write printed is only "updated successfully", which the diff already says.
    let label = match (&tool.detail, output) {
        (_, Some((_, true))) => Some("Error"),
        // The checklist over the message box shows what these did; they print only that it changed.
        _ if matches!(tool.name.as_str(), "TodoWrite" | "TaskCreate" | "TaskUpdate") => None,
        (Detail::Diff(diff), _) if !diff.lines.is_empty() => Some("Changes"),
        (Detail::Write { head, .. }, _) if !head.is_empty() => Some("Content"),
        (Detail::Read(_), _) => None,
        (_, Some(_)) => Some("Output"),
        (Detail::Command(command), None) if command.contains('\n') => Some("Command"),
        _ => None,
    };
    // A failed call shows the end of its error under its line, closed or not: that is what you open it for.
    let error = output.filter(|(_, failed)| *failed && !opened).map(|(said, _)| {
        let lines: Vec<&str> = said.trim_end().lines().collect();
        let tail = lines[lines.len().saturating_sub(4)..].join("\n");
        text(truncate(&tail, 600)).prop("mono", true).prop("size", "small").prop("tone", "danger")
    });
    match label {
        Some(label) => {
            let id = tool.id.clone();
            let body = if opened { body(tool, output) } else { stack() };
            let label = if error.is_some() { "Whole error" } else { label };
            let fold = disclosure(label, body).prop("open", opened).on("onOpenChange", move |_| chat::toggle_output(&id));
            stack().prop("gap", "xs").child(head).maybe(error).child(fold)
        }
        None => head,
    }
    .key(tool.id.as_str())
}

/// A small mono text on a call's head line.
fn fact(said: impl Into<String>, tone: &str) -> Node {
    text(said).prop("mono", true).prop("size", "small").prop("tone", tone)
}

/// What a call's head line says after its summary: the lines an edit changes, the lines written, which lines were read,
/// how many files a search found.
fn facts(tool: &Tool) -> Vec<Node> {
    match &tool.detail {
        Detail::Diff(Diff { added, removed, .. }) => vec![fact(format!("+{added}"), "success"), fact(format!("−{removed}"), "danger")],
        Detail::Write { lines, .. } => vec![fact(format!("+{}", plural(*lines, "line")), "success")],
        Detail::Read(Some(range)) => vec![fact(range.as_str(), "muted")],
        Detail::Search => {
            let found = tool.output.as_ref().and_then(|(output, _)| files_found(&tool.name, output));
            found.map(|n| fact(plural(n, "file"), "muted")).into_iter().collect()
        }
        _ => Vec::new(),
    }
}

/// What a call's fold holds: the diff, the start of the file, a command of several lines, and what it printed.
fn body(tool: &Tool, output: Option<(&str, bool)>) -> Node {
    let failed = output.is_some_and(|(_, failed)| failed);
    let mut body = stack().prop("gap", "s");
    match &tool.detail {
        Detail::Diff(diff) if !failed => body = body.child(diff_lines(diff)),
        Detail::Write { lines, head } if !failed => {
            body = body.child(code_block(&head.join("\n")));
            if *lines > head.len() {
                body = body.child(muted(format!("{} more", plural(lines - head.len(), "line"))));
            }
        }
        Detail::Command(command) if command.contains('\n') => body = body.child(code_block(command)),
        _ => {}
    }
    match output {
        Some((output, _)) if failed || !matches!(tool.detail, Detail::Diff(_) | Detail::Write { .. }) => body.child(code_block(output)),
        _ => body,
    }
}

/// An edit's lines: each taken out in red with a minus, each put in in green with a plus.
fn diff_lines(diff: &Diff) -> Node {
    let rows = diff.lines.iter().map(|(added, line)| {
        let (mark, tone) = if *added { ("+", "success") } else { ("−", "danger") };
        text(format!("{mark} {line}")).prop("mono", true).prop("size", "small").prop("tone", tone)
    });
    let more = (diff.more > 0).then(|| muted(format!("{} more", plural(diff.more, "line"))));
    card().child(stack().prop("gap", "none").children(rows).maybe(more))
}

/// The most checklist lines shown at once.
const TODOS_SHOWN: usize = 12;

/// Claude's checklist, a raised card over the message box: how many are done, folded to that line when you fold it.
fn todo_card(c: &Chat) -> Option<Node> {
    let todos = &c.session.todos;
    if todos.is_empty() {
        return None;
    }
    let done = todos.iter().filter(|todo| todo.progress == Progress::Done).count();
    let folded = c.todos_folded;
    let working = c.session.working;
    let active = todos.iter().find(|todo| todo.progress == Progress::Active);
    // Its count on the left, what is under way beside it once folded, and Hide or Show on the right.
    let mut head = row().prop("gap", "s").prop("align", "center").child(text(format!("{done} of {} done", todos.len())).prop("weight", "medium"));
    let doing = active.filter(|_| folded).map(|todo| text(todo.active.as_str()).prop("tone", "muted").prop("size", "small").prop("truncate", true).prop("shimmer", working));
    head = head.child(stack().prop("grow", true).maybe(doing));
    head = head.child(button(if folded { "Show" } else { "Hide" }, "ghost", false, chat::toggle_todos).prop("size", "sm"));
    let mut list = card().prop("variant", "raised").child(head);
    if !folded {
        // A long list shows what is left first; the header still counts what is done.
        let shown: Vec<&Todo> = if todos.len() > TODOS_SHOWN { todos.iter().filter(|todo| todo.progress != Progress::Done).collect() } else { todos.iter().collect() };
        let rows = shown.iter().take(TODOS_SHOWN).map(|todo| todo_line(todo, working));
        let more = (shown.len() > TODOS_SHOWN).then(|| muted(format!("{} more", plural(shown.len() - TODOS_SHOWN, "item"))));
        list = list.child(stack().prop("gap", "none").children(rows).maybe(more));
    }
    Some(Node::new("Bar").prop("edge", "none").child(stack().prop("gap", "s").prop("maxWidth", COLUMN).child(list)).key("todos"))
}

/// One checklist line: done is muted with a check, the one in progress shimmers while Claude works.
fn todo_line(todo: &Todo, working: bool) -> Node {
    let line = row().prop("gap", "s").prop("align", "baseline").key(todo.id.as_str());
    match todo.progress {
        Progress::Done => line.child(text("✓").prop("tone", "success").prop("size", "small")).child(muted(todo.text.as_str()).prop("size", "small")),
        Progress::Active => line.child(text("●").prop("tone", "info").prop("size", "small")).child(text(todo.active.as_str()).prop("size", "small").prop("weight", "medium").prop("shimmer", working)),
        Progress::Pending => line.child(muted("○").prop("size", "small")).child(text(todo.text.as_str()).prop("size", "small")),
    }
}

/// What Claude Code wrote in your name, as a quiet line on the left you can open, apart from your own messages.
fn note(at: usize, title: &str, body: &str, opened: bool) -> Node {
    let id = format!("note-{at}");
    let text_ = if opened { text(body).prop("tone", "muted").prop("size", "small") } else { stack() };
    let fold = disclosure(format!("⚙ {title}"), text_).prop("open", opened).on("onOpenChange", move |_| chat::toggle_output(&id));
    row().child(fold)
}

/// The card for a question of Claude's: a tool to allow, questions to answer, or a plan to approve.
fn ask_node(ask: &Ask, c: &Chat) -> Node {
    let none = Draft::default();
    let draft = c.drafts.get(&ask.request).unwrap_or(&none);
    match ask.tool.as_str() {
        "AskUserQuestion" if ask.input["questions"].as_array().is_some_and(|questions| !questions.is_empty()) => questions_node(ask, draft),
        "ExitPlanMode" if ask.input["plan"].as_str().is_some() => plan_node(ask, draft),
        _ => permission_node(ask, draft),
    }
}

/// The text field for what Claude should hear along with a no.
fn feedback_field(request: &str, draft: &Draft, label: &str, placeholder: &str) -> Node {
    let request = request.to_owned();
    text_input(label, draft.feedback.as_str(), move |said| chat::edit(&request, |draft| draft.feedback = said)).prop("placeholder", placeholder)
}

fn permission_node(ask: &Ask, draft: &Draft) -> Node {
    let (deny, once, always) = (ask.request.clone(), ask.request.clone(), ask.request.clone());
    let mut body: Vec<Node> = ask.reason.iter().map(|why| muted(why.as_str())).collect();
    body.push(text(ask.detail.as_str()).prop("mono", true).prop("size", "small"));
    if ask.cut_short {
        body.push(text("This is too long to show here in full, so it can't be allowed here. Open the session in the terminal to read it.").prop("tone", "warn"));
    }
    body.push(feedback_field(&ask.request, draft, "Reason for denying", "Why not, or what to do instead (Claude reads it)"));
    let always = (!ask.suggestions.is_empty()).then(|| button("Always allow", "secondary", false, move || chat::allow(&always, true)).prop("disabled", ask.cut_short));
    confirm(
        format!("Allow {}?", ask.tool),
        body,
        [button("Deny", "outline", false, move || chat::deny(&deny)), button("Allow once", "default", false, move || chat::allow(&once, false)).prop("disabled", ask.cut_short)]
            .into_iter()
            .chain(always),
    )
}

/// Each question with its options as chips, and a field for another answer.
fn questions_node(ask: &Ask, draft: &Draft) -> Node {
    let questions = ask.input["questions"].as_array().map(Vec::as_slice).unwrap_or_default();
    let mut body = Vec::new();
    for (q, question) in questions.iter().enumerate() {
        let multi = question["multiSelect"] == true;
        let options = question["options"].as_array().map(Vec::as_slice).unwrap_or_default();
        // One row an option, as Claude Code's own menu lists them, marked when picked: a round mark for one answer, a box
        // for several.
        let chips = list().children(options.iter().enumerate().map(|(o, option)| {
            let request = ask.request.clone();
            let picked = draft.is_picked(q, o);
            let mark = match (multi, picked) {
                (false, true) => "●",
                (false, false) => "○",
                (true, true) => "☑",
                (true, false) => "☐",
            };
            let line = row()
                .prop("gap", "s")
                .prop("align", "baseline")
                .child(text(mark).prop("tone", if picked { "info" } else { "muted" }))
                .child(text(option["label"].as_str().unwrap_or_default()).prop("weight", "medium"))
                .maybe(option["description"].as_str().filter(|said| !said.is_empty()).map(|said| muted(truncate(said, 120)).prop("size", "small").prop("grow", true)));
            list_button([line], move || chat::edit(&request, |draft| draft.toggle(q, o, multi))).key(format!("{q}-{o}"))
        }));
        let request = ask.request.clone();
        let other = text_input("Other", draft.other(q), move |said| chat::edit(&request, |draft| draft.set_other(q, said, multi)))
            .prop("placeholder", "Something else")
            .key(format!("other-{q}"));
        let header = question["header"].as_str().filter(|header| !header.is_empty()).map(|header| badge(header, None));
        body.push(
            stack()
                .prop("gap", "xs")
                .key(format!("q-{q}"))
                .child(row().prop("gap", "s").prop("align", "baseline").maybe(header).child(text(question["question"].as_str().unwrap_or_default()).prop("weight", "medium")))
                .child(chips)
                .child(other),
        );
    }
    let ready = draft.answers(&ask.input["questions"]).is_some();
    let (skip, submit) = (ask.request.clone(), ask.request.clone());
    confirm(
        "Claude has a question",
        body,
        [button("Skip", "outline", false, move || chat::skip(&skip)), button("Submit", "default", false, move || chat::submit(&submit)).prop("disabled", !ready)],
    )
}

/// The plan as Markdown, to approve, or to send back with what to change.
fn plan_node(ask: &Ask, draft: &Draft) -> Node {
    let plan = ask.input["plan"].as_str().unwrap_or_default();
    let shown = markdown(plan);
    let shown = if shown.count() > BUDGET / 4 { text(truncate(plan, 20_000)) } else { shown };
    let (auto, approve, keep) = (ask.request.clone(), ask.request.clone(), ask.request.clone());
    let actions = row()
        .prop("gap", "s")
        .prop("wrap", true)
        .prop("justify", "end")
        .child(button("Keep planning", "outline", false, move || chat::keep_planning(&keep)))
        .child(button("Approve", "secondary", false, move || chat::approve_plan(&approve, "default")))
        .child(button("Approve and auto-accept edits", "default", false, move || chat::approve_plan(&auto, "acceptEdits")));
    card().prop("variant", "raised").child(
        stack()
            .prop("gap", "m")
            .child(text("Claude's plan").prop("weight", "strong"))
            .child(shown)
            .child(feedback_field(&ask.request, draft, "What to change", "What to change in the plan, to keep planning"))
            .child(actions),
    )
}

/// The project's past sessions, newest first, with a search box.
fn past_sessions(past: &Past) -> Node {
    let search = search_field("Search past sessions", past.query.as_str(), |query| chat::load_past(query.to_owned()));
    let body = match (&past.sessions, &past.error) {
        (_, Some(error)) => notice("danger", error.as_str()),
        (None, None) => loading(),
        (Some(sessions), None) if sessions.is_empty() => empty("No past sessions", "Claude Code sessions in this project show here."),
        (Some(sessions), None) => list().children(sessions.iter().map(past_row)),
    };
    Node::new("Scroll").child(column().prop("gap", "m").child(search).child(body))
}

fn past_row(session: &Value) -> Node {
    let id = session["id"].as_str().unwrap_or_default().to_owned();
    let title = session["title"].as_str().filter(|title| !title.is_empty()).or(session["firstPrompt"].as_str()).unwrap_or("Untitled session");
    let mut facts = vec![ago(session["lastActiveMs"].as_f64().unwrap_or(0.0), host::now())];
    facts.extend(session["gitBranch"].as_str().map(str::to_owned));
    facts.push(plural(session["messages"].as_u64().unwrap_or(0) as usize, "message"));
    let open = id.clone();
    list_button([text(truncate(title, 120)).prop("weight", "medium").prop("truncate", true), muted(facts.join(" · "))], move || chat::resume(&open))
        .prop("layout", "column")
        .key(id)
}

/// The message box, floating at the foot: what you type, then the pickers, how full the context is, and Send or Stop.
fn composer(c: &Chat) -> Node {
    let open = c.state == State::Running;
    let field = Node::new("Textarea")
        .prop("label", "Message")
        .prop("placeholder", "Ask Claude to do something, or type / for commands")
        .prop("variant", "bare")
        .prop("rows", 2)
        .prop("submitOnEnter", true)
        .prop("autoFocus", true)
        .prop("value", c.draft.as_str())
        .prop("disabled", !open)
        .on("onChange", |event| chat::with(|c| c.draft = event["value"].as_str().unwrap_or_default().to_owned()))
        .on("onPasteImage", |event| chat::add_image(event["mediaType"].as_str().unwrap_or_default(), event["data"].as_str().unwrap_or_default()))
        .on("onKeyDown", |event| {
            if event["key"] == "Enter" {
                chat::send();
            }
        });
    // One button, as chat apps have: a square to stop while Claude works and nothing is typed, else an arrow to send.
    // What you send while Claude works waits in the queue.
    let typed = !c.draft.trim().is_empty() || !c.images.is_empty();
    let action = if c.session.working && !typed {
        button("", "default", false, chat::stop).prop("size", "icon-sm").prop("label", "Stop").prop("title", "Stop").child(icon(14, 2.0, &[STOP]))
    } else {
        button("", "default", false, chat::send)
            .prop("size", "icon-sm")
            .prop("label", if c.session.working { "Queue" } else { "Send" })
            .prop("title", if c.session.working { "Claude reads it after its current step (Enter)" } else { "Send (Enter)" })
            .prop("disabled", !open || !typed)
            .child(icon(14, 2.0, &[SEND]))
    };
    let controls = row().prop("gap", "xs").prop("align", "center").children(pickers(c)).child(stack().prop("grow", true)).child(action);
    // The field with the slash commands or files that match what you typed, which the app shows as a list over the page
    // that the arrows move through and Enter picks from, as a combobox's. Keyed, so the field stays the same one as the
    // list comes and goes, and keeps the keyboard.
    let field = stack().prop("gap", "s").key("message").child(field.prop("grow", true).key("field")).maybe(suggestions(c));
    let message_box = card().prop("variant", "raised").maybe(pasted(c)).child(field).child(controls);
    Node::new("Bar").prop("edge", "none").child(
        stack()
            .prop("gap", "s")
            .prop("maxWidth", COLUMN)
            .maybe(queued(c))
            .child(message_box)
            .maybe(usage(c)),
    )
}

/// The pictures pasted into the message, as chips above its text, each with a cross to take it out.
fn pasted(c: &Chat) -> Option<Node> {
    if c.images.is_empty() {
        return None;
    }
    // Each picture is one chip with its × inside, which takes it back: a × beside a badge read as a button of its own.
    let chips = (0..c.images.len()).map(|at| {
        button("", "outline", false, move || chat::remove_image(at))
            .prop("size", "sm")
            .prop("label", format!("Remove image {}", at + 1))
            .prop("title", "Remove")
            .child(text(format!("Image {}", at + 1)))
            .child(icon(12, 2.0, &[CLOSE]))
            .key(at.to_string())
    });
    Some(row().prop("gap", "s").prop("wrap", true).children(chips))
}

/// How full the context is and how much of your plan's 5-hour and weekly limits is used, as bars under the message box,
/// each with when it resets: what a terminal's status line shows.
fn usage(c: &Chat) -> Option<Node> {
    let context = c.session.context.map(|(used, most)| gauge("Context", (used * 100 / most.max(1)) as f64, (70, 90), None));
    let limits = c.session.limits.iter().map(|(label, used, resets)| {
        let name = if *label == "5h" { "5-hour limit" } else { "Weekly limit" };
        let resets = parse_time(resets).map(|ms| match *label {
            "5h" => format!("Resets {}", clock(ms)),
            _ => format!("Resets {} {}", WEEKDAYS[local(ms as i64).weekday as usize % 7], clock(ms)),
        });
        gauge(name, *used as f64, (80, 95), resets)
    });
    let bars: Vec<Node> = context.into_iter().chain(limits).map(|bar| stack().prop("grow", true).child(bar)).collect();
    (!bars.is_empty()).then(|| row().prop("gap", "l").prop("align", "start").children(bars))
}

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// A queued message longer than this gets a More button.
const QUEUED_LINE: usize = 60;

/// The messages waiting for Claude to finish, each with Send now and a way to take it back.
fn queued(c: &Chat) -> Option<Node> {
    if c.session.queued.is_empty() {
        return None;
    }
    let rows = c.session.queued.iter().map(|queued| {
        let (now, remove, fold) = (queued.uuid.clone(), queued.uuid.clone(), queued.uuid.clone());
        let open = c.open_queued.contains(&queued.uuid);
        // Nothing here measures the text, so a message counts as long once it can't fit a line of the narrow card.
        let long = queued.text.chars().count() > QUEUED_LINE || queued.text.contains('\n');
        // The text takes the room the buttons leave and is cut short in it, so a long message keeps them in the card;
        // opened, it wraps whole and the buttons stay by its first line.
        let said = if queued.text.is_empty() {
            muted(plural(queued.images.len(), "image"))
        } else if open {
            text(queued.text.as_str())
        } else {
            text(queued.text.as_str()).prop("truncate", true).prop("title", queued.text.as_str())
        };
        row()
            .prop("gap", "xs")
            .prop("align", if open { "start" } else { "center" })
            .child(stack().prop("grow", true).child(said))
            .maybe(long.then(|| button(if open { "Less" } else { "More" }, "ghost", false, move || chat::toggle_queued(&fold)).prop("size", "sm")))
            .child(button("Send now", "ghost", false, move || chat::send_now(&now)).prop("size", "sm").prop("title", "Claude puts what it runs aside and reads it now"))
            .child(
                button("", "ghost", false, move || chat::unqueue(&remove))
                    .prop("size", "icon-sm")
                    .prop("label", "Remove")
                    .prop("title", "Remove")
                    .child(icon(14, 2.0, &[CLOSE])),
            )
            .key(queued.uuid.as_str())
    });
    Some(card().child(muted("Claude reads these after its current step")).child(stack().prop("gap", "xs").children(rows)))
}

/// The model, effort and permission mode, each a menu, once Claude Code has said what it has.
fn pickers(c: &Chat) -> Vec<Node> {
    let session = &c.session;
    let mut pickers = Vec::new();
    if !session.models.is_empty() {
        let options: Vec<(String, String)> = session.models.iter().map(|model| (model.value.clone(), model.name.clone())).collect();
        let value = session.current_model().map_or("default".to_owned(), |model| model.value.clone());
        pickers.push(select("Model", &options, &value, chat::pick_model).prop("variant", "ghost"));
    }
    if let Some(model) = session.current_model().filter(|model| !model.efforts.is_empty()) {
        let options: Vec<(String, String)> = model.efforts.iter().map(|effort| (effort.clone(), if effort == "xhigh" { "Extra high".into() } else { capital(effort) })).collect();
        let value = session.current_effort().unwrap_or_else(|| "high".into());
        pickers.push(select("Effort", &options, &value, chat::pick_effort).prop("variant", "ghost"));
    }
    if let Some(mode) = &session.mode {
        let options: Vec<(String, String)> = MODES.iter().map(|(value, name)| (value.to_string(), name.to_string())).collect();
        pickers.push(select("Permission mode", &options, mode, chat::pick_mode).prop("variant", "ghost"));
    }
    pickers
}

/// What the message box suggests for what you typed: the slash commands that match a `/`, else the files that match an
/// `@word`, as a combobox's options, each a name with what it is beside it.
fn suggestions(c: &Chat) -> Option<Node> {
    let option = |key: &str, label: String, detail: &str, pick: Box<dyn Fn()>| {
        let chip = chip(label, false, move || pick()).key(key);
        if detail.is_empty() { chip } else { chip.child(muted(truncate(detail, 100)).prop("size", "small")) }
    };
    let commands = chat::command_matches(c);
    let options: Vec<Node> = if !commands.is_empty() {
        commands
            .into_iter()
            .map(|command| {
                let name = command.name.clone();
                option(&command.name, format!("/{}", command.name), &command.description, Box::new(move || chat::insert_command(&name)))
            })
            .collect()
    } else {
        let (_, found) = chat::mention_matches(c)?;
        found
            .into_iter()
            .map(|path| {
                let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
                let picked = path.to_owned();
                option(path, name.to_owned(), dir, Box::new(move || chat::insert_mention(&picked)))
            })
            .collect()
    };
    (!options.is_empty()).then(|| row().prop("gap", "s").prop("wrap", true).prop("align", "center").children(options))
}

fn capital(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}
