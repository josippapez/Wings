//! The dialogs of `/status`, `/export` and `/btw` over the chat, and the Copy buttons of the conversation. What these
//! commands answer is shown here and never added to the conversation, apart from the note that says the export was copied.
//! Wings lets a page copy only on a click, once, so the export copies on its dialog's Copy button, not on the Enter that
//! asked for it: the text is not known until Claude Code answers.

use serde_json::json;
use wings_plugin::{
    format::plural,
    host,
    markdown::{code_block, markdown},
    ui::{Node, button, dialog as panel, heading, icon, loading, muted, notice, row, stack, text},
};

use crate::{
    chat::{self, Chat},
    commands::{Export, Section, Side},
    copy::{COPY_MAX, Part, parts},
    session::Item,
};

const WIDTH: u32 = 640;
/// The lines of the exported text the dialog previews.
const PREVIEW: usize = 12;

/// The dialog that is open, over the conversation: a side question's, else the export's, else the status.
pub fn dialog(c: &Chat) -> Option<Node> {
    let cmds = &c.session.cmds;
    if let Some(side) = &cmds.side {
        return Some(side_dialog(side));
    }
    if let Some(export) = &cmds.export {
        return Some(match export {
            None => panel("Export", WIDTH, close_export).child(loading()),
            Some(Err(why)) => panel("Export", WIDTH, close_export).child(notice("danger", why.as_str())),
            Some(Ok(export)) => export_dialog(export),
        });
    }
    let status = cmds.status.as_ref()?;
    let body = match status {
        None => loading(),
        Some(Err(why)) => notice("danger", why.as_str()),
        Some(Ok(sections)) => stack().prop("gap", "l").children(sections.iter().map(|one| section(c, one))),
    };
    Some(panel("Status", WIDTH, close_status).child(body))
}

fn section(c: &Chat, section: &Section) -> Node {
    // Runs of labelled rows sit close together, and a sentence about them (a row with no label) is set apart from them.
    let mut runs: Vec<(bool, Vec<Node>)> = Vec::new();
    for (at, (label, value)) in section.rows.iter().enumerate() {
        let shown = shown_value(c, label, value);
        let node = match label.is_empty() {
            // A row with no label is a sentence about the ones above it.
            true => muted(value.as_str()).prop("size", "small").key(at.to_string()),
            false => row()
                .prop("gap", "m")
                .prop("align", "baseline")
                .child(stack().prop("width", 150).child(muted(label_of(label))))
                .child(stack().prop("grow", true).child(text(shown.as_str()).prop("truncate", true).prop("title", value.as_str())))
                .key(at.to_string()),
        };
        match runs.last_mut() {
            Some((labelled, nodes)) if *labelled == !label.is_empty() => nodes.push(node),
            _ => runs.push((!label.is_empty(), vec![node])),
        }
    }
    let title = (!section.title.is_empty()).then(|| heading(3, section.title.as_str()));
    let runs = runs.into_iter().map(|(_, nodes)| stack().prop("gap", "s").children(nodes));
    // More room between a run and the sentence under it than inside a run.
    stack().prop("gap", "l").maybe(title).children(runs)
}

/// A row's value as the chat shows it elsewhere: the model by the name the composer gives it, a folder from `~` or from
/// the chat's folder.
fn shown_value(c: &Chat, label: &str, value: &str) -> String {
    match label_of(label) {
        "Model" => {
            let models = &c.session.models;
            models.iter().find(|m| m.resolved == value && m.value != "default").or_else(|| models.iter().find(|m| m.resolved == value)).map_or(value, |m| m.name.as_str()).to_owned()
        }
        "Folder" => crate::fork_view::shown_path(&c.cwd, value),
        _ => value.to_owned(),
    }
}

/// A row's label in plain words: Claude Code names the folder `cwd`.
fn label_of(label: &str) -> &str {
    if label.eq_ignore_ascii_case("cwd") { "Folder" } else { label }
}

/// The side question and its answer, as Claude Code's own `/btw` shows them: apart from the conversation.
fn side_dialog(side: &Side) -> Node {
    let answer = match &side.answer {
        None => loading(),
        Some(Err(why)) => notice("danger", why.as_str()),
        Some(Ok(reply)) if reply.text.trim().is_empty() => muted("Claude had nothing to add."),
        Some(Ok(reply)) => markdown(&reply.text),
    };
    panel("Side question", WIDTH, close_side)
        .child(stack().prop("gap", "m").child(text(side.question.as_str()).prop("weight", "medium")).child(answer).child(muted("This isn't added to the conversation.").prop("size", "small")))
}

fn export_dialog(export: &Export) -> Node {
    if export.text.trim().is_empty() {
        return panel("Export", WIDTH, close_export).child(muted("There is nothing to export yet."));
    }
    let lines = export.text.lines().count();
    let shown: Vec<&str> = export.text.lines().take(PREVIEW).collect();
    let more = lines.saturating_sub(PREVIEW);
    let facts = if export.filename.is_empty() { plural(lines, "line") } else { format!("{} · {}", plural(lines, "line"), export.filename) };
    panel("Export", WIDTH, close_export).child(
        stack()
            .prop("gap", "m")
            .child(muted(facts))
            .child(code_block(&shown.join("\n")))
            .maybe((more > 0).then(|| muted(format!("{} more", plural(more, "line"))).prop("size", "small")))
            .child(row().prop("gap", "s").prop("justify", "end").child(button("Close", "outline", false, close_export)).child(button("Copy to clipboard", "default", false, copy_export))),
    )
}

fn close_status() {
    chat::with(|c| c.session.cmds.status = None);
}

fn close_export() {
    chat::with(|c| c.session.cmds.export = None);
}

fn close_side() {
    chat::with(|c| c.session.cmds.side = None);
}

/// Copies the exported conversation, from the click on the dialog's button, and says so in the conversation.
fn copy_export() {
    let text = chat::with(|c| match &c.session.cmds.export {
        Some(Some(Ok(export))) => Some(export.text.clone()),
        _ => None,
    });
    let Some(text) = text else {
        return;
    };
    let said = if text.len() > COPY_MAX {
        Item::Error("The conversation is too long to copy: Wings copies up to 1 MB. Export it from a terminal instead.".into())
    } else {
        drop(host::call("copyText", json!({ "text": text })));
        Item::Notice(format!("Copied the conversation to the clipboard ({})", plural(text.lines().count(), "line")))
    };
    chat::with(|c| {
        c.session.cmds.export = None;
        c.session.items.push(said);
    });
}

/// Lucide's `copy`, and its `check` for a button that was used.
const COPY: [&str; 2] = ["M10 8h10a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H10a2 2 0 0 1-2-2V10a2 2 0 0 1 2-2z", "M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"];
const CHECK: [&str; 1] = ["M20 6 9 17l-5-5"];

/// A small copy icon, a check once it was used. `key` names it among the others.
pub fn copy_button(key: &str, label: &str, c: &Chat, on_click: impl Fn() + 'static) -> Node {
    let copied = c.session.cmds.copied.as_deref() == Some(key);
    let (label, paths): (&str, &[&str]) = if copied { ("Copied", &CHECK) } else { (label, &COPY) };
    button("", "ghost", false, on_click).prop("size", "icon-sm").prop("label", label).prop("title", label).child(icon(24, 2.0, paths).prop("size", 14)).key(key)
}

/// A reply with its code blocks, each over a line with its language and a Copy button, and a Copy button under the reply
/// that copies all of it. A reply without code is Markdown as before, with the one button.
pub fn reply(at: usize, said: &str, c: &Chat) -> Node {
    let blocks = parts(said);
    let mut nodes: Vec<Node> = Vec::new();
    let mut n = 0;
    for part in &blocks {
        match part {
            // Words that name something in Wings, like an issue key, open it there.
            Part::Text(markdown_text) => nodes.push(crate::edits_view::prose(markdown_text, c)),
            Part::Code { lang, text } => {
                let head = row()
                    .prop("gap", "s")
                    .prop("align", "center")
                    .child(stack().prop("grow", true).child(muted(lang.as_str()).prop("size", "small")))
                    .child(copy_button(&format!("code-{at}-{n}"), "Copy this code", c, {
                        let n = n;
                        move || copy_code(at, n)
                    }));
                nodes.push(stack().prop("gap", "xs").child(head).child(crate::code_view::block(&format!("code-{at}-{n}"), text, lang)).key(format!("code-{at}-{n}")));
                n += 1;
            }
        }
    }
    if blocks.is_empty() {
        nodes.push(crate::edits_view::prose(said, c));
    }
    let foot = row().prop("gap", "xs").prop("align", "center").child(copy_button(&format!("reply-{at}"), "Copy this reply", c, move || copy_reply(at))).maybe(crate::bookmarks::toggle_button(at, c));
    stack().prop("gap", "xs").children(nodes).child(foot)
}

/// Puts `text` on the clipboard, from your click on a Copy button, and marks that button Copied.
fn copy(key: String, text: &str) {
    if text.len() > COPY_MAX {
        return chat::with(|c| c.session.items.push(Item::Error("That is too long to copy: Wings copies up to 1 MB.".into())));
    }
    chat::with(|c| c.session.cmds.copied = Some(key.clone()));
    drop(host::call("copyText", json!({ "text": text })));
    // One redraw after the pause puts the copy icon back; a newer copy keeps its own check.
    wings_plugin::task::spawn(async move {
        host::sleep(COPIED_FOR).await;
        forget_copied(&key);
    });
}

/// How long a Copy button stays a check.
const COPIED_FOR: u32 = 2000;

/// Ends the check of the button `key`, unless a later copy moved it to another.
fn forget_copied(key: &str) {
    chat::with(|c| {
        if c.session.cmds.copied.as_deref() == Some(key) {
            c.session.cmds.copied = None;
        }
    });
}

/// The text of the reply or report at `at`, which its Copy button copies.
pub(crate) fn said_text_at(at: usize) -> Option<String> {
    said_at(at)
}

fn said_at(at: usize) -> Option<String> {
    chat::with(|c| match c.session.items.get(at) {
        Some(Item::Text(said)) => Some(said.clone()),
        Some(Item::Report(report)) => Some(report.text.clone()),
        _ => None,
    })
}

fn copy_reply(at: usize) {
    if let Some(said) = said_at(at) {
        copy(format!("reply-{at}"), &said);
    }
}

/// Copies the `n`th code block of the reply at `at`: what is inside its fence.
fn copy_code(at: usize, n: usize) {
    let code = said_at(at).and_then(|said| {
        parts(&said).into_iter().filter_map(|part| if let Part::Code { text, .. } = part { Some(text) } else { None }).nth(n)
    });
    if let Some(code) = code {
        copy(format!("code-{at}-{n}"), &code);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_working_folder_is_named_in_plain_words() {
        assert_eq!(label_of("cwd"), "Folder");
        assert_eq!(label_of("Version"), "Version");
    }

    fn label(key: &str) -> String {
        let node = chat::with(|c| copy_button(key, "Copy this reply", c, || {}));
        wings_plugin::ui::tree_json(node).to_string()
    }

    #[test]
    fn a_copied_button_goes_back_to_copy_and_a_newer_copy_is_kept() {
        chat::with(|c| c.session.cmds.copied = Some("reply-1".into()));
        assert!(label("reply-1").contains("Copied"));
        // A second copy moved the check; the first copy's timer must not clear it.
        chat::with(|c| c.session.cmds.copied = Some("reply-2".into()));
        forget_copied("reply-1");
        assert!(label("reply-2").contains("Copied"));
        forget_copied("reply-2");
        assert!(label("reply-2").contains("Copy this reply"));
        assert!(!label("reply-2").contains("Copied"));
    }
}
