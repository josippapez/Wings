//! "Elsewhere on this Mac": repos outside your projects that have worktrees. A scan asks Wings for every `.git` on
//! every disk, asks git which of those repos have more than one worktree, and keeps their folders in storage, so they
//! are there after a restart. The page loads those folders like any other repo, with git only: the disks are walked
//! again by a scan alone.
//!
//! These repos aren't your projects, so they may be someone else's, and a repo's own settings and hooks can make some
//! git commands run programs (see `git.rs`). Nothing here runs more than `worktree list`.

use crate::{
    git::Repo,
    worktrees::{has_worktrees, parse_worktrees},
};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    rc::Rc,
};
use wings_plugin::{
    git::ok,
    host::{self, Stopper, storage},
    task::map_limit,
};

/// What the last scan left: when it finished, in ms since 1970 (0 before the first), and the main folders of the repos
/// it found with worktrees.
#[derive(Debug, Default, PartialEq)]
pub struct Saved {
    pub at: f64,
    pub folders: Vec<String>,
}

const KEY: &str = "elsewhere";

/// Anything else that is stored under the key is as good as no scan.
pub fn check(saved: &Value) -> Option<Saved> {
    Some(Saved { at: saved["at"].as_f64()?, folders: saved["folders"].as_array()?.iter().filter_map(Value::as_str).map(str::to_owned).collect() })
}

pub fn load() -> Saved {
    storage::get(KEY).ok().and_then(|saved| check(&saved)).unwrap_or_default()
}

pub fn save(saved: &Saved) -> Result<(), String> {
    storage::set(KEY, &json!({ "at": saved.at, "folders": saved.folders }))
}

/// The folder a `.git` is in: a repo's own, or a linked worktree's.
pub fn folder_of(git_path: &str) -> &str {
    match git_path.rfind('/') {
        Some(0) | None => "/",
        Some(at) => &git_path[..at],
    }
}

/// What a scan doesn't open. Packages are never a project of yours, and the Trash holds what you threw away.
const SKIP: [&str; 2] = ["node_modules", ".Trash"];
const STOPPED: &str = "The scan was stopped";

/// A scan that is running, which the Stop button ends.
#[derive(Default)]
pub struct Scan {
    stopped: Cell<bool>,
    /// The disk search that is running now.
    search: RefCell<Option<Stopper>>,
}

impl Scan {
    pub fn stop(&self) {
        self.stopped.set(true);
        if let Some(search) = self.search.borrow().as_ref() {
            search.stop();
        }
    }

    pub fn stopped(&self) -> bool {
        self.stopped.get()
    }
}

/// The folders of the repos and worktrees on every disk, found by looking for `.git` on each disk in turn. A disk
/// that is read-only still counts: the startup disk is, and it reaches your own folders. `on_looked` hears how many
/// files and folders have been looked at so far, over all the disks.
pub async fn find_repo_folders(scan: &Scan, on_looked: impl Fn(u64) + 'static) -> Result<Vec<String>, String> {
    let mut folders = BTreeSet::new();
    let (before, on_looked) = (Rc::new(Cell::new(0)), Rc::new(on_looked));
    for disk in host::disks().await?.as_array().into_iter().flatten() {
        if scan.stopped() {
            return Err(STOPPED.into());
        }
        let (looked, counted) = (on_looked.clone(), before.clone());
        let search = host::disk_find(disk["mount"].as_str().unwrap_or_default(), &[".git"], &SKIP, move |progress| looked(counted.get() + progress["entries"].as_u64().unwrap_or(0)));
        *scan.search.borrow_mut() = Some(search.stopper());
        let found = search.await;
        *scan.search.borrow_mut() = None;
        let found = found?;
        before.set(before.get() + found["entries"].as_u64().unwrap_or(0));
        folders.extend(found["paths"].as_array().into_iter().flatten().filter_map(Value::as_str).map(|path| folder_of(path).to_owned()));
    }
    on_looked(before.get());
    Ok(folders.into_iter().collect())
}

/// The main folders of the repos among `folders` that have worktrees to manage, once each however many of their
/// worktrees were found, so a page doesn't load hundreds of repos that have only their own checkout. A folder git
/// can't read is left out. Fails once the scan is stopped.
pub async fn repos_with_worktrees(folders: Vec<String>, scan: &Scan) -> Result<Vec<String>, String> {
    let found = map_limit(folders, 8, async |folder| {
        if scan.stopped() {
            return None;
        }
        let list = parse_worktrees(&ok(&folder, &["worktree", "list", "--porcelain"]).await.unwrap_or_default());
        has_worktrees(&list).then(|| list[0].path.clone())
    })
    .await;
    if scan.stopped() {
        return Err(STOPPED.into());
    }
    Ok(found.into_iter().flatten().collect::<BTreeSet<_>>().into_iter().collect())
}

/// The repos to list as elsewhere: the ones with worktrees that aren't `above` already, which are your projects' and
/// your panes'.
pub fn elsewhere_repos<'a>(found: &'a [Repo], above: &[Repo]) -> Vec<&'a Repo> {
    found.iter().filter(|repo| above.iter().all(|shown| shown.root != repo.root) && has_worktrees(&repo.list)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktrees::Facts;
    use wings_plugin::task::block_on;

    #[test]
    fn a_scan_looks_for_git_on_each_disk_in_turn_and_keeps_the_folder_of_each_once_sorted() {
        let calls: Rc<RefCell<Vec<Value>>> = Rc::default();
        let seen = calls.clone();
        host::testing::answer(move |method, params| {
            Ok(match (method, params["path"].as_str()) {
                ("disks", _) => json!([{ "mount": "/" }, { "mount": "/Volumes/Backup" }]),
                ("diskFind", Some("/")) => {
                    seen.borrow_mut().push(params.clone());
                    json!({ "paths": ["/a/.git", "/a/wt/.git", "/b/.git"], "entries": 100 })
                }
                ("diskFind", _) => {
                    seen.borrow_mut().push(params.clone());
                    json!({ "paths": ["/Volumes/Backup/c/.git", "/a/.git"], "entries": 40 })
                }
                _ => Value::Null,
            })
        });
        let looked: Rc<Cell<u64>> = Rc::default();
        let total = looked.clone();
        let folders = block_on(async move { find_repo_folders(&Scan::default(), move |count| total.set(count)).await }).unwrap();
        assert_eq!(folders, ["/Volumes/Backup/c", "/a", "/a/wt", "/b"]);
        // What the second disk looked at adds to what the first did.
        assert_eq!(looked.get(), 140);
        let calls = calls.borrow();
        assert_eq!(calls.iter().map(|call| call["path"].as_str().unwrap()).collect::<Vec<_>>(), ["/", "/Volumes/Backup"]);
        assert!(calls.iter().all(|call| call["names"] == json!([".git"]) && call["skip"] == json!(["node_modules", ".Trash"]) && call["stream"] == true));
        assert_eq!(folder_of("/.git"), "/");

        let stopped = Scan::default();
        stopped.stop();
        assert_eq!(block_on(async move { find_repo_folders(&stopped, |_| {}).await }), Err(STOPPED.into()));
    }

    #[test]
    fn only_repos_with_worktrees_to_manage_are_kept_once_each_however_many_of_their_folders_were_found() {
        fn block(path: &str, extra: &str) -> String {
            format!("worktree {path}\nHEAD 1\nbranch refs/heads/b\n{extra}")
        }
        host::testing::answer(|_, params| {
            let pair = format!("{}\n{}", block("/r", ""), block("/r-x", ""));
            Ok(match params["cwd"].as_str().unwrap() {
                // `/r-x` was found as a worktree: git says the same of it.
                "/r" | "/r-x" => json!({ "code": 0, "stdout": pair, "stderr": "" }),
                "/solo" => json!({ "code": 0, "stdout": block("/solo", ""), "stderr": "" }),
                "/gone" => json!({ "code": 0, "stdout": format!("{}\n{}", block("/gone", ""), block("/gone-old", "prunable gitdir file points to non-existent location\n")), "stderr": "" }),
                _ => json!({ "code": 128, "stdout": "", "stderr": "fatal: detected dubious ownership in repository" }),
            })
        });
        let folders = || ["/r", "/r-x", "/solo", "/gone", "/broken"].map(String::from).to_vec();
        assert_eq!(block_on(async move { repos_with_worktrees(folders(), &Scan::default()).await }), Ok(vec!["/gone".to_owned(), "/r".to_owned()]));
        let stopped = Scan::default();
        stopped.stop();
        assert_eq!(block_on(async move { repos_with_worktrees(vec!["/r".into()], &stopped).await }), Err(STOPPED.into()));
    }

    #[test]
    fn what_is_stored_counts_only_when_it_has_the_shape_of_a_scan() {
        assert_eq!(check(&json!({ "at": 5, "folders": ["/a", 3, "/b"] })), Some(Saved { at: 5.0, folders: vec!["/a".into(), "/b".into()] }));
        for junk in [json!(null), json!(4), json!("x"), json!({}), json!({ "at": "5", "folders": [] }), json!({ "at": 5 }), json!({ "at": 5, "folders": "/a" })] {
            assert_eq!(check(&junk), None, "{junk}");
        }
    }

    #[test]
    fn a_repo_is_listed_as_elsewhere_when_it_has_worktrees_and_isnt_one_of_the_repos_shown_above() {
        let repo = |paths: &[&str]| Repo {
            root: paths[0].to_owned(),
            list: parse_worktrees(&paths.iter().map(|path| format!("worktree {path}\nHEAD 1\n")).collect::<Vec<_>>().join("\n")),
            facts: Facts::default(),
            probes: Default::default(),
            checked: false,
        };
        let found = [repo(&["/a", "/a-x"]), repo(&["/b"]), repo(&["/c", "/c-x"]), repo(&["/d", "/d-x"])];
        assert_eq!(elsewhere_repos(&found, &[repo(&["/c"]), repo(&["/e"])]).iter().map(|repo| repo.root.as_str()).collect::<Vec<_>>(), ["/a", "/d"]);
    }
}
