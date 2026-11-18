//! The desktop's `Sheet` from the right, as Past sessions draws it and a plugin page's `Sheet` does: a panel the height
//! of what it covers over a dimmed backdrop, its heading and controls over a hairline, and what it lists under that.

use gpui::{
    AnyElement, BoxShadow, Div, ElementId, FocusHandle, FontWeight, IntoElement, MouseButton,
    Role, SharedString, Stateful, Toggled, div, hsla, point, prelude::*, px, relative, rgb, rgba,
};

use super::{HAIRLINE, INPUT_BORDER, dialog};
use crate::{
    icons::{Icon, icon},
    theme,
};

/// The sheet's width: the desktop's `data-[side=right]:w-[480px]`.
pub const SHEET_WIDTH: f32 = 480.0;
/// The room `search_box` leaves its field in a sheet's header: the sheet less its padding, the box's own and its icons.
pub const SHEET_SEARCH_WIDTH: f32 = 388.0;
/// Behind a searched word where it is found, as Past sessions marks it.
pub const SEARCH_MARK: u32 = theme::WORKING << 8 | 0x40;

/// The sheet's backdrop, over what `id`'s sheet covers, and its panel against the right edge, as `dialog` makes them:
/// add the close handlers to the backdrop, and the header and the list to the panel.
pub fn sheet(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    focus: &FocusHandle,
) -> (Stateful<Div>, Stateful<Div>) {
    let (backdrop, panel) = dialog(id, label, focus, SHEET_WIDTH, 0.0);
    let panel = panel
        .h_full()
        .max_h(relative(1.0))
        .max_w(relative(0.92))
        .rounded(px(0.0))
        .border_0()
        .border_l_1()
        .border_color(rgba(theme::HAIRLINE_STRONG))
        .shadow(vec![BoxShadow {
            color: hsla(0.0, 0.0, 0.0, 0.5),
            offset: point(px(0.0), px(24.0)),
            blur_radius: px(64.0),
            spread_radius: px(0.0),
            inset: false,
        }])
        .flex_col()
        .bg(rgba(theme::POPOVER));
    (backdrop.items_stretch().justify_end(), panel)
}

/// The sheet's top: its title over a line saying what it lists, then what the caller adds (a search, filters), 12
/// points apart, over a hairline.
pub fn sheet_header(title: impl IntoElement, description: impl IntoElement) -> Div {
    let heading = div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(
            div()
                .text_size(px(16.0))
                .font_weight(FontWeight::MEDIUM)
                .child(title),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(rgb(theme::MUTED_FOREGROUND))
                .child(description),
        );
    div()
        .flex_none()
        .flex()
        .flex_col()
        .gap(px(12.0))
        .p(px(16.0))
        .border_b_1()
        .border_color(rgba(HAIRLINE))
        .child(heading)
}

/// What a sheet lists, under its header: a column 8 points in from its edges.
pub fn sheet_list() -> Div {
    div().flex().flex_col().p(px(8.0))
}

/// `button`, the sheet's close button, in its top right corner.
pub fn sheet_corner(button: impl IntoElement) -> Div {
    div().absolute().top(px(12.0)).right(px(12.0)).child(button)
}

/// The sheet's search: a search icon, `field`, and a slot after it for `status`, like a spinner while it searches.
pub fn search_box(field: impl IntoElement, status: Option<AnyElement>) -> Div {
    div()
        .h(px(32.0))
        .flex()
        .items_center()
        .gap(px(4.0))
        .pl(px(10.0))
        .pr(px(8.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(rgba(INPUT_BORDER))
        .bg(rgba(theme::HOVER))
        .child(icon(Icon::Search, 16.0, theme::MUTED_FOREGROUND))
        .child(div().min_w_0().flex_1().overflow_hidden().child(field))
        .child(
            div()
                .size(px(16.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .children(status),
        )
}

/// The desktop's `ToggleGroup`: buttons side by side on a faint track.
pub fn segmented(items: impl IntoIterator<Item = Stateful<Div>>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(2.0))
        .p(px(2.0))
        .rounded(px(8.0))
        .bg(rgba(0xffffff0d))
        .children(items)
}

/// One button of `segmented`, lit while `on`.
pub fn segment(id: impl Into<ElementId>, label: impl Into<SharedString>, on: bool) -> Stateful<Div> {
    let label = label.into();
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label.clone())
        .aria_toggled(if on { Toggled::True } else { Toggled::False })
        .h(px(24.0))
        .flex()
        .items_center()
        .px(px(10.0))
        .rounded(px(6.0))
        .cursor_pointer()
        .text_size(px(12.0))
        .text_color(rgb(if on {
            theme::FOREGROUND
        } else {
            theme::MUTED_FOREGROUND
        }))
        .when(on, |item| item.bg(rgba(0xffffff17)))
        .when(!on, |item| {
            item.hover(|style| style.text_color(rgb(theme::FOREGROUND)))
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(label)
}
