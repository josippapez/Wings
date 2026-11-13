//! The PR tab: one pull request with its status, description and review threads. The tab loads them itself, through the
//! forges, and asks main for what only main can do: the diff viewer that takes replies, Send to agent, one commit's
//! changes, marking the PR read and the host's sign-in. Its status is told to main, so a pane's badge on the PR stays
//! fresh without a second load.

use crate::{
    args::read_args,
    forge::ForgeError,
    forges::{self, Can},
    ipc, problem,
    problem::Problem,
    remote::remote_of_args,
    types::{Comment, Failing, FailureDetail, Link, Overview, PrArgs, Push, Run, RunRef, RunStep, SnippetLine, Status},
    ui::{pr_view, pulls_list, run_view},
};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use wings_plugin::{
    host, poll,
    task::{SingleFlight, join, spawn},
};

/// A tab refreshes what it shows this often while Wings is in front.
const REFETCH_MS: u32 = 120_000;

/// What the tab holds of a code thread's lines: being read, or read (none where they can't be had).
#[derive(Clone, PartialEq, Debug)]
pub enum Snip {
    Loading,
    Done(Option<Vec<SnippetLine>>),
}

pub struct Page {
    pub args: Option<PrArgs>,
    link: Option<Link>,
    pub overview: Option<Overview>,
    pub comments: Option<Vec<Comment>>,
    pub pushes: Vec<Push>,
    pub can: Can,
    /// What the overview, the conversation and the pushes each failed with, in that order.
    errors: [Option<String>; 3],
    pub problem: Option<Problem>,
    pub problem_busy: bool,
    pub notice: Option<String>,
    /// Which toolbar button waits on main: `diff` or `agent`.
    pub busy: Option<&'static str>,
    /// A code thread's lines, by the thread's first comment and whether they are the latest code.
    pub snippets: HashMap<(i64, bool), Snip>,
    /// Goes back to the project's list, when the PR was picked from it in this tab.
    pub back: Option<Rc<dyn Fn()>>,
    /// Whether the tab is the project picker, which a tab opened with no PR starts as.
    pub picking: bool,
}

thread_local! {
    static PAGE: RefCell<Page> = RefCell::new(empty());
    static LOAD: SingleFlight = SingleFlight::new(load_all);
}

pub fn with<T>(change: impl FnOnce(&mut Page) -> T) -> T {
    PAGE.with(|page| change(&mut page.borrow_mut()))
}

pub fn read<T>(read: impl FnOnce(&Page) -> T) -> T {
    PAGE.with(|page| read(&page.borrow()))
}

impl Page {
    /// The first thing that failed, which shows over the PR or in place of it.
    pub fn error(&self) -> Option<&str> {
        self.errors.iter().flatten().next().map(String::as_str)
    }
}

// ---------- loading ----------

async fn link() -> Result<Link, ForgeError> {
    if let Some(link) = read(|p| p.link.clone()) {
        return Ok(link);
    }
    let args = read(|p| p.args.clone()).ok_or_else(|| ForgeError::from("No pull request".to_owned()))?;
    let link = forges::link_of(&args).await?;
    with(|p| p.link = Some(link.clone()));
    Ok(link)
}

/// What a failed load shows: a host that needs signing in, a token or a CLI gets a short message and its button.
async fn explain(error: &ForgeError) -> Option<Problem> {
    let args = read(|p| p.args.clone())?;
    let remote = remote_of_args(&args).await?;
    problem::shown(&remote, error)
}

async fn load_overview() {
    let result = match link().await {
        Ok(link) => forges::overview(&link).await,
        Err(error) => Err(error),
    };
    match result {
        Ok(overview) => {
            let (url, cwd) = read(|p| p.args.as_ref().map(|a| (a.url.clone(), a.cwd.clone()))).unwrap_or_default();
            let mut told = overview.status.clone();
            told.items = None;
            ipc::send(ipc::PR_STATUS, &json!({ "url": url, "cwd": cwd, "status": told }));
            with(|p| {
                p.errors[0] = None;
                p.problem = None;
                p.overview = Some(overview);
            });
            if let Some(status) = status() {
                run_view::status_loaded(&status);
            }
            ensure_snippets();
        }
        Err(error) => {
            let shown = explain(&error).await;
            with(|p| {
                p.problem = shown;
                p.errors[0] = Some(error.to_string());
            });
        }
    }
}

async fn load_comments() {
    let result = match link().await {
        Ok(link) => forges::comments(&link).await,
        Err(error) => Err(error),
    };
    match result {
        Ok(comments) => {
            with(|p| {
                p.errors[1] = None;
                p.comments = Some(comments);
            });
            ensure_snippets();
        }
        Err(error) => {
            let hidden = explain(&error).await.is_some();
            with(|p| {
                p.errors[1] = (!hidden).then(|| error.to_string());
                if hidden {
                    p.comments = Some(Vec::new());
                }
            });
        }
    }
}

async fn load_pushes() {
    let result = match link().await {
        Ok(link) => forges::pushes(&link).await,
        Err(error) => Err(error),
    };
    match result {
        Ok(pushes) => with(|p| {
            p.errors[2] = None;
            p.pushes = pushes;
        }),
        Err(error) => {
            let hidden = explain(&error).await.is_some();
            with(|p| p.errors[2] = (!hidden).then(|| error.to_string()));
        }
    }
}

/// The overview, the conversation and the pushes, at once.
async fn load_all() {
    join(load_overview(), join(load_comments(), load_pushes())).await;
}

/// Loads again, once at a time: a call while one runs makes it go once more when it is done.
pub fn refresh() {
    spawn(LOAD.with(SingleFlight::call));
}

// ---------- what a thread's code needs ----------

/// Reads the lines of each code thread that shows, which the comment doesn't carry: one read per thread and view.
/// Called when the conversation, the pushes or what a card shows changes, never from drawing.
pub fn ensure_snippets() {
    let wanted: Vec<(Comment, bool)> = read(|p| {
        if !p.can.snippet {
            return Vec::new();
        }
        pr_view::wanted_snippets(p).into_iter().filter(|(c, now)| !p.snippets.contains_key(&(c.id, *now))).collect()
    });
    for (root, now) in wanted {
        with(|p| p.snippets.insert((root.id, now), Snip::Loading));
        spawn(async move {
            let (head, pushes) = read(|p| (p.overview.as_ref().and_then(|o| o.status.head_commit.clone()), p.pushes.clone()));
            let lines = match link().await {
                Ok(link) => forges::snippet(&link, &root, &pushes, head.as_deref(), now).await.unwrap_or(None),
                Err(_) => None,
            };
            with(|p| p.snippets.insert((root.id, now), Snip::Done(lines)));
        });
    }
}

// ---------- what only main can do ----------

fn ask_main(op: &str, extra: Value) -> impl std::future::Future<Output = Result<Value, String>> {
    let args = read(|p| p.args.clone());
    let mut payload = json!({ "op": op, "args": args });
    if let (Some(payload), Some(extra)) = (payload.as_object_mut(), extra.as_object()) {
        payload.extend(extra.clone());
    }
    ipc::ask(ipc::PR, payload)
}

/// Opens the diff viewer, or sends the PR's diff to the agent on its branch (`agent`), at a thread's lines when given.
pub fn run(op: &'static str, focus: Option<Value>) {
    with(|p| {
        p.busy = Some(op);
        p.notice = None;
    });
    spawn(async move {
        let result = ask_main(op, json!({ "focus": focus })).await;
        with(|p| {
            p.busy = None;
            p.notice = result.err();
        });
    });
}

/// Opens one commit's changes in the diff viewer.
pub fn open_commit(sha: String, message: String) {
    with(|p| p.notice = None);
    spawn(async move {
        let result = ask_main("commit", json!({ "commit": { "sha": sha, "message": message } })).await;
        with(|p| p.notice = result.err());
    });
}

/// The problem's button: what the pane badge's Sign in, Add token or Install does for this project's host. A sign-in
/// waits for the browser for up to 5 minutes.
pub fn fix_problem() {
    let Some(path) = read(|p| p.args.as_ref().map(|a| a.cwd.clone())) else { return };
    with(|p| {
        p.problem_busy = true;
        p.notice = None;
    });
    spawn(async move {
        let result = ipc::ask(ipc::PULLS_ACTION, json!({ "path": path })).await;
        with(|p| {
            p.problem_busy = false;
            p.notice = result.err();
        });
    });
}

// ---------- what the tab does itself ----------

/// Posts a reply to a thread, which then shows in the conversation.
pub async fn reply(root: Comment, body: String) -> Result<(), String> {
    let link = link().await.map_err(|e| e.to_string())?;
    let reply = forges::reply(&link, &root, &body).await.map_err(|e| e.to_string())?;
    with(|p| p.comments.get_or_insert_with(Vec::new).push(reply));
    Ok(())
}

/// Approves as you at the head commit the tab showed, or votes (Azure DevOps), then reads the PR again.
pub async fn approve(sha: String) -> Result<(), String> {
    let link = link().await.map_err(|e| e.to_string())?;
    forges::approve(&link, &sha).await.map_err(|e| e.to_string())?;
    load_overview().await;
    Ok(())
}

pub async fn vote(vote: String) -> Result<(), String> {
    let link = link().await.map_err(|e| e.to_string())?;
    forges::vote(&link, &vote).await.map_err(|e| e.to_string())?;
    load_overview().await;
    Ok(())
}

/// Why one failed check failed.
pub async fn load_failure(failing: Failing) -> Result<Vec<FailureDetail>, String> {
    let link = link().await.map_err(|e| e.to_string())?;
    forges::failure(&link, &failing).await.map_err(|e| e.to_string())
}

/// A check's run: its jobs and steps.
pub async fn load_run(run: RunRef) -> Result<Run, String> {
    let link = link().await.map_err(|e| e.to_string())?;
    forges::run(&link, &run).await.map_err(|e| e.to_string())
}

/// One step's log, as the CI wrote it.
pub async fn load_run_log(run: RunRef, step: RunStep) -> Result<Vec<String>, String> {
    let link = link().await.map_err(|e| e.to_string())?;
    forges::run_log(&link, &run, &step).await.map_err(|e| e.to_string())
}

/// The status the tab shows, which the checks view and the run view read.
pub fn status() -> Option<Status> {
    read(|p| p.overview.as_ref().map(|o| o.status.clone()))
}

// ---------- the page ----------

fn open_pr(args: PrArgs, back: Option<Rc<dyn Fn()>>) {
    with(|p| *p = Page { can: forges::can(args.provider), args: Some(args), back, ..empty() });
    pr_view::reset();
    run_view::reset();
    // Opening the tab marks the PR read, and tells main which project it came from.
    let seen = ask_main("seen", Value::Null);
    spawn(async move {
        drop(seen.await);
    });
    refresh();
}

fn empty() -> Page {
    Page {
        args: None, link: None, overview: None, comments: None, pushes: Vec::new(), can: Can { approve: false, vote: false, snippet: false, run: false },
        errors: [None, None, None], problem: None, problem_busy: false, notice: None, busy: None, snippets: HashMap::new(), back: None, picking: false,
    }
}

/// Goes back to the project's list.
fn show_picker() {
    with(|p| *p = Page { picking: true, ..empty() });
}

fn view() -> Vec<wings_plugin::ui::Node> {
    if read(|p| p.picking) {
        return pulls_list::picker_view();
    }
    read(pr_view::build)
}

#[unsafe(no_mangle)]
pub fn start() {
    wings_plugin::run_filled(view);
    ipc::hear(ipc::REFRESH, |_| {
        with(|p| p.link = None);
        if read(|p| p.args.is_some()) {
            refresh();
        }
    });
    poll::while_focused(REFETCH_MS, false, || {
        if read(|p| p.args.is_some() && !p.picking) {
            refresh();
        }
    });
    match host::view_args().as_ref().and_then(read_args) {
        Some(args) => open_pr(args, None),
        // A tab opened with no pull request lists the project's open ones; picking one opens it here, so the tab stays one tab.
        None => {
            show_picker();
            pulls_list::start_picker(|picked| {
                let back: Rc<dyn Fn()> = Rc::new(|| {
                    show_picker();
                    pr_view::reset();
                });
                open_pr(picked, Some(back));
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};
    use wings_plugin::task::block_on;

    fn args() -> PrArgs {
        serde_json::from_value(json!({ "provider": "github", "url": "https://github.com/acme/widgets/pull/12", "number": 12, "repo": "acme/widgets", "title": "Add retry", "cwd": "/work/widgets" })).unwrap()
    }

    fn gh_answer(body: &str) -> Rc<RefCell<Vec<String>>> {
        let asked: Rc<RefCell<Vec<String>>> = Rc::default();
        let seen = asked.clone();
        let body = body.to_owned();
        host::testing::answer(move |method, params| {
            seen.borrow_mut().push(format!("{method} {}", params["args"]));
            match method {
                "exec" if params["args"][0] == "pr" && params["args"][1] == "view" && params["args"][4].as_str().is_some_and(|f| f.contains("body")) => Ok(json!({ "code": 0, "stdout": body, "stderr": "" })),
                "exec" if params["args"][0] == "api" => Ok(json!({ "code": 0, "stdout": "[[]]", "stderr": "" })),
                "exec" => Ok(json!({ "code": 0, "stdout": "{\"comments\":[],\"reviews\":[]}", "stderr": "" })),
                _ => Ok(Value::Null),
            }
        });
        asked
    }

    #[test]
    fn opening_a_pull_request_loads_its_overview_conversation_and_pushes_and_tells_main_the_status() {
        let pr = json!({
            "number": 12, "url": "https://github.com/acme/widgets/pull/12", "title": "Add retry", "body": "Retries.", "author": { "login": "ada" }, "createdAt": "2026-11-01T09:00:00Z",
            "state": "OPEN", "isDraft": false, "reviewDecision": "APPROVED", "mergeable": "MERGEABLE", "statusCheckRollup": [], "additions": 3, "deletions": 1, "changedFiles": 2,
            "headRefName": "retry", "baseRefName": "main", "updatedAt": "2026-11-02T09:00:00Z", "latestReviews": []
        });
        gh_answer(&pr.to_string());
        open_pr(args(), None);
        block_on(load_all());
        read(|p| {
            assert_eq!(p.overview.as_ref().map(|o| o.author.as_str()), Some("ada"));
            assert_eq!(p.comments.as_ref().map(Vec::len), Some(0));
            assert!(p.error().is_none() && p.problem.is_none());
            assert!(p.can.approve);
        });
    }

    #[test]
    fn a_failed_overview_is_the_pages_error_and_keeps_what_was_shown() {
        host::testing::answer(|_, _| Err("gh could not run".into()));
        open_pr(args(), None);
        block_on(load_overview());
        read(|p| {
            assert!(p.overview.is_none());
            assert!(p.error().is_some_and(|e| e.contains("gh could not run")));
        });
    }
}
