//! How issues are ordered and grouped for the board, the list, the short list and the breadcrumb.

use crate::domain::{Issue, Status, key_number, priority_rank};
use std::{cmp::Ordering, collections::{HashMap, HashSet}};

fn cmp_key(a: &Issue, b: &Issue) -> Ordering {
    key_number(&a.key).cmp(&key_number(&b.key))
}

fn newest_first(a: &Issue, b: &Issue) -> Ordering {
    b.updated_at.total_cmp(&a.updated_at)
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
/// does), then the rest, each part by status like `open_issues` or by latest change, newest first.
pub fn section_issues<'a>(issues: &'a [Issue], working: impl Fn(&Issue) -> bool, by_date: bool) -> Vec<&'a Issue> {
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
    if by_date {
        sorted.sort_by(|a, b| newest_first(a, b));
    }
    let (first, rest): (Vec<&Issue>, Vec<&Issue>) = sorted.into_iter().partition(|i| active.contains(i.key.as_str()));
    first.into_iter().chain(rest).collect()
}

pub struct Column<'a> {
    pub status: Status,
    pub issues: Vec<&'a Issue>,
}

/// The board's columns: highest priority first, then to do by key and the rest by latest activity. Canceled only when asked for.
pub fn board_columns<'a, I>(issues: I, show_canceled: bool) -> Vec<Column<'a>>
where
    I: IntoIterator<Item = &'a Issue> + Clone,
{
    let statuses: &[Status] = if show_canceled { &Status::ALL } else { &Status::FLOW };
    statuses
        .iter()
        .map(|&status| {
            let mut column: Vec<&Issue> = issues.clone().into_iter().filter(|i| i.status == status).collect();
            column.sort_by(|a, b| priority_rank(a.priority).cmp(&priority_rank(b.priority)).then_with(|| if status == Status::Todo { cmp_key(a, b) } else { newest_first(a, b) }));
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
pub fn board_lanes<'a>(issues: &[&'a Issue], show_canceled: bool) -> Vec<Lane<'a>> {
    let by_key: HashMap<&str, &'a Issue> = issues.iter().map(|i| (i.key.as_str(), *i)).collect();
    let mut groups: HashMap<&str, Vec<&'a Issue>> = HashMap::new();
    for &i in issues {
        if i.status == Status::Canceled && !show_canceled {
            continue;
        }
        let parent = i.parent.as_deref().filter(|p| by_key.contains_key(p)).unwrap_or("");
        groups.entry(parent).or_default().push(i);
    }
    let lane = |parent: Option<&'a Issue>, members: &Vec<&'a Issue>| Lane { parent, columns: board_columns(members.iter().copied(), show_canceled), count: members.len() };
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
/// below it in tree order and indented by its depth (1 for a child), siblings by status (closest to done first), priority,
/// then key. `loose` is the top-level issues with no children. Canceled ones only when asked for.
pub fn issue_tree<'a>(issues: &[&'a Issue], show_canceled: bool) -> (Vec<TreeGroup<'a>>, Vec<&'a Issue>) {
    let shown: Vec<&Issue> = issues.iter().copied().filter(|i| show_canceled || i.status != Status::Canceled).collect();
    let keys: HashSet<&str> = shown.iter().map(|i| i.key.as_str()).collect();
    let rank = |s: Status| [Status::InReview, Status::InProgress, Status::Todo, Status::Done, Status::Canceled].iter().position(|x| *x == s).unwrap_or(0);
    let sort = |list: &mut Vec<&'a Issue>| list.sort_by(|a, b| rank(a.status).cmp(&rank(b.status)).then_with(|| priority_rank(a.priority).cmp(&priority_rank(b.priority))).then_with(|| cmp_key(a, b)));
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
    use crate::domain::IssueType;

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
        let columns = board_columns(&issues, false);
        assert_eq!(columns.iter().map(|c| c.status).collect::<Vec<_>>(), Status::FLOW);
        assert_eq!(columns.iter().map(|c| keys(&c.issues)).collect::<Vec<_>>(), [vec!["WEB-1", "WEB-2"], vec!["WEB-4", "WEB-3"], vec!["WEB-7"], vec!["WEB-5"]]);
        assert_eq!(board_columns(&issues, true).last().unwrap().status, Status::Canceled);
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
        let lanes = board_lanes(&refs, false);
        // A canceled parent still heads its children's row; a parent that isn't in the project leaves its child with no parent.
        assert_eq!(lanes.iter().map(|l| l.parent.map(|p| p.key.clone())).collect::<Vec<_>>(), [Some("WEB-2".into()), Some("WEB-4".into()), Some("WEB-1".into()), Some("WEB-8".into()), None]);
        let lane_keys = |l: &Lane| l.columns.iter().flat_map(|c| keys(&c.issues)).collect::<Vec<_>>();
        assert_eq!(lanes.iter().map(lane_keys).collect::<Vec<_>>(), [vec!["WEB-4"], vec!["WEB-5"], vec!["WEB-3"], vec!["WEB-9"], vec!["WEB-7", "WEB-2", "WEB-1"]]);
        assert_eq!(lanes.iter().map(|l| l.count).collect::<Vec<_>>(), [1, 1, 1, 1, 3]);
        assert_eq!(lanes[0].columns.iter().map(|c| c.status).collect::<Vec<_>>(), Status::FLOW);
        let shown = board_lanes(&refs, true);
        assert_eq!(lane_keys(&shown[0]), ["WEB-4", "WEB-6"]);
        assert_eq!(lane_keys(shown.last().unwrap()), ["WEB-7", "WEB-2", "WEB-1", "WEB-8"]);
        assert!(board_lanes(&[], false).is_empty());
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
        let (groups, loose) = issue_tree(&refs, false);
        assert_eq!(groups.iter().map(|g| g.root.key.as_str()).collect::<Vec<_>>(), ["WEB-1"]);
        assert_eq!(groups[0].rows.iter().map(|(i, d)| format!("{}@{d}", i.key)).collect::<Vec<_>>(), ["WEB-4@1", "WEB-2@1", "WEB-3@2"]);
        // An epic with nothing under it is just a row, like any issue with no parent.
        assert_eq!(keys(&loose), ["WEB-5", "WEB-7"]);
        assert_eq!(issue_tree(&refs, true).0[0].rows.iter().map(|(i, _)| i.key.as_str()).collect::<Vec<_>>(), ["WEB-4", "WEB-2", "WEB-3", "WEB-6"]);
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
        let keys = |working: &[&str], by_date: bool| section_issues(&issues, |i| working.contains(&i.key.as_str()), by_date).iter().map(|i| i.key.replace("WEB-", "")).collect::<Vec<_>>();
        assert_eq!(keys(&[], false), ["2", "5", "1", "3", "4"]);
        assert_eq!(keys(&[], true), ["4", "1", "5", "2", "3"]);
        assert_eq!(keys(&["WEB-4"], false), ["5", "4", "2", "1", "3"], "W-4's parent comes up with it");
        assert_eq!(keys(&["WEB-1", "WEB-3"], true), ["1", "3", "4", "5", "2"]);
    }
}
