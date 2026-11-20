//! Diffs in the chat: a finished edit's lines and the change a permission card asks you to allow, in one column or side by
//! side. Side by side pairs each run of removed lines with the added lines after it, old on the left and new on the right,
//! each side with its own line numbers, as Wings' diff viewer does. A card with no room for two columns keeps one. The card
//! for a permission asks reads the file from disk first (`fetch`), and has a button that opens the change in the diff viewer.

use serde_json::{Value, json};
use wings_plugin::{
    format::plural,
    host,
    markdown::highlighted_code,
    task,
    ui::{Node, button, card, muted, row, stack},
};

use crate::{
    chat::{self, Chat},
    code_view,
    diff_preview::{self, Group, Kind, Line, Numbers, Peek, Preview},
    edits::relative,
    prefs,
    session::{Diff, Item},
};

/// The most bytes of a file read for a diff. A bigger one is not shown as a diff.
const FILE_BYTES: usize = 2_000_000;

/// How the diffs of this chat are drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub split: bool,
    /// Whether the card offers the choice.
    pub toggle: bool,
}

/// The chat's side panel calls a tab under about 1,100 points narrow, where a 760-point column still holds two sides, so
/// the choice is yours at any width. A sidebar (a subagent's, a pane's, the session's) is too narrow for two sides: one
/// column there, with no choice.
pub fn look(c: &Chat) -> Look {
    if SIDEBAR {
        return Look { split: false, toggle: false };
    }
    Look { split: c.diff_split, toggle: true }
}

const SIDEBAR: bool = cfg!(any(feature = "subagent", feature = "pane", feature = "session"));

/// A finished edit's lines: what the call's input says, with no numbers, as it has no file to count in.
pub fn finished(key: &str, language: &str, diff: &Diff, look: Look) -> Node {
    let lines = diff
        .lines
        .iter()
        .map(|(added, text)| Line { kind: if *added { Kind::Added } else { Kind::Removed }, old: None, new: None, text: text.clone() })
        .collect();
    view(key, language, &[lines], diff.more, look)
}

/// The change an ask waits on, as a diff, when it has one to show whole: a preview that was cut, or an ask whose text was
/// cut, is not one to allow by.
pub fn ask_diff<'a>(ask: &'a crate::session::Ask) -> Option<&'a Preview> {
    match &ask.preview {
        Peek::Ready(preview) if !ask.cut_short && preview.more == 0 => Some(preview),
        _ => None,
    }
}

/// The card's body for an ask with a diff: its path, the diff, and the button that opens it in the diff viewer.
pub fn ask_body(ask: &crate::session::Ask, preview: &Preview, look: Look) -> Vec<Node> {
    let language = diff_preview::path_of(&ask.input).map_or("", code_view::language_of_path);
    let request = ask.request.clone();
    let counts = format!("{}{}", if preview.created { "New file, " } else { "" }, plural(preview.added + preview.removed, "line"));
    vec![
        wings_plugin::ui::text(crate::session::ask_head(&ask.tool, &ask.input)).prop("mono", true).prop("size", "small"),
        view(&format!("ask-{}", ask.request), language, &preview.hunks, preview.more, look),
        row()
            .prop("gap", "s")
            .prop("align", "center")
            .child(button("Open in diff viewer", "outline", false, move || open(&request)).prop("size", "sm"))
            .child(muted(counts).prop("size", "small")),
    ]
}

/// `hunks` as a card: unchanged lines plain, removed lines in a red band and added ones in a green one, in the file's
/// language. The first lines show, and a row opens the rest. A choice of one column or two sits above them.
pub fn view(key: &str, language: &str, hunks: &[Vec<Line>], more: usize, look: Look) -> Node {
    let total: usize = hunks.iter().map(Vec::len).sum();
    let open = code_view::is_open(key);
    let shown = if open { total } else { total.min(code_view::FOLD_LINES) };
    let width = diff_preview::number_width(hunks);
    let mut left = shown;
    let mut parts: Vec<Node> = Vec::new();
    for (h, hunk) in hunks.iter().enumerate() {
        if left == 0 {
            break;
        }
        let take = &hunk[..hunk.len().min(left)];
        left -= take.len();
        if h > 0 {
            parts.push(muted("⋯").prop("size", "small").key(format!("gap-{h}")));
        }
        for (g, group) in diff_preview::groups(take).iter().enumerate() {
            parts.push(group_node(group, language, width, look.split).key(format!("h{h}-g{g}")));
        }
    }
    let fold = (total > code_view::FOLD_LINES).then(|| code_view::toggle_row(key, code_view::label(total - shown, open)));
    let more = (more > 0 && (open || total <= code_view::FOLD_LINES)).then(|| muted(format!("{} more", plural(more, "line"))));
    let switch = look.toggle.then(|| {
        let split = look.split;
        row().prop("justify", "end").child(button(if split { "One column" } else { "Side by side" }, "ghost", false, move || drop(prefs::diff_split().set(!split))).prop("size", "sm"))
    });
    card().child(stack().prop("gap", "none").maybe(switch).children(parts).maybe(more).maybe(fold))
}

/// One block of lines as a band in the tone of what it holds: red for removed, green for added, none for the rest.
fn band(lines: &[&Line], numbers: Numbers, width: usize, pad: usize, language: &str, tone: Option<&str>) -> Node {
    let code = highlighted_code(&diff_preview::block_text(lines, numbers, width, pad), language);
    match tone {
        Some(tone) => code.prop("tone", tone),
        None => code.prop("variant", "band"),
    }
}

fn group_node(group: &Group, language: &str, width: usize, split: bool) -> Node {
    // One column: a run of unchanged lines, or the removed run and then the added one, each with both numbers.
    let one = |lines: &[&Line], tone: Option<&str>| band(lines, Numbers::Both, width, 0, language, tone);
    // Two columns of the same width, side by side.
    let two = |left: Node, right: Node| row().prop("gap", "xs").prop("align", "start").child(stack().prop("grow", true).child(left)).child(stack().prop("grow", true).child(right));
    match (group, split) {
        (Group::Same(lines), false) => one(lines, None),
        (Group::Same(lines), true) => two(band(lines, Numbers::Old, width, 0, language, None), band(lines, Numbers::New, width, 0, language, None)),
        (Group::Change { old, new }, false) => stack().prop("gap", "none").maybe((!old.is_empty()).then(|| one(old, Some("danger")))).maybe((!new.is_empty()).then(|| one(new, Some("success")))),
        (Group::Change { old, new }, true) => {
            let tall = old.len().max(new.len());
            // A side with nothing on it is left empty, so the other side's band is the only tint.
            let side = |lines: &[&Line], numbers, tone| if lines.is_empty() { stack() } else { band(lines, numbers, width, tall, language, Some(tone)) };
            two(side(old, Numbers::Old, "danger"), side(new, Numbers::New, "success"))
        }
    }
}

/// Reads the file of every ask that waits on an Edit, MultiEdit or Write, once each, and makes its diff. Called after each
/// line Claude Code prints, for the run `run`. A file that can't be read, or that the call doesn't fit, leaves the card as
/// Claude Code described it.
pub fn fetch(run: u32) {
    let wanted: Vec<(String, String, Value, String)> = chat::with(|c| {
        if c.run != run {
            return Vec::new();
        }
        let cwd = c.cwd.clone();
        c.session
            .items
            .iter_mut()
            .filter_map(|item| match item {
                Item::Ask(ask) if ask.answer.is_none() && ask.preview == Peek::Unasked && diff_preview::is_edit(&ask.tool) => {
                    ask.preview = Peek::Loading;
                    Some((ask.request.clone(), ask.tool.clone(), ask.input.clone(), cwd.clone()))
                }
                _ => None,
            })
            .collect()
    });
    for (request, tool, input, cwd) in wanted {
        task::spawn(async move {
            let preview = read(&cwd, &tool, &input).await;
            chat::with(|c| {
                let ask = c.session.items.iter_mut().find_map(|item| match item {
                    Item::Ask(ask) if ask.request == request => Some(ask),
                    _ => None,
                });
                if let Some(ask) = ask {
                    ask.preview = preview.map_or(Peek::Unavailable, Peek::Ready);
                }
            });
        });
    }
}

/// The diff of a call, from its file as it is now. None where the file can't be read or the call doesn't fit it.
async fn read(cwd: &str, tool: &str, input: &Value) -> Option<Preview> {
    let path = diff_preview::path_of(input)?;
    let out = host::exec_in(cwd, "cat", &[path]).await.ok()?;
    let text = match out["code"].as_i64() {
        Some(0) => Some(out["stdout"].as_str()?.to_owned()).filter(|text| text.len() <= FILE_BYTES)?,
        // A path that isn't there is a file the call makes, what a Write to a new path does.
        _ if out["stderr"].as_str().is_some_and(|said| said.contains("No such file or directory")) => return diff_preview::build(tool, input, None),
        _ => return None,
    };
    diff_preview::build(tool, input, Some(&text))
}

/// Opens the change an ask waits on in the diff viewer, from the card's button.
fn open(request: &str) {
    let found = chat::with(|c| {
        let ask = c.session.items.iter().find_map(|item| match item {
            Item::Ask(ask) if ask.request == request => Some(ask),
            _ => None,
        })?;
        let Peek::Ready(preview) = &ask.preview else { return None };
        let path = diff_preview::path_of(&ask.input)?;
        let shown = relative(&c.cwd, path).trim_start_matches('/').to_owned();
        Some((diff_preview::patch(&shown, preview), shown, c.cwd.clone(), c.folder.clone()))
    });
    if let Some((patch, title, cwd, folder)) = found {
        crate::edits_view::open(json!({ "title": title, "subtitle": folder, "cwd": cwd, "patch": patch, "focus": { "path": title, "side": "new" } }));
    }
}

#[cfg(test)]
mod tests {

    /// The subagent sidebar's diffs offered Side by side, which a sidebar is too narrow for: one column there, no switch.
    #[test]
    #[cfg(feature = "chat")]
    fn a_sidebar_draws_one_column_with_no_switch_and_a_tab_offers_both() {
        crate::chat::with(|c| {
            c.diff_split = true;
            let shown = look(c);
            if SIDEBAR {
                assert_eq!(shown, Look { split: false, toggle: false });
            } else {
                assert_eq!(shown, Look { split: true, toggle: true });
            }
            c.diff_split = false;
        });
    }
    use super::*;

    #[test]
    fn the_choice_is_offered_and_kept_at_any_width() {
        chat::with(|c| {
            for narrow in [None, Some(false), Some(true)] {
                c.narrow = narrow;
                c.diff_split = true;
                assert_eq!(look(c), Look { split: true, toggle: true });
                c.diff_split = false;
                assert_eq!(look(c), Look { split: false, toggle: true });
            }
            c.narrow = None;
        });
    }

    #[test]
    fn an_ask_shows_its_diff_only_when_the_whole_change_is_in_it() {
        let input = serde_json::json!({ "file_path": "/a/b.rs", "old_string": "a", "new_string": "b" });
        let preview = diff_preview::build("Edit", &input, Some("a\n")).unwrap();
        let mut ask = crate::session::Ask {
            request: "r".into(),
            tool: "Edit".into(),
            summary: String::new(),
            detail: String::new(),
            cut_short: false,
            reason: None,
            suggestions: Vec::new(),
            input,
            preview: Peek::Loading,
            answer: None,
        };
        assert!(ask_diff(&ask).is_none());
        ask.preview = Peek::Ready(preview.clone());
        assert!(ask_diff(&ask).is_some());
        ask.cut_short = true;
        assert!(ask_diff(&ask).is_none());
        ask.cut_short = false;
        ask.preview = Peek::Ready(Preview { more: 3, ..preview });
        assert!(ask_diff(&ask).is_none());
        ask.preview = Peek::Unavailable;
        assert!(ask_diff(&ask).is_none());
    }
}
