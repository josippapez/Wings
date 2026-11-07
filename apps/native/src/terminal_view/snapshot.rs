//! The immutable snapshot the model thread publishes and the view paints.

use std::{
    cell::RefCell,
    collections::{HashMap, hash_map::Entry},
    sync::{Arc, Mutex},
};

use gpui::RenderImage;
use wings_terminal::grid::{
    BACKGROUND, CURSOR, CursorShape, Flags, GridPane, ImageLayer, ImagePlacement, Point, TermMode,
    Viewport,
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

/// An image a program printed, where the view draws it.
#[derive(Clone)]
pub(super) struct PaintImage {
    pub(super) image: Arc<RenderImage>,
    pub(super) placement: ImagePlacement,
    /// The image's own size, in its pixels.
    pub(super) width: u32,
    pub(super) height: u32,
}

impl PaintImage {
    pub(super) fn layer(&self) -> ImageLayer {
        self.placement.layer
    }
}

thread_local! {
    /// Each model thread's images as textures, by id and generation, made once off the UI thread and kept while
    /// they're on screen.
    static TEXTURES: RefCell<HashMap<(u32, u64), PaintImage>> = RefCell::default();
}

/// An image's pixels as a texture GPUI can upload, which takes BGRA.
fn texture(pane: &GridPane, placement: ImagePlacement) -> Option<PaintImage> {
    let pixels = pane.image_pixels(placement.id)?;
    let (width, height) = (pixels.width, pixels.height);
    let mut bgra = pixels.pixels;
    for pixel in bgra.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    let frame = image::Frame::new(image::RgbaImage::from_raw(width, height, bgra)?);
    Some(PaintImage {
        image: Arc::new(RenderImage::new([frame])),
        placement,
        width,
        height,
    })
}

/// The images on screen, in the order to draw them. Call after `viewport`, which reads them.
fn images(pane: &mut GridPane) -> Vec<PaintImage> {
    let placements = pane.images();
    TEXTURES.with_borrow_mut(|textures| {
        let mut shown = Vec::with_capacity(placements.len());
        for placement in placements {
            let image = match textures.entry((placement.id, placement.generation)) {
                Entry::Occupied(entry) => entry.get().clone(),
                Entry::Vacant(entry) => match texture(pane, placement) {
                    Some(made) => entry.insert(made).clone(),
                    None => continue,
                },
            };
            shown.push(PaintImage { placement, ..image });
        }
        textures.retain(|key, _| {
            shown
                .iter()
                .any(|image| (image.placement.id, image.placement.generation) == *key)
        });
        shown
    })
}

pub(super) struct Snapshot {
    pub(super) cells: Vec<PaintCell>,
    /// Images programs printed, in the order to draw them.
    pub(super) images: Vec<PaintImage>,
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
            images: Vec::new(),
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

/// What a full-screen program (OpenCode, vim) shows, a line per row without its trailing spaces. `None` on the normal
/// screen. Such a program draws by moving the cursor, so its output has no lines to read, only the screen it leaves.
pub(super) fn screen_text(snapshot: &Snapshot) -> Option<String> {
    if !snapshot.mode.contains(TermMode::ALT_SCREEN) {
        return None;
    }
    let mut cells: Vec<&PaintCell> = snapshot.cells.iter().collect();
    cells.sort_by_key(|cell| (cell.row, cell.col));
    let mut rows = vec![(String::new(), 0); snapshot.rows];
    for cell in cells {
        // An empty cell is a blank or the second half of a wide character, which its first half already covers.
        let Some((line, filled)) = rows.get_mut(cell.row).filter(|_| !cell.text.is_empty()) else {
            continue;
        };
        if cell.col > *filled {
            line.extend(std::iter::repeat_n(' ', cell.col - *filled));
            *filled = cell.col;
        }
        line.push_str(&cell.text);
        *filled += cell.width.max(1);
    }
    Some(
        rows.iter()
            .map(|(line, _)| line.trim_end())
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

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
    let images = images(pane);
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
        images,
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

    #[test]
    fn a_full_screen_program_reads_as_the_screen_it_leaves() {
        let mut pane = GridPane::new(20, 4);
        pane.feed(b"before");
        assert_eq!(screen_text(&snapshot(&mut pane, None, false, None)), None);
        // Drawn by moving the cursor: a word in the middle of row 2, then a wide character and text on row 4.
        pane.feed("\x1b[?1049h\x1b[2;3Hhello\x1b[4;1H界 ok".as_bytes());
        assert_eq!(
            screen_text(&snapshot(&mut pane, None, false, None)).as_deref(),
            Some("\n  hello\n\n界 ok")
        );
    }

    #[test]
    fn images_programs_print_come_as_textures_where_they_go_and_not_with_images_off() {
        let mut pane = GridPane::new(20, 4);
        pane.set_cell_size(10, 20);
        // A red 20 x 20 SIXEL two cells in.
        pane.feed(b"ab\x1bP0;1q\"1;1;20;20#1;2;100;0;0#1!20~-!20~-!20~-!20~\x1b\\");
        let frame = snapshot(&mut pane, None, false, None);
        let [image] = frame.images.as_slice() else {
            panic!("one image");
        };
        assert_eq!((image.placement.col, image.placement.row), (2, 0));
        assert_eq!(image.placement.size, (20, 20));
        assert_eq!((image.width, image.height), (20, 20));
        // BGRA, as GPUI uploads it.
        assert_eq!(&image.image.as_bytes(0).unwrap()[..4], [0, 0, 255, 255]);
        // The texture is made once and kept while the image is on screen.
        let again = snapshot(&mut pane, None, false, None);
        assert_eq!(again.images[0].image.id, image.image.id);
        pane.set_images(false);
        assert!(snapshot(&mut pane, None, false, None).images.is_empty());
        pane.feed(b"\x1bP0;1q\"1;1;20;20#1~\x1b\\");
        assert!(snapshot(&mut pane, None, false, None).images.is_empty());
    }
}
