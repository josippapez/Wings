//! Opening the diff viewer, which only the main script can.

use crate::{
    batch::{batched, fit_patch, placeholder},
    changes::{COLOR, Changes, FileChange, MAX_UNTRACKED_READ, Mode, args, changes, literal},
    dates::date_time_of,
};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use wings_plugin::{
    diff::{open_live, open_while},
    format::{basename, plural},
    git::{raw, run},
    task::join,
};

/// About 2 MB of patch in all. Past this the biggest files turn into "Too large to show" entries, since the viewer
/// would take seconds to draw them.
pub const MAX_PATCH_LINES: u64 = 50_000;
pub const MAX_PATCH_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_PATCH_FILES: usize = 1_500;

/// A patch for files git doesn't track yet, read with `git grep`, since Wings refuses `git diff --no-index`.
async fn untracked_patch(root: &str, files: &[FileChange]) -> Result<String, String> {
    if files.is_empty() {
        return Ok(String::new());
    }
    let paths: Vec<&str> = files.iter().map(|file| file.path.as_str()).collect();
    let output = run(root, &args(&["grep", "--untracked", "-I", "-z", "--no-color", "--no-line-number", "--no-column", "-e", "", "--"], &literal(paths.iter().copied()))).await?;
    if output.code > 1 {
        let said = output.stderr.trim();
        return Err(if said.is_empty() { "git grep failed".into() } else { said.to_owned() });
    }
    let mut text: HashMap<&str, Vec<&str>> = HashMap::new();
    for line in output.stdout.split('\n') {
        if let Some((path, rest)) = line.split_once('\0') {
            text.entry(path).or_default().push(rest);
        }
    }
    // Files with no text lines are empty or binary. Counting without -I tells them apart.
    let rest: Vec<&str> = paths.iter().copied().filter(|path| !text.contains_key(path)).collect();
    let mut binary: HashSet<String> = HashSet::new();
    if !rest.is_empty() {
        let all = run(root, &args(&["grep", "--untracked", "-z", "--count", "--no-color", "-e", "", "--"], &literal(rest))).await?;
        binary.extend(all.stdout.split('\n').filter_map(|line| line.rsplit_once('\0')).map(|(path, _)| path.to_owned()));
    }
    Ok(paths
        .iter()
        .map(|path| {
            let head = format!("diff --git a/{path} b/{path}\nnew file mode 100644\n");
            if binary.contains(*path) {
                return format!("{head}Binary files /dev/null and b/{path} differ\n");
            }
            match text.get(path) {
                Some(body) => format!("{head}--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{} @@\n{}", body.len(), body.iter().map(|line| format!("+{line}\n")).collect::<String>()),
                None => head,
            }
        })
        .collect())
}

/// The patch for one file, or all of them, and a note about what it leaves out.
async fn patch_of(c: &Changes, path: Option<&str>) -> Result<(String, String), String> {
    let files: Vec<FileChange> = c.files.iter().filter(|file| path.is_none_or(|path| file.path == path)).cloned().collect();
    if path.is_some() && files.is_empty() {
        return Err("This file has no changes any more.".into());
    }
    let size: u64 = files.iter().map(|file| file.added.unwrap_or(0) + file.removed.unwrap_or(0)).sum();
    if files.len() > MAX_PATCH_FILES || size > MAX_PATCH_LINES {
        return Err(format!("Too large to show: {}, {}. Open files one at a time from the Changes sidebar.", plural(files.len(), "file"), plural(size as usize, "changed line")));
    }
    let (untracked, tracked): (Vec<FileChange>, Vec<FileChange>) = files.into_iter().partition(|file| file.untracked);
    // Never one giant patch: git's output is capped at 8 MB, and one minified bundle can be most of it.
    let too_big = |file: &FileChange| placeholder(&format!("diff --git a/{0} b/{0}\n--- a/{0}\n+++ b/{0}\n", file.path), "Too large to show (over 8 MB)");
    let skipped = untracked.len().saturating_sub(MAX_UNTRACKED_READ);
    let (diff, added) = join(
        batched(
            tracked,
            async |part: Vec<FileChange>| {
                let paths = literal(part.iter().flat_map(|file| file.old_path.as_deref().into_iter().chain([file.path.as_str()])));
                raw(&c.root, &[&["diff", &c.base, "-M"], &COLOR[..], &["--"], &paths.iter().map(String::as_str).collect::<Vec<_>>()].concat()).await
            },
            too_big,
        ),
        batched(untracked.into_iter().take(MAX_UNTRACKED_READ).collect(), async |part: Vec<FileChange>| untracked_patch(&c.root, &part).await, too_big),
    )
    .await;
    let note = if skipped > 0 { format!(" · {} not shown", plural(skipped, "new file")) } else { String::new() };
    Ok((fit_patch(&format!("{}{}", diff?, added?), MAX_PATCH_BYTES), note))
}

/// Opens the viewer at once in its loading state, fills it when git answers, and keeps it up to date while it's open.
pub async fn open_changes(root: String, mode: Mode, path: Option<String>) -> Result<(), String> {
    let title = path.clone().unwrap_or_else(|| if mode == Mode::Branch { "Changes on this branch" } else { "Uncommitted changes" }.to_owned());
    let subtitle = format!("{} · {}", basename(&root), if mode == Mode::Branch { "This branch" } else { "Working tree against HEAD" });
    let key = root.clone();
    open_live(json!({ "title": title, "cwd": root, "subtitle": subtitle }), &key, move |fresh| {
        let (root, path) = (root.clone(), path.clone());
        async move {
            let c = changes(&root, mode, fresh).await?;
            if c.files.is_empty() {
                return Ok(json!({ "error": if mode == Mode::Branch { "Nothing changed on this branch." } else { "No uncommitted changes." } }));
            }
            let (patch, note) = patch_of(&c, path.as_deref()).await?;
            Ok(if patch.is_empty() { json!({ "error": format!("No diff to show{note}.") }) } else { json!({ "patch": patch }) })
        }
    })
    .await
}

/// `git show` for one commit, a merge against its first parent, so it shows what the merge brought in.
const SHOW: [&str; 4] = ["show", "--format=", "--diff-merges=first-parent", "-M"];

/// Files and changed lines in `git show --numstat -z`.
fn numstat_size(numstat: &str) -> (usize, u64) {
    let fields: Vec<&str> = numstat.split('\0').collect();
    let (mut files, mut size, mut i) = (0, 0, 0);
    while i < fields.len() && !fields[i].is_empty() {
        let mut parts = fields[i].splitn(3, '\t');
        let (added, removed, path) = (parts.next().unwrap_or_default(), parts.next().unwrap_or_default(), parts.next().unwrap_or_default());
        // A rename has an empty path, then its old and new paths as their own fields.
        i += if path.is_empty() { 3 } else { 1 };
        files += 1;
        size += added.parse::<u64>().unwrap_or(0) + removed.parse::<u64>().unwrap_or(0);
    }
    (files, size)
}

/// Opens the viewer at once with what the page already showed of the commit, then fills it with its patch.
pub async fn open_commit(root: &str, commit: &Value) -> Result<(), String> {
    let hash = commit["hash"].as_str().unwrap_or_default();
    // It goes to `git show` as an argument, so only a full hash gets there.
    if hash.len() != 40 || !hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return Err("That isn't a commit hash.".into());
    }
    let (short, subject, author, at) = (&hash[..7], commit["subject"].as_str().unwrap_or_default(), commit["author"].as_str().unwrap_or_default(), commit["at"].as_f64().unwrap_or(0.0));
    let when = if at > 0.0 { date_time_of(at) } else { String::new() };
    let subtitle = [short, author, &when].into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" · ");
    open_while(json!({ "title": if subject.is_empty() { short } else { subject }, "subtitle": subtitle }), async {
        let show = [&SHOW[..], &COLOR].concat();
        let (files, size) = numstat_size(&raw(root, &[&show[..], &["--numstat", "-z", hash, "--"]].concat()).await?);
        if files == 0 {
            return Ok(json!({ "error": "This commit changes no files." }));
        }
        if files > MAX_PATCH_FILES || size > MAX_PATCH_LINES {
            return Err(format!("Too large to show: {}, {}.", plural(files, "file"), plural(size as usize, "changed line")));
        }
        let patch = raw(root, &[&show[..], &["--patch", hash, "--"]].concat()).await?;
        if patch.len() > MAX_PATCH_BYTES {
            return Err(format!("Too large to show: {} MB of changes in {}.", (patch.len() as f64 / 1024.0 / 1024.0).round(), plural(files, "file")));
        }
        Ok(json!({ "patch": patch }))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};
    use wings_plugin::{host, task::block_on};

    /// A fake Wings: git answers by the start of its command line, and what the diff viewer was sent is kept.
    fn fake(git: &'static [(&'static str, i64, &'static str)]) -> Rc<RefCell<Vec<Value>>> {
        let sent: Rc<RefCell<Vec<Value>>> = Rc::default();
        let seen = sent.clone();
        host::testing::answer(move |method, params| {
            if method != "exec" {
                if method == "updateDiff" {
                    seen.borrow_mut().push(params.clone());
                }
                return Ok(json!({ "id": 1 }));
            }
            let line = params["args"].as_array().unwrap().iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" ");
            Ok(match git.iter().find(|answer| line.starts_with(answer.0)) {
                Some((_, code, stdout)) => json!({ "code": code, "stdout": stdout, "stderr": "" }),
                None => json!({ "code": 1, "stdout": "", "stderr": format!("fake git has no answer for {line}") }),
            })
        });
        sent
    }

    #[test]
    fn the_viewer_gets_the_tracked_diff_then_new_files_as_added_lines_and_binaries_as_such() {
        let sent = fake(&[
            ("branch --show-current", 0, "main\n"),
            ("rev-parse --verify --quiet HEAD", 0, "abc\n"),
            ("diff abc --numstat", 0, "1\t0\ta.rs\0"),
            ("diff abc --name-status", 0, "M\0a.rs\0"),
            ("ls-files", 0, "new.txt\0pic.png\0"),
            ("grep --untracked -I -z --count", 0, "new.txt\x002\n"),
            ("diff abc -M", 0, "diff --git a/a.rs b/a.rs\n@@ -1 +1,2 @@\n+x\n"),
            ("grep --untracked -I -z --no-color --no-line-number", 0, "new.txt\0one\nnew.txt\0two\n"),
            ("grep --untracked -z --count", 0, "pic.png\x003\n"),
        ]);
        block_on(open_changes("/r".into(), Mode::Uncommitted, None)).unwrap();
        let patch = sent.borrow()[0]["patch"].as_str().unwrap().to_owned();
        assert_eq!(
            patch,
            "diff --git a/a.rs b/a.rs\n@@ -1 +1,2 @@\n+x\n\
             diff --git a/new.txt b/new.txt\nnew file mode 100644\n--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1,2 @@\n+one\n+two\n\
             diff --git a/pic.png b/pic.png\nnew file mode 100644\nBinary files /dev/null and b/pic.png differ\n"
        );
    }

    #[test]
    fn a_repo_with_nothing_changed_and_a_file_that_is_no_longer_changed_say_so() {
        let sent = fake(&[("branch --show-current", 0, "main\n"), ("rev-parse --verify --quiet HEAD", 0, "abc\n"), ("diff abc", 0, ""), ("ls-files", 0, "")]);
        block_on(open_changes("/clean".into(), Mode::Uncommitted, None)).unwrap();
        assert_eq!(sent.borrow()[0]["error"], "No uncommitted changes.");
        let sent = fake(&[("branch --show-current", 0, "main\n"), ("rev-parse --verify --quiet HEAD", 0, "abc\n"), ("diff abc --numstat", 0, "1\t0\ta.rs\0"), ("diff abc --name-status", 0, "M\0a.rs\0"), ("ls-files", 0, "")]);
        block_on(open_changes("/one".into(), Mode::Uncommitted, Some("gone.rs".into()))).unwrap();
        assert_eq!(sent.borrow()[0]["error"], "This file has no changes any more.");
    }

    #[test]
    fn a_commit_opens_only_by_a_full_hash_and_one_too_large_says_how_large() {
        let hash = "0123456789abcdef0123456789abcdef01234567";
        let sent = fake(&[("show --format= --diff-merges=first-parent -M --no-color --no-ext-diff --no-textconv --no-relative --numstat", 0, "60000\t5\tbig.json\x002\t1\t\0a\0b\0")]);
        assert_eq!(block_on(async { open_commit("/r", &json!({ "hash": "HEAD" })).await }).unwrap_err(), "That isn't a commit hash.");
        block_on(async move { open_commit("/r", &json!({ "hash": hash, "subject": "Add data", "author": "Ana", "at": 0 })).await }).unwrap();
        assert_eq!(sent.borrow()[0]["error"], "Too large to show: 2 files, 60008 changed lines.");
        assert_eq!(numstat_size(""), (0, 0));
    }
}
