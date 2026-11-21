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
        let body = Node::new("Slot").prop("name", "body").child(sidebar_header(c, tab)).child(idle);
        return Node::new("Page").prop("flush", true).on_text_scale(crate::prefs::step_text_size).child(body);
    }
    let header = if tab.sidebar { sidebar_header(c, tab) } else { header(c, tab) };
    // A picture clicked in the subagent's conversation opens over its pane, as it does over the chat.
    let body = Node::new("Slot").prop("name", "body").child(header).child(conversation(c, tab)).child(composer(c, tab)).maybe(crate::pictures_view::dialog_of(c));
    // The right-click menu (Copy, Copy link, Copy code) opens over the pane as it does over the chat.
    let body = body.maybe(crate::mods_view::menu(c));
    Node::new("Page").prop("flush", true).on_text_scale(crate::prefs::step_text_size).child(body)
}

/// How the subagent stands, as a muted line: running since, done in how long, or not running with its chat closed.
fn status(tab: &Tab) -> Option<Node> {
    let state = &tab.state;
    match (state["running"].as_bool(), state["since"].as_u64(), state["took"].as_u64()) {
        (Some(true), Some(since), _) => Some(muted("Working").prop("since", since)),
        (Some(true), None, _) => Some(muted("Working")),
        (Some(false), _, Some(took)) => Some(muted(format!("Done · {}", view::took(took)))),
        (Some(false), _, None) => Some(muted("Done")),
        // Only the chat's Claude Code runs it, so with the chat closed it doesn't run.
        (None, ..) if tab.chat_open == Some(false) => Some(muted("Not running")),
        (None, ..) => None,
    }
}

/// The sidebar's header: the subagent's name, its chat and how it stands, and under them the way back to the chat, the session (when it is in the sidebar) and the
/// button that pops the subagent out into a tab.
fn sidebar_header(c: &Chat, tab: &Tab) -> Node {
    let idle = tab.session.is_empty();
    let name = if idle { "Subagent" } else { tab.title.as_str() };
    let title = row()
        .prop("gap", "m")
        .prop("align", "center")
        .child(Node::new("Icon").prop("name", "claude").prop("size", 14))
        .child(text(name).prop("weight", "strong").prop("truncate", true).prop("grow", true).prop("title", name));
    let chat_open = tab.chat_open == Some(true);
    let chat_name = tab.state["chat"].as_str().map(|chat| truncate(chat, 40));
    let model = model_of(c, tab);
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

/// The subagent's model as its chat last said, else as its own transcript says: a closed chat says nothing.
fn model_of(c: &Chat, tab: &Tab) -> Option<String> {
    tab.state["model"].as_str().map(model_name).or_else(|| c.session.reported_model.as_deref().map(model_name))
}

/// As the chat's header: back to the chat, the subagent's name, and how it stands, with its model and its chat.
fn header(c: &Chat, tab: &Tab) -> Node {
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
    let model = model_of(c, tab);
    let facts: Vec<String> = model.into_iter().chain(chat_name.map(|name| format!("from {name}"))).collect();
    // On the back button's text edge: 14 for the bar, 7 for a row's inset, and 4 for the button's own padding.
    let facts = (!facts.is_empty()).then(|| {
        let said = facts.join(" · ");
        view::inset(muted(said.as_str()).prop("size", "small").prop("truncate", true).prop("title", said.as_str())).prop("indent", 4)
    });
    let title = row()
        .prop("gap", "m")
        .prop("align", "center")
        .prop("grow", true)
        .child(Node::new("Icon").prop("name", "claude").prop("size", 14))
        .child(text(tab.title.as_str()).prop("weight", "strong").prop("truncate", true).prop("title", tab.title.as_str()));
    // The model and chat go under the title, so they don't take the title's room in a narrow tab: they were cut by the
    // line's end while the title was, with the status beside it, and the two groups ran together.
    let line = row().prop("gap", "m").prop("align", "center").child(back).child(title).maybe(status.map(|status| status.prop("size", "small")));
    // Not the slim bar, which is for a pane header's buttons: its 4 point right edge pressed the status against the window's.
    Node::new("Bar").child(stack().prop("gap", "xs").child(line).maybe(facts))
}

/// The subagent's conversation, newest at the foot and kept there while you read the end, built from the newest back until
/// the page's budget is spent.
fn conversation(c: &Chat, tab: &Tab) -> Node {
    // A right-click on a message, a link or a code block opens the chat's menu: Copy, Copy link, Copy code.
    let column = stack().prop("gap", "l").prop("maxWidth", view::COLUMN).on("onContextMenu", |payload| crate::mods_view::open_menu(payload));
    let items = &c.session.items;
    if items.is_empty() && tab.sent.is_empty() {
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
            // Its replies, the report it hands back the last of them, are this page's own items, which a Copy button names.
            Item::Text(said) => Some(crate::status_view::reply(start, said, c).key(start.to_string())),
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
    // What you sent that the chat hasn't passed on yet, at the end, as yours with a line that says so.
    let on_the_way = tab.sent.iter().enumerate().map(|(n, said)| {
        let message = view::item_node(n, &Item::User(said.clone()), c, false).unwrap_or_else(stack);
        let note = row().prop("justify", "end").child(muted("On its way").prop("size", "small"));
        stack().prop("gap", "xs").child(message).child(note).key(format!("sent-{n}"))
    });
    let nodes: Vec<Node> = earlier.into_iter().chain(blocks).chain(on_the_way).chain(working).collect();
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
        _ => muted("Goes through the chat: Claude passes it on, and wakes the subagent if it has finished."),
    };
    let controls = row().prop("gap", "s").prop("align", "center").child(stack().prop("grow", true).child(hint.prop("size", "small"))).child(send);
    let message_box = card().prop("variant", "raised").child(field.key("field")).child(controls);
    let refused = tab.refused.as_deref().map(|why| notice("danger", why));
    Node::new("Bar").prop("edge", "none").child(stack().prop("gap", "s").prop("maxWidth", view::COLUMN).maybe(refused).child(message_box))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_you_sent_shows_at_the_end_as_on_its_way() {
        crate::chat::with(|c| {
            crate::subagents::Heard::default().load(&crate::subagents::tests::woken(), &mut c.session, &mut Vec::new());
            let tab = Tab { session: "s".into(), sent: vec!["Also check the tests.".into()], ..Tab::default() };
            let shown = wings_plugin::ui::tree_json(page(c, &tab)).to_string();
            c.session = crate::session::Session::default();
            let (said, note, reply) = (shown.find("Also check the tests."), shown.find("On its way"), shown.find("Starting with the helper."));
            assert!(matches!((reply, said, note), (Some(reply), Some(said), Some(note)) if reply < said && said < note), "after the conversation, with its note: {shown}");
        });
    }

    #[test]
    fn the_whole_report_a_subagent_hands_back_reaches_its_page_with_a_copy_button() {
        crate::chat::with(|c| {
            crate::subagents::Heard::default().load(&crate::subagents::tests::woken(), &mut c.session, &mut Vec::new());
            let shown = wings_plugin::ui::tree_json(page(c, &Tab { session: "s".into(), ..Tab::default() })).to_string();
            c.session = crate::session::Session::default();
            // Each block of its Markdown, to its last words, which a call's one-line summary cut off.
            for said in ["\"Done\"", "The four sections of the side panel now scroll inside themselves, each up to 240 points tall, and show all their rows.", "\"Left\"", "The render at 640 points still needs a look. Nothing else is left."] {
                assert!(shown.contains(said), "{said} in {shown}");
            }
            assert!(shown.contains("Copy this reply"), "{shown}");
        });
    }

    #[test]
    fn a_tab_whose_chat_is_closed_names_the_model_its_transcript_says() {
        crate::chat::with(|c| {
            c.session.reported_model = Some("claude-haiku-5-5".into());
            assert_eq!(model_of(c, &Tab::default()).as_deref(), Some("Haiku 5.5"));
            let told = Tab { state: serde_json::json!({ "model": "claude-opus-5-5" }), ..Tab::default() };
            assert_eq!(model_of(c, &told).as_deref(), Some("Opus 5.5"), "what the chat says comes first");
        });
    }

    #[test]
    fn a_picture_clicked_in_the_subagents_pane_opens_over_it() {
        crate::chat::with(|c| {
            c.picture = Some("pic-1".into());
            let tree = wings_plugin::ui::tree_json(page(c, &Tab { session: "s".into(), ..Tab::default() })).to_string();
            c.picture = None;
            assert!(tree.contains("\"Dialog\"") && tree.contains("Picture, enlarged"), "{tree}");
        });
    }

    #[test]
    fn a_right_click_in_the_subagents_pane_opens_the_copy_menu() {
        crate::chat::with(|c| {
            c.session.mods.menu = Some(serde_json::json!({ "x": 10, "y": 20, "selection": { "text": "some words" } }));
            let tree = wings_plugin::ui::tree_json(page(c, &Tab { session: "s".into(), ..Tab::default() })).to_string();
            c.session.mods.menu = None;
            assert!(tree.contains("\"PopupMenu\"") && tree.contains("\"Copy\"") && tree.contains("onContextMenu"), "{tree}");
        });
    }
}
