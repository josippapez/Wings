//! Text buttons and badges in shadcn's variants, as plugin pages ask for them (`wings-plugin-ui`'s `Button` and
//! `Badge`). The square icon buttons are `ghost_button` and `icon_button`.

use gpui::{
    Div, ElementId, IntoElement, MouseButton, Role, SharedString, Stateful, div, prelude::*, px,
    rgb, rgba,
};

use super::INPUT_BORDER;
use crate::theme;

/// The `variant` of both a button and a badge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Variant {
    #[default]
    Default,
    Secondary,
    Outline,
    Ghost,
    Destructive,
    Link,
}

/// `--wings-primary-fg`: the dark text on the light filled button.
const PRIMARY_FG: u32 = 0x17151a;

/// A variant's text colour, background and hover background, as plugin-ui.css sets them.
fn colors(variant: Variant) -> (u32, u32, u32) {
    match variant {
        Variant::Default => (PRIMARY_FG, theme::FOREGROUND << 8 | 0xff, 0xece9e4cc),
        Variant::Secondary => (theme::FOREGROUND, 0xffffff14, 0xffffff1d),
        Variant::Outline => (theme::FOREGROUND, theme::HOVER, theme::HOVER_STRONG),
        Variant::Ghost => (theme::FOREGROUND, 0, 0xffffff08),
        Variant::Destructive => (
            theme::DANGER,
            theme::DANGER << 8 | 0x33,
            theme::DANGER << 8 | 0x4d,
        ),
        Variant::Link => (theme::FOREGROUND, 0, 0),
    }
}

/// shadcn's `Button` at its default size, 32px tall around `content`. A disabled one is dimmed and, with no
/// click handler on it, does nothing, and the keyboard skips it. Add the click handler to the result.
pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    variant: Variant,
    disabled: bool,
    content: impl IntoElement,
) -> Stateful<Div> {
    let (fg, bg, hover) = colors(variant);
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .h(px(32.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .px(px(10.0))
        .rounded(px(theme::RADIUS))
        .border_1()
        .border_color(rgba(if variant == Variant::Outline {
            INPUT_BORDER
        } else {
            0
        }))
        .bg(rgba(bg))
        .text_color(rgb(fg))
        .text_size(px(14.0))
        .font_weight(gpui::FontWeight::MEDIUM)
        .whitespace_nowrap()
        .focus_visible(|style| style.border_color(rgb(theme::FOREGROUND)))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .when(disabled, |button| {
            button.opacity(0.5).a11y_synthetic_children(|builder| {
                builder.parent_node().set_disabled();
            })
        })
        .when(!disabled, |button| {
            button
                .focusable()
                .tab_stop(true)
                .cursor_pointer()
                .hover(|style| style.bg(rgba(hover)))
        })
        .child(content)
}

/// shadcn's `Badge`: a small pill. `tone` is a colour that says something (plugin pages' `tone`), as an RGB; it
/// tints the pill and colours the text, and with it a badge no longer has its variant's fill.
pub fn badge(variant: Variant, tone: Option<u32>, small: bool, content: impl IntoElement) -> Div {
    let (fg, bg, _) = colors(variant);
    let (fg, bg, border) = match (tone, variant) {
        (Some(tone), Variant::Outline) => (tone, 0, tone << 8 | 0x73),
        (Some(tone), _) => (tone, tone << 8 | 0x33, 0),
        (None, Variant::Outline) => (fg, 0, INPUT_BORDER),
        // A secondary badge is the same pill as a secondary button, and a default one the filled pill.
        (None, _) => (fg, bg, 0),
    };
    div()
        .h(px(if small { 18.0 } else { 20.0 }))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(4.0))
        .px(px(8.0))
        .overflow_hidden()
        .rounded_full()
        .border_1()
        .border_color(rgba(border))
        .bg(rgba(bg))
        .text_color(rgb(fg))
        .text_size(px(if small { 11.0 } else { 12.0 }))
        .font_weight(gpui::FontWeight::MEDIUM)
        .whitespace_nowrap()
        .child(content)
}
