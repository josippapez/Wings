//! The chat's view modes, named as Claude Code's own are (Claude Desktop's Transcript view): Normal folds tool calls into
//! summaries, Thinking also shows what Claude thought, and Verbose opens every run of tool calls with each call's output,
//! thoughts included. Verbose is the Detailed setting of Settings (`prefs::detailed`); Thinking is `prefs::thinking`, which
//! opens only the thoughts (`folds::is_open`). Both are kept for every chat, so a mode holds across tabs and restarts.

#[cfg(feature = "chat")]
use wings_plugin::ui::{Node, button};

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
fn cycle() {
    set(Mode::of(prefs::detailed().get(), prefs::thinking().get()).next());
}

#[cfg(feature = "chat")]
/// The header's button: the mode in use, which a click or Ctrl+O changes to the next.
pub fn mode_button(c: &Chat) -> Node {
    let mode = Mode::of(c.detailed, c.show_thinking);
    let title = format!("View mode: {}, {}. Click to go on to {}", mode.name(), mode.shows(), mode.next().name());
    button(mode.name(), "ghost", false, cycle).prop("size", "sm").prop("label", "View mode").prop("title", title.as_str()).shortcut("ctrl-o")
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
}
