//! What the chat page shows: the toolbar with the folder and the model, the conversation, newest at the bottom and kept
//! there while you're reading the end, and the message box under it.

use wings_plugin::{
    markdown::{code_block, markdown},
    ui::{Node, badge, button, card, confirm, disclosure, empty, loading, muted, notice, page, row, spinner, stack, text, toolbar_with},
};

use crate::{
    chat::{self, Chat, PAGE, State},
    session::{Ask, Item, Tool},
};

/// The conversation's column, as wide as a page meant for reading.
const COLUMN: u32 = 860;

pub fn view() -> Vec<Node> {
    chat::with(|c| vec![chat_page(c)])
}

fn chat_page(c: &Chat) -> Node {
    let facts = row().prop("gap", "s").child(muted(c.folder.as_str())).maybe(c.session.model.as_deref().map(|model| badge(model, None)));
    let stop = c.session.working.then(|| button("Stop", "outline", false, chat::stop));
    let toolbar = toolbar_with("Claude", facts, stop);
    if c.session.items.is_empty() && c.session.live.is_empty() {
        let body = match &c.state {
            State::Starting => loading(),
            State::Failed(why) => notice("danger", why.as_str()),
            _ => empty("Ask Claude anything", "It works in this folder, with your Claude Code setup. It asks here before it changes anything you haven't allowed."),
        };
        return page(toolbar, [stack().prop("grow", true).prop("gap", "none").child(Node::new("Scroll").child(column().child(body))).child(composer(c))]);
    }
    let conversation = Node::new("Scroll").prop("follow", "end").child(column().children(conversation(c)));
    page(toolbar, [stack().prop("grow", true).prop("gap", "none").child(conversation).child(composer(c))])
}

fn column() -> Node {
    stack().prop("gap", "l").prop("maxWidth", COLUMN)
}

/// The items shown, oldest first, after a Show earlier button when there are more, then the text streaming in.
fn conversation(c: &Chat) -> Vec<Node> {
    let items = &c.session.items;
    let from = items.len().saturating_sub(c.shown);
    let earlier = (from > 0).then(|| row().child(button(format!("Show earlier ({from})"), "ghost", false, || chat::with(|c| c.shown += PAGE))));
    let mut nodes: Vec<Node> = earlier.into_iter().chain(items[from..].iter().enumerate().filter_map(|(at, item)| item_node(from + at, item))).collect();
    if !c.session.live.is_empty() {
        nodes.push(markdown(&c.session.live).key("live"));
    } else if c.session.working && !waiting_for_you(c) {
        nodes.push(row().prop("gap", "s").key("working").child(spinner()).child(muted("Working")));
    }
    match &c.state {
        State::Failed(why) => nodes.push(notice("danger", why.as_str()).key("ended")),
        State::Ended => nodes.push(muted("Claude Code ended. Open a new chat to go on.").key("ended")),
        _ => {}
    }
    nodes
}

/// Claude is waiting for an answer to a question it asked.
fn waiting_for_you(c: &Chat) -> bool {
    c.session.items.iter().any(|item| matches!(item, Item::Ask(Ask { answer: None, .. })))
}

/// An item's node. A question you have answered shows nothing: the tool call it was about shows how it went.
fn item_node(at: usize, item: &Item) -> Option<Node> {
    let node = match item {
        Item::User(said) => card().child(text(said.as_str())),
        Item::Text(said) => markdown(said),
        Item::Tool(tool) => tool_node(tool),
        Item::Ask(ask) if ask.answer.is_some() => return None,
        Item::Ask(ask) => ask_node(ask),
        Item::Error(said) => notice("danger", said.as_str()),
        Item::Stopped => muted("Stopped"),
    };
    Some(node.key(at.to_string()))
}

fn tool_node(tool: &Tool) -> Node {
    let failed = tool.output.as_ref().is_some_and(|(_, failed)| *failed);
    let head = row()
        .prop("gap", "s")
        .prop("align", "baseline")
        .child(text(tool.name.as_str()).prop("weight", "medium"))
        .child(text(tool.summary.as_str()).prop("mono", true).prop("size", "small").prop("tone", "muted").prop("truncate", true).prop("grow", true))
        .maybe(tool.output.is_none().then(spinner))
        .maybe(failed.then(|| badge("Failed", Some("danger"))));
    match &tool.output {
        Some((output, _)) if !output.trim().is_empty() => {
            stack().prop("gap", "xs").child(head).child(disclosure(if failed { "Error" } else { "Output" }, code_block(output)))
        }
        _ => head,
    }
}

fn ask_node(ask: &Ask) -> Node {
    let (deny, allow) = (ask.request.clone(), ask.request.clone());
    confirm(
        format!("Allow {}?", ask.tool),
        [text(ask.summary.as_str()).prop("mono", true)],
        [button("Deny", "outline", false, move || chat::answer(&deny, false)), button("Allow", "default", false, move || chat::answer(&allow, true))],
    )
}

/// The message box: Enter sends, Shift-Enter starts a new line.
fn composer(c: &Chat) -> Node {
    let open = c.state == State::Running;
    let field = Node::new("Textarea")
        .prop("label", "Message")
        .prop("placeholder", "Message Claude")
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
    let send = button("Send", "default", false, chat::send).prop("disabled", !open || c.draft.trim().is_empty());
    let line = row().prop("gap", "s").prop("align", "end").child(field.prop("grow", true)).child(send);
    Node::new("Bar").prop("edge", "top").child(stack().prop("maxWidth", COLUMN).child(line))
}
