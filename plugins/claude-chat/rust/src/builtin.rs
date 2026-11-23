//! Claude Code's built-in commands the chat answers itself, by opening one of its own views the moment they are sent, with
//! no Claude turn. One table maps a name to what opens: adding one is one line here, which the send and the slash menu
//! both read.
//!
//! A command only counts typed whole: with anything after it, it goes to Claude Code. So does one a plugin or skill
//! registered under the same name. Claude Code lists its own built-ins too, `/mcp` and `/usage` among them, marked
//! `"builtin": true`: those still open here.

use crate::{plugins::Show, session::Command, setup::Which, usage::Panel};

/// What a built-in command opens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Local {
    /// The usage or context panel.
    Panel(Panel),
    /// The plugins dialog, leading with this list.
    Dialog(Show),
    /// The hooks, memory or permissions dialog.
    Setup(Which),
}

/// Each built-in command: its name, what the slash menu says it does (empty for an alias the menu doesn't offer) and what
/// it opens. The menu lists them in this order.
pub const TABLE: &[(&str, &str, Local)] = &[
    ("plugins", "Turn plugins on or off for this chat", Local::Dialog(Show::Plugins)),
    ("plugin", "", Local::Dialog(Show::Plugins)),
    ("mcp", "Turn MCP servers on or off for this project", Local::Dialog(Show::Mcp)),
    ("usage", "Show your plan's limits and credits", Local::Panel(Panel::Usage)),
    ("cost", "Show what this session has cost", Local::Panel(Panel::Usage)),
    ("context", "Show how the context window is spent", Local::Panel(Panel::Context)),
    ("hooks", "List the hooks Claude Code runs", Local::Setup(Which::Hooks)),
    ("memory", "Show the instruction files and memories Claude Code loads", Local::Setup(Which::Memory)),
    ("permissions", "List the allow, ask and deny rules", Local::Setup(Which::Permissions)),
    ("allowed-tools", "", Local::Setup(Which::Permissions)),
];

/// What a draft opens, when the draft is exactly a built-in command that no plugin or skill of Claude Code's took.
pub fn find(text: &str, listed: &[Command]) -> Option<Local> {
    let name = text.trim().strip_prefix('/')?;
    if listed.iter().any(|command| command.name == name && !command.builtin) {
        return None;
    }
    TABLE.iter().find(|(known, ..)| *known == name).map(|(.., local)| *local)
}

/// The table's commands for the slash menu, leaving out the aliases and the ones Claude Code lists.
pub fn menu(listed: &[Command]) -> impl Iterator<Item = Command> + '_ {
    TABLE
        .iter()
        .filter(move |(name, description, _)| !description.is_empty() && listed.iter().all(|command| command.name != *name))
        .map(|(name, description, _)| Command { name: (*name).to_owned(), description: (*description).to_owned(), hint: String::new(), plugin: false, builtin: false })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(names: &[&str]) -> Vec<Command> {
        names.iter().map(|name| Command { name: (*name).into(), description: String::new(), hint: String::new(), plugin: false, builtin: false }).collect()
    }

    #[test]
    fn a_whole_built_in_command_opens_its_view() {
        assert_eq!(find("/usage", &[]), Some(Local::Panel(Panel::Usage)));
        assert_eq!(find(" /plugins\n", &[]), Some(Local::Dialog(Show::Plugins)));
        assert_eq!(find("/plugin", &[]), Some(Local::Dialog(Show::Plugins)));
        assert_eq!(find("/allowed-tools", &listed(&["review"])), Some(Local::Setup(Which::Permissions)));
    }

    #[test]
    fn a_command_with_an_argument_or_not_in_the_table_goes_to_claude_code() {
        assert_eq!(find("/usage now", &[]), None);
        assert_eq!(find("/plugins install toolbox", &[]), None);
        assert_eq!(find("tell me about /usage", &[]), None);
        assert_eq!(find("/usage-report", &[]), None);
        assert_eq!(find("/compact", &[]), None);
        assert_eq!(find("usage", &[]), None);
    }

    #[test]
    fn a_command_claude_code_lists_under_the_same_name_goes_to_claude_code() {
        let registered = listed(&["usage", "review"]);
        assert_eq!(find("/usage", &registered), None);
        assert_eq!(find("/plugins", &registered), Some(Local::Dialog(Show::Plugins)));
        assert!(menu(&registered).all(|command| command.name != "usage"));
    }
}
