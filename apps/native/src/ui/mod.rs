//! Wings' native UI kit: the desktop app's shadcn components (`apps/desktop/src/components/ui`) as
//! GPUI elements, so every screen shares one look. Each returns an element to add handlers to.

mod button;
mod menu;
pub mod motion;
mod parts;
mod status_dot;
mod tooltip;

pub use button::{Variant, badge, button, chip};
pub use menu::{context_menu, menu, menu_item, menu_item_with, menu_separator};
pub use parts::{disclosure_trigger, dot, gauge, heading, list, list_item, plain_card};
pub use status_dot::{spinner as working_spinner, status_dot};
pub use tooltip::tooltip;

use gpui::{
    AnyElement, BoxShadow, Div, ElementId, Entity, FocusHandle, FontWeight, IntoElement,
    MouseButton, Role, SharedString, Stateful, Toggled, anchored, deferred, div, hsla, point,
    prelude::*, px, relative, rgb, rgba,
};

use crate::{
    icons::{Icon, icon},
    text_field::TextField,
    theme,
};

/// The desktop's `--hairline` between rows and around cards.
pub const HAIRLINE: u32 = 0xffffff12;
/// The desktop's `--input` border around a field.
const INPUT_BORDER: u32 = 0xffffff1a;
/// The desktop dialog's `--popover`.
const DIALOG: u32 = 0x201d23;
/// The desktop's `bg-black/10` behind a dialog. Its `backdrop-blur-xs` has no GPUI counterpart.
const BACKDROP: u32 = 0x0000001a;
/// `--input` at 30%, the fill of a field or a select, and at 50% when pointed at.
const INPUT_FILL: u32 = 0xffffff08;
const INPUT_FILL_HOVER: u32 = 0xffffff0d;
/// The dialog's `ring-1 ring-foreground/10`.
const DIALOG_RING: u32 = theme::FOREGROUND << 8 | 0x1a;

/// shadcn's square button sizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonSize {
    /// 20px, a tab's close button.
    Tiny,
    /// `icon-xs`, 24px: the sidebar's small header buttons.
    Xs,
    /// `icon-sm`, 28px: chrome and bar buttons.
    Sm,
}

impl ButtonSize {
    fn side(self) -> f32 {
        match self {
            ButtonSize::Tiny => 20.0,
            ButtonSize::Xs => 24.0,
            ButtonSize::Sm => 28.0,
        }
    }

    /// shadcn's `rounded-[min(var(--radius-md),…)]`: 8px at every size.
    fn radius(self) -> f32 {
        8.0
    }

    fn icon(self) -> f32 {
        match self {
            ButtonSize::Sm => 16.0,
            _ => 14.0,
        }
    }
}

/// shadcn's ghost button around any content: a soft hover, a focus ring, and pressed while `on`
/// for a toggle. Its tooltip names it, with `shortcut` where it has one. A click on it never reaches what's behind,
/// like a window drag or pane focus.
pub fn ghost_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    shortcut: Option<SharedString>,
    size: ButtonSize,
    on: Option<bool>,
    content: impl IntoElement,
) -> Stateful<Div> {
    let pressed = on == Some(true);
    let label = label.into();
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label.clone())
        .tooltip(tooltip(label, shortcut))
        .when_some(on, |button, on| {
            button.aria_toggled(if on { Toggled::True } else { Toggled::False })
        })
        .size(px(size.side()))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(size.radius()))
        .border_1()
        .border_color(rgba(0))
        .cursor_pointer()
        .text_size(px(13.0))
        .text_color(rgb(if pressed {
            theme::FOREGROUND
        } else {
            theme::MUTED_FOREGROUND
        }))
        .when(pressed, |button| button.bg(rgba(theme::HOVER)))
        .hover(|style| {
            style
                .bg(rgba(theme::HOVER))
                .text_color(rgb(theme::FOREGROUND))
        })
        .focus_visible(|style| style.border_color(rgb(theme::FOREGROUND)))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(content)
}

/// The desktop's `IconButton`: a ghost button holding one Lucide icon.
pub fn icon_button(
    id: impl Into<ElementId>,
    glyph: Icon,
    label: impl Into<SharedString>,
    size: ButtonSize,
) -> Stateful<Div> {
    shortcut_icon_button(id, glyph, label, None, size)
}

/// An `icon_button` whose tooltip shows its shortcut too, as the desktop's `IconButton shortcut=`.
pub fn shortcut_icon_button(
    id: impl Into<ElementId>,
    glyph: Icon,
    label: impl Into<SharedString>,
    shortcut: Option<SharedString>,
    size: ButtonSize,
) -> Stateful<Div> {
    ghost_button(
        id,
        label,
        shortcut,
        size,
        None,
        icon(glyph, size.icon(), theme::MUTED_FOREGROUND),
    )
}

/// shadcn's `Switch`: a 32 by 18.4 track inside a transparent 1px border, with a 16px knob, on the right and dark when on.
pub fn switch(id: impl Into<ElementId>, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Switch)
        .aria_toggled(if on { Toggled::True } else { Toggled::False })
        .w(px(32.0))
        .h(px(18.4))
        .flex_none()
        .flex()
        .items_center()
        .rounded_full()
        .border_1()
        .border_color(rgba(0))
        .cursor_pointer()
        // Off, `dark:bg-input/80`.
        .bg(rgba(if on {
            theme::FOREGROUND << 8 | 0xff
        } else {
            0xffffff14
        }))
        .when(on, |track| track.justify_end())
        .child(div().size(px(16.0)).rounded_full().bg(rgb(if on {
            0x17151a
        } else {
            theme::FOREGROUND
        })))
}

/// shadcn's `Input` frame around a text field: `rounded-lg`, `dark:bg-input/30` and 14px text.
pub fn input(field: Entity<TextField>) -> Div {
    div()
        .h(px(32.0))
        .flex()
        .items_center()
        .rounded(px(theme::RADIUS))
        .border_1()
        .border_color(rgba(INPUT_BORDER))
        .bg(rgba(INPUT_FILL))
        .text_size(px(14.0))
        .child(field)
}

/// A rounded box of rows split by hairlines, like the desktop's settings groups.
pub fn card(rows: impl IntoIterator<Item = AnyElement>) -> Div {
    div()
        .flex()
        .flex_col()
        .rounded(px(14.0))
        .border_1()
        .border_color(rgba(HAIRLINE))
        .bg(rgba(0xffffff08))
        .children(rows.into_iter().enumerate().map(|(index, row)| {
            div()
                .when(index > 0, |row| {
                    row.border_t_1().border_color(rgba(HAIRLINE))
                })
                .child(row)
        }))
}

/// One setting in a card: its name and what it does on the left, its control on the right.
pub fn setting_row(
    label: impl Into<SharedString>,
    description: impl Into<SharedString>,
    error: Option<SharedString>,
    control: impl IntoElement,
) -> AnyElement {
    let description: SharedString = description.into();
    div()
        .flex()
        .items_center()
        .gap(px(24.0))
        .px(px(16.0))
        .py(px(12.0))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.0))
                // The desktop's `Label` is `leading-none`, and the hint under it `leading-snug`.
                .child(
                    div()
                        .text_size(px(13.0))
                        .line_height(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .child(label.into()),
                )
                // A row without a hint, like the desktop's, keeps its label in the middle.
                .when(!description.is_empty(), |text| {
                    text.child(
                        div()
                            .text_size(px(12.0))
                            .line_height(px(16.5))
                            .text_color(rgb(theme::MUTED_FOREGROUND))
                            .child(description),
                    )
                })
                .when_some(error, |text, error| {
                    text.child(
                        div()
                            .text_size(px(12.0))
                            .text_color(rgb(theme::DANGER))
                            .child(error),
                    )
                }),
        )
        .child(control)
        .into_any_element()
}

/// shadcn's `Dialog`: a dimmed backdrop over the window, and the panel in its middle. Add the
/// close handlers to the backdrop and the content to the panel.
pub fn dialog(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    focus: &FocusHandle,
    width: f32,
    height: f32,
) -> (Stateful<Div>, Stateful<Div>) {
    let id: ElementId = id.into();
    let backdrop = div()
        .id(ElementId::Name(format!("{id}-backdrop").into()))
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgba(BACKDROP))
        .occlude();
    let panel = div()
        .id(id)
        .role(Role::Dialog)
        .aria_label(label)
        .track_focus(focus)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .relative()
        .w(px(width))
        .max_w(relative(0.92))
        .h(px(height))
        .max_h(relative(0.9))
        .flex()
        .overflow_hidden()
        .rounded(px(14.0))
        .border_1()
        .border_color(rgba(DIALOG_RING))
        .bg(rgb(DIALOG))
        .text_color(rgb(theme::FOREGROUND));
    (backdrop, panel)
}

/// shadcn's `SelectTrigger`: the current choice and a chevron, 176px wide like the desktop's.
pub fn select_trigger(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::ComboBox)
        .aria_label(label)
        .w(px(176.0))
        .h(px(32.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .pl(px(10.0))
        .pr(px(8.0))
        .rounded(px(theme::RADIUS))
        .border_1()
        .border_color(rgba(INPUT_BORDER))
        .bg(rgba(INPUT_FILL))
        .cursor_pointer()
        .text_size(px(14.0))
        .hover(|style| style.bg(rgba(INPUT_FILL_HOVER)))
        .focus_visible(|style| style.border_color(rgb(theme::FOREGROUND)))
        .child(div().truncate().child(value.into()))
        .child(icon(Icon::ChevronDown, 16.0, theme::MUTED_FOREGROUND))
}

/// One choice in a select's menu, with a check on the chosen one.
pub fn select_item(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
) -> Stateful<Div> {
    let label = label.into();
    div()
        .id(id)
        .role(Role::MenuItem)
        .aria_label(label.clone())
        .aria_selected(selected)
        .h(px(32.0))
        .flex()
        .items_center()
        .justify_between()
        .px(px(8.0))
        .rounded(px(6.0))
        .cursor_pointer()
        .text_size(px(13.0))
        .hover(|style| style.bg(rgba(theme::HOVER_STRONG)))
        .child(label)
        .when(selected, |item| {
            item.child(icon(Icon::Check, 14.0, theme::FOREGROUND))
        })
}

/// A select's open menu: a popover box 4px under its 32px trigger, drawn above everything else.
/// Put it in a column with the trigger.
pub fn select_menu(items: impl IntoIterator<Item = AnyElement>) -> Div {
    div()
        .w(px(176.0))
        // A popover starts where its column does, so it clears the trigger itself.
        .mt(px(36.0))
        .p(px(4.0))
        .flex()
        .flex_col()
        .rounded(px(10.0))
        .border_1()
        .border_color(rgba(theme::HAIRLINE_STRONG))
        .bg(rgb(DIALOG))
        .shadow(vec![BoxShadow {
            color: hsla(0.0, 0.0, 0.0, 0.4),
            offset: point(px(0.0), px(8.0)),
            blur_radius: px(24.0),
            spread_radius: px(0.0),
            inset: false,
        }])
        .children(items)
}

/// Draws `content` above the rest of the window, where it would sit in the layout, fading and sliding it down into
/// place as the desktop's popovers do.
pub fn popover(content: impl IntoElement) -> AnyElement {
    let content = motion::popup_in("popover-in", motion::From::Above, content);
    deferred(anchored().child(content)).into_any_element()
}
