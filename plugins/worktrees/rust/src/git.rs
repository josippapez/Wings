//! Everything that runs git for the Worktrees page. The only commands that change anything are `git worktree remove`,
//! `git worktree prune` and `git branch -d`, and none is ever given a force flag.
//!
//! A repo's own settings and hooks can make some git commands run programs: `git status` and `git log` (a filter, the
//! file system monitor, a signature check), `git worktree remove` (it checks for changes first) and `git branch -d` (a
//! hook on the ref). `worktree list`, `rev-parse`, `branch --list` and `worktree prune` run none. A repo that isn't
//! one of your projects, like one a scan found on the disk, may be someone else's, so it is loaded with `check: false`,
//! which runs only the ones that run nothing. Removing one worktree in such a repo is allowed after a warning, since
//! `worktree remove` and `branch -d` can run its programs.

use crate::worktrees::{Facts, Probe, Row, Worktree, describe, gone_branches, lines, parse_worktrees};
use std::{collections::HashMap, future::Future, pin::Pin};
use wings_plugin::{
    git::{default_branch, git, ok, repo_root, run},
    task::{join, join_all, map_limit},
};

/// One repo: its worktrees and what we know about each.
#[derive(Clone, Debug)]
pub struct Repo {
    /// The main worktree's folder, which names the repo and is where its git commands run.
    pub root: String,
    pub list: Vec<Worktree>,
    pub facts: Facts,
    pub probes: HashMap<String, Probe>,
    /// Whether `git status` and `git log` were run in its worktrees. They aren't in a repo outside your projects.
    pub checked: bool,
}

async fn probe(wt: Worktree) -> Probe {
    if wt.prunable.is_some() || wt.bare {
        return Probe { dirty: Some(false), last_commit: None };
    }
    let (status, last) = join(ok(&wt.path, &["status", "--porcelain"]), ok(&wt.path, &["log", "--max-count=1", "--format=%ct"])).await;
    Probe {
        dirty: status.map(|status| !status.is_empty()),
        last_commit: last.filter(|last| !last.is_empty() && last.bytes().all(|b| b.is_ascii_digit())).and_then(|last| last.parse().ok()),
    }
}

type Asked = Pin<Box<dyn Future<Output = Option<String>>>>;
type Loaded = Pin<Box<dyn Future<Output = Result<Repo, String>>>>;

/// Reads one repo from any folder in it. With `check: false` it runs no `git status` or `git log` in its worktrees.
pub async fn load_repo(from: &str, check: bool) -> Result<Repo, String> {
    let list = parse_worktrees(&git(from, &["worktree", "list", "--porcelain"]).await?);
    let root = list.first().map_or(from, |main| &main.path).to_owned();
    let def = default_branch(&root).await;
    let ask = |args: Vec<String>| -> Asked {
        let root = root.clone();
        Box::pin(async move { ok(&root, &args.iter().map(String::as_str).collect::<Vec<_>>()).await })
    };
    let none = || -> Asked { Box::pin(async { None }) };
    let branches = join_all(vec![
        def.as_ref().map_or_else(none, |def| ask(["rev-parse", "--verify", "--quiet", &format!("{def}^{{commit}}")].map(String::from).into())),
        def.as_ref().map_or_else(none, |def| ask(["branch", "--list", &format!("--merged={def}"), "--format=%(refname:short)"].map(String::from).into())),
        ask(["branch", "--list", "--format=%(refname:short)%09%(upstream:track)"].map(String::from).into()),
    ]);
    // A few at a time, so a repo with many worktrees doesn't start them all at once.
    let (mut branches, probed) = join(branches, map_limit(if check { list.clone() } else { Vec::new() }, 6, probe)).await;
    let (gone, merged, tip) = (branches.pop().flatten(), branches.pop().flatten(), branches.pop().flatten());
    Ok(Repo {
        probes: list.iter().map(|wt| wt.path.clone()).zip(probed).collect(),
        facts: Facts { default_branch: def, default_tip: tip.filter(|tip| !tip.is_empty()), merged: lines(&merged.unwrap_or_default()), gone: gone_branches(&gone.unwrap_or_default()) },
        root,
        list,
        checked: check,
    })
}

pub fn rows_of(repo: &Repo, pane_cwds: &[String]) -> Vec<Row> {
    describe(&repo.list, &repo.facts, &repo.probes, pane_cwds, !repo.checked)
}

/// The repos holding these folders, one each however many of their worktrees are open, and what went wrong reading
/// the others. A folder that is in no repo is skipped.
pub async fn load_repos(cwds: Vec<String>, check: bool) -> (Vec<Repo>, Vec<String>) {
    let asked: Vec<Asked> = cwds.into_iter().map(|cwd| -> Asked { Box::pin(async move { repo_root(&cwd).await }) }).collect();
    let mut tops: Vec<String> = join_all(asked).await.into_iter().flatten().collect();
    tops.sort();
    tops.dedup();
    let loads: Vec<Loaded> = tops
        .into_iter()
        .map(|top| -> Loaded { Box::pin(async move { load_repo(&top, check).await.map_err(|error| format!("{top}: {error}")) }) })
        .collect();
    let (mut repos, mut errors) = (Vec::<Repo>::new(), Vec::new());
    for loaded in join_all(loads).await {
        match loaded {
            Ok(repo) if repos.iter().all(|known| known.root != repo.root) => repos.push(repo),
            Ok(_) => {}
            Err(error) => errors.push(error),
        }
    }
    repos.sort_by(|a, b| a.root.cmp(&b.root));
    (repos, errors)
}

#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    pub removed: Vec<String>,
    /// The worktrees that stay, each with why.
    pub failed: Vec<(String, String)>,
    /// Branches `git branch -d` refused or couldn't delete, each with why.
    pub branch_errors: Vec<(String, String)>,
}

pub struct Removal {
    pub delete_branch: bool,
    /// Each also has to be merged still and checked clean, which a repo loaded with `check: false` never is.
    pub only_merged: bool,
    /// How the repo was loaded.
    pub check: bool,
}

/// Removes the worktrees at `paths`, each only if it is still removable right now: the page may be minutes old, and a
/// worktree may have gone dirty or opened in a pane since.
pub async fn remove_worktrees(root: &str, paths: &[String], opts: Removal, pane_cwds: impl Fn() -> Vec<String>) -> Result<Outcome, String> {
    let mut out = Outcome::default();
    let fresh = load_repo(root, opts.check).await?;
    for path in paths {
        let rows = rows_of(&fresh, &pane_cwds());
        let Some(row) = rows.iter().find(|row| &row.wt.path == path) else {
            out.failed.push((path.clone(), "It is no longer listed.".into()));
            continue;
        };
        if !row.removable || (opts.only_merged && !row.bulk) {
            let why = row.why.unwrap_or(if row.unchecked { "Its changes weren't checked, so it isn't removed with the others." } else { "It is no longer merged." });
            out.failed.push((path.clone(), why.into()));
            continue;
        }
        if let Err(error) = git(&fresh.root, &["worktree", "remove", "--", path]).await {
            out.failed.push((path.clone(), error));
            continue;
        }
        out.removed.push(path.clone());
        if let (true, Some(branch)) = (opts.delete_branch, &row.wt.branch)
            && let Err(error) = git(&fresh.root, &["branch", "-d", branch]).await
        {
            out.branch_errors.push((branch.clone(), error));
        }
    }
    Ok(out)
}

/// Forgets the worktrees whose folders are gone, and gives what git said it did.
pub async fn prune(root: &str) -> Result<String, String> {
    let output = run(root, &["worktree", "prune", "--verbose"]).await?;
    if output.code != 0 {
        let said = output.stderr.trim();
        return Err(if said.is_empty() { "git worktree prune failed".into() } else { said.to_owned() });
    }
    Ok(format!("{}{}", output.stderr, output.stdout).trim().to_owned())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::{cell::RefCell, rc::Rc};
    use wings_plugin::{
        git::{forget_default_branch, forget_repo_roots},
        host,
        task::block_on,
    };

    /// A fake git where `/r-x` (branch `x`) is merged and clean, so removing it is offered. `more` adds answers, each a
    /// command line with its exit code and what it prints (stdout when 0, stderr otherwise). Gives what it was asked.
    fn fake_git(more: &'static [(&'static str, i64, &'static str)]) -> Rc<RefCell<Vec<String>>> {
        const ANSWERS: [(&str, i64, &str); 7] = [
            ("worktree list --porcelain", 0, "worktree /r\nHEAD 1\nbranch refs/heads/main\n\nworktree /r-x\nHEAD 2\nbranch refs/heads/x\n"),
            ("rev-parse --abbrev-ref origin/HEAD", 0, "origin/main"),
            ("rev-parse --verify --quiet origin/main^{commit}", 0, "1"),
            ("branch --list --merged=origin/main --format=%(refname:short)", 0, "main\nx"),
            ("branch --list --format=%(refname:short)%09%(upstream:track)", 0, ""),
            ("status --porcelain", 0, ""),
            ("log --max-count=1 --format=%ct", 0, "1000"),
        ];
        forget_repo_roots();
        forget_default_branch(None);
        let calls: Rc<RefCell<Vec<String>>> = Rc::default();
        let seen = calls.clone();
        host::testing::answer(move |_, params| {
            let line = params["args"].as_array().unwrap().iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" ");
            seen.borrow_mut().push(line.clone());
            Ok(match more.iter().chain(&ANSWERS).find(|answer| answer.0 == line) {
                Some((_, 0, stdout)) => json!({ "code": 0, "stdout": stdout, "stderr": "" }),
                Some((_, code, stderr)) => json!({ "code": code, "stdout": "", "stderr": stderr }),
                None => json!({ "code": 1, "stdout": "", "stderr": format!("fake git has no answer for {line}") }),
            })
        });
        calls
    }

    fn remove(opts: Removal) -> Outcome {
        block_on(async move { remove_worktrees("/r", &["/r-x".to_owned()], opts, Vec::new).await }).unwrap()
    }
    const MERGED: Removal = Removal { delete_branch: true, only_merged: true, check: true };

    #[test]
    fn when_git_refuses_to_remove_a_worktree_the_page_shows_its_first_line_not_the_advice_after_it() {
        fake_git(&[("worktree remove -- /r-x", 1, "fatal: cannot remove a locked working tree, lock reason: busy\nuse 'remove -f -f' to override or unlock first\n")]);
        let out = remove(MERGED);
        assert_eq!(out.failed, [("/r-x".to_owned(), "fatal: cannot remove a locked working tree, lock reason: busy".to_owned())]);
        assert!(out.removed.is_empty());
    }

    #[test]
    fn when_git_refuses_to_delete_the_branch_the_worktree_is_still_removed_and_the_page_shows_its_first_line() {
        fake_git(&[("worktree remove -- /r-x", 0, ""), ("branch -d x", 1, "error: the branch 'x' is not fully merged\nhint: If you are sure you want to delete it, run 'git branch -D x'\n")]);
        let out = remove(MERGED);
        assert_eq!(out.removed, ["/r-x"]);
        assert_eq!(out.branch_errors, [("x".to_owned(), "error: the branch 'x' is not fully merged".to_owned())]);
    }

    // A repo's own settings and hooks can make these run programs, so they never run in a repo outside your projects.
    fn runs_programs(call: &&String) -> bool {
        call.starts_with("status") || call.starts_with("log")
    }

    #[test]
    fn a_repo_outside_your_projects_is_read_with_no_git_status_or_git_log_and_says_so() {
        let calls = fake_git(&[]);
        let outside = block_on(async { load_repo("/r", false).await }).unwrap();
        assert!(!outside.checked && outside.probes.is_empty());
        assert!(calls.borrow().iter().all(|call| ["worktree list", "rev-parse", "branch --list"].iter().any(|safe| call.starts_with(safe))), "{:?}", calls.borrow());
        assert_eq!(outside.facts.default_tip.as_deref(), Some("1"));
        assert!(outside.facts.merged.contains("x"));
        // A project's repo is checked, as before.
        let calls = fake_git(&[]);
        let project = block_on(async { load_repo("/r", true).await }).unwrap();
        assert!(project.checked);
        assert_eq!(project.probes.len(), 2);
        assert_eq!(project.probes["/r-x"], Probe { dirty: Some(false), last_commit: Some(1000.0) });
        assert!(calls.borrow().iter().any(|call| runs_programs(&call)));
    }

    #[test]
    fn outside_your_projects_one_worktree_is_removed_on_request_and_several_as_merged_and_clean_never_are() {
        let calls = fake_git(&[("worktree remove -- /r-x", 0, "")]);
        let one = remove(Removal { delete_branch: false, only_merged: false, check: false });
        assert_eq!(one.removed, ["/r-x"]);
        assert!(calls.borrow().iter().any(|call| call == "worktree remove -- /r-x"));
        assert_eq!(calls.borrow().iter().filter(runs_programs).count(), 0);

        let calls = fake_git(&[("worktree remove -- /r-x", 0, "")]);
        let several = remove(Removal { delete_branch: false, only_merged: true, check: false });
        assert!(several.removed.is_empty());
        assert_eq!(several.failed, [("/r-x".to_owned(), "Its changes weren't checked, so it isn't removed with the others.".to_owned())]);
        assert!(!calls.borrow().iter().any(|call| call == "worktree remove -- /r-x"));
    }

    #[test]
    fn a_repo_is_loaded_once_however_many_of_its_folders_are_asked_for_and_one_git_cant_read_is_named() {
        fake_git(&[("rev-parse --show-toplevel", 0, "/r\n")]);
        let (repos, errors) = block_on(load_repos(vec!["/r".into(), "/r/src".into()], true));
        assert_eq!(repos.iter().map(|repo| repo.root.as_str()).collect::<Vec<_>>(), ["/r"]);
        assert!(errors.is_empty());
        fake_git(&[("rev-parse --show-toplevel", 0, "/broken\n"), ("worktree list --porcelain", 128, "fatal: detected dubious ownership in repository\n")]);
        let (repos, errors) = block_on(load_repos(vec!["/broken".into()], true));
        assert!(repos.is_empty());
        assert_eq!(errors, ["/broken: fatal: detected dubious ownership in repository"]);
    }
}
