//! Opening the diff viewer, which only the main script can.

use crate::{
    batch::{batched, fit_patch, placeholder},
    changes::{COLOR, Changes, FileChange, MAX_UNTRACKED_READ, Mode, args, changes_against, literal},
    dates::date_time_of,
};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use wings_plugin::{
    diff::{open_live_scoped, open_while},
    format::{basename, plural},
    git::{default_branch, ok, raw, run},
    task::join,
};

/// The id of the sidebar the viewer opens in the place of (`wings-plugin.json`).
const SIDEBAR: &str = "changes";

/// The scopes of a viewer from the sidebar: `uncommitted`, `branch` (since the default branch), and `branch:<name>` for
/// another local branch. The viewer's picker holds 6.
const MAX_BRANCH_SCOPES: usize = 4;
/// The longest a scope's id or label is, in UTF-16 units (the viewer cuts it there).
const MAX_SCOPE_TEXT: usize = 60;
const BRANCH_PREFIX: &str = "branch:";

/// What a scope id asks for: the mode, and the branch to compare with in `Mode::Branch` when it names one.
fn parse_scope(id: &str) -> (Mode, Option<&str>) {
    match id.strip_prefix(BRANCH_PREFIX) {
        Some(name) if !name.is_empty() => (Mode::Branch, Some(name)),
        _ => (Mode::named(id), None),
    }
}

/// The repo's local branches, the one committed to last first. None when git won't list them.
async fn local_branches(root: &str) -> Vec<String> {
    let listed = ok(root, &["for-each-ref", "--sort=-committerdate", "--format=%(refname:short)", "refs/heads"]).await;
    listed.map(|names| names.lines().map(str::trim).filter(|name| !name.is_empty()).map(str::to_owned).collect()).unwrap_or_default()
}

/// The scopes the viewer offers: the two it always has, then up to four other local branches to compare with, the one
/// picked now first. The branch checked out and the default branch's own local copy are no other branch to compare with.
fn scopes_of(branches: &[String], current: Option<&str>, default: Option<&str>, picked: Option<&str>) -> Value {
    let fits = |id: &str, label: &str| id.encode_utf16().count() <= MAX_SCOPE_TEXT && label.encode_utf16().count() <= MAX_SCOPE_TEXT;
    let default_local = default.map(|name| name.strip_prefix("origin/").unwrap_or(name));
    let mut others: Vec<&str> = branches
        .iter()
        .map(String::as_str)
        .filter(|name| Some(*name) != current && Some(*name) != default_local && fits(&format!("{BRANCH_PREFIX}{name}"), &format!("Against {name}")))
        .collect();
    if let Some(at) = others.iter().position(|name| Some(format!("{BRANCH_PREFIX}{name}")).as_deref() == picked) {
        let name = others.remove(at);
        others.insert(0, name);
    }
    let mut scopes = vec![json!({ "id": Mode::Uncommitted.name(), "label": "Uncommitted" }), json!({ "id": Mode::Branch.name(), "label": "Branch" })];
    scopes.extend(others.into_iter().take(MAX_BRANCH_SCOPES).map(|name| json!({ "id": format!("{BRANCH_PREFIX}{name}"), "label": format!("Against {name}") })));
    Value::Array(scopes)
}

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
/// From the sidebar (`from_sidebar`) the viewer takes the sidebar's place and shows every changed file stacked, scrolled
/// to `path`, with a picker between the uncommitted changes and the branch's. Only a change too large for that falls
/// back to `path` alone.
pub async fn open_changes(root: String, mode: Mode, path: Option<String>, from_sidebar: bool) -> Result<(), String> {
    open_changes_as(root, mode, path, from_sidebar, None).await
}

/// `open_changes`, as a review for the Claude chat `chat` names when it does: the viewer lets the user comment on lines,
/// and each comment goes to that chat (`review.rs`).
pub async fn open_changes_as(root: String, mode: Mode, path: Option<String>, from_sidebar: bool, chat: Option<String>) -> Result<(), String> {
    let all = from_sidebar && path.is_some();
    let title = match &path {
        _ if from_sidebar => "Changes".to_owned(),
        Some(path) => path.clone(),
        None => if mode == Mode::Branch { "Changes on this branch" } else { "Uncommitted changes" }.to_owned(),
    };
    // From the sidebar the scope is picked in the viewer, so the subtitle names only the repo.
    let subtitle = if from_sidebar { basename(&root).to_owned() } else { format!("{} · {}", basename(&root), if mode == Mode::Branch { "This branch" } else { "Working tree against HEAD" }) };
    let key = root.clone();
    let mut open = json!({ "title": title, "cwd": root, "subtitle": subtitle });
    if from_sidebar {
        open["sidebar"] = SIDEBAR.into();
        open["scopes"] = scopes_of(&[], None, None, None);
        open["scope"] = mode.name().into();
        // Both diff against the working tree, so the viewer may read the lines between the hunks from the repo.
        open["worktree"] = true.into();
    }
    if all {
        open["focus"] = json!({ "path": path });
    }
    // Comments on lines go to the chat that asked, when one did and the viewer is the sidebar's.
    let reviewing = chat.filter(|_| from_sidebar);
    if reviewing.is_some() {
        open["allowNewComments"] = true.into();
    }
    crate::review::start(reviewing);
    open_live_scoped(open, &key, move |fresh, scope| {
        let (root, path) = (root.clone(), path.clone());
        let (mode, base) = scope.as_deref().map_or((mode, None), |id| {
            let (mode, base) = parse_scope(id);
            (mode, base.map(str::to_owned))
        });
        async move {
            // The picker lists the branches to compare with, and a branch asked for has to be one of them.
            let mut scopes = None;
            if from_sidebar {
                let (branches, current) = join(local_branches(&root), ok(&root, &["branch", "--show-current"])).await;
                let default = default_branch(&root).await;
                let picked = base.as_ref().map(|name| format!("{BRANCH_PREFIX}{name}"));
                scopes = Some(scopes_of(&branches, current.as_deref(), default.as_deref(), picked.as_deref()));
                if let Some(name) = base.as_ref().filter(|name| !branches.contains(name)) {
                    return Ok(json!({ "error": format!("{name} is not a local branch any more."), "scopes": scopes }));
                }
            }
            let c = changes_against(&root, mode, base.as_deref(), fresh).await?;
            let mut shown = if c.files.is_empty() {
                json!({ "error": if mode == Mode::Branch { "Nothing changed on this branch." } else { "No uncommitted changes." } })
            } else {
                let (patch, note) = match patch_of(&c, if all { None } else { path.as_deref() }).await {
                    Err(_) if all => patch_of(&c, path.as_deref()).await?,
                    done => done?,
                };
                if patch.is_empty() { json!({ "error": format!("No diff to show{note}.") }) } else { json!({ "patch": patch }) }
            };
            if let Some(scopes) = scopes {
                shown["scopes"] = scopes;
            }
            Ok(shown)
        }
    })
    .await
    .map(drop)
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
        block_on(open_changes("/r".into(), Mode::Uncommitted, None, false)).unwrap();
        let patch = sent.borrow()[0]["patch"].as_str().unwrap().to_owned();
        assert_eq!(
            patch,
            "diff --git a/a.rs b/a.rs\n@@ -1 +1,2 @@\n+x\n\
             diff --git a/new.txt b/new.txt\nnew file mode 100644\n--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1,2 @@\n+one\n+two\n\
             diff --git a/pic.png b/pic.png\nnew file mode 100644\nBinary files /dev/null and b/pic.png differ\n"
        );
    }

    /// From the sidebar the viewer stacks every file with the uncommitted changes and the branch's to pick from, may read
    /// the repo's files for its folds, and starts on the scope the sidebar showed.
    #[test]
    fn from_the_sidebar_the_viewer_offers_both_scopes_and_starts_on_the_one_asked_for() {
        let git: &'static [(&'static str, i64, &'static str)] = &[
            ("branch --show-current", 0, "feature\n"),
            ("rev-parse --verify --quiet HEAD", 0, "abc\n"),
            ("rev-parse --abbrev-ref origin/HEAD", 0, "origin/main\n"),
            ("merge-base HEAD origin/main", 0, "base1\n"),
            ("diff base1 --numstat", 0, "1\t0\ta.rs\0"),
            ("diff base1 --name-status", 0, "M\0a.rs\0"),
            ("ls-files", 0, ""),
            ("diff base1 -M", 0, "diff --git a/a.rs b/a.rs\n@@ -1 +1,2 @@\n+x\n"),
        ];
        let calls: Rc<RefCell<Vec<(String, Value)>>> = Rc::default();
        let seen = calls.clone();
        host::testing::answer(move |method, params| {
            if method != "exec" {
                seen.borrow_mut().push((method.to_owned(), params.clone()));
                return Ok(json!({ "id": 1 }));
            }
            let line = params["args"].as_array().unwrap().iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" ");
            Ok(match git.iter().find(|answer| line.starts_with(answer.0)) {
                Some((_, code, stdout)) => json!({ "code": code, "stdout": stdout, "stderr": "" }),
                None => json!({ "code": 1, "stdout": "", "stderr": format!("fake git has no answer for {line}") }),
            })
        });
        block_on(open_changes("/r".into(), Mode::Branch, Some("a.rs".into()), true)).unwrap();
        let calls = calls.borrow();
        let sent = |method: &str| calls.iter().find(|call| call.0 == method).map(|call| call.1.clone()).unwrap();
        let opened = sent("openDiff");
        assert_eq!((opened["title"].as_str(), opened["subtitle"].as_str(), opened["sidebar"].as_str()), (Some("Changes"), Some("r"), Some("changes")));
        assert_eq!(opened["scopes"], json!([{ "id": "uncommitted", "label": "Uncommitted" }, { "id": "branch", "label": "Branch" }]));
        assert_eq!((opened["scope"].as_str(), opened["worktree"].as_bool(), opened["focus"]["path"].as_str()), (Some("branch"), Some(true), Some("a.rs")));
        let update = sent("updateDiff");
        assert_eq!(update["scope"], "branch");
        assert!(update["patch"].as_str().unwrap().starts_with("diff --git a/a.rs"));
    }

    fn names(scopes: &Value) -> Vec<&str> {
        scopes.as_array().unwrap().iter().map(|scope| scope["id"].as_str().unwrap()).collect()
    }

    /// The picker's two fixed scopes, then up to four other local branches, the branch checked out and the default
    /// branch's own copy left out, the one picked kept in view, and names that wouldn't fit the viewer's limit left out.
    #[test]
    fn the_picker_offers_other_local_branches_and_keeps_the_one_picked_in_view() {
        let branches: Vec<String> = ["feature", "main", "a", "b", "c", "d", "e"].map(String::from).to_vec();
        let scopes = scopes_of(&branches, Some("feature"), Some("origin/main"), None);
        assert_eq!(names(&scopes), ["uncommitted", "branch", "branch:a", "branch:b", "branch:c", "branch:d"]);
        assert_eq!(scopes[2]["label"], "Against a");
        assert_eq!(names(&scopes_of(&branches, Some("feature"), Some("origin/main"), Some("branch:e")))[2], "branch:e");
        let long = vec!["x".repeat(MAX_SCOPE_TEXT)];
        assert_eq!(names(&scopes_of(&long, None, None, None)), ["uncommitted", "branch"]);
        assert_eq!(names(&scopes_of(&[], None, None, None)), ["uncommitted", "branch"]);
        assert_eq!(parse_scope("branch:release/1.0"), (Mode::Branch, Some("release/1.0")));
        assert_eq!((parse_scope("branch"), parse_scope("branch:"), parse_scope("uncommitted")), ((Mode::Branch, None), (Mode::Uncommitted, None), (Mode::Uncommitted, None)));
    }

    /// A review from the chat lets the viewer take comments, and its picker lists the repo's other local branches.
    /// Without a chat to send them to, or outside the sidebar, no comments are offered.
    #[test]
    fn a_review_opens_with_new_comments_allowed_and_the_repos_branches_to_pick() {
        let git: &'static [(&'static str, i64, &'static str)] = &[
            ("branch --show-current", 0, "feature\n"),
            ("rev-parse --verify --quiet HEAD", 0, "abc\n"),
            ("rev-parse --abbrev-ref origin/HEAD", 0, "origin/main\n"),
            ("for-each-ref --sort=-committerdate", 0, "feature\nmain\ndevelop\n"),
            ("ls-files", 0, ""),
            ("diff abc --numstat", 0, "1\t0\ta.rs\0"),
            ("diff abc --name-status", 0, "M\0a.rs\0"),
            ("diff abc -M", 0, "diff --git a/a.rs b/a.rs\n@@ -1 +1,2 @@\n+y\n"),
        ];
        let calls: Rc<RefCell<Vec<(String, Value)>>> = Rc::default();
        let seen = calls.clone();
        host::testing::answer(move |method, params| {
            if method != "exec" {
                seen.borrow_mut().push((method.to_owned(), params.clone()));
                return Ok(json!({ "id": 1 }));
            }
            let line = params["args"].as_array().unwrap().iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" ");
            Ok(match git.iter().find(|answer| line.starts_with(answer.0)) {
                Some((_, code, stdout)) => json!({ "code": code, "stdout": stdout, "stderr": "" }),
                None => json!({ "code": 1, "stdout": "", "stderr": format!("fake git has no answer for {line}") }),
            })
        });
        block_on(open_changes_as("/r".into(), Mode::Uncommitted, None, true, Some("chat-1".into()))).unwrap();
        let sent = |method: &str| calls.borrow().iter().filter(|call| call.0 == method).map(|call| call.1.clone()).collect::<Vec<_>>();
        assert_eq!(sent("openDiff")[0]["allowNewComments"], true);
        let update = &sent("updateDiff")[0];
        assert_eq!(names(&update["scopes"]), ["uncommitted", "branch", "branch:develop"]);
        // With no chat to send to, or outside the sidebar, comments aren't offered.
        calls.borrow_mut().clear();
        block_on(open_changes_as("/r".into(), Mode::Uncommitted, None, true, None)).unwrap();
        assert!(sent("openDiff")[0]["allowNewComments"].is_null());
        calls.borrow_mut().clear();
        block_on(open_changes_as("/r".into(), Mode::Uncommitted, None, false, Some("chat-1".into()))).unwrap();
        assert!(sent("openDiff")[0]["allowNewComments"].is_null());
    }

    #[test]
    fn a_repo_with_nothing_changed_and_a_file_that_is_no_longer_changed_say_so() {
        let sent = fake(&[("branch --show-current", 0, "main\n"), ("rev-parse --verify --quiet HEAD", 0, "abc\n"), ("diff abc", 0, ""), ("ls-files", 0, "")]);
        block_on(open_changes("/clean".into(), Mode::Uncommitted, None, false)).unwrap();
        assert_eq!(sent.borrow()[0]["error"], "No uncommitted changes.");
        let sent = fake(&[("branch --show-current", 0, "main\n"), ("rev-parse --verify --quiet HEAD", 0, "abc\n"), ("diff abc --numstat", 0, "1\t0\ta.rs\0"), ("diff abc --name-status", 0, "M\0a.rs\0"), ("ls-files", 0, "")]);
        block_on(open_changes("/one".into(), Mode::Uncommitted, Some("gone.rs".into()), false)).unwrap();
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
