//! The pull request of the branch the chat's folder is on, for the side panel (`pull_view.rs`). The PR tracker plugin
//! knows every forge, so the data is its `pull_request` tool (`callTools` in the manifest), asked by folder because a
//! chat is no terminal pane; this file keeps what it answered and acts on it. Nothing here calls a forge, except the
//! merge, which runs `gh pr merge` and so is for GitHub only (`Pr::github`). It is read again after a branch change, and
//! every minute while the panel shows and Wings is in front; the tracker's own cache keeps that to one forge call a
//! minute for a branch, whoever asks.

use std::cell::RefCell;

use serde_json::{Value, json};
use wings_plugin::{format::truncate, host, task};

use crate::chat;

/// How often the PR is read again while the panel shows.
pub const EVERY_MS: u32 = 60_000;
const TOOL: &str = "pr-tracker__pull_request";

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    Open,
    Merged,
    Closed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pr {
    pub number: u64,
    /// `#12`, or `!12` on GitLab.
    pub reference: String,
    pub title: String,
    pub url: String,
    pub state: State,
    pub draft: bool,
    pub review: String,
    /// Passed, failed and running checks.
    pub checks: (u64, u64, u64),
    pub mergeable: bool,
    /// `gh` can merge it.
    pub github: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Load {
    Idle,
    /// A folder no forge is known for, or no PR tracker to ask: the panel says nothing.
    Unsupported,
    NoPr,
    Found(Pr),
    /// Why the tracker couldn't say, like a host that needs signing in.
    Failed(String),
}

/// How `gh pr merge` merges: the repository decides which it allows, so the dialog offers all three.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Method {
    Squash,
    Merge,
    Rebase,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Merge {
    Idle,
    /// The dialog asks which way, and for the go-ahead.
    Asking,
    Running,
    Failed(String),
}

pub struct Pull {
    pub load: Load,
    /// The branch `load` is of.
    pub branch: Option<String>,
    /// The branch being read now, so a burst of triggers is one read.
    reading: Option<String>,
    pub merge: Merge,
}

thread_local! {
    static PULL: RefCell<Pull> = const { RefCell::new(Pull { load: Load::Idle, branch: None, reading: None, merge: Merge::Idle }) };
}

pub fn with<T>(change: impl FnOnce(&mut Pull) -> T) -> T {
    PULL.with(|pull| change(&mut pull.borrow_mut()))
}

/// What the tracker's `pull_request` answered, as JSON text.
pub fn parse(text: &str) -> Load {
    let Ok(found) = serde_json::from_str::<Value>(text) else { return Load::Failed("The PR tracker's answer wasn't readable".into()) };
    if found["supported"] == false {
        return Load::Unsupported;
    }
    if found["found"] != true {
        return Load::NoPr;
    }
    let count = |name: &str| found["checks"][name].as_u64().unwrap_or(0);
    let state = match found["state"].as_str() {
        Some("merged") => State::Merged,
        Some("closed") => State::Closed,
        _ => State::Open,
    };
    let text = |name: &str| found[name].as_str().unwrap_or_default().to_owned();
    match found["number"].as_u64() {
        Some(number) => Load::Found(Pr {
            number,
            reference: text("ref"),
            title: text("title"),
            url: text("url"),
            state,
            draft: found["draft"] == true,
            review: text("review"),
            checks: (count("passed"), count("failed"), count("running")),
            mergeable: found["mergeable"] == true,
            github: found["github"] == true,
        }),
        None => Load::NoPr,
    }
}

/// "1 failed, 2 passed", "3 running, 1 passed", "No checks", most pressing first.
pub fn checks_text((passed, failed, running): (u64, u64, u64)) -> String {
    let mut said = Vec::new();
    if failed > 0 {
        said.push(format!("{failed} failed"));
    }
    if running > 0 {
        said.push(format!("{running} running"));
    }
    if passed > 0 {
        said.push(format!("{passed} passed"));
    }
    if said.is_empty() { "No checks".into() } else { said.join(", ") }
}

/// Whether the panel shows, in the chat or in Wings' sidebar, with this section open: what is not seen is not read.
fn visible(c: &chat::Chat) -> bool {
    let shown = if c.side_in_wings { crate::session_side::shown() } else { c.side_open };
    shown && !c.panel_closed.iter().any(|shut| shut == "pull")
}

/// Reads the PR of the branch now. A read that fails keeps what the panel showed for this branch.
pub fn refresh() {
    read(false);
}

/// `fresh` has the tracker ask the forge again instead of answering from the minute it keeps: after a merge.
fn read(fresh: bool) {
    let Some((cwd, branch)) = chat::with(|c| Some((c.cwd.clone(), c.branch.clone()?))) else {
        return with(|p| *p = Pull { load: Load::Idle, branch: None, reading: None, merge: Merge::Idle });
    };
    if with(|p| p.reading.as_deref() == Some(branch.as_str())) {
        return;
    }
    with(|p| p.reading = Some(branch.clone()));
    task::spawn(async move {
        let asked = host::call("callTool", json!({ "name": TOOL, "arguments": { "cwd": cwd, "fresh": fresh } })).await;
        let load = match asked {
            Ok(answer) if answer["isError"] != true => parse(answer["text"].as_str().unwrap_or_default()),
            Ok(answer) => Load::Failed(truncate(answer["text"].as_str().unwrap_or("The PR tracker couldn't say"), 200)),
            // The call itself was refused: the tracker is off, not installed, or not starting yet. Nothing to show.
            Err(_) => Load::Unsupported,
        };
        with(|p| {
            if p.reading.as_deref() != Some(branch.as_str()) {
                return;
            }
            p.reading = None;
            let same_branch = p.branch.as_deref() == Some(branch.as_str());
            if matches!(load, Load::Failed(_)) && same_branch && matches!(p.load, Load::Found(_) | Load::NoPr) {
                return;
            }
            p.load = load;
            p.branch = Some(branch);
        });
    });
}

/// The poll: read again while the panel shows. The poll itself only runs while Wings is in front.
pub fn tick() {
    if chat::with(|c| visible(c)) {
        refresh();
    }
}

/// After the chat read its folder's git (a switch, a turn's edits): a new branch is read at once, and so is a branch with
/// no PR, which the turn may just have made one for. A read of a branch with a PR waits for the poll.
pub fn after_git() {
    let (branch, shown) = chat::with(|c| (c.branch.clone(), visible(c)));
    let wanted = with(|p| p.branch != branch || matches!(p.load, Load::NoPr | Load::Failed(_)));
    if shown && wanted {
        refresh();
    }
}

/// The merge's arguments for `gh`.
pub fn merge_args(number: u64, how: Method) -> Vec<String> {
    let flag = match how {
        Method::Squash => "--squash",
        Method::Merge => "--merge",
        Method::Rebase => "--rebase",
    };
    vec!["pr".into(), "merge".into(), number.to_string(), flag.into()]
}

/// What `gh pr merge` said, which for a refusal (branch protection, a conflict) is the reason.
fn merge_outcome(answer: Result<Value, String>) -> Result<(), String> {
    let answer = answer?;
    if answer["code"] == 0 {
        return Ok(());
    }
    let said = |field: &str| answer[field].as_str().map(str::trim).filter(|said| !said.is_empty()).map(|said| truncate(said, 400));
    Err(said("stderr").or_else(|| said("stdout")).unwrap_or_else(|| "gh pr merge failed".to_owned()))
}

pub fn ask_merge() {
    with(|p| {
        if matches!(&p.load, Load::Found(pr) if pr.github && pr.mergeable) {
            p.merge = Merge::Asking;
        }
    });
}

pub fn cancel_merge() {
    with(|p| {
        if p.merge != Merge::Running {
            p.merge = Merge::Idle;
        }
    });
}

/// The dialog's go-ahead. Only a PR the tracker said can merge is sent to `gh`, which still refuses what the repository's
/// rules forbid.
pub fn merge(how: Method) {
    let (number, cwd) = match (with(|p| (p.merge == Merge::Asking, p.load.clone())), chat::with(|c| c.cwd.clone())) {
        ((true, Load::Found(pr)), cwd) if pr.github && pr.mergeable => (pr.number, cwd),
        _ => return,
    };
    with(|p| p.merge = Merge::Running);
    task::spawn(async move {
        let args = merge_args(number, how);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let done = merge_outcome(host::exec_in(&cwd, "gh", &args).await);
        with(|p| p.merge = match &done {
            Ok(()) => Merge::Idle,
            Err(why) => Merge::Failed(why.clone()),
        });
        if done.is_ok() {
            with(|p| p.reading = None);
            read(true);
        }
    });
}

/// What the Create PR button sends Claude. Claude pushes and writes the title and description from the work, as the
/// repository's habits ask; Wings opens nothing itself, so a PR is only made by a turn the user can read and stop.
pub fn create_prompt(branch: &str) -> String {
    format!("Create a pull request for this branch (`{branch}`). Make sure the work is committed and pushed first, write the title and description from the changes, and open it with the repository's CLI (gh for GitHub). Tell me the link when it's open.")
}

/// What "Ask Claude why" sends: Claude reads the logs with the tracker's tool, which a chat names its folder for.
pub fn failing_prompt(pr: &Pr, cwd: &str) -> String {
    format!("The checks on pull request {} are failing. Use the pr-tracker failing_checks tool with cwd \"{cwd}\" to read why, then fix what is wrong on this branch.", pr.reference)
}

/// Sends `text` to Claude as your message, queued while it works.
pub fn ask_claude(text: String) {
    chat::write(|c, n| {
        let stamp = host::now() as u64;
        if c.session.working {
            return Some(c.session.queue(&text, Vec::new(), stamp, n));
        }
        let line = c.session.send(&text, &[], stamp, n);
        c.doing = (c.session.activity(), stamp);
        Some(line)
    });
}

pub fn create() {
    if let Some(branch) = chat::with(|c| c.branch.clone()) {
        ask_claude(create_prompt(&branch));
    }
}

pub fn explain_failing() {
    let (found, cwd) = (with(|p| p.load.clone()), chat::with(|c| c.cwd.clone()));
    if let Load::Found(pr) = found {
        ask_claude(failing_prompt(&pr, &cwd));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FOUND: &str = r##"{ "supported": true, "found": true, "provider": "GitHub", "github": true, "number": 12, "ref": "#12", "title": "Add the cart", "url": "https://github.com/acme/shop/pull/12", "state": "open", "draft": false, "label": "Approved", "tone": "success", "review": "Approved", "checks": { "passed": 3, "failed": 0, "running": 1 }, "mergeable": false }"##;

    fn reset(branch: Option<&str>, shown: bool) {
        host::testing::set_now(1_000_000.0);
        with(|p| *p = Pull { load: Load::Idle, branch: None, reading: None, merge: Merge::Idle });
        chat::with(|c| {
            c.cwd = "/work/shop".into();
            c.branch = branch.map(str::to_owned);
            c.side_open = shown;
            c.side_in_wings = false;
            c.panel_closed.clear();
        });
    }

    /// Answers the tracker's tool with `says`, counting the asks.
    fn tracker(says: Result<Value, String>) -> std::rc::Rc<std::cell::Cell<u32>> {
        let asks = std::rc::Rc::new(std::cell::Cell::new(0));
        let counted = asks.clone();
        host::testing::answer(move |method, params| {
            if method == "callTool" && params["name"] == TOOL {
                assert_eq!(params["arguments"]["cwd"], "/work/shop");
                counted.set(counted.get() + 1);
                return says.clone();
            }
            Ok(Value::Null)
        });
        asks
    }

    fn found() -> Pr {
        match parse(FOUND) {
            Load::Found(pr) => pr,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_trackers_answer_is_a_pr_none_a_folder_without_a_forge_or_a_failure() {
        let pr = found();
        assert_eq!((pr.number, pr.reference.as_str(), pr.state, pr.draft, pr.checks, pr.github, pr.mergeable), (12, "#12", State::Open, false, (3, 0, 1), true, false));
        assert_eq!(parse(r#"{ "supported": true, "found": false }"#), Load::NoPr);
        assert_eq!(parse(r#"{ "supported": false, "found": false }"#), Load::Unsupported);
        assert!(matches!(parse("not json"), Load::Failed(_)));
        let merged = parse(&FOUND.replace(r#""state": "open""#, r#""state": "merged""#).replace(r#""draft": false"#, r#""draft": true"#));
        assert!(matches!(merged, Load::Found(Pr { state: State::Merged, draft: true, .. })));
    }

    #[test]
    fn the_checks_say_the_failing_ones_first() {
        assert_eq!(checks_text((3, 1, 2)), "1 failed, 2 running, 3 passed");
        assert_eq!(checks_text((4, 0, 0)), "4 passed");
        assert_eq!(checks_text((0, 0, 0)), "No checks");
    }

    #[test]
    fn a_read_shows_the_pr_of_the_branch_and_a_failed_one_keeps_it() {
        reset(Some("cart"), true);
        let asks = tracker(Ok(json!({ "text": FOUND })));
        refresh();
        task::block_on(async {});
        assert_eq!(with(|p| (p.load.clone(), p.branch.clone())), (Load::Found(found()), Some("cart".into())));
        // The tracker turned off, or the host needs signing in: the panel keeps the PR it had for this branch.
        tracker(Ok(json!({ "isError": true, "text": "Sign in to GitHub" })));
        refresh();
        task::block_on(async {});
        assert_eq!(with(|p| p.load.clone()), Load::Found(found()));
        assert_eq!(asks.get(), 1);
        // On another branch nothing is kept, so the failure shows.
        chat::with(|c| c.branch = Some("other".into()));
        refresh();
        task::block_on(async {});
        assert_eq!(with(|p| p.load.clone()), Load::Failed("Sign in to GitHub".into()));
        // The tracker is off: the call is refused, and the panel has nothing to say.
        tracker(Err("pr-tracker is turned off or not installed".into()));
        refresh();
        task::block_on(async {});
        assert_eq!(with(|p| p.load.clone()), Load::Unsupported);
    }

    #[test]
    fn a_new_branch_is_read_at_once_and_one_with_a_pr_waits_for_the_poll() {
        reset(Some("cart"), true);
        let asks = tracker(Ok(json!({ "text": FOUND })));
        after_git();
        task::block_on(async {});
        assert_eq!(asks.get(), 1);
        after_git();
        task::block_on(async {});
        assert_eq!(asks.get(), 1, "the same branch, with a PR");
        chat::with(|c| c.branch = Some("fix".into()));
        after_git();
        task::block_on(async {});
        assert_eq!(asks.get(), 2);
        // A branch with no PR is looked at again after each turn: Claude may have made one.
        let asks = tracker(Ok(json!({ "text": r#"{ "supported": true, "found": false }"# })));
        chat::with(|c| c.branch = Some("docs".into()));
        after_git();
        task::block_on(async {});
        after_git();
        task::block_on(async {});
        assert_eq!(asks.get(), 2);
    }

    #[test]
    fn nothing_is_read_while_the_panel_is_not_shown_or_the_section_is_folded() {
        reset(Some("cart"), false);
        let asks = tracker(Ok(json!({ "text": FOUND })));
        tick();
        after_git();
        task::block_on(async {});
        chat::with(|c| {
            c.side_open = true;
            c.panel_closed = vec!["pull".into()];
        });
        tick();
        task::block_on(async {});
        assert_eq!(asks.get(), 0);
        chat::with(|c| c.panel_closed.clear());
        tick();
        task::block_on(async {});
        assert_eq!(asks.get(), 1);
    }

    #[test]
    fn a_chat_with_no_branch_has_nothing_to_show() {
        reset(None, true);
        with(|p| p.load = Load::NoPr);
        let asks = tracker(Ok(json!({ "text": FOUND })));
        refresh();
        task::block_on(async {});
        assert_eq!((asks.get(), with(|p| p.load.clone())), (0, Load::Idle));
    }

    #[test]
    fn merging_asks_first_only_for_a_github_pr_that_can_merge_and_names_the_way() {
        reset(Some("cart"), true);
        with(|p| p.load = Load::Found(found()));
        ask_merge();
        assert_eq!(with(|p| p.merge.clone()), Merge::Idle, "the tracker says it can't merge yet");
        with(|p| p.load = Load::Found(Pr { mergeable: true, ..found() }));
        ask_merge();
        assert_eq!(with(|p| p.merge.clone()), Merge::Asking);
        cancel_merge();
        assert_eq!(with(|p| p.merge.clone()), Merge::Idle);
        with(|p| p.load = Load::Found(Pr { mergeable: true, github: false, ..found() }));
        ask_merge();
        assert_eq!(with(|p| p.merge.clone()), Merge::Idle, "gh can't merge another forge's PR");
        assert_eq!(merge_args(12, Method::Squash), ["pr", "merge", "12", "--squash"]);
        assert_eq!(merge_args(12, Method::Rebase)[3], "--rebase");
        assert_eq!(merge_args(12, Method::Merge)[3], "--merge");
    }

    #[test]
    fn nothing_is_merged_without_the_dialog_and_a_refusal_shows_what_gh_said() {
        reset(Some("cart"), true);
        with(|p| p.load = Load::Found(Pr { mergeable: true, ..found() }));
        let ran = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Value>::new()));
        let seen = ran.clone();
        let refused = std::rc::Rc::new(std::cell::Cell::new(true));
        let says = refused.clone();
        host::testing::answer(move |method, params| {
            if method == "exec" && params["program"] == "gh" {
                seen.borrow_mut().push(params.clone());
                return Ok(if says.get() { json!({ "code": 1, "stdout": "", "stderr": "Pull request is not mergeable: the base branch policy prohibits the merge\n" }) } else { json!({ "code": 0, "stdout": "" }) });
            }
            Ok(Value::Null)
        });
        merge(Method::Squash);
        task::block_on(async {});
        assert!(ran.borrow().is_empty());
        ask_merge();
        merge(Method::Squash);
        task::block_on(async {});
        assert_eq!(ran.borrow().len(), 1);
        assert_eq!(ran.borrow()[0]["args"], json!(["pr", "merge", "12", "--squash"]));
        assert_eq!(ran.borrow()[0]["cwd"], "/work/shop");
        assert_eq!(with(|p| p.merge.clone()), Merge::Failed("Pull request is not mergeable: the base branch policy prohibits the merge".into()));
        refused.set(false);
        with(|p| p.merge = Merge::Asking);
        merge(Method::Merge);
        task::block_on(async {});
        assert_eq!(with(|p| p.merge.clone()), Merge::Idle);
    }

    #[test]
    fn the_prompts_name_the_branch_and_the_folder_claude_reads_the_logs_for() {
        assert!(create_prompt("cart").contains("(`cart`)"));
        let said = failing_prompt(&found(), "/work/shop");
        assert!(said.contains("pull request #12") && said.contains("failing_checks") && said.contains(r#"cwd "/work/shop""#));
    }
}
