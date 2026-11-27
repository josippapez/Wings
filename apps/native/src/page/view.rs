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
#[path = "split_view.rs"]
mod split_view;
#[path = "prompt_rail_view.rs"]
mod prompt_rail_view;
#[path = "hover_card_view.rs"]
pub(super) mod hover_card_view;
#[path = "column_window_view.rs"]
mod column_window_view;
#[path = "popup_menu_view.rs"]
mod popup_menu_view;
#[path = "sunburst_view.rs"]
mod sunburst_view;
#[path = "tab_strip_view.rs"]
mod tab_strip_view;
#[cfg(feature = "test-support")]
#[path = "bench_view.rs"]
mod bench_view;
mod actions;
mod button;
mod dialog;
mod helpers;
mod layout;
mod lifecycle;
mod node;
mod probes;
mod scroll;
mod split;
mod table;
mod text;
use helpers::*;
pub(super) use helpers::{baseline_drops, char_width, level_color, three_lines};
#[cfg(all(test, feature = "test-support"))]
pub(super) use helpers::chars_asked;

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
    menu_keys: crate::ui::behaviors::MenuKeys,
    /// The same for an open popup menu.
    popup_keys: crate::ui::behaviors::MenuKeys,
    /// Where each select and combobox was drawn, by node id, and how tall the window was: its menu is as wide, and as
    /// tall as the room on one side of it.
    select_bounds: HashMap<u64, (gpui::Bounds<Pixels>, Pixels)>,
    /// Whether each `Split` was narrower than its `collapseBelow` when it was last drawn, by node id. A split not in
    /// here hasn't been measured yet and draws its fixed pane.
    split_narrow: HashMap<u64, bool>,
    /// Whether each floating `Split` with `insetBelow` was at least that wide when last measured, and how far it sets its
    /// start in then.
    split_inset: HashMap<u64, (bool, f32)>,
    /// The width of each docked `Split` with `onResize` that was dragged: what it was set to, and the `defaultSize` the page
    /// had then, which the width is good for until the page gives another. The split's own width, as last measured.
    split_sizes: HashMap<u64, (f32, f32)>,
    split_widths: HashMap<u64, f32>,
    split_drag: Option<split_view::SplitDrag>,
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
    /// Where each `TabStrip` is scrolled, by node id.
    tab_strips: RefCell<HashMap<u64, tab_strip_view::TabStripState>>,
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
    /// Whether the page is a section of the Settings dialog (`in_dialog`): padded as its built-in sections are, and
    /// round at the dialog's bottom right, which its square ground would otherwise show past.
    in_dialog: bool,
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
    /// The `Card` the pointer is on, whose `showOnHover` buttons show.
    hovered_card: Option<u64>,
    /// While a `Card`'s children are drawn: its id, and whether one of them is a `showOnHover` button.
    drawing_card: Cell<Option<(u64, bool)>>,
    found_links: RefCell<FoundLinks>,
    /// Where its web views keep their data, none while its plugin may not show web pages; what each `WebView`'s
    /// `onState` is, by name; and how a web view tells the page where it is.
    web_profile: Option<crate::web::Profile>,
    /// Its plugin may run scripts in its web pages (`permissions.webScripts`).
    web_scripts: bool,
    web_handlers: HashMap<SharedString, (Option<String>, Option<String>)>,
    web_told: Option<crate::web::Told>,
    _web_events: Option<Task<()>>,
    /// Draws the page's terminals over it, as the workspace does, for the screenshots of a page alone.
    #[cfg(feature = "test-support")]
    shot_terminals: Option<Rc<dyn Fn(&mut gpui::App) -> Vec<(SharedString, AnyElement)>>>,
    /// Draws the layers of what moves in the page over it, as the workspace does, for the screenshots of a page alone.
    #[cfg(feature = "test-support")]
    shot_layers: Vec<gpui::Entity<ui::live::Layer>>,
    /// How many times the page has rendered, for what draws it again.
    #[cfg(feature = "test-support")]
    renders: Cell<usize>,
    /// The `showOnHover` buttons the last render drew hidden.
    #[cfg(feature = "test-support")]
    hidden_buttons: RefCell<Vec<u64>>,
    /// The nodes the typing bench leaves out of the draw (`bench_view`).
    #[cfg(feature = "test-support")]
    bench_hidden: RefCell<HashSet<u64>>,
    _events: Task<()>,
}

impl Render for PageView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui::readout::render("page");
        #[cfg(feature = "test-support")]
        {
            self.renders.set(self.renders.get() + 1);
            self.hidden_buttons.borrow_mut().clear();
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
            self.popup_keys = Default::default();
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
        #[cfg(feature = "test-support")]
        if !self.shot_layers.is_empty() {
            ui::start_draw(id);
            ui::place_view(cx.entity_id(), id);
        }
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
            .when(self.in_dialog, |page| page.rounded_br(px(13.0)))
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
                    .when(!fill && self.popover && !self.in_dialog, |shell| {
                        shell.gap(px(12.0)).pt(px(12.0)).px(px(14.0)).pb(px(14.0))
                    })
                    // Under the dialog's heading, in the 24 points and 12 between parts of its built-in sections.
                    .when(!fill && self.in_dialog, |shell| {
                        shell.gap(px(12.0)).pt(px(12.0)).px(px(24.0)).pb(px(24.0))
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
        #[cfg(feature = "test-support")]
        let page = page.children(self.shot_layers.clone());
        // Its fields learn they are in the page, which keeps its drawing, and under its dialog or sheet when it shows one,
        // so the one being typed in draws its text in the window's field layer only when nothing of the page is over it.
        ui::in_piece(placed.then_some(ui::Piece::View(cx.entity_id())), covered, page)
    }
}

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
