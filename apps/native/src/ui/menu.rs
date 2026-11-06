//! shadcn's `ContextMenu` (`apps/desktop/src/components/ui/context-menu.tsx`): a popover box of items under the
//! pointer, each an icon and a label, with hairlines between groups.

use gpui::{
    AnyElement, BoxShadow, Div, ElementId, IntoElement, Pixels, Point, Role, SharedString,
    Stateful, anchored, deferred, div, hsla, point, prelude::*, px, rgb, rgba,
};

use super::DIALOG;
use crate::{
    icons::{Icon, icon},
    theme,
};

/// The desktop's `--accent` under the item you point at.
const ACCENT: u32 = 0xffffff14;

/// The open menu, drawn above everything else with its corner at `at` (in window coordinates), turned to stay in the
/// window. Add the close handler (`on_mouse_down_out`) to `menu`.
pub fn context_menu(at: Point<Pixels>, menu: Stateful<Div>) -> AnyElement {
    deferred(anchored().position(at).child(menu))
        .with_priority(1)
        .into_any_element()
}

/// The menu's box, 144px wide at least, around `items`.
pub fn menu(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Menu)
        .aria_label(label)
        .occlude()
        .min_w(px(144.0))
        .p(px(4.0))
        .flex()
        .flex_col()
        .rounded(px(8.0))
        .border_1()
        .border_color(rgba(0xece9e41a))
        .bg(rgb(DIALOG))
        .text_color(rgb(theme::FOREGROUND))
        .shadow(vec![BoxShadow {
            color: hsla(0.0, 0.0, 0.0, 0.3),
            offset: point(px(0.0), px(4.0)),
            blur_radius: px(12.0),
            spread_radius: px(0.0),
            inset: false,
        }])
}

/// One item: its icon and label, red when it destroys something, dimmed and inert while `disabled`. Add the click
/// handler to an enabled one.
pub fn menu_item(
    id: impl Into<ElementId>,
    glyph: Icon,
    label: impl Into<SharedString>,
    destructive: bool,
    disabled: bool,
) -> Stateful<Div> {
    let label = label.into();
    let color = if destructive {
        theme::DANGER
    } else {
        theme::FOREGROUND
    };
    div()
        .id(id)
        .role(Role::MenuItem)
        .aria_label(label.clone())
        .flex()
        .items_center()
        .gap(px(6.0))
        .px(px(6.0))
        .py(px(4.0))
        .rounded(px(6.0))
        .text_size(px(14.0))
        .text_color(rgb(color))
        .whitespace_nowrap()
        .child(icon(
            glyph,
            16.0,
            if destructive {
                theme::DANGER
            } else {
                theme::MUTED_FOREGROUND
            },
        ))
        .child(div().flex_1().min_w_0().truncate().child(label))
        .when(disabled, |item| {
            item.opacity(0.5).a11y_synthetic_children(|builder| {
                builder.parent_node().set_disabled();
            })
        })
        .when(!disabled, |item| {
            item.cursor_pointer().hover(move |style| {
                style.bg(rgba(if destructive {
                    theme::DANGER << 8 | 0x33
                } else {
                    ACCENT
                }))
            })
        })
}

/// The hairline between groups of items.
pub fn menu_separator() -> AnyElement {
    div()
        .mx(px(-4.0))
        .my(px(4.0))
        .h(px(1.0))
        .bg(rgba(theme::BORDER))
        .into_any_element()
}
