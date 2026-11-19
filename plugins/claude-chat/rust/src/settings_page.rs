//! The Claude chat page in Wings Settings: the mode a new chat starts in, how much of its tool calls a chat shows and which
//! kinds of fold Detailed opens, and how big its text is.

use wings_plugin::ui::{Node, checkbox, chips as choose, heading as section, muted as note, row, stack};

/// The most points wide a description runs, so a line stays a readable length on a wide page.
const MEASURE: u32 = 560;

use crate::{folds::KINDS, prefs, session::MODES};

/// A section's name, in the page's section heading style, stronger than the description under it.
fn heading(text: &str) -> Node {
    section(3, text)
}

/// A description, held to a readable line length. The app centres a stack with a `maxWidth`, so a growing stack after it
/// in a row takes the room left and keeps it at the left edge.
fn muted(text: &str) -> Node {
    row().child(stack().prop("maxWidth", MEASURE).child(note(text))).child(stack().prop("grow", true))
}

/// A choice between a few, on the segmented track other Wings settings use.
fn chips(label: &str, options: &[(&str, &str)], value: &str, on_change: impl Fn(&str) + 'static) -> Node {
    choose(label, options, value, on_change).prop("segmented", true)
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
        .child(heading("Session panel"))
        .child(muted("Where a chat shows its session: what changed, the context and usage meters, background tasks, subagents and terminals. In the chat it sits beside the conversation, and over it in a narrow tab. In Wings' sidebar it opens in the right panel and follows the chat in front. A chat that is open follows a change at once."))
        .child(chips("Session panel", &prefs::SESSION_PANELS, &prefs::session_panel().get(), |place| drop(prefs::session_panel().set(place.to_owned()))))
        .child(heading("Text size"))
        .child(muted("The size of the text in the conversation, in every chat. A chat that is open follows a change at once."))
        .child(chips("Text size", &prefs::TEXT_SIZES, &prefs::text_size().get(), |size| drop(prefs::text_size().set(size.to_owned()))))]
}

#[unsafe(no_mangle)]
pub fn start() {
    wings_plugin::run(view);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_description_is_held_to_a_readable_width_and_a_choice_sits_on_a_track() {
        let tree = wings_plugin::ui::tree_json(muted("About"));
        assert_eq!(tree["children"][0]["props"]["maxWidth"], MEASURE);
        let tree = wings_plugin::ui::tree_json(chips("Mode", &[("a", "A")], "a", |_| {}));
        assert_eq!(tree["props"]["segmented"], true);
        let tree = wings_plugin::ui::tree_json(heading("Output"));
        assert_eq!(tree["props"]["level"], 3);
    }
}
