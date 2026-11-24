//! The dialogs `/hooks`, `/memory` and `/permissions` open over the chat, as Claude Code's own screens list them: the
//! hooks by event, the instruction files Claude Code reads with the room each takes and the memories it saved, and the
//! permission rules by allow, ask and deny with the mode a session starts in. A hook or a rule in one of your settings
//! files has a Remove button, the permissions dialog adds a rule, and the memory dialog turns auto memory on or off.
//! Each change first asks, in place of what it changes, with what it writes and to which file, and is made by the click
//! that confirms it. A file opens in your editor from its Open button, which Wings allows for Claude Code's own files; its
//! path can be copied instead, and a folder's only is.

use wings_plugin::{
    format::{ago, tokens},
    host, task,
    ui::{Node, actions, badge, button, chips, confirm as asks, dialog as panel, empty, heading, loading, muted, notice, row, search_field, select, stack, text, text_input, title},
};

use crate::{
    chat::{self, Chat},
    plugins_view::Budget,
    setup::{self, Edit, FILES, File, Hook, Hooks, Memory, Rule, RuleForm, Setup, Which, mode_name, place, source_label, source_path},
};

const WIDTH: u32 = 640;
/// The width of the column the permissions list names each rule's source in.
const SOURCE_WIDTH: u32 = 120;
/// The width of the column the hooks list names each hook's source in.
const HOOK_SOURCE_WIDTH: u32 = 112;
/// The columns of the buttons a row ends in, which every row keeps whether it has the button or not, so what is left of
/// them lines up.
const OPEN_WIDTH: u32 = 56;
const REMOVE_WIDTH: u32 = 72;
/// The most saved memories listed: Claude Code lists up to 200.
const SAVED_SHOWN: usize = 30;
/// How long after a change the dialog reads its list again: Claude Code took 1.05 s to list a rule an edit command wrote.
const RELIST_MS: u32 = 1_500;

pub fn dialog(c: &Chat) -> Option<Node> {
    let setup = &c.session.setup;
    Some(match setup.open? {
        Which::Hooks => hooks_dialog(setup, &c.cwd),
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

/// A column `width` points wide with `node` at its right, or only the room when a row has none.
fn column(width: u32, node: Option<Node>) -> Node {
    stack().prop("width", width).child(row().prop("justify", "end").maybe(node))
}

fn notices(list: &[(&'static str, String)]) -> impl Iterator<Item = Node> {
    list.iter().map(|(tone, said)| notice(tone, said.as_str()))
}

/// The card that asks before `edit` is made, in place of what it changes: what it adds, removes or sets as it is
/// written, the settings file it writes, and the button that makes it. An allow rule says what allowing means.
fn asking(setup: &Setup, edit: &Edit) -> Node {
    let (question, what, place) = edit.shown(setup.user_dir.as_deref());
    let allows = matches!(edit, Edit::AddRule { behavior, .. } if behavior == "allow");
    let body = [
        Some(text(what.as_str()).prop("mono", true).prop("size", "small")),
        Some(small(place)),
        allows.then(|| notice("warn", "Claude will do what this rule matches without asking you, in every session that reads this file.")),
    ];
    let variant = if matches!(edit, Edit::RemoveRule { .. } | Edit::RemoveHook { .. }) { "destructive" } else { "default" };
    let buttons = actions(Some(button("Cancel", "ghost", false, cancel).prop("disabled", setup.saving)), [button(edit.verb(), variant, setup.saving, confirm)]);
    asks(question, body.into_iter().flatten(), [buttons])
}

/// The button that asks to remove a hook or a rule from the settings file it is in.
fn remove_button(edit: Edit, from: &str) -> Node {
    button("Remove", "outline", false, move || ask(edit.clone())).prop("size", "sm").prop("title", format!("Removes it from {from}, once you confirm"))
}

fn hooks_dialog(setup: &Setup, cwd: &str) -> Node {
    panel("Hooks", WIDTH, close)
        .children(notices(&setup.outcome))
        .child(filled(&setup.hooks, |hooks| hooks_body(setup, hooks, cwd)))
        .child(small("Remove takes a hook out of the settings file it is in. To add or change one, edit that file, or ask Claude."))
}

fn hooks_body(setup: &Setup, hooks: &Hooks, cwd: &str) -> Node {
    let body = stack().prop("gap", "l").children(notices(&hooks.notices));
    if hooks.hooks.is_empty() {
        return body.child(empty("No hooks", "Claude Code runs no hooks of yours here."));
    }
    let mut budget = Budget::default();
    let events = hooks.events.iter().filter_map(|(event, summary)| {
        let rows: Vec<Node> = hooks.hooks.iter().enumerate().filter(|(_, hook)| hook.event == *event).filter_map(|(at, hook)| budget.take(hook_row(setup, at, hook, cwd))).collect();
        let head = row().prop("gap", "s").prop("align", "baseline").child(heading(3, event.as_str())).child(small(summary.as_str()));
        (!rows.is_empty()).then(|| stack().prop("gap", "s").child(head).children(rows))
    });
    let body = body.children(events.collect::<Vec<_>>());
    body.maybe((budget.left > 0).then(|| small(format!("{} more", budget.left))))
}

/// A hook: what it runs for, what it runs, whether it is kept from running, where it is set, and Remove when it is in one
/// of your settings files.
fn hook_row(setup: &Setup, at: usize, hook: &Hook, cwd: &str) -> Node {
    let remove = Edit::remove_hook(hook);
    if let Some(edit) = remove.as_ref().filter(|edit| setup.pending.as_ref() == Some(*edit)) {
        return asking(setup, edit).key(format!("hook-{at}"));
    }
    let matcher = (!hook.matcher.is_empty()).then(|| badge(hook.matcher.as_str(), None).prop("title", "What it runs for"));
    let what = text(hook.text.as_str()).prop("mono", true).prop("size", "small").prop("truncate", true).prop("grow", true).prop("title", hook.text.as_str());
    // A command is what most are, so only the others say what they are.
    let kind = (hook.kind != "Command" && !hook.kind.is_empty()).then(|| small(hook.kind.as_str()));
    let off = hook.off.then(|| badge("Off", Some("warn")).prop("title", "It doesn't run in this session"));
    let from = small(hook.from.as_str()).prop("title", setup.where_set(&hook.from_detail));
    // Your user settings are named `~/.claude/...` until Claude Code has said which account's folder they are in; a project's
    // are in the project.
    let file = hook.stored.as_ref().and_then(|(source, ..)| source_path(source, setup.user_dir.as_deref())).and_then(|file| match file.strip_prefix(".claude/") {
        Some(rest) => Some(format!("{}/.claude/{rest}", cwd.trim_end_matches('/'))),
        None => file.starts_with('/').then_some(file),
    });
    row()
        .prop("gap", "s")
        .prop("align", "center")
        .key(format!("hook-{at}"))
        .maybe(matcher)
        .maybe(kind)
        .child(what)
        .maybe(off)
        // One width for every row, so the sources form a column.
        .child(stack().prop("width", HOOK_SOURCE_WIDTH).child(from.prop("truncate", true)))
        .child(column(OPEN_WIDTH, file.map(|file| open_button(&file))))
        .child(column(REMOVE_WIDTH, remove.map(|edit| remove_button(edit, hook.from.as_str()))))
}

fn memory_dialog(setup: &Setup) -> Node {
    panel("Memory", WIDTH, close)
        .children(notices(&setup.outcome))
        .child(filled(&setup.memory, |memory| memory_body(setup, memory)))
        .child(small("Open a file in your editor, or copy its path."))
}

fn memory_body(setup: &Setup, memory: &Memory) -> Node {
    let files = memory.files.iter().map(|file| file_row(setup, file));
    let now = host::now();
    let shown = memory.saved.iter().take(SAVED_SHOWN).map(|saved| {
        let size = setup.size(&saved.path).map(|size| format!("{} tokens", tokens(size as f64)));
        let when = (saved.modified > 0.0).then(|| ago(saved.modified, now));
        let facts: Vec<String> = [saved.kind.clone(), size, when].into_iter().flatten().collect();
        let about = saved.description.as_deref().unwrap_or_default();
        path_row(setup, &saved.path, saved.name.as_str(), about, facts.join(" · "), 0, true)
    });
    let more = (memory.saved.len() > SAVED_SHOWN).then(|| small(format!("{} more in the folder", memory.saved.len() - SAVED_SHOWN)));
    let folders = memory.folders.iter().map(|folder| path_row(setup, &folder.path, folder.label.as_str(), "", String::new(), 0, false));
    let dream = memory.dream.as_ref().map(|(status, on)| switch_line(setup, format!("Auto dream is {status}."), "autoDreamEnabled", *on));
    let auto = stack()
        .prop("gap", "s")
        .child(heading(3, "Auto memory"))
        .child(switch_line(setup, format!("Auto memory is {}.", memory.auto), "autoMemoryEnabled", memory.auto_toggle))
        .maybe(dream)
        .children(folders)
        .children(shown)
        .maybe(more);
    stack()
        .prop("gap", "l")
        .child(stack().prop("gap", "s").child(heading(3, "Instruction files")).children(files))
        .child(auto)
}

/// What Claude Code says of a memory setting, with the button that turns it on or off when Claude Code says it may
/// change (`on` is then whether it is on).
fn switch_line(setup: &Setup, said: String, setting: &'static str, on: Option<bool>) -> Node {
    let edit = on.map(|on| Edit::Memory { setting, on: !on });
    if let Some(edit) = edit.as_ref().filter(|edit| setup.pending.as_ref() == Some(*edit)) {
        return asking(setup, edit);
    }
    let switch = edit.map(|edit| {
        let label = edit.verb();
        button(label, "outline", false, move || ask(edit.clone())).prop("size", "sm").prop("title", "Changes your user settings, once you confirm")
    });
    row().prop("gap", "s").prop("align", "center").child(small(said).prop("grow", true)).maybe(switch)
}

/// An instruction file: its name, where it is and the room it takes in the context, or that it isn't made yet.
fn file_row(setup: &Setup, file: &File) -> Node {
    let size = match (file.exists, setup.size(&file.path)) {
        (false, _) => "Not made yet".to_owned(),
        (true, Some(size)) => format!("{} tokens", tokens(size as f64)),
        (true, None) => String::new(),
    };
    path_row(setup, &file.path, file.label.as_str(), file.description.as_str(), size, file.depth, file.exists)
}

/// A file or folder by its name, a line about it, its path, a fact at its end, and the buttons that open a file that is
/// there (`openable`) and copy its path.
fn path_row(setup: &Setup, path: &str, name: &str, about: &str, fact: String, depth: usize, openable: bool) -> Node {
    let head = row().prop("gap", "s").prop("align", "baseline").child(title(name).prop("truncate", true).prop("shrink", "last").prop("title", name)).maybe((!about.is_empty()).then(|| small(about).prop("truncate", true).prop("title", about)));
    let place = text(path).prop("mono", true).prop("size", "small").prop("tone", "muted").prop("truncate", true).prop("title", path);
    let copied = setup.copied.as_deref() == Some(path);
    let owned = path.to_owned();
    let copy_button = button(if copied { "Copied" } else { "Copy path" }, "outline", false, move || copy(&owned)).prop("size", "sm").prop("title", "Copies the path, to open it in Finder");
    row()
        .prop("gap", "s")
        .prop("align", "center")
        .key(path)
        .maybe((depth > 0).then(|| stack().prop("width", 20 * depth as u32)))
        .child(stack().prop("grow", true).prop("gap", "xs").child(head).child(place))
        .maybe((!fact.is_empty()).then(|| small(fact)))
        .maybe(openable.then(|| open_button(path)))
        .child(copy_button)
}

/// The button that opens the file at `path` in your editor.
fn open_button(path: &str) -> Node {
    let owned = path.to_owned();
    button("Open", "outline", false, move || open(&owned)).prop("size", "sm").prop("title", format!("Opens {path} in your editor"))
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
        .children(notices(&setup.outcome))
        .child(filled(&setup.rules, |rules| {
            let count = |behavior: &str| rules.rules.iter().filter(|rule| rule.behavior == behavior).count();
            let labels = [("allow", format!("Allow ({})", count("allow"))), ("ask", format!("Ask ({})", count("ask"))), ("deny", format!("Deny ({})", count("deny")))];
            let options: Vec<(&str, &str)> = labels.iter().map(|(value, label)| (*value, label.as_str())).collect();
            let add = setup.form.is_none().then(|| button("Add rule", "default", false, new_rule).prop("title", "Writes a rule to a settings file, once you confirm"));
            let bar = row()
                .prop("gap", "s")
                .prop("align", "center")
                .prop("wrap", true)
                .child(chips("Rules", &options, &setup.behavior, set_behavior))
                .child(stack().prop("grow", true).child(search_field("Search rules", &setup.query, set_query)))
                .maybe(add);
            let form = setup.form.as_ref().map(|form| rule_form(setup, form));
            stack().prop("gap", "l").children(notices(&rules.notices)).child(bar).maybe(form).child(rule_list(setup, &rules.rules))
        }))
        .child(small("Remove changes the settings file a rule is in. Rules your organization, the command line or this session set stay as they are."))
}

/// The form for a new rule: the rule as you type it, whether it allows, asks or denies, and the settings file it goes in.
/// Review asks before anything is written, in its place.
fn rule_form(setup: &Setup, form: &RuleForm) -> Node {
    if let Some(edit) = setup.pending.as_ref().filter(|edit| matches!(edit, Edit::AddRule { .. })) {
        return asking(setup, edit);
    }
    let files: Vec<(String, String)> = FILES.iter().map(|file| (file.to_string(), place(file, setup.user_dir.as_deref()))).collect();
    let field = text_input("Rule", &form.rule, set_rule).prop("placeholder", "Bash(npm run test:*)").prop("autoFocus", true);
    let choices = row()
        .prop("gap", "s")
        .prop("align", "center")
        .prop("wrap", true)
        .child(chips("Behavior", &[("allow", "Allow"), ("ask", "Ask"), ("deny", "Deny")], &form.behavior, set_rule_behavior))
        .child(select("Save in", &files, &form.file, set_rule_file));
    let buttons = actions(Some(button("Cancel", "ghost", false, close_form)), [button("Review", "default", false, review).prop("disabled", form.rule.trim().is_empty())]);
    stack()
        .prop("gap", "l")
        .child(heading(3, "New rule"))
        .child(field)
        .child(small("A tool, or a tool and what it may do in parentheses, like Bash(npm run test:*) or WebFetch(domain:example.com)."))
        .child(choices)
        .child(buttons)
}

fn rule_list(setup: &Setup, rules: &[Rule]) -> Node {
    let wanted = setup.query.trim().to_lowercase();
    let mut budget = Budget::default();
    let rows: Vec<Node> = rules
        .iter()
        .enumerate()
        .filter(|(_, rule)| rule.behavior == setup.behavior && (rule.rule.to_lowercase().contains(&wanted) || source_label(&rule.source).to_lowercase().contains(&wanted)))
        .filter_map(|(at, rule)| budget.take(rule_row(setup, at, rule)))
        .collect();
    if rows.is_empty() {
        return small(if wanted.is_empty() { format!("No {} rules.", setup.behavior) } else { format!("Nothing matches “{}”.", setup.query.trim()) });
    }
    let more = (budget.left > 0).then(|| small(format!("{} more. Search to find them.", budget.left)));
    stack().prop("gap", "l").children(rows).maybe(more)
}

/// A rule: as it is written, Claude Code's reading of it, where it is set, and Remove when it is in one of your settings
/// files.
fn rule_row(setup: &Setup, at: usize, rule: &Rule) -> Node {
    let remove = Edit::remove_rule(rule);
    if let Some(edit) = remove.as_ref().filter(|edit| setup.pending.as_ref() == Some(*edit)) {
        return asking(setup, edit).key(format!("rule-{at}"));
    }
    let written = text(rule.rule.as_str()).prop("mono", true).prop("size", "small").prop("truncate", true).prop("title", rule.rule.as_str());
    let reading = rule.reading.as_ref().map(|(before, part, after)| {
        Node::new("Text").prop("size", "small").prop("tone", "muted").child(text(before.as_str())).child(text(part.as_str()).prop("weight", "medium")).child(text(after.as_str()))
    });
    let ignored = rule.ignored.then(|| badge("Not in effect", Some("warn")).prop("title", "Your organization's settings allow only their own rules"));
    let mut from = small(source_label(&rule.source)).prop("truncate", true);
    if let Some(file) = source_path(&rule.source, setup.user_dir.as_deref()) {
        from = from.prop("title", file);
    }
    row()
        .prop("gap", "s")
        // The source and Remove sit in the middle of the rule, whether it takes one line or two.
        .prop("align", "center")
        .key(format!("rule-{at}"))
        .child(stack().prop("grow", true).prop("gap", "xs").child(written).maybe(reading))
        .maybe(ignored)
        // One width for every row, so the sources form a column.
        .child(stack().prop("width", SOURCE_WIDTH).child(from))
        // Reserved for a rule that has no Remove, so its source is in the column too.
        .child(column(REMOVE_WIDTH, remove.map(|edit| remove_button(edit, source_label(&rule.source)))))
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

/// Opens `path` in your editor, from your click on its button; a refusal shows in the dialog.
fn open(path: &str) {
    let opening = host::open_file(path, None);
    task::spawn(async move {
        if let Err(why) = opening.await {
            chat::with(|c| c.session.setup.outcome = vec![("danger", why)]);
        }
    });
}

/// Puts `path` on the clipboard, from your click on its button, and marks that row Copied.
fn copy(path: &str) {
    chat::with(|c| c.session.setup.copied = Some(path.to_owned()));
    drop(host::copy_text(&path));
}

/// Shows `edit` with what it changes, to confirm. Nothing is written yet.
fn ask(edit: Edit) {
    chat::with(|c| c.session.setup.ask(edit));
}

fn cancel() {
    chat::with(|c| c.session.setup.cancel());
}

/// Makes the change shown, from your click on its button and nothing else, then reads its list again, which goes to
/// Claude Code with that same click.
fn confirm() {
    let Some((edit, account, cwd)) = chat::with(|c| Some((c.session.setup.confirm()?, chat::account(c, c.session.id.as_deref()), c.cwd.clone()))) else {
        return;
    };
    task::spawn(async move {
        let outcome = setup::run(&account, &cwd, &edit).await;
        let saved = outcome.is_ok();
        chat::with(|c| c.session.setup.saved(&edit, outcome));
        if saved {
            host::sleep(RELIST_MS).await;
            chat::write(|_, _| Some(edit.reread()));
        }
    });
}

fn new_rule() {
    chat::with(|c| c.session.setup.new_rule());
}

fn close_form() {
    chat::with(|c| c.session.setup.form = None);
}

fn review() {
    chat::with(|c| c.session.setup.review_rule());
}

fn with_form(change: impl FnOnce(&mut RuleForm)) {
    chat::with(|c| {
        if let Some(form) = &mut c.session.setup.form {
            change(form);
        }
    });
}

fn set_rule(rule: String) {
    with_form(|form| form.rule = rule);
}

fn set_rule_behavior(behavior: &str) {
    with_form(|form| form.behavior = behavior.to_owned());
}

fn set_rule_file(file: &str) {
    with_form(|form| form.file = file.to_owned());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A memory file's name keeps its width while the line about it has any to give: "feedback_tes…" was cut short beside a
    /// description that read nearly whole.
    #[test]
    fn a_files_name_shrinks_after_the_line_about_it() {
        let shown = wings_plugin::ui::tree_json(path_row(&Setup::default(), "/m/feedback_tests.md", "feedback_tests.md", "Run the checkout tests before saying it is done", String::new(), 0, true)).to_string();
        assert!(shown.contains(r#""shrink":"last""#), "{shown}");
    }
}
