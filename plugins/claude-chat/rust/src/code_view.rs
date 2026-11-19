//! A code block of the conversation, in a reply or in a tool's output: one `Code` node that Wings colours by its language,
//! cut to its first `FOLD_LINES` lines with a row under it that opens the rest in place and shuts it again. Folded, only
//! the shown lines are sent, so a block of thousands of lines costs what a short one does; its Copy button, where it has
//! one, copies from the item's own text and so copies all of it. Which blocks are open is kept here, by a key the caller
//! makes of the item and the block's place in it, so it survives the page being built again.

use std::{cell::RefCell, collections::HashSet};

use wings_plugin::{
    markdown::highlighted_code,
    ui::{Node, button, row, stack},
};

use crate::session::first_lines;

/// A block with more lines than this shows this many, and a row to open the rest.
pub const FOLD_LINES: usize = 20;

thread_local! {
    /// The blocks opened past their first lines, by key.
    static OPEN: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

pub fn is_open(key: &str) -> bool {
    OPEN.with(|open| open.borrow().contains(key))
}

pub fn toggle(key: &str) {
    OPEN.with(|open| {
        let mut open = open.borrow_mut();
        if !open.remove(key) {
            open.insert(key.to_owned());
        }
    });
}

/// What a block shows: the text, and the row that folds it. `row` is None for a block short enough to show whole.
#[derive(Debug, PartialEq)]
pub struct Plan<'a> {
    pub shown: &'a str,
    pub row: Option<String>,
}

/// What the row under a folded block says: how many lines it opens, or Show less once it is open.
pub fn label(hidden: usize, open: bool) -> String {
    match (open, hidden) {
        (true, _) => "Show less".to_owned(),
        (false, 1) => "Show 1 more line".to_owned(),
        (false, hidden) => format!("Show {hidden} more lines"),
    }
}

/// The row that opens and shuts the block `key`.
pub fn toggle_row(key: &str, label: String) -> Node {
    let key = key.to_owned();
    row().child(button(label, "ghost", false, move || toggle(&key)).prop("size", "sm"))
}

pub fn plan(code: &str, open: bool) -> Plan<'_> {
    let (head, hidden) = first_lines(code, FOLD_LINES);
    match (hidden, open) {
        (0, _) => Plan { shown: code, row: None },
        (hidden, true) => Plan { shown: code, row: Some(label(hidden, true)) },
        (hidden, false) => Plan { shown: head, row: Some(label(hidden, false)) },
    }
}

/// A file read as Claude Code prints it has a line number and an arrow or a tab before each line. They are taken off, so
/// the colours read the code. Output where fewer than half the lines have one is left as it is.
pub fn unnumbered(output: &str) -> String {
    fn code(line: &str) -> Option<&str> {
        let line = line.trim_start();
        let digits = line.bytes().take_while(u8::is_ascii_digit).count();
        let rest = &line[digits..];
        if digits == 0 { None } else { rest.strip_prefix('→').or_else(|| rest.strip_prefix('\t')) }
    }
    let lines: Vec<&str> = output.lines().collect();
    let numbered = lines.iter().filter(|line| code(line).is_some()).count();
    if numbered == 0 || numbered * 2 < lines.len() {
        return output.to_owned();
    }
    lines.iter().map(|line| code(line).unwrap_or(*line)).collect::<Vec<_>>().join("\n")
}

/// The language a file's name says, for the colours: its extension, or the whole name where it has none (`Dockerfile`).
/// Wings colours only what it has a grammar for.
pub fn language_of_path(path: &str) -> &str {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => extension,
        _ => name,
    }
}

/// The language a fence's info string names: its first word, as in "ts title=a.ts" or "rust,ignore".
pub fn fence_language(info: &str) -> &str {
    let info = info.trim();
    let end = info.find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '+' | '-' | '#' | '.'))).unwrap_or(info.len());
    &info[..end]
}

/// `code` as a block in `language` (a fence's name or an extension; empty for none), folded unless it is short or you
/// opened it. `key` names it among the page's blocks.
pub fn block(key: &str, code: &str, language: &str) -> Node {
    let plan = plan(code, is_open(key));
    let code = highlighted_code(plan.shown, language);
    match plan.row {
        None => code,
        Some(label) => stack().prop("gap", "xs").child(code).child(toggle_row(key, label)).key(key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(n: usize) -> String {
        (1..=n).map(|at| format!("line {at}")).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn a_long_block_shows_twenty_lines_and_says_how_many_it_keeps_back() {
        let code = lines(50);
        let folded = plan(&code, false);
        assert_eq!(folded.shown.lines().count(), 20);
        assert_eq!(folded.shown.lines().last(), Some("line 20"));
        assert_eq!(folded.row.as_deref(), Some("Show 30 more lines"));
        assert_eq!(plan(&lines(21), false).row.as_deref(), Some("Show 1 more line"));
    }

    #[test]
    fn opening_it_shows_every_line_and_the_row_reads_show_less() {
        let code = lines(50);
        let open = plan(&code, true);
        assert_eq!(open.shown, code);
        assert_eq!(open.row.as_deref(), Some("Show less"));
    }

    #[test]
    fn a_block_of_twenty_lines_or_fewer_shows_whole_with_no_row() {
        for n in [1, 19, 20] {
            let code = lines(n);
            assert_eq!(plan(&code, false), Plan { shown: &code, row: None }, "{n}");
        }
        assert_eq!(plan("", false), Plan { shown: "", row: None });
        // A trailing newline is no extra line.
        assert_eq!(plan(&format!("{}\n", lines(20)), false).row, None);
    }

    #[test]
    fn a_click_opens_one_block_and_a_second_shuts_it_and_blocks_are_told_apart_by_key() {
        assert!(!is_open("code-3-0"));
        toggle("code-3-0");
        assert!(is_open("code-3-0") && !is_open("code-3-1") && !is_open("code-4-0"));
        toggle("code-3-0");
        assert!(!is_open("code-3-0"));
    }

    #[test]
    fn a_folded_block_of_thousands_of_lines_is_as_few_nodes_as_one_of_fifty() {
        let (few, many) = (lines(50), lines(5_000));
        assert_eq!(block("few", &few, "rust").count(), block("many", &many, "rust").count());
        assert_eq!(plan(&many, false).shown.lines().count(), FOLD_LINES);
        // A block that fits has no row, so it is the one node.
        assert_eq!(block("short", &lines(5), "rust").count(), 1);
    }

    #[test]
    fn a_read_loses_its_line_numbers_and_other_output_is_left_alone() {
        assert_eq!(unnumbered("     1→const a = 1;\n     2→\n    10→export {};"), "const a = 1;\n\nexport {};");
        assert_eq!(unnumbered("     1\tfn main() {}"), "fn main() {}");
        // A line that is no number's, among many that are, stays as it is.
        assert_eq!(unnumbered("   1→a\n   2→b\n<note>"), "a\nb\n<note>");
        let printed = "Found 3 files\nsrc/a.ts\n2 tests failed";
        assert_eq!(unnumbered(printed), printed);
        assert_eq!(unnumbered(""), "");
    }

    #[test]
    fn a_fence_names_its_language_by_its_first_word() {
        assert_eq!(fence_language("rust"), "rust");
        assert_eq!(fence_language(" ts title=a.ts"), "ts");
        assert_eq!(fence_language("rust,ignore"), "rust");
        assert_eq!(fence_language("c++"), "c++");
        assert_eq!(fence_language(""), "");
        assert_eq!(fence_language("{.python}"), "");
    }

    #[test]
    fn the_language_comes_from_the_extension_or_the_whole_name() {
        assert_eq!(language_of_path("/work/shop/src/checkout.ts"), "ts");
        assert_eq!(language_of_path("src\\lib.rs"), "rs");
        assert_eq!(language_of_path("archive.tar.gz"), "gz");
        assert_eq!(language_of_path("/work/Dockerfile"), "Dockerfile");
        assert_eq!(language_of_path("/work/.gitignore"), ".gitignore");
        assert_eq!(language_of_path(""), "");
    }
}
