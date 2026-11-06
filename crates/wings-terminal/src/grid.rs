//! Shared Rust-parsed terminal: `alacritty_terminal` keeps the grid, and each benchmark frame carries
//! the rows that changed as style runs, ready for `fillRect` + `fillText` on a canvas (`src/bench-canvas.ts`).
//!
//! Frame layout, little-endian:
//! `u16 cursor_row (0xFFFF = hidden), u16 cursor_col, u16 row_count`, then per row
//! `u16 row, u16 run_count`, then per run `u32 fg, u32 bg, u16 col, u16 cells, u16 text_len, text (UTF-8)`.
//! Colors are 0xRRGGBB; fg's top byte holds bold (1), italic (2), underline (4) and strikeout (8).
//! A run is either printable ASCII, box drawing and block characters (one cell each, in every monospace font),
//! or one other character on its own, so a fallback-font glyph never shifts the cells after it.
//! An all-space run has no text: its background is all there is to draw.

use alacritty_terminal::{
    event::{Event, EventListener, VoidListener, WindowSize},
    index::{Column, Line},
    term::{cell::Flags, test::TermSize, Config, TermDamage},
    vte::ansi::{Color, CursorShape, CursorStyle, NamedColor, Processor, Rgb},
    Term,
};

/// Wings' terminal theme (`src/lib/terminal.ts`), so both renderers show the same colors.
const ANSI: [u32; 16] = [
    0x4a4650, 0xf2786a, 0x8fd19e, 0xe9c46a, 0x7aa7f2, 0xd59cf0, 0x6fd0d0, 0xd8d4ce, 0x6d6872, 0xff9285, 0xa7e3b3,
    0xf3d68b, 0x9cc0ff, 0xe4b8f7, 0x93e2e2, 0xf4f1ec,
];
pub const FOREGROUND: u32 = 0xe9e6e1;
pub const BACKGROUND: u32 = 0x17151b;
pub const CURSOR: u32 = 0xf0ede8;

/// Lines of scrollback per pane, as in the desktop terminal.
pub const HISTORY_LINES: usize = 10_000;

pub struct GridPane<L = VoidListener> {
    term: Term<L>,
    parser: Processor,
    rows: usize,
    cols: usize,
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

fn default_color(index: usize) -> u32 {
    match index {
        0..=255 => indexed(index as u8),
        _ if index == NamedColor::Background as usize => BACKGROUND,
        _ if index == NamedColor::Cursor as usize => CURSOR,
        _ if (NamedColor::DimBlack as usize..=NamedColor::DimWhite as usize).contains(&index) => {
            dim(ANSI[index - NamedColor::DimBlack as usize], BACKGROUND)
        }
        _ if index == NamedColor::DimForeground as usize => dim(FOREGROUND, BACKGROUND),
        _ => FOREGROUND,
    }
}

fn packed(color: Rgb) -> u32 {
    ((color.r as u32) << 16) | ((color.g as u32) << 8) | color.b as u32
}

/// Two thirds of the way from `bg` to `fg`.
fn dim(fg: u32, bg: u32) -> u32 {
    let mix = |s: u32| (((fg >> s) & 0xff) * 2 + ((bg >> s) & 0xff)) / 3;
    (mix(16) << 16) | (mix(8) << 8) | mix(0)
}

fn shares_cells(c: char) -> bool {
    matches!(c, ' '..='~' | '\u{2500}'..='\u{259f}')
}

impl GridPane {
    pub fn new(cols: usize, rows: usize) -> GridPane {
        Self::with_listener(cols, rows, VoidListener)
    }
}

impl<L: EventListener> GridPane<L> {
    pub fn with_listener(cols: usize, rows: usize, listener: L) -> Self {
        let config = Config { scrolling_history: HISTORY_LINES, ..Config::default() };
        Self { term: Term::new(config, &TermSize::new(cols, rows), listener), parser: Processor::new(), rows, cols }
    }

    /// The cursor programs get until they set their own, like the desktop's cursor setting.
    pub fn set_default_cursor(&mut self, shape: CursorShape) {
        let default_cursor_style = CursorStyle { shape, blinking: false };
        self.term.set_options(Config { scrolling_history: HISTORY_LINES, default_cursor_style, ..Config::default() });
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.term.resize(TermSize::new(cols, rows));
        self.cols = cols;
        self.rows = rows;
    }

    /// Native renderers use Alacritty's cells, combining characters, cursor, and damage directly.
    pub fn model(&self) -> &Term<L> {
        &self.term
    }

    /// Scrolling and selection change the view without parsing new output.
    pub fn model_mut(&mut self) -> &mut Term<L> {
        &mut self.term
    }

    /// Resolves a queried palette index, including OSC color overrides, with the frame's theme.
    pub fn color(&self, index: usize) -> Rgb {
        self.term.colors()[index].unwrap_or_else(|| {
            let color = default_color(index);
            Rgb { r: (color >> 16) as u8, g: (color >> 8) as u8, b: color as u8 }
        })
    }

    pub fn resolve_color(&self, color: Color) -> Rgb {
        match color {
            Color::Spec(color) => color,
            Color::Indexed(index) => self.color(index as usize),
            Color::Named(color) => self.color(color as usize),
        }
    }

    /// The same inverse, dim, and hidden-cell colors for native and benchmark renderers.
    pub fn cell_colors(&self, cell: &alacritty_terminal::term::cell::Cell) -> (u32, u32) {
        let (mut fg, mut bg) = (packed(self.resolve_color(cell.fg)), packed(self.resolve_color(cell.bg)));
        if cell.flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
        }
        if cell.flags.contains(Flags::DIM) {
            fg = dim(fg, bg);
        }
        if cell.flags.contains(Flags::HIDDEN) {
            fg = bg;
        }
        (fg, bg)
    }

    /// Drain listener events after feed and write these replies back through the host's PTY writer.
    /// Other events (title, clipboard, bell, etc.) remain the host's responsibility.
    pub fn protocol_reply(&self, event: &Event, cell_width: u16, cell_height: u16) -> Option<String> {
        match event {
            Event::PtyWrite(text) => Some(text.clone()),
            Event::ColorRequest(index, format) => Some(format(self.color(*index))),
            Event::TextAreaSizeRequest(format) => Some(format(WindowSize {
                num_lines: self.rows as u16,
                num_cols: self.cols as u16,
                cell_width,
                cell_height,
            })),
            _ => None,
        }
    }

    pub fn damage(&mut self) -> TermDamage<'_> {
        self.term.damage()
    }

    /// The native renderer and benchmark frame encoder are alternative damage consumers.
    pub fn reset_damage(&mut self) {
        self.term.reset_damage();
    }

    /// Appends a frame of the rows changed since the last call, or of every row if `full`, to `out`.
    /// Returns false, appending nothing, when nothing changed.
    pub fn frame(&mut self, full: bool, out: &mut Vec<u8>) -> bool {
        let lines: Vec<usize> = match self.term.damage() {
            TermDamage::Full => (0..self.rows).collect(),
            _ if full => (0..self.rows).collect(),
            TermDamage::Partial(it) => it.filter(|d| d.is_damaged()).map(|d| d.line).collect(),
        };
        self.term.reset_damage();
        if lines.is_empty() {
            return false;
        }
        let cursor = self.term.renderable_content().cursor;
        let hidden = cursor.shape == CursorShape::Hidden || cursor.point.line.0 < 0;
        put16(out, if hidden { 0xffff } else { cursor.point.line.0 as u16 });
        put16(out, cursor.point.column.0 as u16);
        put16(out, lines.len() as u16);
        let grid = self.term.grid();
        let mut text = String::new();
        for line in lines {
            let row = &grid[Line(line as i32)];
            put16(out, line as u16);
            let count_at = out.len();
            put16(out, 0);
            let mut runs = 0u16;
            let mut col = 0;
            while col < self.cols {
                let cell = &row[Column(col)];
                let style = |cell: &alacritty_terminal::term::cell::Cell| {
                    let (fg, bg) = self.cell_colors(cell);
                    let f = cell.flags;
                    let attrs = f.contains(Flags::BOLD) as u32
                        | (f.contains(Flags::ITALIC) as u32) << 1
                        | (f.intersects(Flags::ALL_UNDERLINES) as u32) << 2
                        | (f.contains(Flags::STRIKEOUT) as u32) << 3;
                    (fg | attrs << 24, bg)
                };
                let (fg, bg) = style(cell);
                let start = col;
                text.clear();
                if shares_cells(cell.c) {
                    while col < self.cols {
                        let c = &row[Column(col)];
                        if !shares_cells(c.c) || style(c) != (fg, bg) {
                            break;
                        }
                        text.push(c.c);
                        col += 1;
                    }
                } else {
                    text.push(cell.c);
                    col += if cell.flags.contains(Flags::WIDE_CHAR) { 2 } else { 1 };
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
        let u16_at = |i: usize| data.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize);
        let u32_at = |i: usize| data.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
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
        let r32 = |i: usize| u32::from_le_bytes([frame[i], frame[i + 1], frame[i + 2], frame[i + 3]]);
        let mut out = Vec::new();
        let mut i = 6;
        for _ in 0..r16(4) {
            let (row, n) = (r16(i), r16(i + 2));
            i += 4;
            for _ in 0..n {
                let (fg, col, len) = (r32(i), r16(i + 8), r16(i + 12) as usize);
                out.push((row, col, String::from_utf8(frame[i + 14..i + 14 + len].to_vec()).unwrap(), fg));
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
        let row = &pane.model().grid()[Line(0)];
        assert_eq!(row[Column(0)].c, '界');
        assert!(row[Column(0)].flags.contains(Flags::WIDE_CHAR));
        assert!(row[Column(1)].flags.contains(Flags::WIDE_CHAR_SPACER));
        assert_eq!(row[Column(2)].c, 'e');
        assert_eq!(row[Column(2)].zerowidth(), Some(['\u{301}'].as_slice()));
        assert_eq!(pane.resolve_color(row[Column(0)].fg), pane.color(1));
    }

    #[test]
    fn native_cell_colors_keep_the_frame_style_rules() {
        let mut pane = GridPane::new(20, 4);
        pane.feed(b"\x1b[7mx\x1b[0;2my\x1b[0;8mz");
        let row = &pane.model().grid()[Line(0)];
        assert_eq!(pane.cell_colors(&row[Column(0)]), (BACKGROUND, FOREGROUND));
        assert_eq!(pane.cell_colors(&row[Column(1)]), (dim(FOREGROUND, BACKGROUND), BACKGROUND));
        assert_eq!(pane.cell_colors(&row[Column(2)]), (BACKGROUND, BACKGROUND));
    }

    #[test]
    fn alternate_screen_resize_cursor_and_damage_use_the_same_model() {
        use alacritty_terminal::{grid::Dimensions, term::TermMode};

        let mut pane = GridPane::new(20, 4);
        pane.feed(b"main\x1b[?1049h\x1b[Halt");
        assert!(pane.model().mode().contains(TermMode::ALT_SCREEN));
        assert_eq!(pane.model().grid()[Line(0)][Column(0)].c, 'a');
        pane.resize(24, 6);
        assert_eq!((pane.model().columns(), pane.model().screen_lines()), (24, 6));
        assert!(matches!(pane.damage(), TermDamage::Full));
        let mut frame = Vec::new();
        assert!(pane.frame(false, &mut frame));
        assert_eq!(u16::from_le_bytes([frame[4], frame[5]]), 6);
        pane.feed(b"\x1b[?1049l");
        assert!(!pane.model().mode().contains(TermMode::ALT_SCREEN));
        assert_eq!(pane.model().grid()[Line(0)][Column(0)].c, 'm');
        pane.reset_damage();
        pane.feed(b"\x1b[3;4H\x1b[?25l");
        let lines: Vec<_> = match pane.damage() {
            TermDamage::Partial(lines) => lines.map(|line| line.line).collect(),
            TermDamage::Full => panic!("cursor movement should not damage the full screen"),
        };
        assert!(lines.contains(&0) && lines.contains(&2), "{lines:?}");
        assert_eq!(pane.model().renderable_content().cursor.shape, CursorShape::Hidden);
        frame.clear();
        pane.frame(false, &mut frame);
        assert_eq!(&frame[..2], &0xffffu16.to_le_bytes());
        pane.feed(b"\x1b[?25h");
        let cursor = pane.model().renderable_content().cursor;
        assert_eq!((cursor.point.line.0, cursor.point.column.0), (2, 3));
        assert_ne!(cursor.shape, CursorShape::Hidden);
    }

    struct Listener(std::sync::mpsc::Sender<Event>);

    impl EventListener for Listener {
        fn send_event(&self, event: Event) {
            let _ = self.0.send(event);
        }
    }

    #[test]
    fn live_listener_replies_with_current_size_and_shared_palette() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut pane = GridPane::with_listener(20, 4, Listener(tx));
        pane.feed(b"\x1b[6n\x1b[18t\x1b[14t\x1b]10;?\x07\x1b]11;?\x07\x1b]12;?\x07");
        let replies: Vec<_> = rx.try_iter().filter_map(|event| pane.protocol_reply(&event, 8, 16)).collect();
        assert!(replies.iter().any(|reply| reply == "\x1b[1;1R"));
        assert!(replies.iter().any(|reply| reply == "\x1b[8;4;20t"));
        assert!(replies.iter().any(|reply| reply == "\x1b[4;64;160t"));
        for (code, color) in [(10, FOREGROUND), (11, BACKGROUND), (12, CURSOR)] {
            let (r, g, b) = ((color >> 16) & 0xff, (color >> 8) & 0xff, color & 0xff);
            let expected = format!("\x1b]{code};rgb:{r:02x}{r:02x}/{g:02x}{g:02x}/{b:02x}{b:02x}\x07");
            assert!(replies.contains(&expected), "{replies:?}");
        }
        pane.feed(b"\x1b]4;1;rgb:12/34/56\x07\x1b]4;1;?\x07\x1b[31mx");
        let replies: Vec<_> = rx.try_iter().filter_map(|event| pane.protocol_reply(&event, 8, 16)).collect();
        assert!(replies.iter().any(|reply| reply == "\x1b]4;1;rgb:1212/3434/5656\x07"));
        let mut frame = Vec::new();
        pane.frame(true, &mut frame);
        assert_eq!(runs(&frame)[0].3, 0x123456);
        pane.feed(b"\x1b]104;1\x07");
        assert_eq!(packed(pane.color(1)), ANSI[1]);
        pane.resize(30, 8);
        pane.feed(b"\x1b[18t\x1b[14t");
        let replies: Vec<_> = rx.try_iter().filter_map(|event| pane.protocol_reply(&event, 8, 16)).collect();
        assert!(replies.iter().any(|reply| reply == "\x1b[8;8;30t"));
        assert!(replies.iter().any(|reply| reply == "\x1b[4;128;240t"));
    }
}
