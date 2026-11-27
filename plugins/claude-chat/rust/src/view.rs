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
        Entry, Node, badge, button, card, chip, confirm, disclosure, empty, entry, gauge, icon, icon_button, icons, list, list_button, loading, muted,
        notice, row, select, slot, spinner, stack, text, text_input,
    },
};

use crate::{
    chat::{self, Chat, PAGE, State},
    code_view,
    folds::Kind,
    pictures_view::thumbs,
    prefs,
    session::{Ask, Detail, Diff, Draft, Finished, Item, MODES, Progress, Report, Session, Task, Todo, Tool, exit_code, files_found, model_name},
    usage::Panel,
};

/// An arrow up, a square and a cross, in a 14-point box.
pub(crate) const SEND: &str = "M7 12V2M2.5 6.5 7 2l4.5 4.5";
/// An arrow down, in a 14-point box.
const DOWN: &str = "M7 2v10M2.5 7.5 7 12l4.5-4.5";
const STOP: &str = "M3.5 3.5h7v7h-7z";
const CLOSE: &str = "M3.5 3.5l7 7M10.5 3.5l-7 7";
/// A paperclip, in the same box.
const ATTACH: &str = "M12.5 6.45 7.15 11.8a3.5 3.5 0 0 1-4.95-4.95l5-5a2.33 2.33 0 0 1 3.3 3.3l-5 5a1.17 1.17 0 0 1-1.65-1.65l4.95-4.95";

/// The conversation's column, as wide as a page meant for reading.
pub(crate) const COLUMN: u32 = 760;
/// The most nodes the conversation sends. The app draws a page of at most 5,000, and a long session's whole history
/// made more: the page was refused and stayed on Loading. The toolbar and the message box take the rest.
pub(crate) const BUDGET: usize = 3_500;

/// The most nodes the whole page may make, under the 5,000 Wings draws: what the conversation leaves to the rest of the page
/// and to what a step adds before the next build.
const PAGE_NODES: usize = 4_800;

/// The least the conversation keeps, whatever the rest of the page takes.
const BUDGET_LEAST: usize = 600;

/// The most subagents the side panel lists, the newest, as it sends a row for each.
const SUBAGENTS_LISTED: usize = 60;

/// How many nodes the conversation may make: `BUDGET`, less what the rest of the page (the header, the message box and the side
/// panel, whose lists grow with the session) and the reply being written take from the page's 5,000.
fn budget(c: &Chat) -> usize {
    let side: usize = if side_shown(c) { side(c).iter().map(Node::count).sum() } else { 0 };
    let outside = header(c).count() + composer(c).count() + side + BUDGET / 4;
    PAGE_NODES.saturating_sub(outside).clamp(BUDGET_LEAST, BUDGET)
}
/// The side panel's width, while it is in the chat. It floats over the right of the tab and takes no width from the
/// conversation, which stays centered in the whole tab. The column is 760 wide, drawn with 8 points of margin either side,
/// so the panel sits beside it while the tab holds that and the panel (with its 12 points of padding) either side of the
/// centre; narrower, it covers the conversation's right side down to the message box, and Escape or a press outside puts
/// it away.
const SIDE: u32 = 340;
const SIDE_BELOW: u32 = COLUMN + 16 + 2 * (SIDE + 12);
/// In a narrower tab, the open panel moves the conversation and the message box left, only as far as keeps the column
/// (with its 8 points of margin either side) clear of the panel: they went under it, cutting off long lines, code and
/// errors. That needs a tab that holds a column of at least 480 points and the panel; with a prompt rail, one that keeps
/// the rail's room beside the whole column once it is moved (the app's 64-point rail and 28-point gap either side, and the
/// app's 8-point steps), so moving never takes the rail away.
const INSET_COLUMN: u32 = COLUMN + 16;
const INSET_BELOW: u32 = 480 + SIDE;
const INSET_BELOW_RAIL: u32 = (INSET_COLUMN + 2 * SIDE + COLUMN + 2 * (64 + 28) + 8) / 2;
/// The widest the collapsed pill gets: the panel's own width. Past it the project and branch are cut.
const RAIL_MOST: u32 = SIDE;
/// The most subagents the session panel lists.
/// How tall a panel section's list is at most, in points: about eight of a pull request's 34-point rows. Past it the list scrolls inside.
pub(crate) const LIST_HEIGHT: u32 = 280;
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

pub(crate) fn chat_page(c: &Chat) -> Node {
    let (body, built, last) = conversation(c);
    // The conversation, the checklist and the message box. Flush, so the conversation scrolls on its own and the checklist
    // and the message box stay at the foot.
    // The prompt rail sits over them against the column's left, where the app has room for it.
    let rail = crate::rail::Rail::of(&c.session.items, &built, crate::rail::state_at());
    let body = crate::rail::watch(body, &rail, last);
    // The rail goes down to the checklist and the message box, not past them: it is laid over the conversation alone.
    let marks = crate::rail::marks(c, &rail);
    let inset_below = if marks.is_empty() { INSET_BELOW } else { INSET_BELOW_RAIL };
    let conversation = stack().prop("fill", true).prop("gap", "none").children(marks).child(body).maybe(latest(c));
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
            .on("onNarrow", |payload| chat::set_narrow(payload["narrow"].as_bool().unwrap_or(false), payload["inset"].as_bool().unwrap_or(false)));
        // An open panel in a narrow tab sets the conversation in beside it where there's room for both.
        let set_in = c.narrow == Some(true) && side_shown(c) && !side_railed(c);
        let split = if set_in { split.prop("insetBelow", inset_below).prop("insetColumn", INSET_COLUMN) } else { split };
        // Where the panel covers the conversation, in a narrow tab, the message box is under the split, not in it: the
        // panel ends above the box and scrolls when it is taller than that, so it never covers what you type, nor the
        // box's buttons. In a wide tab, or set in beside it, the panel sits beside the column and reaches down beside the box.
        // Until the page hears the width (after its first draw) the box is drawn where a narrow tab has it, as most tabs are
        // narrow: the app keeps what you type by the box's place in the tree, so one that moved when the width came lost the
        // text typed before.
        let (start, under) = match c.narrow != Some(false) && !(set_in && c.inset) {
            true => (vec![conversation], Some(composer(c))),
            false => (vec![conversation, composer(c)], None),
        };
        let split = split
            .child(Node::new("Slot").prop("name", "start").children(start))
            .child(Node::new("Slot").prop("name", "end").children(if side_shown(c) { side(c) } else { Vec::new() }));
        // The terminals Claude opened dock beside all of it: the conversation, the panel and the message box. The place of what
        // is in the outer split's start slot never changes with the terminals, so the message box keeps its id: the split is
        // there with none open, and the one slot after the split is where a narrow tab sets the newest terminal.
        let width = dock_width_of(c.dock_width);
        let (beside, below) = match c.dock_narrow {
            Some(false) => (dock_terminals(c, true), Vec::new()),
            _ => (Vec::new(), dock_terminals(c, false).into_iter().take(1).collect()),
        };
        let start: Vec<Node> = [split, stack().prop("gap", "none").children(below)].into_iter().chain(under).collect();
        let dock = Node::new("Split")
            .prop("defaultSize", width)
            .prop("side", "end")
            .prop("dock", true)
            .prop("collapseBelow", DOCK_REST + width)
            // The edge is dragged by the app, which tells the page once, when it is let go; the column keeps at least
            // `DOCK_MIN` and the conversation `DOCK_REST` while it moves, and a double press brings back `DOCK`.
            .prop("minSize", DOCK_MIN)
            .prop("minRest", DOCK_REST)
            .prop("resetSize", DOCK)
            .on("onResize", |payload| chat::set_dock_width(payload["size"].as_f64().unwrap_or(f64::from(DOCK)) as u32))
            .on("onNarrow", |payload| chat::set_dock_narrow(payload["narrow"].as_bool().unwrap_or(true)))
            .child(Node::new("Slot").prop("name", "start").children(start))
            .child(Node::new("Slot").prop("name", "end").children(beside));
        vec![dock]
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
        .maybe(crate::skills_view::dialog(c))
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

/// Lucide's `zap`, on the fast mode button.
const FAST: [&str; 1] = ["M4 14a1 1 0 0 1-.78-1.63l9.9-10.2a.5.5 0 0 1 .86.46l-1.92 6.02A1 1 0 0 0 13 10h7a1 1 0 0 1 .78 1.63l-9.9 10.2a.5.5 0 0 1-.86-.46l1.92-6.02A1 1 0 0 0 11 14z"];

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
    let subagents = c.session.items.iter().filter(|item| matches!(item, Item::Tool(tool) if matches!(tool.name.as_str(), "Task" | "Agent") && c.session.runs(tool))).count();
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
    let glyph = icon(24, 2.0, &icons::PANEL_RIGHT).prop("size", 16);
    let button = button("", if shown { "secondary" } else { "ghost" }, false, chat::toggle_side).prop("label", "Side panel").prop("title", title.as_str()).shortcut("cmd-alt-b");
    Some(match live {
        0 => button.prop("size", "icon-sm").child(glyph),
        // The count is a small badge beside the icon, with room between them.
        n => button.prop("size", "sm").child(row().prop("gap", "s").prop("align", "center").child(glyph).child(badge(n.to_string(), Some("info")))),
    })
}

/// Whether the header keeps to icons, with no version and no account name: in a narrow tab, and with the panel in Wings'
/// sidebar, where the page has no split to say how wide its tab is, and a 1000-point tab cut the title and folder to "…".
fn header_tight(c: &Chat) -> bool {
    c.narrow == Some(true) || c.side_in_wings
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
        .maybe(c.agent_type.as_ref().map(|agent| badge(format!("@{agent}"), Some("info")).prop("title", "Claude Code runs as this agent. Changing it restarts the chat, from the picker before the first message")))
        .maybe((!c.folder.is_empty()).then(|| {
            let place = c.branch.as_ref().map_or_else(|| c.folder.clone(), |branch| format!("{} · {branch}", c.folder));
            muted(place.as_str()).prop("size", "small").prop("truncate", true).prop("title", place.as_str())
        }));
    let tight = header_tight(c);
    // Three groups, a hairline between them: how Claude works (the mode), this session (search, Remote Control, the panel),
    // and the page (past sessions, a new chat, and a menu of what is rarely needed). Narrow, search and Remote Control fold into the menu too,
    // so the session's name keeps its room.
    let actions = row()
        .prop("gap", "xs")
        .prop("align", "center")
        .maybe(crate::claude_version::pill(tight))
        .child(crate::view_mode::mode_menu(c))
        .child(hairline())
        .maybe((!tight).then(|| crate::search::header_button(c)))
        .maybe(remote_button(c, tight))
        .maybe(side_button(c))
        .maybe(account_menu(c))
        .child(hairline())
        .child(icon_button("Past sessions", &HISTORY, chat::open_past).shortcut("cmd-y"))
        .child(icon_button("New chat", &icons::PLUS, chat::new_chat).tooltip(if chat::in_use(c) { "New chat in a new tab, this one keeps running" } else { "New chat" }).shortcut("cmd-shift-n"))
        .child(more_menu(c, tight));
    Node::new("Bar").prop("variant", "slim").child(row().prop("gap", "m").prop("align", "center").child(title).child(actions))
}

/// A thin vertical line between two groups of the header's buttons.
fn hairline() -> Node {
    Node::new("Svg")
        .prop("viewBox", "0 0 10 10")
        .prop("size", 10)
        .prop("tone", "muted")
        .prop("fill", "none")
        .prop("stroke", "currentColor")
        .prop("strokeWidth", 1)
        .prop("shapes", vec![serde_json::json!(["line", { "x1": 5, "y1": 0, "x2": 5, "y2": 10, "opacity": 0.5 }])])
}

/// A row of a menu: a button named by its text.
pub(crate) fn menu_item(label: &str, on_click: impl Fn() + 'static) -> Node {
    Node::new("Button").key(label).on("onClick", move |_| on_click()).text(label)
}

/// The header's "…" menu: what the session and the page rarely need. In a narrow tab it also holds search and Remote Control,
/// the buttons that would take the title's room.
fn more_menu(c: &Chat, tight: bool) -> Node {
    let trigger = Node::new("Button").prop("variant", "ghost").prop("size", "icon-sm").prop("label", "More").prop("title", "More: rename, fork and settings").child(icon(24, 2.0, &ELLIPSIS).prop("size", 16));
    let items: Vec<Node> = [
        tight.then(|| crate::search::menu_item(c)),
        tight.then(|| remote_item(c)).flatten(),
        crate::titles_view::menu_item(c),
        crate::fork_view::menu_item(c),
    ]
    .into_iter()
    .flatten()
    .chain([menu_item("Chat settings", chat::open_settings)])
    .collect();
    wings_plugin::ui::menu(trigger, items, true)
}

/// Said once under a new chat's prompt: the moving background can be turned plain in Settings.
fn background_hint() -> Node {
    row()
        .prop("gap", "s")
        .prop("align", "center")
        .prop("wrap", true)
        .child(muted("Prefer a plain background? Change it in Settings, top right."))
        .child(button("Open Settings", "outline", false, chat::open_settings_from_hint).prop("size", "sm"))
        .child(button("Got it", "ghost", false, chat::dismiss_background_hint).prop("size", "sm"))
        .key("background-hint")
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
            State::Failed(why) => crate::signin::failed_line(why, c),
            // A new chat's prompt sits in the middle of the conversation, over a soft glow. Needs Wings 0.2.58. Over the drift
            // of pixels or a gradient it goes without, so one backdrop moves behind it, not two.
            _ => {
                let new = empty("What should Claude do?", "It works in this folder with your Claude Code setup, and asks here before it does anything you haven't allowed.");
                let new = new.prop("fill", true);
                let new = if c.background_hint && c.backdrop.is_some() { new.child(slot("children", background_hint())) } else { new };
                let new = if c.backdrop.is_some() { new } else { new.prop("backdrop", "glow") };
                return (new, Vec::new(), 0);
            }
        };
        // The same empty first child as the conversation has: its gap keeps a failure's banner clear of the header.
        return (Node::new("Scroll").child(column().child(stack().key("top")).child(body)), Vec::new(), 0);
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
    // An empty first child, so the column's gap sits above the first message: the slim header has no room under it, and
    // the scroll keeps none at its top, so the message touched the header.
    let top = stack().key("top");
    let lead = 1 + usize::from(forked.is_some());
    let prompts: Vec<(usize, usize)> = prompts.into_iter().rev().map(|(item, back)| (item, lead + built - 1 - back)).collect();
    crate::rail::place(placed.into_iter().map(|(start, end, back)| (start, end, lead + built - 1 - back)).collect());
    let mut nodes: Vec<Node> = std::iter::once(top).chain(forked).chain(blocks).collect();
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
    nodes.extend(crate::signin::status(c));
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
        State::Failed(why) => nodes.push(crate::signin::failed_line(why, c).key("ended")),
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
    let budget = budget(c);
    while at > from {
        let (start, block) = block_before(c, at, from);
        at = start;
        let Some(block) = block else { continue };
        let size = block.size();
        if spent + size > budget {
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
        Item::Error(said) => crate::signin::error_line(said, at + 1 == c.session.items.len(), c),
        Item::Shell(tool) => step(tool, c.open_outputs.contains(&tool.id), c),
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
        Item::Report(report) => report_node(at, report, c),
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
    let mut said = describe(tools, &c.session);
    if failed > 0 {
        said.push_str(&format!(" · {failed} failed"));
    }
    let cut = tools.iter().filter(|tool| c.session.was_interrupted(tool)).count();
    if cut > 0 {
        said.push_str(&format!(" · {cut} interrupted"));
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
fn describe(tools: &[&Tool], session: &Session) -> String {
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
            let running = if *verb == "Ran" { tools.iter().filter(|tool| names.contains(&tool.name.as_str()) && session.runs(tool)).count() } else { 0 };
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
        None if c.session.was_interrupted(tool) => lead(text("–").prop("tone", "muted")).prop("title", "Stopped when Claude Code restarted"),
        None => lead(spinner()),
        // The mark of a failure is in the same 16 point box as the others, so the names line up; its pill follows the name.
        Some((_, true)) => lead(text("✗").prop("tone", "danger")),
        Some(_) => lead(text("✓").prop("tone", "success")),
    };
    let failure = tool.output.as_ref().filter(|(_, failed)| *failed).map(|(output, _)| badge(exit_code(output).map_or("Failed".to_owned(), |code| format!("Failed · exit {code}")), Some("danger")));
    // A subagent's description is a sentence, not a command or a path: plain text.
    let subagent = matches!(tool.name.as_str(), "Task" | "Agent");
    let shell = tool.name == "Shell";
    let head = row()
        .prop("gap", "s")
        .prop("align", "baseline")
        .child(mark)
        .maybe((!shell).then(|| text(tool.name.as_str()).prop("weight", "medium")))
        .maybe(failure)
        .child(
            text(tool.summary.as_str())
                .prop("mono", !subagent)
                .prop("size", "small")
                .prop("tone", "muted")
                .prop("truncate", true)
                .prop("grow", true)
                // The line is cut at its first break and its width: the hover has all of it.
                .prop("title", if tool.input.whole.is_empty() { tool.summary.as_str() } else { tool.input.whole.as_str() }),
        )
        .children(facts(tool))
        .maybe(call_note(tool, c))
        .maybe(open_subagent(tool, c))
        .maybe(open_terminal(tool))
        .maybe(crate::plan_doc_view::step_link(c, tool))
        .maybe(crate::actions_view::background_button(tool, c));
    // A mod that draws the call's row draws it here instead, and its result under it.
    let head = crate::mods_view::tool_head(c, tool, head);
    let mod_result = crate::mods_view::tool_result(c, tool);
    // A failed call shows the end of its error under its line, closed or not: that is what you open it for.
    let error = output.filter(|(_, failed)| *failed && !opened).map(|(said, _)| {
        let lines: Vec<&str> = said.trim_end().lines().collect();
        let tail = lines[lines.len().saturating_sub(4)..].join("\n");
        // Under the name, not under the mark: past the line's inset, the 16 point box and the gap after it.
        inset(row().prop("gap", "s").prop("grow", true).child(stack().prop("width", 16)).child(text(truncate(&tail, 600)).prop("mono", true).prop("size", "small").prop("tone", "danger").prop("grow", true)))
    });
    // What it gave back as pictures (a Read of an image, a screenshot) shows under its line, closed or not.
    let pictures = thumbs(&tool.images, "Picture from the tool");
    let row_id = tool.id.clone();
    // Opened, the whole call under its line: its input whole, then what it printed. In the same inset as the error's tail.
    let whole = opened.then(|| inset(row().prop("gap", "s").prop("grow", true).child(stack().prop("width", 16)).child(body(tool, output, c.session.ran_for(&tool.id), crate::diff_view::look(c)).prop("grow", true))));
    // The call's line is what opens and shuts it, as a row of a list does.
    let head = list_button([head.prop("grow", true)], move || chat::toggle_output(&row_id));
    stack().prop("gap", "xs").child(head).maybe(mod_result).maybe(error).maybe(pictures).maybe(whole)
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

/// A call that opened, messaged or read a terminal has an Open that shows it. A terminal Wings no longer lists is closed:
/// its button waits and says so.
fn open_terminal(tool: &Tool) -> Option<Node> {
    let pane = crate::terminals::target(tool)?;
    let open = crate::terminals::is_open(&pane);
    let title = if open { "Show that terminal" } else { "That terminal is closed" };
    Some(button("Open", "ghost", false, move || crate::terminals::focus(&pane)).prop("size", "sm").prop("disabled", !open).prop("title", title).key("open-terminal"))
}

/// What a call's line would leave out: that an MCP server served a skill, and whether a notification went to your phone.
fn call_note(tool: &Tool, c: &Chat) -> Option<Node> {
    if tool.name == "Skill" && c.session.mcp_skills.contains(&tool.id) {
        return Some(badge("MCP", None).prop("title", "An MCP server served this skill"));
    }
    let (said, _) = tool.output.as_ref().filter(|_| tool.name == "PushNotification")?;
    let note = if said.contains("Mobile push requested") {
        "Push requested"
    } else if said.contains("Remote Control inactive") {
        "Not pushed: Remote Control is off"
    } else if said.contains("disabled in /config") {
        "Not pushed: turned off in /config"
    } else {
        return None;
    };
    Some(text(note).prop("size", "small").prop("tone", "muted").prop("title", said.as_str()))
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

/// What a call's fold holds: the diff, the start of the file, its input whole where its line cuts it, and what it printed. A long block
/// shows its first lines with a row that opens the rest (`code_view.rs`), in the file's language where the call names a file.
fn body(tool: &Tool, output: Option<(&str, bool)>, ran: Option<u64>, look: crate::diff_view::Look) -> Node {
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
        Detail::Command(command) if command.contains('\n') || !tool.input.whole.is_empty() => body = body.child(code_view::block(&format!("cmd-{}", tool.id), command, "sh")),
        // The field its line is cut from, whole.
        _ if !tool.input.whole.is_empty() => body = body.child(code_view::block(&format!("in-{}", tool.id), &tool.input.whole, "")),
        // A call whose input fits its line: the line itself, whole.
        Detail::Plain | Detail::Search | Detail::Command(_) if tool.input.fields.is_empty() && !tool.summary.is_empty() => body = body.child(code_view::block(&format!("in-{}", tool.id), &tool.summary, "")),
        _ => {}
    }
    // The rest of what it was given, one field a line.
    for (name, value) in &tool.input.fields {
        let field = row().prop("gap", "s").prop("align", "baseline").child(muted(name.as_str()).prop("size", "small")).child(text(value.as_str()).prop("size", "small").prop("mono", true).prop("grow", true));
        body = body.child(field.key(format!("field-{name}")));
    }
    // How long it ran, in the same line style as the fields above it; unknown, it says nothing.
    if let Some(ms) = ran {
        let field = row().prop("gap", "s").prop("align", "baseline").child(muted("ran").prop("size", "small")).child(text(ran_for(ms)).prop("size", "small").prop("mono", true).prop("grow", true));
        body = body.child(field.key("field-ran"));
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
    let tone = if done.failed() { "danger" } else { "muted" };
    // A command's description can be the whole command, many lines long: the line is cut at its first break and its
    // width, as a call's line is, and the hover has all of it.
    let title = text(done.title()).prop("size", "small").prop("tone", tone).prop("truncate", true).prop("grow", true).prop("title", done.title());
    // Under a second, a time says nothing.
    let time = done.took.filter(|ms| *ms >= 1000).map(|ms| text(took(ms)).prop("size", "small").prop("tone", tone));
    let open = if done.kind == "Agent" {
        let id = done.call.clone();
        (!id.is_empty() && c.session.subagents.contains_key(&id)).then(|| button("Open", "outline", false, move || crate::tasks_view::open_subagent(&id)).prop("title", "Its transcript"))
    } else {
        let id = done.task.clone();
        c.session.tasks.iter().any(|task| task.id == id && task.has_output()).then(|| button("Open", "outline", false, move || chat::open_task(&id)).prop("title", "What it printed"))
    };
    let line = row().prop("gap", "s").prop("align", "center").child(mark).child(title).maybe(time).maybe(open.map(|open| open.prop("size", "sm")));
    let opened = crate::folds::is_open(c, Kind::Summary, c.open_outputs.contains(&format!("finished-{at}")));
    let id = format!("finished-{at}");
    let body = if opened { text(done.report.as_str()).prop("tone", "muted").prop("size", "small") } else { stack() };
    stack()
        .prop("gap", "xs")
        .child(line)
        .maybe((!done.report.is_empty()).then(|| row().child(disclosure("Summary", body).prop("open", opened).on("onOpenChange", move |_| chat::toggle_output(&id)))))
}

/// A subagent's report to this chat: who it is from with an Open button for its tab, Claude Code's warning above it when
/// it flagged one (never folded), and the report as a reply, whole. With the setting on, it starts as its first line.
fn report_node(at: usize, report: &Report, c: &Chat) -> Node {
    let id = if report.from.is_empty() { String::new() } else { c.session.agents.call_of(&report.from).unwrap_or(report.from.as_str()).to_owned() };
    let target = crate::subagents::target(&c.session, &id);
    let who = target.map(|(_, title)| title).filter(|title| !title.is_empty()).or_else(|| Some(report.name.clone()).filter(|name| !name.is_empty())).unwrap_or_else(|| "a subagent".to_owned());
    let open = crate::subagent_relay::can_open(c, &id).then(|| button("Open", "outline", false, move || crate::tasks_view::open_subagent(&id)).prop("title", "Its transcript").prop("size", "sm"));
    let head = row().prop("gap", "s").prop("align", "center").child(text("↩").prop("tone", "muted")).child(text(format!("Report from {who}")).prop("weight", "medium").prop("size", "small")).maybe(open);
    let key = format!("report-{at}");
    let opened = prefs::is_open(!c.fold_reports, c.open_outputs.contains(&key));
    let first = report.text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
    let (summary, body) = if opened { ("Report".to_owned(), crate::status_view::reply(at, &report.text, c)) } else { (truncate(first, 160), stack()) };
    let fold = row().child(disclosure(summary, body).prop("open", opened).on("onOpenChange", move |_| chat::toggle_output(&key)));
    stack().prop("gap", "xs").maybe(report.note.as_deref().map(|note| notice("warn", note))).child(head).child(fold)
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

/// The longest detail a card draws as text: the rest is drawn as code.
const LONG_DETAIL: usize = 16_000;

fn permission_node(ask: &Ask, draft: &Draft, c: &Chat) -> Node {
    let (deny, once, always) = (ask.request.clone(), ask.request.clone(), ask.request.clone());
    let mut body: Vec<Node> = ask.reason.iter().map(|why| muted(why.as_str())).collect();
    // An edit or a write that was read against its file shows as a diff, with the lines round it.
    let diff = crate::diff_view::ask_diff(ask);
    match diff {
        Some(preview) => body.extend(crate::diff_view::ask_body(ask, preview, crate::diff_view::look(c))),
        // Past what a line of text reads well, the detail is one `Code` node, which holds far more than a text.
        None if ask.detail.len() > LONG_DETAIL => body.push(wings_plugin::markdown::highlighted_code(ask.detail.as_str(), "")),
        None => body.push(text(ask.detail.as_str()).prop("mono", true).prop("size", "small")),
    }
    // A diff holds the whole change, so an ask whose text was cut is still one to allow when it shows as one.
    let cut_short = ask.cut_short && diff.is_none();
    if cut_short {
        body.push(text("This is too long to show here in full, so it can't be allowed here. Open the session in the terminal to read it.").prop("tone", "warn"));
    }
    body.push(feedback_field(&ask.request, draft, "Reason for denying", "Why not, or what to do instead (Claude reads it)"));
    let always = (!ask.suggestions.is_empty()).then(|| button("Always allow", "secondary", false, move || chat::allow(&always, true)).prop("disabled", cut_short));
    confirm(
        format!("Allow {}?", ask.tool),
        body,
        [button("Deny", "outline", false, move || chat::deny(&deny)), button("Allow once", "default", false, move || chat::allow(&once, false)).prop("disabled", cut_short)]
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
        // One row an option, as Claude Code's own menu lists them: a mark, the label, and the whole description under it
        // in muted text, so every row's words start at one x. A round mark is one answer, a box is several.
        let advance = count > 1 && !multi && !last;
        let mut rows: Vec<Node> = options
            .iter()
            .enumerate()
            .map(|(o, option)| {
                let request = ask.request.clone();
                let words = stack()
                    .prop("gap", "xs")
                    .prop("grow", true)
                    .child(text(option["label"].as_str().unwrap_or_default()).prop("weight", "medium"))
                    .maybe(option["description"].as_str().filter(|said| !said.is_empty()).map(|said| muted(said).prop("size", "small")));
                let line = row().prop("gap", "m").prop("align", "start").child(choice_mark(multi, draft.is_picked(q, o))).child(words);
                list_button([line], move || {
                    chat::edit(&request, |draft| {
                        draft.toggle(q, o, multi);
                        if advance && draft.has_answer(q) {
                            draft.step = q + 1;
                        }
                    })
                })
                .key(format!("{q}-{o}"))
            })
            .collect();
        // "Something else" is the last option: picked, it opens a text field under it. Enter goes on to the next question,
        // or on the last sends the answers once every question has one.
        let (toggle, request, sent) = (ask.request.clone(), ask.request.clone(), ask.request.clone());
        let other_picked = draft.other_picked(q);
        let other_line = row().prop("gap", "m").prop("align", "center").child(choice_mark(multi, other_picked)).child(text("Something else").prop("weight", "medium").prop("grow", true));
        rows.push(list_button([other_line], move || chat::edit(&toggle, |draft| draft.toggle_other(q, multi))).key(format!("{q}-other")));
        if other_picked {
            let field = text_input("Other", draft.other(q), move |said| chat::edit(&request, |draft| draft.set_other(q, said, multi)))
                .prop("placeholder", "Type your answer")
                .prop("autoFocus", true)
                .prop("grow", true)
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
            // Set in to the words above, past the mark.
            rows.push(row().key(format!("other-row-{q}")).child(stack().prop("width", 24)).child(field));
        }
        // The header is quiet context over the question, not a chip that reads as a button.
        let header = question["header"].as_str().filter(|header| !header.is_empty()).map(|header| muted(header.to_uppercase()).prop("size", "small").prop("weight", "medium"));
        body.push(
            stack()
                .prop("gap", "m")
                .key(format!("q-{q}"))
                .child(stack().prop("gap", "xs").maybe(header).child(text(question["question"].as_str().unwrap_or_default()).prop("weight", "strong")))
                .child(list().children(rows)),
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
        .prop("takesTab", suggested.is_some() || chat::takes_tab(c))
        // Older Wings ignore this and highlight the first command, so Enter inserts it.
        .prop("autoHighlight", chat::mid_matches(c).is_none_or(|(_, found)| found.is_empty()))
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
                Some("Tab") => chat::tab(),
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
    // A key to press, drawn as a keycap, not a button that looks like one more control in the row. The keycap alone: with
    // its "to use it" the full row ran under the send button.
    let tab = suggested.map(|_| {
        row()
            .prop("align", "center")
            .child(badge("Tab", None).prop("title", "Press Tab to use the suggested message"))
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
    let message_box = card().prop("variant", "raised").maybe(keep_notes(c)).maybe(review_chips(c)).maybe(pasted(c)).child(field).child(controls);
    // Reading an older part, the conversation ends at the bar in a line of text cut through: a hairline makes it an edge, and the
    // button over the message box has room round it.
    let older = c.until.is_some();
    Node::new("Bar").prop("edge", if older { "top" } else { "none" }).child(
        stack()
            .prop("gap", if older { "m" } else { "s" })
            .prop("maxWidth", COLUMN)
            .maybe(todo_card(c))
            .maybe(crate::goal_view::chip(c))
            .maybe(crate::mods_view::above_prompt(c))
            .maybe(queued(c))
            .maybe(crate::branch::panel(c))
            .child(message_box)
            .maybe(crate::mods_view::under_prompt(c)),
    )
}

/// While you read an older part of a long session, a small round arrow floating over the foot of the conversation, back to
/// the newest.
fn latest(c: &Chat) -> Option<Node> {
    c.until.is_some().then(|| {
        let arrow = button("", "outline", false, || chat::with(show_latest))
            .prop("size", "icon")
            .prop("round", true)
            .prop("label", "Jump to the latest")
            .prop("title", "Jump to the latest")
            .child(icon(14, 1.5, &[DOWN]));
        stack().prop("floatBottom", true).key("latest").child(arrow)
    })
}

/// What went wrong with keeping what the tab holds: a comment that didn't fit, and a save Wings refused.
fn keep_notes(c: &Chat) -> Option<Node> {
    let notes: Vec<Node> = [(&c.review_note, "keep-review"), (&c.save_error, "keep-save")]
        .into_iter()
        .filter_map(|(note, key)| {
            let said = match key {
                "keep-save" => format!("This tab couldn't be saved, so it would open again without what changed: {}", note.as_deref()?),
                _ => note.clone()?,
            };
            Some(notice("warn", said).key(key))
        })
        .collect();
    (!notes.is_empty()).then(|| stack().prop("gap", "xs").children(notes))
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
    face = face.child(icon(24, 2.0, &icons::CHEVRON_LEFT).prop("size", 14));
    Some(button("", "outline", false, || chat::set_side_rail(false)).prop("size", "sm").prop("label", "Expand the side panel").prop("title", title.as_str()).child(face).key("side-rail"))
}

/// The panel on the right, quiet beside the conversation: a header with the button that collapses it, the folder and
/// branch, and the sections below, in a raised card. Collapsed, it is a slim rail with the button that opens it again and
/// how many things run in it. Nothing when there is nothing to say yet. The terminals Claude opened are docked beside the
/// whole page instead (`dock_terminals`).
fn side(c: &Chat) -> Vec<Node> {
    let sections = side_sections(c, true);
    if side_railed(c) {
        return rail(c, sections.is_empty()).into_iter().collect();
    }
    if sections.is_empty() {
        return Vec::new();
    }
    let collapse = icon_button("Collapse the side panel", &icons::CHEVRON_RIGHT, || chat::set_side_rail(true)).tooltip("Collapse the side panel to a rail");
    let top = row().prop("align", "center").child(inset(muted("Session").prop("size", "small")).prop("grow", true)).child(collapse).key("side-collapse");
    // Taller than the room it has, the card keeps its edge and its sections scroll inside it.
    let summary = card().prop("variant", "raised").prop("scroll", true).child(stack().prop("gap", "m").child(top).children(sections));
    vec![summary]
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
    sections.extend(crate::scheduled_view::section(c));
    sections.extend(crate::fork_view::folders(c));
    sections.extend(crate::refs_view::section(c));
    sections.extend(crate::mods_view::panes(c));
    sections.extend(subagents(c));
    // A stack with nothing in it takes room: leave out the sections that have nothing.
    sections.into_iter().filter(|section| section.count() > 1).collect()
}

/// The width of the column the terminals Claude opened dock in, beside the conversation and the panel, and the tab width
/// below which there is no room for it: the conversation would be under 480 points. Narrower, the newest terminal sits under
/// the conversation, above the message box, `DOCK_BELOW_HEIGHT` tall.
const DOCK: u32 = 420;
/// What you can drag it to: the column keeps at least this much (a terminal's prompt and a few words still fit) and the
/// conversation at least `DOCK_REST` (its column of text and the message box are readable at 480; a tab under
/// `DOCK_REST` plus the column draws the newest terminal under the conversation instead). The width is also never over
/// `DOCK_MAX`, the widest the app sizes a split to.
const DOCK_MIN: u32 = 280;
const DOCK_REST: u32 = 480;
const DOCK_MAX: u32 = 1000;

/// The width the column is drawn at: the one you dragged it to, kept in the tab, within its bounds.
pub(crate) fn dock_width_of(kept: Option<u32>) -> u32 {
    kept.map_or(DOCK, |width| width.clamp(DOCK_MIN, DOCK_MAX))
}
const DOCK_BELOW_HEIGHT: u32 = 200;

/// The terminals Claude opened, newest first: in the docked column (`beside`) each takes an equal share of its height, so
/// up to `TERMINALS_SHOWN` stack there; under the conversation of a narrow tab, one of `DOCK_BELOW_HEIGHT`. Wings draws each
/// live where its node is, with what it runs, whether it still does, and buttons that open it as a tab and close it.
fn dock_terminals(c: &Chat, beside: bool) -> Vec<Node> {
    c.terminals
        .iter()
        .rev()
        .take(TERMINALS_SHOWN)
        .map(|terminal| {
            let node = Node::new("Terminal").prop("pane", terminal.pane.as_str()).key(format!("terminal-{}", terminal.pane));
            if beside { node.prop("fill", true) } else { node.prop("height", DOCK_BELOW_HEIGHT) }
        })
        .collect()
}

/// How many of the terminals Claude opened the session panel shows, newest first, and how tall each is.
pub(crate) const TERMINALS_SHOWN: usize = 3;
pub(crate) const TERMINAL_HEIGHT: u32 = 220;

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

/// A `fold` whose rows are all listed, in a column that scrolls inside itself past `LIST_HEIGHT` (Wings before 0.2.75 draws
/// it at full height).
pub(crate) fn fold_list(c: &Chat, key: &'static str, title: impl Into<String>, rows: impl IntoIterator<Item = Node>) -> Node {
    fold(c, key, title, [wings_plugin::ui::scroll_within(LIST_HEIGHT, rows)])
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
    // Each listed is a row the page sends: a session of hundreds lists the newest.
    let hidden = tasks.len().saturating_sub(SUBAGENTS_LISTED);
    let earlier = (hidden > 0).then(|| muted(format!("{} earlier, not listed", plural(hidden, "subagent"))).prop("size", "small").key("earlier"));
    let rows = earlier.into_iter().chain(tasks.iter().skip(hidden).map(|tool| subagent_row(tool, c)));
    Some(fold_list(c, "subagents", title, rows))
}

/// The background task a subagent's call started, when it went to the background.
fn background_of<'a>(tool: &Tool, c: &'a Chat) -> Option<&'a Task> {
    c.session.tasks.iter().find(|task| !task.call.is_empty() && task.call == tool.id)
}

/// A subagent runs until its call's result, or for one in the background, until its task ends: the call is answered the
/// moment it is launched.
fn subagent_runs(tool: &Tool, c: &Chat) -> bool {
    background_of(tool, c).map_or(c.session.runs(tool), |task| task.ended.is_none())
}

fn subagent_row(tool: &Tool, c: &Chat) -> Node {
    let sub = c.session.subagents.get(&tool.id);
    let background = background_of(tool, c);
    let running = subagent_runs(tool, c);
    let failed = background.map_or(matches!(tool.output, Some((_, true))), |task| task.ended.as_deref() == Some("failed"));
    let mark = match (running, failed) {
        (true, _) => spinner(),
        (false, _) if background.is_none() && c.session.was_interrupted(tool) => text("–").prop("tone", "muted"),
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
    let line = entry(Entry { lead: Some(mark), title: text(name).prop("size", "small").prop("clamp", true).prop("title", name), detail: facts, trailing: crate::refused::badge_for(c, &tool.id) }).key(tool.id.as_str());
    match crate::subagent_relay::can_open(c, &tool.id) {
        true => {
            let id = tool.id.clone();
            line.on("onClick", move |_| crate::tasks_view::open_subagent(&id)).prop("title", "Its conversation, in the sidebar")
        }
        false => line,
    }
}

/// How long a call ran, as "0.4 s", "4.2 s", "21 s", "1 m 12 s", "1 h 4 m".
fn ran_for(ms: u64) -> String {
    match ms {
        0..10_000 => format!("{:.1} s", ms as f64 / 1000.0),
        10_000..60_000 => format!("{} s", ms / 1000),
        60_000..3_600_000 => format!("{} m {} s", ms / 60_000, ms % 60_000 / 1000),
        _ => format!("{} h {} m", ms / 3_600_000, ms % 3_600_000 / 60_000),
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
            text(said_text).prop("truncate", true).prop("expandable", true).prop("title", queued_tip(said_text))
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

/// The most of a queued message its tooltip shows: a big paste filled the screen with it.
const QUEUED_TIP: usize = 300;

/// The tooltip of a queued message: its start, cut short with an ellipsis when it is long.
fn queued_tip(said: &str) -> String {
    match said.char_indices().nth(QUEUED_TIP) {
        Some((end, _)) => format!("{}…", said[..end].trim_end()),
        None => said.to_owned(),
    }
}

/// What a queued Claude Code plugin command says under its text.
const HELD_NOTE: &str = "Runs when Claude finishes";

/// The queue card's line: one message is read, several are read together.
fn queued_note(count: usize) -> &'static str {
    if count == 1 { "Claude reads it after its current step" } else { "Claude reads them after its current step" }
}

/// The value of the menu's Ultracode switch, which no model and effort (`model@effort`) can be.
pub(crate) const ULTRACODE: &str = "ultracode:toggle";
const ULTRACODE_DETAIL: &str = "Runs big tasks as workflows. Costs more.";

/// The model menu, as Claude Code's `/model` and Claude Desktop's picker: a Model group with each model once, and an
/// Effort group with the levels the chosen model takes, the permission mode after it. The button says both, as
/// "Opus 5.5 · High".
pub(crate) fn pickers(c: &Chat) -> Vec<Node> {
    let session = &c.session;
    let mut pickers = Vec::new();
    if !session.models.is_empty() {
        let (options, label) = model_menu(session);
        let mut menu = select("Model and effort", &[], "", |picked| match picked {
            ULTRACODE => chat::toggle_ultracode(),
            _ => match picked.split_once(':') {
                Some((MODEL, model)) => chat::pick_model(model),
                Some((EFFORT, effort)) => chat::pick_effort(effort),
                _ => {}
            },
        })
        .prop("variant", "ghost")
        .prop("options", options)
        .prop("display", label);
        // The Model row in use is the value, so a Wings that doesn't know the groups still checks the model.
        if let Some(model) = session.current_model().or_else(|| session.models.iter().find(|model| model.value == "default")) {
            menu = menu.prop("value", format!("{MODEL}:{}", model.value));
        }
        if session.options.ultracode_available {
            menu = menu.prop("badge", if session.options.ultracode { "Ultracode" } else { "" });
        }
        pickers.push(menu);
    }
    // Which agent Claude Code runs as, until there is a message: Claude Code takes it only as it starts, so a pick restarts it.
    if !session.options.agent_types.is_empty() && session.items.is_empty() && !session.working {
        pickers.push(agent_menu(c));
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
        let options: Vec<(String, String)> = session.options.styles.iter().map(|style| (style.clone(), style_label(style))).collect();
        let value = session.options.style.clone().unwrap_or_default();
        pickers.push(named_select("Output style", &options, &value, chat::pick_style));
    }
    if let Some(mode) = &session.mode {
        let options: Vec<(String, String)> = MODES.iter().map(|(value, name)| (value.to_string(), name.to_string())).collect();
        pickers.push(select("Permission mode", &options, mode, chat::pick_mode).prop("variant", "ghost"));
    }
    pickers.extend(crate::branch::button_node(c));
    pickers.extend(crate::pull_view::create_button(c));
    pickers.extend(crate::worktree::button_node(c));
    pickers
}

/// Lucide's `bot`, on the agent button.
const BOT: [&str; 6] = ["M12 8V4H8", "M6 8h12a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2v-8a2 2 0 0 1 2-2z", "M2 14h2", "M20 14h2", "M15 13v2", "M9 13v2"];

/// The agent Claude Code starts as: a quiet robot until one is picked, then its name. "No agent" as a picker's label took
/// the room the message box's other buttons needed, which were cut to a few letters.
fn agent_menu(c: &Chat) -> Node {
    let glyph = icon(24, 2.0, &BOT).prop("size", 14);
    let trigger = match c.agent_type.as_deref() {
        None => Node::new("Button").prop("variant", "ghost").prop("size", "icon-sm").child(glyph),
        Some(name) => Node::new("Button").prop("variant", "ghost").prop("size", "sm").child(glyph).child(text(own_name(name)).prop("truncate", true)),
    }
    .prop("label", "Start as an agent")
    .prop("title", "Start as an agent. Picking one restarts Claude Code in this chat");
    let none = Node::new("Button").key("agent:").on("onClick", |_| chat::pick_agent("")).text("No agent");
    let agents = c.session.options.agent_types.iter().map(|(name, _)| {
        let picked = name.clone();
        Node::new("Button").key(format!("agent:{name}")).on("onClick", move |_| chat::pick_agent(&picked)).text(name.as_str())
    });
    Node::new("Menu").key("agent").child(trigger).child(none).children(agents)
}

/// A plugin's style by its own name: in full (`concise-output:concise-output`) with a "Style:" before it, it crammed the
/// message box's other buttons to a few letters each.
fn own_name(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, own)| own)
}

/// An output style's name as the picker shows it: `concise-output` as "Concise output", `default` as "Default".
fn style_label(style: &str) -> String {
    let words = own_name(style).replace(['-', '_'], " ");
    let mut letters = words.chars();
    letters.next().map(|first| first.to_uppercase().chain(letters).collect()).unwrap_or_default()
}

/// A quiet `select` whose plugin styles say under their name in the menu which plugin they come from.
fn named_select(title: &str, options: &[(String, String)], value: &str, on_change: impl Fn(&str) + 'static) -> Node {
    let named: Vec<serde_json::Value> = options
        .iter()
        .map(|(value, label)| match value.rsplit_once(':') {
            Some((plugin, _)) => serde_json::json!({ "value": value, "label": label, "detail": plugin }),
            None => serde_json::json!({ "value": value, "label": label }),
        })
        .collect();
    select(title, options, value, on_change).prop("variant", "ghost").prop("options", named)
}

/// Lucide's `smartphone`, on the Remote Control button.
const PHONE: [&str; 2] = ["M7 2h10a2 2 0 0 1 2 2v16a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2z", "M12 18h.01"];

/// The header's Remote Control, once Claude Code runs: a quiet button that turns it on, and while it is on a lit one with a
/// menu to open the chat on claude.ai, copy its link or turn it off, as Claude Code's own indicator does. In the header, not
/// the message box, whose row ran out of room with it there.
fn remote_button(c: &Chat, tight: bool) -> Option<Node> {
    use crate::remote::Remote;
    c.session.id.as_ref()?;
    // Narrow, off it is a row of the "…" menu; on, or turning on, it shows, as it says the chat is on claude.ai.
    if tight && c.session.remote == Remote::Off {
        return None;
    }
    let glyph = || icon(24, 2.0, &PHONE).prop("size", 16);
    Some(match &c.session.remote {
        Remote::Off => button("", "ghost", false, chat::toggle_remote)
            .prop("size", "icon-sm")
            .prop("label", "Remote Control")
            .prop("title", "Remote Control: go on with this chat from claude.ai or the Claude app")
            .child(glyph()),
        Remote::Connecting => button("", "ghost", true, || {}).prop("size", "icon-sm").prop("label", "Remote Control").prop("title", "Turning Remote Control on"),
        Remote::Disconnecting(_) => button("", "ghost", true, || {}).prop("size", "icon-sm").prop("label", "Remote Control").prop("title", "Turning Remote Control off"),
        Remote::On(url) => {
            // The lit icon alone, as the header's other buttons: named, it took the chat's title's room.
            let trigger = Node::new("Button")
                .prop("variant", "secondary")
                .prop("size", "icon-sm")
                .prop("label", "Remote Control")
                .prop("title", "Remote Control is on: this chat is on claude.ai and in the Claude app")
                .child(glyph());
            let (open, copied) = (url.clone(), url.clone());
            let items = vec![
                Node::new("Button").key("open").on("onClick", move |_| wings_plugin::host::open_url(&open)).text("Open on claude.ai"),
                Node::new("Button").key("copy").on("onClick", move |_| drop(wings_plugin::host::copy_text(&copied))).text("Copy link"),
                Node::new("Button").key("off").on("onClick", |_| chat::toggle_remote()).text("Turn off Remote Control"),
            ];
            Node::new("Menu").child(trigger).children(items)
        }
    })
}

/// Lucide's `ellipsis`, on the header's "…" menu.
/// Lucide's `history`, on the Past sessions button.
const HISTORY: [&str; 3] = ["M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8", "M3 3v5h5", "M12 7v5l4 2"];

const ELLIPSIS: [&str; 3] = ["M11 12a1 1 0 1 0 2 0a1 1 0 1 0-2 0", "M18 12a1 1 0 1 0 2 0a1 1 0 1 0-2 0", "M4 12a1 1 0 1 0 2 0a1 1 0 1 0-2 0"];

/// The "…" menu's row for Remote Control while it is off: turning it on.
fn remote_item(c: &Chat) -> Option<Node> {
    use crate::remote::Remote;
    c.session.id.as_ref()?;
    (c.session.remote == Remote::Off).then(|| menu_item("Remote Control", chat::toggle_remote).prop("title", "Remote Control: go on with this chat from claude.ai or the Claude app"))
}

/// Lucide's `circle-user-round`, on the header's account button.
const ACCOUNT: [&str; 3] = ["M18 20a6 6 0 0 0-12 0", "M8 10a4 4 0 1 0 8 0a4 4 0 1 0-8 0", "M2 12a10 10 0 1 0 20 0a10 10 0 1 0-20 0"];

/// An account as the header names it: by its email, else its name. Two accounts signed in with one email (Claude Code's
/// default config folder and another) read the same, so each of those also says its name.
fn account_label(account: &wings_plugin::host::AccountChoice, accounts: &[wings_plugin::host::AccountChoice]) -> String {
    let name = account.name.as_deref().unwrap_or("default");
    match &account.email {
        Some(email) if accounts.iter().filter(|other| other.email.as_ref() == Some(email)).count() > 1 => format!("{email} · {name}"),
        Some(email) => email.clone(),
        None => account.name.clone().unwrap_or_else(|| "Default account".to_owned()),
    }
}

/// The header's account button, with 2 or more accounts: the one the chat runs under by its email, and a menu of them
/// all with that one checked. Picking another starts a new chat under it.
fn account_menu(c: &Chat) -> Option<Node> {
    if c.accounts.len() < 2 {
        return None;
    }
    let current = c.accounts.iter().find(|account| account.current);
    let shown = current.map_or_else(|| "Project's account".to_owned(), |account| account_label(account, &c.accounts));
    let title = format!("Claude account: {shown}. Pick another to start a new chat under it");
    let glyph = icon(24, 2.0, &ACCOUNT).prop("size", 16);
    let trigger = Node::new("Button").prop("variant", "ghost").prop("label", "Claude account").prop("title", title.as_str());
    // A pill with the account's name, which the person picks by. Only the glyph where the header has no room for it: the
    // side panel in Wings' own side, or a tab too narrow to set the chat beside the panel (`inset`), where the chat's
    // name was cut to a few letters. The name is in the tooltip then.
    let trigger = match c.side_in_wings || (c.narrow == Some(true) && !c.inset) {
        true => trigger.prop("size", "icon-sm").child(glyph),
        false => trigger.prop("size", "sm").child(glyph).child(text(shown.as_str()).prop("truncate", true)),
    };
    let items = c.accounts.iter().map(|account| {
        let label = account_label(account, &c.accounts);
        let value = account.name.clone().unwrap_or_else(|| chat::DEFAULT_ACCOUNT.to_owned());
        let picked = account.current;
        // The one in use closes the menu and changes nothing.
        Node::new("Button").key(value.clone()).prop("pressed", picked).on("onClick", move |_| if !picked { chat::pick_account(&value) }).text(label)
    });
    Some(Node::new("Menu").prop("align", "end").child(trigger).children(items.collect::<Vec<_>>()))
}

/// A model row's value in the menu is `model:<value>`, an effort row's `effort:<level>`, a heading's `heading:<name>`.
const MODEL: &str = "model";
const EFFORT: &str = "effort";

/// The model menu's rows and the button's label. A "Select model" heading, then each model once with its description,
/// the one in use checked. Then one Effort row that says the level and opens the levels that model takes, the one shown
/// checked; a model with none has no such row. A "Recommended" pill shows on it while the level is the one a model starts
/// on (High, else its first). Ultracode, while Claude Code offers it, is a switch at the end. The label is the model,
/// then its effort.
fn model_menu(session: &Session) -> (Vec<serde_json::Value>, String) {
    use serde_json::json;
    let current = session.current_model().or_else(|| session.models.iter().find(|model| model.value == "default"));
    let mut rows = vec![json!({ "value": "heading:model", "label": "Select model", "heading": true })];
    for model in &session.models {
        let checked = current.is_some_and(|current| current.value == model.value);
        let mut row = json!({ "value": format!("{MODEL}:{}", model.value), "label": model.name, "checked": checked });
        if !model.description.is_empty() {
            row["detail"] = json!(model.description);
        }
        rows.push(row);
    }
    let shown = session.shown_effort();
    if let (Some(current), Some(level)) = (current.filter(|model| !model.efforts.is_empty()), shown.as_deref()) {
        let levels: Vec<_> = current.efforts.iter().map(|effort| json!({ "value": format!("{EFFORT}:{effort}"), "label": effort_name(effort), "checked": effort == level })).collect();
        let mut row = json!({ "value": "effort:open", "label": "Effort", "trailing": effort_name(level), "options": levels });
        if Some(level) == session.recommended_effort().as_deref() {
            row["pill"] = json!("Recommended");
        }
        rows.push(row);
    }
    if session.options.ultracode_available {
        rows.push(json!({ "value": ULTRACODE, "label": "Ultracode", "detail": ULTRACODE_DETAIL, "toggle": session.options.ultracode }));
    }
    let label = match (current, shown) {
        (Some(model), Some(effort)) => format!("{} · {}", model.name, effort_name(&effort)),
        (Some(model), None) => model.name.clone(),
        _ => String::new(),
    };
    (rows, label)
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
    let mid = chat::mid_matches(c).map(|(_, found)| found).unwrap_or_default();
    let commands = chat::command_matches(c);
    let files = chat::mention_matches(c).map(|(_, found)| found).unwrap_or_default();
    let terminals = chat::terminal_matches(c);
    let options: Vec<Node> = if !mid.is_empty() {
        // Partway through the message: Tab, or an arrow and Enter, write the name in. The field sends `autoHighlight: false`, so
        // nothing is highlighted until an arrow moves and Enter sends the message as typed.
        mid.into_iter()
            .map(|command| {
                let name = command.name.clone();
                let chip = chip(format!("/{}", command.name), false, move || chat::insert_mid_command(&name)).key(&command.name);
                if command.description.is_empty() { chip } else { chip.child(muted(truncate(&command.description, 100)).prop("size", "small")) }
            })
            .collect()
    } else if !commands.is_empty() {
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
mod tests;

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

#[cfg(test)]
mod composer_place_tests {
    use super::*;

    /// The id of the message box in what the page draws.
    fn message_id(nodes: &[serde_json::Value]) -> Option<u64> {
        nodes.iter().find_map(|node| {
            if node["type"] == "Textarea" && node["props"]["label"] == "Message" {
                return node["id"].as_u64();
            }
            node["children"].as_array().and_then(|kids| message_id(kids))
        })
    }

    fn drawn_id() -> u64 {
        let (tree, _) = wings_plugin::ui::export(view());
        message_id(tree.as_array().unwrap()).expect("a message box")
    }

    /// The app keeps what you typed in a field by the node's id, so a message box that gets a new id when the conversation
    /// draws loses the text typed before the page heard it.
    #[test]
    fn the_message_box_keeps_its_id_from_the_empty_chat_to_a_resumed_one() {
        chat::with(|c| {
            c.session = Session::default();
            c.state = State::Running;
        });
        let empty = drawn_id();
        chat::with(|c| c.session.items.push(Item::User("Fix the build".into())));
        let resumed = drawn_id();
        chat::with(|c| c.session = Session::default());
        assert_eq!(empty, resumed);
    }

    /// The page hears that the tab is narrow only after its first draw, and the message box then moves out of the split, so it
    /// has to keep its id there too (a wide tab, the rarer one, still moves it once).
    #[test]
    fn the_message_box_keeps_its_id_when_the_page_hears_the_tab_is_narrow() {
        chat::with(|c| {
            c.session = Session::default();
            c.state = State::Running;
            c.narrow = None;
        });
        let unknown = drawn_id();
        chat::set_narrow(true, false);
        let narrow = drawn_id();
        chat::with(|c| c.narrow = None);
        assert_eq!(unknown, narrow);
    }
}

#[cfg(test)]
mod dock_tests {
    use super::*;

    fn terminal(pane: &str) -> crate::chat::Terminal {
        crate::chat::Terminal { pane: pane.into() }
    }

    /// The `Split` that docks the terminals, in the page's JSON: the one with `dock`.
    fn dock_of(node: &serde_json::Value) -> Option<&serde_json::Value> {
        if node["type"] == "Split" && node["props"]["dock"] == true {
            return Some(node);
        }
        node["children"].as_array()?.iter().find_map(dock_of)
    }

    fn slot_of<'a>(split: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
        split["children"].as_array().unwrap().iter().find(|slot| slot["props"]["name"] == name).unwrap()
    }

    fn terminals_in(node: &serde_json::Value) -> Vec<serde_json::Value> {
        let mut found = Vec::new();
        if node["type"] == "Terminal" {
            found.push(node.clone());
        }
        for child in node["children"].as_array().into_iter().flatten() {
            found.extend(terminals_in(child));
        }
        found
    }

    fn page(set: impl FnOnce(&mut Chat)) -> serde_json::Value {
        chat::with(|c| {
            c.side_in_wings = false;
            c.side_open = true;
            c.session = Session::default();
            c.terminals = vec![terminal("1"), terminal("2")];
            set(c);
            wings_plugin::ui::tree_json(chat_page(c))
        })
    }

    #[test]
    fn in_a_wide_tab_the_terminals_stack_in_a_full_height_column_beside_the_conversation() {
        let tree = page(|c| c.dock_narrow = Some(false));
        let dock = dock_of(&tree).expect("the dock");
        assert_eq!(dock["props"]["side"], "end");
        let column = terminals_in(slot_of(dock, "end"));
        // Newest first, each sharing the column's height.
        let panes: Vec<_> = column.iter().map(|node| node["props"]["pane"].as_str().unwrap()).collect();
        assert_eq!(panes, ["2", "1"]);
        assert!(column.iter().all(|node| node["props"]["fill"] == true));
        assert!(terminals_in(slot_of(dock, "start")).is_empty(), "none under the conversation");
        assert_eq!(terminals_in(&tree).len(), 2, "and none left in the session panel's card");
    }

    #[test]
    fn in_a_narrow_tab_or_before_the_page_has_measured_the_newest_goes_under_the_conversation() {
        for measured in [None, Some(true)] {
            let tree = page(|c| c.dock_narrow = measured);
            let dock = dock_of(&tree).expect("the dock");
            assert!(terminals_in(slot_of(dock, "end")).is_empty(), "{measured:?}: no column");
            let below = terminals_in(slot_of(dock, "start"));
            assert_eq!(below.len(), 1, "{measured:?}");
            assert_eq!(below[0]["props"]["pane"], "2");
            assert_eq!(below[0]["props"]["height"], DOCK_BELOW_HEIGHT);
            assert!(below[0]["props"]["fill"].is_null());
        }
    }

    #[test]
    fn the_dock_is_the_one_split_the_page_keeps_with_or_without_a_terminal() {
        let none = page(|c| c.terminals.clear());
        let dock = dock_of(&none).expect("the dock, with nothing in it");
        assert!(terminals_in(slot_of(dock, "end")).is_empty());
        assert_eq!(dock["props"]["collapseBelow"], DOCK_REST + DOCK);
        assert_eq!(dock["props"]["defaultSize"], DOCK);
    }

    #[test]
    fn the_message_box_keeps_its_id_when_a_terminal_opens_and_when_the_dock_is_measured() {
        fn find(node: &serde_json::Value) -> Option<u64> {
            if node["type"] == "Textarea" && node["props"]["label"] == "Message" {
                return node["id"].as_u64();
            }
            node["children"].as_array()?.iter().find_map(find)
        }
        let closed = find(&page(|c| {
            c.terminals.clear();
            c.dock_narrow = None;
            c.narrow = Some(true);
        }))
        .expect("a message box");
        let opened = find(&page(|c| c.dock_narrow = None)).expect("a message box");
        let measured = find(&page(|c| c.dock_narrow = Some(true))).expect("a message box");
        assert_eq!(closed, opened);
        assert_eq!(closed, measured);
        let dragged = find(&page(|c| c.dock_width = Some(600))).expect("a message box");
        assert_eq!(closed, dragged);
    }
}
