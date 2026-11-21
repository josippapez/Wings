//! Draws a page's tree with the UI kit, in the look of the desktop's plugin pages, and sends its clicks back.

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use gpui::{
    Animation, AnimationExt, AnyElement, Context, ElementId, Focusable, FontWeight, IntoElement,
    KeyDownEvent, Pixels, Render, Role, SharedString, Task, Window, canvas, div, prelude::*, px,
    point, relative, rgb, rgba,
};

#[path = "board_view.rs"]
mod board_view;
#[path = "code_view.rs"]
mod code_view;
#[path = "controls_view.rs"]
mod controls_view;
#[path = "graph_view.rs"]
mod graph_view;
#[path = "picture_view.rs"]
mod picture_view;
#[path = "rich_view.rs"]
mod rich_view;
#[path = "selection_view.rs"]
mod selection_view;
#[path = "sheet_view.rs"]
mod sheet_view;
#[path = "prompt_rail_view.rs"]
mod prompt_rail_view;
#[path = "hover_card_view.rs"]
mod hover_card_view;
#[path = "column_window_view.rs"]
mod column_window_view;
#[path = "popup_menu_view.rs"]
mod popup_menu_view;
#[path = "sunburst_view.rs"]
mod sunburst_view;
#[cfg(feature = "test-support")]
#[path = "bench_view.rs"]
mod bench_view;

use super::{
    plugin::{PageOpener, PluginHost, RESTART_FIRST, RESTART_MOST, RESTART_RESET},
    runtime::{Event, Message, PageRuntime},
    tree::{self, Align, Gap, Justify, Level, Node, TextSize, Tone, Tree, Weight, plain_text},
};
use crate::{terminal_view::DEFAULT_FONT_FAMILY, theme, ui, ui::behaviors::EscapeExt};

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
    paste_image_handler: Option<String>,
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

thread_local! {
    /// The `textScale` of the `Scroll` being drawn, which its texts' sizes are multiplied by. Read, not computed, per text.
    static TEXT_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// A `Stack` being drawn asked for links in its texts (`links: true`).
    static LINKED: Cell<bool> = const { Cell::new(false) };
    /// What the `Dialog` being drawn leaves what it holds, for a picture that keeps within it (`fitWindow`).
    static DIALOG_ROOM: Cell<Option<(f32, f32)>> = const { Cell::new(None) };
    /// How wide each character is at one point in each font a page measures text in (`char_width`).
    static CHAR_WIDTHS: RefCell<HashMap<(gpui::FontId, char), f32>> = RefCell::default();
    /// How many characters `char_width` asked the text system for.
    #[cfg(feature = "test-support")]
    static CHARS_ASKED: Cell<usize> = const { Cell::new(0) };
}

/// A text size in points as the `Scroll` being drawn scales it.
fn scaled(size: f32) -> f32 {
    size * TEXT_SCALE.with(Cell::get)
}

/// A `Scroll` that follows its end: where it is scrolled, and what `onReachStart` needs to know of the last frame.
#[derive(Default)]
struct FollowScroll {
    handle: gpui::ScrollHandle,
    /// How far it could scroll at the last frame, to tell how much its content grew.
    max: Cell<Pixels>,
    /// `onReachStart` was sent and what is above hasn't grown since, so it isn't sent again.
    asked: Cell<bool>,
    /// `onReachEnd` was sent and what is below hasn't grown since. Until it has, the scroll doesn't follow its end, which
    /// the newer items would take it to.
    asked_end: Cell<bool>,
    /// The children in view at the last frame, so a new tree that adds or takes away children above or below them
    /// leaves them where they were.
    anchor: RefCell<Anchor>,
    /// The `scrollTo` token the scroll last acted on.
    jumped: Cell<u64>,
    /// What `onTrack` last said: none before it was said at all, then the tracked child or none.
    tracked: Cell<Option<Option<usize>>>,
    /// It follows its end in the render under way (`follow_end`).
    stuck: Cell<bool>,
    /// Its column's heights and window, when it is drawn a window at a time (`column_window_view`).
    windowing: RefCell<column_window_view::Windowing>,
}

/// What a following `Scroll` with one column had in view.
#[derive(Default)]
struct Anchor {
    /// The tree it was drawn from, by `PageView::trees`.
    tree: u64,
    /// The column's children in view, by id, with how far each one's top was below the view's top. The first of them
    /// still in the next tree is kept in place.
    seen: Vec<(u64, Pixels)>,
}

/// What the column of a following `Scroll` needs to keep its view in place and ask for more at either end.
struct Edges {
    start: Option<String>,
    end: Option<String>,
    /// The id of each element the column's children make, in order.
    ids: Vec<Option<u64>>,
    tree: u64,
}

/// The id of each element a stack's children make, in order, as `PageView::stack` draws them: in a stack that doesn't
/// rise, a run of plain strings is one element, which has none.
fn element_ids(stack: &tree::Stack) -> Vec<Option<u64>> {
    if stack.enter {
        return stack.children.iter().map(Node::id).collect();
    }
    let mut ids = Vec::new();
    let mut run = false;
    for child in &stack.children {
        let string = matches!(child, Node::Str { .. });
        if !string || !run {
            ids.push(child.id().filter(|_| !string));
        }
        run = string;
    }
    ids
}

/// The layer of the window that draws the terminals a tab's agent opened, each at the place its page's `Terminal` leaves.
pub const TERMINALS: &str = "page-terminals";

/// The name of a terminal's place in `TERMINALS`.
pub fn terminal_slot(pane: usize) -> SharedString {
    format!("page-terminal-{pane}").into()
}

/// The pane a `Terminal` node names, when it is one of those the page's tab was handed.
fn embeddable(handed: &[usize], pane: &str) -> Option<usize> {
    pane.parse().ok().filter(|pane| handed.contains(pane))
}

/// The links found in each `Text` with `links: true`, by node id, with the text they were found in, for the links of
/// `generation`.
#[derive(Default)]
struct FoundLinks {
    generation: u64,
    by_text: HashMap<u64, (String, Rc<Vec<wings_plugins::links::Found>>)>,
}

pub struct PageView {
    runtime: Option<PageRuntime>,
    tree: Option<Tree>,
    /// The disclosures that are open, by node id. The page never hears of it: the app keeps it, and an id stays the
    /// same through every render of its instance, so it survives them.
    open: HashSet<u64>,
    /// The select or menu that is open, by node id.
    menu: Option<u64>,
    /// A press on that select's trigger found its menu open, so the click that follows closes it. The menu closes
    /// itself on any press outside it, the trigger's too, which would otherwise have the click open it again.
    pressed_open: bool,
    /// An open `Menu`'s box takes the keyboard, so Escape closes it: a click leaves the button that opened it without.
    menu_focus: gpui::FocusHandle,
    /// What had the keyboard before the open `Menu` took it, its button, which gets it back as the menu closes.
    menu_recovery: crate::ui::behaviors::FocusRecovery,
    /// The open `Menu`'s item the arrow keys picked out, by its index in the menu's items, which Enter picks.
    menu_highlight: Option<usize>,
    /// Where each select and combobox was drawn, by node id, and how tall the window was: its menu is as wide, and as
    /// tall as the room on one side of it.
    select_bounds: HashMap<u64, (gpui::Bounds<Pixels>, Pixels)>,
    /// Whether each `Split` was narrower than its `collapseBelow` when it was last drawn, by node id. A split not in
    /// here hasn't been measured yet and draws its fixed pane.
    split_narrow: HashMap<u64, bool>,
    /// Whether each `expandable` text was cut short when it was last drawn, by node id, and the ones opened.
    text_cut: HashMap<u64, bool>,
    text_open: HashSet<u64>,
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
    prompt_rails: RefCell<HashMap<u64, prompt_rail_view::RailState>>,
    /// Each `HoverCard`'s place and whether its card shows, by node id.
    hover_cards: RefCell<HashMap<u64, hover_card_view::HoverState>>,
    /// The window's size and scale when the page was last drawn, which a picture sizes itself by, and the pictures
    /// still being decoded, by node id.
    viewport: Cell<(f32, f32, f32)>,
    /// The size of the page's area a `Dialog` covers, as last drawn: less than the window's where sidebars share it.
    dialog_area: Cell<Option<(f32, f32)>>,
    images_waiting: RefCell<HashSet<u64>>,
    chip_focus: RefCell<HashMap<(u64, String), gpui::FocusHandle>>,
    /// What the page last threw, shown under it until it renders again.
    error: Option<SharedString>,
    /// The page has stopped: nothing in it answers a click until it is started again, which happens by itself
    /// (`restart_later`) or with the Reload by its error.
    stopped: bool,
    source: PageOpener,
    /// Whether the page was last said to be in view, to tell the next runtime the same.
    shown: Cell<Option<bool>>,
    /// When the runtime that runs now was started, and how long the next restart waits: `RESTART_FIRST`, then twice
    /// as long after each short run up to `RESTART_MOST`, as a plugin's main (`PluginHost::keep_main`).
    started: Instant,
    restart_wait: Duration,
    _restart: Option<Task<()>>,
    /// Shown in a title bar button's popover (`place: "popover"`), whose shell and ground are the popover's.
    popover: bool,
    /// Whether a rounded card holds the page (`in_card`): its ground follows the card's inner corners, which a clip
    /// doesn't (gpui clips to a rectangle), so a square ground shows past the curve against the window.
    in_card: bool,
    /// Where the page's shell is scrolled, so a field that takes the keyboard can be brought into view.
    shell_scroll: gpui::ScrollHandle,
    /// Where the open combobox's list is scrolled, so the option the arrow keys reach is brought into view.
    combobox_scroll: gpui::ScrollHandle,
    /// Where each `Scroll` that follows its end is scrolled, by node id.
    follow_scrolls: RefCell<HashMap<u64, Rc<FollowScroll>>>,
    /// How many trees the page has sent, so a scroll can tell a new one from the frames of the last.
    trees: u64,
    /// A focus handle round each list item's `actions`, by node id, and the items one of whose actions had the
    /// keyboard when the page was last drawn: those show their actions without the pointer on them.
    actions_focus: RefCell<HashMap<u64, gpui::FocusHandle>>,
    actions_within: HashSet<u64>,
    /// A field that took the keyboard and is brought into view once it has been drawn.
    reveal: Option<gpui::Entity<crate::text_field::TextField>>,
    /// The page's `Dialog` takes the keyboard while it shows, by this handle, and gives it back to what had it before.
    dialog_focus: gpui::FocusHandle,
    dialog_shown: Option<u64>,
    focus_before: Option<gpui::FocusHandle>,
    /// What an element under a right press recorded (a `Code`'s text), with where the press was, for the `Stack` round it
    /// that has an `onContextMenu` to send. GPUI hands the press to the inner element first.
    context_hit: RefCell<Option<(gpui::Point<Pixels>, serde_json::Value)>>,
    /// The character the pointer is on in each line of text with a link, by the line's id, for a right-click on a link.
    pointed_chars: RefCell<HashMap<u64, Rc<Cell<Option<usize>>>>>,
    /// The words a person has selected by dragging over a line of text, for Cmd-C and a right-click's payload.
    selection: RefCell<Option<selection_view::Selection>>,
    /// The lines of text a person can select, as the page drew them, for a selection that runs over several.
    lines: RefCell<selection_view::Lines>,
    /// What takes the keyboard while words are selected, so Cmd-C reaches the page: a box with no size, drawn in it.
    selection_focus: gpui::FocusHandle,
    /// An open `PopupMenu` takes the keyboard, so Escape closes it; the node it was drawn for, and what had the keyboard
    /// before it, which gets it back as the menu goes.
    popup_focus: gpui::FocusHandle,
    popup_shown: Option<u64>,
    popup_before: Option<gpui::FocusHandle>,
    /// The focus each `Sheet`'s panel tracks, by node id; the sheets drawn in this render, each with what takes the
    /// keyboard when it opens; the ones the last render showed; and what had the keyboard before a sheet took it.
    sheet_focus: RefCell<HashMap<u64, gpui::FocusHandle>>,
    sheets_drawn: RefCell<Vec<(u64, gpui::FocusHandle)>>,
    sheets_shown: Vec<u64>,
    sheet_return: Option<gpui::FocusHandle>,
    /// The tab that shows the page, whose keyboard counts as the page's (`set_host_focus`).
    host_focus: Option<gpui::FocusHandle>,
    /// When you last clicked or pressed a key in the page, and what had the keyboard as you last pressed in it.
    gesture_at: Cell<Option<Instant>>,
    pressed_with: RefCell<Option<Option<gpui::FocusHandle>>>,
    /// Fields that left the tree and may still hold the keyboard, as one that takes a field's place asks for it.
    former_inputs: RefCell<Vec<gpui::WeakFocusHandle>>,
    /// The terminals the agent of the page's tab opened, by pane: the only ones its `Terminal` nodes draw.
    terminals: Vec<usize>,
    /// The list of them the page was last told, which a page that starts again is told again.
    terminals_told: Option<String>,
    /// A dialog or sheet of the page is over its terminals, which are then not drawn: they draw over the page.
    terminals_covered: Cell<bool>,
    /// The page's spinners, working dots and shimmers move in the window's layer for them (`ui::view_spinner_layer`) in
    /// this render: a step of them then draws the layer, not the whole page. Only while a piece of the window puts the page on screen and draws
    /// that layer (`ui::place_view`), and nothing of the page's own is over them.
    spin_in_layer: Cell<bool>,
    found_links: RefCell<FoundLinks>,
    /// Draws the page's terminals over it, as the workspace does, for the screenshots of a page alone.
    #[cfg(feature = "test-support")]
    shot_terminals: Option<Rc<dyn Fn(&mut gpui::App) -> Vec<(SharedString, AnyElement)>>>,
    /// How many times the page has rendered, for what draws it again.
    #[cfg(feature = "test-support")]
    renders: Cell<usize>,
    /// The nodes the typing bench leaves out of the draw (`bench_view`).
    #[cfg(feature = "test-support")]
    bench_hidden: RefCell<HashSet<u64>>,
    _events: Task<()>,
}

impl PageView {
    /// Whether a select's menu or a combobox's options are open over the page. They are drawn above everything else
    /// and can reach past the page's own bounds, so a press on one may land outside the frame that holds the page.
    pub fn shows_menu(&self) -> bool {
        self.menu.is_some() || self.listing.get()
    }

    /// The backdrop the page asks for behind it (`backdrop: "pixels"` or `"gradient:<preset>"` on its `Page`) and its
    /// palette, when a card holds it. The page leaves its ground to the card then, which draws the backdrop where the
    /// window has one (`ui::backdrop_slot`), else its own plain ground.
    pub fn backdrop(&self) -> Option<(tree::Backdrop, theme::Palette)> {
        let drawn = self.tree.as_ref().filter(|_| self.in_card && !self.popover)?;
        let kind = drawn.children.iter().find_map(|node| match node {
            Node::Page(page) => page.backdrop,
            _ => None,
        })?;
        Some((kind, drawn.palette.unwrap_or(theme::Palette::APP)))
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

    /// Loads the page from `source`, its plugin's next run after an update or restart: it stays where it shows, and
    /// starts over with the new code.
    pub fn reload_from(&mut self, source: PageOpener, cx: &mut Context<Self>) {
        self.source = source;
        self.reload(cx);
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
        if let (Some(runtime), Some(told)) = (&runtime, &self.terminals_told) {
            runtime.inbox().send(Message::Terminals, 0, told.clone());
        }
        self.stopped = runtime.is_none();
        (self.runtime, self.error, self._events) = (runtime, error, events);
        self.started = Instant::now();
        self.tree = None;
        self.open.clear();
        self.menu = None;
        self.pressed_open = false;
        self.select_bounds.clear();
        self.split_narrow.clear();
        self.focused = None;
        self.suggestions_closed = false;
        self.highlight = None;
        self.fields.borrow_mut().clear();
        self.inputs.borrow_mut().clear();
        self.graphs.borrow_mut().clear();
        self.sunbursts.borrow_mut().clear();
        self.prompt_rails.borrow_mut().clear();
        self.hover_cards.borrow_mut().clear();
        self.chip_focus.borrow_mut().clear();
        self.actions_focus.borrow_mut().clear();
        self.actions_within.clear();
        self.reveal = None;
        self.dialog_shown = None;
        self.focus_before = None;
        self.popup_shown = None;
        self.popup_before = None;
        *self.context_hit.borrow_mut() = None;
        self.pointed_chars.borrow_mut().clear();
        *self.selection.borrow_mut() = None;
        self.sheet_focus.borrow_mut().clear();
        self.sheets_shown.clear();
        self.sheet_return = None;
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

    /// Opens `source` as a page of the plugin `host` runs, which can talk to its main and its other pages.
    pub fn in_plugin(host: &PluginHost, source: &str, cx: &mut Context<Self>) -> Self {
        Self::start(cx, host.page(false, 0, None, None, source))
    }

    /// The page, shown in a card with a 14px corner and a 1px border (`workspace::page_card`, the sidebar's cards).
    pub fn in_card(self) -> Self {
        Self {
            in_card: true,
            ..self
        }
    }

    /// Like `in_plugin`, for a page shown in the popover under its sidebar's title bar button.
    pub fn in_plugin_popover(host: &PluginHost, source: &str, cx: &mut Context<Self>) -> Self {
        Self {
            popover: true,
            ..Self::in_plugin(host, source, cx)
        }
    }

    /// Like `in_plugin`, for a tab `wings.openView` opened with `args`, which the page reads with `viewArgs`.
    pub fn in_plugin_with_args(
        host: &PluginHost,
        source: &str,
        args: Option<&serde_json::Value>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::start(cx, host.page(false, 0, args, None, source))
    }

    /// Like `in_plugin_with_args`, for a tab or block whose `wings.tabState`/`blockState` is `state`.
    pub fn in_plugin_with_state(
        host: &PluginHost,
        source: &str,
        args: Option<&serde_json::Value>,
        state: crate::page::plugin::FrameState,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::start(cx, host.page(false, 0, args, Some(state), source))
    }

    pub fn in_plugin_window(
        host: &PluginHost,
        window_id: u64,
        source: &str,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::start(cx, host.page(true, window_id, None, None, source))
    }

    /// Starts the page's runtime: the runtime, or why it didn't start, and the task that takes what it sends.
    fn launch(source: &PageOpener, cx: &mut Context<Self>) -> (Option<PageRuntime>, Option<SharedString>, Task<()>) {
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
                super::log::line(source.plugin(), source.name(), &message);
                (None, Some(message.into()))
            }
        };
        let events = cx.spawn(async move |view, cx| {
            while woken.recv().await.is_ok() {
                let taken = std::mem::take(&mut *pending.lock().unwrap());
                if view
                    .update(cx, |view, cx| {
                        let mut echo = false;
                        if let Some(tree) = taken.tree {
                            // The page sent back only what was just typed, which its fields already show: nothing to draw.
                            // An answer to an earlier key, typed before the page caught up, is an echo too: drawing the
                            // page again for each one rebuilt a long chat on most keys of someone typing at speed.
                            echo = view.tree.as_ref().is_some_and(|old| {
                                old.echoes(&tree, |id, text| {
                                    view.inputs.borrow().get(&id).is_some_and(|state| {
                                        state.field.read(cx).value() == text || state.sent.iter().any(|sent| sent == text)
                                    })
                                })
                            });
                            if echo {
                                view.echoed(&tree.children);
                            }
                            // `WINGS_PAGE_DIFFS=1`: what changed in each tree that isn't an echo of what was typed.
                            #[cfg(feature = "test-support")]
                            if !echo
                                && std::env::var_os("WINGS_PAGE_DIFFS").is_some()
                                && let Some(old) = &view.tree
                            {
                                eprintln!("[tree {}] {}", view.trees + 1, old.differences(&tree).join("\n"));
                            }
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
                                        Node::PromptRail(rail) => {
                                            ids.insert(rail.id);
                                        }
                                        Node::HoverCard(card) => {
                                            ids.insert(card.id);
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
                            let gone: Vec<gpui::WeakFocusHandle> = view
                                .inputs
                                .borrow()
                                .iter()
                                .filter(|(id, _)| !ids.contains(id))
                                .map(|(_, state)| state.field.read(cx).focus_handle(cx).downgrade())
                                .collect();
                            let mut former = view.former_inputs.borrow_mut();
                            former.retain(|field| field.upgrade().is_some());
                            former.extend(gone);
                            drop(former);
                            view.inputs.borrow_mut().retain(|id, _| ids.contains(id));
                            view.graphs.borrow_mut().retain(|id, _| ids.contains(id));
                            view.sunbursts.borrow_mut().retain(|id, _| ids.contains(id));
                            view.prompt_rails.borrow_mut().retain(|id, _| ids.contains(id));
                            view.hover_cards.borrow_mut().retain(|id, _| ids.contains(id));
                            view.chip_focus
                                .borrow_mut()
                                .retain(|(id, _), _| ids.contains(id));
                            view.keep_selection_in(&tree.children);
                            view.tree = Some(tree);
                            view.trees += 1;
                            view.error = None;
                        }
                        if let Some(error) = taken.error {
                            if taken.stopped {
                                super::log::line(view.source.plugin(), view.source.name(), &error);
                            }
                            view.error = Some(error.into());
                        }
                        if taken.stopped {
                            view.stopped = true;
                            view.restart_later(cx);
                        }
                        if !echo || view.error.is_some() || taken.stopped {
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        (runtime, error, events)
    }

    fn start(cx: &mut Context<Self>, source: PageOpener) -> Self {
        let (runtime, error, events) = Self::launch(&source, cx);
        let mut view = Self {
            stopped: runtime.is_none(),
            runtime,
            tree: None,
            open: HashSet::new(),
            menu: None,
            pressed_open: false,
            select_bounds: HashMap::new(),
            split_narrow: HashMap::new(),
            text_cut: HashMap::new(),
            text_open: HashSet::new(),
            focused: None,
            suggestions_closed: false,
            highlight: None,
            listing: Cell::new(false),
            fields: RefCell::new(HashMap::new()),
            inputs: RefCell::new(HashMap::new()),
            graphs: RefCell::new(HashMap::new()),
            sunbursts: RefCell::new(HashMap::new()),
            prompt_rails: RefCell::new(HashMap::new()),
            hover_cards: RefCell::default(),
            viewport: Cell::new((0.0, 0.0, 1.0)),
            dialog_area: Cell::new(None),
            images_waiting: RefCell::default(),
            chip_focus: RefCell::new(HashMap::new()),
            error,
            source,
            shown: Cell::new(None),
            started: Instant::now(),
            restart_wait: RESTART_FIRST,
            _restart: None,
            popover: false,
            in_card: false,
            shell_scroll: gpui::ScrollHandle::new(),
            combobox_scroll: gpui::ScrollHandle::new(),
            follow_scrolls: RefCell::default(),
            trees: 0,
            actions_focus: RefCell::default(),
            menu_focus: cx.focus_handle(),
            menu_recovery: Default::default(),
            menu_highlight: None,
            actions_within: HashSet::new(),
            reveal: None,
            dialog_focus: cx.focus_handle(),
            dialog_shown: None,
            focus_before: None,
            context_hit: RefCell::default(),
            pointed_chars: RefCell::default(),
            selection: RefCell::default(),
            lines: RefCell::default(),
            selection_focus: cx.focus_handle(),
            popup_focus: cx.focus_handle(),
            popup_shown: None,
            popup_before: None,
            sheet_focus: RefCell::default(),
            sheets_drawn: RefCell::default(),
            sheets_shown: Vec::new(),
            sheet_return: None,
            host_focus: None,
            gesture_at: Cell::new(None),
            pressed_with: RefCell::new(None),
            former_inputs: RefCell::default(),
            terminals: Vec::new(),
            terminals_told: None,
            terminals_covered: Cell::new(false),
            spin_in_layer: Cell::new(false),
            found_links: RefCell::default(),
            #[cfg(feature = "test-support")]
            shot_terminals: None,
            #[cfg(feature = "test-support")]
            renders: Cell::new(0),
            #[cfg(feature = "test-support")]
            bench_hidden: RefCell::default(),
            _events: events,
        };
        if view.stopped {
            view.restart_later(cx);
        }
        // The places its spinners left in the window's layer go with it.
        let id = cx.entity_id();
        cx.on_release(move |_, _| ui::forget_view(id)).detach();
        view
    }

    /// The terminals the agent of this page's tab opened, by pane, which its `Terminal` nodes may draw, and the JSON list
    /// the page hears them as (`terminals`). Nothing is sent when it is what the page heard last.
    pub fn set_terminals(&mut self, panes: Vec<usize>, json: String, cx: &mut Context<Self>) {
        if self.terminals_told.as_deref() == Some(json.as_str()) {
            return;
        }
        if let Some(runtime) = &self.runtime {
            runtime.inbox().send(Message::Terminals, 0, json.clone());
        }
        self.terminals = panes;
        self.terminals_told = Some(json);
        cx.notify();
    }

    /// Draws the page's terminals over it with `draw`, as the workspace does, for the screenshots of a page alone.
    #[cfg(feature = "test-support")]
    pub fn draw_terminals_for_shots(
        &mut self,
        draw: impl Fn(&mut gpui::App) -> Vec<(SharedString, AnyElement)> + 'static,
    ) {
        self.shot_terminals = Some(Rc::new(draw));
    }

    /// Where a `Terminal` shows: the workspace draws the terminal over this place, in the `TERMINALS` layer, so what it
    /// prints never draws the page again. One the tab wasn't handed draws a line saying so, and one under the page's own
    /// dialog or sheet is left out, as it would draw over them.
    fn terminal(&self, terminal: &tree::Terminal) -> AnyElement {
        let Some(pane) = embeddable(&self.terminals, &terminal.pane) else {
            return div()
                .text_size(px(12.0))
                .text_color(rgb(theme::palette().muted_foreground))
                .child("Only a terminal this tab's agent opened shows here.")
                .into_any_element();
        };
        let place = div().w_full().flex_none().h(px(terminal.height));
        if self.terminals_covered.get() {
            return place.into_any_element();
        }
        place.child(ui::slot(TERMINALS, terminal_slot(pane), None)).into_any_element()
    }

    /// The links in the text of `id`, found once for each text and set of links rather than each time the page draws.
    fn found_links(&self, id: u64, text: &str, rules: &[wings_plugins::links::Rule], generation: u64) -> Rc<Vec<wings_plugins::links::Found>> {
        let mut cache = self.found_links.borrow_mut();
        // A long chat's texts come and go: a bound on what is kept, past which it starts over.
        if cache.generation != generation || cache.by_text.len() > 4096 {
            cache.generation = generation;
            cache.by_text.clear();
        }
        if let Some((seen, found)) = cache.by_text.get(&id)
            && seen.as_str() == text
        {
            return found.clone();
        }
        let found = Rc::new(wings_plugins::links::find(text, rules));
        cache.by_text.insert(id, (text.to_owned(), found.clone()));
        found
    }

    fn send(&self, handler: &str, payload: Option<&serde_json::Value>) {
        if let Some(runtime) = &self.runtime {
            runtime.dispatch(handler, payload);
        }
    }

    /// `send` for a click, a key press or a pick, which lets the page copy right after.
    fn act(&self, handler: &str, payload: Option<&serde_json::Value>) {
        self.gesture_at.set(Some(Instant::now()));
        if let Some(runtime) = &self.runtime {
            runtime.dispatch_gesture(handler, payload);
        }
    }

    /// Clicks the button whose `shortcut` is `keystroke`, and says whether one had it. While a dialog is open only its
    /// buttons count, as only they can be clicked.
    pub fn press_shortcut(&self, keystroke: &gpui::Keystroke) -> bool {
        let mods = keystroke.modifiers;
        let Some(tree) = self.tree.as_ref().filter(|_| mods.platform || mods.control) else {
            return false;
        };
        let nodes = dialog_in(&tree.children).map_or(&tree.children[..], |dialog| &dialog.children[..]);
        match shortcut_in(nodes, keystroke) {
            Some(handler) => {
                self.act(&handler, None);
                true
            }
            None => false,
        }
    }

    /// Sends the page's `onTextScale` the `step` (`bigger`, `smaller` or `reset`), and says whether it has one.
    pub fn scale_text(&self, step: &str) -> bool {
        let Some(handler) = self.tree.as_ref().and_then(|tree| text_scale_in(&tree.children)) else {
            return false;
        };
        self.act(&handler, Some(&serde_json::json!({ "step": step })));
        true
    }

    /// Whether a click or a key of yours has reached the page.
    #[cfg(test)]
    pub(super) fn acted(&self) -> bool {
        self.gesture_at.get().is_some()
    }

    /// The tab that shows this page: while it has the keyboard, as it does once you open the tab, the page may move it
    /// into a field of its own.
    pub fn set_host_focus(&mut self, focus: gpui::FocusHandle) {
        self.host_focus = Some(focus);
    }

    /// Whether the page may move the keyboard into itself, as a dialog, a sheet or a field with `autoFocus` does: when
    /// the keyboard is in the page or on its tab already, nothing has it, or you just clicked or typed in the page. A
    /// page never takes it from a terminal or another page you are typing in.
    pub(super) fn may_take_keyboard(&self, window: &gpui::Window, cx: &gpui::App) -> bool {
        let focused = window.focused(cx);
        // Right after you used the page, while the keyboard is where it was as you pressed in it: a click in a terminal
        // since then takes it back.
        let recent = self.gesture_at.get().is_some_and(|at| at.elapsed() < GESTURE_TAKES_KEYBOARD);
        if recent && self.pressed_with.borrow().as_ref().is_some_and(|before| *before == focused) {
            return true;
        }
        let Some(focused) = focused else {
            return true;
        };
        // What can hold the keyboard in the page: its fields, menus, chips, action rows, dialog and sheets. Compared by
        // handle, as a window that hasn't drawn yet can't say what holds what.
        self.host_focus.as_ref() == Some(&focused)
            || focused == self.dialog_focus
            || focused == self.popup_focus
            || focused == self.menu_focus
            || self.inputs.borrow().values().any(|state| state.field.read(cx).focus_handle(cx) == focused)
            || self.former_inputs.borrow().iter().any(|field| *field == focused)
            || self.sheet_focus.borrow().values().any(|handle| *handle == focused)
            || self.chip_focus.borrow().values().any(|handle| *handle == focused)
            || self.actions_focus.borrow().values().any(|handle| *handle == focused)
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

    /// The person picked an open `Menu`'s item, whose click is `handler`, by a click or Enter: the menu closes, the
    /// keyboard goes back to its button, and the page hears the click.
    fn pick_menu_item(&mut self, handler: &str, window: &mut gpui::Window, cx: &mut Context<Self>) {
        self.menu = None;
        self.menu_recovery.restore(&self.menu_focus, window, cx);
        self.act(handler, None);
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

    /// Clicks the first `Image` whose label starts with `label`. False when the page has none that takes a click.
    #[cfg(feature = "test-support")]
    pub fn press_image(&self, label: &str) -> bool {
        fn find<'a>(nodes: &'a [Node], label: &str) -> Option<&'a str> {
            nodes.iter().find_map(|node| match node {
                Node::Image(picture) if picture.label.as_deref().is_some_and(|name| name.starts_with(label)) => picture.on_click.as_deref(),
                node => node.parts().into_iter().find_map(|part| find(part, label)),
            })
        }
        let handler = self.tree.as_ref().and_then(|tree| find(&tree.children, label));
        handler.is_some_and(|handler| {
            self.act(handler, None);
            true
        })
    }

    /// Opens or closes the first disclosure whose summary starts with `summary`, as a click on its line would. False when
    /// the page has none.
    /// Presses the More of the first expandable text that is cut short, as a click on it would. False when none is.
    #[cfg(feature = "test-support")]
    pub fn press_more(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(id) = self.text_cut.iter().filter(|(_, cut)| **cut).map(|(id, _)| *id).min() else { return false };
        self.text_open.insert(id);
        cx.notify();
        true
    }

    #[cfg(feature = "test-support")]
    pub fn press_disclosure(&mut self, summary: &str, cx: &mut Context<Self>) -> bool {
        fn find<'a>(nodes: &'a [Node], summary: &str) -> Option<&'a tree::Disclosure> {
            nodes.iter().find_map(|node| match node {
                Node::Disclosure(disclosure) if plain_text(&disclosure.summary).trim().starts_with(summary) => Some(disclosure),
                node => node.parts().into_iter().find_map(|part| find(part, summary)),
            })
        }
        let Some(disclosure) = self.tree.as_ref().and_then(|tree| find(&tree.children, summary)).cloned() else {
            return false;
        };
        let open = disclosure.open.unwrap_or_else(|| self.open.contains(&disclosure.id));
        match &disclosure.on_open_change {
            Some(handler) => self.act(handler, Some(&serde_json::json!({ "open": !open }))),
            None => self.toggle_disclosure(disclosure.id, cx),
        }
        true
    }

    /// Opens the `nth` menu whose button is labelled `label`, by its text or its name, as a click on it would. False
    /// when the page has no such menu.
    #[cfg(feature = "test-support")]
    pub fn open_menu(&mut self, label: &str, nth: usize, cx: &mut Context<Self>) -> bool {
        fn find(nodes: &[Node], label: &str, found: &mut Vec<u64>) {
            for node in nodes {
                if let Node::Menu(menu) = node
                    && let Some(Node::Button(button)) = menu.trigger.first()
                    && (plain_text(&button.children).trim() == label
                        || button.label.as_deref() == Some(label))
                {
                    found.push(menu.id);
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
        let Some(id) = found.get(nth).copied() else {
            return false;
        };
        self.menu = Some(id);
        cx.notify();
        true
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

    /// Clicks the tick of prompt `index` on the first prompt rail, as a click there would. False when the page has none.
    #[cfg(feature = "test-support")]
    pub fn press_rail(&self, index: usize) -> bool {
        fn find(nodes: &[Node]) -> Option<&str> {
            nodes.iter().find_map(|node| match node {
                Node::PromptRail(rail) => rail.on_select.as_deref(),
                other => other.parts().into_iter().find_map(find),
            })
        }
        let handler = self.tree.as_ref().and_then(|tree| find(&tree.children));
        handler.is_some_and(|handler| {
            self.act(handler, Some(&serde_json::json!({ "index": index })));
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

    /// The id of the select named `label`, or showing it, while the page has one.
    #[cfg(feature = "test-support")]
    pub fn select_named(&self, label: &str) -> Option<u64> {
        fn find(nodes: &[Node], label: &str) -> Option<u64> {
            nodes.iter().find_map(|node| match node {
                Node::Select(select) if select.label == label || select.shown() == label => {
                    Some(select.id)
                }
                other => other.parts().into_iter().find_map(|part| find(part, label)),
            })
        }
        self.tree.as_ref().and_then(|tree| find(&tree.children, label))
    }

    /// Opens the menu of the select named `label`, or showing it, as a click on it would. False when the page has no
    /// such select.
    #[cfg(feature = "test-support")]
    pub fn open_select(&mut self, label: &str, cx: &mut Context<Self>) -> bool {
        let found = self.select_named(label);
        if let Some(id) = found {
            self.menu = Some(id);
            cx.notify();
        }
        found.is_some()
    }

    /// Takes the texts of `nodes`' inputs as given back, as a render takes them (`input`): what was sent up to each is
    /// heard, and each is the text the page last gave. A page's echo of what was typed isn't drawn, so this keeps the
    /// texts sent from piling up while someone types, and a later render from taking an earlier one as the page's own.
    fn echoed(&self, nodes: &[Node]) {
        for node in nodes {
            if let Node::Input(input) = node
                && let Some(state) = self.inputs.borrow_mut().get_mut(&input.id)
                && let Some(at) = state.sent.iter().position(|sent| *sent == input.value)
            {
                state.sent.drain(..=at);
                state.given = input.value.clone();
            }
            for part in node.parts() {
                self.echoed(part);
            }
        }
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
        Some(self.input_field(label)?.read(cx).focus_handle(cx))
    }

    /// The field of the last text input named `label`.
    #[cfg(feature = "test-support")]
    pub fn input_field(&self, label: &str) -> Option<gpui::Entity<crate::text_field::TextField>> {
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
        Some(self.inputs.borrow().get(&found?)?.field.clone())
    }

    #[cfg(feature = "test-support")]
    pub fn text(&self) -> String {
        self.tree.as_ref().map(Tree::text).unwrap_or_default()
    }

    /// How many trees the page has sent.
    #[cfg(feature = "test-support")]
    pub fn trees_sent(&self) -> u64 {
        self.trees
    }

    /// How many times the page has rendered.
    #[cfg(feature = "test-support")]
    pub fn renders(&self) -> usize {
        self.renders.get()
    }

    /// How far the following `Scroll` `id` is scrolled from its top, and how far it can scroll at most, in points.
    #[cfg(all(test, feature = "test-support"))]
    pub(super) fn follow_scrolled(&self, id: u64) -> Option<(f32, f32)> {
        let scrolls = self.follow_scrolls.borrow();
        let handle = &scrolls.get(&id)?.handle;
        Some(((-handle.offset().y).as_f32(), handle.max_offset().y.as_f32()))
    }

    /// How tall the view of the `Scroll` `id` was when last drawn, in points.
    #[cfg(all(test, feature = "test-support"))]
    pub(super) fn scrolled_view(&self, id: u64) -> Option<f32> {
        Some(self.follow_scrolls.borrow().get(&id)?.handle.bounds().size.height.as_f32())
    }

    /// Whether the expandable text `id` was cut short when last drawn, and whether it is opened.
    #[cfg(all(test, feature = "test-support"))]
    pub(super) fn text_cut(&self, id: u64) -> (Option<bool>, bool) {
        (self.text_cut.get(&id).copied(), self.text_open.contains(&id))
    }

    /// Whether the split `id` was narrower than its `collapseBelow` when last drawn, none before it was measured.
    #[cfg(all(test, feature = "test-support"))]
    pub(super) fn split_narrow(&self, id: u64) -> Option<bool> {
        self.split_narrow.get(&id).copied()
    }

    /// The mark of rail `id` the pointer is on, and that mark's title as its card shows it.
    #[cfg(all(test, feature = "test-support"))]
    pub(super) fn rail_card(&self, id: u64) -> Option<(usize, String)> {
        let hover = self.prompt_rails.borrow().get(&id)?.hover?;
        let rail = self.tree.as_ref()?.find_rail(id)?;
        rail.marks.get(hover).map(|mark| (hover, mark.title.clone()))
    }

    #[cfg(all(test, feature = "test-support"))]
    pub(super) fn open_disclosures(&self) -> usize {
        self.open.len()
    }

    #[cfg(all(test, feature = "test-support"))]
    pub(super) fn menu_open(&self) -> bool {
        self.menu.is_some()
    }

    /// Where the select `id` was last drawn, which its menu opens beside.
    #[cfg(all(test, feature = "test-support"))]
    pub(super) fn select_box(&self, id: u64) -> Option<gpui::Bounds<Pixels>> {
        self.select_bounds.get(&id).map(|(bounds, _)| *bounds)
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

    /// The handle of a `Scroll` that follows its end, which keeps it at its end through this draw while it is there, as
    /// a chat stays on its newest message. Scrolled up, it stays where it is, and so it does while the page shows an older
    /// part of its list (`newest` false) or newer items it asked for are on their way.
    fn follow_end(&self, id: u64, newest: bool) -> Rc<FollowScroll> {
        let state = self.follow_scrolls.borrow_mut().entry(id).or_default().clone();
        let handle = &state.handle;
        // Its offset grows negative as it scrolls down, to minus the most it can scroll. Not laid out yet, both are 0.
        let stuck = newest && !state.asked_end.get() && -handle.offset().y >= handle.max_offset().y - px(FOLLOW_SLACK);
        state.stuck.set(stuck);
        if stuck {
            handle.scroll_to_bottom();
        }
        state
    }

    /// A `Scroll` with a `maxHeight`: as tall as its children up to that, then it scrolls inside itself, with a thin thumb
    /// at its right edge while it can. GPUI hands a wheel event to every scroll under the pointer, so a list inside a
    /// scrolling card would move the card too: a wheel step the list took stops there, and one it couldn't (it is at that
    /// end, or short) goes on to the parent. The thumb is painted from the last layout, so a step never builds the page.
    fn scroll_within(&self, scroll: &tree::Scroll, cx: &mut Context<Self>) -> AnyElement {
        let handle = self.watched_scroll(scroll.id).handle.clone();
        let wheel = handle.clone();
        let thumb = handle.clone();
        let area = div()
            .id(ElementId::NamedInteger("page-scroll-within".into(), scroll.id))
            .max_h(px(scroll.max_height.unwrap_or_default()))
            .min_w_0()
            .flex()
            .flex_col()
            .pr(px(6.0))
            .overflow_y_scroll()
            .track_scroll(&handle)
            .on_scroll(move |event, window, cx| {
                // The scroll's own listener ran first and moved the offset without clamping it, so what it was before the
                // step is the offset less the step.
                let step = event.delta.pixel_delta(window.line_height()).y;
                let offset = wheel.offset();
                let most = wheel.max_offset().y;
                let before = offset.y - step;
                let moved = (step > px(0.0) && before < px(0.0)) || (step < px(0.0) && before > -most);
                wheel.set_offset(point(offset.x, offset.y.clamp(-most, px(0.0))));
                if moved {
                    cx.stop_propagation();
                }
            })
            .children(self.nodes(&scroll.children, cx));
        let thumb = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let most = thumb.max_offset().y.as_f32();
                if most <= 0.0 {
                    return;
                }
                let height = bounds.size.height.as_f32();
                let long = height + most;
                let size = (height / long * height).max(16.0).min(height);
                let top = (-thumb.offset().y.as_f32() / most).clamp(0.0, 1.0) * (height - size);
                window.paint_quad(
                    gpui::fill(
                        gpui::Bounds::new(
                            point(bounds.right() - px(4.0), bounds.top() + px(top)),
                            gpui::size(px(4.0), px(size)),
                        ),
                        rgba(0xffffff40),
                    )
                    .corner_radii(px(2.0)),
                );
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        div().relative().flex_none().min_w_0().child(area).child(thumb).into_any_element()
    }

    /// The handle of a `Scroll` that doesn't follow its end but is told where to scroll or which children to watch.
    fn watched_scroll(&self, id: u64) -> Rc<FollowScroll> {
        self.follow_scrolls.borrow_mut().entry(id).or_default().clone()
    }

    /// Runs once the column of a `Scroll` with `scrollTo` or `track` is laid out, with the bounds of its children: scrolls to
    /// the child `scrollTo` names, once for each token, and tells the page, with `onTrack`, which tracked child the view has
    /// scrolled to when that changes. Only the children's tops are read, so it never lays anything out again.
    /// `edges`, for a following scroll, first keeps its view in place and asks for more at its ends (`keep_in_place`).
    fn scroll_watch(
        state: Rc<FollowScroll>,
        edges: Option<Edges>,
        to: Option<(usize, u64)>,
        track: Vec<usize>,
        on_track: Option<String>,
        cx: &mut Context<Self>,
    ) -> impl Fn(Vec<gpui::Bounds<Pixels>>, &mut Window, &mut gpui::App) + 'static {
        let entity = cx.entity();
        move |children, window, cx| {
            let handle = &state.handle;
            let view = handle.bounds();
            if view.size.height <= px(0.0) {
                // Not laid out yet: look again next frame.
                window.refresh();
                return;
            }
            if let Some(edges) = &edges
                && Self::keep_in_place(&state, edges, &children, window, &entity, cx)
            {
                return;
            }
            if let Some((index, token)) = to.filter(|(_, token)| state.jumped.get() != *token)
                && let Some(child) = children.get(index)
            {
                state.jumped.set(token);
                let (max, offset) = (handle.max_offset().y, handle.offset());
                // The offset is how far the content moved up, so it is minus what the child is below the view's top.
                let wanted = offset.y + view.top() - child.top();
                handle.set_offset(point(offset.x, wanted.max(-max).min(px(0.0))));
                // Drawn again from the page, which draws the children at the new place in a windowed column.
                window.request_animation_frame();
                return;
            }
            let Some(handler) = on_track.as_ref() else {
                return;
            };
            // The last tracked child whose top is above a line a third of the way down the view, as a reader's eye is; at the
            // end of the scroll, the last one in view, since a short last child never reaches that line. A point of slack
            // for rounding.
            let at_end = handle.offset().y <= -handle.max_offset().y + px(1.0);
            let line = if at_end { view.bottom() } else { view.top() + view.size.height / 3.0 } + px(1.0);
            let reached = track.partition_point(|index| children.get(*index).is_some_and(|child| child.top() <= line));
            let at = reached.checked_sub(1);
            if state.tracked.get() == Some(at) {
                return;
            }
            state.tracked.set(Some(at));
            let handler = handler.clone();
            let entity = entity.clone();
            cx.defer(move |cx| entity.update(cx, |view, _| view.send(&handler, Some(&serde_json::json!({ "at": at })))));
        }
    }

    /// For a following scroll's column, with the bounds of its children: when a new tree adds or takes away children
    /// above or below what was in view, moves the view so the first of those children still there is where it was, as a
    /// list too long to send whole is moved along. Then asks the page for older items within a screen of the top
    /// (`onReachStart`) and newer ones within a screen of the end (`onReachEnd`). Says whether it moved the view, which
    /// this frame then drew in the wrong place.
    fn keep_in_place(state: &FollowScroll, edges: &Edges, children: &[gpui::Bounds<Pixels>], window: &mut Window, entity: &gpui::Entity<Self>, cx: &mut gpui::App) -> bool {
        let handle = &state.handle;
        let view = handle.bounds();
        let (max, offset) = (handle.max_offset().y, handle.offset());
        let before = state.max.replace(max);
        let mut anchor = state.anchor.borrow_mut();
        if anchor.tree != edges.tree {
            anchor.tree = edges.tree;
            // Newer items on their way aren't followed to (`follow_end`), so the view is kept on what it showed.
            let following = edges.end.is_none() && !state.asked_end.get() && -offset.y >= max - px(FOLLOW_SLACK);
            let seen = std::mem::take(&mut anchor.seen);
            // How far down the first child still here has gone: by what was added above it, less what was taken away.
            let moved = seen.iter().find_map(|(id, below)| {
                let at = edges.ids.iter().position(|el| *el == Some(*id))?;
                Some(children.get(at)?.top() - view.top() - *below)
            });
            let Some(moved) = moved else {
                // Nothing of what was in view is left to keep in place, so what was asked for has come.
                state.asked.set(false);
                state.asked_end.set(false);
                return false;
            };
            if moved > px(0.0) {
                state.asked.set(false);
            }
            if max - before - moved > px(0.0) {
                state.asked_end.set(false);
            }
            // At the newest end the scroll follows it instead.
            if !following && moved.abs() > px(0.5) {
                handle.set_offset(point(offset.x, offset.y - moved));
                // `refresh` does nothing while the window draws. Older items that came in a windowed column aren't drawn,
                // so no rise of theirs asks for the next frame either.
                window.request_animation_frame();
                return true;
            }
        }
        anchor.seen = children
            .iter()
            .zip(&edges.ids)
            .skip_while(|(child, _)| child.bottom() <= view.top())
            .take_while(|(child, _)| child.top() < view.bottom())
            .filter_map(|(child, id)| Some(((*id)?, child.top() - view.top())))
            .take(8)
            .collect();
        drop(anchor);
        let height = view.size.height;
        let mut ask = |handler: &String| {
            let (handler, entity) = (handler.clone(), entity.clone());
            cx.defer(move |cx| entity.update(cx, |view, _| view.send(&handler, None)));
        };
        match &edges.start {
            Some(handler) if !state.asked.get() && -offset.y <= height => {
                state.asked.set(true);
                ask(handler);
            }
            Some(_) => {}
            None => state.asked.set(false),
        }
        match &edges.end {
            Some(handler) if !state.asked_end.get() && max + offset.y <= height => {
                state.asked_end.set(true);
                ask(handler);
            }
            Some(_) => {}
            None => state.asked_end.set(false),
        }
        false
    }

    /// Asks the page for older items when `area` is scrolled within a screen of its top, and keeps the view on what
    /// it showed when they arrive. It runs once the area is laid out, so it sees how far it scrolls now.
    /// `handler` is `None` once there is nothing older to ask for, which is also how the last older items arrive.
    fn reach_start(state: Rc<FollowScroll>, handler: Option<String>, cx: &mut Context<Self>) -> impl Fn(Vec<gpui::Bounds<Pixels>>, &mut Window, &mut gpui::App) + 'static {
        let entity = cx.entity();
        move |_, window, cx| {
            let handle = &state.handle;
            let (max, offset) = (handle.max_offset().y, handle.offset());
            let grew = max - state.max.replace(max);
            let at_end = -offset.y >= max - px(FOLLOW_SLACK);
            // Only a request brings older items, and what a request brings is above the view: moving the offset by
            // what it added keeps the same lines under the pointer. At its end the scroll follows on its own.
            if grew > px(0.0) && state.asked.get() {
                state.asked.set(false);
                if !at_end {
                    handle.set_offset(point(offset.x, offset.y - grew));
                    // This frame drew the content shifted, so draw once more with it back in place.
                    window.refresh();
                }
                return;
            }
            let Some(handler) = handler.as_ref() else {
                // Nothing older to ask for now, so the next time there is, it is asked for afresh.
                state.asked.set(false);
                return;
            };
            let height = handle.bounds().size.height;
            if state.asked.get() || height <= px(0.0) || -offset.y > height {
                return;
            }
            state.asked.set(true);
            let handler = handler.clone();
            let entity = entity.clone();
            cx.defer(move |cx| entity.update(cx, |view, _| view.send(&handler, None)));
        }
    }

    /// A `Stack` as a box, before it is made an element, so a `Scroll` that watches its children can ask for their bounds
    /// with `watch`.
    fn stack(&self, stack: &tree::Stack, watch: Option<popup_menu_view::ChildrenWatch>, cx: &mut Context<Self>) -> gpui::Div {
        // The texts it holds find links when it asks, as a Scroll's take its `textScale`.
        let before = LINKED.with(Cell::get);
        LINKED.with(|cell| cell.set(before || stack.links));
        let element = Self::stack_box(stack).map(|el| match stack.enter {
            true => el.children(stack.children.iter().map(|child| self.rising(child, cx))),
            false => el.children(self.items(&stack.children, cx)),
        });
        LINKED.with(|cell| cell.set(before));
        self.with_context_menu(element, stack, watch, None, cx)
    }

    /// A `Stack`'s box, without its children.
    fn stack_box(stack: &tree::Stack) -> gpui::Div {
        div()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(gap(stack.gap)))
            .when(stack.grow, |el| el.flex_1())
            .when(stack.fill, |el| el.flex_1().min_h_0())
            .when_some(stack.width, |el, width| el.w(px(width)).flex_none())
            .when_some(stack.max_width, |el, width| el.w_full().max_w(px(width)).mx_auto())
            .when_some(stack.measure, |el, width| el.max_w(px(width)))
            // A quote's bar: a 3pt border in the muted tone, with the text set in from it.
            .when(stack.quote, |el| {
                el.border_l(px(3.0)).border_color(rgba(theme::palette().muted_foreground << 8 | 0x66)).pl(px(10.0))
            })
            // A list button's 1pt border and its padding, so it starts where the list buttons beside it do.
            .when(stack.inset, |el| {
                el.px(px(7.0)).py(px(1.0 + controls_view::LIST_BUTTON_PADDING))
            })
    }

    fn nodes(&self, nodes: &[Node], cx: &mut Context<Self>) -> Vec<AnyElement> {
        nodes.iter().map(|node| self.node(node, cx)).collect()
    }

    /// What `nodes` say as one piece of text, or as inline parts when they hold more than strings.
    fn inline(&self, nodes: &[Node], cx: &mut Context<Self>) -> AnyElement {
        self.inline_of(nodes, false, cx)
    }

    /// `inline` for the words of a `Text`, which a person can select with the mouse (`select`), unlike a button's label.
    fn inline_of(&self, nodes: &[Node], select: bool, cx: &mut Context<Self>) -> AnyElement {
        if nodes.iter().all(|node| matches!(node, Node::Str { .. })) {
            let text = plain_text(nodes);
            return match nodes.first() {
                Some(Node::Str { id, .. }) if select && !text.is_empty() => self.selectable_string(*id, text, cx),
                _ => text.into_any_element(),
            };
        }
        if let Some(rich) = self.rich_text(nodes, select, cx) {
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
    /// row of "Updated ", "14 h ago" and buttons spread apart put the gap of a row between the two words. A run with no
    /// text is no item, as an empty text node makes no box in CSS: an icon button's empty label took a gap beside its
    /// icon and pushed it off its middle.
    fn items(&self, nodes: &[Node], cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut items = Vec::new();
        let mut run: Vec<Node> = Vec::new();
        let text = |run: &mut Vec<Node>| {
            let text = plain_text(&std::mem::take(run));
            (!text.is_empty()).then(|| div().child(text).into_any_element())
        };
        for node in nodes {
            if matches!(node, Node::Str { .. }) {
                run.push(node.clone());
                continue;
            }
            items.extend(text(&mut run));
            items.push(self.node(node, cx));
        }
        items.extend(text(&mut run));
        items
    }

    /// A child of a stack that rises in: it plays once, the first time a node of its id is drawn, so the items already
    /// there stay put as new ones come. A tween asks for frames only while it runs.
    fn rising(&self, node: &Node, cx: &mut Context<Self>) -> AnyElement {
        let element = div().min_w_0().flex().flex_col().child(self.node(node, cx));
        let Some(id) = node.id() else {
            return element.into_any_element();
        };
        crate::ui::motion::tween(element, ElementId::NamedInteger("page-rise".into(), id), crate::ui::motion::RISE, |el, t, reduced| {
            let (opacity, below) = crate::ui::motion::rise_frame(t, reduced);
            el.opacity(opacity).relative().top(px(below))
        })
        .into_any_element()
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
        if text.expandable && text.truncate {
            return self.expandable_text(text, drop, cx);
        }
        // CSS breaks a line between words, and lets a word wider than its box run over it; GPUI breaks inside a
        // word when it has to. A text that grows in a row, squeezed by the buttons beside it, would come apart a
        // letter or two a line, so it is never narrower than its longest word, as in the desktop. A mono text that
        // grows is a line of code or of a log, which the desktop sets in a box that scrolls sideways and the app
        // can't scroll: it breaks inside a word wider than its box instead, or a log's "=====" rule and a long link
        // ran past the card's edge.
        let code = text.mono && text.grow;
        // A line that pulses or counts the seconds moves in the window's layer, so its steps never draw the page.
        let in_layer = (text.shimmer || text.since.is_some()) && self.spin_in_layer.get();
        let page = cx.entity_id();
        let least = ((text.grow || drop.is_some()) && !text.truncate && !code).then(|| {
            let size = match text.size {
                TextSize::Small => scaled(12.0),
                TextSize::Tiny => scaled(11.0),
                TextSize::Large => scaled(20.0),
                TextSize::Field => scaled(14.0),
                TextSize::Normal => scaled(13.0),
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
            // Shrinking is shared by width, so a long folder before a long file name cut both: the name hardly shrinks.
            .when(text.shrink_last, |mut el| {
                el.style().flex_shrink = Some(0.001);
                el
            })
            .when(text.clamp, three_lines)
            .when(text.mono, |el| el.font_family(DEFAULT_FONT_FAMILY))
            .map(|el| match text.size {
                TextSize::Normal => el,
                TextSize::Small => el.text_size(px(scaled(12.0))),
                TextSize::Tiny => el.text_size(px(scaled(11.0))),
                TextSize::Large => el.text_size(px(scaled(20.0))).line_height(px(scaled(26.0))),
                TextSize::Field => el.text_size(px(scaled(14.0))).line_height(relative(1.5)),
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
                |el| {
                    let content = match text.since {
                        Some(since) => {
                            let took = Node::Str { id: text.id, text: format!(" · {}", took(since, now_ms())) };
                            self.inline(&[text.children.clone(), vec![took]].concat(), cx)
                        }
                        None if text.links || LINKED.with(Cell::get) => self
                            .linked_text(&text.children, text.id, cx)
                            .unwrap_or_else(|| self.inline_of(&text.children, true, cx)),
                        None => self.inline_of(&text.children, true, cx),
                    };
                    if !in_layer {
                        return el.child(content);
                    }
                    // The window's layer draws the words over their own, which keep their room, selection and screen
                    // reader in no colour, and pulses them and counts their `since` up without drawing the page again.
                    let words = plain_text(&text.children);
                    let since = text.since;
                    let said = move || -> SharedString {
                        match since {
                            Some(since) => format!("{words} · {}", took(since, now_ms())).into(),
                            None => words.clone().into(),
                        }
                    };
                    let name = format!("shimmer-{page}-{}", text.id);
                    el.child(
                        div()
                            .relative()
                            .child(div().text_color(gpui::transparent_black()).child(content))
                            .map(|el| match text.shimmer {
                                true => el.child(ui::shimmer_slot(name, since.is_some(), said)),
                                false => el.child(ui::count_slot(name, said)),
                            }),
                    )
                },
            );
        // A shimmer pulses on the spinners' clock, 10 frames a second while the window is in front: in the window's
        // layer for it (`ui::view_spinner_layer`), or in the page, each of whose frames then draws the page again, which
        // counts a `since` up too. Without a shimmer, a second's tick does that.
        let element = match (text.shimmer, text.since) {
            _ if in_layer => element.into_any_element(),
            (true, since) => ui::shimmer(element, since.is_some()).into_any_element(),
            (false, Some(_)) => element
                .with_animation(
                    ElementId::NamedInteger("page-since".into(), text.id),
                    Animation::new(std::time::Duration::from_secs(1)).repeat().with_max_fps(1.0),
                    |el, _| el,
                )
                .into_any_element(),
            (false, None) => element.into_any_element(),
        };
        if text.on_click.is_none() && text.title.is_none() {
            return element;
        }
        // The box round it narrows and grows as the text would, or a title cut short with a tooltip ran its whole
        // length under the buttons beside it, as Claude chat's header title did in a narrow tab.
        let element = div()
            .when(text.truncate || code, |el| el.min_w_0())
            .when(text.grow, |el| el.flex_1())
            .child(element);
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
                .text_color(rgb(text.tone.map_or(theme::palette().info, tone_color)))
                .on_click(cx.listener(move |view, _, _, _| view.act(&handler, None)))
                .into_any_element(),
            None => element.into_any_element(),
        }
    }

    /// A text cut short with More after it while it doesn't fit, and Less once opened: the queued messages over Claude
    /// chat's message box guessed from their length and offered More for a line that fit. The text's whole line is
    /// measured against the width it is drawn at, and a new width that cuts it, or stops cutting it, draws again.
    fn expandable_text(&self, text: &tree::Text, drop: Option<f32>, cx: &mut Context<Self>) -> AnyElement {
        let id = text.id;
        let open = self.text_open.contains(&id);
        let shown = tree::Text { truncate: !open, expandable: false, grow: false, ..text.clone() };
        let said = plain_text(&text.children);
        let (size, weight) = text_font(text);
        let whole = line_width(&said, size, weight, text.mono, cx);
        let lines = said.trim_end().contains('\n');
        let entity = cx.entity();
        let measure = canvas(
            move |bounds, _, cx| {
                let cut = lines || whole > bounds.size.width.as_f32() + 0.5;
                entity.update(cx, |view, cx| {
                    if view.text_cut.insert(id, cut) != Some(cut) {
                        cx.notify();
                    }
                })
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        let cut = self.text_cut.get(&id).copied().unwrap_or(false);
        let label = if open { "Less" } else { "More" };
        let toggle = sized(
            ui::button(ElementId::NamedInteger("page-text-more".into(), id), label, ui::Variant::Ghost, false, label),
            tree::ButtonSize::Sm,
        )
        .on_click(cx.listener(move |view, _, _, cx| {
            cx.stop_propagation();
            if !view.text_open.remove(&id) {
                view.text_open.insert(id);
            }
            cx.notify();
        }));
        // The first line is centred in a button's height, as the toggle is and as the buttons beside it in a row are, so a row
        // that starts its items at the top has Less, Send now and the cross all on the message's first line.
        let line = match text.size {
            TextSize::Normal => scaled(13.0 * 1.45),
            TextSize::Small => scaled(12.0 * 1.45),
            TextSize::Tiny => scaled(11.0 * 1.45),
            TextSize::Large => scaled(26.0),
            TextSize::Field => scaled(14.0 * 1.5),
        };
        let top = ((SM_BUTTON_HEIGHT - line) / 2.0).max(0.0);
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_start()
            .gap(px(4.0))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .pt(px(top))
                    .child(self.text_element(&shown, drop, cx))
                    .child(measure),
            )
            .when(cut || open, |el| {
                el.child(div().flex_none().h(px(SM_BUTTON_HEIGHT)).flex().items_center().child(toggle))
            })
            .into_any_element()
    }

    /// A `Button`, or a chip. In a baseline row it starts `drop` points down, as a text there does.
    fn button(&self, button: &tree::Button, drop: f32, cx: &mut Context<Self>) -> AnyElement {
        let element = self.button_element(button, drop, cx);
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

    /// `button` before its click is added, for a menu's button, whose click opens the menu.
    pub(super) fn button_element(
        &self,
        button: &tree::Button,
        drop: f32,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let id = ElementId::NamedInteger("page-button".into(), button.id);
        let label = button
            .label
            .clone()
            .unwrap_or_else(|| plain_text(&button.children));
        let tip = button.title.clone().or_else(|| button.shortcut.as_ref().map(|_| label.clone()));
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
            .when(button.round, |el| el.rounded_full())
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
            .when_some(button.title.clone(), |el, title| el.aria_description(title))
            // As the app's own header buttons show theirs: the shortcut beside what the tooltip says.
            .when_some(tip, |el, tip| {
                el.tooltip(ui::tooltip(tip, button.shortcut.as_ref().map(|key| key.label().into())))
            })
            .when_some(button.shortcut.as_ref(), |el, key| el.aria_keyshortcuts(key.aria()));
        let key_handler = button.on_key_down.clone();
        element.on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
            if let Some(handler) = &key_handler {
                send_shortcut(view, handler, event, window, cx);
            }
        }))
    }

    fn node(&self, node: &Node, cx: &mut Context<Self>) -> AnyElement {
        #[cfg(feature = "test-support")]
        if self.hidden_for_bench(node) {
            return div().into_any_element();
        }
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
            Node::Stack(stack) => self.stack(stack, None, cx).into_any_element(),
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
                // The tone is 0xRRGGBB: alpha goes below it, as the ground's does.
                .border_color(rgba(notice.tone.map_or(theme::palette().info, tone_color) << 8 | 0x66))
                .bg(rgba(
                    notice.tone.map_or(theme::palette().info, tone_color) << 8 | 0x20,
                ))
                .px(px(10.0))
                .py(px(7.0))
                .text_size(px(12.0))
                .text_color(rgb(notice.tone.map_or(theme::palette().info, tone_color)))
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
                .map(|ring| {
                    if self.spin_in_layer.get() {
                        return ring.child(ui::view_spinner_slot(format!("spinner-{}", spinner.id))).into_any_element();
                    }
                    ring.with_animation(
                        ElementId::NamedInteger("page-spinner-animation".into(), spinner.id),
                        Animation::new(Duration::from_secs(1)).repeat().with_max_fps(30.0),
                        |ring, turn| ring.child(ui::working_spinner(turn * std::f32::consts::TAU)),
                    )
                    .into_any_element()
                }),
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
                    .focus_visible(|style| style.border_color(rgb(theme::palette().foreground)))
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
                .text_color(rgb(theme::palette().muted_foreground))
                .child(if loading.children.is_empty() {
                    "Loading…".into_any_element()
                } else {
                    self.inline(&loading.children, cx)
                })
                .into_any_element(),
            // A card that scrolls shrinks to the room it has and scrolls what it holds, so its edge stays drawn: a
            // floating panel taller than the page was cut off at its foot, border and corners with it.
            Node::Card(card) if card.scrolls => {
                let body = div()
                    .id(ElementId::NamedInteger("page-card-scroll".into(), card.id))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(if card.raised { 6.0 } else { 10.0 }))
                    .children(self.nodes(&card.children, cx));
                let shell = ui::plain_card([body.into_any_element()]).min_h_0().overflow_hidden().children(ui::spot(|| format!("page-card-{}", card.id)));
                match card.raised {
                    true => shell
                        .px(px(12.0))
                        .py(px(10.0))
                        .rounded(px(14.0))
                        .border_color(rgba(theme::HAIRLINE_STRONG))
                        .shadow(vec![gpui::BoxShadow {
                            color: gpui::hsla(0.0, 0.0, 0.0, 0.28),
                            offset: gpui::point(px(0.0), px(6.0)),
                            blur_radius: px(18.0),
                            spread_radius: px(0.0),
                            inset: false,
                        }])
                        .into_any_element(),
                    false => shell.into_any_element(),
                }
            }
            Node::Card(card) if card.raised => ui::plain_card(self.nodes(&card.children, cx))
                .gap(px(6.0))
                .px(px(12.0))
                .py(px(10.0))
                .rounded(px(14.0))
                .border_color(rgba(theme::HAIRLINE_STRONG))
                .shadow(vec![gpui::BoxShadow {
                    color: gpui::hsla(0.0, 0.0, 0.0, 0.28),
                    offset: gpui::point(px(0.0), px(6.0)),
                    blur_radius: px(18.0),
                    spread_radius: px(0.0),
                    inset: false,
                }])
                .into_any_element(),
            // Rows split by hairlines, as Wings' Settings groups its own: each row keeps the card's inset.
            Node::Card(card) if card.rows => ui::plain_card(
                self.nodes(&card.children, cx).into_iter().enumerate().map(|(at, row)| {
                    div()
                        .px(px(12.0))
                        .py(px(10.0))
                        .when(at > 0, |row| row.border_t_1().border_color(rgba(ui::HAIRLINE)))
                        .child(row)
                        .into_any_element()
                }),
            )
            .gap(px(0.0))
            .px(px(0.0))
            .py(px(0.0))
            .into_any_element(),
            Node::Card(card) => ui::plain_card(self.nodes(&card.children, cx)).into_any_element(),
            Node::Heading(heading) => ui::heading(self.inline(&heading.children, cx))
                .when(heading.section, |el| {
                    el.text_size(px(14.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(theme::palette().foreground))
                })
                .into_any_element(),
            Node::Dot(dot) => match dot.state {
                // Turning where the page's spinners turn: in the window's layer for them, or in place.
                Some(state) if self.spin_in_layer.get() => {
                    ui::view_status_dot_slot(state, format!("dot-{}", dot.id))
                }
                Some(state) => ui::status_dot(state).into_any_element(),
                None => ui::dot(
                    dot.color
                        .unwrap_or_else(|| dot.tone.map_or(theme::palette().done, tone_color)),
                )
                .into_any_element(),
            },
            Node::Scroll(scroll) if scroll.max_height.is_some() => self.scroll_within(scroll, cx),
            // A scroll keeps 8 points either side and 12 below, as a list under a header does.
            Node::Scroll(scroll) => {
                let watches = scroll.scroll_to.is_some() || scroll.on_track.is_some();
                let column = match &scroll.children[..] {
                    [Node::Stack(column)] => Some(column),
                    _ => None,
                };
                // A long column is drawn a window at a time, which needs to know where the scroll is: so does one that was,
                // whose place would otherwise be lost as it gets shorter.
                let windowed = column.filter(|column| column_window_view::windowed(column));
                let state = match (scroll.follow_end, watches) {
                    (true, _) => Some(self.follow_end(scroll.id, scroll.on_reach_end.is_none())),
                    (false, true) => Some(self.watched_scroll(scroll.id)),
                    (false, false) if windowed.is_some() || self.follow_scrolls.borrow().contains_key(&scroll.id) => {
                        Some(self.watched_scroll(scroll.id))
                    }
                    (false, false) => None,
                };
                // A following scroll with one column keeps what is in view in place by the column's children, as children
                // come and go at either end; another keeps it by how much grew above.
                let edges = column.filter(|_| scroll.follow_end).map(|column| Edges {
                    start: scroll.on_reach_start.clone(),
                    end: scroll.on_reach_end.clone(),
                    ids: element_ids(column),
                    tree: self.trees,
                });
                let reach = state.as_ref().filter(|_| scroll.follow_end && edges.is_none()).map(|state| Self::reach_start(state.clone(), scroll.on_reach_start.clone(), cx));
                let mut base = div();
                if let Some(reach) = reach {
                    base = base.on_children_prepainted(reach);
                }
                // A scale sizes the plain text the scroll holds from here, which the other sizes follow with `scaled`.
                let before = TEXT_SCALE.with(Cell::get);
                let scale = scroll.text_scale.unwrap_or(before);
                TEXT_SCALE.with(|cell| cell.set(scale));
                // Following, or with a place to scroll to or children to track, its one column reports where its children are.
                let reports = watches || edges.is_some();
                let children = match (column, state.as_ref()) {
                    (Some(column), Some(state)) if windowed.is_some() || reports => {
                        let watch: Option<popup_menu_view::ChildrenWatch> = match reports {
                            true => Some(Box::new(Self::scroll_watch(state.clone(), edges, scroll.scroll_to, scroll.track.clone(), scroll.on_track.clone(), cx))),
                            false => None,
                        };
                        let element = match windowed {
                            Some(_) => self.windowed_column(column, state, scroll.follow_end && state.stuck.get(), watch, cx),
                            None => self.stack(column, watch, cx),
                        };
                        vec![element.into_any_element()]
                    }
                    _ => self.nodes(&scroll.children, cx),
                };
                TEXT_SCALE.with(|cell| cell.set(before));
                let area = base
                    .id(ElementId::NamedInteger("page-scroll".into(), scroll.id))
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    // As far in as a Bar's content, so a page's text lines up with its header and footer in a narrow pane.
                    .px(px(14.0))
                    .pb(px(12.0))
                    .overflow_y_scroll()
                    .when(scale != 1.0, |el| el.text_size(px(13.0 * scale)))
                    .children(children);
                match state {
                    Some(state) => area.track_scroll(&state.handle).into_any_element(),
                    None => area.into_any_element(),
                }
            }
            Node::Bar(bar) => div()
                .flex_none()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(gap(bar.gap)))
                .px(px(14.0))
                .py(px(8.0))
                // A terminal pane's header: 30 points tall with its 28 point buttons, and closer to its right edge.
                .when(bar.slim, |el| el.py(px(1.0)).pr(px(4.0)))
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
            Node::HoverCard(card) => self.hover_card(card, cx),
            Node::PopupMenu(menu) => self.popup_menu(menu, cx),
            Node::Select(select) => self.select(select, cx),
            Node::Combobox(combobox) => self.combobox(combobox, cx),
            Node::Menu(menu) => self.menu(menu, cx),
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
                    tree::IconName::Claude => crate::icons::Icon::Claude,
                },
                icon.size,
                // The Claude spark is drawn in its own orange, as Wings draws it beside a Claude session.
                match icon.name {
                    tree::IconName::Claude => theme::CLAUDE,
                    _ => theme::palette().foreground,
                },
            )
            .into_any_element(),
            // GPUI draws an svg as a mask in its text colour, which is the drawing's `currentColor`.
            Node::Svg(drawing) => {
                let image = gpui::svg()
                    .data(drawing.markup.as_bytes())
                    .size(px(drawing.size))
                    .flex_none()
                    .text_color(rgb(drawing.tone.map_or(theme::palette().foreground, tone_color)));
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
            Node::Image(picture) => self.picture(picture, cx),
            Node::Graph(graph) => self.graph(graph, cx),
            Node::Avatar(avatar) => rich_view::avatar(avatar),
            Node::Skeleton(skeleton) => rich_view::skeleton(skeleton),
            Node::Sunburst(map) => self.sunburst(map, cx),
            Node::PromptRail(rail) => self.prompt_rail(rail, cx),
            Node::Code(code) => self.code(code, cx),
            // Drawn over the whole page by `render`, not where it sits.
            Node::Dialog(_) => div().into_any_element(),
            Node::Sheet(sheet) => self.sheet(sheet, cx),
            Node::Terminal(terminal) => self.terminal(terminal),
            // At the end of the line whether it is in a column (a dialog) or a row (a confirm's actions).
            Node::Actions(actions) => div()
                .w_full()
                .min_w_0()
                .flex()
                .flex_wrap()
                .justify_end()
                .items_center()
                .gap(px(8.0))
                .children(self.nodes(&actions.children, cx))
                .into_any_element(),
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
        let content = self.empty_content(empty, cx);
        if !empty.fill {
            return content;
        }
        // A little above the middle, where the eye puts the middle of a page.
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .pb(px(48.0))
            .when(empty.glow, |el| {
                el.child(div().absolute().top_0().left_0().size_full().child(ui::glow()))
            })
            .child(content)
            .into_any_element()
    }

    fn empty_content(&self, empty: &tree::Empty, cx: &mut Context<Self>) -> AnyElement {
        div()
            .relative()
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
                            .text_size(px(if empty.fill { 22.0 } else { 14.0 }))
                            .font_weight(if empty.fill { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
                            .child(self.inline(&empty.title, cx)),
                    )
                    .when(!empty.description.is_empty(), |header| {
                        header.child(
                            div()
                                .text_size(px(14.0))
                                .text_color(rgb(theme::palette().muted_foreground))
                                .child(self.inline(&empty.description, cx)),
                        )
                    }),
            )
            .children(self.nodes(&empty.children, cx))
            .into_any_element()
    }

    /// Each pane scrolls on its own, as plugin-ui.css's `.w-split-pane { overflow: auto }`: Disk usage's list of
    /// files ran past the bottom of its tab. With `side: "end"` the fixed pane is on the right, borderless with 12
    /// points round it, for floating cards, and the other pane doesn't scroll: it is a page that fills its frame and
    /// scrolls what it holds itself. That fixed pane isn't drawn while the split is narrower than `collapseBelow`
    /// (unless pinned), or when it holds nothing.
    fn split(&self, split: &tree::Split, cx: &mut Context<Self>) -> AnyElement {
        let id = split.id;
        // Measures the split's own width, which the fixed pane doesn't change, and draws again only when it crosses
        // the threshold. A drag that crosses it shows or hides the pane a frame late, which nobody sees as wrong, and
        // it saves sizing the split from what it holds.
        let measure = split.collapse_below.map(|below| {
            let entity = cx.entity();
            let on_narrow = split.on_narrow.clone();
            canvas(
                move |bounds, _, cx| {
                    let narrow = bounds.size.width.as_f32() < below;
                    entity.update(cx, |view, cx| {
                        if view.split_narrow.insert(id, narrow) != Some(narrow) {
                            if let Some(handler) = &on_narrow {
                                view.send(handler, Some(&serde_json::json!({ "narrow": narrow })));
                            }
                            cx.notify();
                        }
                    })
                },
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
        });
        let narrow = self.split_narrow.get(&id).copied().unwrap_or(false);
        let fixed = if split.fixed_end { &split.end } else { &split.start };
        let drawn = !fixed.is_empty() && (split.pinned || !narrow);
        let fixed_pane = |name: &'static str| {
            div()
                .id(ElementId::NamedInteger(name.into(), id))
                .overflow_y_scroll()
                .w(px(split.size))
                .max_w(relative(0.5))
                .flex_none()
                .min_h_0()
                .flex()
                .flex_col()
        };
        let container = div().relative().flex_1().min_h_0().min_w_0().flex().children(measure);
        if split.fixed_end && split.float {
            return self.floating_split(split, container, cx);
        }
        if split.fixed_end {
            return container
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .overflow_hidden()
                        .children(self.nodes(&split.start, cx)),
                )
                .when(drawn, |el| {
                    el.child(fixed_pane("page-split-end-fixed").p(px(12.0)).gap(px(8.0)).children(self.nodes(&split.end, cx)))
                })
                .into_any_element();
        }
        container
            .when(drawn, |el| {
                el.child(
                    fixed_pane("page-split-start")
                        .border_r_1()
                        .border_color(rgba(ui::HAIRLINE))
                        .children(self.nodes(&split.start, cx)),
                )
            })
            .child(
                div()
                    .id(ElementId::NamedInteger("page-split-end".into(), id))
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

    /// A split with `float: true`: `start` takes the whole width and the fixed `end` pane floats over its right edge,
    /// as tall as what it holds, so the main pane never moves for it. The page decides when it shows (it hears
    /// `onNarrow`). With `onClose` the pane is open over the content: it has the keyboard, Escape in it and a press
    /// outside it send `onClose`.
    fn floating_split(&self, split: &tree::Split, container: gpui::Div, cx: &mut Context<Self>) -> AnyElement {
        let id = split.id;
        let main = div().flex_1().min_w_0().min_h_0().flex().flex_col().overflow_hidden().children(self.nodes(&split.start, cx));
        if split.end.is_empty() {
            return container.child(main).into_any_element();
        }
        // A field of the start's under the pane, being typed in, draws its text under the pane, in place.
        let over = Rc::new(Cell::new(gpui::Bounds::default()));
        let container = container.child(ui::under(over.clone(), main));
        // A card that scrolls what it holds (`scroll: true`) shrinks to the pane, which then neither scrolls nor clips: its
        // clip cut off the card's foot and its shadow. Any other pane scrolls as a whole.
        let card_scrolls = split.end.iter().any(|node| matches!(node, Node::Card(card) if card.scrolls));
        let mut pane = div()
            .id(ElementId::NamedInteger("page-split-float".into(), id))
            .absolute()
            .top_0()
            .right_0()
            .w(px(split.size))
            .max_w(relative(0.5))
            .max_h(relative(1.0))
            .when(!card_scrolls, |pane| pane.overflow_y_scroll())
            // The pane, not its cards, keeps the pointer from the page under it: a card that did was hit first and hid
            // the pane from the wheel, so a panel taller than the page didn't scroll.
            .occlude()
            .p(px(12.0))
            .flex()
            .flex_col()
            .gap(px(8.0));
        let mut container = container;
        if let Some(close) = split.on_close.clone() {
            let focus = self.sheet_focus.borrow_mut().entry(id).or_insert_with(|| cx.focus_handle()).clone();
            self.sheets_drawn.borrow_mut().push((id, focus.clone()));
            let on_press = close.clone();
            container = container.child(
                div().absolute().top_0().left_0().size_full().on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(move |view, _, _, _| view.act(&on_press, None)),
                ),
            );
            let page = cx.entity();
            pane = pane.track_focus(&focus).capture_escape(move |_, window, cx| {
                page.update(cx, |view, _| {
                    if view.shows_menu() {
                        return false;
                    }
                    view.send(&close, None);
                    window.prevent_default();
                    true
                })
            });
        }
        // The cards' surface is see-through, which only shows once the pane floats over the page's text: an opaque layer
        // under them, at the raised card's radius, keeps that text out, and the pointer with it. It is the dialogs' and
        // menus' lighter fill with their deep soft shadow, so the pane stands off the page as they do: on the page's own
        // colour only its light edge told it apart, which read as an outline.
        let under = div()
            .id(ElementId::NamedInteger("page-split-float-under".into(), id))
            .flex()
            .flex_col()
            .when(card_scrolls, |under| under.min_h_0())
            .gap(px(8.0))
            .rounded(px(14.0))
            .bg(rgb(theme::palette().dialog))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0.0, 0.0, 0.0, 0.45),
                offset: gpui::point(px(0.0), px(10.0)),
                blur_radius: px(32.0),
                spread_radius: px(0.0),
                inset: false,
            }])
            .child(ui::bounds_into(over))
            .children(self.nodes(&split.end, cx));
        container.child(pane.child(under)).into_any_element()
    }

    /// A `Dialog` over the whole page, as Wings' own dialogs are (`ui::dialog`): its title and a close button at the top,
    /// and what it holds under them, which scrolls. Escape, a press on the backdrop and the close button send `onClose`.
    fn dialog(&self, dialog: &tree::Dialog, cx: &mut Context<Self>) -> AnyElement {
        let id = dialog.id;
        let (backdrop, panel) = ui::dialog(
            ElementId::NamedInteger("page-dialog".into(), id),
            dialog.title.clone(),
            &self.dialog_focus,
            dialog.width,
            None,
        );
        let (on_backdrop, on_button, on_escape) =
            (dialog.on_close.clone(), dialog.on_close.clone(), dialog.on_close.clone());
        let header = div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .pl(px(24.0))
            .pr(px(12.0))
            .pt(px(14.0))
            .pb(px(6.0))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(16.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(dialog.title.clone()),
            )
            .child(
                ui::icon_button(
                    ElementId::NamedInteger("page-dialog-close".into(), id),
                    crate::icons::Icon::X,
                    "Close",
                    ui::ButtonSize::Sm,
                )
                .on_click(cx.listener(move |view, _, _, cx| {
                    cx.stop_propagation();
                    if let Some(handler) = &on_button {
                        view.act(handler, None);
                    }
                })),
            );
        let body = div()
            .id(ElementId::NamedInteger("page-dialog-body".into(), id))
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_y_scroll();
        let (window_w, window_h, _) = self.viewport.get();
        let area = self.dialog_area.get().unwrap_or((window_w, window_h));
        let room = DIALOG_ROOM.with(|cell| cell.replace(Some(tree::picture_room(area, dialog.width))));
        // The padding is on a child, inside what scrolls: padding on the scroll box itself is clipped away with the overflow,
        // so the last row met the panel's edge.
        let body = body.child(
            div()
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(12.0))
                .px(px(24.0))
                .pt(px(4.0))
                .pb(px(28.0))
                .children(self.nodes(&dialog.children, cx)),
        );
        DIALOG_ROOM.with(|cell| cell.set(room));
        let entity = cx.entity();
        let measure = canvas(
            move |bounds, _, cx| {
                let area = Some((f32::from(bounds.size.width), f32::from(bounds.size.height)));
                entity.update(cx, |view, cx| {
                    if view.dialog_area.replace(area) != area {
                        cx.notify();
                    }
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        backdrop
            .child(measure)
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |view, _, _, _| {
                    if let Some(handler) = &on_backdrop {
                        view.act(handler, None);
                    }
                }),
            )
            .child(ui::dialog_panel(
                panel
                    .flex_col()
                    // Before a field in it, which keeps Escape. A select's menu or a field's options open in the dialog
                    // close first, on their own.
                    .capture_escape({
                        let page = cx.entity();
                        move |_, window, cx| {
                            page.update(cx, |view, _| {
                                if view.shows_menu() {
                                    return false;
                                }
                                if let Some(handler) = &on_escape {
                                    view.send(handler, None);
                                }
                                window.prevent_default();
                                true
                            })
                        }
                    })
                    .children(ui::spot(|| "page-dialog".into()))
                    .child(header)
                    .child(body),
            ))
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
        ui::readout::render("page");
        #[cfg(feature = "test-support")]
        {
            self.renders.set(self.renders.get() + 1);
        }
        self.lines.borrow_mut().start();
        let over_backdrop = self.backdrop().is_some();
        // A page with a palette of its own paints in it while it builds its elements, which read their colours then.
        let palette = self.tree.as_ref().and_then(|tree| tree.palette);
        let _painting = palette.map(theme::painting);
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
        let size = window.viewport_size();
        self.viewport.set((size.width.into(), size.height.into(), window.scale_factor()));
        // The textures of pictures the store let go of leave the window's atlas.
        for gone in crate::page::images::take_retired() {
            let _ = window.drop_image(gone.image.clone());
        }
        // A dialog takes the keyboard from the page under it as it opens, and gives it back as it closes. A field in it
        // that asks for the keyboard takes it after this.
        let may_take = self.may_take_keyboard(window, cx);
        let dialog = self.tree.as_ref().and_then(|tree| dialog_in(&tree.children)).map(|dialog| dialog.id);
        if dialog.is_some() != self.dialog_shown.is_some() {
            if dialog.is_some() {
                if may_take {
                    self.focus_before = window.focused(cx);
                    window.focus(&self.dialog_focus, cx);
                }
            } else if let Some(before) = self.focus_before.take() {
                window.focus(&before, cx);
            }
        }
        self.dialog_shown = dialog;
        // A popup menu takes the keyboard as it opens, from a right-click, and gives it back as it goes.
        let popup = self.tree.as_ref().and_then(|tree| popup_in(&tree.children)).map(|menu| menu.id);
        if popup.is_some() != self.popup_shown.is_some() {
            // Only where the page may take the keyboard, as a dialog: a menu never pulls it from a terminal.
            if popup.is_some() {
                if may_take {
                    self.popup_before = window.focused(cx);
                    window.focus(&self.popup_focus, cx);
                }
            } else if let Some(before) = self.popup_before.take() {
                window.focus(&before, cx);
            }
        }
        self.popup_shown = popup;
        self.actions_within = self
            .actions_focus
            .borrow()
            .iter()
            .filter(|(_, focus)| focus.contains_focused(window, cx))
            .map(|(id, _)| *id)
            .collect();
        let tree_nodes = |view: &Self, cx: &mut Context<Self>| {
            view.tree
                .as_ref()
                .map(|tree| view.nodes(&tree.children, cx))
                .unwrap_or_default()
        };
        let covered = dialog.is_some() || self.tree.as_ref().is_some_and(|tree| sheet_in(&tree.children));
        let id = window.window_handle().window_id();
        let placed = ui::view_placed(cx.entity_id(), id);
        // Under a dialog or sheet of its own, the page's spinners turn in place: the layer would draw them over it.
        self.spin_in_layer.set(placed && !covered);
        if !self.terminals.is_empty() {
            self.terminals_covered.set(covered);
        }
        // Its terminals' and spinners' places are kept from this draw until the page draws again, which a terminal's
        // output or a spinner's step doesn't make it do.
        let body = if placed {
            ui::drawing_piece(ui::Piece::View(cx.entity_id()), id, || tree_nodes(self, cx))
        } else if self.terminals.is_empty() {
            tree_nodes(self, cx)
        } else {
            ui::drawing_piece(TERMINALS, id, || tree_nodes(self, cx))
        };
        let overlay = dialog
            .and(self.tree.as_ref())
            .and_then(|tree| dialog_in(&tree.children))
            .map(|dialog| self.dialog(dialog, cx));
        let fill = self.tree.as_ref().is_some_and(|tree| tree.fill);
        self.settle_sheets(window, cx);
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
                if state.auto_focus && may_take {
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
        #[cfg(feature = "test-support")]
        let shot_terminals = self
            .shot_terminals
            .clone()
            .map(|draw| ui::slot_layer(TERMINALS, draw(cx)).into_any_element());
        #[cfg(not(feature = "test-support"))]
        let shot_terminals: Option<AnyElement> = None;
        let page = div()
            .id("native-page")
            .capture_any_mouse_down(cx.listener(|view, event, window, cx| {
                *view.pressed_with.borrow_mut() = Some(window.focused(cx));
                view.release_selection(event, cx);
            }))
            .key_context(page_context)
            .on_key_down(cx.listener(Self::tab))
            .on_key_down(cx.listener(Self::copy_selection))
            .on_mouse_move(cx.listener(Self::drag_selection))
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                if view.press_shortcut(&event.keystroke) {
                    window.prevent_default();
                    cx.stop_propagation();
                }
            }))
            // Unless the page sizes its own text, the font keys go on to the workspace, which sizes the terminals'.
            .on_action(cx.listener(|view, _: &crate::actions::IncreaseFontSize, _, cx| {
                if !view.scale_text("bigger") {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|view, _: &crate::actions::DecreaseFontSize, _, cx| {
                if !view.scale_text("smaller") {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|view, _: &crate::actions::ResetFontSize, _, cx| {
                if !view.scale_text("reset") {
                    cx.propagate();
                }
            }))
            .relative()
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            // The card's radius less its border: the ground is as round as the card's inside.
            .when(self.in_card, |page| page.rounded(px(13.0)))
            // The desktop's page is transparent on the `bg-surface` of the card, panel or window that holds it, or on the
            // `bg-popover` of a popover. Over a backdrop, the card draws the ground.
            .when(!over_backdrop, |page| {
                page.bg(rgb(if self.popover {
                    theme::palette().dialog
                } else {
                    theme::palette().surface
                }))
                .when_some(palette.and_then(|palette| palette.ground).filter(|_| !self.popover), |page, (from, to, angle)| {
                    page.bg(gpui::linear_gradient(angle, gpui::linear_color_stop(rgb(from), 0.0), gpui::linear_color_stop(rgb(to), 1.0)))
                })
            })
            .text_color(rgb(theme::palette().foreground))
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
                                .text_color(rgb(theme::palette().danger))
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
            .child(div().absolute().size_0().track_focus(&self.selection_focus))
            .children(overlay)
            .children(shot_terminals);
        // Its fields learn they are in the page, which keeps its drawing, and under its dialog or sheet when it shows one,
        // so the one being typed in draws its text in the window's field layer only when nothing of the page is over it.
        ui::in_piece(placed.then_some(ui::Piece::View(cx.entity_id())), covered, page)
    }
}

/// Whether the page shows a sheet, wherever in its tree.
fn sheet_in(nodes: &[Node]) -> bool {
    nodes
        .iter()
        .any(|node| matches!(node, Node::Sheet(_)) || node.parts().into_iter().any(sheet_in))
}

/// The dialog the page shows, wherever in its tree it put it.
fn dialog_in(nodes: &[Node]) -> Option<&tree::Dialog> {
    nodes.iter().rev().find_map(|node| match node {
        Node::Dialog(dialog) => Some(dialog),
        other => other.parts().into_iter().rev().find_map(dialog_in),
    })
}

/// The click handler of the button in `nodes` whose `shortcut` is `keystroke`.
fn shortcut_in(nodes: &[Node], keystroke: &gpui::Keystroke) -> Option<String> {
    let mods = keystroke.modifiers;
    nodes.iter().find_map(|node| {
        let button = match node {
            Node::Button(button) => Some(button),
            Node::ListButton(item) => Some(&item.button),
            _ => None,
        };
        button
            .filter(|button| {
                button.shortcut.as_ref().is_some_and(|key| {
                    (key.cmd, key.ctrl, key.alt, key.shift) == (mods.platform, mods.control, mods.alt, mods.shift)
                        && key.key == keystroke.key
                })
            })
            .and_then(|button| button.on_click.clone())
            .or_else(|| node.parts().into_iter().find_map(|nodes| shortcut_in(nodes, keystroke)))
    })
}

/// The `onTextScale` of the page's `Page`, wherever in its tree it is.
fn text_scale_in(nodes: &[Node]) -> Option<String> {
    nodes.iter().find_map(|node| match node {
        Node::Page(page) if page.on_text_scale.is_some() => page.on_text_scale.clone(),
        other => other.parts().into_iter().find_map(text_scale_in),
    })
}

/// The popup menu the page shows, wherever in its tree it put it.
fn popup_in(nodes: &[Node]) -> Option<&tree::PopupMenu> {
    nodes.iter().rev().find_map(|node| match node {
        Node::PopupMenu(menu) => Some(menu),
        other => other.parts().into_iter().rev().find_map(popup_in),
    })
}

/// Milliseconds since 1970, as a `since` counts from.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |now| now.as_millis() as i64)
}

/// How long ago `since` was, at `now` (both ms since the epoch), as a status line counts: "16s", "2m 5s", "1h 4m".
fn took(since: i64, now: i64) -> String {
    let seconds = (now - since).max(0) / 1000;
    match seconds {
        ..60 => format!("{seconds}s"),
        ..3600 => format!("{}m {}s", seconds / 60, seconds % 60),
        _ => format!("{}h {}m", seconds / 3600, seconds / 60 % 60),
    }
}

/// How long after a click or key in a page it may still move the keyboard into itself, as a dialog it opened does.
const GESTURE_TAKES_KEYBOARD: Duration = Duration::from_secs(2);

/// How near its end, in points, a following `Scroll` still counts as there: a line's worth of a pointer's drift.
const FOLLOW_SLACK: f32 = 24.0;
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
        let dialog = self.dialog_shown.is_some().then(|| self.dialog_focus.clone());
        let inside = |window: &Window, cx: &gpui::App| match &dialog {
            // A dialog keeps the keyboard in it, as a modal does.
            Some(dialog) => dialog.contains_focused(window, cx),
            None => window
                .context_stack()
                .iter()
                .any(|context| context.get(PAGE_CONTEXT).is_some_and(|id| **id == page)),
        };
        let from = window.focused(cx);
        // Bounded, as a page with no control besides the focused one comes back to it.
        for _ in 0..1000 {
            if mods.shift {
                window.focus_prev(cx);
            } else {
                window.focus_next(cx);
            }
            if inside(window, &**cx) || window.focused(cx) == from {
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
                TextSize::Normal => (scaled(13.0), line),
                TextSize::Small => (scaled(12.0), line),
                TextSize::Tiny => (scaled(11.0), line),
                TextSize::Large => (scaled(20.0), scaled(26.0)),
                TextSize::Field => (scaled(14.0), scaled(21.0)),
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
            TextSize::Normal => scaled(13.0 * 1.45),
            TextSize::Small => scaled(12.0 * 1.45),
            TextSize::Tiny => scaled(11.0 * 1.45),
            TextSize::Large => scaled(26.0),
            TextSize::Field => scaled(14.0 * 1.5),
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
        Tone::Muted => theme::palette().muted_foreground,
        Tone::Info => theme::palette().info,
        Tone::Success => theme::palette().done,
        Tone::Warn => theme::palette().working,
        Tone::Danger => theme::palette().danger,
    }
}

/// A gauge's bar is `--wings-info` until it reaches a level.
pub(super) fn level_color(level: Level) -> u32 {
    match level {
        Level::Normal => theme::palette().info,
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

/// The most a text holds out for its longest word. A word wider than this, like a long path in a chat reply, breaks
/// inside itself as a browser's `overflow-wrap: anywhere` does: holding out for all of it pushed the line past the page's
/// edge.
const LONGEST_WORD_KEPT: f32 = 240.0;

/// The width of the widest word of `text` at `size`, in the page's font or the terminal's with `mono`, up to
/// `LONGEST_WORD_KEPT`.
/// A text's size and weight, as `text_element` sets them.
fn text_font(text: &tree::Text) -> (f32, FontWeight) {
    let size = match text.size {
        TextSize::Small => scaled(12.0),
        TextSize::Tiny => scaled(11.0),
        TextSize::Large => scaled(20.0),
        TextSize::Field => scaled(14.0),
        TextSize::Normal => scaled(13.0),
    };
    let weight = match text.weight {
        Weight::Normal => FontWeight::NORMAL,
        Weight::Medium => FontWeight::MEDIUM,
        Weight::Strong => FontWeight::SEMIBOLD,
    };
    (size, weight)
}

/// How wide `c` is in `font` at `size`. The text system asks Core Text each time, for the glyph and then its advance; a
/// long chat's page measured the words of its growing texts that way, every character on every render: about a tenth of
/// the page's draw in a profile of the app while typing there. A character's width is its width at one point times the
/// size, so each is asked once.
pub(super) fn char_width(font: gpui::FontId, size: f32, c: char, cx: &gpui::App) -> f32 {
    let unit = CHAR_WIDTHS.with_borrow_mut(|widths| {
        *widths.entry((font, c)).or_insert_with(|| {
            #[cfg(feature = "test-support")]
            CHARS_ASKED.with(|asked| asked.set(asked.get() + 1));
            cx.text_system().advance(font, px(1.0), c).map_or(0.0, |advance| advance.width.as_f32())
        })
    });
    unit * size
}

/// How many characters the page's text measuring has asked the text system for on this thread.
#[cfg(feature = "test-support")]
pub(super) fn chars_asked() -> usize {
    CHARS_ASKED.with(Cell::get)
}

/// How wide `text` is on one line.
fn line_width(text: &str, size: f32, weight: FontWeight, mono: bool, cx: &gpui::App) -> f32 {
    let font = page_font(weight, mono, cx);
    text.chars().map(|c| char_width(font, size, c, cx)).sum()
}

fn longest_word(text: &str, size: f32, weight: FontWeight, mono: bool, cx: &gpui::App) -> f32 {
    let font = page_font(weight, mono, cx);
    let width = |word: &str| word.chars().map(|c| char_width(font, size, c, cx)).sum::<f32>();
    // A point over, so rounding in the line's own layout never leaves the word a hair too wide to fit.
    (text.split_whitespace().map(width).fold(0.0, f32::max).ceil() + 1.0).min(LONGEST_WORD_KEPT)
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

/// How tall a small button is.
const SM_BUTTON_HEIGHT: f32 = 28.0;

#[cfg(test)]
#[test]
fn a_status_line_counts_seconds_then_minutes_then_hours() {
    let at = 1_763_470_000_000;
    assert_eq!(took(at, at + 16_400), "16s");
    assert_eq!(took(at, at + 125_000), "2m 5s");
    assert_eq!(took(at, at + 3_840_000), "1h 4m");
    // A clock that steps back reads as no time, not a negative one.
    assert_eq!(took(at, at - 5_000), "0s");
}

/// A page draws a terminal only by the id of one its tab was handed: any other pane, or a word it made up, is none.
#[cfg(test)]
#[test]
fn a_page_embeds_only_a_terminal_its_tab_was_handed() {
    assert_eq!(embeddable(&[3, 7], "7"), Some(7));
    assert_eq!(embeddable(&[3, 7], "4"), None);
    assert_eq!(embeddable(&[], "3"), None);
    assert_eq!(embeddable(&[3], "tab-3"), None);
    assert_eq!(embeddable(&[3], ""), None);
}
