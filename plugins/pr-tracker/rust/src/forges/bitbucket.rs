//! Bitbucket Cloud: pull requests through the REST API 2.0 with an API token, the diff from local git, replies and approvals through the same token.

use super::shared::{Facts, Words as Labels, counts, encode_query, label_of, open_or_newest, with_second_page};
use super::token::{Account, forge_json, forge_post, forge_user, forget_users};
use crate::checklist::real;
use crate::forge::ForgeError;
use crate::snippet::diff_window;
use crate::types::*;
use serde_json::{Value, json};
use std::collections::HashMap;
use wings_plugin::{cache::Cached, git, remote::Remote, task::join};

const API: &str = "https://api.bitbucket.org/2.0/";
/// Bitbucket answers at most 50 pull requests a page.
const PAGE: usize = 50;

fn account() -> Account {
    Account {
        key: "bitbucket".into(),
        secret: "bitbucket".into(),
        name: "Bitbucket".into(),
        scope_hint: "The API token needs the read:pullrequest:bitbucket and read:user:bitbucket scopes.",
        write_hint: "The API token needs the write:pullrequest:bitbucket scope to reply and approve.",
    }
}

thread_local! {
    /// The token's account in full, for a mention, which Bitbucket writes with the account id or the uuid.
    static PROFILE: Cached<String, Value> = Cached::new(f64::INFINITY, 0.0, |_: String| async { forge_json(&account(), &format!("{API}user")).await.map_err(|error| error.to_wire()) });
    /// The two branches a PR is between, which its snippets are diffed over, by the PR's url.
    static BRANCHES: Cached<(String, u64, Remote), (String, String)> = Cached::new(f64::INFINITY, 0.0, |(_, number, remote): (String, u64, Remote)| async move {
        let Remote::Bitbucket { workspace, repo } = remote else { return Err("not a Bitbucket remote".to_owned()) };
        let pr = forge_json(&account(), &format!("{API}repositories/{workspace}/{repo}/pullrequests/{number}")).await.map_err(|error| error.to_wire())?;
        Ok((pr["destination"]["branch"]["name"].as_str().unwrap_or_default().to_owned(), pr["source"]["branch"]["name"].as_str().unwrap_or_default().to_owned()))
    });
    /// The branches fetched once, for every thread that reads a snippet.
    static SYNCED: Cached<(String, String, String), ()> = Cached::new(f64::INFINITY, 0.0, |(cwd, base, head): (String, String, String)| async move {
        git::git(&cwd, &["fetch", "--quiet", "--end-of-options", "origin", &base, &head]).await.map(drop)
    });
}

pub fn problem_key(_remote: &Remote) -> String {
    account().key
}

pub fn repo_key(remote: &Remote) -> String {
    match remote {
        Remote::Bitbucket { workspace, repo } => format!("bitbucket:{workspace}/{repo}"),
        _ => String::new(),
    }
}

// ---------- the API's shapes and how they map ----------

/// The list leaves out each PR's reviewers and participants as too costly, so they are asked for: `+` adds to the default fields and is sent as `%2B` (Bitbucket REST intro, "Partial responses").
pub const LIST_FIELDS: &str = "fields=%2Bvalues.participants,%2Bvalues.reviewers";

fn name_of(user: &Value) -> String {
    user["display_name"].as_str().or(user["nickname"].as_str()).unwrap_or("unknown").to_owned()
}

fn state_of(status: &Value) -> CheckState {
    match status["state"].as_str() {
        Some("SUCCESSFUL") => CheckState::Passed,
        Some("FAILED" | "STOPPED") => CheckState::Failed,
        _ => CheckState::Running,
    }
}

fn note_of(status: &Value) -> Option<String> {
    status["description"].as_str().filter(|d| !d.is_empty()).map(str::to_owned).or_else(|| (status["state"] == "STOPPED").then(|| "Stopped".to_owned()))
}

/// A build result as a check of the Checks view.
pub fn bb_item(s: &Value) -> CheckItem {
    let key = s["key"].as_str().unwrap_or_default();
    CheckItem {
        id: key.to_owned(),
        name: s["name"].as_str().unwrap_or(key).to_owned(),
        state: state_of(s),
        required: None,
        kind: Some(if s["type"] == "build" { "Build" } else { "Status check" }.to_owned()),
        at: real(s["updated_on"].as_str()).or_else(|| real(s["created_on"].as_str())),
        seconds: None,
        url: s["url"].as_str().map(str::to_owned),
        note: note_of(s),
        failing: None,
        run: None,
        needs: None,
    }
}

/// Reviewers who approved, and whether any asked for changes. Bitbucket marks a reviewer `approved`, or `state: "changes_requested"`.
fn bb_reviews(pr: &Value) -> (usize, bool) {
    let reviewers: Vec<&Value> = pr["participants"].as_array().into_iter().flatten().filter(|p| p["role"] == "REVIEWER").collect();
    (reviewers.iter().filter(|p| p["approved"] == true).count(), reviewers.iter().any(|p| p["state"] == "changes_requested"))
}

pub fn bb_status_of(pr: &Value, statuses: &[Value]) -> Status {
    let results: Vec<&str> = statuses.iter().map(|s| match state_of(s) { CheckState::Passed => "passed", CheckState::Failed => "failed", _ => "running" }).collect();
    let failing: Vec<Failing> = statuses
        .iter()
        .zip(&results)
        .filter(|(_, result)| **result == "failed")
        .map(|(s, _)| {
            let key = s["key"].as_str().unwrap_or_default();
            Failing { id: key.to_owned(), name: Some(s["name"].as_str().unwrap_or(key).to_owned()), url: s["url"].as_str().map(str::to_owned), summary: note_of(s), ..Failing::default() }
        })
        .collect();
    let checks = counts(&results);
    let (approved, changes) = bb_reviews(pr);
    let state = pr["state"].as_str();
    let (label, tone, icon, state) = label_of(
        &Facts { merged: state == Some("MERGED"), closed: matches!(state, Some("DECLINED" | "SUPERSEDED")), draft: pr["draft"] == true, checks, changes, approved: approved > 0, ..Facts::default() },
        &Labels::default(),
    );
    let (base, head) = (pr["destination"]["branch"]["name"].as_str().unwrap_or_default(), pr["source"]["branch"]["name"].as_str().unwrap_or_default());
    Status {
        // A new push changes the source commit, which is what invalidates the cached diff.
        key: pr["source"]["commit"]["hash"].as_str().or(pr["updated_on"].as_str()).unwrap_or_default().to_owned(),
        title: pr["title"].as_str().unwrap_or_default().to_owned(),
        label,
        tone,
        icon,
        state,
        checks,
        failing,
        items: Some(statuses.iter().map(bb_item).collect()),
        review: if changes { "Changes requested".into() } else if approved > 0 { format!("{approved} approved") } else { "No approvals yet".into() },
        changes: None,
        branch: format!("{head} → {base}"),
        updated: pr["updated_on"].as_str().unwrap_or_default().to_owned(),
        merge: None,
        base: Some(base.to_owned()),
        head: Some(head.to_owned()),
        base_commit: None,
        head_commit: None,
    }
}

/// The PRs whose branch is in this repository: a branch name alone also matches a fork's branch of the same name.
pub fn bb_own<'a>(prs: &'a [Value], full_name: &str) -> Vec<&'a Value> {
    prs.iter().filter(|p| p["source"]["repository"]["full_name"].as_str().is_some_and(|name| name.to_lowercase() == full_name.to_lowercase())).collect()
}

/// Who a listed PR is for: `(mine, reviewing)`, written by `me`, or with `me` among its reviewers.
pub fn bb_who(pr: &Value, me: &str) -> (bool, bool) {
    let reviewing = pr["participants"].as_array().is_some_and(|list| list.iter().any(|p| p["role"] == "REVIEWER" && p["user"]["uuid"] == me))
        || pr["reviewers"].as_array().is_some_and(|list| list.iter().any(|u| u["uuid"] == me));
    (pr["author"]["uuid"] == me, reviewing)
}

/// Every open PR of a repo, in the list's order, each marked yours or waiting for you. `link` builds the ref, since a Bitbucket link carries the folder.
pub fn bb_listed(prs: &[Value], me: &str, link: impl Fn(&Value) -> Link) -> Vec<ListedPr> {
    prs.iter()
        .map(|pr| {
            let (mine, reviewing) = bb_who(pr, me);
            let (_, changes) = bb_reviews(pr);
            let (label, tone, icon, _) = label_of(&Facts { draft: pr["draft"] == true, changes, ..Facts::default() }, &Labels::default());
            ListedPr {
                link: link(pr),
                title: pr["title"].as_str().unwrap_or_default().to_owned(),
                head: pr["source"]["branch"]["name"].as_str().unwrap_or_default().to_owned(),
                mine,
                reviewing,
                author: pr["author"].is_object().then(|| name_of(&pr["author"])),
                status: Status { label, tone, icon, ..bb_status_of(pr, &[]) },
                loaded: false,
                activity: pr["comment_count"].as_u64(),
                updated_at: pr["updated_on"].as_str().map(str::to_owned),
            }
        })
        .collect()
}

/// Comments in Bitbucket's order. A reply's `parent` is the comment it answers, which may itself be a reply, so each is moved under its thread's first comment.
pub fn bb_comments(values: &[Value], url: &str) -> Vec<Comment> {
    let live: Vec<&Value> = values.iter().filter(|c| c["deleted"] != true).collect();
    let parents: HashMap<i64, Option<i64>> = live.iter().map(|c| (c["id"].as_i64().unwrap_or(0), c["parent"]["id"].as_i64())).collect();
    let root_of = |mut id: i64| {
        while let Some(Some(parent)) = parents.get(&id).copied() {
            if !parents.contains_key(&parent) {
                break;
            }
            id = parent;
        }
        id
    };
    live.iter()
        .map(|c| {
            let id = c["id"].as_i64().unwrap_or(0);
            let root = root_of(id);
            let inline = c["inline"].is_object().then(|| &c["inline"]);
            let to = inline.and_then(|i| i["to"].as_i64());
            let line = inline.and_then(|i| to.or(i["from"].as_i64()));
            Comment {
                id,
                reply_to: (root != id).then_some(root),
                path: inline.and_then(|i| i["path"].as_str()).map(str::to_owned),
                line,
                side: if to.is_some_and(|to| to != 0) || inline.is_none() { "additions" } else { "deletions" }.into(),
                author: name_of(&c["user"]),
                by: c["user"]["uuid"].as_str().map(str::to_owned),
                body: c["content"]["raw"].as_str().unwrap_or_default().to_owned(),
                created_at: c["created_on"].as_str().unwrap_or_default().to_owned(),
                url: c["links"]["html"]["href"].as_str().unwrap_or(url).to_owned(),
                can_reply: Some(root == id),
                discussion: None,
                thread: None,
                range: line.filter(|line| *line != 0 && root == id).map(|line| LineRange { start: line, end: line, iteration: None }),
                snippet: None,
                refs: None,
                system: false,
            }
        })
        .collect()
}

// ---------- calling it ----------

fn bitbucket(link: &Link) -> Result<(&str, &str), ForgeError> {
    match &link.remote {
        Some(Remote::Bitbucket { workspace, repo }) => Ok((workspace, repo)),
        _ => Err("This pull request doesn't say which Bitbucket repository it is in.".into()),
    }
}

fn repo_path(workspace: &str, repo: &str) -> String {
    format!("{API}repositories/{workspace}/{repo}")
}

fn pr_url(link: &Link) -> Result<String, ForgeError> {
    let (workspace, repo) = bitbucket(link)?;
    Ok(format!("{}/pullrequests/{}", repo_path(workspace, repo), link.number))
}

async fn get_page(url: &str) -> Result<Vec<Value>, ForgeError> {
    Ok(forge_json(&account(), url).await?["values"].as_array().cloned().unwrap_or_default())
}

/// The account the token belongs to, by uuid, which Bitbucket filters and marks authors and reviewers with.
async fn user() -> Result<String, ForgeError> {
    forge_user(&account(), &format!("{API}user"), |user| user["uuid"].as_str().map(str::to_owned)).await
}

fn to_link(pr: &Value, workspace: &str, repo: &str, cwd: &str) -> Link {
    Link {
        provider: Provider::Bitbucket,
        url: pr["links"]["html"]["href"].as_str().unwrap_or_default().to_owned(),
        number: pr["id"].as_u64().unwrap_or(0),
        repo: format!("{workspace}/{repo}"),
        remote: Some(Remote::Bitbucket { workspace: workspace.to_owned(), repo: repo.to_owned() }),
        cwd: Some(cwd.to_owned()),
        raw: None,
    }
}

async fn load(link: &Link) -> Result<(Value, Vec<Value>), ForgeError> {
    let url = pr_url(link)?;
    let (pr, statuses) = join(forge_json(&account(), &url), get_page(&format!("{url}/statuses?pagelen=100"))).await;
    Ok((pr?, statuses.unwrap_or_default()))
}

pub async fn comments(link: &Link) -> Result<Vec<Comment>, ForgeError> {
    Ok(bb_comments(&get_page(&format!("{}/comments?pagelen=100", pr_url(link)?)).await?, &link.url))
}

pub async fn find(cwd: &str, remote: &Remote, branch: &str) -> Result<Option<Link>, ForgeError> {
    let Remote::Bitbucket { workspace, repo } = remote else { return Ok(None) };
    // A quote or backslash in the branch name would end the filter's string early.
    if branch.contains(['"', '\\']) {
        return Ok(None);
    }
    let states = ["OPEN", "MERGED", "DECLINED", "SUPERSEDED"].map(|s| format!("state={s}")).join("&");
    let q = encode_query(&format!("source.branch.name=\"{branch}\""));
    let found = get_page(&format!("{}/pullrequests?q={q}&{states}&sort=-updated_on&pagelen=20", repo_path(workspace, repo))).await?;
    let own: Vec<Value> = bb_own(&found, &format!("{workspace}/{repo}")).into_iter().cloned().collect();
    Ok(open_or_newest(&own, |p| p["state"] == "OPEN").map(|pr| to_link(pr, workspace, repo, cwd)))
}

/// The account the token belongs to. Its uuid is what a comment's `by` is, and a mention is the account id or the uuid.
pub async fn me(_remote: &Remote) -> Result<Me, ForgeError> {
    let profile = PROFILE.with(Cached::clone).get("user".to_owned()).await?;
    let uuid = profile["uuid"].as_str().unwrap_or_default().to_owned();
    let handles = [Some(uuid.clone()), profile["account_id"].as_str().map(str::to_owned), profile["nickname"].as_str().map(str::to_owned)].into_iter().flatten().filter(|h| !h.is_empty()).collect();
    Ok(Me { ids: vec![uuid], handles })
}

/// Every open PR of the repo: one call, and a second for a repo with more than a page. The list has no check results, which the PR's own status adds.
pub async fn list(cwd: &str, remote: &Remote) -> Result<Vec<ListedPr>, ForgeError> {
    let Remote::Bitbucket { workspace, repo } = remote else { return Ok(Vec::new()) };
    let url = format!("{}/pullrequests?state=OPEN&sort=-updated_on&pagelen={PAGE}&{LIST_FIELDS}", repo_path(workspace, repo));
    let (me, first) = join(user(), get_page(&url)).await;
    let prs = with_second_page(first?, PAGE, || async { get_page(&format!("{url}&page=2")).await }).await?;
    Ok(bb_listed(&prs, &me?, |pr| to_link(pr, workspace, repo, cwd)))
}

pub async fn link_of(args: &PrArgs) -> Result<Link, ForgeError> {
    let Some(remote @ Remote::Bitbucket { .. }) = &args.remote else {
        return Err("This tab doesn't say which Bitbucket repository its pull request is in.".into());
    };
    Ok(Link { provider: Provider::Bitbucket, url: args.url.clone(), number: args.number, repo: args.repo.clone(), remote: Some(remote.clone()), cwd: Some(args.cwd.clone()), raw: None })
}

pub async fn status(link: &Link, _fresh: bool) -> Result<Status, ForgeError> {
    let (pr, statuses) = load(link).await?;
    Ok(bb_status_of(&pr, &statuses))
}

pub async fn overview(link: &Link) -> Result<Overview, ForgeError> {
    let (pr, statuses) = load(link).await?;
    Ok(Overview {
        status: bb_status_of(&pr, &statuses),
        author: pr["author"]["display_name"].as_str().unwrap_or("unknown").to_owned(),
        body: pr["description"].as_str().unwrap_or_default().to_owned(),
        created_at: None,
        reviewers: None,
    })
}

/// The diff comes from local git, as for Azure DevOps: the API's own diff answers with a redirect that `wings.fetch`
/// doesn't follow. Only a PR from a branch of this repository is fetched; one from a fork isn't on `origin`.
pub async fn diff(link: &Link, st: &Status) -> Result<(String, Vec<Comment>), ForgeError> {
    let cwd = link.cwd.as_deref().unwrap_or_default();
    let (base, head) = (st.base.as_deref().unwrap_or_default(), st.head.as_deref().unwrap_or_default());
    git::git(cwd, &["fetch", "--quiet", "--end-of-options", "origin", base, head]).await?;
    let patch = git::raw(cwd, &["diff", "--no-color", "--no-ext-diff", &format!("origin/{base}...origin/{head}")]).await?;
    Ok((patch, comments(link).await?))
}

/// The lines of the PR's diff of the file, up to the commented one, read from local git like the diff itself.
pub async fn snippet(link: &Link, root: &Comment, _pushes: &[Push], _head: Option<&str>, _now: bool) -> Result<Option<Vec<SnippetLine>>, ForgeError> {
    let (Some(path), Some(range)) = (&root.path, &root.range) else { return Ok(None) };
    // A PR from a fork isn't on `origin`, as for the diff itself.
    Ok(read_snippet(link, path, &root.side, range).await.ok().flatten())
}

async fn read_snippet(link: &Link, path: &str, side: &str, range: &LineRange) -> Result<Option<Vec<SnippetLine>>, ForgeError> {
    let cwd = link.cwd.as_deref().unwrap_or_default().to_owned();
    let (Some(remote), url) = (link.remote.clone(), link.url.clone()) else { return Ok(None) };
    let (base, head) = BRANCHES.with(Cached::clone).get((url, link.number, remote)).await?;
    SYNCED.with(Cached::clone).get((cwd.clone(), base.clone(), head.clone())).await?;
    let patch = git::raw(&cwd, &["diff", "--no-color", "--no-ext-diff", &format!("origin/{base}...origin/{head}"), "--", path]).await?;
    Ok(diff_window(&patch, side, range.start, range.end))
}

pub use super::shared::plain_failure as failure;

/// Replies in the thread. The API threads a comment under `parent`, and a reply to a reply is moved under the thread's first comment on read.
pub async fn reply(link: &Link, root: &Comment, body: &str) -> Result<Comment, ForgeError> {
    let fields = json!({ "content": { "raw": body }, "parent": { "id": root.id } });
    let created: Value = serde_json::from_str(&forge_post(&account(), &format!("{}/comments", pr_url(link)?), Some(&fields)).await?).map_err(|error| ForgeError::Other(error.to_string()))?;
    let made = bb_comments(&[created], &link.url).remove(0);
    Ok(Comment { reply_to: Some(root.id), can_reply: Some(false), ..made })
}

/// Approves as the token's account.
pub async fn approve(link: &Link, _sha: &str) -> Result<(), ForgeError> {
    forge_post(&account(), &format!("{}/approve", pr_url(link)?), None).await.map(drop)
}

pub fn forget() {
    forget_users();
    PROFILE.with(Cached::clear);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bb(over: Value) -> Value {
        let mut base = json!({
            "id": 7, "title": "Add x", "state": "OPEN", "draft": false, "comment_count": 3,
            "author": { "uuid": "{me}", "display_name": "Me" },
            "source": { "branch": { "name": "feat" }, "commit": { "hash": "abc123" }, "repository": { "full_name": "w/r" } },
            "destination": { "branch": { "name": "main" } },
            "updated_on": "2026-10-01T10:00:00+00:00",
            "links": { "html": { "href": "https://bitbucket.org/w/r/pull-requests/7" } },
        });
        for (key, value) in over.as_object().unwrap() {
            base[key] = value.clone();
        }
        base
    }

    fn label(pr: Value, statuses: Vec<Value>) -> String {
        bb_status_of(&pr, &statuses).label
    }

    fn link_of_pr(pr: &Value) -> Link {
        to_link(pr, "w", "r", "/work/r")
    }

    #[test]
    fn bitbucket_status_state_draft_checks_and_reviews_decide_the_label() {
        assert_eq!(label(bb(json!({ "state": "MERGED" })), vec![]), "Merged");
        assert_eq!(label(bb(json!({ "state": "DECLINED" })), vec![]), "Closed");
        assert_eq!(label(bb(json!({ "draft": true })), vec![]), "Draft");
        let failed = bb_status_of(&bb(json!({})), &[json!({ "key": "k1", "state": "FAILED", "name": "Pipeline #1", "url": "https://x/1" }), json!({ "key": "k2", "state": "SUCCESSFUL" }), json!({ "key": "k3", "state": "INPROGRESS" })]);
        assert_eq!(failed.label, "Checks failing");
        assert_eq!(failed.checks, Counts { passed: 1, failed: 1, running: 1 });
        assert_eq!(failed.failing.iter().map(|f| (f.id.as_str(), f.name.as_deref(), f.url.as_deref())).collect::<Vec<_>>(), [("k1", Some("Pipeline #1"), Some("https://x/1"))]);
        assert_eq!(label(bb(json!({})), vec![json!({ "key": "k", "state": "INPROGRESS" })]), "Checks running");
        let approved = bb(json!({ "participants": [{ "role": "REVIEWER", "approved": true, "user": { "uuid": "{a}" } }] }));
        assert_eq!(label(approved.clone(), vec![]), "Approved");
        assert_eq!(bb_status_of(&approved, &[]).review, "1 approved");
        let changes = bb(json!({ "participants": [{ "role": "REVIEWER", "approved": false, "state": "changes_requested", "user": { "uuid": "{a}" } }] }));
        assert_eq!(label(changes, vec![]), "Changes requested");
        let st = bb_status_of(&bb(json!({})), &[]);
        assert_eq!((st.key.as_str(), st.branch.as_str(), st.base.as_deref(), st.head.as_deref(), st.label.as_str()), ("abc123", "feat → main", Some("main"), Some("feat"), "In review"));
    }

    #[test]
    fn bitbucket_statuses_are_checks_builds_stopped_ones_and_a_state_it_doesnt_name() {
        assert_eq!(bb_item(&json!({ "key": "k", "type": "build", "state": "SUCCESSFUL", "name": "Build", "updated_on": "2026-01-01T00:00:00Z" })).state, CheckState::Passed);
        assert_eq!(bb_item(&json!({ "key": "k", "state": "INPROGRESS" })).state, CheckState::Running);
        let stopped = bb_item(&json!({ "key": "k", "state": "STOPPED" }));
        assert_eq!((stopped.state, stopped.note.as_deref(), stopped.kind.as_deref()), (CheckState::Failed, Some("Stopped"), Some("Status check")));
        assert_eq!(bb_item(&json!({ "key": "k", "type": "build", "state": "FAILED" })).kind.as_deref(), Some("Build"));
    }

    #[test]
    fn bitbucket_list_keeps_every_open_pr_and_marks_the_ones_you_wrote_or_review() {
        let prs = [
            bb(json!({ "id": 1, "author": { "uuid": "{other}" } })),
            bb(json!({ "id": 2, "author": { "uuid": "{other}" }, "participants": [{ "role": "REVIEWER", "user": { "uuid": "{me}" } }] })),
            bb(json!({ "id": 3, "author": { "uuid": "{other}" }, "reviewers": [{ "uuid": "{me}" }] })),
            bb(json!({ "id": 4 })),
            bb(json!({ "id": 5, "author": { "uuid": "{other}" }, "participants": [{ "role": "PARTICIPANT", "user": { "uuid": "{me}" } }] })),
        ];
        assert_eq!(bb_who(&prs[1], "{me}"), (false, true));
        let rows = bb_listed(&prs, "{me}", link_of_pr);
        assert_eq!(rows.iter().map(|r| (r.link.number, r.mine, r.reviewing)).collect::<Vec<_>>(), [(1, false, false), (2, false, true), (3, false, true), (4, true, false), (5, false, false)]);
        assert_eq!(rows[3].activity, Some(3));
        assert_eq!(rows[3].head, "feat");
        assert!(!rows[3].loaded, "the list has no check results");
    }

    #[test]
    fn bitbucket_comments_inline_position_deleted_ones_dropped_replies_under_the_threads_first_comment() {
        let url = "https://bitbucket.org/w/r/pull-requests/7";
        let list = bb_comments(
            &[
                json!({ "id": 1, "created_on": "t1", "content": { "raw": "top" }, "user": { "display_name": "A" }, "inline": { "path": "src/a.ts", "to": 12, "from": null } }),
                json!({ "id": 2, "created_on": "t2", "content": { "raw": "reply" }, "user": { "display_name": "B" }, "parent": { "id": 1 }, "inline": { "path": "src/a.ts", "to": 12 } }),
                json!({ "id": 3, "created_on": "t3", "content": { "raw": "reply to reply" }, "user": { "display_name": "A" }, "parent": { "id": 2 } }),
                json!({ "id": 4, "created_on": "t4", "content": { "raw": "gone" }, "deleted": true }),
                json!({ "id": 5, "created_on": "t5", "content": { "raw": "whole PR" }, "user": { "display_name": "C" } }),
                json!({ "id": 6, "created_on": "t6", "content": { "raw": "old line" }, "inline": { "path": "b.ts", "from": 3, "to": null } }),
            ],
            url,
        );
        assert_eq!(
            list.iter().map(|c| (c.id, c.reply_to, c.path.as_deref(), c.line, c.side.as_str())).collect::<Vec<_>>(),
            [
                (1, None, Some("src/a.ts"), Some(12), "additions"),
                (2, Some(1), Some("src/a.ts"), Some(12), "additions"),
                (3, Some(1), None, None, "additions"),
                (5, None, None, None, "additions"),
                (6, None, Some("b.ts"), Some(3), "deletions"),
            ]
        );
        assert_eq!(list.iter().map(|c| c.can_reply).collect::<Vec<_>>(), [Some(true), Some(false), Some(false), Some(true), Some(true)], "a thread takes a reply on its first comment");
        assert_eq!(list[3].author, "C");
        assert_eq!(list[3].url, url);
    }

    #[test]
    fn the_list_asks_for_reviewers_and_participants_which_the_api_leaves_out_by_default_in_normal_form() {
        assert_eq!(LIST_FIELDS, "fields=%2Bvalues.participants,%2Bvalues.reviewers");
        assert!(!LIST_FIELDS.contains('+'));
    }

    #[test]
    fn a_branch_lookup_keeps_only_prs_from_this_repository_not_a_forks_branch_of_the_same_name() {
        let prs = [
            bb(json!({ "id": 1, "source": { "branch": { "name": "main" }, "repository": { "full_name": "someone/r" } } })),
            bb(json!({ "id": 2, "source": { "branch": { "name": "main" }, "repository": { "full_name": "W/R" } } })),
            bb(json!({ "id": 3, "source": { "branch": { "name": "main" }, "repository": null } })),
        ];
        assert_eq!(bb_own(&prs, "w/r").iter().map(|p| p["id"].as_u64().unwrap()).collect::<Vec<_>>(), [2]);
    }
}
