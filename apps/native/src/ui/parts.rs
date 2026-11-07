//! The small parts plugin pages are made of (`wings-plugin-ui`'s `Card`, `Heading`, `List`, `Dot`, `Gauge` and
//! `Disclosure`), drawn as `packages/plugin-ui/plugin-ui.css` draws them. They have no shadcn twin in the app, so they
//! sit apart from the kit's settings controls.

use gpui::{
    AnyElement, Div, ElementId, FontWeight, IntoElement, Role, SharedString, Stateful, div,
    prelude::*, px, relative, rgb, rgba,
};

use super::HAIRLINE;
use crate::{
    icons::{Icon, icon},
    theme,
};

/// `--wings-surface`, under a card.
const SURFACE: u32 = 0xffffff0a;
/// `--wings-hairline-strong` as it sits under a bar's fill.
const TRACK: u32 = 0xffffff29;

/// `.w-card`: a rounded box with a hairline edge around related content, 10px apart.
pub fn plain_card(content: impl IntoIterator<Item = AnyElement>) -> Div {
    div()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .px(px(12.0))
        .py(px(10.0))
        .rounded(px(theme::RADIUS))
        .border_1()
        .border_color(rgba(HAIRLINE))
        .bg(rgba(SURFACE))
        .children(content)
}

/// `.w-board-column` and `.w-board-cell`: a box on the surface colour with a hairline edge, 8px in.
pub fn board_box() -> Div {
    div()
        .p(px(8.0))
        .rounded(px(theme::RADIUS))
        .border_1()
        .border_color(rgba(HAIRLINE))
        .bg(rgba(SURFACE))
}

/// `.w-heading`: a section's name in small muted type, a little further from what's above than other rows are.
pub fn heading(content: impl IntoElement) -> Div {
    div()
        .mt(px(6.0))
        .text_size(px(12.0))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(theme::MUTED_FOREGROUND))
        .child(content)
}

/// `.w-list`: rows one under the other, in 12px type.
pub fn list(rows: impl IntoIterator<Item = AnyElement>) -> Div {
    div()
        .min_w_0()
        .flex()
        .flex_col()
        .text_size(px(12.0))
        .children(rows)
}

/// `.w-list-item`: a row that lights up under the pointer, its parts side by side.
pub fn list_item(
    id: impl Into<ElementId>,
    parts: impl IntoIterator<Item = AnyElement>,
) -> Stateful<Div> {
    div()
        .id(id)
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(8.0))
        .px(px(6.0))
        .py(px(4.0))
        .rounded(px(6.0))
        .hover(|style| style.bg(rgba(theme::HOVER)))
        .children(parts)
}

/// `.w-dot`: a 6px status dot in `color`.
pub fn dot(color: u32) -> Div {
    div()
        .size(px(6.0))
        .flex_none()
        .rounded_full()
        .bg(rgb(color))
}

/// `.w-gauge`: a label and its percentage over a 4px bar filled to `percent` (0 to 100) in `color`, and a note under
/// them when there is one.
pub fn gauge(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    percent: f64,
    color: u32,
    note: Option<AnyElement>,
) -> Stateful<Div> {
    let label = label.into();
    let fill = (percent / 100.0).clamp(0.0, 1.0) as f32;
    div()
        .id(id)
        .role(Role::Meter)
        .aria_label(label.clone())
        .aria_min_numeric_value(0.0)
        .aria_max_numeric_value(100.0)
        .aria_numeric_value(percent)
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .text_size(px(12.0))
        .child(
            div()
                .flex()
                .gap(px(8.0))
                .child(div().flex_1().min_w_0().child(label))
                .child(
                    div()
                        .flex_none()
                        .font_weight(FontWeight::MEDIUM)
                        .child(format!("{percent}%")),
                ),
        )
        .child(
            div()
                .h(px(4.0))
                .rounded(px(2.0))
                .overflow_hidden()
                .bg(rgba(TRACK))
                .child(
                    div()
                        .h_full()
                        .w(relative(fill))
                        .rounded(px(2.0))
                        .bg(rgb(color)),
                ),
        )
        .when_some(note, |gauge, note| {
            gauge.child(div().text_color(rgb(theme::MUTED_FOREGROUND)).child(note))
        })
}

/// `.w-disclosure-trigger`: the line that opens a disclosure, a chevron and its summary in muted type that lights up
/// under the pointer. The chevron turns down while it's open. Add the click handler to the result.
pub fn disclosure_trigger(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    open: bool,
    summary: impl IntoElement,
) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .aria_expanded(open)
        .flex()
        .items_center()
        .gap(px(4.0))
        .px(px(6.0))
        .py(px(4.0))
        .rounded(px(6.0))
        .cursor_pointer()
        .text_size(px(12.0))
        .text_color(rgb(theme::MUTED_FOREGROUND))
        .hover(|style| style.text_color(rgb(theme::FOREGROUND)))
        .focusable()
        .tab_stop(true)
        .focus_visible(|style| style.bg(rgba(theme::HOVER)))
        .child(icon(
            if open {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            },
            12.0,
            theme::MUTED_FOREGROUND,
        ))
        .child(summary)
}
