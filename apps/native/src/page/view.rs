//! Draws a page's tree with the UI kit, in the look of the desktop's plugin pages, and sends its clicks back.

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use gpui::{
    Animation, AnimationExt, AnyElement, Context, ElementId, Focusable, FontWeight, IntoElement,
    KeyDownEvent, Pixels, Render, Role, SharedString, Task, Window, canvas, div, prelude::*, px,
    relative, rgb, rgba,
};

#[path = "board_view.rs"]
mod board_view;
#[path = "controls_view.rs"]
mod controls_view;
#[path = "graph_view.rs"]
mod graph_view;
#[path = "rich_view.rs"]
mod rich_view;
#[path = "sunburst_view.rs"]
mod sunburst_view;

use super::{
    plugin::{PageOpener, PluginHost, RESTART_FIRST, RESTART_MOST, RESTART_RESET},
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
    /// The error was the page's last: it has stopped.
    stopped: bool,
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
    open_on_focus: bool,
    blur_subscription: Option<gpui::Subscription>,
    /// The text the page last gave it. The field takes the page's text when this changes, which is when the page set it
    /// (a form filled in once it loaded), and not on every render, which would undo a key the page hasn't heard yet.
    given: String,
    /// The texts sent to the page that it hasn't given back yet. A page that is behind gives back each of them in turn,
    /// and taking one of those would undo the keys typed since.
    sent: Vec<String>,
}

/// How the page is started, kept to start it again after it stopped.
enum Source {
    /// A bundle with no plugin behind it.
    Bare(String),
    Plugin(PageOpener),
}

impl Source {
    fn open(&self, on_event: Box<dyn FnMut(Event) + Send>) -> std::io::Result<PageRuntime> {
        match self {
            Source::Bare(source) => PageRuntime::spawn(source, on_event),
            Source::Plugin(opener) => opener.open(on_event),
        }
    }

    /// Whether the plugin behind the page is gone, so the page is not started again.
    fn gone(&self) -> bool {
        matches!(self, Source::Plugin(opener) if opener.gone())
    }

    /// Records why the page stopped where the installed app keeps it.
    fn log(&self, message: &str) {
        if let Source::Plugin(opener) = self {
            super::log::line(opener.plugin(), opener.name(), message);
        }
    }
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
    /// Where each select and combobox was drawn, by node id, and how tall the window was: its menu is as wide, and as
    /// tall as the room on one side of it.
    select_bounds: HashMap<u64, (gpui::Bounds<Pixels>, Pixels)>,
    /// The text input that has the keyboard, by node id, as of the last render: a combobox's shows its options under
    /// it.
    focused: Option<u64>,
    /// The focused combobox's options were put away, by Escape, a pick or a press outside, until its field changes, is
    /// pressed or takes the keyboard again.
    suggestions_closed: bool,
    /// The option of the focused combobox the arrow keys are on.
    highlight: Option<usize>,
    /// A combobox's options were drawn over the page in the last render.
    listing: Cell<bool>,
    fields: RefCell<HashMap<u64, SearchState>>,
    inputs: RefCell<HashMap<u64, InputState>>,
    graphs: RefCell<HashMap<u64, graph_view::GraphState>>,
    sunbursts: RefCell<HashMap<u64, sunburst_view::SunburstState>>,
    chip_focus: RefCell<HashMap<(u64, String), gpui::FocusHandle>>,
    /// What the page last threw, shown under it until it renders again.
    error: Option<SharedString>,
    /// The page has stopped: nothing in it answers a click until it is started again, which happens by itself
    /// (`restart_later`) or with the Reload by its error.
    stopped: bool,
    source: Source,
    /// Whether the page was last said to be in view, to tell the next runtime the same.
    shown: Cell<Option<bool>>,
    /// When the runtime that runs now was started, and how long the next restart waits: `RESTART_FIRST`, then twice
    /// as long after each short run up to `RESTART_MOST`, as a plugin's main (`PluginHost::keep_main`).
    started: Instant,
    restart_wait: Duration,
    _restart: Option<Task<()>>,
    /// Shown in a title bar button's popover (`place: "popover"`), whose shell and ground are the popover's.
    popover: bool,
    /// Where the page's shell is scrolled, so a field that takes the keyboard can be brought into view.
    shell_scroll: gpui::ScrollHandle,
    /// Where the open combobox's list is scrolled, so the option the arrow keys reach is brought into view.
    combobox_scroll: gpui::ScrollHandle,
    /// A focus handle round each list item's `actions`, by node id, and the items one of whose actions had the
    /// keyboard when the page was last drawn: those show their actions without the pointer on them.
    actions_focus: RefCell<HashMap<u64, gpui::FocusHandle>>,
    actions_within: HashSet<u64>,
    /// A field that took the keyboard and is brought into view once it has been drawn.
    reveal: Option<gpui::Entity<crate::text_field::TextField>>,
    _events: Task<()>,
}

impl PageView {
    /// Whether a select's menu or a combobox's options are open over the page. They are drawn above everything else
    /// and can reach past the page's own bounds, so a press on one may land outside the frame that holds the page.
    pub fn shows_menu(&self) -> bool {
        self.menu.is_some() || self.listing.get()
    }

    pub fn set_visible(&self, visible: bool) {
        self.shown.set(Some(visible));
        if let Some(runtime) = &self.runtime {
            runtime.set_visible(visible);
        }
    }

    /// Whether the page has stopped and not started again yet.
    #[cfg(feature = "test-support")]
    pub fn stopped(&self) -> bool {
        self.stopped
    }

    /// Starts a stopped page again now, as the Reload by its error does.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self._restart = None;
        self.restart_wait = RESTART_FIRST;
        self.restart(cx);
    }

    /// Starts the page from its beginning in place of the runtime that stopped. What the page drew and what was open
    /// or typed in it go with the old one.
    fn restart(&mut self, cx: &mut Context<Self>) {
        let (runtime, error, events) = Self::launch(&self.source, cx);
        if let (Some(runtime), Some(shown)) = (&runtime, self.shown.get()) {
            runtime.set_visible(shown);
        }
        self.stopped = runtime.is_none();
        (self.runtime, self.error, self._events) = (runtime, error, events);
        self.started = Instant::now();
        self.tree = None;
        self.open.clear();
        self.menu = None;
        self.pressed_open = false;
        self.select_bounds.clear();
        self.focused = None;
        self.suggestions_closed = false;
        self.highlight = None;
        self.fields.borrow_mut().clear();
        self.inputs.borrow_mut().clear();
        self.graphs.borrow_mut().clear();
        self.sunbursts.borrow_mut().clear();
        self.chip_focus.borrow_mut().clear();
        self.actions_focus.borrow_mut().clear();
        self.actions_within.clear();
        self.reveal = None;
        if self.stopped {
            self.restart_later(cx);
        }
        cx.notify();
    }

    /// Starts the stopped page again after a wait, so it comes back without a click. Not once its plugin is gone.
    fn restart_later(&mut self, cx: &mut Context<Self>) {
        if self.source.gone() {
            return;
        }
        let wait = if self.started.elapsed() >= RESTART_RESET { RESTART_FIRST } else { self.restart_wait };
        self.restart_wait = (wait * 2).min(RESTART_MOST);
        self._restart = Some(cx.spawn(async move |view, cx| {
            cx.background_executor().timer(wait).await;
            let _ = view.update(cx, |view, cx| view.restart(cx));
        }));
    }

    /// Starts the page bundle `source`; its first tree is drawn when it arrives.
    pub fn new(source: &str, cx: &mut Context<Self>) -> Self {
        Self::start(cx, Source::Bare(source.into()))
    }

    /// Opens `source` as a page of the plugin `host` runs, which can talk to its main and its other pages.
    pub fn in_plugin(host: &PluginHost, source: &str, cx: &mut Context<Self>) -> Self {
        Self::start(cx, Source::Plugin(host.page(false, 0, None, None, source)))
    }

    /// Like `in_plugin`, for a page shown in the popover under its sidebar's title bar button.
    pub fn in_plugin_popover(host: &PluginHost, source: &str, cx: &mut Context<Self>) -> Self {
        Self {
            popover: true,
            ..Self::in_plugin(host, source, cx)
        }
    }

    /// A bare page drawn as a popover's, for tests.
    #[cfg(test)]
    pub fn new_popover(source: &str, cx: &mut Context<Self>) -> Self {
        Self {
            popover: true,
            ..Self::new(source, cx)
        }
    }

    /// Like `in_plugin`, for a tab `wings.openView` opened with `args`, which the page reads with `viewArgs`.
    pub fn in_plugin_with_args(
        host: &PluginHost,
        source: &str,
        args: Option<&serde_json::Value>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::start(cx, Source::Plugin(host.page(false, 0, args, None, source)))
    }

    /// Like `in_plugin_with_args`, for a tab or block whose `wings.tabState`/`blockState` is `state`.
    pub fn in_plugin_with_state(
        host: &PluginHost,
        source: &str,
        args: Option<&serde_json::Value>,
        state: crate::page::plugin::FrameState,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::start(cx, Source::Plugin(host.page(false, 0, args, Some(state), source)))
    }

    pub fn in_plugin_window(
        host: &PluginHost,
        window_id: u64,
        source: &str,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::start(cx, Source::Plugin(host.page(true, window_id, None, None, source)))
    }

    /// Starts the page's runtime: the runtime, or why it didn't start, and the task that takes what it sends.
    fn launch(source: &Source, cx: &mut Context<Self>) -> (Option<PageRuntime>, Option<SharedString>, Task<()>) {
        let pending = Arc::new(Mutex::new(Pending::default()));
        let (wake, woken) = async_channel::bounded::<()>(1);
        let runtime = source.open(Box::new({
            let pending = pending.clone();
            move |event| {
                {
                    let mut pending = pending.lock().unwrap();
                    match event {
                        Event::Tree(tree) => pending.tree = Some(tree),
                        Event::Error { message, fatal } => {
                            pending.error = Some(message);
                            pending.stopped |= fatal;
                        }
                    }
                }
                let _ = wake.try_send(());
            }
        }));
        let (runtime, error) = match runtime {
            Ok(runtime) => (Some(runtime), None),
            Err(error) => {
                let message = format!("the page didn't start: {error}");
                source.log(&message);
                (None, Some(message.into()))
            }
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
                            if taken.stopped {
                                view.source.log(&error);
                            }
                            view.error = Some(error.into());
                        }
                        if taken.stopped {
                            view.stopped = true;
                            view.restart_later(cx);
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        (runtime, error, events)
    }

    fn start(cx: &mut Context<Self>, source: Source) -> Self {
        let (runtime, error, events) = Self::launch(&source, cx);
        let mut view = Self {
            stopped: runtime.is_none(),
            runtime,
            tree: None,
            open: HashSet::new(),
            menu: None,
            pressed_open: false,
            select_bounds: HashMap::new(),
            focused: None,
            suggestions_closed: false,
            highlight: None,
            listing: Cell::new(false),
            fields: RefCell::new(HashMap::new()),
            inputs: RefCell::new(HashMap::new()),
            graphs: RefCell::new(HashMap::new()),
            sunbursts: RefCell::new(HashMap::new()),
            chip_focus: RefCell::new(HashMap::new()),
            error,
            source,
            shown: Cell::new(None),
            started: Instant::now(),
            restart_wait: RESTART_FIRST,
            _restart: None,
            popover: false,
            shell_scroll: gpui::ScrollHandle::new(),
            combobox_scroll: gpui::ScrollHandle::new(),
            actions_focus: RefCell::default(),
            actions_within: HashSet::new(),
            reveal: None,
            _events: events,
        };
        if view.stopped {
            view.restart_later(cx);
        }
        view
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

    /// The person picked a combobox's option, whose chip's click is `handler`: the page hears that click, and the list
    /// goes away until the field changes again.
    fn pick_suggestion(&mut self, handler: &str, cx: &mut Context<Self>) {
        self.suggestions_closed = true;
        self.highlight = None;
        self.act(handler, None);
        cx.notify();
    }

    /// Presses the `nth` button labelled `label`, by its text or an icon button's name, as a click would. False when
    /// the page has no such button.
    #[cfg(feature = "test-support")]
    pub fn press(&self, label: &str, nth: usize) -> bool {
        fn find<'a>(nodes: &'a [Node], label: &str, found: &mut Vec<&'a str>) {
            for node in nodes {
                if let Node::Button(button) = node
                    && (plain_text(&button.children).trim() == label
                        || button.label.as_deref() == Some(label))
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

    /// Turns on the `Chip` that shows `label`, as a click on it would, unless it's on already. False when the page has
    /// no such chip.
    #[cfg(feature = "test-support")]
    pub fn turn_on_chip(&self, label: &str) -> bool {
        fn find<'a>(nodes: &'a [Node], label: &str) -> Option<&'a tree::Button> {
            nodes.iter().find_map(|node| match node {
                Node::Button(button)
                    if button.chip && plain_text(&button.children).starts_with(label) =>
                {
                    Some(button)
                }
                other => other.parts().into_iter().find_map(|part| find(part, label)),
            })
        }
        let found = self
            .tree
            .as_ref()
            .and_then(|tree| find(&tree.children, label));
        found.is_some_and(|chip| {
            if let (false, Some(handler)) = (chip.pressed, &chip.on_click) {
                self.act(handler, None);
            }
            true
        })
    }

    /// Opens the menu of the select named `label`, or showing it, as a click on it would. False when the page has no
    /// such select.
    #[cfg(feature = "test-support")]
    pub fn open_select(&mut self, label: &str, cx: &mut Context<Self>) -> bool {
        fn find(nodes: &[Node], label: &str) -> Option<u64> {
            nodes.iter().find_map(|node| match node {
                Node::Select(select) if select.label == label || select.shown() == label => {
                    Some(select.id)
                }
                other => other.parts().into_iter().find_map(|part| find(part, label)),
            })
        }
        let found = self
            .tree
            .as_ref()
            .and_then(|tree| find(&tree.children, label));
        if let Some(id) = found {
            self.menu = Some(id);
            cx.notify();
        }
        found.is_some()
    }

    /// Scrolls the page's shell as little as brings `field` wholly into view, with the shell's 14pt padding around it.
    /// False until the field has been drawn, when there is nothing yet to scroll to.
    fn reveal(
        &self,
        field: &gpui::Entity<crate::text_field::TextField>,
        cx: &mut Context<Self>,
    ) -> bool {
        let shown = self.shell_scroll.bounds();
        let at = field.read(cx).bounds();
        if at.size.height <= px(0.0) || shown.size.height <= px(0.0) {
            return false;
        }
        let offset = self.shell_scroll.offset();
        let shift = if at.bottom() + px(14.0) > shown.bottom() {
            shown.bottom() - at.bottom() - px(14.0)
        } else if at.top() - px(14.0) < shown.top() {
            shown.top() - at.top() + px(14.0)
        } else {
            return true;
        };
        self.shell_scroll
            .set_offset(gpui::point(offset.x, (offset.y + shift).min(px(0.0))));
        cx.notify();
        true
    }

    /// How far the page's shell is scrolled down, in points.
    #[cfg(test)]
    pub(super) fn scrolled(&self) -> f32 {
        -self.shell_scroll.offset().y.as_f32()
    }

    /// The keyboard focus of the page's first text input, for the screenshots to type into.
    #[cfg(feature = "test-support")]
    pub fn first_input_focus(&self, cx: &gpui::App) -> Option<gpui::FocusHandle> {
        let inputs = self.inputs.borrow();
        let first = inputs.keys().min()?;
        Some(inputs[first].field.read(cx).focus_handle(cx))
    }

    /// The focus of the last text input named `label`, as the page drew it: an entry's edit form comes after the start
    /// card's fields of the same names.
    #[cfg(feature = "test-support")]
    pub fn input_focus(&self, label: &str, cx: &gpui::App) -> Option<gpui::FocusHandle> {
        fn find(nodes: &[Node], label: &str, found: &mut Option<u64>) {
            for node in nodes {
                match node {
                    Node::Input(input) if input.label == label => *found = Some(input.id),
                    other => {
                        for part in other.parts() {
                            find(part, label, found);
                        }
                    }
                }
            }
        }
        let mut found = None;
        find(&self.tree.as_ref()?.children, label, &mut found);
        let inputs = self.inputs.borrow();
        Some(inputs.get(&found?)?.field.read(cx).focus_handle(cx))
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

    /// Whether the drawn graph's rows show their author and their date, as its width allows.
    #[cfg(all(test, feature = "test-support"))]
    pub(super) fn graph_columns(&self) -> Option<(bool, bool)> {
        use graph_view::Columns;
        let columns = self.graphs.borrow().values().next()?.columns;
        Some((columns == Columns::All, columns != Columns::NoAuthorOrDate))
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

    /// What a button, chip or badge holds, laid out as the desktop's `inline-flex` box lays out its children: each
    /// component in it is an item of its own, centred, `gap` points from the next, and a run of plain strings is one
    /// item. Only strings are one line of text. A dot drawn as a word of a line sat at the line's foot against the word
    /// after it ("•Active"), and an id and a title ran together ("WEB-10Checkout redesign").
    fn contents(&self, nodes: &[Node], gap: f32, cx: &mut Context<Self>) -> AnyElement {
        if nodes.iter().all(|node| matches!(node, Node::Str { .. })) {
            return self.inline(nodes, cx);
        }
        div()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(gap))
            .children(self.items(nodes, cx))
            .into_any_element()
    }

    /// The items of a flex box, as CSS makes them: each component one, and a run of plain strings one between them. A
    /// row of "Updated ", "14 h ago" and buttons spread apart put the gap of a row between the two words.
    fn items(&self, nodes: &[Node], cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut items = Vec::new();
        let mut run: Vec<Node> = Vec::new();
        for node in nodes {
            if matches!(node, Node::Str { .. }) {
                run.push(node.clone());
                continue;
            }
            if !run.is_empty() {
                items.push(
                    div()
                        .child(plain_text(&std::mem::take(&mut run)))
                        .into_any_element(),
                );
            }
            items.push(self.node(node, cx));
        }
        if !run.is_empty() {
            items.push(div().child(plain_text(&run)).into_any_element());
        }
        items
    }

    /// A `Text`. In a baseline row it has a `drop`: its first line starts that many points down, onto the row's
    /// baseline, and it narrows to its longest word and wraps when the row is short of room, as a CSS flex item does
    /// even when it doesn't grow. GPUI takes a text's narrowest as its whole line, so an issue's long title ran over the
    /// key and the Edit after it.
    fn text_element(
        &self,
        text: &tree::Text,
        drop: Option<f32>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // CSS breaks a line between words, and lets a word wider than its box run over it; GPUI breaks inside a
        // word when it has to. A text that grows in a row, squeezed by the buttons beside it, would come apart a
        // letter or two a line, so it is never narrower than its longest word, as in the desktop. A mono text that
        // grows is a line of code or of a log, which the desktop sets in a box that scrolls sideways and the app
        // can't scroll: it breaks inside a word wider than its box instead, or a log's "=====" rule and a long link
        // ran past the card's edge.
        let code = text.mono && text.grow;
        let least = ((text.grow || drop.is_some()) && !text.truncate && !code).then(|| {
            let size = match text.size {
                TextSize::Small => 12.0,
                TextSize::Tiny => 11.0,
                TextSize::Large => 20.0,
                TextSize::Field => 14.0,
                TextSize::Normal => 13.0,
            };
            let weight = match text.weight {
                Weight::Normal => FontWeight::NORMAL,
                Weight::Medium => FontWeight::MEDIUM,
                Weight::Strong => FontWeight::SEMIBOLD,
            };
            longest_word(&plain_text(&text.children), size, weight, text.mono, cx)
        });
        let element = div()
            .when_some(drop.filter(|drop| *drop > 0.0), |el, drop| el.pt(px(drop)))
            .when(text.truncate || code, |el| el.min_w_0())
            .when_some(least, |el, least| el.min_w(px(least)))
            .when(text.grow, |el| el.flex_1())
            .when(text.truncate, |el| el.truncate())
            .when(text.clamp, three_lines)
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
            // A text with nothing in it takes no line, as an empty span takes none in CSS: PR tracker's "A
            // token is saved." left a blank line over the field while it was still asking.
            .when(
                text.children
                    .iter()
                    .any(|node| !matches!(node, Node::Str { text, .. } if text.is_empty())),
                |el| el.child(self.inline(&text.children, cx)),
            );
        if text.on_click.is_none() && text.title.is_none() {
            return element.into_any_element();
        }
        // A tooltip needs an id to hang on.
        let element = element
            .id(ElementId::NamedInteger("page-text".into(), text.id))
            .when_some(text.title.clone(), |el, title| {
                el.aria_description(title.clone())
                    .tooltip(ui::tooltip(title, None))
            });
        match text.on_click.clone() {
            Some(handler) => element
                .cursor_pointer()
                .text_color(rgb(text.tone.map_or(theme::INFO, tone_color)))
                .on_click(cx.listener(move |view, _, _, _| view.act(&handler, None)))
                .into_any_element(),
            None => element.into_any_element(),
        }
    }

    /// A `Button`, or a chip. In a baseline row it starts `drop` points down, as a text there does.
    fn button(&self, button: &tree::Button, drop: f32, cx: &mut Context<Self>) -> AnyElement {
        let id = ElementId::NamedInteger("page-button".into(), button.id);
        let label = button
            .label
            .clone()
            .unwrap_or_else(|| plain_text(&button.children));
        // plugin-ui.css's gap between a button's parts: 6px, and 4px for a chip and a small button.
        let between =
            if button.chip || matches!(button.size, tree::ButtonSize::Xs | tree::ButtonSize::Sm) {
                4.0
            } else {
                6.0
            };
        let content = if button.wrap
            && !button
                .children
                .iter()
                .all(|node| matches!(node, Node::Str { .. }))
        {
            // Each text never narrower than its longest word, as in a baseline row: GPUI took a title's narrowest as
            // its whole line. The box round them sets no least width of its own, or it would be the whole line's and
            // the title would run on past the row instead of wrapping.
            div()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(between))
                .children(button.children.iter().map(|node| match node {
                    Node::Text(text) => self.text_element(text, Some(0.0), cx),
                    node => self.node(node, cx),
                }))
                .into_any_element()
        } else {
            self.contents(&button.children, between, cx)
        };
        let element = if button.chip {
            ui::chip(id, label, button.pressed, button.disabled, content)
        } else {
            sized(
                ui::button(id, label, variant(button.variant), button.disabled, content),
                button.size,
            )
        };
        let element = element
            // A button that holds a text cut short narrows to fit its row and cuts that text, as each desktop page
            // with one makes it do in its own CSS (Issues' linked issues and parent lanes, Disk usage's folders): an
            // issue linked by a long title ran past the properties column and the window's edge.
            .when(
                button
                    .children
                    .iter()
                    .any(|node| matches!(node, Node::Text(text) if text.truncate)),
                |el| el.flex_shrink_1().min_w_0(),
            )
            .when(button.wrap, |el| {
                el.flex_shrink_1()
                    .min_w_0()
                    .h_auto()
                    .py(px(WRAPPED_PADDING))
                    .justify_start()
                    .whitespace_normal()
            })
            .when(drop > 0.0, |el| el.mt(px(drop)))
            .when_some(button.title.clone(), |el, title| {
                el.aria_description(title.clone())
                    .tooltip(ui::tooltip(title, None))
            });
        let key_handler = button.on_key_down.clone();
        let element =
            element.on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
                if let Some(handler) = &key_handler {
                    send_shortcut(view, handler, event, window, cx);
                }
            }));
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
                    .children(page.body.iter().map(|node| match node {
                        Node::Board(board) if page.flush => self.board(board, true, cx),
                        node => self.node(node, cx),
                    }));
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
            Node::Toolbar(toolbar) => self.toolbar(toolbar, cx),
            Node::Stack(stack) => div()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(gap(stack.gap)))
                .when(stack.grow, |el| el.flex_1())
                .when_some(stack.width, |el, width| el.w(px(width)).flex_none())
                .when_some(stack.max_width, |el, width| {
                    el.w_full().max_w(px(width)).mx_auto()
                })
                .children(self.items(&stack.children, cx))
                .into_any_element(),
            Node::Row(row) => self.row(row, cx),
            Node::Text(text) => self.text_element(text, None, cx),
            Node::Button(button) => self.button(button, 0.0, cx),
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
            Node::Checkbox(checkbox) => self.checkbox(checkbox, cx),
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
            Node::Badge(badge) => {
                let pill = ui::badge(
                    variant(badge.variant),
                    badge.tone.map(tone_color),
                    badge.small,
                    self.contents(&badge.children, 4.0, cx),
                );
                let Some(handler) = badge.on_click.clone() else {
                    return pill.into_any_element();
                };
                // A summary chip that opens something, like a pull request's checks: the same pill, a button that
                // lights up under the pointer, as plugin-ui.css's `button.w-summary-chip` does. GPUI clicks a focused
                // one on Enter and Space.
                pill.id(ElementId::NamedInteger("page-badge".into(), badge.id))
                    .role(Role::Button)
                    .aria_label(plain_text(&badge.children))
                    .focusable()
                    .tab_stop(true)
                    .cursor_pointer()
                    .hover(|style| style.bg(rgba(theme::HOVER)))
                    .focus_visible(|style| style.border_color(rgb(theme::FOREGROUND)))
                    .on_click(cx.listener(move |view, _, _, cx| {
                        cx.stop_propagation();
                        view.act(&handler, None);
                    }))
                    .into_any_element()
            }
            Node::Empty(empty) => self.empty(empty, cx),
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
            Node::Heading(heading) => ui::heading(self.inline(&heading.children, cx))
                .when(heading.section, |el| {
                    el.text_size(px(14.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(theme::FOREGROUND))
                })
                .into_any_element(),
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
            Node::Split(split) => self.split(split, cx),
            Node::Gauge(gauge) => ui::gauge(
                ElementId::NamedInteger("page-gauge".into(), gauge.id),
                gauge.label.clone(),
                gauge.percent,
                level_color(gauge.level()),
                (!gauge.note.is_empty()).then(|| self.inline(&gauge.note, cx)),
            )
            .into_any_element(),
            Node::Meter(meter) => board_view::meter(meter),
            Node::Board(board) => self.board(board, false, cx),
            Node::BoardRow(row) => self.board_row(&row.children, cx),
            Node::BoardColumn(column) => self.board_column(column, cx),
            Node::List(list) => ui::list(self.nodes(&list.children, cx)).into_any_element(),
            Node::ListItem(item) => self.list_item(item, cx),
            Node::Disclosure(disclosure) => self.disclosure(disclosure, cx),
            Node::Select(select) => self.select(select, cx),
            Node::Combobox(combobox) => self.combobox(combobox, cx),
            Node::Chips(select) => self.chips(select, cx),
            Node::ListButton(item) => self.list_button(item, None, cx),
            Node::SearchField(field) => self.search_field(field, cx),
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
            // GPUI draws an svg as a mask in its text colour, which is the drawing's `currentColor`.
            Node::Svg(drawing) => {
                let image = gpui::svg()
                    .data(drawing.markup.as_bytes())
                    .size(px(drawing.size))
                    .flex_none()
                    .text_color(rgb(drawing.tone.map_or(theme::FOREGROUND, tone_color)));
                match &drawing.label {
                    Some(label) => div()
                        .id(ElementId::NamedInteger("page-svg".into(), drawing.id))
                        .role(Role::Image)
                        .aria_label(label.clone())
                        .flex_none()
                        .child(image)
                        .into_any_element(),
                    None => image.into_any_element(),
                }
            }
            Node::Graph(graph) => self.graph(graph, cx),
            Node::Avatar(avatar) => rich_view::avatar(avatar),
            Node::Skeleton(skeleton) => rich_view::skeleton(skeleton),
            Node::Sunburst(map) => self.sunburst(map, cx),
        }
    }

    fn toolbar(&self, toolbar: &tree::Toolbar, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id(ElementId::NamedInteger("page-toolbar".into(), toolbar.id))
            .role(Role::Toolbar)
            .flex_none()
            .min_h(px(44.0))
            .flex()
            // Too narrow for the title and the actions, the actions go to a line of their own: on one line they ran
            // off the frame's edge, as a 250 point sidebar cut off Changes' Refresh.
            .flex_wrap()
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
            .into_any_element()
    }

    fn row(&self, row: &tree::Row, cx: &mut Context<Self>) -> AnyElement {
        div()
            .min_w_0()
            .flex()
            .gap(px(gap(row.gap)))
            .map(|el| match row.align {
                Align::Center => el.items_center(),
                Align::Start => el.items_start(),
                // GPUI's text tells the layout no baseline, so a baseline row lines up the bottoms of its items'
                // first boxes: a value that starts with a dot, like Properties' "● New", sat under its label, and a
                // value that wraps put its label on its last line, as a log line's number sat by its last line.
                // The row's texts take the tallest of their lines, and each starts as far down as puts its first
                // baseline on the lowest one (`baseline_drops`).
                Align::Baseline => match line_height(&row.children) {
                    Some(height) => el.items_start().line_height(px(height)),
                    None => el.items_baseline(),
                },
            })
            .map(|el| match row.justify {
                Justify::Start => el,
                Justify::End => el.justify_end(),
                Justify::Between => el.justify_between(),
            })
            .when(row.wrap, |el| el.flex_wrap())
            .when(row.grow, |el| el.flex_1())
            .children(match baseline_drops(row, cx) {
                Some(drops) => row
                    .children
                    .iter()
                    .zip(drops)
                    .map(|(node, drop)| match node {
                        Node::Text(text) => self.text_element(text, Some(drop), cx),
                        Node::Button(button) => self.button(button, drop, cx),
                        Node::ListButton(item) => self.list_button(item, Some(drop), cx),
                        other => self.node(other, cx),
                    })
                    .collect(),
                None => self.items(&row.children, cx),
            })
            .into_any_element()
    }

    fn empty(&self, empty: &tree::Empty, cx: &mut Context<Self>) -> AnyElement {
        div()
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
            .into_any_element()
    }

    /// Each pane scrolls on its own, as plugin-ui.css's `.w-split-pane { overflow: auto }`: Disk usage's list of
    /// files ran past the bottom of its tab.
    fn split(&self, split: &tree::Split, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .child(
                div()
                    .id(ElementId::NamedInteger("page-split-start".into(), split.id))
                    .overflow_y_scroll()
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
                    .id(ElementId::NamedInteger("page-split-end".into(), split.id))
                    .overflow_y_scroll()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .children(self.nodes(&split.end, cx)),
            )
            .into_any_element()
    }

    fn list_item(&self, item: &tree::ListItem, cx: &mut Context<Self>) -> AnyElement {
        if item
            .children
            .iter()
            .any(|node| matches!(node, Node::ListButton(_)))
        {
            // A row that opens to a card shows the card under it, as the desktop's block list item does.
            let opened = item
                .children
                .iter()
                .any(|node| matches!(node, Node::Card(_)));
            div()
                .min_w_0()
                .w_full()
                .flex()
                .when(opened, |el| el.flex_col())
                .when(!opened, |el| el.items_center())
                .children(self.nodes(&item.children, cx))
                .into_any_element()
        } else {
            // Its actions show over the row's end while the row is pointed at or one of them has the keyboard, and the
            // row's last part gives way to them, as plugin-ui.css's `.w-list-item-actions`: Cyclops' entries' buttons
            // take the place of their times.
            let has_actions = !item.actions.is_empty();
            let within = self.actions_within.contains(&item.id);
            let actions = has_actions.then(|| {
                let focus = self
                    .actions_focus
                    .borrow_mut()
                    .entry(item.id)
                    .or_insert_with(|| cx.focus_handle())
                    .clone();
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(2.0))
                    .track_focus(&focus)
                    .when(!within, |el| {
                        el.opacity(0.0)
                            .group_hover(LIST_ITEM_GROUP, |style| style.opacity(1.0))
                    })
                    .children(self.nodes(&item.actions, cx))
            });
            let last = item.children.len().saturating_sub(1);
            // A card in a row takes the row's width, as a page's own style would have it do.
            ui::list_item(
                ElementId::NamedInteger("page-list-item".into(), item.id),
                item.children.iter().enumerate().map(|(at, child)| {
                    let element = self.node(child, cx);
                    if matches!(child, Node::Card(_)) {
                        div().flex_1().min_w_0().child(element).into_any_element()
                    } else if has_actions && at == last {
                        div()
                            .min_w_0()
                            .flex()
                            .when(within, |el| el.opacity(0.0))
                            .group_hover(LIST_ITEM_GROUP, |style| style.opacity(0.0))
                            .child(element)
                            .into_any_element()
                    } else {
                        element
                    }
                }),
            )
            .when_some(actions, |row, actions| {
                row.group(LIST_ITEM_GROUP).relative().child(actions)
            })
            .into_any_element()
        }
    }
}

impl Render for PageView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A change of focus redraws every view, cached ones too. A combobox field that has just taken the keyboard
        // shows its options afresh.
        let focused = self
            .inputs
            .borrow()
            .iter()
            .find(|(_, state)| state.field.read(cx).focus_handle(cx).is_focused(window))
            .map(|(id, state)| (*id, state.open_on_focus));
        if focused.map(|(id, _)| id) != self.focused {
            self.focused = focused.map(|(id, _)| id);
            // One the page asked not to open on focus stays shut, as the desktop's: a press in it or typing opens it.
            self.suggestions_closed = focused.is_some_and(|(_, open)| !open);
            self.highlight = None;
        }
        self.listing.set(false);
        self.actions_within = self
            .actions_focus
            .borrow()
            .iter()
            .filter(|(_, focus)| focus.contains_focused(window, cx))
            .map(|(id, _)| *id)
            .collect();
        let body = self
            .tree
            .as_ref()
            .map(|tree| self.nodes(&tree.children, cx))
            .unwrap_or_default();
        let fill = self.tree.as_ref().is_some_and(|tree| tree.fill);
        // A field that took the keyboard in the last render is brought into view once it has been drawn.
        if let Some(field) = self.reveal.clone() {
            if self.reveal(&field, cx) {
                self.reveal = None;
            } else {
                window.request_animation_frame();
            }
        }
        let mut reveal = None;
        for (id, state) in self.inputs.borrow_mut().iter_mut() {
            if !state.initialized {
                state.initialized = true;
                let focus = state.field.read(cx).focus_handle(cx);
                if state.auto_focus {
                    // As a browser scrolls a field it focuses into view: one that opens below the fold, like an entry's
                    // edit form, is brought up once it is drawn.
                    reveal = Some(state.field.clone());
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
        if reveal.is_some() {
            self.reveal = reveal;
            window.request_animation_frame();
        }
        // Names this page in the focused control's key contexts, so Tab can tell when focus has left it.
        let mut page_context = gpui::KeyContext::default();
        page_context.set(PAGE_CONTEXT, cx.entity_id().to_string());
        div()
            .id("native-page")
            .key_context(page_context)
            .on_key_down(cx.listener(Self::tab))
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            // The desktop's page is transparent on the `bg-surface` of the card, panel or window that holds it, or on the
            // `bg-popover` of a popover.
            .bg(rgb(if self.popover {
                ui::DIALOG
            } else {
                theme::SURFACE
            }))
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
                    .when(!fill && !self.popover, |shell| {
                        shell.gap(px(10.0)).p(px(14.0))
                    })
                    // `.w-shell[data-place="popover"]`.
                    .when(!fill && self.popover, |shell| {
                        shell.gap(px(12.0)).pt(px(12.0)).px(px(14.0)).pb(px(14.0))
                    })
                    // A desktop page longer than its frame scrolls in it.
                    .when(!fill, |shell| {
                        shell.overflow_y_scroll().track_scroll(&self.shell_scroll)
                    })
                    .children(body),
            )
            .when_some(self.error.clone(), |page, error| {
                // A page that stopped can't be clicked back to life, so its error comes with a way to load it again.
                let reload = self.stopped.then(|| cx.listener(|view, _: &gpui::ClickEvent, _, cx| view.reload(cx)));
                page.child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(12.0))
                        .px(px(14.0))
                        .py(px(8.0))
                        .border_t_1()
                        .border_color(rgba(ui::HAIRLINE))
                        .child(
                            div()
                                .id("page-error")
                                .role(Role::Alert)
                                .aria_label(error.clone())
                                .flex_1()
                                .min_w_0()
                                .text_size(px(12.0))
                                .text_color(rgb(theme::DANGER))
                                .child(error),
                        )
                        .when_some(reload, |row, reload| {
                            row.child(
                                ui::button("page-reload", "Reload", ui::Variant::Outline, false, "Reload")
                                    .on_click(reload),
                            )
                        }),
                )
            })
    }
}

const PAGE_CONTEXT: &str = "plugin-page";
/// The group a list item with actions names itself, for its actions to show while it is pointed at.
const LIST_ITEM_GROUP: &str = "page-list-item";

impl PageView {
    /// Tab and Shift-Tab go to the page's next and previous control, as in a browser, and round to its first and last
    /// instead of leaving it: a popover's page shares the window with the panes behind it.
    fn tab(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let mods = event.keystroke.modifiers;
        if event.keystroke.key != "tab" || mods.number_of_modifiers() != u8::from(mods.shift) {
            return;
        }
        let page = cx.entity_id().to_string();
        let inside = |window: &Window| {
            window
                .context_stack()
                .iter()
                .any(|context| context.get(PAGE_CONTEXT).is_some_and(|id| **id == page))
        };
        let from = window.focused(cx);
        // Bounded, as a page with no control besides the focused one comes back to it.
        for _ in 0..1000 {
            if mods.shift {
                window.focus_prev(cx);
            } else {
                window.focus_next(cx);
            }
            if inside(window) || window.focused(cx) == from {
                break;
            }
        }
        window.prevent_default();
        cx.stop_propagation();
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
        // An issue's title edits on F2, as a name does in Finder.
        "f2" => "F2",
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

/// A `clamp`ed text: three lines, the last ending in an ellipsis where the text goes on, as `-webkit-line-clamp` ends
/// it. Without the ellipsis the third line ran on to the box's edge and was cut there, mid-word ("keep the tota").
pub(super) fn three_lines<E: Styled>(el: E) -> E {
    el.line_clamp(3).text_ellipsis()
}

/// The font a page's text is drawn in.
fn page_font(weight: FontWeight, mono: bool, cx: &gpui::App) -> gpui::FontId {
    let family = if mono {
        DEFAULT_FONT_FAMILY
    } else {
        ".SystemUIFont"
    };
    cx.text_system().resolve_font(&gpui::Font {
        weight,
        ..gpui::font(family)
    })
}

/// How far down each text of a baseline row of texts starts its first line, so that their baselines meet, as CSS lines
/// them up; `None` for any other row. Their lines are the tallest's, but for the sizes that set their own, and GPUI sets
/// a line's text in its middle with the baseline `(ascent - descent) / 2` under it, so a smaller text's baseline sat
/// higher than a larger one's: an issue's key three points over its title's. A `wrap` button of one text is that text,
/// inside the button's border and padding.
pub(super) fn baseline_drops(row: &tree::Row, cx: &gpui::App) -> Option<Vec<f32>> {
    let line = line_height(&row.children).filter(|_| row.align == Align::Baseline)?;
    let system = cx.text_system();
    let baselines: Vec<f32> = row
        .children
        .iter()
        .map(|node| {
            let Some((text, top)) = row_text(node) else {
                return 0.0;
            };
            let (size, line) = match text.size {
                TextSize::Normal => (13.0, line),
                TextSize::Small => (12.0, line),
                TextSize::Tiny => (11.0, line),
                TextSize::Large => (20.0, 26.0),
                TextSize::Field => (14.0, 21.0),
            };
            let weight = match text.weight {
                Weight::Normal => FontWeight::NORMAL,
                Weight::Medium => FontWeight::MEDIUM,
                Weight::Strong => FontWeight::SEMIBOLD,
            };
            let font = page_font(weight, text.mono, cx);
            // A font's descent is under its baseline, so GPUI gives it as a negative length.
            let (ascent, descent) = (
                system.ascent(font, px(size)),
                system.descent(font, px(size)),
            );
            top + line / 2.0 + (ascent + descent).as_f32() / 2.0
        })
        .collect();
    let lowest = baselines.iter().copied().fold(0.0, f32::max);
    Some(
        baselines
            .into_iter()
            .map(|baseline| lowest - baseline)
            .collect(),
    )
}

/// The height of the tallest line of `nodes`, when they are all texts (`row_text`): a text's lines are 1.45 of its size,
/// the page's line height, but for the sizes that set their own.
fn line_height(nodes: &[Node]) -> Option<f32> {
    nodes.iter().try_fold(None::<f32>, |tallest, node| {
        let (text, _) = row_text(node)?;
        let height = match text.size {
            TextSize::Normal => 13.0 * 1.45,
            TextSize::Small => 12.0 * 1.45,
            TextSize::Tiny => 11.0 * 1.45,
            TextSize::Large => 26.0,
            TextSize::Field => 14.0 * 1.5,
        };
        Some(Some(tallest.map_or(height, |tallest| tallest.max(height))))
    })?
}

/// The padding over and under a `wrap` button's text, as plugin-ui.css's `.w-button[data-wrap]`.
const WRAPPED_PADDING: f32 = 2.0;

/// The text a baseline row lines `node` up by, and how far into `node` its top is: a text's own, or the one text of a
/// `wrap` button or of a list button, under the button's 1pt border and its padding. A log's folded group is its
/// number beside a list button that holds its title.
fn row_text(node: &Node) -> Option<(&tree::Text, f32)> {
    match node {
        Node::Text(text) => Some((text, 0.0)),
        Node::Button(button) if button.wrap => match button.children.as_slice() {
            [Node::Text(text)] => Some((text, 1.0 + WRAPPED_PADDING)),
            _ => None,
        },
        Node::ListButton(item) if !item.vertical => match item.button.children.as_slice() {
            [Node::Text(text)] => Some((text, 1.0 + controls_view::LIST_BUTTON_PADDING)),
            _ => None,
        },
        _ => None,
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

/// The width of the widest word of `text` at `size`, in the page's font or the terminal's with `mono`.
fn longest_word(text: &str, size: f32, weight: FontWeight, mono: bool, cx: &gpui::App) -> f32 {
    let system = cx.text_system();
    let font = page_font(weight, mono, cx);
    let width = |word: &str| {
        word.chars()
            .filter_map(|c| system.advance(font, px(size), c).ok())
            .map(|advance| advance.width.as_f32())
            .sum::<f32>()
    };
    // A point over, so rounding in the line's own layout never leaves the word a hair too wide to fit.
    text.split_whitespace()
        .map(width)
        .fold(0.0, f32::max)
        .ceil()
        + 1.0
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
