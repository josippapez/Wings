//! What each turn changed in files, for the card at the turn's end: the Edit, Write and NotebookEdit calls' own results,
//! not `git diff`, which a later turn could change for the same files. Claude Code prints a result as `tool_use_result` on
//! the stream and as `toolUseResult` in the transcript, in the same shape. Also Undo, which puts the files back as they were
//! before the turn's message (`rewind_files`, after a dialog), and the patch View changes shows.

use std::collections::HashSet;

use serde_json::{Value, json};

use crate::session::{Item, Prompt, Session, request};

/// The request Undo makes, apart from the rewind dialog's, so each one's answer is told apart.
pub const UNDO_FILES: &str = "undo-files";
/// The most lines of one call's patch kept. A written file can be huge, and a long session keeps every call.
const PATCH_LINES: usize = 5000;

/// One file one call changed.
#[derive(Debug, PartialEq)]
pub struct Edited {
    /// Where the conversation stood (its item count) when the result came: which turn it belongs to.
    at: usize,
    /// As the tool named it, a full path.
    pub path: String,
    pub added: usize,
    pub removed: usize,
    /// The call's hunks, each against the file as the call found it.
    hunks: Vec<Hunk>,
    created: bool,
}

/// One hunk of a call: where it starts in the file the call found, as the index of its first line there, and its lines,
/// each with its ` `, `-` or `+`.
#[derive(Debug, PartialEq)]
struct Hunk {
    at: usize,
    lines: Vec<String>,
}

/// A file a turn changed, with the lines it added and took out over all the calls that touched it.
#[derive(Debug, PartialEq)]
pub struct Changed {
    pub path: String,
    pub added: usize,
    pub removed: usize,
}

/// What a turn changed.
#[derive(Debug, Default, PartialEq)]
pub struct Turn {
    pub files: Vec<Changed>,
    pub added: usize,
    pub removed: usize,
}

/// The Undo dialog, while it is open.
#[derive(Debug, PartialEq)]
pub struct Undo {
    /// The message of yours the turn began with.
    pub uuid: String,
    pub files: Vec<String>,
    /// Files later turns changed. Going back to before this turn takes their changes too.
    pub later: Vec<String>,
    pub waiting: bool,
    pub error: Option<String>,
}

#[derive(Debug, Default)]
pub struct Edits {
    list: Vec<Edited>,
    /// Each file in `list` once, in the order they first came: drawing a reply looks words up in it.
    paths: Vec<String>,
    /// The messages whose turns' files were put back, by uuid.
    undone: HashSet<String>,
    pub undo: Option<Undo>,
}

impl Edits {
    /// Takes in a line of the stream or the transcript: a successful edit's result, at conversation position `at`.
    pub fn heard(&mut self, line: &Value, at: usize) {
        let result = if line["tool_use_result"].is_object() { &line["tool_use_result"] } else { &line["toolUseResult"] };
        let failed = line["message"]["content"].as_array().is_some_and(|blocks| blocks.iter().any(|block| block["is_error"] == true));
        if failed {
            return;
        }
        if let Some(edit) = parse(result, at) {
            if !self.paths.contains(&edit.path) {
                self.paths.push(edit.path.clone());
            }
            self.list.push(edit);
        }
    }

    /// Your message at conversation position `at` and all after it are gone.
    pub fn cut(&mut self, at: usize) {
        self.list.retain(|edit| edit.at <= at);
        let list = &self.list;
        self.paths.retain(|path| list.iter().any(|edit| edit.path == *path));
    }

    pub fn clear(&mut self) {
        *self = Edits::default();
    }

    /// The files changed by the calls whose results came after position `from` up to `to`: one turn.
    pub fn turn(&self, from: usize, to: usize) -> Turn {
        let mut turn = Turn::default();
        for edit in self.within(from, to) {
            turn.added += edit.added;
            turn.removed += edit.removed;
            match turn.files.iter_mut().find(|file| file.path == edit.path) {
                Some(file) => (file.added, file.removed) = (file.added + edit.added, file.removed + edit.removed),
                None => turn.files.push(Changed { path: edit.path.clone(), added: edit.added, removed: edit.removed }),
            }
        }
        turn
    }

    fn within(&self, from: usize, to: usize) -> impl Iterator<Item = &Edited> {
        self.list.iter().filter(move |edit| edit.at > from && edit.at <= to)
    }

    /// The full paths of every file the session changed.
    pub fn files(&self) -> &[String] {
        &self.paths
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// The turn's changes as a patch, each path as it is under `cwd`.
    pub fn patch(&self, from: usize, to: usize, cwd: &str) -> String {
        render(&self.within(from, to).collect::<Vec<_>>(), cwd)
    }

    /// Everything the session changed in one file, as a patch. None when it changed nothing there.
    pub fn file_patch(&self, path: &str, cwd: &str) -> Option<String> {
        let edits: Vec<&Edited> = self.list.iter().filter(|edit| edit.path == path).collect();
        (!edits.is_empty()).then(|| render(&edits, cwd))
    }

    pub fn is_undone(&self, uuid: &str) -> bool {
        self.undone.contains(uuid)
    }

    /// Opens the Undo dialog for the turn of `uuid`, between positions `from` and `to`. A turn that changed no files opens nothing.
    pub fn ask_undo(&mut self, uuid: &str, from: usize, to: usize) {
        let files = self.turn(from, to).files.into_iter().map(|file| file.path).collect::<Vec<_>>();
        let mut later: Vec<String> = Vec::new();
        for edit in self.list.iter().filter(|edit| edit.at > to) {
            if !later.contains(&edit.path) {
                later.push(edit.path.clone());
            }
        }
        if !files.is_empty() {
            self.undo = Some(Undo { uuid: uuid.to_owned(), files, later, waiting: false, error: None });
        }
    }

    /// The request that puts the files back as they were before the dialog's turn, from the dialog's button: one write.
    /// None while an earlier click waits, or when no dialog is open.
    pub fn confirm_undo(&mut self) -> Option<String> {
        let undo = self.undo.as_mut().filter(|undo| !undo.waiting)?;
        undo.waiting = true;
        undo.error = None;
        Some(request(UNDO_FILES, json!({ "subtype": "rewind_files", "user_message_id": undo.uuid })))
    }

    /// Claude Code's answer to Undo. False for any other request.
    pub fn answered(&mut self, id: &str, response: &Value, prompts: &[Prompt], items: &mut Vec<Item>) -> bool {
        if id != UNDO_FILES {
            return false;
        }
        let answer = &response["response"];
        if answer["canRewind"] == true {
            if let Some(undo) = self.undo.take() {
                self.mark_undone(prompts, &undo.uuid);
            }
            return true;
        }
        let why = answer["error"].as_str().or(response["error"].as_str()).unwrap_or("Claude Code didn't say why.");
        let error = format!("Couldn't undo the changes: {why}");
        match self.undo.as_mut() {
            Some(undo) => (undo.waiting, undo.error) = (false, Some(error)),
            // The dialog was closed before the answer came.
            None => items.push(Item::Error(error)),
        }
        true
    }

    /// The files were put back to before the message `uuid`: its turn and every later one are undone.
    pub fn mark_undone(&mut self, prompts: &[Prompt], uuid: &str) {
        let Some(first) = prompts.iter().find(|prompt| prompt.uuid == uuid) else {
            return;
        };
        self.undone.extend(prompts.iter().filter(|prompt| prompt.at >= first.at).map(|prompt| prompt.uuid.clone()));
    }
}

impl Session {
    /// The turn that ends before item `end`, as (the position of your message that began it, what it changed). A turn ends
    /// where the next message of yours begins, or at the conversation's end once Claude has stopped working. None when no
    /// turn ends there, and when the turn changed no files.
    pub fn turn_ending_at(&self, end: usize) -> Option<(usize, Turn)> {
        let asks = |item: &Item| matches!(item, Item::User(_) | Item::UserImages(..));
        let ends = if end == self.items.len() { !self.working } else { self.items.get(end).is_some_and(asks) };
        if !ends || end == 0 {
            return None;
        }
        let from = self.items[..end].iter().rposition(asks)?;
        let turn = self.edits.turn(from, end);
        (!turn.files.is_empty()).then_some((from, turn))
    }
}

/// The path as it is under `cwd`, else as it is.
pub fn relative<'a>(cwd: &str, path: &'a str) -> &'a str {
    path.strip_prefix(cwd.trim_end_matches('/')).and_then(|inside| inside.strip_prefix('/')).unwrap_or(path)
}

/// One call's result as the file it changed: an Edit or a Write (`filePath` with `structuredPatch`, and for a new file
/// `type: "create"` with its `content`), or a NotebookEdit's cell (`notebook_path` with `old_source` and `new_source`).
fn parse(result: &Value, at: usize) -> Option<Edited> {
    if let (Some(path), Some(hunks)) = (result["filePath"].as_str(), result["structuredPatch"].as_array()) {
        if result["type"] == "create" {
            let content = result["content"].as_str().unwrap_or_default();
            let count = content.lines().count();
            let lines = content.lines().take(PATCH_LINES).map(|line| format!("+{line}")).collect();
            return Some(Edited { at, path: path.to_owned(), added: count, removed: 0, hunks: vec![Hunk { at: 0, lines }], created: true });
        }
        let (mut list, mut kept, mut added, mut removed) = (Vec::new(), 0, 0, 0);
        for hunk in hunks {
            let lines: Vec<&str> = hunk["lines"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            added += lines.iter().filter(|line| line.starts_with('+')).count();
            removed += lines.iter().filter(|line| line.starts_with('-')).count();
            if kept + lines.len() > PATCH_LINES {
                continue;
            }
            kept += lines.len();
            // A hunk that takes no old lines starts after the line it names, as in a unified diff.
            let (start, old) = (hunk["oldStart"].as_u64().unwrap_or(0) as usize, hunk["oldLines"].as_u64().unwrap_or(0));
            let first = if old == 0 { start } else { start.saturating_sub(1) };
            list.push(Hunk { at: first, lines: lines.iter().map(|line| (*line).to_owned()).collect() });
        }
        return Some(Edited { at, path: path.to_owned(), added, removed, hunks: list, created: false });
    }
    let path = result["notebook_path"].as_str()?;
    let (old, new) = (result["old_source"].as_str().unwrap_or_default(), result["new_source"].as_str().unwrap_or_default());
    let (old_lines, new_lines) = (old.lines().count(), new.lines().count());
    let lines = old.lines().map(|line| format!("-{line}")).chain(new.lines().map(|line| format!("+{line}"))).take(PATCH_LINES).collect();
    Some(Edited { at, path: path.to_owned(), added: new_lines, removed: old_lines, hunks: vec![Hunk { at: 0, lines }], created: false })
}

/// A line of what a file's calls did to it, against the file before the first of them.
#[derive(Debug)]
enum Op {
    /// Lines no call touched or showed, unchanged.
    Gap(usize),
    Keep(String),
    Del(String),
    Add(String),
}

/// The index in `ops` where the line `line` (counting from 0) of the file as it is now starts, splitting a gap there.
fn split(ops: &mut Vec<Op>, line: usize) -> usize {
    let (mut i, mut seen) = (0, 0);
    while i < ops.len() && seen < line {
        match ops[i] {
            Op::Del(_) => {}
            Op::Gap(n) if seen + n > line => {
                let before = line - seen;
                ops[i] = Op::Gap(before);
                ops.insert(i + 1, Op::Gap(n - before));
                return i + 1;
            }
            Op::Gap(n) => seen += n,
            Op::Keep(_) | Op::Add(_) => seen += 1,
        }
        i += 1;
    }
    if seen < line {
        ops.push(Op::Gap(line - seen));
        return ops.len();
    }
    i
}

/// Takes in one call's hunks, each against the file as that call found it: after the calls before it.
fn apply(ops: &mut Vec<Op>, hunks: &[Hunk]) {
    // From the last, so the places of the ones before it stay where the call saw them.
    let mut order: Vec<&Hunk> = hunks.iter().collect();
    order.sort_by_key(|hunk| std::cmp::Reverse(hunk.at));
    for hunk in order {
        let mut j = split(ops, hunk.at);
        for line in &hunk.lines {
            let (kind, text) = (line.chars().next(), line.get(1..).unwrap_or_default().to_owned());
            match kind {
                Some('+') => {
                    ops.insert(j, Op::Add(text));
                    j += 1;
                }
                Some(kind @ (' ' | '-')) => {
                    while matches!(ops.get(j), Some(Op::Del(_))) {
                        j += 1;
                    }
                    // A line no call showed yet is the one this hunk names.
                    let gap = match ops.get(j) {
                        None => Some(1),
                        Some(Op::Gap(n)) => Some(*n),
                        Some(_) => None,
                    };
                    if let Some(n) = gap {
                        if j == ops.len() {
                            ops.push(Op::Keep(text.clone()));
                        } else {
                            ops[j] = Op::Keep(text.clone());
                        }
                        if n > 1 {
                            ops.insert(j + 1, Op::Gap(n - 1));
                        }
                    }
                    if kind == ' ' {
                        j += 1;
                    } else if matches!(ops[j], Op::Add(_)) {
                        ops.remove(j);
                    } else {
                        let Op::Keep(kept) = std::mem::replace(&mut ops[j], Op::Gap(0)) else { unreachable!("a line of the file") };
                        ops[j] = Op::Del(kept);
                        j += 1;
                    }
                }
                // "\ No newline at end of file" and the like.
                _ => {}
            }
        }
    }
}

/// The lines of context a hunk keeps round a change, as git's.
const CONTEXT: usize = 3;

/// The hunks of `ops`, with as much context round each change as is known, up to `CONTEXT` lines.
fn hunks_text(ops: &[Op]) -> String {
    let change = |op: &Op| matches!(op, Op::Del(_) | Op::Add(_));
    // How many kept lines each one is from the nearest change before it (or after it, run backwards); a gap ends the count.
    let distances = |ops: Vec<&Op>| -> Vec<usize> {
        let mut since = usize::MAX;
        ops.into_iter()
            .map(|op| {
                since = match op {
                    _ if change(op) => 0,
                    Op::Gap(_) => usize::MAX,
                    _ => since.saturating_add(1),
                };
                since
            })
            .collect()
    };
    let after = distances(ops.iter().collect());
    let mut before = distances(ops.iter().rev().collect());
    before.reverse();
    let shown: Vec<bool> = ops.iter().enumerate().map(|(i, op)| change(op) || (matches!(op, Op::Keep(_)) && after[i].min(before[i]) <= CONTEXT)).collect();
    let (mut out, mut old, mut new, mut i) = (String::new(), 0, 0, 0);
    while i < ops.len() {
        if !shown[i] {
            match &ops[i] {
                Op::Gap(n) => (old, new) = (old + n, new + n),
                Op::Keep(_) => (old, new) = (old + 1, new + 1),
                _ => {}
            }
            i += 1;
            continue;
        }
        let start = (old, new);
        let mut body = String::new();
        while i < ops.len() && shown[i] {
            match &ops[i] {
                Op::Keep(text) => (body, old, new) = (body + &format!(" {text}\n"), old + 1, new + 1),
                Op::Del(text) => (body, old) = (body + &format!("-{text}\n"), old + 1),
                Op::Add(text) => (body, new) = (body + &format!("+{text}\n"), new + 1),
                Op::Gap(_) => {}
            }
            i += 1;
        }
        let (old_lines, new_lines) = (old - start.0, new - start.1);
        // A side with no lines names the line before the hunk, as a unified diff does.
        let first = |at: usize, lines: usize| if lines == 0 { at } else { at + 1 };
        out.push_str(&format!("@@ -{},{old_lines} +{},{new_lines} @@\n{body}", first(start.0, old_lines), first(start.1, new_lines)));
    }
    out
}

/// Git-style patch text for `edits`, a file's calls together in the order they ran.
fn render(edits: &[&Edited], cwd: &str) -> String {
    let mut paths: Vec<&str> = Vec::new();
    for edit in edits {
        if !paths.contains(&edit.path.as_str()) {
            paths.push(&edit.path);
        }
    }
    let mut out = String::new();
    for path in paths {
        let shown = relative(cwd, path).trim_start_matches('/');
        let calls: Vec<&&Edited> = edits.iter().filter(|edit| edit.path == path).collect();
        // Each call's hunks are against the file as that call found it: put one after another they ran out of order or
        // over each other, which the diff viewer refused. Together they are one change from the file before the first.
        let mut ops = Vec::new();
        calls.iter().for_each(|edit| apply(&mut ops, &edit.hunks));
        let hunks = hunks_text(&ops);
        // Calls that undid each other leave nothing to show.
        if hunks.is_empty() {
            continue;
        }
        out.push_str(&format!("diff --git a/{shown} b/{shown}\n"));
        match calls[0].created {
            true => out.push_str(&format!("new file mode 100644\n--- /dev/null\n+++ b/{shown}\n{hunks}")),
            false => out.push_str(&format!("--- a/{shown}\n+++ b/{shown}\n{hunks}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CWD: &str = "/work/shop";

    /// What `claude -p --output-format stream-json` printed for a Write that made a file, trimmed to what matters.
    fn written() -> Value {
        json!({ "type": "user", "message": { "content": [{ "type": "tool_result", "tool_use_id": "w1", "content": "File created" }] },
            "tool_use_result": { "type": "create", "filePath": "/work/shop/src/new.ts", "content": "one\ntwo", "structuredPatch": [], "originalFile": null, "userModified": false } })
    }

    /// An Edit's, with the transcript's spelling of the key when `transcript` is set.
    fn edited(path: &str, transcript: bool) -> Value {
        let result = json!({ "filePath": path, "oldString": "beta\ngamma", "newString": "BETA\nGAMMA\nextra", "originalFile": "alpha\nbeta\ngamma\n",
            "structuredPatch": [{ "oldStart": 1, "oldLines": 3, "newStart": 1, "newLines": 4, "lines": [" alpha", "-beta", "-gamma", "+BETA", "+GAMMA", "+extra"] }], "userModified": false, "replaceAll": false });
        let key = if transcript { "toolUseResult" } else { "tool_use_result" };
        json!({ "type": "user", "message": { "content": [{ "type": "tool_result", "tool_use_id": "e1", "content": "updated" }] }, key: result })
    }

    #[test]
    fn a_turn_with_an_edit_and_a_write_lists_both_files_with_their_counts() {
        let mut edits = Edits::default();
        edits.heard(&edited("/work/shop/src/cart.ts", false), 3);
        edits.heard(&written(), 5);
        let turn = edits.turn(1, 6);
        assert_eq!(
            turn.files,
            [Changed { path: "/work/shop/src/cart.ts".into(), added: 3, removed: 2 }, Changed { path: "/work/shop/src/new.ts".into(), added: 2, removed: 0 }]
        );
        assert_eq!((turn.added, turn.removed), (5, 2));
    }

    #[test]
    fn a_file_a_turn_edits_twice_is_one_row_and_a_later_turn_is_not_in_it() {
        let mut edits = Edits::default();
        edits.heard(&edited("/work/shop/src/cart.ts", false), 3);
        edits.heard(&edited("/work/shop/src/cart.ts", false), 4);
        edits.heard(&edited("/work/shop/src/cart.ts", false), 9);
        let turn = edits.turn(1, 5);
        assert_eq!(turn.files.len(), 1);
        assert_eq!((turn.added, turn.removed), (6, 4));
    }

    #[test]
    fn a_resumed_sessions_transcript_entry_gives_the_same_card() {
        let mut stream = Edits::default();
        let mut transcript = Edits::default();
        stream.heard(&edited("/work/shop/src/cart.ts", false), 3);
        transcript.heard(&edited("/work/shop/src/cart.ts", true), 3);
        assert_eq!(stream.turn(1, 4), transcript.turn(1, 4));
        assert_eq!(stream.turn(1, 4).files.len(), 1);
    }

    #[test]
    fn a_failed_call_or_a_read_changes_nothing() {
        let mut edits = Edits::default();
        let mut failed = edited("/work/shop/src/cart.ts", false);
        failed["message"]["content"][0]["is_error"] = json!(true);
        edits.heard(&failed, 2);
        let read = json!({ "type": "user", "message": { "content": [{ "type": "tool_result", "tool_use_id": "r1", "content": "x" }] },
            "tool_use_result": { "type": "text", "file": { "filePath": "/work/shop/a.txt", "content": "a", "numLines": 1 } } });
        edits.heard(&read, 3);
        assert!(edits.is_empty());
    }

    #[test]
    fn view_changes_builds_one_patch_for_the_turn_with_paths_under_the_folder() {
        let mut edits = Edits::default();
        edits.heard(&edited("/work/shop/src/cart.ts", false), 3);
        edits.heard(&written(), 5);
        assert_eq!(
            edits.patch(1, 6, CWD),
            "diff --git a/src/cart.ts b/src/cart.ts\n--- a/src/cart.ts\n+++ b/src/cart.ts\n@@ -1,3 +1,4 @@\n alpha\n-beta\n-gamma\n+BETA\n+GAMMA\n+extra\n\
             diff --git a/src/new.ts b/src/new.ts\nnew file mode 100644\n--- /dev/null\n+++ b/src/new.ts\n@@ -0,0 +1,2 @@\n+one\n+two\n"
        );
    }

    /// An Edit's result for `hunk`, as Claude Code's `structuredPatch` has it: against the file as that call found it.
    fn edit_at(old_start: u64, lines: &[&str]) -> Value {
        let (old, new) = (lines.iter().filter(|line| !line.starts_with('+')).count(), lines.iter().filter(|line| !line.starts_with('-')).count());
        json!({ "type": "user", "message": { "content": [{ "type": "tool_result", "tool_use_id": "e", "content": "updated" }] },
            "tool_use_result": { "filePath": "/work/shop/src/hold.ts", "structuredPatch": [{ "oldStart": old_start, "oldLines": old, "newStart": old_start, "newLines": new, "lines": lines }] } })
    }

    /// Whether the diff viewer's parser takes `patch`, as Wings reads View changes'.
    fn parses(patch: &str) -> Result<(), String> {
        use diffy::patch_set::{ParseOptions, PatchSet};
        PatchSet::parse(patch, ParseOptions::gitdiff()).try_for_each(|file| file.map(drop).map_err(|error| error.to_string()))
    }

    #[test]
    fn a_file_edited_twice_is_one_patch_against_the_file_before_the_first_edit() {
        // The second edit is earlier in the file than the first: one after the other, its hunk came out of order, and
        // View changes said "hunks not in order or overlap".
        let mut edits = Edits::default();
        edits.heard(&edit_at(6, &[" a6", " a7", " a8", "-a9", "+A9", " a10", " a11", " a12"]), 3);
        edits.heard(&edit_at(1, &[" a1", "-a2", "+A2", "+new", " a3", " a4", " a5"]), 4);
        let patch = edits.patch(1, 5, CWD);
        assert_eq!(parses(&patch), Ok(()));
        assert_eq!(
            patch,
            "diff --git a/src/hold.ts b/src/hold.ts\n--- a/src/hold.ts\n+++ b/src/hold.ts\n\
             @@ -1,12 +1,13 @@\n a1\n-a2\n+A2\n+new\n a3\n a4\n a5\n a6\n a7\n a8\n-a9\n+A9\n a10\n a11\n a12\n"
        );
        // Two edits to lines next to each other, whose hunks lay over each other.
        let mut edits = Edits::default();
        edits.heard(&edit_at(2, &[" a2", " a3", " a4", "-a5", "+A5", " a6", " a7", " a8"]), 3);
        edits.heard(&edit_at(3, &[" a3", " a4", " A5", "-a6", "+A6", " a7", " a8", " a9"]), 4);
        let patch = edits.file_patch("/work/shop/src/hold.ts", CWD).unwrap();
        assert_eq!(parses(&patch), Ok(()));
        assert!(patch.ends_with("@@ -2,8 +2,8 @@\n a2\n a3\n a4\n-a5\n+A5\n-a6\n+A6\n a7\n a8\n a9\n"), "{patch}");
        // Far apart, they stay two hunks, each with its own context.
        let mut edits = Edits::default();
        edits.heard(&edit_at(40, &[" b40", "-b41", "+B41", " b42"]), 3);
        edits.heard(&edit_at(1, &[" a1", "+top", " a2"]), 4);
        let patch = edits.patch(1, 5, CWD);
        assert_eq!(parses(&patch), Ok(()));
        assert!(patch.ends_with("@@ -1,2 +1,3 @@\n a1\n+top\n a2\n@@ -40,3 +41,3 @@\n b40\n-b41\n+B41\n b42\n"), "{patch}");
    }

    #[test]
    fn edits_to_a_file_written_in_the_turn_show_in_the_new_file() {
        let mut edits = Edits::default();
        edits.heard(&written(), 3);
        let mut edit = edit_at(1, &[" one", "-two", "+TWO"]);
        edit["tool_use_result"]["filePath"] = json!("/work/shop/src/new.ts");
        edits.heard(&edit, 4);
        let patch = edits.patch(1, 5, CWD);
        assert_eq!(parses(&patch), Ok(()));
        assert!(patch.ends_with("new file mode 100644\n--- /dev/null\n+++ b/src/new.ts\n@@ -0,0 +1,2 @@\n+one\n+TWO\n"), "{patch}");
    }

    #[test]
    fn a_notebook_cell_counts_its_old_and_new_source() {
        let result = json!({ "tool_use_result": { "new_source": "print(2)", "old_source": "print(1)", "edit_mode": "replace", "cell_id": "c1", "notebook_path": "/work/shop/n.ipynb" }, "message": { "content": [] } });
        let mut edits = Edits::default();
        edits.heard(&result, 2);
        assert_eq!(edits.turn(0, 2).files, [Changed { path: "/work/shop/n.ipynb".into(), added: 1, removed: 1 }]);
    }

    #[test]
    fn undo_asks_first_and_sends_one_rewind_for_the_turns_message() {
        let mut edits = Edits::default();
        edits.heard(&edited("/work/shop/src/cart.ts", false), 3);
        edits.heard(&written(), 5);
        edits.ask_undo("m1", 1, 6);
        // Opening the dialog writes nothing.
        let dialog = edits.undo.as_ref().unwrap();
        assert_eq!(dialog.files.len(), 2);
        assert!(!dialog.waiting);
        let sent = edits.confirm_undo().unwrap();
        let line: Value = serde_json::from_str(sent.trim()).unwrap();
        assert_eq!(sent.matches('\n').count(), 1);
        assert_eq!(line["request_id"], UNDO_FILES);
        assert_eq!(line["request"], json!({ "subtype": "rewind_files", "user_message_id": "m1" }));
        // A second click while it waits writes nothing.
        assert_eq!(edits.confirm_undo(), None);
    }

    #[test]
    fn undo_names_the_files_later_turns_changed_too() {
        let mut edits = Edits::default();
        edits.heard(&edited("/work/shop/src/cart.ts", false), 3);
        edits.heard(&edited("/work/shop/src/cart.ts", false), 9);
        edits.heard(&written(), 11);
        edits.ask_undo("m1", 1, 5);
        assert_eq!(edits.undo.as_ref().unwrap().later, ["/work/shop/src/cart.ts", "/work/shop/src/new.ts"]);
    }

    #[test]
    fn an_undone_turn_and_the_later_ones_show_undone_and_a_refusal_shows_in_the_dialog() {
        let prompt = |at: usize, uuid: &str| Prompt { at, uuid: uuid.into(), parent: None };
        let prompts = [prompt(0, "m1"), prompt(6, "m2")];
        let mut edits = Edits::default();
        let mut items = Vec::new();
        edits.heard(&edited("/work/shop/src/cart.ts", false), 3);
        edits.ask_undo("m1", 0, 5);
        edits.confirm_undo();
        let refused = json!({ "response": { "canRewind": false, "error": "No checkpoint" } });
        assert!(edits.answered(UNDO_FILES, &refused, &prompts, &mut items));
        assert_eq!(edits.undo.as_ref().unwrap().error.as_deref(), Some("Couldn't undo the changes: No checkpoint"));
        assert!(!edits.is_undone("m1"));
        assert!(edits.confirm_undo().is_some());
        assert!(edits.answered(UNDO_FILES, &json!({ "response": { "canRewind": true } }), &prompts, &mut items));
        assert!(edits.undo.is_none());
        assert!(edits.is_undone("m1") && edits.is_undone("m2"));
        assert!(!edits.answered("rewind-files", &json!({}), &prompts, &mut items));
    }

    #[test]
    fn a_rewound_conversation_drops_the_edits_after_the_cut() {
        let mut edits = Edits::default();
        edits.heard(&edited("/work/shop/src/cart.ts", false), 3);
        edits.heard(&written(), 9);
        edits.cut(6);
        assert_eq!(edits.files().to_vec(), ["/work/shop/src/cart.ts"]);
    }

    /// The lines of a made-up session: your message, an Edit and a Write with their results, the turn's end.
    fn session(transcript: bool) -> Session {
        let mut session = Session::default();
        let key = if transcript { "toolUseResult" } else { "tool_use_result" };
        let call = |id: &str, name: &str| json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "id": id, "name": name, "input": { "file_path": "/work/shop/src/cart.ts" } }] } });
        let mut lines = vec![
            json!({ "type": "user", "message": { "content": "Add a discount" }, "uuid": "m1" }),
            call("e1", "Edit"),
            edited("/work/shop/src/cart.ts", transcript),
            call("w1", "Write"),
            written(),
            json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": "Done." }] } }),
        ];
        lines[2]["message"]["content"][0]["tool_use_id"] = json!("e1");
        lines[4]["message"]["content"][0]["tool_use_id"] = json!("w1");
        if transcript {
            lines[4][key] = lines[4]["tool_use_result"].take();
        } else {
            lines.push(json!({ "type": "result", "subtype": "success", "is_error": false, "duration_ms": 9000, "num_turns": 2, "result": "" }));
        }
        lines.push(json!({ "type": "user", "message": { "content": "Thanks" }, "uuid": "m2" }));
        lines.iter().for_each(|line| session.hear(line));
        session
    }

    #[test]
    fn a_turn_card_has_the_turns_files_and_comes_before_your_next_message() {
        let session = session(false);
        let next = session.items.iter().position(|item| matches!(item, Item::User(said) if said == "Thanks")).unwrap();
        let (from, turn) = session.turn_ending_at(next).unwrap();
        assert_eq!((from, turn.files.len(), turn.added, turn.removed), (0, 2, 5, 2));
        // Nothing ends in the middle of the turn, and the turn under way has no card yet.
        assert_eq!(session.turn_ending_at(2), None);
        assert_eq!(session.turn_ending_at(session.items.len()), None);
    }

    #[test]
    fn a_resumed_sessions_card_comes_from_its_transcript() {
        let session = session(true);
        let next = session.items.iter().position(|item| matches!(item, Item::User(said) if said == "Thanks")).unwrap();
        assert_eq!(session.turn_ending_at(next).map(|(_, turn)| turn.files.len()), Some(2));
    }

    #[test]
    fn undo_goes_back_to_before_the_message_the_turn_began_with() {
        let mut session = session(false);
        let next = session.items.iter().position(|item| matches!(item, Item::User(said) if said == "Thanks")).unwrap();
        let uuid = session.prompts.iter().find(|prompt| prompt.at == 0).unwrap().uuid.clone();
        session.edits.ask_undo(&uuid, 0, next);
        let sent = session.edits.confirm_undo().unwrap();
        assert!(sent.contains("\"user_message_id\":\"m1\"") && sent.contains("rewind_files"));
    }
}
