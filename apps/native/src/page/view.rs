//! Draws a page's tree with the UI kit, in the look of the desktop's plugin pages, and sends its clicks back.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use gpui::{
    AnyElement, Context, ElementId, FontWeight, IntoElement, Pixels, Render, Role, SharedString,
    Task, Window, canvas, div, prelude::*, px, relative, rgb, rgba,
};

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
    /// What the page last threw, shown under it until it renders again.
    error: Option<SharedString>,
    _events: Task<()>,
}

impl PageView {
    /// Starts the page bundle `source`; its first tree is drawn when it arrives.
    pub fn new(source: &str, cx: &mut Context<Self>) -> Self {
        Self::start(cx, |on_event| PageRuntime::spawn(source, on_event))
    }

    /// Opens `source` as a page of the plugin `host` runs, which can talk to its main and its other pages.
    pub fn in_plugin(host: &PluginHost, source: &str, cx: &mut Context<Self>) -> Self {
        Self::start(cx, |on_event| host.open_page(source, on_event))
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
            error,
            _events: events,
        }
    }

    fn send(&self, handler: &str, payload: Option<&serde_json::Value>) {
        if let Some(runtime) = &self.runtime {
            runtime.dispatch(handler, payload);
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
            self.send(handler, Some(&serde_json::json!({ "value": value })));
        }
        cx.notify();
    }

    #[cfg(all(test, feature = "test-support"))]
    pub(crate) fn text(&self) -> String {
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

    fn nodes(&self, nodes: &[Node], cx: &mut Context<Self>) -> Vec<AnyElement> {
        nodes.iter().map(|node| self.node(node, cx)).collect()
    }

    /// What `nodes` say as one piece of text, or as inline parts when they hold more than strings.
    fn inline(&self, nodes: &[Node], cx: &mut Context<Self>) -> AnyElement {
        if nodes.iter().all(|node| matches!(node, Node::Str { .. })) {
            return plain_text(nodes).into_any_element();
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
            Node::Text(text) => div()
                .when(text.grow || text.truncate, |el| el.min_w_0())
                .when(text.grow, |el| el.flex_1())
                .when(text.truncate, |el| el.truncate())
                .when(text.clamp, |el| el.line_clamp(3))
                .when(text.mono, |el| el.font_family(DEFAULT_FONT_FAMILY))
                .map(|el| match text.size {
                    TextSize::Normal => el,
                    TextSize::Small => el.text_size(px(12.0)),
                    TextSize::Tiny => el.text_size(px(11.0)),
                })
                .map(|el| match text.weight {
                    Weight::Normal => el,
                    Weight::Medium => el.font_weight(FontWeight::MEDIUM),
                    Weight::Strong => el.font_weight(FontWeight::SEMIBOLD),
                })
                .when_some(text.tone, |el, tone| el.text_color(rgb(tone_color(tone))))
                .child(self.inline(&text.children, cx))
                .into_any_element(),
            Node::Button(button) => {
                let element = ui::button(
                    ElementId::NamedInteger("page-button".into(), button.id),
                    plain_text(&button.children),
                    variant(button.variant),
                    button.disabled,
                    self.inline(&button.children, cx),
                );
                match button.on_click.clone() {
                    Some(handler) => element
                        .on_click(cx.listener(move |view, _, _, _| view.send(&handler, None)))
                        .into_any_element(),
                    None => element.into_any_element(),
                }
            }
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
            Node::Dot(dot) => ui::dot(dot.tone.map_or(theme::DONE, tone_color)).into_any_element(),
            Node::Gauge(gauge) => ui::gauge(
                ElementId::NamedInteger("page-gauge".into(), gauge.id),
                gauge.label.clone(),
                gauge.percent,
                level_color(gauge.level()),
                (!gauge.note.is_empty()).then(|| self.inline(&gauge.note, cx)),
            )
            .into_any_element(),
            Node::List(list) => ui::list(self.nodes(&list.children, cx)).into_any_element(),
            Node::ListItem(item) => ui::list_item(
                ElementId::NamedInteger("page-list-item".into(), item.id),
                self.nodes(&item.children, cx),
            )
            .into_any_element(),
            Node::Disclosure(disclosure) => self.disclosure(disclosure, cx),
            Node::Select(select) => self.select(select, cx),
        }
    }

    fn disclosure(&self, disclosure: &tree::Disclosure, cx: &mut Context<Self>) -> AnyElement {
        let id = disclosure.id;
        let open = self.open.contains(&id);
        let trigger = ui::disclosure_trigger(
            ElementId::NamedInteger("page-disclosure".into(), id),
            plain_text(&disclosure.summary),
            open,
            self.inline(&disclosure.summary, cx),
        )
        .on_click(cx.listener(move |view, _, _, cx| view.toggle_disclosure(id, cx)));
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = self
            .tree
            .as_ref()
            .map(|tree| self.nodes(&tree.children, cx))
            .unwrap_or_default();
        div()
            .id("native-page")
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(rgb(theme::BACKGROUND))
            .text_color(rgb(theme::FOREGROUND))
            .text_size(px(13.0))
            // plugin-ui.css's body line height, so rows come out as tall as the desktop's.
            .line_height(relative(1.45))
            .children(body)
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
