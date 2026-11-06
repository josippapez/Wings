//! A hidden pane gives its scrollback back, like the desktop terminal does with xterm.js's serializer:
//! the lines above the screen become text and style codes, a small fraction of the grid's cells, and
//! go back above the screen when the pane is shown again.

use ghostty_sys::*;

use crate::grid::GridPane;

/// Fewer lines than this save too little to be worth storing (the desktop's `MIN_HISTORY_LINES`).
pub const MIN_LINES: usize = 500;

impl GridPane {
    /// Writes the lines above the screen as bytes that draw them again, and drops them. `None` while the program is
    /// in the middle of a sequence, which the clearing one would break.
    pub fn store_history(&mut self) -> Option<Vec<u8>> {
        let (excess, history) = self.history_rows();
        if history == 0 || !self.at_ground() {
            return None;
        }
        let selection = self.rows_selection(excess, excess + history - 1)?;
        let mut bytes = self.format(
            GHOSTTY_FORMATTER_FORMAT_VT,
            Some(&selection),
            true,
            true,
            GHOSTTY_FORMATTER_EXTRAS_NONE,
        );
        // Trimming drops the blank lines at the end, which are part of the history.
        let blank = self.blank_rows_up_to(excess + history - 1);
        bytes.extend(b"\r\n".repeat(blank.min(history - 1)));
        // ED 3 clears the scrollback and leaves the screen.
        self.feed(b"\x1b[3J");
        Some(bytes)
    }

    /// Puts stored lines back above the screen and the history that came after them. Past the
    /// history limit, the oldest stored lines are left out.
    pub fn restore_history(&mut self, stored: &[u8]) {
        if stored.is_empty() {
            return;
        }
        // The terminal is drawn again from the stored lines, then from what it holds now with its modes and cursor.
        // That starts at the top of the screen, so a screen of new lines first moves the stored ones above it.
        let kept = self.format(
            GHOSTTY_FORMATTER_FORMAT_VT,
            None,
            true,
            false,
            GHOSTTY_FORMATTER_EXTRAS_ALL,
        );
        self.replace_terminal(|next| {
            next.feed(stored);
            next.feed(b"\x1b[0m");
            next.feed(&b"\r\n".repeat(next.screen_lines()));
            next.feed(&kept);
        });
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write;

    use super::*;
    use crate::grid::{CursorShape, Flags, HISTORY_LINES};

    /// Each line's cells as text and style, without the wrap flag: the seam between stored and kept
    /// lines starts a new line, as in the desktop terminal.
    fn lines(pane: &mut GridPane) -> Vec<String> {
        let mut lines = Vec::new();
        pane.for_each_row(|_, row| {
            lines.push(
                row.iter()
                    .map(|cell| {
                        let flags = cell.flags - Flags::WRAPLINE;
                        format!("{}{:x}{:x}{:?}", cell.text, cell.fg, cell.bg, flags)
                    })
                    .collect(),
            )
        });
        lines
    }

    fn text(pane: &mut GridPane, line: usize) -> String {
        let mut text = String::new();
        pane.for_each_row(|row, cells| {
            if row == line {
                text = cells.iter().map(|cell| cell.text.as_str()).collect();
            }
        });
        text
    }

    fn output(lines: std::ops::Range<usize>) -> Vec<u8> {
        let mut out = String::new();
        for i in lines {
            match i % 5 {
                0 => write!(out, "\x1b[1;31mred {i}\x1b[0m plain\r\n"),
                1 => write!(out, "\x1b[38;5;208;48;2;10;20;30mcolors {i}\x1b[0m\r\n"),
                2 => write!(
                    out,
                    "wide 漢字 {i} e\u{301}\x1b[4:3;58;5;1m curl\x1b[0m\r\n"
                ),
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
        let lines_before = stored.history_size();
        let bytes = stored.store_history().unwrap();
        assert_eq!(stored.history_size(), 0);
        assert!(
            bytes.len() < lines_before * 40 * 24 / 10,
            "{} bytes for {lines_before} lines",
            bytes.len()
        );

        stored.feed(&output(800..900));
        kept.feed(&output(800..900));
        stored.restore_history(&bytes);
        assert_eq!(lines(&mut stored), lines(&mut kept));
        assert_eq!(stored.viewport().cursor, kept.viewport().cursor);
    }

    #[test]
    fn a_restore_past_the_history_limit_keeps_the_newest_lines() {
        let mut pane = GridPane::new(20, 5);
        pane.feed(
            &(0..6000)
                .flat_map(|i| format!("old {i}\r\n").into_bytes())
                .collect::<Vec<_>>(),
        );
        let bytes = pane.store_history().unwrap();
        pane.feed(
            &(0..6000)
                .flat_map(|i| format!("new {i}\r\n").into_bytes())
                .collect::<Vec<_>>(),
        );
        pane.restore_history(&bytes);
        assert_eq!(pane.history_size(), HISTORY_LINES);
        assert_eq!(text(&mut pane, 0).trim(), "old 1996");
        assert_eq!(text(&mut pane, HISTORY_LINES - 1).trim(), "new 5995");
    }

    #[test]
    fn a_longer_scrollback_setting_gets_all_its_stored_lines_back() {
        let mut pane = GridPane::new(20, 5);
        pane.set_options(CursorShape::Block, 25_000);
        pane.feed(
            &(0..15_000)
                .flat_map(|i| format!("old {i}\r\n").into_bytes())
                .collect::<Vec<_>>(),
        );
        let before = pane.history_size();
        assert!(before > HISTORY_LINES);
        let bytes = pane.store_history().unwrap();
        pane.restore_history(&bytes);
        assert_eq!(pane.history_size(), before);
        assert_eq!(&text(&mut pane, 0)[..5], "old 0");
    }
}
