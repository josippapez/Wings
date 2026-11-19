//! The Claude chat page in Wings Settings: the mode a new chat starts in, how much of its tool calls a chat shows and which
//! kinds of fold Detailed opens, and how big its text is.

use wings_plugin::ui::{Node, checkbox, chips, muted, stack};

use crate::{folds::KINDS, prefs, session::MODES};

fn heading(text: &str) -> Node {
    Node::new("Heading").text(text)
}

fn view() -> Vec<Node> {
    let outputs = [("compact", "Compact"), ("detailed", "Detailed")];
    let output = if prefs::detailed().get() { "detailed" } else { "compact" };
    let opens = KINDS.iter().map(|(kind, key, label)| {
        let kind = *kind;
        checkbox(*label, prefs::opens(kind).get(), move |on| drop(prefs::opens(kind).set(on))).key(*key)
    });
    vec![stack()
        .prop("gap", "m")
        .child(heading("New chats"))
        .child(muted("The permission mode a chat starts in. The picker in a chat's message box still changes it for that chat."))
        .child(chips("Permission mode", &MODES, &prefs::mode().get(), |mode| drop(prefs::mode().set(mode.to_owned()))))
        .child(heading("Output"))
        .child(muted("Compact folds a run of tool calls into one line. Detailed shows every run open, with each command and what it printed, as Claude Code's verbose mode does. A chat that is open follows a change at once."))
        .child(chips("Tool calls", &outputs, output, |value| drop(prefs::detailed().set(value == "detailed"))))
        .child(heading("What Detailed opens"))
        .child(muted("Each kind starts open in Detailed unless you clear it here. A click on a fold still opens or shuts it, and Compact keeps all of them shut."))
        .child(stack().prop("gap", "s").children(opens))
        .child(heading("Plan limit"))
        .child(muted("When your plan's 5-hour or weekly usage passes 90%, Claude is told to pause and ask you before more work, in every Claude Code session in Wings, chat or terminal. Past the limit, work is billed as extra usage."))
        .child(checkbox("Tell Claude to pause near the limit", prefs::pause_near_limit().get(), |on| drop(prefs::pause_near_limit().set(on))).key("pause-near-limit"))
        .child(heading("Text size"))
        .child(muted("The size of the text in the conversation, in every chat. A chat that is open follows a change at once."))
        .child(chips("Text size", &prefs::TEXT_SIZES, &prefs::text_size().get(), |size| drop(prefs::text_size().set(size.to_owned()))))]
}

#[unsafe(no_mangle)]
pub fn start() {
    wings_plugin::run(view);
}
