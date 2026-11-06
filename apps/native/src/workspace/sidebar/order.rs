//! The order of the sidebar's projects: the desktop's `orderProjects` (`src/lib/project-order.ts`).

use std::{cmp::Ordering, collections::BTreeMap, collections::HashSet, path::Path, path::PathBuf};

use crate::projects::ProjectEntry;

/// A project nobody touched for this long goes under "Older".
pub(super) const IDLE_AFTER_MS: u64 = 3 * 24 * 60 * 60 * 1000;
/// Activity is stamped at most this often per project.
pub(super) const TOUCH_EVERY_MS: u64 = 60_000;

/// How the sidebar orders projects. Pinned ones always lead; `Manual` keeps the list's own order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Sort {
    #[default]
    Manual,
    Added,
    Worked,
    Name,
}

impl Sort {
    pub(super) const ALL: [Sort; 4] = [Sort::Manual, Sort::Added, Sort::Worked, Sort::Name];

    pub(super) fn label(self) -> &'static str {
        match self {
            Sort::Manual => "Manual",
            Sort::Added => "Added",
            Sort::Worked => "Last worked on",
            Sort::Name => "Name",
        }
    }

    /// The name in the workspace file, as the desktop app names them.
    pub(super) fn key(self) -> &'static str {
        match self {
            Sort::Manual => "manual",
            Sort::Added => "added",
            Sort::Worked => "worked",
            Sort::Name => "name",
        }
    }

    /// Anything unknown is `Manual`.
    pub(super) fn parse(key: Option<&str>) -> Sort {
        Sort::ALL
            .into_iter()
            .find(|sort| Some(sort.key()) == key)
            .unwrap_or_default()
    }
}

/// Rows by index into the entries: the listed projects, then those under "Older", each project followed by its worktrees.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Ordered {
    pub(super) shown: Vec<usize>,
    pub(super) older: Vec<usize>,
}

/// What the order depends on besides the entries.
pub(super) struct Context<'a> {
    pub(super) sort: Sort,
    pub(super) last_active: &'a BTreeMap<String, u64>,
    pub(super) now: u64,
    /// The project on screen, which is never older.
    pub(super) active: &'a Path,
    /// The projects running an agent, which are never older.
    pub(super) busy: &'a HashSet<PathBuf>,
}

/// A project is older when it isn't pinned or shown, runs no agent (nor do its worktrees) and neither it nor a worktree
/// was worked on for three days. One with no time yet is not older.
pub(super) fn order(entries: &[ProjectEntry], by: &Context) -> Ordered {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        match groups.last_mut() {
            Some(group) if entry.child => group.push(index),
            _ => groups.push(vec![index]),
        }
    }
    let activity = |group: &[usize]| {
        group
            .iter()
            .filter_map(|&row| by.last_active.get(&*entries[row].path.to_string_lossy()))
            .max()
            .copied()
    };
    let top = |group: &[usize]| &entries[group[0]];
    // A stable sort, so ties keep the list's order.
    groups.sort_by(|a, b| {
        top(b)
            .pinned
            .cmp(&top(a).pinned)
            .then_with(|| match by.sort {
                Sort::Manual => Ordering::Equal,
                Sort::Added => top(b).added.unwrap_or(0).cmp(&top(a).added.unwrap_or(0)),
                Sort::Worked => activity(b).unwrap_or(0).cmp(&activity(a).unwrap_or(0)),
                Sort::Name => natural(&top(a).name, &top(b).name),
            })
    });
    let older = |group: &Vec<usize>| {
        !top(group).pinned
            && activity(group).is_some_and(|last| by.now.saturating_sub(last) > IDLE_AFTER_MS)
            && !group
                .iter()
                .any(|&row| entries[row].path == by.active || by.busy.contains(&entries[row].path))
    };
    let (older_groups, shown_groups): (Vec<_>, Vec<_>) = groups.into_iter().partition(older);
    Ordered {
        shown: shown_groups.into_iter().flatten().collect(),
        older: older_groups.into_iter().flatten().collect(),
    }
}

/// Names in the order a person reads them: case doesn't matter and "app2" comes before "app10".
fn natural(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    let number = |chars: &mut std::iter::Peekable<std::str::Chars>| {
        let mut digits = String::new();
        while let Some(digit) = chars.next_if(char::is_ascii_digit) {
            digits.push(digit);
        }
        digits.trim_start_matches('0').to_owned()
    };
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let (x, y) = (number(&mut a), number(&mut b));
                match x.len().cmp(&y.len()).then_with(|| x.cmp(&y)) {
                    Ordering::Equal => {}
                    other => return other,
                }
            }
            (Some(x), Some(y)) => {
                a.next();
                b.next();
                match x.to_lowercase().cmp(y.to_lowercase()) {
                    Ordering::Equal => {}
                    other => return other,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 24 * 60 * 60 * 1000;
    const NOW: u64 = 100 * DAY;

    fn entry(name: &str, child: bool, pinned: bool, added: u64) -> ProjectEntry {
        ProjectEntry {
            name: name.into(),
            path: PathBuf::from("/code").join(name),
            child,
            pinned,
            branch: None,
            added: Some(added),
        }
    }

    /// `/code/<name>` -> time, for the projects worked on at that time.
    fn worked(times: &[(&str, u64)]) -> BTreeMap<String, u64> {
        times
            .iter()
            .map(|(name, time)| (format!("/code/{name}"), *time))
            .collect()
    }

    fn names(entries: &[ProjectEntry], rows: &[usize]) -> Vec<String> {
        rows.iter().map(|&row| entries[row].name.clone()).collect()
    }

    fn arrange(
        entries: &[ProjectEntry],
        sort: Sort,
        last_active: &BTreeMap<String, u64>,
        active: &str,
        busy: &[&str],
    ) -> Ordered {
        order(
            entries,
            &Context {
                sort,
                last_active,
                now: NOW,
                active: &PathBuf::from("/code").join(active),
                busy: &busy
                    .iter()
                    .map(|name| PathBuf::from("/code").join(name))
                    .collect(),
            },
        )
    }

    #[test]
    fn every_sort_orders_the_projects_and_pinned_ones_lead_in_all_of_them() {
        let entries = [
            entry("zeta", false, true, 5),
            entry("zeta-fix", true, false, 31),
            entry("beta", false, false, 30),
            entry("App10", false, false, 10),
            entry("app2", false, false, 20),
        ];
        let fresh = worked(&[
            ("beta", NOW - 1),
            ("App10", NOW - 3),
            ("app2", NOW - 2),
            ("zeta", NOW - 5),
        ]);
        let by = |sort| {
            let ordered = arrange(&entries, sort, &fresh, "beta", &[]);
            assert!(ordered.older.is_empty());
            names(&entries, &ordered.shown)
        };
        // The list's own order, and the worktree right under its project.
        assert_eq!(
            by(Sort::Manual),
            ["zeta", "zeta-fix", "beta", "App10", "app2"]
        );
        assert_eq!(
            by(Sort::Added),
            ["zeta", "zeta-fix", "beta", "app2", "App10"]
        );
        assert_eq!(
            by(Sort::Worked),
            ["zeta", "zeta-fix", "beta", "app2", "App10"]
        );
        assert_eq!(
            by(Sort::Name),
            ["zeta", "zeta-fix", "app2", "App10", "beta"]
        );
    }

    #[test]
    fn a_worktree_worked_on_last_lifts_its_project_in_last_worked_on() {
        let entries = [
            entry("a", false, false, 1),
            entry("b", false, false, 2),
            entry("b-fix", true, false, 3),
        ];
        let last_active = worked(&[("a", NOW - 9), ("b", NOW - 8), ("b-fix", NOW - 1)]);
        let ordered = arrange(&entries, Sort::Worked, &last_active, "a", &[]);
        assert_eq!(names(&entries, &ordered.shown), ["b", "b-fix", "a"]);
    }

    #[test]
    fn a_project_idle_for_three_days_goes_under_older_unless_something_keeps_it_out() {
        let entries = [
            entry("shown", false, false, 1),
            entry("pinned", false, true, 2),
            entry("agent", false, false, 3),
            entry("old", false, false, 4),
            entry("old-fix", true, false, 5),
            entry("untimed", false, false, 6),
            entry("tree-fresh", false, false, 7),
            entry("tree-fresh-fix", true, false, 8),
        ];
        let stale = NOW - IDLE_AFTER_MS - 1;
        let last_active = worked(&[
            ("shown", stale),
            ("pinned", stale),
            ("agent", stale),
            ("old", stale),
            ("old-fix", stale),
            ("tree-fresh", stale),
            ("tree-fresh-fix", NOW - DAY),
        ]);
        let ordered = arrange(&entries, Sort::Manual, &last_active, "shown", &["agent"]);
        assert_eq!(
            names(&entries, &ordered.shown),
            [
                "pinned",
                "shown",
                "agent",
                "untimed",
                "tree-fresh",
                "tree-fresh-fix"
            ]
        );
        assert_eq!(names(&entries, &ordered.older), ["old", "old-fix"]);
        // Exactly three days is not yet older.
        let edge = worked(&[("old", NOW - IDLE_AFTER_MS)]);
        let ordered = arrange(&entries[3..5], Sort::Manual, &edge, "shown", &[]);
        assert!(ordered.older.is_empty());
    }
}
