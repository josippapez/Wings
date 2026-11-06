//! The terminal's find: the search the model thread runs over the grid, and its results.

use std::{ops::ControlFlow, time::Duration};

use wings_terminal::{
    grid::{GridPane, Point},
    search::{Match, Regex},
};

/// The desktop find bar's options (`FindOptions` in the desktop terminal).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct FindQuery {
    pub(super) term: String,
    pub(super) case_sensitive: bool,
    pub(super) regex: bool,
}

/// Where a find goes from the current match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FindMove {
    /// Typing: keep the current match while it still matches.
    Stay,
    Next,
    Previous,
}

/// The desktop find bar counts at most this many matches and shows "1000+".
pub(super) const FIND_LIMIT: usize = 1000;
/// A re-find scans the whole history (about 20 ms for 10,000 lines that all match), so output
/// triggers one at most this often, and once more when output has been quiet for `FIND_SETTLE`.
pub(super) const REFIND_EVERY: Duration = Duration::from_millis(100);
pub(super) const FIND_SETTLE: Duration = Duration::from_millis(150);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct FindResults {
    pub(super) index: Option<usize>,
    pub(super) count: usize,
}

pub(super) struct Find {
    pub(super) regex: Regex,
    pub(super) active: Option<Match>,
    pub(super) results: FindResults,
    /// Output or a resize moved the lines since `active` was found, so it's hidden until a re-find.
    pub(super) stale: bool,
}

/// The flag group gives the desktop's explicit match-case option.
fn find_pattern(query: &FindQuery) -> String {
    let flag = if query.case_sensitive {
        "(?-i)"
    } else {
        "(?i)"
    };
    if query.regex {
        return format!("{flag}{}", query.term);
    }
    let mut pattern = flag.to_string();
    for c in query.term.chars() {
        if "\\.+*?()|[]{}^$#&-~".contains(c) {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern
}

/// Moves to a match like the desktop find bar. An empty term or a regex that doesn't parse finds nothing.
pub(super) fn find_step(
    pane: &mut GridPane,
    previous: Option<Find>,
    query: &FindQuery,
    movement: FindMove,
) -> Option<Find> {
    if query.term.is_empty() {
        return None;
    }
    let regex = Regex::new(&find_pattern(query)).ok()?;
    let top = pane.viewport_top();
    // From where the search goes, forwards or backwards, wrapping around the ends.
    let target = match (previous.and_then(|find| find.active), movement) {
        (Some(found), FindMove::Stay) => Target::From(found.start),
        (Some(found), FindMove::Next) => Target::After(found.end),
        (Some(found), FindMove::Previous) => Target::Before(found.start),
        (None, FindMove::Previous) => Target::Before(Point {
            row: top + pane.screen_lines(),
            col: 0,
        }),
        (None, _) => Target::From(Point { row: top, col: 0 }),
    };
    let (active, results) = scan(pane, &regex, target);
    if let Some(found) = &active {
        pane.scroll_to_point(found.start);
    }
    Some(Find {
        regex,
        active,
        results,
        stale: false,
    })
}

/// Output or a resize moved the lines. Rewrapping changes line numbers but not the order of the
/// matches, so the current match is found again by its place in that order, without scrolling.
pub(super) fn refind(pane: &mut GridPane, mut find: Find) -> Find {
    let (active, results) = match find.results.index {
        Some(index) => scan(pane, &find.regex, Target::Nth(index)),
        None => (None, scan(pane, &find.regex, Target::None).1),
    };
    find.active = active;
    find.results = results;
    find.stale = false;
    find
}

/// Which match a scan makes current.
#[derive(Clone, Copy)]
enum Target {
    None,
    /// The first that starts at or after the point.
    From(Point),
    /// The first that starts after the point.
    After(Point),
    /// The last that starts before the point.
    Before(Point),
    /// The match at this place in the order.
    Nth(usize),
}

/// Finds the `target` match and counts up to the desktop's limit, going on past it only to find the target's place.
// debt: scans the whole history on every find step, and per output batch while the bar is open.
fn scan(pane: &mut GridPane, regex: &Regex, target: Target) -> (Option<Match>, FindResults) {
    let mut results = FindResults::default();
    let mut active: Option<(usize, Match)> = None;
    // A target past the last match or before the first wraps around to the first or the last.
    let (mut first, mut last): (Option<Match>, Option<(usize, Match)>) = (None, None);
    let mut seen = 0;
    pane.search(regex, 0..usize::MAX, |found| {
        let ordinal = seen;
        seen += 1;
        first.get_or_insert(found);
        last = Some((ordinal, found));
        let decided = match target {
            Target::None => true,
            Target::From(point) | Target::After(point) => {
                let hit = if matches!(target, Target::From(_)) {
                    found.start >= point
                } else {
                    found.start > point
                };
                if hit && active.is_none() {
                    active = Some((ordinal, found));
                }
                active.is_some()
            }
            Target::Before(point) => {
                if found.start < point {
                    active = Some((ordinal, found));
                }
                active.is_some() && found.start >= point
            }
            Target::Nth(index) => {
                if ordinal == index {
                    active = Some((ordinal, found));
                }
                active.is_some()
            }
        };
        results.count = seen.min(FIND_LIMIT);
        if seen >= FIND_LIMIT && decided {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    });
    let active = active.or(match target {
        Target::From(_) | Target::After(_) => first.map(|found| (0, found)),
        Target::Before(_) => last,
        Target::None | Target::Nth(_) => None,
    });
    results.index = active.map(|(ordinal, _)| ordinal);
    (active.map(|(_, found)| found), results)
}

/// The matches on screen, and on the lines that go on to it from up to `REACH` rows above it.
pub(super) fn visible_matches(pane: &mut GridPane, regex: &Regex) -> Vec<Match> {
    const REACH: usize = 100;
    let top = pane.viewport_top();
    let bottom = top + pane.screen_lines();
    let mut matches = Vec::new();
    pane.search(regex, top.saturating_sub(REACH)..bottom, |found| {
        if found.end.row >= top && found.start.row < bottom {
            matches.push(found);
        }
        ControlFlow::Continue(())
    });
    matches
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal_view::snapshot::{Found, snapshot};

    fn query(term: &str, case_sensitive: bool, regex: bool) -> FindQuery {
        FindQuery {
            term: term.into(),
            case_sensitive,
            regex,
        }
    }

    #[test]
    fn find_pattern_escapes_plain_text_and_sets_case() {
        assert_eq!(
            find_pattern(&query("a.b(c)", false, false)),
            r"(?i)a\.b\(c\)"
        );
        assert_eq!(find_pattern(&query("a.b", true, true)), "(?-i)a.b");
    }

    #[test]
    fn find_counts_moves_and_marks_matches_like_the_desktop_bar() {
        let mut pane = GridPane::new(20, 4);
        pane.feed(b"Foo foo\r\nbar foo\r\nFOO");
        let step = |pane: &mut GridPane, previous, term, case_sensitive, regex, movement| {
            find_step(
                pane,
                previous,
                &query(term, case_sensitive, regex),
                movement,
            )
        };
        // Capitals don't switch on case matching: there is no smart case.
        let first = step(&mut pane, None, "Foo", false, false, FindMove::Stay).unwrap();
        assert_eq!(
            first.results,
            FindResults {
                index: Some(0),
                count: 4
            }
        );
        let second = step(&mut pane, Some(first), "Foo", false, false, FindMove::Next).unwrap();
        assert_eq!(second.results.index, Some(1));
        let first = step(&mut pane, None, "foo", false, false, FindMove::Stay).unwrap();
        let wrapped = step(
            &mut pane,
            Some(first),
            "foo",
            false,
            false,
            FindMove::Previous,
        );
        assert_eq!(wrapped.unwrap().results.index, Some(3));
        let count = |pane: &mut GridPane, term, case_sensitive, regex| {
            step(pane, None, term, case_sensitive, regex, FindMove::Stay)
                .map(|find| find.results.count)
        };
        assert_eq!(count(&mut pane, "Foo", true, false), Some(1));
        assert_eq!(count(&mut pane, "f.o", false, true), Some(4));
        assert_eq!(count(&mut pane, "f.o", false, false), Some(0));
        assert_eq!(count(&mut pane, "(", false, true), None);
        assert_eq!(count(&mut pane, "", false, false), None);

        let mut find = step(&mut pane, None, "foo", false, false, FindMove::Stay).unwrap();
        let frame = snapshot(&mut pane, None, false, Some(&mut find));
        let found = |row: usize, col: usize| frame.cells[row * 20 + col].found;
        assert_eq!((found(0, 0), found(0, 2)), (Found::Active, Found::Active));
        assert_eq!(
            (found(0, 3), found(0, 4), found(1, 6)),
            (Found::No, Found::Match, Found::Match)
        );
        assert_eq!(
            frame.find,
            Some(FindResults {
                index: Some(0),
                count: 4
            })
        );
    }

    /// The whole line the current match is on, wherever it is in the history.
    fn active_line(pane: &mut GridPane, find: &Find) -> String {
        let line = find.active.as_ref().unwrap().start.row;
        let mut text = String::new();
        pane.for_each_row(|row, cells| {
            if row == line {
                text = cells.iter().map(|cell| cell.text.as_str()).collect();
            }
        });
        text.trim_end().to_string()
    }

    #[test]
    fn the_current_match_follows_its_text_when_output_or_a_resize_moves_it() {
        let mut pane = GridPane::new(20, 6);
        // The long line rewraps when the width changes, which moves the lines below it.
        pane.feed(b"a-line-that-wraps-at-twenty-columns\r\nfoo-x\r\nb2\r\nfoo-y\r\n");
        let foo = query("foo", false, false);
        let first = find_step(&mut pane, None, &foo, FindMove::Stay).unwrap();
        let find = find_step(&mut pane, Some(first), &foo, FindMove::Next).unwrap();
        assert_eq!(active_line(&mut pane, &find), "foo-y");
        pane.feed(b"c3\r\nd4\r\ne5\r\n");
        let find = refind(&mut pane, find);
        assert_eq!(active_line(&mut pane, &find), "foo-y");
        assert_eq!(
            find.results,
            FindResults {
                index: Some(1),
                count: 2
            }
        );
        pane.resize(20, 3);
        let find = refind(&mut pane, find);
        assert_eq!(active_line(&mut pane, &find), "foo-y");
        pane.resize(12, 3);
        let find = refind(&mut pane, find);
        assert_eq!(active_line(&mut pane, &find), "foo-y");
        pane.resize(40, 8);
        let find = refind(&mut pane, find);
        assert_eq!(active_line(&mut pane, &find), "foo-y");
    }
}
