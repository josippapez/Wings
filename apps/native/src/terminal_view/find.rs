//! The terminal's find: the search the model thread runs over the grid, and its results.

use std::time::Duration;

use wings_terminal::{
    alacritty_terminal::{
        event::EventListener,
        grid::Dimensions,
        index::{Boundary, Column, Direction, Line, Point as GridPoint, Side},
        term::{
            Term,
            search::{Match, RegexIter, RegexSearch},
        },
    },
    grid::GridPane,
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
    pub(super) regex: RegexSearch,
    pub(super) active: Option<Match>,
    pub(super) results: FindResults,
    /// Output or a resize moved the lines since `active` was found, so it's hidden until a re-find.
    pub(super) stale: bool,
}

/// Alacritty's search is smart-case. The flag group gives the desktop's explicit match-case option instead.
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
pub(super) fn find_step<L: EventListener>(
    pane: &mut GridPane<L>,
    previous: Option<Find>,
    query: &FindQuery,
    movement: FindMove,
) -> Option<Find> {
    if query.term.is_empty() {
        return None;
    }
    let mut regex = RegexSearch::new(&find_pattern(query)).ok()?;
    let term = pane.model();
    let top = Line(-(term.grid().display_offset() as i32));
    let (origin, direction, side) = match (previous.and_then(|find| find.active), movement) {
        (Some(found), FindMove::Stay) => (*found.start(), Direction::Right, Side::Left),
        (Some(found), FindMove::Next) => (
            found.end().add(term, Boundary::None, 1),
            Direction::Right,
            Side::Left,
        ),
        (Some(found), FindMove::Previous) => (
            found.start().sub(term, Boundary::None, 1),
            Direction::Left,
            Side::Right,
        ),
        (None, FindMove::Previous) => (
            GridPoint::new(top + term.screen_lines() as i32 - 1, term.last_column()),
            Direction::Left,
            Side::Right,
        ),
        (None, _) => (GridPoint::new(top, Column(0)), Direction::Right, Side::Left),
    };
    let active = term.search_next(&mut regex, origin, direction, side, None);
    let results = count_matches(term, &mut regex, active.as_ref());
    if let Some(found) = &active {
        pane.model_mut().scroll_to_point(*found.start());
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
pub(super) fn refind<L: EventListener>(pane: &GridPane<L>, mut find: Find) -> Find {
    let term = pane.model();
    let (first, last) = whole_grid(term);
    find.active = find.results.index.and_then(|index| {
        RegexIter::new(first, last, Direction::Right, term, &mut find.regex)
            .take(index + 1)
            .last()
    });
    find.results = count_matches(term, &mut find.regex, find.active.as_ref());
    find.stale = false;
    find
}

fn whole_grid<L>(term: &Term<L>) -> (GridPoint, GridPoint) {
    (
        GridPoint::new(term.topmost_line(), Column(0)),
        GridPoint::new(term.bottommost_line(), term.last_column()),
    )
}

/// Counts up to the desktop's limit, and keeps going only to find which match is `active`.
// debt: scans the whole history on every find step, and per output batch while the bar is open.
fn count_matches<L>(
    term: &Term<L>,
    regex: &mut RegexSearch,
    active: Option<&Match>,
) -> FindResults {
    let (first, last) = whole_grid(term);
    let mut results = FindResults::default();
    for (ordinal, found) in RegexIter::new(first, last, Direction::Right, term, regex).enumerate() {
        if Some(&found) == active {
            results.index = Some(ordinal);
        }
        results.count = (ordinal + 1).min(FIND_LIMIT);
        if ordinal + 1 >= FIND_LIMIT && (results.index.is_some() || active.is_none()) {
            break;
        }
    }
    results
}

pub(super) fn visible_matches<L>(term: &Term<L>, regex: &mut RegexSearch) -> Vec<Match> {
    let top = Line(-(term.grid().display_offset() as i32));
    let start = GridPoint::new(top, Column(0));
    let end = GridPoint::new(top + term.screen_lines() as i32 - 1, term.last_column());
    RegexIter::new(start, end, Direction::Right, term, regex).collect()
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
        // Capitals don't switch on case matching, unlike Alacritty's smart case.
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
        let frame = snapshot(&pane, None, false, Some(&mut find));
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
    fn active_line(pane: &GridPane, find: &Find) -> String {
        let line = find.active.as_ref().unwrap().start().line;
        let term = pane.model();
        term.bounds_to_string(
            GridPoint::new(line, Column(0)),
            GridPoint::new(line, term.last_column()),
        )
        .trim_end()
        .to_string()
    }

    #[test]
    fn the_current_match_follows_its_text_when_output_or_a_resize_moves_it() {
        let mut pane = GridPane::new(20, 6);
        // The long line rewraps when the width changes, which moves the lines below it.
        pane.feed(b"a-line-that-wraps-at-twenty-columns\r\nfoo-x\r\nb2\r\nfoo-y\r\n");
        let foo = query("foo", false, false);
        let first = find_step(&mut pane, None, &foo, FindMove::Stay).unwrap();
        let find = find_step(&mut pane, Some(first), &foo, FindMove::Next).unwrap();
        assert_eq!(active_line(&pane, &find), "foo-y");
        pane.feed(b"c3\r\nd4\r\ne5\r\n");
        let find = refind(&pane, find);
        assert_eq!(active_line(&pane, &find), "foo-y");
        assert_eq!(
            find.results,
            FindResults {
                index: Some(1),
                count: 2
            }
        );
        pane.resize(20, 3);
        let find = refind(&pane, find);
        assert_eq!(active_line(&pane, &find), "foo-y");
        pane.resize(12, 3);
        let find = refind(&pane, find);
        assert_eq!(active_line(&pane, &find), "foo-y");
        pane.resize(40, 8);
        let find = refind(&pane, find);
        assert_eq!(active_line(&pane, &find), "foo-y");
    }
}
