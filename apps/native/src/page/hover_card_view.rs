//! The `HoverCard`: what it holds, drawn where it sits, and a card over the page under it while the pointer is on
//! either, as a link's preview. The card is the view's own state, so hovering never reaches the page.
use super::*;
use crate::ui::behaviors::popup;
use crate::ui::{hover_cue, live};
use gpui::{Bounds, Point, canvas};
use std::time::Duration;

/// The room between what the card is for and the card, which the pointer crosses without the card going away.
const GAP: f32 = 6.0;

/// How long the pointer rests on what a card is for before the card opens. 0.6 s is past a pointer sweeping down a list
/// of rows (a row takes well under 0.3 s to cross) and short enough that resting on one does not feel like waiting;
/// the desktop's tooltips wait 0.3 s, which is what opened a card for every row passed over.
pub(crate) const OPEN_DELAY: Duration = Duration::from_millis(600);

/// How far under the pointer the cue's bar sits, below the pointer's own height, as far as the row allows.
const CUE_BELOW: f32 = 18.0;

/// Where the cue goes along one axis: `at`, kept inside what the card is for, `room` long. The lists a trigger sits in
/// clip what lies outside their rows, so a cue below a short row's bottom would be cut off.
fn cue_at(at: Pixels, room: Pixels, size: f32) -> Pixels {
    let edge = px(2.0);
    at.min(room - px(size) - edge).max(edge)
}

/// The pointer is waiting on a card to open: where it was when it came onto what the card is for (in the window), and
/// when on the live clock.
#[derive(Clone, Copy)]
struct Waiting {
    at: Point<Pixels>,
    started: Duration,
}

#[derive(Default)]
pub(super) struct HoverState {
    /// Where what it holds was last drawn, in the window.
    bounds: Bounds<Pixels>,
    over_trigger: bool,
    over_card: bool,
    shown: bool,
    /// The card is about to open: the cue shows by the pointer until it does, or the pointer leaves.
    waiting: Option<Waiting>,
    /// Counts the waits begun, so a timer that ran out after its wait was cancelled opens nothing.
    waits: u64,
}

impl PageView {
    pub(super) fn hover_card(&self, card: &tree::HoverCard, cx: &mut Context<Self>) -> AnyElement {
        let id = card.id;
        let (shown, bounds, waiting) = self
            .hover_cards
            .borrow()
            .get(&id)
            .map_or((false, Bounds::default(), None), |state| (state.shown, state.bounds, state.waiting));
        let has_card = !card.card.is_empty();
        let entity = cx.entity();
        let measure = canvas(
            move |bounds, _, cx| {
                entity.update(cx, |view, _| view.hover_cards.borrow_mut().entry(id).or_default().bounds = bounds);
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let trigger = div()
            .id(ElementId::NamedInteger("page-hover-card".into(), id))
            .relative()
            .min_w_0()
            .flex()
            .flex_col()
            .child(measure)
            .children(self.items(&card.children, cx))
            .on_hover(cx.listener(move |view, over: &bool, window, cx| {
                view.hover_card_over(id, false, !has_card, *over, window, cx)
            }))
            // Only while a piece puts the page on screen with the layer for it (`spin_in_layer`): in the page a bar that
            // fills would draw the page at every step. Without one the delay still applies and the cue is left out.
            .when_some(waiting.filter(|_| self.spin_in_layer.get()), |el, waiting| {
                let (fill, track) = (theme::palette().working << 8 | 0xff, theme::palette().muted_foreground << 8 | 0x59);
                el.child(
                    div()
                        .absolute()
                        .left(cue_at(waiting.at.x - bounds.origin.x - px(hover_cue::WIDTH / 2.0), bounds.size.width, hover_cue::WIDTH))
                        .top(cue_at(waiting.at.y - bounds.origin.y + px(CUE_BELOW), bounds.size.height, hover_cue::HEIGHT))
                        .w(px(hover_cue::WIDTH))
                        .h(px(hover_cue::HEIGHT))
                        .child(hover_cue::hover_cue_slot(format!("hover-cue-{id}"), waiting.started, OPEN_DELAY, fill, track)),
                )
            });
        if !shown || card.card.is_empty() {
            return trigger.into_any_element();
        }
        let body = div()
            .w(px(card.width))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .px(px(12.0))
            .py(px(10.0))
            .rounded(px(14.0))
            .border_1()
            .border_color(rgba(theme::HAIRLINE_STRONG))
            .bg(rgba(theme::palette().popover))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0.0, 0.0, 0.0, 0.28),
                offset: point(px(0.0), px(6.0)),
                blur_radius: px(18.0),
                spread_radius: px(0.0),
                inset: false,
            }])
            .children(self.items(&card.card, cx));
        let strip = div()
            .id(ElementId::NamedInteger("page-hover-card-card".into(), id))
            .occlude()
            .pt(px(GAP))
            .child(body)
            .on_hover(cx.listener(move |view, over: &bool, window, cx| {
                view.hover_card_over(id, true, false, *over, window, cx)
            }));
        // The strip's own top is the gap, so the pointer crosses no hole.
        trigger
            .child(popup(bounds, strip).side_offset(0.0).layer())
            .into_any_element()
    }

    /// The pointer entered or left what a card is for, or the card. Coming onto what a card is for starts the wait
    /// (`OPEN_DELAY`) with the cue by the pointer, unless a card of the page is open already: then the next opens at
    /// once, as tooltips do. Leaving it before the wait ends cancels it. An open card stays while the pointer is on
    /// either, and goes once it is on neither, checked after the move: crossing from one to the other leaves one before
    /// it enters the other. The card's strip starts right under what it is for, so nothing lies between.
    fn hover_card_over(&mut self, id: u64, card: bool, no_card: bool, over: bool, window: &Window, cx: &mut Context<Self>) {
        let (notify, wait) = {
            let mut states = self.hover_cards.borrow_mut();
            let other_open = states.iter().any(|(other, state)| *other != id && state.shown);
            let state = states.entry(id).or_default();
            if card {
                state.over_card = over;
            } else {
                state.over_trigger = over;
            }
            let (mut notify, mut wait) = (false, None);
            if over && !card && !state.shown && !no_card {
                if other_open {
                    state.shown = true;
                    notify = true;
                } else {
                    state.waits += 1;
                    state.waiting = Some(Waiting { at: window.mouse_position(), started: live::clock(cx) });
                    wait = Some(state.waits);
                    notify = true;
                }
            } else if over && card {
                state.shown = true;
            } else if !over && !card && state.waiting.take().is_some() {
                state.waits += 1;
                notify = true;
            }
            (notify, wait)
        };
        if notify {
            cx.notify();
        }
        if let Some(wait) = wait {
            cx.spawn(async move |view, cx| {
                cx.background_executor().timer(OPEN_DELAY).await;
                let _ = view.update(cx, |view, cx| {
                    let opened = view.hover_cards.borrow_mut().get_mut(&id).is_some_and(|state| {
                        let waited = state.waits == wait && state.over_trigger && state.waiting.take().is_some();
                        state.shown |= waited;
                        waited
                    });
                    if opened {
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        if over {
            return;
        }
        cx.spawn(async move |view, cx| {
            let _ = view.update(cx, |view, cx| {
                let hidden = view.hover_cards.borrow_mut().get_mut(&id).is_some_and(|state| {
                    !state.over_trigger && !state.over_card && std::mem::take(&mut state.shown)
                });
                if hidden {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Whether the cue of `id` shows, the card waiting to open.
    #[cfg(all(test, feature = "test-support"))]
    pub(crate) fn hover_card_waiting(&self, id: u64) -> bool {
        self.hover_cards.borrow().get(&id).is_some_and(|state| state.waiting.is_some())
    }

    /// Whether the card of `id` shows.
    #[cfg(all(test, feature = "test-support"))]
    pub(crate) fn hover_card_shown(&self, id: u64) -> bool {
        self.hover_cards.borrow().get(&id).is_some_and(|state| state.shown)
    }
}
