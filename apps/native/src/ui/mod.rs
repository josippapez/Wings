//! Wings' native UI kit: the desktop app's shadcn components (`apps/desktop/src/components/ui`) as
//! GPUI elements, so every screen shares one look. Each returns an element to add handlers to.

mod backdrop;
pub mod behaviors;
mod button;
#[cfg(feature = "test-support")]
pub mod cutoff;
/// Without `test-support`, nothing notes what a frame cuts off: the pieces `cutoff` reads add nothing to a frame.
#[cfg(not(feature = "test-support"))]
pub(crate) mod cutoff {
    pub(crate) fn named(_: &str) -> Option<gpui::AnyElement> {
        None
    }

    pub(crate) fn label(_: impl FnOnce() -> Option<String>) -> Option<gpui::AnyElement> {
        None
    }

    pub(crate) fn clips_on_purpose() -> Option<gpui::AnyElement> {
        None
    }

    pub(crate) fn card() -> (Option<gpui::AnyElement>, Option<gpui::AnyElement>) {
        (None, None)
    }
}
mod drag;
mod glow;
pub mod hover_cue;
pub mod live;
mod menu;
pub mod motion;
#[cfg(target_os = "macos")]
mod native_backdrop;
#[cfg(target_os = "macos")]
mod native_spinners;
mod parts;
pub mod readout;
#[cfg(target_os = "macos")]
mod shader;
mod sheet;
mod shimmer;
pub(crate) mod slots;
mod spinner_host;
mod status_dot;
mod switch;
mod tooltip;

pub use backdrop::{BackdropLayer, backdrop_slot, window_glass};
#[cfg(test)]
pub use backdrop::PAINTED_GLASS;
#[cfg(feature = "test-support")]
pub use backdrop::set_screenshot_ground;
#[cfg(target_os = "macos")]
pub use backdrop::draw_backdrops_under;
#[cfg(all(test, feature = "test-support"))]
pub(crate) use backdrop::STEP as BACKDROP_STEP;
pub use button::{Variant, badge, button, chip};
pub use glow::glow;
pub use live::view_spinner_layer;
pub use drag::{RESIZE_EDGE, Side, resize_edge, window_drag};
pub use menu::{
    context_menu, dropdown_menu, dropup_menu, menu, menu_highlight, menu_item, menu_item_text, menu_item_with, menu_separator,
};
pub use parts::{board_box, disclosure_trigger, dot, gauge, heading, list, list_item, plain_card};
pub use sheet::{
    search_mark, SHEET_SEARCH_WIDTH, SHEET_WIDTH, search_box, segment, segmented, sheet, sheet_corner,
    sheet_header, sheet_list,
};
pub use shimmer::{count_slot, shimmer, shimmer_slot};
#[cfg(all(test, feature = "test-support"))]
pub(crate) use status_dot::SPIN_STEP;
pub use slots::{Piece, bounds_into, cover, drawing_piece, passive_cover, in_piece, start_draw, forget_piece, forget_view, place_view, slot, slot_layer, under, view_placed};
pub use spinner_host::spinner_sync;
#[cfg(target_os = "macos")]
pub use spinner_host::turn_spinners_natively;
pub use status_dot::{
    spinner as working_spinner, spinner_layer, spinner_slot, status_dot, status_dot_slot, view_spinner_slot,
    view_status_dot_slot,
};
pub use switch::switch;
pub use tooltip::{hold_tooltips, tip, tooltip, tooltip_exit, tooltip_with};
#[cfg(all(test, feature = "test-support"))]
pub(crate) use tooltip::shown_in as tooltip_shown_in;

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
/// Room the window's close, minimize and zoom buttons take at the left of whatever is drawn under the title bar, as the
/// desktop's `pl-[84px]`.
pub const TRAFFIC_LIGHTS: f32 = 84.0;
/// The desktop's `--input` border around a field.
const INPUT_BORDER: u32 = 0xffffff1a;
/// The desktop dialog's `--popover`.
/// Behind a dialog. The desktop's is `bg-black/10` with `backdrop-blur-xs`, and the blur is what sets the dialog apart;
/// GPUI can't blur what's behind an element, so at 10% the window behind read as not dimmed at all. 40% stands in.
const BACKDROP: u32 = 0x00000066;
/// `--input` at 30%, the fill of a field or a select, and at 50% when pointed at.
pub(crate) const INPUT_FILL: u32 = 0xffffff08;
const INPUT_FILL_HOVER: u32 = 0xffffff0d;
/// The dialog's `ring-1 ring-foreground/10`.
fn dialog_ring() -> u32 {
    theme::foreground() << 8 | 0x1a
}

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
    let label = label.into();
    untipped_ghost_button(id, label.clone(), size, on, content).tooltip(tooltip(label, shortcut))
}

/// `ghost_button` with no tooltip, for a button that shows something else when you point at it.
pub fn untipped_ghost_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    size: ButtonSize,
    on: Option<bool>,
    content: impl IntoElement,
) -> Stateful<Div> {
    let pressed = on == Some(true);
    let label = label.into();
    div()
        .id(id)
        .children(cutoff::named(&label))
        .role(Role::Button)
        .aria_label(label)
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
            theme::palette().foreground
        } else {
            theme::palette().muted_foreground
        }))
        .when(pressed, |button| button.bg(rgba(theme::hover())))
        .hover(|style| {
            style
                .bg(rgba(theme::hover()))
                .text_color(rgb(theme::palette().foreground))
        })
        .focus_visible(|style| style.border_color(rgb(theme::palette().foreground)))
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
        icon(glyph, size.icon(), theme::palette().muted_foreground),
    )
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
    let (opens, closes) = cutoff::card();
    div()
        .children(opens)
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
        .children(closes)
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
                            .text_color(rgb(theme::palette().muted_foreground))
                            .child(description),
                    )
                })
                .when_some(error, |text, error| {
                    text.child(
                        div()
                            .text_size(px(12.0))
                            .text_color(rgb(theme::palette().danger))
                            .child(error),
                    )
                }),
        )
        .child(control)
        .into_any_element()
}

/// shadcn's `Dialog`: a dimmed backdrop over the window, and the panel in its middle. Add the
/// close handlers to the backdrop and the content to the panel. With no `height`, the panel is as
/// tall as its content.
pub fn dialog(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    focus: &FocusHandle,
    width: f32,
    height: impl Into<Option<f32>>,
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
        // A margin to the window's edges, so the panel never reaches what is under its end (the composer).
        .py(px(32.0))
        .bg(rgba(BACKDROP))
        .occlude()
        .child(slots::backdrop_mark());
    let panel = div()
        .id(id)
        .role(Role::Dialog)
        .aria_label(label)
        .track_focus(focus)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .relative()
        .w(px(width))
        .max_w(relative(0.92))
        .map(|panel| match height.into() {
            Some(height) => panel.h(px(height)),
            None => panel.h_auto(),
        })
        .max_h(relative(1.0))
        .flex()
        .overflow_hidden()
        .rounded(px(14.0))
        .border_1()
        .border_color(rgba(dialog_ring()))
        .bg(rgb(theme::palette().dialog))
        .text_color(rgb(theme::palette().foreground))
        // Where it is, so a web page under the backdrop shows dimmed around it.
        .child(slots::cover(div().absolute().top_0().left_0().size_full()).rounded(14.0));
    (backdrop, panel)
}

/// In builds with `test-support`, notes where its parent is drawn under `name`, so the screenshot example points at
/// it from the layout as a person would, rather than at coordinates that go stale (`spot_at`). Nothing otherwise.
pub fn spot(name: impl FnOnce() -> String) -> Option<AnyElement> {
    #[cfg(feature = "test-support")]
    {
        let name = name();
        Some(
            gpui::canvas(
                move |bounds, _, _| SPOTS.with_borrow_mut(|spots| spots.insert(name, bounds)),
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .into_any_element(),
        )
    }
    #[cfg(not(feature = "test-support"))]
    {
        let _ = name;
        None
    }
}

#[cfg(feature = "test-support")]
thread_local! {
    static SPOTS: std::cell::RefCell<std::collections::HashMap<String, gpui::Bounds<gpui::Pixels>>> =
        Default::default();
}

/// Where the element spotted as `name` was last drawn.
#[cfg(feature = "test-support")]
pub fn spot_at(name: &str) -> Option<gpui::Bounds<gpui::Pixels>> {
    SPOTS.with_borrow(|spots| spots.get(name).copied())
}

/// Forgets every spot, so a test can tell an element that is gone from one drawn earlier.
#[cfg(feature = "test-support")]
pub fn forget_spots() {
    SPOTS.with_borrow_mut(|spots| spots.clear());
}

/// The dimness of a dialog's backdrop, for one that fades on its own.
pub fn backdrop_color() -> gpui::Hsla {
    rgba(BACKDROP).into()
}

/// A dialog's panel, with its children: its surface zooms as the desktop dialog's `zoom-in-95` does, about its middle
/// (see `motion::surface`).
pub fn dialog_panel(panel: impl Styled + IntoElement) -> motion::Surface {
    motion::surface(panel, (0.5, 0.5))
}

/// A dialog's content, 24px in from its edges and 16px between its parts.
pub fn dialog_body() -> Div {
    div().w_full().flex().flex_col().gap(px(16.0)).p(px(24.0))
}

/// The desktop's `DialogHeader`: the dialog's title over its description.
pub fn dialog_header(title: impl IntoElement, description: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(
            div()
                .text_size(px(16.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(description)
}

/// The desktop's `DialogFooter`: a dialog's or a notice's buttons, on the right and 8px apart.
pub fn dialog_footer() -> Div {
    div().flex().justify_end().gap(px(8.0))
}

/// shadcn's `SelectTrigger`: the current choice and a chevron, 176px wide like the desktop's.
pub fn select_trigger(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
) -> Stateful<Div> {
    select_trigger_with(id, label, div().truncate().child(value.into()))
}

/// A `select_trigger` showing `content` where its value goes.
pub fn select_trigger_with(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    content: impl IntoElement,
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
        .focus_visible(|style| style.border_color(rgb(theme::palette().foreground)))
        .child(content)
        .child(icon(Icon::ChevronDown, 16.0, theme::palette().muted_foreground))
}

/// The colour of a setting a select's menu offers as a toggle, and of the word its trigger shows while it is on.
pub const TOGGLE_ACCENT: u32 = 0xc4a7ff;

/// A select's toggle row: a setting, not a choice. A sparkle and its `label` in the accent colour, a muted `detail`
/// under it, and a switch showing `on`, on a faint accent ground, set apart from the choices above by a line.
pub fn toggle_item(
    id: ElementId,
    label: impl Into<SharedString>,
    detail: Option<SharedString>,
    on: bool,
) -> Stateful<Div> {
    let label = label.into();
    div()
        .id(id)
        .role(Role::MenuItem)
        .aria_label(label.clone())
        .aria_toggled(if on { Toggled::True } else { Toggled::False })
        .flex_none()
        .flex()
        .items_center()
        .gap(px(8.0))
        .px(px(8.0))
        .py(px(6.0))
        .rounded(px(6.0))
        .cursor_pointer()
        .bg(rgba(TOGGLE_ACCENT << 8 | 0x1a))
        .hover(|style| style.bg(rgba(TOGGLE_ACCENT << 8 | 0x2e)))
        .child(icon(Icon::Sparkles, 14.0, TOGGLE_ACCENT))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(px(13.0))
                        .text_color(rgb(TOGGLE_ACCENT))
                        .child(label),
                )
                .children(detail.map(|detail| {
                    div()
                        .text_size(px(11.0))
                        .text_color(rgb(theme::palette().muted_foreground))
                        .child(detail)
                })),
        )
        .child(switch("toggle-item-switch", on))
}

/// A section label in a select's menu: muted small text over the rows after it, not a choice. `first` is the menu's top
/// one, which needs no room above it.
pub fn select_heading(label: impl Into<SharedString>, first: bool) -> Div {
    div()
        .flex_none()
        .px(px(8.0))
        .pt(px(if first { 4.0 } else { 10.0 }))
        .pb(px(2.0))
        .text_size(px(11.0))
        .text_color(rgb(theme::palette().muted_foreground))
        .child(label.into())
}

/// A row of a select's menu that opens more rows under it, like a chat's Effort: its label, then at the end an optional
/// `pill`, the `trailing` text and a chevron that turns down while it is `open`. Set apart from the choices above by a
/// line.
pub fn select_expander(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    pill: Option<SharedString>,
    trailing: Option<SharedString>,
    open: bool,
) -> Stateful<Div> {
    let label = label.into();
    let muted = theme::palette().muted_foreground;
    div()
        .id(id)
        .role(Role::MenuItem)
        .aria_label(label.clone())
        .aria_expanded(open)
        .flex_none()
        .flex()
        .items_center()
        .gap(px(8.0))
        .h(px(36.0))
        .px(px(8.0))
        .rounded(px(6.0))
        .cursor_pointer()
        .text_size(px(13.0))
        .hover(|style| style.bg(rgba(theme::hover_strong())))
        .child(div().flex_1().min_w_0().truncate().child(label))
        .children(pill.map(|pill| {
            div()
                .flex_none()
                .px(px(8.0))
                .py(px(1.0))
                .rounded_full()
                .bg(rgba(theme::hover_strong()))
                .text_size(px(11.0))
                .text_color(rgb(muted))
                .child(pill)
        }))
        .children(trailing.map(|text| div().flex_none().child(text)))
        .child(icon(
            if open { Icon::ChevronDown } else { Icon::ChevronRight },
            14.0,
            muted,
        ))
}

/// A choice in a select's menu with a muted `detail` under its label, and a check on the chosen one.
pub fn select_item_detail(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    selected: bool,
    rule: bool,
) -> Stateful<Div> {
    let label = label.into();
    let content = div().flex().flex_col().child(label.clone()).child(
        div()
            .text_size(px(11.0))
            .text_color(rgb(theme::palette().muted_foreground))
            .child(detail.into()),
    );
    // Two lines: the row is as tall as they are instead of 32pt.
    let row = select_row(id.into(), label, content.into_any_element(), selected).h_auto().py(px(5.0));
    // A hairline between rows, which the row's own hover fill rounds past.
    if rule {
        row.border_t_1().border_color(rgba(HAIRLINE))
    } else {
        row
    }
}

/// One choice in a select's menu, with a check on the chosen one.
pub fn select_item(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
) -> Stateful<Div> {
    let label = label.into();
    select_row(id.into(), label.clone(), label.into_any_element(), selected)
}

/// A `select_item` with a 16px icon before its label, like a plugin's block in the project page's Add menu.
pub fn select_item_with(
    id: impl Into<ElementId>,
    glyph: impl IntoElement,
    label: impl Into<SharedString>,
) -> Stateful<Div> {
    let label = label.into();
    let content = div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(glyph)
        .child(label.clone());
    select_row(id.into(), label, content.into_any_element(), false)
}

/// A combobox's option, plugin-ui.css's `.w-option`: its label, cut short with an ellipsis, and its `detail` on the
/// right at 12px, muted, as a Cyclops project's client is.
pub fn option_item(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    detail: Option<SharedString>,
) -> Stateful<Div> {
    let label = label.into();
    let content = div()
        .w_full()
        .flex()
        .items_baseline()
        .gap(px(8.0))
        .child(behaviors::cut_text("label", label.clone()).flex_1())
        .children(detail.map(|detail| {
            div()
                .flex_none()
                // The label keeps what it needs up to the rest: a short name with a long description was cut at
                // under half the menu with the room beside it empty.
                .max_w(relative(0.7))
                .truncate()
                .text_size(px(12.0))
                .text_color(rgb(theme::palette().muted_foreground))
                .child(detail)
        }));
    select_row(id.into(), label, content.into_any_element(), false)
}

fn select_row(
    id: ElementId,
    label: SharedString,
    content: AnyElement,
    selected: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::MenuItem)
        .aria_label(label.clone())
        .aria_selected(selected)
        .h(px(32.0))
        // 32pt in a menu that scrolls too, which would otherwise squeeze its rows to fit.
        .flex_none()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .px(px(8.0))
        .rounded(px(6.0))
        .cursor_pointer()
        .text_size(px(13.0))
        .hover(|style| style.bg(rgba(theme::hover_strong())))
        // A label wider than the menu, like a Cyclops project's name and client, ends in an ellipsis inside it.
        .child(div().flex_1().min_w_0().truncate().child(content))
        .when(selected, |item| {
            item.child(icon(Icon::Check, 14.0, theme::palette().foreground))
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
        .border_color(rgba(theme::hairline_strong()))
        .bg(rgb(theme::palette().dialog))
        .shadow(vec![BoxShadow {
            color: hsla(0.0, 0.0, 0.0, 0.4),
            offset: point(px(0.0), px(8.0)),
            blur_radius: px(24.0),
            spread_radius: px(0.0),
            inset: false,
        }])
        .children(items)
}

/// Draws `content` above the rest of the window, where it would sit in the layout, fading, zooming and sliding it down
/// into place as the desktop's popovers do, or out in the phase popups are built in (`motion::with_phase`). A press on
/// it is its own, as on a `behaviors::bind_layer` layer: nothing under it, like a dialog's backdrop, hears it.
pub fn popover(content: impl Styled + InteractiveElement + IntoElement + 'static) -> AnyElement {
    let content = motion::popup("popover", motion::From::Above, motion::POPUP_IN, content.occlude());
    // Kept 8pt inside the window, as `behaviors::position` keeps a popup, instead of touching its edge.
    deferred(anchored().snap_to_window_with_margin(px(8.0)).child(cover(content))).into_any_element()
}
