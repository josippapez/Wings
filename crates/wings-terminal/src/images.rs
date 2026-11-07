//! Images that programs print, as the desktop's xterm.js image addon (`@xterm/addon-image` 0.9) draws them: SIXEL and
//! iTerm2 inline images (OSC 1337 `File=`). libghostty-vt has neither, only the Kitty graphics protocol, so this
//! module takes both out of the output before Ghostty sees it, decodes them, and hands each one to Ghostty as a Kitty
//! image placed at the cursor. Ghostty then keeps it with the text: it scrolls, goes into the scrollback, is cleared
//! with the screen and dropped, oldest first, past the storage limit, the same for all three protocols.
//!
//! Every image is untrusted input from any program, so each limit is the desktop's (`IMAGE_OPTIONS` in
//! `apps/desktop/src/lib/terminal.ts`): a SIXEL sequence and an iTerm2 file of at most 8 MB, an image of at most
//! 2048 x 2048 pixels, and 24 MB of images per pane. Anything malformed or over a limit is dropped.

use std::{ffi::c_void, io::Cursor, sync::Once};

use base64::{
    engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig},
    Engine,
};
use ghostty_sys::*;
use image::{imageops::FilterType, ImageFormat, ImageReader, Limits};

/// The bytes of images a pane keeps: the desktop's `storageLimit: 24` (MB of RGBA pixels).
pub const STORAGE_LIMIT: u64 = 24_000_000;
/// The most pixels one image may have: the desktop's `pixelLimit: 2048 * 2048`.
pub const PIXEL_LIMIT: u64 = 2048 * 2048;
/// The most bytes of SIXEL data one sequence may carry: the desktop's `sixelSizeLimit`.
pub const SIXEL_SIZE_LIMIT: usize = 8_000_000;
/// The largest file an iTerm2 inline image may declare: the desktop's `iipSizeLimit`.
pub const IIP_SIZE_LIMIT: usize = 8_000_000;
/// The addon's header parser gives up on a key or value longer than this (`MAX_FIELDCHARS`).
const MAX_FIELD: usize = 1024;
/// The longest SIXEL introducer kept while deciding whether a DCS is one.
const MAX_HEAD: usize = 64;
/// The addon's fallback cell size, for a terminal that hasn't been told its own.
const DEFAULT_CELL: (u32, u32) = (7, 14);

/// A decoded image: `width * height` pixels of 8-bit RGBA, row by row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// What the scanner found in the output, in order.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Piece<'a> {
    /// Output for Ghostty as it is.
    Bytes(&'a [u8]),
    /// Output held back while the scanner looked at it, which turned out not to be an image.
    Owned(Vec<u8>),
    /// A whole SIXEL sequence: its parameters (`P1;P2;P3`) and its data.
    Sixel(Vec<u8>, Vec<u8>),
    /// A whole iTerm2 inline image: everything after `ESC ] 1337;File=`.
    Iterm(Vec<u8>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    Escape,
    /// After `ESC P`, collecting parameters up to the final byte.
    DcsHead,
    /// After `ESC ]`, matching `1337;File=`.
    OscHead,
    Sixel,
    Iterm,
}

const ITERM_FILE: &[u8] = b"1337;File=";
const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
/// CAN and SUB end a control string without completing it.
const CAN: u8 = 0x18;
const SUB: u8 = 0x1a;

/// Splits SIXEL and iTerm2 image sequences out of the output, across reads. Every other byte goes to Ghostty in
/// its order. An ESC always ends a control string and starts a new sequence, as in the VT parser both terminals use,
/// so a sequence is found wherever a program starts one.
pub(crate) struct Scanner {
    state: State,
    /// The start of a sequence not yet known to be an image.
    head: Vec<u8>,
    /// The image sequence so far, or nothing once it went over its limit.
    data: Vec<u8>,
    params: Vec<u8>,
    over: bool,
}

impl Scanner {
    pub(crate) fn new() -> Self {
        Self {
            state: State::Ground,
            head: Vec::new(),
            data: Vec::new(),
            params: Vec::new(),
            over: false,
        }
    }

    /// The largest image sequence kept: a SIXEL's data, or an iTerm2 header and its file in base64.
    fn limit(&self) -> usize {
        match self.state {
            State::Sixel => SIXEL_SIZE_LIMIT,
            _ => IIP_SIZE_LIMIT.div_ceil(3) * 4 + 8 * MAX_FIELD,
        }
    }

    /// Hands what's held back to Ghostty and looks at `byte` again from the ground state.
    fn give_up<'a>(&mut self, out: &mut Vec<Piece<'a>>) {
        out.push(Piece::Owned(std::mem::take(&mut self.head)));
        self.state = State::Ground;
    }

    fn keep(&mut self, chunk: &[u8]) {
        if self.over {
            return;
        }
        if self.data.len() + chunk.len() > self.limit() {
            self.over = true;
            self.data = Vec::new();
        } else {
            self.data.extend_from_slice(chunk);
        }
    }

    fn finish<'a>(&mut self, complete: bool, out: &mut Vec<Piece<'a>>) {
        let data = std::mem::take(&mut self.data);
        let params = std::mem::take(&mut self.params);
        if complete && !self.over {
            out.push(match self.state {
                State::Sixel => Piece::Sixel(params, data),
                _ => Piece::Iterm(data),
            });
        }
        self.over = false;
        self.state = State::Ground;
    }

    pub(crate) fn scan<'a>(&mut self, input: &'a [u8], out: &mut Vec<Piece<'a>>) {
        let mut i = 0;
        while i < input.len() {
            match self.state {
                State::Ground => {
                    let start = i;
                    match input[i..].iter().position(|&b| b == ESC) {
                        Some(at) => {
                            i += at;
                            if i > start {
                                out.push(Piece::Bytes(&input[start..i]));
                            }
                            self.head.clear();
                            self.head.push(ESC);
                            self.state = State::Escape;
                            i += 1;
                        }
                        None => {
                            out.push(Piece::Bytes(&input[start..]));
                            i = input.len();
                        }
                    }
                }
                State::Escape => match input[i] {
                    b'P' => {
                        self.head.push(b'P');
                        self.state = State::DcsHead;
                        i += 1;
                    }
                    b']' => {
                        self.head.push(b']');
                        self.state = State::OscHead;
                        i += 1;
                    }
                    _ => self.give_up(out),
                },
                State::DcsHead => {
                    let b = input[i];
                    match b {
                        b'0'..=b'9' | b';' if self.head.len() < MAX_HEAD => {
                            self.head.push(b);
                            i += 1;
                        }
                        // `q` with no intermediates is SIXEL. DECRQSS (`$q`) and XTGETTCAP (`+q`) have one.
                        b'q' => {
                            self.params = self.head[2..].to_vec();
                            self.head.clear();
                            self.data.clear();
                            self.over = false;
                            self.state = State::Sixel;
                            i += 1;
                        }
                        _ => self.give_up(out),
                    }
                }
                State::OscHead => {
                    let b = input[i];
                    let at = self.head.len() - 2;
                    if ITERM_FILE[at] == b {
                        self.head.push(b);
                        i += 1;
                        if at + 1 == ITERM_FILE.len() {
                            self.head.clear();
                            self.data.clear();
                            self.over = false;
                            self.state = State::Iterm;
                        }
                    } else {
                        self.give_up(out);
                    }
                }
                State::Sixel | State::Iterm => {
                    let sixel = self.state == State::Sixel;
                    let end = input[i..]
                        .iter()
                        .position(|&b| b == ESC || b == CAN || b == SUB || (!sixel && b == BEL));
                    let Some(at) = end else {
                        self.keep(&input[i..]);
                        i = input.len();
                        continue;
                    };
                    self.keep(&input[i..i + at]);
                    i += at;
                    match input[i] {
                        // The ESC of `ESC \` (or of whatever comes next) goes on to Ghostty, which ignores a lone ST.
                        ESC => self.finish(true, out),
                        BEL => {
                            self.finish(true, out);
                            i += 1;
                        }
                        _ => {
                            self.finish(false, out);
                            i += 1;
                        }
                    }
                }
            }
        }
    }
}

/// The sixel library's default palette, as the addon sets it up: ANSI 256 colors with the VT340's 16 first.
fn default_palette() -> [u32; 256] {
    let pct = |r: u32, g: u32, b: u32| rgba(pct255(r), pct255(g), pct255(b), 255);
    let vt340 = [
        pct(0, 0, 0),
        pct(20, 20, 80),
        pct(80, 13, 13),
        pct(20, 80, 20),
        pct(80, 20, 80),
        pct(20, 80, 80),
        pct(80, 80, 20),
        pct(53, 53, 53),
        pct(26, 26, 26),
        pct(33, 33, 60),
        pct(60, 26, 26),
        pct(33, 60, 33),
        pct(60, 33, 60),
        pct(33, 60, 60),
        pct(60, 60, 33),
        pct(80, 80, 80),
    ];
    let mut palette = [0; 256];
    palette[..16].copy_from_slice(&vt340);
    let levels = [0, 95, 135, 175, 215, 255];
    let mut i = 16;
    for r in levels {
        for g in levels {
            for b in levels {
                palette[i] = rgba(r, g, b, 255);
                i += 1;
            }
        }
    }
    for v in (8..=238).step_by(10) {
        palette[i] = rgba(v, v, v, 255);
        i += 1;
    }
    palette
}

/// A pixel as its bytes in memory: R, G, B, A.
fn rgba(r: u32, g: u32, b: u32, a: u32) -> u32 {
    u32::from_le_bytes([r as u8, g as u8, b as u8, a as u8])
}

fn pct255(v: u32) -> u32 {
    (v.min(100) as f64 / 100.0 * 255.0).round() as u32
}

/// DEC's HLS, where hue 0 is blue, as the sixel library converts it.
fn hls(h: u32, l: u32, s: u32) -> u32 {
    let (h, l, s) = (
        ((h % 360) as f64 + 240.0) / 360.0,
        l.min(100) as f64 / 100.0,
        s.min(100) as f64 / 100.0,
    );
    if s == 0.0 {
        let v = (l * 255.0).round() as u32;
        return rgba(v, v, v, 255);
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let hue = |mut t: f64| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        let v = if t * 6.0 < 1.0 {
            p + (q - p) * 6.0 * t
        } else if t * 2.0 < 1.0 {
            q
        } else if t * 3.0 < 2.0 {
            p + (q - p) * (4.0 - t * 6.0)
        } else {
            p
        };
        (v * 255.0).round().clamp(0.0, 255.0) as u32
    };
    rgba(hue(h + 1.0 / 3.0), hue(h), hue(h - 1.0 / 3.0), 255)
}

/// Reads `;`-separated numbers from `data` at `i`, as many as there are, moving `i` past them.
fn numbers(data: &[u8], i: &mut usize, out: &mut [u64]) -> usize {
    let mut count = 0;
    let mut current: Option<u64> = None;
    while *i < data.len() {
        match data[*i] {
            b @ b'0'..=b'9' => {
                let digit = (b - b'0') as u64;
                current = Some(
                    current
                        .unwrap_or(0)
                        .saturating_mul(10)
                        .saturating_add(digit),
                );
            }
            b';' => {
                if count < out.len() {
                    out[count] = current.unwrap_or(0);
                }
                count += 1;
                current = None;
            }
            _ => break,
        }
        *i += 1;
    }
    if let Some(value) = current {
        if count < out.len() {
            out[count] = value;
        }
        count += 1;
    }
    count.min(out.len())
}

/// Decodes a SIXEL image. `params` are the DCS parameters: a second one of 1 leaves unset pixels transparent, else they
/// take `background` (RGBA). `None` for an image with no pixels or more than the pixel limit.
pub fn decode_sixel(params: &[u8], data: &[u8], background: u32) -> Option<Rgba> {
    let mut p = [0u64; 3];
    let mut at = 0;
    numbers(params, &mut at, &mut p);
    let transparent = p[1] == 1;

    // First pass: the canvas size, so it can be checked before anything is allocated.
    let (mut x, mut band, mut max_x, mut bands) = (0u64, 0u64, 0u64, 0u64);
    let mut raster = (0u64, 0u64);
    let mut i = 0;
    while i < data.len() {
        match data[i] {
            b'"' => {
                i += 1;
                let mut values = [0u64; 4];
                numbers(data, &mut i, &mut values);
                if values[2] > 0 && values[3] > 0 {
                    raster = (values[2], values[3]);
                }
            }
            b'#' => {
                i += 1;
                numbers(data, &mut i, &mut [0; 5]);
            }
            b'!' => {
                i += 1;
                let mut n = [0u64; 1];
                numbers(data, &mut i, &mut n);
                if let Some(b'?'..=b'~') = data.get(i) {
                    x = x.saturating_add(n[0].max(1));
                    max_x = max_x.max(x);
                    bands = bands.max(band + 1);
                    i += 1;
                }
            }
            b'$' => {
                x = 0;
                i += 1;
            }
            b'-' => {
                x = 0;
                band = band.saturating_add(1);
                i += 1;
            }
            b'?'..=b'~' => {
                x += 1;
                max_x = max_x.max(x);
                bands = bands.max(band + 1);
                i += 1;
            }
            _ => i += 1,
        }
    }
    let (width, height) = if raster.0 > 0 {
        raster
    } else {
        (max_x, bands.saturating_mul(6))
    };
    if width == 0 || height == 0 || width.saturating_mul(height) > PIXEL_LIMIT {
        return None;
    }
    let (w, h) = (width as usize, height as usize);
    let fill = if transparent { 0 } else { background };
    let mut canvas = vec![fill; w * h];

    // Second pass: draw.
    let mut palette = default_palette();
    let mut color = palette[0];
    let (mut x, mut y) = (0usize, 0usize);
    let draw = |canvas: &mut [u32], x: usize, y: usize, bits: u8, n: usize, color: u32| {
        if x >= w {
            return;
        }
        let end = x.saturating_add(n).min(w);
        for bit in 0..6 {
            let row = y + bit;
            if bits & (1 << bit) != 0 && row < h {
                canvas[row * w + x..row * w + end].fill(color);
            }
        }
    };
    let mut i = 0;
    while i < data.len() {
        match data[i] {
            b'"' => {
                i += 1;
                numbers(data, &mut i, &mut [0; 4]);
            }
            b'#' => {
                i += 1;
                let mut v = [0u64; 5];
                let count = numbers(data, &mut i, &mut v);
                let register = (v[0] % 256) as usize;
                if count >= 5 {
                    let clamp = |v: u64| v.min(360) as u32;
                    palette[register] = match v[1] {
                        1 => hls(clamp(v[2]), clamp(v[3]), clamp(v[4])),
                        2 => rgba(
                            pct255(clamp(v[2])),
                            pct255(clamp(v[3])),
                            pct255(clamp(v[4])),
                            255,
                        ),
                        _ => palette[register],
                    };
                }
                color = palette[register];
            }
            b'!' => {
                i += 1;
                let mut n = [0u64; 1];
                numbers(data, &mut i, &mut n);
                if let Some(&c @ b'?'..=b'~') = data.get(i) {
                    let n = n[0].max(1).min(w as u64) as usize;
                    draw(&mut canvas, x, y, c - b'?', n, color);
                    x = x.saturating_add(n);
                    i += 1;
                }
            }
            b'$' => {
                x = 0;
                i += 1;
            }
            b'-' => {
                x = 0;
                y = y.saturating_add(6);
                i += 1;
            }
            c @ b'?'..=b'~' => {
                draw(&mut canvas, x, y, c - b'?', 1, color);
                x = x.saturating_add(1);
                i += 1;
            }
            _ => i += 1,
        }
    }
    let pixels = canvas.into_iter().flat_map(u32::to_le_bytes).collect();
    Some(Rgba {
        width: width as u32,
        height: height as u32,
        pixels,
    })
}

/// An iTerm2 inline image's header, as the addon's `IIPHeaderParser` reads it.
#[derive(Debug, Default, PartialEq, Eq)]
struct Header {
    size: usize,
    width: Option<String>,
    height: Option<String>,
    preserve_aspect_ratio: bool,
    inline: bool,
}

/// `N`, `Npx`, `N%` or `auto`.
fn valid_size(value: &[u8]) -> bool {
    if value == b"auto" {
        return true;
    }
    let digits = value.iter().take_while(|b| b.is_ascii_digit()).count();
    digits > 0 && matches!(&value[digits..], b"" | b"px" | b"%")
}

fn integer(value: &[u8]) -> Option<usize> {
    if value.iter().any(|b| !b.is_ascii_digit()) {
        return None;
    }
    Some(value.iter().fold(0usize, |n, b| {
        n.saturating_mul(10).saturating_add((b - b'0') as usize)
    }))
}

/// Splits the header (`key=value;...`) from the base64 data after the first `:`. `None` when malformed.
fn header(body: &[u8]) -> Option<(Header, &[u8])> {
    let colon = body.iter().position(|&b| b == b':')?;
    let mut header = Header {
        preserve_aspect_ratio: true,
        ..Header::default()
    };
    for field in body[..colon].split(|&b| b == b';') {
        let eq = field.iter().position(|&b| b == b'=')?;
        let (key, value) = (&field[..eq], &field[eq + 1..]);
        if key.is_empty() || key.len() > MAX_FIELD || value.len() > MAX_FIELD {
            return None;
        }
        match key {
            b"size" => header.size = integer(value)?,
            b"inline" => header.inline = integer(value)? != 0,
            b"preserveAspectRatio" => header.preserve_aspect_ratio = integer(value)? != 0,
            b"width" | b"height" => {
                if !valid_size(value) {
                    return None;
                }
                let value = Some(String::from_utf8_lossy(value).into_owned());
                if key == b"width" {
                    header.width = value;
                } else {
                    header.height = value;
                }
            }
            _ => {}
        }
    }
    Some((header, &body[colon + 1..]))
}

/// One dimension the program asked for, in pixels; 0 for `auto`. `total` is the screen's and `cell` a cell's.
fn dimension(value: Option<&str>, total: f64, cell: f64) -> f64 {
    let Some(value) = value.filter(|value| *value != "auto") else {
        return 0.0;
    };
    let number = |s: &str| s.parse::<f64>().unwrap_or(0.0);
    if let Some(percent) = value.strip_suffix('%') {
        number(percent) * total / 100.0
    } else if let Some(pixels) = value.strip_suffix("px") {
        number(pixels)
    } else {
        number(value) * cell
    }
}

/// The size to draw an image of `w x h` at, as the addon's `IIPHandler._resize`: what the program asked for, else the
/// image's own size made smaller to fit the screen less a line.
fn fitted(header: &Header, w: f64, h: f64, cell: (f64, f64), screen: (f64, f64)) -> (f64, f64) {
    let rw = dimension(header.width.as_deref(), screen.0, cell.0);
    let rh = dimension(header.height.as_deref(), screen.1, cell.1);
    if rw == 0.0 && rh == 0.0 {
        let f = (screen.0 / w).min((screen.1 - cell.1) / h);
        return if f < 1.0 { (w * f, h * f) } else { (w, h) };
    }
    if rw == 0.0 {
        (w * rh / h, rh)
    } else if header.preserve_aspect_ratio || rh == 0.0 {
        (rw, h * rw / w)
    } else {
        (rw, rh)
    }
}

const BASE64: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_padding_mode(DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true),
);

/// The addon draws PNG, JPEG and GIF files only.
fn file_format(file: &[u8]) -> Option<ImageFormat> {
    if file.len() < 24 {
        return None;
    }
    if file.starts_with(b"\x89PNG\r\n\x1a\n") && &file[12..16] == b"IHDR" {
        Some(ImageFormat::Png)
    } else if file.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(ImageFormat::Jpeg)
    } else if file.starts_with(b"GIF87a") || file.starts_with(b"GIF89a") {
        Some(ImageFormat::Gif)
    } else {
        None
    }
}

/// Decodes `data` of `format` at most `width x height` pixels, never using more memory than such an image takes.
fn decode_file(
    data: &[u8],
    format: ImageFormat,
    width: u32,
    height: u32,
) -> Option<image::RgbaImage> {
    let mut limits = Limits::default();
    limits.max_image_width = Some(width);
    limits.max_image_height = Some(height);
    limits.max_alloc = Some((width as u64 * height as u64 * 4).saturating_mul(2) + (1 << 20));
    let mut reader = ImageReader::with_format(Cursor::new(data), format);
    reader.limits(limits);
    Some(reader.decode().ok()?.into_rgba8())
}

/// Decodes an iTerm2 inline image from what follows `File=`, at the size the addon draws it. `cell` is a cell's size
/// and `screen` the screen's, in pixels. `None` when malformed, not shown inline, unsupported or over a limit.
pub fn decode_iterm(body: &[u8], cell: (u32, u32), screen: (u32, u32)) -> Option<Rgba> {
    let (header, encoded) = header(body)?;
    if !header.inline || header.size == 0 || header.size > IIP_SIZE_LIMIT {
        return None;
    }
    if encoded.len() > header.size.div_ceil(3) * 4 {
        return None;
    }
    let file = BASE64.decode(encoded).ok()?;
    let format = file_format(&file)?;
    let (w, h) = ImageReader::with_format(Cursor::new(&file), format)
        .into_dimensions()
        .ok()?;
    if w == 0 || h == 0 || w as u64 * h as u64 >= PIXEL_LIMIT {
        return None;
    }
    let cell = if cell.0 == 0 || cell.1 == 0 {
        DEFAULT_CELL
    } else {
        cell
    };
    let (cell, screen) = (
        (cell.0 as f64, cell.1 as f64),
        (screen.0 as f64, screen.1 as f64),
    );
    let (fw, fh) = fitted(&header, w as f64, h as f64, cell, screen);
    let (fw, fh) = (fw.floor(), fh.floor());
    if !(fw >= 1.0 && fh >= 1.0) || fw * fh >= PIXEL_LIMIT as f64 {
        return None;
    }
    let (fw, fh) = (fw as u32, fh as u32);
    let decoded = decode_file(&file, format, w, h)?;
    let decoded = if (fw, fh) == (w, h) {
        decoded
    } else {
        image::imageops::resize(&decoded, fw, fh, FilterType::Triangle)
    };
    Some(Rgba {
        width: fw,
        height: fh,
        pixels: decoded.into_raw(),
    })
}

/// The Kitty graphics command that shows `image` at the cursor, leaving the cursor where it is.
pub(crate) fn kitty_place(image: &Rgba) -> Vec<u8> {
    let head = format!(
        "\x1b_Ga=T,f=32,s={},v={},C=1,q=2;",
        image.width, image.height
    );
    let mut out = Vec::with_capacity(head.len() + image.pixels.len().div_ceil(3) * 4 + 2);
    out.extend_from_slice(head.as_bytes());
    let start = out.len();
    out.resize(start + image.pixels.len().div_ceil(3) * 4, 0);
    let written = BASE64
        .encode_slice(&image.pixels, &mut out[start..])
        .unwrap_or(0);
    out.truncate(start + written);
    out.extend_from_slice(b"\x1b\\");
    out
}

/// The rows of `cell_height` an image `height` pixels tall covers.
pub(crate) fn rows_for(height: u32, cell_height: u32) -> u32 {
    let cell = if cell_height == 0 {
        DEFAULT_CELL.1
    } else {
        cell_height
    };
    height.div_ceil(cell)
}

/// Decodes the PNG files Kitty graphics programs send, which Ghostty asks its host for. At most what a pane may keep.
unsafe extern "C" fn decode_png(
    _: *mut c_void,
    allocator: *const GhosttyAllocator,
    data: *const u8,
    len: usize,
    out: *mut GhosttySysImage,
) -> bool {
    if data.is_null() || out.is_null() {
        return false;
    }
    let bytes = unsafe { std::slice::from_raw_parts(data, len) };
    let decoded = std::panic::catch_unwind(|| {
        let mut limits = Limits::default();
        limits.max_image_width = Some(10_000);
        limits.max_image_height = Some(10_000);
        limits.max_alloc = Some(STORAGE_LIMIT * 2);
        let mut reader = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png);
        reader.limits(limits);
        let image = reader.decode().ok()?.into_rgba8();
        (image.as_raw().len() as u64 <= STORAGE_LIMIT).then_some(image)
    });
    let Ok(Some(image)) = decoded else {
        return false;
    };
    let pixels = image.as_raw();
    let buffer = unsafe { ghostty_alloc(allocator, pixels.len()) };
    if buffer.is_null() {
        return false;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), buffer, pixels.len());
        *out = GhosttySysImage {
            width: image.width(),
            height: image.height(),
            data: buffer,
            data_len: pixels.len(),
        };
    }
    true
}

/// Gives Ghostty the PNG decoder, once for the whole process.
pub(crate) fn install_png_decoder() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let decode: GhosttySysDecodePngFn = Some(decode_png);
        unsafe {
            ghostty_sys_set(
                GHOSTTY_SYS_OPT_DECODE_PNG,
                std::mem::transmute::<GhosttySysDecodePngFn, *const c_void>(decode),
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_all(chunks: &[&[u8]]) -> (Vec<u8>, Vec<String>) {
        let mut scanner = Scanner::new();
        let (mut passed, mut images) = (Vec::new(), Vec::new());
        for chunk in chunks {
            let mut out = Vec::new();
            scanner.scan(chunk, &mut out);
            for piece in out {
                match piece {
                    Piece::Bytes(bytes) => passed.extend_from_slice(bytes),
                    Piece::Owned(bytes) => passed.extend_from_slice(&bytes),
                    Piece::Sixel(params, data) => images.push(format!(
                        "sixel {} {}",
                        String::from_utf8_lossy(&params),
                        String::from_utf8_lossy(&data)
                    )),
                    Piece::Iterm(body) => {
                        images.push(format!("iterm {}", String::from_utf8_lossy(&body)))
                    }
                }
            }
        }
        (passed, images)
    }

    #[test]
    fn the_scanner_takes_out_images_split_across_reads_and_passes_everything_else() {
        let input = b"a\x1bP0;1;0q#1~~\x1b\\b\x1b]1337;File=inline=1:QUJD\x07c\x1b]0;title\x07\x1bP$qm\x1b\\\x1b[31md";
        let (passed, images) = scan_all(&[input]);
        assert_eq!(passed, b"a\x1b\\bc\x1b]0;title\x07\x1bP$qm\x1b\\\x1b[31md");
        assert_eq!(images, ["sixel 0;1;0 #1~~", "iterm inline=1:QUJD"]);
        // The same output a byte at a time.
        let chunks: Vec<&[u8]> = input.chunks(1).collect();
        assert_eq!(scan_all(&chunks), (passed, images));
    }

    #[test]
    fn a_sequence_cut_off_or_over_its_limit_is_dropped() {
        let (passed, images) = scan_all(&[b"\x1bPq~~\x18after"]);
        assert_eq!((passed.as_slice(), images.len()), (&b"after"[..], 0));
        let big = vec![b'~'; SIXEL_SIZE_LIMIT + 1];
        let (passed, images) = scan_all(&[b"\x1bPq", &big, b"\x1b\\ok"]);
        assert_eq!((passed.as_slice(), images.len()), (&b"\x1b\\ok"[..], 0));
        let (_, images) = scan_all(&[b"\x1bPq", &big[..SIXEL_SIZE_LIMIT], b"\x1b\\"]);
        assert_eq!(images.len(), 1);
    }

    #[test]
    fn sixel_pixels_take_the_selected_colors_and_size() {
        // Red (#1 as RGB 100;0;0), 2 columns of all six rows, then a new band with one pixel of the default blue.
        let image = decode_sixel(b"0;1", b"#1;2;100;0;0#1~~-#4@", 0).unwrap();
        assert_eq!((image.width, image.height), (2, 12));
        let px = |x: usize, y: usize| &image.pixels[(y * 2 + x) * 4..][..4];
        assert_eq!(px(0, 0), [255, 0, 0, 255]);
        assert_eq!(px(1, 5), [255, 0, 0, 255]);
        assert_eq!(px(0, 6), [204, 51, 204, 255]);
        assert_eq!(px(1, 6), [0, 0, 0, 0]);
        // Raster attributes set the size, and pixels past it are cut off.
        let image = decode_sixel(b"", b"\"1;1;3;2!5~", u32::MAX).unwrap();
        assert_eq!((image.width, image.height, image.pixels.len()), (3, 2, 24));
        // HLS hue 120 is red in DEC's wheel.
        assert_eq!(hls(120, 50, 100), rgba(255, 0, 0, 255));
    }

    #[test]
    fn a_sixel_over_the_pixel_limit_or_without_pixels_is_dropped() {
        assert_eq!(decode_sixel(b"", b"\"1;1;2049;2048~", 0), None);
        assert_eq!(decode_sixel(b"", b"!99999999999999999999~", 0), None);
        assert_eq!(decode_sixel(b"", b"#1;2;0;0;0", 0), None);
        assert!(decode_sixel(b"", b"\"1;1;2048;2048~", 0).is_some());
    }

    #[test]
    fn iterm_sizes_follow_the_header_like_the_addon() {
        let header = |width: Option<&str>, height: Option<&str>, keep: bool| Header {
            size: 1,
            width: width.map(str::to_owned),
            height: height.map(str::to_owned),
            preserve_aspect_ratio: keep,
            inline: true,
        };
        let (cell, screen) = ((10.0, 20.0), (800.0, 480.0));
        assert_eq!(
            fitted(&header(None, None, true), 100.0, 50.0, cell, screen),
            (100.0, 50.0)
        );
        // Too big for the screen less a line: made smaller to fit.
        assert_eq!(
            fitted(&header(None, None, true), 1600.0, 460.0, cell, screen),
            (800.0, 230.0)
        );
        assert_eq!(
            fitted(&header(Some("4"), None, true), 100.0, 50.0, cell, screen),
            (40.0, 20.0)
        );
        assert_eq!(
            fitted(&header(Some("50%"), None, true), 100.0, 50.0, cell, screen),
            (400.0, 200.0)
        );
        assert_eq!(
            fitted(&header(None, Some("30px"), true), 100.0, 50.0, cell, screen),
            (60.0, 30.0)
        );
        assert_eq!(
            fitted(
                &header(Some("4"), Some("1"), false),
                100.0,
                50.0,
                cell,
                screen
            ),
            (40.0, 20.0)
        );
        assert_eq!(
            fitted(
                &header(Some("4"), Some("2"), true),
                100.0,
                50.0,
                cell,
                screen
            ),
            (40.0, 20.0)
        );
    }

    /// A `w x h` PNG of one color.
    pub(crate) fn png(w: u32, h: u32, color: [u8; 4]) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(w, h, image::Rgba(color));
        let mut out = Cursor::new(Vec::new());
        image.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn iterm_images_decode_only_when_inline_whole_and_within_limits() {
        let file = png(4, 2, [0, 255, 0, 255]);
        let encoded = BASE64.encode(&file);
        let body = |header: &str| format!("{header}:{encoded}").into_bytes();
        let size = file.len();
        let image = decode_iterm(
            &body(&format!("inline=1;size={size}")),
            (10, 20),
            (800, 480),
        )
        .unwrap();
        assert_eq!((image.width, image.height), (4, 2));
        assert_eq!(&image.pixels[..4], [0, 255, 0, 255]);
        // Not inline, no size, a size over the limit or shorter than the data, bad fields and bad data.
        for header in [
            format!("size={size}"),
            "inline=1".to_owned(),
            format!("inline=1;size={}", IIP_SIZE_LIMIT + 1),
            "inline=1;size=3".to_owned(),
            format!("inline=1;size={size};width=wide"),
            format!("inline=x;size={size}"),
        ] {
            assert_eq!(
                decode_iterm(&body(&header), (10, 20), (800, 480)),
                None,
                "{header}"
            );
        }
        assert_eq!(
            decode_iterm(b"inline=1;size=100:!!!!", (10, 20), (800, 480)),
            None
        );
        assert_eq!(
            decode_iterm(b"inline=1;size=8:QUJDREVGR0g=", (10, 20), (800, 480)),
            None
        );
        assert_eq!(decode_iterm(b"no colon", (10, 20), (800, 480)), None);
        // A file claiming to be a PNG but cut short.
        let cut = BASE64.encode(&file[..30]);
        assert_eq!(
            decode_iterm(
                format!("inline=1;size=30:{cut}").as_bytes(),
                (10, 20),
                (800, 480)
            ),
            None
        );
        // Larger than the pixel limit.
        let wide = png(4096, 1025, [0, 0, 0, 255]);
        let body = format!("inline=1;size={}:{}", wide.len(), BASE64.encode(&wide));
        assert_eq!(
            decode_iterm(body.as_bytes(), (10, 20), (100_000, 100_000)),
            None
        );
    }

    #[test]
    fn the_kitty_command_carries_the_pixels() {
        let image = Rgba {
            width: 1,
            height: 1,
            pixels: vec![1, 2, 3, 4],
        };
        assert_eq!(
            kitty_place(&image),
            b"\x1b_Ga=T,f=32,s=1,v=1,C=1,q=2;AQIDBA==\x1b\\"
        );
        assert_eq!(rows_for(40, 20), 2);
        assert_eq!(rows_for(41, 20), 3);
    }

    mod pane {
        use super::*;
        use crate::grid::{Event, GridPane, ImageLayer, Scroll};

        /// A 40 x 10 pane of 10 x 20 pixel cells.
        fn pane() -> GridPane {
            let mut pane = GridPane::new(40, 10);
            pane.set_cell_size(10, 20);
            pane
        }

        /// A red SIXEL of `w x h` pixels.
        fn sixel(w: u32, h: u32) -> Vec<u8> {
            let band = format!("!{w}~-").repeat(h.div_ceil(6) as usize);
            format!("\x1bP0;1;0q\"1;1;{w};{h}#1;2;100;0;0#1{band}\x1b\\").into_bytes()
        }

        fn iterm(file: &[u8], fields: &str) -> Vec<u8> {
            format!(
                "\x1b]1337;File={fields}size={};inline=1:{}\x07",
                file.len(),
                BASE64.encode(file)
            )
            .into_bytes()
        }

        fn shown(pane: &mut GridPane) -> Vec<(i32, i32, (u32, u32))> {
            pane.viewport();
            pane.images()
                .iter()
                .map(|image| (image.col, image.row, image.size))
                .collect()
        }

        fn cursor(pane: &mut GridPane) -> (usize, usize) {
            let cursor = pane.viewport().cursor.unwrap();
            (cursor.row, cursor.col)
        }

        #[test]
        fn a_sixel_is_placed_at_the_cursor_and_the_cursor_ends_on_its_last_row() {
            let mut pane = pane();
            pane.feed(b"\x1b[3;5H");
            pane.feed(&sixel(20, 40));
            assert_eq!(shown(&mut pane), [(4, 2, (20, 40))]);
            assert_eq!(cursor(&mut pane), (3, 4));
            let image = pane.images()[0];
            assert_eq!(image.layer, ImageLayer::AboveText);
            assert_eq!(image.source, [0, 0, 20, 40]);
            let pixels = pane.image_pixels(image.id).unwrap();
            assert_eq!((pixels.width, pixels.height), (20, 40));
            assert_eq!(&pixels.pixels[..4], [255, 0, 0, 255]);
            // Text after it goes on from there.
            pane.feed(b"x");
            assert_eq!(pane.viewport().cells[3 * 40 + 4].text, "x");
        }

        #[test]
        fn an_iterm_image_is_placed_at_its_size_and_one_bigger_than_the_screen_shrinks() {
            let mut pane = pane();
            pane.feed(b"ab");
            pane.feed(&iterm(&png(30, 20, [0, 0, 255, 255]), "name=eA==;"));
            assert_eq!(shown(&mut pane), [(2, 0, (30, 20))]);
            assert_eq!(cursor(&mut pane), (0, 2));
            // 400 x 200 on a 400 x 200 screen fits in 400 x 180, less a line.
            let mut pane = self::pane();
            pane.feed(&iterm(&png(800, 400, [0, 0, 255, 255]), ""));
            assert_eq!(shown(&mut pane), [(0, 0, (360, 180))]);
            assert_eq!(cursor(&mut pane), (8, 0));
            // Sized in cells.
            let mut pane = self::pane();
            pane.feed(&iterm(&png(30, 20, [0, 0, 255, 255]), "width=6;"));
            assert_eq!(shown(&mut pane), [(0, 0, (60, 40))]);
        }

        #[test]
        fn images_scroll_with_the_text_and_into_the_scrollback() {
            let mut pane = pane();
            pane.feed(b"\x1b[3;1H");
            pane.feed(&sixel(20, 40));
            pane.feed(&b"\r\n".repeat(9));
            // The image's top was on the third row and the cursor on its last; six lines took the cursor to the
            // bottom and three more scrolled.
            assert_eq!(shown(&mut pane), [(0, -1, (20, 40))]);
            pane.scroll(Scroll::Delta(3));
            assert_eq!(shown(&mut pane), [(0, 2, (20, 40))]);
            pane.scroll(Scroll::Bottom);
            pane.feed(&b"\r\n".repeat(10));
            assert_eq!(shown(&mut pane), []);
        }

        #[test]
        fn clearing_the_screen_or_resetting_drops_images() {
            for clear in [&b"\x1b[H\x1b[2J"[..], b"\x1bc", b"\x1b[H\x1b[2J\x1b[3J"] {
                let mut pane = pane();
                pane.feed(&sixel(20, 40));
                pane.feed(&iterm(&png(30, 20, [0, 0, 255, 255]), ""));
                assert_eq!(shown(&mut pane).len(), 2);
                pane.feed(clear);
                assert_eq!(shown(&mut pane), [], "{clear:?}");
            }
        }

        #[test]
        fn a_pane_keeps_at_most_24_mb_of_images_dropping_the_oldest() {
            let mut pane = pane();
            let image = 1024 * 1024 * 4;
            for _ in 0..6 {
                pane.feed(&sixel(1024, 1024));
            }
            let kept = pane.image_bytes();
            assert!(kept <= STORAGE_LIMIT, "{kept}");
            assert_eq!(kept, 5 * image);
            // The newest is the one on screen.
            assert_eq!(shown(&mut pane).len(), 1);
        }

        #[test]
        fn images_over_a_limit_and_malformed_ones_draw_nothing_and_leave_the_text() {
            let mut pane = pane();
            let mut wide = b"\x1bPq\"1;1;4096;4096".to_vec();
            wide.extend_from_slice(b"#1~\x1b\\");
            for input in [
                wide,
                b"\x1bPq\x1b\\".to_vec(),
                b"\x1bPq#1;2;100;0;0!99999999999999999999999~\x1b\\".to_vec(),
                b"\x1b]1337;File=inline=1;size=10:@@@@\x07".to_vec(),
                b"\x1b]1337;File=inline=1;size=4:AAAAAAAA\x07".to_vec(),
                b"\x1b]1337;File=size=4:AAAA\x07".to_vec(),
                b"\x1b]1337;File=inline=1;size=4:AAAA\x07".to_vec(),
                iterm(b"GIF89a not really a gif at all", ""),
                b"\x1bP0;1q#1;9;9;9;9#999999999999999!~~\x18".to_vec(),
            ] {
                pane.feed(&input);
                pane.feed(b"ok ");
            }
            assert_eq!(shown(&mut pane), []);
            let text: String = pane.viewport().cells[..27]
                .iter()
                .map(|cell| cell.text.as_str())
                .collect();
            assert_eq!(text, "ok ok ok ok ok ok ok ok ok ");
        }

        #[test]
        fn with_images_off_nothing_is_drawn_and_sixel_isnt_reported() {
            let mut pane = pane();
            pane.feed(b"\x1b[c");
            let reply = |pane: &mut GridPane| -> Vec<u8> {
                pane.take_events()
                    .into_iter()
                    .filter_map(|event| match event {
                        Event::PtyWrite(bytes) => Some(bytes),
                        _ => None,
                    })
                    .flatten()
                    .collect()
            };
            assert_eq!(reply(&mut pane), b"\x1b[?6;4c");
            pane.feed(&sixel(20, 40));
            pane.set_images(false);
            // Turning them off drops the ones there were.
            assert_eq!(shown(&mut pane), []);
            pane.feed(&sixel(20, 40));
            pane.feed(&iterm(&png(30, 20, [0, 0, 255, 255]), ""));
            pane.feed(
                format!(
                    "\x1b_Ga=T,f=100,q=2;{}\x1b\\",
                    BASE64.encode(png(8, 8, [9; 4]))
                )
                .as_bytes(),
            );
            assert_eq!(shown(&mut pane), []);
            pane.feed(b"\x1b[c");
            assert_eq!(reply(&mut pane), b"\x1b[?6c");
            pane.set_images(true);
            pane.feed(&sixel(20, 40));
            assert_eq!(shown(&mut pane).len(), 1);
        }

        #[test]
        fn kitty_images_in_png_are_drawn_too() {
            let mut pane = pane();
            pane.feed(
                format!(
                    "\x1b_Ga=T,f=100,q=2;{}\x1b\\",
                    BASE64.encode(png(8, 8, [9, 8, 7, 255]))
                )
                .as_bytes(),
            );
            assert_eq!(shown(&mut pane), [(0, 0, (8, 8))]);
            let image = pane.images()[0];
            assert_eq!(
                &pane.image_pixels(image.id).unwrap().pixels[..4],
                [9, 8, 7, 255]
            );
            // A PNG that isn't one is turned away.
            let mut pane = self::pane();
            pane.feed(
                format!(
                    "\x1b_Ga=T,f=100,q=2;{}\x1b\\",
                    BASE64.encode(b"\x89PNG\r\n\x1a\nnope")
                )
                .as_bytes(),
            );
            assert_eq!(shown(&mut pane), []);
        }
    }
}
