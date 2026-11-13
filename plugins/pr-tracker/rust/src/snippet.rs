//! The few lines of a file around a commented range.

use crate::types::SnippetLine;
use std::collections::BTreeSet;

const ABOVE: i64 = 4;
const BELOW: i64 = 3;
/// A comment on a long range shows its first lines, not the whole range.
const MAX_RANGE: i64 = 6;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Span {
    pub from: i64,
    pub to: i64,
    pub last: i64,
}

/// The first and last line a snippet of a commented range shows, and its last commented line, before the file's end cuts it. A card holds this room while the file loads. A single line has `end` equal to `start`.
pub fn snippet_span(start: i64, end: i64) -> Span {
    let last = end.max(start).min(start + MAX_RANGE - 1);
    Span { from: (start - ABOVE).max(1), to: last + BELOW, last }
}

/// The new-side lines a `diff -U0` adds or changes, read from its hunk headers (`@@ -a,b +c,d @@`).
pub fn added_lines_of(diff: &str) -> BTreeSet<i64> {
    let mut added = BTreeSet::new();
    for line in diff.lines() {
        let Some(rest) = line.strip_prefix("@@ -") else { continue };
        let Some((old, rest)) = rest.split_once(' ') else { continue };
        if old.is_empty() || old.contains(char::is_whitespace) {
            continue;
        }
        let Some(rest) = rest.strip_prefix('+') else { continue };
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let Ok(first) = rest[..digits].parse::<i64>() else { continue };
        let tail = &rest[digits..];
        let (count, tail) = match tail.strip_prefix(',') {
            Some(more) => {
                let n = more.bytes().take_while(u8::is_ascii_digit).count();
                match more[..n].parse::<i64>() {
                    Ok(count) => (count, &more[n..]),
                    Err(_) => continue,
                }
            }
            None => (1, tail),
        };
        if tail.starts_with(" @@") {
            added.extend(first..first + count);
        }
    }
    added
}

/// The lines from 4 above the commented range to 3 below it (1-based, inclusive). Empty when the range is past the end of the file. `added`, when given, marks the lines the PR added. A single line has `end` equal to `start`.
pub fn snippet_of(content: &str, start: i64, end: i64, added: Option<&BTreeSet<i64>>) -> Vec<SnippetLine> {
    let mut lines: Vec<&str> = content.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    if start < 1 || start > lines.len() as i64 {
        return Vec::new();
    }
    let span = snippet_span(start, end);
    let to = (lines.len() as i64).min(span.to);
    (span.from..=to)
        .map(|n| SnippetLine { n, text: lines[(n - 1) as usize].trim_end_matches('\r').to_owned(), hit: n >= start && n <= span.last, added: added.map(|a| a.contains(&n)), removed: None })
        .collect()
}

#[derive(PartialEq, Clone, Copy)]
enum Kind {
    Context,
    Added,
    Removed,
}

struct Row<'a> {
    kind: Kind,
    text: &'a str,
    old: i64,
    now: i64,
}

fn number_at(text: &str) -> Option<(i64, &str)> {
    let n = text.bytes().take_while(u8::is_ascii_digit).count();
    (n > 0).then(|| text[..n].parse().ok().map(|v| (v, &text[n..]))).flatten()
}

/// The `-old` and `+new` start lines of a hunk header `@@ -a,b +c,d @@`.
fn hunk_header(line: &str) -> Option<(i64, i64)> {
    let (old, rest) = number_at(line.strip_prefix("@@ -")?)?;
    let rest = match rest.strip_prefix(',') {
        Some(more) => number_at(more)?.1,
        None => rest,
    };
    let (now, _) = number_at(rest.strip_prefix(" +")?)?;
    Some((old, now))
}

/// The rows of a unified diff that end at the commented lines, as a host's page shows a review thread: up to 4 rows above
/// the first commented line (context, `+` and `-` alike), then the commented ones. `side` (`additions` or `deletions`)
/// and the numbers say which lines were commented: new-file numbers for `additions`, old-file numbers for `deletions`.
/// None when the diff has no such line. A single line has `end` equal to `start`.
pub fn diff_window(patch: &str, side: &str, start: i64, end: i64) -> Option<Vec<SnippetLine>> {
    let mut rows: Vec<Row> = Vec::new();
    let (mut old, mut now, mut in_hunk) = (0, 0, false);
    let lines: Vec<&str> = patch.split('\n').collect();
    for (i, line) in lines.iter().enumerate() {
        if let Some((o, n)) = hunk_header(line) {
            (old, now, in_hunk) = (o, n, true);
        } else if line.starts_with("diff ") {
            in_hunk = false;
        } else if in_hunk && line.starts_with('+') {
            rows.push(Row { kind: Kind::Added, text: &line[1..], old, now });
            now += 1;
        } else if in_hunk && line.starts_with('-') {
            rows.push(Row { kind: Kind::Removed, text: &line[1..], old, now });
            old += 1;
        } else if in_hunk && (line.starts_with(' ') || (line.is_empty() && i < lines.len() - 1)) {
            // A blank context line may have lost its leading space; the patch's last blank line is only its final newline.
            rows.push(Row { kind: Kind::Context, text: line.get(1..).unwrap_or(""), old, now });
            old += 1;
            now += 1;
        }
    }
    let on = |r: &Row| if side == "additions" { r.kind != Kind::Removed && r.now >= start && r.now <= end } else { r.kind != Kind::Added && r.old >= start && r.old <= end };
    let first = rows.iter().position(on)?;
    let last = rows.iter().rposition(on)?.min(first + MAX_RANGE as usize - 1);
    let from = first.saturating_sub(ABOVE as usize);
    Some(
        rows[from..=last]
            .iter()
            .enumerate()
            .map(|(i, r)| SnippetLine {
                n: if r.kind == Kind::Removed { r.old } else { r.now },
                text: r.text.to_owned(),
                hit: from + i >= first && on(r),
                added: (r.kind == Kind::Added).then_some(true),
                removed: (r.kind == Kind::Removed).then_some(true),
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hunk() -> String {
        ["@@ -10,6 +20,7 @@ fn", " a", " b", "-old1", "-old2", "+new1", "+new2", " c", " d", "+tail"].join("\n")
    }

    fn view(lines: &[SnippetLine]) -> Vec<String> {
        lines.iter().map(|l| format!("{}{}{}{}", l.n, if l.added == Some(true) { "+" } else if l.removed == Some(true) { "-" } else { " " }, if l.hit { "*" } else { " " }, l.text)).collect()
    }

    fn file() -> String {
        (1..=50).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n") + "\n"
    }

    fn hits(lines: &[SnippetLine]) -> Vec<i64> {
        lines.iter().filter(|l| l.hit).map(|l| l.n).collect()
    }

    #[test]
    fn a_diff_window_ends_at_the_commented_line_and_marks_rows_numbering_removed_rows_by_their_old_line() {
        assert_eq!(view(&diff_window(&hunk(), "additions", 25, 25).unwrap()), ["13- old2", "22+ new1", "23+ new2", "24  c", "25 *d"]);
        assert_eq!(view(&diff_window(&hunk(), "additions", 22, 23).unwrap()), ["20  a", "21  b", "12- old1", "13- old2", "22+*new1", "23+*new2"]);
        assert_eq!(view(&diff_window(&hunk(), "deletions", 13, 13).unwrap()), ["20  a", "21  b", "12- old1", "13-*old2"]);
    }

    #[test]
    fn a_diff_window_is_none_when_the_diff_has_no_such_line_and_a_long_range_shows_its_first_rows() {
        assert_eq!(diff_window(&hunk(), "additions", 99, 99), None);
        assert_eq!(diff_window("not a diff", "additions", 1, 1), None);
        let long = format!("@@ -1,0 +1,12 @@\n{}", (1..=12).map(|i| format!("+l{i}")).collect::<Vec<_>>().join("\n"));
        assert_eq!(hits(&diff_window(&long, "additions", 2, 12).unwrap()), [2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn a_snippet_runs_4_lines_above_to_3_below_the_commented_line() {
        let lines = snippet_of(&file(), 20, 20, None);
        assert_eq!((lines[0].n, lines.last().unwrap().n), (16, 23));
        assert_eq!(hits(&lines), [20]);
        assert_eq!(lines[4].text, "line 20");
    }

    #[test]
    fn a_snippet_marks_the_whole_commented_range_and_stops_at_the_files_edges() {
        assert_eq!(hits(&snippet_of(&file(), 2, 4, None)), [2, 3, 4]);
        assert_eq!(snippet_of(&file(), 2, 4, None)[0].n, 1);
        assert_eq!(snippet_of(&file(), 49, 50, None).last().unwrap().n, 50);
        assert_eq!(snippet_of(&file(), 51, 51, None).len(), 0);
        assert_eq!(snippet_of(&file(), 0, 0, None).len(), 0);
    }

    #[test]
    fn a_snippet_from_line_1_keeps_the_first_lines_indentation() {
        assert_eq!(snippet_of("    indented\nnext\n", 1, 1, None)[0].text, "    indented");
    }

    #[test]
    fn the_room_a_snippet_needs_is_known_before_the_file_is_read() {
        assert_eq!(snippet_span(20, 20), Span { from: 16, to: 23, last: 20 });
        assert_eq!(snippet_span(2, 4), Span { from: 1, to: 7, last: 4 });
        let long = snippet_span(10, 40);
        assert_eq!(long.to - long.from + 1, 4 + 6 + 3);
        assert_eq!(snippet_of(&file(), 20, 20, None).len() as i64, snippet_span(20, 20).to - snippet_span(20, 20).from + 1);
    }

    #[test]
    fn added_lines_come_from_the_hunk_headers_of_a_zero_context_diff() {
        let diff = "diff --git a/f b/f\n@@ -3,2 +3,0 @@ x\n@@ -9 +7 @@\n@@ -12,0 +12,3 @@\n";
        let added = added_lines_of(diff);
        assert_eq!(added.iter().copied().collect::<Vec<_>>(), [7, 12, 13, 14]);
        let lines = snippet_of(&file(), 12, 12, Some(&added));
        assert_eq!(lines.iter().filter(|l| l.added == Some(true)).map(|l| l.n).collect::<Vec<_>>(), [12, 13, 14]);
        assert_eq!(snippet_of(&file(), 12, 12, None)[0].added, None);
    }

    #[test]
    fn a_long_commented_range_shows_its_first_lines() {
        assert_eq!(hits(&snippet_of(&file(), 10, 40, None)).len(), 6);
    }
}
