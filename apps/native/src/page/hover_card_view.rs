//! The `HoverCard`: what it holds, drawn where it sits, and a card over the page under it while the pointer is on
//! either, as a link's preview. The card is the view's own state, so hovering never reaches the page.
use super::*;
use gpui::{Anchor, Bounds, anchored, canvas, deferred};

/// The room between what the card is for and the card, which the pointer crosses without the card going away.
const GAP: f32 = 6.0;

#[derive(Default)]
pub(super) struct HoverState {
    /// Where what it holds was last drawn, in the window.
    bounds: Bounds<Pixels>,
    over_trigger: bool,
    over_card: bool,
    shown: bool,
}

impl PageView {
    pub(super) fn hover_card(&self, card: &tree::HoverCard, cx: &mut Context<Self>) -> AnyElement {
        let id = card.id;
        let (shown, bounds) = self
            .hover_cards
            .borrow()
            .get(&id)
            .map_or((false, Bounds::default()), |state| (state.shown, state.bounds));
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
            .on_hover(cx.listener(move |view, over: &bool, _, cx| view.hover_card_over(id, false, *over, cx)));
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
            .bg(rgba(theme::POPOVER))
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
            .on_hover(cx.listener(move |view, over: &bool, _, cx| view.hover_card_over(id, true, *over, cx)));
        let at = point(bounds.left(), bounds.bottom());
        trigger
            .child(deferred(anchored().anchor(Anchor::TopLeft).position(at).snap_to_window_with_margin(px(8.0)).child(strip)).with_priority(1))
            .into_any_element()
    }

    /// The pointer entered or left what a card is for, or the card. The card shows once the pointer is on the first, stays
    /// while it is on either, and goes once it is on neither, checked after the move: crossing from one to the other
    /// leaves one before it enters the other. The card's strip starts right under what it is for, so nothing lies between.
    fn hover_card_over(&mut self, id: u64, card: bool, over: bool, cx: &mut Context<Self>) {
        let show = {
            let mut states = self.hover_cards.borrow_mut();
            let state = states.entry(id).or_default();
            if card {
                state.over_card = over;
            } else {
                state.over_trigger = over;
            }
            let show = over && !state.shown;
            state.shown |= over;
            show
        };
        if show {
            cx.notify();
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

    /// Whether the card of `id` shows.
    #[cfg(all(test, feature = "test-support"))]
    pub(crate) fn hover_card_shown(&self, id: u64) -> bool {
        self.hover_cards.borrow().get(&id).is_some_and(|state| state.shown)
    }
}
