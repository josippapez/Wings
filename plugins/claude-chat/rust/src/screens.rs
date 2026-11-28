//! The bare slash commands that open one of the chat's own screens, as VS Code's do, instead of going to Claude Code
//! headless, where `/model` and the rest print a line nobody sees: `/model` and `/effort` (the model dialog), `/rewind`,
//! `/resume`, `/fork` and `/branch`, `/tasks`, `/workflows`, `/bookmarks` and `/copy [N]`, as `builtin::TABLE` names them.
//!
//! Wings lets a page copy only on a click, so `/copy` opens a dialog with the reply and a Copy button, as `/export` does.

use wings_plugin::{
    markdown::code_block,
    ui::{Node, button, chip, chips, dialog as panel, loading, muted, row, stack, text},
};

pub use crate::builtin::Screen;
use crate::{
    builtin::{Found, Local},
    chat::{self, Chat, State},
    session::{Command, Item},
};

const WIDTH: u32 = 560;
/// The lines of the reply the dialog previews.
const PREVIEW: usize = 12;

/// What a draft opens, when it is exactly one of these commands and no plugin or skill of Claude Code's took the name.
pub fn find(text: &str, listed: &[Command]) -> Option<Screen> {
    match crate::builtin::recognize(text, listed)? {
        Found::Own(Local::Screen(screen), _) => Some(screen),
        _ => None,
    }
}

/// Opens the screen, from the Enter that sent its command.
pub fn open(screen: Screen) {
    match screen {
        Screen::Model => chat::with(|c| c.model_dialog = true),
        Screen::Rewind => rewind(),
        Screen::Resume => chat::open_past(),
        Screen::Fork => crate::fork_view::ask_fork(),
        Screen::Panel(group) => reveal(group),
        Screen::Copy(n) => chat::with(|c| match reply(c, n) {
            Some(_) => c.session.cmds.copy = Some(n),
            None => c.session.items.push(Item::Notice(if n == 1 { "Claude hasn't replied yet, so there is nothing to copy".into() } else { format!("There is no reply {n} back to copy") })),
        }),
    }
}

fn note(said: &str) {
    chat::with(|c| c.session.items.push(Item::Notice(said.to_owned())));
}

/// The rewind dialog for your latest message that Claude Code knows by a uuid, which has the files it would restore.
fn rewind() {
    let uuid = chat::with(|c| (c.state == State::Running && !c.session.working).then(|| c.session.prompts.last().map(|prompt| prompt.uuid.clone())));
    match uuid {
        Some(Some(uuid)) => chat::open_rewind(&uuid),
        Some(None) => note("There is no message of yours to rewind to yet"),
        None => note("Rewind waits until Claude has finished"),
    }
}

/// Shows the session panel with `group` open, or says the group has nothing in it yet.
fn reveal(group: &'static str) {
    let (key, empty) = chat::with(|c| match group {
        "workflows" => ("tasks", (!c.session.flows.any()).then_some("No workflows have run in this chat yet")),
        "tasks" => ("tasks", c.session.tasks.is_empty().then_some("No background tasks in this chat yet")),
        _ => ("bookmarks", c.bookmarks.is_empty().then_some("No bookmarked messages yet: the flag under a reply bookmarks it")),
    });
    if let Some(said) = empty {
        return note(said);
    }
    chat::set_panel_open(key, true);
    chat::show_side();
}

/// The `n`th latest reply of Claude's, by its place in the conversation.
fn reply(c: &Chat, n: usize) -> Option<(usize, &str)> {
    c.session.items.iter().enumerate().rev().filter_map(|(at, item)| if let Item::Text(said) = item { Some((at, said.as_str())) } else { None }).nth(n - 1)
}

/// The model dialog while it is open: each model to pick, what the chosen one is for, and the efforts it takes.
pub fn model_dialog(c: &Chat) -> Option<Node> {
    if !c.model_dialog {
        return None;
    }
    let session = &c.session;
    let body = if session.models.is_empty() {
        loading()
    } else {
        let current = session.current_model().or_else(|| session.models.iter().find(|model| model.value == "default"));
        let models = row().prop("gap", "s").prop("wrap", true).children(session.models.iter().map(|model| {
            let value = model.value.clone();
            chip(model.name.as_str(), current.is_some_and(|current| current.value == model.value), move || chat::pick_model(&value)).key(model.value.as_str())
        }));
        let about = current.filter(|model| !model.description.is_empty()).map(|model| muted(model.description.as_str()).prop("size", "small"));
        let efforts = match (current, session.shown_effort()) {
            (Some(model), Some(level)) if !model.efforts.is_empty() => {
                let options: Vec<(&str, String)> = model.efforts.iter().map(|effort| (effort.as_str(), crate::view::effort_name(effort))).collect();
                let options: Vec<(&str, &str)> = options.iter().map(|(value, label)| (*value, label.as_str())).collect();
                Some(chips("Effort", &options, &level, chat::pick_effort))
            }
            _ => None,
        };
        let effort_label = efforts.is_some().then(|| text("Effort").prop("weight", "medium"));
        stack().prop("gap", "m").child(text("Model").prop("weight", "medium")).child(models).maybe(about).maybe(effort_label).maybe(efforts)
    };
    Some(panel("Model and effort", WIDTH, close_model).child(body))
}

fn close_model() {
    chat::with(|c| c.model_dialog = false);
}

/// The copy dialog while it is open: the reply, and a Copy button, which copies it on your click.
pub fn copy_dialog(c: &Chat) -> Option<Node> {
    let n = c.session.cmds.copy?;
    let (at, said) = reply(c, n)?;
    let lines = said.lines().count();
    let shown: Vec<&str> = said.lines().take(PREVIEW).collect();
    let more = lines.saturating_sub(PREVIEW);
    let title = if n == 1 { "Copy the last reply".to_owned() } else { format!("Copy the reply {n} back") };
    Some(panel(&title, WIDTH, close_copy).child(
        stack()
            .prop("gap", "m")
            .child(code_block(&shown.join("\n")))
            .maybe((more > 0).then(|| muted(format!("{more} more lines")).prop("size", "small")))
            .child(row().prop("gap", "s").prop("justify", "end").child(button("Close", "outline", false, close_copy)).child(button("Copy to clipboard", "default", false, move || copy(at)))),
    ))
}

fn close_copy() {
    chat::with(|c| c.session.cmds.copy = None);
}

fn copy(at: usize) {
    close_copy();
    crate::status_view::copy_reply(at);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(names: &[(&str, bool)]) -> Vec<Command> {
        names.iter().map(|(name, builtin)| Command { name: (*name).into(), builtin: *builtin, ..Command::default() }).collect()
    }

    #[test]
    fn the_bare_commands_open_the_chats_own_screens() {
        for (typed, screen) in [
            ("/model", Screen::Model),
            (" /effort\n", Screen::Model),
            ("/rewind", Screen::Rewind),
            ("/resume", Screen::Resume),
            ("/fork", Screen::Fork),
            ("/branch", Screen::Fork),
            ("/continue", Screen::Resume),
            ("/bashes", Screen::Panel("tasks")),
            ("/tasks", Screen::Panel("tasks")),
            ("/workflows", Screen::Panel("workflows")),
            ("/bookmarks", Screen::Panel("bookmarks")),
            ("/copy", Screen::Copy(1)),
            ("/copy 3", Screen::Copy(3)),
        ] {
            assert_eq!(find(typed, &[]), Some(screen), "{typed}");
            // Claude Code lists its own commands of these names, which doesn't take them from the chat.
            assert_eq!(find(typed, &listed(&[("model", true), ("copy", true), ("tasks", true)])), Some(screen), "{typed}");
        }
    }

    #[test]
    fn a_command_with_arguments_a_plugins_namesake_or_another_command_opens_no_screen() {
        for typed in ["/model opus", "/effort high", "/resume abc", "/fork name", "/tasks now", "/copy 0", "/copy two", "/copy 1 2", "/rewinds", "model", "see /model"] {
            assert_eq!(find(typed, &[]), None, "{typed}");
        }
        assert_eq!(find("/model", &listed(&[("model", false)])), None, "a plugin's or skill's /model wins");
    }

    /// The slash menu offers each of them while you type, once: the ones Claude Code lists (`/model`, `/effort`) as it
    /// lists them, the rest from here.
    #[test]
    fn the_slash_menu_offers_every_screen_command_once() {
        let claude_codes = listed(&[("model", true), ("effort", true)]);
        let ours: Vec<String> = crate::builtin::all(&claude_codes).into_iter().map(|command| command.name).filter(|name| ["model", "effort", "rewind", "resume", "fork", "tasks", "workflows", "copy", "bookmarks"].contains(&name.as_str())).collect();
        assert_eq!(ours, ["model", "effort", "rewind", "resume", "fork", "tasks", "workflows", "copy", "bookmarks"]);
        for typed in ["rew", "res", "fo", "tas", "work", "cop", "book"] {
            let names: Vec<String> = crate::commands::menu(&claude_codes, typed).into_iter().map(|command| command.name).collect();
            assert!(names.iter().any(|name| name.starts_with(typed)), "/{typed} offers nothing: {names:?}");
        }
    }
}
