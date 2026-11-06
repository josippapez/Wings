//! The immutable snapshot the model thread publishes and the view paints.

use std::sync::{Arc, Mutex};

use wings_terminal::{
    alacritty_terminal::{
        event::EventListener,
        grid::Dimensions,
        term::{TermMode, cell::Flags},
        vte::ansi::{CursorShape, NamedColor},
    },
    grid::{BACKGROUND, CURSOR, GridPane},
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

fn packed(color: wings_terminal::alacritty_terminal::vte::ansi::Rgb) -> u32 {
    (color.r as u32) << 16 | (color.g as u32) << 8 | color.b as u32
}

pub(super) fn snapshot<L: EventListener>(
    pane: &GridPane<L>,
    status: Option<String>,
    closed: bool,
    find: Option<&mut Find>,
) -> Snapshot {
    let find_stale = find.as_ref().is_some_and(|find| find.stale);
    let (matches, active, results) = match find {
        Some(find) => (
            visible_matches(pane.model(), &mut find.regex),
            find.active.clone(),
            Some(find.results),
        ),
        None => (Vec::new(), None, None),
    };
    let mut next_match = 0;
    let content = pane.model().renderable_content();
    let (selection, cursor_point, cursor_shape) = (
        content.selection,
        content.cursor.point,
        content.cursor.shape,
    );
    let mut cells = Vec::new();
    for indexed in content.display_iter {
        let selected = selection
            .is_some_and(|range| range.contains_cell(&indexed, cursor_point, cursor_shape));
        // Cells and matches both come in reading order, so one pass marks them.
        while matches
            .get(next_match)
            .is_some_and(|found| *found.end() < indexed.point)
        {
            next_match += 1;
        }
        let found = if active
            .as_ref()
            .is_some_and(|found| found.contains(&indexed.point))
        {
            Found::Active
        } else if matches
            .get(next_match)
            .is_some_and(|found| found.contains(&indexed.point))
        {
            Found::Match
        } else {
            Found::No
        };
        let cell = indexed.cell;
        let row = (indexed.point.line.0 + content.display_offset as i32) as usize;
        let spacer = cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER);
        let decorated_space = cell
            .flags
            .intersects(Flags::ALL_UNDERLINES | Flags::STRIKEOUT);
        let mut text = if spacer || (cell.c == ' ' && !decorated_space) {
            String::new()
        } else {
            cell.c.to_string()
        };
        if !spacer && let Some(combining) = cell.zerowidth() {
            if text.is_empty() {
                text.push(cell.c);
            }
            text.extend(combining);
        }
        let (fg, bg) = pane.cell_colors(cell);
        cells.push(PaintCell {
            row,
            col: indexed.point.column.0,
            width: if cell.flags.contains(Flags::WIDE_CHAR) {
                2
            } else {
                1
            },
            text,
            fg,
            bg,
            flags: cell.flags,
            selected,
            found,
        });
    }
    let cursor = content.cursor;
    let row = cursor.point.line.0 + content.display_offset as i32;
    let cursor = if cursor.shape == CursorShape::Hidden
        || row < 0
        || row as usize >= pane.model().screen_lines()
    {
        None
    } else {
        let width = if pane.model().grid()[cursor.point]
            .flags
            .contains(Flags::WIDE_CHAR)
        {
            2
        } else {
            1
        };
        Some((row as usize, cursor.point.column.0, width, cursor.shape))
    };
    Snapshot {
        cells,
        cursor,
        cursor_color: packed(pane.color(NamedColor::Cursor as usize)),
        background: packed(pane.color(NamedColor::Background as usize)),
        mode: content.mode,
        rows: pane.model().screen_lines(),
        cols: pane.model().columns(),
        display_offset: content.display_offset,
        history: pane.model().grid().history_size(),
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
        let frame = snapshot(&pane, None, false, None);
        assert_eq!(frame.cells[0].text, "界");
        assert_eq!(frame.cells[0].width, 2);
        assert_eq!(frame.cells[1].text, "");
        assert_eq!(frame.cells[2].text, "e\u{301}");
        assert_eq!(frame.cells[0].fg, packed(pane.color(1)));
        assert_eq!(
            frame.cursor.map(|(row, col, _, _)| (row, col)),
            Some((0, 3))
        );
        pane.feed(b"\x1b[?25l");
        assert!(snapshot(&pane, None, false, None).cursor.is_none());
    }
}
