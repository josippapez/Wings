//! Where the graph draws each commit.

use crate::log::Commit;

/// Lanes past this are cut off: a repo rarely has this many branches side by side.
pub const MAX_LANES: usize = 24;

/// A commit and where the graph draws it. `lines` holds four numbers a line: from lane, to lane, part and colour,
/// where part 0 runs from the row's top into the dot, 1 from the dot to the row's bottom, and 2 straight through.
#[derive(Clone, Debug)]
pub struct Placed {
    pub commit: Commit,
    pub lane: usize,
    pub color: usize,
    pub lines: Vec<usize>,
}

/// Gives each commit a lane, top to bottom in topological order. Each lane waits for one hash: a commit takes the
/// lane waiting for it (the others waiting for it end there), and hands it to its first parent. Other parents get a
/// free lane unless one already waits for them. Lanes keep their place, so lines only bend at merges and forks.
pub fn layout(commits: Vec<Commit>) -> (Vec<Placed>, usize) {
    let mut lanes: Vec<Option<String>> = Vec::new();
    let mut colors: Vec<usize> = Vec::new();
    let (mut next, mut widest) = (0, 1);
    fn free(lanes: &[Option<String>]) -> usize {
        lanes.iter().position(Option::is_none).unwrap_or(lanes.len())
    }
    fn put<T: Clone>(list: &mut Vec<T>, at: usize, value: T, empty: T) {
        if list.len() <= at {
            list.resize(at + 1, empty);
        }
        list[at] = value;
    }
    let rows = commits
        .into_iter()
        .map(|commit| {
            let waiting = |lanes: &[Option<String>], hash: &str| lanes.iter().position(|lane| lane.as_deref() == Some(hash));
            let lane = waiting(&lanes, &commit.hash).unwrap_or_else(|| {
                // A branch tip: nothing above leads to it, so it starts in a free lane with a colour of its own.
                let lane = free(&lanes);
                put(&mut colors, lane, next, 0);
                next += 1;
                lane
            });
            let color = colors[lane];
            let mut lines = Vec::new();
            for j in 0..lanes.len() {
                if lanes[j].as_deref() == Some(&commit.hash) {
                    lines.extend([j, lane, 0, colors[j]]);
                    lanes[j] = None;
                } else if lanes[j].is_some() {
                    lines.extend([j, j, 2, colors[j]]);
                }
            }
            if lane < lanes.len() {
                lanes[lane] = None;
            }
            for (k, parent) in commit.parents.iter().enumerate() {
                let at = waiting(&lanes, parent).unwrap_or_else(|| {
                    let at = if k == 0 { lane } else { free(&lanes) };
                    put(&mut lanes, at, Some(parent.clone()), None);
                    put(&mut colors, at, if k == 0 { color } else { next }, 0);
                    if k != 0 {
                        next += 1;
                    }
                    at
                });
                lines.extend([lane, at, 1, colors[at]]);
            }
            while lanes.last().is_some_and(Option::is_none) {
                lanes.pop();
            }
            widest = widest.max(lane + 1).max(lanes.len());
            Placed { commit, lane, color, lines }
        })
        .collect();
    (rows, widest.min(MAX_LANES))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::{parse_log, tests::log_of};

    #[test]
    fn a_merge_takes_a_second_lane_for_its_other_parent_which_joins_the_first_at_the_fork() {
        // m merges b into a's line. Both come from r.
        let (rows, lanes) = layout(parse_log(&log_of(&[("m", "a b", "", "merge"), ("a", "r", "", "a"), ("b", "r", "", "b"), ("r", "", "", "root")])));
        assert_eq!(lanes, 2);
        assert_eq!(rows.iter().map(|row| (row.lane, row.color)).collect::<Vec<_>>(), [(0, 0), (0, 0), (1, 1), (0, 0)]);
        // The merge: down its own lane to a, and across to a new lane for b.
        assert_eq!(rows[0].lines, [0, 0, 1, 0, 0, 1, 1, 1]);
        // a: its lane comes in, b's runs through, and its lane goes on to r.
        assert_eq!(rows[1].lines, [0, 0, 0, 0, 1, 1, 2, 1, 0, 0, 1, 0]);
        // b: a's lane runs through, its own comes in, and it joins the lane that already waits for r.
        assert_eq!(rows[2].lines, [0, 0, 2, 0, 1, 1, 0, 1, 1, 0, 1, 0]);
        // r: the one lane left ends in it.
        assert_eq!(rows[3].lines, [0, 0, 0, 0]);
        assert!(layout(Vec::new()).0.is_empty());
    }
}
