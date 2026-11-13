//! The shapes the PR tracker keeps in its maps and passes between its modules and its frames (`lib/types.ts`). They
//! serialize with the TypeScript's field names, so what is stored or sent between frames reads the same.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use wings_plugin::remote::Remote;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Github,
    Azure,
    Gitlab,
    Bitbucket,
    Gitea,
}

/// A remote in a JSON field: `Remote::to_json` and `Remote::from_json`.
mod remote_json {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(remote: &Option<Remote>, s: S) -> Result<S::Ok, S::Error> {
        remote.as_ref().map(Remote::to_json).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Remote>, D::Error> {
        Ok(Option::<Value>::deserialize(d)?.and_then(|value| Remote::from_json(&value)))
    }
}

/// The PR a pane, a list row or a tab is on: `{ provider, url, number, repo, ...provider fields }`.
/// `remote` is set for every provider but GitHub (the remote of the project); `cwd` for Azure DevOps and Bitbucket
/// (the project's folder, which the diff is made in); `raw` for Azure DevOps (the PR as `az repos pr` prints it).
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Link {
    pub provider: Provider,
    pub url: String,
    pub number: u64,
    pub repo: String,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "remote_json")]
    pub remote: Option<Remote>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrState {
    Open,
    Merged,
    Closed,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Counts {
    pub passed: u32,
    pub failed: u32,
    pub running: u32,
}

/// A failed check as the status lists it. Which fields are set depends on the provider. `id` and `job` are text, which
/// the numeric ones of GitLab and Azure DevOps are written as.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct Failing {
    pub id: String,
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<u64>,
}

/// Where a check's run is read: an Azure DevOps build, or a GitHub Actions or GitLab job.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct RunRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<u64>,
}

/// What a check is doing. `Skipped` is not run, not applicable or waiting for someone to start it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckState {
    Failed,
    Running,
    Queued,
    Passed,
    Skipped,
}

/// One step of a job. `log` is what the forge reads the step's log by, none where the step has none.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct RunStep {
    pub id: String,
    pub name: String,
    pub state: CheckState,
    pub seconds: Option<f64>,
    pub log: Option<String>,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct RunJob {
    pub id: String,
    pub name: String,
    /// The stage it ran in, where the run has more than one.
    pub stage: Option<String>,
    pub state: CheckState,
    pub seconds: Option<f64>,
    pub steps: Vec<RunStep>,
}

/// A check's run: its jobs and their steps, and where the provider shows it.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Run {
    pub jobs: Vec<RunJob>,
    pub url: Option<String>,
    /// Why no step has a log yet, where the provider says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// One check or policy of a PR, in the shape the Checks view shows whatever the provider.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct CheckItem {
    pub id: String,
    pub name: String,
    pub state: CheckState,
    /// True when it blocks the merge, false when it doesn't, none where the provider doesn't say.
    pub required: Option<bool>,
    /// What sort of check the provider says it is: a workflow, a build, a status check, required reviewers...
    pub kind: Option<String>,
    /// When it finished, else when it started (ISO).
    pub at: Option<String>,
    pub seconds: Option<f64>,
    /// Where the provider shows it. Only https is ever opened.
    pub url: Option<String>,
    /// Why it is in this state, from the provider's data: "2 of 3 required reviewers approved".
    pub note: Option<String>,
    /// Set where its log can be fetched, which a forge's `failure` takes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failing: Option<Failing>,
    /// Set where its run's jobs, steps and logs can be read, which a forge's `run` takes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<RunRef>,
    /// The page fills the note from what it has loaded: a comment requirement counts the open threads (`"unresolved"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub needs: Option<String>,
}

/// Normalized status of a PR.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Status {
    pub key: String,
    pub title: String,
    pub label: String,
    /// What the tracker decides by, never the label: a forge words a closed PR its own way, like Azure DevOps's "Abandoned".
    pub state: PrState,
    pub tone: String,
    pub icon: String,
    pub checks: Counts,
    pub review: String,
    pub failing: Vec<Failing>,
    /// Every check, from the data the status already loaded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<CheckItem>>,
    pub changes: Option<String>,
    pub branch: String,
    pub updated: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "baseCommit")]
    pub base_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "headCommit")]
    pub head_commit: Option<String>,
}

/// A failed check's details: its failed step, the line that says why, and the end of its log.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct FailureDetail {
    pub check: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    pub summary: String,
    pub log: String,
    pub url: Option<String>,
}

/// The few lines of a file around a commented range.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct SnippetLine {
    pub n: i64,
    pub text: String,
    /// In the commented range.
    pub hit: bool,
    /// The PR added or changed this line. Set only when the diff was read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added: Option<bool>,
    /// The PR removed this line, so `n` is its number before the change. Set only on rows read from a diff.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed: Option<bool>,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct ThreadState {
    /// `active` or `resolved`.
    pub state: String,
    pub label: String,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct LineRange {
    pub start: i64,
    pub end: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iteration: Option<i64>,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct CommitRefs {
    pub base: String,
    pub head: String,
}

/// A comment, as the diff viewer takes it (`WingsComment`) and the PR tab shows it.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub id: i64,
    pub reply_to: Option<i64>,
    pub path: Option<String>,
    pub line: Option<i64>,
    /// `additions` or `deletions`.
    pub side: String,
    pub author: String,
    pub body: String,
    pub created_at: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_reply: Option<bool>,
    /// The account that wrote it, in the form `Me.ids` has. `author` is a display name on some forges and can't be matched with that.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    /// The GitLab discussion a note belongs to, which a reply goes to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discussion: Option<String>,
    /// On a thread's first comment, where the provider keeps thread states.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<ThreadState>,
    /// On a code thread's first comment, the commented lines and the PR iteration they were written on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<LineRange>,
    /// On a code thread's first comment, the code it is on, where the comment itself carries it (GitHub's diff hunk).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<Vec<SnippetLine>>,
    /// On a code thread's first comment, the two commits its lines are read between, where the provider says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refs: Option<CommitRefs>,
    /// Written by the provider itself (a policy update, a status change) rather than a person.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub system: bool,
}

/// A reviewer's latest verdict on a PR: `vote` is `approved`, `changes`, `waiting` or `none`.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Reviewer {
    pub name: String,
    pub vote: String,
}

/// The status, author and description of a PR, which the PR tab shows.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub status: Status,
    pub author: String,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewers: Option<Vec<Reviewer>>,
}

/// What the diff viewer shows of a PR.
#[derive(Clone, PartialEq, Debug)]
pub struct Diff {
    pub key: Option<String>,
    pub patch: String,
    pub comments: Vec<Comment>,
}

/// One commit of a push.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct PushCommit {
    pub sha: String,
    /// The first line of the commit message.
    pub message: String,
    pub author: String,
    pub at: String,
}

/// One push to the PR's branch: Azure DevOps numbers them as the PR's iterations.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Push {
    pub id: i64,
    pub author: String,
    pub at: String,
    pub commits: Vec<PushCommit>,
    /// The list was cut by the provider, so there are more commits than shown.
    pub more: bool,
    /// The source and the merge base at this iteration, which a thread's snippet is read from.
    pub head: Option<String>,
    pub base: Option<String>,
    /// The push that opened the PR: Azure DevOps gives its iteration the reason `create`.
    pub created: bool,
}

/// Who you are on a forge: `ids` match `Comment.by`, `handles` are what a mention in a comment's text is written with (`@handle`).
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct Me {
    pub ids: Vec<String>,
    pub handles: Vec<String>,
}

/// An open PR from a repo's list, as a project's Pull requests section shows it.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListedPr {
    pub link: Link,
    pub title: String,
    /// The source branch, which an agent pane's branch is matched against.
    pub head: String,
    /// You wrote it.
    pub mine: bool,
    /// Your review is asked for.
    pub reviewing: bool,
    pub author: Option<String>,
    /// From the list alone, so GitLab and Azure DevOps have no check counts until the PR's own status loads.
    pub status: Status,
    /// True where the list carries what a PR's own status does, so it can stand for that status.
    pub loaded: bool,
    /// Comments and reviews so far, or none where the list doesn't say.
    pub activity: Option<u64>,
    /// When the PR last changed (ISO). None where the list doesn't say, as on Azure DevOps.
    pub updated_at: Option<String>,
}

/// What a "pr" tab is opened with (`open_view_with("pr", "tab", args)`, read with `view_args`): enough to find the PR
/// again without the pane or the list that showed it.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct PrArgs {
    pub provider: Provider,
    pub url: String,
    pub number: u64,
    pub repo: String,
    pub title: String,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "remote_json")]
    pub remote: Option<Remote>,
}
