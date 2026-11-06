//! The immutable snapshot the model thread publishes and the view paints.

use std::sync::{Arc, Mutex};

use wings_terminal::grid::{
    BACKGROUND, CURSOR, CursorShape, Flags, GridPane, Point, TermMode, Viewport,
};

use super::find::{Find, FindResults, visible_matches};

#[derive(Clone)]
pub(super) struct PaintCell {
    pub(super) row: usize,
    pub(super) col: usize,
    pub(super) width: usize,
    pub(super) text: String,
    pub(super) fg: u32,
    pub(super) bg: u32,
    pub(super) flags: Flags,
    pub(super) selected: bool,
    pub(super) found: Found,
}

/// How a cell is part of a find match.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Found {
    #[default]
    No,
    Match,
    Active,
}

pub(super) struct Snapshot {
    pub(super) cells: Vec<PaintCell>,
    pub(super) cursor: Option<(usize, usize, usize, CursorShape)>,
    pub(super) cursor_color: u32,
    pub(super) background: u32,
    pub(super) mode: TermMode,
    pub(super) rows: usize,
    pub(super) cols: usize,
    pub(super) display_offset: usize,
    pub(super) history: usize,
    /// Set while a find term is active: which match is current, of how many.
    pub(super) find: Option<FindResults>,
    pub(super) find_stale: bool,
    pub(super) status: Option<String>,
    pub(super) closed: bool,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            cells: Vec::new(),
            cursor: None,
            cursor_color: CURSOR,
            background: BACKGROUND,
            mode: TermMode::default(),
            rows: 0,
            cols: 0,
            display_offset: 0,
            history: 0,
            find: None,
            find_stale: false,
            status: None,
            closed: false,
        }
    }
}

pub(super) type Latest = Arc<Mutex<Arc<Snapshot>>>;

pub(super) fn snapshot(
    pane: &mut GridPane,
    status: Option<String>,
    closed: bool,
    find: Option<&mut Find>,
) -> Snapshot {
    let find_stale = find.as_ref().is_some_and(|find| find.stale);
    let (matches, active, results) = match find {
        Some(find) => (
            visible_matches(pane, &find.regex),
            find.active,
            Some(find.results),
        ),
        None => (Vec::new(), None, None),
    };
    let mut next_match = 0;
    let (rows, cols) = (pane.screen_lines(), pane.columns());
    let Viewport {
        cells: grid,
        cursor,
    } = pane.viewport();
    let top = pane.viewport_top();
    let mut cells = Vec::with_capacity(grid.len());
    for (i, cell) in grid.into_iter().enumerate() {
        let (row, col) = (i / cols.max(1), i % cols.max(1));
        let point = Point {
            row: top + row,
            col,
        };
        // Cells and matches both come in reading order, so one pass marks them.
        while matches
            .get(next_match)
            .is_some_and(|found| found.end < point)
        {
            next_match += 1;
        }
        let found = if active.is_some_and(|found| found.contains(point)) {
            Found::Active
        } else if matches
            .get(next_match)
            .is_some_and(|found| found.contains(point))
        {
            Found::Match
        } else {
            Found::No
        };
        let decorated_space = cell
            .flags
            .intersects(Flags::ALL_UNDERLINES | Flags::STRIKEOUT);
        let text = if cell.text == " " && !decorated_space {
            String::new()
        } else {
            cell.text
        };
        cells.push(PaintCell {
            row,
            col,
            width: if cell.flags.contains(Flags::WIDE_CHAR) {
                2
            } else {
                1
            },
            text,
            fg: cell.fg,
            bg: cell.bg,
            flags: cell.flags,
            selected: cell.selected,
            found,
        });
    }
    let cursor = cursor
        .filter(|cursor| cursor.shape != CursorShape::Hidden)
        .map(|cursor| {
            let wide = cells
                .get(cursor.row * cols + cursor.col)
                .is_some_and(|cell| cell.flags.contains(Flags::WIDE_CHAR));
            (
                cursor.row,
                cursor.col,
                if wide { 2 } else { 1 },
                cursor.shape,
            )
        });
    let (_, background, cursor_color) = pane.theme_colors();
    Snapshot {
        cells,
        cursor,
        cursor_color,
        background,
        mode: pane.mode(),
        rows,
        cols,
        display_offset: pane.display_offset(),
        history: pane.history_size(),
        find: results,
        find_stale,
        status,
        closed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_snapshot_keeps_combining_wide_cells_colors_and_cursor() {
        let mut pane = GridPane::new(20, 4);
        pane.feed("\x1b[31m界e\u{301}\x1b[0m".as_bytes());
        let frame = snapshot(&mut pane, None, false, None);
        assert_eq!(frame.cells[0].text, "界");
        assert_eq!(frame.cells[0].width, 2);
        assert_eq!(frame.cells[1].text, "");
        assert_eq!(frame.cells[2].text, "e\u{301}");
        assert_eq!(frame.cells[0].fg, pane.color(1));
        assert_eq!(
            frame.cursor.map(|(row, col, _, _)| (row, col)),
            Some((0, 3))
        );
        pane.feed(b"\x1b[?25l");
        assert!(snapshot(&mut pane, None, false, None).cursor.is_none());
    }
}
