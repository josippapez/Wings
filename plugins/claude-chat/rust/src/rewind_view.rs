//! Rewind, as Claude Code's own `/rewind`: a quiet button beside each of your messages opens a dialog that goes back to
//! before it. It lists the files restoring the code would write back, from a dry run, before you can restore them, and
//! offers the same three choices: the code and the conversation, the conversation, or the code. Restoring the conversation
//! drops the message and all after it, and puts the message back in the box to edit and send again.

use wings_plugin::{
    format::{plural, truncate},
    ui::{self, Node, card, dialog as panel, icon, muted, notice, row, spinner, stack, text},
};

use crate::{
    chat::{self, Chat, State},
    session::{Check, Restore},
};

/// Lucide's `undo-2`, in a 24-point box.
const UNDO: [&str; 2] = ["M9 14 4 9l5-5", "M4 9h10.5a5.5 5.5 0 0 1 5.5 5.5a5.5 5.5 0 0 1-5.5 5.5H11"];
const WIDTH: u32 = 560;
/// The most files the dialog lists by name.
const FILES_SHOWN: usize = 30;

/// The button beside your message at `at` that opens the dialog, once Claude Code knows the message by a uuid. It waits
/// while Claude works, as Claude Code rewinds only a conversation at rest.
pub fn button(at: usize, c: &Chat) -> Option<Node> {
    let uuid = c.session.prompts.iter().find(|prompt| prompt.at == at)?.uuid.clone();
    (c.state == State::Running).then(|| {
        ui::button("", "ghost", false, move || chat::open_rewind(&uuid))
            .prop("size", "icon-sm")
            .prop("label", "Rewind")
            .prop("title", "Rewind to before this message")
            .prop("disabled", c.session.working)
            .child(icon(24, 2.0, &UNDO).prop("size", 14))
    })
}

/// The dialog, while it is open.
pub fn dialog(c: &Chat) -> Option<Node> {
    let rewind = c.session.rewind.as_ref()?;
    let said = match rewind.text.trim() {
        "" => muted("A message of pictures only"),
        said => text(truncate(said, 600)),
    };
    let check = match &rewind.check {
        Some(Ok(check)) if !check.files.is_empty() => Some(check),
        _ => None,
    };
    let changed = match (&rewind.check, check) {
        (_, Some(check)) => files(check, &c.cwd),
        (None, _) => row().prop("gap", "s").prop("align", "center").child(spinner()).child(muted("Checking which files restoring the code would change").prop("size", "small")),
        (Some(Err(why)), _) => muted(format!("The code can't be restored here: {why}")).prop("size", "small"),
        (Some(Ok(_)), None) => muted("No files changed since this message, so there is no code to restore.").prop("size", "small"),
    };
    let warning = match check {
        Some(_) => "Restoring code writes these files back on disk as they were before this message. Restoring the conversation drops this message and everything after it, and can't be undone here.",
        None => "Restoring the conversation drops this message and everything after it, and can't be undone here.",
    };
    let busy = !rewind.waiting.is_empty();
    let choice = |label: &str, variant: &str, how: Restore, ready: bool| ui::button(label, variant, false, move || chat::restore(how)).prop("disabled", busy || !ready);
    // Cancel at the left, and at the right what to restore: the two smaller choices, then the main one. Their labels are
    // short, as the word before them says what they do.
    let restore = row()
        .prop("gap", "s")
        .prop("align", "center")
        .child(muted("Restore").prop("size", "small"))
        .maybe(busy.then(|| spinner()))
        .child(choice("Code", "outline", Restore::Code, check.is_some()))
        .child(choice("Conversation", "outline", Restore::Conversation, true))
        .child(choice("Code and conversation", "default", Restore::Both, check.is_some()));
    let actions = row().prop("gap", "m").prop("justify", "between").prop("align", "center").prop("wrap", true).child(ui::button("Cancel", "ghost", false, chat::close_rewind)).child(restore);
    Some(
        panel("Rewind", WIDTH, chat::close_rewind)
            .child(muted("Go back to before this message of yours:").prop("size", "small"))
            .child(card().child(said))
            .child(changed)
            .child(notice("warn", warning))
            .maybe(rewind.error.as_deref().map(|why| notice("danger", why)))
            .child(actions),
    )
}

/// The files restoring the code writes back, each by its path in the project, with the lines it adds and takes out.
fn files(check: &Check, cwd: &str) -> Node {
    let path = |file: &str| file.strip_prefix(cwd).and_then(|inside| inside.strip_prefix('/')).unwrap_or(file).to_owned();
    let head = row()
        .prop("gap", "s")
        .prop("align", "baseline")
        .child(text(format!("Restoring the code changes {}", plural(check.files.len(), "file"))).prop("size", "small").prop("weight", "medium").prop("grow", true))
        .child(text(format!("+{}", check.added)).prop("mono", true).prop("size", "small").prop("tone", "success"))
        .child(text(format!("−{}", check.removed)).prop("mono", true).prop("size", "small").prop("tone", "danger"));
    let names = check.files.iter().take(FILES_SHOWN).map(|file| {
        let shown = path(file);
        text(shown.as_str()).prop("mono", true).prop("size", "small").prop("truncate", true).prop("title", file.as_str()).key(file.as_str())
    });
    let more = (check.files.len() > FILES_SHOWN).then(|| muted(format!("{} more", plural(check.files.len() - FILES_SHOWN, "file"))).prop("size", "small"));
    stack().prop("gap", "xs").child(head).child(stack().prop("gap", "none").children(names)).maybe(more)
}
