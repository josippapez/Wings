//! What Claude is told at the start of a session in a project Issues knows. Short: it goes out every time.

use crate::{domain::Issue, journal::is_live};

/// How long an issue in progress or in review may sit unchanged before the session start line mentions it.
const STALE_DAYS: f64 = 3.0;
/// The most stale issues the line names.
const STALE_SHOWN: usize = 3;
const DAY: f64 = 24.0 * 60.0 * 60.0 * 1000.0;

/// The issues in progress or in review untouched for `STALE_DAYS` or more, longest first.
pub fn stale_issues(issues: &[Issue], now: f64) -> Vec<&Issue> {
    let mut stale: Vec<&Issue> = issues.iter().filter(|i| is_live(i) && now - i.updated_at >= STALE_DAYS * DAY).collect();
    stale.sort_by(|a, b| a.updated_at.total_cmp(&b.updated_at));
    stale
}

/// Added to each message in a session whose issue waits in review, which the user often settles in their next message.
pub fn in_review_text(issue: &Issue) -> String {
    format!(
        "This session's {} \"{}\" is In review. If this message says the work is done or works, set it Done with tasks__update; if it reports a problem with it, set it back to In progress.",
        issue.key, issue.title
    )
}

pub struct Start<'a> {
    pub project_name: &'a str,
    pub keep_local: bool,
    pub issue: Option<&'a Issue>,
    pub stale: &'a [&'a Issue],
    pub now: f64,
}

pub fn session_start_text(o: &Start) -> String {
    let mut lines = vec![format!(
        "Wings Issues is the local tracker for {}, kept on this computer for the user and you{}.",
        o.project_name,
        if o.keep_local { ", and its keys never go in commits, branches or PRs" } else { "" }
    )];
    match o.issue {
        Some(issue) => lines.push(format!(
            "This session is on {} \"{}\" ({}). Keep it current: set the status with tasks__update as the work moves, and add a tasks__log note when a turn changed files or finished, paused or blocked the work. A turn that only rechecks, reviews, answers or decides something needs no note.",
            issue.key,
            issue.title,
            issue.status.label()
        )),
        None => lines.push("When a task will change files, find its issue with tasks__list or create one with tasks__create before you edit. If you already started editing without an issue, open it now. A question, a check, a review or a lookup gets no issue: read issues with tasks__get or tasks__list when the answer needs them.".into()),
    }
    if !o.stale.is_empty() {
        let named: Vec<String> = o.stale.iter().take(STALE_SHOWN).map(|i| format!("{} is {} but unchanged for {} days.", i.key, i.status.label(), ((o.now - i.updated_at) / DAY).floor())).collect();
        let more = if o.stale.len() > STALE_SHOWN { format!(" And {} more.", o.stale.len() - STALE_SHOWN) } else { String::new() };
        lines.push(format!("Worth checking: {}{more}", named.join(" ")));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{IssueType, Status};

    fn issue(key: &str, status: Status, days: f64, now: f64) -> Issue {
        Issue { key: key.into(), title: key.into(), description: None, status, kind: IssueType::Task, priority: None, parent: None, related: None, blocked_by: None, links: None, files: None, sessions: vec![], log: vec![], created_at: 0.0, updated_at: now - days * DAY }
    }

    #[test]
    fn the_session_start_line_names_the_linked_issue_or_asks_for_an_issue() {
        let mut issue = issue("WEB-4", Status::InProgress, 0.0, 0.0);
        issue.title = "Checkout summary".into();
        let start = |issue: Option<&Issue>, keep_local: bool| session_start_text(&Start { project_name: "web", keep_local, issue, stale: &[], now: 0.0 });
        assert!(start(Some(&issue), true).contains("WEB-4 \"Checkout summary\" (In progress)"));
        let fresh = start(None, true);
        assert!(fresh.lines().any(|l| l.starts_with("When a task will change files")));
        assert!(fresh.contains("A question, a check, a review or a lookup gets no issue"));
        assert!(fresh.contains("tasks__list or create one with tasks__create"));
        assert!(!fresh.contains("tasks__todo"));
        assert!(!start(None, false).contains("never go in commits"));
    }

    #[test]
    fn an_issue_in_progress_or_in_review_is_listed_as_stale_from_3_days_not_at_2() {
        let now = 100.0 * DAY;
        let issues = [issue("WEB-1", Status::InProgress, 5.0, now), issue("WEB-2", Status::InProgress, 2.0, now), issue("WEB-3", Status::InReview, 3.0, now), issue("WEB-4", Status::Done, 9.0, now), issue("WEB-5", Status::Todo, 9.0, now)];
        let stale = stale_issues(&issues, now);
        assert_eq!(stale.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(), ["WEB-1", "WEB-3"]);
        let text = session_start_text(&Start { project_name: "web", keep_local: true, issue: None, stale: &stale, now });
        assert!(text.contains("WEB-1 is In progress but unchanged for 5 days."));
        assert!(text.contains("WEB-3 is In review but unchanged for 3 days."));
        assert!(!["WEB-2", "WEB-4", "WEB-5"].iter().any(|k| text.contains(k)));
        let two = [issue("WEB-2", Status::InProgress, 2.0, now)];
        assert!(!session_start_text(&Start { project_name: "web", keep_local: true, issue: None, stale: &stale_issues(&two, now), now }).contains("unchanged"));
        let many: Vec<Issue> = (0..5).map(|i| issue(&format!("WEB-{i}"), Status::InProgress, 4.0 + f64::from(i), now)).collect();
        assert!(session_start_text(&Start { project_name: "web", keep_local: true, issue: None, stale: &stale_issues(&many, now), now }).ends_with("And 2 more."));
    }
}
