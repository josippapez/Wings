//! The dialogs `/hooks`, `/memory` and `/permissions` open over the chat, as Claude Code's own screens list them: the
//! hooks by event, the instruction files Claude Code reads with the room each takes and the memories it saved, and the
//! permission rules by allow, ask and deny with the mode a session starts in. They only list. Wings has no call that
//! opens a file, so a file's path is copied, to open in your editor or Finder.

use serde_json::json;
use wings_plugin::{
    format::{ago, tokens},
    host,
    ui::{Node, badge, button, chips, dialog as panel, empty, heading, loading, muted, notice, row, search_field, stack, text, title},
};

use crate::{
    chat::{self, Chat},
    plugins_view::Budget,
    setup::{File, Hook, Hooks, Memory, Rule, Setup, Which, mode_name, source_file, source_label},
};

const WIDTH: u32 = 640;
/// The most saved memories listed: Claude Code lists up to 200.
const SAVED_SHOWN: usize = 30;

pub fn dialog(c: &Chat) -> Option<Node> {
    let setup = &c.session.setup;
    Some(match setup.open? {
        Which::Hooks => hooks_dialog(setup),
        Which::Memory => memory_dialog(setup),
        Which::Permissions => permissions_dialog(setup),
    })
}

fn small(said: impl Into<String>) -> Node {
    muted(said).prop("size", "small")
}

/// A refused read says why, and one not answered yet loads.
fn filled<T>(answer: &Option<Result<T, String>>, body: impl FnOnce(&T) -> Node) -> Node {
    match answer {
        None => loading(),
        Some(Err(why)) => notice("danger", why.as_str()),
        Some(Ok(answer)) => body(answer),
    }
}

fn notices(list: &[(&'static str, String)]) -> impl Iterator<Item = Node> {
    list.iter().map(|(tone, said)| notice(tone, said.as_str()))
}

fn hooks_dialog(setup: &Setup) -> Node {
    panel("Hooks", WIDTH, close)
        .child(filled(&setup.hooks, hooks_body))
        .child(small("This list is read-only, as /hooks is in a terminal. To add or change a hook, edit the settings file it comes from, or ask Claude."))
}

fn hooks_body(hooks: &Hooks) -> Node {
    let body = stack().prop("gap", "l").children(notices(&hooks.notices));
    if hooks.hooks.is_empty() {
        return body.child(empty("No hooks", "Claude Code runs no hooks of yours here."));
    }
    let mut budget = Budget::default();
    let events = hooks.events.iter().filter_map(|(event, summary)| {
        let rows: Vec<Node> = hooks.hooks.iter().enumerate().filter(|(_, hook)| hook.event == *event).filter_map(|(at, hook)| budget.take(hook_row(at, hook))).collect();
        let head = row().prop("gap", "s").prop("align", "baseline").child(heading(3, event.as_str())).child(small(summary.as_str()));
        (!rows.is_empty()).then(|| stack().prop("gap", "s").child(head).children(rows))
    });
    let body = body.children(events.collect::<Vec<_>>());
    body.maybe((budget.left > 0).then(|| small(format!("{} more", budget.left))))
}

/// A hook: what it runs for, what it runs, whether it is kept from running, and where it is set.
fn hook_row(at: usize, hook: &Hook) -> Node {
    let matcher = (!hook.matcher.is_empty()).then(|| badge(hook.matcher.as_str(), None).prop("title", "What it runs for"));
    let what = text(hook.text.as_str()).prop("mono", true).prop("size", "small").prop("truncate", true).prop("grow", true).prop("title", hook.text.as_str());
    // A command is what most are, so only the others say what they are.
    let kind = (hook.kind != "Command" && !hook.kind.is_empty()).then(|| small(hook.kind.as_str()));
    let off = hook.off.then(|| badge("Off", Some("warn")).prop("title", "It doesn't run in this session"));
    let from = small(hook.from.as_str()).prop("title", hook.from_detail.as_str());
    row().prop("gap", "s").prop("align", "center").key(format!("hook-{at}")).maybe(matcher).maybe(kind).child(what).maybe(off).child(from)
}

fn memory_dialog(setup: &Setup) -> Node {
    panel("Memory", WIDTH, close)
        .child(filled(&setup.memory, |memory| memory_body(setup, memory)))
        .child(small("Copy a path to open the file in your editor or Finder. To turn auto memory on or off, use /memory in a terminal."))
}

fn memory_body(setup: &Setup, memory: &Memory) -> Node {
    let files = memory.files.iter().map(|file| file_row(setup, file));
    let now = host::now();
    let shown = memory.saved.iter().take(SAVED_SHOWN).map(|saved| {
        let size = setup.size(&saved.path).map(|size| format!("{} tokens", tokens(size as f64)));
        let when = (saved.modified > 0.0).then(|| ago(saved.modified, now));
        let facts: Vec<String> = [saved.kind.clone(), size, when].into_iter().flatten().collect();
        let about = saved.description.as_deref().unwrap_or_default();
        path_row(setup, &saved.path, saved.name.as_str(), about, facts.join(" · "), 0)
    });
    let more = (memory.saved.len() > SAVED_SHOWN).then(|| small(format!("{} more in the folder", memory.saved.len() - SAVED_SHOWN)));
    let folders = memory.folders.iter().map(|folder| path_row(setup, &folder.path, folder.label.as_str(), "", String::new(), 0));
    let auto = stack()
        .prop("gap", "s")
        .child(heading(3, "Auto memory"))
        .child(small(format!("Auto memory is {}.", memory.auto)))
        .children(folders)
        .children(shown)
        .maybe(more);
    stack()
        .prop("gap", "l")
        .child(stack().prop("gap", "s").child(heading(3, "Instruction files")).children(files))
        .child(auto)
}

/// An instruction file: its name, where it is and the room it takes in the context, or that it isn't made yet.
fn file_row(setup: &Setup, file: &File) -> Node {
    let size = match (file.exists, setup.size(&file.path)) {
        (false, _) => "Not made yet".to_owned(),
        (true, Some(size)) => format!("{} tokens", tokens(size as f64)),
        (true, None) => String::new(),
    };
    path_row(setup, &file.path, file.label.as_str(), file.description.as_str(), size, file.depth)
}

/// A file or folder by its name, a line about it, its path, a fact at its end, and the button that copies its path.
fn path_row(setup: &Setup, path: &str, name: &str, about: &str, fact: String, depth: usize) -> Node {
    let head = row().prop("gap", "s").prop("align", "baseline").child(title(name)).maybe((!about.is_empty()).then(|| small(about).prop("truncate", true).prop("title", about)));
    let place = text(path).prop("mono", true).prop("size", "small").prop("tone", "muted").prop("truncate", true).prop("title", path);
    let copied = setup.copied.as_deref() == Some(path);
    let owned = path.to_owned();
    let copy_button = button(if copied { "Copied" } else { "Copy path" }, "ghost", false, move || copy(&owned)).prop("size", "sm").prop("title", path);
    row()
        .prop("gap", "s")
        .prop("align", "center")
        .key(path)
        .maybe((depth > 0).then(|| stack().prop("width", 20 * depth as u32)))
        .child(stack().prop("grow", true).prop("gap", "xs").child(head).child(place))
        .maybe((!fact.is_empty()).then(|| small(fact)))
        .child(copy_button)
}

fn permissions_dialog(setup: &Setup) -> Node {
    // The mode a new session starts in, and the settings that set it.
    let mode = setup.mode.as_ref().map(|mode| {
        let name = mode_name(mode.mode.as_deref().unwrap_or("default"));
        let from = match (&mode.mode, &mode.from) {
            (None, _) => Some("Claude Code's default".to_owned()),
            (Some(_), from) => from.as_ref().map(|from| format!("set in {from}")),
        };
        row().prop("gap", "s").prop("align", "baseline").child(text("Default mode").prop("weight", "medium")).child(text(name)).maybe(from.map(small))
    });
    panel("Permissions", WIDTH, close)
        .maybe(mode)
        .child(filled(&setup.rules, |rules| {
            let count = |behavior: &str| rules.rules.iter().filter(|rule| rule.behavior == behavior).count();
            let labels = [("allow", format!("Allow ({})", count("allow"))), ("ask", format!("Ask ({})", count("ask"))), ("deny", format!("Deny ({})", count("deny")))];
            let options: Vec<(&str, &str)> = labels.iter().map(|(value, label)| (*value, label.as_str())).collect();
            let bar = row()
                .prop("gap", "s")
                .prop("align", "center")
                .prop("wrap", true)
                .child(chips("Rules", &options, &setup.behavior, set_behavior))
                .child(stack().prop("grow", true).child(search_field("Search rules", &setup.query, set_query)));
            stack().prop("gap", "m").children(notices(&rules.notices)).child(bar).child(rule_list(setup, &rules.rules))
        }))
        .child(small("Change these with /permissions in a terminal."))
}

fn rule_list(setup: &Setup, rules: &[Rule]) -> Node {
    let wanted = setup.query.trim().to_lowercase();
    let mut budget = Budget::default();
    let rows: Vec<Node> = rules
        .iter()
        .enumerate()
        .filter(|(_, rule)| rule.behavior == setup.behavior && (rule.rule.to_lowercase().contains(&wanted) || source_label(&rule.source).to_lowercase().contains(&wanted)))
        .filter_map(|(at, rule)| budget.take(rule_row(at, rule)))
        .collect();
    if rows.is_empty() {
        return small(if wanted.is_empty() { format!("No {} rules.", setup.behavior) } else { format!("Nothing matches “{}”.", setup.query.trim()) });
    }
    let more = (budget.left > 0).then(|| small(format!("{} more. Search to find them.", budget.left)));
    stack().prop("gap", "s").children(rows).maybe(more)
}

/// A rule: as it is written, Claude Code's reading of it, and where it is set.
fn rule_row(at: usize, rule: &Rule) -> Node {
    let written = text(rule.rule.as_str()).prop("mono", true).prop("size", "small").prop("truncate", true).prop("title", rule.rule.as_str());
    let reading = rule.reading.as_ref().map(|(before, part, after)| {
        Node::new("Text").prop("size", "small").prop("tone", "muted").child(text(before.as_str())).child(text(part.as_str()).prop("weight", "medium")).child(text(after.as_str()))
    });
    let ignored = rule.ignored.then(|| badge("Not in effect", Some("warn")).prop("title", "Your organization's settings allow only their own rules"));
    let mut from = small(source_label(&rule.source));
    if let Some(file) = source_file(&rule.source) {
        from = from.prop("title", file);
    }
    row()
        .prop("gap", "s")
        .prop("align", "center")
        .key(format!("rule-{at}"))
        .child(stack().prop("grow", true).prop("gap", "xs").child(written).maybe(reading))
        .maybe(ignored)
        .child(from)
}

fn close() {
    chat::with(|c| c.session.setup.open = None);
}

fn set_behavior(behavior: &str) {
    chat::with(|c| c.session.setup.behavior = behavior.to_owned());
}

fn set_query(query: &str) {
    chat::with(|c| c.session.setup.query = query.to_owned());
}

/// Puts `path` on the clipboard, from your click on its button, and marks that row Copied.
fn copy(path: &str) {
    chat::with(|c| c.session.setup.copied = Some(path.to_owned()));
    drop(host::call("copyText", json!({ "text": path })));
}
