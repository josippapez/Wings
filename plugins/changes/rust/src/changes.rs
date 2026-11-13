//! The changed files of a repo.

use std::{cell::RefCell, collections::HashMap, rc::Rc};
use wings_plugin::{
    format::basename,
    git::{branch_label, default_branch, forget_default_branch, forget_repo_roots, git, ok, raw, run},
    host,
    task::join,
};

/// Git's empty tree, the base of a repo with no commits yet.
pub const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
pub const COLOR: [&str; 4] = ["--no-color", "--no-ext-diff", "--no-textconv", "--no-relative"];

/// Pathspecs that match these exact paths, not as globs.
pub fn literal<'a>(paths: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    paths.into_iter().map(|path| format!(":(literal){path}")).collect()
}

/// `git` with `fixed` arguments and then `more`.
pub fn args<'a>(fixed: &[&'a str], more: &'a [String]) -> Vec<&'a str> {
    fixed.iter().copied().chain(more.iter().map(String::as_str)).collect()
}

/// For Refresh: the next look at a folder asks git again which repo it is in and which branch is its default.
pub fn forget_repos() {
    forget_repo_roots();
    forget_default_branch(None);
}

/// A result this fresh is reused, so the badge poll and a tool don't run git twice.
const FRESH_MS: f64 = 3_000.0;
/// Untracked files read through `git grep`, which takes their paths as arguments.
pub const MAX_UNTRACKED_READ: usize = 200;

/// What the files are against: HEAD, or where this branch left the default branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    Uncommitted,
    Branch,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Self::Uncommitted => "uncommitted",
            Self::Branch => "branch",
        }
    }

    pub fn named(name: &str) -> Self {
        if name == "branch" { Self::Branch } else { Self::Uncommitted }
    }
}

/// `added` and `removed` are none for binary files, and `added` for untracked files git couldn't read as text.
#[derive(Clone, Debug, PartialEq)]
pub struct FileChange {
    pub path: String,
    pub old_path: Option<String>,
    pub status: String,
    pub added: Option<u64>,
    pub removed: Option<u64>,
    pub untracked: bool,
}

#[derive(Clone, Debug)]
pub struct Changes {
    pub root: String,
    pub name: String,
    pub branch: String,
    pub mode: Mode,
    pub base: String,
    /// What the files are against, like "Working tree against HEAD".
    pub against: String,
    pub files: Vec<FileChange>,
    pub added: u64,
    pub removed: u64,
    pub partial: bool,
}

/// Splits `git diff --numstat -z` and `--name-status -z` into one entry per file.
fn parse_diff(numstat: &str, names: &str) -> Vec<FileChange> {
    let mut files: Vec<FileChange> = Vec::new();
    let n: Vec<&str> = names.split('\0').collect();
    let mut i = 0;
    while i < n.len() && !n[i].is_empty() {
        let status = &n[i][..1];
        let two = status == "R" || status == "C";
        let (path, old_path) = if two { (n.get(i + 2), n.get(i + 1)) } else { (n.get(i + 1), None) };
        let Some(path) = path else { break };
        files.retain(|file| file.path != *path);
        files.push(FileChange { path: (*path).to_owned(), old_path: old_path.map(|old| (*old).to_owned()), status: status.to_owned(), added: Some(0), removed: Some(0), untracked: false });
        i += if two { 3 } else { 2 };
    }
    let s: Vec<&str> = numstat.split('\0').collect();
    let mut i = 0;
    while i < s.len() && !s[i].is_empty() {
        let mut fields = s[i].splitn(3, '\t');
        let (added, removed, path) = (fields.next().unwrap_or_default(), fields.next().unwrap_or_default(), fields.next().unwrap_or_default());
        // A rename has an empty path, then its old and new paths as their own fields.
        let (path, step) = if path.is_empty() { (s.get(i + 2).copied().unwrap_or_default(), 3) } else { (path, 1) };
        i += step;
        if let Some(file) = files.iter_mut().find(|file| file.path == path) {
            file.added = added.parse().ok();
            file.removed = removed.parse().ok();
        }
    }
    files
}

/// Line counts of untracked text files, from `git grep --count`. Binary and empty files are left out.
async fn line_counts(root: &str, paths: &[String]) -> Result<HashMap<String, u64>, String> {
    let mut counts = HashMap::new();
    if paths.is_empty() {
        return Ok(counts);
    }
    let paths = literal(paths.iter().map(String::as_str));
    let output = run(root, &args(&["grep", "--untracked", "-I", "-z", "--count", "--no-color", "-e", "", "--"], &paths)).await?;
    // 1 is "no match": every file was empty or binary.
    if output.code > 1 {
        let said = output.stderr.trim();
        return Err(if said.is_empty() { "git grep failed".into() } else { said.to_owned() });
    }
    for line in output.stdout.split('\n') {
        if let Some((path, count)) = line.rsplit_once('\0').filter(|(path, _)| !path.is_empty()) {
            counts.insert(path.to_owned(), count.parse().unwrap_or(0));
        }
    }
    Ok(counts)
}

/// The changed files of a repo: against HEAD, or against where this branch left the default branch.
async fn load(root: &str, mode: Mode) -> Result<Changes, String> {
    let (branch, head) = join(ok(root, &["branch", "--show-current"]), ok(root, &["rev-parse", "--verify", "--quiet", "HEAD"])).await;
    let head = head.filter(|head| !head.is_empty());
    let mut base = head.clone().unwrap_or_else(|| EMPTY_TREE.to_owned());
    let mut against = "Working tree against HEAD".to_owned();
    if mode == Mode::Branch {
        let name = default_branch(root).await.ok_or("No default branch found (origin/HEAD, main or master).")?;
        if head.is_none() {
            return Err("This repo has no commits yet.".into());
        }
        base = git(root, &["merge-base", "HEAD", &name]).await?;
        against = format!("Since {name} at {}", base.chars().take(7).collect::<String>());
    }
    let numstat = [&["diff", &base, "--numstat", "-z", "-M"][..], &COLOR].concat();
    let names = [&["diff", &base, "--name-status", "-z", "-M"][..], &COLOR].concat();
    let ((numstat, names), others) = join(join(raw(root, &numstat), raw(root, &names)), raw(root, &["ls-files", "--others", "--exclude-standard", "-z"])).await;
    let mut files = parse_diff(&numstat?, &names?);
    let untracked: Vec<String> = others?.split('\0').filter(|path| !path.is_empty()).map(str::to_owned).collect();
    let counts = line_counts(root, &untracked[..untracked.len().min(MAX_UNTRACKED_READ)]).await?;
    for path in &untracked {
        files.push(FileChange { path: path.clone(), old_path: None, status: "?".into(), added: counts.get(path).copied(), removed: Some(0), untracked: true });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Changes {
        root: root.to_owned(),
        name: basename(root).to_owned(),
        branch: branch_label(branch.as_deref(), head.as_deref()),
        mode,
        base,
        against,
        added: files.iter().filter_map(|file| file.added).sum(),
        removed: files.iter().filter_map(|file| file.removed).sum(),
        files,
        partial: untracked.len() > MAX_UNTRACKED_READ,
    })
}

/// By repo and mode: when they were read, and what was.
type Kept = HashMap<(String, Mode), (f64, Rc<Changes>)>;

thread_local! {
    static KEPT: RefCell<Kept> = RefCell::new(HashMap::new());
}

/// Kept for a moment, unless `fresh`.
pub async fn changes(root: &str, mode: Mode, fresh: bool) -> Result<Rc<Changes>, String> {
    let key = (root.to_owned(), mode);
    if !fresh && let Some(kept) = KEPT.with(|kept| kept.borrow().get(&key).filter(|(at, _)| host::now() - at < FRESH_MS).map(|(_, changes)| changes.clone())) {
        return Ok(kept);
    }
    let loaded = Rc::new(load(root, mode).await?);
    KEPT.with(|kept| kept.borrow_mut().insert(key, (host::now(), loaded.clone())));
    Ok(loaded)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::{Value, json};
    use wings_plugin::task::block_on;

    /// A fake git that answers a command line starting with one of `answers` with its exit code and stdout, and gives
    /// what it was asked.
    pub fn fake_git(answers: &'static [(&'static str, i64, &'static str)]) -> Rc<RefCell<Vec<String>>> {
        forget_repos();
        let calls: Rc<RefCell<Vec<String>>> = Rc::default();
        let seen = calls.clone();
        host::testing::answer(move |_, params| {
            let line = params["args"].as_array().unwrap().iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" ");
            seen.borrow_mut().push(line.clone());
            Ok(match answers.iter().find(|answer| line.starts_with(answer.0)) {
                Some((_, code, stdout)) => json!({ "code": code, "stdout": stdout, "stderr": "" }),
                None => json!({ "code": 1, "stdout": "", "stderr": format!("fake git has no answer for {line}") }),
            })
        });
        calls
    }

    #[test]
    fn a_diff_reads_as_one_entry_a_file_with_renames_binaries_and_new_files() {
        fake_git(&[
            ("branch --show-current", 0, "main\n"),
            ("rev-parse --verify --quiet HEAD", 0, "abc1234def\n"),
            ("diff abc1234def --numstat", 0, "3\t1\tsrc/a.rs\0-\t-\timg.png\x005\t0\t\0old.rs\0new.rs\0"),
            ("diff abc1234def --name-status", 0, "M\0src/a.rs\0M\0img.png\0R100\0old.rs\0new.rs\0"),
            ("ls-files", 0, "notes.txt\0empty.bin\0"),
            ("grep --untracked -I -z --count", 0, "notes.txt\x004\n"),
        ]);
        let c = block_on(async { changes("/r", Mode::Uncommitted, true).await }).unwrap();
        let rows: Vec<_> = c.files.iter().map(|f| (f.status.as_str(), f.path.as_str(), f.added, f.removed, f.old_path.as_deref())).collect();
        assert_eq!(
            rows,
            [
                ("?", "empty.bin", None, Some(0), None),
                ("M", "img.png", None, None, None),
                ("R", "new.rs", Some(5), Some(0), Some("old.rs")),
                ("?", "notes.txt", Some(4), Some(0), None),
                ("M", "src/a.rs", Some(3), Some(1), None),
            ]
        );
        assert_eq!((c.added, c.removed, c.partial), (12, 1, false));
        assert_eq!((c.branch.as_str(), c.against.as_str(), c.name.as_str()), ("main", "Working tree against HEAD", "r"));
    }

    #[test]
    fn changes_on_the_branch_are_since_it_left_the_default_branch_and_a_fresh_answer_is_reused() {
        let calls = fake_git(&[
            ("branch --show-current", 0, "feature\n"),
            ("rev-parse --verify --quiet HEAD", 0, "abc\n"),
            ("rev-parse --abbrev-ref origin/HEAD", 0, "origin/main\n"),
            ("merge-base HEAD origin/main", 0, "1234567890\n"),
            ("diff 1234567890", 0, ""),
            ("ls-files", 0, ""),
        ]);
        let c = block_on(async { changes("/r", Mode::Branch, false).await }).unwrap();
        assert_eq!((c.against.as_str(), c.files.len()), ("Since origin/main at 1234567", 0));
        let asked = calls.borrow().len();
        block_on(async { changes("/r", Mode::Branch, false).await }).unwrap();
        assert_eq!(calls.borrow().len(), asked);
        block_on(async { changes("/r", Mode::Branch, true).await }).unwrap();
        assert!(calls.borrow().len() > asked);
        fake_git(&[("branch --show-current", 0, "main\n")]);
        assert_eq!(block_on(async { changes("/new", Mode::Branch, true).await }).unwrap_err(), "No default branch found (origin/HEAD, main or master).");
    }
}
