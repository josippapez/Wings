//! A new agent in its own git worktree: `git worktree add -b <branch> <path> <base>`, then the UI opens a tab there.
//!
//! Worktrees go in a sibling folder, `<repo>-worktrees/<branch>`, next to the repo and not inside it. Inside
//! (`.worktrees/`) they would show up as untracked files in the repo's own status, get walked by its linters,
//! test runners and file watchers, and need an ignore entry written into the user's repo. A sibling folder
//! leaves the repo untouched, and `git worktree remove` still cleans it up.

use std::{
    path::{Path, PathBuf},
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

/// Makes the worktree and returns its folder. Refuses when the folder or the branch exists. On failure the
/// error is git's own text.
pub fn create(dir: &Path, branch: &str, base: &str) -> Result<PathBuf, String> {
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
    Ok(path)
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

        let path = create(&repo, "wings/one", "main").unwrap();
        assert_eq!(path, base.join("app-worktrees/wings-one"));
        assert!(path.join(".git").exists());
        assert!(create(&repo, "wings/one", "main").unwrap_err().contains("already exists"));
        std::fs::remove_dir_all(&path).unwrap();
        run(&["worktree", "prune"]);
        assert!(create(&repo, "wings/one", "main").unwrap_err().contains("branch named wings/one already exists"));
        assert!(create(&repo, "wings/two", "nope").unwrap_err().contains("isn't a branch"));
        // From inside a linked worktree the new one still goes beside the main repo.
        let one = create(&repo, "wings/three", "main").unwrap();
        assert_eq!(create(&one, "wings/four", "main").unwrap(), base.join("app-worktrees/wings-four"));
        assert!(create(&base, "x", "main").unwrap_err().contains("isn't a git repository"));
        let _ = std::fs::remove_dir_all(&base);
    }
}
