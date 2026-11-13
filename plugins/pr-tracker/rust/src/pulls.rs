//! The Pull requests block on a Project page: the order and the filters of its rows.

use crate::types::{Counts, PrArgs};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The most rows a list asks for, which is what a provider's one call returns.
pub const BLOCK_ROWS: usize = 100;

/// Whose a pull request is: you wrote it, your review is asked for, or neither.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Mine,
    Review,
    Other,
}

/// What a list shows: every open PR, yours, the ones asked of you, or drafts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Filter {
    All,
    Mine,
    Review,
    Drafts,
}

impl Filter {
    pub fn name(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Mine => "mine",
            Self::Review => "review",
            Self::Drafts => "drafts",
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct RowBadge {
    pub text: String,
    pub tone: String,
}

/// A Claude session in one of your panes is on this PR.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    Working,
    Here,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct PullRow {
    pub id: String,
    pub title: String,
    /// `#12`, or `!12` on GitLab.
    pub number: String,
    pub author: Option<String>,
    /// `feature/x → main`.
    pub branch: String,
    pub scope: Scope,
    pub draft: bool,
    pub badge: RowBadge,
    /// The reviewers' verdict in words, "2 approved", or "" where the list doesn't say.
    pub review: String,
    /// Check counts, all zero where the list can't see them.
    pub checks: Counts,
    /// ISO time.
    pub updated: String,
    /// Comments since you last looked.
    pub unread: u32,
    pub agent: Option<Agent>,
    /// What the "pr" view is opened with.
    pub args: PrArgs,
}

/// Yours first, then the ones waiting for your review, then the rest, each group in the order the list gave.
pub fn order_pulls(rows: &[PullRow]) -> Vec<PullRow> {
    [Scope::Mine, Scope::Review, Scope::Other].into_iter().flat_map(|scope| rows.iter().filter(move |r| r.scope == scope).cloned()).collect()
}

pub fn filter_pulls(rows: &[PullRow], filter: Filter) -> Vec<PullRow> {
    rows.iter()
        .filter(|r| match filter {
            Filter::All => true,
            Filter::Drafts => r.draft,
            Filter::Mine => r.scope == Scope::Mine,
            Filter::Review => r.scope == Scope::Review,
        })
        .cloned()
        .collect()
}

/// How many rows each filter shows, for its chip: `[all, mine, review, drafts]`.
pub fn count_pulls(rows: &[PullRow]) -> [usize; 4] {
    [Filter::All, Filter::Mine, Filter::Review, Filter::Drafts].map(|f| filter_pulls(rows, f).len())
}

/// A saved filter (`{ filter }`), or "all" when what was saved isn't one.
pub fn filter_of(saved: &Value) -> Filter {
    match saved.get("filter").and_then(Value::as_str) {
        Some("mine") => Filter::Mine,
        Some("review") => Filter::Review,
        Some("drafts") => Filter::Drafts,
        _ => Filter::All,
    }
}

/// The leading `[tag]` of `text` and what follows it, when the tag is 1 to 40 characters.
fn leading_tag(text: &str) -> Option<(&str, &str)> {
    let inner = text.strip_prefix('[')?;
    let end = inner.find(']')?;
    let tag = &inner[..end];
    (1..=40).contains(&tag.chars().count()).then(|| (tag, inner[end + 1..].trim_start()))
}

/// A title's leading `[tags]` (a ticket, an area) apart from its text, so the page can show them small and muted.
/// Only a title that has text after the tags is split.
pub fn split_title(title: &str) -> (Vec<String>, String) {
    let mut tags = Vec::new();
    let mut rest = title.trim();
    while let Some((tag, after)) = leading_tag(rest) {
        tags.push(tag.trim().to_owned());
        rest = after;
    }
    if rest.is_empty() { (Vec::new(), title.trim().to_owned()) } else { (tags, rest.to_owned()) }
}

/// What main answers a block or a tab with. `none` is a project with no remote of a provider the tracker knows, and
/// `host` the host nothing recognised, which Check again looks up once more.
#[derive(Clone, Default, PartialEq, Debug, Serialize, Deserialize)]
pub struct PullsAnswer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows: Option<Vec<PullRow>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub none: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<crate::problem::Problem>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Provider;
    use serde_json::json;

    fn row(id: &str, scope: Scope, draft: bool) -> PullRow {
        PullRow {
            id: id.into(),
            title: String::new(),
            number: String::new(),
            author: None,
            branch: String::new(),
            scope,
            draft,
            badge: RowBadge { text: String::new(), tone: String::new() },
            review: String::new(),
            checks: Counts::default(),
            updated: String::new(),
            unread: 0,
            agent: None,
            args: PrArgs { provider: Provider::Github, url: String::new(), number: 1, repo: String::new(), title: String::new(), cwd: String::new(), remote: None },
        }
    }

    fn rows() -> Vec<PullRow> {
        vec![row("a", Scope::Other, false), row("b", Scope::Mine, false), row("c", Scope::Review, true), row("d", Scope::Other, true), row("e", Scope::Mine, false)]
    }

    fn ids(rows: Vec<PullRow>) -> Vec<String> {
        rows.into_iter().map(|r| r.id).collect()
    }

    #[test]
    fn order_pulls_puts_yours_first_then_the_ones_asked_of_you_then_the_rest_each_in_the_lists_order() {
        assert_eq!(ids(order_pulls(&rows())), ["b", "e", "c", "a", "d"]);
        assert!(order_pulls(&[]).is_empty());
    }

    #[test]
    fn each_filter_shows_its_own_rows_and_all_open_shows_every_one() {
        assert_eq!(ids(filter_pulls(&rows(), Filter::All)), ["a", "b", "c", "d", "e"]);
        assert_eq!(ids(filter_pulls(&rows(), Filter::Mine)), ["b", "e"]);
        assert_eq!(ids(filter_pulls(&rows(), Filter::Review)), ["c"]);
        assert_eq!(ids(filter_pulls(&rows(), Filter::Drafts)), ["c", "d"]);
        assert_eq!(count_pulls(&rows()), [5, 2, 1, 2]);
        assert_eq!(count_pulls(&[]), [0, 0, 0, 0]);
    }

    #[test]
    fn a_saved_filter_is_used_only_when_it_is_one_of_the_filters() {
        assert_eq!(filter_of(&json!({ "filter": "mine" })), Filter::Mine);
        assert_eq!(filter_of(&json!({ "filter": "drafts" })), Filter::Drafts);
        assert_eq!(filter_of(&json!({ "filter": "everything" })), Filter::All);
        assert_eq!(filter_of(&Value::Null), Filter::All);
        assert_eq!(filter_of(&json!("mine")), Filter::All);
    }

    #[test]
    fn leading_tags_of_a_title_are_split_off_and_a_title_that_is_only_tags_stays_whole() {
        assert_eq!(split_title("[ABC-12] [web] Add filters"), (vec!["ABC-12".to_owned(), "web".to_owned()], "Add filters".to_owned()));
        assert_eq!(split_title("Fix [typo] in docs"), (vec![], "Fix [typo] in docs".to_owned()));
        assert_eq!(split_title("[WIP]"), (vec![], "[WIP]".to_owned()));
        assert_eq!(split_title("  plain  "), (vec![], "plain".to_owned()));
    }
}
