//! The `PromptRail`: ticks painted in one canvas, the fisheye on the current and the hovered one, and a card beside the
//! hovered tick. The page sends the words once; hovering, scrolling a long rail and the card are the view's own state, so
//! none of it reaches the page. Only a click does, with the mark's index.
use super::*;
use gpui::{
    Anchor, Bounds, ClickEvent, CursorStyle, MouseMoveEvent, ScrollEvent, anchored, canvas,
    deferred, fill, point, size,
};
use tree::{PromptRail, prompt_rail as geo};

/// How wide the card is, and the gap the pointer crosses from the rail to it.
const CARD_WIDTH: f32 = 280.0;
const CARD_GAP: f32 = 8.0;
/// The card's top above the tick's centre, which puts the first line level with the tick.
const CARD_RISE: f32 = 18.0;
/// How wide a rail beside a column is: the longest tick with its margin.
const BESIDE_WIDTH: f32 = 64.0;

/// What the view keeps of a rail between renders.
#[derive(Default)]
pub(super) struct RailState {
    pub(super) hover: Option<usize>,
    bounds: Bounds<Pixels>,
    /// How far a rail with more ticks than fit is scrolled.
    scroll: f32,
    /// The current mark and mark count the scroll was last fitted to.
    followed: Option<(usize, usize)>,
    over_rail: bool,
    over_card: bool,
    /// How wide the page beside a `beside` rail is, as last drawn: it shows only where the column leaves it room.
    room: f32,
}

#[derive(Clone, Copy)]
enum Part {
    Rail,
    Card,
}

impl PageView {
    pub(super) fn prompt_rail(&self, rail: &PromptRail, cx: &mut Context<Self>) -> AnyElement {
        let id = rail.id;
        let count = rail.marks.len();
        let current = rail.current;
        let (hover, bounds, scroll, room) = {
            let states = self.prompt_rails.borrow();
            let state = states.get(&id);
            (
                state.and_then(|state| state.hover).filter(|at| *at < count),
                state.map(|state| state.bounds).unwrap_or_default(),
                state.map_or(0.0, |state| state.scroll),
                state.map_or(0.0, |state| state.room),
            )
        };
        let entity = cx.entity();
        let surface = canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| {
                let height = f32::from(bounds.size.height);
                let pitch = geo::pitch(count, height);
                let scroll = entity.update(cx, |view, _| {
                    let mut states = view.prompt_rails.borrow_mut();
                    let state = states.entry(id).or_default();
                    state.bounds = bounds;
                    state.scroll = if state.followed == Some((current, count)) {
                        state.scroll.clamp(0.0, geo::max_scroll(count, pitch, height))
                    } else {
                        state.followed = Some((current, count));
                        geo::keep_in_view(state.scroll, current, count, pitch, height)
                    };
                    state.scroll
                });
                let first = (((scroll - geo::PAD_Y) / pitch).floor().max(0.0)) as usize;
                let last = ((((scroll + height - geo::PAD_Y) / pitch).ceil()).max(0.0) as usize).min(count);
                for at in first..last {
                    let color = if at == current {
                        rgb(theme::FOREGROUND)
                    } else if hover == Some(at) {
                        rgba(theme::FOREGROUND << 8 | 0xcc)
                    } else {
                        rgba(theme::MUTED_FOREGROUND << 8 | 0x80)
                    };
                    let centre = geo::row_top(at, pitch) + pitch / 2.0 - scroll;
                    let tick = Bounds::new(
                        point(
                            bounds.origin.x + px(geo::PAD_X),
                            bounds.origin.y + px(centre - geo::TICK_HEIGHT / 2.0),
                        ),
                        size(px(geo::length(at, current, hover)), px(geo::TICK_HEIGHT)),
                    );
                    window.paint_quad(fill(tick, color).corner_radii(px(geo::TICK_HEIGHT / 2.0)));
                }
            },
        )
        .absolute()
        .size_full();
        let handler = rail.on_select.clone();
        let titles = format!("{count} prompts");
        let rail_view = div()
            .id(ElementId::NamedInteger("page-prompt-rail".into(), id))
            .role(Role::List)
            .aria_label(titles)
            .relative()
            .flex_1()
            .w_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .when(hover.is_some(), |el| el.cursor(CursorStyle::PointingHand))
            .child(surface)
            .on_mouse_move(cx.listener(move |view, event: &MouseMoveEvent, _, cx| {
                view.prompt_rail_point(id, count, Some(event.position), cx);
            }))
            .on_hover(cx.listener(move |view, over: &bool, _, cx| {
                view.prompt_rail_over(id, Part::Rail, *over, cx);
            }))
            .on_scroll(cx.listener(move |view, event: &ScrollEvent, window, cx| {
                let moved = {
                    let mut states = view.prompt_rails.borrow_mut();
                    let state = states.entry(id).or_default();
                    let height = f32::from(state.bounds.size.height);
                    let max = geo::max_scroll(count, geo::pitch(count, height), height);
                    let to = (state.scroll - event.delta.pixel_delta(px(geo::PITCH_MAX)).y.as_f32())
                        .clamp(0.0, max);
                    let moved = to != state.scroll;
                    state.scroll = to;
                    moved
                };
                if moved {
                    view.prompt_rail_point(id, count, Some(window.mouse_position()), cx);
                    cx.notify();
                }
            }))
            .on_click({
                let handler = handler.clone();
                cx.listener(move |view, event: &ClickEvent, _, _| {
                    let Some(handler) = &handler else {
                        return;
                    };
                    let at = event.mouse_position().and_then(|position| {
                        let states = view.prompt_rails.borrow();
                        let state = states.get(&id)?;
                        let height = f32::from(state.bounds.size.height);
                        let y = f32::from(position.y - state.bounds.origin.y);
                        geo::hit(y, state.scroll, count, geo::pitch(count, height))
                    });
                    if let Some(index) = at {
                        view.act(handler, Some(&serde_json::json!({ "index": index })));
                    }
                })
            })
            .when_some(hover, |el, at| {
                let height = f32::from(bounds.size.height);
                let pitch = geo::pitch(count, height);
                let centre = geo::row_top(at, pitch) + pitch / 2.0 - scroll;
                let top_left = point(
                    bounds.origin.x + bounds.size.width,
                    bounds.origin.y + px(centre - CARD_RISE),
                );
                el.child(self.prompt_rail_card(rail, id, at, top_left, handler, cx))
            });
        let Some(column) = rail.beside else {
            return rail_view.into_any_element();
        };
        // Over the page, against the left of its centred column: only the ticks take the pointer, so the page under the
        // rest scrolls and clicks as before. It shows once the page is known to leave it room.
        let wide = column + 2.0 * BESIDE_WIDTH;
        let entity = cx.entity();
        let measure = canvas(
            move |bounds, _, cx| {
                let width = f32::from(bounds.size.width);
                entity.update(cx, |view, cx| {
                    let changed = {
                        let mut states = view.prompt_rails.borrow_mut();
                        let state = states.entry(id).or_default();
                        let changed = (state.room - width).abs() > 0.5;
                        state.room = width;
                        changed
                    };
                    if changed {
                        cx.notify();
                    }
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(measure)
            .when(room >= wide, |el| {
                el.child(div().h_full().w(px(wide)).flex().child(rail_view.w(px(BESIDE_WIDTH)).flex_none()))
            })
            .into_any_element()
    }

    /// The card of mark `at`, drawn over the page with its top-left at `top_left` (in the window): the raised card's
    /// look, a bold first line over the start of Claude's reply. Its left edge is a transparent strip, so the pointer
    /// can cross from the tick to the card without the card going away.
    fn prompt_rail_card(
        &self,
        rail: &PromptRail,
        id: u64,
        at: usize,
        top_left: gpui::Point<Pixels>,
        handler: Option<String>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mark = &rail.marks[at];
        let body = div()
            .w(px(CARD_WIDTH))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .px(px(12.0))
            .py(px(10.0))
            .rounded(px(14.0))
            .border_1()
            .border_color(rgba(theme::HAIRLINE_STRONG))
            .bg(rgba(theme::POPOVER))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0.0, 0.0, 0.0, 0.28),
                offset: point(px(0.0), px(6.0)),
                blur_radius: px(18.0),
                spread_radius: px(0.0),
                inset: false,
            }])
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .truncate()
                    .text_size(px(13.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(theme::FOREGROUND))
                    .child(mark.title.clone()),
            )
            .when(!mark.preview.is_empty(), |el| {
                el.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .line_clamp(3)
                        .text_ellipsis()
                        .text_size(px(12.0))
                        .text_color(rgb(theme::MUTED_FOREGROUND))
                        .child(mark.preview.clone()),
                )
            });
        let strip = div()
            .id(ElementId::NamedInteger("page-prompt-rail-card".into(), id))
            .occlude()
            .pl(px(CARD_GAP))
            .cursor(CursorStyle::PointingHand)
            .child(body)
            .on_hover(cx.listener(move |view, over: &bool, _, cx| {
                view.prompt_rail_over(id, Part::Card, *over, cx);
            }))
            .on_click(cx.listener(move |view, _, _, _| {
                let hover = view.prompt_rails.borrow().get(&id).and_then(|state| state.hover);
                if let (Some(handler), Some(index)) = (&handler, hover) {
                    view.act(handler, Some(&serde_json::json!({ "index": index })));
                }
            }));
        deferred(anchored().anchor(Anchor::TopLeft).position(top_left).child(strip))
            .with_priority(1)
            .into_any_element()
    }

    /// The pointer moved over the rail (or was left at `position` by a scroll): the tick under it is hovered.
    fn prompt_rail_point(
        &mut self,
        id: u64,
        count: usize,
        position: Option<gpui::Point<Pixels>>,
        cx: &mut Context<Self>,
    ) {
        let changed = {
            let mut states = self.prompt_rails.borrow_mut();
            let state = states.entry(id).or_default();
            state.over_rail = true;
            let height = f32::from(state.bounds.size.height);
            let found = position.and_then(|position| {
                let y = f32::from(position.y - state.bounds.origin.y);
                geo::hit(y, state.scroll, count, geo::pitch(count, height))
            });
            let changed = state.hover != found;
            state.hover = found;
            changed
        };
        if changed {
            cx.notify();
        }
    }

    /// The pointer entered or left the rail or its card. The card stays while the pointer is on either, and goes once a
    /// moment after it is on neither: crossing from one to the other leaves one before it enters the other.
    fn prompt_rail_over(&mut self, id: u64, part: Part, over: bool, cx: &mut Context<Self>) {
        {
            let mut states = self.prompt_rails.borrow_mut();
            let state = states.entry(id).or_default();
            match part {
                Part::Rail => state.over_rail = over,
                Part::Card => state.over_card = over,
            }
        }
        if over {
            return;
        }
        cx.spawn(async move |view, cx| {
            let _ = view.update(cx, |view, cx| {
                let cleared = view
                    .prompt_rails
                    .borrow_mut()
                    .get_mut(&id)
                    .is_some_and(|state| {
                        !state.over_rail && !state.over_card && state.hover.take().is_some()
                    });
                if cleared {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Where rail `id` was last drawn.
    #[cfg(all(test, feature = "test-support"))]
    pub(crate) fn rail_bounds(&self, id: u64) -> Option<Bounds<Pixels>> {
        let bounds = self.prompt_rails.borrow().get(&id)?.bounds;
        (bounds.size.height > px(0.0)).then_some(bounds)
    }
}
