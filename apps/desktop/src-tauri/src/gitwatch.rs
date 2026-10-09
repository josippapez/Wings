//! Refreshes a project's git status as soon as its repo changes, instead of waiting for the next poll.
//! It watches the few places git writes when HEAD, the index or a ref moves (a pull, commit, checkout,
//! fetch or push, from a pane or from anywhere else), waits for the writes to settle, then refreshes only
//! the projects that repo belongs to. Edits to files in the working tree are not watched; the slow poll
//! in `start_git_status` still picks those up.

use notify::{EventKind, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::OnceLock;
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
}

static TX: OnceLock<Sender<Msg>> = OnceLock::new();

/// Tells the watcher the list of projects changed.
pub fn resync() {
    if let Some(tx) = TX.get() {
        let _ = tx.send(Msg::Resync);
    }
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

/// Runs the watcher until the channel closes. `spaces` lists (id, path) of the projects, `refresh` is
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
            repos = spaces()
                .into_iter()
                .filter_map(|(id, path)| git_dirs(&path).map(|(git_dir, common)| Repo { id, git_dir, common }))
                .collect();
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

/// Starts the watcher thread for the app's projects.
pub fn start(app: tauri::AppHandle) {
    use tauri::Manager;
    let (tx, rx) = mpsc::channel();
    if TX.set(tx.clone()).is_err() {
        return;
    }
    thread::spawn(move || {
        let spaces = {
            let app = app.clone();
            move || {
                let state = app.state::<crate::AppState>();
                let list = state.spaces.lock().unwrap().spaces.iter().map(|s| (s.id.clone(), PathBuf::from(&s.path))).collect();
                list
            }
        };
        run(tx, rx, spaces, |ids| crate::git_refresh_spaces(&app, &ids));
    });
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

    /// Starts the watcher on one repo and returns the refresh calls it makes, with the time each arrived.
    fn watch(repo: &Path) -> Arc<Mutex<Vec<(Instant, Vec<String>)>>> {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = calls.clone();
        let (tx, rx) = mpsc::channel();
        let list = vec![("a".to_string(), repo.to_path_buf())];
        thread::spawn(move || run(tx, rx, move || list.clone(), move |ids| seen.lock().unwrap().push((Instant::now(), ids))));
        thread::sleep(Duration::from_millis(500));
        calls
    }

    #[test]
    fn refreshes_after_commit_checkout_and_ref_move() {
        let root = std::env::temp_dir().join(format!("wings-gitwatch-{}", std::process::id()));
        let repo = root.join("repo");
        fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "one"]);
        let calls = watch(&repo);

        let t = Instant::now();
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "two"]);
        thread::sleep(Duration::from_millis(900));
        {
            let c = calls.lock().unwrap();
            assert_eq!(c.len(), 1, "one debounced refresh for a commit, got {}", c.len());
            assert_eq!(c[0].1, vec!["a".to_string()]);
            eprintln!("commit -> refresh in {:?}", c[0].0 - t);
        }

        // A remote-tracking ref moving, which is what a fetch or pull does to the behind count.
        git(&repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
        thread::sleep(Duration::from_millis(900));
        assert_eq!(calls.lock().unwrap().len(), 2, "ref move refreshes");

        git(&repo, &["checkout", "-q", "-b", "other"]);
        thread::sleep(Duration::from_millis(900));
        assert_eq!(calls.lock().unwrap().len(), 3, "checkout refreshes");

        // Nothing changes, nothing fires.
        thread::sleep(Duration::from_millis(1200));
        assert_eq!(calls.lock().unwrap().len(), 3, "idle repo stays quiet");
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
        let calls = watch(&wt);
        git(&wt, &["commit", "-q", "--allow-empty", "-m", "two"]);
        thread::sleep(Duration::from_millis(900));
        assert!(!calls.lock().unwrap().is_empty(), "commit in a worktree refreshes");
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
}
