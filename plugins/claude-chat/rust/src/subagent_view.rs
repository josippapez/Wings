//! What a subagent's tab shows (`subagent_tab.rs`): a header that stays at the top with the way back to the chat, what the
//! subagent is for and how it stands; its conversation, drawn as the chat draws Claude's; and a message box at the foot
//! that sends through the chat.

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
    let body = Node::new("Slot").prop("name", "body").child(header(tab)).child(conversation(c, tab)).child(composer(c, tab));
    Node::new("Page").prop("flush", true).child(body)
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
    let status = match (state["running"].as_bool(), state["since"].as_u64(), state["took"].as_u64()) {
        (Some(true), Some(since), _) => Some(muted("Running").prop("since", since)),
        (Some(true), None, _) => Some(muted("Running")),
        (Some(false), _, Some(took)) => Some(muted(format!("Done · {}", view::took(took)))),
        (Some(false), _, None) => Some(muted("Done")),
        // Only the chat's Claude Code runs it, so with the chat closed it doesn't run.
        (None, ..) if tab.chat_open == Some(false) => Some(muted("Not running")),
        (None, ..) => None,
    };
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
        .prop("autoFocus", true)
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
