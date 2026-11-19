//! What a subagent's page shows (`subagent_tab.rs`), as a tab or in Wings' sidebar: a header that stays at the top with the
//! way back to the chat, what the subagent is for and how it stands (and in the sidebar, a button that pops it out into a
//! tab); its conversation, drawn as the chat draws Claude's; and a message box at the foot that sends through the chat.

use wings_plugin::{
    format::truncate,
    ui::{Node, button, card, empty, icon, loading, muted, notice, row, stack, text},
};

use crate::{
    chat::{self, Chat},
    session::{Item, Tool, model_name},
    subagent_tab::{self, Tab},
    view,
};

pub fn view() -> Vec<Node> {
    chat::with(|c| subagent_tab::with(|tab| vec![page(c, tab)]).unwrap_or_default())
}

/// Flush, so the conversation scrolls on its own between the header and the message box.
fn page(c: &Chat, tab: &Tab) -> Node {
    // The sidebar before the chat has said which subagent: a header and a line, and no message box.
    if tab.sidebar && tab.session.is_empty() {
        let idle = empty("No subagent open", "Click a subagent in a chat, in its side panel or on its call, to read it here and message it.");
        let body = Node::new("Slot").prop("name", "body").child(sidebar_header(tab)).child(idle);
        return Node::new("Page").prop("flush", true).on_text_scale(crate::prefs::step_text_size).child(body);
    }
    let header = if tab.sidebar { sidebar_header(tab) } else { header(tab) };
    let body = Node::new("Slot").prop("name", "body").child(header).child(conversation(c, tab)).child(composer(c, tab));
    Node::new("Page").prop("flush", true).on_text_scale(crate::prefs::step_text_size).child(body)
}

/// How the subagent stands, as a muted line: running since, done in how long, or not running with its chat closed.
fn status(tab: &Tab) -> Option<Node> {
    let state = &tab.state;
    match (state["running"].as_bool(), state["since"].as_u64(), state["took"].as_u64()) {
        (Some(true), Some(since), _) => Some(muted("Running").prop("since", since)),
        (Some(true), None, _) => Some(muted("Running")),
        (Some(false), _, Some(took)) => Some(muted(format!("Done · {}", view::took(took)))),
        (Some(false), _, None) => Some(muted("Done")),
        // Only the chat's Claude Code runs it, so with the chat closed it doesn't run.
        (None, ..) if tab.chat_open == Some(false) => Some(muted("Not running")),
        (None, ..) => None,
    }
}

/// The sidebar's header: the subagent's name, its chat and how it stands, and under them the way back to the chat, the session (when it is in the sidebar) and the
/// button that pops the subagent out into a tab.
fn sidebar_header(tab: &Tab) -> Node {
    let idle = tab.session.is_empty();
    let name = if idle { "Subagent" } else { tab.title.as_str() };
    let title = row()
        .prop("gap", "m")
        .prop("align", "center")
        .child(Node::new("Icon").prop("name", "claude").prop("size", 14))
        .child(text(name).prop("weight", "strong").prop("truncate", true).prop("grow", true).prop("title", name));
    let chat_open = tab.chat_open == Some(true);
    let chat_name = tab.state["chat"].as_str().map(|chat| truncate(chat, 40));
    let model = tab.state["model"].as_str().map(model_name);
    let facts: Vec<String> = model.into_iter().chain(chat_name.as_ref().map(|chat| format!("from {chat}"))).collect();
    let facts = (!idle && !facts.is_empty()).then(|| {
        let said = facts.join(" · ");
        muted(said.as_str()).prop("size", "small").prop("truncate", true).prop("title", said.as_str())
    });
    let back_title = match (&chat_name, chat_open) {
        (_, false) => "The chat this subagent belongs to is closed".to_owned(),
        (Some(chat), true) => format!("Show {chat}"),
        (None, true) => "Show the chat".to_owned(),
    };
    let actions = (!idle).then(|| {
        row()
            .prop("gap", "xs")
            .prop("align", "center")
            .child(button("Chat", "ghost", false, subagent_tab::back).prop("size", "sm").prop("disabled", !chat_open).prop("title", back_title))
            .maybe(crate::prefs::in_sidebar(&crate::prefs::session_panel().get()).then(|| {
                button("Session", "ghost", false, subagent_tab::show_session).prop("size", "sm").prop("disabled", !chat_open).prop("title", "Show the chat's session in the sidebar again")
            }))
            .child(button("Pop out", "ghost", false, subagent_tab::pop_out).prop("size", "sm").prop("title", "Open it as a tab beside its chat"))
            .child(stack().prop("grow", true))
            .maybe(status(tab).map(|status| status.prop("size", "small")))
    });
    Node::new("Bar").child(stack().prop("gap", "xs").child(title).maybe(facts).maybe(actions))
}

/// As the chat's header: back to the chat, the subagent's name, and how it stands, with its model and its chat.
fn header(tab: &Tab) -> Node {
    let state = &tab.state;
    let open = tab.chat_open == Some(true);
    let chat_name = state["chat"].as_str().map(|name| truncate(name, 40));
    let back_title = match (&chat_name, open) {
        (_, false) => "The chat this subagent belongs to is closed".to_owned(),
        (Some(name), true) => format!("Show {name}"),
        (None, true) => "Show the chat".to_owned(),
    };
    let back = button("← Back to chat", "ghost", false, subagent_tab::back).prop("size", "sm").prop("disabled", !open).prop("title", back_title);
    let status = status(tab);
    let model = state["model"].as_str().map(model_name);
    let facts: Vec<String> = model.into_iter().chain(chat_name.map(|name| format!("from {name}"))).collect();
    let facts = (!facts.is_empty()).then(|| {
        let said = facts.join(" · ");
        muted(said.as_str()).prop("size", "small").prop("truncate", true).prop("title", said.as_str())
    });
    let title = row()
        .prop("gap", "m")
        .prop("align", "center")
        .prop("grow", true)
        .child(Node::new("Icon").prop("name", "claude").prop("size", 14))
        .child(text(tab.title.as_str()).prop("weight", "strong").prop("truncate", true).prop("title", tab.title.as_str()));
    let line = row().prop("gap", "m").prop("align", "center").child(back).child(title).maybe(status.map(|status| status.prop("size", "small"))).maybe(facts);
    Node::new("Bar").prop("variant", "slim").child(line)
}

/// The subagent's conversation, newest at the foot and kept there while you read the end, built from the newest back until
/// the page's budget is spent.
fn conversation(c: &Chat, tab: &Tab) -> Node {
    let column = stack().prop("gap", "l").prop("maxWidth", view::COLUMN);
    let items = &c.session.items;
    if items.is_empty() {
        let body = match (&tab.error, tab.heard.loaded) {
            (_, false) => loading(),
            (Some(why), true) => notice("danger", format!("Couldn't read this subagent's conversation: {why}")),
            (None, true) => empty("Nothing yet", "What the subagent says and does shows here as it works."),
        };
        return Node::new("Scroll").child(column.child(body));
    }
    let (mut blocks, mut spent, mut cut) = (Vec::new(), 0, false);
    let mut end = items.len();
    while end > 0 {
        let mut start = end - 1;
        while view::folds(&items[start]) && start > 0 && view::folds(&items[start - 1]) {
            start -= 1;
        }
        let block = match &items[start] {
            item if view::folds(item) => {
                let tools: Vec<&Tool> = items[start..end].iter().filter_map(|item| if let Item::Tool(tool) = item { Some(tool) } else { None }).collect();
                (!tools.is_empty()).then(|| view::steps(start, &tools, c))
            }
            // Its messages aren't the chat's, so they have no rewind.
            item => view::item_node(start, item, c, false),
        };
        end = start;
        let Some(block) = block else { continue };
        spent += block.count();
        if spent > view::BUDGET && !blocks.is_empty() {
            cut = true;
            break;
        }
        blocks.push(block);
    }
    blocks.reverse();
    let earlier = cut.then(|| muted("Earlier steps aren't shown here.").prop("size", "small").key("earlier"));
    let working = match (tab.state["running"] == true, tab.state["since"].as_u64()) {
        (true, Some(since)) => Some(text("Working").prop("tone", "muted").prop("shimmer", true).prop("since", since).key("working")),
        (true, None) => Some(text("Working").prop("tone", "muted").prop("shimmer", true).key("working")),
        (false, _) => None,
    };
    let nodes: Vec<Node> = earlier.into_iter().chain(blocks).chain(working).collect();
    Node::new("Scroll").prop("follow", "end").prop("textScale", c.text_scale as f64).child(column.prop("enter", "rise").children(nodes))
}

/// The message box at the foot: Enter has the chat's Claude pass what you typed on. Closed while the chat can't, which
/// it says in place of how a message goes.
fn composer(c: &Chat, tab: &Tab) -> Node {
    let closed = tab.closed();
    let open = closed.is_none();
    let field = Node::new("Textarea")
        .prop("label", "Message")
        .prop("placeholder", format!("Message {}", truncate(&tab.title, 60)))
        .prop("variant", "bare")
        .prop("rows", 2)
        .prop("submitOnEnter", true)
        // A tab you opened takes the keyboard; the sidebar sits beside what you were typing in.
        .prop("autoFocus", !tab.sidebar)
        .prop("value", c.draft.as_str())
        .prop("disabled", !open)
        .on("onChange", |event| chat::with(|c| c.draft = event["value"].as_str().unwrap_or_default().to_owned()))
        .on("onKeyDown", |event| {
            if event["key"] == "Enter" {
                subagent_tab::send();
            }
        });
    let typed = !c.draft.trim().is_empty();
    let send = button("", "default", false, subagent_tab::send)
        .prop("size", "icon-sm")
        .prop("round", true)
        .prop("label", "Send")
        .prop("title", "Send (Enter)")
        .prop("disabled", !open || !typed)
        .child(icon(14, 2.0, &[view::SEND]));
    let hint = match closed {
        Some(Some(why)) => text(why).prop("tone", "warn"),
        _ => muted("Goes through the chat: its Claude passes it on with SendMessage, which wakes a subagent that has finished."),
    };
    let controls = row().prop("gap", "s").prop("align", "center").child(stack().prop("grow", true).child(hint.prop("size", "small"))).child(send);
    let message_box = card().prop("variant", "raised").child(field.key("field")).child(controls);
    let refused = tab.refused.as_deref().map(|why| notice("danger", why));
    Node::new("Bar").prop("edge", "none").child(stack().prop("gap", "s").prop("maxWidth", view::COLUMN).maybe(refused).child(message_box))
}
