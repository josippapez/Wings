//! What the chat page shows: the conversation in a reading column, newest at the bottom and kept there while you read
//! the end, and a floating message box with the model, effort and mode pickers in it. Your messages are bubbles on the
//! right; Claude's replies are plain text; its tool calls fold into one line per run of them; a question it asks is a
//! card: Allow, Always allow and Deny with a reason; its questions with options to pick; its plan to approve. A header
//! like a terminal pane's names the session and holds the chat's actions, and the project's past sessions open over it
//! all in a sheet (`past_view.rs`).

use wings_plugin::{
    format::{clock, plural, truncate},
    local_time::local,
    markdown::{code_block, markdown},
    ui::{
        Node, badge, button, card, chip, confirm, disclosure, empty, icon, list, list_button, loading, muted, notice, row,
        select, spinner, stack, text, text_input,
    },
};

use crate::{
    chat::{self, Chat, PAGE, State},
    pictures_view::thumbs,
    prefs,
    session::{Ask, Detail, Diff, Draft, Finished, Item, MODES, Progress, Session, Task, Todo, Tool, exit_code, files_found, first_lines, model_name},
    usage::Panel,
};

/// An arrow up, a square and a cross, in a 14-point box.
pub(crate) const SEND: &str = "M7 12V2M2.5 6.5 7 2l4.5 4.5";
const STOP: &str = "M3.5 3.5h7v7h-7z";
const CLOSE: &str = "M3.5 3.5l7 7M10.5 3.5l-7 7";
/// A paperclip, in the same box.
const ATTACH: &str = "M12.5 6.45 7.15 11.8a3.5 3.5 0 0 1-4.95-4.95l5-5a2.33 2.33 0 0 1 3.3 3.3l-5 5a1.17 1.17 0 0 1-1.65-1.65l4.95-4.95";

/// The conversation's column, as wide as a page meant for reading.
pub(crate) const COLUMN: u32 = 760;
/// The most nodes the conversation sends. The app draws a page of at most 5,000, and a long session's whole history
/// made more: the page was refused and stayed on Loading. The toolbar and the message box take the rest.
pub(crate) const BUDGET: usize = 3_500;
/// The side panel's width. It floats over the right of the tab and takes no width from the conversation, which stays
/// centered in the whole tab. The column is 760 wide, drawn with 8 points of margin either side, so the panel sits beside it
/// while the tab holds that and the panel (with its 12 points of padding) either side of the centre; narrower, it covers the
/// conversation's right side, and Escape or a press outside puts it away.
const SIDE: u32 = 340;
const SIDE_BELOW: u32 = COLUMN + 16 + 2 * (SIDE + 12);
/// The most subagents the panel lists.
const SUBAGENTS_LISTED: usize = 12;
/// The most lines of one call's output Detailed shows, about 3 nodes each: a long output is cut to this and says how many
/// lines it leaves out. Opening one yourself in Compact shows it whole, as before.
const DETAILED_LINES: usize = 30;

pub fn view() -> Vec<Node> {
    chat::with(|c| vec![chat_page(c)])
}

fn chat_page(c: &Chat) -> Node {
    let (body, built) = conversation(c);
    // The conversation, the checklist and the message box in the main pane, and a card on the right that the app leaves out
    // of a narrow tab. Flush, so the conversation scrolls on its own and the checklist and the message box stay at the foot.
    // The prompt rail is the slim pane at their left, which the app leaves out of a narrow tab.
    let rail = crate::rail::Rail::of(&c.session.items, &built, crate::rail::state_at());
    let body = crate::rail::watch(body, &rail);
    let main = Node::new("Split")
        .prop("defaultSize", crate::rail::WIDTH)
        .prop("collapseBelow", crate::rail::BELOW)
        .on("onNarrow", |payload| crate::rail::set_narrow(payload["narrow"].as_bool().unwrap_or(false)))
        .child(Node::new("Slot").prop("name", "start").children(crate::rail::marks(c, &rail)))
        .child(Node::new("Slot").prop("name", "end").children([body].into_iter().chain(todo_card(c)).chain([composer(c)])));
    // The panel floats over the right of the tab and shows by default; only the header's button puts it away and brings it
    // back, also where it covers the conversation in a narrow tab. A panel left out leaves its terminals out with it.
    let shown = side_shown(c);
    let split = Node::new("Split")
        .prop("defaultSize", SIDE)
        .prop("side", "end")
        .prop("float", true)
        .prop("collapseBelow", SIDE_BELOW)
        .on("onNarrow", |payload| chat::set_narrow(payload["narrow"].as_bool().unwrap_or(false)));
    let split = split
        .child(Node::new("Slot").prop("name", "start").child(main))
        .child(Node::new("Slot").prop("name", "end").children(if shown { side(c) } else { Vec::new() }));
    // No page toolbar: the tab and the project's header over it already name the place, so the card starts with a
    // header as a terminal pane's does. The dialogs, which the app draws over the whole page, and the past
    // sessions, which open over it all as Wings' own sheet, come after.
    let body = Node::new("Slot")
        .prop("name", "body")
        .child(header(c))
        .child(split)
        .maybe(crate::plugins_view::dialog(c))
        .maybe(crate::setup_view::dialog(c))
        .maybe(crate::tasks_view::dialog(c))
        .maybe(crate::rewind_view::dialog(c))
        .maybe(crate::fork_view::dialog(c))
        .maybe(crate::edits_view::dialog(c))
        .maybe(crate::fork_view::header_dialog(c))
        .maybe(crate::actions_view::message_dialog(c))
        .maybe(crate::usage_view::dialog(c))
        .maybe(crate::pictures_view::dialog_of(c))
        .maybe(crate::status_view::dialog(c))
        .maybe(crate::past_view::sheet_of(c));
    Node::new("Page").prop("flush", true).child(body)
}

/// Lucide's icons, as the app draws them in its headers: `square-terminal`, `rotate-ccw-clock` (Past sessions), `plus`
/// and `settings`, in a 24-point box.
const TERMINAL: [&str; 3] = ["m7 11 2-2-2-2", "M11 13h4", "M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z"];
const HISTORY: [&str; 3] = ["M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8", "M3 3v5h5", "M12 7v5l4 2"];
const PLUS: [&str; 2] = ["M5 12h14", "M12 5v14"];
/// Lucide's `zap`, on the fast mode button.
const FAST: [&str; 1] = ["M4 14a1 1 0 0 1-.78-1.63l9.9-10.2a.5.5 0 0 1 .86.46l-1.92 6.02A1 1 0 0 0 13 10h7a1 1 0 0 1 .78 1.63l-9.9 10.2a.5.5 0 0 1-.86-.46l1.92-6.02A1 1 0 0 0 11 14z"];
/// Lucide's `panel-right`.
const PANEL: [&str; 2] = ["M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z", "M15 3v18"];
const SETTINGS: [&str; 2] = [
    "M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915",
    "M9 12a3 3 0 1 0 6 0a3 3 0 1 0-6 0",
];

/// A quiet square button holding one of those icons, named by its tooltip, as the app's pane and header buttons are.
fn icon_button(label: &str, paths: &[&str], on_click: impl Fn() + 'static) -> Node {
    button("", "ghost", false, on_click).prop("size", "icon-sm").prop("label", label).prop("title", label).child(icon(24, 2.0, paths).prop("size", 16))
}

/// Whether the side panel is on the page: by default, at any width, beside the conversation in a wide tab and over it in a
/// narrow one. The header's button turns it off for every chat; Escape or a press outside it puts it away for now.
fn side_shown(c: &Chat) -> bool {
    c.side_open
}

/// What is alive in the side panel: a terminal Claude opened, a background task and a subagent that run.
pub fn live_in_side(c: &Chat) -> usize {
    let subagents = c.session.items.iter().filter(|item| matches!(item, Item::Tool(tool) if matches!(tool.name.as_str(), "Task" | "Agent") && tool.output.is_none())).count();
    c.terminals.len() + c.session.tasks.iter().filter(|task| task.ended.is_none()).count() + subagents
}

/// The header's button for the side panel, at any width: lit while the panel shows, and with how many things run in it
/// while it is away.
fn side_button(c: &Chat) -> Option<Node> {
    let live = if c.side_open { 0 } else { live_in_side(c) };
    let title = match (c.side_open, live) {
        (true, _) => "Hide the side panel".to_owned(),
        (false, 0) => "Show the side panel".to_owned(),
        (false, n) => format!("Show the side panel, {n} running"),
    };
    let glyph = icon(24, 2.0, &PANEL).prop("size", 16);
    let button = button("", if c.side_open { "secondary" } else { "ghost" }, false, chat::toggle_side).prop("label", "Side panel").prop("title", title.as_str());
    Some(match live {
        0 => button.prop("size", "icon-sm").child(glyph),
        n => button.prop("size", "sm").child(row().prop("gap", "xs").prop("align", "center").child(glyph).child(text(n.to_string()).prop("size", "small"))),
    })
}

fn header(c: &Chat) -> Node {
    let name = chat::name(c).unwrap_or_else(|| "Claude chat".into());
    let title = row()
        .prop("gap", "m")
        .prop("align", "center")
        .prop("grow", true)
        .child(Node::new("Icon").prop("name", "claude").prop("size", 14))
        .child(text(name.as_str()).prop("weight", "strong").prop("truncate", true).prop("title", name.as_str()));
    let terminal = (c.session.id.is_some() && c.state != State::Moved).then(|| icon_button("Open in terminal", &TERMINAL, chat::to_terminal));
    let actions = row()
        .prop("gap", "xs")
        .prop("align", "center")
        .maybe(terminal)
        .maybe(crate::fork_view::header_button(c))
        .maybe(side_button(c))
        .child(icon_button("Past sessions", &HISTORY, chat::open_past))
        .child(icon_button("New chat", &PLUS, chat::new_chat))
        .child(icon_button("Settings", &SETTINGS, chat::open_settings));
    Node::new("Bar").prop("variant", "slim").child(row().prop("gap", "m").prop("align", "center").child(title).child(actions))
}

fn column() -> Node {
    stack().prop("gap", "l").prop("maxWidth", COLUMN)
}

/// The conversation, or what shows before it has any, with where each of your messages is among the children of its column,
/// as (item, child), oldest first.
fn conversation(c: &Chat) -> (Node, Vec<(usize, usize)>) {
    if c.session.items.is_empty() && c.session.live.is_empty() {
        let body = match &c.state {
            State::Starting => loading(),
            State::Failed(why) => notice("danger", why.as_str()),
            State::Moved => muted("This session goes on in a terminal."),
            _ => empty("What should Claude do?", "It works in this folder with your Claude Code setup, and asks here before it does anything you haven't allowed."),
        };
        return (Node::new("Scroll").child(column().child(body)), Vec::new());
    }
    let items = &c.session.items;
    let from = items.len().saturating_sub(c.shown);
    // Built from the newest back until the budget is spent, so a long session's oldest items stay out of the page.
    let (mut blocks, mut spent, mut cut) = (Vec::new(), 0, false);
    let mut prompts: Vec<(usize, usize)> = Vec::new();
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
            false => item_node(start, &items[start], c, true),
        };
        // A turn that changed files ends with a card of them, after its last item.
        let edited = c.session.turn_ending_at(end).map(|(from, turn)| crate::edits_view::card_of(c, from, end, &turn));
        end = start;
        let Some(block) = block else { continue };
        let size = block.count() + edited.as_ref().map_or(0, Node::count);
        if spent + size > BUDGET {
            cut = true;
            // The newest item always shows, as plain text when it is too big to lay out.
            if blocks.is_empty() {
                blocks.push(plain(start, &items[start]));
            }
            break;
        }
        spent += size;
        blocks.extend(edited);
        if matches!(items[start], Item::User(_) | Item::UserImages(..)) {
            prompts.push((start, blocks.len()));
        }
        blocks.push(block);
    }
    // Counted from the newest back while the blocks were collected, so they turn round with them.
    let built = blocks.len();
    blocks.reverse();
    // Older items load as you scroll up to them. What the budget cut can't be added to, so a note stands for it.
    let earlier = cut.then(|| muted("Older messages aren't shown here. Open the session in a terminal to see all of it.").key("earlier"));
    let more = from > 0 && !cut;
    // A fork says where it came from, at the top.
    let forked = c.forked_from.as_ref().map(|from| muted(format!("Forked from {}", truncate(from, 80))).prop("size", "small").key("forked"));
    let lead = usize::from(forked.is_some()) + usize::from(earlier.is_some());
    let prompts: Vec<(usize, usize)> = prompts.into_iter().rev().map(|(item, back)| (item, lead + built - 1 - back)).collect();
    let mut nodes: Vec<Node> = forked.into_iter().chain(earlier).chain(blocks).collect();
    // What an MCP server asks you waits at the foot, where Claude's own questions do.
    nodes.extend(crate::elicitation_view::cards(c));
    if !c.session.live.is_empty() {
        let live = markdown(&c.session.live).prop("links", true);
        let live = if live.count() > BUDGET / 4 { text(truncate(&c.session.live, 20_000)) } else { live };
        nodes.push(live.key("live"));
    } else if c.session.thinking || (c.session.working && !asking(c)) {
        // The app adds how long it has been and counts it up: "Calling wings · 16s".
        let status = match c.session.waiting_on_subagents() {
            Some(n) => row()
                .prop("gap", "s")
                .prop("align", "center")
                .child(text(format!("Waiting on {}", plural(n, "subagent"))).prop("tone", "muted").prop("shimmer", true).prop("since", c.doing.1))
                .child(button("Stop", "ghost", false, chat::stop).prop("size", "sm")),
            None => {
                let what = if c.doing.0.is_empty() { "Working" } else { c.doing.0.as_str() };
                row().prop("gap", "s").prop("align", "center").child(text(what).prop("tone", "muted").prop("shimmer", true).prop("since", c.doing.1)).maybe(crate::actions_view::status_button(c))
            }
        };
        // While subagents of the turn run, how many, over the status line.
        let (started, running) = c.session.turn_subagents();
        nodes.push(match running {
            0 => status.key("working"),
            _ => stack()
                .prop("gap", "xs")
                .child(muted(format!("{} · {running} working", plural(started, "subagent"))).prop("size", "small"))
                .child(status)
                .key("working"),
        });
    }
    match &c.state {
        State::Failed(why) => nodes.push(notice("danger", why.as_str()).key("ended")),
        State::Ended => nodes.push(muted("Claude Code ended. Start a new chat, or pick a past session, to go on.").key("ended")),
        State::Moved => nodes.push(muted("This session goes on in a terminal.").key("ended")),
        _ => {}
    }
    let scroll = Node::new("Scroll").prop("follow", "end").prop("textScale", c.text_scale as f64).child(column().prop("enter", "rise").children(nodes));
    let scroll = match more {
        true => scroll.on("onReachStart", |_| chat::with(|c| c.shown += PAGE)),
        false => scroll,
    };
    (scroll, prompts)
}

/// An item that folds into the line of the tool calls around it: a call, or a question you answered about one.
pub(crate) fn folds(item: &Item) -> bool {
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
    c.session.items.iter().any(|item| matches!(item, Item::Ask(Ask { answer: None, .. }))) || !c.session.ops.elicitations.is_empty()
}

/// Your message on the right, as chat apps show it, and never wider than the column less a margin on its left: a long one
/// spread over the whole width read like Claude's. `action`, the rewind and fork buttons, sits under it at the right, where
/// it never shares a row with the card.
fn bubble(card: Node, action: Option<Node>) -> Node {
    let said = row().prop("justify", "end").prop("align", "center").child(stack().prop("width", 64)).child(card);
    match action {
        Some(action) => stack().prop("gap", "xs").child(said).child(row().prop("justify", "end").child(action)),
        None => said,
    }
}

/// An item of the conversation, or with `own` false, of a subagent's, whose messages can't be rewound.
pub(crate) fn item_node(at: usize, item: &Item, c: &Chat, own: bool) -> Option<Node> {
    let rewind = || own.then(|| crate::fork_view::message_actions(at, c)).flatten();
    let node = match item {
        Item::User(said) => bubble(card().child(text(said.as_str())), rewind()),
        Item::UserImages(said, pictures) => {
            let card = card().maybe(thumbs(pictures, "Attached picture"));
            bubble(if said.is_empty() { card } else { card.child(text(said.as_str())) }, rewind())
        }
        Item::Images(pictures) => thumbs(pictures, "Picture").unwrap_or_else(stack),
        // A subagent's replies sit in its own list, which the Copy buttons' indexes don't name.
        Item::Text(said) if own => crate::status_view::reply(at, said, c),
        // Words that name something in Wings, like an issue key, open it there.
        Item::Text(said) => crate::edits_view::prose(said, c),
        Item::Divider(said) => row().prop("justify", "center").child(muted(format!("── {said} ──")).prop("size", "small")),
        Item::Notice(said) => muted(said.as_str()).prop("size", "small"),
        Item::Thought(said, ms) => thought(at, said, *ms, c.open_outputs.contains(&format!("thought-{at}"))),
        Item::Ask(ask) if ask.answer.is_none() => ask_node(ask, c),
        Item::Error(said) => notice("danger", said.as_str()),
        Item::Stopped => muted("Stopped"),
        Item::Worked(ms) => muted(format!("Worked for {}", took(*ms))).prop("size", "small"),
        Item::Note(title, body) => note(at, title, body, c.open_outputs.contains(&format!("note-{at}"))),
        Item::Finished(done) => finished(at, done, c),
        Item::Tool(_) | Item::Ask(_) => return None,
    };
    Some(node.key(at.to_string()))
}

/// A run of tool calls as one line that says what they did, open only when you open it (or, in Detailed, until you shut
/// it): one that opened while a call ran and shut once it ended showed the calls for a moment and hid them again. The
/// line under the conversation says what runs. Closed, its calls aren't built at all.
pub(crate) fn steps(first: usize, tools: &[&Tool], c: &Chat) -> Node {
    let open = prefs::is_open(c.detailed, c.open_steps.contains(&first));
    let failed = tools.iter().filter(|tool| matches!(tool.output, Some((_, true)))).count();
    let mut said = describe(tools);
    if failed > 0 {
        said.push_str(&format!(" · {failed} failed"));
    }
    // With `outputs_open` each call's output shows unless you shut it, else only what you opened.
    let calls = |outputs_open: bool| {
        let cap = c.detailed.then_some(DETAILED_LINES);
        stack().prop("gap", "s").children(tools.iter().map(|tool| step(tool, prefs::is_open(outputs_open, c.open_outputs.contains(&tool.id)), cap, c)))
    };
    let rows = if open {
        let rows = calls(c.detailed);
        // Every output of a long run, even cut to its first lines, would take most of the page's nodes and push the rest
        // of the conversation out: its calls then show as lines, with the outputs for you to open.
        if c.detailed && rows.count() > BUDGET / 2 { calls(false) } else { rows }
    } else {
        stack().prop("gap", "s")
    };
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

/// One tool call: what it is, and what it did or printed, folded. The fold is built only once you open it. With a `cap`
/// its output shows no more lines than that.
fn step(tool: &Tool, opened: bool, cap: Option<usize>, c: &Chat) -> Node {
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
        .children(facts(tool))
        .maybe(open_subagent(tool, c))
        .maybe(crate::actions_view::background_button(tool, c));
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
    // What it gave back as pictures (a Read of an image, a screenshot) shows under its line, closed or not.
    let pictures = thumbs(&tool.images, "Picture from the tool");
    match label {
        Some(label) => {
            let id = tool.id.clone();
            let body = if opened { body(tool, output, cap) } else { stack() };
            let label = if error.is_some() { "Whole error" } else { label };
            let fold = disclosure(label, body).prop("open", opened).on("onOpenChange", move |_| chat::toggle_output(&id));
            stack().prop("gap", "xs").child(head).maybe(error).maybe(pictures).child(fold)
        }
        None => match pictures {
            Some(pictures) => stack().prop("gap", "xs").child(head).child(pictures),
            None => head,
        },
    }
    .key(tool.id.as_str())
}

/// A subagent's call opens its conversation in the sidebar, once Claude Code has said its id.
fn open_subagent(tool: &Tool, c: &Chat) -> Option<Node> {
    if !matches!(tool.name.as_str(), "Task" | "Agent") || !crate::subagent_relay::can_open(c, &tool.id) {
        return None;
    }
    let id = tool.id.clone();
    Some(button("Open", "ghost", false, move || crate::tasks_view::open_subagent(&id)).prop("size", "sm").prop("title", "Its conversation, in the sidebar"))
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
fn body(tool: &Tool, output: Option<(&str, bool)>, cap: Option<usize>) -> Node {
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
        Some((output, _)) if failed || !matches!(tool.detail, Detail::Diff(_) | Detail::Write { .. }) => {
            let (shown, more) = cap.map_or((output, 0), |cap| first_lines(output, cap));
            let rest = (more > 0).then(|| muted(format!("{} more", plural(more, "line"))));
            body.child(code_block(shown)).maybe(rest)
        }
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

/// What Claude thought before it answered, folded under "Thought for 12s" until you open it.
fn thought(at: usize, said: &str, ms: u64, opened: bool) -> Node {
    let id = format!("thought-{at}");
    // Under a second, or unknown as in a transcript, a time says nothing.
    let title = if ms >= 1000 { format!("Thought for {}", took(ms)) } else { "Thought".to_owned() };
    let body = if opened { text(said).prop("tone", "muted").prop("size", "small") } else { stack() };
    row().child(disclosure(title, body).prop("open", opened).on("onOpenChange", move |_| chat::toggle_output(&id)))
}

/// What Claude Code wrote in your name, as a quiet line on the left you can open, apart from your own messages.
fn note(at: usize, title: &str, body: &str, opened: bool) -> Node {
    let id = format!("note-{at}");
    let text_ = if opened { text(body).prop("tone", "muted").prop("size", "small") } else { stack() };
    let fold = disclosure(format!("⚙ {title}"), text_).prop("open", opened).on("onOpenChange", move |_| chat::toggle_output(&id));
    row().child(fold)
}

/// The line where a background agent, command or monitor ended: a mark, what ended and how long it ran, an Open button
/// while Claude Code still has the agent or the command's output, and what it reported folded under it.
fn finished(at: usize, done: &Finished, c: &Chat) -> Node {
    let mark = match done.status.as_str() {
        "failed" => text("✗").prop("tone", "danger"),
        "stopped" | "killed" => muted("■"),
        _ => text("✓").prop("tone", "success"),
    };
    // Under a second, a time says nothing.
    let title = match done.took {
        Some(ms) if ms >= 1000 => format!("{} · {}", done.title(), took(ms)),
        _ => done.title(),
    };
    let title = text(title).prop("size", "small").prop("tone", if done.failed() { "danger" } else { "muted" });
    let open = if done.kind == "Agent" {
        let id = done.call.clone();
        (!id.is_empty() && c.session.subagents.contains_key(&id)).then(|| button("Open", "ghost", false, move || crate::tasks_view::open_subagent(&id)).prop("title", "Its transcript"))
    } else {
        let id = done.task.clone();
        c.session.tasks.iter().any(|task| task.id == id && task.has_output()).then(|| button("Open", "ghost", false, move || chat::open_task(&id)).prop("title", "What it printed"))
    };
    let line = row().prop("gap", "s").prop("align", "center").child(mark).child(title).maybe(open.map(|open| open.prop("size", "sm")));
    let opened = c.open_outputs.contains(&format!("finished-{at}"));
    let id = format!("finished-{at}");
    let body = if opened { text(done.report.as_str()).prop("tone", "muted").prop("size", "small") } else { stack() };
    stack()
        .prop("gap", "xs")
        .child(line)
        .maybe((!done.report.is_empty()).then(|| row().child(disclosure("Summary", body).prop("open", opened).on("onOpenChange", move |_| chat::toggle_output(&id)))))
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

/// The message box, floating at the foot: what you type, then a paperclip to attach images, the pickers, and Send or Stop.
fn composer(c: &Chat) -> Node {
    let open = c.state == State::Running;
    // Claude Code's suggested next message shows as grey text in the empty box, and Tab fills the box with it.
    let suggested = chat::suggested(c);
    let field = Node::new("Textarea")
        .prop("label", "Message")
        .prop("placeholder", suggested.unwrap_or("Ask anything. @ files, / commands"))
        .prop("takesTab", suggested.is_some())
        // Shift-Tab goes to the next permission mode, and Up and Down from the first and last line recall prompts.
        .prop("takesShiftTab", true)
        .prop("historyKeys", true)
        .prop("variant", "bare")
        .prop("rows", 2)
        .prop("submitOnEnter", true)
        .prop("autoFocus", true)
        .prop("value", c.draft.as_str())
        .prop("disabled", !open)
        .on("onChange", |event| chat::typed(event["value"].as_str().unwrap_or_default()))
        .on("onPasteImage", |event| chat::add_image(event["mediaType"].as_str().unwrap_or_default(), event["data"].as_str().unwrap_or_default()))
        .on("onKeyDown", |event| {
            let shift = event["shiftKey"] == true;
            match event["key"].as_str() {
                Some("Enter") => chat::send(),
                Some("Tab") if shift => chat::cycle_mode(),
                Some("Tab") => chat::accept_suggestion(),
                Some("Escape") => chat::escape(),
                Some("ArrowUp") => chat::recall_older(),
                Some("ArrowDown") => chat::recall_newer(),
                _ => {}
            }
        });
    // One round button, as chat apps have: a square to stop while Claude works and nothing is typed, else an arrow to send.
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
    let tab = suggested.map(|_| chip("Tab", false, chat::accept_suggestion).prop("title", "Use the suggested message (Tab)").key("tab"));
    // As big and as round as the send button, so the row's ends match.
    let attach = button("", "ghost", false, chat::attach)
        .prop("size", "icon-sm")
        .prop("round", true)
        .prop("label", "Attach images")
        .prop("title", "Attach images")
        .prop("disabled", !open)
        .child(icon(14, 1.5, &[ATTACH]));
    let controls = row()
        .prop("gap", "xs")
        .prop("align", "center")
        .child(attach)
        .children(pickers(c))
        .child(stack().prop("grow", true))
        .maybe(tab)
        .child(action.prop("round", true));
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
            .child(message_box),
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

/// The panel on the right, quiet beside the conversation: the folder and branch, what the working tree changed, how full the
/// context is and how much of your plan's limits is used, and the session's subagents, each of which opens in the sidebar. Nothing when there is nothing to say yet.
fn side(c: &Chat) -> Vec<Node> {
    let small = |said: String| text(said).prop("size", "small");
    let mut sections: Vec<Node> = Vec::new();
    let place = match (c.folder.is_empty(), &c.branch) {
        (true, _) => None,
        (false, Some(branch)) => Some(format!("{} · {branch}", c.folder)),
        (false, None) => Some(c.folder.clone()),
    };
    let mut head = stack().prop("gap", "xs");
    if let Some(place) = place {
        head = head.child(small(place.clone()).prop("tone", "muted").prop("truncate", true).prop("title", place));
    }
    if let Some((added, removed)) = c.changes {
        // A click shows Wings' Changes sidebar.
        head = head.child(
            list_button(
                [small("Changes".into()).prop("grow", true), fact(format!("+{added}"), "success"), fact(format!("−{removed}"), "danger")],
                chat::open_changes,
            )
            .prop("title", "Open Changes"),
        );
    }
    sections.push(head);
    let mut limits = stack().prop("gap", "s");
    if let Some((used, most)) = c.session.context {
        limits = limits.child(meter_button("Context", used * 100 / most.max(1), (70, 90), None, Panel::Context));
    }
    for (label, used, resets) in &c.session.limits {
        let resets = resets.map(|ms| match *label {
            "5h" => format!("resets {}", clock(ms)),
            _ => format!("resets {} {}", WEEKDAYS[local(ms as i64).weekday as usize % 7], clock(ms)),
        });
        limits = limits.child(meter_button(if *label == "5h" { "5-hour" } else { "Week" }, *used, (80, 95), resets, Panel::Usage));
    }
    sections.push(limits);
    sections.extend(crate::tasks_view::section(c));
    sections.extend(crate::fork_view::folders(c));
    sections.extend(subagents(c));
    // A stack with nothing in it takes room: leave out the sections that have nothing.
    let sections: Vec<Node> = sections.into_iter().filter(|section| section.count() > 1).collect();
    let summary = (!sections.is_empty()).then(|| card().prop("variant", "raised").child(stack().prop("gap", "m").children(sections)));
    summary.into_iter().chain(terminals(c)).collect()
}

/// How many of the terminals Claude opened the panel shows, newest first, and how tall each is.
const TERMINALS_SHOWN: usize = 3;
const TERMINAL_HEIGHT: u32 = 220;

/// The terminals Claude opened with Wings' `open_pane`, under the session card: Wings draws each live where its node is,
/// with what it runs, whether it still does, and buttons that open it as a tab and close it.
fn terminals(c: &Chat) -> Vec<Node> {
    c.terminals
        .iter()
        .rev()
        .take(TERMINALS_SHOWN)
        .map(|terminal| Node::new("Terminal").prop("pane", terminal.pane.as_str()).prop("height", TERMINAL_HEIGHT).key(format!("terminal-{}", terminal.pane)))
        .collect()
}

/// A meter row that is a button: a click opens the dialog with the detail behind it.
fn meter_button(label: &str, percent: u64, levels: (u64, u64), resets: Option<String>, panel: Panel) -> Node {
    let title = match panel {
        Panel::Context => "Open Context",
        Panel::Usage => "Open Usage",
    };
    list_button([meter_row(label, percent, levels, resets)], move || chat::open_panel(panel)).prop("title", title).key(label)
}

/// One line of the panel: its name on the left and a percent on the right, a thin bar under it that turns amber at `levels`'
/// first and red at its second, and when it resets in small muted text beside the name.
fn meter_row(label: &str, percent: u64, levels: (u64, u64), resets: Option<String>) -> Node {
    let line = row()
        .prop("gap", "s")
        .prop("align", "baseline")
        .child(text(label).prop("size", "small"))
        .child(stack().prop("grow", true).maybe(resets.map(|resets| muted(resets).prop("size", "small").prop("truncate", true))))
        .child(fact(format!("{percent}%"), "muted"));
    let meter = Node::new("Meter").prop("label", label).prop("percent", percent.min(100) as f64);
    let meter = match level(percent, levels) {
        Some(tone) => meter.prop("tone", tone),
        None => meter,
    };
    stack().prop("gap", "xs").child(line).child(meter).key(label)
}

/// The tone a meter at `percent` takes: none below the first level, amber from it, red from the second.
fn level(percent: u64, levels: (u64, u64)) -> Option<&'static str> {
    match percent {
        p if p >= levels.1 => Some("danger"),
        p if p >= levels.0 => Some("warn"),
        _ => None,
    }
}

/// The subagents this session started, from its Task calls: each with its model, whether it runs, and how long it has or
/// took, and a click opens its conversation in the sidebar. The newest few, as the panel is small.
fn subagents(c: &Chat) -> Option<Node> {
    let tasks: Vec<&Tool> = c.session.items.iter().filter_map(|item| match item {
        Item::Tool(tool) if matches!(tool.name.as_str(), "Task" | "Agent") => Some(tool),
        _ => None,
    }).collect();
    if tasks.is_empty() {
        return None;
    }
    let running = tasks.iter().filter(|tool| subagent_runs(tool, c)).count();
    let title = if running > 0 { format!("Subagents · {running} running") } else { format!("Subagents · {} done", tasks.len()) };
    let hidden = tasks.len().saturating_sub(SUBAGENTS_LISTED);
    let rows = tasks[hidden..].iter().map(|tool| subagent_row(tool, c));
    Some(
        stack()
            .prop("gap", "xs")
            .child(muted(title).prop("size", "small"))
            .maybe((hidden > 0).then(|| muted(format!("{hidden} earlier")).prop("size", "small")))
            .child(stack().prop("gap", "none").children(rows)),
    )
}

/// The background task a subagent's call started, when it went to the background.
fn background_of<'a>(tool: &Tool, c: &'a Chat) -> Option<&'a Task> {
    c.session.tasks.iter().find(|task| !task.call.is_empty() && task.call == tool.id)
}

/// A subagent runs until its call's result, or for one in the background, until its task ends: the call is answered the
/// moment it is launched.
fn subagent_runs(tool: &Tool, c: &Chat) -> bool {
    background_of(tool, c).map_or(tool.output.is_none(), |task| task.ended.is_none())
}

fn subagent_row(tool: &Tool, c: &Chat) -> Node {
    let sub = c.session.subagents.get(&tool.id);
    let background = background_of(tool, c);
    let running = subagent_runs(tool, c);
    let failed = background.map_or(matches!(tool.output, Some((_, true))), |task| task.ended.as_deref() == Some("failed"));
    let mark = match (running, failed) {
        (true, _) => spinner(),
        (false, true) => text("✗").prop("tone", "danger"),
        (false, false) => text("✓").prop("tone", "success"),
    };
    let model = sub.and_then(|sub| sub.reported_model.as_deref()).map(model_name);
    let name = if tool.summary.is_empty() { "Subagent" } else { tool.summary.as_str() };
    let ran = background.filter(|task| task.ended.is_some() && task.started > 0).map(|task| task.ended_at.saturating_sub(task.started));
    let time = match (running, sub.filter(|sub| sub.started > 0)) {
        (true, Some(sub)) => Some(muted("working").prop("size", "small").prop("since", sub.started)),
        (false, Some(sub)) => Some(fact(took(ran.unwrap_or(sub.last.saturating_sub(sub.started))), "muted")),
        (false, None) => ran.map(|ms| fact(took(ms), "muted")),
        _ => None,
    };
    // The name has the whole row, three lines at most; under it the model and the time on one muted line.
    let facts = row().prop("gap", "s").prop("align", "center").maybe(model.map(|model| muted(model).prop("size", "small"))).maybe(time);
    let line = row()
        .prop("gap", "s")
        .prop("align", "center")
        .child(mark)
        .child(stack().prop("grow", true).prop("gap", "none").child(text(name).prop("size", "small").prop("clamp", true).prop("title", name)).child(facts));
    match crate::subagent_relay::can_open(c, &tool.id) {
        true => {
            let id = tool.id.clone();
            list_button([line], move || crate::tasks_view::open_subagent(&id)).prop("title", "Its conversation, in the sidebar").key(tool.id.as_str())
        }
        false => line.key(tool.id.as_str()),
    }
}

/// How long, as "21s", "2m 5s", "1h 4m".
pub fn took(ms: u64) -> String {
    let seconds = ms / 1000;
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m {}s", seconds / 60, seconds % 60),
        _ => format!("{}h {}m", seconds / 3600, seconds % 3600 / 60),
    }
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
        // A message from a subagent's tab says what you sent, not how Claude is asked to pass it on.
        let shown = crate::actions::relayed_note(&queued.text);
        let said_text = shown.as_deref().unwrap_or(queued.text.as_str());
        // Nothing here measures the text, so a message counts as long once it can't fit a line of the narrow card.
        let long = said_text.chars().count() > QUEUED_LINE || said_text.contains('\n');
        // The text takes the room the buttons leave and is cut short in it, so a long message keeps them in the card;
        // opened, it wraps whole and the buttons stay by its first line.
        let said = if queued.text.is_empty() {
            muted(plural(queued.images.len(), "image"))
        } else if open {
            text(said_text)
        } else {
            text(said_text).prop("truncate", true).prop("title", said_text)
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

/// The model with its effort in one menu, as "Opus 5.5 · High", and the permission mode, once Claude Code has said what
/// it has.
fn pickers(c: &Chat) -> Vec<Node> {
    let session = &c.session;
    let mut pickers = Vec::new();
    if !session.models.is_empty() {
        let (options, value) = model_options(session);
        pickers.push(select("Model and effort", &options, &value, chat::pick_model_effort).prop("variant", "ghost"));
    }
    // Fast mode, for a model that has it: a lit button while it is on.
    if session.current_model().is_some_and(|model| model.fast) {
        let options = &session.options;
        let title = match (&options.fast_blocked, options.fast) {
            (Some(why), _) => format!("Fast mode can't be used: {}", why.replace('_', " ")),
            (None, true) => "Fast mode is on. Click to turn it off".to_owned(),
            (None, false) => "Fast mode: faster replies that use more of your limits".to_owned(),
        };
        let fast = button("", if options.fast { "secondary" } else { "ghost" }, false, chat::toggle_fast)
            .prop("size", "icon-sm")
            .prop("label", if options.fast { "Fast mode, on" } else { "Fast mode" })
            .prop("title", title.as_str())
            .prop("disabled", options.fast_blocked.is_some())
            .child(icon(24, 2.0, &FAST).prop("size", 14));
        pickers.push(fast);
    }
    if session.options.styles.len() > 1 {
        let options: Vec<(String, String)> = session.options.styles.iter().map(|style| (style.clone(), format!("Style: {style}"))).collect();
        let value = session.options.style.clone().unwrap_or_default();
        pickers.push(select("Output style", &options, &value, chat::pick_style).prop("variant", "ghost"));
    }
    if let Some(mode) = &session.mode {
        let options: Vec<(String, String)> = MODES.iter().map(|(value, name)| (value.to_string(), name.to_string())).collect();
        pickers.push(select("Permission mode", &options, mode, chat::pick_mode).prop("variant", "ghost"));
    }
    pickers
}

/// The model menu's options, as `<model>@<effort>` and its label: each model with each effort it takes, as "Opus 5.5 ·
/// Extra high", or alone when it takes none. And the one in use, where an effort the model doesn't list, or none known,
/// shows as High, else its first.
fn model_options(session: &Session) -> (Vec<(String, String)>, String) {
    let options = session
        .models
        .iter()
        .flat_map(|model| {
            let alone = model.efforts.is_empty().then(|| (format!("{}@", model.value), model.name.clone()));
            let efforts = model.efforts.iter().map(move |effort| (format!("{}@{effort}", model.value), format!("{} · {}", model.name, effort_name(effort))));
            alone.into_iter().chain(efforts)
        })
        .collect();
    let model = session.current_model().or_else(|| session.models.iter().find(|model| model.value == "default"));
    let value = model.map_or_else(String::new, |model| {
        let listed = |effort: &String| model.efforts.contains(effort);
        let effort = session.current_effort().filter(listed).or_else(|| model.efforts.iter().find(|effort| *effort == "high").or(model.efforts.first()).cloned());
        format!("{}@{}", model.value, effort.unwrap_or_default())
    });
    (options, value)
}

/// An effort as the menu says it: "xhigh" is "Extra high".
fn effort_name(effort: &str) -> String {
    if effort == "xhigh" { "Extra high".into() } else { capital(effort) }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn terminal(pane: &str) -> crate::chat::Terminal {
        crate::chat::Terminal { pane: pane.into() }
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
    fn the_side_panel_shows_by_default_at_any_width_and_the_button_turns_it_off_and_on() {
        for narrow in [None, Some(false), Some(true)] {
            chat::with(|c| {
                c.side_open = true;
                c.narrow = narrow;
                assert!(side_shown(c), "{narrow:?}");
                assert!(side_button(c).is_some(), "the button is there at any width");
            });
        }
        chat::set_narrow(false);
        chat::toggle_side();
        assert!(!chat::with(|c| side_shown(c)));
        chat::toggle_side();
        assert!(chat::with(|c| side_shown(c)));
    }

    #[test]
    fn the_toggle_flips_the_panel_and_a_chat_starts_with_it_on() {
        // The choice is a setting every chat follows (`prefs::side_panel`, which Wings stores); outside Wings nothing is
        // stored, so here only this chat's panel flips.
        assert!(prefs::side_panel().get());
        chat::with(|c| c.side_open = true);
        chat::toggle_side();
        assert!(!chat::with(|c| c.side_open));
        chat::toggle_side();
        assert!(chat::with(|c| c.side_open));
    }

    #[test]
    fn only_the_button_puts_the_panel_away_even_over_a_narrow_tab() {
        chat::set_narrow(true);
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

    #[test]
    fn the_header_button_shows_a_count_when_something_runs_in_the_hidden_panel() {
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
    fn each_message_you_sent_is_found_among_the_children_of_the_conversations_column() {
        chat::with(|c| {
            c.session.items = vec![
                Item::User("one".into()),
                Item::Text("reply".into()),
                Item::Tool(crate::session::Tool { id: "t".into(), name: "Read".into(), summary: String::new(), output: None, detail: crate::session::Detail::Plain, images: Vec::new() }),
                Item::User("two".into()),
                Item::Text("reply".into()),
            ];
            c.forked_from = Some("Fix the build".into());
        });
        let (_, found) = chat::with(|c| conversation(c));
        // A fork's note is the first child, so the first message is the second.
        assert_eq!(found, [(0, 1), (3, 4)]);
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

    #[test]
    fn a_duration_is_told_in_seconds_minutes_and_hours() {
        assert_eq!([took(21_400), took(125_000), took(3_900_000)], ["21s", "2m 5s", "1h 5m"]);
    }

    #[test]
    fn a_meter_turns_amber_then_red_at_its_levels() {
        assert_eq!([level(40, (70, 90)), level(70, (70, 90)), level(95, (70, 90))], [None, Some("warn"), Some("danger")]);
    }

    #[test]
    fn the_model_menu_lists_each_model_with_the_efforts_it_takes() {
        let model = |value: &str, name: &str, efforts: &[&str]| crate::session::Model {
            value: value.into(),
            name: name.into(),
            efforts: efforts.iter().map(|effort| effort.to_string()).collect(),
            resolved: format!("claude-{value}-5-5"),
            fast: false,
        };
        let mut session = Session::default();
        session.models = vec![model("opus", "Opus 5.5", &["medium", "high", "xhigh"]), model("haiku", "Haiku 5.5", &[])];
        session.model = Some("opus".into());
        let (options, value) = model_options(&session);
        let labels: Vec<(&str, &str)> = options.iter().map(|(value, label)| (value.as_str(), label.as_str())).collect();
        assert_eq!(labels, [("opus@medium", "Opus 5.5 · Medium"), ("opus@high", "Opus 5.5 · High"), ("opus@xhigh", "Opus 5.5 · Extra high"), ("haiku@", "Haiku 5.5")]);
        // With no effort known yet, High.
        assert_eq!(value, "opus@high");
        session.effort = Some("xhigh".into());
        assert_eq!(model_options(&session).1, "opus@xhigh");
        session.model = Some("haiku".into());
        assert_eq!(model_options(&session).1, "haiku@");
    }
}
