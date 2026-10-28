//! A new agent in its own git worktree: `git worktree add -b <branch> <path> <base>`, then the UI opens a tab there.
//!
//! Worktrees go in a sibling folder, `<repo>-worktrees/<branch>`, next to the repo and not inside it. Inside
//! (`.worktrees/`) they would show up as untracked files in the repo's own status, get walked by its linters,
//! test runners and file watchers, and need an ignore entry written into the user's repo. A sibling folder
//! leaves the repo untouched, and `git worktree remove` still cleans it up.

use std::{
    io::Write,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
};

use serde::Serialize;

use crate::plugins::find_program;

const BRANCH_MAX: usize = 200;
/// Folders a worktree must never end up in or under: dependencies and build output get wiped and re-created.
const FORBIDDEN_DIRS: [&str; 3] = ["node_modules", "target", ".git"];

/// Why git would refuse `name` as a branch name, or why it can't be turned into a folder name. Stricter than git
/// in one way: a leading `-` and control characters are refused here, so the name can't pass for a git option.
/// The rest follows `git check-ref-format`.
pub fn validate_branch(name: &str) -> Result<(), String> {
    let bad = |why: &str| Err(format!("\"{name}\" isn't a usable branch name: {why}"));
    if name.is_empty() {
        return Err("Give the branch a name".into());
    }
    if name.len() > BRANCH_MAX {
        return bad("it is too long");
    }
    if name.starts_with('-') {
        return bad("it can't start with -");
    }
    if name.starts_with('/') || name.ends_with('/') || name.contains("//") {
        return bad("it can't start or end with /, or hold // ");
    }
    if name.ends_with('.') || name == "@" {
        return bad("it can't end with . or be just @");
    }
    if name.contains("..") || name.contains("@{") {
        return bad("it can't hold .. or @{");
    }
    if let Some(c) = name.chars().find(|c| c.is_control() || c.is_whitespace() || "~^:?*[\\".contains(*c)) {
        return bad(&format!("it can't hold {}", if c.is_whitespace() { "spaces".to_string() } else if c.is_control() { "control characters".to_string() } else { format!("\"{c}\"") }));
    }
    if name.split('/').any(|part| part.starts_with('.') || part.ends_with(".lock")) {
        return bad("no part of it can start with . or end with .lock");
    }
    Ok(())
}

/// The folder name for a branch: `/` and anything odd become `-`, so `wings/fix-login` is `wings-fix-login`.
pub fn slug(branch: &str) -> String {
    let s: String = branch.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '-' }).collect();
    s.trim_matches(['-', '.']).to_string()
}

/// Where the worktree for `branch` goes: `<repo>-worktrees/<slug>` beside the repo.
pub fn worktree_path(root: &Path, branch: &str) -> Result<PathBuf, String> {
    validate_branch(branch)?;
    let name = root.file_name().and_then(|n| n.to_str()).ok_or("The project has no folder name")?;
    let parent = root.parent().ok_or("The project is at the top of the disk, so there's no room beside it")?;
    let slug = slug(branch);
    if slug.is_empty() {
        return Err(format!("\"{branch}\" has no letters or digits to name a folder with"));
    }
    let path = parent.join(format!("{name}-worktrees")).join(slug);
    if let Some(dir) = path.components().filter_map(|c| c.as_os_str().to_str()).find(|c| FORBIDDEN_DIRS.contains(c)) {
        return Err(format!("A worktree can't go in a folder named {dir}"));
    }
    Ok(path)
}

/// A base to branch from. A leading `-` would read as a git option.
pub fn validate_base(base: &str) -> Result<(), String> {
    if base.is_empty() || base.starts_with('-') || base.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(format!("\"{base}\" isn't a branch to start from"));
    }
    Ok(())
}

/// The default branch: what `origin/HEAD` says, else the usual names, first that exists.
pub fn pick_default(origin_head: Option<&str>, exists: impl Fn(&str) -> bool) -> Option<String> {
    origin_head
        .map(str::trim)
        .filter(|r| !r.is_empty() && *r != "origin/HEAD")
        .map(str::to_string)
        .or_else(|| ["origin/main", "origin/master", "main", "master"].into_iter().find(|n| exists(n)).map(str::to_string))
}

/// Branch names from `git for-each-ref`, without the remote's own `HEAD` pointer.
pub fn parse_branches(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && *l != "origin" && *l != "origin/HEAD") {
        if !out.iter().any(|b| b == line) {
            out.push(line.to_string());
        }
    }
    out
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let git = find_program("git").ok_or("git isn't installed, or isn't on your PATH")?;
    let out = Command::new(git)
        .args(args)
        .current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).trim().to_string());
    }
    let text = String::from_utf8_lossy(&out.stderr);
    Err(text.trim().strip_prefix("fatal: ").unwrap_or(text.trim()).to_string())
}

fn git_ok(root: &Path, args: &[&str]) -> bool {
    git(root, args).is_ok()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeInfo {
    pub default_branch: Option<String>,
    pub branches: Vec<String>,
}

/// What the dialog offers for a project: the default branch and the branches to start from.
pub fn info(dir: &Path) -> Result<WorktreeInfo, String> {
    let root = PathBuf::from(git(dir, &["rev-parse", "--show-toplevel"]).map_err(|_| format!("{} isn't a git repository", dir.display()))?);
    let head = git(&root, &["rev-parse", "--abbrev-ref", "origin/HEAD"]).ok();
    let default_branch = pick_default(head.as_deref(), |n| git_ok(&root, &["rev-parse", "--verify", "--quiet", &format!("{n}^{{commit}}")]));
    let refs = git(&root, &["for-each-ref", "--format=%(refname:short)", "--sort=-committerdate", "--count=300", "refs/heads", "refs/remotes/origin"])?;
    Ok(WorktreeInfo { default_branch, branches: parse_branches(&refs) })
}

/// Gitignored local setup an agent needs and a fresh checkout lacks, copied when the project has no
/// `.worktreeinclude` and no list of its own. Not `.claude/settings*.json`: a worktree already reads the main
/// checkout's local settings, and a copy would run its hooks twice.
pub const DEFAULT_INCLUDE: [&str; 9] = [".claude/rules", ".claude/rules-optional", ".claude/agents", ".claude/skills", ".claude/hooks", ".claude/commands", ".mcp.json", ".env", ".env.*"];

pub fn default_include() -> String {
    DEFAULT_INCLUDE.join("\n")
}

/// The patterns in a list: one per line, blank lines and `#` comments dropped.
fn pattern_lines(text: &str) -> Vec<String> {
    text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(str::to_string).collect()
}

/// Whether the repo has its own `.worktreeinclude`, which then wins over the project's list.
pub fn has_include_file(root: &Path) -> bool {
    root.join(".worktreeinclude").is_file()
}

/// What copying the local setup into a new worktree did. Failures here never fail the worktree.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalCopy {
    pub copied: usize,
    /// Already in the worktree, or a link, which isn't followed.
    pub skipped: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Created {
    pub path: PathBuf,
    pub local: LocalCopy,
}

/// Runs git with raw output, optionally feeding `input` to stdin. `ok` lists exit codes that aren't failures.
fn git_bytes(root: &Path, args: &[String], input: Option<&[u8]>, ok: &[i32]) -> Result<Vec<u8>, String> {
    let git = find_program("git").ok_or("git isn't installed, or isn't on your PATH")?;
    let mut child = Command::new(git)
        .args(args)
        .current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        // A thread writes it so a big list can't fill the pipe while git is still writing its answer.
        let input = input.to_vec();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() || out.status.code().is_some_and(|c| ok.contains(&c)) {
        return Ok(out.stdout);
    }
    Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
}

/// Untracked files under `root` that match any of `patterns`, as git's own gitignore matching sees them.
fn matching(root: &Path, patterns: &[&str]) -> Result<Vec<u8>, String> {
    let mut args: Vec<String> = ["ls-files", "-z", "--others", "--ignored"].map(String::from).to_vec();
    args.extend(patterns.iter().map(|p| format!("--exclude={p}")));
    git_bytes(root, &args, None, &[])
}

fn split_z(bytes: &[u8]) -> Vec<String> {
    bytes.split(|b| *b == 0).filter(|p| !p.is_empty()).map(|p| String::from_utf8_lossy(p).into_owned()).collect()
}

/// The files to copy, relative to `root`: untracked files that match a pattern and are also ignored. `-o -i` with
/// only the patterns (as `--exclude`) finds the first set; `check-ignore` keeps the ones the repo's ignore rules
/// cover, so a committed `.env.example` is left alone (a tracked file is never in `ls-files -o` anyway).
/// A `!pattern` takes its files out of the result, wherever it stands in the list. It isn't passed to git as a
/// gitignore negation: git can't re-include a file whose folder an earlier pattern (`.claude/**`) excluded.
fn local_files(root: &Path, patterns: &[String]) -> Result<Vec<String>, String> {
    let (skip, keep): (Vec<&str>, Vec<&str>) = patterns.iter().map(String::as_str).partition(|p| p.starts_with('!'));
    if keep.is_empty() {
        return Ok(Vec::new());
    }
    let matched = matching(root, &keep)?;
    if matched.is_empty() {
        return Ok(Vec::new());
    }
    // Exit code 1 is "none of them are ignored".
    let ignored = git_bytes(root, &["check-ignore".into(), "--stdin".into(), "-z".into()], Some(&matched), &[1])?;
    let mut files = split_z(&ignored);
    if !skip.is_empty() {
        let skip: Vec<&str> = skip.iter().map(|p| p[1..].trim()).filter(|p| !p.is_empty()).collect();
        if !skip.is_empty() {
            let skipped = split_z(&matching(root, &skip)?);
            files.retain(|f| !skipped.contains(f));
        }
    }
    Ok(files)
}

/// Copies the project's gitignored local setup from the main checkout `root` into the new worktree `to`. The
/// patterns come from `root/.worktreeinclude` when it exists, else `custom` (the project's own list), else
/// `DEFAULT_INCLUDE`. Files are copied, not linked, so an edit in a worktree can't change the main checkout's copy;
/// one that's already there stays. The executable bit goes along (`fs::copy` keeps the mode).
pub fn copy_local(root: &Path, to: &Path, custom: Option<&str>) -> LocalCopy {
    let mut out = LocalCopy::default();
    let text = match std::fs::read_to_string(root.join(".worktreeinclude")) {
        Ok(text) => text,
        Err(_) => custom.map(str::to_string).unwrap_or_else(default_include),
    };
    let files = match local_files(root, &pattern_lines(&text)) {
        Ok(files) => files,
        Err(e) => {
            out.errors.push(format!("Couldn't list the local files to copy: {e}"));
            return out;
        }
    };
    for rel in files {
        // git only lists paths inside the repo; this keeps a bad one from writing outside the worktree.
        if Path::new(&rel).components().any(|c| !matches!(c, Component::Normal(_))) {
            out.skipped += 1;
            continue;
        }
        let (src, dest) = (root.join(&rel), to.join(&rel));
        if std::fs::symlink_metadata(&dest).is_ok() || std::fs::symlink_metadata(&src).map_or(true, |m| !m.is_file()) {
            out.skipped += 1;
            continue;
        }
        // A folder on the way that is a symlink, tracked in the repo, would take the copy outside the worktree.
        let parent = dest.parent().unwrap_or(to);
        if let Err(e) = std::fs::create_dir_all(parent) {
            if out.errors.len() < 20 {
                out.errors.push(format!("{rel}: {e}"));
            }
            continue;
        }
        if !within(parent, to) || !within(src.parent().unwrap_or(root), root) {
            out.skipped += 1;
            continue;
        }
        let copied = std::fs::copy(&src, &dest).map(|_| ());
        match copied {
            Ok(()) => out.copied += 1,
            Err(e) if out.errors.len() < 20 => out.errors.push(format!("{rel}: {e}")),
            Err(_) => {}
        }
    }
    out
}

/// Whether `path`, with its symlinks resolved, is inside `root`.
fn within(path: &Path, root: &Path) -> bool {
    matches!((path.canonicalize(), root.canonicalize()), (Ok(p), Ok(r)) if p.starts_with(&r))
}

/// Makes the worktree and returns its folder, with the project's gitignored local setup copied into it (`include`
/// is the project's own pattern list, see `copy_local`). Refuses when the folder or the branch exists. On failure
/// the error is git's own text.
pub fn create(dir: &Path, branch: &str, base: &str, include: Option<&str>) -> Result<Created, String> {
    validate_branch(branch)?;
    validate_base(base)?;
    let root = PathBuf::from(git(dir, &["rev-parse", "--show-toplevel"]).map_err(|_| format!("{} isn't a git repository", dir.display()))?);
    // From a linked worktree, `--show-toplevel` is that worktree: use the main one, so they all end up together.
    let root = match git(&root, &["rev-parse", "--path-format=absolute", "--git-common-dir"]) {
        Ok(common) => Path::new(&common).parent().filter(|p| p.join(".git").exists()).map(Path::to_path_buf).unwrap_or(root),
        Err(_) => root,
    };
    let path = worktree_path(&root, branch)?;
    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }
    if git_ok(&root, &["show-ref", "--verify", "--quiet", &format!("refs/heads/{branch}")]) {
        return Err(format!("A branch named {branch} already exists"));
    }
    if !git_ok(&root, &["rev-parse", "--verify", "--quiet", &format!("{base}^{{commit}}")]) {
        return Err(format!("{base} isn't a branch or commit in this repo"));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Couldn't create {}: {e}", parent.display()))?;
    }
    // `--no-track`: a branch cut from `origin/main` would otherwise push to and pull from main.
    let path_arg = path.to_str().ok_or("The worktree's path isn't valid text")?;
    git(&root, &["worktree", "add", "--no-track", "-b", branch, path_arg, base])?;
    let local = copy_local(&root, &path, include);
    Ok(Created { path, local })
}

/// What the delete dialog shows about a worktree.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeState {
    pub path: String,
    pub branch: Option<String>,
    /// Uncommitted and untracked changes, as `git status --porcelain` lines (gitignored files don't show).
    pub changes: Vec<String>,
}

/// What a delete did: the branch it kept, when git wouldn't delete it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Deleted {
    pub kept_branch: Option<String>,
}

/// The linked worktrees of `repo` as (folder, branch), the main checkout left out.
fn linked(repo: &Path) -> Result<Vec<(PathBuf, Option<String>)>, String> {
    let text = git(repo, &["worktree", "list", "--porcelain"])?;
    let mut out: Vec<(PathBuf, Option<String>)> = Vec::new();
    for block in text.split("\n\n") {
        let mut path = None;
        let mut branch = None;
        for line in block.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                path = Some(PathBuf::from(p));
            } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
                branch = Some(b.to_string());
            }
        }
        if let Some(path) = path {
            out.push((path, branch));
        }
    }
    // The first entry is always the main checkout.
    Ok(out.into_iter().skip(1).collect())
}

/// `path` as a linked worktree of `repo`, or why it isn't one. A main checkout is never one.
fn registered(repo: &Path, path: &Path) -> Result<(PathBuf, Option<String>), String> {
    let want = path.canonicalize().map_err(|_| format!("{} isn't on disk any more", path.display()))?;
    linked(repo)?
        .into_iter()
        .find(|(p, _)| p.canonicalize().map(|p| p == want).unwrap_or(false))
        .map(|(_, branch)| (want, branch))
        .ok_or_else(|| format!("{} isn't a git worktree of {}", path.display(), repo.display()))
}

/// The worktree's branch and uncommitted changes, after checking it's a linked worktree of `repo`.
pub fn inspect(repo: &Path, path: &Path) -> Result<WorktreeState, String> {
    let (path, branch) = registered(repo, path)?;
    let status = git(&path, &["status", "--porcelain"])?;
    Ok(WorktreeState { path: path.to_string_lossy().into_owned(), branch, changes: status.lines().map(str::to_string).collect() })
}

/// Removes the worktree folder, then deletes its branch when git considers it merged (`branch -d`). Refuses a
/// worktree with changes unless `force`, and anything that isn't a linked worktree of `repo`.
pub fn delete(repo: &Path, path: &Path, force: bool) -> Result<Deleted, String> {
    let state = inspect(repo, path)?;
    if !force && !state.changes.is_empty() {
        return Err("The worktree has uncommitted changes".to_string());
    }
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(&state.path);
    git(repo, &args)?;
    let kept_branch = match &state.branch {
        Some(b) if git(repo, &["branch", "-d", b]).is_err() => Some(b.clone()),
        _ => None,
    };
    // `<repo>-worktrees` goes with its last worktree. `remove_dir` leaves a folder that still has something in it.
    if let Some(dir) = Path::new(&state.path).parent().filter(|d| d.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with("-worktrees"))) {
        let _ = std::fs::remove_dir(dir);
    }
    Ok(Deleted { kept_branch })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ordinary_branch_names() {
        for ok in ["wings/fix-login", "feature/x", "a", "release-1.2", "user/JIRA_12/thing"] {
            assert!(validate_branch(ok).is_ok(), "{ok}");
        }
    }

    #[test]
    fn refuses_names_git_or_the_folder_cant_take() {
        for bad in [
            "", "-x", "--force", "/x", "x/", "a//b", "a..b", "a b", "a\tb", "a~1", "a^", "a:b", "a?", "a*", "a[", "a\\b", "x.", "@", "a@{u}", ".hidden", "a/.b", "a/b.lock", "x\u{7f}",
        ] {
            assert!(validate_branch(bad).is_err(), "{bad:?}");
        }
        assert!(validate_branch(&"a".repeat(201)).is_err());
    }

    #[test]
    fn slugs_flatten_slashes() {
        assert_eq!(slug("wings/fix-login"), "wings-fix-login");
        assert_eq!(slug("a/b/c.d"), "a-b-c.d");
        assert_eq!(slug("é/x"), "x");
    }

    #[test]
    fn the_worktree_goes_beside_the_repo() {
        let p = worktree_path(Path::new("/code/app"), "wings/fix-login").unwrap();
        assert_eq!(p, PathBuf::from("/code/app-worktrees/wings-fix-login"));
    }

    #[test]
    fn never_inside_dependency_or_build_folders() {
        assert!(worktree_path(Path::new("/code/node_modules/app"), "x").is_err());
        assert!(worktree_path(Path::new("/code/app/target/inner"), "x").is_err());
        assert!(worktree_path(Path::new("/code/app/.git/x"), "x").is_err());
        assert!(worktree_path(Path::new("/code/app"), "///").is_err());
        assert!(worktree_path(Path::new("/"), "x").is_err());
    }

    #[test]
    fn bases_cant_pass_for_options() {
        assert!(validate_base("origin/main").is_ok());
        assert!(validate_base("-b").is_err());
        assert!(validate_base("a b").is_err());
        assert!(validate_base("").is_err());
    }

    #[test]
    fn a_symlinked_folder_out_of_the_root_is_not_within_it() {
        let base = std::env::temp_dir().join(format!("wings-within-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("root/inside")).unwrap();
        std::fs::create_dir_all(base.join("outside")).unwrap();
        std::os::unix::fs::symlink(base.join("outside"), base.join("root/link")).unwrap();
        assert!(within(&base.join("root/inside"), &base.join("root")));
        assert!(!within(&base.join("root/link"), &base.join("root")));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn default_branch_prefers_origin_head() {
        assert_eq!(pick_default(Some("origin/develop\n"), |_| false).as_deref(), Some("origin/develop"));
        assert_eq!(pick_default(Some("origin/HEAD"), |n| n == "master").as_deref(), Some("master"));
        assert_eq!(pick_default(None, |n| n == "main" || n == "master").as_deref(), Some("main"));
        assert_eq!(pick_default(None, |_| false), None);
    }

    #[test]
    fn branch_lists_drop_the_remote_pointer_and_repeats() {
        assert_eq!(parse_branches("main\norigin\norigin/HEAD\norigin/main\nmain\n\nfeature/x\n"), ["main", "origin/main", "feature/x"]);
    }

    /// A throwaway repo and a real `git worktree add`, in a folder of its own.
    #[test]
    fn creates_a_worktree_and_refuses_repeats() {
        if find_program("git").is_none() {
            return;
        }
        let base = std::env::temp_dir().join(format!("wings-wt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        // git reports real paths: /var is /private/var on macOS.
        let base = base.canonicalize().unwrap();
        let repo = base.join("app");
        std::fs::create_dir_all(&repo).unwrap();
        let run = |args: &[&str]| {
            let ok = Command::new("git")
                .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "init.defaultBranch=main"])
                .args(args)
                .current_dir(&repo)
                .stdout(Stdio::null())
                .status()
                .unwrap()
                .success();
            assert!(ok, "{args:?}");
        };
        run(&["init"]);
        run(&["commit", "--allow-empty", "-m", "first"]);

        let found = info(&repo).unwrap();
        assert_eq!(found.default_branch.as_deref(), Some("main"));

        let path = create(&repo, "wings/one", "main", None).unwrap().path;
        assert_eq!(path, base.join("app-worktrees/wings-one"));
        assert!(path.join(".git").exists());
        assert!(create(&repo, "wings/one", "main", None).unwrap_err().contains("already exists"));
        std::fs::remove_dir_all(&path).unwrap();
        run(&["worktree", "prune"]);
        assert!(create(&repo, "wings/one", "main", None).unwrap_err().contains("branch named wings/one already exists"));
        assert!(create(&repo, "wings/two", "nope", None).unwrap_err().contains("isn't a branch"));
        // From inside a linked worktree the new one still goes beside the main repo.
        let one = create(&repo, "wings/three", "main", None).unwrap().path;
        assert_eq!(create(&one, "wings/four", "main", None).unwrap().path, base.join("app-worktrees/wings-four"));
        assert!(create(&base, "x", "main", None).unwrap_err().contains("isn't a git repository"));
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A repo with ignored local setup, a tracked file the patterns also match, and real worktrees made from it.
    #[test]
    fn copies_ignored_local_setup_into_the_worktree() {
        use std::os::unix::fs::PermissionsExt;
        if find_program("git").is_none() {
            return;
        }
        let base = std::env::temp_dir().join(format!("wings-wt-local-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let repo = base.join("app");
        std::fs::create_dir_all(&repo).unwrap();
        let repo = repo.canonicalize().unwrap();
        let run = |args: &[&str]| {
            let ok = Command::new("git")
                .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "init.defaultBranch=main"])
                .args(args)
                .current_dir(&repo)
                .stdout(Stdio::null())
                .status()
                .unwrap()
                .success();
            assert!(ok, "{args:?}");
        };
        let write = |rel: &str, text: &str| {
            let p = repo.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        write(".gitignore", ".claude/rules/\n.claude/hooks/\n.env*\n!.env.example\nnode_modules/\n");
        write(".env.example", "tracked");
        write("src.txt", "x");
        run(&["init"]);
        run(&["add", "."]);
        run(&["commit", "-m", "first"]);
        write(".env", "A=1");
        write(".env.local", "B=2");
        write(".claude/rules/a.md", "rule");
        write(".claude/hooks/fast.sh", "#!/bin/sh\n");
        write(".claude/hooks/slow.sh", "#!/bin/sh\n");
        write("node_modules/pkg/index.js", "dep");
        std::fs::set_permissions(repo.join(".claude/hooks/fast.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();

        // No .worktreeinclude and no list of its own: the defaults. node_modules and the tracked file stay out.
        let made = create(&repo, "wings/a", "main", None).unwrap();
        assert!(made.local.errors.is_empty(), "{:?}", made.local.errors);
        assert_eq!(made.local.copied, 5);
        let wt = &made.path;
        assert_eq!(std::fs::read_to_string(wt.join(".env.local")).unwrap(), "B=2");
        assert!(wt.join(".claude/rules/a.md").is_file() && wt.join(".claude/hooks/slow.sh").is_file());
        assert!(!wt.join("node_modules").exists());
        // Tracked, so the checkout has it; it wasn't copied over.
        assert_eq!(std::fs::read_to_string(wt.join(".env.example")).unwrap(), "tracked");
        assert_eq!(std::fs::metadata(wt.join(".claude/hooks/fast.sh")).unwrap().permissions().mode() & 0o111, 0o111);

        // The project's own list, with a `!` exclude after a broad include.
        let custom = "# hooks\n.claude/**\n!.claude/hooks/slow.sh\n";
        let made = create(&repo, "wings/b", "main", Some(custom)).unwrap();
        assert!(made.path.join(".claude/hooks/fast.sh").is_file() && made.path.join(".claude/rules/a.md").is_file());
        assert!(!made.path.join(".claude/hooks/slow.sh").exists());
        assert!(!made.path.join(".env").exists());

        // A .worktreeinclude in the repo wins over the project's list; an existing file isn't overwritten.
        write(".worktreeinclude", ".env\n");
        let to = base.join("manual");
        std::fs::create_dir_all(&to).unwrap();
        std::fs::write(to.join(".env"), "mine").unwrap();
        std::fs::write(to.join("keep"), "").unwrap();
        let out = copy_local(&repo, &to, Some(".env.local"));
        assert_eq!((out.copied, out.skipped), (0, 1));
        assert_eq!(std::fs::read_to_string(to.join(".env")).unwrap(), "mine");
        assert!(!to.join(".env.local").exists());
        assert!(has_include_file(&repo));
        let made = create(&repo, "wings/c", "main", Some(".env.local")).unwrap();
        assert!(made.path.join(".env").is_file() && !made.path.join(".env.local").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn deletes_a_worktree_and_its_merged_branch_only_when_safe() {
        if find_program("git").is_none() {
            return;
        }
        let base = std::env::temp_dir().join(format!("wings-wt-delete-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let repo = base.join("app");
        std::fs::create_dir_all(&repo).unwrap();
        let repo = repo.canonicalize().unwrap();
        let run = |dir: &Path, args: &[&str]| {
            let ok = Command::new("git")
                .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "init.defaultBranch=main"])
                .args(args)
                .current_dir(dir)
                .stdout(Stdio::null())
                .status()
                .unwrap()
                .success();
            assert!(ok, "{args:?}");
        };
        run(&repo, &["init"]);
        run(&repo, &["commit", "--allow-empty", "-m", "first"]);
        let has_branch = |b: &str| git_ok(&repo, &["show-ref", "--verify", "--quiet", &format!("refs/heads/{b}")]);

        // A main checkout, or a folder that isn't a worktree, is refused.
        assert!(delete(&repo, &repo, true).unwrap_err().contains("isn't a git worktree"));
        assert!(repo.exists());
        let stray = base.join("stray");
        std::fs::create_dir_all(&stray).unwrap();
        assert!(delete(&repo, &stray, true).is_err());
        assert!(stray.exists());

        // Clean: the folder, the branch and the then-empty `app-worktrees` go.
        let one = create(&repo, "wings/one", "main", None).unwrap().path;
        assert!(inspect(&repo, &one).unwrap().changes.is_empty());
        assert_eq!(delete(&repo, &one, false).unwrap().kept_branch, None);
        assert!(!one.exists() && !has_branch("wings/one") && !base.join("app-worktrees").exists());

        // Dirty: refused without force, deleted with it.
        let two = create(&repo, "wings/two", "main", None).unwrap().path;
        std::fs::write(two.join("note.txt"), "x").unwrap();
        assert_eq!(inspect(&repo, &two).unwrap().changes.len(), 1);
        assert!(delete(&repo, &two, false).unwrap_err().contains("uncommitted"));
        assert!(two.exists());
        assert_eq!(delete(&repo, &two, true).unwrap().kept_branch, None);
        assert!(!two.exists() && !has_branch("wings/two"));

        // Unmerged: the folder goes, the branch stays and is reported.
        let three = create(&repo, "wings/three", "main", None).unwrap().path;
        run(&three, &["commit", "--allow-empty", "-m", "work"]);
        assert_eq!(delete(&repo, &three, false).unwrap().kept_branch.as_deref(), Some("wings/three"));
        assert!(!three.exists() && has_branch("wings/three"));
        let _ = std::fs::remove_dir_all(&base);
    }
}
