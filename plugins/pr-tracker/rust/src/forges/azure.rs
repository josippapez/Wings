//! Azure DevOps: the PR for the branch (`az repos`), its policies, failed builds, the diff from local git, comments from
//! threads and replies. The `az` shapes are read as JSON, by the field names Azure DevOps gives them.

use super::shared::{Facts, Words as Labels, counts, label_of, no_log, open_or_newest, or_else, same};
use crate::checklist::{between, real};
use crate::forge::{AccountKind, ForgeError, Words};
use crate::logs::{clean_log, detail};
use crate::pushes::pushes_of;
use crate::snippet::{added_lines_of, snippet_of};
use crate::types::*;
use crate::vote::vote_args;
use serde_json::{Value, json};
use std::{cell::RefCell, collections::{BTreeSet, HashMap, HashSet}};
use wings_plugin::{
    azure::{AzureRemote, api_org},
    cache::Cached,
    cli::{Cli, CliError, last_line},
    git, host,
    remote::Remote,
    task::join_all,
};

thread_local! {
    // `az` words a signed-out state in many ways, and every one says `login`.
    static AZ: Cli = Cli::new("az", "Azure CLI", |stderr| stderr.to_lowercase().contains("login"), &["login", "--allow-no-subscriptions", "--output", "none"]);
    /// The user `az` is signed in as, per organization, which marks the PRs you wrote and the ones that wait for you. Kept for good: each `az account show` is a Python start, and it was asked on every list and every look at a PR's comments.
    static WHO: Cached<String, String> = Cached::new(f64::INFINITY, 0.0, |org: String| async move {
        let args = ["account", "show", "--query", "user.name", "-o", "tsv"].map(str::to_owned);
        let name = cli().run(&args, &org, false).await.map_err(|error| ForgeError::from(error).to_wire())?;
        let name = name.trim();
        if name.is_empty() { Err("az account show printed no user".to_owned()) } else { Ok(name.to_owned()) }
    });
    /// organization → your identity id, as a PR in the list shows it.
    static IDENTITIES: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

pub fn cli() -> Cli {
    AZ.with(Cli::clone)
}

pub fn problem_key(remote: &Remote) -> String {
    match remote {
        Remote::Azure(a) => a.org.clone(),
        _ => String::new(),
    }
}

pub fn repo_key(remote: &Remote) -> String {
    match remote {
        Remote::Azure(a) => format!("azure:{}/{}/{}", a.org, a.project, a.repo),
        _ => String::new(),
    }
}

pub fn words(_remote: &Remote) -> Words {
    Words {
        signed_out: "Sign in to Azure DevOps to see this branch's pull request.".into(),
        missing: "PR tracker needs the Azure CLI (az) to find this branch's pull request.".into(),
    }
}

pub async fn sign_in(remote: &Remote, _on_code: impl Fn(&str) + 'static) -> Result<(), String> {
    cli().sign_in(Some(&problem_key(remote))).await
}

/// Forgets who you are, after a sign-in or Try again.
pub fn forget() {
    WHO.with(Cached::clear);
}

// ---------- the CLI's shapes and how they map ----------

/// What `encodeURIComponent` leaves as it is.
fn uri_component(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn text(value: &Value) -> Option<&str> {
    value.as_str()
}

/// A record of a build's timeline: a Stage holds Phases, a Phase holds Jobs, a Job holds Tasks.
struct Record<'a>(&'a Value);

impl<'a> Record<'a> {
    fn id(&self) -> &'a str {
        self.0["id"].as_str().unwrap_or_default()
    }
    fn parent(&self) -> &'a str {
        self.0["parentId"].as_str().unwrap_or_default()
    }
    fn kind(&self) -> &'a str {
        self.0["type"].as_str().unwrap_or_default()
    }
    fn name(&self) -> &'a str {
        self.0["name"].as_str().unwrap_or_default()
    }
    fn order(&self) -> f64 {
        self.0["order"].as_f64().unwrap_or(0.0)
    }
    fn start(&self) -> Option<&'a str> {
        self.0["startTime"].as_str()
    }
    fn finish(&self) -> Option<&'a str> {
        self.0["finishTime"].as_str()
    }
    fn state(&self) -> CheckState {
        let (state, result) = (self.0["state"].as_str(), self.0["result"].as_str());
        if state == Some("pending") {
            CheckState::Queued
        } else if state == Some("inProgress") {
            CheckState::Running
        } else if result == Some("skipped") {
            CheckState::Skipped
        } else if matches!(result, Some("succeeded" | "succeededWithIssues")) {
            CheckState::Passed
        } else {
            CheckState::Failed
        }
    }
}

fn by_order(a: &Record, b: &Record) -> std::cmp::Ordering {
    a.order().partial_cmp(&b.order()).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.start().unwrap_or("").cmp(b.start().unwrap_or("")))
}

/// A build's timeline as its jobs and their tasks, in the order the build page lists them.
pub fn azure_run(records: &[Value], url: &str) -> Run {
    let records: Vec<Record> = records.iter().map(Record).collect();
    let by_id: HashMap<&str, &Record> = records.iter().map(|r| (r.id(), r)).collect();
    let stage_of = |r: &Record| {
        let mut parent = by_id.get(r.parent()).copied();
        while let Some(p) = parent {
            if p.kind() == "Stage" {
                return Some(p);
            }
            parent = by_id.get(p.parent()).copied();
        }
        None
    };
    // Every job of a phase has order 1: the phase's order is the one the build page lists them by.
    let phase_of = |r| by_id.get(Record::parent(r)).copied().unwrap_or(r);
    let mut jobs: Vec<&Record> = records.iter().filter(|r| r.kind() == "Job").collect();
    jobs.sort_by(|a, b| {
        by_order(stage_of(a).unwrap_or(a), stage_of(b).unwrap_or(b)).then_with(|| by_order(phase_of(a), phase_of(b))).then_with(|| by_order(a, b))
    });
    let stages: HashSet<Option<&str>> = jobs.iter().map(|j| stage_of(j).map(|s| s.id())).collect();
    Run {
        url: Some(url.to_owned()),
        note: None,
        jobs: jobs
            .iter()
            .map(|j| {
                let mut tasks: Vec<&Record> = records.iter().filter(|t| t.kind() == "Task" && t.parent() == j.id()).collect();
                tasks.sort_by(|a, b| by_order(a, b));
                RunJob {
                    id: j.id().to_owned(),
                    name: j.name().to_owned(),
                    stage: if stages.len() > 1 { stage_of(j).map(|s| s.name().to_owned()) } else { None },
                    state: j.state(),
                    seconds: between(j.start(), j.finish()),
                    steps: tasks
                        .iter()
                        .map(|t| RunStep {
                            id: t.id().to_owned(),
                            name: t.name().to_owned(),
                            state: t.state(),
                            seconds: between(t.start(), t.finish()),
                            log: t.0["log"]["id"].as_u64().map(|id| id.to_string()),
                        })
                        .collect(),
                }
            })
            .collect(),
    }
}

pub fn azure_link(pr: &Value, remote: &AzureRemote, cwd: &str) -> Link {
    let base = pr["repository"]["webUrl"].as_str().map_or_else(|| format!("{}/{}/_git/{}", remote.org, uri_component(&remote.project), uri_component(&remote.repo)), str::to_owned);
    let number = pr["pullRequestId"].as_u64().unwrap_or(0);
    Link {
        provider: Provider::Azure,
        url: format!("{base}/pullrequest/{number}"),
        number,
        repo: format!("{}/{}", remote.project, remote.repo),
        remote: Some(Remote::Azure(remote.clone())),
        cwd: Some(cwd.to_owned()),
        raw: Some(pr.clone()),
    }
}

fn short(reference: &Value) -> String {
    let reference = reference.as_str().unwrap_or_default();
    reference.strip_prefix("refs/heads/").unwrap_or(reference).to_owned()
}

fn votes(pr: &Value) -> Vec<i64> {
    pr["reviewers"].as_array().map_or_else(Vec::new, |list| list.iter().map(|r| r["vote"].as_i64().unwrap_or(0)).collect())
}

/// Azure status from the PR plus its policy checks. Reviewer votes: 10 approved, 5 approved with suggestions, -5 waiting for author, -10 rejected.
pub fn azure_status(pr: &Value, checks: Counts) -> Status {
    let votes = votes(pr);
    let (label, tone, icon, state) = label_of(
        &Facts {
            merged: pr["status"] == "completed",
            closed: pr["status"] == "abandoned",
            draft: pr["isDraft"] == true,
            conflicts: pr["mergeStatus"] == "conflicts",
            rejected: votes.contains(&-10),
            checks,
            changes: votes.contains(&-5),
            approved: votes.iter().any(|v| *v > 0),
            ..Facts::default()
        },
        &Labels { closed: Some("Abandoned"), changes: Some("Waiting for author"), rest: None },
    );
    let count = |keep: &dyn Fn(i64) -> bool| votes.iter().filter(|v| keep(**v)).count();
    let review = [(count(&|v| v > 0), "approved"), (count(&|v| v == -5), "waiting for author"), (count(&|v| v == -10), "rejected")]
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, what)| format!("{n} {what}"))
        .collect::<Vec<_>>()
        .join(", ");
    let commit = |name: &str| pr[name]["commitId"].as_str().map(str::to_owned);
    Status {
        // A new push changes the source commit, which is what invalidates the cached diff.
        key: commit("lastMergeSourceCommit").or_else(|| pr["creationDate"].as_str().map(str::to_owned)).unwrap_or_default(),
        title: pr["title"].as_str().unwrap_or_default().to_owned(),
        label,
        tone,
        icon,
        state,
        checks,
        review: if review.is_empty() { "No votes yet".into() } else { review },
        failing: Vec::new(),
        items: None,
        changes: None,
        branch: format!("{} → {}", short(&pr["sourceRefName"]), short(&pr["targetRefName"])),
        updated: pr["closedDate"].as_str().or(pr["creationDate"].as_str()).unwrap_or_default().to_owned(),
        merge: None,
        base: Some(short(&pr["targetRefName"])),
        head: Some(short(&pr["sourceRefName"])),
        // A completed PR's branch is often deleted, but its commits can still be fetched by id.
        base_commit: commit("lastMergeTargetCommit"),
        head_commit: commit("lastMergeSourceCommit"),
    }
}

// Azure shows this policy as the reviewer requirement, not as a check, and it stays queued until someone votes.
const MIN_REVIEWERS_POLICY: &str = "fa4e907d-c16b-4a4c-9dfa-4906e5d171dd";
// Azure reports this one rejected until you pick a merge type when completing, so it gets its own row instead of counting as a failed check.
const MERGE_STRATEGY_POLICY: &str = "fa4e907d-c16b-4a4c-9dfa-4916e5d171ab";
const MERGE_TYPES: [(&str, &str); 4] = [("allowSquash", "Squash"), ("allowNoFastForward", "Merge"), ("allowRebase", "Rebase"), ("allowRebaseMerge", "Semi-linear merge")];

fn type_id(policy: &Value) -> Option<&str> {
    policy["configuration"]["type"]["id"].as_str()
}

fn enabled(policy: &Value) -> bool {
    policy["configuration"]["isEnabled"] != false
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Bool(b) => *b,
        Value::Null => false,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

fn merge_strategy(records: &[Value]) -> Option<String> {
    let r = records.iter().find(|p| type_id(p) == Some(MERGE_STRATEGY_POLICY) && enabled(p))?;
    let types = MERGE_TYPES.iter().filter(|(key, _)| truthy(&r["configuration"]["settings"][*key])).map(|(_, name)| *name).collect::<Vec<_>>().join(" or ");
    Some(if r["status"] == "approved" { types } else { format!("{types} only, pick it when completing") })
}

/// Azure's policy types, by the name its policy list gives them. Build and the two below are told apart by what the record holds.
const AZURE_KINDS: [(&[&str], &str, Option<&str>); 7] = [
    (&["comment"], "Comment requirements", Some("unresolved")),
    (&["work item"], "Work item linking", None),
    (&["required reviewers", "automatically included"], "Required reviewers", None),
    (&["minimum", "number of reviewers"], "Minimum reviewers", None),
    (&["merge strategy"], "Merge strategy", None),
    (&["build"], "Build", None),
    (&["status"], "Status check", None),
];

fn azure_state(status: &str) -> CheckState {
    match status {
        "approved" => CheckState::Passed,
        "rejected" | "broken" => CheckState::Failed,
        "running" => CheckState::Running,
        "queued" => CheckState::Queued,
        _ => CheckState::Skipped,
    }
}

/// A pull request's policies as checks. `org` is the organization's URL. A disabled policy isn't one.
pub fn azure_items(policies: &[Value], pr: &Value, org: &str, project: &str) -> Vec<CheckItem> {
    let reviewers: &[Value] = pr["reviewers"].as_array().map_or(&[], Vec::as_slice);
    let voted = |r: &Value| r["vote"].as_i64().unwrap_or(0) > 0;
    let required: Vec<&Value> = reviewers.iter().filter(|r| r["isRequired"] == true).collect();
    let waiting: Vec<&str> = required.iter().filter(|r| !voted(r)).map(|r| r["displayName"].as_str().unwrap_or("unknown")).collect();
    let approvals = reviewers.iter().filter(|r| voted(r)).count();
    policies
        .iter()
        .filter(|p| enabled(p))
        .enumerate()
        .map(|(i, p)| {
            let type_name = p["configuration"]["type"]["displayName"].as_str().unwrap_or("");
            let settings = &p["configuration"]["settings"];
            let lower = type_name.to_lowercase();
            let build = p["context"]["buildId"].as_u64().filter(|id| *id != 0);
            let found = AZURE_KINDS.iter().find(|(words, ..)| words.iter().any(|word| lower.contains(word)));
            let (kind, needs) = match found {
                Some((_, kind, needs)) => ((*kind).to_owned(), needs.map(str::to_owned)),
                None => (if build.is_some() { "Build".to_owned() } else if type_name.is_empty() { "Policy".to_owned() } else { type_name.to_owned() }, None),
            };
            let status = p["status"].as_str().unwrap_or_default();
            let state = azure_state(status);
            let name = text(&settings["displayName"])
                .or(text(&p["context"]["buildDefinitionName"]))
                .or(text(&settings["statusName"]))
                .unwrap_or(if type_name.is_empty() { "Policy" } else { type_name })
                .to_owned();
            let note = if state == CheckState::Skipped {
                Some("Doesn't apply to this pull request".to_owned())
            } else if kind == "Required reviewers" && !required.is_empty() {
                let waiting_note = if waiting.is_empty() { String::new() } else { format!(". Waiting for {}", waiting.join(", ")) };
                Some(format!("{} of {} required reviewers approved{waiting_note}", required.len() - waiting.len(), required.len()))
            } else if kind == "Minimum reviewers" {
                let need = settings["minimumApproverCount"].as_f64().or_else(|| settings["minimumApproverCount"].as_str().and_then(|n| n.parse().ok())).unwrap_or(0.0);
                (need > 0.0).then(|| format!("{} of {need} approvals", (approvals as f64).min(need)))
            } else if kind == "Work item linking" {
                Some(if state == CheckState::Passed { "A work item is linked" } else { "Link a work item to this pull request" }.to_owned())
            } else if kind == "Comment requirements" && state != CheckState::Passed {
                Some("Comment threads are unresolved".to_owned())
            } else if kind == "Merge strategy" && state != CheckState::Passed {
                Some("Pick the merge type when completing".to_owned())
            } else if state == CheckState::Failed && build.is_none() {
                Some(if status == "broken" { "The policy hit an error" } else { "Not met" }.to_owned())
            } else if state == CheckState::Failed {
                Some(if status == "broken" { "The build policy hit an error" } else { "The build failed" }.to_owned())
            } else if state == CheckState::Queued && build.is_some() {
                Some("Waiting for the build to start".to_owned())
            } else {
                None
            };
            CheckItem {
                id: format!("{}:{}:{i}", type_id(p).unwrap_or("policy"), build.map_or_else(|| name.clone(), |b| b.to_string())),
                name: name.clone(),
                state: if kind == "Merge strategy" && state == CheckState::Failed { CheckState::Queued } else { state },
                required: p["configuration"]["isBlocking"].as_bool(),
                kind: Some(kind),
                at: real(p["completedDate"].as_str()).or_else(|| real(p["startedDate"].as_str())),
                seconds: between(p["startedDate"].as_str(), p["completedDate"].as_str()),
                url: build.map(|b| format!("{org}/{}/_build/results?buildId={b}", uri_component(project))),
                note,
                needs,
                failing: build.filter(|_| state == CheckState::Failed).map(|b| Failing { id: b.to_string(), name: Some(name), build: Some(b), ..Failing::default() }),
                run: build.map(|b| RunRef { build: Some(b), ..RunRef::default() }),
            }
        })
        .collect()
}

/// The status of a PR from its policy records.
pub fn azure_status_of(pr: &Value, records: &[Value], remote: &AzureRemote) -> Status {
    let policies: Vec<&Value> = records
        .iter()
        .filter(|r| r["status"] != "notApplicable" && enabled(r))
        .filter(|r| !matches!(type_id(r), Some(MIN_REVIEWERS_POLICY | MERGE_STRATEGY_POLICY)))
        .collect();
    let results: Vec<&str> = policies
        .iter()
        .map(|r| match r["status"].as_str() {
            Some("approved") => "passed",
            Some("rejected" | "broken") => "failed",
            _ => "running",
        })
        .collect();
    // A build policy's context names the build that failed; other policies, like comment requirements, have none.
    let failing = policies
        .iter()
        .zip(&results)
        .filter(|(_, result)| **result == "failed")
        .map(|(r, _)| {
            let name = text(&r["configuration"]["settings"]["displayName"]).or(text(&r["context"]["buildDefinitionName"])).or(text(&r["configuration"]["type"]["displayName"])).unwrap_or("Policy");
            let build = r["context"]["buildId"].as_u64();
            Failing { id: build.map_or_else(|| name.to_owned(), |b| b.to_string()), name: Some(name.to_owned()), build, ..Failing::default() }
        })
        .collect();
    Status { failing, items: Some(azure_items(records, pr, &remote.org, &remote.project)), merge: merge_strategy(records), ..azure_status(pr, counts(&results)) }
}

pub fn azure_reviewers(pr: &Value) -> Vec<Reviewer> {
    pr["reviewers"]
        .as_array()
        .map_or_else(Vec::new, |list| {
            list.iter()
                .map(|r| {
                    let vote = r["vote"].as_i64().unwrap_or(0);
                    Reviewer { name: r["displayName"].as_str().unwrap_or("unknown").to_owned(), vote: if vote > 0 { "approved" } else if vote == -10 { "changes" } else if vote == -5 { "waiting" } else { "none" }.to_owned() }
                })
                .collect()
        })
}

/// Whose a listed PR is: `(mine, reviewing)`. `me` is what `az account show` calls the user, which Azure DevOps knows as `uniqueName`. Any reviewer counts, voted or not, as the sidebar's `--reviewer` query did.
pub fn az_who(pr: &Value, me: &str) -> (bool, bool) {
    let reviewing = pr["reviewers"].as_array().is_some_and(|list| list.iter().any(|r| same(r["uniqueName"].as_str(), me)));
    (same(pr["createdBy"]["uniqueName"].as_str(), me), reviewing)
}

/// Every active PR of the repo, each marked yours or waiting for you.
pub fn azure_listed(prs: &[Value], me: &str, remote: &AzureRemote, cwd: &str) -> Vec<ListedPr> {
    prs.iter()
        .map(|pr| {
            let status = azure_status(pr, Counts::default());
            let (mine, reviewing) = az_who(pr, me);
            ListedPr {
                link: azure_link(pr, remote, cwd),
                title: pr["title"].as_str().unwrap_or_default().to_owned(),
                head: status.head.clone().unwrap_or_default(),
                mine,
                reviewing,
                author: pr["createdBy"]["displayName"].as_str().map(str::to_owned),
                status,
                loaded: false,
                activity: None,
                updated_at: None,
            }
        })
        .collect()
}

fn thread_state(status: &str) -> Option<ThreadState> {
    let (state, label) = match status {
        "active" => ("active", "Active"),
        "pending" => ("active", "Pending"),
        "fixed" => ("resolved", "Resolved"),
        "wontFix" => ("resolved", "Won't fix"),
        "closed" => ("resolved", "Closed"),
        "byDesign" => ("resolved", "By design"),
        _ => return None,
    };
    Some(ThreadState { state: state.into(), label: label.into() })
}

/// The PR's threads as comments. `system` adds Azure's own notes, like policy and vote updates, marked as such. Comment ids are `thread * 10000 + comment`.
pub fn azure_comments(threads: &[Value], url: &str, system: bool) -> Vec<Comment> {
    let mut comments = Vec::new();
    for t in threads {
        if t["isDeleted"] == true {
            continue;
        }
        let ctx = &t["threadContext"];
        // Threads without a file are comments on the whole PR.
        let path = ctx["filePath"].as_str().filter(|p| !p.is_empty()).map(|p| p.strip_prefix('/').unwrap_or(p).to_owned());
        let right = ctx["rightFileStart"]["line"].as_i64();
        let left = ctx["leftFileStart"]["line"].as_i64();
        let on_right = right.is_some_and(|l| l != 0);
        let thread = t["id"].as_i64().unwrap_or(0);
        let texts: Vec<&Value> = t["comments"]
            .as_array()
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .filter(|c| (c["commentType"] == "text" || (system && c["commentType"] == "system")) && c["isDeleted"] != true)
            .collect();
        for (i, c) in texts.iter().enumerate() {
            let first = i == 0;
            let start = right.or(left).filter(|l| *l != 0);
            comments.push(Comment {
                id: thread * 10_000 + c["id"].as_i64().unwrap_or(0),
                // Azure replies can nest; Wings shows them flat under the thread's first comment.
                reply_to: if first { None } else { Some(thread * 10_000 + texts[0]["id"].as_i64().unwrap_or(0)) },
                path: path.clone(),
                line: right.or(left),
                side: if on_right { "additions" } else { "deletions" }.into(),
                author: c["author"]["displayName"].as_str().unwrap_or("unknown").to_owned(),
                by: c["author"]["uniqueName"].as_str().map(str::to_owned),
                body: c["content"].as_str().unwrap_or_default().to_owned(),
                created_at: c["publishedDate"].as_str().unwrap_or_default().to_owned(),
                url: url.to_owned(),
                can_reply: Some(first && c["commentType"] == "text"),
                discussion: None,
                range: start.filter(|_| first).map(|start| LineRange {
                    start,
                    end: (if on_right { ctx["rightFileEnd"]["line"].as_i64() } else { ctx["leftFileEnd"]["line"].as_i64() }).unwrap_or(start),
                    iteration: t["pullRequestThreadContext"]["iterationContext"]["secondComparingIteration"].as_i64(),
                }),
                thread: if first { t["status"].as_str().and_then(thread_state) } else { None },
                snippet: None,
                refs: None,
                system: c["commentType"] == "system",
            });
        }
    }
    comments
}

/// The comment a reply made, in the thread of `root`.
pub fn azure_replied(root: &Comment, c: &Value, body: &str) -> Comment {
    Comment {
        id: root.id.div_euclid(10_000) * 10_000 + c["id"].as_i64().unwrap_or(0),
        reply_to: Some(root.id),
        author: c["author"]["displayName"].as_str().unwrap_or("you").to_owned(),
        body: c["content"].as_str().unwrap_or(body).to_owned(),
        created_at: c["publishedDate"].as_str().unwrap_or_default().to_owned(),
        can_reply: Some(false),
        ..root.clone()
    }
}

// ---------- calling it ----------

fn azure(link: &Link) -> Result<&AzureRemote, ForgeError> {
    match &link.remote {
        Some(Remote::Azure(remote)) => Ok(remote),
        _ => Err("This pull request doesn't say which Azure DevOps repository it is in.".into()),
    }
}

fn cwd_of(link: &Link) -> &str {
    link.cwd.as_deref().unwrap_or_default()
}

fn owned(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

/// `az` printing JSON, which is never nothing here.
async fn az_json(org: &str, args: Vec<String>) -> Result<Value, ForgeError> {
    Ok(cli().json(&args, org, false, None).await?)
}

/// A PR by number, for a page opened with only its identifiers.
async fn show(remote: &AzureRemote, number: u64) -> Result<Value, ForgeError> {
    let number = number.to_string();
    az_json(&remote.org, owned(&["repos", "pr", "show", "--id", &number, "--organization", &remote.org, "-o", "json"])).await
}

/// A resource of the Azure DevOps REST API, through `az devops invoke`.
async fn invoke(org: &str, area: &str, resource: &str, route: &[String], extra: &[&str]) -> Result<Value, ForgeError> {
    let mut args = owned(&["devops", "invoke", "--area", area, "--resource", resource, "--route-parameters"]);
    args.extend(route.iter().cloned());
    args.extend(owned(extra));
    args.extend(owned(&["--organization", org, "--api-version", "7.1", "-o", "json"]));
    az_json(org, args).await
}

fn pr_route(link: &Link, remote: &AzureRemote) -> Vec<String> {
    vec![format!("project={}", remote.project), format!("repositoryId={}", remote.repo), format!("pullRequestId={}", link.number)]
}

pub async fn status(link: &Link, _fresh: bool) -> Result<Status, ForgeError> {
    let remote = azure(link)?;
    let number = link.number.to_string();
    let args = owned(&["repos", "pr", "policy", "list", "--id", &number, "--organization", &remote.org, "-o", "json"]);
    let parsed = or_else(az_json(&remote.org, args).await, Value::Null)?;
    let records = parsed.as_array().or_else(|| parsed["value"].as_array()).cloned().unwrap_or_default();
    Ok(azure_status_of(link.raw.as_ref().unwrap_or(&Value::Null), &records, remote))
}

pub async fn comments(link: &Link) -> Result<Vec<Comment>, ForgeError> {
    comments_with(link, false).await
}

async fn comments_with(link: &Link, system: bool) -> Result<Vec<Comment>, ForgeError> {
    let remote = azure(link)?;
    let threads = or_else(invoke(&remote.org, "git", "pullRequestThreads", &pr_route(link, remote), &[]).await, Value::Null)?;
    Ok(azure_comments(threads["value"].as_array().map_or(&[], Vec::as_slice), &link.url, system))
}

fn is_sha(text: &str) -> bool {
    text.len() == 40 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The lines of a file the PR added or changed at a push: a local `git diff -U0` from the merge base, as Azure DevOps compares. None when git can't tell, so no line is tinted.
async fn added_at(link: &Link, base: &str, sha: &str, path: &str) -> Option<BTreeSet<i64>> {
    if !is_sha(base) {
        return None;
    }
    let patch = git::raw(cwd_of(link), &["diff", "--no-color", "--no-ext-diff", "-U0", base, sha, "--", path]).await.ok()?;
    Some(added_lines_of(&patch))
}

/// A file as it was at a commit, from the project's own clone; a commit it doesn't have is fetched once. None when neither works.
async fn file_at(link: &Link, sha: &str, path: &str) -> Option<String> {
    if !is_sha(sha) {
        return None;
    }
    let spec = format!("{sha}:{path}");
    let cwd = cwd_of(link);
    if let Ok(text) = git::raw(cwd, &["show", &spec]).await {
        return Some(text);
    }
    git::git(cwd, &["fetch", "--quiet", "--end-of-options", "origin", sha]).await.ok()?;
    git::raw(cwd, &["show", &spec]).await.ok()
}

/// The lines around `start`-`end` of a file on one side of a push: the merge base for deletions, else the push's head.
async fn lines_at(link: &Link, path: &str, side: &str, base: Option<&str>, head: Option<&str>, start: i64, end: i64) -> Option<Vec<SnippetLine>> {
    let sha = if side == "deletions" { base } else { head };
    let text = match sha {
        Some(sha) => file_at(link, sha, path).await,
        None => None,
    };
    let added = match (&text, sha, base) {
        (Some(_), Some(sha), Some(base)) if side != "deletions" => added_at(link, base, sha, path).await,
        _ => None,
    };
    let lines = text.map_or_else(Vec::new, |text| snippet_of(&text, start, end, added.as_ref()));
    (!lines.is_empty()).then_some(lines)
}

async fn who(org: &str) -> Result<String, ForgeError> {
    Ok(WHO.with(Cached::clone).get(org.to_owned()).await?)
}

pub async fn find(cwd: &str, remote: &Remote, branch: &str) -> Result<Option<Link>, ForgeError> {
    let Remote::Azure(remote) = remote else { return Ok(None) };
    let args = owned(&["repos", "pr", "list", "--organization", &remote.org, "--project", &remote.project, "--repository", &remote.repo, "--source-branch", branch, "--status", "all", "--top", "5", "-o", "json"]);
    let prs = match az_json(&remote.org, args).await {
        Ok(prs) => prs,
        // az ran and failed for a reason that isn't being signed out: the pane says pull requests are unavailable.
        Err(ForgeError::Cli(CliError { kind: wings_plugin::cli::Kind::Failed, message })) if message != "az failed" => {
            return Err(ForgeError::account(AccountKind::Unavailable, last_line(&message)));
        }
        Err(error) => return Err(error),
    };
    let prs = prs.as_array().cloned().unwrap_or_default();
    Ok(open_or_newest(&prs, |p| p["status"] == "active").map(|pr| azure_link(pr, remote, cwd)))
}

/// `az account show` says who you are: the same name Azure DevOps knows you by as `uniqueName`. A mention is written `@<id>` with your identity id, which a PR you wrote or review carries, so it is read from the list.
pub async fn me(remote: &Remote) -> Result<Me, ForgeError> {
    let org = problem_key(remote);
    let name = who(&org).await?;
    let id = IDENTITIES.with(|ids| ids.borrow().get(&org).cloned());
    Ok(Me { ids: vec![name.clone()], handles: match id { Some(id) => vec![name, id], None => vec![name] } })
}

/// Every active PR of the repo (up to 100) in one `az repos pr list`, each marked yours or waiting for you.
pub async fn list(cwd: &str, remote: &Remote) -> Result<Vec<ListedPr>, ForgeError> {
    let Remote::Azure(remote) = remote else { return Ok(Vec::new()) };
    let me = who(&remote.org).await?;
    let args = owned(&["repos", "pr", "list", "--organization", &remote.org, "--project", &remote.project, "--repository", &remote.repo, "--status", "active", "--top", "100", "-o", "json"]);
    let prs = az_json(&remote.org, args).await?;
    let prs = prs.as_array().cloned().unwrap_or_default();
    let mine = prs.iter().flat_map(|p| std::iter::once(&p["createdBy"]).chain(p["reviewers"].as_array().into_iter().flatten())).find(|u| same(u["uniqueName"].as_str(), &me) && u["id"].as_str().is_some_and(|id| !id.is_empty()));
    if let Some(id) = mine.and_then(|u| u["id"].as_str()) {
        IDENTITIES.with(|ids| ids.borrow_mut().insert(remote.org.clone(), id.to_owned()));
    }
    Ok(azure_listed(&prs, &me, remote, cwd))
}

/// Azure DevOps looks the PR up again, because its status needs the PR's own fields.
pub async fn link_of(args: &PrArgs) -> Result<Link, ForgeError> {
    let Some(Remote::Azure(remote)) = &args.remote else {
        return Err("This tab doesn't say which Azure DevOps repository its pull request is in.".into());
    };
    Ok(Link { url: args.url.clone(), ..azure_link(&show(remote, args.number).await?, remote, &args.cwd) })
}

pub async fn overview(link: &Link) -> Result<Overview, ForgeError> {
    let remote = azure(link)?;
    let raw = show(remote, link.number).await?;
    let status = status(&Link { raw: Some(raw.clone()), ..link.clone() }, false).await?;
    Ok(Overview {
        status,
        author: raw["createdBy"]["displayName"].as_str().unwrap_or("unknown").to_owned(),
        body: raw["description"].as_str().unwrap_or_default().to_owned(),
        created_at: raw["creationDate"].as_str().map(str::to_owned),
        reviewers: Some(azure_reviewers(&raw)),
    })
}

pub async fn diff(link: &Link, st: &Status) -> Result<(String, Vec<Comment>), ForgeError> {
    // Fetch only the PR's two commits, then diff from their merge base, like the PR page does.
    let cwd = cwd_of(link);
    let (from, to, range) = match (&st.base_commit, &st.head_commit) {
        (Some(base), Some(head)) => (base.clone(), head.clone(), format!("{base}...{head}")),
        _ => {
            let (base, head) = (st.base.clone().unwrap_or_default(), st.head.clone().unwrap_or_default());
            (base.clone(), head.clone(), format!("origin/{base}...origin/{head}"))
        }
    };
    git::git(cwd, &["fetch", "--quiet", "--end-of-options", "origin", &from, &to]).await?;
    let patch = git::raw(cwd, &["diff", "--no-color", "--no-ext-diff", &range]).await?;
    Ok((patch, comments(link).await?))
}

/// A failed Azure DevOps policy: for a build, its failed tasks from the build timeline and each task's log.
pub async fn failure(link: &Link, f: &Failing) -> Result<Vec<FailureDetail>, ForgeError> {
    let Some(build_id) = f.build.filter(|b| *b != 0) else {
        return Ok(no_log(f.name.as_deref(), "Not met", Some(&link.url)));
    };
    let remote = azure(link)?;
    let (org, name) = (remote.org.clone(), remote.project.clone());
    let route = |extra: Option<String>| [Some(format!("project={name}")), Some(format!("buildId={build_id}")), extra].into_iter().flatten().collect::<Vec<_>>();
    let url = format!("{org}/{}/_build/results?buildId={build_id}&view=logs", uri_component(&name));
    let timeline = invoke(&org, "build", "timeline", &route(None), &[]).await?;
    let tasks: Vec<Value> = timeline["records"].as_array().map_or_else(Vec::new, |records| records.iter().filter(|r| r["type"] == "Task" && r["result"] == "failed").cloned().collect());
    if tasks.is_empty() {
        return Ok(no_log(f.name.as_deref(), "Failed", Some(&url)));
    }
    let reads: Vec<std::pin::Pin<Box<dyn std::future::Future<Output = Result<FailureDetail, ForgeError>>>>> = tasks
        .into_iter()
        .map(|r| {
            let (org, check, url, route) = (org.clone(), f.name.clone(), url.clone(), route(r["log"]["id"].as_u64().map(|id| format!("logId={id}"))));
            Box::pin(async move {
                let issues: Vec<String> = r["issues"].as_array().map_or_else(Vec::new, |list| list.iter().filter(|i| i["type"] == "error").map(|i| i["message"].as_str().unwrap_or_default().to_owned()).collect());
                let mut lines = if r["log"].is_object() {
                    let logs = invoke(&org, "build", "logs", &route, &[]).await?;
                    clean_log(&logs["value"].as_array().map_or_else(Vec::new, |lines| lines.iter().map(|l| l.as_str().unwrap_or_default().to_owned()).collect::<Vec<_>>()))
                } else {
                    Vec::new()
                };
                // A script task's log starts with the task's description and the script; its output follows this line.
                if let Some(start) = lines.iter().position(|l| l.contains("Starting Command Output")) {
                    lines.drain(..=start);
                }
                Ok(detail(check.as_deref(), r["name"].as_str(), &lines, &issues, Some(&url)))
            }) as std::pin::Pin<Box<dyn std::future::Future<Output = Result<FailureDetail, ForgeError>>>>
        })
        .collect();
    join_all(reads).await.into_iter().collect()
}

pub async fn run(link: &Link, reference: &RunRef) -> Result<Run, ForgeError> {
    let remote = azure(link)?;
    let build = reference.build.unwrap_or(0);
    let route = [format!("project={}", remote.project), format!("buildId={build}")];
    let timeline = invoke(&remote.org, "build", "timeline", &route, &[]).await?;
    Ok(azure_run(timeline["records"].as_array().map_or(&[], Vec::as_slice), &format!("{}/{}/_build/results?buildId={build}&view=logs", remote.org, uri_component(&remote.project))))
}

pub async fn run_log(link: &Link, reference: &RunRef, step: &RunStep) -> Result<Vec<String>, ForgeError> {
    let remote = azure(link)?;
    let route = [format!("project={}", remote.project), format!("buildId={}", reference.build.unwrap_or(0)), format!("logId={}", step.log.as_deref().unwrap_or_default())];
    let out = invoke(&remote.org, "build", "logs", &route, &[]).await?;
    Ok(out["value"].as_array().map_or_else(Vec::new, |lines| lines.iter().map(|l| l.as_str().unwrap_or_default().to_owned()).collect()))
}

/// Azure comment ids are `thread * 10000 + comment`, as `azure_comments` builds them.
pub async fn reply(link: &Link, root: &Comment, body: &str) -> Result<Comment, ForgeError> {
    let remote = azure(link)?;
    let thread = root.id.div_euclid(10_000);
    let url = format!(
        "{}/{}/_apis/git/repositories/{}/pullRequests/{}/threads/{thread}/comments?api-version=7.1",
        api_org(&remote.org),
        uri_component(&remote.project),
        uri_component(&remote.repo),
        link.number
    );
    let fields = json!({ "content": body, "parentCommentId": root.id.rem_euclid(10_000), "commentType": 1 });
    let created = post(&url, &fields).await?;
    Ok(azure_replied(root, &created, body))
}

/// What `az` printed for a POST `permissions.post` names, or its error.
async fn post(url: &str, fields: &Value) -> Result<Value, ForgeError> {
    let out = host::call("post", json!({ "program": "az", "url": url, "host": null, "fields": fields })).await?;
    if out["code"] != 0 {
        let message = last_line(out["stderr"].as_str().unwrap_or_default());
        return Err(if message.is_empty() { "az failed".into() } else { message.into() });
    }
    serde_json::from_str(out["stdout"].as_str().unwrap_or_default()).map_err(|error| ForgeError::Other(error.to_string()))
}

/// Casts your vote through `az repos pr set-vote`, which works out your reviewer identity itself.
pub async fn vote(link: &Link, vote: &str) -> Result<(), ForgeError> {
    let remote = azure(link)?;
    cli().run(&vote_args(link.number, &remote.org, vote)?, &remote.org, false).await?;
    Ok(())
}

/// The PR's pushes with their commits: every iteration and its commits in one call, not one call per push.
pub async fn pushes(link: &Link) -> Result<Vec<Push>, ForgeError> {
    let remote = azure(link)?;
    let out = or_else(invoke(&remote.org, "git", "pullRequestIterations", &pr_route(link, remote), &["--query-parameters", "includeCommits=true"]).await, Value::Null)?;
    let iterations: Vec<crate::pushes::AzureIteration> = serde_json::from_value(out["value"].clone()).unwrap_or_default();
    Ok(pushes_of(&iterations))
}

pub async fn snippet(link: &Link, root: &Comment, pushes: &[Push], head: Option<&str>, now: bool) -> Result<Option<Vec<SnippetLine>>, ForgeError> {
    let (Some(path), Some(range)) = (&root.path, &root.range) else { return Ok(None) };
    if !now {
        let push = pushes.iter().find(|p| Some(p.id) == range.iteration);
        return Ok(lines_at(link, path, &root.side, push.and_then(|p| p.base.as_deref()), push.and_then(|p| p.head.as_deref()).or(head), range.start, range.end).await);
    }
    // Azure moves the thread's lines to where they sit at the latest push, as its own page shows them.
    let Some(latest) = pushes.iter().fold(None::<&Push>, |a, p| Some(if a.is_some_and(|a| a.id > p.id) { a.unwrap() } else { p })) else { return Ok(None) };
    let remote = azure(link)?;
    let mut route = pr_route(link, remote);
    route.push(format!("threadId={}", root.id.div_euclid(10_000)));
    let query = format!("$iteration={}", latest.id);
    let t = or_else(invoke(&remote.org, "git", "pullRequestThreads", &route, &["--query-parameters", &query]).await, Value::Null)?;
    let ctx = &t["threadContext"];
    let right = ctx["rightFileStart"]["line"].as_i64();
    let start = right.or(ctx["leftFileStart"]["line"].as_i64());
    let (Some(file), Some(start)) = (ctx["filePath"].as_str().filter(|f| !f.is_empty()), start.filter(|s| *s != 0)) else { return Ok(None) };
    let end = (if right.is_some_and(|r| r != 0) { ctx["rightFileEnd"]["line"].as_i64() } else { ctx["leftFileEnd"]["line"].as_i64() }).unwrap_or(start);
    let side = if right.is_some_and(|r| r != 0) { "additions" } else { "deletions" };
    Ok(lines_at(link, file.strip_prefix('/').unwrap_or(file), side, latest.base.as_deref(), head.or(latest.head.as_deref()), start, end).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORG: &str = "https://dev.azure.com/acme";

    fn remote() -> AzureRemote {
        AzureRemote { org: ORG.into(), project: "My Project".into(), repo: "app".into() }
    }

    fn pr(over: Value) -> Value {
        let mut base = json!({
            "pullRequestId": 42, "status": "active", "title": "Add z", "reviewers": [], "creationDate": "2026-10-01T09:00:00Z",
            "lastMergeSourceCommit": { "commitId": "a".repeat(40) }, "lastMergeTargetCommit": { "commitId": "b".repeat(40) },
            "sourceRefName": "refs/heads/feat", "targetRefName": "refs/heads/main", "createdBy": { "displayName": "Ada", "uniqueName": "ada@example.com" },
        });
        for (key, value) in over.as_object().unwrap() {
            base[key] = value.clone();
        }
        base
    }

    fn none() -> Counts {
        Counts::default()
    }

    fn policy(kind: &str, status: &str, extra: Value) -> Value {
        let mut configuration = json!({ "isEnabled": true, "isBlocking": true, "type": { "id": "t", "displayName": kind }, "settings": {} });
        for (key, value) in extra["configuration"].as_object().into_iter().flatten() {
            configuration[key] = value.clone();
        }
        let mut record = json!({ "status": status, "configuration": configuration });
        for (key, value) in extra["rest"].as_object().into_iter().flatten() {
            record[key] = value.clone();
        }
        record
    }

    fn label(over: Value, checks: Counts) -> String {
        azure_status(&pr(over), checks).label
    }

    #[test]
    fn azure_status_words_its_own_states_abandoned_rejected_and_waiting_for_the_author() {
        let (failed, running) = (Counts { failed: 1, ..none() }, Counts { running: 1, ..none() });
        assert_eq!(label(json!({ "status": "completed", "isDraft": true }), none()), "Merged");
        assert_eq!(label(json!({ "status": "abandoned" }), none()), "Abandoned");
        assert_eq!(label(json!({ "isDraft": true, "mergeStatus": "conflicts" }), none()), "Draft");
        assert_eq!(label(json!({ "mergeStatus": "conflicts", "reviewers": [{ "vote": -10 }] }), none()), "Conflicts");
        assert_eq!(label(json!({ "reviewers": [{ "vote": -10 }, { "vote": 10 }] }), failed), "Rejected");
        assert_eq!(label(json!({ "reviewers": [{ "vote": -5 }] }), failed), "Checks failing");
        assert_eq!(label(json!({ "reviewers": [{ "vote": -5 }, { "vote": 10 }] }), running), "Waiting for author");
        assert_eq!(label(json!({ "reviewers": [{ "vote": 5 }] }), running), "Checks running");
        assert_eq!(label(json!({ "reviewers": [{ "vote": 5 }] }), none()), "Approved");
        assert_eq!(label(json!({ "reviewers": [{ "vote": 0 }] }), none()), "In review");
        let abandoned = azure_status(&pr(json!({ "status": "abandoned" })), none());
        assert_eq!((abandoned.tone.as_str(), abandoned.icon.as_str()), ("neutral", "pr-closed"));
    }

    #[test]
    fn an_abandoned_azure_devops_pr_is_closed_though_its_label_says_abandoned_and_a_completed_one_merged() {
        assert_eq!(azure_status(&pr(json!({ "status": "abandoned" })), none()).state, PrState::Closed);
        assert_eq!(azure_status(&pr(json!({ "status": "completed" })), none()).state, PrState::Merged);
        assert_eq!(azure_status(&pr(json!({ "status": "active", "isDraft": true })), none()).state, PrState::Open);
    }

    #[test]
    fn azure_status_the_review_text_counts_votes_and_the_key_branches_and_commits_come_from_the_pr() {
        let st = azure_status(&pr(json!({ "reviewers": [{ "vote": 10 }, { "vote": 5 }, { "vote": -5 }, { "vote": -10 }, { "vote": 0 }] })), none());
        assert_eq!(st.review, "2 approved, 1 waiting for author, 1 rejected");
        assert_eq!(azure_status(&pr(json!({})), none()).review, "No votes yet");
        assert_eq!(
            (st.key.as_str(), st.branch.as_str(), st.base.as_deref(), st.head.as_deref(), st.base_commit, st.head_commit, st.updated.as_str()),
            (&*"a".repeat(40), "feat → main", Some("main"), Some("feat"), Some("b".repeat(40)), Some("a".repeat(40)), "2026-10-01T09:00:00Z")
        );
        assert_eq!(azure_status(&pr(json!({ "lastMergeSourceCommit": null, "status": "completed", "closedDate": "2026-10-02T00:00:00Z" })), none()).key, "2026-10-01T09:00:00Z");
        assert_eq!(azure_status(&pr(json!({ "closedDate": "2026-10-02T00:00:00Z" })), none()).updated, "2026-10-02T00:00:00Z");
    }

    #[test]
    fn azure_status_from_policies_not_applicable_and_disabled_ones_dont_count_a_build_names_its_failure_the_merge_strategy_has_its_own_row() {
        let records = vec![
            policy("Build", "rejected", json!({ "configuration": { "settings": { "displayName": "CI" } }, "rest": { "context": { "buildId": 42 } } })),
            policy("Build", "approved", json!({ "rest": { "context": { "buildId": 43, "buildDefinitionName": "Lint" } } })),
            policy("Status", "notApplicable", json!({})),
            policy("Build", "running", json!({ "configuration": { "isEnabled": false } })),
            policy("Comment requirements", "broken", json!({})),
            policy("Minimum number of reviewers", "queued", json!({ "configuration": { "type": { "id": "fa4e907d-c16b-4a4c-9dfa-4906e5d171dd", "displayName": "Minimum number of reviewers" } } })),
            policy("Require a merge strategy", "rejected", json!({ "configuration": { "type": { "id": "fa4e907d-c16b-4a4c-9dfa-4916e5d171ab", "displayName": "Require a merge strategy" }, "settings": { "allowSquash": true, "allowRebase": true } } })),
        ];
        let st = azure_status_of(&pr(json!({})), &records, &remote());
        assert_eq!(st.checks, Counts { passed: 1, failed: 2, running: 0 });
        assert_eq!(
            st.failing,
            [
                Failing { id: "42".into(), name: Some("CI".into()), build: Some(42), ..Failing::default() },
                Failing { id: "Comment requirements".into(), name: Some("Comment requirements".into()), build: None, ..Failing::default() }
            ]
        );
        assert_eq!(st.merge.as_deref(), Some("Squash or Rebase only, pick it when completing"));
        assert_eq!(st.label, "Checks failing");
        assert_eq!(st.items.unwrap().len(), 6, "the checks view lists every enabled policy, the merge strategy too");
        assert_eq!(azure_status_of(&pr(json!({})), &[], &remote()).merge, None);
    }

    #[test]
    fn azure_policies_required_reviewers_say_who_is_missing_a_build_links_to_its_run() {
        let reviewers = pr(json!({ "reviewers": [{ "vote": 10, "displayName": "Ada", "isRequired": true }, { "vote": 0, "displayName": "Grace", "isRequired": true }, { "vote": 5, "displayName": "Alan" }] }));
        let items = azure_items(
            &[
                policy("Required reviewers", "queued", json!({})),
                policy("Build", "rejected", json!({ "configuration": { "settings": { "displayName": "CI" } }, "rest": { "context": { "buildId": 42 }, "startedDate": "2026-01-01T00:00:00Z", "completedDate": "2026-01-01T00:03:00Z" } })),
                policy("Work item linking", "rejected", json!({})),
                policy("Comment requirements", "rejected", json!({})),
                policy("Minimum number of reviewers", "queued", json!({ "configuration": { "settings": { "minimumApproverCount": 2 } } })),
                policy("Status", "notApplicable", json!({ "configuration": { "isBlocking": false } })),
                policy("Build", "running", json!({ "configuration": { "isEnabled": false } })),
            ],
            &reviewers,
            ORG,
            "My Project",
        );
        let by = |kind: &str| items.iter().find(|c| c.kind.as_deref() == Some(kind)).unwrap();
        assert_eq!(items.len(), 6, "a disabled policy is not a check");
        assert_eq!(by("Required reviewers").note.as_deref(), Some("1 of 2 required reviewers approved. Waiting for Grace"));
        assert_eq!(by("Minimum reviewers").note.as_deref(), Some("2 of 2 approvals"));
        let build = by("Build");
        assert_eq!(
            (build.name.as_str(), build.state, build.required, build.seconds, build.url.clone(), build.failing.clone()),
            ("CI", CheckState::Failed, Some(true), Some(180.0), Some(format!("{ORG}/My%20Project/_build/results?buildId=42")), Some(Failing { id: "42".into(), name: Some("CI".into()), build: Some(42), ..Failing::default() }))
        );
        assert_eq!(by("Work item linking").state, CheckState::Failed);
        assert_eq!(by("Work item linking").failing, None);
        assert_eq!(by("Comment requirements").needs.as_deref(), Some("unresolved"));
        let na = by("Status check");
        assert_eq!((na.state, na.required), (CheckState::Skipped, Some(false)));
    }

    #[test]
    fn an_unknown_azure_policy_keeps_the_name_azure_gives_it() {
        let items = azure_items(&[policy("File size restriction", "approved", json!({}))], &json!({}), ORG, "p");
        assert_eq!((items[0].name.as_str(), items[0].kind.as_deref(), items[0].state), ("File size restriction", Some("File size restriction"), CheckState::Passed));
    }

    #[test]
    fn the_merge_strategy_policy_waits_instead_of_failing() {
        let items = azure_items(&[policy("Require a merge strategy", "rejected", json!({}))], &json!({}), ORG, "p");
        assert_eq!((items[0].state, items[0].note.as_deref()), (CheckState::Queued, Some("Pick the merge type when completing")));
    }

    #[test]
    fn azure_reviewers_vote_approved_waiting_rejected_or_not_at_all() {
        let list = azure_reviewers(&pr(json!({ "reviewers": [{ "vote": 10, "displayName": "A" }, { "vote": 5, "displayName": "B" }, { "vote": -5 }, { "vote": -10, "displayName": "D" }, { "vote": 0, "displayName": "E" }] })));
        let pairs: Vec<(&str, &str)> = list.iter().map(|r| (r.name.as_str(), r.vote.as_str())).collect();
        assert_eq!(pairs, [("A", "approved"), ("B", "approved"), ("unknown", "waiting"), ("D", "changes"), ("E", "none")]);
    }

    #[test]
    fn azure_list_marks_the_prs_you_created_or_were_asked_to_review_by_unique_name_without_case_and_says_the_list_has_no_check_counts() {
        let prs = [
            pr(json!({ "pullRequestId": 1, "createdBy": { "displayName": "Me", "uniqueName": "ME@example.com" } })),
            pr(json!({ "pullRequestId": 2, "reviewers": [{ "vote": 0, "uniqueName": "me@EXAMPLE.com" }] })),
            pr(json!({ "pullRequestId": 3 })),
        ];
        let rows = azure_listed(&prs, "me@example.com", &remote(), "/work/app");
        assert_eq!(rows.iter().map(|r| (r.link.number, r.mine, r.reviewing)).collect::<Vec<_>>(), [(1, true, false), (2, false, true), (3, false, false)]);
        assert_eq!((rows[0].head.as_str(), rows[0].author.as_deref(), rows[0].loaded, rows[0].activity, rows[0].status.failing.len()), ("feat", Some("Me"), false, None, 0));
        assert_eq!(az_who(&json!({ "createdBy": { "uniqueName": "ann@example.com" }, "reviewers": [{}] }), "me@example.com"), (false, false));
    }

    #[test]
    fn an_azure_link_has_the_prs_web_address_else_one_built_from_the_organization() {
        let link = azure_link(&pr(json!({ "repository": { "webUrl": "https://acme.visualstudio.com/My%20Project/_git/app" } })), &remote(), "/work/app");
        assert_eq!((link.url.as_str(), link.number, link.repo.as_str(), link.cwd.as_deref(), link.provider), ("https://acme.visualstudio.com/My%20Project/_git/app/pullrequest/42", 42, "My Project/app", Some("/work/app"), Provider::Azure));
        assert_eq!(azure_link(&pr(json!({})), &remote(), "/w").url, format!("{ORG}/My%20Project/_git/app/pullrequest/42"));
    }

    #[test]
    fn azure_threads_ids_carry_the_thread_replies_sit_under_the_first_comment_system_notes_only_when_asked_for() {
        let threads = vec![
            json!({ "id": 3, "status": "active", "threadContext": { "filePath": "/src/a.ts", "rightFileStart": { "line": 8 }, "rightFileEnd": { "line": 10 } }, "pullRequestThreadContext": { "iterationContext": { "secondComparingIteration": 2 } }, "comments": [
                { "id": 1, "commentType": "text", "author": { "displayName": "Ann" }, "content": "why?", "publishedDate": "t1" },
                { "id": 2, "commentType": "text", "author": { "displayName": "Bob" }, "content": "because", "publishedDate": "t2" },
                { "id": 3, "commentType": "text", "isDeleted": true, "content": "oops", "publishedDate": "t3" },
            ] }),
            json!({ "id": 4, "isDeleted": true, "comments": [{ "id": 1, "commentType": "text", "content": "gone", "publishedDate": "t4" }] }),
            json!({ "id": 5, "status": "fixed", "threadContext": { "filePath": "/b.ts", "leftFileStart": { "line": 2 } }, "comments": [{ "id": 1, "commentType": "text", "author": {}, "content": "old", "publishedDate": "t5" }] }),
            json!({ "id": 6, "comments": [{ "id": 1, "commentType": "system", "author": { "displayName": "Azure" }, "content": "Policy updated", "publishedDate": "t6" }, { "id": 2, "commentType": "text", "content": "whole PR", "publishedDate": "t7" }] }),
        ];
        let url = format!("{ORG}/My%20Project/_git/app/pullrequest/42");
        let plain = azure_comments(&threads, &url, false);
        let rows: Vec<_> = plain.iter().map(|c| (c.id, c.reply_to, c.path.as_deref(), c.line, c.side.as_str(), c.can_reply, c.thread.as_ref().map(|t| t.label.as_str()))).collect();
        assert_eq!(
            rows,
            [
                (30001, None, Some("src/a.ts"), Some(8), "additions", Some(true), Some("Active")),
                (30002, Some(30001), Some("src/a.ts"), Some(8), "additions", Some(false), None),
                (50001, None, Some("b.ts"), Some(2), "deletions", Some(true), Some("Resolved")),
                (60002, None, None, None, "deletions", Some(true), None),
            ]
        );
        assert_eq!(plain[0].range, Some(LineRange { start: 8, end: 10, iteration: Some(2) }));
        assert_eq!((plain[2].author.as_str(), plain[2].range.clone()), ("unknown", Some(LineRange { start: 2, end: 2, iteration: None })));
        let all = azure_comments(&threads, &url, true);
        let note = all.iter().find(|c| c.id == 60001).unwrap();
        assert_eq!((all.len(), note.system, note.can_reply, note.reply_to, all.iter().find(|c| c.id == 60002).unwrap().reply_to), (5, true, Some(false), None, Some(60001)));
    }

    #[test]
    fn an_azure_reply_keeps_the_threads_file_and_is_a_reply_to_its_first_comment_with_the_thread_in_its_id() {
        let root = Comment {
            id: 30001, reply_to: None, path: Some("src/a.ts".into()), line: Some(8), side: "additions".into(), author: "Ann".into(), body: "why?".into(), created_at: "t1".into(), url: "u".into(),
            can_reply: Some(true), by: None, discussion: None, thread: Some(ThreadState { state: "active".into(), label: "Active".into() }), range: None, snippet: None, refs: None, system: false,
        };
        let reply = azure_replied(&root, &json!({ "id": 7, "author": { "displayName": "Me" }, "content": "because", "publishedDate": "t9" }), "because");
        assert_eq!((reply.id, reply.reply_to, reply.path.as_deref(), reply.author.as_str(), reply.can_reply, reply.thread.unwrap().label.as_str()), (30007, Some(30001), Some("src/a.ts"), "Me", Some(false), "Active"));
        assert_eq!(azure_replied(&root, &json!({ "id": 8, "publishedDate": "t" }), "typed").body, "typed");
        assert_eq!(azure_replied(&root, &json!({ "id": 8, "publishedDate": "t" }), "typed").author, "you");
    }

    #[test]
    fn a_build_timeline_is_its_jobs_and_tasks_in_the_builds_order() {
        let records = vec![
            json!({ "id": "s1", "type": "Stage", "name": "Build", "order": 1 }),
            json!({ "id": "s2", "type": "Stage", "name": "Deploy", "order": 2 }),
            json!({ "id": "j2", "parentId": "s2", "type": "Job", "name": "Ship", "order": 1, "result": "succeeded" }),
            json!({ "id": "j1", "parentId": "s1", "type": "Job", "name": "Compile", "order": 1, "result": "failed", "startTime": "2026-01-01T00:00:00Z", "finishTime": "2026-01-01T00:01:00Z" }),
            json!({ "id": "t2", "parentId": "j1", "type": "Task", "name": "Test", "order": 2, "result": "failed", "log": { "id": 9 } }),
            json!({ "id": "t1", "parentId": "j1", "type": "Task", "name": "Restore", "order": 1, "result": "succeeded" }),
        ];
        let run = azure_run(&records, "u");
        assert_eq!(run.jobs.iter().map(|j| (j.name.as_str(), j.stage.as_deref(), j.state)).collect::<Vec<_>>(), [("Compile", Some("Build"), CheckState::Failed), ("Ship", Some("Deploy"), CheckState::Passed)]);
        assert_eq!(run.jobs[0].seconds, Some(60.0));
        assert_eq!(run.jobs[0].steps.iter().map(|s| (s.name.as_str(), s.log.as_deref())).collect::<Vec<_>>(), [("Restore", None), ("Test", Some("9"))]);
    }
}
