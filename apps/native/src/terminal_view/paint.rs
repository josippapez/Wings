//! Drawing the terminal: glyph runs, the cell styles they use, and the view's render pass.

use std::sync::Arc;

use gpui::{
    Bounds, ContentMask, Context, DispatchPhase, ElementInputHandler, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, Role, ShapedLine, StrikethroughStyle, TextAlign, TextRun, UnderlineStyle,
    Window, canvas, div, fill, font, point, prelude::*, px, rgb, rgba, size,
};
use wings_terminal::{
    alacritty_terminal::{term::cell::Flags, vte::ansi::CursorShape},
    grid::{BACKGROUND, FOREGROUND},
};

use crate::actions::{self, Copy, Paste};

use super::{
    TerminalView, font_size, line_height,
    snapshot::{Found, PaintCell, Snapshot},
    worker::Command,
};

/// The desktop terminal's `selectionBackground`.
const SELECTION: u32 = 0xffffff33;
const SCROLLBAR: u32 = 0xffffff40;
/// The desktop find bar's `matchBackground` and `activeMatchBackground`.
const FIND_MATCH: u32 = 0x5b4b1f;
const FIND_ACTIVE: u32 = 0xe9a23b;

/// The text style a cell draws with. A focused block cursor draws its cell in the background colour.
fn cell_run(cell: &PaintCell, snapshot: &Snapshot, focused: bool, base: &TextRun) -> TextRun {
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

/// One shaped line per run of same-style text instead of one per cell. Like the desktop's frame
/// encoder, a run holds only characters that fill exactly one cell in any monospace font (printable
/// ASCII, box drawing and blocks), so fixed-width shaping keeps every glyph on its cell. Anything else
/// gets a line of its own. Blank cells inside a run become spaces.
fn glyph_runs(
    snapshot: &Snapshot,
    focused: bool,
    base: &TextRun,
) -> Vec<(usize, usize, String, TextRun)> {
    struct Open {
        row: usize,
        col: usize,
        /// Cells covered so far; box drawing takes three bytes for one cell.
        cells: usize,
        text: String,
        style: TextRun,
    }
    fn shares_cells(text: &str) -> bool {
        let mut chars = text.chars();
        matches!((chars.next(), chars.next()), (Some(c), None) if matches!(c, ' '..='~' | '\u{2500}'..='\u{259f}'))
    }
    fn finish(runs: &mut Vec<(usize, usize, String, TextRun)>, mut open: Open) {
        open.text.truncate(open.text.trim_end_matches(' ').len());
        if !open.text.is_empty() {
            open.style.len = open.text.len();
            runs.push((open.row, open.col, open.text, open.style));
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
            }
            continue;
        }
        let style = cell_run(cell, snapshot, focused, base);
        if shares_cells(&cell.text)
            && let Some(run) = &mut open
            && next(run)
            && run.style == style
        {
            run.text.push_str(&cell.text);
            run.cells += 1;
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
                style,
            });
        } else {
            let mut style = style;
            style.len = cell.text.len();
            runs.push((cell.row, cell.col, cell.text.clone(), style));
        }
    }
    if let Some(done) = open {
        finish(&mut runs, done);
    }
    runs
}

struct PaintData {
    snapshot: Arc<Snapshot>,
    glyphs: Vec<(Point<Pixels>, ShapedLine)>,
    cell_width: Pixels,
    line_height: Pixels,
    focused: bool,
    link: Vec<(usize, usize)>,
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.autofocus && self.visible {
            self.focus.focus(window, cx);
            self.autofocus = false;
        }
        let prepaint_view = cx.entity();
        let paint_view = cx.entity();
        let surface = canvas(
            move |bounds, window, cx| {
                let font_px = font_size(cx);
                let row_height = line_height(font_px);
                let base_run = TextRun {
                    font: font(super::look(cx).family),
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
                let cell_width = px(measured.as_f32().ceil());
                prepaint_view.update(cx, |view, cx| {
                    view.layout(bounds, cell_width, row_height, cx)
                });
                let view = prepaint_view.read(cx);
                let snapshot = view.snapshot.clone();
                let focused = view.focus.is_focused(window);
                let mut glyphs = Vec::new();
                for (row, col, text, run) in glyph_runs(&snapshot, focused, &base_run) {
                    let line = window.text_system().shape_line(
                        text.into(),
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
                    for cell in &data.snapshot.cells {
                        if cell.bg != data.snapshot.background {
                            let origin = point(
                                bounds.left() + data.cell_width * cell.col,
                                bounds.top() + data.line_height * cell.row,
                            );
                            window.paint_quad(fill(
                                Bounds::new(
                                    origin,
                                    size(data.cell_width * cell.width, data.line_height),
                                ),
                                rgb(cell.bg),
                            ));
                        }
                    }
                    for cell in &data.snapshot.cells {
                        let color = match cell.found {
                            Found::No => continue,
                            Found::Match => FIND_MATCH,
                            Found::Active => FIND_ACTIVE,
                        };
                        window.paint_quad(fill(
                            Bounds::new(
                                point(
                                    bounds.left() + data.cell_width * cell.col,
                                    bounds.top() + data.line_height * cell.row,
                                ),
                                size(data.cell_width * cell.width, data.line_height),
                            ),
                            rgb(color),
                        ));
                    }
                    if data.focused
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
                    for (origin, line) in data.glyphs {
                        if let Err(error) =
                            line.paint(origin, data.line_height, TextAlign::Left, None, window, cx)
                        {
                            paint_error = Some(format!("Terminal drawing failed: {error}"));
                            break;
                        }
                    }
                    for cell in data.snapshot.cells.iter().filter(|cell| cell.selected) {
                        window.paint_quad(fill(
                            Bounds::new(
                                point(
                                    bounds.left() + data.cell_width * cell.col,
                                    bounds.top() + data.line_height * cell.row,
                                ),
                                size(data.cell_width * cell.width, data.line_height),
                            ),
                            rgba(SELECTION),
                        ));
                    }
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
                    if data.focused
                        && let Some((row, col, width, shape)) = data.snapshot.cursor
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
                });
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
            .p(px(8.0))
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
    fn text_is_shaped_per_style_run_and_odd_widths_on_their_own() {
        let mut pane = GridPane::new(20, 3);
        pane.feed("ab cd\x1b[31mef\x1b[0m界x  ─┼\r\ne\u{301}g".as_bytes());
        let frame = snapshot(&pane, None, false, None);
        let base = TextRun {
            font: font("Menlo"),
            ..Default::default()
        };
        let runs: Vec<_> = glyph_runs(&frame, false, &base)
            .into_iter()
            .map(|(row, col, text, run)| (row, col, text, run.len))
            .collect();
        let run = |row, col, text: &str| (row, col, text.to_string(), text.len());
        assert_eq!(
            runs,
            vec![
                run(0, 0, "ab cd"),
                run(0, 5, "ef"),
                run(0, 7, "界"),
                run(0, 9, "x  ─┼"),
                run(1, 0, "e\u{301}"),
                run(1, 1, "g"),
            ]
        );
    }
}
