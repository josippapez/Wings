//! The card at the end of a turn that changed files ("Edited 2 files +46 −2", each file, Undo and View changes), the dialog
//! Undo asks first with, and Claude's replies with file references as links (`file_links.rs`). The numbers and the patch
//! are in `edits.rs`.

use std::{cell::Cell, collections::hash_map::DefaultHasher, hash::{Hash, Hasher}};

use serde_json::json;
use wings_plugin::{
    diff, host,
    format::plural,
    markdown::{Block, markdown, parse_blocks, parse_inline, Span},
    task,
    ui::{self, Node, card, dialog as panel, muted, notice, row, stack, text},
};

use crate::{
    chat::{self, Chat},
    edits::{Turn, relative},
    file_links::{self, Piece, Reference, pieces, resolve},
    fork_view,
    session::Item,
};

const WIDTH: u32 = 520;
/// The most files the card and the dialog list by name.
const FILES_SHOWN: usize = 8;

/// The card for a turn: its files, and what to do with them. `from` is the position of your message that began the turn
/// and `end` where it ends.
pub fn card_of(c: &Chat, from: usize, end: usize, turn: &Turn) -> Node {
    let uuid = c.session.prompts.iter().find(|prompt| prompt.at == from).map(|prompt| prompt.uuid.clone());
    let undone = uuid.as_deref().is_some_and(|uuid| c.session.edits.is_undone(uuid));
    let head = row()
        .prop("gap", "s")
        .prop("align", "baseline")
        .child(text(format!("Edited {}", plural(turn.files.len(), "file"))).prop("size", "small").prop("weight", "medium").prop("grow", true))
        .child(counts(turn.added, turn.removed));
    let files = turn.files.iter().take(FILES_SHOWN).map(|file| {
        let shown = relative(&c.cwd, &file.path);
        let (dir, name) = shown.rsplit_once('/').map_or(("", shown), |(dir, name)| (dir, name));
        let name_line = row()
            .prop("gap", "none")
            .prop("align", "baseline")
            .prop("grow", true)
            .maybe((!dir.is_empty()).then(|| muted(format!("{dir}/")).prop("size", "small").prop("truncate", true)))
            .child(text(name).prop("size", "small").prop("weight", "medium"));
        row().prop("gap", "s").prop("align", "baseline").key(file.path.as_str()).prop("title", file.path.as_str()).child(name_line).child(counts(file.added, file.removed))
    });
    let more = (turn.files.len() > FILES_SHOWN).then(|| muted(format!("{} more", plural(turn.files.len() - FILES_SHOWN, "file"))).prop("size", "small"));
    let waits = fork_view::busy(c).is_some();
    let undo = match (&uuid, undone) {
        (_, true) => Some(muted("Undone").prop("size", "small")),
        (Some(uuid), false) => {
            let uuid = uuid.clone();
            Some(ui::button("Undo", "outline", false, move || ask_undo(&uuid, from, end)).prop("size", "sm").prop("disabled", waits))
        }
        (None, false) => None,
    };
    let actions = row()
        .prop("gap", "s")
        .prop("wrap", true)
        .prop("align", "center")
        .maybe(undo)
        .child(ui::button("View changes", "outline", false, move || view_changes(from, end)).prop("size", "sm"));
    card().child(stack().prop("gap", "s").child(head).child(stack().prop("gap", "xs").children(files).maybe(more)).child(actions)).key(format!("edits-{from}"))
}

fn counts(added: usize, removed: usize) -> Node {
    row()
        .prop("gap", "xs")
        .prop("align", "baseline")
        .child(text(format!("+{added}")).prop("mono", true).prop("size", "small").prop("tone", "success"))
        .child(text(format!("−{removed}")).prop("mono", true).prop("size", "small").prop("tone", "danger"))
}

/// The dialog Undo asks first with, while it is open.
pub fn dialog(c: &Chat) -> Option<Node> {
    let undo = c.session.edits.undo.as_ref()?;
    let names = |files: &[String]| files.iter().map(|file| relative(&c.cwd, file).to_owned()).collect::<Vec<_>>();
    let listed = undo.files.iter().take(FILES_SHOWN).map(|file| text(relative(&c.cwd, file)).prop("mono", true).prop("size", "small").prop("truncate", true).prop("title", file.as_str()).key(file.as_str()));
    let more = (undo.files.len() > FILES_SHOWN).then(|| muted(format!("{} more", plural(undo.files.len() - FILES_SHOWN, "file"))).prop("size", "small"));
    let same: Vec<String> = undo.later.iter().filter(|file| undo.files.contains(file)).cloned().collect();
    let other = undo.later.len() - same.len();
    let mut later = String::new();
    if !same.is_empty() {
        later.push_str(&format!("Later turns edited {} as well, so those edits are undone too.", names(&same).join(", ")));
    }
    if other > 0 {
        later.push_str(&format!(" Undo also puts back {} that later turns changed.", plural(other, "other file")));
    }
    let waits = fork_view::busy(c);
    let actions = row()
        .prop("gap", "s")
        .prop("wrap", true)
        .prop("justify", "end")
        .prop("align", "center")
        .child(ui::button("Cancel", "ghost", false, close_undo))
        .child(ui::button("Undo", "default", undo.waiting, confirm_undo).prop("disabled", waits.is_some() || undo.waiting));
    Some(
        panel("Undo changes", WIDTH, close_undo)
            .child(text(format!("Undo the changes to {} from this turn?", plural(undo.files.len(), "file"))))
            .child(stack().prop("gap", "none").children(listed).maybe(more))
            .child(muted("They go back to how they were before your message. The conversation stays as it is.").prop("size", "small"))
            .maybe((!later.is_empty()).then(|| notice("warn", later.trim().to_owned())))
            .maybe(waits.map(|why| notice("warn", why)))
            .maybe(undo.error.as_deref().map(|why| notice("danger", why)))
            .child(actions),
    )
}

/// Opens the dialog, from the card's Undo. Nothing is written until the dialog's button.
fn ask_undo(uuid: &str, from: usize, end: usize) {
    chat::with(|c| c.session.edits.ask_undo(uuid, from, end));
}

pub fn close_undo() {
    chat::with(|c| c.session.edits.undo = None);
}

/// The dialog's Undo: one `rewind_files` for the turn's message.
fn confirm_undo() {
    chat::write(|c, _| match fork_view::busy(c) {
        Some(_) => None,
        None => c.session.edits.confirm_undo(),
    });
}

/// Tells you in the conversation why something didn't open.
fn say(why: String) {
    chat::with(|c| c.session.items.push(Item::Error(why)));
}

/// Opens the turn's changes in the diff viewer, from the card's View changes.
fn view_changes(from: usize, end: usize) {
    let (patch, cwd, folder) = chat::with(|c| (c.session.edits.patch(from, end, &c.cwd), c.cwd.clone(), c.folder.clone()));
    open(json!({ "title": "Changes in this turn", "subtitle": folder, "cwd": cwd, "patch": patch }));
}

/// Opens a file a reply names in your editor, at its line. Where that fails (no editor, or an older Wings), a file the
/// session changed shows in the diff viewer instead, at the line.
fn open_file(reference: &Reference) {
    let opening = host::open_file(&reference.path, reference.line);
    let reference = reference.clone();
    task::spawn(async move {
        if let Err(why) = opening.await
            && !show_changes(&reference)
        {
            say(format!("Couldn't open {}: {why}", reference.path));
        }
    });
}

/// What the session changed in the file, in the diff viewer, at the line. False when it changed nothing there.
fn show_changes(reference: &Reference) -> bool {
    let found = chat::with(|c| c.session.edits.file_patch(&reference.path, &c.cwd).map(|patch| (patch, relative(&c.cwd, &reference.path).to_owned(), c.cwd.clone(), c.folder.clone())));
    let Some((patch, path, cwd, folder)) = found else {
        return false;
    };
    let mut shown = json!({ "title": path, "subtitle": folder, "cwd": cwd, "patch": patch });
    if let Some(line) = reference.line {
        shown["focus"] = json!({ "path": path, "line": line, "side": "new" });
    }
    open(shown);
    true
}

fn open(shown: serde_json::Value) {
    task::spawn(async move {
        if let Err(why) = diff::open(shown).await {
            say(format!("Couldn't open the changes: {why}"));
        }
    });
}

/// Claude's text as Markdown, with the files it names as links: the project's files and the ones the session changed.
/// Without either it is the plain renderer. This repeats that renderer's blocks, which can't link a word of its own choosing.
pub fn prose(said: &str, c: &Chat) -> Node {
    let mut hasher = DefaultHasher::new();
    said.hash(&mut hasher);
    prose_in(&format!("prose-{:x}", hasher.finish()), said, c)
}

/// `prose` for text that keeps its place on the page while it changes, as a reply streaming in: `scope` names it, so its
/// code blocks keep whether you opened them (`code_view.rs`).
pub fn prose_in(scope: &str, said: &str, c: &Chat) -> Node {
    let changed = c.session.edits.files();
    // Code blocks are this renderer's: the plain one draws them without colours or a fold.
    if changed.is_empty() && c.files.is_empty() && !said.contains("```") && !said.contains("~~~") {
        return markdown(said).prop("links", true);
    }
    let known = file_links::known(changed, &c.cwd, &c.files);
    let blocks = Cell::new(0);
    // `links` also gives Wings' own links (an issue key) in every text below.
    stack().prop("gap", "m").prop("links", true).children(parse_blocks(said, 0).iter().map(|block| block_node(block, &c.cwd, &known, scope, &blocks)))
}

fn block_node(block: &Block, cwd: &str, known: &dyn Fn(&str) -> bool, scope: &str, blocks: &Cell<usize>) -> Node {
    let spans = |source: &str| spans_node(source, cwd, known);
    match block {
        Block::Heading(level, source) => Node::new("Text").prop("weight", "strong").prop("size", if *level <= 2 { "field" } else { "small" }).child(spans(source)),
        Block::Paragraph(source) => spans(source),
        Block::List { ordered, items } => stack().prop("gap", "xs").children(items.iter().enumerate().map(|(at, item)| {
            let mark = match item.checked {
                None if *ordered => format!("{}.", at + 1),
                None => "\u{2022}".to_owned(),
                Some(true) => "\u{2611}".to_owned(),
                Some(false) => "\u{2610}".to_owned(),
            };
            row().prop("gap", "s").prop("align", "start").child(stack().prop("width", item.depth * 16)).child(text(mark).prop("tone", "muted")).child(spans(&item.text))
        })),
        Block::Code { lang, text: code } => {
            let at = blocks.get();
            blocks.set(at + 1);
            crate::code_view::block(&format!("{scope}-{at}"), code, crate::code_view::fence_language(lang))
        }
        Block::Quote(source) => row().prop("gap", "s").prop("align", "start").child(text("\u{258e}").prop("tone", "muted")).child(spans(source).prop("tone", "muted")),
        Block::Rule => text("────────────────").prop("tone", "muted"),
        Block::Table { head, rows } => stack()
            .prop("gap", "xs")
            .child(row().prop("gap", "m").children(head.iter().map(|cell| spans(cell).prop("weight", "strong"))))
            .children(rows.iter().map(|cells| row().prop("gap", "m").prop("align", "start").children(cells.iter().map(|cell| spans(cell))))),
        Block::Details { summary, blocks: blocks_in } => {
            let body = stack().prop("gap", "m").children(blocks_in.iter().map(|block| block_node(block, cwd, known, scope, blocks)));
            Node::new("Disclosure").child(Node::new("Slot").prop("name", "summary").child(spans(summary))).child(Node::new("Slot").prop("name", "body").child(body))
        }
    }
}

/// A line of inline Markdown, a file reference in its words or in backticks a link.
fn spans_node(source: &str, cwd: &str, known: &dyn Fn(&str) -> bool) -> Node {
    let file = |shown: &str, reference: Reference, mono: bool| {
        let node = Node::new("Text").on("onClick", move |_| open_file(&reference)).text(shown);
        if mono { node.prop("mono", true) } else { node }
    };
    let mut line = Node::new("Text").prop("grow", true);
    for span in parse_inline(source) {
        line = match span {
            Span::Text(words) | Span::Em(words) => pieces(cwd, &words, known).into_iter().fold(line, |line, piece| match piece {
                Piece::Text(words) => line.text(words),
                Piece::File(shown, reference) => line.child(file(&shown, reference, false)),
            }),
            Span::Code(code) => match resolve(cwd, code.trim(), known) {
                Some(reference) => line.child(file(&code, reference, true)),
                None => line.child(Node::new("Text").prop("mono", true).text(code)),
            },
            Span::Strong(words) => line.child(Node::new("Text").prop("weight", "strong").text(words)),
            Span::Del(words) => line.child(Node::new("Text").prop("tone", "muted").text(words)),
            Span::Link { text: label, href } => line.child(Node::new("Text").prop("href", href.as_str()).on("onClick", move |_| host::open_url(&href)).text(label)),
        };
    }
    line
}
