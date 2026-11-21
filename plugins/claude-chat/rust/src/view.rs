//! What the chat page shows: the conversation in a reading column, newest at the bottom and kept there while you read
//! the end, and a floating message box with the model, effort and mode pickers in it. Your messages are bubbles on the
//! right; Claude's replies are plain text; its tool calls fold into one line per run of them; a question it asks is a
//! card: Allow, Always allow and Deny with a reason; its questions with options to pick; its plan to approve. A header
//! like a terminal pane's names the session and holds the chat's actions, and Wings' own Past sessions opens over it.

use wings_plugin::{
    format::{clock, plural, truncate},
    local_time::local,
    markdown::markdown,
    ui::{
        Entry, Node, badge, button, card, chip, confirm, disclosure, empty, entry, gauge, icon, list, list_button, loading, muted,
        notice, row, select, spinner, stack, text, text_input,
    },
};

use crate::{
    chat::{self, Chat, PAGE, State},
    code_view,
    folds::Kind,
    pictures_view::thumbs,
    prefs,
    session::{Ask, Detail, Diff, Draft, Finished, Item, MODES, Progress, Session, Task, Todo, Tool, exit_code, files_found, model_name},
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
/// The side panel's width, while it is in the chat. It floats over the right of the tab and takes no width from the
/// conversation, which stays centered in the whole tab. The column is 760 wide, drawn with 8 points of margin either side,
/// so the panel sits beside it while the tab holds that and the panel (with its 12 points of padding) either side of the
/// centre; narrower, it covers the conversation's right side down to the message box, and Escape or a press outside puts
/// it away.
const SIDE: u32 = 340;
const SIDE_BELOW: u32 = COLUMN + 16 + 2 * (SIDE + 12);
/// The widest the collapsed pill gets: the panel's own width. Past it the project and branch are cut.
const RAIL_MOST: u32 = SIDE;
/// The most subagents the session panel lists.
const SUBAGENTS_LISTED: usize = 12;
pub fn view() -> Vec<Node> {
    let page = chat::with(|c| {
        crate::refs_view::scan(c);
        wings_plugin::ui::palette(crate::mods_view::palette(c));
        crate::session_side::publish(c);
        crate::search::refresh(c);
        crate::bookmarks::refresh(c);
        // A plan opened as a document takes the tab's place until you go back.
        vec![crate::plan_doc_view::page_of(c).unwrap_or_else(|| chat_page(c))]
    });
    // The tool rows the page drew are asked of the mods once it is built, as asking writes to Claude Code.
    crate::mods_view::ask_rows();
    page
}

fn chat_page(c: &Chat) -> Node {
    let (body, built, last) = conversation(c);
    // The conversation, the checklist and the message box. Flush, so the conversation scrolls on its own and the checklist
    // and the message box stay at the foot.
    // The prompt rail sits over them against the column's left, where the app has room for it.
    let rail = crate::rail::Rail::of(&c.session.items, &built, crate::rail::state_at());
    let body = crate::rail::watch(body, &rail, last);
    // The rail goes down to the checklist and the message box, not past them: it is laid over the conversation alone.
    let conversation = stack().prop("fill", true).prop("gap", "none").children(crate::rail::marks(c, &rail)).child(body);
    // The session panel is a card on the right that the app leaves out of a narrow tab and that shows by default; only the
    // header's button puts it away and brings it back, also where it covers the conversation in a narrow tab. A panel left
    // out leaves its terminals out with it. In Wings' sidebar (`session_side.rs`) the conversation has the tab's width.
    let main: Vec<Node> = if c.side_in_wings {
        vec![conversation, composer(c)]
    } else {
        let split = Node::new("Split")
            .prop("defaultSize", if side_shown(c) && side_railed(c) { rail_width(c) } else { SIDE })
            .prop("side", "end")
            .prop("float", true)
            .prop("collapseBelow", SIDE_BELOW)
            .on("onNarrow", |payload| chat::set_narrow(payload["narrow"].as_bool().unwrap_or(false)));
        // Where the panel covers the conversation, in a narrow tab, the message box is under the split, not in it: the
        // panel ends above the box and scrolls when it is taller than that, so it never covers what you type, nor the
        // box's buttons. In a wide tab it sits beside the column and reaches down beside the box.
        let (start, under) = match c.narrow == Some(true) {
            true => (vec![conversation], Some(composer(c))),
            false => (vec![conversation, composer(c)], None),
        };
        let split = split
            .child(Node::new("Slot").prop("name", "start").children(start))
            .child(Node::new("Slot").prop("name", "end").children(if side_shown(c) { side(c) } else { Vec::new() }));
        std::iter::once(split).chain(under).collect()
    };
    // No page toolbar: the tab and the project's header over it already name the place, so the card starts with a
    // header as a terminal pane's does. The dialogs, which the app draws over the whole page, and the past
    // sessions, which open over it all as Wings' own sheet, come after.
    let body = Node::new("Slot")
        .prop("name", "body")
        .child(header(c))
        .maybe(crate::search::bar(c))
        .children(main)
        .maybe(crate::plugins_view::dialog(c))
        .maybe(crate::setup_view::dialog(c))
        .maybe(crate::tasks_view::dialog(c))
        .maybe(crate::rewind_view::dialog(c))
        .maybe(crate::fork_view::dialog(c))
        .maybe(crate::edits_view::dialog(c))
        .maybe(crate::fork_view::header_dialog(c))
        .maybe(crate::titles_view::dialog(c))
        .maybe(crate::actions_view::message_dialog(c))
        .maybe(crate::usage_view::dialog(c))
        .maybe(crate::pictures_view::dialog_of(c))
        .maybe(crate::status_view::dialog(c))
        .maybe(crate::pull_view::dialog(c))
        .maybe(crate::mods_view::menu(c));
    let page = Node::new("Page").prop("flush", true).on_text_scale(crate::prefs::step_text_size);
    // The pattern of pixels or the gradient you picked in Settings, behind the whole tab. Needs Wings 0.2.59, 0.2.60 for
    // a gradient and a Wings after 0.2.60 for a pattern other than the drift; an older one keeps the ground.
    let page = match &c.backdrop {
        Some(kind) => page.backdrop(kind),
        None => page,
    };
    page.child(body)
}

/// Lucide's icons, as the app draws them in its headers: `square-terminal`, `rotate-ccw-clock` (Past sessions), `plus`
/// and `settings`, in a 24-point box.
const HISTORY: [&str; 3] = ["M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8", "M3 3v5h5", "M12 7v5l4 2"];
const PLUS: [&str; 2] = ["M5 12h14", "M12 5v14"];
/// Lucide's `zap`, on the fast mode button.
const FAST: [&str; 1] = ["M4 14a1 1 0 0 1-.78-1.63l9.9-10.2a.5.5 0 0 1 .86.46l-1.92 6.02A1 1 0 0 0 13 10h7a1 1 0 0 1 .78 1.63l-9.9 10.2a.5.5 0 0 1-.86-.46l1.92-6.02A1 1 0 0 0 11 14z"];
/// Lucide's `chevron-right` and `chevron-left`, which collapse the side panel to its rail and open it again.
const CHEVRON_RIGHT: [&str; 1] = ["m9 18 6-6-6-6"];
const CHEVRON_LEFT: [&str; 1] = ["m15 18-6-6 6-6"];
/// Lucide's `panel-right`.
const PANEL: [&str; 2] = ["M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z", "M15 3v18"];
const SETTINGS: [&str; 2] = [
    "M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915",
    "M9 12a3 3 0 1 0 6 0a3 3 0 1 0-6 0",
];

/// A quiet square button holding one of those icons, named by its tooltip, as the app's pane and header buttons are.
pub(crate) fn icon_button(label: &str, paths: &[&str], on_click: impl Fn() + 'static) -> Node {
    icon_button_titled(label, label, paths, on_click)
}

/// `icon_button`, with a tooltip other than its label.
fn icon_button_titled(label: &str, title: &str, paths: &[&str], on_click: impl Fn() + 'static) -> Node {
    button("", "ghost", false, on_click).prop("size", "icon-sm").prop("label", label).prop("title", title).child(icon(24, 2.0, paths).prop("size", 16))
}

/// Whether the side panel is on the page while it is in the chat: by default, at any width, beside the conversation in a
/// wide tab and over it in a narrow one. The header's button turns it off for every chat.
fn side_shown(c: &Chat) -> bool {
    c.side_open
}

/// Whether the panel in the chat is collapsed to its rail: as you left it with the panel's own button, else collapsed while
/// the tab is narrow enough for the panel to cover the conversation, and open where it sits beside it.
pub(crate) fn side_railed(c: &Chat) -> bool {
    c.side_rail.unwrap_or(c.narrow == Some(true))
}

/// What is alive in the session panel: a terminal Claude opened, a background task and a subagent that run.
pub fn live_in_side(c: &Chat) -> usize {
    let subagents = c.session.items.iter().filter(|item| matches!(item, Item::Tool(tool) if matches!(tool.name.as_str(), "Task" | "Agent") && tool.output.is_none())).count();
    c.terminals.len() + c.session.tasks.iter().filter(|task| task.ended.is_none()).count() + subagents
}

/// The header's button for the session panel, at any width: lit while the panel shows (in the chat, or in the sidebar
/// with this chat), and with how many things run in it while it doesn't.
fn side_button(c: &Chat) -> Option<Node> {
    let (shown, place) = if c.side_in_wings { (crate::session_side::shown(), "session sidebar") } else { (side_shown(c), "side panel") };
    let live = if shown { 0 } else { live_in_side(c) };
    let title = match (shown, live) {
        (true, _) => format!("Hide the {place}"),
        (false, 0) => format!("Show the {place}"),
        (false, n) => format!("Show the {place}, {n} running"),
    };
    let glyph = icon(24, 2.0, &PANEL).prop("size", 16);
    let button = button("", if shown { "secondary" } else { "ghost" }, false, chat::toggle_side).prop("label", "Side panel").prop("title", title.as_str()).shortcut("cmd-alt-b");
    Some(match live {
        0 => button.prop("size", "icon-sm").child(glyph),
        // The count is a small badge beside the icon, with room between them.
        n => button.prop("size", "sm").child(row().prop("gap", "s").prop("align", "center").child(glyph).child(badge(n.to_string(), Some("info")))),
    })
}

fn header(c: &Chat) -> Node {
    let name = chat::name(c).unwrap_or_else(|| "Claude chat".into());
    let title = row()
        .prop("gap", "m")
        .prop("align", "center")
        .prop("grow", true)
        // The app's status dot and the folder after the title, as a terminal pane's header has them.
        .child(Node::new("Dot").prop("state", c.agent.as_ref().map_or("idle", |agent| agent.0.as_str())))
        .child(text(name.as_str()).prop("weight", "strong").prop("truncate", true).prop("title", name.as_str()))
        .maybe((!c.folder.is_empty()).then(|| {
            let place = c.branch.as_ref().map_or_else(|| c.folder.clone(), |branch| format!("{} · {branch}", c.folder));
            muted(place.as_str()).prop("size", "small").prop("truncate", true).prop("title", place.as_str())
        }));
    let actions = row()
        .prop("gap", "xs")
        .prop("align", "center")
        .maybe(crate::claude_version::pill(c.narrow == Some(true)))
        .child(crate::search::header_button(c))
        .child(crate::view_mode::mode_button(c))
        .maybe(crate::titles_view::header_button(c))
        .maybe(crate::fork_view::header_button(c))
        .maybe(side_button(c))
        .maybe(account_menu(c))
        .child(icon_button("Past sessions", &HISTORY, chat::open_past).shortcut("cmd-y"))
        .child(icon_button_titled("New chat", if chat::in_use(c) { "New chat in a new tab, this one keeps running" } else { "New chat" }, &PLUS, chat::new_chat).shortcut("cmd-shift-n"))
        .child(icon_button("Settings", &SETTINGS, chat::open_settings));
    Node::new("Bar").prop("variant", "slim").child(row().prop("gap", "m").prop("align", "center").child(title).child(actions))
}

fn column() -> Node {
    stack().prop("gap", "l").prop("maxWidth", COLUMN)
}

/// The conversation, or what shows before it has any, with where each of your messages is among the children of its column,
/// as (item, child), oldest first, and the column's last child.
fn conversation(c: &Chat) -> (Node, Vec<(usize, usize)>, usize) {
    if c.session.items.is_empty() && c.session.live.is_empty() {
        let body = match &c.state {
            State::Starting => loading(),
            State::Failed(why) => notice("danger", why.as_str()),
            // A new chat's prompt sits in the middle of the conversation, over a soft glow. Needs Wings 0.2.58. Over the drift
            // of pixels or a gradient it goes without, so one backdrop moves behind it, not two.
            _ => {
                let new = empty("What should Claude do?", "It works in this folder with your Claude Code setup, and asks here before it does anything you haven't allowed.");
                let new = new.prop("fill", true);
                let new = if c.backdrop.is_some() { new } else { new.prop("backdrop", "glow") };
                return (new, Vec::new(), 0);
            }
        };
        return (Node::new("Scroll").child(column().child(body)), Vec::new(), 0);
    }
    let items = &c.session.items;
    let (end, newest) = (end_of(c), c.until.is_none());
    let (window, _) = window(c, end, end.saturating_sub(c.shown));
    let first = window.last().map_or(end, |block| block.start);
    let mut blocks = Vec::new();
    let mut prompts: Vec<(usize, usize)> = Vec::new();
    let mut placed: Vec<(usize, usize, usize)> = Vec::new();
    for block in window {
        blocks.extend(block.edited);
        placed.push((block.start, block.end, blocks.len()));
        if matches!(items[block.start], Item::User(_) | Item::UserImages(..)) {
            prompts.push((block.start, blocks.len()));
        }
        blocks.push(block.node);
    }
    // Counted from the newest back while the blocks were collected, so they turn round with them.
    let built = blocks.len();
    blocks.reverse();
    // A fork says where it came from, at the top.
    let forked = c.forked_from.as_ref().map(|from| muted(format!("Forked from {}", truncate(from, 80))).prop("size", "small").key("forked"));
    let lead = usize::from(forked.is_some());
    let prompts: Vec<(usize, usize)> = prompts.into_iter().rev().map(|(item, back)| (item, lead + built - 1 - back)).collect();
    crate::rail::place(placed.into_iter().map(|(start, end, back)| (start, end, lead + built - 1 - back)).collect());
    let mut nodes: Vec<Node> = forked.into_iter().chain(blocks).collect();
    // Reading an older part, what Claude does now isn't here: it shows once you are back at the newest.
    if newest {
        newest_end(c, &mut nodes);
    }
    let last = nodes.len().saturating_sub(1);
    // A right-click on a message, a link or a code block opens a menu with what mods add to it.
    let column = column().prop("enter", "rise").on("onContextMenu", |payload| crate::mods_view::open_menu(payload)).children(nodes);
    let scroll = Node::new("Scroll").prop("follow", "end").prop("textScale", c.text_scale as f64).child(column);
    // Older items load as you scroll up to them, and newer ones as you scroll back down.
    let scroll = match first > 0 {
        true => scroll.on("onReachStart", |_| chat::with(show_older)),
        false => scroll,
    };
    let scroll = match newest {
        true => scroll,
        false => scroll.on("onReachEnd", |_| chat::with(show_newer)),
    };
    (scroll, prompts, last)
}

/// What follows the newest item: what an MCP server asks you, what Claude is writing or doing, and how the session ended.
fn newest_end(c: &Chat, nodes: &mut Vec<Node>) {
    // What an MCP server asks you waits at the foot, where Claude's own questions do.
    nodes.extend(crate::elicitation_view::cards(c));
    // A hit usage limit, once the turn that hit it is over.
    nodes.extend(crate::limit_view::card(c));
    if !c.session.live.is_empty() {
        let live = crate::edits_view::prose_in("live", &c.session.live, c);
        let live = if live.count() > BUDGET / 4 { text(truncate(&c.session.live, 20_000)) } else { live };
        nodes.push(live.key("live"));
    } else if c.session.thinking || (c.session.working && !asking(c)) {
        // The app adds how long it has been and counts it up: "Calling wings · 16s".
        let status = match c.session.waiting_on_subagents() {
            Some(n) => row()
                .prop("gap", "s")
                .prop("align", "center")
                .child(text(format!("Waiting on {}", plural(n, "subagent"))).prop("tone", "muted").prop("shimmer", true).prop("since", c.doing.1))
                .child(button("Stop", "outline", false, chat::stop).prop("size", "sm")),
            None => {
                // A mod may draw the line, or reword it.
                let (drawn, said) = crate::mods_view::spinner(c);
                let what = said.unwrap_or_else(|| if c.doing.0.is_empty() { "Working".to_owned() } else { c.doing.0.clone() });
                // "Retrying in 5s" already names a time: the elapsed counter beside it would be a second timer.
                let retrying = what.starts_with("Retrying in ");
                let line = match drawn {
                    Some(drawn) => drawn,
                    None if retrying => text(what).prop("tone", "muted").prop("shimmer", true),
                    None => text(what).prop("tone", "muted").prop("shimmer", true).prop("since", c.doing.1),
                };
                // "Working · 16s" and then "· 2.1k tokens": what Claude has written this turn, thinking included.
                let tokens = crate::spend::live_tokens(c.session.meter.tokens()).map(|said| text(format!("· {said}")).prop("tone", "muted"));
                row().prop("gap", "s").prop("align", "center").child(line).maybe(tokens).maybe(crate::actions_view::status_button(c))
            }
        };
        // While subagents of the turn run, how many, over the status line.
        let (started, running) = c.session.turn_subagents();
        // Waiting on subagents already says how many, and the line has its Stop beside it.
        nodes.push(match running {
            0 => status.key("working"),
            _ if c.session.waiting_on_subagents().is_some() => status.key("working"),
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
        _ => {}
    }
}

/// One block of the conversation: an item, or a run of tool calls, from `start` to before `end`, with the card of the
/// files its turn changed when it ends one.
pub(crate) struct Block {
    pub start: usize,
    pub end: usize,
    node: Node,
    edited: Option<Node>,
}

impl Block {
    fn size(&self) -> usize {
        self.node.count() + self.edited.as_ref().map_or(0, Node::count)
    }
}

/// The block that ends before `end`, not reaching before `from`, and where it starts. None for one that shows nothing.
fn block_before(c: &Chat, end: usize, from: usize) -> (usize, Option<Block>) {
    let items = &c.session.items;
    let mut start = end - 1;
    while folds(&items[start]) && start > from && folds(&items[start - 1]) {
        start -= 1;
    }
    let node = match folds(&items[start]) {
        true => {
            let tools: Vec<&Tool> = items[start..end].iter().filter_map(|item| if let Item::Tool(tool) = item { Some(tool) } else { None }).collect();
            (!tools.is_empty()).then(|| steps(start, &tools, c))
        }
        false => item_node(start, &items[start], c, true),
    };
    let node = node.map(|node| crate::search::mark(c, start, end, node));
    let node = node.map(|node| crate::mods_view::placed_after(c, start, end, node));
    // A turn that changed files ends with a card of them, after its last item.
    let edited = c.session.turn_ending_at(end).map(|(from, turn)| crate::edits_view::card_of(c, from, end, &turn));
    (start, node.map(|node| Block { start, end, node, edited }))
}

/// The blocks the page shows of what ends before `end`, newest first: from there back to `from` until the page's budget is
/// spent, so a long session's other items stay out of it. Says whether the budget cut them short of `from`.
pub(crate) fn window(c: &Chat, end: usize, from: usize) -> (Vec<Block>, bool) {
    let (mut blocks, mut spent, mut at) = (Vec::new(), 0, end);
    while at > from {
        let (start, block) = block_before(c, at, from);
        at = start;
        let Some(block) = block else { continue };
        let size = block.size();
        if spent + size > BUDGET {
            // The newest item always shows, as plain text when it is too big to lay out.
            if blocks.is_empty() {
                blocks.push(Block { node: plain(start, &c.session.items[start]), edited: None, ..block });
            }
            return (blocks, true);
        }
        spent += size;
        blocks.push(block);
    }
    (blocks, false)
}

/// Where the conversation the page shows ends: at the newest item, or where an older part you are reading ends.
pub(crate) fn end_of(c: &Chat) -> usize {
    let len = c.session.items.len();
    c.until.map_or(len, |until| until.min(len))
}

/// The end of the blocks from `start` on that make about `size` nodes, at least one: what to show after `start`.
fn forward(c: &Chat, start: usize, size: usize) -> usize {
    let items = &c.session.items;
    let (mut at, mut spent) = (start, 0);
    while at < items.len() && spent < size {
        let mut end = at + 1;
        while folds(&items[at]) && end < items.len() && folds(&items[end]) {
            end += 1;
        }
        spent += block_before(c, end, at).1.map_or(0, |block| block.size());
        at = end;
    }
    at
}

/// Shows the conversation up to `end`, or to the newest item, from as far back as the page has room for.
fn show_until(c: &mut Chat, end: usize) {
    let len = c.session.items.len();
    c.until = (end < len).then_some(end);
    c.shown = end;
}

/// Scrolled to the top of what shows: older items come in, and where the page has no room for them, the newest of what
/// shows goes to make it. What shows ends then where the older half of it does, so what you read stays.
pub fn show_older(c: &mut Chat) {
    let end = end_of(c);
    let (shown, cut) = window(c, end, end.saturating_sub(c.shown));
    let Some(oldest) = shown.last().filter(|_| cut) else {
        c.shown += PAGE;
        return;
    };
    let (mut kept, mut until) = (0, end);
    for block in shown.iter().rev() {
        kept += block.size();
        until = block.end;
        if kept >= BUDGET / 2 {
            break;
        }
    }
    // What is kept is too big to leave room for anything older: what shows ends where it starts instead.
    if window(c, until, 0).0.last().is_none_or(|block| block.start >= oldest.start) {
        until = oldest.start;
    }
    show_until(c, until);
}

/// Scrolled to the end of an older part: newer items come in, half a page of them, and the oldest of what shows goes.
pub fn show_newer(c: &mut Chat) {
    let end = end_of(c);
    show_until(c, forward(c, end, BUDGET / 2));
}

/// Shows the newest items, as when the chat opened, and scrolls to them.
pub fn show_latest(c: &mut Chat) {
    c.until = None;
    c.shown = PAGE;
    crate::rail::to_end();
}

/// Brings the item at `item` (a message of yours, which starts a block) into what shows, with half a page after it.
pub fn show_item(c: &mut Chat, item: usize) {
    let end = end_of(c);
    let (shown, _) = window(c, end, end.saturating_sub(c.shown));
    if item < end && shown.last().is_some_and(|block| block.start <= item) {
        return;
    }
    let until = forward(c, item, BUDGET / 2);
    let shown = c.shown;
    show_until(c, until);
    if c.until.is_none() {
        c.shown = shown.max(c.session.items.len() - item);
    }
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
    let rewind = || own.then(|| crate::bookmarks::with_button(at, c, crate::fork_view::message_actions(at, c))).flatten();
    // What a mod made of a message of yours or Claude's: its own tree in its place, or its words reworded.
    let (drawn, reworded) = if own { crate::mods_view::message(c, at, item) } else { (None, None) };
    let said_now = |said: &str| reworded.clone().unwrap_or_else(|| said.to_owned());
    let node = match item {
        Item::User(_) if drawn.is_some() => bubble(card().maybe(drawn), rewind()),
        Item::User(said) => {
            let (typed, attached) = crate::terminals::typed(said);
            let note = (!attached.is_empty()).then(|| muted(format!("Attached the output of {}", attached.join(", "))).prop("size", "small"));
            bubble(card().child(text(reworded.clone().unwrap_or_else(|| typed.to_owned()))).maybe(note), rewind())
        }
        Item::UserImages(said, pictures) => {
            let card = card().maybe(thumbs(pictures, "Attached picture"));
            bubble(if said.is_empty() { card } else { card.child(text(said.as_str())) }, rewind())
        }
        Item::Images(pictures) => thumbs(pictures, "Picture").unwrap_or_else(stack),
        // A subagent's replies sit in its own list, which the Copy buttons' indexes don't name.
        Item::Text(_) if own && drawn.is_some() => drawn.unwrap_or_else(stack),
        Item::Text(said) if own => crate::status_view::reply(at, &said_now(said), c),
        // Words that name something in Wings, like an issue key, open it there.
        Item::Text(said) => crate::edits_view::prose(said, c),
        Item::Divider(said) => row().prop("justify", "center").child(muted(format!("── {said} ──")).prop("size", "small")),
        Item::Notice(said) => crate::blocked::notice(at, said, c),
        Item::Allowed(what, said) => crate::blocked::allowed_row(what, said),
        Item::Thought(said, ms) => thought(at, said, *ms, crate::folds::is_open(c, Kind::Thought, c.open_outputs.contains(&format!("thought-{at}")))),
        Item::Ask(ask) if ask.answer.is_none() => ask_node(ask, c),
        Item::Error(said) => notice("danger", said.as_str()),
        Item::Stopped => muted("Stopped"),
        Item::Worked(ms, spend) => {
            // The cost shows when an API key pays per token; on a subscription it is only in the hover.
            let (said, hover) = crate::spend::worked(&took(*ms), spend, c.session.meter.per_token);
            let line = muted(said).prop("size", "small");
            match hover {
                Some(hover) => line.prop("title", hover),
                None => line,
            }
        }
        Item::Note(title, body) => note(at, title, body, crate::folds::is_open(c, Kind::Note, c.open_outputs.contains(&format!("note-{at}")))),
        Item::Finished(done) => finished(at, done, c),
        Item::Tool(_) | Item::Ask(_) => return None,
    };
    Some(node.key(at.to_string()))
}

/// A run of tool calls as one line that says what they did, open only when you open it (or, in Detailed, until you shut
/// it): one that opened while a call ran and shut once it ended showed the calls for a moment and hid them again. The
/// line under the conversation says what runs. Closed, its calls aren't built at all.
pub(crate) fn steps(first: usize, tools: &[&Tool], c: &Chat) -> Node {
    let open = crate::folds::is_open(c, Kind::Steps, c.open_steps.contains(&first));
    let failed = tools.iter().filter(|tool| matches!(tool.output, Some((_, true)))).count();
    let mut said = describe(tools);
    if failed > 0 {
        said.push_str(&format!(" · {failed} failed"));
    }
    // With `outputs_open` each call's output shows unless its kind starts shut or you shut it, else only what you opened.
    let calls = |outputs_open: bool| {
        stack().prop("gap", "s").children(tools.iter().map(|tool| {
            let opened = prefs::is_open(outputs_open && c.opens.get(Kind::of_tool(&tool.name)), c.open_outputs.contains(&tool.id));
            step(tool, opened, c)
        }))
    };
    let rows = if open {
        let rows = calls(c.detailed);
        // Every output of a long run, even folded to its first lines, would take most of the page's nodes and push the rest
        // of the conversation out: its calls then show as lines, with the outputs for you to open.
        if c.detailed && rows.count() > BUDGET / 2 { calls(false) } else { rows }
    } else {
        stack().prop("gap", "s")
    };
    // A mod may draw the count line in place of the chat's.
    let fold = match crate::mods_view::group(c, first, tools) {
        Some(drawn) => Node::new("Disclosure").child(Node::new("Slot").prop("name", "summary").child(drawn)).child(Node::new("Slot").prop("name", "body").child(rows)),
        None => disclosure(said, rows),
    };
    fold.prop("open", open).on("onOpenChange", move |_| chat::toggle_steps(first)).key(format!("steps-{first}"))
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
            // A call that has no result yet still runs: "Running 2 commands" when all do, "Ran 1 command · 1 running" when some
            // are done, so the count never says running for one that has ended.
            let running = if *verb == "Ran" { tools.iter().filter(|tool| names.contains(&tool.name.as_str()) && tool.output.is_none()).count() } else { 0 };
            (n > 0).then(|| match (noun.is_empty(), running) {
                (true, _) => (*verb).to_owned(),
                (false, 0) => format!("{verb} {}", plural(n, noun)),
                (false, all) if all == n => format!("Running {}", plural(n, noun)),
                (false, some) => format!("Ran {} · {some} running", plural(n - some, noun)),
            })
        })
        .collect();
    let others = tools.iter().filter(|tool| !known(tool)).count();
    if others > 0 {
        parts.push(format!("Used {}", plural(others, "tool")));
    }
    parts.join(" · ")
}

/// One tool call: what it is, and what it did or printed, folded. The fold is built only once you open it.
fn step(tool: &Tool, opened: bool, c: &Chat) -> Node {
    let output = tool.output.as_ref().map(|(output, failed)| (output.as_str(), *failed)).filter(|(output, _)| !output.trim().is_empty());
    let mark = match &tool.output {
        None => lead(spinner()),
        // The mark of a failure is in the same 16 point box as the others, so the names line up; its pill follows the name.
        Some((_, true)) => lead(text("✗").prop("tone", "danger")),
        Some(_) => lead(text("✓").prop("tone", "success")),
    };
    let failure = tool.output.as_ref().filter(|(_, failed)| *failed).map(|(output, _)| badge(exit_code(output).map_or("Failed".to_owned(), |code| format!("Failed · exit {code}")), Some("danger")));
    // A subagent's description is a sentence, not a command or a path: plain text.
    let subagent = matches!(tool.name.as_str(), "Task" | "Agent");
    let head = row()
        .prop("gap", "s")
        .prop("align", "baseline")
        .child(mark)
        .child(text(tool.name.as_str()).prop("weight", "medium"))
        .maybe(failure)
        .child(text(tool.summary.as_str()).prop("mono", !subagent).prop("size", "small").prop("tone", "muted").prop("truncate", true).prop("grow", true))
        .children(facts(tool))
        .maybe(open_subagent(tool, c))
        .maybe(crate::plan_doc_view::step_link(c, tool))
        .maybe(crate::actions_view::background_button(tool, c));
    // A mod that draws the call's row draws it here instead, and its result under it.
    let head = crate::mods_view::tool_head(c, tool, head);
    let mod_result = crate::mods_view::tool_result(c, tool);
    // What an edit or a write printed is only "updated successfully", which the diff already says.
    let label = match (&tool.detail, output) {
        (_, Some((_, true))) => Some("Error"),
        // The checklist over the message box shows what these did; they print only that it changed.
        _ if matches!(tool.name.as_str(), "TodoWrite" | "TaskCreate" | "TaskUpdate") => None,
        (Detail::Diff(diff), _) if !diff.lines.is_empty() => Some("Changes"),
        (Detail::Write { head, .. }, _) if !head.is_empty() => Some("Content"),
        (Detail::Read(_), Some(_)) => Some("Content"),
        (_, Some(_)) => Some("Output"),
        (Detail::Command(command), None) if command.contains('\n') => Some("Command"),
        _ => None,
    };
    // A failed call shows the end of its error under its line, closed or not: that is what you open it for.
    let error = output.filter(|(_, failed)| *failed && !opened).map(|(said, _)| {
        let lines: Vec<&str> = said.trim_end().lines().collect();
        let tail = lines[lines.len().saturating_sub(4)..].join("\n");
        // Under the name, not under the mark: past the 16 point box and the gap after it.
        row().prop("gap", "s").child(stack().prop("width", 16)).child(text(truncate(&tail, 600)).prop("mono", true).prop("size", "small").prop("tone", "danger").prop("grow", true))
    });
    // What it gave back as pictures (a Read of an image, a screenshot) shows under its line, closed or not.
    let pictures = thumbs(&tool.images, "Picture from the tool");
    match label {
        Some(label) => {
            let id = tool.id.clone();
            let body = if opened { body(tool, output, crate::diff_view::look(c)) } else { stack() };
            let label = if error.is_some() { "Whole error" } else { label };
            let fold = disclosure(label, body).prop("open", opened).on("onOpenChange", move |_| chat::toggle_output(&id));
            stack().prop("gap", "xs").child(head).maybe(mod_result).maybe(error).maybe(pictures).child(fold)
        }
        None => match (pictures, mod_result) {
            (None, None) => head,
            (pictures, drawn) => stack().prop("gap", "xs").child(head).maybe(drawn).maybe(pictures),
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

/// What a call's fold holds: the diff, the start of the file, a command of several lines, and what it printed. A long block
/// shows its first lines with a row that opens the rest (`code_view.rs`), in the file's language where the call names a file.
fn body(tool: &Tool, output: Option<(&str, bool)>, look: crate::diff_view::Look) -> Node {
    let failed = output.is_some_and(|(_, failed)| failed);
    let language = code_view::language_of_path(&tool.summary);
    let mut body = stack().prop("gap", "s");
    match &tool.detail {
        Detail::Diff(diff) if !failed => body = body.child(crate::diff_view::finished(&format!("diff-{}", tool.id), language, diff, look)),
        Detail::Write { lines, head } if !failed => {
            let key = format!("write-{}", tool.id);
            body = body.child(code_view::block(&key, &head.join("\n"), language));
            if *lines > head.len() && (code_view::is_open(&key) || head.len() <= code_view::FOLD_LINES) {
                body = body.child(muted(format!("{} more", plural(lines - head.len(), "line"))));
            }
        }
        Detail::Command(command) if command.contains('\n') => body = body.child(code_view::block(&format!("cmd-{}", tool.id), command, "sh")),
        _ => {}
    }
    match output {
        Some((output, _)) if failed || !matches!(tool.detail, Detail::Diff(_) | Detail::Write { .. }) => {
            let key = format!("out-{}", tool.id);
            // A file read is the file: its language comes from its name. What a command or a search printed stays plain.
            match (&tool.detail, failed) {
                (Detail::Read(_), false) => body.child(code_view::block(&key, &code_view::unnumbered(output), language)),
                _ => body.child(code_view::block(&key, output, "")),
            }
        }
        _ => body,
    }
}

/// The most checklist lines shown at once.
const TODOS_SHOWN: usize = 12;

/// Claude's checklist, a raised card at the top of the message box's own column, so it keeps the gap the queue card and the
/// box have between them: how many are done, folded to that line when you fold it.
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
    Some(list.key("todos"))
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
        (!id.is_empty() && c.session.subagents.contains_key(&id)).then(|| button("Open", "outline", false, move || crate::tasks_view::open_subagent(&id)).prop("title", "Its transcript"))
    } else {
        let id = done.task.clone();
        c.session.tasks.iter().any(|task| task.id == id && task.has_output()).then(|| button("Open", "outline", false, move || chat::open_task(&id)).prop("title", "What it printed"))
    };
    let line = row().prop("gap", "s").prop("align", "center").child(mark).child(title).maybe(open.map(|open| open.prop("size", "sm")));
    let opened = crate::folds::is_open(c, Kind::Summary, c.open_outputs.contains(&format!("finished-{at}")));
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
        // What mods add to the dialog shows above it.
        "AskUserQuestion" if ask.input["questions"].as_array().is_some_and(|questions| !questions.is_empty()) => match crate::mods_view::question(c, &ask.request) {
            Some(added) => stack().prop("gap", "s").child(added).child(questions_node(ask, draft)),
            None => questions_node(ask, draft),
        },
        "ExitPlanMode" if ask.input["plan"].as_str().is_some() => plan_node(ask, draft, c),
        _ => permission_node(ask, draft, c),
    }
}

/// The text field for what Claude should hear along with a no.
fn feedback_field(request: &str, draft: &Draft, label: &str, placeholder: &str) -> Node {
    let request = request.to_owned();
    text_input(label, draft.feedback.as_str(), move |said| chat::edit(&request, |draft| draft.feedback = said)).prop("placeholder", placeholder)
}

fn permission_node(ask: &Ask, draft: &Draft, c: &Chat) -> Node {
    let (deny, once, always) = (ask.request.clone(), ask.request.clone(), ask.request.clone());
    let mut body: Vec<Node> = ask.reason.iter().map(|why| muted(why.as_str())).collect();
    // An edit or a write that was read against its file shows as a diff, with the lines round it.
    match crate::diff_view::ask_diff(ask) {
        Some(preview) => body.extend(crate::diff_view::ask_body(ask, preview, crate::diff_view::look(c))),
        None => body.push(text(ask.detail.as_str()).prop("mono", true).prop("size", "small")),
    }
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

/// Lucide's `circle` and `square` outlines (the same 18-point size in a 24-point box), with the dot a picked round mark
/// holds and the tick a picked box holds.
const RING: &str = "M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18z";
const BOX: &str = "M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z";
const DOT: &str = "M12 9a3 3 0 1 0 0 6a3 3 0 1 0 0-6z";
const TICK: &str = "m9 12 2 2 4-4";

/// The mark before an option: a ring for one answer and a box for several, the same size, filled in when picked.
fn choice_mark(multi: bool, picked: bool) -> Node {
    let shapes: Vec<&str> = match (multi, picked) {
        (false, false) => vec![RING],
        (false, true) => vec![RING, DOT],
        (true, false) => vec![BOX],
        (true, true) => vec![BOX, TICK],
    };
    icon(24, 2.0, &shapes).prop("size", 16).prop("tone", if picked { "info" } else { "muted" })
}

/// The questions with their options, and a field for another answer each. Several are asked one at a time, as Claude
/// Code's own menu does: Next once one is answered, and picking the one answer of a question moves on.
fn questions_node(ask: &Ask, draft: &Draft) -> Node {
    let questions = ask.input["questions"].as_array().map(Vec::as_slice).unwrap_or_default();
    let count = questions.len();
    let step = draft.step.min(count.saturating_sub(1));
    let last = step + 1 >= count;
    let mut body = Vec::new();
    for (q, question) in questions.iter().enumerate().filter(|(q, _)| count < 2 || *q == step) {
        let multi = question["multiSelect"] == true;
        let options = question["options"].as_array().map(Vec::as_slice).unwrap_or_default();
        // One row an option, as Claude Code's own menu lists them, marked when picked: a round mark for one answer, a box
        // for several.
        let chips = list().children(options.iter().enumerate().map(|(o, option)| {
            let request = ask.request.clone();
            let picked = draft.is_picked(q, o);
            let mark = choice_mark(multi, picked);
            let line = row()
                .prop("gap", "s")
                .prop("align", "center")
                .child(mark)
                .child(text(option["label"].as_str().unwrap_or_default()).prop("weight", "medium"))
                .maybe(option["description"].as_str().filter(|said| !said.is_empty()).map(|said| muted(truncate(said, 120)).prop("size", "small").prop("grow", true)));
            let advance = count > 1 && !multi && !last;
            list_button([line], move || {
                chat::edit(&request, |draft| {
                    draft.toggle(q, o, multi);
                    if advance && draft.has_answer(q) {
                        draft.step = q + 1;
                    }
                })
            })
            .key(format!("{q}-{o}"))
        }));
        let (request, sent) = (ask.request.clone(), ask.request.clone());
        // Enter goes on to the next question, or on the last sends the answers once every question has one.
        let other = text_input("Other", draft.other(q), move |said| chat::edit(&request, |draft| draft.set_other(q, said, multi)))
            .prop("placeholder", "Something else")
            .on("onKeyDown", move |event| {
                if event["key"] != "Enter" {
                    return;
                }
                if last {
                    chat::submit(&sent);
                } else {
                    chat::edit(&sent, |draft| {
                        if draft.has_answer(q) {
                            draft.step = q + 1;
                        }
                    });
                }
            })
            .key(format!("other-{q}"));
        let header = question["header"].as_str().filter(|header| !header.is_empty()).map(|header| badge(header, None));
        body.push(
            stack()
                .prop("gap", "xs")
                .key(format!("q-{q}"))
                .child(row().prop("gap", "s").prop("align", "start").maybe(header).child(text(question["question"].as_str().unwrap_or_default()).prop("weight", "medium").prop("grow", true)))
                .child(chips)
                .child(other),
        );
    }
    let ready = draft.answers(&ask.input["questions"]).is_some();
    let (skip, submit, back, next) = (ask.request.clone(), ask.request.clone(), ask.request.clone(), ask.request.clone());
    let title = if count > 1 { format!("Claude has {count} questions · {} of {count}", step + 1) } else { "Claude has a question".to_owned() };
    let mut actions = vec![button("Skip", "outline", false, move || chat::skip(&skip))];
    if count > 1 && step > 0 {
        actions.push(button("Back", "outline", false, move || chat::edit(&back, |draft| draft.step = step - 1)));
    }
    if last {
        actions.push(button("Submit", "default", false, move || chat::submit(&submit)).prop("disabled", !ready));
    } else {
        actions.push(button("Next", "default", false, move || chat::edit(&next, |draft| draft.step = step + 1)).prop("disabled", !draft.has_answer(step)));
    }
    confirm(title, body, actions)
}

/// The plan as Markdown, to approve, or to send back with what to change.
fn plan_node(ask: &Ask, draft: &Draft, c: &Chat) -> Node {
    let plan = ask.input["plan"].as_str().unwrap_or_default();
    let shown = markdown(plan);
    let shown = if shown.count() > BUDGET / 4 { text(truncate(plan, 20_000)) } else { shown };
    let (auto, approve, keep) = (ask.request.clone(), ask.request.clone(), ask.request.clone());
    let actions = row()
        .prop("gap", "s")
        .prop("wrap", true)
        .prop("justify", "end")
        .child(button(crate::plan_doc_view::revise_label(c), "outline", false, move || chat::keep_planning(&keep)))
        .child(button("Approve", "secondary", false, move || chat::approve_plan(&approve, "default")))
        .child(button("Approve and auto-accept edits", "default", false, move || chat::approve_plan(&auto, "acceptEdits")));
    card().prop("variant", "raised").child(
        stack()
            .prop("gap", "m")
            .child(row().prop("gap", "s").prop("align", "center").child(text("Claude's plan").prop("weight", "strong").prop("grow", true)).maybe(crate::plan_doc_view::card_link(c, plan)))
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
        .on("onChange", |event| {
            if !chat::typed(event["value"].as_str().unwrap_or_default()) {
                wings_plugin::ui::unchanged();
            }
        })
        .on("onPasteImage", |event| chat::add_image(event["mediaType"].as_str().unwrap_or_default(), event["data"].as_str().unwrap_or_default()))
        .on("onKeyDown", |event| {
            let shift = event["shiftKey"] == true;
            match event["key"].as_str() {
                Some("Enter") => chat::send(),
                Some("Tab") if shift => chat::cycle_mode(),
                Some("Tab") => chat::accept_suggestion(),
                Some("Escape") => chat::escape(),
                Some("ArrowUp") => {
                    if !chat::take_queued() {
                        chat::recall_older()
                    }
                }
                Some("ArrowDown") => chat::recall_newer(),
                _ => {}
            }
        });
    // One round button, as chat apps have: a square to stop while Claude works and nothing is typed, else an arrow to send.
    // What you send while Claude works waits in the queue.
    let typed = !c.draft.trim().is_empty() || !c.images.is_empty() || !c.review.is_empty();
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
    // A key to press, drawn as a keycap beside what it does, not a button that looks like one more control in the row.
    let tab = suggested.map(|_| {
        row()
            .prop("gap", "xs")
            .prop("align", "center")
            .child(badge("Tab", None).prop("title", "Use the suggested message"))
            .child(muted("to use it"))
            // A gap before the send button, which the hint butted against.
            .child(stack().prop("width", 6))
            .key("tab")
    });
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
    let message_box = card().prop("variant", "raised").maybe(review_chips(c)).maybe(pasted(c)).child(field).child(controls);
    // Reading an older part, the conversation ends at the bar in a line of text cut through: a hairline makes it an edge, and the
    // button over the message box has room round it.
    let older = c.until.is_some();
    Node::new("Bar").prop("edge", if older { "top" } else { "none" }).child(
        stack()
            .prop("gap", if older { "m" } else { "s" })
            .prop("maxWidth", COLUMN)
            .maybe(latest(c))
            .maybe(todo_card(c))
            .maybe(crate::mods_view::above_prompt(c))
            .maybe(queued(c))
            .maybe(crate::branch::panel(c))
            .child(message_box)
            .maybe(crate::mods_view::under_prompt(c)),
    )
}

/// While you read an older part of a long session, a button over the message box back to the newest.
fn latest(c: &Chat) -> Option<Node> {
    c.until.is_some().then(|| row().prop("justify", "center").child(button("Jump to the latest", "outline", false, || chat::with(show_latest)).prop("size", "sm")))
}

/// The comments left in Wings' diff viewer, above the message that will carry them: how many and on how many files, a chip
/// for each with its file and line and a cross to take it back, and one to take them all off.
fn review_chips(c: &Chat) -> Option<Node> {
    if c.review.is_empty() {
        return None;
    }
    let chips = c.review.iter().enumerate().map(|(at, comment)| {
        button("", "outline", false, move || chat::remove_review(at))
            .prop("size", "sm")
            .prop("label", format!("Remove the comment on {}", comment.place()))
            .prop("title", comment.body.chars().take(200).collect::<String>())
            .child(text(comment.chip()))
            .child(icon(12, 2.0, &[CLOSE]))
            .key(format!("review-{at}"))
    });
    let clear = button("Clear", "ghost", false, chat::clear_review).prop("size", "sm").prop("label", "Remove all the comments").key("review-clear");
    Some(row().prop("gap", "s").prop("wrap", true).prop("align", "center").child(muted(crate::review::summary(&c.review)).key("review-summary")).children(chips).child(clear))
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

/// What the collapsed panel says: the folder and branch, and what the working tree changed, as the open panel's first rows do.
fn rail_place(c: &Chat) -> Option<String> {
    match (c.folder.is_empty(), &c.branch) {
        (true, _) => None,
        (false, Some(branch)) => Some(format!("{} · {branch}", c.folder)),
        (false, None) => Some(c.folder.clone()),
    }
}

/// The floating pane's width for the pill, as wide as what it says (about 6.3 points a character at the small size, which is
/// an estimate: the pane has a width, not a size from its content), up to the panel's own width.
fn rail_width(c: &Chat) -> u32 {
    let place = rail_place(c).map_or("Session".len(), |place| place.chars().count());
    let changes = c.changes.map_or(0, |(added, removed)| format!("+{added} −{removed}").chars().count() + 1);
    let badge = if live_in_side(c) > 0 { 30 } else { 0 };
    // The pane's padding either side, the button's, the chevron and the gaps.
    (24 + 40 + 20 + (place as u32 + changes as u32) * 63 / 10 + badge).clamp(120, RAIL_MOST)
}

/// The collapsed panel: one pill, as tall as a header control, that says the folder, branch and changes and opens the panel
/// when pressed. Nothing when the panel has nothing to say.
fn rail(c: &Chat, no_sections: bool) -> Option<Node> {
    let live = live_in_side(c);
    if no_sections && live == 0 && c.terminals.is_empty() {
        return None;
    }
    let place = rail_place(c).unwrap_or_else(|| "Session".to_owned());
    let title = if live == 0 { "Expand the side panel".to_owned() } else { format!("Expand the side panel, {live} running") };
    let mut face = row().prop("gap", "s").prop("align", "center").child(text(place.as_str()).prop("size", "small").prop("tone", "muted").prop("truncate", true).prop("grow", true));
    if let Some((added, removed)) = c.changes {
        face = face.child(fact(format!("+{added}"), "success")).child(fact(format!("−{removed}"), "danger"));
    }
    if live > 0 {
        face = face.child(badge(live.to_string(), Some("info")));
    }
    face = face.child(icon(24, 2.0, &CHEVRON_LEFT).prop("size", 14));
    Some(button("", "outline", false, || chat::set_side_rail(false)).prop("size", "sm").prop("label", "Expand the side panel").prop("title", title.as_str()).child(face).key("side-rail"))
}

/// The panel on the right, quiet beside the conversation: a header with the button that collapses it, the folder and
/// branch, and the sections below, in a raised card, with the terminals Claude opened under it. Collapsed, it is a slim
/// rail with the button that opens it again and how many things run in it, and the terminals are left out. Nothing when
/// there is nothing to say yet.
fn side(c: &Chat) -> Vec<Node> {
    let sections = side_sections(c, true);
    if side_railed(c) {
        return rail(c, sections.is_empty()).into_iter().collect();
    }
    if sections.is_empty() {
        return terminals(c);
    }
    let collapse = icon_button_titled("Collapse the side panel", "Collapse the side panel to a rail", &CHEVRON_RIGHT, || chat::set_side_rail(true));
    let top = row().prop("align", "center").child(inset(muted("Session").prop("size", "small")).prop("grow", true)).child(collapse).key("side-collapse");
    // Taller than the room it has, the card keeps its edge and its sections scroll inside it.
    let summary = card().prop("variant", "raised").prop("scroll", true).child(stack().prop("gap", "m").child(top).children(sections));
    std::iter::once(summary).chain(terminals(c)).collect()
}

/// The session panel's sections, which the chat draws in its own card and tells Wings' sidebar (`session_side.rs`): the
/// folder and branch when `place` (the sidebar's header says them), what the working tree changed, how full the context is
/// and how much of your plan's limits is used, the background tasks, the folders you added, the pull requests and issues the
/// conversation names, the panes plugins opened, and the session's subagents, each of which opens in the sidebar. Those with
/// nothing to say are left out.
pub(crate) fn side_sections(c: &Chat, place: bool) -> Vec<Node> {
    let small = |said: String| text(said).prop("size", "small");
    let mut sections: Vec<Node> = Vec::new();
    let mut head = stack().prop("gap", "xs");
    let place = place.then(|| match (c.folder.is_empty(), &c.branch) {
        (true, _) => None,
        (false, Some(branch)) => Some(format!("{} · {branch}", c.folder)),
        (false, None) => Some(c.folder.clone()),
    });
    if let Some(Some(place)) = place {
        head = head.child(inset(small(place.clone()).prop("tone", "muted").prop("truncate", true).prop("title", place)));
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
        // Every change in the folder in the diff viewer, not only this turn's (the card's View changes).
        head = head.child(list_button([small("Review changes".into()).prop("grow", true)], chat::review_changes).prop("title", "Review all changes in the diff viewer"));
    }
    sections.push(head);
    sections.extend(crate::pull_view::section(c));
    sections.extend(crate::blocked::section(c));
    let mut limits = stack().prop("gap", "s");
    if let Some((used, most)) = c.session.context {
        limits = limits.child(meter_button("Context", used * 100 / most.max(1), (70, 90), None, Panel::Context));
    }
    // "Cache warm · 3m left", counted from the last request; the chat redraws it every 15 s while Wings is in front.
    if let Some(warm) = c.session.meter.cache(wings_plugin::host::now() as u64) {
        limits = limits.child(inset(muted(warm).prop("size", "small").prop("title", "The prompt cache keeps what Claude has read, so the next message is cheaper and quicker while it is warm")).key("cache"));
    }
    for (label, used, resets) in &c.session.limits {
        let resets = resets.map(|ms| match *label {
            "5h" => format!("resets {}", clock(ms)),
            _ => format!("resets {} {}", WEEKDAYS[local(ms as i64).weekday as usize % 7], clock(ms)),
        });
        limits = limits.child(meter_button(if *label == "5h" { "5-hour" } else { "Week" }, *used, (80, 95), resets, Panel::Usage));
    }
    // Folds as the sections below do; the folder, branch and changes above it stay.
    if limits.count() > 1 {
        sections.push(fold(c, "usage", "Usage", [limits]));
    }
    sections.extend(crate::bookmarks::section(c));
    sections.extend(crate::plan_doc_view::section(c));
    sections.extend(crate::tasks_view::section(c));
    sections.extend(crate::fork_view::folders(c));
    sections.extend(crate::refs_view::section(c));
    sections.extend(crate::mods_view::panes(c));
    sections.extend(subagents(c));
    // A stack with nothing in it takes room: leave out the sections that have nothing.
    sections.into_iter().filter(|section| section.count() > 1).collect()
}

/// How many of the terminals Claude opened the session panel shows, newest first, and how tall each is.
pub(crate) const TERMINALS_SHOWN: usize = 3;
pub(crate) const TERMINAL_HEIGHT: u32 = 220;

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
    // The gauge takes the row's width, as the HUD sidebar's do: left alone in a row it is as wide as its label.
    let gauge = gauge(label, percent as f64, (levels.0 as u32, levels.1 as u32), resets);
    list_button([stack().prop("grow", true).child(gauge)], move || chat::open_panel(panel)).prop("title", title).key(label)
}

/// A session panel section you can fold: `rows` under `title` (put the count in it, which shows when it is closed too). Its
/// state is kept per `key` for every chat and every restart (`prefs::panel_closed`), in the chat's panel and in Wings'
/// sidebar alike. Closed, it sends no rows. Wings before 0.2.60 draws it open.
pub(crate) fn fold(c: &Chat, key: &'static str, title: impl Into<String>, rows: impl IntoIterator<Item = Node>) -> Node {
    let open = !c.panel_closed.iter().any(|shut| shut == key);
    let rows: Vec<Node> = if open { rows.into_iter().collect() } else { Vec::new() };
    wings_plugin::ui::section_folding(title, None, open, move |open| chat::set_panel_open(key, open), rows).key(format!("fold-{key}"))
}

/// A section's heading, in the inset a list button has, so the heading and the rows under it share one left edge: a button
/// is drawn 7 points in from its container, a bare heading is not.
pub(crate) fn heading(title: impl Into<String>) -> Node {
    inset(Node::new("Heading").text(title))
}

/// `content` in the same inset as a row that is a list button, without being one: a button nothing presses.
pub(crate) fn inset(content: Node) -> Node {
    Node::new("ListButton").prop("disabled", true).child(content)
}

/// A row's lead icon (a spinner, a mark) in a box of one width, so the text after it starts at one x whichever it is.
pub(crate) fn lead(mark: Node) -> Node {
    stack().prop("width", 16).child(row().prop("justify", "center").prop("align", "center").child(mark))
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
    let earlier = (hidden > 0).then(|| entry(Entry { lead: None, title: muted(format!("{hidden} earlier")).prop("size", "small"), detail: None, trailing: None }));
    let rows = tasks[hidden..].iter().map(|tool| subagent_row(tool, c));
    Some(fold(c, "subagents", title, earlier.into_iter().chain(rows)))
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
    // "Haiku 5.5 · 12s", while it runs (the app counts the seconds up) and when it is done, in one small muted text.
    let lapse = match (running, sub.filter(|sub| sub.started > 0)) {
        // A resumed background agent counts from when it woke, not from its first run.
        (true, Some(sub)) => Some(Err(background.filter(|task| task.started > 0).map_or(sub.started, |task| task.started))),
        (false, Some(sub)) => Some(Ok(took(ran.unwrap_or(sub.last.saturating_sub(sub.started))))),
        (false, None) => ran.map(|ms| Ok(took(ms))),
        _ => None,
    };
    // What `task_progress` counted: "Haiku 5.5 · 7 tool calls · 12s".
    let calls = c.session.meter.agents.get(&tool.id).filter(|seen| seen.tool_uses > 0).map(|seen| plural(seen.tool_uses as usize, "tool call"));
    let model = match (model, calls) {
        (Some(model), Some(calls)) => Some(format!("{model} · {calls}")),
        (model, calls) => model.or(calls),
    };
    let facts = match (model, lapse) {
        (Some(model), Some(Ok(time))) => Some(muted(format!("{model} · {time}"))),
        (Some(model), Some(Err(since))) => Some(muted(model).prop("since", since)),
        (None, Some(Ok(time))) => Some(muted(time)),
        (None, Some(Err(since))) => Some(muted("Working").prop("since", since)),
        (Some(model), None) => Some(muted(model)),
        (None, None) => None,
    }
    .map(|facts| facts.prop("size", "small"));
    let line = entry(Entry { lead: Some(mark), title: text(name).prop("size", "small").prop("clamp", true).prop("title", name), detail: facts, trailing: None }).key(tool.id.as_str());
    match crate::subagent_relay::can_open(c, &tool.id) {
        true => {
            let id = tool.id.clone();
            line.on("onClick", move |_| crate::tasks_view::open_subagent(&id)).prop("title", "Its conversation, in the sidebar")
        }
        false => line,
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


/// The messages waiting for Claude to finish, each with Send now and a way to take it back.
fn queued(c: &Chat) -> Option<Node> {
    if c.session.queued.is_empty() {
        return None;
    }
    let rows = c.session.queued.iter().map(|queued| {
        let (now, remove) = (queued.uuid.clone(), queued.uuid.clone());
        // A Claude Code plugin's own command isn't read at Claude's next step: Claude Code holds it until the turn ends.
        let held = c.session.runs_after_turn(&queued.text);
        // A message from a subagent's tab says what you sent, not how Claude is asked to pass it on.
        let shown = crate::actions::relayed_note(&queued.text);
        let said_text = shown.as_deref().unwrap_or(queued.text.as_str());
        // The text takes the room the buttons leave and is cut short in it, so a long message keeps them in the card. The
        // app adds More only while it is cut short.
        let said = if queued.text.is_empty() {
            muted(plural(queued.images.len(), "image"))
        } else {
            text(said_text).prop("truncate", true).prop("expandable", true).prop("title", said_text)
        };
        // From the top: an expanded message is many lines, and More or Less, Send now and the cross sit on its first. The app
        // centres that line in a button's height, so the labels are level on one line too.
        row()
            .prop("gap", "xs")
            .prop("align", "start")
            .child(stack().prop("grow", true).child(said).maybe(held.then(|| muted(HELD_NOTE))))
            // Sent now, Claude Code still holds a plugin's command for about 5 s and stops Claude's turn for it.
            .maybe((!held).then(|| button("Send now", "ghost", false, move || chat::send_now(&now)).prop("size", "sm").prop("title", "Claude puts what it runs aside and reads it now")))
            .child(
                button("", "ghost", false, move || chat::unqueue(&remove))
                    .prop("size", "icon-sm")
                    .prop("label", "Remove")
                    .prop("title", "Remove")
                    .child(icon(14, 2.0, &[CLOSE])),
            )
            .key(queued.uuid.as_str())
    });
    // The card's line is about the messages Claude reads at its next step, so it goes when only held commands wait.
    let read_soon = c.session.queued.iter().filter(|queued| !c.session.runs_after_turn(&queued.text)).count();
    Some(card().maybe((read_soon > 0).then(|| muted(queued_note(read_soon)))).child(stack().prop("gap", "xs").children(rows)))
}

/// What a queued Claude Code plugin command says under its text.
const HELD_NOTE: &str = "Runs when Claude finishes";

/// The queue card's line: one message is read, several are read together.
fn queued_note(count: usize) -> &'static str {
    if count == 1 { "Claude reads it after its current step" } else { "Claude reads them after its current step" }
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
    pickers.extend(remote_button(c));
    pickers.extend(crate::branch::button_node(c));
    pickers.extend(crate::pull_view::create_button(c));
    pickers.extend(crate::worktree::button_node(c));
    pickers
}

/// Lucide's `smartphone`, on the Remote Control button.
const PHONE: [&str; 2] = ["M7 2h10a2 2 0 0 1 2 2v16a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2z", "M12 18h.01"];

/// Remote Control, once Claude Code runs: a quiet button that turns it on, and while it is on a lit one with a menu to open
/// the chat on claude.ai, copy its link or turn it off, as Claude Code's own indicator does.
fn remote_button(c: &Chat) -> Option<Node> {
    use crate::remote::Remote;
    c.session.id.as_ref()?;
    let glyph = || icon(24, 2.0, &PHONE).prop("size", 14);
    Some(match &c.session.remote {
        Remote::Off => button("", "ghost", false, chat::toggle_remote)
            .prop("size", "icon-sm")
            .prop("label", "Remote Control")
            .prop("title", "Remote Control: go on with this chat from claude.ai or the Claude app")
            .child(glyph()),
        Remote::Connecting | Remote::Disconnecting(_) => button("Remote Control", "ghost", true, || {}).prop("size", "sm"),
        Remote::On(url) => {
            let trigger = Node::new("Button")
                .prop("variant", "secondary")
                .prop("size", "sm")
                .prop("label", "Remote Control")
                .prop("title", "Remote Control is on: this chat is on claude.ai and in the Claude app")
                .child(glyph())
                .child(text("Remote"));
            let (open, copied) = (url.clone(), url.clone());
            let items = vec![
                Node::new("Button").key("open").on("onClick", move |_| wings_plugin::host::open_url(&open)).text("Open on claude.ai"),
                Node::new("Button").key("copy").on("onClick", move |_| drop(wings_plugin::host::call("copyText", serde_json::json!({ "text": copied })))).text("Copy link"),
                Node::new("Button").key("off").on("onClick", |_| chat::toggle_remote()).text("Turn off Remote Control"),
            ];
            Node::new("Menu").child(trigger).children(items)
        }
    })
}

/// Lucide's `circle-user-round`, on the header's account button.
const ACCOUNT: [&str; 3] = ["M18 20a6 6 0 0 0-12 0", "M8 10a4 4 0 1 0 8 0a4 4 0 1 0-8 0", "M2 12a10 10 0 1 0 20 0a10 10 0 1 0-20 0"];

/// An account as the menu names it: by its email, else its name.
fn account_label(account: &wings_plugin::host::AccountChoice) -> String {
    match (&account.email, &account.name) {
        (Some(email), _) => email.clone(),
        (None, Some(name)) => name.clone(),
        (None, None) => "Default account".to_owned(),
    }
}

/// The header's account button, with 2 or more accounts: the one the chat runs under by its email, and a menu of them
/// all with that one checked. Picking another starts a new chat under it.
fn account_menu(c: &Chat) -> Option<Node> {
    if c.accounts.len() < 2 {
        return None;
    }
    let current = c.accounts.iter().find(|account| account.current);
    let shown = current.map_or_else(|| "Project's account".to_owned(), account_label);
    let title = format!("Claude account: {shown}. Pick another to start a new chat under it");
    let glyph = icon(24, 2.0, &ACCOUNT).prop("size", 16);
    let trigger = Node::new("Button").prop("variant", "ghost").prop("label", "Claude account").prop("title", title.as_str());
    let trigger = trigger.prop("size", "sm").child(glyph).child(text(shown.as_str()).prop("truncate", true));
    let items = c.accounts.iter().map(|account| {
        let label = account_label(account);
        let value = account.name.clone().unwrap_or_else(|| chat::DEFAULT_ACCOUNT.to_owned());
        let picked = account.current;
        // The one in use closes the menu and changes nothing.
        Node::new("Button").key(value.clone()).prop("pressed", picked).on("onClick", move |_| if !picked { chat::pick_account(&value) }).text(label)
    });
    Some(Node::new("Menu").prop("align", "end").child(trigger).children(items.collect::<Vec<_>>()))
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
/// `@word` (and the terminals that open), else the issues that match a `#word`, as a combobox's options, each a name with what it is beside it.
fn suggestions(c: &Chat) -> Option<Node> {
    let option = |key: &str, label: String, detail: &str, pick: Box<dyn Fn()>| {
        let chip = chip(label, false, move || pick()).key(key);
        if detail.is_empty() { chip } else { chip.child(muted(truncate(detail, 100)).prop("size", "small")) }
    };
    let commands = chat::command_matches(c);
    let files = chat::mention_matches(c).map(|(_, found)| found).unwrap_or_default();
    let terminals = chat::terminal_matches(c);
    let options: Vec<Node> = if !commands.is_empty() {
        commands
            .into_iter()
            .map(|command| {
                let name = command.name.clone();
                option(&command.name, format!("/{}", command.name), &command.description, Box::new(move || chat::insert_command(&name)))
            })
            .collect()
    } else if !files.is_empty() || !terminals.is_empty() {
        // A folder ends in `/`: its name is the last part before that, with its parent beside it.
        let paths = files.into_iter().map(|path| {
            let (dir, name) = path.trim_end_matches('/').rsplit_once('/').unwrap_or(("", path.trim_end_matches('/')));
            let name = if path.ends_with('/') { format!("{name}/") } else { name.to_owned() };
            let picked = path.to_owned();
            option(path, name, dir, Box::new(move || chat::insert_mention(&picked)))
        });
        let panes = terminals.into_iter().map(|pane| {
            let picked = crate::terminals::mention(&pane);
            let key = picked.clone();
            option(&key, picked.clone(), &pane.detail, Box::new(move || chat::insert_mention(&picked)))
        });
        paths.chain(panes).collect()
    } else if let Some((_, found)) = chat::issue_matches(c) {
        // The key or number, with the title and the status muted beside it.
        found
            .into_iter()
            .map(|issue| {
                let picked = issue.id.clone();
                option(&issue.id, issue.id.clone(), &format!("{} · {}", issue.title, issue.status), Box::new(move || chat::insert_issue(&picked)))
            })
            .collect()
    } else if !c.draft.starts_with('/') && c.issues.loading() && crate::issues::active(&c.draft).is_some() {
        // Nothing to pick until the lists arrive.
        vec![muted("Loading issues…").prop("size", "small").key("loading")]
    } else {
        return None;
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

    /// Where what the page shows of the conversation starts and ends, as items.
    fn shows(c: &Chat) -> (usize, usize) {
        let end = end_of(c);
        let (window, _) = window(c, end, end.saturating_sub(c.shown));
        (window.last().map_or(end, |block| block.start), end)
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
    fn a_fold_of_calls_still_running_says_running_not_ran() {
        let done = call("a", "Bash", Detail::Plain, "ok");
        let mut pending = call("b", "Bash", Detail::Plain, "");
        pending.output = None;
        assert_eq!(describe(&[&done]), "Ran 1 command");
        assert_eq!(describe(&[&pending]), "Running 1 command");
        assert_eq!(describe(&[&done, &pending]), "Ran 1 command · 1 running");
    }

    #[test]
    fn a_queued_plugin_command_says_it_runs_when_claude_finishes_and_a_message_does_not() {
        let shown = |texts: &[&str]| {
            chat::with(|c| {
                c.session.queued.clear();
                c.session.commands = crate::session::Session::default().commands;
                c.session.commands.push(crate::session::Command { name: "toolbox:lint".into(), description: String::new(), hint: String::new(), plugin: true });
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

    fn page_text(c: &Chat) -> String {
        wings_plugin::ui::tree_json(chat_page(c)).to_string()
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
    fn in_the_chat_the_panel_floats_over_the_conversation_with_the_place_and_the_terminals() {
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
            assert!(page.contains("shop · main") && page.contains("Open Changes") && page.contains("Context") && page.contains("\"Terminal\""));
            assert!(page.contains("raised"), "the card");
            // Put away, the panel and its terminals leave the page, and the conversation stays in the split.
            c.side_open = false;
            let page = page_text(c);
            assert!(page.contains("\"Split\"") && !page.contains("Open Changes") && !page.contains("\"Terminal\""));
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
            assert!(open.contains("Collapse the side panel") && open.contains("Open Changes") && open.contains("\"Terminal\""), "{open}");
            assert!(!open.contains("Expand the side panel"));
        });
        chat::set_side_rail(true);
        chat::with(|c| {
            let rail = page_text(c);
            assert!(rail.contains("Expand the side panel") && !rail.contains("Open Changes") && !rail.contains("Context") && !rail.contains("\"Terminal\""), "{rail}");
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
    /// beside the column reaches down beside it, as before.
    #[test]
    fn over_a_narrow_tab_the_panel_ends_above_the_message_box() {
        wings_plugin::host::testing::answer(|_, _| Ok(serde_json::Value::Null));
        fn split_of(tree: &serde_json::Value) -> Option<&serde_json::Value> {
            if tree["type"] == "Split" {
                return Some(tree);
            }
            tree["children"].as_array()?.iter().find_map(split_of)
        }
        chat::with(|c| {
            c.side_in_wings = false;
            c.side_open = true;
            for (narrow, in_split) in [(Some(true), false), (Some(false), true), (None, true)] {
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
                Item::Tool(crate::session::Tool { id: "t".into(), name: "Read".into(), summary: String::new(), output: None, detail: crate::session::Detail::Plain, images: Vec::new() }),
                Item::User("two".into()),
                Item::Text("reply".into()),
            ];
            c.forked_from = Some("Fix the build".into());
        });
        let (_, found, _) = chat::with(|c| conversation(c));
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

    fn call(id: &str, name: &str, detail: Detail, output: &str) -> Tool {
        Tool { id: id.into(), name: name.into(), summary: "src/a.ts".into(), output: Some((output.into(), false)), detail, images: Vec::new() }
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
            let menu = tree_json(account_menu(c).expect("a menu with two accounts"));
            let items: Vec<(&serde_json::Value, &serde_json::Value)> = menu["children"].as_array().unwrap()[1..].iter().map(|item| (&item["children"][0]["text"], &item["props"]["pressed"])).collect();
            assert_eq!(items, [(&serde_json::json!("me@home.example"), &serde_json::json!(false)), (&serde_json::json!("work"), &serde_json::json!(true))]);
            // The button reads the one in use.
            assert!(menu["children"][0]["props"]["title"].as_str().unwrap().contains("work"));
            // Before Wings says which, it is the project's.
            c.accounts[1].current = false;
            assert!(tree_json(account_menu(c).unwrap())["children"][0]["props"]["title"].as_str().unwrap().contains("Project's account"));
            // One account, nothing to pick.
            c.accounts.truncate(1);
            assert!(account_menu(c).is_none());
            c.accounts.clear();
        });
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
}

#[cfg(test)]
mod choice_mark_tests {
    use super::*;

    #[test]
    fn a_round_mark_and_a_box_are_drawn_the_same_size_and_filled_when_picked() {
        let drawn = |multi, picked| wings_plugin::ui::tree_json(choice_mark(multi, picked));
        for (multi, picked) in [(false, false), (false, true), (true, false), (true, true)] {
            let mark = drawn(multi, picked);
            assert_eq!(mark["type"], "Svg");
            assert_eq!(mark["props"]["size"], 16);
            assert_eq!(mark["props"]["tone"], if picked { "info" } else { "muted" });
            assert_eq!(mark["props"]["shapes"].as_array().unwrap().len(), if picked { 2 } else { 1 });
        }
    }
}
