//! The change a permission card asks you to allow, as a real diff: Edit, MultiEdit and Write applied to the file as it is on
//! disk (`build`), the lines round each change with their numbers, grouped the way the side-by-side view pairs them
//! (`groups`), and as the patch the diff viewer opens (`patch`). Only what is pure is here; drawing is in `diff_view.rs`.

use serde_json::Value;

use crate::session::shown;

/// The lines of context round a change, as git's.
const CONTEXT: usize = 3;
/// The most lines of one preview kept. A bigger one is not shown as a diff: the card shows the change as Claude sent it.
const KEPT: usize = 2000;
/// The most cells of the table a diff of two files' changed middle is worked out in; past it the middle is all taken out and
/// all put in.
const MAX_CELLS: usize = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Context,
    Removed,
    Added,
}

/// One line of a diff, with its number in the file before and in the file after (None where it isn't in one).
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub kind: Kind,
    pub old: Option<usize>,
    pub new: Option<usize>,
    pub text: String,
}

/// What a call does to its file: its hunks, each with up to `CONTEXT` lines round every change.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Preview {
    pub hunks: Vec<Vec<Line>>,
    pub added: usize,
    pub removed: usize,
    /// The lines past `KEPT`, left out.
    pub more: usize,
    /// The file isn't on disk: the call makes it.
    pub created: bool,
}

/// Whether the card has a diff to show: it is read from the file once the question comes.
#[derive(Debug, Default, PartialEq)]
pub enum Peek {
    #[default]
    Unasked,
    Loading,
    Ready(Preview),
    /// The file couldn't be read, or the call doesn't fit it: the card shows the call as Claude sent it.
    Unavailable,
}

/// Whether a tool's ask can have a diff.
pub fn is_edit(tool: &str) -> bool {
    matches!(tool, "Edit" | "MultiEdit" | "Write")
}

/// The file `input` names, if it is a full path.
pub fn path_of(input: &Value) -> Option<&str> {
    input["file_path"].as_str().filter(|path| path.starts_with('/'))
}

/// The call `tool` with `input` against the file's text, None where the file isn't there. None when the call doesn't fit
/// the file (its text isn't found, or is found twice without `replace_all`), and when it changes nothing.
pub fn build(tool: &str, input: &Value, file: Option<&str>) -> Option<Preview> {
    let after = match tool {
        "Write" => input["content"].as_str()?.to_owned(),
        "Edit" => replaced(file, input)?,
        "MultiEdit" => {
            let mut text = file.map(str::to_owned);
            for edit in input["edits"].as_array()? {
                text = Some(replaced(text.as_deref(), edit)?);
            }
            text?
        }
        _ => return None,
    };
    preview(file.unwrap_or_default(), &after, file.is_none())
}

/// `edit` applied to `text`, or to no file at all: an empty `old_string` then makes the file.
fn replaced(text: Option<&str>, edit: &Value) -> Option<String> {
    let (old, new) = (edit["old_string"].as_str()?, edit["new_string"].as_str()?);
    let Some(text) = text else {
        return old.is_empty().then(|| new.to_owned());
    };
    if old.is_empty() {
        return None;
    }
    match (text.matches(old).count(), edit["replace_all"] == true) {
        (0, _) => None,
        (1, _) => Some(text.replacen(old, new, 1)),
        (_, true) => Some(text.replace(old, new)),
        _ => None,
    }
}

/// How each line of `old` and `new` stands, with its numbers: the common start and end first, then the longest common run
/// of lines between them.
fn ops(old: &[&str], new: &[&str]) -> Vec<(Kind, Option<usize>, Option<usize>)> {
    let mut start = 0;
    while start < old.len() && start < new.len() && old[start] == new[start] {
        start += 1;
    }
    let mut end = 0;
    while end < old.len() - start && end < new.len() - start && old[old.len() - 1 - end] == new[new.len() - 1 - end] {
        end += 1;
    }
    let (a, b) = (&old[start..old.len() - end], &new[start..new.len() - end]);
    let mut out: Vec<(Kind, Option<usize>, Option<usize>)> = (0..start).map(|i| (Kind::Context, Some(i + 1), Some(i + 1))).collect();
    let (mut i, mut j) = (0, 0);
    if (a.len() + 1) * (b.len() + 1) <= MAX_CELLS {
        // `table[i][j]` is how many lines the tails `a[i..]` and `b[j..]` have in common.
        let width = b.len() + 1;
        let mut table = vec![0u32; (a.len() + 1) * width];
        for i in (0..a.len()).rev() {
            for j in (0..b.len()).rev() {
                table[i * width + j] = if a[i] == b[j] { table[(i + 1) * width + j + 1] + 1 } else { table[(i + 1) * width + j].max(table[i * width + j + 1]) };
            }
        }
        while i < a.len() && j < b.len() {
            if a[i] == b[j] {
                out.push((Kind::Context, Some(start + i + 1), Some(start + j + 1)));
                (i, j) = (i + 1, j + 1);
            } else if table[(i + 1) * width + j] >= table[i * width + j + 1] {
                out.push((Kind::Removed, Some(start + i + 1), None));
                i += 1;
            } else {
                out.push((Kind::Added, None, Some(start + j + 1)));
                j += 1;
            }
        }
    }
    out.extend((i..a.len()).map(|i| (Kind::Removed, Some(start + i + 1), None)));
    out.extend((j..b.len()).map(|j| (Kind::Added, None, Some(start + j + 1))));
    out.extend((0..end).map(|k| (Kind::Context, Some(old.len() - end + k + 1), Some(new.len() - end + k + 1))));
    out
}

/// The change from the text `before` to `after`, as hunks. None when there is none.
pub fn preview(before: &str, after: &str, created: bool) -> Option<Preview> {
    let (old, new): (Vec<&str>, Vec<&str>) = (before.lines().collect(), after.lines().collect());
    let all = ops(&old, &new);
    let changes: Vec<usize> = (0..all.len()).filter(|&i| all[i].0 != Kind::Context).collect();
    if changes.is_empty() {
        return None;
    }
    let mut near = vec![false; all.len()];
    for &at in &changes {
        near[at.saturating_sub(CONTEXT)..=(at + CONTEXT).min(all.len() - 1)].fill(true);
    }
    let added = changes.iter().filter(|&&i| all[i].0 == Kind::Added).count();
    let mut preview = Preview { added, removed: changes.len() - added, created, ..Preview::default() };
    let mut hunk: Vec<Line> = Vec::new();
    let mut kept = 0;
    for (i, (kind, o, n)) in all.iter().enumerate() {
        if !near[i] {
            if !hunk.is_empty() {
                preview.hunks.push(std::mem::take(&mut hunk));
            }
            continue;
        }
        if kept >= KEPT {
            preview.more += 1;
            continue;
        }
        kept += 1;
        let text = match kind {
            Kind::Added => new[n.unwrap_or(1) - 1],
            _ => old[o.unwrap_or(1) - 1],
        };
        // Whatever hides in the text is written out, as on the plain card: you are allowing what you can read.
        hunk.push(Line { kind: *kind, old: *o, new: *n, text: shown(text, false) });
    }
    if !hunk.is_empty() {
        preview.hunks.push(hunk);
    }
    Some(preview)
}

/// Lines of one hunk that are drawn together: unchanged ones, or the lines taken out and the lines put in after them.
#[derive(Debug, PartialEq)]
pub enum Group<'a> {
    Same(Vec<&'a Line>),
    Change { old: Vec<&'a Line>, new: Vec<&'a Line> },
}

/// `lines` in the groups the side-by-side view draws a row each: each run of removed lines pairs with the added lines after
/// it, and unchanged lines are on both sides.
pub fn groups(lines: &[Line]) -> Vec<Group<'_>> {
    fn same<'a>(out: &mut Vec<Group<'a>>, run: &mut Vec<&'a Line>) {
        if !run.is_empty() {
            out.push(Group::Same(std::mem::take(run)));
        }
    }
    fn change<'a>(out: &mut Vec<Group<'a>>, old: &mut Vec<&'a Line>, new: &mut Vec<&'a Line>) {
        if !old.is_empty() || !new.is_empty() {
            out.push(Group::Change { old: std::mem::take(old), new: std::mem::take(new) });
        }
    }
    let (mut out, mut kept, mut old, mut new) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for line in lines {
        match line.kind {
            Kind::Context => {
                change(&mut out, &mut old, &mut new);
                kept.push(line);
            }
            Kind::Removed => {
                same(&mut out, &mut kept);
                if !new.is_empty() {
                    change(&mut out, &mut old, &mut new);
                }
                old.push(line);
            }
            Kind::Added => {
                same(&mut out, &mut kept);
                new.push(line);
            }
        }
    }
    same(&mut out, &mut kept);
    change(&mut out, &mut old, &mut new);
    out
}

/// Which numbers a block of lines shows before each line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Numbers {
    /// Where it was and where it is, in one column.
    Both,
    /// Where it was: the left side of two.
    Old,
    /// Where it is: the right side of two.
    New,
}

/// The digits of the biggest line number in `hunks`, 0 when no line has one.
pub fn number_width(hunks: &[Vec<Line>]) -> usize {
    let biggest = hunks.iter().flatten().filter_map(|line| line.old.max(line.new)).max().unwrap_or(0);
    if biggest == 0 { 0 } else { biggest.to_string().len() }
}

/// The text of a block of `lines`, one to a line: its numbers (when `width` is not 0), a `+`, `−` or space, and the line.
/// The numbers are in the text the colours read, as the block wraps a long line under its own start and a column of its
/// own would not wrap with it. `pad` lines or more: shorter blocks end in empty lines, so two sides of a row are as tall.
pub fn block_text(lines: &[&Line], numbers: Numbers, width: usize, pad: usize) -> String {
    let number = |n: Option<usize>| match n {
        Some(n) => format!("{n:>width$}"),
        None => " ".repeat(width),
    };
    let mut rows: Vec<String> = lines
        .iter()
        .map(|line| {
            let before = match (width, numbers) {
                (0, _) => String::new(),
                (_, Numbers::Both) => format!("{} {} ", number(line.old), number(line.new)),
                (_, Numbers::Old) => format!("{} ", number(line.old)),
                (_, Numbers::New) => format!("{} ", number(line.new)),
            };
            let mark = match line.kind {
                Kind::Context => " ",
                Kind::Removed => "−",
                Kind::Added => "+",
            };
            format!("{before}{mark} {}", line.text)
        })
        .collect();
    while rows.len() < pad {
        rows.push(String::new());
    }
    rows.join("\n")
}

/// The preview as a git patch for the diff viewer, the file named as it is under the folder (`shown`).
pub fn patch(shown: &str, preview: &Preview) -> String {
    let mut out = format!("diff --git a/{shown} b/{shown}\n");
    match preview.created {
        true => out.push_str(&format!("new file mode 100644\n--- /dev/null\n+++ b/{shown}\n")),
        false => out.push_str(&format!("--- a/{shown}\n+++ b/{shown}\n")),
    }
    for hunk in &preview.hunks {
        let (old_lines, new_lines) = (hunk.iter().filter(|line| line.kind != Kind::Added).count(), hunk.iter().filter(|line| line.kind != Kind::Removed).count());
        let first = |numbers: Vec<Option<usize>>| numbers.into_iter().flatten().next().unwrap_or(0);
        let (old_at, new_at) = (first(hunk.iter().map(|line| line.old).collect()), first(hunk.iter().map(|line| line.new).collect()));
        out.push_str(&format!("@@ -{old_at},{old_lines} +{new_at},{new_lines} @@\n"));
        for line in hunk {
            let mark = match line.kind {
                Kind::Context => ' ',
                Kind::Removed => '-',
                Kind::Added => '+',
            };
            out.push_str(&format!("{mark}{}\n", line.text));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn file(n: usize) -> String {
        (1..=n).map(|at| format!("line {at}\n")).collect()
    }

    fn marks(lines: &[Line]) -> Vec<String> {
        lines.iter().map(|line| format!("{}{}", match line.kind { Kind::Context => ' ', Kind::Removed => '-', Kind::Added => '+' }, line.text)).collect()
    }

    #[test]
    fn an_edit_shows_the_lines_round_it_from_the_file_with_their_numbers() {
        let input = json!({ "file_path": "/a/b.rs", "old_string": "line 5", "new_string": "five\nand a half" });
        let preview = build("Edit", &input, Some(&file(10))).unwrap();
        assert_eq!((preview.added, preview.removed, preview.created, preview.more), (2, 1, false, 0));
        assert_eq!(preview.hunks.len(), 1);
        assert_eq!(marks(&preview.hunks[0]), [" line 2", " line 3", " line 4", "-line 5", "+five", "+and a half", " line 6", " line 7", " line 8"]);
        let numbers: Vec<(Option<usize>, Option<usize>)> = preview.hunks[0].iter().map(|line| (line.old, line.new)).collect();
        assert_eq!(numbers[0], (Some(2), Some(2)));
        assert_eq!(numbers[3], (Some(5), None));
        assert_eq!(numbers[4], (None, Some(5)));
        // After the two lines put in for one, the lines below are a line further on.
        assert_eq!(numbers[6], (Some(6), Some(7)));
    }

    #[test]
    fn a_replace_all_changes_every_match_and_a_match_found_twice_without_it_is_no_diff() {
        let text = "x = 1\ny\nx = 1\n";
        let once = json!({ "old_string": "x = 1", "new_string": "x = 2" });
        assert_eq!(build("Edit", &once, Some(text)), None);
        let all = json!({ "old_string": "x = 1", "new_string": "x = 2", "replace_all": true });
        let preview = build("Edit", &all, Some(text)).unwrap();
        assert_eq!((preview.added, preview.removed), (2, 2));
    }

    #[test]
    fn a_multi_edit_makes_a_hunk_for_each_change_far_apart_and_each_edit_sees_the_one_before() {
        let input = json!({ "file_path": "/a", "edits": [
            { "old_string": "line 3\n", "new_string": "three\n" },
            { "old_string": "line 19", "new_string": "nineteen" },
            { "old_string": "three\n", "new_string": "second\n" },
        ] });
        let preview = build("MultiEdit", &input, Some(&file(20))).unwrap();
        assert_eq!(preview.hunks.len(), 2);
        assert_eq!(marks(&preview.hunks[0]), [" line 1", " line 2", "-line 3", "+second", " line 4", " line 5", " line 6"]);
        assert_eq!(marks(&preview.hunks[1]), [" line 16", " line 17", " line 18", "-line 19", "+nineteen", " line 20"]);
        assert_eq!((preview.added, preview.removed), (2, 2));
    }

    #[test]
    fn a_write_to_a_path_that_is_not_there_reads_as_all_added() {
        let input = json!({ "file_path": "/a/new.rs", "content": "fn main() {}\n\nfn two() {}\n" });
        let preview = build("Write", &input, None).unwrap();
        assert!(preview.created);
        assert_eq!((preview.added, preview.removed), (3, 0));
        assert_eq!(marks(&preview.hunks[0]), ["+fn main() {}", "+", "+fn two() {}"]);
        assert_eq!(preview.hunks[0][2].new, Some(3));
        assert_eq!(preview.hunks[0][2].old, None);
        // An Edit with no old text makes the file too, and one that names text in a file that isn't there is no diff.
        let makes = json!({ "old_string": "", "new_string": "a\n" });
        assert_eq!(build("Edit", &makes, None).unwrap().added, 1);
        assert_eq!(build("Edit", &json!({ "old_string": "a", "new_string": "b" }), None), None);
    }

    #[test]
    fn a_write_over_a_file_shows_what_differs_from_it() {
        let input = json!({ "file_path": "/a", "content": "one\nTWO\nthree\nfour\n" });
        let preview = build("Write", &input, Some("one\ntwo\nthree\n")).unwrap();
        assert!(!preview.created);
        assert_eq!((preview.added, preview.removed), (2, 1));
        assert_eq!(marks(&preview.hunks[0]), [" one", "-two", "+TWO", " three", "+four"]);
        // The same content is no change.
        assert_eq!(build("Write", &json!({ "content": "one\n" }), Some("one\n")), None);
    }

    #[test]
    fn a_call_that_does_not_fit_the_file_has_no_diff_so_the_card_keeps_its_plain_lines() {
        let missing = json!({ "old_string": "not there", "new_string": "x" });
        assert_eq!(build("Edit", &missing, Some("some text\n")), None);
        assert_eq!(build("MultiEdit", &json!({ "edits": [{ "old_string": "some", "new_string": "x" }, missing] }), Some("some text\n")), None);
        assert_eq!(build("MultiEdit", &json!({ "file_path": "/a" }), Some("x")), None);
        assert_eq!(build("Bash", &json!({ "command": "ls" }), Some("x")), None);
        assert_eq!(path_of(&json!({ "file_path": "src/a.rs" })), None);
        assert_eq!(path_of(&json!({ "file_path": "/src/a.rs" })), Some("/src/a.rs"));
    }

    #[test]
    fn what_hides_in_a_line_is_written_out_but_tabs_stay() {
        let input = json!({ "old_string": "a", "new_string": "\tb\u{202e}c" });
        let preview = build("Edit", &input, Some("a\n")).unwrap();
        assert_eq!(preview.hunks[0][1].text, "\tb\\u{202e}c");
    }

    #[test]
    fn a_run_of_removed_lines_pairs_with_the_added_lines_after_it() {
        let line = |kind, n| Line { kind, old: Some(n), new: Some(n), text: n.to_string() };
        let lines = [line(Kind::Context, 1), line(Kind::Removed, 2), line(Kind::Removed, 3), line(Kind::Added, 2), line(Kind::Added, 3), line(Kind::Added, 4), line(Kind::Context, 4), line(Kind::Added, 5), line(Kind::Removed, 6)];
        let grouped = groups(&lines);
        assert_eq!(grouped.len(), 5);
        assert!(matches!(&grouped[0], Group::Same(same) if same.len() == 1));
        assert!(matches!(&grouped[1], Group::Change { old, new } if old.len() == 2 && new.len() == 3));
        assert!(matches!(&grouped[2], Group::Same(same) if same.len() == 1));
        // Added lines then a removed one are two changes: a removed run pairs only with what comes after it.
        assert!(matches!(&grouped[3], Group::Change { old, new } if old.is_empty() && new.len() == 1));
        assert!(matches!(&grouped[4], Group::Change { old, new } if old.len() == 1 && new.is_empty()));
    }

    #[test]
    fn a_block_has_its_numbers_before_the_mark_and_a_short_side_is_padded_to_the_tall_one() {
        let input = json!({ "old_string": "line 9", "new_string": "nine" });
        let preview = build("Edit", &input, Some(&file(12))).unwrap();
        let width = number_width(&preview.hunks);
        assert_eq!(width, 2);
        let hunk = &preview.hunks[0];
        let removed: Vec<&Line> = hunk.iter().filter(|line| line.kind == Kind::Removed).collect();
        let added: Vec<&Line> = hunk.iter().filter(|line| line.kind == Kind::Added).collect();
        assert_eq!(block_text(&removed, Numbers::Old, width, 1), " 9 − line 9");
        assert_eq!(block_text(&added, Numbers::New, width, 3), " 9 + nine\n\n");
        let context: Vec<&Line> = hunk.iter().skip(2).take(1).collect();
        assert_eq!(block_text(&context, Numbers::Both, width, 0), " 8  8   line 8");
        assert_eq!(block_text(&removed, Numbers::Both, width, 0), " 9    − line 9");
        // A diff with no numbers, as a finished edit's, is the marks and the text alone.
        let plain = Line { kind: Kind::Added, old: None, new: None, text: "x".into() };
        assert_eq!(number_width(&[vec![plain.clone()]]), 0);
        assert_eq!(block_text(&[&plain], Numbers::Both, 0, 0), "+ x");
    }

    #[test]
    fn the_patch_for_the_diff_viewer_names_each_hunks_lines() {
        let edit = json!({ "old_string": "line 5", "new_string": "five" });
        let preview = build("Edit", &edit, Some(&file(10))).unwrap();
        assert_eq!(
            patch("src/a.rs", &preview),
            "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -2,7 +2,7 @@\n line 2\n line 3\n line 4\n-line 5\n+five\n line 6\n line 7\n line 8\n"
        );
        let made = build("Write", &json!({ "content": "a\nb\n" }), None).unwrap();
        assert_eq!(patch("n.txt", &made), "diff --git a/n.txt b/n.txt\nnew file mode 100644\n--- /dev/null\n+++ b/n.txt\n@@ -0,0 +1,2 @@\n+a\n+b\n");
    }

    #[test]
    fn a_change_too_long_to_keep_says_how_many_lines_it_left_out() {
        let content: String = (1..=KEPT + 5).map(|at| format!("l{at}\n")).collect();
        let preview = build("Write", &json!({ "content": content }), None).unwrap();
        assert_eq!((preview.hunks[0].len(), preview.more, preview.added), (KEPT, 5, KEPT + 5));
    }
}
