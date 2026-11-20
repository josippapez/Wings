//! The right sidebar's pages, stacked one above another like a tab's split panes: which are open, how tall each is, the
//! dividers between them, and how a click, a menu or a plugin opens and closes one.

use gpui::{
    AnyElement, Bounds, ClickEvent, Context, ElementId, FontWeight, MouseButton, MouseDownEvent,
    Orientation, Pixels, Role, SharedString, Window, canvas, div, prelude::*, px, rgb, rgba,
};

use crate::{
    icons::Icon,
    layout::DIVIDER,
    page::PageView,
    saved::SavedSplit,
    theme,
    ui::{self, ButtonSize},
};

use super::{
    HAIRLINE, Workspace,
    context_menu::MenuTarget,
    pane_card::DIVIDER_HOVER,
    plugin_ui::sidebar_glyph,
};

/// A sidebar page: its plugin's id and its own.
pub(super) type Key = (String, String);

/// The most pages the panel stacks. A new one past it takes the last one's place.
pub(super) const MAX_PAGES: usize = 4;
/// The least a split can be dragged down to, its header and card.
const MIN_SPLIT_HEIGHT: f32 = 96.0;
/// A split's header when there are two or more: slimmer than the lone page's.
const SPLIT_HEADER: f32 = 28.0;
/// As tall as the project bar beside it, so the sidebar's card starts level with the tab's: at 32 it started 4 points
/// higher.
const LONE_HEADER: f32 = super::project::PROJECT_BAR;

/// The open pages, top to bottom, and each one's share of the height, which together make 1.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Stack {
    pages: Vec<Key>,
    sizes: Vec<f32>,
    /// The page last opened or asked for, whose header reads brighter.
    focus: usize,
}

impl Stack {
    pub(super) fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    pub(super) fn len(&self) -> usize {
        self.pages.len()
    }

    pub(super) fn keys(&self) -> &[Key] {
        &self.pages
    }

    pub(super) fn sizes(&self) -> &[f32] {
        &self.sizes
    }

    pub(super) fn contains(&self, key: &Key) -> bool {
        self.pages.contains(key)
    }

    /// The page, when it is the only one open.
    pub(super) fn only(&self) -> Option<&Key> {
        match self.pages.as_slice() {
            [only] => Some(only),
            _ => None,
        }
    }

    pub(super) fn focused(&self) -> Option<&Key> {
        self.pages.get(self.focus)
    }

    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    /// Only this page.
    pub(super) fn show_alone(&mut self, key: Key) {
        self.pages = vec![key];
        self.sizes = vec![1.0];
        self.focus = 0;
    }

    /// This page below the others, taking an equal share of the height. One already open stays where it is and takes the
    /// focus. At the cap it takes the last page's place.
    pub(super) fn add(&mut self, key: Key) {
        if let Some(at) = self.pages.iter().position(|open| *open == key) {
            self.focus = at;
            return;
        }
        let count = self.pages.len();
        if count >= MAX_PAGES {
            self.pages[count - 1] = key;
        } else if count == 0 {
            return self.show_alone(key);
        } else {
            let keep = count as f32 / (count + 1) as f32;
            self.sizes.iter_mut().for_each(|size| *size *= keep);
            self.sizes.push(1.0 / (count + 1) as f32);
            self.pages.push(key);
        }
        self.focus = self.pages.len() - 1;
    }

    /// Closes the page, its height going to the rest in proportion.
    pub(super) fn remove(&mut self, key: &Key) -> bool {
        self.remove_where(|open| open == key)
    }

    pub(super) fn remove_plugin(&mut self, plugin: &str) -> bool {
        self.remove_where(|(open, _)| open == plugin)
    }

    pub(super) fn remove_where(&mut self, gone: impl Fn(&Key) -> bool) -> bool {
        let before = self.pages.len();
        let mut at = 0;
        self.pages.retain(|key| {
            let keep = !gone(key);
            if !keep {
                self.sizes.remove(at);
            } else {
                at += 1;
            }
            keep
        });
        if self.pages.len() == before {
            return false;
        }
        self.normalize();
        self.focus = self.focus.min(self.pages.len().saturating_sub(1));
        true
    }

    /// Moves the line between the page at `divider` and the next to `at`, a share of the whole height from the top, with
    /// each of the two keeping at least `min`.
    pub(super) fn set_boundary(&mut self, divider: usize, at: f32, min: f32) {
        if divider + 1 >= self.pages.len() {
            return;
        }
        let start: f32 = self.sizes[..divider].iter().sum();
        let end = start + self.sizes[divider] + self.sizes[divider + 1];
        let (low, high) = (start + min, end - min);
        if low > high {
            return;
        }
        let at = at.clamp(low, high);
        self.sizes[divider] = at - start;
        self.sizes[divider + 1] = end - at;
    }

    /// Shares that add up to 1, or equal ones when they can't.
    fn normalize(&mut self) {
        let total: f32 = self.sizes.iter().sum();
        let ok = self.sizes.len() == self.pages.len()
            && self.sizes.iter().all(|size| size.is_finite() && *size > 0.0)
            && total > 0.0;
        if ok {
            self.sizes.iter_mut().for_each(|size| *size /= total);
        } else {
            let count = self.pages.len().max(1) as f32;
            self.sizes = vec![1.0 / count; self.pages.len()];
        }
    }

    pub(super) fn from_saved(saved: &[SavedSplit]) -> Self {
        let mut stack = Self::default();
        for split in saved {
            let key = (split.plugin.clone(), split.page.clone());
            if stack.pages.len() < MAX_PAGES && !stack.contains(&key) {
                stack.pages.push(key);
                stack.sizes.push(split.size.unwrap_or(0.0));
            }
        }
        stack.normalize();
        stack
    }

    pub(super) fn to_saved(&self) -> Vec<SavedSplit> {
        self.pages
            .iter()
            .zip(&self.sizes)
            .map(|((plugin, page), size)| SavedSplit {
                plugin: plugin.clone(),
                page: page.clone(),
                size: Some(*size),
            })
            .collect()
    }
}

/// What dragging a divider needs: which one, and where the stack was last drawn.
#[derive(Default)]
pub(super) struct SplitDrag {
    pub(super) dragging: Option<usize>,
    pub(super) area: Option<Bounds<Pixels>>,
}

impl Workspace {
    /// Starts the page's own copy the first time, so it keeps running from then on. False when no such sidebar is
    /// installed.
    pub(super) fn ensure_sidebar_page(&mut self, key: &Key, cx: &mut Context<Self>) -> bool {
        if self.sidebar_pages.contains_key(key) {
            return true;
        }
        let Some(installed) = self.plugins.iter().find(|installed| installed.id == key.0) else {
            return false;
        };
        let Some(page) = installed.sidebars.iter().find(|page| page.id == key.1) else {
            return false;
        };
        let (host, source) = (installed.host.clone(), page.source.clone());
        let page = cx.new(|cx| PageView::in_plugin(&host, &source, cx).in_card());
        self.sidebar_pages.insert(key.clone(), page);
        self.sync_page_terminals(cx);
        true
    }

    /// Shows the sidebar alone in the panel, starting its page the first time. The page keeps running while the panel is
    /// closed or shows another, as the desktop's does.
    pub(super) fn show_sidebar(&mut self, plugin: &str, sidebar: &str, cx: &mut Context<Self>) {
        let key = (plugin.to_owned(), sidebar.to_owned());
        if !self.ensure_sidebar_page(&key, cx) {
            return;
        }
        self.right.show_alone(key);
        self.right_changed(cx);
    }

    /// Adds the sidebar as a split below the open ones. One already open takes the focus.
    pub(super) fn split_sidebar(&mut self, plugin: &str, sidebar: &str, cx: &mut Context<Self>) {
        let key = (plugin.to_owned(), sidebar.to_owned());
        if !self.ensure_sidebar_page(&key, cx) {
            return;
        }
        self.right.add(key);
        self.right_changed(cx);
    }

    /// What `openView(id, { as: "sidebar" })` does: a sidebar already open stays where it is, else it joins the splits
    /// when there are two or more, and is shown alone otherwise. False when the plugin has no such sidebar.
    pub(super) fn open_sidebar_from_plugin(
        &mut self,
        plugin: &str,
        sidebar: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let key = (plugin.to_owned(), sidebar.to_owned());
        if self.right.contains(&key) {
            return true;
        }
        if self.right.len() >= 2 {
            self.split_sidebar(plugin, sidebar, cx);
        } else {
            self.show_sidebar(plugin, sidebar, cx);
        }
        self.right.contains(&key)
    }

    /// Closes one split and leaves the others.
    pub(super) fn close_split(&mut self, key: &Key, cx: &mut Context<Self>) {
        if self.right.remove(key) {
            self.right_changed(cx);
        }
    }

    pub(super) fn close_right(&mut self, cx: &mut Context<Self>) {
        if !self.right.is_empty() {
            self.right.clear();
            self.right_changed(cx);
        }
    }

    /// Makes the shown stack the project's: its pages started, those no plugin has any more dropped.
    pub(super) fn apply_right(&mut self, mut stack: Stack, cx: &mut Context<Self>) {
        let missing: Vec<Key> = stack
            .keys()
            .iter()
            .filter(|key| !self.ensure_sidebar_page(key, cx))
            .cloned()
            .collect();
        stack.remove_where(|key| missing.contains(key));
        self.right = stack;
        self.sync_plugin_visibility(cx);
        cx.notify();
    }

    /// After the pages open changed: visible ones run drawn, the project keeps the stack, the window redraws.
    fn right_changed(&mut self, cx: &mut Context<Self>) {
        self.sync_plugin_visibility(cx);
        self.save(cx);
        cx.notify();
    }

    /// The title bar button of a sidebar. A click shows it alone, or closes the panel when it is the only one showing; a
    /// click with the shortcut key down adds it as a split.
    pub(super) fn toggle_sidebar(
        &mut self,
        plugin: &str,
        sidebar: &str,
        split: bool,
        cx: &mut Context<Self>,
    ) {
        let key = (plugin.to_owned(), sidebar.to_owned());
        if split {
            self.split_sidebar(plugin, sidebar, cx);
        } else if self.right.only() == Some(&key) {
            self.close_right(cx);
        } else {
            self.show_sidebar(plugin, sidebar, cx);
        }
        if self.right.contains(&key) {
            // Not over its own open sidebar: closed rather than hidden, so it doesn't come back when the sidebar closes.
            if self.plugin_ui.popover.open.as_ref() == Some(&key) {
                self.hide_popover(cx);
            }
            self.plugin_ui.popover.hovered = None;
        }
    }

    /// The right-click menu of a sidebar's title bar button or header.
    pub(super) fn open_sidebar_menu(
        &mut self,
        key: Key,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        self.open_menu(MenuTarget::Sidebar(key), event.position, cx);
    }

    /// The pointer went down on the divider under page `index`.
    fn start_split_drag(&mut self, index: usize, cx: &mut Context<Self>) {
        self.plugin_ui.right_split.dragging = Some(index);
        cx.notify();
    }

    /// Follows the pointer. Only the sidebar lays out again: the panes keep their size. Nothing is saved until the
    /// pointer is released.
    fn split_drag_to(&mut self, y: Pixels, cx: &mut Context<Self>) {
        let Some(index) = self.plugin_ui.right_split.dragging else {
            return;
        };
        let Some(area) = self.plugin_ui.right_split.area else {
            return;
        };
        let count = self.right.len();
        // The pages share what the dividers leave.
        let space = area.size.height.as_f32() - (count.saturating_sub(1)) as f32 * DIVIDER;
        if space <= 0.0 {
            return;
        }
        let above = (y - area.top()).as_f32() - index as f32 * DIVIDER - DIVIDER / 2.0;
        let min = (MIN_SPLIT_HEIGHT / space).min(1.0 / count as f32 / 2.0);
        self.right.set_boundary(index, above / space, min);
        cx.notify();
    }

    fn end_split_drag(&mut self, cx: &mut Context<Self>) {
        if self.plugin_ui.right_split.dragging.take().is_some() {
            self.save(cx);
            cx.notify();
        }
    }

    /// The pages of `shown` in the panel, top to bottom: with one, its header and card as ever; with more, each under a
    /// slim header, and a divider to drag between them.
    pub(super) fn right_pages(&self, shown: &Stack, window: gpui::WindowId, cx: &mut Context<Self>) -> Option<AnyElement> {
        let many = shown.len() > 1;
        let mut splits: Vec<(Key, AnyElement, f32)> = Vec::new();
        for (index, key) in shown.keys().iter().enumerate() {
            let Some(page) = self.sidebar_pages.get(key) else {
                continue;
            };
            let Some(installed) = self.plugins.iter().find(|installed| installed.id == key.0)
            else {
                continue;
            };
            let Some(title) = installed
                .sidebars
                .iter()
                .find(|page| page.id == key.1)
                .map(|page| page.title.clone())
            else {
                continue;
            };
            // On screen, so its spinners turn in the window's layer and a step of them doesn't draw the page again.
            crate::ui::place_view(page.entity_id(), window);
            let glyph = sidebar_glyph(installed, &key.1, 14.0);
            let focused = shown.focused() == Some(key);
            let header = self.split_header(key, title.clone(), glyph, many, focused, cx);
            let card = div()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .rounded(px(14.0))
                .border_1()
                .border_color(rgba(HAIRLINE))
                .bg(rgb(theme::SURFACE))
                // The page pads itself, as the desktop's `.w-shell` does, 14px in from the card's edge. It keeps its
                // last drawing until it changes, so a busy terminal doesn't rebuild it every frame.
                .child(page.clone().cached(gpui::StyleRefinement::default().size_full()));
            let split = div()
                .id(SharedString::from(format!("right-split-{}-{}", key.0, key.1)))
                .role(Role::Group)
                .aria_label(title)
                .size_full()
                .min_h_0()
                .flex()
                .flex_col()
                .child(header)
                .child(card)
                .into_any_element();
            let size = shown.sizes().get(index).copied().unwrap_or(1.0);
            splits.push((key.clone(), split, size));
        }
        if splits.is_empty() {
            return None;
        }
        let count = splits.len();
        let mut column = div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(self.split_measure(cx));
        for (index, (_, split, size)) in splits.into_iter().enumerate() {
            column = column.child(
                div()
                    .min_h_0()
                    .flex()
                    .flex_basis(px(0.0))
                    .flex_grow(if count == 1 { 1.0 } else { size })
                    .child(split),
            );
            if index + 1 < count {
                column = column.child(self.split_divider(index, cx));
            }
        }
        Some(column.into_any_element())
    }

    /// A page's header: its icon and title, and the button that closes it. A right-click on it opens the sidebar's menu.
    fn split_header(
        &self,
        key: &Key,
        title: String,
        glyph: AnyElement,
        many: bool,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (close_id, close_label, close_key) = if many {
            (
                ElementId::from(SharedString::from(format!("close-right-split-{}-{}", key.0, key.1))),
                "Close split",
                key.clone(),
            )
        } else {
            (ElementId::from("close-right-sidebar"), "Close sidebar", key.clone())
        };
        let menu_key = key.clone();
        div()
            .h(px(if many { SPLIT_HEADER } else { LONE_HEADER }))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.0))
            .pl(px(4.0))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.open_sidebar_menu(menu_key.clone(), event, cx)
                }),
            )
            // The desktop's `h2`: the sidebar's icon, then its title.
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .pl(px(8.0))
                    .text_size(px(12.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(if many && focused {
                        theme::FOREGROUND
                    } else {
                        theme::MUTED_FOREGROUND
                    }))
                    .child(glyph)
                    .child(div().min_w_0().truncate().child(title)),
            )
            .child(
                ui::icon_button(close_id, Icon::X, close_label, ButtonSize::Xs).on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if many {
                            this.close_split(&close_key, cx);
                        } else {
                            this.close_right(cx);
                        }
                    }),
                ),
            )
            .into_any_element()
    }

    /// Notes where the stack is drawn, for the drag to turn the pointer into a share of its height, and follows the
    /// pointer while a divider is held.
    fn split_measure(&self, cx: &mut Context<Self>) -> AnyElement {
        let entity = cx.entity();
        let dragging = self.plugin_ui.right_split.dragging.is_some();
        canvas(
            {
                let entity = entity.clone();
                move |bounds, _, cx| entity.update(cx, |this, _| this.plugin_ui.right_split.area = Some(bounds))
            },
            move |_, _, window: &mut Window, _| {
                if dragging {
                    ui::window_drag(
                        window,
                        &entity,
                        |this, event, _, cx| {
                            if event.dragging() {
                                this.split_drag_to(event.position.y, cx);
                            } else {
                                this.end_split_drag(cx);
                            }
                        },
                        |this, _, cx| this.end_split_drag(cx),
                    );
                }
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .into_any_element()
    }

    /// The line between two splits, drawn as a tab's panes' dividers are.
    fn split_divider(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let group = SharedString::from(format!("right-divider-{index}"));
        div()
            .id(group.clone())
            .group(group.clone())
            .role(Role::Splitter)
            .aria_label("Resize sidebars")
            .aria_orientation(Orientation::Horizontal)
            .relative()
            .flex_none()
            .h(px(DIVIDER))
            .w_full()
            .cursor_row_resize()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| this.start_split_drag(index, cx)),
            )
            .child(
                div()
                    .absolute()
                    .rounded_full()
                    .left(px(24.0))
                    .right(px(24.0))
                    .top(px(3.0))
                    .h(px(2.0))
                    .group_hover(group, |style| style.bg(rgba(DIVIDER_HOVER))),
            )
            .children(ui::spot(move || format!("right-divider-{index}")))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(name: &str) -> Key {
        ("plug".into(), name.into())
    }

    #[test]
    fn pages_stack_with_equal_shares_and_an_open_one_is_not_added_twice() {
        let mut stack = Stack::default();
        stack.add(key("a"));
        stack.add(key("b"));
        stack.add(key("c"));
        assert_eq!(stack.len(), 3);
        assert!(stack.sizes().iter().all(|size| (size - 1.0 / 3.0).abs() < 1e-5));
        stack.add(key("a"));
        assert_eq!(stack.len(), 3);
        assert_eq!(stack.focused(), Some(&key("a")));
    }

    #[test]
    fn a_page_past_the_cap_takes_the_last_ones_place() {
        let mut stack = Stack::default();
        for name in ["a", "b", "c", "d", "e"] {
            stack.add(key(name));
        }
        let names: Vec<_> = stack.keys().iter().map(|key| key.1.as_str()).collect();
        assert_eq!(names, ["a", "b", "c", "e"]);
    }

    #[test]
    fn closing_one_gives_its_height_to_the_rest_and_a_divider_keeps_a_minimum() {
        let mut stack = Stack::default();
        stack.add(key("a"));
        stack.add(key("b"));
        stack.add(key("c"));
        assert!(stack.remove(&key("b")));
        assert_eq!(stack.len(), 2);
        assert!((stack.sizes().iter().sum::<f32>() - 1.0).abs() < 1e-5);
        stack.set_boundary(0, 0.01, 0.2);
        assert!((stack.sizes()[0] - 0.2).abs() < 1e-5);
        assert!((stack.sizes()[1] - 0.8).abs() < 1e-5);
    }

    #[test]
    fn what_was_saved_comes_back_and_bad_sizes_become_equal() {
        let mut stack = Stack::default();
        stack.add(key("a"));
        stack.add(key("b"));
        stack.set_boundary(0, 0.3, 0.1);
        let back = Stack::from_saved(&stack.to_saved());
        assert_eq!(back.keys(), stack.keys());
        assert!((back.sizes()[0] - 0.3).abs() < 1e-5);
        let bad = [
            SavedSplit { plugin: "p".into(), page: "a".into(), size: Some(-1.0) },
            SavedSplit { plugin: "p".into(), page: "b".into(), size: None },
        ];
        assert_eq!(Stack::from_saved(&bad).sizes(), [0.5, 0.5]);
    }
}
