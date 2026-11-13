//! Notifications about pull requests: which ones you hear about, how often the lists are checked while Wings is in the
//! background, and which PRs in a list are new or newly ask for your review. The settings themselves are kept by the
//! frames that read them.

use crate::types::ListedPr;
use serde_json::{Value, json};
use std::collections::HashSet;

/// The minutes you can pick between background checks, set in Settings.
pub const WATCH_MINUTES: [u32; 4] = [5, 10, 15, 30];
/// The storage key the settings page saves the minutes under.
pub const WATCH_KEY: &str = "watch-minutes";
/// More new PRs than this at once are one notification, so a burst stays under the host's 20 a minute.
pub const MAX_SINGLE: usize = 3;

/// A saved interval, or none when what was saved isn't one, which leaves the default (5).
pub fn watch_of(saved: &Value) -> Option<u32> {
    WATCH_MINUTES.into_iter().find(|m| saved.as_u64() == Some(u64::from(*m)))
}

/// What you're notified about: nothing, the PRs that ask for your review, or those and every other new PR.
pub const NOTIFY: [&str; 3] = ["off", "review", "all"];

pub fn notify_of(saved: &Value) -> Option<&'static str> {
    NOTIFY.into_iter().find(|n| saved.as_str() == Some(n))
}

/// What the sidebar lists under each project: yours and the ones asked of you, or every open PR.
pub const SIDEBAR: [&str; 2] = ["mine", "all"];

pub fn sidebar_of(saved: &Value) -> Option<&'static str> {
    SIDEBAR.into_iter().find(|n| saved.as_str() == Some(n))
}

/// The PRs in `items` that weren't open on the last check, leaving out yours. A repo listed for the first time has none,
/// since its first list is where counting starts.
pub fn new_pulls<'a>(before: Option<&HashSet<String>>, items: &'a [ListedPr]) -> Vec<&'a ListedPr> {
    let Some(before) = before else { return Vec::new() };
    items.iter().filter(|i| !i.mine && !before.contains(&i.link.url)).collect()
}

/// The PRs in `items` that ask for your review and didn't on the last check: a new PR with you as a reviewer, or an open
/// one you were added to. `before` holds the ones that asked last time, and a repo listed for the first time has none.
pub fn new_reviews<'a>(before: Option<&HashSet<String>>, items: &'a [ListedPr]) -> Vec<&'a ListedPr> {
    let Some(before) = before else { return Vec::new() };
    items.iter().filter(|i| !i.mine && i.reviewing && !before.contains(&i.link.url)).collect()
}

/// What `wings.notify` shows: a title, a body and buttons, each `{ label, open: { view, args? } }` or `{ label, url }`.
#[derive(Clone, PartialEq, Debug)]
pub struct Notification {
    pub title: String,
    pub body: String,
    pub actions: Vec<Value>,
}

/// A PR a notification names.
pub struct NoteItem {
    pub title: String,
    pub author: Option<String>,
    pub url: String,
    /// What the "pr" view is opened with.
    pub args: Value,
}

/// The notifications for a project's new PRs, or its new review requests with `review`: one each, with buttons to open
/// it in a tab or the browser, or one for all of them when there are more than `MAX_SINGLE`. Every one can open the
/// project's list of open PRs.
pub fn notes_for(project: &str, items: &[NoteItem], review: bool) -> Vec<Notification> {
    let all = json!({ "label": "All pull requests", "open": { "view": "pr" } });
    if items.len() > MAX_SINGLE {
        let title = if review { format!("{} pull requests to review in {project}", items.len()) } else { format!("{} new pull requests in {project}", items.len()) };
        return vec![Notification { title, body: items.iter().map(|i| i.title.as_str()).collect::<Vec<_>>().join("\n"), actions: vec![all] }];
    }
    items
        .iter()
        .map(|i| Notification {
            title: if review { format!("Review asked in {project}") } else { format!("New pull request in {project}") },
            body: match &i.author {
                Some(author) if !author.is_empty() => format!("{} · {author}", i.title),
                _ => i.title.clone(),
            },
            actions: vec![json!({ "label": "Open", "open": { "view": "pr", "args": i.args } }), json!({ "label": "Open in browser", "url": i.url }), all.clone()],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Counts, Link, PrState, Provider, Status};

    fn pr(url: &str, mine: bool, reviewing: bool) -> ListedPr {
        ListedPr {
            link: Link { provider: Provider::Github, url: url.into(), number: 1, repo: "o/r".into(), remote: None, cwd: None, raw: None },
            title: String::new(),
            head: String::new(),
            mine,
            reviewing,
            author: None,
            status: Status { key: String::new(), title: String::new(), label: String::new(), state: PrState::Open, tone: String::new(), icon: String::new(), checks: Counts::default(), review: String::new(), failing: vec![], items: None, changes: None, branch: String::new(), updated: String::new(), merge: None, base: None, head: None, base_commit: None, head_commit: None },
            loaded: true,
            activity: None,
            updated_at: None,
        }
    }

    fn urls(items: Vec<&ListedPr>) -> Vec<&str> {
        items.into_iter().map(|i| i.link.url.as_str()).collect()
    }

    fn set(urls: &[&str]) -> HashSet<String> {
        urls.iter().map(|u| (*u).to_owned()).collect()
    }

    #[test]
    fn a_repos_first_list_is_where_counting_starts_so_nothing_in_it_is_new() {
        assert!(new_pulls(None, &[pr("a", false, false), pr("b", false, false)]).is_empty());
    }

    #[test]
    fn a_pr_that_wasnt_open_on_the_last_check_is_new_unless_you_wrote_it() {
        let before = set(&["a"]);
        assert_eq!(urls(new_pulls(Some(&before), &[pr("a", false, false), pr("b", false, false), pr("c", true, false)])), ["b"]);
        assert_eq!(urls(new_pulls(Some(&set(&[])), &[pr("a", false, false)])), ["a"]);
    }

    #[test]
    fn a_pr_asks_for_your_review_when_its_new_with_you_on_it_or_youre_added_to_an_open_one_never_on_the_first_list() {
        assert!(new_reviews(None, &[pr("a", false, true)]).is_empty());
        let items = [pr("a", false, true), pr("b", false, true), pr("c", false, false), pr("d", true, true)];
        assert_eq!(urls(new_reviews(Some(&set(&["a"])), &items)), ["b"]);
        // `before` only holds the ones that asked last time, so an open PR you're added to now counts.
        assert_eq!(urls(new_reviews(Some(&set(&[])), &[pr("c", false, true)])), ["c"]);
    }

    fn item(title: &str, author: Option<&str>, n: u32) -> NoteItem {
        NoteItem { title: title.into(), author: author.map(str::to_owned), url: format!("https://host/pr/{n}"), args: json!({ "n": n }) }
    }

    #[test]
    fn a_review_request_reads_as_one_alone_and_in_a_burst() {
        assert_eq!(notes_for("web", &[item("PR 1", Some("Ana"), 1)], true)[0].title, "Review asked in web");
        let burst: Vec<NoteItem> = (1..=4).map(|n| item(&format!("PR {n}"), Some("Ana"), n)).collect();
        assert_eq!(notes_for("web", &burst, true)[0].title, "4 pull requests to review in web");
    }

    #[test]
    fn each_new_pr_gets_its_own_notification_with_its_buttons_and_a_burst_gets_one() {
        let all = json!({ "label": "All pull requests", "open": { "view": "pr" } });
        let notes = notes_for("web", &[item("Fix login", Some("Ana"), 1), item("Docs", None, 2)], false);
        assert_eq!(
            notes,
            [
                Notification { title: "New pull request in web".into(), body: "Fix login · Ana".into(), actions: vec![json!({ "label": "Open", "open": { "view": "pr", "args": { "n": 1 } } }), json!({ "label": "Open in browser", "url": "https://host/pr/1" }), all.clone()] },
                Notification { title: "New pull request in web".into(), body: "Docs".into(), actions: vec![json!({ "label": "Open", "open": { "view": "pr", "args": { "n": 2 } } }), json!({ "label": "Open in browser", "url": "https://host/pr/2" }), all.clone()] },
            ]
        );
        let burst: Vec<NoteItem> = (0..=MAX_SINGLE as u32).map(|n| item(&format!("PR {n}"), None, n)).collect();
        assert_eq!(notes_for("web", &burst, false), [Notification { title: format!("{} new pull requests in web", MAX_SINGLE + 1), body: burst.iter().map(|i| i.title.as_str()).collect::<Vec<_>>().join("\n"), actions: vec![all] }]);
    }

    #[test]
    fn a_saved_interval_that_isnt_one_of_the_choices_is_not_accepted() {
        assert_eq!(watch_of(&json!(10)), Some(10));
        assert_eq!(watch_of(&json!(0)), None);
        assert_eq!(watch_of(&json!(7)), None);
        assert_eq!(watch_of(&json!("10")), None);
        assert_eq!(watch_of(&Value::Null), None);
        assert_eq!(notify_of(&json!("review")), Some("review"));
        assert_eq!(notify_of(&json!("everything")), None);
        assert_eq!(sidebar_of(&json!("every")), None);
        assert_eq!(sidebar_of(&json!("all")), Some("all"));
    }
}
