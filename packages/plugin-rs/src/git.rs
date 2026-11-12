//! Reading git from a plugin, as `wings-plugin-sdk/git` does, with one rule for error text and trimming. A plugin
//! still lists each git command it runs in `permissions.exec`.

use crate::{host, task::SingleFlight};
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeSet, HashMap},
    future::Future,
    rc::Rc,
    task::{Poll, Waker},
};

/// What a git command left: its exit code and what it printed, exactly as git gave them.
pub struct Output {
    pub code: i64,
    pub stdout: String,
    pub stderr: String,
}

/// Runs `git args` in `cwd`. An error is Wings refusing the call, not git failing.
pub async fn run(cwd: &str, args: &[&str]) -> Result<Output, String> {
    let answer = host::exec_in(cwd, "git", args).await?;
    let text = |name: &str| answer[name].as_str().unwrap_or_default().to_owned();
    Ok(Output { code: answer["code"].as_i64().unwrap_or(-1), stdout: text("stdout"), stderr: text("stderr") })
}

/// The first line of what git said went wrong. ssh prints warnings, like OpenSSH's post-quantum notice, as "** "
/// lines before the real error.
fn error_line(stderr: &str) -> Option<&str> {
    stderr.lines().find(|line| !line.trim().is_empty() && !line.starts_with("** ")).map(str::trim)
}

/// Runs `git args` in `cwd` and gives its stdout with the whitespace around it trimmed. A non-zero exit is an error
/// of git's first line of stderr, or `git <command> failed`.
pub async fn git(cwd: &str, args: &[&str]) -> Result<String, String> {
    let output = run(cwd, args).await?;
    if output.code != 0 {
        return Err(error_line(&output.stderr).map_or_else(|| format!("git {} failed", args.first().copied().unwrap_or_default()), str::to_owned));
    }
    Ok(output.stdout.trim().to_owned())
}

/// Runs `git args` in `cwd` and gives its trimmed stdout, or none when git exits non-zero or Wings refuses the call.
pub async fn ok(cwd: &str, args: &[&str]) -> Option<String> {
    run(cwd, args).await.ok().filter(|output| output.code == 0).map(|output| output.stdout.trim().to_owned())
}

enum Slot {
    /// Being looked up, with who waits for it.
    Loading(Vec<Waker>),
    Found(String),
    /// Git said there is none, at this time.
    Missing(f64),
}

/// A lookup kept per argument. An answer is kept for good and a "none" for `miss_ttl` ms. A call that couldn't be
/// asked is kept for neither. However many ask while one lookup runs, it runs once.
struct Kept {
    slots: RefCell<HashMap<String, Slot>>,
    miss_ttl: f64,
    /// Goes up when something is forgotten, so a lookup that was running then doesn't keep its answer.
    forgotten: Cell<u64>,
}

impl Kept {
    fn new(miss_ttl: f64) -> Self {
        Self { slots: RefCell::new(HashMap::new()), miss_ttl, forgotten: Cell::new(0) }
    }

    fn forget(&self, arg: Option<&str>) {
        self.forgotten.set(self.forgotten.get() + 1);
        let gone: Vec<Slot> = match arg {
            Some(arg) => self.slots.borrow_mut().remove(arg).into_iter().collect(),
            None => self.slots.borrow_mut().drain().map(|(_, slot)| slot).collect(),
        };
        for slot in gone {
            if let Slot::Loading(waiting) = slot {
                waiting.into_iter().for_each(Waker::wake);
            }
        }
    }
}

/// The kept answer for `arg`, else what `look` finds: a value, none, or an error when git couldn't be asked.
async fn kept(cache: &'static std::thread::LocalKey<Kept>, arg: &str, look: impl Future<Output = Result<Option<String>, String>>) -> Option<String> {
    loop {
        let loading = cache.with(|kept| match kept.slots.borrow().get(arg) {
            Some(Slot::Found(value)) => Err(Some(value.clone())),
            Some(Slot::Missing(at)) if host::now() - at < kept.miss_ttl => Err(None),
            Some(Slot::Loading(_)) => Ok(true),
            _ => Ok(false),
        });
        match loading {
            Err(answer) => return answer,
            Ok(false) => break,
            Ok(true) => {
                // Until the lookup that is running has its answer, or was forgotten.
                std::future::poll_fn(|cx| {
                    cache.with(|kept| match kept.slots.borrow_mut().get_mut(arg) {
                        Some(Slot::Loading(waiting)) => {
                            waiting.push(cx.waker().clone());
                            Poll::Pending
                        }
                        _ => Poll::Ready(()),
                    })
                })
                .await;
            }
        }
    }
    let started = cache.with(|kept| {
        kept.slots.borrow_mut().insert(arg.to_owned(), Slot::Loading(Vec::new()));
        kept.forgotten.get()
    });
    let found = look.await;
    cache.with(|kept| {
        if kept.forgotten.get() != started {
            return;
        }
        let mut slots = kept.slots.borrow_mut();
        let was = match &found {
            Ok(Some(value)) => slots.insert(arg.to_owned(), Slot::Found(value.clone())),
            Ok(None) if kept.miss_ttl > 0.0 => slots.insert(arg.to_owned(), Slot::Missing(host::now())),
            _ => slots.remove(arg),
        };
        if let Some(Slot::Loading(waiting)) = was {
            waiting.into_iter().for_each(Waker::wake);
        }
    });
    found.ok().flatten()
}

thread_local! {
    /// How long a folder git said is in no repo is believed, so a poll doesn't ask again for each such pane.
    static ROOTS: Kept = Kept::new(60_000.0);
    static DEFAULTS: Kept = Kept::new(0.0);
}

/// The top folder of the repo that `cwd` is in, or none outside one. A found root is kept for good. A folder git says
/// is in no repo is believed for a minute, so a poll doesn't ask again for it.
pub async fn repo_root(cwd: &str) -> Option<String> {
    kept(&ROOTS, cwd, async {
        if cwd.is_empty() {
            return Ok(None);
        }
        let output = run(cwd, &["rev-parse", "--show-toplevel"]).await?;
        Ok((output.code == 0).then(|| output.stdout.trim().to_owned()).filter(|root| !root.is_empty()))
    })
    .await
}

/// Forgets every repo root, for a Refresh button: a new repo shows at once.
pub fn forget_repo_roots() {
    ROOTS.with(|kept| kept.forget(None));
}

/// The branch a repo's "this branch" is compared against: `origin/HEAD` as the remote set it, else `origin/main`,
/// `origin/master`, `main` or `master`. None when none exists. Kept once found, until forgotten.
pub async fn default_branch(root: &str) -> Option<String> {
    kept(&DEFAULTS, root, async {
        if let Some(head) = ok(root, &["rev-parse", "--abbrev-ref", "origin/HEAD"]).await.filter(|head| !head.is_empty() && head != "origin/HEAD") {
            return Ok(Some(head));
        }
        for name in ["origin/main", "origin/master", "main", "master"] {
            if ok(root, &["rev-parse", "--verify", "--quiet", &format!("{name}^{{commit}}")]).await.is_some_and(|commit| !commit.is_empty()) {
                return Ok(Some(name.to_owned()));
            }
        }
        Ok(None)
    })
    .await
}

/// Forgets the default branch of the repo at `root`, or of every repo.
pub fn forget_default_branch(root: Option<&str>) {
    DEFAULTS.with(|kept| kept.forget(root));
}

/// Calls `handler(project ids, repo roots)` when the repo of a project changes: a commit, checkout, pull, fetch,
/// push, amend or reset, from anywhere. Needs a `git` entry in `permissions.exec`. Events are held while the page is
/// out of view or Wings isn't focused, and handled once, with every id that came meanwhile, when it is back. A burst
/// while `handler` runs makes it run once more. The default branch of each changed repo is forgotten first, so
/// `handler` reads it fresh. Edits to files in the working tree don't send an event.
pub fn on_change<F, Fut>(handler: F)
where
    F: Fn(Vec<String>, Vec<String>) -> Fut + 'static,
    Fut: Future<Output = ()> + 'static,
{
    let ids: Rc<RefCell<BTreeSet<String>>> = Rc::default();
    let handler = Rc::new(handler);
    let held = ids.clone();
    let flush = SingleFlight::new(move || {
        let (held, handler) = (held.clone(), handler.clone());
        async move {
            let batch: Vec<String> = std::mem::take(&mut *held.borrow_mut()).into_iter().collect();
            if batch.is_empty() {
                return;
            }
            let projects = host::projects().await.unwrap_or_default();
            let mut roots = Vec::new();
            for project in projects.as_array().into_iter().flatten() {
                if project["id"].as_str().is_some_and(|id| batch.iter().any(|changed| changed == id))
                    && let Some(root) = repo_root(project["path"].as_str().unwrap_or_default()).await
                    && !roots.contains(&root)
                {
                    roots.push(root);
                }
            }
            for root in &roots {
                forget_default_branch(Some(root));
            }
            handler(batch, roots).await;
        }
    });
    let (held, run) = (ids.clone(), flush.clone());
    host::on("git", move |_, change| {
        held.borrow_mut().extend(change["projectIds"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned));
        if host::focused() {
            drop(run.call());
        }
    });
    host::on_focus(move |focused| {
        if focused && !ids.borrow().is_empty() {
            drop(flush.call());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::block_on;
    use serde_json::json;

    /// A fake git that answers each command line with `(code, stdout, stderr)`, and counts what it was asked.
    fn fake(answers: &'static [(&'static str, i64, &'static str, &'static str)]) -> Rc<RefCell<Vec<String>>> {
        let asked: Rc<RefCell<Vec<String>>> = Rc::default();
        let seen = asked.clone();
        host::testing::answer(move |_, params| {
            let line = params["args"].as_array().unwrap().iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" ");
            seen.borrow_mut().push(format!("{} {line}", params["cwd"].as_str().unwrap()));
            let (_, code, stdout, stderr) = answers.iter().find(|answer| answer.0 == line).copied().unwrap_or(("", 1, "", ""));
            Ok(json!({ "code": code, "stdout": stdout, "stderr": stderr }))
        });
        asked
    }

    #[test]
    fn a_failure_reads_as_gits_first_line_past_the_ssh_warnings() {
        fake(&[("fetch", 1, "", "** WARNING: connection is not using a post-quantum key exchange algorithm.\n\nfatal: no such remote\nhint: more\n"), ("push", 1, "", ""), ("status", 0, " M a\n", "")]);
        assert_eq!(block_on(git("/r", &["fetch"])), Err("fatal: no such remote".into()));
        assert_eq!(block_on(git("/r", &["push"])), Err("git push failed".into()));
        assert_eq!(block_on(git("/r", &["status"])), Ok("M a".into()));
        assert_eq!(block_on(ok("/r", &["push"])), None);
        host::testing::answer(|_, _| Err("git status isn't in the plugin's permissions".into()));
        assert_eq!(block_on(ok("/r", &["status"])), None);
    }

    #[test]
    fn a_repo_root_is_asked_for_once_and_again_after_it_is_forgotten() {
        let asked = fake(&[("rev-parse --show-toplevel", 0, "/r\n", "")]);
        assert_eq!(block_on(repo_root("/r/src")), Some("/r".into()));
        assert_eq!(block_on(repo_root("/r/src")), Some("/r".into()));
        assert_eq!(block_on(repo_root("")), None);
        assert_eq!(asked.borrow().len(), 1);
        forget_repo_roots();
        assert_eq!(block_on(repo_root("/r/src")), Some("/r".into()));
        assert_eq!(asked.borrow().len(), 2);
        // A folder in no repo is believed, and a call Wings refused is asked again.
        fake(&[]);
        assert_eq!(block_on(repo_root("/tmp")), None);
        let asked = fake(&[("rev-parse --show-toplevel", 0, "/tmp\n", "")]);
        assert_eq!(block_on(repo_root("/tmp")), None);
        assert!(asked.borrow().is_empty());
        host::testing::answer(|_, _| Err("refused".into()));
        assert_eq!(block_on(repo_root("/later")), None);
        fake(&[("rev-parse --show-toplevel", 0, "/later\n", "")]);
        assert_eq!(block_on(repo_root("/later")), Some("/later".into()));
    }

    #[test]
    fn the_default_branch_is_what_the_remote_set_else_the_first_that_exists() {
        fake(&[("rev-parse --abbrev-ref origin/HEAD", 0, "origin/trunk\n", "")]);
        assert_eq!(block_on(default_branch("/a")), Some("origin/trunk".into()));
        let asked = fake(&[("rev-parse --abbrev-ref origin/HEAD", 0, "origin/HEAD\n", ""), ("rev-parse --verify --quiet master^{commit}", 0, "abc\n", "")]);
        assert_eq!(block_on(default_branch("/b")), Some("master".into()));
        assert_eq!(asked.borrow().len(), 5);
        // None isn't kept: a branch made later is found.
        fake(&[]);
        assert_eq!(block_on(default_branch("/c")), None);
        fake(&[("rev-parse --verify --quiet main^{commit}", 0, "abc\n", "")]);
        assert_eq!(block_on(default_branch("/c")), Some("main".into()));
        forget_default_branch(Some("/c"));
        fake(&[]);
        assert_eq!(block_on(default_branch("/c")), None);
    }
}
