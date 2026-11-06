//! Regex search over the kept history and the screen, a line at a time: rows that wrap are one line, so a match
//! can go on to the next row, as in Alacritty's search.

use std::ops::{ControlFlow, Range};

pub use regex::Regex;

use crate::grid::{Cell, Flags, GridPane, Point};

/// A match from its first cell to its last, both included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Match {
    pub start: Point,
    pub end: Point,
}

impl Match {
    pub fn contains(&self, point: Point) -> bool {
        self.start <= point && point <= self.end
    }
}

/// One line being put together from its rows: its text and the cell each character starts at.
#[derive(Default)]
struct Line {
    text: String,
    /// The byte offset in `text` where each cell's characters start, with its place.
    cells: Vec<(usize, Point)>,
}

impl Line {
    fn push_row(&mut self, row: usize, cells: &[Cell]) {
        for (col, cell) in cells.iter().enumerate() {
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            self.cells.push((self.text.len(), Point { row, col }));
            if cell.text.is_empty() {
                self.text.push(' ');
            } else {
                self.text.push_str(&cell.text);
            }
        }
    }

    /// The cell the byte at `offset` belongs to.
    fn point(&self, offset: usize) -> Point {
        let i = self.cells.partition_point(|(start, _)| *start <= offset);
        self.cells[i.saturating_sub(1)].1
    }

    fn matches(
        &mut self,
        regex: &Regex,
        f: &mut impl FnMut(Match) -> ControlFlow<()>,
    ) -> ControlFlow<()> {
        for found in regex.find_iter(&self.text) {
            if found.is_empty() {
                continue;
            }
            f(Match {
                start: self.point(found.start()),
                end: self.point(found.end() - 1),
            })?;
        }
        self.text.clear();
        self.cells.clear();
        ControlFlow::Continue(())
    }
}

impl GridPane {
    /// Calls `f` with each match of `regex` on the lines that start in `rows`, in reading order, until it breaks.
    /// A line that starts in `rows` is read to its end, past `rows` if it wraps on.
    pub fn search(
        &mut self,
        regex: &Regex,
        rows: Range<usize>,
        mut f: impl FnMut(Match) -> ControlFlow<()>,
    ) {
        let mut line = Line::default();
        let mut wrapped = false;
        let mut done = false;
        self.for_each_row_in(rows.start..usize::MAX, |row, cells| {
            if row >= rows.end && !wrapped {
                return ControlFlow::Break(());
            }
            line.push_row(row, cells);
            wrapped = cells
                .last()
                .is_some_and(|cell| cell.flags.contains(Flags::WRAPLINE));
            if wrapped {
                return ControlFlow::Continue(());
            }
            let result = line.matches(regex, &mut f);
            done = result.is_break();
            result
        });
        if !done && !line.text.is_empty() {
            let _ = line.matches(regex, &mut f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(pane: &mut GridPane, pattern: &str, rows: Range<usize>) -> Vec<Match> {
        let regex = Regex::new(pattern).unwrap();
        let mut out = Vec::new();
        pane.search(&regex, rows, |found| {
            out.push(found);
            ControlFlow::Continue(())
        });
        out
    }

    fn at(row: usize, col: usize) -> Point {
        Point { row, col }
    }

    #[test]
    fn matches_cross_wrapped_rows_and_skip_wide_spacers() {
        let mut pane = GridPane::new(10, 4);
        pane.feed("abc 界x foo\r\nxx-foo-bar\r\n".as_bytes());
        // "abc 界x fo" fills the first row (界 takes two cells), "o" wraps.
        let found = all(&mut pane, "foo", 0..usize::MAX);
        assert_eq!(
            found,
            vec![
                Match {
                    start: at(0, 8),
                    end: at(1, 0)
                },
                Match {
                    start: at(2, 3),
                    end: at(2, 5)
                },
            ]
        );
        assert_eq!(
            all(&mut pane, "界x", 0..usize::MAX),
            vec![Match {
                start: at(0, 4),
                end: at(0, 6)
            }]
        );
        // A line that starts in the range is read to its end.
        assert_eq!(all(&mut pane, "foo", 0..1).len(), 1);
        assert_eq!(all(&mut pane, "foo", 2..3).len(), 1);
    }

    #[test]
    fn a_break_stops_the_search() {
        let mut pane = GridPane::new(10, 4);
        pane.feed(b"a a a\r\na a");
        let regex = Regex::new("a").unwrap();
        let mut seen = 0;
        pane.search(&regex, 0..usize::MAX, |_| {
            seen += 1;
            if seen == 2 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        });
        assert_eq!(seen, 2);
    }
}
