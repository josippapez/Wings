//! Drawing the terminal: glyph runs, the cell styles they use, and the view's render pass.

use std::sync::Arc;

use gpui::{
    BorderStyle, Bounds, ContentMask, Context, Corners, DispatchPhase, ElementInputHandler, Global,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, RenderImage, Role, ShapedLine, StrikethroughStyle,
    TextAlign, TextRun, UnderlineStyle, Window, canvas, div, fill, font, outline, point,
    prelude::*, px, rgb, rgba, size,
};
use wings_terminal::grid::{BACKGROUND, CursorShape, FOREGROUND, Flags, ImageLayer};

use crate::actions::{self, Copy, Paste};

use super::{
    InactiveCursor, TerminalView, font_size, line_height,
    snapshot::{Found, PaintCell, Snapshot},
    worker::Command,
};

/// The desktop terminal's `selectionBackground`.
const SELECTION: u32 = 0xffffff33;
const SCROLLBAR: u32 = 0xffffff40;
/// The desktop find bar's `matchBackground` and `activeMatchBackground`.
const FIND_MATCH: u32 = 0x5b4b1f;
const FIND_ACTIVE: u32 = 0xe9a23b;

/// WCAG's relative luminance of a 0xRRGGBB colour, as xterm.js works it out.
fn luminance(color: u32) -> f32 {
    let channel = |shift: u32| {
        let c = ((color >> shift) & 0xff) as f32 / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    channel(16) * 0.2126 + channel(8) * 0.7152 + channel(0) * 0.0722
}

fn contrast(a: f32, b: f32) -> f32 {
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// `fg` moved 10% at a time towards black (`darker`) or white until it reaches `ratio` against `bg`, or can't move.
fn shift(bg: u32, fg: u32, ratio: f32, darker: bool) -> u32 {
    let bg = luminance(bg);
    let mut rgb = [(fg >> 16) & 0xff, (fg >> 8) & 0xff, fg & 0xff];
    let pack = |rgb: [u32; 3]| rgb[0] << 16 | rgb[1] << 8 | rgb[2];
    while contrast(luminance(pack(rgb)), bg) < ratio
        && rgb.iter().any(|&c| if darker { c > 0 } else { c < 0xff })
    {
        for c in &mut rgb {
            *c = if darker {
                *c - (*c as f32 * 0.1).ceil() as u32
            } else {
                (*c + ((0xff - *c) as f32 * 0.1).ceil() as u32).min(0xff)
            };
        }
    }
    pack(rgb)
}

/// The desktop's minimum contrast (xterm.js's `ensureContrastRatio`): text too close to its background is darkened
/// or lightened, away from the background first, and the other way when that can't reach `ratio`.
fn readable(bg: u32, fg: u32, ratio: f32) -> u32 {
    let (bg_l, fg_l) = (luminance(bg), luminance(fg));
    if contrast(bg_l, fg_l) >= ratio {
        return fg;
    }
    let first = shift(bg, fg, ratio, fg_l < bg_l);
    let first_ratio = contrast(bg_l, luminance(first));
    if first_ratio >= ratio {
        return first;
    }
    let second = shift(bg, fg, ratio, fg_l >= bg_l);
    if first_ratio > contrast(bg_l, luminance(second)) {
        first
    } else {
        second
    }
}

/// The text style a cell draws with. A focused block cursor draws its cell in the background colour.
fn cell_run(
    cell: &PaintCell,
    snapshot: &Snapshot,
    focused: bool,
    min_contrast: f32,
    base: &TextRun,
) -> TextRun {
    let mut run = base.clone();
    if cell.flags.contains(Flags::BOLD) {
        run.font = run.font.bold();
    }
    if cell.flags.contains(Flags::ITALIC) {
        run.font = run.font.italic();
    }
    let block_cursor = focused
        && snapshot.cursor.is_some_and(|(row, col, _, shape)| {
            row == cell.row && col == cell.col && shape == CursorShape::Block
        });
    run.color = rgb(if block_cursor {
        snapshot.background
    } else if min_contrast > 1.0 {
        readable(cell.bg, cell.fg, min_contrast)
    } else {
        cell.fg
    })
    .into();
    if cell.flags.intersects(Flags::ALL_UNDERLINES) {
        run.underline = Some(UnderlineStyle {
            thickness: px(1.0),
            color: Some(run.color),
            wavy: cell.flags.contains(Flags::UNDERCURL),
        });
    }
    if cell.flags.contains(Flags::STRIKEOUT) {
        run.strikethrough = Some(StrikethroughStyle {
            thickness: px(1.0),
            color: Some(run.color),
        });
    }
    run
}

/// One shaped line per stretch of a row instead of one per cell, with a text run for each style in it. Each line takes
/// a place of its own in the window's draw order, which costs more than its glyphs, so styles share a line. Like the
/// desktop's frame encoder, a line holds only characters that fill exactly one cell in any monospace font (printable
/// ASCII, box drawing and blocks), so fixed-width shaping keeps every glyph on its cell. Anything else gets a line of
/// its own. Blank cells inside a line become spaces.
fn glyph_runs(
    snapshot: &Snapshot,
    focused: bool,
    min_contrast: f32,
    base: &TextRun,
) -> Vec<(usize, usize, String, Vec<TextRun>)> {
    struct Open {
        row: usize,
        col: usize,
        /// Cells covered so far; box drawing takes three bytes for one cell.
        cells: usize,
        text: String,
        /// The styles in order; each one's `len` is the bytes of `text` it covers.
        styles: Vec<TextRun>,
    }
    fn shares_cells(text: &str) -> bool {
        let mut chars = text.chars();
        matches!((chars.next(), chars.next()), (Some(c), None) if matches!(c, ' '..='~' | '\u{2500}'..='\u{259f}'))
    }
    fn finish(runs: &mut Vec<(usize, usize, String, Vec<TextRun>)>, mut open: Open) {
        // Trailing blanks only pad the last style, so cutting them never empties a style but the last.
        let trailing = open.text.len() - open.text.trim_end_matches(' ').len();
        open.text.truncate(open.text.len() - trailing);
        if let Some(last) = open.styles.last_mut() {
            last.len -= trailing;
            if last.len == 0 {
                open.styles.pop();
            }
        }
        if !open.text.is_empty() {
            runs.push((open.row, open.col, open.text, open.styles));
        }
    }
    let mut runs = Vec::new();
    let mut open: Option<Open> = None;
    for cell in &snapshot.cells {
        let next = |open: &Open| open.row == cell.row && open.col + open.cells == cell.col;
        if cell.text.is_empty() {
            if let Some(run) = &mut open
                && next(run)
            {
                run.text.push(' ');
                run.cells += 1;
                if let Some(last) = run.styles.last_mut() {
                    last.len += 1;
                }
            }
            continue;
        }
        let mut style = cell_run(cell, snapshot, focused, min_contrast, base);
        style.len = cell.text.len();
        if shares_cells(&cell.text)
            && let Some(run) = &mut open
            && next(run)
        {
            run.text.push_str(&cell.text);
            run.cells += 1;
            match run.styles.last_mut() {
                Some(last)
                    if TextRun {
                        len: style.len,
                        ..last.clone()
                    } == style =>
                {
                    last.len += style.len
                }
                _ => run.styles.push(style),
            }
            continue;
        }
        if let Some(done) = open.take() {
            finish(&mut runs, done);
        }
        if shares_cells(&cell.text) {
            open = Some(Open {
                row: cell.row,
                col: cell.col,
                cells: 1,
                text: cell.text.clone(),
                styles: vec![style],
            });
        } else {
            runs.push((cell.row, cell.col, cell.text.clone(), vec![style]));
        }
    }
    if let Some(done) = open {
        finish(&mut runs, done);
    }
    runs
}

/// The stretches of side-by-side cells on a row that `color` gives the same colour, as (row, first column, cells) and
/// the colour: one quad each instead of one per cell.
fn spans(
    cells: &[PaintCell],
    color: impl Fn(&PaintCell) -> Option<u32>,
) -> Vec<((usize, usize, usize), u32)> {
    let mut spans: Vec<((usize, usize, usize), u32)> = Vec::new();
    for cell in cells {
        let Some(color) = color(cell) else {
            continue;
        };
        if let Some(((row, col, len), last)) = spans.last_mut()
            && *row == cell.row
            && *last == color
            && cell.col <= *col + *len
        {
            // A wide character's second column comes after it as a cell of its own.
            *len = (*len).max(cell.col + cell.width - *col);
            continue;
        }
        spans.push(((cell.row, cell.col, cell.width), color));
    }
    spans
}

/// Textures of closed panes' images, freed by the next frame a terminal draws.
#[derive(Default)]
pub(super) struct RetiredImages(pub(super) Vec<Arc<RenderImage>>);

impl Global for RetiredImages {}

/// Draws the images of `layer`: each from its top-left cell, at its size, the part of it the terminal says to.
fn paint_images(window: &mut Window, bounds: Bounds<Pixels>, data: &PaintData, layer: ImageLayer) {
    for image in data
        .snapshot
        .images
        .iter()
        .filter(|image| image.layer() == layer)
    {
        let placement = &image.placement;
        let [src_x, src_y, src_w, src_h] = placement.source.map(|v| v as f32);
        let (width, height) = (placement.size.0 as f32, placement.size.1 as f32);
        if src_w <= 0.0 || src_h <= 0.0 || width <= 0.0 || height <= 0.0 {
            continue;
        }
        let origin = point(
            bounds.left() + data.cell_width * placement.col as f32 + px(placement.offset.0 as f32),
            bounds.top() + data.line_height * placement.row as f32 + px(placement.offset.1 as f32),
        );
        let shown = Bounds::new(origin, size(px(width), px(height)));
        // The whole image, scaled so its source part lands on `shown`; GPUI draws only where the two meet.
        let (scale_x, scale_y) = (width / src_w, height / src_h);
        let whole = Bounds::new(
            point(
                origin.x - px(src_x * scale_x),
                origin.y - px(src_y * scale_y),
            ),
            size(
                px(image.width as f32 * scale_x),
                px(image.height as f32 * scale_y),
            ),
        );
        let _ = window.paint_image(
            shown.intersect(&bounds),
            whole,
            Corners::default(),
            image.image.clone(),
            0,
            false,
        );
    }
}

struct PaintData {
    snapshot: Arc<Snapshot>,
    glyphs: Vec<(Point<Pixels>, ShapedLine)>,
    cell_width: Pixels,
    line_height: Pixels,
    focused: bool,
    /// False in the off half of a blink, when the focused cursor isn't drawn.
    lit: bool,
    inactive_cursor: InactiveCursor,
    link: Vec<(usize, usize)>,
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.autofocus && self.visible {
            self.focus.focus(window, cx);
            self.autofocus = false;
        }
        self.update_blink(window, cx);
        let prepaint_view = cx.entity();
        let paint_view = cx.entity();
        let surface = canvas(
            move |bounds, window, cx| {
                let font_px = font_size(cx);
                let look = super::look(cx);
                let row_height = line_height(font_px, look.line_height);
                let base_run = TextRun {
                    font: font(look.family),
                    color: rgb(FOREGROUND).into(),
                    ..Default::default()
                };
                let measured = window
                    .text_system()
                    .shape_line(
                        "M".into(),
                        px(font_px),
                        &[TextRun {
                            len: 1,
                            ..base_run.clone()
                        }],
                        None,
                    )
                    .width();
                // Whole device pixels, rounded down, as the desktop's xterm.js WebGL renderer does: rounding SF Mono's
                // 8.04pt at 13px up to whole points made 9pt cells and spaced the text out.
                let scale = window.scale_factor();
                let cell_width = px((measured.as_f32() * scale).floor() / scale);
                prepaint_view.update(cx, |view, cx| {
                    view.layout(bounds, cell_width, row_height, cx)
                });
                let view = prepaint_view.read(cx);
                let snapshot = view.snapshot.clone();
                let focused = view.focus.is_focused(window);
                let lit = view.cursor_lit;
                let mut glyphs = Vec::new();
                for (row, col, text, runs) in
                    glyph_runs(&snapshot, focused && lit, look.min_contrast, &base_run)
                {
                    let line = window.text_system().shape_line(
                        text.into(),
                        px(font_px),
                        &runs,
                        Some(cell_width),
                    );
                    glyphs.push((
                        point(
                            bounds.left() + cell_width * col,
                            bounds.top() + px(row_height) * row,
                        ),
                        line,
                    ));
                }
                if !view.marked.is_empty()
                    && let Some((row, col, _, _)) = snapshot.cursor
                {
                    let run = TextRun {
                        len: view.marked.len(),
                        underline: Some(UnderlineStyle {
                            thickness: px(1.0),
                            color: None,
                            wavy: false,
                        }),
                        ..base_run
                    };
                    let line = window.text_system().shape_line(
                        view.marked.clone().into(),
                        px(font_px),
                        &[run],
                        Some(cell_width),
                    );
                    glyphs.push((
                        point(
                            bounds.left() + cell_width * col,
                            bounds.top() + px(row_height) * row,
                        ),
                        line,
                    ));
                }
                PaintData {
                    snapshot,
                    glyphs,
                    cell_width,
                    line_height: px(row_height),
                    focused,
                    lit,
                    inactive_cursor: look.inactive_cursor,
                    link: view
                        .link
                        .as_ref()
                        .map_or_else(Vec::new, |link| link.cells.clone()),
                }
            },
            move |bounds, data, window, cx| {
                let focus = paint_view.read(cx).focus.clone();
                window.handle_input(
                    &focus,
                    ElementInputHandler::new(bounds, paint_view.clone()),
                    cx,
                );
                let move_view = paint_view.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble {
                        move_view.update(cx, |view, cx| view.mouse_move(event, cx));
                    }
                });
                let up_view = paint_view.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble {
                        up_view.update(cx, |view, cx| view.mouse_up(event, cx));
                    }
                });
                let mut paint_error = None;
                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    window.paint_quad(fill(bounds, rgb(data.snapshot.background)));
                    paint_images(window, bounds, &data, ImageLayer::BelowBackground);
                    let span = |(row, col, cells): (usize, usize, usize)| {
                        Bounds::new(
                            point(
                                bounds.left() + data.cell_width * col,
                                bounds.top() + data.line_height * row,
                            ),
                            size(data.cell_width * cells, data.line_height),
                        )
                    };
                    // In one layer, so the quads share one place in the window's draw order instead of each taking one.
                    window.paint_layer(bounds, |window| {
                        let background = data.snapshot.background;
                        for (at, color) in spans(&data.snapshot.cells, |cell| {
                            (cell.bg != background).then_some(cell.bg)
                        }) {
                            window.paint_quad(fill(span(at), rgb(color)));
                        }
                        for (at, color) in spans(&data.snapshot.cells, |cell| match cell.found {
                            Found::No => None,
                            Found::Match => Some(FIND_MATCH),
                            Found::Active => Some(FIND_ACTIVE),
                        }) {
                            window.paint_quad(fill(span(at), rgb(color)));
                        }
                    });
                    paint_images(window, bounds, &data, ImageLayer::BelowText);
                    if data.focused
                        && data.lit
                        && let Some((row, col, width, CursorShape::Block)) = data.snapshot.cursor
                    {
                        window.paint_quad(fill(
                            Bounds::new(
                                point(
                                    bounds.left() + data.cell_width * col,
                                    bounds.top() + data.line_height * row,
                                ),
                                size(data.cell_width * width, data.line_height),
                            ),
                            rgb(data.snapshot.cursor_color),
                        ));
                    }
                    for (origin, line) in &data.glyphs {
                        if let Err(error) =
                            line.paint(*origin, data.line_height, TextAlign::Left, None, window, cx)
                        {
                            paint_error = Some(format!("Terminal drawing failed: {error}"));
                            break;
                        }
                    }
                    paint_images(window, bounds, &data, ImageLayer::AboveText);
                    window.paint_layer(bounds, |window| {
                        for (at, color) in spans(&data.snapshot.cells, |cell| {
                            cell.selected.then_some(SELECTION)
                        }) {
                            window.paint_quad(fill(span(at), rgba(color)));
                        }
                    });
                    let snapshot = &data.snapshot;
                    for &(row, col) in &data.link {
                        let Some(cell) = snapshot.cells.get(row * snapshot.cols + col) else {
                            continue;
                        };
                        window.paint_quad(fill(
                            Bounds::new(
                                point(
                                    bounds.left() + data.cell_width * col,
                                    bounds.top() + data.line_height * (row + 1) - px(2.0),
                                ),
                                size(data.cell_width * cell.width, px(1.0)),
                            ),
                            rgb(cell.fg),
                        ));
                    }
                    if snapshot.display_offset > 0 {
                        // Only while scrolled back, so it's clear the view isn't following new output.
                        let total = (snapshot.history + snapshot.rows) as f32;
                        let height = bounds.size.height.as_f32();
                        let thumb = (snapshot.rows as f32 / total * height).max(16.0);
                        let top = ((snapshot.history - snapshot.display_offset) as f32 / total
                            * height)
                            .min(height - thumb);
                        window.paint_quad(
                            fill(
                                Bounds::new(
                                    point(bounds.right() - px(4.0), bounds.top() + px(top)),
                                    size(px(4.0), px(thumb)),
                                ),
                                rgba(SCROLLBAR),
                            )
                            .corner_radii(px(2.0)),
                        );
                    }
                    if data.focused {
                        if let Some((row, col, width, shape)) = data.snapshot.cursor
                            && data.lit
                        {
                            let origin = point(
                                bounds.left() + data.cell_width * col,
                                bounds.top() + data.line_height * row,
                            );
                            let cursor = match shape {
                                CursorShape::Beam => {
                                    Some(Bounds::new(origin, size(px(2.0), data.line_height)))
                                }
                                CursorShape::Underline => Some(Bounds::new(
                                    point(origin.x, origin.y + data.line_height - px(2.0)),
                                    size(data.cell_width * width, px(2.0)),
                                )),
                                _ => None,
                            };
                            if let Some(cursor) = cursor {
                                window.paint_quad(fill(cursor, rgb(data.snapshot.cursor_color)));
                            }
                        }
                    } else if let Some((row, col, width, _)) = data.snapshot.cursor {
                        // Without focus the cursor takes the Settings' style, whatever the program set.
                        let origin = point(
                            bounds.left() + data.cell_width * col,
                            bounds.top() + data.line_height * row,
                        );
                        let cell = size(data.cell_width * width, data.line_height);
                        let color = rgb(data.snapshot.cursor_color);
                        match data.inactive_cursor {
                            InactiveCursor::Outline => window.paint_quad(outline(
                                Bounds::new(origin, cell),
                                color,
                                BorderStyle::Solid,
                            )),
                            InactiveCursor::Block => {
                                window.paint_quad(fill(Bounds::new(origin, cell), color))
                            }
                            InactiveCursor::Bar => window.paint_quad(fill(
                                Bounds::new(origin, size(px(2.0), data.line_height)),
                                color,
                            )),
                            InactiveCursor::Underline => window.paint_quad(fill(
                                Bounds::new(
                                    point(origin.x, origin.y + data.line_height - px(2.0)),
                                    size(cell.width, px(2.0)),
                                ),
                                color,
                            )),
                            InactiveCursor::Hidden => {}
                        }
                    }
                });
                // Textures of images no longer on screen leave the window's atlas.
                let shown: Vec<_> = data
                    .snapshot
                    .images
                    .iter()
                    .map(|image| image.image.clone())
                    .collect();
                let mut gone = paint_view.update(cx, |view, _| {
                    let mut gone = std::mem::replace(&mut view.painted_images, shown);
                    gone.retain(|old| !view.painted_images.iter().any(|new| new.id == old.id));
                    gone
                });
                if let Some(retired) = cx.try_global::<RetiredImages>()
                    && !retired.0.is_empty()
                {
                    gone.extend(std::mem::take(&mut cx.global_mut::<RetiredImages>().0));
                }
                for image in gone {
                    let _ = window.drop_image(image);
                }
                if let Some(error) = paint_error {
                    paint_view.update(cx, |view, cx| {
                        if view.error.as_ref() != Some(&error) {
                            view.error = Some(error);
                            cx.notify();
                        }
                    });
                }
            },
        )
        .size_full();
        let status = self
            .error
            .as_ref()
            .or(self.snapshot.status.as_ref())
            .cloned();
        div()
            .id("native-terminal")
            .role(Role::Terminal)
            .aria_label("Terminal")
            .size_full()
            .min_w_0()
            .min_h_0()
            .relative()
            // The desktop's `pl-3.5 pr-1 pb-0.5`: the text starts under the pane title, with no gap under the header.
            .pl(px(14.0))
            .pr(px(4.0))
            .pb(px(2.0))
            .bg(rgb(BACKGROUND))
            .overflow_hidden()
            .track_focus(&self.focus)
            .key_context("Terminal")
            .on_key_down(cx.listener(Self::key_down))
            .on_action(cx.listener(|view, _: &Copy, _, cx| view.send(Command::Copy, cx)))
            .on_action(cx.listener(|view, _: &Paste, _, cx| view.paste(cx)))
            .on_action(
                cx.listener(|view, _: &actions::Find, window, cx| view.open_find(window, cx)),
            )
            .on_any_mouse_down(cx.listener(Self::mouse_down))
            .on_scroll(cx.listener(Self::scroll))
            .map(|view| {
                if self.link.is_some() {
                    view.cursor_pointer()
                } else {
                    view.cursor_text()
                }
            })
            .when(self.visible, |view| view.child(surface))
            .when_some(status, |view, status| {
                view.child(
                    div()
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .w_full()
                        .p(px(8.0))
                        .bg(rgb(BACKGROUND))
                        .text_color(rgb(FOREGROUND))
                        .text_size(px(12.0))
                        .child(status),
                )
            })
            .when_some(self.find.as_ref().filter(|find| find.open), |view, find| {
                view.child(self.find_bar(find, cx))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal_view::snapshot::snapshot;
    use wings_terminal::grid::GridPane;

    #[test]
    fn minimum_contrast_lightens_dim_text_and_leaves_readable_text() {
        // The theme's bright black on its background is about 2:1.
        let dim = 0x4a4650;
        assert!(contrast(luminance(BACKGROUND), luminance(dim)) < 4.5);
        let lifted = readable(BACKGROUND, dim, 4.5);
        assert!(contrast(luminance(BACKGROUND), luminance(lifted)) >= 4.5);
        assert!(luminance(lifted) > luminance(dim));
        assert_eq!(readable(BACKGROUND, FOREGROUND, 7.0), FOREGROUND);
    }

    #[test]
    fn a_row_is_shaped_as_one_line_per_stretch_with_a_run_per_style_and_odd_widths_on_their_own() {
        let mut pane = GridPane::new(20, 3);
        pane.feed("ab cd\x1b[31mef  \x1b[0mg界x  ─┼\r\ne\u{301}g".as_bytes());
        let frame = snapshot(&mut pane, None, false, None);
        let base = TextRun {
            font: font("Menlo"),
            ..Default::default()
        };
        let lines: Vec<_> = glyph_runs(&frame, false, 1.0, &base)
            .into_iter()
            .map(|(row, col, text, runs)| {
                let lens: Vec<_> = runs.iter().map(|run| run.len).collect();
                (row, col, text, lens)
            })
            .collect();
        let line =
            |row, col, text: &str, lens: &[usize]| (row, col, text.to_string(), lens.to_vec());
        assert_eq!(
            lines,
            vec![
                // The red "ef" and the blanks after it are one run; the line ends where the wide character starts.
                line(0, 0, "ab cdef  g", &[5, 4, 1]),
                line(0, 10, "界", &[3]),
                line(0, 12, "x  ─┼", &[9]),
                line(1, 0, "e\u{301}", &[3]),
                line(1, 1, "g", &[1]),
            ]
        );
    }

    #[test]
    fn backgrounds_are_one_quad_per_same_colour_stretch_of_a_row() {
        let mut pane = GridPane::new(10, 2);
        pane.feed("\x1b[41mab\x1b[42m界c\x1b[0m d\r\n\x1b[41mx".as_bytes());
        let frame = snapshot(&mut pane, None, false, None);
        let background = frame.background;
        let quads = spans(&frame.cells, |cell| {
            (cell.bg != background).then_some(cell.bg)
        });
        let red = frame.cells[0].bg;
        let green = frame.cells[2].bg;
        // The wide character's second column doesn't start a quad of its own.
        assert_eq!(
            quads,
            vec![((0, 0, 2), red), ((0, 2, 3), green), ((1, 0, 1), red)]
        );
    }
}
