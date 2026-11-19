//! The pull requests and issues a conversation names, for the chat's side panel: Wings issue keys (`WIN-12`) in what you
//! and Claude wrote, and the addresses of GitHub and Azure DevOps pull requests anywhere in it, a command's output too,
//! as `gh pr create` prints one. Each comes once, in the order the conversation first named it. What the Issues plugin,
//! `gh` and `az` say of each is read once and kept (`Detail`).

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::session::Item;

#[derive(Clone, Debug, PartialEq)]
pub enum Ref {
    /// An issue key, which only names an issue once the Issues plugin knows it.
    Issue(String),
    GitHub { url: String, repo: String, number: u64 },
    Azure { url: String, org: String, repo: String, number: u64 },
}

impl Ref {
    /// What it is known by: the key, or the address.
    pub fn key(&self) -> &str {
        match self {
            Ref::Issue(key) => key,
            Ref::GitHub { url, .. } | Ref::Azure { url, .. } => url,
        }
    }

    /// How a row names it: `WIN-12`, `cli/cli#123`, `shop !45`.
    pub fn label(&self) -> String {
        match self {
            Ref::Issue(key) => key.clone(),
            Ref::GitHub { repo, number, .. } => format!("{repo}#{number}"),
            Ref::Azure { repo, number, .. } => format!("{repo} !{number}"),
        }
    }
}

/// An issue as the Issues plugin's `get` describes it.
#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
    pub title: String,
    /// As the plugin writes it: `todo`, `in_progress`, `in_review`, `done`...
    pub status: String,
    /// Its type, with its priority when it has one: `bug`, `feature, high`.
    pub kind: String,
    pub priority: Option<String>,
    /// The parent's key and title.
    pub parent: Option<String>,
}

/// A pull request as `gh` or `az` describes it.
#[derive(Clone, Debug, PartialEq)]
pub struct Pr {
    pub title: String,
    /// Open, Draft, Merged or Closed.
    pub state: &'static str,
    pub author: String,
    /// `head → base`.
    pub branch: String,
    /// Checks passed, failed and still running, where the forge says.
    pub checks: Option<(usize, usize, usize)>,
    /// Lines added and removed, where the forge says.
    pub lines: Option<(u64, u64)>,
    /// What the reviewers decided, in a few words.
    pub review: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Detail {
    Loading,
    /// Read and found nothing: an issue key the Issues plugin doesn't know, which isn't listed, or a pull request `gh` or
    /// `az` couldn't read, which is listed without a card.
    Missing,
    Issue(Issue),
    Pr(Pr),
}

/// What a conversation named, as far as it was read, and what is known of each.
#[derive(Default)]
pub struct Refs {
    pub list: Vec<Ref>,
    pub details: HashMap<String, Detail>,
    seen: HashSet<String>,
    /// The first item not read for good: one that may still change, as a tool call before its output.
    read: usize,
    /// How many items there were, so a conversation that got shorter, rewound or cleared, is read again.
    len: usize,
}

impl Refs {
    /// Reads the items not read yet and says what they name that wasn't named before, to be looked up.
    pub fn scan(&mut self, items: &[Item]) -> Vec<Ref> {
        if items.len() < self.len {
            *self = Refs { details: std::mem::take(&mut self.details), ..Refs::default() };
        }
        self.len = items.len();
        let mut new = Vec::new();
        let mut settled = true;
        for (at, item) in items.iter().enumerate().skip(self.read) {
            let (said, issues) = match item {
                Item::User(said) | Item::UserImages(said, _) | Item::Text(said) => (said.as_str(), true),
                Item::Tool(tool) => match &tool.output {
                    Some((output, _)) => (output.as_str(), false),
                    None => {
                        settled = false;
                        continue;
                    }
                },
                _ => continue,
            };
            if settled {
                self.read = at + 1;
            }
            for found in found_in(said, issues) {
                if self.seen.insert(found.key().to_owned()) {
                    self.list.push(found.clone());
                    new.push(found);
                }
            }
        }
        new
    }

    /// What the panel lists, newest first: pull requests, and the issues the Issues plugin knows.
    pub fn shown(&self) -> Vec<(&Ref, Option<&Detail>)> {
        self.list
            .iter()
            .rev()
            .map(|found| (found, self.details.get(found.key())))
            .filter(|(found, detail)| !matches!(found, Ref::Issue(_)) || matches!(detail, Some(Detail::Issue(_))))
            .collect()
    }
}

/// The pull requests `text` names, and with `issues` the issue keys, in order.
pub fn found_in(text: &str, issues: bool) -> Vec<Ref> {
    let mut found = Vec::new();
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if text[at..].starts_with("https://") {
            let end = text[at..].find(|c: char| c.is_whitespace() || "<>\"'`()[]{},".contains(c)).map_or(text.len(), |len| at + len);
            found.extend(pull_request(text[at..end].trim_end_matches(['.', ':', ';', '!', '?'])));
            at = end;
            continue;
        }
        if issues && bytes[at].is_ascii_uppercase() && (at == 0 || !bytes[at - 1].is_ascii_alphanumeric()) {
            let letters = bytes[at..].iter().take_while(|b| b.is_ascii_uppercase()).count();
            let digits = bytes.get(at + letters + 1..).map_or(0, |rest| rest.iter().take_while(|b| b.is_ascii_digit()).count());
            let end = at + letters + 1 + digits;
            let ends_word = bytes.get(end).is_none_or(|b| !b.is_ascii_alphanumeric());
            if (1..=3).contains(&letters) && bytes.get(at + letters) == Some(&b'-') && (1..=9).contains(&digits) && ends_word {
                found.push(Ref::Issue(text[at..end].to_owned()));
                at = end;
                continue;
            }
            at += letters;
            continue;
        }
        at += text[at..].chars().next().map_or(1, char::len_utf8);
    }
    found
}

/// The pull request an address names: `https://github.com/<owner>/<repo>/pull/<n>` or
/// `https://dev.azure.com/<org>/<project>/_git/<repo>/pullrequest/<n>`, with nothing after the number but a fragment or
/// a further page of it (`/files`).
fn pull_request(url: &str) -> Option<Ref> {
    let url = url.split(['#', '?']).next()?;
    let number = |text: &str| text.parse::<u64>().ok().filter(|n| *n > 0);
    if let Some(path) = url.strip_prefix("https://github.com/") {
        let parts: Vec<&str> = path.split('/').collect();
        if let [owner, repo, "pull", n, ..] = parts[..]
            && !owner.is_empty()
            && !repo.is_empty()
            && let Some(number) = number(n)
        {
            return Some(Ref::GitHub { url: format!("https://github.com/{owner}/{repo}/pull/{number}"), repo: format!("{owner}/{repo}"), number });
        }
    }
    let path = url.strip_prefix("https://dev.azure.com/")?;
    let parts: Vec<&str> = path.split('/').collect();
    if let [org, project, "_git", repo, "pullrequest", n, ..] = parts[..]
        && !org.is_empty()
        && !project.is_empty()
        && !repo.is_empty()
        && let Some(number) = number(n)
    {
        let url = format!("https://dev.azure.com/{org}/{project}/_git/{repo}/pullrequest/{number}");
        return Some(Ref::Azure { url, org: org.to_owned(), repo: repo.to_owned(), number });
    }
    None
}

/// The issue in what the Issues plugin's `get` answers: `WIN-12 [in_progress] (bug) Fix the build`, then lines such as
/// `Priority: High` and `Parent: WIN-3 [todo] (epic) Checkout`.
pub fn issue_of(text: &str) -> Option<Issue> {
    let mut lines = text.lines();
    let first = lines.next()?;
    let (_, rest) = first.split_once(" [")?;
    let (status, rest) = rest.split_once("] (")?;
    let (kind, title) = rest.split_once(") ")?;
    let mut issue = Issue { title: title.trim().to_owned(), status: status.to_owned(), kind: kind.to_owned(), priority: None, parent: None };
    for line in lines {
        if let Some(priority) = line.strip_prefix("Priority: ").filter(|priority| *priority != "No priority") {
            issue.priority = Some(priority.to_owned());
        }
        if let Some(parent) = line.strip_prefix("Parent: ") {
            let key = parent.split(' ').next().unwrap_or_default();
            let title = parent.split_once(") ").map_or("", |(_, title)| title);
            issue.parent = Some(format!("{key} {title}").trim().to_owned());
        }
    }
    Some(issue)
}

/// The pull request `gh pr view --json` describes, with the fields `GH_FIELDS` asks for.
pub fn github_pr(pr: &Value) -> Option<Pr> {
    let title = pr["title"].as_str()?.to_owned();
    let state = match (pr["state"].as_str()?, pr["isDraft"] == true) {
        ("MERGED", _) => "Merged",
        ("CLOSED", _) => "Closed",
        (_, true) => "Draft",
        _ => "Open",
    };
    let checks = pr["statusCheckRollup"].as_array().filter(|checks| !checks.is_empty()).map(|checks| {
        let mut counts = (0, 0, 0);
        for check in checks {
            // A check run has a status and, once completed, a conclusion; a commit status has a state.
            let outcome = check["conclusion"].as_str().filter(|_| check["status"] == "COMPLETED").or(check["state"].as_str()).unwrap_or("PENDING");
            match outcome {
                "SUCCESS" | "NEUTRAL" | "SKIPPED" => counts.0 += 1,
                "FAILURE" | "ERROR" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED" | "STARTUP_FAILURE" => counts.1 += 1,
                _ => counts.2 += 1,
            }
        }
        counts
    });
    let review = match pr["reviewDecision"].as_str() {
        Some("APPROVED") => Some("Approved".to_owned()),
        Some("CHANGES_REQUESTED") => Some("Changes requested".to_owned()),
        Some("REVIEW_REQUIRED") => Some("Review required".to_owned()),
        _ => None,
    };
    Some(Pr {
        title,
        state,
        author: pr["author"]["login"].as_str().unwrap_or_default().to_owned(),
        branch: format!("{} → {}", pr["headRefName"].as_str().unwrap_or_default(), pr["baseRefName"].as_str().unwrap_or_default()),
        checks,
        lines: pr["additions"].as_u64().zip(pr["deletions"].as_u64()),
        review,
    })
}

/// What `gh pr view` is asked for.
pub const GH_FIELDS: &str = "title,state,isDraft,author,headRefName,baseRefName,reviewDecision,additions,deletions,statusCheckRollup";

/// The pull request `az repos pr show` describes. Reviewer votes: 10 approved, 5 approved with suggestions, -5 waiting
/// for the author, -10 rejected.
pub fn azure_pr(pr: &Value) -> Option<Pr> {
    let title = pr["title"].as_str()?.to_owned();
    let state = match (pr["status"].as_str()?, pr["isDraft"] == true) {
        ("completed", _) => "Merged",
        ("abandoned", _) => "Closed",
        (_, true) => "Draft",
        _ => "Open",
    };
    let short = |name: &str| pr[name].as_str().unwrap_or_default().trim_start_matches("refs/heads/").to_owned();
    let votes: Vec<i64> = pr["reviewers"].as_array().map_or_else(Vec::new, |reviewers| reviewers.iter().filter_map(|r| r["vote"].as_i64()).collect());
    let review = if votes.contains(&-10) {
        Some("Rejected")
    } else if votes.contains(&-5) {
        Some("Waiting for the author")
    } else if votes.iter().any(|vote| *vote > 0) {
        Some("Approved")
    } else {
        None
    };
    Some(Pr {
        title,
        state,
        author: pr["createdBy"]["displayName"].as_str().unwrap_or_default().to_owned(),
        branch: format!("{} → {}", short("sourceRefName"), short("targetRefName")),
        checks: None,
        lines: None,
        review: review.map(str::to_owned),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn issue_keys_and_pull_request_addresses_are_found_in_order_and_nothing_else_is() {
        let said = "Fixed WIN-12 and (WIN-7). See https://github.com/cli/cli/pull/15094/files, and \
            https://dev.azure.com/Org/Shop/_git/web/pullrequest/6762. Not UTF8, ABCD-1, A-1234567890, WIN-12a or xWIN-3.";
        let found = found_in(said, true);
        assert_eq!(found.iter().map(Ref::label).collect::<Vec<_>>(), ["WIN-12", "WIN-7", "cli/cli#15094", "web !6762"]);
        assert_eq!(found[2].key(), "https://github.com/cli/cli/pull/15094");
        // A command's output names pull requests, but its capitals are no issue keys.
        assert_eq!(found_in("SHA-256 ok\nhttps://github.com/o/r/pull/9\n", false).iter().map(Ref::label).collect::<Vec<_>>(), ["o/r#9"]);
        assert!(found_in("https://github.com/o/r/issues/9 https://github.com/o/r/pull/x", true).is_empty());
    }

    #[test]
    fn each_is_named_once_newest_first_and_an_unknown_issue_key_isnt_listed() {
        let tool = |output: Option<&str>| {
            Item::Tool(crate::session::Tool {
                id: "t".into(),
                name: "Bash".into(),
                summary: String::new(),
                output: output.map(|output| (output.to_owned(), false)),
                detail: crate::session::Detail::Plain,
                images: Vec::new(),
            })
        };
        let mut items = vec![Item::User("Look at WIN-1 and UTF-8".into()), tool(None)];
        let mut refs = Refs::default();
        assert_eq!(refs.scan(&items).iter().map(Ref::label).collect::<Vec<_>>(), ["WIN-1", "UTF-8"]);
        // The call's output comes later, and is read then.
        items[1] = tool(Some("https://github.com/o/r/pull/3"));
        items.push(Item::Text("WIN-1 again".into()));
        assert_eq!(refs.scan(&items).iter().map(Ref::label).collect::<Vec<_>>(), ["o/r#3"]);
        refs.details.insert("WIN-1".into(), Detail::Issue(issue_of("WIN-1 [todo] (bug) Fix it").unwrap()));
        refs.details.insert("UTF-8".into(), Detail::Missing);
        assert_eq!(refs.shown().iter().map(|(found, _)| found.label()).collect::<Vec<_>>(), ["o/r#3", "WIN-1"]);
        // A conversation that got shorter is read again.
        assert_eq!(refs.scan(&items[..1]).iter().map(Ref::label).collect::<Vec<_>>(), ["WIN-1", "UTF-8"]);
    }

    #[test]
    fn an_issue_reads_from_what_the_issues_plugin_says_of_it() {
        let said = "WIN-231 [in_review] (bug) Load a long session\nPriority: High\ncreated 2026-11-19T11:01Z\n\nParent: WIN-220 [in_progress] (epic) Agent UI: a chat view\nRelated:";
        let issue = issue_of(said).unwrap();
        assert_eq!((issue.title.as_str(), issue.status.as_str(), issue.kind.as_str()), ("Load a long session", "in_review", "bug"));
        assert_eq!((issue.priority.as_deref(), issue.parent.as_deref()), (Some("High"), Some("WIN-220 Agent UI: a chat view")));
        assert_eq!(issue_of("No issue WIN-9 in wings. Use list to see the keys."), None);
    }

    #[test]
    fn a_pull_request_reads_from_what_gh_and_az_print() {
        let gh = json!({ "title": "docs: note", "state": "OPEN", "isDraft": false, "author": { "login": "babakks" }, "headRefName": "fix", "baseRefName": "trunk",
            "reviewDecision": "APPROVED", "additions": 4, "deletions": 1, "statusCheckRollup": [
                { "__typename": "CheckRun", "status": "COMPLETED", "conclusion": "SUCCESS" },
                { "__typename": "CheckRun", "status": "COMPLETED", "conclusion": "FAILURE" },
                { "__typename": "CheckRun", "status": "IN_PROGRESS", "conclusion": "" },
                { "__typename": "StatusContext", "state": "SUCCESS" }] });
        let pr = github_pr(&gh).unwrap();
        assert_eq!((pr.state, pr.branch.as_str(), pr.checks, pr.lines, pr.review.as_deref()), ("Open", "fix → trunk", Some((2, 1, 1)), Some((4, 1)), Some("Approved")));
        let az = json!({ "title": "Checkout", "status": "active", "isDraft": true, "createdBy": { "displayName": "Ana" },
            "sourceRefName": "refs/heads/feature/pay", "targetRefName": "refs/heads/main", "reviewers": [{ "vote": -5 }, { "vote": 10 }] });
        let pr = azure_pr(&az).unwrap();
        assert_eq!((pr.state, pr.author.as_str(), pr.branch.as_str(), pr.review.as_deref()), ("Draft", "Ana", "feature/pay → main", Some("Waiting for the author")));
        assert_eq!(azure_pr(&json!({ "title": "x", "status": "completed" })).unwrap().state, "Merged");
    }
}
