//! A page's `TabStrip`: one line of rounded tabs as Wings' own title bar draws them, the shown one filled and a close
//! button on it and on the one under the pointer. Tabs shrink evenly from `MAX_TAB` to `MIN_TAB` as the strip fills, then
//! the strip scrolls sideways with the "+" staying at its end. A group is a small label in its tone, and its tabs carry a
//! line of the same tone along their foot.

use std::{collections::HashMap, rc::Rc};

use gpui::{
    AnyElement, Context, ElementId, FontWeight, KeyDownEvent, MouseButton, MouseDownEvent, Role, ScrollEvent, ScrollHandle,
    SharedString, div, point, prelude::*, px, rgb, rgba,
};

use super::{PageView, tree};
use crate::{
    icons::Icon,
    theme,
    ui::{self, ButtonSize, behaviors::{Reveal, RevealExt}},
};

/// A tab at most this wide, which a strip with room gives each.
const MAX_TAB: f32 = 200.0;
/// A tab never narrower than this: past it the strip scrolls.
const MIN_TAB: f32 = 72.0;
/// A group's label never narrower than this.
const MIN_LABEL: f32 = 48.0;
/// The strip's height, and a tab's inside it.
const STRIP_HEIGHT: f32 = 32.0;
const TAB_HEIGHT: f32 = 28.0;
/// The room over the strip, off the top of the page.
const STRIP_TOP: f32 = 6.0;
/// The close button's `top-1 right-1`, measured inside the tab's 1px border, as the title bar's.
const CLOSE_INSET: f32 = 3.0;
/// The room a tab keeps at its end for the close button, so a title never runs under it.
const CLOSE_ROOM: f32 = 24.0;

/// What a strip remembers between renders: where it is scrolled, and the tab it last scrolled into view.
#[derive(Default)]
pub(super) struct TabStripState {
    scroll: ScrollHandle,
    revealed: Option<String>,
    /// The strip was asked for once more before it had a width to scroll in.
    waited: bool,
}

/// A group's colour, a fixed set of the theme's own.
fn tone_color(tone: tree::GroupTone) -> u32 {
    let palette = theme::palette();
    match tone {
        tree::GroupTone::Neutral => palette.muted_foreground,
        tree::GroupTone::Info => palette.info,
        tree::GroupTone::Success => palette.done,
        tree::GroupTone::Warning => palette.working,
        tree::GroupTone::Danger => palette.danger,
        tree::GroupTone::Accent => theme::claude(),
    }
}

/// A tab's tooltip: its full title, and its address under it.
fn tooltip_of(tab: &tree::Tab) -> SharedString {
    match (tab.title.is_empty(), tab.url.is_empty()) {
        (_, true) => tab.title.clone().into(),
        (true, false) => tab.url.clone().into(),
        (false, false) => format!("{}\n{}", tab.title, tab.url).into(),
    }
}

impl PageView {
    pub(super) fn tab_strip(&self, strip: &tree::TabStrip, cx: &mut Context<Self>) -> AnyElement {
        let shown: Vec<&tree::TabItem> = strip.shown().collect();
        let tones: HashMap<&str, tree::GroupTone> = strip
            .items
            .iter()
            .filter_map(|item| match item {
                tree::TabItem::Group(group) => Some((group.id.as_str(), group.tone)),
                tree::TabItem::Tab(_) => None,
            })
            .collect();
        // The tabs in the order the keys move along them, with the place each has among what the strip draws.
        let order: Rc<[(String, usize)]> = shown
            .iter()
            .enumerate()
            .filter_map(|(at, item)| match item {
                tree::TabItem::Tab(tab) => Some((tab.id.clone(), at)),
                tree::TabItem::Group(_) => None,
            })
            .collect();
        let focus: Rc<HashMap<String, gpui::FocusHandle>> = Rc::new(
            order
                .iter()
                .map(|(id, _)| {
                    let handle = self
                        .chip_focus
                        .borrow_mut()
                        .entry((strip.id, id.clone()))
                        .or_insert_with(|| cx.focus_handle().tab_stop(true))
                        .clone();
                    (id.clone(), handle)
                })
                .collect(),
        );
        let scroll = {
            let mut strips = self.tab_strips.borrow_mut();
            let state = strips.entry(strip.id).or_default();
            // The tab that turned active comes into view on the next draw.
            let active = shown.iter().enumerate().find_map(|(at, item)| match item {
                tree::TabItem::Tab(tab) if tab.active => Some((at, tab.id.clone())),
                _ => None,
            });
            if let Some((at, id)) = active
                && state.revealed.as_ref() != Some(&id)
            {
                // The first draw lays the strip out, and a scroll asked for before it has a width goes nowhere: the
                // page is drawn once more, which scrolls it.
                if state.scroll.bounds().size.width > px(0.0) || state.waited {
                    state.scroll.reveal(at, Reveal::Nearest);
                    state.revealed = Some(id);
                } else {
                    state.waited = true;
                    let this = cx.entity();
                    cx.defer(move |cx| this.update(cx, |_, cx| cx.notify()));
                }
            }
            state.scroll.clone()
        };
        let items = shown
            .iter()
            .enumerate()
            .map(|(at, item)| match item {
                tree::TabItem::Group(group) => self.tab_group(strip, group, at, cx),
                tree::TabItem::Tab(tab) => {
                    let tone = tab.group.as_deref().and_then(|group| tones.get(group)).copied();
                    self.strip_tab(strip, tab, at, tone, &order, &focus, &scroll, cx)
                }
            })
            .collect::<Vec<_>>();
        let on_new = strip.on_new.clone();
        div()
            .id(ElementId::NamedInteger("page-tab-strip".into(), strip.id))
            .role(Role::TabList)
            .aria_label(strip.label.clone())
            .w_full()
            // In from the page's sides as the toolbar's content is, and off the top of the frame.
            .h(px(STRIP_HEIGHT + STRIP_TOP))
            .pt(px(STRIP_TOP))
            .px(px(14.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.0))
            .child(
                div()
                    .id(ElementId::NamedInteger("page-tab-strip-scroll".into(), strip.id))
                    .min_w_0()
                    .h_full()
                    .flex()
                    .items_center()
                    .gap(px(2.0))
                    .overflow_x_scroll()
                    .track_scroll(&scroll)
                    // A wheel turns the strip sideways, as a trackpad's swipe already does.
                    .on_scroll({
                        let scroll = scroll.clone();
                        cx.listener(move |_, event: &ScrollEvent, _, cx| {
                            let delta = event.delta.pixel_delta(px(STRIP_HEIGHT));
                            let room = scroll.max_offset().x;
                            if room <= px(0.0) || delta.y.abs() <= delta.x.abs() {
                                return;
                            }
                            let offset = scroll.offset();
                            scroll.set_offset(point((offset.x + delta.y).clamp(-room, px(0.0)), offset.y));
                            cx.stop_propagation();
                            cx.notify();
                        })
                    })
                    .children(items)
                    .children(ui::cutoff::clips_on_purpose()),
            )
            .when_some(on_new, |row, handler| {
                row.child(
                    ui::icon_button(ElementId::NamedInteger("page-tab-new".into(), strip.id), Icon::Plus, "New tab", ButtonSize::Sm)
                        .flex_none()
                        .on_click(cx.listener(move |view, _, _, _| view.act(&handler, Some(&serde_json::json!({})))))
                        .into_any_element(),
                )
            })
            .into_any_element()
    }

    /// A group's label: its name in its tone, "name · N" when its tabs are folded away. A click is the page's to answer.
    fn tab_group(&self, strip: &tree::TabStrip, group: &tree::TabGroup, at: usize, cx: &mut Context<Self>) -> AnyElement {
        let color = tone_color(group.tone);
        let name = if group.name.is_empty() { "Group".to_owned() } else { group.name.clone() };
        let label = if group.collapsed { format!("{name} · {}", group.count) } else { name.clone() };
        let state = if group.collapsed { "collapsed" } else { "expanded" };
        let handler = strip.on_group.clone();
        let (click, keys, id) = (handler.clone(), handler, group.id.clone());
        let key_id = id.clone();
        div()
            .id(ElementId::NamedInteger(format!("page-tab-group-{}", strip.id).into(), at as u64))
            .role(Role::Button)
            .aria_label(format!("{name}, group of {} tabs, {state}", group.count))
            .aria_expanded(!group.collapsed)
            .tooltip(ui::tooltip(name, None))
            // Shrinks with the tabs when the strip fills, down to a few letters of its name: kept whole, a long name left
            // the tabs around it three letters each.
            .flex_shrink(1.0)
            .min_w(px(MIN_LABEL))
            .max_w(px(160.0))
            .h(px(20.0))
            .px(px(8.0))
            .when(at > 0, |label| label.ml(px(4.0)))
            .flex()
            .items_center()
            .rounded(px(6.0))
            .border_1()
            .border_color(rgba(color << 8 | 0x00))
            .bg(rgba(color << 8 | 0x2a))
            .text_size(px(11.0))
            .font_weight(FontWeight::MEDIUM)
            .text_color(rgb(color))
            .when(click.is_some(), |label| {
                label
                    .cursor_pointer()
                    .hover(|style| style.bg(rgba(color << 8 | 0x40)))
                    .focusable()
                    .tab_stop(true)
                    .focus_visible(|style| style.border_color(rgb(theme::foreground())))
                    .on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            if let Some(handler) = &keys {
                                view.act(handler, Some(&serde_json::json!({ "id": key_id })));
                            }
                            window.prevent_default();
                            cx.stop_propagation();
                        }
                    }))
                    .on_click(cx.listener(move |view, _, _, cx| {
                        cx.stop_propagation();
                        if let Some(handler) = &click {
                            view.act(handler, Some(&serde_json::json!({ "id": id })));
                        }
                    }))
            })
            .child(div().min_w_0().truncate().child(label))
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn strip_tab(
        &self,
        strip: &tree::TabStrip,
        tab: &tree::Tab,
        at: usize,
        group_tone: Option<tree::GroupTone>,
        order: &Rc<[(String, usize)]>,
        focus: &Rc<HashMap<String, gpui::FocusHandle>>,
        scroll: &ScrollHandle,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = tab.active;
        let id = ElementId::NamedInteger(format!("page-tab-{}", strip.id).into(), at as u64);
        let hover_group = SharedString::from(format!("page-tab-{}-{at}", strip.id));
        let title = if tab.title.is_empty() { tab.url.clone() } else { tab.title.clone() };
        let (select, key_select, key_close) = (strip.on_select.clone(), strip.on_select.clone(), strip.on_close.clone());
        let (select_id, middle_id, key_id, button_id) = (tab.id.clone(), tab.id.clone(), tab.id.clone(), tab.id.clone());
        let (order, focus_all, own_focus, scroll) = (order.clone(), focus.clone(), focus.get(&tab.id).cloned(), scroll.clone());
        let click_focus = own_focus.clone();
        let middle_close = strip.on_close.clone();
        let close_button = strip.on_close.as_ref().map(|handler| {
            let handler = handler.clone();
            ui::icon_button(
                ElementId::NamedInteger(format!("page-tab-close-{}", strip.id).into(), at as u64),
                Icon::X,
                "Close tab",
                ButtonSize::Tiny,
            )
            .absolute()
            .top(px(CLOSE_INSET))
            .right(px(CLOSE_INSET))
            .opacity(if selected { 1.0 } else { 0.0 })
            .group_hover(hover_group.clone(), |style| style.opacity(1.0))
            .on_click(cx.listener(move |view, _, _, cx| {
                cx.stop_propagation();
                view.act(&handler, Some(&serde_json::json!({ "id": button_id })));
            }))
        });
        div()
            .id(id)
            .group(hover_group)
            .role(Role::Tab)
            .aria_label(title.clone())
            .aria_selected(selected)
            .tooltip(ui::tooltip(tooltip_of(tab), None))
            .when_some(own_focus, |tab, focus| tab.track_focus(&focus))
            .w(px(MAX_TAB))
            .min_w(px(MIN_TAB))
            .h(px(TAB_HEIGHT))
            .relative()
            .flex()
            .items_center()
            .gap(px(6.0))
            .pl(px(10.0))
            .pr(px(CLOSE_ROOM))
            .rounded(px(theme::RADIUS))
            .border_1()
            .border_color(rgba(0))
            .cursor_pointer()
            .text_size(px(12.0))
            .font_weight(FontWeight::MEDIUM)
            .text_color(rgb(if selected { theme::foreground() } else { theme::muted_foreground() }))
            .when(selected, |tab| tab.bg(rgba(theme::hover_strong())).border_color(rgba(theme::hairline_strong())))
            .when(!selected, |tab| tab.hover(|style| style.bg(rgba(theme::hover())).text_color(rgb(theme::foreground()))))
            .focus_visible(|style| style.border_color(rgb(theme::foreground())))
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    if let Some(handler) = &middle_close {
                        view.act(handler, Some(&serde_json::json!({ "id": middle_id })));
                    }
                }),
            )
            .on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
                let here = order.iter().position(|(id, _)| *id == key_id).unwrap_or_default();
                let key = event.keystroke.key.as_str();
                let target = match key {
                    "left" => order.get(here.checked_sub(1).unwrap_or(order.len().saturating_sub(1))),
                    "right" => order.get((here + 1) % order.len().max(1)),
                    "home" => order.first(),
                    "end" => order.last(),
                    "enter" | "space" => {
                        if let Some(handler) = &key_select {
                            view.act(handler, Some(&serde_json::json!({ "id": key_id })));
                        }
                        window.prevent_default();
                        cx.stop_propagation();
                        return;
                    }
                    "delete" | "backspace" => {
                        if let Some(handler) = &key_close {
                            view.act(handler, Some(&serde_json::json!({ "id": key_id })));
                        }
                        window.prevent_default();
                        cx.stop_propagation();
                        return;
                    }
                    _ => return,
                };
                if let Some((id, place)) = target {
                    if let Some(handle) = focus_all.get(id) {
                        handle.focus(window, cx);
                    }
                    scroll.reveal(*place, Reveal::Nearest);
                    cx.notify();
                }
                window.prevent_default();
                cx.stop_propagation();
            }))
            .on_click(cx.listener(move |view, _, window, cx| {
                if let Some(focus) = &click_focus {
                    focus.focus(window, cx);
                }
                if let Some(handler) = &select {
                    view.act(handler, Some(&serde_json::json!({ "id": select_id })));
                }
            }))
            .when(tab.loading, |row| {
                let name = format!("tab-spinner-{}-{}", strip.id, tab.id);
                row.child(div().size(px(10.0)).flex_none().map(|ring| {
                    if self.spin_in_layer.get() {
                        ring.child(ui::view_spinner_slot(name))
                    } else {
                        ring.child(ui::working_spinner(0.0))
                    }
                }))
            })
            .child(div().min_w_0().truncate().child(title))
            .when_some(group_tone, |tab, tone| {
                tab.child(div().absolute().left(px(8.0)).right(px(8.0)).bottom(px(1.0)).h(px(2.0)).rounded_full().bg(rgb(tone_color(tone))))
            })
            .children(close_button)
            .into_any_element()
    }

    /// Where the strip `id` laid out, as last drawn: its scrolling box, each thing it draws in it, and how far it is
    /// scrolled sideways (0 or less).
    #[cfg(all(test, feature = "test-support"))]
    pub(in super::super) fn tab_strip_bounds(&self, id: u64) -> Option<(gpui::Bounds<gpui::Pixels>, Vec<gpui::Bounds<gpui::Pixels>>, f32)> {
        let strips = self.tab_strips.borrow();
        let scroll = &strips.get(&id)?.scroll;
        let items = (0..).map_while(|at| scroll.bounds_for_item(at)).collect();
        Some((scroll.bounds(), items, scroll.offset().x.as_f32()))
    }
}
