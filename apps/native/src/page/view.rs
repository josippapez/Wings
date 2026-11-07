//! Draws a page's tree with the UI kit, in the look of the desktop's plugin pages, and sends its clicks back.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::Duration,
};

use gpui::{
    Animation, AnimationExt, AnyElement, Context, ElementId, Focusable, FontWeight, IntoElement,
    KeyDownEvent, Pixels, Render, Role, SharedString, Task, Toggled, Window, canvas, div,
    prelude::*, px, relative, rgb, rgba,
};

#[path = "board_view.rs"]
mod board_view;
#[path = "graph_view.rs"]
mod graph_view;
#[path = "rich_view.rs"]
mod rich_view;
#[path = "sunburst_view.rs"]
mod sunburst_view;

use super::{
    plugin::PluginHost,
    runtime::{Event, PageRuntime},
    tree::{self, Align, Gap, Justify, Level, Node, TextSize, Tone, Tree, Weight, plain_text},
};
use crate::{terminal_view::DEFAULT_FONT_FAMILY, theme, ui};

/// What the page thread has sent that the view hasn't taken yet: only the newest tree matters, so a page that
/// renders faster than the window can draw never builds up a backlog.
#[derive(Default)]
struct Pending {
    tree: Option<Tree>,
    error: Option<String>,
}

struct SearchState {
    field: gpui::Entity<crate::text_field::TextField>,
    handler: Option<String>,
}

/// A form input. The page owns its text; the field has its own between the page's renders.
struct InputState {
    field: gpui::Entity<crate::text_field::TextField>,
    handler: Option<String>,
    key_handler: Option<String>,
    blur_handler: Option<String>,
    initialized: bool,
    auto_focus: bool,
    select_on_focus: bool,
    blur_subscription: Option<gpui::Subscription>,
    /// The text the page last gave it. The field takes the page's text when this changes, which is when the page set it
    /// (a form filled in once it loaded), and not on every render, which would undo a key the page hasn't heard yet.
    given: String,
    /// The texts sent to the page that it hasn't given back yet. A page that is behind gives back each of them in turn,
    /// and taking one of those would undo the keys typed since.
    sent: Vec<String>,
}

pub struct PageView {
    runtime: Option<PageRuntime>,
    tree: Option<Tree>,
    /// The disclosures that are open, by node id. The page never hears of it: the app keeps it, and an id stays the
    /// same through every render of its instance, so it survives them.
    open: HashSet<u64>,
    /// The select whose menu is open, by node id.
    menu: Option<u64>,
    /// A press on that select's trigger found its menu open, so the click that follows closes it. The menu closes
    /// itself on any press outside it, the trigger's too, which would otherwise have the click open it again.
    pressed_open: bool,
    /// How wide each select was drawn, by node id: its menu is as wide.
    select_widths: HashMap<u64, Pixels>,
    fields: RefCell<HashMap<u64, SearchState>>,
    inputs: RefCell<HashMap<u64, InputState>>,
    graphs: RefCell<HashMap<u64, graph_view::GraphState>>,
    sunbursts: RefCell<HashMap<u64, sunburst_view::SunburstState>>,
    chip_focus: RefCell<HashMap<(u64, String), gpui::FocusHandle>>,
    /// What the page last threw, shown under it until it renders again.
    error: Option<SharedString>,
    _events: Task<()>,
}

impl PageView {
    pub fn set_visible(&self, visible: bool) {
        if let Some(runtime) = &self.runtime {
            runtime.set_visible(visible);
        }
    }

    /// Starts the page bundle `source`; its first tree is drawn when it arrives.
    pub fn new(source: &str, cx: &mut Context<Self>) -> Self {
        Self::start(cx, |on_event| PageRuntime::spawn(source, on_event))
    }

    /// Opens `source` as a page of the plugin `host` runs, which can talk to its main and its other pages.
    pub fn in_plugin(host: &PluginHost, source: &str, cx: &mut Context<Self>) -> Self {
        Self::start(cx, |on_event| host.open_hidden_page(source, on_event))
    }

    /// Like `in_plugin`, for a tab `wings.openView` opened with `args`, which the page reads with `viewArgs`.
    pub fn in_plugin_with_args(
        host: &PluginHost,
        source: &str,
        args: Option<&serde_json::Value>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::start(cx, |on_event| {
            host.open_hidden_page_with_args(source, args, on_event)
        })
    }

    /// Like `in_plugin_with_args`, for a tab or block whose `wings.tabState`/`blockState` is `state`.
    pub fn in_plugin_with_state(
        host: &PluginHost,
        source: &str,
        args: Option<&serde_json::Value>,
        state: crate::page::plugin::FrameState,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::start(cx, |on_event| {
            host.open_hidden_page_with_state(source, args, state, on_event)
        })
    }

    pub fn in_plugin_window(
        host: &PluginHost,
        window_id: u64,
        source: &str,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::start(cx, |on_event| {
            host.open_window_page(source, window_id, on_event)
        })
    }

    fn start(
        cx: &mut Context<Self>,
        spawn: impl FnOnce(Box<dyn FnMut(Event) + Send>) -> std::io::Result<PageRuntime>,
    ) -> Self {
        let pending = Arc::new(Mutex::new(Pending::default()));
        let (wake, woken) = async_channel::bounded::<()>(1);
        let runtime = spawn(Box::new({
            let pending = pending.clone();
            move |event| {
                {
                    let mut pending = pending.lock().unwrap();
                    match event {
                        Event::Tree(tree) => pending.tree = Some(tree),
                        Event::Error { message, .. } => pending.error = Some(message),
                    }
                }
                let _ = wake.try_send(());
            }
        }));
        let (runtime, error) = match runtime {
            Ok(runtime) => (Some(runtime), None),
            Err(error) => (None, Some(format!("the page didn't start: {error}").into())),
        };
        let events = cx.spawn(async move |view, cx| {
            while woken.recv().await.is_ok() {
                let taken = std::mem::take(&mut *pending.lock().unwrap());
                if view
                    .update(cx, |view, cx| {
                        if let Some(tree) = taken.tree {
                            let mut ids = HashSet::new();
                            fn controls(nodes: &[Node], ids: &mut HashSet<u64>) {
                                for node in nodes {
                                    match node {
                                        Node::SearchField(field) => {
                                            ids.insert(field.id);
                                        }
                                        Node::Input(input) => {
                                            ids.insert(input.id);
                                        }
                                        Node::Graph(graph) => {
                                            ids.insert(graph.id);
                                        }
                                        Node::Sunburst(map) => {
                                            ids.insert(map.id);
                                        }
                                        Node::Chips(chips) => {
                                            ids.insert(chips.id);
                                        }
                                        _ => {}
                                    }
                                    for part in node.parts() {
                                        controls(part, ids);
                                    }
                                }
                            }
                            controls(&tree.children, &mut ids);
                            view.fields.borrow_mut().retain(|id, _| ids.contains(id));
                            view.inputs.borrow_mut().retain(|id, _| ids.contains(id));
                            view.graphs.borrow_mut().retain(|id, _| ids.contains(id));
                            view.sunbursts.borrow_mut().retain(|id, _| ids.contains(id));
                            view.chip_focus
                                .borrow_mut()
                                .retain(|(id, _), _| ids.contains(id));
                            view.tree = Some(tree);
                            view.error = None;
                        }
                        if let Some(error) = taken.error {
                            view.error = Some(error.into());
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            runtime,
            tree: None,
            open: HashSet::new(),
            menu: None,
            pressed_open: false,
            select_widths: HashMap::new(),
            fields: RefCell::new(HashMap::new()),
            inputs: RefCell::new(HashMap::new()),
            graphs: RefCell::new(HashMap::new()),
            sunbursts: RefCell::new(HashMap::new()),
            chip_focus: RefCell::new(HashMap::new()),
            error,
            _events: events,
        }
    }

    fn send(&self, handler: &str, payload: Option<&serde_json::Value>) {
        if let Some(runtime) = &self.runtime {
            runtime.dispatch(handler, payload);
        }
    }

    /// `send` for a click, a key press or a pick, which lets the page copy right after.
    fn act(&self, handler: &str, payload: Option<&serde_json::Value>) {
        if let Some(runtime) = &self.runtime {
            runtime.dispatch_gesture(handler, payload);
        }
    }

    fn toggle_disclosure(&mut self, id: u64, cx: &mut Context<Self>) {
        if !self.open.remove(&id) {
            self.open.insert(id);
        }
        cx.notify();
    }

    fn toggle_menu(&mut self, id: u64, cx: &mut Context<Self>) {
        let was_open = std::mem::take(&mut self.pressed_open) || self.menu == Some(id);
        self.menu = (!was_open).then_some(id);
        cx.notify();
    }

    /// The person picked `value`: the page hears it as the select's `onChange` does, with `{"value": ...}`.
    fn pick(&mut self, on_change: Option<&str>, value: &str, cx: &mut Context<Self>) {
        self.menu = None;
        if let Some(handler) = on_change {
            self.act(handler, Some(&serde_json::json!({ "value": value })));
        }
        cx.notify();
    }

    /// Presses the `nth` button labelled `label`, as a click would. False when the page has no such button.
    #[cfg(feature = "test-support")]
    pub fn press(&self, label: &str, nth: usize) -> bool {
        fn find<'a>(nodes: &'a [Node], label: &str, found: &mut Vec<&'a str>) {
            for node in nodes {
                if let Node::Button(button) = node
                    && plain_text(&button.children).trim() == label
                    && let Some(handler) = &button.on_click
                {
                    found.push(handler);
                }
                for part in node.parts() {
                    find(part, label, found);
                }
            }
        }
        let mut found = Vec::new();
        if let Some(tree) = &self.tree {
            find(&tree.children, label, &mut found);
        }
        found.get(nth).is_some_and(|handler| {
            self.act(handler, None);
            true
        })
    }

    /// Presses the `nth` row that opens (`aria-expanded`) and shows `text`, as a click would. False when the page has none.
    #[cfg(feature = "test-support")]
    pub fn press_row(&self, text: &str, nth: usize) -> bool {
        fn find<'a>(nodes: &'a [Node], text: &str, found: &mut Vec<&'a str>) {
            for node in nodes {
                if let Node::ListButton(row) = node
                    && row.expanded.is_some()
                    && plain_text(&row.button.children).contains(text)
                    && let Some(handler) = &row.button.on_click
                {
                    found.push(handler);
                }
                for part in node.parts() {
                    find(part, text, found);
                }
            }
        }
        let mut found = Vec::new();
        if let Some(tree) = &self.tree {
            find(&tree.children, text, &mut found);
        }
        found.get(nth).is_some_and(|handler| {
            self.act(handler, None);
            true
        })
    }

    /// Presses the first `ListButton` that shows `text`, as a click would. False when the page has none.
    #[cfg(feature = "test-support")]
    pub fn press_list_button(&self, text: &str) -> bool {
        fn find<'a>(nodes: &'a [Node], text: &str) -> Option<&'a str> {
            nodes.iter().find_map(|node| match node {
                Node::ListButton(row) if plain_text(&row.button.children).contains(text) => {
                    row.button.on_click.as_deref()
                }
                other => other.parts().into_iter().find_map(|part| find(part, text)),
            })
        }
        let handler = self
            .tree
            .as_ref()
            .and_then(|tree| find(&tree.children, text));
        handler.is_some_and(|handler| {
            self.act(handler, None);
            true
        })
    }

    /// Picks the chip of `Chips` that shows `label`, as a click would. False when the page has no such chip.
    #[cfg(feature = "test-support")]
    pub fn press_chip(&self, label: &str) -> bool {
        fn find(nodes: &[Node], label: &str) -> Option<(String, String)> {
            nodes.iter().find_map(|node| match node {
                Node::Chips(chips) => chips
                    .options
                    .iter()
                    .find(|option| option.label.starts_with(label))
                    .zip(chips.on_change.clone())
                    .map(|(option, handler)| (handler, option.value.clone())),
                other => other.parts().into_iter().find_map(|part| find(part, label)),
            })
        }
        let found = self
            .tree
            .as_ref()
            .and_then(|tree| find(&tree.children, label));
        found.is_some_and(|(handler, value)| {
            self.act(&handler, Some(&serde_json::json!({ "value": value })));
            true
        })
    }

    #[cfg(feature = "test-support")]
    pub fn text(&self) -> String {
        self.tree.as_ref().map(Tree::text).unwrap_or_default()
    }

    #[cfg(all(test, feature = "test-support"))]
    pub(super) fn open_disclosures(&self) -> usize {
        self.open.len()
    }

    #[cfg(all(test, feature = "test-support"))]
    pub(super) fn menu_open(&self) -> bool {
        self.menu.is_some()
    }

    #[cfg(all(test, feature = "test-support"))]
    pub(super) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    #[cfg(all(test, feature = "test-support"))]
    pub(super) fn checkbox_checked(&self) -> Option<bool> {
        fn find(nodes: &[Node]) -> Option<bool> {
            nodes.iter().find_map(|node| match node {
                Node::Checkbox(checkbox) => Some(checkbox.checked),
                other => other.parts().into_iter().find_map(find),
            })
        }
        self.tree.as_ref().and_then(|tree| find(&tree.children))
    }

    #[cfg(all(test, feature = "test-support"))]
    pub(super) fn graph_window(&self) -> Option<(usize, usize, usize)> {
        fn find(nodes: &[Node]) -> Option<(usize, usize, usize)> {
            nodes.iter().find_map(|node| match node {
                Node::Graph(graph) => Some((graph.total, graph.offset, graph.rows.len())),
                other => other.parts().into_iter().find_map(find),
            })
        }
        self.tree.as_ref().and_then(|tree| find(&tree.children))
    }

    fn nodes(&self, nodes: &[Node], cx: &mut Context<Self>) -> Vec<AnyElement> {
        nodes.iter().map(|node| self.node(node, cx)).collect()
    }

    /// What `nodes` say as one piece of text, or as inline parts when they hold more than strings.
    fn inline(&self, nodes: &[Node], cx: &mut Context<Self>) -> AnyElement {
        if nodes.iter().all(|node| matches!(node, Node::Str { .. })) {
            return plain_text(nodes).into_any_element();
        }
        if let Some(rich) = self.rich_text(nodes, cx) {
            return rich;
        }
        div()
            .flex()
            .flex_wrap()
            .items_baseline()
            .children(self.nodes(nodes, cx))
            .into_any_element()
    }

    fn node(&self, node: &Node, cx: &mut Context<Self>) -> AnyElement {
        match node {
            Node::Str { text, .. } => div().child(text.clone()).into_any_element(),
            // Only a component that names its parts reads them; one out of place just shows what it holds.
            Node::Slot(slot) => div()
                .children(self.nodes(&slot.children, cx))
                .into_any_element(),
            Node::Page(page) => {
                let body = div()
                    .id(ElementId::NamedInteger("page-body".into(), page.id))
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .when(page.flush, |body| body.overflow_hidden())
                    .when(!page.flush, |body| {
                        body.gap(px(10.0)).p(px(14.0)).overflow_y_scroll()
                    })
                    .children(self.nodes(&page.body, cx));
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .children(self.nodes(&page.toolbar, cx))
                    .child(body)
                    .into_any_element()
            }
            Node::Toolbar(toolbar) => div()
                .id(ElementId::NamedInteger("page-toolbar".into(), toolbar.id))
                .role(Role::Toolbar)
                .flex_none()
                .min_h(px(44.0))
                .flex()
                .items_center()
                .gap(px(10.0))
                .px(px(14.0))
                .py(px(8.0))
                .border_b_1()
                .border_color(rgba(ui::HAIRLINE))
                .when(!toolbar.title.is_empty(), |bar| {
                    bar.child(
                        div()
                            .flex_none()
                            .max_w(relative(0.4))
                            .truncate()
                            .text_size(px(14.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.inline(&toolbar.title, cx)),
                    )
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.0))
                        .children(self.nodes(&toolbar.filters, cx)),
                )
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .children(self.nodes(&toolbar.actions, cx)),
                )
                .into_any_element(),
            Node::Stack(stack) => div()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(gap(stack.gap)))
                .when(stack.grow, |el| el.flex_1())
                .when_some(stack.width, |el, width| el.w(px(width)).flex_none())
                .children(self.nodes(&stack.children, cx))
                .into_any_element(),
            Node::Row(row) => div()
                .min_w_0()
                .flex()
                .gap(px(gap(row.gap)))
                .map(|el| match row.align {
                    Align::Center => el.items_center(),
                    Align::Start => el.items_start(),
                    Align::Baseline => el.items_baseline(),
                })
                .map(|el| match row.justify {
                    Justify::Start => el,
                    Justify::End => el.justify_end(),
                    Justify::Between => el.justify_between(),
                })
                .when(row.wrap, |el| el.flex_wrap())
                .when(row.grow, |el| el.flex_1())
                .children(self.nodes(&row.children, cx))
                .into_any_element(),
            Node::Text(text) => {
                let element = div()
                    .when(text.grow || text.truncate, |el| el.min_w_0())
                    .when(text.grow, |el| el.flex_1())
                    .when(text.truncate, |el| el.truncate())
                    .when(text.clamp, |el| el.line_clamp(3))
                    .when(text.mono, |el| el.font_family(DEFAULT_FONT_FAMILY))
                    .map(|el| match text.size {
                        TextSize::Normal => el,
                        TextSize::Small => el.text_size(px(12.0)),
                        TextSize::Tiny => el.text_size(px(11.0)),
                        TextSize::Large => el.text_size(px(20.0)).line_height(px(26.0)),
                        TextSize::Field => el.text_size(px(14.0)).line_height(relative(1.5)),
                    })
                    .map(|el| match text.weight {
                        Weight::Normal => el,
                        Weight::Medium => el.font_weight(FontWeight::MEDIUM),
                        Weight::Strong => el.font_weight(FontWeight::SEMIBOLD),
                    })
                    .when_some(text.tone, |el, tone| el.text_color(rgb(tone_color(tone))))
                    .child(self.inline(&text.children, cx));
                match text.on_click.clone() {
                    Some(handler) => element
                        .id(ElementId::NamedInteger("page-text".into(), text.id))
                        .cursor_pointer()
                        .text_color(rgb(text.tone.map_or(theme::INFO, tone_color)))
                        .on_click(cx.listener(move |view, _, _, _| view.act(&handler, None)))
                        .into_any_element(),
                    None => element.into_any_element(),
                }
            }
            Node::Button(button) => {
                let id = ElementId::NamedInteger("page-button".into(), button.id);
                let label = button
                    .label
                    .clone()
                    .unwrap_or_else(|| plain_text(&button.children));
                let content = self.inline(&button.children, cx);
                let element = if button.chip {
                    ui::chip(id, label, button.pressed, button.disabled, content)
                } else {
                    sized(
                        ui::button(id, label, variant(button.variant), button.disabled, content),
                        button.size,
                    )
                };
                let key_handler = button.on_key_down.clone();
                let element = element.on_key_down(cx.listener(
                    move |view, event: &KeyDownEvent, window, cx| {
                        if let Some(handler) = &key_handler {
                            send_shortcut(view, handler, event, window, cx);
                        }
                    },
                ));
                match button.on_click.clone() {
                    Some(handler) => element
                        .on_click(cx.listener(move |view, _, _, cx| {
                            cx.stop_propagation();
                            view.act(&handler, None);
                        }))
                        .into_any_element(),
                    None => element.into_any_element(),
                }
            }
            Node::Notice(notice) => div()
                .id(ElementId::NamedInteger("page-notice".into(), notice.id))
                .role(if matches!(notice.tone, Some(Tone::Warn | Tone::Danger)) {
                    Role::Alert
                } else {
                    Role::Status
                })
                .rounded(px(8.0))
                .border_1()
                .border_color(rgba(notice.tone.map_or(theme::INFO, tone_color)))
                .bg(rgba(
                    notice.tone.map_or(theme::INFO, tone_color) << 8 | 0x20,
                ))
                .px(px(10.0))
                .py(px(7.0))
                .text_size(px(12.0))
                .text_color(rgb(notice.tone.map_or(theme::INFO, tone_color)))
                .child(self.inline(&notice.children, cx))
                .into_any_element(),
            Node::Spinner(spinner) => div()
                .id(ElementId::NamedInteger("page-spinner".into(), spinner.id))
                .role(Role::Status)
                .when(!spinner.label.is_empty(), |el| {
                    el.aria_label(spinner.label.clone())
                })
                .size(px(if spinner.large { 18.0 } else { 14.0 }))
                .flex_none()
                .with_animation(
                    ElementId::NamedInteger("page-spinner-animation".into(), spinner.id),
                    Animation::new(Duration::from_secs(1))
                        .repeat()
                        .with_max_fps(30.0),
                    |ring, turn| ring.child(ui::working_spinner(turn * std::f32::consts::TAU)),
                )
                .into_any_element(),
            Node::Checkbox(checkbox) => {
                let label = plain_text(&checkbox.children);
                let checked = checkbox.checked;
                let element = div()
                    .id(ElementId::NamedInteger("page-checkbox".into(), checkbox.id))
                    .role(Role::Button)
                    .aria_label(label)
                    .aria_toggled(if checkbox.checked {
                        Toggled::True
                    } else {
                        Toggled::False
                    })
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .cursor_pointer()
                    .opacity(if checkbox.disabled { 0.5 } else { 1.0 })
                    .child(
                        div()
                            .size(px(16.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.0))
                            .border_1()
                            .border_color(rgba(ui::HAIRLINE))
                            .bg(rgba(if checkbox.checked {
                                theme::INFO << 8 | 0xff
                            } else {
                                0
                            }))
                            .text_size(px(12.0))
                            .text_color(rgb(theme::BACKGROUND))
                            .child(if checkbox.checked {
                                "✓".into_any_element()
                            } else {
                                div().into_any_element()
                            }),
                    )
                    .child(self.inline(&checkbox.children, cx));
                match checkbox.on_change.clone() {
                    Some(handler) => {
                        let keyboard_handler = handler.clone();
                        element
                            .focusable()
                            .tab_stop(true)
                            .focus_visible(|style| style.border_color(rgb(theme::FOREGROUND)))
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation()
                            })
                            .on_key_down(cx.listener(move |view, event: &KeyDownEvent, _, _| {
                                if !event.keystroke.modifiers.modified()
                                    && matches!(event.keystroke.key.as_str(), "space" | "enter")
                                {
                                    view.act(
                                        &keyboard_handler,
                                        Some(&serde_json::json!({ "checked": !checked })),
                                    );
                                }
                            }))
                            .on_click(cx.listener(move |view, _, _, _| {
                                view.act(
                                    &handler,
                                    Some(&serde_json::json!({ "checked": !checked })),
                                )
                            }))
                            .into_any_element()
                    }
                    None => element
                        .cursor_default()
                        .a11y_synthetic_children(|builder| {
                            builder.parent_node().set_disabled();
                        })
                        .into_any_element(),
                }
            }
            Node::Confirm(confirm) => ui::plain_card(vec![
                div()
                    .px(px(12.0))
                    .pt(px(12.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.inline(&confirm.title, cx))
                    .into_any_element(),
                div()
                    .px(px(12.0))
                    .py(px(8.0))
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .children(self.nodes(&confirm.body, cx))
                    .into_any_element(),
                div()
                    .px(px(12.0))
                    .pb(px(12.0))
                    .flex()
                    .flex_wrap()
                    .gap(px(4.0))
                    .children(self.nodes(&confirm.actions, cx))
                    .into_any_element(),
            ])
            .into_any_element(),
            Node::Badge(badge) => ui::badge(
                variant(badge.variant),
                badge.tone.map(tone_color),
                badge.small,
                self.inline(&badge.children, cx),
            )
            .into_any_element(),
            Node::Empty(empty) => div()
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(16.0))
                .p(px(24.0))
                .text_center()
                .child(
                    div()
                        .max_w(px(384.0))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            div()
                                .text_size(px(14.0))
                                .font_weight(FontWeight::MEDIUM)
                                .child(self.inline(&empty.title, cx)),
                        )
                        .when(!empty.description.is_empty(), |header| {
                            header.child(
                                div()
                                    .text_size(px(14.0))
                                    .text_color(rgb(theme::MUTED_FOREGROUND))
                                    .child(self.inline(&empty.description, cx)),
                            )
                        }),
                )
                .children(self.nodes(&empty.children, cx))
                .into_any_element(),
            Node::Loading(loading) => div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .text_color(rgb(theme::MUTED_FOREGROUND))
                .child(if loading.children.is_empty() {
                    "Loading…".into_any_element()
                } else {
                    self.inline(&loading.children, cx)
                })
                .into_any_element(),
            Node::Card(card) => ui::plain_card(self.nodes(&card.children, cx)).into_any_element(),
            Node::Heading(heading) => {
                ui::heading(self.inline(&heading.children, cx)).into_any_element()
            }
            Node::Dot(dot) => ui::dot(
                dot.color
                    .unwrap_or_else(|| dot.tone.map_or(theme::DONE, tone_color)),
            )
            .into_any_element(),
            // A scroll keeps 8 points either side and 12 below, as a list under a header does.
            Node::Scroll(scroll) => div()
                .id(ElementId::NamedInteger("page-scroll".into(), scroll.id))
                .flex_1()
                .min_h_0()
                .min_w_0()
                .flex()
                .flex_col()
                .px(px(8.0))
                .pb(px(12.0))
                .overflow_y_scroll()
                .children(self.nodes(&scroll.children, cx))
                .into_any_element(),
            Node::Bar(bar) => div()
                .flex_none()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(gap(bar.gap)))
                .px(px(14.0))
                .py(px(8.0))
                .when(bar.edge == tree::Edge::Top, |el| {
                    el.border_t_1().border_color(rgba(ui::HAIRLINE))
                })
                .when(bar.edge == tree::Edge::Bottom, |el| {
                    el.border_b_1().border_color(rgba(ui::HAIRLINE))
                })
                .children(self.nodes(&bar.children, cx))
                .into_any_element(),
            Node::Split(split) => div()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .flex()
                .child(
                    div()
                        .w(px(split.size))
                        .max_w(relative(0.5))
                        .flex_none()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .border_r_1()
                        .border_color(rgba(ui::HAIRLINE))
                        .children(self.nodes(&split.start, cx)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .children(self.nodes(&split.end, cx)),
                )
                .into_any_element(),
            Node::Gauge(gauge) => ui::gauge(
                ElementId::NamedInteger("page-gauge".into(), gauge.id),
                gauge.label.clone(),
                gauge.percent,
                level_color(gauge.level()),
                (!gauge.note.is_empty()).then(|| self.inline(&gauge.note, cx)),
            )
            .into_any_element(),
            Node::Meter(meter) => board_view::meter(meter),
            Node::Board(board) => self.board(board, cx),
            Node::BoardRow(row) => self.board_row(&row.children, cx),
            Node::BoardColumn(column) => self.board_column(column, cx),
            Node::List(list) => ui::list(self.nodes(&list.children, cx)).into_any_element(),
            Node::ListItem(item) => {
                if item
                    .children
                    .iter()
                    .any(|node| matches!(node, Node::ListButton(_)))
                {
                    div()
                        .min_w_0()
                        .w_full()
                        .flex()
                        .items_center()
                        .children(self.nodes(&item.children, cx))
                        .into_any_element()
                } else {
                    // A card in a row takes the row's width, as a page's own style would have it do.
                    ui::list_item(
                        ElementId::NamedInteger("page-list-item".into(), item.id),
                        item.children.iter().map(|child| {
                            let element = self.node(child, cx);
                            if matches!(child, Node::Card(_)) {
                                div().flex_1().min_w_0().child(element).into_any_element()
                            } else {
                                element
                            }
                        }),
                    )
                    .into_any_element()
                }
            }
            Node::Disclosure(disclosure) => self.disclosure(disclosure, cx),
            Node::Select(select) => self.select(select, cx),
            Node::Chips(select) => {
                let focus = select
                    .options
                    .iter()
                    .filter(|o| !o.disabled)
                    .map(|option| {
                        let handle = self
                            .chip_focus
                            .borrow_mut()
                            .entry((select.id, option.value.clone()))
                            .or_insert_with(|| cx.focus_handle().tab_stop(true))
                            .clone();
                        (option.value.clone(), handle)
                    })
                    .collect::<HashMap<_, _>>();
                let options = select
                    .options
                    .iter()
                    .enumerate()
                    .map(|(index, option)| {
                        let handler = select.on_change.clone();
                        let value = option.value.clone();
                        let keyboard_handler = handler.clone();
                        let keyboard_value = value.clone();
                        let next = select
                            .options
                            .iter()
                            .filter(|o| !o.disabled)
                            .map(|o| o.value.clone())
                            .collect::<Vec<_>>();
                        let selected = select.current() == Some(value.as_str());
                        let disabled = option.disabled;
                        let keyboard_focus = focus.clone();
                        let click_focus = focus.get(&value).cloned();
                        ui::chip(
                            ElementId::NamedInteger(
                                format!("page-chips-{}", select.id).into(),
                                index as u64,
                            ),
                            option.label.clone(),
                            selected,
                            option.disabled,
                            option.label.clone(),
                        )
                        .when_some(focus.get(&value), |button, focus| button.track_focus(focus))
                        .on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
                            let at = next
                                .iter()
                                .position(|v| v == &keyboard_value)
                                .unwrap_or_default();
                            let target = match event.keystroke.key.as_str() {
                                "left" => next
                                    .get(at.checked_sub(1).unwrap_or(next.len().saturating_sub(1))),
                                "right" => next.get((at + 1) % next.len().max(1)),
                                "home" => next.first(),
                                "end" => next.last(),
                                "space" | "enter" => next.get(at),
                                _ => return,
                            };
                            if let (Some(handler), Some(value)) = (&keyboard_handler, target) {
                                view.act(handler, Some(&serde_json::json!({ "value": value })));
                                if let Some(focus) = keyboard_focus.get(value) {
                                    focus.focus(window, cx);
                                }
                            }
                            window.prevent_default();
                            cx.stop_propagation();
                        }))
                        .on_click(cx.listener(move |view, _, window, cx| {
                            if !disabled {
                                if let Some(focus) = &click_focus {
                                    focus.focus(window, cx);
                                }
                                if let Some(handler) = &handler {
                                    view.act(handler, Some(&serde_json::json!({ "value": value })));
                                }
                            }
                        }))
                        .into_any_element()
                    })
                    .collect::<Vec<_>>();
                div()
                    .id(ElementId::NamedInteger("page-chips".into(), select.id))
                    .role(Role::Group)
                    .aria_label(select.label.clone())
                    .flex()
                    .flex_wrap()
                    // plugin-ui.css's `.w-chips`.
                    .gap(px(8.0))
                    .children(options)
                    .into_any_element()
            }
            Node::ListButton(item) => {
                let button = &item.button;
                let handler = button.on_click.clone();
                let keyboard = handler.clone();
                let key_handler = button.on_key_down.clone();
                let mut element = div()
                    .id(ElementId::NamedInteger(
                        "page-list-button".into(),
                        button.id,
                    ))
                    .role(Role::Button)
                    .aria_label(plain_text(&button.children))
                    .w_full()
                    .min_w_0()
                    .flex()
                    .gap(px(8.0))
                    .items_center()
                    .px(px(6.0))
                    .pl(px(6.0 + item.indent))
                    .py(px(2.0))
                    .rounded(px(6.0))
                    .border_1()
                    .border_color(rgba(0))
                    .when(item.vertical, |el| el.flex_col().items_stretch())
                    .when_some(item.expanded, |el, expanded| {
                        el.aria_expanded(expanded)
                            .child(if expanded { "⌄" } else { "›" })
                    })
                    .children(self.nodes(&button.children, cx));
                if !button.disabled {
                    element = element
                        .focusable()
                        .tab_stop(true)
                        .cursor_pointer()
                        .hover(|s| s.bg(rgba(theme::HOVER)))
                        .focus_visible(|s| s.border_color(rgb(theme::FOREGROUND)))
                        .on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
                            if let Some(handler) = &key_handler
                                && send_shortcut(view, handler, event, window, cx)
                            {
                                return;
                            }
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                if let Some(handler) = &keyboard {
                                    view.act(handler, None);
                                }
                                window.prevent_default();
                                cx.stop_propagation();
                            }
                        }))
                        .on_click(cx.listener(move |view, _, _, cx| {
                            cx.stop_propagation();
                            if let Some(handler) = &handler {
                                view.act(handler, None);
                            }
                        }));
                }
                element.into_any_element()
            }
            Node::SearchField(field) => {
                let mut fields = self.fields.borrow_mut();
                let state = fields.entry(field.id).or_insert_with(|| {
                    let value = field.value.clone().unwrap_or_default();
                    let entity = cx.new(|cx| {
                        let mut input = crate::text_field::TextField::new(
                            Role::SearchInput,
                            field.label.clone(),
                            field.placeholder.clone(),
                            px(280.0),
                            cx,
                        );
                        input.text = value;
                        input
                    });
                    let id = field.id;
                    cx.subscribe(&entity, move |view, entity, event, cx| {
                        if matches!(event, crate::text_field::FieldEvent::Changed)
                            && let Some(handler) = view
                                .fields
                                .borrow()
                                .get(&id)
                                .and_then(|state| state.handler.as_deref())
                        {
                            view.send(
                                handler,
                                Some(&serde_json::json!({ "value": entity.read(cx).text })),
                            );
                        }
                    })
                    .detach();
                    SearchState {
                        field: entity,
                        handler: field.on_input.clone(),
                    }
                });
                state.handler = field.on_input.clone();
                let entity = state.field.clone();
                div()
                    .min_w_0()
                    .border_1()
                    .border_color(rgba(ui::HAIRLINE))
                    .rounded(px(6.0))
                    .child(entity)
                    .into_any_element()
            }
            Node::Input(input) => self.input(input, cx),
            Node::Icon(icon) => crate::icons::icon(
                match icon.name {
                    tree::IconName::Check => crate::icons::Icon::Check,
                    tree::IconName::ChevronDown => crate::icons::Icon::ChevronDown,
                    tree::IconName::Copy => crate::icons::Icon::Copy,
                    tree::IconName::Trash => crate::icons::Icon::Trash,
                },
                16.0,
                theme::FOREGROUND,
            )
            .into_any_element(),
            Node::Graph(graph) => self.graph(graph, cx),
            Node::Avatar(avatar) => rich_view::avatar(avatar),
            Node::Skeleton(skeleton) => rich_view::skeleton(skeleton),
            Node::Sunburst(map) => self.sunburst(map, cx),
        }
    }

    fn input(&self, input: &tree::Input, cx: &mut Context<Self>) -> AnyElement {
        let mut inputs = self.inputs.borrow_mut();
        let state = inputs.entry(input.id).or_insert_with(|| {
            let entity = cx.new(|cx| {
                let role = if input.rows.is_some() {
                    Role::MultilineTextInput
                } else {
                    Role::TextInput
                };
                let mut field = crate::text_field::TextField::new(
                    role,
                    input.label.clone(),
                    input.placeholder.clone(),
                    px(0.0),
                    cx,
                );
                field.fill = true;
                field.rows = input.rows;
                field
            });
            let id = input.id;
            cx.subscribe(&entity, move |view, entity, event, cx| {
                if let crate::text_field::FieldEvent::Submit { .. }
                | crate::text_field::FieldEvent::Cancel = event
                {
                    let handler = view
                        .inputs
                        .borrow()
                        .get(&id)
                        .and_then(|state| state.key_handler.clone());
                    if let Some(handler) = handler {
                        let key = if matches!(event, crate::text_field::FieldEvent::Cancel) {
                            "Escape"
                        } else {
                            "Enter"
                        };
                        let payload = serde_json::json!({"type":"keydown", "key":key});
                        // Escape gives no user activation in a browser either.
                        if key == "Enter" {
                            view.act(&handler, Some(&payload));
                        } else {
                            view.send(&handler, Some(&payload));
                        }
                    }
                    return;
                }
                if !matches!(event, crate::text_field::FieldEvent::Changed) {
                    return;
                }
                let text = entity.read(cx).text.clone();
                let handler = view.inputs.borrow_mut().get_mut(&id).and_then(|state| {
                    let handler = state.handler.clone()?;
                    state.sent.push(text.clone());
                    Some(handler)
                });
                if let Some(handler) = handler {
                    view.send(&handler, Some(&serde_json::json!({ "value": text })));
                }
            })
            .detach();
            InputState {
                field: entity,
                handler: None,
                key_handler: None,
                blur_handler: None,
                initialized: false,
                auto_focus: input.auto_focus,
                select_on_focus: input.select_on_focus,
                blur_subscription: None,
                given: String::new(),
                sent: Vec::new(),
            }
        });
        state.handler = input.on_change.clone();
        state.key_handler = input.on_key_down.clone();
        state.blur_handler = input.on_blur.clone();
        let (disabled, max_chars) = (input.disabled, input.max_length);
        let echo = state.sent.iter().position(|sent| *sent == input.value);
        if let Some(echo) = echo {
            state.sent.drain(..=echo);
        }
        state.field.update(cx, |field, cx| {
            if echo.is_none() && state.given != input.value && field.text != input.value {
                field.text = input.value.clone();
                state.sent.clear();
                cx.notify();
            }
            if field.disabled != disabled || field.max_chars != max_chars {
                (field.disabled, field.max_chars) = (disabled, max_chars);
                cx.notify();
            }
        });
        state.given = input.value.clone();
        state.field.clone().into_any_element()
    }

    fn disclosure(&self, disclosure: &tree::Disclosure, cx: &mut Context<Self>) -> AnyElement {
        let id = disclosure.id;
        let open = disclosure.open.unwrap_or_else(|| self.open.contains(&id));
        let handler = disclosure.on_open_change.clone();
        let trigger = ui::disclosure_trigger(
            ElementId::NamedInteger("page-disclosure".into(), id),
            plain_text(&disclosure.summary),
            open,
            self.inline(&disclosure.summary, cx),
        )
        .on_click(cx.listener(move |view, _, _, cx| {
            cx.stop_propagation();
            if let Some(handler) = &handler {
                view.act(handler, Some(&serde_json::json!({"open":!open})));
            } else {
                view.toggle_disclosure(id, cx);
            }
        }));
        div()
            .min_w_0()
            .flex()
            .flex_col()
            .child(trigger)
            .when(open, |el| el.children(self.nodes(&disclosure.body, cx)))
            .into_any_element()
    }

    fn select(&self, select: &tree::Select, cx: &mut Context<Self>) -> AnyElement {
        let id = select.id;
        let open = self.menu == Some(id);
        let name = if select.label.is_empty() {
            "Select"
        } else {
            &select.label
        };
        // Pressed before the menu's own press-outside closes it, so the click knows the menu was open.
        let trigger = ui::select_trigger(
            ElementId::NamedInteger("page-select".into(), id),
            name.to_owned(),
            select.shown().to_owned(),
        )
        .w_full()
        .capture_any_mouse_down(cx.listener(move |view, _, _, _| {
            view.pressed_open = view.menu == Some(id);
        }))
        .on_click(cx.listener(move |view, _, _, cx| view.toggle_menu(id, cx)));
        let entity = cx.entity();
        let measure = canvas(
            |_, _, _| {},
            move |bounds, _, _, cx| {
                entity.update(cx, |view, _| {
                    view.select_widths.insert(id, bounds.size.width);
                });
            },
        )
        .absolute()
        .size_full();
        let menu = open.then(|| {
            // As wide as the select, and no narrower than the desktop's 144px.
            let width = (self.select_widths.get(&id).copied().unwrap_or_default()).max(px(144.0));
            let current = select.current();
            let items = select.options.iter().enumerate().map(|(at, option)| {
                let item = ui::select_item(
                    ElementId::NamedInteger(format!("page-select-{id}-item").into(), at as u64),
                    option.label.clone(),
                    current == Some(option.value.as_str()),
                );
                if option.disabled {
                    return item.opacity(0.5).cursor_default().into_any_element();
                }
                let (on_change, value) = (select.on_change.clone(), option.value.clone());
                item.on_click(
                    cx.listener(move |view, _, _, cx| view.pick(on_change.as_deref(), &value, cx)),
                )
                .into_any_element()
            });
            ui::popover(
                ui::select_menu(items)
                    .w(width)
                    .on_mouse_down_out(cx.listener(|view, _, _, cx| {
                        view.menu = None;
                        cx.notify();
                    })),
            )
        });
        div()
            .relative()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .child(measure)
            .child(trigger)
            .children(menu)
            .into_any_element()
    }
}

impl Render for PageView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = self
            .tree
            .as_ref()
            .map(|tree| self.nodes(&tree.children, cx))
            .unwrap_or_default();
        let fill = self.tree.as_ref().is_some_and(|tree| tree.fill);
        for (id, state) in self.inputs.borrow_mut().iter_mut() {
            if !state.initialized {
                state.initialized = true;
                let focus = state.field.read(cx).focus_handle(cx);
                if state.auto_focus {
                    if state.select_on_focus {
                        state
                            .field
                            .update(cx, |field, cx| field.select_all(window, cx));
                    } else {
                        focus.focus(window, cx);
                    }
                }
                let id = *id;
                state.blur_subscription = Some(cx.on_blur(&focus, window, move |view, _, _| {
                    let handler = view
                        .inputs
                        .borrow()
                        .get(&id)
                        .and_then(|state| state.blur_handler.clone());
                    if let Some(handler) = handler {
                        view.send(&handler, Some(&serde_json::json!({"type":"blur"})));
                    }
                }));
            }
        }
        div()
            .id("native-page")
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            // The desktop's page is transparent on the `bg-surface` of the card, panel or window that holds it.
            .bg(rgb(theme::SURFACE))
            .text_color(rgb(theme::FOREGROUND))
            .text_size(px(13.0))
            // plugin-ui.css's body line height, so rows come out as tall as the desktop's.
            .line_height(relative(1.45))
            .child(
                // plugin-ui.css's `.w-shell`: a page is padded and spaced, unless it fills the frame and lays itself
                // out edge to edge (`.w-shell[data-fill]`).
                div()
                    .id("page-shell")
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .when(!fill, |shell| shell.gap(px(10.0)).p(px(14.0)))
                    .children(body),
            )
            .when_some(self.error.clone(), |page, error| {
                page.child(
                    div()
                        .id("page-error")
                        .role(Role::Alert)
                        .aria_label(error.clone())
                        .flex_none()
                        .px(px(14.0))
                        .py(px(8.0))
                        .border_t_1()
                        .border_color(rgba(ui::HAIRLINE))
                        .text_size(px(12.0))
                        .text_color(rgb(theme::DANGER))
                        .child(error),
                )
            })
    }
}

/// Only shortcuts used by Issues cross the page boundary. Ordinary editing/navigation stays with the native control.
fn send_shortcut(
    view: &PageView,
    handler: &str,
    event: &KeyDownEvent,
    window: &mut Window,
    cx: &mut Context<PageView>,
) -> bool {
    let key = match event.keystroke.key.as_str() {
        "left" if event.keystroke.modifiers.alt => "ArrowLeft",
        "right" if event.keystroke.modifiers.alt => "ArrowRight",
        "delete" | "backspace" => "Delete",
        _ => return false,
    };
    let modifiers = event.keystroke.modifiers;
    view.act(handler, Some(&serde_json::json!({"type":"keydown", "key":key, "altKey":modifiers.alt, "metaKey":modifiers.platform, "ctrlKey":modifiers.control, "shiftKey":modifiers.shift})));
    window.prevent_default();
    cx.stop_propagation();
    true
}

fn gap(gap: Gap) -> f32 {
    match gap {
        Gap::None => 0.0,
        Gap::Xs => 2.0,
        Gap::S => 4.0,
        Gap::M => 8.0,
        Gap::L => 12.0,
    }
}

fn tone_color(tone: Tone) -> u32 {
    match tone {
        Tone::Muted => theme::MUTED_FOREGROUND,
        Tone::Info => theme::INFO,
        Tone::Success => theme::DONE,
        Tone::Warn => theme::WORKING,
        Tone::Danger => theme::DANGER,
    }
}

/// A gauge's bar is `--wings-info` until it reaches a level.
pub(super) fn level_color(level: Level) -> u32 {
    match level {
        Level::Normal => theme::INFO,
        Level::Warn => tone_color(Tone::Warn),
        Level::Danger => tone_color(Tone::Danger),
    }
}

fn variant(variant: tree::Variant) -> ui::Variant {
    match variant {
        tree::Variant::Default => ui::Variant::Default,
        tree::Variant::Secondary => ui::Variant::Secondary,
        tree::Variant::Outline => ui::Variant::Outline,
        tree::Variant::Ghost => ui::Variant::Ghost,
        tree::Variant::Destructive => ui::Variant::Destructive,
        tree::Variant::Link => ui::Variant::Link,
    }
}

/// A button at its `size`, as plugin-ui.css's `.w-button[data-size]`: `xs` and `sm` are smaller with smaller text, the
/// `icon` sizes square.
fn sized(button: gpui::Stateful<gpui::Div>, size: tree::ButtonSize) -> gpui::Stateful<gpui::Div> {
    use tree::ButtonSize as Size;
    match size {
        Size::Default => button,
        Size::Xs => button
            .h(px(24.0))
            .gap(px(4.0))
            .px(px(8.0))
            .rounded(px(8.0))
            .text_size(px(12.0)),
        Size::Sm => button
            .h(px(28.0))
            .gap(px(4.0))
            .px(px(10.0))
            .rounded(px(8.0))
            .text_size(px(12.8)),
        Size::Lg => button.h(px(36.0)),
        Size::Icon => button.w(px(32.0)).px_0(),
        Size::IconXs => button.size(px(24.0)).px_0().rounded(px(8.0)),
        Size::IconSm => button.size(px(28.0)).px_0().rounded(px(8.0)),
        Size::IconLg => button.size(px(36.0)).px_0(),
    }
}
