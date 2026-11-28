//! Every slash command the chat knows apart from the ones Claude Code lists, in one place: the ones it answers itself
//! (`TABLE`, adding one is one line), and Claude Code's that run only in its terminal (`terminal_commands.rs`, read from
//! Claude Code itself), which the menu offers with a note. The send, the slash menu and `/help` all read this.
//!
//! What a typed `/name` does, in order: a plugin's, skill's or your own command Claude Code lists under that name or alias
//! goes to Claude Code. One of `TABLE`'s, by its name or an alias Claude Code gives that name, is answered here, Claude
//! Code's own of the same name included (it lists `/mcp` and `/usage`, `"builtin": true`, which still open here); with
//! what it doesn't take after it, it goes to Claude Code when Claude Code lists it (`/model opus`), else the chat says
//! how it is typed. Anything else Claude Code lists goes to it. A terminal command gets its note. Any other `/word` is
//! sent as text, which Claude Code also does with a `/word` it doesn't know.

use crate::{
    plugins::Show,
    session::Command,
    setup::Which,
    terminal_commands::{COMMANDS as TERMINAL, Terminal},
    usage::Panel,
};

/// Where `/release-notes` goes: the changelog Claude Code's own `/release-notes` reads.
const CHANGELOG: &str = "https://code.claude.com/docs/en/changelog";

/// The bare slash commands that open one of the chat's own screens (`screens.rs`), as VS Code's do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Screen {
    /// The model and effort pickers, in a dialog.
    Model,
    /// The rewind dialog, for your latest message.
    Rewind,
    /// Wings' Past sessions.
    Resume,
    /// The header's Fork.
    Fork,
    /// The side panel's group of this name.
    Panel(&'static str),
    /// The dialog that copies the `n`th latest reply, the latest being 1.
    Copy(usize),
}

/// What one of the chat's own commands does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Local {
    /// The usage or context panel.
    Panel(Panel),
    /// The plugins dialog, leading with this list.
    Dialog(Show),
    /// The hooks, memory or permissions dialog.
    Setup(Which),
    /// The skills dialog.
    Skills,
    Screen(Screen),
    /// Dialogs filled by a control request (`commands.rs`).
    Status,
    Export,
    Btw,
    /// Plan mode, and the task after it if there is one.
    Plan,
    /// Remote Control on, under the name after it, or off.
    RemoteControl,
    /// Claude Code started again with the folder after it.
    AddDir,
    /// The chat's own sign-in.
    Login,
    /// Closes the tab, and the Claude Code it runs.
    Exit,
    /// Every command, in a dialog, found by what follows.
    Help,
    /// The diff viewer on this folder's changes.
    Diff,
    /// A page in your browser.
    Open(&'static str),
}

impl Local {
    /// Whether it takes `rest` after its name.
    fn takes(self, rest: &str) -> bool {
        match self {
            Local::Screen(Screen::Copy(_)) => rest.is_empty() || rest.parse::<usize>().is_ok_and(|n| n >= 1),
            Local::Btw => !rest.is_empty(),
            Local::Plan | Local::RemoteControl | Local::AddDir | Local::Help => true,
            _ => rest.is_empty(),
        }
    }
}

/// The commands the chat answers itself: each its name, what the slash menu says it does, what it takes and what it does.
/// The other names Claude Code gives a command (`/cost` for `/usage`, `/quit` for `/exit`) run it too. The menu lists them
/// in this order.
pub const TABLE: &[(&str, &str, &str, Local)] = &[
    ("status", "Show the version, account, model and settings sources", "", Local::Status),
    ("export", "Export the conversation as text", "", Local::Export),
    ("btw", "Ask a side question without adding it to the conversation", "<question>", Local::Btw),
    ("plan", "Switch to plan mode, and plan a task", "[task]", Local::Plan),
    ("remote-control", "Continue this chat from claude.ai or the Claude app", "[name]", Local::RemoteControl),
    ("plugins", "Turn plugins on or off for this chat", "", Local::Dialog(Show::Plugins)),
    ("mcp", "Turn MCP servers on or off for this project", "", Local::Dialog(Show::Mcp)),
    ("usage", "Show your plan's limits and credits, and what this session has cost", "", Local::Panel(Panel::Usage)),
    ("context", "Show how the context window is spent", "", Local::Panel(Panel::Context)),
    ("hooks", "List the hooks Claude Code runs", "", Local::Setup(Which::Hooks)),
    ("memory", "Show the instruction files and memories Claude Code loads", "", Local::Setup(Which::Memory)),
    ("permissions", "List the allow, ask and deny rules", "", Local::Setup(Which::Permissions)),
    ("skills", "List the skills Claude Code has, and change how Claude sees each", "", Local::Skills),
    ("model", "Choose the model and how hard it thinks", "", Local::Screen(Screen::Model)),
    ("effort", "Choose how hard Claude thinks", "", Local::Screen(Screen::Model)),
    ("rewind", "Go back to your latest message, files and all", "", Local::Screen(Screen::Rewind)),
    ("resume", "Open a past session", "", Local::Screen(Screen::Resume)),
    ("fork", "Branch this conversation into a new chat", "", Local::Screen(Screen::Fork)),
    ("branch", "Branch this conversation into a new chat, as /fork does", "", Local::Screen(Screen::Fork)),
    ("tasks", "Show the background tasks", "", Local::Screen(Screen::Panel("tasks"))),
    ("workflows", "Show the workflows this chat ran", "", Local::Screen(Screen::Panel("workflows"))),
    ("copy", "Copy Claude's latest reply, or the Nth back", "[N]", Local::Screen(Screen::Copy(1))),
    ("bookmarks", "Show your bookmarked messages", "", Local::Screen(Screen::Panel("bookmarks"))),
    ("diff", "Review the changes in this folder in the diff viewer", "", Local::Diff),
    ("add-dir", "Let Claude work in another folder too", "<path>", Local::AddDir),
    ("help", "List every command", "[command]", Local::Help),
    ("login", "Sign in to Claude Code", "", Local::Login),
    ("release-notes", "Open Claude Code's changelog", "", Local::Open(CHANGELOG)),
    ("exit", "Close this chat", "", Local::Exit),
];

/// What a typed command is, when it isn't one to send to Claude Code.
#[derive(Debug, PartialEq)]
pub enum Found<'a> {
    /// One of the chat's own, and what follows its name.
    Own(Local, &'a str),
    /// One of the chat's own with what it doesn't take after it: its name and what it takes.
    Misused(&'static str, &'static str),
    /// One of Claude Code's that runs only in its terminal.
    Terminal(&'static Terminal),
}

/// The names that run the same command as `name`: those of Claude Code's listed command that answers to it, else of its
/// terminal one, else `name` alone.
fn names<'a>(name: &'a str, listed: &'a [Command]) -> Vec<&'a str> {
    if let Some(command) = listed.iter().find(|command| command.answers(name)) {
        return std::iter::once(command.name.as_str()).chain(command.aliases.iter().map(String::as_str)).collect();
    }
    match terminal(name) {
        Some(found) => std::iter::once(found.name).chain(found.aliases.iter().copied()).collect(),
        None => vec![name],
    }
}

fn terminal(name: &str) -> Option<&'static Terminal> {
    TERMINAL.iter().find(|found| found.name == name || found.aliases.contains(&name))
}

/// The row of `TABLE` that `/name` runs.
fn own(name: &str, listed: &[Command]) -> Option<&'static (&'static str, &'static str, &'static str, Local)> {
    let names = names(name, listed);
    TABLE.iter().find(|(own, ..)| names.contains(own))
}

/// What the draft `text` is, when it is a `/name` the chat answers itself rather than sends.
pub fn recognize<'a>(text: &'a str, listed: &[Command]) -> Option<Found<'a>> {
    let typed = text.trim().strip_prefix('/')?;
    let (name, rest) = typed.split_once(char::is_whitespace).map_or((typed, ""), |(name, rest)| (name, rest.trim()));
    if name.is_empty() || name.contains('/') {
        return None;
    }
    let lists = listed.iter().find(|command| command.answers(name));
    if lists.is_some_and(|command| !command.builtin) {
        return None;
    }
    if let Some((own, _, hint, local)) = own(name, listed) {
        return match (local.takes(rest), lists) {
            (true, _) => Some(Found::Own(with_number(*local, rest), rest)),
            (false, Some(_)) => None,
            (false, None) => Some(Found::Misused(own, hint)),
        };
    }
    if lists.is_some() {
        return None;
    }
    terminal(name).map(Found::Terminal)
}

/// `/copy 3` copies the third reply back.
fn with_number(local: Local, rest: &str) -> Local {
    match local {
        Local::Screen(Screen::Copy(_)) => Local::Screen(Screen::Copy(rest.parse().unwrap_or(1))),
        other => other,
    }
}

/// What the dialogs and panels of the table open, when the draft is exactly one of them.
pub fn find(text: &str, listed: &[Command]) -> Option<Local> {
    match recognize(text, listed)? {
        Found::Own(local @ (Local::Panel(_) | Local::Dialog(_) | Local::Setup(_) | Local::Skills), _) => Some(local),
        _ => None,
    }
}

/// Whether picking `name` in the slash menu runs it at once, as Enter on the typed name does: a command the chat answers
/// alone, and a terminal one, which says so. The rest are filled in for what you write after them: a task, a name, a
/// folder, or what a skill takes.
pub fn runs_bare(name: &str, listed: &[Command]) -> bool {
    match recognize(&format!("/{name}"), listed) {
        Some(Found::Own(local, _)) => !matches!(local, Local::Plan | Local::RemoteControl | Local::AddDir),
        Some(Found::Terminal(_)) => true,
        _ => false,
    }
}

/// What the chat says to a command it can't run as typed.
pub fn note(found: &Found) -> Option<String> {
    match found {
        Found::Misused(name, "") => Some(format!("Type /{name} alone, with nothing after it")),
        Found::Misused(name, hint) => Some(format!("Type /{name} {hint}")),
        Found::Terminal(command) => Some(format!("/{} runs only in Claude Code in a terminal", command.name)),
        Found::Own(..) => None,
    }
}

/// Every command, for the slash menu and `/help`: Claude Code's as it lists them, then the chat's own it doesn't list,
/// each with the other names Claude Code gives it, then its terminal ones that neither has.
pub fn all(listed: &[Command]) -> Vec<Command> {
    let aliases = |name: &str| names(name, listed).into_iter().filter(|other| *other != name).map(str::to_owned).collect();
    let own = TABLE.iter().filter(|(name, ..)| listed.iter().all(|command| command.name != *name)).map(|(name, description, hint, _)| Command {
        name: (*name).to_owned(),
        description: (*description).to_owned(),
        hint: (*hint).to_owned(),
        aliases: aliases(name),
        ..Command::default()
    });
    let terminal = TERMINAL.iter().filter(|found| listed.iter().all(|command| !command.answers(found.name)) && own_of(found).is_none()).map(|found| Command {
        name: found.name.to_owned(),
        description: found.description.to_owned(),
        hint: found.hint.to_owned(),
        aliases: found.aliases.iter().map(|alias| (*alias).to_owned()).collect(),
        terminal: true,
        ..Command::default()
    });
    listed.iter().cloned().chain(own).chain(terminal).collect()
}

/// The row of `TABLE` that answers a terminal command instead.
fn own_of(found: &Terminal) -> Option<&'static str> {
    TABLE.iter().map(|(name, ..)| *name).find(|name| found.name == *name || found.aliases.contains(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(names: &[(&str, bool)]) -> Vec<Command> {
        names.iter().map(|(name, builtin)| Command { name: (*name).into(), builtin: *builtin, ..Command::default() }).collect()
    }

    #[test]
    fn a_whole_built_in_command_opens_its_view() {
        assert_eq!(find("/usage", &[]), Some(Local::Panel(Panel::Usage)));
        assert_eq!(find(" /plugins\n", &[]), Some(Local::Dialog(Show::Plugins)));
        assert_eq!(find("/plugin", &[]), Some(Local::Dialog(Show::Plugins)));
        assert_eq!(find("/allowed-tools", &listed(&[("review", false)])), Some(Local::Setup(Which::Permissions)));
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
        let registered = listed(&[("usage", false), ("review", false)]);
        assert_eq!(find("/usage", &registered), None);
        assert_eq!(find("/plugins", &registered), Some(Local::Dialog(Show::Plugins)));
        assert!(all(&registered).iter().filter(|command| command.name == "usage").count() == 1);
    }

    /// Claude Code 2.1.332 lists `/clear` with `"aliases": ["reset", "new"]` and `/usage` with `["cost", "stats"]`: the
    /// chat reads them, so an alias runs what its command runs, and one a plugin's command has takes the name from the chat
    /// as its name would.
    #[test]
    fn an_alias_runs_its_command_and_a_plugins_alias_wins_over_the_chats_namesake() {
        let commands = crate::session::commands(&serde_json::json!([
            { "name": "clear", "description": "Start a new session", "argumentHint": "[name]", "aliases": ["reset", "new"], "builtin": true },
            { "name": "usage", "description": "Show session cost", "argumentHint": "", "aliases": ["cost", "stats"], "builtin": true },
            { "name": "deploy", "description": "Ship it", "argumentHint": "", "aliases": ["status"] },
        ]));
        assert_eq!(commands[0].aliases, ["reset", "new"]);
        assert_eq!(recognize("/stats", &commands), Some(Found::Own(Local::Panel(Panel::Usage), "")));
        assert_eq!(recognize("/reset", &commands), None, "Claude Code's /clear runs it");
        assert_eq!(recognize("/status", &commands), None, "the plugin's alias");
        assert_eq!(recognize("/status", &[]), Some(Found::Own(Local::Status, "")));
        // The aliases Claude Code's terminal table gives (`/quit`, `/rc`, `/checkpoint`) run the chat's own too.
        assert_eq!(recognize("/quit", &[]), Some(Found::Own(Local::Exit, "")));
        assert_eq!(recognize(" /rc  Shop checkout \n", &[]), Some(Found::Own(Local::RemoteControl, "Shop checkout")));
        assert_eq!(recognize("/checkpoint", &[]), Some(Found::Own(Local::Screen(Screen::Rewind), "")));
        // The menu shows a command once, with its other names.
        let usage = all(&commands).into_iter().find(|command| command.name == "usage").unwrap();
        assert_eq!(usage.aliases, ["cost", "stats"]);
        assert!(all(&commands).iter().all(|command| command.name != "cost"));
    }

    /// `claude -p` answers each of these with "/name isn't available in this environment.": the chat says where it runs
    /// instead, or how it is typed here, and sends none of them.
    #[test]
    fn a_terminal_command_gets_a_note_and_a_misused_one_says_how_it_is_typed() {
        let Some(Found::Terminal(theme)) = recognize("/theme", &[]) else { panic!() };
        assert_eq!(note(&Found::Terminal(theme)).unwrap(), "/theme runs only in Claude Code in a terminal");
        assert!(matches!(recognize("/theme dark", &[]), Some(Found::Terminal(_))));
        assert!(matches!(recognize("/ios", &[]), Some(Found::Terminal(command)) if command.name == "mobile"));
        assert_eq!(recognize("/tasks now", &[]), Some(Found::Misused("tasks", "")));
        assert_eq!(note(&recognize("/copy two", &[]).unwrap()).unwrap(), "Type /copy [N]");
        assert_eq!(note(&recognize("/btw", &[]).unwrap()).unwrap(), "Type /btw <question>");
        // Claude Code lists /model, which takes a name in -p: with one it goes there.
        assert_eq!(recognize("/model opus", &listed(&[("model", true)])), None);
        // A /word nobody knows is a message, and so is a path.
        assert_eq!(recognize("/frobnicate the cache", &[]), None);
        assert_eq!(recognize("/tmp/notes.md says hi", &[]), None);
        // A plugin's or skill's command of a terminal command's name is theirs.
        assert_eq!(recognize("/theme", &listed(&[("theme", false)])), None);
    }

    #[test]
    fn the_chats_own_take_what_they_take() {
        assert_eq!(recognize("/plan", &[]), Some(Found::Own(Local::Plan, "")), "Claude Code 2.1.332 can't run a bare /plan in -p");
        assert_eq!(recognize("/plan add a dark mode", &[]), Some(Found::Own(Local::Plan, "add a dark mode")));
        assert_eq!(recognize("/btw  what is a monad? ", &[]), Some(Found::Own(Local::Btw, "what is a monad?")));
        assert_eq!(recognize("/copy 3", &[]), Some(Found::Own(Local::Screen(Screen::Copy(3)), "3")));
        assert_eq!(recognize("/help usage", &[]), Some(Found::Own(Local::Help, "usage")));
        assert_eq!(recognize("/add-dir \"/work/my docs\"", &[]), Some(Found::Own(Local::AddDir, "\"/work/my docs\"")));
        assert_eq!(recognize("/release-notes", &[]), Some(Found::Own(Local::Open(CHANGELOG), "")));
        assert_eq!(recognize("/diff", &[]), Some(Found::Own(Local::Diff, "")));
        assert_eq!(recognize("/login", &[]), Some(Found::Own(Local::Login, "")));
        assert_eq!(recognize("/rcx", &[]), None);
        assert_eq!(recognize("/remote-controls", &[]), None);
    }

    /// Every terminal command is offered once, under the chat's own when it has one, and never one Claude Code lists.
    #[test]
    fn the_menu_offers_every_terminal_command_that_neither_claude_code_nor_the_chat_runs() {
        let commands = all(&listed(&[("color", true)]));
        let names: Vec<&str> = commands.iter().map(|command| command.name.as_str()).collect();
        let once = names.iter().all(|name| names.iter().filter(|other| *other == name).count() == 1);
        assert!(once, "{names:?}");
        let theme = commands.iter().find(|command| command.name == "theme").unwrap();
        assert!(theme.terminal && !theme.description.is_empty());
        assert!(commands.iter().any(|command| command.name == "color" && !command.terminal), "Claude Code runs /color");
        // /plugin is the chat's /plugins, and /quit is /exit's.
        assert!(!names.contains(&"plugin") && !names.contains(&"quit"));
        let plugins = commands.iter().find(|command| command.name == "plugins").unwrap();
        assert!(plugins.aliases.contains(&"plugin".to_owned()) && !plugins.terminal);
        for found in TERMINAL {
            assert!(names.contains(&found.name) || own_of(found).is_some() || found.name == "color", "{}", found.name);
        }
    }

    /// The terminal table was read from one Claude Code (`terminal_commands.py`). When the installed one has other
    /// commands that `-p` can't run, this fails and says how to write the table again. With no `claude` or `python3` on
    /// the PATH it checks nothing.
    #[test]
    fn the_terminal_table_is_the_installed_claude_codes() {
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/terminal_commands.py");
        let Ok(out) = std::process::Command::new("python3").arg(script).arg("--check").output() else { return };
        assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    }

    #[test]
    fn picking_a_command_that_takes_nothing_runs_it_and_the_rest_are_filled_in() {
        assert!(["mcp", "theme", "status", "quit", "copy", "help"].iter().all(|name| runs_bare(name, &[])));
        assert!(["plan", "btw", "remote-control", "add-dir", "review"].iter().all(|name| !runs_bare(name, &[])));
    }
}
