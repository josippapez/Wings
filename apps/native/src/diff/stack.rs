//! Every file's diff stacked in one scroll, as a sidebar's widened diff shows it: a header per file, its hunks' lines,
//! and the unchanged lines the patch leaves out before, between and after its hunks, each run folded into one row
//! that opens to show them once the file's text is read (`worktree`).

use std::collections::HashSet;

use super::{
    patch::{DiffFile, DiffHunk, DiffLine, DiffLineKind, DiffStatus},
    rows::file_path,
};

/// A run of unchanged lines the patch leaves out. `hunk` is the hunk it comes before, or the number of hunks for the run
/// after the last one. Lines count from 1 on each side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fold {
    pub hunk: usize,
    pub old_start: usize,
    pub new_start: usize,
    pub len: usize,
}

/// A row of the stack. Each names its file by its place in the patch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StackRow {
    /// The file's path, line counts and actions.
    Header(usize),
    /// Why a file has no lines to show: binary, too large, or only renamed.
    Note(usize),
    /// A closed fold, by its place in the file's folds.
    Fold { file: usize, fold: usize },
    /// A line of an open fold, by its place in the fold.
    Unchanged { file: usize, fold: usize, line: usize },
    /// A line of one of the file's hunks.
    Line { file: usize, hunk: usize, line: usize },
    /// The space after a file, before the next one's header.
    Gap(usize),
}

impl StackRow {
    pub fn file(self) -> usize {
        match self {
            Self::Header(file) | Self::Note(file) | Self::Gap(file) => file,
            Self::Fold { file, .. } | Self::Unchanged { file, .. } | Self::Line { file, .. } => file,
        }
    }
}

/// Lines added and removed in one file.
pub fn counts(file: &DiffFile) -> (usize, usize) {
    let lines = file.hunks.iter().flat_map(|hunk| &hunk.lines);
    lines.fold((0, 0), |(added, removed), line| match line.kind {
        DiffLineKind::Added => (added + 1, removed),
        DiffLineKind::Deleted => (added, removed + 1),
        DiffLineKind::Context => (added, removed),
    })
}

/// Lines added and removed over every file.
pub fn totals(files: &[DiffFile]) -> (usize, usize) {
    files
        .iter()
        .map(counts)
        .fold((0, 0), |(a, r), (fa, fr)| (a + fa, r + fr))
}

/// Whether the file has an old side and a new one whose unchanged lines can fold: changed in place, or renamed or
/// copied with changes. An added or deleted file is all in its hunks.
pub fn foldable(file: &DiffFile) -> bool {
    matches!(
        file.status,
        DiffStatus::Modified | DiffStatus::Renamed | DiffStatus::Copied
    ) && !file.is_binary
        && !file.is_placeholder
        && !file.hunks.is_empty()
}

/// Whether the file shows a note instead of lines.
pub fn has_no_lines(file: &DiffFile) -> bool {
    file.hunks.is_empty() || file.is_binary || file.is_placeholder
}

/// The first line a hunk covers on one side and the line after its last. A hunk with no line on that side sits right
/// after its range's start, as `@@ -5,2 +4,0 @@` removes lines after new line 4.
fn span(hunk: &DiffHunk, old: bool) -> (usize, usize) {
    let number = |line: &DiffLine| if old { line.old_line } else { line.new_line };
    let first = hunk.lines.iter().find_map(number);
    let last = hunk.lines.iter().rev().find_map(number);
    match (first, last) {
        (Some(first), Some(last)) => (first, last + 1),
        _ => {
            let at = if old {
                hunk.old_range.start
            } else {
                hunk.new_range.start
            } + 1;
            (at, at)
        }
    }
}

/// The file's folds: before its first hunk, between each two, and after its last when `lines`, how many lines its new
/// side has, is known. A fold's length comes from the plugin's hunk headers, so once the file's length is known no fold
/// goes past it.
pub fn folds(file: &DiffFile, lines: Option<usize>) -> Vec<Fold> {
    if !foldable(file) {
        return Vec::new();
    }
    let room = |new_at: usize| lines.map_or(usize::MAX, |lines| (lines + 1).saturating_sub(new_at));
    let mut folds = Vec::new();
    let (mut old_at, mut new_at) = (1, 1);
    for (h, hunk) in file.hunks.iter().enumerate() {
        let (_, old_after) = span(hunk, true);
        let (new_first, new_after) = span(hunk, false);
        let len = new_first.saturating_sub(new_at).min(room(new_at));
        if len > 0 {
            folds.push(Fold {
                hunk: h,
                old_start: old_at,
                new_start: new_at,
                len,
            });
        }
        old_at = old_at.max(old_after);
        new_at = new_at.max(new_after);
    }
    if let Some(lines) = lines
        && lines + 1 > new_at
    {
        folds.push(Fold {
            hunk: file.hunks.len(),
            old_start: old_at,
            new_start: new_at,
            len: lines + 1 - new_at,
        });
    }
    folds
}

/// The rows of a fold's lines, open.
pub fn fold_lines(file: usize, fold: usize, len: usize) -> impl Iterator<Item = StackRow> {
    (0..len).map(move |line| StackRow::Unchanged { file, fold, line })
}

/// The stack's rows: each file's header, then its folds and hunk lines in order, with a fold in `open` (by file and its
/// place in the file's folds) as its lines, and a gap before the next file. A file with no lines to show gets a note.
pub fn rows(
    files: &[DiffFile],
    folds: &[Vec<Fold>],
    open: &HashSet<(usize, usize)>,
) -> Vec<StackRow> {
    let mut rows = Vec::new();
    for (f, file) in files.iter().enumerate() {
        if f > 0 {
            rows.push(StackRow::Gap(f - 1));
        }
        rows.push(StackRow::Header(f));
        if has_no_lines(file) {
            rows.push(StackRow::Note(f));
            continue;
        }
        let file_folds = folds.get(f).map_or(&[][..], Vec::as_slice);
        let push_folds = |rows: &mut Vec<StackRow>, before: usize| {
            for (k, fold) in file_folds.iter().enumerate() {
                if fold.hunk != before {
                    continue;
                }
                if open.contains(&(f, k)) {
                    rows.extend(fold_lines(f, k, fold.len));
                } else {
                    rows.push(StackRow::Fold { file: f, fold: k });
                }
            }
        };
        for (h, hunk) in file.hunks.iter().enumerate() {
            push_folds(&mut rows, h);
            rows.extend((0..hunk.lines.len()).map(|line| StackRow::Line {
                file: f,
                hunk: h,
                line,
            }));
        }
        push_folds(&mut rows, file.hunks.len());
    }
    rows
}

/// Where each file's header is in `rows`.
pub fn headers(rows: &[StackRow], files: usize) -> Vec<usize> {
    let mut at = vec![0; files];
    for (index, row) in rows.iter().enumerate() {
        if let StackRow::Header(file) = row
            && let Some(slot) = at.get_mut(*file)
        {
            *slot = index;
        }
    }
    at
}

/// A row named by what it shows rather than where it is, so it can be found again in the rows of the next patch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowKey {
    Header(String),
    Note(String),
    Gap(String),
    /// A fold by its first old line.
    Fold(String, usize),
    /// A line by its old number, or by its new one when it was added and has no old one (`false`).
    Line(String, bool, usize),
}

impl RowKey {
    fn path(&self) -> &str {
        match self {
            Self::Header(path)
            | Self::Note(path)
            | Self::Gap(path)
            | Self::Fold(path, _)
            | Self::Line(path, ..) => path,
        }
    }
}

fn fold_at(folds: &[Vec<Fold>], file: usize, fold: usize) -> Option<Fold> {
    folds.get(file)?.get(fold).copied()
}

pub fn key(row: StackRow, files: &[DiffFile], folds: &[Vec<Fold>]) -> RowKey {
    let path = |f: usize| files.get(f).map_or_else(String::new, |file| file_path(file).to_owned());
    match row {
        StackRow::Header(f) => RowKey::Header(path(f)),
        StackRow::Note(f) => RowKey::Note(path(f)),
        StackRow::Gap(f) => RowKey::Gap(path(f)),
        StackRow::Fold { file, fold } => RowKey::Fold(
            path(file),
            fold_at(folds, file, fold).map_or(0, |fold| fold.old_start),
        ),
        StackRow::Unchanged { file, fold, line } => RowKey::Line(
            path(file),
            true,
            fold_at(folds, file, fold).map_or(0, |fold| fold.old_start + line),
        ),
        StackRow::Line { file, hunk, line } => {
            let found = files
                .get(file)
                .and_then(|f| f.hunks.get(hunk))
                .and_then(|h| h.lines.get(line));
            match found {
                Some(DiffLine {
                    old_line: Some(old),
                    ..
                }) => RowKey::Line(path(file), true, *old),
                Some(line) => RowKey::Line(path(file), false, line.new_line.unwrap_or(0)),
                None => RowKey::Header(path(file)),
            }
        }
    }
}

/// Where the row `key` names is in `rows`, and whether it is that row: else it is its file's header, when the file is
/// still in the patch.
pub fn find(
    key: &RowKey,
    rows: &[StackRow],
    files: &[DiffFile],
    folds: &[Vec<Fold>],
) -> Option<(usize, bool)> {
    let file = files.iter().position(|file| file_path(file) == key.path())?;
    let start = rows.iter().position(|row| *row == StackRow::Header(file))?;
    let end = rows[start + 1..]
        .iter()
        .position(|row| matches!(row, StackRow::Header(_)))
        .map_or(rows.len(), |after| start + 1 + after);
    let exact = (start..end).find(|&at| self::key(rows[at], files, folds) == *key);
    Some(exact.map_or((start, false), |at| (at, true)))
}

/// The row that shows line `number` of `file`, on its old side or its new one, in a hunk or an open fold.
pub fn line_row(
    rows: &[StackRow],
    files: &[DiffFile],
    folds: &[Vec<Fold>],
    file: usize,
    number: usize,
    old: bool,
) -> Option<usize> {
    rows.iter().position(|row| match *row {
        StackRow::Line {
            file: f,
            hunk,
            line,
        } if f == file => files[f].hunks.get(hunk).and_then(|h| h.lines.get(line)).is_some_and(
            |line| {
                (if old { line.old_line } else { line.new_line }) == Some(number)
            },
        ),
        StackRow::Unchanged { file: f, fold, line } if f == file => fold_at(folds, f, fold)
            .is_some_and(|fold| {
                (if old { fold.old_start } else { fold.new_start }) + line == number
            }),
        _ => false,
    })
}

/// The first line of the file's new side that changed, else its first hunk's, to open the file at.
pub fn first_change(file: &DiffFile) -> Option<usize> {
    let lines = || file.hunks.iter().flat_map(|hunk| &hunk.lines);
    lines()
        .find(|line| line.kind == DiffLineKind::Added)
        .and_then(|line| line.new_line)
        .or_else(|| lines().find_map(|line| line.new_line))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::patch::parse_patch;

    /// `a.rs` changed in two places with twelve unchanged lines between them, `b.rs` added, and `c.png` binary.
    const PATCH: &str = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -5,3 +5,4 @@\n five\n+new six\n six\n seven\n@@ -20,3 +21,2 @@\n twenty\n-twenty-one\n twenty-two\ndiff --git a/b.rs b/b.rs\nnew file mode 100644\n--- /dev/null\n+++ b/b.rs\n@@ -0,0 +1,2 @@\n+one\n+two\ndiff --git a/c.png b/c.png\nBinary files a/c.png and b/c.png differ\n";

    fn fold(hunk: usize, old_start: usize, new_start: usize, len: usize) -> Fold {
        Fold {
            hunk,
            old_start,
            new_start,
            len,
        }
    }

    #[test]
    fn folds_sit_before_between_and_after_the_hunks() {
        let diff = parse_patch(PATCH).unwrap();
        let a = &diff.files[0];
        // Before the first hunk, new lines 1 to 4; between, new 9 to 20, old 8 to 19; after the last only once the
        // file's length is known.
        assert_eq!(folds(a, None), [fold(0, 1, 1, 4), fold(1, 8, 9, 12)]);
        assert_eq!(folds(a, Some(30))[2], fold(2, 23, 23, 8));
        // A file as long as its last hunk has nothing after it.
        assert_eq!(folds(a, Some(22)).len(), 2);
        // An added file is all in its hunk, and a binary one has none.
        assert!(folds(&diff.files[1], Some(2)).is_empty());
        assert!(folds(&diff.files[2], Some(9)).is_empty());
    }

    #[test]
    fn each_file_gets_a_header_its_folds_and_lines_and_a_gap_before_the_next() {
        let diff = parse_patch(PATCH).unwrap();
        let folds: Vec<_> = diff.files.iter().map(|f| folds(f, Some(30))).collect();
        let rows = rows(&diff.files, &folds, &HashSet::new());
        let line = |hunk, line| StackRow::Line { file: 0, hunk, line };
        let closed = |fold| StackRow::Fold { file: 0, fold };
        assert_eq!(
            rows,
            [
                StackRow::Header(0),
                closed(0),
                line(0, 0),
                line(0, 1),
                line(0, 2),
                line(0, 3),
                closed(1),
                line(1, 0),
                line(1, 1),
                line(1, 2),
                closed(2),
                StackRow::Gap(0),
                StackRow::Header(1),
                StackRow::Line { file: 1, hunk: 0, line: 0 },
                StackRow::Line { file: 1, hunk: 0, line: 1 },
                StackRow::Gap(1),
                StackRow::Header(2),
                StackRow::Note(2),
            ]
        );
        assert_eq!(headers(&rows, 3), [0, 12, 16]);
        assert_eq!(totals(&diff.files), (3, 1));
    }

    #[test]
    fn an_open_fold_is_its_lines_in_place_of_its_row() {
        let diff = parse_patch(PATCH).unwrap();
        let folds: Vec<_> = diff.files.iter().map(|f| folds(f, None)).collect();
        let rows = rows(&diff.files, &folds, &HashSet::from([(0, 1)]));
        let between: Vec<_> = rows[6..18].to_vec();
        assert_eq!(between, fold_lines(0, 1, 12).collect::<Vec<_>>());
        assert_eq!(rows[18], StackRow::Line { file: 0, hunk: 1, line: 0 });
        // Its fifth line is new line 13, old line 12.
        assert_eq!(line_row(&rows, &diff.files, &folds, 0, 13, false), Some(10));
        assert_eq!(line_row(&rows, &diff.files, &folds, 0, 12, true), Some(10));
        assert_eq!(first_change(&diff.files[0]), Some(6));
    }

    /// A hunk header is the plugin's: one that says its hunk sits millions of lines down makes no fold longer than the
    /// file is, once its length is known.
    #[test]
    fn a_fold_never_runs_past_the_end_of_the_file() {
        let patch = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,1 +4000000,0 @@\n-x\n";
        let diff = parse_patch(patch).unwrap();
        let file = &diff.files[0];
        assert_eq!(folds(file, None)[0].len, 4_000_000);
        assert_eq!(folds(file, Some(30)), [fold(0, 1, 1, 30)]);
    }

    /// A reload finds the row that was at the top by what it shows: the same line once a file above it grew, else its
    /// file's header.
    #[test]
    fn a_row_is_found_again_in_the_next_patch() {
        let diff = parse_patch(PATCH).unwrap();
        let folds: Vec<_> = diff.files.iter().map(|f| folds(f, None)).collect();
        let rows = rows(&diff.files, &folds, &HashSet::new());
        // b.rs's second line, and a.rs's removed line.
        let second = key(rows[13], &diff.files, &folds);
        assert_eq!(second, RowKey::Line("b.rs".into(), false, 2));
        assert_eq!(
            key(rows[8], &diff.files, &folds),
            RowKey::Line("a.rs".into(), true, 21)
        );
        // a.rs loses its first hunk, so b.rs moves up: its line is found where it went.
        let next = PATCH.replace(
            "@@ -5,3 +5,4 @@\n five\n+new six\n six\n seven\n@@ -20,3 +21,2 @@",
            "@@ -20,3 +20,2 @@",
        );
        let next = parse_patch(&next).unwrap();
        let next_folds: Vec<_> = next.files.iter().map(|f| super::folds(f, None)).collect();
        let next_rows = super::rows(&next.files, &next_folds, &HashSet::new());
        assert_eq!(
            find(&second, &next_rows, &next.files, &next_folds),
            Some((8, true))
        );
        // A line that is gone leaves its file's header, and a file that is gone nothing.
        let gone = RowKey::Line("a.rs".into(), true, 5);
        assert_eq!(find(&gone, &next_rows, &next.files, &next_folds), Some((0, false)));
        let elsewhere = RowKey::Header("d.rs".into());
        assert_eq!(find(&elsewhere, &next_rows, &next.files, &next_folds), None);
    }
}
