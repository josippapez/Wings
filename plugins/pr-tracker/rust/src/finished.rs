//! Pull requests that left a repo's open list: which ones, and what a forge's status says became of them. The tracker only
//! lists open PRs, so a merge shows up as a disappearance and the final state is asked for once.

use crate::types::{Link, ListedPr, PrState, Provider, Status};
use serde::{Deserialize, Serialize};

/// An open PR as the last list had it, with what is needed to look it up and tell Issues about it.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tracked {
    pub link: Link,
    pub title: String,
    /// The source branch.
    pub head: String,
    /// The project whose list had it.
    pub project_id: String,
    pub cwd: String,
    /// Lookups of its final state that failed so far.
    pub tries: u32,
}

/// How a PR ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ended {
    Merged,
    Closed,
}

/// What `pr-state` carries: a tracked PR that merged or was closed.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrStateEvent {
    pub url: String,
    pub state: Ended,
    pub source_branch: String,
    pub repo: String,
    pub provider: Provider,
    pub number: u64,
    pub project_id: String,
}

/// A failed lookup is tried again on the next lists, this many times in all.
pub const MAX_TRIES: u32 = 3;

/// The PRs of `before` that aren't in the new list `items` (they merged, closed or were deleted), and what the next
/// check compares against: this list's PRs, in the list's order. `before` none is a repo listed for the first time, which
/// has none gone.
pub fn settle(before: Option<&[Tracked]>, items: &[ListedPr], project_id: &str, cwd: &str) -> (Vec<Tracked>, Vec<Tracked>) {
    let next: Vec<Tracked> = items.iter().map(|i| Tracked { link: i.link.clone(), title: i.title.clone(), head: i.head.clone(), project_id: project_id.to_owned(), cwd: cwd.to_owned(), tries: 0 }).collect();
    let gone = before.unwrap_or_default().iter().filter(|t| !next.iter().any(|n| n.link.url == t.link.url)).cloned().collect();
    (next, gone)
}

/// How a PR ended, or none for one that is still open.
pub fn final_state(status: &Status) -> Option<Ended> {
    match status.state {
        PrState::Open => None,
        PrState::Merged => Some(Ended::Merged),
        PrState::Closed => Some(Ended::Closed),
    }
}

pub fn event_for(t: &Tracked, state: Ended) -> PrStateEvent {
    PrStateEvent { url: t.link.url.clone(), state, source_branch: t.head.clone(), repo: t.link.repo.clone(), provider: t.link.provider, number: t.link.number, project_id: t.project_id.clone() }
}

/// Puts a PR whose lookup failed back into the next check's list, until it has been tried `MAX_TRIES` times.
pub fn retry(next: &mut Vec<Tracked>, t: &Tracked) {
    if t.tries + 1 < MAX_TRIES && !next.iter().any(|n| n.link.url == t.link.url) {
        next.push(Tracked { tries: t.tries + 1, ..t.clone() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Counts;

    fn item(n: u64) -> ListedPr {
        ListedPr {
            link: Link { provider: Provider::Github, url: format!("https://github.com/acme/web/pull/{n}"), number: n, repo: "acme/web".into(), remote: None, cwd: None, raw: None },
            title: format!("PR {n}"),
            head: format!("feature/{n}"),
            mine: false,
            reviewing: false,
            author: None,
            status: status(PrState::Open),
            loaded: true,
            activity: None,
            updated_at: None,
        }
    }

    fn status(state: PrState) -> Status {
        Status { key: String::new(), title: String::new(), label: String::new(), state, tone: String::new(), icon: String::new(), checks: Counts::default(), review: String::new(), failing: vec![], items: None, changes: None, branch: String::new(), updated: String::new(), merge: None, base: None, head: None, base_commit: None, head_commit: None }
    }

    #[test]
    fn a_repo_listed_for_the_first_time_has_nothing_gone() {
        let (next, gone) = settle(None, &[item(1)], "p1", "/repo");
        assert!(gone.is_empty());
        assert_eq!(next.iter().map(|t| t.link.url.as_str()).collect::<Vec<_>>(), ["https://github.com/acme/web/pull/1"]);
    }

    #[test]
    fn a_pr_missing_from_the_new_list_is_gone_with_its_source_branch() {
        let (next, _) = settle(None, &[item(1), item(2)], "p1", "/repo");
        let (_, gone) = settle(Some(&next), &[item(2)], "p1", "/repo");
        assert_eq!(gone.iter().map(|t| t.head.as_str()).collect::<Vec<_>>(), ["feature/1"]);
        assert_eq!(
            event_for(&gone[0], Ended::Merged),
            PrStateEvent { url: "https://github.com/acme/web/pull/1".into(), state: Ended::Merged, source_branch: "feature/1".into(), repo: "acme/web".into(), provider: Provider::Github, number: 1, project_id: "p1".into() }
        );
        assert_eq!(serde_json::to_value(event_for(&gone[0], Ended::Merged)).unwrap()["sourceBranch"], "feature/1");
    }

    #[test]
    fn final_state_reads_merged_and_closed_from_the_statuss_state_and_says_nothing_for_an_open_pr() {
        assert_eq!(final_state(&status(PrState::Merged)), Some(Ended::Merged));
        assert_eq!(final_state(&status(PrState::Closed)), Some(Ended::Closed));
        assert_eq!(final_state(&status(PrState::Open)), None);
    }

    #[test]
    fn a_failed_lookup_is_kept_for_the_next_check_three_tries_in_all() {
        let (next, _) = settle(None, &[item(1)], "p1", "/repo");
        let mut tracked = next[0].clone();
        for _ in 0..2 {
            let mut after = Vec::new();
            retry(&mut after, &tracked);
            assert_eq!(after.len(), 1);
            tracked = after.remove(0);
        }
        let mut last = Vec::new();
        retry(&mut last, &tracked);
        assert!(last.is_empty());
    }
}
