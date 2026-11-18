//! What the chat page shows: the conversation in a reading column, newest at the bottom and kept there while you read
//! the end, and a floating message box with the model, effort and mode pickers in it. Your messages are bubbles on the
//! right; Claude's replies are plain text; its tool calls fold into one line per run of them; a question it asks is a
//! card with Allow and Deny. The project's past sessions open in place of the conversation.

use serde_json::Value;
use wings_plugin::{
    format::{ago, plural, truncate},
    host,
    markdown::{code_block, markdown},
    ui::{
        Node, badge, button, card, confirm, disclosure, empty, icon, list, list_button, loading, muted, notice, page, row,
        search_field, select, spinner, stack, text, toolbar_with,
    },
};

use crate::{
    chat::{self, Chat, PAGE, Past, State},
    session::{Ask, Item, MODES, Tool},
};

/// An arrow up, and a square, in a 14-point box.
const SEND: &str = "M7 12V2M2.5 6.5 7 2l4.5 4.5";
const STOP: &str = "M3.5 3.5h7v7h-7z";

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
    // Flush, so the conversation scrolls on its own and the message box stays at the foot.
    page(toolbar, [body, composer(c)]).prop("flush", true)
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
    } else if c.session.thinking || (c.session.working && !waiting(c)) {
        let what = if c.session.thinking { "Thinking" } else { "Working" };
        nodes.push(row().key("working").child(text(what).prop("tone", "muted").prop("shimmer", true)));
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
        Item::Text(said) | Item::User(said) | Item::Error(said) => truncate(said, 20_000),
        _ => "This is too long to show here.".to_owned(),
    };
    text(said).key(at.to_string())
}

/// Claude is waiting on you, or on a tool it started.
fn waiting(c: &Chat) -> bool {
    c.session.items.iter().any(|item| matches!(item, Item::Ask(Ask { answer: None, .. }) | Item::Tool(Tool { output: None, .. })))
}

fn item_node(at: usize, item: &Item, c: &Chat) -> Option<Node> {
    let node = match item {
        Item::User(said) => row().prop("justify", "end").child(card().child(text(said.as_str()))),
        Item::Text(said) => markdown(said),
        Item::Ask(ask) if ask.answer.is_none() => ask_node(ask),
        Item::Error(said) => notice("danger", said.as_str()),
        Item::Stopped => muted("Stopped"),
        Item::Note(title, body) => note(at, title, body, c.open_outputs.contains(&format!("note-{at}"))),
        Item::Tool(_) | Item::Ask(_) => return None,
    };
    Some(node.key(at.to_string()))
}

/// A run of tool calls as one line that says what they did, open while one runs or when you open it. Closed, its calls
/// aren't built at all.
fn steps(first: usize, tools: &[&Tool], c: &Chat) -> Node {
    let running = tools.iter().any(|tool| tool.output.is_none());
    let open = running || c.open_steps.contains(&first);
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
    let kinds: [(&[&str], &str, &str); 5] = [
        (&["Bash"], "Ran", "command"),
        (&["Read"], "Read", "file"),
        (&["Edit", "MultiEdit", "Write", "NotebookEdit"], "Edited", "file"),
        (&["Grep", "Glob"], "Searched", "time"),
        (&["WebFetch", "WebSearch"], "Looked up", "page"),
    ];
    let known = |tool: &&&Tool| kinds.iter().any(|(names, ..)| names.contains(&tool.name.as_str()));
    let mut parts: Vec<String> = kinds
        .iter()
        .filter_map(|(names, verb, noun)| {
            let n = tools.iter().filter(|tool| names.contains(&tool.name.as_str())).count();
            (n > 0).then(|| format!("{verb} {}", plural(n, noun)))
        })
        .collect();
    let others = tools.iter().filter(|tool| !known(tool)).count();
    if others > 0 {
        parts.push(format!("Used {}", plural(others, "tool")));
    }
    parts.join(" · ")
}

/// One tool call: what it is, and what it printed, folded. Its output is built only once you open it.
fn step(tool: &Tool, opened: bool) -> Node {
    let mark = match &tool.output {
        None => spinner(),
        Some((_, true)) => badge("Failed", Some("danger")),
        Some(_) => text("✓").prop("tone", "success"),
    };
    let head = row()
        .prop("gap", "s")
        .prop("align", "baseline")
        .child(mark)
        .child(text(tool.name.as_str()).prop("weight", "medium"))
        .child(text(tool.summary.as_str()).prop("mono", true).prop("size", "small").prop("tone", "muted").prop("truncate", true).prop("grow", true));
    match &tool.output {
        Some((output, failed)) if !output.trim().is_empty() => {
            let id = tool.id.clone();
            let body = if opened { code_block(output) } else { stack() };
            let fold = disclosure(if *failed { "Error" } else { "Output" }, body).prop("open", opened).on("onOpenChange", move |_| chat::toggle_output(&id));
            stack().prop("gap", "xs").child(head).child(fold)
        }
        _ => head,
    }
    .key(tool.id.as_str())
}

/// What Claude Code wrote in your name, as a quiet line on the left you can open, apart from your own messages.
fn note(at: usize, title: &str, body: &str, opened: bool) -> Node {
    let id = format!("note-{at}");
    let text_ = if opened { text(body).prop("tone", "muted").prop("size", "small") } else { stack() };
    let fold = disclosure(format!("⚙ {title}"), text_).prop("open", opened).on("onOpenChange", move |_| chat::toggle_output(&id));
    row().child(fold)
}

fn ask_node(ask: &Ask) -> Node {
    let (deny, allow) = (ask.request.clone(), ask.request.clone());
    confirm(
        format!("Allow {}?", ask.tool),
        [text(ask.summary.as_str()).prop("mono", true)],
        [button("Deny", "outline", false, move || chat::answer(&deny, false)), button("Allow", "default", false, move || chat::answer(&allow, true))],
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
        .on("onKeyDown", |event| {
            if event["key"] == "Enter" {
                chat::send();
            }
        });
    // A round button with an arrow, or a square to stop, as chat apps have.
    let go = if c.session.working {
        button("", "secondary", false, chat::stop).prop("size", "icon-sm").prop("label", "Stop").prop("title", "Stop").child(icon(14, 2.0, &[STOP]))
    } else {
        button("", "default", false, chat::send)
            .prop("size", "icon-sm")
            .prop("label", "Send")
            .prop("title", "Send (Enter)")
            .prop("disabled", !open || c.draft.trim().is_empty())
            .child(icon(14, 2.0, &[SEND]))
    };
    let context = c.session.context.map(|(used, most)| {
        text(format!("{}%", (used * 100 / most.max(1)).min(100))).prop("tone", "muted").prop("size", "small").prop("title", "How full the context window is")
    });
    let controls = row().prop("gap", "xs").prop("align", "center").children(pickers(c)).child(stack().prop("grow", true)).maybe(context).child(go);
    let message_box = card().prop("variant", "raised").child(field.prop("grow", true)).child(controls);
    Node::new("Bar").prop("edge", "none").child(stack().prop("gap", "s").prop("maxWidth", COLUMN).maybe(commands(c).or_else(|| file_menu(c))).child(message_box))
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
        let options: Vec<(String, String)> = model.efforts.iter().map(|effort| (effort.clone(), capital(effort))).collect();
        let value = session.effort.clone().unwrap_or_else(|| "high".into());
        pickers.push(select("Effort", &options, &value, chat::pick_effort).prop("variant", "ghost"));
    }
    if let Some(mode) = &session.mode {
        let options: Vec<(String, String)> = MODES.iter().map(|(value, name)| (value.to_string(), name.to_string())).collect();
        pickers.push(select("Permission mode", &options, mode, chat::pick_mode).prop("variant", "ghost"));
    }
    pickers
}

/// The slash commands that match what you typed, over the message box.
fn commands(c: &Chat) -> Option<Node> {
    let matches = chat::command_matches(c);
    if matches.is_empty() {
        return None;
    }
    let rows = matches.into_iter().map(|command| {
        let name = command.name.clone();
        let line = row()
            .prop("gap", "s")
            .prop("align", "baseline")
            .child(text(format!("/{}", command.name)).prop("mono", true).prop("weight", "medium"))
            .child(text(truncate(&command.description, 100)).prop("tone", "muted").prop("size", "small").prop("truncate", true).prop("grow", true));
        list_button([line], move || chat::insert_command(&name)).key(command.name.as_str())
    });
    Some(card().child(list().children(rows)))
}

/// The files that match the `@word` you typed, over the message box.
fn file_menu(c: &Chat) -> Option<Node> {
    let (_, found) = chat::mention_matches(c)?;
    let rows = found.into_iter().map(|path| {
        let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
        let line = row()
            .prop("gap", "s")
            .prop("align", "baseline")
            .child(text(name).prop("mono", true).prop("weight", "medium"))
            .child(text(dir).prop("tone", "muted").prop("size", "small").prop("truncate", true).prop("grow", true));
        let picked = path.to_owned();
        list_button([line], move || chat::insert_mention(&picked)).key(path)
    });
    Some(card().child(list().children(rows)))
}

fn capital(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}
