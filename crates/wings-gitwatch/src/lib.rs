//! Tells an app which projects' git repos changed as soon as they do, instead of waiting for its next poll.
//! It watches the few places git writes when HEAD, the index or a ref moves (a pull, commit, checkout,
//! fetch or push, from a pane or from anywhere else), waits for the writes to settle, then reports the
//! projects that repo belongs to. Edits to files in the working tree are not watched.

use notify::{EventKind, RecursiveMode, Watcher as _};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

/// Quiet time after the last write before refreshing: a pull touches refs, FETCH_HEAD, HEAD and the index
/// over a few hundred milliseconds, and the status should describe the finished state.
const SETTLE: Duration = Duration::from_millis(250);
/// A busy repo (a long rebase) still refreshes at least this often.
const MAX_WAIT: Duration = Duration::from_secs(1);

enum Msg {
    Changed(PathBuf),
    Resync,
    Stop,
}

/// Where git keeps a project's state: its own dir (HEAD, index) and the common dir shared by linked
/// worktrees (refs, packed-refs). They are the same for a normal repo. `None` outside a repo.
fn git_dirs(dir: &Path) -> Option<(PathBuf, PathBuf)> {
    let dot_git = dir.ancestors().map(|d| d.join(".git")).find(|p| p.exists())?;
    let git_dir = if dot_git.is_file() {
        let text = fs::read_to_string(&dot_git).ok()?;
        dot_git.parent()?.join(text.trim().strip_prefix("gitdir:")?.trim())
    } else {
        dot_git
    };
    let common = match fs::read_to_string(git_dir.join("commondir")) {
        Ok(rel) => git_dir.join(rel.trim()),
        Err(_) => git_dir.clone(),
    };
    // FSEvents reports real paths, so compare against real paths.
    Some((git_dir.canonicalize().ok()?, common.canonicalize().ok()?))
}

/// Whether a write at `path` can change what the status shows.
fn relevant(path: &Path, git_dir: &Path, common: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.ends_with(".lock") {
        return false;
    }
    if path.starts_with(common.join("refs")) {
        return true;
    }
    let top = |root: &Path| path.parent() == Some(root);
    (top(git_dir) && matches!(name, "HEAD" | "index" | "FETCH_HEAD" | "ORIG_HEAD" | "MERGE_HEAD")) || (top(common) && name == "packed-refs")
}

struct Repo {
    id: String,
    git_dir: PathBuf,
    common: PathBuf,
}

/// Runs the watcher until the channel closes or a `Msg::Stop` arrives. `spaces` lists (id, path) of the projects, `refresh` is
/// called with the ids whose repo changed.
fn run(tx: Sender<Msg>, rx: Receiver<Msg>, spaces: impl Fn() -> Vec<(String, PathBuf)>, refresh: impl Fn(Vec<String>)) {
    let Ok(mut watcher) = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            // Reading a file is not changing it; git status reads the index, and reacting to that would loop.
            if !matches!(event.kind, EventKind::Access(_)) {
                for path in event.paths {
                    let _ = tx.send(Msg::Changed(path));
                }
            }
        }
    }) else {
        return;
    };
    let mut repos: Vec<Repo> = Vec::new();
    let mut watched: HashSet<(PathBuf, bool)> = HashSet::new();
    let mut pending: HashSet<String> = HashSet::new();
    let mut first = Instant::now();
    let mut last = first;
    let mut resync = true;
    loop {
        if resync {
            resync = false;
            repos = spaces().into_iter().filter_map(|(id, path)| git_dirs(&path).map(|(git_dir, common)| Repo { id, git_dir, common })).collect();
            let mut wanted: HashSet<(PathBuf, bool)> = HashSet::new();
            for r in &repos {
                wanted.insert((r.git_dir.clone(), false));
                wanted.insert((r.common.clone(), false));
                wanted.insert((r.common.join("refs"), true));
            }
            for (path, _) in watched.difference(&wanted) {
                let _ = watcher.unwatch(path);
            }
            for (path, recursive) in wanted.difference(&watched) {
                let mode = if *recursive { RecursiveMode::Recursive } else { RecursiveMode::NonRecursive };
                let _ = watcher.watch(path, mode);
            }
            watched = wanted;
        }
        let msg = if pending.is_empty() {
            match rx.recv() {
                Ok(m) => Some(m),
                Err(_) => return,
            }
        } else {
            let deadline = (last + SETTLE).min(first + MAX_WAIT);
            match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(m) => Some(m),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        };
        match msg {
            Some(Msg::Resync) => resync = true,
            Some(Msg::Stop) => return,
            Some(Msg::Changed(path)) => {
                let hits: Vec<&Repo> = repos.iter().filter(|r| relevant(&path, &r.git_dir, &r.common)).collect();
                if !hits.is_empty() {
                    if pending.is_empty() {
                        first = Instant::now();
                    }
                    last = Instant::now();
                    pending.extend(hits.into_iter().map(|r| r.id.clone()));
                }
            }
            None => refresh(pending.drain().collect()),
        }
    }
}

/// A watcher thread over a list of projects. Dropping it stops the thread.
pub struct Watcher {
    tx: Sender<Msg>,
}

impl Watcher {
    /// `spaces` lists (id, path) of the projects whenever asked (on start and after `resync`), `changed` is called from the
    /// watcher thread with the ids whose repo changed, one call per settled batch.
    pub fn start(spaces: impl Fn() -> Vec<(String, PathBuf)> + Send + 'static, changed: impl Fn(Vec<String>) + Send + 'static) -> Watcher {
        let (tx, rx) = mpsc::channel();
        let own = tx.clone();
        thread::spawn(move || run(own, rx, spaces, changed));
        Watcher { tx }
    }

    /// Tells the watcher the list of projects changed.
    pub fn resync(&self) {
        let _ = self.tx.send(Msg::Resync);
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        // `run` keeps a sender for the file events, so the channel never closes on its own.
        let _ = self.tx.send(Msg::Stop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::sync::{Arc, Mutex};

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("/usr/bin/git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?} failed");
    }

    type Calls = Arc<Mutex<Vec<(Instant, Vec<String>)>>>;

    /// How long a condition may take before a test fails. A loaded machine waits longer, a fast one doesn't.
    const DEADLINE: Duration = Duration::from_secs(10);
    /// Quiet time that proves no further refresh is coming: four `SETTLE` windows, so a second refresh for the
    /// same write would land inside it.
    const QUIET: Duration = Duration::from_millis(1000);

    /// Polls `done` every 20 ms until it holds or `within` passes. Returns whether it held.
    fn wait_for(within: Duration, mut done: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + within;
        while !done() {
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(20));
        }
        true
    }

    fn count(calls: &Calls) -> usize {
        calls.lock().unwrap().len()
    }

    /// Starts the watcher on one repo and returns the refresh calls it makes, with the time each arrived.
    /// The watcher starts on its own thread and only sees writes once it has installed its watches, so
    /// `probe` (a write to the repo) is repeated until the watcher reports one. Those refreshes are cleared.
    fn watch(repo: &Path, probe: impl Fn()) -> Calls {
        watch_projects(vec![("a".to_string(), repo.to_path_buf())], probe)
    }

    /// Like `watch`, for several projects at once.
    fn watch_projects(list: Vec<(String, PathBuf)>, probe: impl Fn()) -> Calls {
        let calls: Calls = Arc::new(Mutex::new(Vec::new()));
        let seen = calls.clone();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || run(tx, rx, move || list.clone(), move |ids| seen.lock().unwrap().push((Instant::now(), ids))));
        let deadline = Instant::now() + DEADLINE;
        loop {
            probe();
            if wait_for(Duration::from_secs(2), || count(&calls) > 0) {
                break;
            }
            assert!(Instant::now() < deadline, "watcher never reported a write");
        }
        thread::sleep(QUIET);
        calls.lock().unwrap().clear();
        calls
    }

    /// Waits for at least `n` refreshes, then one more debounce window, and asserts there are exactly `n`.
    fn expect_refreshes(calls: &Calls, n: usize, what: &str) {
        assert!(wait_for(DEADLINE, || count(calls) >= n), "{what}: got {} refreshes, wanted {n}", count(calls));
        thread::sleep(QUIET);
        assert_eq!(count(calls), n, "{what}");
    }

    #[test]
    fn refreshes_after_commit_checkout_and_ref_move() {
        let root = std::env::temp_dir().join(format!("wings-gitwatch-{}", std::process::id()));
        let repo = root.join("repo");
        fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "one"]);
        let calls = watch(&repo, || git(&repo, &["commit", "-q", "--allow-empty", "-m", "probe"]));

        let t = Instant::now();
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "two"]);
        expect_refreshes(&calls, 1, "one debounced refresh for a commit");
        {
            let c = calls.lock().unwrap();
            assert_eq!(c[0].1, vec!["a".to_string()]);
            eprintln!("commit -> refresh in {:?}", c[0].0 - t);
        }

        // A remote-tracking ref moving, which is what a fetch or pull does to the behind count.
        git(&repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
        expect_refreshes(&calls, 2, "ref move refreshes");

        git(&repo, &["checkout", "-q", "-b", "other"]);
        expect_refreshes(&calls, 3, "checkout refreshes");

        // Nothing changes, nothing fires.
        thread::sleep(Duration::from_millis(1200));
        assert_eq!(count(&calls), 3, "idle repo stays quiet");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn follows_a_linked_worktree() {
        let root = std::env::temp_dir().join(format!("wings-gitwatch-wt-{}", std::process::id()));
        let repo = root.join("repo");
        let wt = root.join("wt");
        fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "one"]);
        git(&repo, &["worktree", "add", "-q", "-b", "feat", wt.to_str().unwrap()]);
        let calls = watch(&wt, || git(&wt, &["commit", "-q", "--allow-empty", "-m", "probe"]));
        git(&wt, &["commit", "-q", "--allow-empty", "-m", "two"]);
        assert!(wait_for(DEADLINE, || count(&calls) > 0), "commit in a worktree refreshes");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_commit_in_one_worktree_reports_every_project_of_the_repo_in_one_batch() {
        let root = std::env::temp_dir().join(format!("wings-gitwatch-two-{}", std::process::id()));
        let repo = root.join("repo");
        let wt = root.join("wt");
        fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "one"]);
        git(&repo, &["worktree", "add", "-q", "-b", "feat", wt.to_str().unwrap()]);
        let list = vec![("a".to_string(), repo.clone()), ("b".to_string(), wt.clone())];
        let calls = watch_projects(list, || git(&wt, &["commit", "-q", "--allow-empty", "-m", "probe"]));

        git(&wt, &["commit", "-q", "--allow-empty", "-m", "two"]);
        expect_refreshes(&calls, 1, "one batch for a commit in a linked worktree");
        let mut ids = calls.lock().unwrap()[0].1.clone();
        ids.sort();
        assert_eq!(ids, vec!["a".to_string(), "b".to_string()], "both projects share the refs the commit moved");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn ignores_lock_files_and_unrelated_writes() {
        let git_dir = Path::new("/r/.git");
        assert!(relevant(Path::new("/r/.git/HEAD"), git_dir, git_dir));
        assert!(relevant(Path::new("/r/.git/refs/remotes/origin/main"), git_dir, git_dir));
        assert!(!relevant(Path::new("/r/.git/index.lock"), git_dir, git_dir));
        assert!(!relevant(Path::new("/r/.git/objects/ab/cdef"), git_dir, git_dir));
        assert!(!relevant(Path::new("/r/.git/refs/heads/x.lock"), git_dir, git_dir));
    }

    /// A `Watcher` whose `spaces` reports each call on the returned channel. The channel closes when the watcher
    /// thread ends, because the closure the thread owns is dropped with it.
    fn counting_watcher() -> (Watcher, Receiver<()>) {
        let (called, calls) = mpsc::channel();
        let watcher = Watcher::start(
            move || {
                let _ = called.send(());
                Vec::new()
            },
            |_| {},
        );
        (watcher, calls)
    }

    #[test]
    fn resync_asks_for_the_projects_again() {
        let (watcher, calls) = counting_watcher();
        calls.recv_timeout(DEADLINE).expect("the projects are listed on start");
        watcher.resync();
        calls.recv_timeout(DEADLINE).expect("the projects are listed again after a resync");
    }

    #[test]
    fn dropping_the_watcher_ends_its_thread() {
        let (watcher, calls) = counting_watcher();
        calls.recv_timeout(DEADLINE).expect("the projects are listed on start");
        drop(watcher);
        loop {
            match calls.recv_timeout(DEADLINE) {
                Ok(()) => {}
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => panic!("the watcher thread still runs after the Watcher dropped"),
            }
        }
    }
}
