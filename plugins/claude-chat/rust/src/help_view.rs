//! The dialog `/help` opens over the chat: every slash command, the ones that run here and then the ones that run only in
//! Claude Code in a terminal, each with what it does, found by the search field. A click on one that runs here picks it,
//! as the slash menu does.

use wings_plugin::ui::{Entry, Node, dialog as panel, entry, muted, search_field, section, stack, text};

use crate::{
    chat::{self, Chat},
    plugins_view::Budget,
    session::Command,
};

const WIDTH: u32 = 640;

pub fn dialog(c: &Chat) -> Option<Node> {
    let query = c.help.as_deref()?;
    let wanted = query.trim().trim_start_matches('/').to_lowercase();
    let found = |command: &Command| {
        wanted.is_empty()
            || command.name.to_lowercase().contains(&wanted)
            || command.aliases.iter().any(|alias| alias.to_lowercase().contains(&wanted))
            || command.description.to_lowercase().contains(&wanted)
    };
    let mut commands: Vec<Command> = crate::builtin::all(&c.session.commands).into_iter().filter(found).collect();
    commands.sort_by(|a, b| a.name.cmp(&b.name));
    let mut budget = Budget::default();
    let (here, terminal): (Vec<Command>, Vec<Command>) = commands.into_iter().partition(|command| !command.terminal);
    let mut rows = |list: &[Command]| list.iter().filter_map(|command| budget.take(row(command))).collect::<Vec<Node>>();
    let (here_rows, terminal_rows) = (rows(&here), rows(&terminal));
    let sections = [
        (!here.is_empty()).then(|| section(format!("In this chat · {}", here.len()), None, here_rows)),
        (!terminal.is_empty()).then(|| section(format!("Only in Claude Code in a terminal · {}", terminal.len()), Some("Open this session in a terminal to use these."), terminal_rows)),
    ];
    let none = (here.is_empty() && terminal.is_empty()).then(|| muted(format!("No command matches “{}”.", query.trim())).prop("size", "small"));
    let more = (budget.left > 0).then(|| muted(format!("{} more. Search to find them.", budget.left)).prop("size", "small"));
    Some(
        panel("Commands", WIDTH, close)
            .child(search_field("Search commands", query, search))
            .child(stack().prop("gap", "l").children(sections.into_iter().flatten()).maybe(none).maybe(more)),
    )
}

/// A command: its name and what it takes, with its other names, over what it does.
fn row(command: &Command) -> Node {
    let mut name = format!("/{}", command.name);
    if !command.hint.is_empty() {
        name = format!("{name} {}", command.hint);
    }
    let also = (!command.aliases.is_empty()).then(|| format!("Also /{} · ", command.aliases.join(", /"))).unwrap_or_default();
    let detail = format!("{also}{}", command.description);
    let row = entry(Entry {
        lead: None,
        title: text(name.as_str()).prop("size", "small").prop("clamp", true).prop("title", name.as_str()),
        detail: (!detail.is_empty()).then(|| muted(detail.as_str()).prop("size", "small").prop("clamp", true).prop("title", detail.as_str())),
        trailing: None,
    })
    .key(command.name.as_str());
    if command.terminal {
        return row;
    }
    let picked = command.name.clone();
    row.on("onClick", move |_| pick(&picked)).prop("title", format!("Use /{}", command.name))
}

fn pick(name: &str) {
    close();
    chat::insert_command(name);
}

fn close() {
    chat::with(|c| c.help = None);
}

fn search(query: &str) {
    chat::with(|c| c.help = Some(query.to_owned()));
}
