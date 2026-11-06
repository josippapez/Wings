//! Parsed unified Git patches for the native changes viewer.

use std::error::Error;
use std::fmt;

use diffy::Line as DiffyLine;
use diffy::patch_set::{FileOperation, ParseOptions, PatchKind, PatchSet};

/// The desktop host accepts patches up to 8 MiB. Keep parsing within that same
/// transport boundary rather than imposing the Changes plugin's smaller view limit.
pub const MAX_PATCH_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diff {
    pub files: Vec<DiffFile>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffFile {
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub status: DiffStatus,
    pub is_binary: bool,
    pub is_placeholder: bool,
    pub hunks: Vec<DiffHunk>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffStatus {
    Added,
    Deleted,
    Modified,
    Renamed,
    Copied,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffHunk {
    pub old_range: LineRange,
    pub new_range: LineRange,
    pub function_context: Option<String>,
    pub lines: Vec<DiffLine>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineRange {
    pub start: usize,
    pub len: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub text: String,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
    pub missing_newline: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffLineKind {
    Context,
    Added,
    Deleted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    message: String,
}

impl ParseError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ParseError {}

/// Parses one Git unified patch into data owned by the native viewer.
pub fn parse_patch(patch: &str) -> Result<Diff, ParseError> {
    if patch.len() > MAX_PATCH_BYTES {
        return Err(ParseError::new(format!(
            "Patch is too large to show ({} MiB limit).",
            MAX_PATCH_BYTES / 1024 / 1024
        )));
    }

    let mut files = Vec::new();
    for file in PatchSet::parse(patch, ParseOptions::gitdiff()) {
        let file = file.map_err(|error| ParseError::new(format!("Invalid Git patch: {error}")))?;
        files.push(adapt_file(file)?);
    }

    Ok(Diff { files })
}

fn adapt_file(file: diffy::patch_set::FilePatch<'_, str>) -> Result<DiffFile, ParseError> {
    let (old_path, new_path, status) = match file.operation() {
        FileOperation::Create(path) => (None, Some(strip_prefix(path, "b/")), DiffStatus::Added),
        FileOperation::Delete(path) => (Some(strip_prefix(path, "a/")), None, DiffStatus::Deleted),
        FileOperation::Modify { original, modified } => (
            Some(strip_prefix(original, "a/")),
            Some(strip_prefix(modified, "b/")),
            DiffStatus::Modified,
        ),
        FileOperation::Rename { from, to } => (
            Some(from.to_string()),
            Some(to.to_string()),
            DiffStatus::Renamed,
        ),
        FileOperation::Copy { from, to } => (
            Some(from.to_string()),
            Some(to.to_string()),
            DiffStatus::Copied,
        ),
    };
    let is_binary = file.patch().is_binary();
    let hunks = match file.patch() {
        PatchKind::Text(patch) => patch.hunks().iter().map(adapt_hunk).collect(),
        PatchKind::Binary(_) => Vec::new(),
    };
    let is_placeholder = hunks.len() == 1
        && hunks[0].lines.len() == 1
        && hunks[0].lines[0].kind == DiffLineKind::Context
        && hunks[0].lines[0].text.starts_with("Too large to show");

    if status == DiffStatus::Modified
        && !is_binary
        && hunks.is_empty()
        && file.old_mode().is_none()
        && file.new_mode().is_none()
    {
        let path = new_path
            .as_deref()
            .or(old_path.as_deref())
            .unwrap_or("unknown file");
        return Err(ParseError::new(format!(
            "Invalid Git patch: modification of '{path}' has no hunks, binary marker, or mode change."
        )));
    }

    Ok(DiffFile {
        old_path,
        new_path,
        status,
        is_binary,
        is_placeholder,
        hunks,
    })
}

fn strip_prefix(path: &str, prefix: &str) -> String {
    path.strip_prefix(prefix).unwrap_or(path).to_string()
}

fn adapt_hunk(hunk: &diffy::Hunk<'_, str>) -> DiffHunk {
    let old_range = hunk.old_range();
    let new_range = hunk.new_range();
    let mut old_line = old_range.start();
    let mut new_line = new_range.start();
    let lines = hunk
        .lines()
        .iter()
        .map(|line| match line {
            DiffyLine::Context(text) => {
                let line = DiffLine {
                    kind: DiffLineKind::Context,
                    text: trim_line_ending(text),
                    old_line: Some(old_line),
                    new_line: Some(new_line),
                    missing_newline: !text.ends_with('\n'),
                };
                old_line += 1;
                new_line += 1;
                line
            }
            DiffyLine::Delete(text) => {
                let line = DiffLine {
                    kind: DiffLineKind::Deleted,
                    text: trim_line_ending(text),
                    old_line: Some(old_line),
                    new_line: None,
                    missing_newline: !text.ends_with('\n'),
                };
                old_line += 1;
                line
            }
            DiffyLine::Insert(text) => {
                let line = DiffLine {
                    kind: DiffLineKind::Added,
                    text: trim_line_ending(text),
                    old_line: None,
                    new_line: Some(new_line),
                    missing_newline: !text.ends_with('\n'),
                };
                new_line += 1;
                line
            }
        })
        .collect();

    DiffHunk {
        old_range: LineRange {
            start: old_range.start(),
            len: old_range.len(),
        },
        new_range: LineRange {
            start: new_range.start(),
            len: new_range.len(),
        },
        function_context: hunk.function_context().map(trim_line_ending),
        lines,
    }
}

fn trim_line_ending(line: &str) -> String {
    line.strip_suffix('\n').unwrap_or(line).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tracked_modification_with_line_numbers() {
        let diff = parse_patch(
            "diff --git a/src/lib.rs b/src/lib.rs\nindex 1111111..2222222 100644\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -2,2 +2,2 @@ fn main()\n old\n-old\n+new\n",
        )
        .unwrap();

        assert_eq!(diff.files.len(), 1);
        let file = &diff.files[0];
        assert_eq!(file.status, DiffStatus::Modified);
        assert_eq!(file.old_path.as_deref(), Some("src/lib.rs"));
        assert_eq!(file.hunks[0].function_context.as_deref(), Some("fn main()"));
        assert_eq!(file.hunks[0].lines[1].old_line, Some(3));
        assert_eq!(file.hunks[0].lines[2].new_line, Some(3));
    }

    #[test]
    fn parses_added_and_deleted_files() {
        let diff = parse_patch(
            "diff --git a/new.txt b/new.txt\nnew file mode 100644\n--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1 @@\n+new\ndiff --git a/old.txt b/old.txt\ndeleted file mode 100644\n--- a/old.txt\n+++ /dev/null\n@@ -1 +0,0 @@\n-old\n",
        )
        .unwrap();

        assert_eq!(diff.files[0].status, DiffStatus::Added);
        assert_eq!(diff.files[0].new_path.as_deref(), Some("new.txt"));
        assert_eq!(diff.files[1].status, DiffStatus::Deleted);
        assert_eq!(diff.files[1].old_path.as_deref(), Some("old.txt"));
    }

    #[test]
    fn parses_renames_with_spaces_and_c_quoted_unicode_paths() {
        let diff = parse_patch(
            "diff --git \"a/old name \\303\\244.txt\" \"b/new name \\303\\244.txt\"\nsimilarity index 100%\nrename from old name ä.txt\nrename to new name ä.txt\n",
        )
        .unwrap();

        let file = &diff.files[0];
        assert_eq!(file.status, DiffStatus::Renamed);
        assert_eq!(file.old_path.as_deref(), Some("old name ä.txt"));
        assert_eq!(file.new_path.as_deref(), Some("new name ä.txt"));
        assert!(file.hunks.is_empty());
    }

    #[test]
    fn parses_binary_markers_without_hunks() {
        let diff = parse_patch(
            "diff --git a/image.png b/image.png\nnew file mode 100644\nindex 0000000..1111111\nBinary files /dev/null and b/image.png differ\n",
        )
        .unwrap();

        assert!(diff.files[0].is_binary);
        assert!(diff.files[0].hunks.is_empty());
    }

    #[test]
    fn records_missing_newlines() {
        let diff = parse_patch(
            "diff --git a/file.txt b/file.txt\n--- a/file.txt\n+++ b/file.txt\n@@ -1 +1 @@\n-old\n\\ No newline at end of file\n+new\n\\ No newline at end of file\n",
        )
        .unwrap();

        assert!(
            diff.files[0].hunks[0]
                .lines
                .iter()
                .all(|line| line.missing_newline)
        );
    }

    #[test]
    fn parses_file_only_empty_and_untracked_synthesized_patches() {
        let empty =
            parse_patch("diff --git a/empty.txt b/empty.txt\nnew file mode 100644\n").unwrap();
        assert_eq!(empty.files[0].status, DiffStatus::Added);
        assert!(empty.files[0].hunks.is_empty());

        let untracked = parse_patch(
            "diff --git a/new file.txt b/new file.txt\nnew file mode 100644\n--- /dev/null\n+++ b/new file.txt\n@@ -0,0 +1,2 @@\n+one\n+two\n",
        )
        .unwrap();
        assert_eq!(untracked.files[0].status, DiffStatus::Added);
        assert_eq!(untracked.files[0].new_path.as_deref(), Some("new file.txt"));
    }

    #[test]
    fn recognizes_batch_placeholders() {
        let diff = parse_patch(
            "diff --git a/big.js b/big.js\n--- a/big.js\n+++ b/big.js\n@@ -1 +1 @@\n Too large to show (8.0 MB)\n",
        )
        .unwrap();

        assert!(diff.files[0].is_placeholder);
    }

    #[test]
    fn enforces_the_host_patch_size_boundary() {
        let fixture = "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-old\n+new\n";
        let preamble = "x\n".repeat((MAX_PATCH_BYTES - fixture.len()) / 2);
        assert!(parse_patch(&format!("{preamble}{fixture}")).is_ok());
        assert!(parse_patch(&"x".repeat(MAX_PATCH_BYTES + 1)).is_err());
    }

    #[test]
    fn rejects_truncated_hunks() {
        let error =
            parse_patch("diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-old\n").unwrap_err();
        assert!(error.to_string().starts_with("Invalid Git patch:"));
    }

    #[test]
    fn rejects_malformed_file_entries_that_have_no_change_data() {
        let error = parse_patch("diff --git a/a b/a\nnot a Git diff header\n").unwrap_err();
        assert!(error.to_string().contains("modification of 'a'"));
    }
}
