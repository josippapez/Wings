//! A linked pull request's state, read with the host's CLI: how to ask for it, what the answer means, and what that does to
//! an issue that is still in progress or in review.

use crate::{
    domain::{Issue, Op, Status, Update},
    journal::is_live,
    relations::children_of,
};
use serde_json::Value;
use wings_plugin::azure::{self, AzureRemote};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrState {
    Merged,
    Closed,
    Open,
}

/// A program and its arguments.
pub type Command = (&'static str, Vec<String>);

fn digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

/// The program and arguments that print a PR's state, or none for a link that isn't a GitHub or Azure DevOps PR.
pub fn pr_state_command(url: &str) -> Option<Command> {
    let words = |args: &[&str]| args.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>();
    if let Some(path) = url.strip_prefix("https://github.com/") {
        let parts: Vec<&str> = path.split('/').collect();
        if let [owner, repo, "pull", number] = parts[..] {
            if !owner.is_empty() && !repo.is_empty() && digits(number) {
                return Some(("gh", words(&["pr", "view", url, "--json", "state", "--jq", ".state"])));
            }
        }
    }
    let path = url.strip_prefix("https://dev.azure.com/")?;
    let parts: Vec<&str> = path.split('/').collect();
    if let [org, project, "_git", repo, "pullrequest", number] = parts[..] {
        if !org.is_empty() && !project.is_empty() && !repo.is_empty() && digits(number) {
            return Some(("az", words(&["repos", "pr", "show", "--id", number, "--organization", &format!("https://dev.azure.com/{org}"), "--query", "status", "--output", "tsv"])));
        }
    }
    None
}

/// What the CLI printed: gh says MERGED, CLOSED or OPEN, az says completed, abandoned or active.
pub fn pr_state_of(output: &str) -> Option<PrState> {
    match output.trim().to_lowercase().as_str() {
        "merged" | "completed" => Some(PrState::Merged),
        "closed" | "abandoned" => Some(PrState::Closed),
        "open" | "active" => Some(PrState::Open),
        _ => None,
    }
}

/// The note Issues leaves on a live issue whose PR is closed without merging. It's left once, and the issue stays as it is.
pub fn closed_note(url: &str) -> String {
    format!("The linked pull request was closed without merging: {url}. Still open here, so check whether the work is done.")
}

/// The note on a merged PR's issue that still has open child issues, which stays open with them. Left once.
pub fn children_note(url: &str) -> String {
    format!("The linked pull request was merged: {url}. Its child issues are still open, so it stays open too.")
}

fn note_once(issue: &Issue, text: String) -> Vec<Op> {
    if issue.log.iter().any(|e| e.kind == "note" && e.text == text) { Vec::new() } else { vec![Op::Note { key: issue.key.clone(), text }] }
}

fn done(key: &str) -> Op {
    Op::Update(Update { key: key.to_owned(), status: Some(Status::Done.as_str().to_owned()), ..Update::default() })
}

/// What a PR's state does to an issue linked to it, among the project's `issues`: a merged PR finishes a live issue unless a
/// child issue is still open, a closed one leaves a note once. Nothing for an issue that isn't in progress or in review, or a
/// PR still open.
pub fn pr_ops(issue: &Issue, state: PrState, issues: &[Issue]) -> Vec<Op> {
    let Some(url) = issue.links.as_ref().and_then(|l| l.pr.as_deref()).filter(|_| is_live(issue)) else {
        return Vec::new();
    };
    match state {
        PrState::Merged => {
            let open = children_of(issues, &issue.key).iter().any(|c| c.status != Status::Done && c.status != Status::Canceled);
            if open { note_once(issue, children_note(url)) } else { vec![Op::Note { key: issue.key.clone(), text: format!("The linked pull request was merged: {url}.") }, done(&issue.key)] }
        }
        PrState::Closed => note_once(issue, closed_note(url)),
        PrState::Open => Vec::new(),
    }
}

/// A PR found for a branch: its link and state.
#[derive(Clone, PartialEq, Debug)]
pub struct FoundPr {
    pub url: String,
    pub state: PrState,
}

/// Where a repo is hosted, as far as PRs can be looked up: github.com and Azure DevOps.
#[derive(Clone, PartialEq, Debug)]
pub enum Remote {
    Github,
    Azure(AzureRemote),
    Other,
}

/// The host a remote URL names (`https://github.com/a/b`, `git@github.com:a/b.git`, `ssh://git@ssh.github.com:443/a/b`).
pub fn parse_remote(url: &str) -> Remote {
    let url = url.trim();
    if let Some(remote) = azure::parse_remote(url) {
        return Remote::Azure(remote);
    }
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let rest = rest.rsplit_once('@').map_or(rest, |(_, host)| host);
    let host = rest.split(['/', ':']).next().unwrap_or_default();
    // ssh.github.com on port 443 is github.com's ssh over the https port.
    if host == "github.com" || host == "ssh.github.com" { Remote::Github } else { Remote::Other }
}

/// The program and arguments that list the PRs whose source branch is `branch`, open or not, run in the project's folder.
/// None for a host other than github.com and Azure DevOps, or a branch that could pass for a flag.
pub fn branch_pr_command(remote: &Remote, branch: &str) -> Option<Command> {
    if branch.is_empty() || branch.starts_with('-') {
        return None;
    }
    let words = |args: &[&str]| args.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>();
    match remote {
        Remote::Github => Some(("gh", words(&["pr", "list", "--head", branch, "--state", "all", "--limit", "5", "--json", "url,state"]))),
        Remote::Azure(r) => Some(("az", words(&["repos", "pr", "list", "--organization", &r.org, "--project", &r.project, "--repository", &r.repo, "--source-branch", branch, "--status", "all", "--top", "5", "--output", "json"]))),
        Remote::Other => None,
    }
}

/// `encodeURIComponent`.
fn encode(text: &str) -> String {
    text.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// The PR of a branch from what that command printed, newest first: an open one, else the newest. None when there is none.
pub fn branch_pr_of(remote: &Remote, output: &str) -> Option<FoundPr> {
    let list: Value = serde_json::from_str(output).ok()?;
    let found: Vec<FoundPr> = list
        .as_array()?
        .iter()
        .filter_map(|pr| match remote {
            Remote::Github => Some(FoundPr { url: pr["url"].as_str()?.to_owned(), state: pr_state_of(pr["state"].as_str().unwrap_or_default())? }),
            Remote::Azure(r) => {
                let state = pr_state_of(pr["status"].as_str().unwrap_or_default())?;
                let base = pr["repository"]["webUrl"].as_str().map_or_else(|| format!("{}/{}/_git/{}", r.org, encode(&r.project), encode(&r.repo)), str::to_owned);
                Some(FoundPr { url: format!("{base}/pullrequest/{}", pr["pullRequestId"].as_u64()?), state })
            }
            Remote::Other => None,
        })
        .collect();
    found.iter().find(|f| f.state == PrState::Open).or_else(|| found.first()).cloned()
}

/// What the PR tracker's `pr-state` event says: a tracked PR merged or closed.
#[derive(Clone, PartialEq, Debug)]
pub struct PrStateEvent {
    pub url: String,
    pub state: PrState,
    pub source_branch: String,
    pub project_id: String,
}

/// The event's fields, or none when what arrived isn't one. It comes from another plugin as JSON, so it is checked.
pub fn pr_state_event_of(data: &Value) -> Option<PrStateEvent> {
    let text = |name: &str| data[name].as_str().filter(|s| !s.is_empty());
    let state = match data["state"].as_str()? {
        "merged" => PrState::Merged,
        "closed" => PrState::Closed,
        _ => return None,
    };
    Some(PrStateEvent { url: text("url")?.to_owned(), state, source_branch: text("sourceBranch")?.to_owned(), project_id: data["projectId"].as_str()?.to_owned() })
}

/// Whether a PR that merged or closed is this live issue's: its link is that PR, or it has no link and its branch is the
/// PR's source branch.
pub fn is_pr_of(issue: &Issue, url: &str, source_branch: &str) -> bool {
    if !is_live(issue) {
        return false;
    }
    let links = issue.links.as_ref();
    match links.and_then(|l| l.pr.as_deref()) {
        Some(linked) => linked == url,
        None => links.and_then(|l| l.branch.as_deref()) == Some(source_branch),
    }
}

/// What finding a PR for a live issue does: link it when the issue has no link yet, then what its state does (`pr_ops`).
pub fn found_ops(issue: &Issue, found: &FoundPr, issues: &[Issue]) -> Vec<Op> {
    let linked = issue.links.as_ref().and_then(|l| l.pr.as_deref()) == Some(found.url.as_str());
    let mut ops = if linked { Vec::new() } else { vec![Op::Update(Update { key: issue.key.clone(), pr: Some(found.url.clone()), ..Update::default() })] };
    let mut with_pr = issue.clone();
    with_pr.links.get_or_insert_with(Default::default).pr = Some(found.url.clone());
    ops.extend(pr_ops(&with_pr, found.state, issues));
    ops
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{IssueType, Links, LogEntry, Actor};
    use serde_json::json;

    const ADO: &str = "https://dev.azure.com/Org/Some%20Project/_git/Repo/pullrequest/6762";
    const GH: &str = "https://github.com/owner/repo/pull/50";

    #[test]
    fn a_github_or_azure_devops_pr_link_becomes_the_cli_call_that_prints_its_state_anything_else_is_skipped() {
        let to = |args: &[&str]| args.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>();
        assert_eq!(pr_state_command(GH), Some(("gh", to(&["pr", "view", GH, "--json", "state", "--jq", ".state"]))));
        assert_eq!(pr_state_command(ADO), Some(("az", to(&["repos", "pr", "show", "--id", "6762", "--organization", "https://dev.azure.com/Org", "--query", "status", "--output", "tsv"]))));
        for other in ["https://gitlab.com/a/b/-/merge_requests/3", "https://github.com/owner/repo/pull/50/files", "not a url"] {
            assert_eq!(pr_state_command(other), None, "{other}");
        }
    }

    #[test]
    fn gh_and_az_words_read_as_merged_closed_or_open() {
        let read: Vec<Option<PrState>> = ["MERGED\n", "completed\n", "CLOSED", "abandoned", "OPEN", "active", "draft", ""].into_iter().map(pr_state_of).collect();
        let (m, c, o) = (Some(PrState::Merged), Some(PrState::Closed), Some(PrState::Open));
        assert_eq!(read, [m, m, c, c, o, o, None, None]);
    }

    fn issue(status: Status, log: Vec<LogEntry>) -> Issue {
        Issue { key: "SCI-7".into(), title: "t".into(), description: None, status, kind: IssueType::Task, priority: None, parent: None, related: None, blocked_by: None, links: Some(Links { pr: Some(ADO.into()), ..Links::default() }), files: None, sessions: vec![], log, created_at: 0.0, updated_at: 0.0 }
    }

    fn note(key: &str, text: String) -> Op {
        Op::Note { key: key.into(), text }
    }

    #[test]
    fn a_merged_pr_finishes_a_live_issue_with_a_note_saying_why_done_to_do_and_open_prs_are_left_alone() {
        assert_eq!(pr_ops(&issue(Status::InReview, vec![]), PrState::Merged, &[]), [note("SCI-7", format!("The linked pull request was merged: {ADO}.")), done("SCI-7")]);
        assert_eq!(pr_ops(&issue(Status::InProgress, vec![]), PrState::Merged, &[]).last(), Some(&done("SCI-7")));
        for status in [Status::Todo, Status::Done, Status::Canceled] {
            assert!(pr_ops(&issue(status, vec![]), PrState::Merged, &[]).is_empty(), "{status:?}");
        }
        assert!(pr_ops(&issue(Status::InReview, vec![]), PrState::Open, &[]).is_empty());
        let mut unlinked = issue(Status::InReview, vec![]);
        unlinked.links = Some(Links::default());
        assert!(pr_ops(&unlinked, PrState::Merged, &[]).is_empty());
    }

    fn logged(text: String) -> Vec<LogEntry> {
        vec![LogEntry { at: 1.0, by: Actor::Git, kind: "note".into(), text }]
    }

    #[test]
    fn a_pr_closed_without_merging_leaves_a_note_once_and_keeps_the_status() {
        assert_eq!(pr_ops(&issue(Status::InReview, vec![]), PrState::Closed, &[]), [note("SCI-7", closed_note(ADO))]);
        assert!(pr_ops(&issue(Status::InReview, logged(closed_note(ADO))), PrState::Closed, &[]).is_empty());
    }

    #[test]
    fn a_merged_pr_leaves_an_issue_with_open_children_open_with_a_note_once_finished_children_dont_hold_it() {
        let mut epic = issue(Status::InReview, vec![]);
        epic.kind = IssueType::Epic;
        let child = |status: Status| {
            let mut c = issue(status, vec![]);
            c.key = "SCI-26".into();
            c.parent = Some("SCI-7".into());
            c.links = None;
            c
        };
        assert_eq!(pr_ops(&epic, PrState::Merged, &[epic.clone(), child(Status::Todo)]), [note("SCI-7", children_note(ADO))]);
        let mut noted = epic.clone();
        noted.log = logged(children_note(ADO));
        assert!(pr_ops(&noted, PrState::Merged, &[epic.clone(), child(Status::Todo)]).is_empty());
        assert_eq!(pr_ops(&epic, PrState::Merged, &[epic.clone(), child(Status::Done), child(Status::Canceled)]).last(), Some(&done("SCI-7")));
    }

    fn azure() -> Remote {
        Remote::Azure(AzureRemote { org: "https://dev.azure.com/Org".into(), project: "Some Project".into(), repo: "Repo".into() })
    }

    #[test]
    fn a_remote_is_read_for_the_hosts_that_have_prs_to_look_up() {
        assert_eq!(parse_remote("git@github.com:owner/repo.git"), Remote::Github);
        assert_eq!(parse_remote("https://github.com/owner/repo"), Remote::Github);
        assert_eq!(parse_remote("ssh://git@ssh.github.com:443/owner/repo.git"), Remote::Github);
        assert_eq!(parse_remote("git@ssh.dev.azure.com:v3/Org/Some%20Project/Repo"), azure());
        assert_eq!(parse_remote("https://gitlab.com/a/b"), Remote::Other);
        assert_eq!(parse_remote("https://notgithub.com/a/b"), Remote::Other);
        assert_eq!(parse_remote(""), Remote::Other);
    }

    #[test]
    fn a_branchs_prs_are_listed_with_the_hosts_cli_for_github_and_azure_devops_only_and_never_for_a_branch_that_looks_like_a_flag() {
        let to = |args: &[&str]| args.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>();
        assert_eq!(branch_pr_command(&Remote::Github, "feature/x"), Some(("gh", to(&["pr", "list", "--head", "feature/x", "--state", "all", "--limit", "5", "--json", "url,state"]))));
        let (_, args) = branch_pr_command(&azure(), "feature/x").unwrap();
        assert_eq!(args.last().unwrap(), "json");
        assert_eq!(args[args.iter().position(|a| a == "--source-branch").unwrap() + 1], "feature/x");
        assert_eq!(branch_pr_command(&Remote::Other, "x"), None);
        assert_eq!(branch_pr_command(&Remote::Github, "--repo=evil"), None);
    }

    #[test]
    fn the_pr_of_a_branch_is_the_open_one_else_the_newest_nothing_for_no_prs_or_output_that_isnt_a_list() {
        let gh = json!([{ "url": "https://github.com/owner/repo/pull/9", "state": "MERGED" }, { "url": "https://github.com/owner/repo/pull/8", "state": "CLOSED" }]).to_string();
        assert_eq!(branch_pr_of(&Remote::Github, &gh), Some(FoundPr { url: "https://github.com/owner/repo/pull/9".into(), state: PrState::Merged }));
        assert_eq!(branch_pr_of(&Remote::Github, &json!([{ "url": "u", "state": "CLOSED" }, { "url": "v", "state": "OPEN" }]).to_string()).unwrap().url, "v");
        let az = json!([{ "pullRequestId": 6762, "status": "completed", "repository": { "webUrl": "https://dev.azure.com/Org/Some%20Project/_git/Repo" } }]).to_string();
        assert_eq!(branch_pr_of(&azure(), &az), Some(FoundPr { url: ADO.into(), state: PrState::Merged }));
        assert_eq!(branch_pr_of(&azure(), &json!([{ "pullRequestId": 5, "status": "active" }]).to_string()).unwrap().url, "https://dev.azure.com/Org/Some%20Project/_git/Repo/pullrequest/5");
        for out in ["[]", "", "not json", "{}"] {
            assert_eq!(branch_pr_of(&Remote::Github, out), None, "{out}");
        }
    }

    fn on_branch(status: Status, links: Links) -> Issue {
        let mut i = issue(status, vec![]);
        i.links = Some(links);
        i
    }

    #[test]
    fn a_merged_pr_belongs_to_a_live_issue_by_its_link_or_by_its_branch_when_it_has_no_link() {
        let branch = || Links { branch: Some("feature/x".into()), ..Links::default() };
        assert!(is_pr_of(&on_branch(Status::InReview, branch()), GH, "feature/x"));
        assert!(is_pr_of(&on_branch(Status::InReview, Links { pr: Some(GH.into()), ..Links::default() }), GH, "feature/x"));
        assert!(!is_pr_of(&on_branch(Status::InReview, Links { pr: Some(ADO.into()), branch: Some("feature/x".into()), ..Links::default() }), GH, "feature/x"));
        assert!(!is_pr_of(&on_branch(Status::InReview, Links { branch: Some("other".into()), ..Links::default() }), GH, "feature/x"));
        assert!(!is_pr_of(&on_branch(Status::Done, branch()), GH, "feature/x"));
        assert!(!is_pr_of(&on_branch(Status::Todo, branch()), GH, "feature/x"));
    }

    #[test]
    fn finding_a_pr_links_it_and_then_does_what_its_state_does() {
        let link = |url: &str| Op::Update(Update { key: "SCI-7".into(), pr: Some(url.into()), ..Update::default() });
        let branch = || Links { branch: Some("feature/x".into()), ..Links::default() };
        assert_eq!(found_ops(&on_branch(Status::InProgress, branch()), &FoundPr { url: GH.into(), state: PrState::Merged }, &[]), [link(GH), note("SCI-7", format!("The linked pull request was merged: {GH}.")), done("SCI-7")]);
        assert_eq!(found_ops(&on_branch(Status::InProgress, branch()), &FoundPr { url: GH.into(), state: PrState::Open }, &[]), [link(GH)]);
        assert!(matches!(found_ops(&on_branch(Status::InProgress, Links { pr: Some(GH.into()), ..Links::default() }), &FoundPr { url: GH.into(), state: PrState::Merged }, &[])[0], Op::Note { .. }));
    }

    #[test]
    fn the_pr_state_event_is_read_from_json_and_refused_when_a_field_is_missing_or_wrong() {
        let event = pr_state_event_of(&json!({ "url": GH, "state": "merged", "sourceBranch": "b", "projectId": "p", "extra": 1 }));
        assert_eq!(event, Some(PrStateEvent { url: GH.into(), state: PrState::Merged, source_branch: "b".into(), project_id: "p".into() }));
        for bad in [json!(null), json!({}), json!({ "url": GH, "state": "open", "sourceBranch": "b", "projectId": "p" }), json!({ "url": GH, "state": "merged", "sourceBranch": "", "projectId": "p" })] {
            assert_eq!(pr_state_event_of(&bad), None);
        }
    }
}
