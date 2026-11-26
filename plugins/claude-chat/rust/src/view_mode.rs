//! The chat's view modes, named as Claude Code's own are (Claude Desktop's Transcript view): Normal folds tool calls into
//! summaries, Thinking also shows what Claude thought, and Verbose opens every run of tool calls with each call's output,
//! thoughts included. Verbose is the Detailed setting of Settings (`prefs::detailed`); Thinking is `prefs::thinking`, which
//! opens only the thoughts (`folds::is_open`). Both are kept for every chat, so a mode holds across tabs and restarts.

#[cfg(feature = "chat")]
use wings_plugin::ui::Node;

use crate::prefs;
#[cfg(feature = "chat")]
use crate::chat::Chat;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Thinking,
    Verbose,
}

impl Mode {
    /// The mode two settings make: Verbose wins, as it shows everything Thinking does.
    pub fn of(detailed: bool, thinking: bool) -> Mode {
        match (detailed, thinking) {
            (true, _) => Mode::Verbose,
            (false, true) => Mode::Thinking,
            (false, false) => Mode::Normal,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Mode::Normal => "Normal",
            Mode::Thinking => "Thinking",
            Mode::Verbose => "Verbose",
        }
    }

    /// What it shows, for the button's tooltip.
    #[cfg(feature = "chat")]
    fn shows(self) -> &'static str {
        match self {
            Mode::Normal => "tool calls folded into summaries",
            Mode::Thinking => "tool calls folded, with Claude's thinking",
            Mode::Verbose => "every tool call with its output, and Claude's thinking",
        }
    }

    /// Ctrl+O cycles in this order, as in Claude Desktop.
    #[cfg(any(feature = "chat", test))]
    pub fn next(self) -> Mode {
        match self {
            Mode::Normal => Mode::Thinking,
            Mode::Thinking => Mode::Verbose,
            Mode::Verbose => Mode::Normal,
        }
    }

    /// The (Detailed, Thinking) settings this mode is.
    fn settings(self) -> (bool, bool) {
        (self == Mode::Verbose, self != Mode::Normal)
    }
}

/// Makes `mode` the mode of every chat.
pub fn set(mode: Mode) {
    let (detailed, thinking) = mode.settings();
    drop(prefs::detailed().set(detailed));
    drop(prefs::thinking().set(thinking));
}

#[cfg(feature = "chat")]
/// The header's mode menu: the mode in use with a chevron, and the three to pick. Ctrl+O still goes on to the next, as the
/// shortcut of that mode's row.
pub fn mode_menu(c: &Chat) -> Node {
    use wings_plugin::ui::{icon, icons, menu, text};
    let mode = Mode::of(c.detailed, c.show_thinking);
    let title = format!("View mode: {}, {}. Ctrl+O goes on to {}", mode.name(), mode.shows(), mode.next().name());
    let trigger = Node::new("Button")
        .prop("variant", "ghost")
        .prop("size", "sm")
        .prop("label", "View mode")
        .prop("title", title.as_str())
        .child(text(mode.name()))
        .child(icon(24, 2.0, &icons::CHEVRON_DOWN).prop("size", 12));
    let rows = [Mode::Normal, Mode::Thinking, Mode::Verbose].map(|row| {
        let item = Node::new("Button").key(row.name()).prop("pressed", row == mode).prop("title", row.shows()).on("onClick", move |_| set(row)).text(row.name());
        if row == mode.next() { item.shortcut("ctrl-o") } else { item }
    });
    menu(trigger, rows, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_modes_are_the_two_settings_and_cycle_normal_thinking_verbose() {
        assert_eq!(Mode::of(false, false), Mode::Normal);
        assert_eq!(Mode::of(false, true), Mode::Thinking);
        assert_eq!(Mode::of(true, false), Mode::Verbose, "Detailed from Settings alone is Verbose");
        assert_eq!(Mode::of(true, true), Mode::Verbose);
        assert_eq!([Mode::Normal.next(), Mode::Thinking.next(), Mode::Verbose.next()], [Mode::Thinking, Mode::Verbose, Mode::Normal]);
        // Each mode reads back as itself from the settings it writes.
        for mode in [Mode::Normal, Mode::Thinking, Mode::Verbose] {
            let (detailed, thinking) = mode.settings();
            assert_eq!(Mode::of(detailed, thinking), mode);
        }
    }

    #[test]
    #[cfg(feature = "chat")]
    fn thinking_opens_what_claude_thought_and_leaves_tool_calls_folded() {
        use crate::{chat, folds::{self, Kind}};
        chat::with(|c| {
            c.detailed = false;
            c.show_thinking = false;
        });
        assert!(!chat::with(|c| folds::is_open(c, Kind::Thought, false)));
        chat::with(|c| c.show_thinking = true);
        assert!(chat::with(|c| folds::is_open(c, Kind::Thought, false)));
        assert!(!chat::with(|c| folds::is_open(c, Kind::Steps, false)));
        assert!(!chat::with(|c| folds::is_open(c, Kind::Command, false)));
        // A click on a thought still shuts it.
        assert!(!chat::with(|c| folds::is_open(c, Kind::Thought, true)));
        chat::with(|c| c.show_thinking = false);
    }

    #[test]
    #[cfg(feature = "chat")]
    fn verbose_keeps_shut_what_its_list_clears_thoughts_and_notes_too() {
        use crate::{chat, folds::{self, Kind}};
        let (detailed, thinking) = Mode::Verbose.settings();
        chat::with(|c| {
            c.detailed = detailed;
            c.show_thinking = thinking;
            c.opens = folds::Opens::default();
        });
        assert!(chat::with(|c| folds::is_open(c, Kind::Thought, false) && folds::is_open(c, Kind::Note, false)));
        chat::with(|c| {
            c.opens.set(Kind::Thought, false);
            c.opens.set(Kind::Note, false);
        });
        assert!(!chat::with(|c| folds::is_open(c, Kind::Thought, false)), "a cleared Thinking stays shut in Verbose");
        assert!(!chat::with(|c| folds::is_open(c, Kind::Note, false)));
        chat::with(|c| {
            c.detailed = false;
            c.show_thinking = false;
            c.opens = folds::Opens::default();
        });
    }
}
