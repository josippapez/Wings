//! A hidden pane gives its scrollback back, like the desktop terminal does with xterm.js's serializer:
//! the lines above the screen become text and style codes, a small fraction of the grid's cells, and
//! go back above the screen when the pane is shown again.

use std::fmt::Write;

use alacritty_terminal::{
    event::EventListener,
    grid::{Dimensions, GridCell, Row, Scroll},
    index::{Column, Line},
    term::cell::{Cell, Flags},
    vte::ansi::{Color, NamedColor},
};

use crate::grid::{GridPane, HISTORY_LINES};

/// Fewer lines than this save too little to be worth storing (the desktop's `MIN_HISTORY_LINES`).
pub const MIN_LINES: usize = 500;

/// The attributes written back, with their SGR parameters.
const ATTRIBUTES: [(Flags, &str); 11] = [
    (Flags::BOLD, "1"),
    (Flags::DIM, "2"),
    (Flags::ITALIC, "3"),
    (Flags::UNDERLINE, "4"),
    (Flags::DOUBLE_UNDERLINE, "4:2"),
    (Flags::UNDERCURL, "4:3"),
    (Flags::DOTTED_UNDERLINE, "4:4"),
    (Flags::DASHED_UNDERLINE, "4:5"),
    (Flags::INVERSE, "7"),
    (Flags::HIDDEN, "8"),
    (Flags::STRIKEOUT, "9"),
];

#[derive(Clone, Copy, PartialEq)]
struct Style {
    fg: Color,
    bg: Color,
    flags: Flags,
    underline: Option<Color>,
}

impl Style {
    const PLAIN: Style = Style {
        fg: Color::Named(NamedColor::Foreground),
        bg: Color::Named(NamedColor::Background),
        flags: Flags::empty(),
        underline: None,
    };

    fn of(cell: &Cell) -> Style {
        let attributes = ATTRIBUTES.iter().fold(Flags::empty(), |all, (flag, _)| all | *flag);
        Style {
            fg: cell.fg,
            bg: cell.bg,
            flags: cell.flags & attributes,
            underline: cell.underline_color(),
        }
    }

    fn write(&self, out: &mut String) {
        out.push_str("\x1b[0");
        for (flag, code) in ATTRIBUTES {
            if self.flags.contains(flag) {
                out.push(';');
                out.push_str(code);
            }
        }
        color(out, self.fg, 30, 90, 38);
        color(out, self.bg, 40, 100, 48);
        // The underline color has no short codes, so the 16 named colors go by their palette index.
        if let Some(underline) = self.underline {
            let underline = match underline {
                Color::Named(name) if (name as usize) < 16 => Color::Indexed(name as u8),
                other => other,
            };
            color(out, underline, 0, 0, 58);
        }
        out.push('m');
    }
}

/// Named colors past the 16 are the defaults, which the reset before them already set.
fn color(out: &mut String, color: Color, base: u8, bright: u8, extended: u8) {
    let _ = match color {
        Color::Named(name) if (name as usize) < 8 => write!(out, ";{}", base + name as u8),
        Color::Named(name) if (name as usize) < 16 => write!(out, ";{}", bright + name as u8 - 8),
        Color::Named(_) => Ok(()),
        Color::Indexed(index) => write!(out, ";{extended};5;{index}"),
        Color::Spec(rgb) => write!(out, ";{extended};2;{};{};{}", rgb.r, rgb.g, rgb.b),
    };
}

/// Cells up to the last one that shows something.
fn length(row: &Row<Cell>) -> usize {
    (0..row.len())
        .rev()
        .find(|&col| !row[Column(col)].is_empty())
        .map_or(0, |col| col + 1)
}

impl<L: EventListener> GridPane<L> {
    /// Writes the lines above the screen as bytes that draw them again, and drops them.
    pub fn store_history(&mut self) -> Vec<u8> {
        let grid = self.model().grid();
        let history = grid.history_size() as i32;
        let mut out = String::new();
        let mut style = Style::PLAIN;
        let mut continued = false;
        for line in -history..0 {
            let row = &grid[Line(line)];
            let wraps = row.last().is_some_and(|cell| cell.flags.contains(Flags::WRAPLINE));
            // A wrapped line fills its row so the terminal wraps it again. The line it continues on keeps
            // at least one cell, or the two would merge into one.
            let end = if wraps {
                row.len()
            } else {
                length(row).max(usize::from(continued))
            };
            for col in 0..end {
                let cell = &row[Column(col)];
                if cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }
                let next = Style::of(cell);
                if next != style {
                    next.write(&mut out);
                    style = next;
                }
                // A tab keeps its character for copying. Writing it jumps to the next tab stop, so the
                // cursor comes back to the next cell.
                if cell.c == '\t' {
                    let _ = write!(out, "\t\x1b[{}G", col + 2);
                } else {
                    out.push(cell.c);
                }
                out.extend(cell.zerowidth().into_iter().flatten());
            }
            if !wraps && line < -1 {
                // A new line takes the current background, so the style ends first.
                if style != Style::PLAIN {
                    out.push_str("\x1b[0m");
                    style = Style::PLAIN;
                }
                out.push_str("\r\n");
            }
            continued = wraps;
        }
        self.model_mut().grid_mut().clear_history();
        out.into_bytes()
    }

    /// Puts stored lines back above the screen and the history that came after them. Past the
    /// history limit, the oldest stored lines are left out.
    pub fn restore_history(&mut self, stored: &[u8]) {
        if stored.is_empty() {
            return;
        }
        let (cols, rows) = (self.model().columns(), self.model().screen_lines());
        let mut scratch = GridPane::new(cols, rows);
        scratch.feed(stored);
        let source = scratch.model().grid();
        let last = source.cursor.point.line.0;
        let first = -(source.history_size() as i32);
        let grid = self.model_mut().grid_mut();
        let kept = grid.history_size() as i32;
        let count = ((last - first + 1) as usize).min(HISTORY_LINES - kept as usize) as i32;
        if count == 0 {
            return;
        }
        // Scrolling the whole screen up makes `count` lines of room in the history. Everything moves
        // back down by as much, so the room is above the kept history and the screen stays put.
        grid.scroll_up(&(Line(0)..Line(rows as i32)), count as usize);
        for line in (-kept..rows as i32).rev() {
            grid[Line(line)] = std::mem::replace(&mut grid[Line(line - count)], Row::new(cols));
        }
        for (offset, line) in (last + 1 - count..=last).enumerate() {
            grid[Line(-kept - count + offset as i32)] = source[Line(line)].clone();
        }
        grid.scroll_display(Scroll::Bottom);
        self.model_mut().selection = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each line's cells as text and style, without the wrap flag: the seam between stored and kept
    /// lines starts a new line, as in the desktop terminal.
    fn lines(pane: &GridPane) -> Vec<String> {
        let grid = pane.model().grid();
        let top = -(grid.history_size() as i32);
        (top..grid.screen_lines() as i32)
            .map(|line| {
                let row = &grid[Line(line)];
                (0..row.len())
                    .map(|col| {
                        let cell = &row[Column(col)];
                        let flags = cell.flags - Flags::WRAPLINE;
                        format!("{}{:?}{:?}{:?}{:?}", cell.c, cell.fg, cell.bg, flags, cell.zerowidth())
                    })
                    .collect()
            })
            .collect()
    }

    fn output(lines: std::ops::Range<usize>) -> Vec<u8> {
        let mut out = String::new();
        for i in lines {
            match i % 5 {
                0 => write!(out, "\x1b[1;31mred {i}\x1b[0m plain\r\n"),
                1 => write!(out, "\x1b[38;5;208;48;2;10;20;30mcolors {i}\x1b[0m\r\n"),
                2 => write!(out, "wide 漢字 {i} e\u{301}\x1b[4:3;58;5;1m curl\x1b[0m\r\n"),
                3 => write!(out, "{}{i}\r\n", "long ".repeat(30)),
                _ => write!(out, "\x1b[7m inverse {i}\x1b[0m\ttab\r\n\r\n"),
            }
            .unwrap();
        }
        out.into_bytes()
    }

    #[test]
    fn stored_history_comes_back_above_what_arrived_while_it_was_away() {
        let mut stored = GridPane::new(40, 10);
        let mut kept = GridPane::new(40, 10);
        stored.feed(&output(0..800));
        kept.feed(&output(0..800));
        let lines_before = stored.model().grid().history_size();
        let bytes = stored.store_history();
        assert_eq!(stored.model().grid().history_size(), 0);
        assert!(
            bytes.len() < lines_before * 40 * 24 / 10,
            "{} bytes for {lines_before} lines",
            bytes.len()
        );

        stored.feed(&output(800..900));
        kept.feed(&output(800..900));
        stored.restore_history(&bytes);
        assert_eq!(lines(&stored), lines(&kept));
        assert_eq!(stored.model().grid().cursor.point, kept.model().grid().cursor.point);
    }

    #[test]
    fn a_restore_past_the_history_limit_keeps_the_newest_lines() {
        let mut pane = GridPane::new(20, 5);
        pane.feed(
            &(0..6000)
                .flat_map(|i| format!("old {i}\r\n").into_bytes())
                .collect::<Vec<_>>(),
        );
        let bytes = pane.store_history();
        pane.feed(
            &(0..6000)
                .flat_map(|i| format!("new {i}\r\n").into_bytes())
                .collect::<Vec<_>>(),
        );
        pane.restore_history(&bytes);
        let grid = pane.model().grid();
        assert_eq!(grid.history_size(), HISTORY_LINES);
        let text = |line: i32| (0..9).map(|col| grid[Line(line)][Column(col)].c).collect::<String>();
        assert_eq!(text(-(HISTORY_LINES as i32)).trim(), "old 1996");
        assert_eq!(text(-1).trim(), "new 5995");
    }
}
