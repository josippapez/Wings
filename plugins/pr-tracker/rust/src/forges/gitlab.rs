//! GitLab: the merge request for the branch, its pipeline, approvals and discussions, all through `glab api`.

use super::shared::{self, Facts, Words as Phrases, counts, label_of, ladder, no_log, open_or_newest, same};
use crate::{
    checklist::real,
    forge::{AccountKind, ForgeError, Words},
    logs::{clean_log, detail},
    snippet::diff_window,
    types::*,
};
use serde_json::{Value, json};
use wings_plugin::{
    cache::Cached,
    cli::{Cli, CliError, Kind, last_line},
    remote::Remote,
    task::join,
};

fn key_of(host: &str) -> String {
    format!("gitlab:{host}")
}

fn host_of_key(key: &str) -> &str {
    key.strip_prefix("gitlab:").unwrap_or_default()
}

/// Whether `word` stands alone in `text`, like a regex's `\b404\b`.
fn has_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(at, _)| {
        let edge = |c: Option<char>| c.is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
        edge(text[..at].chars().next_back()) && edge(text[at + word.len()..].chars().next())
    })
}

thread_local! {
    static GLAB: Cli = Cli::new("glab", "GitLab CLI", |stderr| has_word(stderr, "401"), &[]);
    /// The username `glab` is signed in as, per host.
    static USERS: Cached<String, String> = Cached::new(f64::INFINITY, 0.0, |host: String| async move {
        let user = api(&host, "user", false).await.map_err(|e| e.to_wire())?;
        match user["username"].as_str().filter(|name| !name.is_empty()) {
            Some(name) => Ok(name.to_owned()),
            None => Err("GitLab didn't say who you are.".to_owned()),
        }
    });
    /// The files an MR's commits changed, as GitLab diffs them. One call serves every thread between the same two commits.
    static COMPARED: Cached<(String, String, String, String), Value> = Cached::new(f64::INFINITY, 0.0, |(host, path, base, head): (String, String, String, String)| async move {
        api(&host, &format!("{}/repository/compare?from={base}&to={head}&straight=true", project(&path)), false).await.map_err(|e| e.to_wire())
    });
}

pub fn cli() -> Cli {
    GLAB.with(Cli::clone)
}

// ---------- the API's shapes and how they map ----------

const GL_QUEUED: [&str; 5] = ["created", "pending", "scheduled", "waiting_for_resource", "waiting_for_callback"];

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value[key].as_str()
}

fn owned(value: &Value, key: &str) -> Option<String> {
    text(value, key).map(str::to_owned)
}

fn id_text(value: &Value) -> String {
    value["id"].to_string()
}

/// A pipeline job as a check of the Checks view.
pub fn gl_item(j: &Value, project_id: Option<u64>) -> CheckItem {
    let status = text(j, "status").unwrap_or_default();
    let allow_failure = j["allow_failure"] == true;
    let (state, note) = match status {
        "success" => (CheckState::Passed, None),
        "skipped" => (CheckState::Skipped, Some("Skipped")),
        "manual" => (CheckState::Skipped, Some("Manual job: start it in GitLab")),
        "failed" if allow_failure => (CheckState::Passed, Some("Failed, but allowed to fail")),
        "failed" => (CheckState::Failed, None),
        "canceled" | "canceling" => (CheckState::Failed, Some("Canceled")),
        _ if GL_QUEUED.contains(&status) => (CheckState::Queued, None),
        _ => (CheckState::Running, None),
    };
    let project_id = project_id.filter(|id| *id != 0);
    CheckItem {
        id: id_text(j),
        name: owned(j, "name").unwrap_or_default(),
        state,
        required: Some(!allow_failure),
        kind: Some(text(j, "stage").filter(|stage| !stage.is_empty()).map_or_else(|| "Job".to_owned(), |stage| format!("Job in {stage}"))),
        at: real(text(j, "finished_at")).or_else(|| real(text(j, "started_at"))),
        seconds: j["duration"].as_f64(),
        url: owned(j, "web_url"),
        note: note.map(str::to_owned),
        failing: (state == CheckState::Failed).then(|| Failing {
            id: id_text(j),
            name: owned(j, "name"),
            job: Some(id_text(j)),
            project: project_id,
            url: owned(j, "web_url"),
            summary: (status == "canceled").then(|| "Canceled".to_owned()),
            build: None,
        }),
        run: project_id.map(|project| RunRef { job: Some(id_text(j)), project: Some(project), build: None }),
        needs: None,
    }
}

fn branch_of(mr: &Value) -> String {
    format!("{} → {}", text(mr, "source_branch").unwrap_or_default(), text(mr, "target_branch").unwrap_or_default())
}

/// GitLab status from the MR, its approvals and its head pipeline's jobs.
pub fn gl_status_of(mr: &Value, approvals: &Value, jobs: &[Value]) -> Status {
    let project = mr["head_pipeline"]["project_id"].as_u64();
    // Manual jobs haven't run and don't block, so they don't count. A failure that's allowed to fail passes.
    let counted: Vec<&Value> = jobs.iter().filter(|j| text(j, "status") != Some("manual")).collect();
    let results: Vec<&str> = counted
        .iter()
        .map(|j| match text(j, "status").unwrap_or_default() {
            "success" | "skipped" => "passed",
            "failed" if j["allow_failure"] == true => "passed",
            "failed" | "canceled" => "failed",
            _ => "running",
        })
        .collect();
    let checks = counts(&results);
    let failing = counted
        .iter()
        .zip(&results)
        .filter(|(_, result)| **result == "failed")
        .map(|(j, _)| Failing {
            id: id_text(j),
            name: owned(j, "name"),
            job: Some(id_text(j)),
            project,
            url: owned(j, "web_url"),
            summary: (text(j, "status") == Some("canceled")).then(|| "Canceled".to_owned()),
            build: None,
        })
        .collect();
    let merge = text(mr, "detailed_merge_status");
    let state = text(mr, "state");
    let (label, tone, icon, state) = label_of(
        &Facts {
            merged: state == Some("merged"),
            closed: matches!(state, Some("closed" | "locked")),
            draft: mr["draft"] == true,
            conflicts: mr["has_conflicts"] == true || merge == Some("conflict"),
            checks,
            changes: merge == Some("requested_changes"),
            unresolved: merge == Some("discussions_not_resolved"),
            approved: approvals["approved"] == true,
            ..Facts::default()
        },
        &Phrases::default(),
    );
    let approved_by = approvals["approved_by"].as_array().map_or(0, Vec::len);
    let left = approvals["approvals_left"].as_u64().unwrap_or(0);
    let mut review = Vec::new();
    if approved_by > 0 {
        review.push(format!("{approved_by} approved"));
    }
    if left > 0 {
        review.push(format!("{left} more needed"));
    }
    Status {
        // A new push changes the head commit, which is what invalidates the cached diff.
        key: owned(mr, "sha").unwrap_or_default(),
        title: owned(mr, "title").unwrap_or_default(),
        label,
        tone,
        icon,
        state,
        checks,
        review: if review.is_empty() { "No approvals yet".to_owned() } else { review.join(", ") },
        failing,
        // Manual jobs are listed, though they don't count.
        items: Some(jobs.iter().map(|j| gl_item(j, project)).collect()),
        changes: text(mr, "changes_count").filter(|count| !count.is_empty()).map(|count| format!("{count} files")),
        branch: branch_of(mr),
        updated: owned(mr, "updated_at").unwrap_or_default(),
        merge: None,
        base: None,
        head: None,
        base_commit: None,
        head_commit: None,
    }
}

/// What the list can say without a call per MR: the merge status stands in for the pipeline's jobs.
pub fn gl_list_status(mr: &Value) -> Status {
    let merge = text(mr, "detailed_merge_status");
    let conflicts = mr["has_conflicts"] == true || merge == Some("conflict");
    let (label, tone, icon) = ladder(
        &[
            (mr["draft"] == true, "Draft", "neutral", Some("pr-draft")),
            (conflicts, "Conflicts", "danger", None),
            (merge == Some("ci_must_pass"), "Checks not passing", "warning", None),
            (merge == Some("requested_changes"), "Changes requested", "danger", None),
            (merge == Some("ci_still_running"), "Checks running", "warning", None),
            (merge == Some("discussions_not_resolved"), "Unresolved threads", "warning", None),
            (merge == Some("mergeable"), "Ready to merge", "success", None),
        ],
        None,
    );
    Status {
        key: owned(mr, "sha").unwrap_or_default(),
        title: owned(mr, "title").unwrap_or_default(),
        label,
        tone,
        icon,
        // The list asks for open MRs only.
        state: PrState::Open,
        checks: Counts::default(),
        review: String::new(),
        failing: vec![],
        items: None,
        changes: None,
        branch: branch_of(mr),
        updated: owned(mr, "updated_at").unwrap_or_default(),
        merge: None,
        base: None,
        head: None,
        base_commit: None,
        head_commit: None,
    }
}

fn name_of(user: &Value) -> Option<String> {
    owned(user, "name").or_else(|| owned(user, "username"))
}

/// Whose a listed MR is. `me` is the username `glab api user` gives.
pub fn gl_who(mr: &Value, me: &str) -> (bool, bool) {
    let reviewing = mr["reviewers"].as_array().is_some_and(|reviewers| reviewers.iter().any(|r| same(text(r, "username"), me)));
    (same(text(&mr["author"], "username"), me), reviewing)
}

/// Every open MR of the project, each marked yours or waiting for you.
pub fn gl_listed(mrs: &[Value], me: &str, to_link: impl Fn(&Value) -> Link) -> Vec<ListedPr> {
    mrs.iter()
        .map(|mr| {
            let (mine, reviewing) = gl_who(mr, me);
            ListedPr {
                link: to_link(mr),
                title: owned(mr, "title").unwrap_or_default(),
                head: owned(mr, "source_branch").unwrap_or_default(),
                mine,
                reviewing,
                author: name_of(&mr["author"]),
                status: gl_list_status(mr),
                loaded: false,
                activity: mr["user_notes_count"].as_u64(),
                updated_at: owned(mr, "updated_at"),
            }
        })
        .collect()
}

pub fn gl_reviewers(approvals: &Value) -> Vec<Reviewer> {
    approvals["approved_by"]
        .as_array()
        .map(|approved| approved.iter().map(|a| Reviewer { name: name_of(&a["user"]).unwrap_or_else(|| "unknown".to_owned()), vote: "approved".to_owned() }).collect())
        .unwrap_or_default()
}

/// A line number that counts: GitLab's null and 0 are none.
fn line(value: &Value) -> Option<i64> {
    value.as_i64().filter(|n| *n != 0)
}

/// The MR's discussions as comments. `system` adds GitLab's own notes ("added 2 commits"), marked as such.
pub fn gl_comments(discussions: &Value, url: &str, system: bool) -> Vec<Comment> {
    let mut comments = Vec::new();
    for d in discussions.as_array().into_iter().flatten() {
        let notes: Vec<&Value> = d["notes"].as_array().into_iter().flatten().filter(|n| system || n["system"] != true).collect();
        for (i, n) in notes.iter().enumerate() {
            // Only diff notes have a position; the rest are comments on the whole MR.
            let pos = (text(n, "type") == Some("DiffNote")).then(|| &n["position"]).filter(|pos| !pos.is_null());
            let right = pos.is_some_and(|pos| line(&pos["new_line"]).is_some());
            let end = pos.and_then(|pos| pos["new_line"].as_i64().or_else(|| pos["old_line"].as_i64()));
            let range_start = pos.map(|pos| &pos["line_range"]["start"]).and_then(|start| if right { start["new_line"].as_i64() } else { start["old_line"].as_i64() });
            let start = range_start.or(end);
            let first = i == 0;
            comments.push(Comment {
                id: n["id"].as_i64().unwrap_or(0),
                reply_to: if first { None } else { notes[0]["id"].as_i64() },
                path: pos.and_then(|pos| owned(pos, "new_path").or_else(|| owned(pos, "old_path"))),
                line: pos.and_then(|pos| pos["new_line"].as_i64().or_else(|| pos["old_line"].as_i64())),
                side: if right { "additions" } else { "deletions" }.to_owned(),
                author: name_of(&n["author"]).unwrap_or_else(|| "unknown".to_owned()),
                by: owned(&n["author"], "username"),
                body: text(n, "body").unwrap_or_default().to_owned(),
                created_at: owned(n, "created_at").unwrap_or_default(),
                url: url.to_owned(),
                can_reply: Some(first && n["system"] != true),
                discussion: owned(d, "id"),
                thread: (first && n["resolvable"] == true).then(|| {
                    if n["resolved"] == true { ThreadState { state: "resolved".into(), label: "Resolved".into() } } else { ThreadState { state: "active".into(), label: "Unresolved".into() } }
                }),
                system: n["system"] == true,
                range: match (first, start.filter(|n| *n != 0), end.filter(|n| *n != 0)) {
                    (true, Some(start), Some(end)) => Some(LineRange { start, end, iteration: None }),
                    _ => None,
                },
                refs: pos.filter(|_| first).and_then(|pos| match (nonempty(pos, "base_sha"), nonempty(pos, "head_sha")) {
                    (Some(base), Some(head)) => Some(CommitRefs { base: base.to_owned(), head: head.to_owned() }),
                    _ => None,
                }),
                snippet: None,
            });
        }
    }
    comments
}

fn nonempty<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    text(value, key).filter(|text| !text.is_empty())
}

/// The comment a reply made, in the thread of `root`.
pub fn gl_replied(root: &Comment, note: &Value, body: &str) -> Comment {
    Comment {
        id: note["id"].as_i64().unwrap_or(0),
        reply_to: Some(root.id),
        author: name_of(&note["author"]).unwrap_or_else(|| "you".to_owned()),
        body: text(note, "body").unwrap_or(body).to_owned(),
        created_at: owned(note, "created_at").unwrap_or_default(),
        can_reply: Some(false),
        ..root.clone()
    }
}

// ---------- calling it ----------

/// `encodeURIComponent`.
fn component(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'!' | b'\'' | b'(' | b')' | b'*') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn api_args(host: &str, endpoint: &str, paginate: bool) -> Vec<String> {
    let mut args = vec!["api".to_owned(), "--hostname".to_owned(), host.to_owned()];
    if paginate {
        args.push("--paginate".to_owned());
    }
    args.push(endpoint.to_owned());
    args
}

/// A call whose program never ran (it printed nothing to stderr, as a program that didn't start does) can't say which MR a branch has: the pane says PRs are unavailable. A command that ran and failed is just an error.
fn ran(error: CliError) -> ForgeError {
    if error.kind == Kind::Failed && error.message == "glab failed" {
        return ForgeError::account(AccountKind::Unavailable, error.message);
    }
    error.into()
}

/// Runs `glab args` for the host. A private project looks missing (404) to a signed-out user, so a 404 asks glab whether
/// it's signed in. Public projects answer some calls without sign-in, so a sign-in problem stays until Try again or a
/// sign-in, or the badge would flip back and forth.
async fn glab_run(host: &str, args: Vec<String>) -> Result<String, CliError> {
    let (glab, key) = (cli(), key_of(host));
    let before = glab.problem(&key);
    match glab.run(&args, &key, false).await {
        Ok(out) => {
            if let Some((Kind::SignedOut, message)) = before {
                glab.record_signed_out(&key, &message);
            }
            Ok(out)
        }
        Err(error) if error.kind == Kind::Failed && has_word(&error.message, "404") => {
            let status = ["auth", "status", "--hostname", host_of_key(&key)].map(str::to_owned);
            if glab.exec(&status, &key, true).await.is_ok_and(|out| out.code != 0) {
                glab.record_signed_out(&key, last_line(&error.message));
                return Err(CliError { kind: Kind::SignedOut, message: error.message });
            }
            Err(error)
        }
        Err(error) => Err(error),
    }
}

/// `glab api` against the host, which prints JSON.
async fn api(host: &str, endpoint: &str, paginate: bool) -> Result<Value, ForgeError> {
    let out = glab_run(host, api_args(host, endpoint, paginate)).await.map_err(ran)?;
    if out.trim().is_empty() {
        return Err(ForgeError::Other("glab printed nothing".to_owned()));
    }
    serde_json::from_str(&out).map_err(|error| ForgeError::Other(error.to_string()))
}

fn host_path(remote: &Remote) -> (&str, &str) {
    match remote {
        Remote::Gitlab { host, path } => (host, path),
        _ => ("", ""),
    }
}

fn project(path: &str) -> String {
    format!("projects/{}", component(path))
}

fn mr_path(link: &Link) -> String {
    let (_, path) = host_path(link.remote.as_ref().unwrap_or(&Remote::None { host: None }));
    format!("{}/merge_requests/{}", project(path), link.number)
}

fn link_host(link: &Link) -> &str {
    host_path(link.remote.as_ref().unwrap_or(&Remote::None { host: None })).0
}

fn to_link(mr: &Value, remote: &Remote) -> Link {
    Link {
        provider: Provider::Gitlab,
        url: owned(mr, "web_url").unwrap_or_default(),
        number: mr["iid"].as_u64().unwrap_or(0),
        repo: host_path(remote).1.to_owned(),
        remote: Some(remote.clone()),
        cwd: None,
        raw: None,
    }
}

/// The MR with what a status is made of: its approvals and its head pipeline's jobs, which a failure of either leaves out.
async fn load(link: &Link) -> Result<(Value, Value, Vec<Value>), ForgeError> {
    let host = link_host(link);
    let mr = api(host, &mr_path(link), false).await?;
    let pipeline = mr["head_pipeline"].clone();
    let approvals = async { api(host, &format!("{}/approvals", mr_path(link)), false).await.unwrap_or(Value::Null) };
    let jobs = async {
        if pipeline.is_null() {
            return vec![];
        }
        let endpoint = format!("projects/{}/pipelines/{}/jobs?per_page=100", pipeline["project_id"], pipeline["id"]);
        api(host, &endpoint, true).await.ok().and_then(|jobs| jobs.as_array().cloned()).unwrap_or_default()
    };
    let (approvals, jobs) = join(approvals, jobs).await;
    Ok((mr, approvals, jobs))
}

async fn comments_of(link: &Link, system: bool) -> Vec<Comment> {
    let discussions = api(link_host(link), &format!("{}/discussions?per_page=100", mr_path(link)), true).await.unwrap_or_else(|_| json!([]));
    gl_comments(&discussions, &link.url, system)
}

pub fn problem_key(remote: &Remote) -> String {
    key_of(host_path(remote).0)
}

pub fn repo_key(remote: &Remote) -> String {
    let (host, path) = host_path(remote);
    format!("gitlab:{host}/{path}")
}

pub fn words(_remote: &Remote) -> Words {
    Words {
        signed_out: "Sign in to GitLab to see this branch's merge request.".to_owned(),
        missing: "PR tracker needs the GitLab CLI (glab) to find this branch's pull request.".to_owned(),
    }
}

pub async fn sign_in(remote: &Remote, _on_code: impl Fn(&str) + 'static) -> Result<(), String> {
    let host = host_path(remote).0;
    let args = ["auth", "login", "--web", "--hostname", host].map(str::to_owned);
    cli().sign_in_args(Some(&key_of(host)), &args, |_| {}).await
}

pub async fn find(_cwd: &str, remote: &Remote, branch: &str) -> Result<Option<Link>, ForgeError> {
    let (host, path) = host_path(remote);
    let endpoint = format!("{}/merge_requests?source_branch={}&state=all&order_by=updated_at&per_page=5", project(path), component(branch));
    let mrs = api(host, &endpoint, false).await?;
    let mrs = mrs.as_array().cloned().unwrap_or_default();
    Ok(open_or_newest(&mrs, |m| text(m, "state") == Some("opened")).map(|mr| to_link(mr, remote)))
}

async fn username(host: &str) -> Result<String, ForgeError> {
    Ok(USERS.with(Cached::clone).get(host.to_owned()).await?)
}

/// The username `glab` is signed in as, which is also what a mention is written with.
pub async fn me(remote: &Remote) -> Result<Me, ForgeError> {
    let name = username(host_path(remote).0).await?;
    Ok(Me { ids: vec![name.clone()], handles: vec![name] })
}

/// Every open MR of the project (up to 100) in one call, each marked yours or waiting for you.
pub async fn list(_cwd: &str, remote: &Remote) -> Result<Vec<ListedPr>, ForgeError> {
    let (host, path) = host_path(remote);
    let endpoint = format!("{}/merge_requests?state=opened&order_by=updated_at&per_page=100", project(path));
    let (user, mrs) = join(username(host), api(host, &endpoint, false)).await;
    let (user, mrs) = (user?, mrs?);
    Ok(gl_listed(mrs.as_array().map_or(&[], Vec::as_slice), &user, |mr| to_link(mr, remote)))
}

pub async fn link_of(args: &PrArgs) -> Result<Link, ForgeError> {
    match &args.remote {
        Some(remote @ Remote::Gitlab { .. }) => Ok(Link { provider: Provider::Gitlab, url: args.url.clone(), number: args.number, repo: args.repo.clone(), remote: Some(remote.clone()), cwd: None, raw: None }),
        _ => Err(ForgeError::Other("This tab doesn't say which GitLab project its merge request is in.".to_owned())),
    }
}

pub async fn status(link: &Link, _fresh: bool) -> Result<Status, ForgeError> {
    let (mr, approvals, jobs) = load(link).await?;
    Ok(gl_status_of(&mr, &approvals, &jobs))
}

pub async fn overview(link: &Link) -> Result<Overview, ForgeError> {
    let (mr, approvals, jobs) = load(link).await?;
    Ok(Overview {
        status: gl_status_of(&mr, &approvals, &jobs),
        author: name_of(&mr["author"]).unwrap_or_else(|| "unknown".to_owned()),
        body: text(&mr, "description").unwrap_or_default().to_owned(),
        created_at: owned(&mr, "created_at"),
        reviewers: Some(gl_reviewers(&approvals)),
    })
}

/// The lines GitLab's own thread shows: the diff of the file between the note's commits, up to the commented line.
pub async fn snippet(link: &Link, root: &Comment, _pushes: &[Push], _head: Option<&str>, _now: bool) -> Result<Option<Vec<SnippetLine>>, ForgeError> {
    let (Some(path), Some(range), Some(refs)) = (&root.path, &root.range, &root.refs) else {
        return Ok(None);
    };
    let (host, project_path) = host_path(link.remote.as_ref().unwrap_or(&Remote::None { host: None }));
    let compared = COMPARED.with(Cached::clone).get((host.to_owned(), project_path.to_owned(), refs.base.clone(), refs.head.clone())).await?;
    let file = compared["diffs"].as_array().and_then(|diffs| diffs.iter().find(|d| text(d, "new_path") == Some(path) || text(d, "old_path") == Some(path)));
    Ok(file.and_then(|file| diff_window(text(file, "diff").unwrap_or_default(), &root.side, range.start, range.end)))
}

pub async fn diff(link: &Link, _status: &Status) -> Result<(String, Vec<Comment>), ForgeError> {
    let host = link_host(link);
    let (_, path) = host_path(link.remote.as_ref().unwrap_or(&Remote::None { host: None }));
    let args = ["mr", "diff", &link.number.to_string(), "--raw", "--repo", &format!("https://{host}/{path}")].map(str::to_owned).to_vec();
    let patch = async { glab_run(host, args).await.map_err(ForgeError::from) };
    let (patch, conversation) = join(patch, comments_of(link, false)).await;
    Ok((patch?, conversation))
}

/// The MR's discussions with GitLab's own notes, which the diff viewer leaves out.
pub async fn comments(link: &Link) -> Result<Vec<Comment>, ForgeError> {
    Ok(comments_of(link, true).await)
}

/// A failed GitLab job: its trace.
pub async fn failure(link: &Link, f: &Failing) -> Result<Vec<FailureDetail>, ForgeError> {
    if let Some(summary) = &f.summary {
        return Ok(no_log(f.name.as_deref(), summary, f.url.as_deref()));
    }
    let trace = trace_of(link, f.project, f.job.as_deref()).await?;
    let lines: Vec<String> = trace.split('\n').map(str::to_owned).collect();
    Ok(vec![detail(f.name.as_deref(), None, &clean_log(&lines), &[], f.url.as_deref())])
}

async fn trace_of(link: &Link, project: Option<u64>, job: Option<&str>) -> Result<String, ForgeError> {
    let host = link_host(link);
    let endpoint = format!("projects/{}/jobs/{}/trace", project.unwrap_or(0), job.unwrap_or_default());
    glab_run(host, api_args(host, &endpoint, false)).await.map_err(ran)
}

/// A GitLab job has no steps of its own: its log is one, folded by the sections the runner marks.
pub async fn run(link: &Link, reference: &RunRef) -> Result<Run, ForgeError> {
    let endpoint = format!("projects/{}/jobs/{}", reference.project.unwrap_or(0), reference.job.as_deref().unwrap_or_default());
    let j = api(link_host(link), &endpoint, false).await?;
    let item = gl_item(&j, reference.project);
    Ok(Run {
        url: owned(&j, "web_url"),
        note: None,
        jobs: vec![RunJob {
            id: id_text(&j),
            name: owned(&j, "name").unwrap_or_default(),
            stage: None,
            state: item.state,
            seconds: item.seconds,
            steps: vec![RunStep { id: "log".to_owned(), name: "Job log".to_owned(), state: item.state, seconds: item.seconds, log: Some("trace".to_owned()) }],
        }],
    })
}

pub async fn run_log(link: &Link, reference: &RunRef, _step: &RunStep) -> Result<Vec<String>, ForgeError> {
    Ok(trace_of(link, reference.project, reference.job.as_deref()).await?.split('\n').map(str::to_owned).collect())
}

pub async fn reply(link: &Link, root: &Comment, body: &str) -> Result<Comment, ForgeError> {
    let url = format!("{}/discussions/{}/notes", mr_path(link), root.discussion.as_deref().unwrap_or_default());
    let host = link_host(link);
    let note = shared::post_json(&cli(), &url, json!({ "body": body }), Some(host), &key_of(host)).await?;
    Ok(gl_replied(root, &note, body))
}

/// Approves the head commit you loaded (`sha`, the status's key), not whatever the head is now: GitLab answers 409 when
/// the branch has moved on, so you never approve commits you haven't seen.
pub async fn approve(link: &Link, sha: &str) -> Result<(), ForgeError> {
    let host = link_host(link);
    match shared::post_json(&cli(), &format!("{}/approve", mr_path(link)), json!({ "sha": sha }), Some(host), &key_of(host)).await {
        Ok(_) => Ok(()),
        Err(error) => {
            let message = error.to_string();
            let lower = message.to_lowercase();
            if has_word(&message, "409") || lower.contains("conflict") || lower.contains("does not match") {
                return Err(ForgeError::Other("New commits since you loaded this PR. Refresh and review again.".to_owned()));
            }
            Err(error)
        }
    }
}

pub fn forget() {
    USERS.with(Cached::clear);
}

#[cfg(test)]
mod tests {
    use super::*;
    use wings_plugin::{host::testing, task::block_on};

    fn mr(over: Value) -> Value {
        let mut base = json!({
            "iid": 5, "web_url": "https://gitlab.example.com/g/p/-/merge_requests/5", "state": "opened", "title": "Add y", "author": { "name": "Ada", "username": "ada" }, "reviewers": [],
            "user_notes_count": 4, "sha": "c0ffee", "source_branch": "feat", "target_branch": "main", "updated_at": "2026-10-01T10:00:00Z", "changes_count": "7", "head_pipeline": { "id": 31, "project_id": 3 },
        });
        for (key, value) in over.as_object().unwrap() {
            base[key] = value.clone();
        }
        base
    }

    fn job(status: &str, extra: Value) -> Value {
        let mut base = json!({ "id": 5, "name": "test", "status": status, "stage": "verify", "web_url": "https://gitlab.example.com/j/5", "duration": 61.2 });
        for (key, value) in extra.as_object().unwrap() {
            base[key] = value.clone();
        }
        base
    }

    fn so(over: Value, jobs: &[Value], approvals: Value) -> Status {
        gl_status_of(&mr(over), &approvals, jobs)
    }

    #[test]
    fn gitlab_status_the_first_thing_that_is_true_names_the_mr_from_merged_down_to_approved() {
        let label = |over: Value, jobs: &[Value], approvals: Value| so(over, jobs, approvals).label;
        let none = || Value::Null;
        assert_eq!(label(json!({ "state": "merged", "draft": true }), &[], none()), "Merged");
        assert_eq!(label(json!({ "state": "locked" }), &[], none()), "Closed");
        assert_eq!(label(json!({ "draft": true, "has_conflicts": true }), &[], none()), "Draft");
        assert_eq!(label(json!({ "detailed_merge_status": "conflict" }), &[job("failed", json!({}))], none()), "Conflicts");
        assert_eq!(label(json!({ "detailed_merge_status": "requested_changes" }), &[job("failed", json!({}))], none()), "Checks failing");
        assert_eq!(label(json!({ "detailed_merge_status": "requested_changes" }), &[job("running", json!({}))], none()), "Changes requested");
        assert_eq!(label(json!({ "detailed_merge_status": "discussions_not_resolved" }), &[job("running", json!({}))], none()), "Checks running");
        assert_eq!(label(json!({ "detailed_merge_status": "discussions_not_resolved" }), &[], json!({ "approved": true })), "Unresolved threads");
        assert_eq!(label(json!({}), &[], json!({ "approved": true })), "Approved");
        assert_eq!(label(json!({}), &[], none()), "In review");
    }

    #[test]
    fn gitlab_status_manual_jobs_dont_count_allowed_failures_pass_canceled_ones_fail_and_the_review_says_who_is_missing() {
        let jobs = [
            job("success", json!({ "id": 1 })),
            job("manual", json!({ "id": 2 })),
            job("failed", json!({ "id": 3, "allow_failure": true })),
            job("canceled", json!({ "id": 4 })),
            job("running", json!({ "id": 6 })),
            job("failed", json!({ "id": 7 })),
        ];
        let st = so(json!({}), &jobs, json!({ "approved_by": [{ "user": { "username": "bob" } }], "approvals_left": 2 }));
        assert_eq!(st.checks, Counts { passed: 2, failed: 2, running: 1 });
        let failing: Vec<_> = st.failing.iter().map(|f| (f.id.clone(), f.summary.clone(), f.project)).collect();
        assert_eq!(failing, [("4".into(), Some("Canceled".into()), Some(3)), ("7".into(), None, Some(3))]);
        assert_eq!(st.items.as_ref().unwrap().len(), 6, "manual jobs are listed, though they don't count");
        assert_eq!((st.review.as_str(), st.key.as_str(), st.branch.as_str(), st.changes.as_deref()), ("1 approved, 2 more needed", "c0ffee", "feat → main", Some("7 files")));
        assert_eq!(so(json!({ "changes_count": null }), &[], Value::Null).changes, None);
        assert_eq!(so(json!({}), &[], Value::Null).review, "No approvals yet");
    }

    #[test]
    fn gitlab_list_status_stands_in_for_the_pipeline_with_the_merge_status() {
        let label = |over: Value| gl_list_status(&mr(over)).label;
        assert_eq!(label(json!({ "draft": true, "has_conflicts": true })), "Draft");
        assert_eq!(label(json!({ "has_conflicts": true })), "Conflicts");
        assert_eq!(label(json!({ "detailed_merge_status": "ci_must_pass" })), "Checks not passing");
        assert_eq!(label(json!({ "detailed_merge_status": "requested_changes" })), "Changes requested");
        assert_eq!(label(json!({ "detailed_merge_status": "ci_still_running" })), "Checks running");
        assert_eq!(label(json!({ "detailed_merge_status": "discussions_not_resolved" })), "Unresolved threads");
        let ready = gl_list_status(&mr(json!({ "detailed_merge_status": "mergeable" })));
        assert_eq!((ready.label.as_str(), ready.tone.as_str()), ("Ready to merge", "success"));
        assert_eq!(label(json!({ "detailed_merge_status": "checking" })), "In review");
        assert_eq!(gl_list_status(&mr(json!({}))).checks, Counts::default());
    }

    fn stub_link(mr: &Value) -> Link {
        Link { provider: Provider::Gitlab, url: String::new(), number: mr["iid"].as_u64().unwrap(), repo: String::new(), remote: None, cwd: None, raw: None }
    }

    #[test]
    fn gitlab_list_marks_the_mrs_you_wrote_or_review_by_username_and_says_the_list_has_no_check_counts() {
        let mut no_notes = mr(json!({}));
        no_notes.as_object_mut().unwrap().remove("user_notes_count");
        let mrs = [mr(json!({ "author": { "username": "Me", "name": "Me M" } })), mr(json!({ "reviewers": [{ "username": "ME" }] })), no_notes];
        let rows = gl_listed(&mrs, "me", stub_link);
        assert_eq!(rows.iter().map(|r| (r.mine, r.reviewing)).collect::<Vec<_>>(), [(true, false), (false, true), (false, false)]);
        assert_eq!((rows[0].author.as_deref(), rows[0].head.as_str(), rows[0].activity, rows[2].activity, rows[0].loaded), (Some("Me M"), "feat", Some(4), None, false));
        assert_eq!(gl_who(&json!({ "author": { "username": "ann" }, "reviewers": [{ "username": "bob" }, {}] }), "me"), (false, false));
    }

    #[test]
    fn gitlab_jobs_manual_ones_are_not_run_allowed_failures_dont_block_canceled_ones_failed() {
        let item = |status: &str, extra: Value| gl_item(&job(status, extra), Some(3));
        assert_eq!(item("success", json!({})).state, CheckState::Passed);
        assert_eq!(item("manual", json!({})).state, CheckState::Skipped);
        assert_eq!(item("skipped", json!({})).state, CheckState::Skipped);
        assert_eq!(item("running", json!({})).state, CheckState::Running);
        for s in ["created", "pending", "scheduled", "waiting_for_resource"] {
            assert_eq!(item(s, json!({})).state, CheckState::Queued, "{s}");
        }
        let allowed = item("failed", json!({ "allow_failure": true }));
        assert_eq!((allowed.state, allowed.required, allowed.failing), (CheckState::Passed, Some(false), None));
        let failed = item("failed", json!({}));
        assert_eq!((failed.state, failed.required, failed.kind.as_deref(), failed.failing.as_ref().unwrap().project, failed.seconds), (CheckState::Failed, Some(true), Some("Job in verify"), Some(3), Some(61.2)));
        assert_eq!(item("canceled", json!({})).state, CheckState::Failed);
        assert_eq!(item("canceled", json!({})).note.as_deref(), Some("Canceled"));
    }

    #[test]
    fn gitlab_reviewers_are_the_approvals_by_name_else_username() {
        let reviewers = gl_reviewers(&json!({ "approved_by": [{ "user": { "name": "Ann", "username": "ann" } }, { "user": { "username": "bob" } }, {}] }));
        let names: Vec<_> = reviewers.iter().map(|r| (r.name.as_str(), r.vote.as_str())).collect();
        assert_eq!(names, [("Ann", "approved"), ("bob", "approved"), ("unknown", "approved")]);
        assert!(gl_reviewers(&Value::Null).is_empty());
    }

    #[test]
    fn gitlab_discussions_a_diff_note_has_its_file_and_line_replies_sit_under_the_first_note_and_system_notes_are_left_out_unless_asked_for() {
        let discussions = json!([
            { "id": "d1", "notes": [
                { "id": 1, "type": "DiffNote", "resolvable": true, "resolved": false, "position": { "new_path": "a.ts", "old_path": "a.ts", "new_line": 12, "old_line": null }, "author": { "name": "Ann" }, "body": "why?", "created_at": "t1" },
                { "id": 2, "author": { "username": "bob" }, "body": "because", "created_at": "t2" },
            ] },
            { "id": "d2", "notes": [{ "id": 3, "system": true, "author": { "username": "gitlab" }, "body": "added 2 commits", "created_at": "t3" }] },
            { "id": "d3", "notes": [{ "id": 4, "type": "DiffNote", "resolvable": true, "resolved": true, "position": { "old_path": "b.ts", "new_line": null, "old_line": 3 }, "author": { "name": "Cy" }, "body": "gone", "created_at": "t4" }] },
        ]);
        let url = "https://gitlab.example.com/g/p/-/merge_requests/5";
        let plain = gl_comments(&discussions, url, false);
        let rows: Vec<_> = plain.iter().map(|c| (c.id, c.reply_to, c.path.clone(), c.line, c.side.clone(), c.can_reply, c.thread.as_ref().map(|t| t.label.clone()))).collect();
        assert_eq!(
            rows,
            [
                (1, None, Some("a.ts".to_owned()), Some(12), "additions".to_owned(), Some(true), Some("Unresolved".to_owned())),
                (2, Some(1), None, None, "deletions".to_owned(), Some(false), None),
                (4, None, Some("b.ts".to_owned()), Some(3), "deletions".to_owned(), Some(true), Some("Resolved".to_owned())),
            ]
        );
        assert_eq!((plain[0].discussion.as_deref(), plain[0].author.as_str(), plain[1].author.as_str(), plain[0].url.as_str()), (Some("d1"), "Ann", "bob", url));
        let with_system = gl_comments(&discussions, url, true);
        let note = with_system.iter().find(|c| c.id == 3).unwrap();
        assert_eq!((with_system.len(), note.system, note.can_reply), (4, true, Some(false)));
    }

    #[test]
    fn a_gitlab_reply_keeps_the_threads_file_and_discussion_and_is_a_reply_to_its_first_comment() {
        let root = Comment {
            id: 1, reply_to: None, path: Some("a.ts".into()), line: Some(12), side: "additions".into(), author: "Ann".into(), body: "why?".into(), created_at: "t1".into(), url: "u".into(),
            can_reply: Some(true), by: None, discussion: Some("d1".into()), thread: None, range: None, snippet: None, refs: None, system: false,
        };
        let reply = gl_replied(&root, &json!({ "id": 9, "author": { "username": "me" }, "body": "because", "created_at": "t9" }), "because");
        assert_eq!((reply.id, reply.reply_to, reply.path.as_deref(), reply.discussion.as_deref(), reply.author.as_str(), reply.can_reply), (9, Some(1), Some("a.ts"), Some("d1"), "me", Some(false)));
        assert_eq!(gl_replied(&root, &json!({ "id": 10, "created_at": "t" }), "typed").body, "typed");
        assert_eq!(gl_replied(&root, &json!({ "id": 10, "created_at": "t" }), "typed").author, "you");
    }

    #[test]
    fn a_status_costs_three_glab_calls_the_mr_its_approvals_and_its_jobs() {
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let seen = calls.clone();
        testing::answer(move |method, params| {
            assert_eq!(method, "exec");
            let args: Vec<String> = params["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_owned()).collect();
            seen.borrow_mut().push(args.last().unwrap().clone());
            let out = match args.last().unwrap().as_str() {
                path if path.ends_with("/approvals") => json!({ "approved": false }),
                path if path.contains("/jobs") => json!([job("success", json!({}))]),
                _ => mr(json!({})),
            };
            Ok(json!({ "code": 0, "stdout": out.to_string(), "stderr": "" }))
        });
        let remote = Remote::Gitlab { host: "gitlab.example.com".into(), path: "g/p".into() };
        let link = to_link(&mr(json!({})), &remote);
        let status = block_on(async move { status(&link, false).await }).unwrap();
        assert_eq!(status.checks, Counts { passed: 1, failed: 0, running: 0 });
        assert_eq!(calls.borrow().len(), 3);
    }

    #[test]
    fn approving_a_head_that_moved_on_says_to_refresh() {
        testing::answer(|_, _| Ok(json!({ "code": 1, "stdout": "", "stderr": "glab: 409 Conflict, sha does not match" })));
        let remote = Remote::Gitlab { host: "gitlab.example.com".into(), path: "g/p".into() };
        let link = to_link(&mr(json!({})), &remote);
        let error = block_on(async move { approve(&link, "abc").await }).unwrap_err();
        assert_eq!(error.to_string(), "New commits since you loaded this PR. Refresh and review again.");
    }
}
