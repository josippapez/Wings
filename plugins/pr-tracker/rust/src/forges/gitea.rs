//! Gitea and Forgejo (Codeberg): pull requests through `/api/v1` with an access token, the diff from the API's `.diff`, replies and approvals.

use super::shared::{Facts, Words as Labels, counts, encode_query, label_of, open_or_newest, with_second_page};
use super::token::{Account, forge_get, forge_json, forge_post, forge_user, forget_users};
use crate::checklist::real;
use crate::forge::ForgeError;
use crate::types::*;
use serde_json::{Value, json};
use std::collections::HashMap;
use wings_plugin::{remote::Remote, task::join};

/// Gitea answers at most 50 items a page (`MAX_RESPONSE_ITEMS`).
const PAGE: usize = 50;

/// One token per host, saved under a name made of the host's letters: `codeberg` for codeberg.org.
fn account_of(host: &str) -> Account {
    Account {
        key: format!("gitea:{host}"),
        secret: host.split('.').next().unwrap_or_default().to_owned(),
        name: host.to_owned(),
        scope_hint: "The access token needs the read:repository, read:issue and read:user scopes.",
        write_hint: "The access token needs the write:issue scope to reply and write:repository to approve.",
    }
}

pub fn problem_key(remote: &Remote) -> String {
    match remote {
        Remote::Gitea { host, .. } => account_of(host).key,
        _ => String::new(),
    }
}

pub fn repo_key(remote: &Remote) -> String {
    match remote {
        Remote::Gitea { host, owner, repo } => format!("gitea:{host}/{owner}/{repo}"),
        _ => String::new(),
    }
}

// ---------- the API's shapes and how they map ----------

fn name_of(user: &Value) -> String {
    user["full_name"].as_str().filter(|n| !n.is_empty()).or(user["login"].as_str().filter(|n| !n.is_empty())).unwrap_or("unknown").to_owned()
}

fn state_of(status: &str) -> CheckState {
    match status {
        "success" | "warning" => CheckState::Passed,
        "skipped" => CheckState::Skipped,
        "pending" => CheckState::Running,
        _ => CheckState::Failed,
    }
}

fn check_id(s: &Value) -> String {
    s["context"].as_str().or(s["target_url"].as_str()).unwrap_or("check").to_owned()
}

/// A commit status as a check of the Checks view.
pub fn gt_item(s: &Value) -> CheckItem {
    let status = s["status"].as_str().unwrap_or_default();
    CheckItem {
        id: check_id(s),
        name: s["context"].as_str().unwrap_or("Check").to_owned(),
        state: state_of(status),
        required: None,
        kind: Some("Status check".into()),
        at: real(s["updated_at"].as_str()).or_else(|| real(s["created_at"].as_str())),
        seconds: None,
        url: s["target_url"].as_str().map(str::to_owned),
        note: s["description"].as_str().filter(|d| !d.is_empty()).map(str::to_owned).or_else(|| (status == "warning").then(|| "Warning".to_owned())),
        failing: None,
        run: None,
        needs: None,
    }
}

pub fn gt_status_of(pr: &Value, statuses: &[Value], reviews: &[Value]) -> Status {
    // `warning` doesn't block a merge, and `skipped` didn't run.
    let results: Vec<&str> = statuses
        .iter()
        .map(|s| match s["status"].as_str().unwrap_or_default() {
            "success" | "warning" | "skipped" => "passed",
            "pending" => "running",
            _ => "failed",
        })
        .collect();
    let failing = statuses
        .iter()
        .zip(&results)
        .filter(|(_, result)| **result == "failed")
        .map(|(s, _)| Failing { id: check_id(s), name: s["context"].as_str().map(str::to_owned), url: s["target_url"].as_str().map(str::to_owned), summary: s["description"].as_str().filter(|d| !d.is_empty()).map(str::to_owned), ..Failing::default() })
        .collect();
    let checks = counts(&results);
    // A reviewer's latest verdict counts, and a dismissed one doesn't.
    let mut sorted: Vec<&Value> = reviews.iter().collect();
    sorted.sort_by(|a, b| a["submitted_at"].as_str().unwrap_or_default().cmp(b["submitted_at"].as_str().unwrap_or_default()));
    let mut latest: HashMap<String, &str> = HashMap::new();
    for r in sorted {
        let state = r["state"].as_str().unwrap_or_default();
        if r["dismissed"] == true || !matches!(state, "APPROVED" | "REQUEST_CHANGES") {
            continue;
        }
        latest.insert(r["user"]["login"].as_str().map_or_else(|| r["id"].to_string(), str::to_owned), state);
    }
    let approved = latest.values().filter(|v| **v == "APPROVED").count();
    let changes = latest.values().any(|v| *v == "REQUEST_CHANGES");
    let (label, tone, icon, state) = label_of(
        &Facts { merged: pr["merged"] == true, closed: pr["state"] == "closed", draft: pr["draft"] == true, conflicts: pr["mergeable"] == false, checks, changes, approved: approved > 0, ..Facts::default() },
        &Labels::default(),
    );
    let (head, base) = (pr["head"]["ref"].as_str().unwrap_or_default(), pr["base"]["ref"].as_str().unwrap_or_default());
    Status {
        key: pr["head"]["sha"].as_str().unwrap_or_default().to_owned(),
        title: pr["title"].as_str().unwrap_or_default().to_owned(),
        label,
        tone,
        icon,
        state,
        checks,
        failing,
        items: Some(statuses.iter().map(gt_item).collect()),
        review: if changes { "Changes requested".into() } else if approved > 0 { format!("{approved} approved") } else { "No approvals yet".into() },
        changes: pr["changed_files"].as_i64().map(|files| format!("+{} −{} in {files} files", pr["additions"].as_i64().unwrap_or(0), pr["deletions"].as_i64().unwrap_or(0))),
        branch: format!("{head} → {base}"),
        updated: pr["updated_at"].as_str().unwrap_or_default().to_owned(),
        merge: None,
        base: None,
        head: None,
        base_commit: None,
        head_commit: None,
    }
}

/// The PRs whose branch is in this repository: a branch name alone also matches a fork's branch of the same name.
pub fn gt_own<'a>(prs: &'a [Value], full_name: &str) -> Vec<&'a Value> {
    prs.iter().filter(|p| p["head"]["repo"]["full_name"].as_str().is_some_and(|name| name.to_lowercase() == full_name.to_lowercase())).collect()
}

/// Who a listed PR is for: `(mine, reviewing)`, written by `me`, or with `me` among the reviewers asked.
fn gt_who(pr: &Value, me: &str) -> (bool, bool) {
    let lower = me.to_lowercase();
    let is_me = |user: &Value| user["login"].as_str().is_some_and(|login| login.to_lowercase() == lower);
    (is_me(&pr["user"]), pr["requested_reviewers"].as_array().is_some_and(|list| list.iter().any(is_me)))
}

pub fn gt_listed(prs: &[Value], me: &str, link: impl Fn(&Value) -> Link) -> Vec<ListedPr> {
    prs.iter()
        .map(|pr| {
            let (mine, reviewing) = gt_who(pr, me);
            let (label, tone, icon, _) = label_of(&Facts { draft: pr["draft"] == true, conflicts: pr["mergeable"] == false, ..Facts::default() }, &Labels::default());
            ListedPr {
                link: link(pr),
                title: pr["title"].as_str().unwrap_or_default().to_owned(),
                head: pr["head"]["ref"].as_str().unwrap_or_default().to_owned(),
                mine,
                reviewing,
                author: pr["user"].is_object().then(|| name_of(&pr["user"])),
                status: Status { label, tone, icon, ..gt_status_of(pr, &[], &[]) },
                loaded: false,
                activity: Some(pr["comments"].as_u64().unwrap_or(0) + pr["review_comments"].as_u64().unwrap_or(0)),
                updated_at: pr["updated_at"].as_str().map(str::to_owned),
            }
        })
        .collect()
}

/// The comments on the whole PR: its conversation and the reviews' summaries. Line comments live under each
/// review (`.../reviews/{id}/comments`) and aren't read, so they show on the forge's own page only. Reviews get
/// negative ids, which can't clash with the conversation's.
pub fn gt_comments(comments: &[Value], reviews: &[Value], url: &str) -> Vec<Comment> {
    let conversation = comments.iter().map(|c| (c["id"].as_i64().unwrap_or(0), c, c["created_at"].as_str()));
    let summaries = reviews.iter().filter(|r| r["body"].as_str().is_some_and(|b| !b.is_empty()) && r["state"] != "PENDING").map(|r| (-r["id"].as_i64().unwrap_or(0), r, r["submitted_at"].as_str()));
    conversation
        .chain(summaries)
        .map(|(id, c, at)| Comment {
            id,
            reply_to: None,
            path: None,
            line: None,
            side: "additions".into(),
            author: name_of(&c["user"]),
            by: c["user"]["login"].as_str().map(str::to_owned),
            body: c["body"].as_str().unwrap_or_default().to_owned(),
            created_at: at.unwrap_or_default().to_owned(),
            url: c["html_url"].as_str().unwrap_or(url).to_owned(),
            can_reply: Some(true),
            discussion: None,
            thread: None,
            range: None,
            snippet: None,
            refs: None,
            system: false,
        })
        .collect()
}

/// A reply to a Codeberg comment. The API has no threaded replies (a review comment can't take a parent, and the conversation
/// is flat), so a reply is a new comment on the pull request that quotes the first lines of what it answers.
pub fn gt_quoted(author: &str, body: &str, reply: &str) -> String {
    let all: Vec<&str> = body.trim().split('\n').collect();
    let mut quoted: Vec<String> = all.iter().take(6).map(|l| format!("> {l}").trim_end().to_owned()).collect();
    if all.len() > 6 {
        quoted.push("> …".into());
    }
    format!("> **{author}** wrote:\n{}\n\n{reply}", quoted.join("\n"))
}

// ---------- calling it ----------

fn gitea(link: &Link) -> Result<(&str, &str, &str), ForgeError> {
    match &link.remote {
        Some(Remote::Gitea { host, owner, repo }) => Ok((host, owner, repo)),
        _ => Err("This pull request doesn't say which Codeberg repository it is in.".into()),
    }
}

fn api(host: &str, owner: &str, repo: &str) -> String {
    format!("https://{host}/api/v1/repos/{owner}/{repo}")
}

fn pr_url(link: &Link) -> Result<(Account, String, String), ForgeError> {
    let (host, owner, repo) = gitea(link)?;
    let api = api(host, owner, repo);
    Ok((account_of(host), format!("{api}/pulls/{}", link.number), api))
}

fn to_link(pr: &Value, host: &str, owner: &str, repo: &str) -> Link {
    Link {
        provider: Provider::Gitea,
        url: pr["html_url"].as_str().unwrap_or_default().to_owned(),
        number: pr["number"].as_u64().unwrap_or(0),
        repo: format!("{owner}/{repo}"),
        remote: Some(Remote::Gitea { host: host.to_owned(), owner: owner.to_owned(), repo: repo.to_owned() }),
        cwd: None,
        raw: None,
    }
}

async fn load(link: &Link) -> Result<(Value, Vec<Value>, Vec<Value>), ForgeError> {
    let (account, pr_url, api) = pr_url(link)?;
    let (host, ..) = gitea(link)?;
    let pr = forge_json(&account, &pr_url).await?;
    let sha = pr["head"]["sha"].as_str().unwrap_or_default();
    let (combined, reviews) = join(forge_json(&account, &format!("{api}/commits/{sha}/status?limit=50")), forge_json(&account, &format!("{pr_url}/reviews"))).await;
    // Forgejo's actions give the check's page as a path on the host.
    let statuses = combined.ok().and_then(|c| c["statuses"].as_array().cloned()).unwrap_or_default().into_iter().map(|mut c| {
        if let Some(path) = c["target_url"].as_str().filter(|url| url.starts_with('/')) {
            c["target_url"] = format!("https://{host}{path}").into();
        }
        c
    });
    Ok((pr, statuses.collect(), reviews.ok().and_then(|r| r.as_array().cloned()).unwrap_or_default()))
}

pub async fn comments(link: &Link) -> Result<Vec<Comment>, ForgeError> {
    let (account, pr_url, api) = pr_url(link)?;
    let (conversation, reviews) = join(forge_json(&account, &format!("{api}/issues/{}/comments", link.number)), forge_json(&account, &format!("{pr_url}/reviews"))).await;
    // The conversation is the issue's comments, which need their own scope.
    let conversation = conversation.map_err(|error| if error.to_string().contains("(403)") { ForgeError::from("Comments need the read:issue scope on the token.") } else { error })?;
    let as_list = |value: Value| value.as_array().cloned().unwrap_or_default();
    Ok(gt_comments(&as_list(conversation), &as_list(reviews?), &link.url))
}

pub async fn find(_cwd: &str, remote: &Remote, branch: &str) -> Result<Option<Link>, ForgeError> {
    let Remote::Gitea { host, owner, repo } = remote else { return Ok(None) };
    let url = format!("{}/pulls?state=all&sort=recentupdate&limit=20&head={}", api(host, owner, repo), encode_query(branch));
    let found = forge_json(&account_of(host), &url).await?.as_array().cloned().unwrap_or_default();
    let own: Vec<Value> = gt_own(&found, &format!("{owner}/{repo}")).into_iter().cloned().collect();
    Ok(open_or_newest(&own, |p| p["state"] == "open").map(|pr| to_link(pr, host, owner, repo)))
}

async fn login(host: &str) -> Result<String, ForgeError> {
    forge_user(&account_of(host), &format!("https://{host}/api/v1/user"), |u| u["login"].as_str().map(str::to_owned)).await
}

/// The login the token belongs to, which is also what a mention is written with.
pub async fn me(remote: &Remote) -> Result<Me, ForgeError> {
    let Remote::Gitea { host, .. } = remote else { return Ok(Me::default()) };
    let login = login(host).await?;
    Ok(Me { ids: vec![login.clone()], handles: vec![login] })
}

/// One call for the repo's open PRs, kept when you wrote them or were asked to review them.
pub async fn list(_cwd: &str, remote: &Remote) -> Result<Vec<ListedPr>, ForgeError> {
    let Remote::Gitea { host, owner, repo } = remote else { return Ok(Vec::new()) };
    let url = format!("{}/pulls?state=open&sort=recentupdate&limit={PAGE}", api(host, owner, repo));
    let page = |url: String| {
        let account = account_of(host);
        async move { Ok::<_, ForgeError>(forge_json(&account, &url).await?.as_array().cloned().unwrap_or_default()) }
    };
    let (user, first) = join(login(host), page(url.clone())).await;
    let prs = with_second_page(first?, PAGE, || page(format!("{url}&page=2"))).await?;
    Ok(gt_listed(&prs, &user?, |pr| to_link(pr, host, owner, repo)))
}

pub async fn link_of(args: &PrArgs) -> Result<Link, ForgeError> {
    let Some(remote @ Remote::Gitea { .. }) = &args.remote else {
        return Err("This tab doesn't say which Codeberg repository its pull request is in.".into());
    };
    Ok(Link { provider: Provider::Gitea, url: args.url.clone(), number: args.number, repo: args.repo.clone(), remote: Some(remote.clone()), cwd: None, raw: None })
}

pub async fn status(link: &Link, _fresh: bool) -> Result<Status, ForgeError> {
    let (pr, statuses, reviews) = load(link).await?;
    Ok(gt_status_of(&pr, &statuses, &reviews))
}

pub async fn overview(link: &Link) -> Result<Overview, ForgeError> {
    let (pr, statuses, reviews) = load(link).await?;
    Ok(Overview { status: gt_status_of(&pr, &statuses, &reviews), author: name_of(&pr["user"]), body: pr["body"].as_str().unwrap_or_default().to_owned(), created_at: None, reviewers: None })
}

pub async fn diff(link: &Link, _st: &Status) -> Result<(String, Vec<Comment>), ForgeError> {
    let (account, pr_url, _) = pr_url(link)?;
    let (patch, conversation) = join(forge_get(&account, &format!("{pr_url}.diff"), "text/plain"), comments(link)).await;
    Ok((patch?, conversation?))
}

pub use super::shared::plain_failure as failure;

/// Quotes the comment and posts it as a new comment on the pull request: the API has no reply to a comment.
pub async fn reply(link: &Link, root: &Comment, body: &str) -> Result<Comment, ForgeError> {
    let (account, _, api) = pr_url(link)?;
    let fields = json!({ "body": gt_quoted(&root.author, &root.body, body) });
    let created: Value = serde_json::from_str(&forge_post(&account, &format!("{api}/issues/{}/comments", link.number), Some(&fields)).await?).map_err(|error| ForgeError::Other(error.to_string()))?;
    let made = gt_comments(&[created], &[], &link.url).remove(0);
    Ok(Comment { reply_to: Some(root.id), can_reply: Some(false), ..made })
}

/// Approves as the token's account; Gitea refuses it for your own pull request.
pub async fn approve(link: &Link, _sha: &str) -> Result<(), ForgeError> {
    let (account, pr_url, _) = pr_url(link)?;
    forge_post(&account, &format!("{pr_url}/reviews"), Some(&json!({ "event": "APPROVED" }))).await.map(drop)
}

pub fn forget() {
    forget_users();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gt(over: Value) -> Value {
        let mut base = json!({
            "number": 9, "title": "Fix y", "state": "open", "draft": false, "merged": false, "mergeable": true,
            "comments": 2, "review_comments": 1, "additions": 10, "deletions": 4, "changed_files": 3,
            "user": { "login": "Me" }, "requested_reviewers": [],
            "head": { "ref": "fix", "sha": "deadbeef", "repo": { "full_name": "o/r" } }, "base": { "ref": "main" },
            "updated_at": "2026-10-01T10:00:00+02:00", "html_url": "https://codeberg.org/o/r/pulls/9",
        });
        for (key, value) in over.as_object().unwrap() {
            base[key] = value.clone();
        }
        base
    }

    fn label(over: Value) -> String {
        gt_status_of(&gt(over), &[], &[]).label
    }

    fn link_of_pr(pr: &Value) -> Link {
        to_link(pr, "codeberg.org", "o", "r")
    }

    #[test]
    fn gitea_status_merged_closed_draft_conflicts_checks_and_reviews_decide_the_label() {
        assert_eq!(label(json!({ "state": "closed", "merged": true })), "Merged");
        assert_eq!(label(json!({ "state": "closed" })), "Closed");
        assert_eq!(label(json!({ "draft": true })), "Draft");
        assert_eq!(label(json!({ "mergeable": false })), "Conflicts");
        let checks = gt_status_of(
            &gt(json!({})),
            &[json!({ "status": "failure", "context": "ci / build", "target_url": "/o/r/actions/runs/1", "description": "Failing" }), json!({ "status": "success", "context": "lint" }), json!({ "status": "skipped" }), json!({ "status": "pending", "context": "e2e" }), json!({ "status": "warning" })],
            &[],
        );
        assert_eq!(checks.checks, Counts { passed: 3, failed: 1, running: 1 });
        assert_eq!(checks.label, "Checks failing");
        assert_eq!(checks.failing.iter().map(|f| (f.id.as_str(), f.name.as_deref(), f.summary.as_deref())).collect::<Vec<_>>(), [("ci / build", Some("ci / build"), Some("Failing"))]);
        let reviews = vec![
            json!({ "id": 1, "user": { "login": "a" }, "state": "REQUEST_CHANGES", "submitted_at": "2026-10-01T09:00:00Z" }),
            json!({ "id": 2, "user": { "login": "a" }, "state": "APPROVED", "submitted_at": "2026-10-01T10:00:00Z" }),
            json!({ "id": 3, "user": { "login": "b" }, "state": "REQUEST_CHANGES", "submitted_at": "2026-10-01T10:00:00Z", "dismissed": true }),
            json!({ "id": 4, "user": { "login": "c" }, "state": "REQUEST_REVIEW", "submitted_at": "2026-10-01T10:00:00Z" }),
        ];
        let reviewed = gt_status_of(&gt(json!({})), &[], &reviews);
        assert_eq!(reviewed.label, "Approved");
        assert_eq!(reviewed.review, "1 approved");
        assert_eq!(gt_status_of(&gt(json!({})), &[], &reviews[..1]).label, "Changes requested");
        let st = gt_status_of(&gt(json!({})), &[], &[]);
        assert_eq!((st.key.as_str(), st.branch.as_str(), st.changes.as_deref(), st.label.as_str()), ("deadbeef", "fix → main", Some("+10 −4 in 3 files"), "In review"));
    }

    #[test]
    fn gitea_statuses_are_checks() {
        let states: Vec<CheckState> = ["success", "warning", "skipped", "pending", "error", "failure"].iter().map(|status| gt_item(&json!({ "status": status, "context": "c" })).state).collect();
        assert_eq!(states, [CheckState::Passed, CheckState::Passed, CheckState::Skipped, CheckState::Running, CheckState::Failed, CheckState::Failed]);
    }

    #[test]
    fn gitea_list_keeps_every_open_pr_and_marks_the_ones_you_wrote_or_were_asked_to_review_matching_the_login_without_case() {
        let prs = [
            gt(json!({ "number": 1, "user": { "login": "other" } })),
            gt(json!({ "number": 2, "user": { "login": "other" }, "requested_reviewers": [{ "login": "ME" }] })),
            gt(json!({ "number": 3 })),
            gt(json!({ "number": 4, "requested_reviewers": null, "user": { "login": "x" } })),
        ];
        let rows = gt_listed(&prs, "me", link_of_pr);
        assert_eq!(rows.iter().map(|r| (r.link.number, r.mine, r.reviewing)).collect::<Vec<_>>(), [(1, false, false), (2, false, true), (3, true, false), (4, false, false)]);
        assert_eq!(rows[2].activity, Some(3));
        assert_eq!(rows[2].head, "fix");
        assert_eq!(gt_listed(&[gt(json!({ "draft": true }))], "me", link_of_pr)[0].status.label, "Draft");
    }

    #[test]
    fn gitea_comments_the_conversation_and_each_reviews_summary_with_negative_ids_for_reviews() {
        let list = gt_comments(
            &[json!({ "id": 10, "user": { "login": "a", "full_name": "Ann" }, "body": "hi", "created_at": "t1" })],
            &[
                json!({ "id": 5, "user": { "login": "b" }, "state": "APPROVED", "body": "LGTM", "submitted_at": "t2", "html_url": "https://x/r5" }),
                json!({ "id": 6, "user": { "login": "c" }, "state": "COMMENT", "body": "", "submitted_at": "t3" }),
                json!({ "id": 7, "user": { "login": "d" }, "state": "PENDING", "body": "draft", "submitted_at": "t4" }),
            ],
            "https://codeberg.org/o/r/pulls/9",
        );
        assert_eq!(list.iter().map(|c| (c.id, c.author.as_str(), c.body.as_str(), c.url.as_str())).collect::<Vec<_>>(), [(10, "Ann", "hi", "https://codeberg.org/o/r/pulls/9"), (-5, "b", "LGTM", "https://x/r5")]);
    }

    #[test]
    fn a_branch_lookup_keeps_only_prs_from_this_repository_not_a_forks_branch_of_the_same_name() {
        let gts = [
            gt(json!({ "number": 1, "head": { "ref": "forgejo", "sha": "a", "repo": { "full_name": "fork/r" } } })),
            gt(json!({ "number": 2, "head": { "ref": "forgejo", "sha": "b", "repo": { "full_name": "O/R" } } })),
            gt(json!({ "number": 3, "head": { "ref": "forgejo", "sha": "c", "repo": null } })),
        ];
        assert_eq!(gt_own(&gts, "o/r").iter().map(|p| p["number"].as_u64().unwrap()).collect::<Vec<_>>(), [2]);
    }

    #[test]
    fn a_codeberg_reply_quotes_the_first_lines_of_the_comment_it_answers() {
        assert_eq!(gt_quoted("Ana", "Why 2?", "Because."), "> **Ana** wrote:\n> Why 2?\n\nBecause.");
        let long = (1..=9).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let quoted = gt_quoted("Ana", &format!("{long}\n"), "ok");
        assert_eq!(quoted.split('\n').filter(|l| l.starts_with("> line")).count(), 6);
        assert!(quoted.ends_with("> …\n\nok"));
        assert_eq!(gt_quoted("Ana", "a\n\nb", "ok"), "> **Ana** wrote:\n> a\n>\n> b\n\nok", "a blank line stays inside the quote");
    }

    #[test]
    fn codeberg_conversation_comments_take_a_reply() {
        let list = gt_comments(&[json!({ "id": 5, "user": { "login": "ana" }, "body": "hi", "created_at": "t", "html_url": "https://codeberg.org/o/r/pulls/1#issuecomment-5" })], &[], "https://codeberg.org/o/r/pulls/1");
        assert_eq!(list[0].can_reply, Some(true));
    }
}
