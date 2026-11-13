//! The Checks view's data: every check or policy of a PR as one list, however its provider words it, and the groups it
//! shows in. Each provider's own mapper builds the `CheckItem`s.

use crate::types::{CheckItem, CheckState, Failing};
use wings_plugin::format::plural;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GroupId {
    Failed,
    Active,
    Passed,
    Other,
}

impl GroupId {
    pub fn name(self) -> &'static str {
        match self {
            Self::Failed => "failed",
            Self::Active => "active",
            Self::Passed => "passed",
            Self::Other => "other",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Failed => "Failed",
            Self::Active => "Running and queued",
            Self::Passed => "Passed",
            Self::Other => "Optional and not run",
        }
    }
}

pub struct Group {
    pub id: GroupId,
    pub label: &'static str,
    pub items: Vec<CheckItem>,
}

/// Where a check shows: what failed first, then what is still going, then what passed; optional passes and checks that didn't run last.
fn group_of(c: &CheckItem) -> GroupId {
    match c.state {
        CheckState::Failed => GroupId::Failed,
        CheckState::Running | CheckState::Queued => GroupId::Active,
        CheckState::Skipped => GroupId::Other,
        CheckState::Passed if c.required == Some(false) => GroupId::Other,
        CheckState::Passed => GroupId::Passed,
    }
}

fn rank(state: CheckState) -> u8 {
    match state {
        CheckState::Failed => 0,
        CheckState::Running => 1,
        CheckState::Queued => 2,
        CheckState::Passed => 3,
        CheckState::Skipped => 4,
    }
}

/// The non-empty groups in display order. Inside a group a required check comes first, a running one before a queued one, then by name.
pub fn group_checks(items: &[CheckItem]) -> Vec<Group> {
    let mut sorted = items.to_vec();
    sorted.sort_by(|a, b| {
        (b.required == Some(true))
            .cmp(&(a.required == Some(true)))
            .then(rank(a.state).cmp(&rank(b.state)))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    [GroupId::Failed, GroupId::Active, GroupId::Passed, GroupId::Other]
        .into_iter()
        .map(|id| Group { id, label: id.label(), items: sorted.iter().filter(|c| group_of(c) == id).cloned().collect() })
        .filter(|group| !group.items.is_empty())
        .collect()
}

/// "45s", "2m 14s", "1h 05m".
pub fn duration_text(seconds: Option<f64>) -> Option<String> {
    let seconds = seconds.filter(|s| s.is_finite() && *s >= 0.0)?;
    let s = seconds.round() as u64;
    Some(if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m {:02}s", s / 60, s % 60)
    } else {
        format!("{}h {:02}m", s / 3600, (s % 3600) / 60)
    })
}

/// Milliseconds since the epoch of an ISO time like `2026-01-01T00:02:14Z` or `2026-01-01T00:02:14.5+02:00`, or none
/// when it isn't one.
pub fn iso_to_ms(iso: &str) -> Option<f64> {
    let (date, time) = iso.split_once(['T', ' '])?;
    let mut d = date.split('-');
    let (year, month, day): (i64, i64, i64) = (d.next()?.parse().ok()?, d.next()?.parse().ok()?, d.next()?.parse().ok()?);
    if d.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (clock, offset) = match time.find(['Z', 'z', '+', '-']) {
        Some(at) => (&time[..at], &time[at..]),
        None => (time, "Z"),
    };
    let mut c = clock.split(':');
    let (hour, minute): (f64, f64) = (c.next()?.parse().ok()?, c.next()?.parse().ok()?);
    let second: f64 = c.next().unwrap_or("0").parse().ok()?;
    let offset_minutes = if offset.eq_ignore_ascii_case("z") {
        0.0
    } else {
        let sign = if offset.starts_with('-') { -1.0 } else { 1.0 };
        let digits: String = offset[1..].chars().filter(char::is_ascii_digit).collect();
        if digits.len() < 2 {
            return None;
        }
        let h: f64 = digits[..2].parse().ok()?;
        let m: f64 = digits.get(2..4).map_or(Some(0.0), |m| m.parse().ok())?;
        sign * (h * 60.0 + m)
    };
    // Days from the civil date, as Howard Hinnant's algorithm counts them.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 86_400) as f64 + hour * 3600.0 + minute * 60.0 + second - offset_minutes * 60.0) * 1000.0)
}

/// GitHub and Azure DevOps send year 1 or 0001 for a time that hasn't happened.
pub fn real(iso: Option<&str>) -> Option<String> {
    let iso = iso?;
    (iso_to_ms(iso)? > iso_to_ms("2000-01-01T00:00:00Z")?).then(|| iso.to_owned())
}

/// The seconds between two ISO times, or none when either is missing, a provider's "never" date or out of order.
pub fn between(start: Option<&str>, end: Option<&str>) -> Option<f64> {
    let (a, b) = (iso_to_ms(&real(start)?)?, iso_to_ms(&real(end)?)?);
    (b >= a).then(|| (b - a) / 1000.0)
}

/// The note a comment requirement shows once the threads are known.
pub fn with_unresolved(items: &[CheckItem], unresolved: Option<usize>) -> Vec<CheckItem> {
    items
        .iter()
        .map(|c| match unresolved {
            Some(n) if c.needs.as_deref() == Some("unresolved") => {
                let note = if n > 0 { format!("{} unresolved", plural(n, "comment thread")) } else { "All comment threads resolved".to_owned() };
                CheckItem { note: Some(note), ..c.clone() }
            }
            _ => c.clone(),
        })
        .collect()
}

/// Every failed check's `Failing`, which the badge and the MCP tool list.
pub fn failing_of(items: &[CheckItem]) -> Vec<Failing> {
    items.iter().filter(|c| c.state == CheckState::Failed).filter_map(|c| c.failing.clone()).collect()
}

/// Whether a failed check has a log to fetch: a GitHub Actions job, a GitLab job or an Azure build.
pub fn has_log(f: Option<&Failing>) -> bool {
    f.is_some_and(|f| f.job.as_deref().is_some_and(|job| !job.is_empty()) || f.build.is_some_and(|build| build != 0))
}

const SHOWN_LINES: usize = 40;

/// The end of a log for the page: at most `SHOWN_LINES` lines, and how many came before them.
pub fn log_tail(log: &str) -> (String, usize) {
    log_tail_of(log, SHOWN_LINES)
}

pub fn log_tail_of(log: &str, max: usize) -> (String, usize) {
    let lines: Vec<&str> = log.split('\n').collect();
    let hidden = lines.len().saturating_sub(max);
    (lines[hidden..].join("\n"), hidden)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(name: &str, state: CheckState, required: Option<bool>) -> CheckItem {
        CheckItem { id: name.into(), name: name.into(), state, required, kind: None, at: None, seconds: None, url: None, note: None, failing: None, run: None, needs: None }
    }

    fn names(group: &Group) -> Vec<&str> {
        group.items.iter().map(|c| c.name.as_str()).collect()
    }

    #[test]
    fn groups_run_failed_running_and_queued_passed_then_optional_and_not_run_and_skip_empty_ones() {
        use CheckState::*;
        let groups = group_checks(&[
            item("lint", Passed, None),
            item("deploy preview", Skipped, None),
            item("e2e", Queued, None),
            item("unit", Failed, None),
            item("build", Running, None),
            item("coverage", Passed, Some(false)),
        ]);
        let shown: Vec<(&str, Vec<&str>)> = groups.iter().map(|g| (g.id.name(), names(g))).collect();
        assert_eq!(shown, [("failed", vec!["unit"]), ("active", vec!["build", "e2e"]), ("passed", vec!["lint"]), ("other", vec!["coverage", "deploy preview"])]);
        assert_eq!(group_checks(&[item("a", Passed, None)]).iter().map(|g| g.id.name()).collect::<Vec<_>>(), ["passed"]);
        assert!(group_checks(&[]).is_empty());
    }

    #[test]
    fn an_optional_failure_still_shows_under_failed_and_required_checks_come_before_optional_ones() {
        let groups = group_checks(&[item("b", CheckState::Failed, Some(false)), item("a", CheckState::Failed, Some(true)), item("c", CheckState::Failed, None)]);
        assert_eq!(names(&groups[0]), ["a", "b", "c"]);
    }

    #[test]
    fn durations_read_as_seconds_minutes_or_hours_and_bad_times_give_nothing() {
        assert_eq!(duration_text(Some(45.0)).as_deref(), Some("45s"));
        assert_eq!(duration_text(Some(134.4)).as_deref(), Some("2m 14s"));
        assert_eq!(duration_text(Some(3900.0)).as_deref(), Some("1h 05m"));
        assert_eq!(duration_text(None), None);
        assert_eq!(duration_text(Some(-1.0)), None);
        assert_eq!(between(Some("2026-01-01T00:00:00Z"), Some("2026-01-01T00:02:14Z")), Some(134.0));
        assert_eq!(between(Some("0001-01-01T00:00:00Z"), Some("2026-01-01T00:02:14Z")), None);
        assert_eq!(between(Some("2026-01-02T00:00:00Z"), Some("2026-01-01T00:00:00Z")), None);
    }

    #[test]
    fn iso_times_read_with_fractions_and_offsets() {
        assert_eq!(iso_to_ms("1970-01-01T00:00:00Z"), Some(0.0));
        assert_eq!(iso_to_ms("2026-01-01T02:00:00.5+02:00"), iso_to_ms("2026-01-01T00:00:00.5Z"));
        assert_eq!(iso_to_ms("2026-01-01T00:00:00.5Z"), Some(1_767_225_600_500.0));
        assert_eq!(iso_to_ms("nope"), None);
    }

    #[test]
    fn a_log_can_be_read_where_the_failure_names_a_job_or_a_build() {
        assert!(has_log(Some(&Failing { id: "1".into(), name: Some("unit".into()), job: Some("77".into()), ..Default::default() })));
        assert!(has_log(Some(&Failing { id: "2".into(), name: Some("ci".into()), build: Some(42), ..Default::default() })));
        assert!(!has_log(Some(&Failing { id: "3".into(), name: Some("deploy".into()), ..Default::default() })));
        assert!(!has_log(None));
    }

    #[test]
    fn a_comment_requirement_counts_the_open_threads_once_they_are_known() {
        let c = CheckItem { needs: Some("unresolved".into()), note: Some("Comment threads are unresolved".into()), ..item("Comment requirements", CheckState::Failed, None) };
        let note = |items: Vec<CheckItem>| items[0].note.clone().unwrap();
        assert_eq!(note(with_unresolved(&[c.clone()], None)), "Comment threads are unresolved");
        assert_eq!(note(with_unresolved(&[c.clone()], Some(3))), "3 comment threads unresolved");
        assert_eq!(note(with_unresolved(&[c.clone()], Some(1))), "1 comment thread unresolved");
        assert_eq!(note(with_unresolved(&[c], Some(0))), "All comment threads resolved");
        let plain = CheckItem { note: Some("n".into()), ..item("x", CheckState::Passed, None) };
        assert_eq!(note(with_unresolved(&[plain], Some(5))), "n");
    }

    #[test]
    fn a_log_shows_its_last_lines_and_says_how_many_came_before() {
        let log = (0..100).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let (text, hidden) = log_tail(&log);
        assert_eq!(hidden, 60);
        assert_eq!(text.split('\n').count(), 40);
        assert_eq!(text.split('\n').next_back(), Some("line 99"));
        assert_eq!(log_tail("a\nb"), ("a\nb".to_owned(), 0));
    }
}
