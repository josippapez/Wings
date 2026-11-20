//! The Claude chat page in Wings Settings: the mode a new chat starts in, how much of its tool calls a chat shows and which
//! kinds of fold Detailed opens, how big its text is, and what lies behind it.

use wings_plugin::ui::{Node, checkbox, chips as choose, heading as section, muted as note, row, select, stack};

/// The most points wide a description runs, so a line stays a readable length on a wide page.
const MEASURE: u32 = 560;

use crate::{claude_version, folds::KINDS, prefs, session::MODES};

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
        .child(heading("Remote Control"))
        .child(muted("Each new chat turns on Remote Control as it starts, so you can follow and answer it on claude.ai or in the Claude app. The button by the model menu still turns it off for that chat."))
        .child(checkbox("Turn on Remote Control in every new chat", prefs::remote_on_start().get(), |on| drop(prefs::remote_on_start().set(on))).key("remote-on-start"))
        .child(heading("Session panel"))
        .child(muted("Where a chat shows its session: what changed, the context and usage meters, background tasks, subagents and terminals. In the chat it sits beside the conversation, and over it in a narrow tab. In Wings' sidebar it opens in the right panel and follows the chat in front. A chat that is open follows a change at once."))
        .child(chips("Session panel", &prefs::SESSION_PANELS, &prefs::session_panel().get(), |place| drop(prefs::session_panel().set(place.to_owned()))))
        .child(heading("Text size"))
        .child(muted("The size of the text in the conversation, in every chat. A chat that is open follows a change at once."))
        .child(chips("Text size", &prefs::TEXT_SIZES, &prefs::text_size().get(), |size| drop(prefs::text_size().set(size.to_owned()))))
        .child(heading("Background"))
        .child(muted("What lies behind the conversation, in every chat. Pixel drift, Pixel rain, Checker shimmer and Embers are dim patterns of pixels in the chat's colours; all but Pixel drift need a Wings after 0.2.60. Halo, Pensive, Mint, Interstella and Nighty night are dim moving gradients in the colours of ShaderGradient's presets, and need Wings 0.2.60. Each moves slowly while Wings is in front and stands still when your Mac reduces motion. A chat that is open follows a change at once."))
        .child(background())
        .child(heading("Claude Code"))
        .child(muted("The version of Claude Code that every chat runs, and whether a newer one is out. Update Claude Code runs `claude upgrade`."))
        .child(claude_version::section())]
}

/// The background's choice: a menu, as ten don't fit on a track in a narrow page.
fn background() -> Node {
    let options: Vec<(String, String)> =
        prefs::BACKGROUNDS.iter().map(|(value, label)| ((*value).to_owned(), (*label).to_owned())).collect();
    select("Background", &options, &prefs::background().get(), |kind| drop(prefs::background().set(kind.to_owned())))
}

// A test build holds the main frame's `start` too: only the page's module exports this one.
#[cfg_attr(not(test), unsafe(no_mangle))]
pub fn start() {
    claude_version::check();
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

    #[test]
    fn the_background_is_a_choice_of_the_plain_ground_pixel_drift_or_a_gradient_and_starts_plain() {
        fn select_named<'a>(node: &'a serde_json::Value, label: &str) -> Option<&'a serde_json::Value> {
            if node["type"] == "Select" && node["props"]["label"] == label {
                return Some(node);
            }
            node["children"].as_array()?.iter().find_map(|child| select_named(child, label))
        }
        let page = wings_plugin::ui::tree_json(view().remove(0));
        let select = select_named(&page, "Background").expect("the background's choice");
        assert_eq!(select["props"]["value"], "plain");
        let options = select["props"]["options"].as_array().unwrap();
        let labels: Vec<&str> = options.iter().map(|option| option["label"].as_str().unwrap()).collect();
        assert_eq!(
            labels,
            ["Plain", "Pixel drift", "Pixel rain", "Checker shimmer", "Embers", "Halo", "Pensive", "Mint", "Interstella", "Nighty night"]
        );
        assert_eq!(options[2]["value"], "pixels:rain");
        assert_eq!(options[5]["value"], "gradient:halo");
    }
}
