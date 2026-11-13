//! What an agent's change journal reports at the end of a turn, as the issue's commit links and files. Nothing here touches
//! Wings.

use crate::domain::{Issue, Status};
use serde_json::Value;
use wings_plugin::format::parse_time;

pub struct Journal {
    pub files: Vec<(String, String)>,
    pub commits: Vec<String>,
    /// When the turn's first prompt arrived.
    pub started: Option<f64>,
}

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The files of a report with a path, and the commit hashes of 7 to 40 hex digits, dropping what isn't. Anything may be
/// missing or malformed: it is another tool's output. `more` (files beyond those listed) has no row to make.
pub fn journal_of(input: &Value) -> Journal {
    let files = input["files"].as_array().into_iter().flatten().filter_map(|f| {
        let path = one_line(f["path"].as_str()?);
        (!path.is_empty()).then(|| (path, f["what"].as_str().map(one_line).unwrap_or_default()))
    });
    let commits = input["commits"].as_array().into_iter().flatten().filter_map(Value::as_str).filter(|c| (7..=40).contains(&c.len()) && c.bytes().all(|b| b.is_ascii_hexdigit()));
    Journal { files: files.collect(), commits: commits.map(str::to_owned).collect(), started: input["started"].as_str().and_then(parse_time) }
}

/// In progress or in review: the statuses that say someone is working on it.
pub fn is_live(issue: &Issue) -> bool {
    matches!(issue.status, Status::InProgress | Status::InReview)
}

/// The issue a journal report belongs to. The one the session touched since the turn started, whatever its status, since
/// closing it is often the turn's last step; without `started`, the session's most recently updated one if it is in progress
/// or in review. Either way a child of that issue the session touched as well wins over it, so an epic logged last doesn't
/// take its task's files.
pub fn journal_issue<'a>(issues: &'a [Issue], session_id: &str, started: Option<f64>) -> Option<&'a Issue> {
    let seen = |i: &Issue| i.sessions.iter().find(|s| s.id == session_id).map_or(-1.0, |s| s.seen);
    let mine: Vec<&Issue> = issues.iter().filter(|i| seen(i) >= 0.0).collect();
    let pick = |list: Vec<&'a Issue>, by: &dyn Fn(&Issue) -> f64| -> Option<&'a Issue> {
        let mut sorted = list;
        sorted.sort_by(|a, b| by(b).total_cmp(&by(a)));
        let first = *sorted.first()?;
        Some(sorted.iter().copied().find(|i| i.parent.as_deref() == Some(&first.key)).unwrap_or(first))
    };
    if let Some(started) = started {
        if let Some(turn) = pick(mine.iter().copied().filter(|i| seen(i) >= started).collect(), &seen) {
            return Some(turn);
        }
    }
    let latest = pick(mine.clone(), &|i| i.updated_at)?;
    is_live(latest).then(|| pick(mine.iter().copied().filter(|i| is_live(i)).collect(), &|i| i.updated_at)).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{IssueType, SessionRef};
    use serde_json::json;

    #[test]
    fn a_report_keeps_files_with_a_path_and_real_hashes() {
        let out = journal_of(&json!({ "files": [{ "path": "a.ts", "what": "x\ny" }, { "what": "no path" }, null, "str"], "commits": ["abc1234", "xyz", 5, "ABCDEF1234567890"] }));
        assert_eq!((out.files, out.commits, out.started), (vec![("a.ts".into(), "x y".into())], vec!["abc1234".to_owned(), "ABCDEF1234567890".to_owned()], None));
        let none = journal_of(&json!({ "files": "no", "commits": null }));
        assert!(none.files.is_empty() && none.commits.is_empty());
    }

    fn issue(key: &str, status: Status, updated_at: f64, ids: &[&str], parent: Option<&str>) -> Issue {
        Issue {
            key: key.into(),
            title: String::new(),
            description: None,
            status,
            kind: IssueType::Task,
            priority: None,
            parent: parent.map(Into::into),
            related: None,
            blocked_by: None,
            links: None,
            files: None,
            sessions: ids.iter().map(|id| SessionRef { id: (*id).into(), name: None, kind: None, seen: 1.0 }).collect(),
            log: vec![],
            created_at: 0.0,
            updated_at,
        }
    }

    fn seen(key: &str, status: Status, seen: f64, parent: Option<&str>) -> Issue {
        let mut i = issue(key, status, seen, &["s"], parent);
        i.sessions[0].seen = seen;
        i
    }

    fn key(issues: &[Issue], started: Option<f64>) -> Option<String> {
        journal_issue(issues, "s", started).map(|i| i.key.clone())
    }

    #[test]
    fn the_issue_is_the_sessions_latest_and_only_while_it_is_in_progress_or_in_review() {
        let pick = |issues: &[Issue], id: &str| journal_issue(issues, id, None).map(|i| i.key.clone());
        assert_eq!(pick(&[issue("A-1", Status::InProgress, 1.0, &["s"], None), issue("A-2", Status::InReview, 5.0, &["s"], None), issue("A-3", Status::InProgress, 9.0, &["other"], None)], "s"), Some("A-2".into()));
        assert_eq!(pick(&[issue("A-1", Status::InProgress, 1.0, &["s"], None), issue("A-2", Status::Done, 5.0, &["s"], None)], "s"), None);
        assert_eq!(pick(&[issue("A-1", Status::Todo, 1.0, &["s"], None)], "s"), None);
        assert_eq!(pick(&[issue("A-1", Status::InProgress, 1.0, &["s"], None)], "unknown"), None);
    }

    #[test]
    fn an_issue_the_session_touched_this_turn_gets_the_report_closed_or_not_started_and_its_child_beats_it() {
        // Closed as the turn's last step, while an older issue is still in progress.
        assert_eq!(key(&[seen("A-1", Status::InProgress, 50.0, None), seen("A-2", Status::Done, 120.0, None)], Some(100.0)), Some("A-2".into()));
        assert_eq!(key(&[seen("A-1", Status::Todo, 120.0, None)], Some(100.0)), Some("A-1".into()));
        // The epic was logged last, but the turn worked on its task.
        assert_eq!(key(&[seen("E-1", Status::InProgress, 130.0, None), seen("A-3", Status::InProgress, 110.0, Some("E-1"))], Some(100.0)), Some("A-3".into()));
        assert_eq!(key(&[seen("E-1", Status::InProgress, 130.0, None), seen("A-3", Status::InProgress, 90.0, Some("E-1"))], Some(100.0)), Some("E-1".into()), "a child from an earlier turn doesn't count");
        // Nothing touched this turn: the latest issue, only while it is live.
        assert_eq!(key(&[seen("A-1", Status::InProgress, 50.0, None), seen("A-2", Status::Done, 60.0, None)], Some(100.0)), None);
        assert_eq!(key(&[seen("E-1", Status::InProgress, 60.0, None), seen("A-3", Status::InReview, 50.0, Some("E-1"))], Some(100.0)), Some("A-3".into()));
    }

    #[test]
    fn a_reports_start_time_is_read_when_it_is_a_real_time() {
        assert_eq!(journal_of(&json!({ "started": "2026-10-20T14:02:11.000Z" })).started, parse_time("2026-10-20T14:02:11.000Z"));
        assert!(parse_time("2026-10-20T14:02:11.000Z").is_some());
        assert_eq!(journal_of(&json!({ "started": "soon" })).started, None);
        assert_eq!(journal_of(&json!({ "started": 5 })).started, None);
    }
}
