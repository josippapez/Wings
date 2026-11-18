//! The Claude chat page in Wings Settings: the mode a new chat starts in, and how much of its tool calls a chat shows.

use wings_plugin::ui::{Node, chips, muted, stack};

use crate::{prefs, session::MODES};

fn heading(text: &str) -> Node {
    Node::new("Heading").text(text)
}

fn view() -> Vec<Node> {
    let outputs = [("compact", "Compact"), ("detailed", "Detailed")];
    let output = if prefs::detailed().get() { "detailed" } else { "compact" };
    vec![stack()
        .prop("gap", "m")
        .child(heading("New chats"))
        .child(muted("The permission mode a chat starts in. The picker in a chat's message box still changes it for that chat."))
        .child(chips("Permission mode", &MODES, &prefs::mode().get(), |mode| drop(prefs::mode().set(mode.to_owned()))))
        .child(heading("Output"))
        .child(muted("Compact folds a run of tool calls into one line. Detailed shows every run open, with each command and what it printed, as Claude Code's verbose mode does. A chat that is open follows a change at once."))
        .child(chips("Tool calls", &outputs, output, |value| drop(prefs::detailed().set(value == "detailed"))))]
}

#[unsafe(no_mangle)]
pub fn start() {
    wings_plugin::run(view);
}
