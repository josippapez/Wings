//! How issues are ordered and grouped for the board, the list, the short list and the breadcrumb.

use crate::domain::{Issue, Status, key_number, priority_rank};
use std::{cmp::Ordering, collections::{HashMap, HashSet}};

fn cmp_key(a: &Issue, b: &Issue) -> Ordering {
    key_number(&a.key).cmp(&key_number(&b.key))
}

fn newest_first(a: &Issue, b: &Issue) -> Ordering {
    b.updated_at.total_cmp(&a.updated_at)
}

/// How the board and the list order the issues inside a column or group: the usual order, or by date, newest first.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Sort {
    #[default]
    Default,
    Created,
    Updated,
}

impl Sort {
    /// The board's Sort choices as (value, label).
    pub const OPTIONS: [(&'static str, &'static str); 3] = [("default", "Default"), ("created", "Created"), ("updated", "Updated")];

    pub fn as_str(self) -> &'static str {
        match self {
            Sort::Default => "default",
            Sort::Created => "created",
            Sort::Updated => "updated",
        }
    }

    pub fn parse(value: &str) -> Option<Sort> {
        match value {
            "default" => Some(Sort::Default),
            "created" => Some(Sort::Created),
            "updated" => Some(Sort::Updated),
            _ => None,
        }
    }

    /// The sort a project section's order stands for: "date" is the latest change, "created" the creation date, anything
    /// else is by status.
    pub fn of_section(order: &str) -> Sort {
        match order {
            "date" => Sort::Updated,
            "created" => Sort::Created,
            _ => Sort::Default,
        }
    }

    /// Newest first by this sort's date, the oldest key breaking a tie; None for the usual order.
    fn by_date(self, a: &Issue, b: &Issue) -> Option<Ordering> {
        let order = match self {
            Sort::Default => return None,
            Sort::Created => b.created_at.total_cmp(&a.created_at),
            Sort::Updated => newest_first(a, b),
        };
        Some(order.then_with(|| cmp_key(a, b)))
    }
}

pub fn is_open(issue: &Issue) -> bool {
    issue.status != Status::Done && issue.status != Status::Canceled
}

/// The open issues of the short list and the project section: in progress first, then in review, then to do, each oldest key
/// first.
pub fn open_issues(issues: &[Issue]) -> Vec<&Issue> {
    let rank = |s: Status| match s {
        Status::InProgress => 0,
        Status::InReview => 1,
        _ => 2,
    };
    let mut open: Vec<&Issue> = issues.iter().filter(|i| is_open(i)).collect();
    open.sort_by(|a, b| rank(a.status).cmp(&rank(b.status)).then_with(|| cmp_key(a, b)));
    open
}

/// The project section's issues: the open ones an agent works on now first (an epic or feature counts when one under it
/// does), then the rest, each part by status like `open_issues` or by latest change or creation date, newest first.
pub fn section_issues<'a>(issues: &'a [Issue], working: impl Fn(&Issue) -> bool, sort: Sort) -> Vec<&'a Issue> {
    let open = open_issues(issues);
    let by_key: HashMap<&str, &Issue> = open.iter().map(|i| (i.key.as_str(), *i)).collect();
    let mut active: HashSet<&str> = HashSet::new();
    for i in &open {
        if !working(i) {
            continue;
        }
        // Stops at a key already marked, so a loop of parents ends.
        let mut at = Some(*i);
        while let Some(issue) = at.filter(|issue| !active.contains(issue.key.as_str())) {
            active.insert(&issue.key);
            at = issue.parent.as_deref().and_then(|p| by_key.get(p).copied());
        }
    }
    let mut sorted = open;
    if sort != Sort::Default {
        sorted.sort_by(|a, b| sort.by_date(a, b).unwrap_or(Ordering::Equal));
    }
    let (first, rest): (Vec<&Issue>, Vec<&Issue>) = sorted.into_iter().partition(|i| active.contains(i.key.as_str()));
    first.into_iter().chain(rest).collect()
}

pub struct Column<'a> {
    pub status: Status,
    pub issues: Vec<&'a Issue>,
}

/// The board's columns: highest priority first, then to do by key and the rest by latest activity, or by `sort`'s date when
/// it isn't the default. Canceled only when asked for.
pub fn board_columns<'a, I>(issues: I, show_canceled: bool, sort: Sort) -> Vec<Column<'a>>
where
    I: IntoIterator<Item = &'a Issue> + Clone,
{
    let statuses: &[Status] = if show_canceled { &Status::ALL } else { &Status::FLOW };
    statuses
        .iter()
        .map(|&status| {
            let mut column: Vec<&Issue> = issues.clone().into_iter().filter(|i| i.status == status).collect();
            column.sort_by(|a, b| sort.by_date(a, b).unwrap_or_else(|| priority_rank(a.priority).cmp(&priority_rank(b.priority)).then_with(|| if status == Status::Todo { cmp_key(a, b) } else { newest_first(a, b) })));
            Column { status, issues: column }
        })
        .collect()
}

/// A row of the board grouped by parent: an epic or feature and its children, or `parent` none for the issues with none.
pub struct Lane<'a> {
    pub parent: Option<&'a Issue>,
    pub columns: Vec<Column<'a>>,
    pub count: usize,
}

fn flow_rank(s: Status) -> usize {
    [Status::InProgress, Status::InReview, Status::Todo, Status::Done, Status::Canceled].iter().position(|x| *x == s).unwrap_or(0)
}

/// The board in rows, one per parent, like Linear's sub-grouping by parent issue: each issue is a card once, in the row of
/// the epic or feature it sits under. Open parents come first (in progress, in review, to do, then done and canceled), each
/// oldest key first, and the issues with no parent last.
pub fn board_lanes<'a>(issues: &[&'a Issue], show_canceled: bool, sort: Sort) -> Vec<Lane<'a>> {
    let by_key: HashMap<&str, &'a Issue> = issues.iter().map(|i| (i.key.as_str(), *i)).collect();
    let mut groups: HashMap<&str, Vec<&'a Issue>> = HashMap::new();
    for &i in issues {
        if i.status == Status::Canceled && !show_canceled {
            continue;
        }
        let parent = i.parent.as_deref().filter(|p| by_key.contains_key(p)).unwrap_or("");
        groups.entry(parent).or_default().push(i);
    }
    let lane = |parent: Option<&'a Issue>, members: &Vec<&'a Issue>| Lane { parent, columns: board_columns(members.iter().copied(), show_canceled, sort), count: members.len() };
    let mut parents: Vec<&'a Issue> = groups.keys().filter(|k| !k.is_empty()).map(|k| by_key[k]).collect();
    parents.sort_by(|a, b| flow_rank(a.status).cmp(&flow_rank(b.status)).then_with(|| cmp_key(a, b)));
    let mut lanes: Vec<Lane<'a>> = parents.iter().map(|p| lane(Some(p), &groups[p.key.as_str()])).collect();
    if let Some(members) = groups.get("") {
        lanes.push(lane(None, members));
    }
    lanes
}

/// A group of the list by parent: a top-level epic or feature over its children and theirs, each with its depth under it.
pub struct TreeGroup<'a> {
    pub root: &'a Issue,
    pub rows: Vec<(&'a Issue, usize)>,
}

/// The list grouped by parent, like Linear's sub-issues: one group per top-level issue that has children, with every issue
/// below it in tree order and indented by its depth (1 for a child), siblings by status (closest to done first), then
/// priority and key, or by `sort`'s date when it isn't the default. `loose` is the top-level issues with no children. Canceled ones only when asked for.
pub fn issue_tree<'a>(issues: &[&'a Issue], show_canceled: bool, sort: Sort) -> (Vec<TreeGroup<'a>>, Vec<&'a Issue>) {
    let shown: Vec<&Issue> = issues.iter().copied().filter(|i| show_canceled || i.status != Status::Canceled).collect();
    let keys: HashSet<&str> = shown.iter().map(|i| i.key.as_str()).collect();
    let rank = |s: Status| [Status::InReview, Status::InProgress, Status::Todo, Status::Done, Status::Canceled].iter().position(|x| *x == s).unwrap_or(0);
    let sort = |list: &mut Vec<&'a Issue>| list.sort_by(|a, b| rank(a.status).cmp(&rank(b.status)).then_with(|| sort.by_date(a, b).unwrap_or_else(|| priority_rank(a.priority).cmp(&priority_rank(b.priority)).then_with(|| cmp_key(a, b)))));
    let mut kids: HashMap<&str, Vec<&'a Issue>> = HashMap::new();
    for &i in &shown {
        if let Some(p) = i.parent.as_deref() {
            kids.entry(p).or_default().push(i);
        }
    }
    for list in kids.values_mut() {
        sort(list);
    }
    // A loop of parents would never end: a key already on the way down stops it.
    fn below<'a>(kids: &HashMap<&str, Vec<&'a Issue>>, key: &str, depth: usize, path: &mut Vec<String>, out: &mut Vec<(&'a Issue, usize)>) {
        for issue in kids.get(key).into_iter().flatten() {
            if path.contains(&issue.key) {
                continue;
            }
            out.push((issue, depth));
            path.push(issue.key.clone());
            below(kids, &issue.key, depth + 1, path, out);
            path.pop();
        }
    }
    let mut roots: Vec<&'a Issue> = shown.iter().copied().filter(|i| i.parent.as_deref().is_none_or(|p| !keys.contains(p))).collect();
    sort(&mut roots);
    let mut groups = Vec::new();
    let mut loose = Vec::new();
    for root in roots {
        let mut rows = Vec::new();
        below(&kids, &root.key, 1, &mut vec![root.key.clone()], &mut rows);
        if rows.is_empty() { loose.push(root) } else { groups.push(TreeGroup { root, rows }) }
    }
    (groups, loose)
}

/// The issues a tab shows in its breadcrumb at most, oldest dropped first.
pub const TRAIL_LENGTH: usize = 8;

/// The breadcrumb trail for opening `next` from `current`, whose own trail is `trail`: `current` goes on the end. Opening
/// one that's already on the trail goes back to it, so the trail never loops.
pub fn trail_to(trail: &[String], current: &str, next: &str) -> Vec<String> {
    let mut path: Vec<String> = trail.to_vec();
    path.push(current.to_owned());
    if let Some(back) = path.iter().position(|k| k == next) {
        path.truncate(back);
    }
    path.split_off(path.len().saturating_sub(TRAIL_LENGTH))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{IssueType, Priority};

    fn issue(n: u32, status: Status, updated_at: f64, extra: impl FnOnce(&mut Issue)) -> Issue {
        let mut i = Issue { key: format!("WEB-{n}"), title: format!("t{n}"), description: None, status, kind: IssueType::Task, priority: None, parent: None, related: None, blocked_by: None, links: None, files: None, sessions: vec![], log: vec![], created_at: 0.0, updated_at };
        extra(&mut i);
        i
    }
    fn keys(issues: &[&Issue]) -> Vec<String> {
        issues.iter().map(|i| i.key.clone()).collect()
    }

    #[test]
    fn board_columns_and_the_open_list_to_do_by_key_others_by_activity_canceled_only_on_request() {
        let issues = [issue(2, Status::Todo, 5.0, |_| {}), issue(1, Status::Todo, 9.0, |_| {}), issue(3, Status::InProgress, 1.0, |_| {}), issue(4, Status::InProgress, 8.0, |_| {}), issue(5, Status::Done, 2.0, |_| {}), issue(6, Status::Canceled, 3.0, |_| {}), issue(7, Status::InReview, 3.0, |_| {})];
        let columns = board_columns(&issues, false, Sort::Default);
        assert_eq!(columns.iter().map(|c| c.status).collect::<Vec<_>>(), Status::FLOW);
        assert_eq!(columns.iter().map(|c| keys(&c.issues)).collect::<Vec<_>>(), [vec!["WEB-1", "WEB-2"], vec!["WEB-4", "WEB-3"], vec!["WEB-7"], vec!["WEB-5"]]);
        assert_eq!(board_columns(&issues, true, Sort::Default).last().unwrap().status, Status::Canceled);
        assert_eq!(keys(&open_issues(&issues)), ["WEB-3", "WEB-4", "WEB-7", "WEB-1", "WEB-2"]);
    }

    fn lanes_sample() -> Vec<Issue> {
        vec![
            issue(1, Status::Done, 1.0, |i| i.kind = IssueType::Epic),
            issue(2, Status::InProgress, 2.0, |i| i.kind = IssueType::Epic),
            issue(3, Status::Todo, 3.0, |i| i.parent = Some("WEB-1".into())),
            issue(4, Status::InReview, 4.0, |i| { i.parent = Some("WEB-2".into()); i.kind = IssueType::Feature; }),
            issue(5, Status::Todo, 5.0, |i| i.parent = Some("WEB-4".into())),
            issue(6, Status::Canceled, 6.0, |i| i.parent = Some("WEB-2".into())),
            issue(7, Status::Todo, 7.0, |i| i.parent = Some("WEB-99".into())),
            issue(8, Status::Canceled, 8.0, |i| i.kind = IssueType::Feature),
            issue(9, Status::Todo, 9.0, |i| i.parent = Some("WEB-8".into())),
        ]
    }

    #[test]
    fn board_lanes_one_row_per_parent_open_parents_first_each_issue_once_no_parent_row_last() {
        let issues = lanes_sample();
        let refs: Vec<&Issue> = issues.iter().collect();
        let lanes = board_lanes(&refs, false, Sort::Default);
        // A canceled parent still heads its children's row; a parent that isn't in the project leaves its child with no parent.
        assert_eq!(lanes.iter().map(|l| l.parent.map(|p| p.key.clone())).collect::<Vec<_>>(), [Some("WEB-2".into()), Some("WEB-4".into()), Some("WEB-1".into()), Some("WEB-8".into()), None]);
        let lane_keys = |l: &Lane| l.columns.iter().flat_map(|c| keys(&c.issues)).collect::<Vec<_>>();
        assert_eq!(lanes.iter().map(lane_keys).collect::<Vec<_>>(), [vec!["WEB-4"], vec!["WEB-5"], vec!["WEB-3"], vec!["WEB-9"], vec!["WEB-7", "WEB-2", "WEB-1"]]);
        assert_eq!(lanes.iter().map(|l| l.count).collect::<Vec<_>>(), [1, 1, 1, 1, 3]);
        assert_eq!(lanes[0].columns.iter().map(|c| c.status).collect::<Vec<_>>(), Status::FLOW);
        let shown = board_lanes(&refs, true, Sort::Default);
        assert_eq!(lane_keys(&shown[0]), ["WEB-4", "WEB-6"]);
        assert_eq!(lane_keys(shown.last().unwrap()), ["WEB-7", "WEB-2", "WEB-1", "WEB-8"]);
        assert!(board_lanes(&[], false, Sort::Default).is_empty());
    }

    #[test]
    fn the_list_by_parent_a_group_per_top_level_parent_children_indented_in_tree_order_the_rest_loose() {
        let issues = [
            issue(1, Status::InProgress, 1.0, |i| i.kind = IssueType::Epic),
            issue(2, Status::Todo, 2.0, |i| { i.parent = Some("WEB-1".into()); i.kind = IssueType::Feature; }),
            issue(3, Status::Todo, 3.0, |i| i.parent = Some("WEB-2".into())),
            issue(4, Status::InReview, 4.0, |i| i.parent = Some("WEB-1".into())),
            issue(5, Status::Todo, 5.0, |_| {}),
            issue(6, Status::Canceled, 6.0, |i| i.parent = Some("WEB-1".into())),
            issue(7, Status::Done, 7.0, |i| i.kind = IssueType::Epic),
        ];
        let refs: Vec<&Issue> = issues.iter().collect();
        let (groups, loose) = issue_tree(&refs, false, Sort::Default);
        assert_eq!(groups.iter().map(|g| g.root.key.as_str()).collect::<Vec<_>>(), ["WEB-1"]);
        assert_eq!(groups[0].rows.iter().map(|(i, d)| format!("{}@{d}", i.key)).collect::<Vec<_>>(), ["WEB-4@1", "WEB-2@1", "WEB-3@2"]);
        // An epic with nothing under it is just a row, like any issue with no parent.
        assert_eq!(keys(&loose), ["WEB-5", "WEB-7"]);
        assert_eq!(issue_tree(&refs, true, Sort::Default).0[0].rows.iter().map(|(i, _)| i.key.as_str()).collect::<Vec<_>>(), ["WEB-4", "WEB-2", "WEB-3", "WEB-6"]);
    }

    #[test]
    fn the_breadcrumb_trail_adds_the_issue_you_leave_goes_back_to_one_already_on_it_and_keeps_the_last_eight() {
        let t = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(trail_to(&[], "WIN-12", "WIN-15"), t(&["WIN-12"]));
        assert_eq!(trail_to(&t(&["WIN-12"]), "WIN-15", "WIN-20"), t(&["WIN-12", "WIN-15"]));
        assert_eq!(trail_to(&t(&["WIN-12", "WIN-15"]), "WIN-20", "WIN-12"), t(&[]));
        assert_eq!(trail_to(&t(&["WIN-12", "WIN-15"]), "WIN-20", "WIN-15"), t(&["WIN-12"]));
        let long: Vec<String> = (1..=8).map(|i| format!("WIN-{i}")).collect();
        let mut expected = long[1..].to_vec();
        expected.push("WIN-9".into());
        assert_eq!(trail_to(&long, "WIN-9", "WIN-10"), expected);
    }

    #[test]
    fn the_section_puts_issues_an_agent_works_on_first_with_their_epic_then_sorts_the_rest_by_status_or_latest_change() {
        let issues = [
            issue(1, Status::Todo, 5.0, |_| {}),
            issue(2, Status::InProgress, 1.0, |_| {}),
            issue(3, Status::Todo, 0.0, |_| {}),
            issue(4, Status::Todo, 9.0, |i| i.parent = Some("WEB-5".into())),
            issue(5, Status::InReview, 2.0, |_| {}),
            issue(6, Status::Done, 99.0, |_| {}),
        ];
        let keys = |working: &[&str], by_date: bool| section_issues(&issues, |i| working.contains(&i.key.as_str()), if by_date { Sort::Updated } else { Sort::Default }).iter().map(|i| i.key.replace("WEB-", "")).collect::<Vec<_>>();
        assert_eq!(keys(&[], false), ["2", "5", "1", "3", "4"]);
        assert_eq!(keys(&[], true), ["4", "1", "5", "2", "3"]);
        assert_eq!(keys(&["WEB-4"], false), ["5", "4", "2", "1", "3"], "W-4's parent comes up with it");
        assert_eq!(keys(&["WEB-1", "WEB-3"], true), ["1", "3", "4", "5", "2"]);
    }

    /// Created and updated run opposite ways here, so neither can pass for the other or for the usual order.
    fn dated_sample() -> Vec<Issue> {
        vec![
            issue(1, Status::Todo, 5.0, |i| { i.created_at = 30.0; i.priority = Some(Priority::Low); }),
            issue(2, Status::Todo, 9.0, |i| { i.created_at = 10.0; i.priority = Some(Priority::Urgent); }),
            issue(3, Status::Todo, 1.0, |i| i.created_at = 20.0),
            issue(4, Status::InProgress, 7.0, |i| { i.created_at = 1.0; i.kind = IssueType::Epic; }),
            issue(5, Status::InProgress, 2.0, |i| { i.created_at = 40.0; i.parent = Some("WEB-4".into()); }),
            issue(6, Status::InProgress, 8.0, |i| { i.created_at = 3.0; i.parent = Some("WEB-4".into()); }),
        ]
    }

    #[test]
    fn the_sort_parses_its_values_and_maps_the_sections_orders() {
        assert!(Sort::OPTIONS.iter().all(|(value, _)| Sort::parse(value).is_some_and(|s| s.as_str() == *value)));
        assert_eq!(Sort::parse("bogus"), None);
        assert_eq!((Sort::of_section("status"), Sort::of_section("date"), Sort::of_section("created")), (Sort::Default, Sort::Updated, Sort::Created));
    }

    #[test]
    fn board_columns_by_created_or_updated_newest_first_and_the_default_keeps_priority_then_key() {
        let issues = dated_sample();
        let todo = |sort| keys(&board_columns(&issues, false, sort)[0].issues);
        assert_eq!(todo(Sort::Default), ["WEB-2", "WEB-1", "WEB-3"]);
        assert_eq!(todo(Sort::Created), ["WEB-1", "WEB-3", "WEB-2"]);
        assert_eq!(todo(Sort::Updated), ["WEB-2", "WEB-1", "WEB-3"]);
        let progress = |sort| keys(&board_columns(&issues, false, sort)[1].issues);
        assert_eq!(progress(Sort::Created), ["WEB-5", "WEB-6", "WEB-4"]);
        assert_eq!(progress(Sort::Updated), ["WEB-6", "WEB-4", "WEB-5"]);
        // The same date falls back to the oldest key.
        let tied = [issue(2, Status::Todo, 1.0, |i| i.created_at = 5.0), issue(1, Status::Todo, 1.0, |i| i.created_at = 5.0)];
        assert_eq!(keys(&board_columns(&tied, false, Sort::Created)[0].issues), ["WEB-1", "WEB-2"]);
    }

    #[test]
    fn board_lanes_follow_the_sort_inside_each_lane() {
        let issues = dated_sample();
        let refs: Vec<&Issue> = issues.iter().collect();
        let lanes = board_lanes(&refs, false, Sort::Created);
        let progress = |l: &Lane| keys(&l.columns[1].issues);
        assert_eq!(lanes.iter().map(progress).collect::<Vec<_>>(), [vec!["WEB-5", "WEB-6"], vec!["WEB-4"]]);
        assert_eq!(keys(&board_lanes(&refs, false, Sort::Updated)[0].columns[1].issues), ["WEB-6", "WEB-5"]);
        assert_eq!(keys(&board_lanes(&refs, false, Sort::Default)[0].columns[1].issues), ["WEB-6", "WEB-5"]);
    }

    #[test]
    fn the_list_sorts_siblings_by_date_inside_a_status_after_the_status_rank() {
        let issues = [
            issue(1, Status::InProgress, 1.0, |i| { i.created_at = 1.0; i.kind = IssueType::Epic; }),
            issue(2, Status::Todo, 5.0, |i| { i.created_at = 10.0; i.parent = Some("WEB-1".into()); i.priority = Some(Priority::Urgent); }),
            issue(3, Status::Todo, 9.0, |i| { i.created_at = 30.0; i.parent = Some("WEB-1".into()); }),
            issue(4, Status::Todo, 2.0, |i| { i.created_at = 20.0; i.parent = Some("WEB-1".into()); }),
            issue(5, Status::InReview, 0.0, |i| { i.created_at = 0.0; i.parent = Some("WEB-1".into()); }),
        ];
        let refs: Vec<&Issue> = issues.iter().collect();
        let rows = |sort| issue_tree(&refs, false, sort).0[0].rows.iter().map(|(i, _)| i.key.clone()).collect::<Vec<_>>();
        assert_eq!(rows(Sort::Default), ["WEB-5", "WEB-2", "WEB-3", "WEB-4"]);
        assert_eq!(rows(Sort::Created), ["WEB-5", "WEB-3", "WEB-4", "WEB-2"]);
        assert_eq!(rows(Sort::Updated), ["WEB-5", "WEB-3", "WEB-2", "WEB-4"]);
    }

    #[test]
    fn the_section_can_sort_by_creation_date_newest_first_with_working_issues_still_first() {
        let issues = [
            issue(1, Status::Todo, 50.0, |i| i.created_at = 3.0),
            issue(2, Status::InProgress, 1.0, |i| i.created_at = 9.0),
            issue(3, Status::Todo, 0.0, |i| i.created_at = 7.0),
            issue(4, Status::Done, 99.0, |i| i.created_at = 99.0),
        ];
        let keys = |working: &[&str], sort| section_issues(&issues, |i| working.contains(&i.key.as_str()), sort).iter().map(|i| i.key.replace("WEB-", "")).collect::<Vec<_>>();
        assert_eq!(keys(&[], Sort::Created), ["2", "3", "1"]);
        assert_eq!(keys(&[], Sort::Updated), ["1", "2", "3"]);
        assert_eq!(keys(&["WEB-1"], Sort::Created), ["1", "2", "3"]);
    }
}
