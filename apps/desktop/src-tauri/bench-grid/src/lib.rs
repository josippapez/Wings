//! Benchmark-only prototype of a Rust-parsed terminal: `alacritty_terminal` keeps the grid, and each frame carries
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
    event::VoidListener,
    index::{Column, Line},
    term::{cell::Flags, test::TermSize, Config, TermDamage},
    vte::ansi::{Color, CursorShape, NamedColor, Processor},
    Term,
};

/// Wings' terminal theme (`src/lib/terminal.ts`), so both renderers show the same colors.
const ANSI: [u32; 16] = [
    0x4a4650, 0xf2786a, 0x8fd19e, 0xe9c46a, 0x7aa7f2, 0xd59cf0, 0x6fd0d0, 0xd8d4ce, 0x6d6872, 0xff9285, 0xa7e3b3,
    0xf3d68b, 0x9cc0ff, 0xe4b8f7, 0x93e2e2, 0xf4f1ec,
];
const FOREGROUND: u32 = 0xe9e6e1;
const BACKGROUND: u32 = 0x17151b;
const CURSOR: u32 = 0xf0ede8;

pub struct GridPane {
    term: Term<VoidListener>,
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

fn rgb(color: Color) -> u32 {
    match color {
        Color::Spec(c) => ((c.r as u32) << 16) | ((c.g as u32) << 8) | c.b as u32,
        Color::Indexed(i) => indexed(i),
        Color::Named(n) => {
            let n = n as usize;
            match n {
                0..=15 => ANSI[n],
                _ if n == NamedColor::Background as usize => BACKGROUND,
                _ if n == NamedColor::Cursor as usize => CURSOR,
                _ if (NamedColor::DimBlack as usize..=NamedColor::DimWhite as usize).contains(&n) => {
                    dim(ANSI[n - NamedColor::DimBlack as usize], BACKGROUND)
                }
                _ if n == NamedColor::DimForeground as usize => dim(FOREGROUND, BACKGROUND),
                _ => FOREGROUND,
            }
        }
    }
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
        let config = Config { scrolling_history: 10_000, ..Config::default() };
        GridPane { term: Term::new(config, &TermSize::new(cols, rows), VoidListener), parser: Processor::new(), rows, cols }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
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
                    let (mut fg, mut bg) = (rgb(cell.fg), rgb(cell.bg));
                    if cell.flags.contains(Flags::INVERSE) {
                        std::mem::swap(&mut fg, &mut bg);
                    }
                    if cell.flags.contains(Flags::DIM) {
                        fg = dim(fg, bg);
                    }
                    if cell.flags.contains(Flags::HIDDEN) {
                        fg = bg;
                    }
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
}
