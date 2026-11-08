//! Shared terminal model: libghostty-vt (Ghostty's terminal library) parses the output and keeps the screen and
//! its scrollback, read back here as plain cells for the native renderer, and as benchmark frames that carry the
//! rows that changed as style runs, ready for `fillRect` + `fillText` on a canvas (`src/bench-canvas.ts`).
//!
//! Frame layout, little-endian:
//! `u16 cursor_row (0xFFFF = hidden), u16 cursor_col, u16 row_count`, then per row
//! `u16 row, u16 run_count`, then per run `u32 fg, u32 bg, u16 col, u16 cells, u16 text_len, text (UTF-8)`.
//! Colors are 0xRRGGBB; fg's top byte holds bold (1), italic (2), underline (4) and strikeout (8).
//! A run is either printable ASCII, box drawing and block characters (one cell each, in every monospace font),
//! or one other character on its own, so a fallback-font glyph never shifts the cells after it.
//! An all-space run has no text: its background is all there is to draw.
//!
//! Rows are counted from the top of the kept history (`Point::row`), so a match or a selection end can be
//! compared with any other. Ghostty keeps history in whole pages, so it holds up to a page more lines than the
//! scrollback setting asks for; the extra lines above the limit are never shown, searched or stored.

use std::{
    ffi::c_void,
    ops::{BitOr, ControlFlow, Range},
    ptr,
};

use ghostty_sys::*;

/// Wings' terminal theme (`src/lib/terminal.ts`), so both renderers show the same colors.
pub(crate) const ANSI: [u32; 16] = [
    0x4a4650, 0xf2786a, 0x8fd19e, 0xe9c46a, 0x7aa7f2, 0xd59cf0, 0x6fd0d0, 0xd8d4ce, 0x6d6872,
    0xff9285, 0xa7e3b3, 0xf3d68b, 0x9cc0ff, 0xe4b8f7, 0x93e2e2, 0xf4f1ec,
];
pub const FOREGROUND: u32 = 0xe9e6e1;
pub const BACKGROUND: u32 = 0x17151b;
pub const CURSOR: u32 = 0xf0ede8;

/// Lines of scrollback per pane, as in the desktop terminal.
pub const HISTORY_LINES: usize = 10_000;

/// Rows the find scan reads per render-state update, which bounds the memory a scan holds.
const SCAN_ROWS: u16 = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorShape {
    Block,
    Underline,
    Beam,
    HollowBlock,
    Hidden,
}

/// The terminal modes the view reads, by the names Alacritty gave them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TermMode(u32);

impl TermMode {
    pub const SHOW_CURSOR: Self = Self(1);
    pub const APP_CURSOR: Self = Self(1 << 1);
    pub const LINE_FEED_NEW_LINE: Self = Self(1 << 2);
    pub const BRACKETED_PASTE: Self = Self(1 << 3);
    pub const MOUSE_REPORT_CLICK: Self = Self(1 << 4);
    pub const MOUSE_DRAG: Self = Self(1 << 5);
    pub const MOUSE_MOTION: Self = Self(1 << 6);
    pub const SGR_MOUSE: Self = Self(1 << 7);
    pub const ALT_SCREEN: Self = Self(1 << 8);
    pub const ALTERNATE_SCROLL: Self = Self(1 << 9);
    pub const FOCUS_IN_OUT: Self = Self(1 << 10);
    pub const MOUSE_MODE: Self =
        Self(Self::MOUSE_REPORT_CLICK.0 | Self::MOUSE_DRAG.0 | Self::MOUSE_MOTION.0);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

impl Default for TermMode {
    /// A fresh terminal's modes.
    fn default() -> Self {
        Self::SHOW_CURSOR | Self::ALTERNATE_SCROLL
    }
}

impl BitOr for TermMode {
    type Output = Self;
    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// A cell's attributes, by the names Alacritty gave them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Flags(u32);

impl Flags {
    pub const BOLD: Self = Self(1);
    pub const ITALIC: Self = Self(1 << 1);
    pub const DIM: Self = Self(1 << 2);
    pub const UNDERLINE: Self = Self(1 << 3);
    pub const DOUBLE_UNDERLINE: Self = Self(1 << 4);
    pub const UNDERCURL: Self = Self(1 << 5);
    pub const DOTTED_UNDERLINE: Self = Self(1 << 6);
    pub const DASHED_UNDERLINE: Self = Self(1 << 7);
    pub const INVERSE: Self = Self(1 << 8);
    pub const HIDDEN: Self = Self(1 << 9);
    pub const STRIKEOUT: Self = Self(1 << 10);
    pub const WIDE_CHAR: Self = Self(1 << 11);
    /// The second cell of a wide character.
    pub const WIDE_CHAR_SPACER: Self = Self(1 << 12);
    /// The last cell of a row whose wide character went on to the next row.
    pub const LEADING_WIDE_CHAR_SPACER: Self = Self(1 << 13);
    /// On the last cell of a row the line goes on from.
    pub const WRAPLINE: Self = Self(1 << 14);
    pub const ALL_UNDERLINES: Self = Self(
        Self::UNDERLINE.0
            | Self::DOUBLE_UNDERLINE.0
            | Self::UNDERCURL.0
            | Self::DOTTED_UNDERLINE.0
            | Self::DASHED_UNDERLINE.0,
    );

    pub const fn empty() -> Self {
        Self(0)
    }

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

impl BitOr for Flags {
    type Output = Self;
    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::Sub for Flags {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
}

/// How the view moves through the history. `Delta` > 0 is towards older output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scroll {
    Delta(i32),
    Top,
    Bottom,
}

/// The half of a cell the pointer is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionType {
    Simple,
    /// A double click: whole words.
    Semantic,
    /// A triple click: whole lines.
    Lines,
}

/// What the terminal asks of its host while it parses output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// Replies to the program's queries, for the PTY.
    PtyWrite(Vec<u8>),
    /// The title the program set, or `None` when it cleared it.
    Title(Option<String>),
    /// Text a program put on the clipboard (OSC 52). Programs can never read it.
    Clipboard(String),
}

/// A cell to draw, colors resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    /// The cell's characters, empty for a cell nothing was written to.
    pub text: String,
    pub fg: u32,
    pub bg: u32,
    pub flags: Flags,
    pub selected: bool,
}

/// A cell, with rows counted from the top of the kept history.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Point {
    pub row: usize,
    pub col: usize,
}

/// Where the cursor is drawn on screen, if it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    pub shape: CursorShape,
}

/// The screen as it is now: `rows * cols` cells in reading order.
pub struct Viewport {
    pub cells: Vec<Cell>,
    pub cursor: Option<Cursor>,
}

/// What the terminal's callbacks write into; boxed so its address stays put.
#[derive(Default)]
struct Shared {
    events: Vec<Event>,
    size: GhosttySizeReportSize,
    /// Device attributes report SIXEL while images are on, as the desktop's image addon does.
    sixel: bool,
}

struct Selecting {
    ty: SelectionType,
    anchor: GhosttyTrackedGridRef,
    side: Side,
}

pub struct GridPane {
    term: GhosttyTerminal,
    render: GhosttyRenderState,
    rows_it: GhosttyRenderStateRowIterator,
    cells: GhosttyRenderStateRowCells,
    shared: Box<Shared>,
    rows: usize,
    cols: usize,
    /// Lines of scrollback kept, which `set_options` changes.
    history_limit: usize,
    cursor: CursorShape,
    /// A new default cursor waits for the parser to be between sequences.
    cursor_pending: bool,
    selecting: Option<Selecting>,
    /// Takes SIXEL and iTerm2 images out of the output while images are on.
    #[cfg(feature = "images")]
    scanner: Option<crate::images::Scanner>,
    #[cfg(feature = "images")]
    image_iter: GhosttyRenderStateImageIterator,
}

/// Where an image is drawn among the cells: under their backgrounds, between the backgrounds and the text, or over it.
#[cfg(feature = "images")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ImageLayer {
    BelowBackground,
    BelowText,
    AboveText,
}

/// An image on screen, from its top-left cell. Pixels are the terminal's, of the cell size it was given.
#[cfg(feature = "images")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImagePlacement {
    pub id: u32,
    /// Changes whenever the image's pixels do; never reused.
    pub generation: u64,
    pub layer: ImageLayer,
    pub z: i32,
    /// The top-left cell, from the view's top left. Negative when the image starts above or left of it.
    pub col: i32,
    pub row: i32,
    /// From the top-left cell's corner to the image's.
    pub offset: (u32, u32),
    /// The size to draw it at.
    pub size: (u32, u32),
    /// The part of the image to draw, in its own pixels: x, y, width, height.
    pub source: [u32; 4],
}

fn indexed(i: u8) -> u32 {
    match i {
        0..=15 => ANSI[i as usize],
        16..=231 => {
            let i = i - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + 40 * v as u32 };
            (level(i / 36) << 16) | (level((i / 6) % 6) << 8) | level(i % 6)
        }
        _ => {
            let v = 8 + 10 * (i as u32 - 232);
            (v << 16) | (v << 8) | v
        }
    }
}

pub(crate) fn rgb(color: u32) -> GhosttyColorRgb {
    GhosttyColorRgb {
        r: (color >> 16) as u8,
        g: (color >> 8) as u8,
        b: color as u8,
    }
}

fn packed(color: GhosttyColorRgb) -> u32 {
    ((color.r as u32) << 16) | ((color.g as u32) << 8) | color.b as u32
}

/// Two thirds of the way from `bg` to `fg`.
pub(crate) fn dim(fg: u32, bg: u32) -> u32 {
    let mix = |s: u32| (((fg >> s) & 0xff) * 2 + ((bg >> s) & 0xff)) / 3;
    (mix(16) << 16) | (mix(8) << 8) | mix(0)
}

fn shares_cells(c: char) -> bool {
    matches!(c, ' '..='~' | '\u{2500}'..='\u{259f}')
}

const fn mode(value: u16, ansi: bool) -> GhosttyMode {
    (value & 0x7fff) | ((ansi as u16) << 15)
}

const STYLE_BOLD: u16 = 1;
const STYLE_ITALIC: u16 = 1 << 1;
const STYLE_FAINT: u16 = 1 << 2;
const STYLE_INVERSE: u16 = 1 << 4;
const STYLE_INVISIBLE: u16 = 1 << 5;
const STYLE_STRIKETHROUGH: u16 = 1 << 6;

fn check(result: GhosttyResult, what: &str) {
    assert_eq!(result, GHOSTTY_SUCCESS, "libghostty-vt {what} failed");
}

fn sized<T: Default>() -> T {
    let mut value = T::default();
    // Every sized struct starts with its size.
    unsafe { *(&mut value as *mut T as *mut usize) = std::mem::size_of::<T>() };
    value
}

/// `value` as the `void *out` a Ghostty getter writes its answer into.
fn out_ptr<T>(value: &mut T) -> *mut c_void {
    value as *mut T as *mut c_void
}

// Typed forms of Ghostty's getters. Each writes what `data` names into `value`, and some read it first: a sized
// struct's size, the handle to fill, a buffer's capacity.
//
// Safety: the handle is live, and `T` is the type Ghostty documents for `data`.

unsafe fn terminal_get<T>(
    term: GhosttyTerminal,
    data: GhosttyTerminalData,
    value: &mut T,
) -> GhosttyResult {
    unsafe { ghostty_terminal_get(term, data, out_ptr(value)) }
}

unsafe fn render_state_get<T>(
    render: GhosttyRenderState,
    data: GhosttyRenderStateData,
    value: &mut T,
) -> GhosttyResult {
    unsafe { ghostty_render_state_get(render, data, out_ptr(value)) }
}

unsafe fn render_state_row_get<T>(
    rows: GhosttyRenderStateRowIterator,
    data: GhosttyRenderStateRowData,
    value: &mut T,
) -> GhosttyResult {
    unsafe { ghostty_render_state_row_get(rows, data, out_ptr(value)) }
}

unsafe fn render_state_row_cells_get<T>(
    cells: GhosttyRenderStateRowCells,
    data: GhosttyRenderStateRowCellsData,
    value: &mut T,
) -> GhosttyResult {
    unsafe { ghostty_render_state_row_cells_get(cells, data, out_ptr(value)) }
}

unsafe fn row_get<T>(row: GhosttyRow, data: GhosttyRowData, value: &mut T) -> GhosttyResult {
    unsafe { ghostty_row_get(row, data, out_ptr(value)) }
}

unsafe fn cell_get<T>(cell: GhosttyCell, data: GhosttyCellData, value: &mut T) -> GhosttyResult {
    unsafe { ghostty_cell_get(cell, data, out_ptr(value)) }
}

fn shared(userdata: *mut c_void) -> &'static mut Shared {
    unsafe { &mut *(userdata as *mut Shared) }
}

unsafe extern "C" fn on_write_pty(
    _: GhosttyTerminal,
    userdata: *mut c_void,
    data: *const u8,
    len: usize,
) {
    if data.is_null() || len == 0 {
        return;
    }
    let bytes = unsafe { std::slice::from_raw_parts(data, len) };
    let events = &mut shared(userdata).events;
    match events.last_mut() {
        Some(Event::PtyWrite(pending)) => pending.extend_from_slice(bytes),
        _ => events.push(Event::PtyWrite(bytes.to_vec())),
    }
}

/// Ghostty sets the title before it calls this, so the title event keeps its place among the others.
unsafe extern "C" fn on_title(term: GhosttyTerminal, userdata: *mut c_void) {
    let mut title = GhosttyString::default();
    let title =
        unsafe { terminal_get(term, GHOSTTY_TERMINAL_DATA_TITLE, &mut title) == GHOSTTY_SUCCESS }
            .then(|| String::from_utf8_lossy(unsafe { string(title) }).into_owned())
            .filter(|title| !title.is_empty());
    shared(userdata).events.push(Event::Title(title));
}

unsafe extern "C" fn on_size(
    _: GhosttyTerminal,
    userdata: *mut c_void,
    out: *mut GhosttySizeReportSize,
) -> bool {
    unsafe { *out = shared(userdata).size };
    true
}

/// The same primary device attributes as Alacritty: a VT102. With images on, also SIXEL graphics.
unsafe extern "C" fn on_device_attributes(
    _: GhosttyTerminal,
    userdata: *mut c_void,
    out: *mut GhosttyDeviceAttributes,
) -> bool {
    let primary = unsafe { &mut (*out).primary };
    primary.conformance_level = 6;
    if shared(userdata).sixel {
        primary.features[0] = GHOSTTY_DA_FEATURE_SIXEL as u16;
        primary.num_features = 1;
    }
    true
}

unsafe extern "C" fn on_clipboard_write(
    _: GhosttyTerminal,
    userdata: *mut c_void,
    request: *const GhosttyClipboardWrite,
) {
    let request = unsafe { &*request };
    let contents = if request.contents.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(request.contents, request.contents_len) }
    };
    let text = contents.iter().find_map(|content| {
        let mime = unsafe { string(content.mime) };
        (mime.starts_with(b"text/plain")).then(|| unsafe { string(content.data) })
    });
    let Some(text) = text.and_then(|bytes| std::str::from_utf8(bytes).ok()) else {
        return;
    };
    shared(userdata)
        .events
        .push(Event::Clipboard(text.to_owned()));
    if let Some(reply) = request.reply {
        let answer = GhosttyClipboardWriteReply {
            size: std::mem::size_of::<GhosttyClipboardWriteReply>(),
            result: GHOSTTY_CLIPBOARD_WRITE_RESULT_SUCCESS,
            remember: false,
        };
        unsafe { reply(request, &answer) };
    }
}

unsafe fn string<'a>(text: GhosttyString) -> &'a [u8] {
    if text.ptr.is_null() || text.len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(text.ptr, text.len) }
    }
}

fn cursor_style(shape: CursorShape) -> GhosttyCursorStyle {
    match shape {
        CursorShape::Beam => GHOSTTY_CURSOR_STYLE_BAR,
        CursorShape::Underline => GHOSTTY_CURSOR_STYLE_UNDERLINE,
        CursorShape::HollowBlock => GHOSTTY_CURSOR_STYLE_BLOCK_HOLLOW,
        _ => GHOSTTY_CURSOR_STYLE_BLOCK,
    }
}

/// The most rows a Ghostty page holds at `cols` columns: a standard page is about 384 KB, and each row takes 8 bytes
/// plus 8 a cell (`Capacity.fitRows` in `vendor/ghostty/src/terminal/page.zig`).
fn page_rows(cols: usize) -> usize {
    400_000 / (8 + 8 * cols.max(1)) + 1
}

/// Ghostty drops history a whole page at a time once it is over its limit, which can leave a page fewer lines than
/// the limit. A page more than the setting keeps at least the setting.
fn set_line_limit(term: GhosttyTerminal, scrollback: usize, cols: usize) {
    let lines = scrollback.saturating_add(page_rows(cols));
    check(
        unsafe {
            ghostty_terminal_set(
                term,
                GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_LINES,
                &lines as *const _ as *const c_void,
            )
        },
        "set",
    );
}

/// A new terminal with Wings' theme and the host's callbacks.
fn new_terminal(
    cols: usize,
    rows: usize,
    shared: &mut Shared,
    scrollback: usize,
) -> GhosttyTerminal {
    unsafe {
        let mut term = ptr::null_mut();
        check(
            ghostty_terminal_new(
                ptr::null(),
                &mut term,
                cols.max(1) as u16,
                rows.max(1) as u16,
            ),
            "terminal_new",
        );
        let set =
            |option, value: *const c_void| check(ghostty_terminal_set(term, option, value), "set");
        set(
            GHOSTTY_TERMINAL_OPT_USERDATA,
            shared as *mut Shared as *const c_void,
        );
        let write: GhosttyTerminalWritePtyFn = Some(on_write_pty);
        set(
            GHOSTTY_TERMINAL_OPT_WRITE_PTY,
            std::mem::transmute::<GhosttyTerminalWritePtyFn, *const c_void>(write),
        );
        let title: GhosttyTerminalTitleChangedFn = Some(on_title);
        set(
            GHOSTTY_TERMINAL_OPT_TITLE_CHANGED,
            std::mem::transmute::<GhosttyTerminalTitleChangedFn, *const c_void>(title),
        );
        let size: GhosttyTerminalSizeFn = Some(on_size);
        set(
            GHOSTTY_TERMINAL_OPT_SIZE,
            std::mem::transmute::<GhosttyTerminalSizeFn, *const c_void>(size),
        );
        let attributes: GhosttyTerminalDeviceAttributesFn = Some(on_device_attributes);
        set(
            GHOSTTY_TERMINAL_OPT_DEVICE_ATTRIBUTES,
            std::mem::transmute::<GhosttyTerminalDeviceAttributesFn, *const c_void>(attributes),
        );
        let clipboard: GhosttyTerminalClipboardWriteFn = Some(on_clipboard_write);
        set(
            GHOSTTY_TERMINAL_OPT_CLIPBOARD_WRITE,
            std::mem::transmute::<GhosttyTerminalClipboardWriteFn, *const c_void>(clipboard),
        );
        // No read callback: programs can never read the clipboard, as with Alacritty's default.
        set(
            GHOSTTY_TERMINAL_OPT_COLOR_FOREGROUND,
            &rgb(FOREGROUND) as *const _ as *const c_void,
        );
        set(
            GHOSTTY_TERMINAL_OPT_COLOR_BACKGROUND,
            &rgb(BACKGROUND) as *const _ as *const c_void,
        );
        set(
            GHOSTTY_TERMINAL_OPT_COLOR_CURSOR,
            &rgb(CURSOR) as *const _ as *const c_void,
        );
        let palette: [GhosttyColorRgb; 256] = std::array::from_fn(|i| rgb(indexed(i as u8)));
        set(
            GHOSTTY_TERMINAL_OPT_COLOR_PALETTE,
            palette.as_ptr() as *const c_void,
        );
        // Ghostty limits scrollback to 10,000 bytes unless told otherwise; lines are what Wings limits.
        let unlimited = usize::MAX;
        set(
            GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES,
            &unlimited as *const _ as *const c_void,
        );
        set_line_limit(term, scrollback, cols);
        term
    }
}

impl GridPane {
    pub fn new(cols: usize, rows: usize) -> GridPane {
        let mut shared = Box::new(Shared {
            size: GhosttySizeReportSize {
                rows: rows as u16,
                columns: cols as u16,
                cell_width: 0,
                cell_height: 0,
            },
            ..Shared::default()
        });
        let term = new_terminal(cols, rows, &mut shared, HISTORY_LINES);
        let (mut render, mut rows_it, mut cells) =
            (ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
        unsafe {
            check(
                ghostty_render_state_new(ptr::null(), &mut render),
                "render_state_new",
            );
            check(
                ghostty_render_state_row_iterator_new(ptr::null(), &mut rows_it),
                "row_iterator_new",
            );
            check(
                ghostty_render_state_row_cells_new(ptr::null(), &mut cells),
                "row_cells_new",
            );
        }
        #[allow(unused_mut)]
        let mut pane = Self {
            term,
            render,
            rows_it,
            cells,
            shared,
            rows,
            cols,
            history_limit: HISTORY_LINES,
            cursor: CursorShape::Block,
            cursor_pending: false,
            selecting: None,
            #[cfg(feature = "images")]
            scanner: None,
            #[cfg(feature = "images")]
            image_iter: unsafe {
                let mut images = ptr::null_mut();
                check(
                    ghostty_render_state_image_iterator_new(ptr::null(), &mut images),
                    "image_iterator_new",
                );
                images
            },
        };
        #[cfg(feature = "images")]
        {
            crate::images::install_png_decoder();
            pane.set_images(true);
        }
        pane
    }

    /// Whether programs' images are drawn, as the desktop's Show images in the terminal. Turning them off drops the
    /// images the pane has.
    #[cfg(feature = "images")]
    pub fn set_images(&mut self, on: bool) {
        if on == self.scanner.is_some() {
            return;
        }
        self.scanner = on.then(crate::images::Scanner::new);
        self.shared.sixel = on;
        let limit: u64 = if on { crate::images::STORAGE_LIMIT } else { 0 };
        unsafe {
            ghostty_terminal_set(
                self.term,
                GHOSTTY_TERMINAL_OPT_KITTY_IMAGE_STORAGE_LIMIT,
                &limit as *const _ as *const c_void,
            );
        }
    }

    #[cfg(feature = "images")]
    pub fn images_on(&self) -> bool {
        self.scanner.is_some()
    }

    /// The cursor programs get until they set their own and the lines of history kept, like the desktop's cursor
    /// and scrollback settings. Fewer lines drop the oldest.
    pub fn set_options(&mut self, shape: CursorShape, scrollback: usize) {
        self.history_limit = scrollback;
        set_line_limit(self.term, scrollback, self.cols);
        if shape != self.cursor {
            // A cursor still in the old default is one the program never set, so it takes the new one.
            let unset = self.current_cursor_style() == cursor_style(self.cursor);
            self.cursor = shape;
            let style = cursor_style(shape);
            unsafe {
                ghostty_terminal_set(
                    self.term,
                    GHOSTTY_TERMINAL_OPT_DEFAULT_CURSOR_STYLE,
                    &style as *const _ as *const c_void,
                );
            }
            self.cursor_pending = unset;
            self.apply_default_cursor();
        }
    }

    fn current_cursor_style(&self) -> GhosttyCursorStyle {
        let mut cursor: GhosttyRenderStateCursor = sized();
        unsafe {
            ghostty_render_state_update(self.render, self.term);
            render_state_get(self.render, GHOSTTY_RENDER_STATE_DATA_CURSOR, &mut cursor);
        }
        cursor.visual_style
    }

    /// Whether the parser is between sequences, where one of Wings' own can go in.
    pub(crate) fn at_ground(&self) -> bool {
        self.get::<bool>(GHOSTTY_TERMINAL_DATA_VT_GROUND) == Some(true)
    }

    /// DECSCUSR 0 takes the default cursor. It goes in only between sequences, so it never splits a program's.
    fn apply_default_cursor(&mut self) {
        if self.cursor_pending && self.at_ground() {
            self.cursor_pending = false;
            unsafe { ghostty_terminal_vt_write(self.term, b"\x1b[0 q".as_ptr(), 5) };
        }
    }

    /// Lines of scrollback kept.
    pub fn history_limit(&self) -> usize {
        self.history_limit
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        #[cfg(feature = "images")]
        if let Some(mut scanner) = self.scanner.take() {
            let mut pieces = Vec::new();
            scanner.scan(bytes, &mut pieces);
            for piece in pieces {
                self.feed_piece(piece);
            }
            self.scanner = Some(scanner);
            self.apply_default_cursor();
            return;
        }
        self.write(bytes);
        self.apply_default_cursor();
    }

    fn write(&self, bytes: &[u8]) {
        unsafe { ghostty_terminal_vt_write(self.term, bytes.as_ptr(), bytes.len()) };
    }

    #[cfg(feature = "images")]
    fn feed_piece(&mut self, piece: crate::images::Piece<'_>) {
        use crate::images::{decode_iterm, decode_sixel, Piece};
        let image = match piece {
            Piece::Bytes(bytes) => return self.write(bytes),
            Piece::Owned(bytes) => return self.write(&bytes),
            Piece::Sixel(params, data) => {
                let background = self.theme_colors().1;
                let [b, g, r, _] = background.to_le_bytes();
                decode_sixel(&params, &data, u32::from_le_bytes([r, g, b, 255]))
            }
            Piece::Iterm(body) => {
                let size = self.shared.size;
                decode_iterm(
                    &body,
                    (size.cell_width, size.cell_height),
                    (
                        size.cell_width * size.columns as u32,
                        size.cell_height * size.rows as u32,
                    ),
                )
            }
        };
        let Some(image) = image else {
            return;
        };
        self.write(&crate::images::kitty_place(&image));
        // Like the addon, the cursor ends on the image's last row, in the column it started in. Past the screen and
        // the scrollback the image is gone, so a very tall one moves no further.
        let rows = crate::images::rows_for(image.height, self.shared.size.cell_height)
            .saturating_sub(1)
            .min((self.history_limit + self.rows) as u32);
        self.write(&b"\x1bD".repeat(rows as usize));
    }

    /// What the terminal asked of its host since the last call.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.shared.events)
    }

    /// Size reports (CSI 14 t) give the screen in pixels from these.
    /// Ghostty sizes images by them too, so a new size goes to it at once.
    pub fn set_cell_size(&mut self, width: u16, height: u16) {
        let size = &mut self.shared.size;
        if (size.cell_width, size.cell_height) == (width as u32, height as u32) {
            return;
        }
        size.cell_width = width as u32;
        size.cell_height = height as u32;
        unsafe {
            ghostty_terminal_resize(
                self.term,
                self.cols as u16,
                self.rows as u16,
                size.cell_width,
                size.cell_height,
            );
        }
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.cols = cols;
        self.rows = rows;
        let size = &mut self.shared.size;
        size.columns = cols as u16;
        size.rows = rows as u16;
        unsafe {
            ghostty_terminal_resize(
                self.term,
                cols as u16,
                rows as u16,
                size.cell_width,
                size.cell_height,
            );
        }
        set_line_limit(self.term, self.history_limit, cols);
    }

    pub fn columns(&self) -> usize {
        self.cols
    }

    pub fn screen_lines(&self) -> usize {
        self.rows
    }

    fn get<T: Default>(&self, data: GhosttyTerminalData) -> Option<T> {
        let mut value = T::default();
        let result = unsafe { terminal_get(self.term, data, &mut value) };
        (result == GHOSTTY_SUCCESS).then_some(value)
    }

    fn mode_on(&self, mode: GhosttyMode) -> bool {
        let mut config = GhosttyTerminalModeConfig { mode, value: false };
        unsafe {
            terminal_get(self.term, GHOSTTY_TERMINAL_DATA_MODE, &mut config);
        }
        config.value
    }

    pub fn mode(&self) -> TermMode {
        let mut modes = TermMode::empty();
        for (flag, value, ansi) in [
            (TermMode::SHOW_CURSOR, 25, false),
            (TermMode::APP_CURSOR, 1, false),
            (TermMode::LINE_FEED_NEW_LINE, 20, true),
            (TermMode::BRACKETED_PASTE, 2004, false),
            (TermMode::MOUSE_REPORT_CLICK, 1000, false),
            (TermMode::MOUSE_DRAG, 1002, false),
            (TermMode::MOUSE_MOTION, 1003, false),
            (TermMode::SGR_MOUSE, 1006, false),
            (TermMode::ALTERNATE_SCROLL, 1007, false),
            (TermMode::FOCUS_IN_OUT, 1004, false),
        ] {
            if self.mode_on(mode(value, ansi)) {
                modes = modes | flag;
            }
        }
        if self.get::<GhosttyTerminalScreen>(GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN)
            == Some(GHOSTTY_TERMINAL_SCREEN_ALTERNATE)
        {
            modes = modes | TermMode::ALT_SCREEN;
        }
        modes
    }

    fn scrollbar(&self) -> GhosttyTerminalScrollbar {
        self.get(GHOSTTY_TERMINAL_DATA_SCROLLBAR)
            .unwrap_or_default()
    }

    /// Lines Ghostty holds above the scrollback limit, which are left out.
    fn excess(&self, bar: &GhosttyTerminalScrollbar) -> usize {
        ((bar.total - bar.len) as usize).saturating_sub(self.history_limit)
    }

    /// Lines of history above the screen.
    pub fn history_size(&self) -> usize {
        let bar = self.scrollbar();
        (bar.total - bar.len) as usize - self.excess(&bar)
    }

    /// Lines the view is scrolled back by.
    pub fn display_offset(&self) -> usize {
        let bar = self.scrollbar();
        (bar.total - bar.len - bar.offset) as usize
    }

    fn scroll_viewport(
        &self,
        tag: GhosttyTerminalScrollViewportTag,
        value: GhosttyTerminalScrollViewportValue,
    ) {
        unsafe {
            ghostty_terminal_scroll_viewport(
                self.term,
                GhosttyTerminalScrollViewport { tag, value },
            )
        };
    }

    fn scroll_to_row(&self, row: usize) {
        self.scroll_viewport(
            GHOSTTY_TERMINAL_SCROLL_VIEWPORT_ROW,
            GhosttyTerminalScrollViewportValue { row },
        );
    }

    /// The view never goes above the lines past the limit.
    fn clamp_view(&self) {
        let bar = self.scrollbar();
        let excess = self.excess(&bar);
        if (bar.offset as usize) < excess {
            self.scroll_to_row(excess);
        }
    }

    pub fn scroll(&mut self, scroll: Scroll) {
        match scroll {
            Scroll::Delta(lines) => self.scroll_viewport(
                GHOSTTY_TERMINAL_SCROLL_VIEWPORT_DELTA,
                GhosttyTerminalScrollViewportValue {
                    delta: -lines as isize,
                },
            ),
            Scroll::Top => self.scroll_to_row(self.excess(&self.scrollbar())),
            Scroll::Bottom => self.scroll_viewport(
                GHOSTTY_TERMINAL_SCROLL_VIEWPORT_BOTTOM,
                GhosttyTerminalScrollViewportValue { row: 0 },
            ),
        }
        self.clamp_view();
    }

    /// Scrolls just far enough to show `point`, like Alacritty's `scroll_to_point`.
    pub fn scroll_to_point(&mut self, point: Point) {
        let top = self.history_size() - self.display_offset();
        if point.row < top {
            self.scroll(Scroll::Delta((top - point.row) as i32));
        } else if point.row >= top + self.rows {
            self.scroll(Scroll::Delta(-((point.row + 1 - top - self.rows) as i32)));
        }
    }

    /// The row of the view's first line, counted from the top of the kept history.
    pub fn viewport_top(&self) -> usize {
        self.history_size() - self.display_offset()
    }

    /// A palette entry, with the program's OSC 4 changes.
    pub fn color(&self, index: usize) -> u32 {
        let mut palette = [GhosttyColorRgb::default(); 256];
        unsafe {
            terminal_get(self.term, GHOSTTY_TERMINAL_DATA_COLOR_PALETTE, &mut palette);
        }
        packed(palette[index])
    }

    /// The default foreground, background and cursor colors, with the program's OSC 10, 11 and 12 changes.
    pub fn theme_colors(&self) -> (u32, u32, u32) {
        let color = |data, default| self.get::<GhosttyColorRgb>(data).map_or(default, packed);
        (
            color(GHOSTTY_TERMINAL_DATA_COLOR_FOREGROUND, FOREGROUND),
            color(GHOSTTY_TERMINAL_DATA_COLOR_BACKGROUND, BACKGROUND),
            color(GHOSTTY_TERMINAL_DATA_COLOR_CURSOR, CURSOR),
        )
    }

    /// The screen's cells and cursor.
    pub fn viewport(&mut self) -> Viewport {
        self.clamp_view();
        unsafe { ghostty_render_state_update(self.render, self.term) };
        let mut cells = Vec::with_capacity(self.rows * self.cols);
        self.read_rows(self.render, |_, row| cells.extend(row));
        let mut cursor: GhosttyRenderStateCursor = sized();
        unsafe {
            render_state_get(self.render, GHOSTTY_RENDER_STATE_DATA_CURSOR, &mut cursor);
        }
        let cursor = (cursor.visible && cursor.viewport_has_value).then(|| Cursor {
            row: cursor.viewport_y as usize,
            col: cursor.viewport_x.saturating_sub(cursor.wide_tail as u16) as usize,
            shape: match cursor.visual_style {
                GHOSTTY_CURSOR_STYLE_BAR => CursorShape::Beam,
                GHOSTTY_CURSOR_STYLE_UNDERLINE => CursorShape::Underline,
                GHOSTTY_CURSOR_STYLE_BLOCK_HOLLOW => CursorShape::HollowBlock,
                _ => CursorShape::Block,
            },
        });
        Viewport { cells, cursor }
    }

    /// The images on screen as of the last `viewport`, in the order to draw them.
    #[cfg(feature = "images")]
    pub fn images(&mut self) -> Vec<ImagePlacement> {
        let mut placements = Vec::new();
        unsafe {
            let mut iter = self.image_iter;
            if render_state_get(
                self.render,
                GHOSTTY_RENDER_STATE_DATA_IMAGE_ITERATOR,
                &mut iter,
            ) != GHOSTTY_SUCCESS
            {
                return placements;
            }
            while ghostty_render_state_image_iterator_next(iter) {
                let mut protocol: GhosttyImageProtocol = GHOSTTY_IMAGE_PROTOCOL_MAX_VALUE;
                let (mut id, mut generation, mut layer, mut z, mut col, mut row) =
                    (0u32, 0u64, 0 as GhosttyImageLayer, 0i32, 0i32, 0i32);
                let (mut offset_x, mut offset_y, mut width, mut height) = (0u32, 0u32, 0u32, 0u32);
                let mut source = [0u32; 4];
                let keys = [
                    GHOSTTY_RENDER_STATE_IMAGE_DATA_PROTOCOL,
                    GHOSTTY_RENDER_STATE_IMAGE_DATA_IMAGE_ID,
                    GHOSTTY_RENDER_STATE_IMAGE_DATA_IMAGE_GENERATION,
                    GHOSTTY_RENDER_STATE_IMAGE_DATA_LAYER,
                    GHOSTTY_RENDER_STATE_IMAGE_DATA_Z,
                    GHOSTTY_RENDER_STATE_IMAGE_DATA_VIEWPORT_COL,
                    GHOSTTY_RENDER_STATE_IMAGE_DATA_VIEWPORT_ROW,
                    GHOSTTY_RENDER_STATE_IMAGE_DATA_CELL_OFFSET_X,
                    GHOSTTY_RENDER_STATE_IMAGE_DATA_CELL_OFFSET_Y,
                    GHOSTTY_RENDER_STATE_IMAGE_DATA_PIXEL_WIDTH,
                    GHOSTTY_RENDER_STATE_IMAGE_DATA_PIXEL_HEIGHT,
                    GHOSTTY_RENDER_STATE_IMAGE_DATA_SOURCE_X,
                    GHOSTTY_RENDER_STATE_IMAGE_DATA_SOURCE_Y,
                    GHOSTTY_RENDER_STATE_IMAGE_DATA_SOURCE_WIDTH,
                    GHOSTTY_RENDER_STATE_IMAGE_DATA_SOURCE_HEIGHT,
                ];
                let [sx, sy, sw, sh] = &mut source;
                let mut values: [*mut c_void; 15] = [
                    out_ptr(&mut protocol),
                    out_ptr(&mut id),
                    out_ptr(&mut generation),
                    out_ptr(&mut layer),
                    out_ptr(&mut z),
                    out_ptr(&mut col),
                    out_ptr(&mut row),
                    out_ptr(&mut offset_x),
                    out_ptr(&mut offset_y),
                    out_ptr(&mut width),
                    out_ptr(&mut height),
                    out_ptr(sx),
                    out_ptr(sy),
                    out_ptr(sw),
                    out_ptr(sh),
                ];
                let read = ghostty_render_state_image_get_multi(
                    iter,
                    keys.len(),
                    keys.as_ptr(),
                    values.as_mut_ptr(),
                    ptr::null_mut(),
                );
                if read != GHOSTTY_SUCCESS || protocol != GHOSTTY_IMAGE_PROTOCOL_KITTY {
                    continue;
                }
                placements.push(ImagePlacement {
                    id,
                    generation,
                    layer: match layer {
                        GHOSTTY_IMAGE_LAYER_BELOW_BACKGROUND => ImageLayer::BelowBackground,
                        GHOSTTY_IMAGE_LAYER_BELOW_TEXT => ImageLayer::BelowText,
                        _ => ImageLayer::AboveText,
                    },
                    z,
                    col,
                    row,
                    offset: (offset_x, offset_y),
                    size: (width, height),
                    source,
                });
            }
        }
        placements.sort_by_key(|placement| (placement.layer, placement.z));
        placements
    }

    /// Bytes of images the pane keeps, on both screens.
    #[cfg(all(test, feature = "images"))]
    pub(crate) fn image_bytes(&self) -> u64 {
        let mut usage: GhosttyTerminalMemoryUsage = sized();
        unsafe {
            terminal_get(self.term, GHOSTTY_TERMINAL_DATA_MEMORY_USAGE, &mut usage);
        }
        usage.primary_image_bytes + usage.alternate_image_bytes
    }

    /// An image's pixels as RGBA, by its id in a placement.
    #[cfg(feature = "images")]
    pub fn image_pixels(&self, id: u32) -> Option<crate::images::Rgba> {
        let mut graphics: GhosttyKittyGraphics = ptr::null_mut();
        unsafe {
            if terminal_get(
                self.term,
                GHOSTTY_TERMINAL_DATA_KITTY_GRAPHICS,
                &mut graphics,
            ) != GHOSTTY_SUCCESS
                || graphics.is_null()
            {
                return None;
            }
            let image = ghostty_kitty_graphics_image(graphics, id);
            if image.is_null() {
                return None;
            }
            let (mut width, mut height) = (0u32, 0u32);
            let mut format: GhosttyKittyImageFormat = GHOSTTY_KITTY_IMAGE_FORMAT_MAX_VALUE;
            let mut pixels = GhosttyString::default();
            let keys = [
                GHOSTTY_KITTY_GRAPHICS_IMAGE_DATA_WIDTH,
                GHOSTTY_KITTY_GRAPHICS_IMAGE_DATA_HEIGHT,
                GHOSTTY_KITTY_GRAPHICS_IMAGE_DATA_FORMAT,
                GHOSTTY_KITTY_GRAPHICS_IMAGE_DATA_PIXELS,
            ];
            let mut values: [*mut c_void; 4] = [
                out_ptr(&mut width),
                out_ptr(&mut height),
                out_ptr(&mut format),
                out_ptr(&mut pixels),
            ];
            if ghostty_kitty_graphics_image_get_multi(
                image,
                keys.len(),
                keys.as_ptr(),
                values.as_mut_ptr(),
                ptr::null_mut(),
            ) != GHOSTTY_SUCCESS
            {
                return None;
            }
            let data = string(pixels);
            let count = width as usize * height as usize;
            let rgba: Vec<u8> = match format {
                GHOSTTY_KITTY_IMAGE_FORMAT_RGBA if data.len() == count * 4 => data.to_vec(),
                GHOSTTY_KITTY_IMAGE_FORMAT_RGB if data.len() == count * 3 => data
                    .chunks_exact(3)
                    .flat_map(|p| [p[0], p[1], p[2], 255])
                    .collect(),
                GHOSTTY_KITTY_IMAGE_FORMAT_GRAY_ALPHA if data.len() == count * 2 => data
                    .chunks_exact(2)
                    .flat_map(|p| [p[0], p[0], p[0], p[1]])
                    .collect(),
                GHOSTTY_KITTY_IMAGE_FORMAT_GRAY if data.len() == count => {
                    data.iter().flat_map(|&v| [v, v, v, 255]).collect()
                }
                _ => return None,
            };
            Some(crate::images::Rgba {
                width,
                height,
                pixels: rgba,
            })
        }
    }

    /// Calls `f` with each row a render state holds, as its position from the top of the view and its cells.
    fn read_rows(&self, render: GhosttyRenderState, mut f: impl FnMut(i32, Vec<Cell>)) {
        let mut colors: GhosttyRenderStateColors = sized();
        unsafe {
            render_state_get(render, GHOSTTY_RENDER_STATE_DATA_COLORS, &mut colors);
            let mut rows_it = self.rows_it;
            render_state_get(render, GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR, &mut rows_it);
            let (fg, bg) = (packed(colors.foreground), packed(colors.background));
            let mut utf8 = Vec::new();
            while ghostty_render_state_row_iterator_next(rows_it) {
                let mut y: i32 = 0;
                render_state_row_get(rows_it, GHOSTTY_RENDER_STATE_ROW_DATA_VIEWPORT_Y, &mut y);
                let mut raw: GhosttyRow = 0;
                render_state_row_get(rows_it, GHOSTTY_RENDER_STATE_ROW_DATA_RAW, &mut raw);
                let mut wraps = false;
                row_get(raw, GHOSTTY_ROW_DATA_WRAP, &mut wraps);
                let mut selection: GhosttyRenderStateRowSelection = sized();
                let selected = render_state_row_get(
                    rows_it,
                    GHOSTTY_RENDER_STATE_ROW_DATA_SELECTION,
                    &mut selection,
                ) == GHOSTTY_SUCCESS;
                let mut cells = self.cells;
                render_state_row_get(rows_it, GHOSTTY_RENDER_STATE_ROW_DATA_CELLS, &mut cells);
                let mut row = Vec::with_capacity(self.cols);
                while ghostty_render_state_row_cells_next(cells) {
                    let x = row.len() as u16;
                    let mut cell = read_cell(cells, fg, bg, &mut utf8);
                    cell.selected = selected && x >= selection.start_x && x <= selection.end_x;
                    row.push(cell);
                }
                if let (true, Some(last)) = (wraps, row.last_mut()) {
                    last.flags = last.flags | Flags::WRAPLINE;
                }
                f(y, row);
            }
        }
    }

    /// Calls `f` with each line of the kept history and the screen, from the top, as its row and cells.
    pub fn for_each_row(&mut self, mut f: impl FnMut(usize, &[Cell])) {
        self.for_each_row_in(0..usize::MAX, |row, cells| {
            f(row, cells);
            ControlFlow::Continue(())
        });
    }

    /// `for_each_row` for the rows in `rows`, counted from the top of the kept history, until `f` breaks.
    pub fn for_each_row_in(
        &mut self,
        rows: Range<usize>,
        mut f: impl FnMut(usize, &[Cell]) -> ControlFlow<()>,
    ) {
        let bar = self.scrollbar();
        let excess = self.excess(&bar);
        let total = (bar.total as usize).min(rows.end.saturating_add(excess));
        let was = (bar.offset as usize, bar.offset + bar.len == bar.total);
        let mut render = ptr::null_mut();
        unsafe {
            check(
                ghostty_render_state_new(ptr::null(), &mut render),
                "render_state_new",
            );
            let overscan = GhosttyRenderStateOverscan {
                above: 0,
                below: rows.len().min(SCAN_ROWS as usize) as u16,
            };
            ghostty_render_state_set(
                render,
                GHOSTTY_RENDER_STATE_OPT_OVERSCAN,
                &overscan as *const _ as *const c_void,
            );
        }
        // The view moves through the history a chunk at a time, then goes back where it was.
        let mut start = rows.start.saturating_add(excess);
        let mut stopped = false;
        while start < total && !stopped {
            self.scroll_to_row(start);
            let top = self.scrollbar().offset as usize;
            unsafe { ghostty_render_state_update(render, self.term) };
            let mut next = start;
            self.read_rows(render, |y, row| {
                let row_at = (top as i64 + y as i64) as usize;
                if row_at >= next && row_at < total && !stopped {
                    stopped = f(row_at - excess, &row).is_break();
                    next = row_at + 1;
                }
            });
            if next <= start {
                break;
            }
            start = next;
        }
        unsafe { ghostty_render_state_free(render) };
        if was.1 {
            self.scroll(Scroll::Bottom);
        } else {
            self.scroll_to_row(was.0);
        }
    }

    /// Rows that changed since `reset_damage`, or `None` when all of them did.
    pub fn damage(&mut self) -> Option<Vec<usize>> {
        unsafe {
            ghostty_render_state_update(self.render, self.term);
            let mut dirty: GhosttyRenderStateDirty = GHOSTTY_RENDER_STATE_DIRTY_FULL;
            render_state_get(self.render, GHOSTTY_RENDER_STATE_DATA_DIRTY, &mut dirty);
            if dirty == GHOSTTY_RENDER_STATE_DIRTY_FULL {
                return None;
            }
            let mut lines = Vec::new();
            if dirty == GHOSTTY_RENDER_STATE_DIRTY_FALSE {
                return Some(lines);
            }
            let mut rows_it = self.rows_it;
            render_state_get(
                self.render,
                GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR,
                &mut rows_it,
            );
            let mut y = 0;
            while ghostty_render_state_row_iterator_next(rows_it) {
                let mut row_dirty = false;
                render_state_row_get(rows_it, GHOSTTY_RENDER_STATE_ROW_DATA_DIRTY, &mut row_dirty);
                if row_dirty {
                    lines.push(y);
                }
                y += 1;
            }
            Some(lines)
        }
    }

    /// The native renderer and benchmark frame encoder are alternative damage consumers.
    pub fn reset_damage(&mut self) {
        unsafe {
            ghostty_render_state_update(self.render, self.term);
            ghostty_render_state_clean(self.render);
        }
    }

    fn grid_ref(&self, tag: GhosttyPointTag, x: usize, y: usize) -> Option<GhosttyGridRef> {
        let point = GhosttyPoint {
            tag,
            value: GhosttyPointValue {
                coordinate: GhosttyPointCoordinate {
                    x: x as u16,
                    y: y as u32,
                },
            },
        };
        let mut out = GhosttyGridRef::default();
        let result = unsafe { ghostty_terminal_grid_ref(self.term, point, &mut out) };
        (result == GHOSTTY_SUCCESS).then_some(out)
    }

    /// A reference's place in the whole screen, scrollback first.
    fn screen_point(&self, at: &GhosttyGridRef) -> (usize, usize) {
        let mut out = GhosttyPointCoordinate::default();
        unsafe {
            ghostty_terminal_point_from_grid_ref(self.term, at, GHOSTTY_POINT_TAG_SCREEN, &mut out)
        };
        (out.y as usize, out.x as usize)
    }

    /// Starts a selection at a cell of the view.
    pub fn select_from(&mut self, row: usize, col: usize, side: Side, ty: SelectionType) {
        self.clear_selection();
        let Some(at) = self.grid_ref(GHOSTTY_POINT_TAG_VIEWPORT, col, row) else {
            return;
        };
        let (y, x) = self.screen_point(&at);
        let point = GhosttyPoint {
            tag: GHOSTTY_POINT_TAG_SCREEN,
            value: GhosttyPointValue {
                coordinate: GhosttyPointCoordinate {
                    x: x as u16,
                    y: y as u32,
                },
            },
        };
        let mut anchor = ptr::null_mut();
        if unsafe { ghostty_terminal_grid_ref_track(self.term, point, &mut anchor) }
            != GHOSTTY_SUCCESS
        {
            return;
        }
        self.selecting = Some(Selecting { ty, anchor, side });
        self.select_to(row, col, side);
    }

    /// Moves the selection's other end to a cell of the view, like Alacritty's selections: a simple one takes
    /// the cells between the two halves the pointer was on, a semantic one whole words, a line one whole lines.
    pub fn select_to(&mut self, row: usize, col: usize, side: Side) {
        let Some(selecting) = &self.selecting else {
            return;
        };
        let mut anchor = GhosttyGridRef::default();
        let (Some(end), true) = (
            self.grid_ref(GHOSTTY_POINT_TAG_VIEWPORT, col, row),
            unsafe { ghostty_tracked_grid_ref_snapshot(selecting.anchor, &mut anchor) }
                == GHOSTTY_SUCCESS,
        ) else {
            return;
        };
        let (ty, anchor_side) = (selecting.ty, selecting.side);
        let mut ends = [
            (self.screen_point(&anchor), anchor_side),
            (self.screen_point(&end), side),
        ];
        ends.sort_by_key(|((y, x), side)| (*y, *x, *side == Side::Right));
        let [(mut start, start_side), (mut last, last_side)] = ends;
        let cols = self.cols;
        let selection = match ty {
            SelectionType::Simple => {
                if start_side == Side::Right {
                    start = if start.1 + 1 < cols {
                        (start.0, start.1 + 1)
                    } else {
                        (start.0 + 1, 0)
                    };
                }
                if last_side == Side::Left {
                    if last.1 > 0 {
                        last.1 -= 1;
                    } else if last.0 > 0 {
                        last = (last.0 - 1, cols - 1);
                    } else {
                        return self.install(None);
                    }
                }
                if start > last {
                    return self.install(None);
                }
                self.grid_ref(GHOSTTY_POINT_TAG_SCREEN, start.1, start.0)
                    .zip(self.grid_ref(GHOSTTY_POINT_TAG_SCREEN, last.1, last.0))
            }
            SelectionType::Semantic | SelectionType::Lines => {
                let around = |(y, x): (usize, usize)| -> Option<GhosttySelection> {
                    let at = self.grid_ref(GHOSTTY_POINT_TAG_SCREEN, x, y)?;
                    let mut out = GhosttySelection::default();
                    let result = unsafe {
                        if ty == SelectionType::Semantic {
                            let mut options: GhosttyTerminalSelectWordOptions = sized();
                            options.ref_ = at;
                            ghostty_terminal_select_word(self.term, &options, &mut out)
                        } else {
                            let mut options: GhosttyTerminalSelectLineOptions = sized();
                            options.ref_ = at;
                            ghostty_terminal_select_line(self.term, &options, &mut out)
                        }
                    };
                    if result == GHOSTTY_SUCCESS {
                        Some(out)
                    } else {
                        Some(GhosttySelection {
                            start: at,
                            end: at,
                            rectangle: false,
                        })
                    }
                };
                around(start)
                    .zip(around(last))
                    .map(|(first, last)| (first.start, last.end))
            }
        };
        self.install(selection.map(|(start, end)| GhosttySelection {
            start,
            end,
            rectangle: false,
        }));
    }

    fn install(&self, selection: Option<GhosttySelection>) {
        let value = selection.as_ref().map_or(ptr::null(), |selection| {
            selection as *const _ as *const c_void
        });
        unsafe { ghostty_terminal_set(self.term, GHOSTTY_TERMINAL_OPT_SELECTION, value) };
    }

    pub fn clear_selection(&mut self) {
        if let Some(selecting) = self.selecting.take() {
            unsafe { ghostty_tracked_grid_ref_free(selecting.anchor) };
        }
        self.install(None);
    }

    /// The selected text as Ghostty copies it: wrapped lines joined and the spaces at line ends left out.
    pub fn selection_to_string(&self) -> Option<String> {
        let selection = self.get::<GhosttySelection>(GHOSTTY_TERMINAL_DATA_SELECTION)?;
        let text = self.format(
            GHOSTTY_FORMATTER_FORMAT_PLAIN,
            Some(&selection),
            true,
            true,
            GHOSTTY_FORMATTER_EXTRAS_NONE,
        );
        Some(String::from_utf8_lossy(&text).into_owned())
    }

    /// The screen, or a part of it, as text or as the VT sequences that draw it again.
    pub(crate) fn format(
        &self,
        emit: GhosttyFormatterFormat,
        selection: Option<&GhosttySelection>,
        unwrap: bool,
        trim: bool,
        extras: u32,
    ) -> Vec<u8> {
        unsafe {
            let mut formatter = ptr::null_mut();
            check(
                ghostty_formatter_terminal_new(ptr::null(), &mut formatter, self.term),
                "formatter_new",
            );
            let set = |option, value: *const c_void| {
                check(
                    ghostty_formatter_set(formatter, option, value),
                    "formatter_set",
                )
            };
            set(
                GHOSTTY_FORMATTER_OPT_EMIT,
                &emit as *const _ as *const c_void,
            );
            set(
                GHOSTTY_FORMATTER_OPT_UNWRAP,
                &unwrap as *const _ as *const c_void,
            );
            set(
                GHOSTTY_FORMATTER_OPT_TRIM,
                &trim as *const _ as *const c_void,
            );
            set(
                GHOSTTY_FORMATTER_OPT_EXTRAS,
                &extras as *const _ as *const c_void,
            );
            if let Some(selection) = selection {
                set(
                    GHOSTTY_FORMATTER_OPT_SELECTION,
                    selection as *const _ as *const c_void,
                );
            }
            let (mut out, mut len) = (ptr::null_mut(), 0);
            check(
                ghostty_formatter_format_alloc(formatter, ptr::null(), &mut out, &mut len),
                "format",
            );
            let bytes = if out.is_null() {
                Vec::new()
            } else {
                std::slice::from_raw_parts(out, len).to_vec()
            };
            ghostty_free(ptr::null(), out, len);
            ghostty_formatter_free(formatter);
            bytes
        }
    }

    /// The selection covering lines `first..=last` of the whole screen (scrollback included).
    pub(crate) fn rows_selection(&self, first: usize, last: usize) -> Option<GhosttySelection> {
        Some(GhosttySelection {
            start: self.grid_ref(GHOSTTY_POINT_TAG_SCREEN, 0, first)?,
            end: self.grid_ref(GHOSTTY_POINT_TAG_SCREEN, self.cols - 1, last)?,
            rectangle: false,
        })
    }

    /// How many rows up from `last` of the whole screen have no text.
    pub(crate) fn blank_rows_up_to(&self, last: usize) -> usize {
        let has_text = |y: usize| {
            (0..self.cols).any(|x| {
                let Some(at) = self.grid_ref(GHOSTTY_POINT_TAG_SCREEN, x, y) else {
                    return false;
                };
                let (mut cell, mut text): (GhosttyCell, bool) = (0, false);
                unsafe {
                    ghostty_grid_ref_cell(&at, &mut cell) == GHOSTTY_SUCCESS
                        && cell_get(cell, GHOSTTY_CELL_DATA_HAS_TEXT, &mut text) == GHOSTTY_SUCCESS
                        && text
                }
            })
        };
        (0..=last).rev().take_while(|&y| !has_text(y)).count()
    }

    /// Rows of the whole screen above the kept history, and the history rows: `(excess, history)`.
    pub(crate) fn history_rows(&self) -> (usize, usize) {
        let bar = self.scrollbar();
        let excess = self.excess(&bar);
        (excess, (bar.total - bar.len) as usize - excess)
    }

    /// Swaps in a terminal made with `make`, which gets this one's size, theme, callbacks and options.
    pub(crate) fn replace_terminal(&mut self, make: impl FnOnce(&mut GridPane)) {
        let mut next = GridPane::new(self.cols, self.rows);
        next.shared.size = self.shared.size;
        next.set_options(self.cursor, self.history_limit);
        #[cfg(feature = "images")]
        next.set_images(self.images_on());
        unsafe {
            // The new terminal reports to this pane's callbacks.
            ghostty_terminal_set(
                next.term,
                GHOSTTY_TERMINAL_OPT_USERDATA,
                &mut *self.shared as *mut Shared as *const c_void,
            );
            ghostty_terminal_resize(
                next.term,
                self.cols as u16,
                self.rows as u16,
                self.shared.size.cell_width,
                self.shared.size.cell_height,
            );
        }
        let events = self.shared.events.len();
        make(&mut next);
        // Restoring draws the screen again; nothing it sets off belongs to the program.
        self.shared.events.truncate(events);
        self.clear_selection();
        std::mem::swap(&mut self.term, &mut next.term);
        std::mem::swap(&mut self.render, &mut next.render);
        // `next` now owns the old terminal; its own callbacks point at `next.shared` again before it is freed.
        unsafe {
            ghostty_terminal_set(
                next.term,
                GHOSTTY_TERMINAL_OPT_USERDATA,
                &mut *next.shared as *mut Shared as *const c_void,
            );
        }
    }

    /// Appends a frame of the rows changed since the last call, or of every row if `full`, to `out`.
    /// Returns false, appending nothing, when nothing changed.
    pub fn frame(&mut self, full: bool, out: &mut Vec<u8>) -> bool {
        let lines: Vec<usize> = match self.damage() {
            None => (0..self.rows).collect(),
            _ if full => (0..self.rows).collect(),
            Some(lines) => lines,
        };
        self.reset_damage();
        if lines.is_empty() {
            return false;
        }
        let Viewport { cells, cursor } = self.viewport();
        match cursor {
            Some(cursor) => {
                put16(out, cursor.row as u16);
                put16(out, cursor.col as u16);
            }
            None => {
                put16(out, 0xffff);
                put16(out, 0);
            }
        }
        put16(out, lines.len() as u16);
        let mut text = String::new();
        let style = |cell: &Cell| {
            let f = cell.flags;
            let attrs = f.contains(Flags::BOLD) as u32
                | (f.contains(Flags::ITALIC) as u32) << 1
                | (f.intersects(Flags::ALL_UNDERLINES) as u32) << 2
                | (f.contains(Flags::STRIKEOUT) as u32) << 3;
            (cell.fg | attrs << 24, cell.bg)
        };
        let char_of = |cell: &Cell| cell.text.chars().next().unwrap_or(' ');
        for line in lines {
            let row = &cells[line * self.cols..(line + 1) * self.cols];
            put16(out, line as u16);
            let count_at = out.len();
            put16(out, 0);
            let mut runs = 0u16;
            let mut col = 0;
            while col < self.cols {
                let cell = &row[col];
                let (fg, bg) = style(cell);
                let start = col;
                text.clear();
                if shares_cells(char_of(cell)) && cell.text.chars().nth(1).is_none() {
                    while col < self.cols {
                        let c = &row[col];
                        if !shares_cells(char_of(c))
                            || c.text.chars().nth(1).is_some()
                            || style(c) != (fg, bg)
                        {
                            break;
                        }
                        text.push(char_of(c));
                        col += 1;
                    }
                } else {
                    text.push_str(&cell.text);
                    col += if cell.flags.contains(Flags::WIDE_CHAR) {
                        2
                    } else {
                        1
                    };
                }
                if text.bytes().all(|b| b == b' ') {
                    text.clear();
                }
                put32(out, fg);
                put32(out, bg);
                put16(out, start as u16);
                put16(out, (col - start).min(self.cols - start) as u16);
                put16(out, text.len() as u16);
                out.extend_from_slice(text.as_bytes());
                runs += 1;
            }
            out[count_at..count_at + 2].copy_from_slice(&runs.to_le_bytes());
        }
        true
    }
}

impl Drop for GridPane {
    fn drop(&mut self) {
        self.clear_selection();
        unsafe {
            ghostty_render_state_row_cells_free(self.cells);
            ghostty_render_state_row_iterator_free(self.rows_it);
            #[cfg(feature = "images")]
            ghostty_render_state_image_iterator_free(self.image_iter);
            ghostty_render_state_free(self.render);
            ghostty_terminal_free(self.term);
        }
    }
}

/// The cell a render-state cell iterator is on, with inverse, dim and hidden applied to its colors.
unsafe fn read_cell(
    cells: GhosttyRenderStateRowCells,
    default_fg: u32,
    default_bg: u32,
    utf8: &mut Vec<u8>,
) -> Cell {
    unsafe {
        let mut raw: GhosttyCell = 0;
        render_state_row_cells_get(cells, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_RAW, &mut raw);
        let mut wide: GhosttyCellWide = GHOSTTY_CELL_WIDE_NARROW;
        cell_get(raw, GHOSTTY_CELL_DATA_WIDE, &mut wide);
        let mut style = GhosttyStyle::default();
        render_state_row_cells_get(cells, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_STYLE, &mut style);
        let mut color = GhosttyColorRgb::default();
        let mut fg = if render_state_row_cells_get(
            cells,
            GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_FG_COLOR,
            &mut color,
        ) == GHOSTTY_SUCCESS
        {
            packed(color)
        } else {
            default_fg
        };
        let mut bg = if render_state_row_cells_get(
            cells,
            GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_BG_COLOR,
            &mut color,
        ) == GHOSTTY_SUCCESS
        {
            packed(color)
        } else {
            default_bg
        };
        let style_flags = style.flags;
        let on = |bit: u16| style_flags & bit != 0;
        let mut flags = Flags::empty();
        for (bit, flag) in [
            (STYLE_BOLD, Flags::BOLD),
            (STYLE_ITALIC, Flags::ITALIC),
            (STYLE_FAINT, Flags::DIM),
            (STYLE_INVERSE, Flags::INVERSE),
            (STYLE_INVISIBLE, Flags::HIDDEN),
            (STYLE_STRIKETHROUGH, Flags::STRIKEOUT),
        ] {
            if on(bit) {
                flags = flags | flag;
            }
        }
        flags = flags
            | match ((style_flags >> 8) & 0x7) as GhosttySgrUnderline {
                GHOSTTY_SGR_UNDERLINE_SINGLE => Flags::UNDERLINE,
                GHOSTTY_SGR_UNDERLINE_DOUBLE => Flags::DOUBLE_UNDERLINE,
                GHOSTTY_SGR_UNDERLINE_CURLY => Flags::UNDERCURL,
                GHOSTTY_SGR_UNDERLINE_DOTTED => Flags::DOTTED_UNDERLINE,
                GHOSTTY_SGR_UNDERLINE_DASHED => Flags::DASHED_UNDERLINE,
                _ => Flags::empty(),
            }
            | match wide {
                GHOSTTY_CELL_WIDE_WIDE => Flags::WIDE_CHAR,
                GHOSTTY_CELL_WIDE_SPACER_TAIL => Flags::WIDE_CHAR_SPACER,
                GHOSTTY_CELL_WIDE_SPACER_HEAD => Flags::LEADING_WIDE_CHAR_SPACER,
                _ => Flags::empty(),
            };
        if flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
        }
        if flags.contains(Flags::DIM) {
            fg = dim(fg, bg);
        }
        if flags.contains(Flags::HIDDEN) {
            fg = bg;
        }
        let mut text = String::new();
        if wide == GHOSTTY_CELL_WIDE_NARROW || wide == GHOSTTY_CELL_WIDE_WIDE {
            let mut buffer = GhosttyBuffer {
                ptr: utf8.as_mut_ptr(),
                cap: utf8.capacity(),
                len: 0,
            };
            let mut result = render_state_row_cells_get(
                cells,
                GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8,
                &mut buffer,
            );
            if result == GHOSTTY_OUT_OF_SPACE {
                utf8.reserve(buffer.len);
                buffer = GhosttyBuffer {
                    ptr: utf8.as_mut_ptr(),
                    cap: utf8.capacity(),
                    len: 0,
                };
                result = render_state_row_cells_get(
                    cells,
                    GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8,
                    &mut buffer,
                );
            }
            if result == GHOSTTY_SUCCESS && buffer.len > 0 {
                text = String::from_utf8_lossy(std::slice::from_raw_parts(buffer.ptr, buffer.len))
                    .into_owned();
            }
        }
        Cell {
            text,
            fg,
            bg,
            flags,
            selected: false,
        }
    }
}

fn put16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// A PTY recording: `u16 cols, u16 rows`, then chunks of `u32 ms since start, u32 len, bytes`.
pub struct Recording {
    pub cols: usize,
    pub rows: usize,
    pub chunks: Vec<(u32, Vec<u8>)>,
}

impl Recording {
    pub fn parse(data: &[u8]) -> Option<Recording> {
        let u16_at = |i: usize| {
            data.get(i..i + 2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]) as usize)
        };
        let u32_at = |i: usize| {
            data.get(i..i + 4)
                .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        };
        let (cols, rows) = (u16_at(0)?, u16_at(2)?);
        let mut chunks = Vec::new();
        let mut i = 4;
        while i < data.len() {
            let (t, n) = (u32_at(i)?, u32_at(i + 4)? as usize);
            chunks.push((t, data.get(i + 8..i + 8 + n)?.to_vec()));
            i += 8 + n;
        }
        Some(Recording { cols, rows, chunks })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads the runs of a frame back as (row, col, text) with the fg color.
    fn runs(frame: &[u8]) -> Vec<(u16, u16, String, u32)> {
        let r16 = |i: usize| u16::from_le_bytes([frame[i], frame[i + 1]]);
        let r32 =
            |i: usize| u32::from_le_bytes([frame[i], frame[i + 1], frame[i + 2], frame[i + 3]]);
        let mut out = Vec::new();
        let mut i = 6;
        for _ in 0..r16(4) {
            let (row, n) = (r16(i), r16(i + 2));
            i += 4;
            for _ in 0..n {
                let (fg, col, len) = (r32(i), r16(i + 8), r16(i + 12) as usize);
                out.push((
                    row,
                    col,
                    String::from_utf8(frame[i + 14..i + 14 + len].to_vec()).unwrap(),
                    fg,
                ));
                i += 14 + len;
            }
        }
        out
    }

    #[test]
    fn colored_text_becomes_runs_and_only_changed_rows_are_sent() {
        let mut pane = GridPane::new(20, 4);
        pane.feed(b"ab\x1b[31mcd\x1b[0m \xe2\x9c\xbb e");
        let mut frame = Vec::new();
        assert!(pane.frame(false, &mut frame));
        let row0: Vec<_> = runs(&frame).into_iter().filter(|r| r.0 == 0).collect();
        assert_eq!(row0[0], (0, 0, "ab".into(), FOREGROUND));
        assert_eq!(row0[1], (0, 2, "cd".into(), ANSI[1]));
        assert_eq!(row0[3], (0, 5, "\u{273b}".into(), FOREGROUND));
        frame.clear();
        pane.feed(b"\x1b[3;1Hx");
        assert!(pane.frame(false, &mut frame));
        let rows: std::collections::BTreeSet<u16> = runs(&frame).iter().map(|r| r.0).collect();
        assert!(rows.contains(&2) && rows.len() <= 2, "{rows:?}");
    }

    #[test]
    fn split_utf8_and_ansi_preserve_wide_and_combining_cells() {
        let mut pane = GridPane::new(20, 4);
        for byte in "\x1b[31m界e\u{301}".as_bytes() {
            pane.feed(std::slice::from_ref(byte));
        }
        let row = pane.viewport().cells;
        assert_eq!(row[0].text, "界");
        assert!(row[0].flags.contains(Flags::WIDE_CHAR));
        assert!(row[1].flags.contains(Flags::WIDE_CHAR_SPACER));
        assert_eq!(row[2].text, "e\u{301}");
        assert_eq!(row[0].fg, pane.color(1));
    }

    #[test]
    fn native_cell_colors_keep_the_frame_style_rules() {
        let mut pane = GridPane::new(20, 4);
        pane.feed(b"\x1b[7mx\x1b[0;2my\x1b[0;8mz");
        let row = pane.viewport().cells;
        assert_eq!((row[0].fg, row[0].bg), (BACKGROUND, FOREGROUND));
        assert_eq!(
            (row[1].fg, row[1].bg),
            (dim(FOREGROUND, BACKGROUND), BACKGROUND)
        );
        assert_eq!((row[2].fg, row[2].bg), (BACKGROUND, BACKGROUND));
    }

    #[test]
    fn alternate_screen_resize_cursor_and_damage_use_the_same_model() {
        let mut pane = GridPane::new(20, 4);
        pane.feed(b"main\x1b[?1049h\x1b[Halt");
        assert!(pane.mode().contains(TermMode::ALT_SCREEN));
        assert_eq!(pane.viewport().cells[0].text, "a");
        pane.resize(24, 6);
        assert_eq!((pane.columns(), pane.screen_lines()), (24, 6));
        assert_eq!(pane.viewport().cells.len(), 24 * 6);
        assert!(pane.damage().is_none());
        let mut frame = Vec::new();
        assert!(pane.frame(false, &mut frame));
        assert_eq!(u16::from_le_bytes([frame[4], frame[5]]), 6);
        pane.feed(b"\x1b[?1049l");
        assert!(!pane.mode().contains(TermMode::ALT_SCREEN));
        assert_eq!(pane.viewport().cells[0].text, "m");
        pane.reset_damage();
        pane.feed(b"\x1b[3;4H\x1b[?25l");
        let lines = pane
            .damage()
            .expect("cursor movement should not damage the full screen");
        assert!(lines.contains(&0) && lines.contains(&2), "{lines:?}");
        assert_eq!(pane.viewport().cursor, None);
        frame.clear();
        pane.frame(false, &mut frame);
        assert_eq!(&frame[..2], &0xffffu16.to_le_bytes());
        pane.feed(b"\x1b[?25h");
        let cursor = pane.viewport().cursor.unwrap();
        assert_eq!((cursor.row, cursor.col), (2, 3));
    }

    fn replies(pane: &mut GridPane) -> Vec<String> {
        pane.take_events()
            .into_iter()
            .filter_map(|event| match event {
                Event::PtyWrite(bytes) => Some(String::from_utf8(bytes).unwrap()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn live_listener_replies_with_current_size_and_shared_palette() {
        let mut pane = GridPane::new(20, 4);
        pane.set_cell_size(8, 16);
        let reply = |pane: &mut GridPane, query: &[u8]| {
            pane.feed(query);
            replies(pane).concat()
        };
        assert_eq!(reply(&mut pane, b"\x1b[6n"), "\x1b[1;1R");
        assert_eq!(reply(&mut pane, b"\x1b[18t"), "\x1b[8;4;20t");
        assert_eq!(reply(&mut pane, b"\x1b[14t"), "\x1b[4;64;160t");
        for (code, color) in [(10, FOREGROUND), (11, BACKGROUND), (12, CURSOR)] {
            let (r, g, b) = ((color >> 16) & 0xff, (color >> 8) & 0xff, color & 0xff);
            let expected =
                format!("\x1b]{code};rgb:{r:02x}{r:02x}/{g:02x}{g:02x}/{b:02x}{b:02x}\x07");
            assert_eq!(
                reply(&mut pane, format!("\x1b]{code};?\x07").as_bytes()),
                expected
            );
        }
        assert_eq!(
            reply(
                &mut pane,
                b"\x1b]4;1;rgb:12/34/56\x07\x1b]4;1;?\x07\x1b[31mx"
            ),
            "\x1b]4;1;rgb:1212/3434/5656\x07"
        );
        let mut frame = Vec::new();
        pane.frame(true, &mut frame);
        assert_eq!(runs(&frame)[0].3, 0x123456);
        pane.feed(b"\x1b]104;1\x07");
        assert_eq!(pane.color(1), ANSI[1]);
        pane.resize(30, 8);
        assert_eq!(reply(&mut pane, b"\x1b[18t"), "\x1b[8;8;30t");
        assert_eq!(reply(&mut pane, b"\x1b[14t"), "\x1b[4;128;240t");
    }

    #[test]
    fn titles_and_clipboard_writes_reach_the_host() {
        let mut pane = GridPane::new(20, 4);
        pane.feed(b"\x1b]0;hello\x07\x1b]52;c;aGk=\x07\x1b]2;\x07");
        assert_eq!(
            pane.take_events(),
            vec![
                Event::Title(Some("hello".into())),
                Event::Clipboard("hi".into()),
                Event::Title(None),
            ]
        );
    }
}
