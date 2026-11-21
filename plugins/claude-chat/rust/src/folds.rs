//! What each kind of fold shows open to start with. In Compact every fold starts shut; in Detailed each kind starts open
//! unless its setting (`prefs::opens`, on the Settings page) says it starts shut. A click on a fold flips what it starts as,
//! as before (`prefs::is_open`).

/// A kind of fold the conversation holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A run of tool calls, as one line.
    Steps,
    /// What a command printed (Bash).
    Command,
    /// The file a read gave back.
    Read,
    /// An edit's diff, a written file's content.
    Edit,
    /// What a Grep or a Glob found.
    Search,
    /// A page fetched or a web search.
    Web,
    /// A subagent call's result.
    Subagent,
    /// What any other tool printed (an MCP tool, a skill).
    Other,
    /// What Claude thought.
    Thought,
    /// What a finished agent or command reported.
    Summary,
    /// What Claude Code wrote in your name: a hook's output, a memory saved, an action auto mode blocked.
    Note,
}

/// Each kind, the key its setting is kept under, and how the Settings page names it.
pub const KINDS: [(Kind, &str, &str); 11] = [
    (Kind::Steps, "open-steps", "Runs of tool calls"),
    (Kind::Command, "open-command", "Command output"),
    (Kind::Read, "open-read", "File reads"),
    (Kind::Edit, "open-edit", "Edits and writes, with their changes"),
    (Kind::Search, "open-search", "Search results"),
    (Kind::Web, "open-web", "Web fetches and searches"),
    (Kind::Subagent, "open-subagent", "Subagent calls"),
    (Kind::Other, "open-other", "Other tools"),
    (Kind::Thought, "open-thought", "Thinking"),
    (Kind::Summary, "open-summary", "Summaries of finished agents and commands"),
    (Kind::Note, "open-note", "Notes from Claude Code, like hook output and saved memories"),
];

impl Kind {
    fn at(self) -> usize {
        KINDS.iter().position(|(kind, ..)| *kind == self).unwrap_or(0)
    }

    pub fn key(self) -> &'static str {
        KINDS[self.at()].1
    }

    /// The kind of fold a tool call's output is.
    pub fn of_tool(name: &str) -> Kind {
        match name {
            "Bash" => Kind::Command,
            "Read" => Kind::Read,
            "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => Kind::Edit,
            "Grep" | "Glob" => Kind::Search,
            "WebFetch" | "WebSearch" => Kind::Web,
            "Task" | "Agent" => Kind::Subagent,
            _ => Kind::Other,
        }
    }
}

/// Which kinds start open in Detailed. All of them until a setting says otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opens([bool; KINDS.len()]);

impl Default for Opens {
    fn default() -> Self {
        Opens([true; KINDS.len()])
    }
}

impl Opens {
    pub fn get(&self, kind: Kind) -> bool {
        self.0[kind.at()]
    }

    pub fn set(&mut self, kind: Kind, open: bool) {
        self.0[kind.at()] = open;
    }

    /// What the settings say now.
    pub fn stored() -> Opens {
        let mut opens = Opens::default();
        for (kind, ..) in KINDS {
            opens.set(kind, crate::prefs::opens(kind).get());
        }
        opens
    }
}

/// Reads the settings into the chat and follows a change of any of them, from this frame or another: the clicks made
/// against the old ones go with them.
#[cfg(feature = "chat")]
pub fn follow() {
    use crate::chat;
    chat::with(|c| {
        c.opens = Opens::stored();
        c.fold_reports = crate::prefs::fold_reports().get();
    });
    crate::prefs::fold_reports().on_change(|fold| {
        chat::with(|c| {
            c.fold_reports = *fold;
            c.open_outputs.clear();
        })
    });
    for (kind, ..) in KINDS {
        crate::prefs::opens(kind).on_change(move |open| {
            chat::with(|c| {
                c.opens.set(kind, *open);
                c.open_steps.clear();
                c.open_outputs.clear();
            })
        });
    }
}

/// Whether a fold of `kind` shows open, given whether you clicked it.
#[cfg(feature = "chat")]
pub fn is_open(c: &crate::chat::Chat, kind: Kind, clicked: bool) -> bool {
    // The Thinking view mode opens what Claude thought and nothing else (`view_mode.rs`). In Verbose your list decides,
    // thoughts too.
    let thinking = !c.detailed && c.show_thinking && kind == Kind::Thought;
    crate::prefs::is_open((c.detailed && c.opens.get(kind)) || thinking, clicked)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_its_own_key_and_starts_open() {
        let keys: std::collections::HashSet<_> = KINDS.iter().map(|(_, key, _)| *key).collect();
        assert_eq!(keys.len(), KINDS.len());
        let opens = Opens::default();
        assert!(KINDS.iter().all(|(kind, ..)| opens.get(*kind) && kind.key().starts_with("open-")));
        // Outside Wings nothing is kept, so the settings read as the defaults.
        assert_eq!(Opens::stored(), opens);
    }

    #[test]
    fn a_kind_set_shut_leaves_the_others_open() {
        let mut opens = Opens::default();
        opens.set(Kind::Command, false);
        assert!(!opens.get(Kind::Command));
        assert!(KINDS.iter().filter(|(kind, ..)| *kind != Kind::Command).all(|(kind, ..)| opens.get(*kind)));
    }

    #[test]
    fn a_tool_is_the_kind_of_fold_its_output_makes() {
        assert_eq!(Kind::of_tool("Bash"), Kind::Command);
        assert_eq!(Kind::of_tool("Write"), Kind::Edit);
        assert_eq!(Kind::of_tool("Glob"), Kind::Search);
        assert_eq!(Kind::of_tool("WebSearch"), Kind::Web);
        assert_eq!(Kind::of_tool("Agent"), Kind::Subagent);
        assert_eq!(Kind::of_tool("mcp__wings__notify"), Kind::Other);
    }
}
