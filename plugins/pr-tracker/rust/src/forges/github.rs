//! GitHub: the PR from the transcript or `gh pr list`, its status and failed Actions jobs, the diff, comments and replies.

use super::shared::{self, Facts, Words as Phrases, counts, label_of, no_log, or_else, same};
use crate::{
    checklist::{between, failing_of, real},
    forge::{ForgeError, Words},
    logs::{clean_log, detail},
    runlog::step_lines,
    snippet::diff_window,
    types::*,
};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::HashMap};
use wings_plugin::{
    cache::Cached,
    cli::Cli,
    github::{GithubRepo, github_pulls},
    host,
    remote::Remote,
    task::join,
};

const GH_FIELDS: &str = "number,title,state,isDraft,reviewDecision,mergeable,statusCheckRollup,additions,deletions,changedFiles,headRefName,baseRefName,url,updatedAt";

/// What one GraphQL query reads of each open PR: the status fields of `GH_FIELDS`, the author and the comment and review
/// counts. `reviewRequests` isn't read: it costs a point per repository each time, and the review-requested search that
/// rides along says which PRs ask for you or your team. Every nested connection with a `first` counts, so keep to
/// these two (measured: 1 point per repository for the PRs, 1 more per connection under them).
const GH_SELECTION: &str = "number title state isDraft reviewDecision mergeable additions deletions changedFiles headRefName baseRefName url updatedAt
  author { login } comments { totalCount } reviews { totalCount }
  statusCheckRollup { contexts(first: 100) { nodes { __typename
    ... on CheckRun { name status conclusion detailsUrl startedAt completedAt checkSuite { workflowRun { workflow { name } } } }
    ... on StatusContext { context state targetUrl description } } } }";
/// A little under the 60 s poll, so each poll reads again while the list and the pane statuses of one poll share one read.
const REPOS_TTL_MS: f64 = 55_000.0;

const DOTCOM: &str = "github.com";

/// The host a PR URL is on: github.com or a GitHub Enterprise Server.
fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    authority.rsplit('@').next().unwrap_or_default().to_lowercase()
}

/// The key of a host's sign-in problem: "github" for github.com, as it always was.
fn gh_key(host: &str) -> String {
    if host == DOTCOM { "github".to_owned() } else { format!("github:{host}") }
}

fn host_of_key(key: &str) -> &str {
    key.strip_prefix("github:").unwrap_or(DOTCOM)
}

/// `owner/repo` from a PR URL on any host.
fn repo_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let path = rest.split(['?', '#']).next().unwrap_or_default().split_once('/').map_or("", |(_, path)| path);
    let parts: Vec<&str> = path.split('/').collect();
    match parts.as_slice() {
        [owner, repo, "pull", ..] if !owner.is_empty() && !repo.is_empty() => format!("{owner}/{repo}"),
        _ => String::new(),
    }
}

/// `gh api` against the PR's host; gh's default is github.com.
fn api_args(host: &str, path: &str, rest: &[&str]) -> Vec<String> {
    let mut args = vec!["api".to_owned()];
    if host != DOTCOM {
        args.extend(["--hostname".to_owned(), host.to_owned()]);
    }
    args.push(path.to_owned());
    args.extend(rest.iter().map(|arg| (*arg).to_owned()));
    args
}

/// gh finds the repo of a folder by its remotes, but names the repo outright for an Enterprise host, where a remote that gh isn't signed in to is skipped.
fn repo_args(host: &str, path: &str) -> Vec<String> {
    if host == DOTCOM { vec![] } else { vec!["--repo".to_owned(), format!("{host}/{path}")] }
}

/// `wings.post` names the Enterprise host for `gh`; github.com is gh's default.
fn post_host(url: &str) -> Option<String> {
    Some(host_of(url)).filter(|host| host != DOTCOM)
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

thread_local! {
    // No token prints `gh auth login` or GH_TOKEN; an expired token that gh can renew prints "Try authenticating with:
    // gh auth refresh -h <host>" (cli/cli internal/ghcmd/cmd.go). A missing scope also suggests `gh auth refresh`, but
    // not after "Try authenticating with:", so it doesn't count.
    static GH: Cli = Cli::new("gh", "GitHub CLI", signed_out, &[]);
    /// The login `gh` is signed in as, per host, which marks the PRs you wrote and the ones that wait for you.
    static LOGINS: Cached<String, String> = Cached::new(f64::INFINITY, 0.0, |host: String| async move {
        let out = gh().run(&api_args(&host, "user", &["--jq", ".login"]), &gh_key(&host), false).await.map_err(|e| ForgeError::from(e).to_wire())?;
        let login = out.trim().to_owned();
        if login.is_empty() { Err("gh api user printed no login".to_owned()) } else { Ok(login) }
    });
    /// host → the open PRs of each repo, read through one GraphQL call for every repo asked at once, and kept for a poll so the sidebar lists and the pane statuses share it.
    static READERS: RefCell<HashMap<String, Cached<String, Listed>>> = RefCell::new(HashMap::new());
}

fn signed_out(stderr: &str) -> bool {
    stderr.contains("gh auth login") || stderr.contains("GH_TOKEN") || {
        stderr.find("Try authenticating with:").is_some_and(|at| {
            let rest = stderr[at + "Try authenticating with:".len()..].trim_start();
            rest.strip_prefix("gh auth refresh").is_some_and(|after| after.chars().next().is_none_or(|c| !(c.is_alphanumeric() || c == '_')))
        })
    }
}

pub fn cli() -> Cli {
    GH.with(Cli::clone)
}

fn gh() -> Cli {
    cli()
}

/// `gh` printing JSON, which is never nothing.
async fn gh_json(key: &str, args: Vec<String>, cwd: Option<&str>) -> Result<Value, ForgeError> {
    Ok(gh().json_in(cwd, &args, key, false, None).await?)
}

/// The open PRs of a repo as the GraphQL query reads them, and the numbers of the ones that ask for your review.
#[derive(Clone)]
struct Listed {
    pulls: Vec<Value>,
    asked: Vec<u64>,
}

fn open_pulls(host: &str) -> Cached<String, Listed> {
    READERS.with(|readers| {
        readers
            .borrow_mut()
            .entry(host.to_owned())
            .or_insert_with(|| {
                let reader = github_pulls(&gh(), GH_SELECTION, host, &gh_key(host));
                Cached::new(REPOS_TTL_MS, 0.0, move |repo: String| {
                    let reader = reader.clone();
                    async move {
                        let found = reader.get(parts_of(&repo)).await?;
                        Ok(Listed { pulls: found.pulls, asked: found.asked })
                    }
                })
            })
            .clone()
    })
}

/// `owner/repo` as the parts a GraphQL `repository` takes.
fn parts_of(repo: &str) -> GithubRepo {
    let (owner, name) = repo.split_once('/').unwrap_or((repo, ""));
    GithubRepo { owner: owner.to_owned(), name: name.to_owned() }
}

// ---------- the CLI's shapes and how they map ----------

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value[key].as_str()
}

fn owned(value: &Value, key: &str) -> Option<String> {
    text(value, key).map(str::to_owned)
}

/// The shape `gh pr view --json` gives, so one mapper serves the list and the PR's own read.
pub fn from_graphql(pr: &Value) -> Value {
    let mut out = pr.clone();
    let count = |key: &str| pr[key]["totalCount"].as_u64().unwrap_or(0);
    let author = if pr["author"].is_null() { Value::Null } else { pr["author"].clone() };
    let checks: Vec<Value> = pr["statusCheckRollup"]["contexts"]["nodes"]
        .as_array()
        .map(|nodes| {
            nodes
                .iter()
                .map(|node| {
                    let mut check = node.clone();
                    let workflow = node["checkSuite"]["workflowRun"]["workflow"]["name"].clone();
                    check.as_object_mut().map(|object| object.remove("checkSuite"));
                    check["workflowName"] = workflow;
                    check
                })
                .collect()
        })
        .unwrap_or_default();
    out["author"] = author;
    out["reviewDecision"] = pr["reviewDecision"].as_str().unwrap_or("").into();
    out["activity"] = (count("comments") + count("reviews")).into();
    out["statusCheckRollup"] = checks.into();
    if let Some(object) = out.as_object_mut() {
        object.remove("comments");
        object.remove("reviews");
    }
    out
}

fn job_state(status: &str, conclusion: Option<&str>) -> CheckState {
    if status != "completed" {
        return if status == "in_progress" { CheckState::Running } else { CheckState::Queued };
    }
    match conclusion {
        Some("success" | "neutral") => CheckState::Passed,
        Some("skipped") => CheckState::Skipped,
        _ => CheckState::Failed,
    }
}

/// A check run's job and its steps. GitHub keeps a job's log only once the job has finished, which `run_log` cuts into its steps.
pub fn gh_run(job: &Value) -> Run {
    let done = text(job, "status") == Some("completed");
    let steps = job["steps"]
        .as_array()
        .map(|steps| {
            steps
                .iter()
                .map(|step| RunStep {
                    id: step["number"].to_string(),
                    name: owned(step, "name").unwrap_or_default(),
                    state: job_state(text(step, "status").unwrap_or_default(), text(step, "conclusion")),
                    seconds: between(text(step, "started_at"), text(step, "completed_at")),
                    log: (done && step["started_at"].as_str().is_some_and(|at| !at.is_empty())).then(|| step["number"].to_string()),
                })
                .collect()
        })
        .unwrap_or_default();
    Run {
        url: owned(job, "html_url"),
        note: (!done).then(|| "GitHub shows the log once the job finishes.".to_owned()),
        jobs: vec![RunJob {
            id: job["id"].to_string(),
            name: owned(job, "name").unwrap_or_default(),
            stage: None,
            state: job_state(text(job, "status").unwrap_or_default(), text(job, "conclusion")),
            seconds: between(text(job, "started_at"), text(job, "completed_at")),
            steps,
        }],
    }
}

const GH_FAILED: [&str; 6] = ["FAILURE", "ERROR", "TIMED_OUT", "CANCELLED", "ACTION_REQUIRED", "STARTUP_FAILURE"];

fn gh_note(conclusion: &str) -> Option<&'static str> {
    Some(match conclusion {
        "TIMED_OUT" => "Timed out",
        "CANCELLED" => "Cancelled",
        "ACTION_REQUIRED" => "Action required",
        "STARTUP_FAILURE" => "Failed to start",
        _ => return None,
    })
}

pub fn gh_state(c: &Value) -> CheckState {
    if text(c, "__typename") == Some("StatusContext") {
        let state = text(c, "state").unwrap_or_default();
        return match state {
            "SUCCESS" => CheckState::Passed,
            "EXPECTED" => CheckState::Queued,
            _ if GH_FAILED.contains(&state) => CheckState::Failed,
            _ => CheckState::Running,
        };
    }
    if text(c, "status") == Some("COMPLETED") {
        let conclusion = text(c, "conclusion").unwrap_or_default();
        return match conclusion {
            "SUCCESS" | "NEUTRAL" => CheckState::Passed,
            "SKIPPED" => CheckState::Skipped,
            _ if GH_FAILED.contains(&conclusion) => CheckState::Failed,
            _ => CheckState::Running,
        };
    }
    if text(c, "status") == Some("IN_PROGRESS") { CheckState::Running } else { CheckState::Queued }
}

/// The id of the Actions job a check's link names, when the link is on `host`.
fn job_of(url: &str, host: &str) -> Option<String> {
    if !url.starts_with(&format!("https://{host}/")) {
        return None;
    }
    let after = &url[url.find("/actions/runs/")? + "/actions/runs/".len()..];
    let (run, rest) = after.split_at(after.find(|c: char| !c.is_ascii_digit())?);
    let id = rest.strip_prefix("/job/")?;
    let id: String = id.chars().take_while(char::is_ascii_digit).collect();
    (!run.is_empty() && !id.is_empty()).then_some(id)
}

/// A non-empty string, which is what `||` takes in the TypeScript.
fn nonempty<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    text(value, key).filter(|text| !text.is_empty())
}

/// `host` is the PR's host: only an Actions job there has a log to fetch. `index` is the check's place in the rollup: two contexts can share a name or a link, and each needs its own key.
pub fn gh_item(c: &Value, host: &str, index: usize) -> CheckItem {
    let url = text(c, "detailsUrl").or_else(|| text(c, "targetUrl"));
    let name = text(c, "name").or_else(|| text(c, "context")).unwrap_or("Check");
    let state = gh_state(c);
    let job = url.and_then(|url| job_of(url, host));
    let note = nonempty(c, "description").or_else(|| text(c, "conclusion").and_then(gh_note));
    let own = text(c, "name").or_else(|| text(c, "context"));
    let failing = Failing {
        id: url.or(own).unwrap_or_default().to_owned(),
        name: own.map(str::to_owned),
        url: url.map(str::to_owned),
        job: job.clone(),
        summary: nonempty(c, "description").map(str::to_owned),
        ..Failing::default()
    };
    CheckItem {
        id: format!("{name}|{}|{index}", url.unwrap_or_default()),
        name: name.to_owned(),
        state,
        // `gh pr view` doesn't say which checks branch protection requires.
        required: None,
        kind: Some(nonempty(c, "workflowName").map_or_else(|| if text(c, "__typename") == Some("StatusContext") { "Status check" } else { "Check run" }, |name| name).to_owned()),
        at: real(text(c, "completedAt")).or_else(|| real(text(c, "startedAt"))),
        seconds: between(text(c, "startedAt"), text(c, "completedAt")),
        url: url.map(str::to_owned),
        note: note.map(str::to_owned),
        failing: (state == CheckState::Failed).then_some(failing),
        run: job.map(|job| RunRef { job: Some(job), ..RunRef::default() }),
        needs: None,
    }
}

fn review_text(decision: &str) -> &'static str {
    match decision {
        "APPROVED" => "Approved",
        "CHANGES_REQUESTED" => "Changes requested",
        "REVIEW_REQUIRED" => "Waiting for review",
        _ => "No review yet",
    }
}

pub fn github_status_of(pr: &Value) -> Status {
    let url = text(pr, "url").unwrap_or_default();
    let host = host_of(url);
    let items: Vec<CheckItem> = pr["statusCheckRollup"].as_array().map(|checks| checks.iter().enumerate().map(|(i, c)| gh_item(c, &host, i)).collect()).unwrap_or_default();
    let results: Vec<&str> = items.iter().map(|c| match c.state {
        CheckState::Failed => "failed",
        CheckState::Running | CheckState::Queued => "running",
        _ => "passed",
    }).collect();
    let checks = counts(&results);
    let decision = text(pr, "reviewDecision").unwrap_or_default();
    let (label, tone, icon, state) = label_of(
        &Facts {
            merged: text(pr, "state") == Some("MERGED"),
            closed: text(pr, "state") == Some("CLOSED"),
            draft: pr["isDraft"] == true,
            conflicts: text(pr, "mergeable") == Some("CONFLICTING"),
            checks,
            changes: decision == "CHANGES_REQUESTED",
            approved: decision == "APPROVED",
            reviewing: decision == "REVIEW_REQUIRED",
            ..Facts::default()
        },
        &Phrases { rest: Some("Open"), ..Phrases::default() },
    );
    let n = |key: &str| pr[key].as_i64().unwrap_or(0);
    let updated = owned(pr, "updatedAt").unwrap_or_default();
    Status {
        key: updated.clone(),
        title: owned(pr, "title").unwrap_or_default(),
        label,
        tone,
        icon,
        state,
        checks,
        review: review_text(decision).to_owned(),
        // A GitHub Actions job on the PR's own host has a log; other apps' checks only link to their own page.
        failing: failing_of(&items),
        items: Some(items),
        changes: Some(format!("+{} −{} in {} files", n("additions"), n("deletions"), n("changedFiles"))),
        branch: format!("{} → {}", text(pr, "headRefName").unwrap_or_default(), text(pr, "baseRefName").unwrap_or_default()),
        updated,
        merge: None,
        base: None,
        head: None,
        base_commit: None,
        head_commit: None,
    }
}

/// The reviewers who gave a verdict, from `latestReviews`: a comment-only review isn't one.
pub fn github_reviewers(reviews: &Value) -> Vec<Reviewer> {
    reviews
        .as_array()
        .map(|reviews| {
            reviews
                .iter()
                .filter_map(|r| {
                    let vote = match text(r, "state")? {
                        "APPROVED" => "approved",
                        "CHANGES_REQUESTED" => "changes",
                        _ => return None,
                    };
                    Some(Reviewer { name: text(&r["author"], "login").unwrap_or("unknown").to_owned(), vote: vote.to_owned() })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Whose a listed PR is. `me` is the login `gh api user` gives. `requested` holds the numbers the search `review-requested:@me` found, which includes your teams.
pub fn gh_who(pr: &Value, me: &str, requested: &[u64]) -> (bool, bool) {
    (same(text(&pr["author"], "login"), me), pr["number"].as_u64().is_some_and(|number| requested.contains(&number)))
}

fn link_of_pr(pr: &Value) -> Link {
    let url = text(pr, "url").unwrap_or_default();
    Link { provider: Provider::Github, url: url.to_owned(), number: pr["number"].as_u64().unwrap_or(0), repo: repo_of(url), remote: None, cwd: None, raw: None }
}

/// Every open PR of a repo, each marked yours or waiting for you. The list carries what a PR's own status does, so the list's status stands for it.
pub fn github_listed(prs: &[Value], me: &str, requested: &[u64]) -> Vec<ListedPr> {
    prs.iter()
        .map(|pr| {
            let (mine, reviewing) = gh_who(pr, me, requested);
            ListedPr {
                link: link_of_pr(pr),
                title: owned(pr, "title").unwrap_or_default(),
                head: owned(pr, "headRefName").unwrap_or_default(),
                mine,
                reviewing,
                author: owned(&pr["author"], "login"),
                status: github_status_of(pr),
                loaded: true,
                activity: Some(pr["activity"].as_u64().unwrap_or(0)),
                updated_at: owned(pr, "updatedAt"),
            }
        })
        .collect()
}

/// A line comment from the REST API. GitHub threads only line comments, so only a thread's first one takes replies, on github.com and on an Enterprise host alike.
pub fn github_comment(c: &Value) -> Comment {
    let side = if text(c, "side") == Some("LEFT") { "deletions" } else { "additions" };
    let reply_to = c["in_reply_to_id"].as_i64().filter(|id| *id != 0);
    // The hunk is the diff as it was when the comment was written and ends at that line, so the thread's code is already in the comment: no call to read the file. `line` is where the line sits now, which the hunk's numbers don't follow once the PR has moved on.
    let line_of = |key: &str| c[key].as_i64().filter(|n| *n != 0);
    let end = line_of("original_line").or_else(|| line_of("line"));
    let first = line_of("original_start_line").or_else(|| line_of("start_line"));
    let snippet = match (reply_to, nonempty(c, "diff_hunk"), end) {
        (None, Some(hunk), Some(end)) => {
            let same_side = text(c, "start_side").or_else(|| text(c, "side")) == text(c, "side");
            diff_window(hunk, side, first.filter(|_| same_side).unwrap_or(end), end)
        }
        _ => None,
    };
    Comment {
        id: c["id"].as_i64().unwrap_or(0),
        reply_to,
        path: owned(c, "path"),
        line: c["line"].as_i64(),
        side: side.to_owned(),
        author: text(&c["user"], "login").unwrap_or("unknown").to_owned(),
        by: owned(&c["user"], "login"),
        body: text(c, "body").unwrap_or_default().to_owned(),
        created_at: owned(c, "created_at").unwrap_or_default(),
        url: owned(c, "html_url").unwrap_or_default(),
        can_reply: Some(reply_to.is_none()),
        discussion: None,
        thread: None,
        range: None,
        snippet,
        refs: None,
        system: false,
    }
}

/// The PR's conversation: comments on the whole PR, which are the conversation comments plus the review summaries, then
/// the line comments. The whole-PR ones have GraphQL string ids, so they get negative numbers that can't clash with
/// the line comments' ids.
pub fn github_conversation(discussion: &Value, line_comments: &[Value], url: &str) -> Vec<Comment> {
    let empty = Vec::new();
    let comments = discussion["comments"].as_array().unwrap_or(&empty).iter().map(|c| (c, text(c, "createdAt"), text(c, "url").unwrap_or_default()));
    let reviews = discussion["reviews"].as_array().unwrap_or(&empty).iter().filter(|r| nonempty(r, "body").is_some()).map(|r| (r, text(r, "submittedAt"), url));
    let general = comments.chain(reviews).enumerate().map(|(i, (c, at, link))| Comment {
        id: -(i as i64 + 1),
        reply_to: None,
        path: None,
        line: None,
        side: "additions".to_owned(),
        author: text(&c["author"], "login").unwrap_or("unknown").to_owned(),
        by: owned(&c["author"], "login"),
        body: text(c, "body").unwrap_or_default().to_owned(),
        created_at: at.unwrap_or_default().to_owned(),
        url: link.to_owned(),
        can_reply: None,
        discussion: None,
        thread: None,
        range: None,
        snippet: None,
        refs: None,
        system: false,
    });
    general.chain(line_comments.iter().map(github_comment)).collect()
}

/// The PR a `pr-link` transcript entry records, or none for any other entry.
pub fn pr_link_of(entry: &Value) -> Option<Link> {
    let url = text(entry, "prUrl").filter(|url| !url.is_empty())?;
    (text(entry, "type") == Some("pr-link")).then(|| Link {
        provider: Provider::Github,
        url: url.to_owned(),
        number: entry["prNumber"].as_u64().unwrap_or(0),
        repo: owned(entry, "prRepository").unwrap_or_default(),
        remote: None,
        cwd: None,
        raw: None,
    })
}

// ---------- calling it ----------

fn github_host(remote: &Remote) -> &str {
    match remote {
        Remote::Github { host, .. } => host,
        _ => DOTCOM,
    }
}

fn github_path(remote: &Remote) -> &str {
    match remote {
        Remote::Github { path, .. } => path,
        _ => "",
    }
}

pub fn problem_key(remote: &Remote) -> String {
    gh_key(github_host(remote))
}

pub fn repo_key(remote: &Remote) -> String {
    format!("github:{}/{}", github_host(remote), github_path(remote))
}

pub fn words(remote: &Remote) -> Words {
    let host = github_host(remote);
    Words {
        signed_out: format!("Sign in to {} to see this branch's pull request.", if host == DOTCOM { "GitHub" } else { host }),
        missing: "PR tracker needs the GitHub CLI (gh) to find this branch's pull request.".to_owned(),
    }
}

/// `gh` doesn't open the browser without a terminal: it prints a one-time code and a URL, so the badge shows the code
/// and Wings opens the URL.
pub async fn sign_in(remote: &Remote, on_code: impl Fn(&str) + 'static) -> Result<(), String> {
    let host = github_host(remote).to_owned();
    let args = strings(&["auth", "login", "--web", "--clipboard", "--hostname", &host]);
    gh().sign_in_args(Some(&gh_key(&host)), &args, move |line| {
        if let Some(code) = code_in(line) {
            on_code(code);
        }
        if let Some(url) = url_in(line, &host) {
            host::open_url(url);
        }
    })
    .await
}

/// The one-time code in a line of `gh auth login`, like `code: ABCD-1234` or `(code ABCD-1234`.
fn code_in(line: &str) -> Option<&str> {
    line.match_indices("code").find_map(|(at, word)| {
        let rest = &line[at + word.len()..];
        let rest = rest.strip_prefix(':').or_else(|| rest.strip_prefix(" ("))?.trim_start();
        let code = rest.get(..9)?;
        let (a, b) = code.split_once('-')?;
        let part = |p: &str| p.len() == 4 && p.bytes().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
        (part(a) && part(b)).then_some(code)
    })
}

/// The first `https://<host>/...` in a line.
fn url_in<'a>(line: &'a str, host: &str) -> Option<&'a str> {
    let start = line.find(&format!("https://{host}/"))?;
    let rest = &line[start..];
    Some(rest.split(char::is_whitespace).next().unwrap_or(rest))
}

/// A Claude pane can know its PR from the transcript without gh, so this can be the first gh call.
async fn view(url: &str, fields: &str) -> Result<Value, ForgeError> {
    gh_json(&gh_key(&host_of(url)), strings(&["pr", "view", url, "--json", fields]), None).await
}

async fn comments_inner(link: &Link) -> Result<Vec<Comment>, ForgeError> {
    let host = host_of(&link.url);
    let key = gh_key(&host);
    let lines = gh_json(&key, api_args(&host, &format!("repos/{}/pulls/{}/comments", link.repo, link.number), &["--paginate", "--slurp"]), None);
    let discussion = gh_json(&key, strings(&["pr", "view", &link.url, "--json", "comments,reviews"]), None);
    let (lines, discussion) = join(lines, discussion).await;
    let lines = or_else(lines, json!([]))?;
    let discussion = or_else(discussion, json!({ "comments": [], "reviews": [] }))?;
    let flat: Vec<Value> = lines.as_array().map(|pages| pages.iter().flat_map(|page| page.as_array().cloned().unwrap_or_default()).collect()).unwrap_or_default();
    Ok(github_conversation(&discussion, &flat, &link.url))
}

pub async fn from_session(session_id: &str) -> Option<Link> {
    // A brand-new session may not have a transcript yet.
    let entries = host::transcript(session_id, &["pr-link"], Some(1)).await.ok()?;
    pr_link_of(entries.as_array()?.last()?)
}

pub async fn find(cwd: &str, remote: &Remote, branch: &str) -> Result<Option<Link>, ForgeError> {
    let mut args = strings(&["pr", "list"]);
    args.extend(repo_args(github_host(remote), github_path(remote)));
    args.extend(strings(&["--head", branch, "--state", "all", "--limit", "1", "--json", "url,number"]));
    // gh ran and failed for a reason that isn't being signed out: nothing to show.
    let prs = or_else(gh_json(&problem_key(remote), args, Some(cwd)).await, Value::Null)?;
    Ok(prs.get(0).map(|pr| Link {
        provider: Provider::Github,
        url: text(pr, "url").unwrap_or_default().to_owned(),
        number: pr["number"].as_u64().unwrap_or(0),
        repo: repo_of(text(pr, "url").unwrap_or_default()),
        remote: None,
        cwd: None,
        raw: None,
    }))
}

async fn login(host: &str) -> Result<String, ForgeError> {
    Ok(LOGINS.with(Cached::clone).get(host.to_owned()).await?)
}

/// The login `gh` is signed in as, which is also what a mention is written with.
pub async fn me(remote: &Remote) -> Result<Me, ForgeError> {
    let login = login(github_host(remote)).await?;
    Ok(Me { ids: vec![login.clone()], handles: vec![login] })
}

/// Every open PR of the repo (up to 100), each marked yours or waiting for you, from one GraphQL call that every repo asked in the same turn shares.
pub async fn list(_cwd: &str, remote: &Remote) -> Result<Vec<ListedPr>, ForgeError> {
    let host = github_host(remote);
    let (user, found) = join(login(host), async { open_pulls(host).get(github_path(remote).to_lowercase()).await.map_err(ForgeError::from) }).await;
    let (user, found) = (user?, found?);
    let pulls: Vec<Value> = found.pulls.iter().map(from_graphql).collect();
    Ok(github_listed(&pulls, &user, &found.asked))
}

pub async fn link_of(args: &PrArgs) -> Result<Link, ForgeError> {
    Ok(Link { provider: Provider::Github, url: args.url.clone(), number: args.number, repo: args.repo.clone(), remote: None, cwd: None, raw: None })
}

/// An open PR's status is in its repo's list, which the sidebar reads anyway: `gh pr view` is for one the list doesn't have, like a merged one. `fresh` skips a list that's still kept.
pub async fn status(link: &Link, fresh: bool) -> Result<Status, ForgeError> {
    let reader = open_pulls(&host_of(&link.url));
    let repo = link.repo.to_lowercase();
    if fresh {
        reader.forget(&repo);
    }
    // A list that can't be read leaves it to `gh pr view`, which fails the same way when it is a sign-in.
    let open = reader.get(repo).await.ok();
    let pr = open.and_then(|open| open.pulls.into_iter().find(|p| p["number"].as_u64() == Some(link.number)));
    Ok(github_status_of(&match pr {
        Some(pr) => from_graphql(&pr),
        None => view(&link.url, GH_FIELDS).await?,
    }))
}

pub async fn overview(link: &Link) -> Result<Overview, ForgeError> {
    let pr = view(&link.url, &format!("{GH_FIELDS},body,author,createdAt,latestReviews")).await?;
    Ok(Overview {
        status: github_status_of(&pr),
        author: text(&pr["author"], "login").unwrap_or("unknown").to_owned(),
        body: text(&pr, "body").unwrap_or_default().to_owned(),
        created_at: owned(&pr, "createdAt"),
        reviewers: Some(github_reviewers(&pr["latestReviews"])),
    })
}

pub async fn diff(link: &Link, _status: &Status) -> Result<(String, Vec<Comment>), ForgeError> {
    let patch = async { Ok::<_, ForgeError>(gh().run(&strings(&["pr", "diff", &link.url]), &gh_key(&host_of(&link.url)), false).await?) };
    let (patch, conversation) = join(patch, comments_inner(link)).await;
    Ok((patch?, conversation?))
}

pub async fn comments(link: &Link) -> Result<Vec<Comment>, ForgeError> {
    comments_inner(link).await
}

/// A failed GitHub Actions job: its failed step and the end of its log. Unlike `gh run view`, this works while the rest of the run is still going.
pub async fn failure(link: &Link, f: &Failing) -> Result<Vec<FailureDetail>, ForgeError> {
    let Some(job) = &f.job else {
        return Ok(no_log(f.name.as_deref(), f.summary.as_deref().unwrap_or("Failed"), f.url.as_deref()));
    };
    let host = host_of(&link.url);
    let key = gh_key(&host);
    let steps = gh_json(&key, api_args(&host, &format!("repos/{}/actions/jobs/{job}", link.repo), &[]), None);
    // The log keeps the CI's colours, which gh only prints with this flag.
    let log = async { Ok::<_, ForgeError>(gh().run(&api_args(&host, &format!("repos/{}/actions/jobs/{job}/logs", link.repo), &["--allow-escape-sequences"]), &key, false).await?) };
    let (job, log) = join(steps, log).await;
    let job = or_else(job, Value::Null)?;
    let step = job["steps"].as_array().and_then(|steps| steps.iter().find(|s| text(s, "conclusion") == Some("failure"))).and_then(|s| text(s, "name"));
    let lines: Vec<String> = log?.split('\n').map(str::to_owned).collect();
    let mut lines = clean_log(&lines);
    // The runner's own cleanup follows the failed step and would push the error out of the tail.
    if let Some(cleanup) = lines.iter().position(|line| line == "Post job cleanup.") {
        lines.truncate(cleanup);
    }
    Ok(vec![detail(f.name.as_deref(), step, &lines, &[], f.url.as_deref())])
}

pub async fn run(link: &Link, reference: &RunRef) -> Result<Run, ForgeError> {
    let host = host_of(&link.url);
    let job = gh_json(&gh_key(&host), api_args(&host, &format!("repos/{}/actions/jobs/{}", link.repo, reference.job.clone().unwrap_or_default()), &[]), None).await?;
    Ok(gh_run(&job))
}

/// Actions keeps one log per job, which is cut into its steps by when each ran.
pub async fn run_log(link: &Link, reference: &RunRef, step: &RunStep) -> Result<Vec<String>, ForgeError> {
    let host = host_of(&link.url);
    let key = gh_key(&host);
    let id = reference.job.clone().unwrap_or_default();
    let job = gh_json(&key, api_args(&host, &format!("repos/{}/actions/jobs/{id}", link.repo), &[]), None);
    let log = async { Ok::<_, ForgeError>(gh().run(&api_args(&host, &format!("repos/{}/actions/jobs/{id}/logs", link.repo), &["--allow-escape-sequences"]), &key, false).await?) };
    let (job, log) = join(job, log).await;
    let (job, log) = (job?, log?);
    let steps = job["steps"].as_array().cloned().unwrap_or_default();
    let times: Vec<(Option<String>, Option<String>)> = steps.iter().map(|s| (owned(s, "started_at"), owned(s, "completed_at"))).collect();
    let lines: Vec<String> = log.split('\n').map(str::to_owned).collect();
    let mut parts = step_lines(&lines, &times);
    Ok(steps.iter().position(|s| s["number"].to_string() == step.id).and_then(|at| (at < parts.len()).then(|| parts.swap_remove(at))).unwrap_or_default())
}

pub async fn reply(link: &Link, root: &Comment, body: &str) -> Result<Comment, ForgeError> {
    let url = format!("repos/{}/pulls/{}/comments/{}/replies", link.repo, link.number, root.id);
    let note = shared::post_json(&gh(), &url, json!({ "body": body }), post_host(&link.url).as_deref(), &gh_key(&host_of(&link.url))).await?;
    Ok(github_comment(&note))
}

/// Approves as the signed-in user. The manifest pins the review's `event` to APPROVE, so this call can't request changes or comment.
pub async fn approve(link: &Link, _sha: &str) -> Result<(), ForgeError> {
    let url = format!("repos/{}/pulls/{}/reviews", link.repo, link.number);
    shared::post_json(&gh(), &url, json!({ "event": "APPROVE" }), post_host(&link.url).as_deref(), &gh_key(&host_of(&link.url))).await.map(drop)
}

pub fn forget() {
    LOGINS.with(Cached::clear);
    READERS.with(|readers| readers.borrow_mut().clear());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checklist::has_log;
    use wings_plugin::{host::testing, task::block_on};

    fn merged(base: Value, over: Value) -> Value {
        let mut base = base;
        for (key, value) in over.as_object().unwrap() {
            base[key] = value.clone();
        }
        base
    }

    fn pr(over: Value) -> Value {
        merged(
            json!({
                "number": 12, "url": "https://github.com/o/r/pull/12", "title": "Fix x", "state": "OPEN", "isDraft": false, "reviewDecision": "", "mergeable": "MERGEABLE",
                "statusCheckRollup": [], "additions": 5, "deletions": 2, "changedFiles": 3, "headRefName": "fix", "baseRefName": "main", "updatedAt": "2026-10-01T10:00:00Z", "author": { "login": "ada" },
            }),
            over,
        )
    }

    fn run_check(conclusion: &str, name: &str) -> Value {
        json!({ "__typename": "CheckRun", "name": name, "status": "COMPLETED", "conclusion": conclusion, "detailsUrl": format!("https://github.com/o/r/actions/runs/9/job/{}", name.len()) })
    }

    #[test]
    fn github_status_the_first_thing_that_is_true_names_the_pr_from_merged_down_to_the_review_decision() {
        let label = |over: Value| github_status_of(&pr(over)).label;
        let fail = || run_check("FAILURE", "unit");
        assert_eq!(label(json!({ "state": "MERGED", "isDraft": true })), "Merged");
        assert_eq!(label(json!({ "state": "CLOSED", "isDraft": true })), "Closed");
        assert_eq!(label(json!({ "isDraft": true, "mergeable": "CONFLICTING" })), "Draft");
        assert_eq!(label(json!({ "mergeable": "CONFLICTING", "statusCheckRollup": [fail()] })), "Conflicts");
        assert_eq!(label(json!({ "statusCheckRollup": [fail()], "reviewDecision": "CHANGES_REQUESTED" })), "Checks failing");
        let mut running = run_check("SUCCESS", "unit");
        running["status"] = "IN_PROGRESS".into();
        assert_eq!(label(json!({ "reviewDecision": "CHANGES_REQUESTED", "statusCheckRollup": [running] })), "Changes requested");
        assert_eq!(label(json!({ "statusCheckRollup": [{ "__typename": "CheckRun", "name": "e2e", "status": "IN_PROGRESS" }], "reviewDecision": "APPROVED" })), "Checks running");
        assert_eq!(label(json!({ "reviewDecision": "APPROVED" })), "Approved");
        assert_eq!(label(json!({ "reviewDecision": "REVIEW_REQUIRED" })), "In review");
        assert_eq!(label(json!({})), "Open", "a PR with no review policy has nothing to wait for");
        let st = github_status_of(&pr(json!({ "state": "MERGED" })));
        assert_eq!((st.tone.as_str(), st.icon.as_str()), ("merged", "pr-merged"));
    }

    #[test]
    fn github_status_carries_the_key_branches_size_review_text_and_the_failed_jobs_that_have_a_log() {
        let st = github_status_of(&pr(json!({
            "reviewDecision": "REVIEW_REQUIRED",
            "statusCheckRollup": [run_check("SUCCESS", "lint"), run_check("FAILURE", "unit"), { "__typename": "StatusContext", "context": "legacy", "state": "SUCCESS" }],
        })));
        assert_eq!(
            (st.key.as_str(), st.title.as_str(), st.branch.as_str(), st.changes.as_deref(), st.review.as_str(), st.updated.as_str()),
            ("2026-10-01T10:00:00Z", "Fix x", "fix → main", Some("+5 −2 in 3 files"), "Waiting for review", "2026-10-01T10:00:00Z")
        );
        assert_eq!(st.checks, Counts { passed: 2, failed: 1, running: 0 });
        assert_eq!(st.failing.iter().map(|f| (f.name.clone(), f.job.clone())).collect::<Vec<_>>(), [(Some("unit".to_owned()), Some("4".to_owned()))]);
        assert_eq!(st.items.unwrap().len(), 3);
        assert_eq!(github_status_of(&pr(json!({ "reviewDecision": "CHANGES_REQUESTED" }))).review, "Changes requested");
        assert_eq!(github_status_of(&pr(json!({}))).review, "No review yet");
    }

    #[test]
    fn github_list_marks_the_prs_you_wrote_or_the_review_search_found_matching_the_login_without_case_and_carries_the_full_status() {
        let prs = [pr(json!({ "number": 1, "author": { "login": "Me" } })), pr(json!({ "number": 2 })), pr(json!({ "number": 4, "activity": 3 }))];
        let rows = github_listed(&prs, "me", &[4]);
        assert_eq!(rows.iter().map(|r| (r.link.number, r.mine, r.reviewing)).collect::<Vec<_>>(), [(1, true, false), (2, false, false), (4, false, true)]);
        assert_eq!(rows[0].link, Link { provider: Provider::Github, url: "https://github.com/o/r/pull/12".into(), number: 1, repo: "o/r".into(), remote: None, cwd: None, raw: None });
        assert_eq!((rows[0].head.as_str(), rows[0].author.as_deref(), rows[0].loaded, rows[2].activity), ("fix", Some("Me"), true, Some(3)));
        assert_eq!(rows[0].status.title, "Fix x");
    }

    #[test]
    fn github_you_wrote_it_or_the_search_for_your_review_requests_your_teams_included_found_it() {
        assert_eq!(gh_who(&json!({ "number": 7, "author": { "login": "Me" } }), "me", &[]), (true, false));
        assert_eq!(gh_who(&json!({ "number": 7, "author": { "login": "ann" } }), "me", &[7]), (false, true));
        assert_eq!(gh_who(&json!({ "number": 8, "author": {} }), "me", &[7]), (false, false));
    }

    #[test]
    fn a_graphql_pr_becomes_what_gh_pr_view_gives_the_workflow_name_on_a_check_run_no_decision_as_empty_the_counts_as_activity() {
        let node = json!({
            "number": 3, "url": "https://github.com/o/r/pull/3", "title": "T", "state": "OPEN", "isDraft": false, "reviewDecision": null, "mergeable": "MERGEABLE", "additions": 1, "deletions": 2, "changedFiles": 1,
            "headRefName": "h", "baseRefName": "main", "updatedAt": "t", "author": null, "comments": { "totalCount": 2 }, "reviews": { "totalCount": 1 },
            "statusCheckRollup": { "contexts": { "nodes": [
                { "__typename": "CheckRun", "name": "unit", "status": "COMPLETED", "conclusion": "FAILURE", "detailsUrl": "https://github.com/o/r/actions/runs/9/job/5", "checkSuite": { "workflowRun": { "workflow": { "name": "CI" } } } },
                { "__typename": "StatusContext", "context": "legacy", "state": "SUCCESS" },
            ] } },
        });
        let gh = from_graphql(&node);
        assert_eq!((gh["reviewDecision"].clone(), gh["activity"].clone(), gh["author"].clone()), (json!(""), json!(3), Value::Null));
        assert_eq!((gh["statusCheckRollup"][0]["workflowName"].clone(), gh["statusCheckRollup"][1]["workflowName"].clone()), (json!("CI"), Value::Null));
        let st = github_status_of(&gh);
        let items = st.items.as_ref().unwrap();
        assert_eq!((st.label.as_str(), items[0].kind.as_deref(), items[0].run.clone(), st.failing.len()), ("Checks failing", Some("CI"), Some(RunRef { job: Some("5".into()), ..RunRef::default() }), 1));
        let mut bare = node.clone();
        bare["statusCheckRollup"] = Value::Null;
        assert_eq!(github_status_of(&from_graphql(&bare)).checks.failed, 0, "a PR with no checks has no rollup");
    }

    #[test]
    fn a_reviews_verdict_is_a_reviewers_vote_and_a_comment_only_review_is_none() {
        let reviewers = github_reviewers(&json!([{ "author": { "login": "a" }, "state": "APPROVED" }, { "author": {}, "state": "CHANGES_REQUESTED" }, { "author": { "login": "c" }, "state": "COMMENTED" }]));
        assert_eq!(reviewers, [Reviewer { name: "a".into(), vote: "approved".into() }, Reviewer { name: "unknown".into(), vote: "changes".into() }]);
        assert!(github_reviewers(&Value::Null).is_empty());
    }

    #[test]
    fn github_conversation_comments_on_the_whole_pr_first_with_ids_that_cant_clash_with_line_comments_then_the_line_comments() {
        let discussion = json!({
            "comments": [{ "author": { "login": "ada" }, "body": "hi", "createdAt": "t1", "url": "https://github.com/o/r/pull/12#c1" }, { "body": "from nobody", "createdAt": "t2", "url": "u2" }],
            "reviews": [{ "author": { "login": "bob" }, "body": "LGTM", "submittedAt": "t3" }, { "author": { "login": "cy" }, "body": "", "submittedAt": "t4" }],
        });
        let line = json!({ "id": 55, "path": "a.ts", "line": 4, "side": "RIGHT", "user": { "login": "dee" }, "body": "nit", "created_at": "t5", "html_url": "u5" });
        let list = github_conversation(&discussion, &[line], "https://github.com/o/r/pull/12");
        assert_eq!(
            list.iter().map(|c| (c.id, c.author.as_str(), c.path.as_deref(), c.url.as_str())).collect::<Vec<_>>(),
            [
                (-1, "ada", None, "https://github.com/o/r/pull/12#c1"),
                (-2, "unknown", None, "u2"),
                (-3, "bob", None, "https://github.com/o/r/pull/12"),
                (55, "dee", Some("a.ts"), "u5"),
            ]
        );
        assert_eq!(list[0].can_reply, None, "the whole-PR ones take no reply");
    }

    #[test]
    fn a_github_line_comment_sits_on_a_side_and_only_a_threads_first_comment_takes_a_reply() {
        let first = github_comment(&json!({ "id": 1, "path": "a.ts", "line": 3, "side": "LEFT", "user": { "login": "a" }, "body": "x", "created_at": "t", "html_url": "u" }));
        assert_eq!((first.side.as_str(), first.reply_to, first.can_reply, first.line), ("deletions", None, Some(true), Some(3)));
        let reply = github_comment(&json!({ "id": 2, "in_reply_to_id": 1, "path": "a.ts", "side": "RIGHT", "created_at": "t", "html_url": "u" }));
        assert_eq!((reply.side.as_str(), reply.reply_to, reply.can_reply, reply.line, reply.author.as_str(), reply.body.as_str()), ("additions", Some(1), Some(false), None, "unknown", ""));
    }

    #[test]
    fn github_check_runs_and_status_contexts_map_to_a_state() {
        let run = |status: &str, conclusion: Option<&str>| gh_state(&json!({ "__typename": "CheckRun", "status": status, "conclusion": conclusion }));
        assert_eq!(run("COMPLETED", Some("SUCCESS")), CheckState::Passed);
        assert_eq!(run("COMPLETED", Some("NEUTRAL")), CheckState::Passed);
        assert_eq!(run("COMPLETED", Some("SKIPPED")), CheckState::Skipped);
        for conclusion in ["FAILURE", "TIMED_OUT", "CANCELLED", "ACTION_REQUIRED", "STARTUP_FAILURE"] {
            assert_eq!(run("COMPLETED", Some(conclusion)), CheckState::Failed, "{conclusion}");
        }
        assert_eq!(run("IN_PROGRESS", None), CheckState::Running);
        for status in ["QUEUED", "WAITING", "PENDING", "REQUESTED"] {
            assert_eq!(run(status, None), CheckState::Queued, "{status}");
        }
        let context = |state: &str| gh_state(&json!({ "__typename": "StatusContext", "state": state }));
        assert_eq!(context("SUCCESS"), CheckState::Passed);
        assert_eq!(context("ERROR"), CheckState::Failed);
        assert_eq!(context("PENDING"), CheckState::Running);
        assert_eq!(context("EXPECTED"), CheckState::Queued);
    }

    #[test]
    fn a_failed_actions_job_on_the_prs_host_can_be_read_another_apps_check_only_links_out() {
        let job = gh_item(
            &json!({ "__typename": "CheckRun", "name": "unit", "workflowName": "CI", "status": "COMPLETED", "conclusion": "FAILURE", "detailsUrl": "https://github.com/o/r/actions/runs/9/job/77", "startedAt": "2026-01-01T00:00:00Z", "completedAt": "2026-01-01T00:01:30Z" }),
            "github.com",
            0,
        );
        assert_eq!(job.failing.as_ref().unwrap().job.as_deref(), Some("77"));
        assert!(has_log(job.failing.as_ref()));
        assert_eq!(job.kind.as_deref(), Some("CI"));
        assert_eq!(job.seconds, Some(90.0));
        assert_eq!(job.at.as_deref(), Some("2026-01-01T00:01:30Z"));
        let app = gh_item(&json!({ "__typename": "CheckRun", "name": "deploy", "status": "COMPLETED", "conclusion": "TIMED_OUT", "detailsUrl": "https://ci.example.com/b/1" }), "github.com", 0);
        assert!(!has_log(app.failing.as_ref()));
        assert_eq!(app.note.as_deref(), Some("Timed out"));
        let ctx = gh_item(&json!({ "__typename": "StatusContext", "context": "legacy/ci", "state": "SUCCESS", "targetUrl": "https://ci.example.com/2", "startedAt": "0001-01-01T00:00:00Z" }), "github.com", 0);
        assert_eq!(ctx.kind.as_deref(), Some("Status check"));
        assert_eq!(ctx.at, None);
        assert_eq!(ctx.failing, None);
    }

    #[test]
    fn two_github_contexts_that_share_a_link_and_even_a_name_get_different_ids() {
        let ctx = |context: &str, i: usize| gh_item(&json!({ "__typename": "StatusContext", "context": context, "state": "SUCCESS", "targetUrl": "https://codecov.example/pr/1" }), "github.com", i);
        assert_ne!(ctx("codecov/patch", 0).id, ctx("codecov/project", 1).id);
        assert_ne!(ctx("codecov/patch", 0).id, ctx("codecov/patch", 1).id);
    }

    #[test]
    fn gh_run_gives_no_step_a_log_until_the_job_finishes_because_github_has_none_before() {
        let step = json!({ "number": 1, "name": "Set up job", "status": "completed", "conclusion": "success", "started_at": "2026-10-17T07:00:00Z", "completed_at": "2026-10-17T07:00:02Z" });
        let running = gh_run(&json!({ "id": 7, "name": "test", "status": "in_progress", "conclusion": null, "steps": [step] }));
        assert_eq!(running.jobs[0].steps[0].log, None);
        assert!(running.note.unwrap().contains("once the job finishes"));
        let done = gh_run(&json!({ "id": 7, "name": "test", "status": "completed", "conclusion": "success", "steps": [step] }));
        assert_eq!(done.jobs[0].steps[0].log.as_deref(), Some("1"));
        assert_eq!(done.note, None);
    }

    #[test]
    fn the_signed_out_and_login_lines_are_told_from_gh_output() {
        assert!(signed_out("To get started with GitHub CLI, please run:  gh auth login"));
        assert!(signed_out("Try authenticating with:  gh auth refresh -h ghe.example.com"));
        assert!(!signed_out("missing scope; run gh auth refresh -s repo"));
        assert_eq!(code_in("! First copy your one-time code: ABCD-1234"), Some("ABCD-1234"));
        assert_eq!(code_in("one-time code (WXYZ-0001)"), Some("WXYZ-0001"));
        assert_eq!(code_in("a code 9F2K-77PQ"), None);
        assert_eq!(url_in("Open https://ghe.example.com/login/device in a browser", "ghe.example.com"), Some("https://ghe.example.com/login/device"));
    }

    #[test]
    fn a_pr_link_entry_gives_a_link_and_any_other_entry_none() {
        let link = pr_link_of(&json!({ "type": "pr-link", "prUrl": "https://github.com/o/r/pull/3", "prNumber": 3, "prRepository": "o/r" })).unwrap();
        assert_eq!((link.number, link.repo.as_str()), (3, "o/r"));
        assert!(pr_link_of(&json!({ "type": "user", "prUrl": "x" })).is_none());
    }

    #[test]
    fn a_status_of_an_open_pr_costs_the_one_graphql_call_every_repo_of_the_turn_shares_and_a_merged_one_a_pr_view() {
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let seen = calls.clone();
        testing::answer(move |method, params| {
            if method == "post" {
                seen.borrow_mut().push("graphql".to_owned());
                let data = json!({ "data": { "r0": { "pullRequests": { "nodes": [merged(pr(json!({})), json!({ "comments": { "totalCount": 0 }, "reviews": { "totalCount": 0 }, "statusCheckRollup": null }))] } }, "asked": { "nodes": [] } } });
                return Ok(json!({ "code": 0, "stdout": data.to_string(), "stderr": "" }));
            }
            assert_eq!(method, "exec");
            seen.borrow_mut().push(format!("gh {}", params["args"][0].as_str().unwrap()));
            Ok(json!({ "code": 0, "stdout": pr(json!({ "number": 13, "state": "MERGED" })).to_string(), "stderr": "" }))
        });
        let open = Link { provider: Provider::Github, url: "https://github.com/o/r/pull/12".into(), number: 12, repo: "o/r".into(), remote: None, cwd: None, raw: None };
        let done = Link { number: 13, url: "https://github.com/o/r/pull/13".into(), ..open.clone() };
        let (a, b) = block_on(async move { join(status(&open, false), status(&done, false)).await });
        assert_eq!((a.unwrap().label, b.unwrap().label), ("Open".to_owned(), "Merged".to_owned()));
        assert_eq!(*calls.borrow(), ["graphql", "gh pr"]);
    }
}
