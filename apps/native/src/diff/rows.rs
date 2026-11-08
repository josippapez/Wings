//! What the diff viewer lists and draws, as the desktop's `diff-viewer.tsx` lays it out: the file list, flat or as a
//! folder tree, and the code rows, in one column or side by side.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::patch::{DiffFile, DiffHunk, DiffLineKind};

/// The path a file is listed under: its new one, else the one it had.
pub fn file_path(file: &DiffFile) -> &str {
    file.new_path
        .as_deref()
        .or(file.old_path.as_deref())
        .unwrap_or_default()
}

/// A row of the file list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListRow {
    /// A folder in the tree, by its full path. `label` joins a folder holding only one folder with it (`src/lib`).
    Folder {
        path: String,
        label: String,
        depth: usize,
        open: bool,
    },
    /// A file, by its index in the patch. `depth` is its indent in the tree; the flat list has none and names the folder.
    File { index: usize, depth: Option<usize> },
}

/// The deepest folder the tree shows. A patch is plugin input, and a path of thousands of `/` parts would otherwise
/// nest the tree, and the recursion that builds, lists and drops it, that deep; deeper parts join the file's name.
const MAX_DEPTH: usize = 256;

#[derive(Default)]
struct Folder<'a> {
    folders: BTreeMap<&'a str, Folder<'a>>,
    files: Vec<(usize, String)>,
}

/// The files whose path holds `filter`, ignoring case, as a flat list in patch order or as a tree: folders first, then
/// files, each by name, with collapsed folders' contents left out.
pub fn list_rows(
    files: &[DiffFile],
    filter: &str,
    tree: bool,
    collapsed: &HashSet<String>,
) -> Vec<ListRow> {
    let filter = filter.trim().to_lowercase();
    let shown = files
        .iter()
        .enumerate()
        .map(|(index, file)| (index, file_path(file)))
        .filter(|(_, path)| path.to_lowercase().contains(&filter));
    if !tree {
        return shown
            .map(|(index, _)| ListRow::File { index, depth: None })
            .collect();
    }
    let mut root = Folder::default();
    for (index, path) in shown {
        let mut node = &mut root;
        let mut parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
        let name = parts
            .split_off(parts.len().saturating_sub(1).min(MAX_DEPTH))
            .join("/");
        for part in parts {
            node = node.folders.entry(part).or_default();
        }
        node.files.push((index, name));
    }
    let mut rows = Vec::new();
    push_rows(&root, "", 0, collapsed, &mut rows);
    rows
}

fn push_rows(
    node: &Folder,
    path: &str,
    depth: usize,
    collapsed: &HashSet<String>,
    rows: &mut Vec<ListRow>,
) {
    let mut folders: Vec<_> = node.folders.iter().collect();
    folders.sort_by_key(|(name, _)| name.to_lowercase());
    for (name, top) in folders {
        let mut folder = top;
        let mut label = name.to_string();
        let mut full = join(path, name);
        while folder.files.is_empty() && folder.folders.len() == 1 {
            let (name, only) = folder.folders.iter().next().unwrap();
            label = format!("{label}/{name}");
            full = join(&full, name);
            folder = only;
        }
        let open = !collapsed.contains(&full);
        rows.push(ListRow::Folder {
            path: full.clone(),
            label,
            depth,
            open,
        });
        if open {
            push_rows(folder, &full, depth + 1, collapsed, rows);
        }
    }
    let mut files = node.files.clone();
    files.sort_by_key(|(_, name)| name.to_lowercase());
    rows.extend(files.into_iter().map(|(index, _)| ListRow::File {
        index,
        depth: Some(depth),
    }));
}

fn join(path: &str, name: &str) -> String {
    if path.is_empty() {
        name.to_owned()
    } else {
        format!("{path}/{name}")
    }
}

/// A row of code: a hunk's header, or a line by its hunk and index. Side by side, a row holds the old line on the left
/// and the new one on the right; an unchanged line is on both. Under a line, the review threads on it (`Notes`, by its
/// place in the list `with_notes` makes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeRow {
    Hunk(usize),
    Line(usize, usize),
    Split {
        hunk: usize,
        left: Option<usize>,
        right: Option<usize>,
    },
    Notes(usize),
}

/// The threads under a line, on its old number and on its new one, by their places in the viewer's threads. In one
/// column both sit under the line; side by side, each under its own side.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Notes {
    pub old: Vec<usize>,
    pub new: Vec<usize>,
}

/// `rows` with a `CodeRow::Notes` after each line that has threads in `on_lines` (by side, true for the old one, and
/// line number), and what each of those holds. As pierre/diffs' `getAnnotations`: a line shows the threads on its old
/// number and those on its new one. A thread on a line the diff doesn't show isn't placed.
pub fn with_notes(
    rows: Vec<CodeRow>,
    hunks: &[DiffHunk],
    on_lines: &HashMap<(bool, usize), Vec<usize>>,
) -> (Vec<CodeRow>, Vec<Notes>) {
    if on_lines.is_empty() {
        return (rows, Vec::new());
    }
    let mut placed = Vec::with_capacity(rows.len());
    let mut notes = Vec::new();
    let threads = |old: bool, line: Option<usize>| {
        line.and_then(|line| on_lines.get(&(old, line)))
            .cloned()
            .unwrap_or_default()
    };
    for row in rows {
        placed.push(row);
        let (old, new) = match row {
            CodeRow::Hunk(_) | CodeRow::Notes(_) => continue,
            CodeRow::Line(h, l) => {
                let line = &hunks[h].lines[l];
                (line.old_line, line.new_line)
            }
            CodeRow::Split { hunk, left, right } => {
                let lines = &hunks[hunk].lines;
                (
                    left.and_then(|l| lines[l].old_line),
                    right.and_then(|l| lines[l].new_line),
                )
            }
        };
        let here = Notes {
            old: threads(true, old),
            new: threads(false, new),
        };
        if !here.old.is_empty() || !here.new.is_empty() {
            placed.push(CodeRow::Notes(notes.len()));
            notes.push(here);
        }
    }
    (placed, notes)
}

/// The code rows of one file. Side by side, each run of removed lines pairs with the added lines after it, row by row.
pub fn code_rows(hunks: &[DiffHunk], split: bool) -> Vec<CodeRow> {
    let mut rows = Vec::new();
    for (h, hunk) in hunks.iter().enumerate() {
        rows.push(CodeRow::Hunk(h));
        if !split {
            rows.extend((0..hunk.lines.len()).map(|l| CodeRow::Line(h, l)));
            continue;
        }
        let (mut removed, mut added) = (Vec::new(), Vec::new());
        let flush = |rows: &mut Vec<CodeRow>, removed: &mut Vec<usize>, added: &mut Vec<usize>| {
            for i in 0..removed.len().max(added.len()) {
                rows.push(CodeRow::Split {
                    hunk: h,
                    left: removed.get(i).copied(),
                    right: added.get(i).copied(),
                });
            }
            removed.clear();
            added.clear();
        };
        for (l, line) in hunk.lines.iter().enumerate() {
            match line.kind {
                DiffLineKind::Deleted => {
                    if !added.is_empty() {
                        flush(&mut rows, &mut removed, &mut added);
                    }
                    removed.push(l);
                }
                DiffLineKind::Added => added.push(l),
                DiffLineKind::Context => {
                    flush(&mut rows, &mut removed, &mut added);
                    rows.push(CodeRow::Split {
                        hunk: h,
                        left: Some(l),
                        right: Some(l),
                    });
                }
            }
        }
        flush(&mut rows, &mut removed, &mut added);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::patch::parse_patch;

    fn files(paths: &[&str]) -> Vec<DiffFile> {
        let patch: String = paths
            .iter()
            .map(|p| format!("diff --git a/{p} b/{p}\n--- a/{p}\n+++ b/{p}\n@@ -1 +1 @@\n-a\n+b\n"))
            .collect();
        parse_patch(&patch).unwrap().files
    }

    fn names(files: &[DiffFile], rows: &[ListRow]) -> Vec<String> {
        rows.iter()
            .map(|row| match row {
                ListRow::Folder {
                    label, depth, open, ..
                } => format!(
                    "{}{label}/{}",
                    "  ".repeat(*depth),
                    if *open { "" } else { "+" }
                ),
                ListRow::File { index, depth } => {
                    let path = file_path(&files[*index]);
                    match depth {
                        Some(depth) => format!(
                            "{}{}",
                            "  ".repeat(*depth),
                            path.rsplit('/').next().unwrap()
                        ),
                        None => path.to_owned(),
                    }
                }
            })
            .collect()
    }

    #[test]
    fn the_filter_keeps_files_whose_path_holds_it_ignoring_case() {
        let files = files(&["src/App.tsx", "src/lib/api.ts", "README.md"]);
        let rows = list_rows(&files, " app ", false, &HashSet::new());
        assert_eq!(names(&files, &rows), ["src/App.tsx"]);
        let rows = list_rows(&files, "", false, &HashSet::new());
        assert_eq!(
            names(&files, &rows),
            ["src/App.tsx", "src/lib/api.ts", "README.md"]
        );
    }

    #[test]
    fn the_tree_lists_folders_first_joins_lone_folders_and_hides_collapsed_ones() {
        let files = files(&[
            "README.md",
            "apps/native/src/diff/viewer.rs",
            "apps/native/src/diff/rows.rs",
            "apps/native/Cargo.toml",
            "docs/a.md",
        ]);
        let rows = list_rows(&files, "", true, &HashSet::new());
        assert_eq!(
            names(&files, &rows),
            [
                "apps/native/",
                "  src/diff/",
                "    rows.rs",
                "    viewer.rs",
                "  Cargo.toml",
                "docs/",
                "  a.md",
                "README.md",
            ]
        );
        let collapsed = HashSet::from(["apps/native/src/diff".to_owned()]);
        let rows = list_rows(&files, "", true, &collapsed);
        assert_eq!(
            names(&files, &rows)[..3],
            ["apps/native/", "  src/diff/+", "  Cargo.toml"]
        );
        // The filter applies before the tree is made.
        let rows = list_rows(&files, "rows", true, &HashSet::new());
        assert_eq!(names(&files, &rows), ["apps/native/src/diff/", "  rows.rs"]);
    }

    #[test]
    fn side_by_side_pairs_removed_lines_with_the_added_ones_after_them() {
        let patch = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,5 +1,4 @@\n same\n-old 1\n-old 2\n+new 1\n same 2\n+only new\n-only old\n";
        let diff = parse_patch(patch).unwrap();
        let hunks = &diff.files[0].hunks;
        let split = |left, right| CodeRow::Split {
            hunk: 0,
            left,
            right,
        };
        assert_eq!(
            code_rows(hunks, true),
            [
                CodeRow::Hunk(0),
                split(Some(0), Some(0)),
                split(Some(1), Some(3)),
                split(Some(2), None),
                split(Some(4), Some(4)),
                split(None, Some(5)),
                split(Some(6), None),
            ]
        );
        assert_eq!(code_rows(hunks, false).len(), 1 + hunks[0].lines.len());
    }

    #[test]
    fn threads_sit_under_their_line_on_its_side() {
        // Line 2 is unchanged, old 3 is removed and new 3 is added.
        let patch =
            "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,3 +1,3 @@\n same\n same 2\n-old\n+new\n";
        let diff = parse_patch(patch).unwrap();
        let hunks = &diff.files[0].hunks;
        // Thread 0 on the unchanged line's old number, 1 and 2 on its new one, 3 on the removed line, 4 on the added
        // one, and 5 on a line the diff doesn't show.
        let on_lines = HashMap::from([
            ((true, 2), vec![0]),
            ((false, 2), vec![1, 2]),
            ((true, 3), vec![3]),
            ((false, 3), vec![4]),
            ((false, 40), vec![5]),
        ]);
        let notes = |old: &[usize], new: &[usize]| Notes {
            old: old.to_vec(),
            new: new.to_vec(),
        };
        let (rows, placed) = with_notes(code_rows(hunks, false), hunks, &on_lines);
        assert_eq!(
            rows,
            [
                CodeRow::Hunk(0),
                CodeRow::Line(0, 0),
                CodeRow::Line(0, 1),
                CodeRow::Notes(0),
                CodeRow::Line(0, 2),
                CodeRow::Notes(1),
                CodeRow::Line(0, 3),
                CodeRow::Notes(2),
            ]
        );
        assert_eq!(
            placed,
            [notes(&[0], &[1, 2]), notes(&[3], &[]), notes(&[], &[4])]
        );
        // Side by side the removed and the added line share a row, and so do their threads.
        let (rows, placed) = with_notes(code_rows(hunks, true), hunks, &on_lines);
        assert_eq!(rows.len(), 6);
        assert_eq!(rows[5], CodeRow::Notes(1));
        assert_eq!(placed, [notes(&[0], &[1, 2]), notes(&[3], &[4])]);
    }

    #[test]
    fn a_path_of_very_many_parts_lists_without_nesting_that_deep() {
        let deep = format!("{}file.rs", "d/".repeat(100_000));
        let files = files(&[deep.as_str(), "a//b.rs"]);
        let rows = list_rows(&files, "", true, &HashSet::new());
        // Empty parts make no nameless folder.
        assert_eq!(
            rows[..2],
            [
                ListRow::Folder {
                    path: "a".into(),
                    label: "a".into(),
                    depth: 0,
                    open: true
                },
                ListRow::File {
                    index: 1,
                    depth: Some(1)
                },
            ]
        );
        // The lone folders join into one row, MAX_DEPTH deep at most, and the rest joins the file's name.
        let ListRow::Folder { label, .. } = &rows[2] else {
            panic!("{:?}", rows[2]);
        };
        assert_eq!(label.split('/').count(), MAX_DEPTH);
        assert_eq!(
            rows[3..],
            [ListRow::File {
                index: 0,
                depth: Some(1)
            }]
        );
    }
}
