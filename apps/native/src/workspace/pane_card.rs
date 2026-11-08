//! A tab's panes as cards: the split layout with its dividers, each pane's header and buttons.

use std::{cell::Cell, rc::Rc};

use gpui::{
    AnyElement, BorderStyle, Bounds, BoxShadow, Context, Corners, Edges, Hsla, MouseButton,
    MouseDownEvent, Orientation, Pixels, Role, SharedString, Window, canvas, div, hsla, point,
    prelude::*, px, quad, relative, rgb, rgba,
};

use crate::{
    icons::{Icon, icon},
    layout::{DIVIDER, Dir, Layout},
    theme,
    ui::{self, ButtonSize},
};

use super::{HAIRLINE, Workspace, context_menu::MenuTarget, shortcut_label};

/// A divider's line while the pointer is on it (`bg-white/25`).
const DIVIDER_HOVER: u32 = 0xffffff40;
/// A pane card's corners.
const CARD_RADIUS: f32 = 14.0;

/// The layer of the window that draws the panes' terminals, over their cards (`ui::slot_layer`).
pub(super) const TERMINALS: &str = "terminals";

/// The name of a pane's slot in the terminal layer.
pub(super) fn terminal_slot(pane: usize) -> SharedString {
    format!("pane-{pane}").into()
}

impl Workspace {
    pub(super) fn layout_element(
        &self,
        tab: usize,
        node: &Layout,
        path: &mut Vec<bool>,
        focused: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Layout::Split { dir, ratio, a, b } = node else {
            let Layout::Pane(pane) = node else {
                unreachable!()
            };
            return self.pane_card(*pane, *pane == focused, false, cx);
        };
        let row = *dir == Dir::Row;
        path.push(false);
        let first = self.layout_element(tab, a, path, focused, cx);
        path.pop();
        path.push(true);
        let second = self.layout_element(tab, b, path, focused, cx);
        path.pop();
        let at = path.clone();
        let group = SharedString::from(format!("divider-{tab}-{at:?}"));
        let divider = div()
            .id(group.clone())
            .group(group.clone())
            .role(Role::Splitter)
            .aria_label("Resize panes")
            .aria_orientation(if row {
                Orientation::Vertical
            } else {
                Orientation::Horizontal
            })
            .relative()
            .flex_none()
            .map(|d| {
                if row {
                    d.w(px(DIVIDER)).h_full().cursor_col_resize()
                } else {
                    d.h(px(DIVIDER)).w_full().cursor_row_resize()
                }
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| this.start_drag(tab, at.clone(), cx)),
            )
            .child(
                div()
                    .absolute()
                    .rounded_full()
                    .group_hover(group, |style| style.bg(rgba(DIVIDER_HOVER)))
                    .map(|line| {
                        if row {
                            line.top(px(24.0)).bottom(px(24.0)).left(px(3.0)).w(px(2.0))
                        } else {
                            line.left(px(24.0)).right(px(24.0)).top(px(3.0)).h(px(2.0))
                        }
                    }),
            );
        div()
            .flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .map(|d| if row { d.flex_row() } else { d.flex_col() })
            .child(
                div()
                    .flex()
                    .flex_none()
                    .flex_basis(relative(*ratio))
                    .min_w_0()
                    .min_h_0()
                    .child(first),
            )
            .child(divider)
            .child(div().flex().flex_1().min_w_0().min_h_0().child(second))
            .into_any_element()
    }

    /// Like the desktop's pane card: a header with the title and the split and close buttons, which
    /// show on the focused pane and on hover.
    pub(super) fn pane_card(
        &self,
        pane: usize,
        focused: bool,
        zoomed: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let title = self.pane_title(pane, cx);
        let detail = self.pane_detail(pane);
        let state = self.agents.get(&pane).map(|agent| agent.state);
        let renaming = self
            .panes
            .get(&pane)
            .and_then(|pane| pane.renaming.as_ref())
            .map(|(field, _)| field.clone());
        let group = SharedString::from(format!("pane-{pane}"));
        let border: Hsla = rgba(if focused {
            theme::HAIRLINE_STRONG
        } else {
            HAIRLINE
        })
        .into();
        // Where the card is, for its border over the terminal.
        let card = Rc::new(Cell::new(None));
        let body = match self.panes.get(&pane).map(|p| (&p.view, &p.focus)) {
            // The terminal draws in the window's terminal layer at this slot, so its output draws neither the main
            // column again nor the other terminals in it (`Workspace::render`). As a child of the card would, it has
            // the card's border over it, where its corners curve over the terminal.
            Some((Ok(_), _)) => {
                let card = card.clone();
                ui::slot(
                    TERMINALS,
                    terminal_slot(pane),
                    Some(Rc::new(move |window: &mut Window| {
                        if let Some(card) = card.get() {
                            paint_card_border(card, border, window);
                        }
                    })),
                )
                .into_any_element()
            }
            Some((Err(error), focus)) => div()
                .track_focus(focus)
                .p(px(24.0))
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child("Couldn't start the terminal"),
                )
                .child(div().text_color(rgb(theme::DANGER)).child(error.clone()))
                .into_any_element(),
            None => div().into_any_element(),
        };
        let badges = self.badge_pills(pane, cx);
        let strip = self.status_strip(pane, cx);
        let header = div()
            .id(SharedString::from(format!("pane-header-{pane}")))
            .h(px(30.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .pl(px(14.0))
            .pr(px(4.0))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    if event.click_count >= 2 {
                        this.start_rename(pane, window, cx);
                    } else {
                        this.focus_pane(pane, window, cx);
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.open_menu(MenuTarget::Pane(pane), event.position, cx)
                }),
            )
            .child(match state {
                Some(state) => crate::ui::status_dot_slot(state, format!("pane-{pane}")),
                None => icon(Icon::Terminal, 14.0, theme::MUTED_FOREGROUND).into_any_element(),
            })
            .map(|header| match renaming {
                // Bordered like the desktop's InlineName, with its text where the title was.
                Some(field) => header.child(
                    div()
                        .ml(px(-9.0))
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(rgba(theme::HAIRLINE_STRONG))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(field),
                ),
                // The desktop's `max-w-[50%] truncate`: in a narrow pane the title gives way, not the buttons.
                None => header.child(
                    div()
                        .min_w_0()
                        .max_w(relative(0.5))
                        .truncate()
                        .text_size(px(13.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(if focused {
                            theme::FOREGROUND
                        } else {
                            theme::MUTED_FOREGROUND
                        }))
                        .child(title.clone()),
                ),
            })
            .when_some(detail, |header, detail| {
                header.child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.0))
                        .text_color(rgb(theme::MUTED_FOREGROUND))
                        .child(detail),
                )
            })
            .child(div().flex_1())
            .children(badges)
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(2.0))
                    .opacity(if focused { 0.7 } else { 0.0 })
                    .group_hover(group.clone(), |style| style.opacity(1.0))
                    .child(pane_button(
                        pane,
                        "Split right",
                        Some(shortcut_label("D", false)),
                        Icon::Columns,
                        cx,
                        |this, pane, window, cx| {
                            this.focus_pane(pane, window, cx);
                            this.split(Dir::Row, window, cx);
                        },
                    ))
                    .child(pane_button(
                        pane,
                        "Split down",
                        Some(shortcut_label("D", true)),
                        Icon::Rows,
                        cx,
                        |this, pane, window, cx| {
                            this.focus_pane(pane, window, cx);
                            this.split(Dir::Column, window, cx);
                        },
                    ))
                    .child(pane_button(
                        pane,
                        if zoomed { "Restore" } else { "Zoom" },
                        None,
                        if zoomed {
                            Icon::Minimize
                        } else {
                            Icon::Maximize
                        },
                        cx,
                        |this, pane, window, cx| this.toggle_zoom(pane, window, cx),
                    ))
                    .child(pane_button(
                        pane,
                        "Close pane",
                        Some(shortcut_label("W", false)),
                        Icon::X,
                        cx,
                        |this, pane, window, cx| this.close_pane(pane, window, cx),
                    )),
            );
        let place = canvas(
            // Inside the border, which is a pixel wide.
            move |inside, _, _| card.set(Some(inside.dilate(px(1.0)))),
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        div()
            .id(SharedString::from(format!("pane-{pane}")))
            .group(group)
            .role(Role::Group)
            .aria_label(title)
            .flex()
            .flex_col()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .rounded(px(CARD_RADIUS))
            .border_1()
            .border_color(border)
            .bg(rgb(theme::SURFACE))
            .when(focused, |card| {
                card.shadow(vec![
                    BoxShadow {
                        color: hsla(0.0, 0.0, 0.0, 0.28),
                        offset: point(px(0.0), px(10.0)),
                        blur_radius: px(30.0),
                        spread_radius: px(0.0),
                        inset: false,
                    },
                    BoxShadow {
                        color: hsla(0.0, 0.0, 1.0, 0.05),
                        offset: point(px(0.0), px(1.0)),
                        blur_radius: px(0.0),
                        spread_radius: px(0.0),
                        inset: true,
                    },
                ])
            })
            .child(place)
            .child(header)
            .child(div().flex().flex_1().min_h_0().child(body))
            .children(strip)
            .into_any_element()
    }
}

/// A card's border as the card draws it (gpui's `Style::paint`), for drawing it again over what a layer draws in it.
fn paint_card_border(card: Bounds<Pixels>, color: Hsla, window: &mut Window) {
    let mut background = color;
    background.a = 0.0;
    window.paint_quad(quad(
        card,
        Corners::all(px(CARD_RADIUS)).clamp_radii_for_quad_size(card.size),
        background,
        Edges::all(px(1.0)),
        color,
        BorderStyle::default(),
    ));
}

/// A pane header's button: the desktop's `IconButton`, 28px with a 16px icon, aimed at this pane.
fn pane_button(
    pane: usize,
    label: &'static str,
    shortcut: Option<String>,
    glyph: Icon,
    cx: &mut Context<Workspace>,
    action: fn(&mut Workspace, usize, &mut Window, &mut Context<Workspace>),
) -> impl IntoElement + use<> {
    ui::shortcut_icon_button(
        SharedString::from(format!("{label}-{pane}")),
        glyph,
        label,
        shortcut.map(Into::into),
        ButtonSize::Sm,
    )
    .on_click(cx.listener(move |this, _, window, cx| action(this, pane, window, cx)))
}
