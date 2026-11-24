//! The page as a tree of the components `wings-plugin-ui/native` has, by the names and props the app reads
//! (`apps/native/src/page/tree.rs`). A node's id is made from its place: its parent's id, its key or its index among
//! its parent's children, and its kind. So a field keeps its text and the keyboard from one build of the page to the
//! next as long as it stays where it is, and a row of a list keeps them when it is keyed.

mod more;
pub use more::*;
mod parts;
pub use parts::*;

use serde_json::{Map, Value, json};
use std::{borrow::Cow, cell::{Cell, RefCell}, collections::{HashMap, VecDeque}, rc::Rc};

type Handler = Rc<dyn Fn(&Value)>;

pub struct Node {
    // Owned only for a node `import` made from another page's JSON.
    kind: Cow<'static, str>,
    key: Option<String>,
    props: Map<String, Value>,
    handlers: Vec<(Cow<'static, str>, Handler)>,
    children: Vec<Child>,
}

enum Child {
    Node(Node),
    Text(String),
    /// A list of nodes in one child's place, as an array among a React element's children, or nothing.
    Group(Vec<Node>),
}

impl Node {
    /// How many nodes it makes in the page, itself and all it holds. The app draws a page of at most 5,000.
    pub fn count(&self) -> usize {
        let children = self.children.iter().map(|child| match child {
            Child::Node(node) => node.count(),
            Child::Text(_) => 1,
            Child::Group(nodes) => nodes.iter().map(Node::count).sum(),
        });
        1 + children.sum::<usize>()
    }

    pub fn new(kind: &'static str) -> Self {
        Self { kind: Cow::Borrowed(kind), key: None, props: Map::new(), handlers: Vec::new(), children: Vec::new() }
    }

    /// Names the node among its siblings, so it keeps its id when they change order or number.
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// A prop the app reads by this name. `title` on a `Button` or `Text` is its hover text.
    pub fn prop(mut self, name: &str, value: impl Into<Value>) -> Self {
        self.props.insert(name.to_owned(), value.into());
        self
    }

    /// The handler the app calls for `name` (`onClick`, `onChange`...), with the event's payload.
    pub fn on(mut self, name: &'static str, handler: impl Fn(&Value) + 'static) -> Self {
        self.handlers.push((Cow::Borrowed(name), Rc::new(handler)));
        self
    }

    /// The key that clicks a `Button` while its page's tab has the keyboard, shown in its tooltip as the app's own
    /// buttons show theirs. Written as `"cmd-shift-p"`: Cmd or Ctrl, maybe Alt and Shift, and a letter, a digit, a
    /// punctuation key or F1 to F12. The app drops one it or a text field already answers to (Cmd-T, Cmd-W, Cmd-N,
    /// Cmd-comma, Cmd-1 to 9, Cmd-A, Cmd-Z...) and Ctrl-Shift without Cmd, which are the app's keys off macOS.
    /// Needs Wings 0.2.54.
    pub fn shortcut(self, keys: &str) -> Self {
        self.prop("shortcut", keys)
    }

    /// On a `Page`: the page sizes its own text, so while its tab has the keyboard Cmd-+, Cmd-- and Cmd-0 call
    /// `on_step` with `"bigger"`, `"smaller"` or `"reset"` in place of changing the terminals' font. Needs Wings 0.2.54.
    pub fn on_text_scale(self, on_step: impl Fn(&str) + 'static) -> Self {
        self.on("onTextScale", move |payload| on_step(payload["step"].as_str().unwrap_or_default()))
    }

    /// On a `Page`: what the app draws behind the whole page in place of its plain ground. `"pixels"` is a slow drift of
    /// dim, dithered pixels in the page's colours, and `"pixels:<pattern>"` another pattern of them: `rain`, `checker` or
    /// `embers` (a Wings after 0.2.60; a pattern it doesn't know is the drift). `"gradient:<preset>"` is a slow gradient mesh a shader draws, dimmed so
    /// the page's muted text stays readable on it, in the colours of one of ShaderGradient's presets: `halo`, `pensive`,
    /// `mint`, `interstella` or `nighty-night`. Either moves only while the window is in front and stands still under
    /// Reduce motion. The app draws it where the page fills a tab; elsewhere, and in an older Wings, the page keeps its
    /// plain ground. Needs Wings 0.2.59, and 0.2.60 for a gradient.
    pub fn backdrop(self, kind: &str) -> Self {
        self.prop("backdrop", kind)
    }

    pub fn child(mut self, node: Node) -> Self {
        self.children.push(Child::Node(node));
        self
    }

    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.children.push(Child::Text(text.into()));
        self
    }

    /// `node` or nothing, in a place of its own either way: what follows keeps its place.
    pub fn maybe(mut self, node: Option<Node>) -> Self {
        self.children.push(Child::Group(node.into_iter().collect()));
        self
    }

    /// Nodes in one place: what follows keeps its place however many there are. Key them when they can change order.
    pub fn children(mut self, nodes: impl IntoIterator<Item = Node>) -> Self {
        self.children.push(Child::Group(nodes.into_iter().collect()));
        self
    }
}

thread_local! {
    static HANDLERS: RefCell<HashMap<String, Handler>> = RefCell::new(HashMap::new());
    /// The pasted-image handlers that the last few builds dropped, newest build last: the app sends an event by the name
    /// it drew last, which may be of a page this plugin has since built again with that field in another place.
    static GONE: RefCell<VecDeque<HashMap<String, Handler>>> = const { RefCell::new(VecDeque::new()) };
    static FILL: Cell<bool> = const { Cell::new(false) };
    /// A handler said the page is as it was (`unchanged`).
    static UNCHANGED: Cell<bool> = const { Cell::new(false) };
    static PALETTE: RefCell<Option<Value>> = const { RefCell::new(None) };
}

/// Paints the page in `colors` in place of the app's, from its next render: each of `base` (under a floating pane),
/// `page`, `dialog`, `popover`, `text`,
/// `muted`, `info`, `danger`, `done` and `working` as `#rrggbb`, and `ground`, what the page lies on, as
/// `{ "from": "#rrggbb", "to": "#rrggbb", "angle": degrees }`. A role left out keeps the app's colour. None gives the
/// app's palette back. Needs Wings 0.2.52.
pub fn palette(colors: Option<Value>) {
    PALETTE.with(|palette| *palette.borrow_mut() = colors);
}

/// Has the page laid out edge to edge with no padding round it, as `showTab(element, { fill: true })` does.
pub(crate) fn fill() {
    FILL.with(|fill| fill.set(true));
}

/// FNV-1a over the parent's id and the node's place, kept under 2^53 so it is a whole number anywhere JSON goes.
fn id_of(parent: u64, place: &str, kind: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in parent.to_le_bytes().iter().chain(place.as_bytes()).chain(b"/").chain(kind.as_bytes()) {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    (hash & ((1 << 53) - 1)) | 1
}

fn place(key: Option<&String>, index: usize) -> String {
    key.map_or_else(|| format!("i{index}"), |key| format!("k{key}"))
}

fn build(children: Vec<Child>, parent: u64, handlers: &mut HashMap<String, Handler>) -> Vec<Value> {
    let mut out = Vec::new();
    let mut node = |node: Node, parent: u64, index: usize, out: &mut Vec<Value>| {
        let id = id_of(parent, &place(node.key.as_ref(), index), &node.kind);
        let mut props = node.props;
        for (name, handler) in node.handlers {
            let named = format!("{id}:{name}");
            props.insert(name.into_owned(), named.clone().into());
            handlers.insert(named, handler);
        }
        out.push(json!({ "id": id, "type": node.kind, "props": props, "children": build(node.children, id, handlers) }));
    };
    for (index, child) in children.into_iter().enumerate() {
        match child {
            Child::Node(child) => node(child, parent, index, &mut out),
            Child::Text(text) => out.push(json!({ "id": id_of(parent, &place(None, index), "#text"), "type": "#text", "text": text })),
            Child::Group(nodes) => {
                let group = id_of(parent, &place(None, index), "#group");
                for (at, child) in nodes.into_iter().enumerate() {
                    node(child, group, at, &mut out);
                }
            }
        }
    }
    out
}

/// A node as the JSON the app reads, for a test to look at what a page drew. Its handlers are left behind.
pub fn tree_json(node: Node) -> Value {
    build(vec![Child::Node(node)], 0, &mut HashMap::new()).remove(0)
}

/// The handlers of an `export`, by the name the exported JSON gives each.
#[derive(Default)]
pub struct Handlers(HashMap<String, Handler>);

impl Handlers {
    /// Runs the handler named `name` with the event's payload: false when this export has none by that name.
    pub fn call(&self, name: &str, payload: &Value) -> bool {
        let Some(handler) = self.0.get(name).cloned() else {
            return false;
        };
        handler(payload);
        true
    }
}

/// Nodes drawn for another page of the plugin to show, as a chat draws its summary for a sidebar: the JSON the app reads,
/// to send with `pages::tell`, and the handlers, which stay here. The other page shows the JSON with `import`, and hands
/// each event back by its handler's name to be run with `Handlers::call`. A node's handler name is the same from one
/// export to the next while the node keeps its place, so an event for an older export still reaches it.
pub fn export(nodes: Vec<Node>) -> (Value, Handlers) {
    let mut handlers = HashMap::new();
    let tree = build(nodes.into_iter().map(Child::Node).collect(), 0, &mut handlers);
    (Value::Array(tree), Handlers(handlers))
}

/// The nodes of an `export`'s JSON, to draw in this page: each event of theirs calls `on` with the handler's name and the
/// payload, for the page that exported them to run. Each node is keyed by its exported id, so it keeps its place here as
/// it does there.
pub fn import(tree: &Value, on: &Rc<dyn Fn(&str, &Value)>) -> Vec<Node> {
    tree.as_array().into_iter().flatten().filter_map(|node| imported(node, on)).collect()
}

fn imported(tree: &Value, on: &Rc<dyn Fn(&str, &Value)>) -> Option<Node> {
    let kind = tree["type"].as_str().filter(|kind| !kind.starts_with('#'))?;
    let id = tree["id"].as_u64()?;
    let mut node = Node { kind: Cow::Owned(kind.to_owned()), key: Some(id.to_string()), props: Map::new(), handlers: Vec::new(), children: Vec::new() };
    for (name, value) in tree["props"].as_object().into_iter().flatten() {
        // `build` writes a handler as the string `<id>:<prop>`, which no other prop of this node holds.
        match value.as_str().filter(|named| *named == format!("{id}:{name}")) {
            Some(named) => {
                let (on, named) = (on.clone(), named.to_owned());
                node.handlers.push((Cow::Owned(name.clone()), Rc::new(move |payload: &Value| on(&named, payload))));
            }
            None => {
                node.props.insert(name.clone(), value.clone());
            }
        }
    }
    for child in tree["children"].as_array().into_iter().flatten() {
        match child["type"].as_str() {
            Some("#text") => node.children.push(Child::Text(child["text"].as_str().unwrap_or_default().to_owned())),
            _ => node.children.extend(imported(child, on).map(Child::Node)),
        }
    }
    Some(node)
}

/// How many builds a dropped handler is kept for: the app may be that many builds behind the page when its event is sent.
const GONE_BUILDS: usize = 16;

/// The one event kept past its node's move: a paste carries its data and isn't about an item of a list. A click's handler
/// may hold the index of a row the page has since changed, so it is never run from an older build.
const KEPT: &str = ":onPasteImage";

/// The page's JSON, as `wings.render` takes it. The handlers of this build replace the last one's, but a pasted-image
/// handler this build has no node for keeps answering for `GONE_BUILDS` builds.
pub(crate) fn render(page: Vec<Node>) -> String {
    let mut handlers = HashMap::new();
    let children = build(page.into_iter().map(Child::Node).collect(), 0, &mut handlers);
    let last = HANDLERS.with(|slot| slot.replace(handlers));
    HANDLERS.with(|slot| {
        let now = slot.borrow();
        GONE.with(|gone| {
            let mut gone = gone.borrow_mut();
            gone.push_back(last.into_iter().filter(|(name, _)| name.ends_with(KEPT) && !now.contains_key(name)).collect());
            if gone.len() > GONE_BUILDS {
                gone.pop_front();
            }
        });
    });
    let mut props = if FILL.with(Cell::get) { json!({ "fill": true }) } else { json!({}) };
    if let Some(palette) = PALETTE.with(|palette| palette.borrow().clone()) {
        props["palette"] = palette;
    }
    json!({ "id": 0, "type": "#root", "props": props, "children": children }).to_string()
}

/// Runs the handler `handler` names. True when it called `unchanged`, so the page needn't be built for it.
pub(crate) fn dispatch(handler: &str, payload: Value) -> bool {
    UNCHANGED.with(|flag| flag.set(false));
    let found = HANDLERS
        .with(|handlers| handlers.borrow().get(handler).cloned())
        .or_else(|| GONE.with(|gone| gone.borrow().iter().rev().find_map(|dropped| dropped.get(handler).cloned())));
    if let Some(handler) = found {
        handler(&payload);
    }
    UNCHANGED.with(|flag| flag.replace(false))
}

/// The name of the handler whose name ends in `suffix`, for a test to send its event.
#[cfg(test)]
pub(crate) fn handler_named(suffix: &str) -> String {
    HANDLERS.with(|handlers| handlers.borrow().keys().find(|name| name.ends_with(suffix)).cloned().expect("a handler"))
}

/// Called from an event handler: the page would be built exactly as it was before this event, so it isn't built, and
/// nothing is compared or sent. The page is built as usual at the next turn that doesn't say so, from the state as it is
/// then, so a change this event made that the page does show still arrives, a turn later. Say it only when sure: a
/// handler that changed something drawn and called this leaves the screen behind until something else changes the page.
/// A message box's `onChange` that only stores the text, when nothing drawn depends on it, is what this is for: the app
/// shows the typed text itself, and building a long page for every key cost a core's fifth at eight keys a second.
pub fn unchanged() {
    UNCHANGED.with(|flag| flag.set(true));
}

// ---------- the components, as `wings-plugin-ui/native` writes them ----------

pub fn row() -> Node {
    Node::new("Row")
}
pub fn stack() -> Node {
    Node::new("Stack")
}
pub fn card() -> Node {
    Node::new("Card")
}
/// A column set in from a bar down its left edge, as a Markdown quote is drawn. The bar is part of the box: it runs the
/// quote's whole height and is never copied with its text. Needs Wings 0.2.72; an older Wings draws it without the bar.
pub fn quote(children: impl IntoIterator<Item = Node>) -> Node {
    Node::new("Stack").prop("quote", true).children(children)
}
/// A card that, short of room (as a floating panel taller than the page), shrinks to fit and scrolls what it holds inside,
/// its edge and corners still drawn. Needs Wings 0.2.71; an older Wings draws a plain card.
pub fn scrolling_card() -> Node {
    Node::new("Card").prop("scroll", true)
}
/// `children` in a column that is as tall as they are up to `max_height` points and scrolls inside itself past that, so a
/// long list in a section shows all its rows without making the section long. The wheel over it scrolls it while it can go
/// that way and what holds it when it can't. Needs Wings 0.2.75; an older Wings draws it at full height.
pub fn scroll_within(max_height: u32, children: impl IntoIterator<Item = Node>) -> Node {
    Node::new("Scroll").prop("maxHeight", max_height).child(Node::new("Stack").children(children))
}
/// A card of `rows`, each split from the one above by a hairline, as Wings' Settings groups its rows. Needs Wings 0.2.70;
/// an older Wings draws a plain card.
pub fn rows_card(rows: impl IntoIterator<Item = Node>) -> Node {
    Node::new("Card").prop("variant", "rows").children(rows)
}
pub fn list() -> Node {
    Node::new("List")
}
pub fn text(text: impl Into<String>) -> Node {
    Node::new("Text").text(text)
}
pub fn muted(text: impl Into<String>) -> Node {
    self::text(text).prop("tone", "muted")
}
pub fn title(text: impl Into<String>) -> Node {
    self::text(text).prop("weight", "strong")
}
pub fn spinner() -> Node {
    Node::new("Spinner")
}
pub fn dot() -> Node {
    Node::new("Dot")
}
pub fn heading(level: u8, text: impl Into<String>) -> Node {
    Node::new("Heading").prop("level", level).text(text)
}

/// A button showing `label`. A busy one shows a spinner before it and takes no clicks.
pub fn button(label: impl Into<String>, variant: &str, busy: bool, on_click: impl Fn() + 'static) -> Node {
    let button = Node::new("Button").prop("variant", variant).on("onClick", move |_| on_click());
    if busy { button.prop("disabled", true).child(spinner()).text(label) } else { button.text(label) }
}

/// What a component takes by name, like a gauge's `note` or a disclosure's `summary`.
pub fn slot(name: &str, node: Node) -> Node {
    Node::new("Slot").prop("name", name).child(node)
}

/// A labelled bar with its percentage. `levels` are where it turns amber and red, and `note` goes under it.
pub fn gauge(label: impl Into<String>, percent: f64, levels: (u32, u32), note: Option<String>) -> Node {
    Node::new("Gauge")
        .prop("label", label.into())
        .prop("percent", percent)
        .prop("levels", json!([levels.0, levels.1]))
        .maybe(note.filter(|note| !note.is_empty()).map(|note| Node::new("Slot").prop("name", "note").text(note)))
}

/// More behind a `summary` line that opens it. The app folds and unfolds it on its own.
pub fn disclosure(summary: impl Into<String>, body: Node) -> Node {
    Node::new("Disclosure").child(Node::new("Slot").prop("name", "summary").text(summary)).child(slot("body", body))
}

/// A dialog over the page with `title` and a close button, `width` points wide at most, holding the children you add.
/// The app draws it over the whole page wherever it is in the tree, and calls `on_close` on Escape, a press beside it or
/// its close button: leave it out of the page then. Needs Wings 0.2.36.
pub fn dialog(title: &str, width: u32, on_close: impl Fn() + 'static) -> Node {
    Node::new("Dialog").prop("title", title).prop("width", width).on("onClose", move |_| on_close())
}

/// What a page shows when it has nothing to list.
pub fn empty(title: &str, description: &str) -> Node {
    Node::new("Empty").child(Node::new("Slot").prop("name", "title").text(title)).child(Node::new("Slot").prop("name", "description").text(description))
}

/// A page that fills its frame: a `toolbar` over the `body`.
pub fn page(toolbar: Node, body: impl IntoIterator<Item = Node>) -> Node {
    Node::new("Page").child(slot("toolbar", toolbar)).child(Node::new("Slot").prop("name", "body").children(body))
}

/// A page's top row: its `title` on the left and `actions` on the right.
pub fn toolbar(title: &str, actions: Option<Node>) -> Node {
    Node::new("Toolbar").child(Node::new("Slot").prop("name", "title").text(title)).maybe(actions.map(|actions| slot("actions", actions)))
}

/// A page's top row with `children` after its title, like the branch a repo is on.
pub fn toolbar_with(title: &str, children: Node, actions: Option<Node>) -> Node {
    Node::new("Toolbar")
        .child(Node::new("Slot").prop("name", "title").text(title))
        .child(slot("children", children))
        .maybe(actions.map(|actions| slot("actions", actions)))
}

/// A panel that slides in over what holds it from the right, as Wings' Past sessions: its `title` and `description`,
/// the `header` under them (a `search_field` and `chips` there are the sheet's own search and toggle), then `body`,
/// which scrolls. `on_close` hears Escape, a press beside it and its close button. Put it last in the page's body.
/// Wings 0.2.37.
pub fn sheet(title: &str, description: &str, header: impl IntoIterator<Item = Node>, body: impl IntoIterator<Item = Node>, on_close: impl Fn() + 'static) -> Node {
    Node::new("Sheet")
        .child(Node::new("Slot").prop("name", "title").text(title))
        .child(Node::new("Slot").prop("name", "description").text(description))
        .child(Node::new("Slot").prop("name", "header").children(header))
        .children(body)
        .on("onClose", move |_| on_close())
}

/// A button that is a whole row of a `List`, to put in a `list_row`: `children` side by side, or one under the other
/// with `.prop("layout", "column")`. `.prop("indent", n)` moves it in by `n` points.
pub fn list_button(children: impl IntoIterator<Item = Node>, on_click: impl Fn() + 'static) -> Node {
    Node::new("ListButton").on("onClick", move |_| on_click()).children(children)
}

/// A row of a `List` holding `content`, named `key` among the rows.
pub fn list_row(key: impl Into<String>, content: Node) -> Node {
    Node::new("ListItem").key(key).child(content)
}

/// What an `entry` shows: an optional `lead` (an icon, a spinner, a ✓), its `title`, a `detail` under the title, and
/// `trailing` at the row's end (a badge, a small button, a count).
pub struct Entry {
    pub lead: Option<Node>,
    pub title: Node,
    pub detail: Option<Node>,
    pub trailing: Option<Node>,
}

/// A row of a list, laid out by the app: the lead in a box of one width, so every row's title starts at one x, the
/// title over its detail, and `trailing` at the end. Add `.on("onClick", ...)` and it is a list button; without one it
/// sits in the same inset, so it lines up with the rows that are buttons and isn't called one. `.prop("title", ...)`
/// is a clickable one's hover text. Put entries in a `section`. Needs Wings 0.2.57.
pub fn entry(entry: Entry) -> Node {
    Node::new("ListRow")
        .maybe(entry.lead.map(|lead| slot("lead", lead)))
        .child(slot("title", entry.title))
        .maybe(entry.detail.map(|detail| slot("detail", detail)))
        .maybe(entry.trailing.map(|trailing| slot("trailing", trailing)))
}

/// A group of rows under a heading, laid out by the app: the heading and the `description` start where the rows' text
/// does, the description is muted and at most about 80 characters wide, and the `rows` (`entry`s, `list_button`s) are
/// one under the other. Needs Wings 0.2.57.
pub fn section(title: impl Into<String>, description: Option<&str>, rows: impl IntoIterator<Item = Node>) -> Node {
    Node::new("Section")
        .child(Node::new("Slot").prop("name", "title").text(title))
        .maybe(description.map(|said| Node::new("Slot").prop("name", "description").text(said)))
        .children(rows)
}

/// A `section` the person can fold: the heading gets a chevron, the whole heading line is the button (Enter and Space
/// too while it has the keyboard), and while `open` is false the rows and the description are not drawn, so a closed
/// section sends no rows. Put the count in `title` ("Subagents · 4 running"), which stays when it is closed. The app
/// calls `on_toggle` with the state the person asked for: keep it (in storage, to survive a restart) and pass it as
/// `open` on the next draw. Needs Wings 0.2.60; an older Wings ignores the props and draws the section open.
pub fn section_folding(
    title: impl Into<String>,
    description: Option<&str>,
    open: bool,
    on_toggle: impl Fn(bool) + 'static,
    rows: impl IntoIterator<Item = Node>,
) -> Node {
    section(title, description, rows)
        .prop("collapsible", true)
        .prop("open", open)
        .on("onOpenChange", move |event| on_toggle(event["open"].as_bool().unwrap_or(!open)))
}

/// A dialog's or a confirm's buttons at the end of the line: `dismiss` (Cancel, Close) first, which the app draws
/// ghost whatever its variant, then `act`, the buttons that do something, last of which the main one. Needs Wings 0.2.57.
pub fn actions(dismiss: Option<Node>, act: impl IntoIterator<Item = Node>) -> Node {
    Node::new("Actions").maybe(dismiss.map(|dismiss| slot("dismiss", dismiss))).children(act)
}

/// A search box. The app keeps its text: `on_input` hears it after each change, and `text` is only what it starts with.
pub fn search_field(label: &str, text: &str, on_input: impl Fn(&str) + 'static) -> Node {
    Node::new("SearchField")
        .prop("label", label)
        .prop("placeholder", label)
        .prop("defaultValue", text)
        .on("onInput", move |event| on_input(event["value"].as_str().unwrap_or_default()))
}

/// What a `graph` is given: the rows from `offset` on that the app last asked for, of `total`.
pub struct Graph {
    pub total: usize,
    pub lanes: usize,
    /// The row that is selected, or -1.
    pub selected: i64,
    pub offset: usize,
    /// Each `{ hash, subject, author, date, refs, lane, color, merge, lines }`.
    pub rows: Vec<Value>,
}

/// A commit graph, which the app scrolls on its own: only the rows it shows cross over. `on_range` hears the rows it
/// wants next, as `(start, end)`, and `on_select` the row that was clicked.
pub fn graph(g: Graph, on_range: impl Fn(usize, usize) + 'static, on_select: impl Fn(usize) + 'static) -> Node {
    let index = |value: &Value| value.as_u64().unwrap_or(0) as usize;
    Node::new("Graph")
        .prop("total", g.total)
        .prop("lanes", g.lanes)
        .prop("selected", g.selected)
        .prop("offset", g.offset)
        .prop("rows", g.rows)
        .on("onRangeChange", move |event| on_range(index(&event["start"]), index(&event["end"])))
        .on("onSelect", move |event| on_select(index(&event["index"])))
}

/// A line that tells what happened, in the tone `success`, `warn` or `danger`.
pub fn notice(tone: &str, text: impl Into<String>) -> Node {
    Node::new("Notice").prop("tone", tone).text(text)
}

/// A small label, like a branch's state. With a tone it is filled, without one outlined.
pub fn badge(label: impl Into<String>, tone: Option<&str>) -> Node {
    match tone {
        Some(tone) => Node::new("Badge").prop("tone", tone),
        None => Node::new("Badge").prop("variant", "outline"),
    }
    .text(label)
}

/// A checkbox with its label. `on_change` hears its next state.
pub fn checkbox(label: impl Into<String>, checked: bool, on_change: impl Fn(bool) + 'static) -> Node {
    Node::new("Checkbox").prop("checked", checked).on("onChange", move |event| on_change(event["checked"] == true)).text(label)
}

/// A card in the page that asks before something is done: its `title`, a `body`, and `actions`, the buttons that do
/// it or cancel.
pub fn confirm(title: impl Into<String>, body: impl IntoIterator<Item = Node>, actions: impl IntoIterator<Item = Node>) -> Node {
    Node::new("Confirm")
        .child(Node::new("Slot").prop("name", "title").text(title))
        .child(Node::new("Slot").prop("name", "body").children(body))
        .child(Node::new("Slot").prop("name", "actions").children(actions))
}

/// What shows before the first data comes: "Loading…".
pub fn loading() -> Node {
    Node::new("Loading")
}

/// One of `options`, as `(value, label)`, picked from a menu named `label`. `on_change` hears the value picked.
pub fn select(label: &str, options: &[(String, String)], value: &str, on_change: impl Fn(&str) + 'static) -> Node {
    let options: Vec<Value> = options.iter().map(|(value, label)| json!({ "value": value, "label": label })).collect();
    Node::new("Select")
        .prop("label", label)
        .prop("options", options)
        .prop("value", value)
        .on("onChange", move |event| on_change(event["value"].as_str().unwrap_or_default()))
}

/// A small quiet button that is on or off, drawn as one of `chips` is.
pub fn chip(label: impl Into<String>, pressed: bool, on_click: impl Fn() + 'static) -> Node {
    Node::new("Button").prop("chip", true).prop("pressed", pressed).on("onClick", move |_| on_click()).text(label)
}

/// One of `options`, as `(value, label)`, picked from a row of chips named `label`. `on_change` hears the value picked.
pub fn chips(label: &str, options: &[(&str, &str)], value: &str, on_change: impl Fn(&str) + 'static) -> Node {
    let options: Vec<Value> = options.iter().map(|(value, label)| json!({ "value": value, "label": label })).collect();
    Node::new("Chips")
        .prop("label", label)
        .prop("options", options)
        .prop("value", value)
        .on("onChange", move |event| on_change(event["value"].as_str().unwrap_or_default()))
}

/// A row of a `List`: `children`, and `actions` that show over its end while you point at it or tab into them.
pub fn list_item(children: impl IntoIterator<Item = Node>, actions: Node) -> Node {
    Node::new("ListItem").children(children).child(Node::new("Slot").prop("name", "actions").child(actions))
}

/// A one-line text field. `on_change` hears the text after each change. With `.prop("type", "time")` it takes a time
/// as HH:MM, and with `"password"` it shows dots.
pub fn text_input(label: &str, value: &str, on_change: impl Fn(String) + 'static) -> Node {
    Node::new("TextInput")
        .prop("label", label)
        .prop("value", value)
        .on("onChange", move |event| on_change(event["value"].as_str().unwrap_or_default().to_owned()))
}

/// A control with its label before it on one line.
pub fn field_inline(label: &str, control: Node) -> Node {
    stack().prop("gap", "s").child(row().child(text(label).prop("weight", "medium").prop("size", "field")).child(control))
}

/// A drawing of `paths` in a `size`-wide view box, 14 points wide, in the colour of muted text.
pub fn icon(size: u8, stroke: f64, paths: &[&str]) -> Node {
    Node::new("Svg")
        .prop("viewBox", format!("0 0 {size} {size}"))
        .prop("size", 14)
        .prop("tone", "muted")
        .prop("fill", "none")
        .prop("stroke", "currentColor")
        .prop("strokeWidth", stroke)
        .prop("strokeLinecap", "round")
        .prop("strokeLinejoin", "round")
        .prop("shapes", paths.iter().map(|d| json!(["path", { "d": d }])).collect::<Vec<_>>())
}

/// A picture registered with `host::register_image` under `id` (Wings 0.2.40), as big as `max_width` by `max_height`
/// points allow: scaled down to fit, never up, and with `.prop("fit", "cover")` filling the box and cropping instead.
/// `label` is what a screen reader says. More props: `radius` (points), `fitWindow` (also keep within the window, for a
/// picture in a dialog) and `onClick`. It is drawn only from an id, never from a path or a URL, and a picture Wings no
/// longer keeps shows as a small note.
pub fn image(id: &str, label: &str, max_width: u32, max_height: u32) -> Node {
    Node::new("Image").prop("src", id).prop("label", label).prop("maxWidth", max_width).prop("maxHeight", max_height)
}

pub struct Choice {
    pub value: String,
    pub label: String,
    pub detail: Option<String>,
}

/// What a `combobox` is given.
pub struct Combobox {
    pub label: &'static str,
    pub placeholder: &'static str,
    /// What the field holds: the text of a free-text field, or the label of what a picker is on.
    pub value: String,
    /// A picker's search since it last took the keyboard or a pick, kept by the page. `None` for a free-text field,
    /// whose search is its text.
    pub query: Option<Option<String>>,
    pub options: Vec<Choice>,
    /// Shown when something is searched for and nothing matches.
    pub empty: &'static str,
    pub auto_focus: bool,
}

/// A text field with its matching options under it, which the app draws as a list over the page: the tree
/// `wings-plugin-ui/native`'s `Combobox` renders. `on_text` hears a free-text field's text, or a picker's search (and
/// `None` when it loses the keyboard, which ends the search); `on_pick` the value picked.
pub fn combobox(o: Combobox, on_text: impl Fn(Option<String>) + 'static, on_pick: impl Fn(&str) + 'static) -> Node {
    let free_text = o.query.is_none();
    let search = match &o.query {
        None => o.value.clone(),
        Some(query) => query.clone().unwrap_or_default(),
    };
    let wanted = search.to_lowercase();
    let mut found: Vec<&Choice> =
        o.options.iter().filter(|c| format!("{} {}", c.label, c.detail.as_deref().unwrap_or_default()).to_lowercase().contains(&wanted)).collect();
    if free_text {
        found.truncate(8);
    }
    let done = free_text && found.len() == 1 && found[0].value == o.value;
    let (on_text, on_pick) = (Rc::new(on_text), Rc::new(on_pick));
    let shown = match &o.query {
        Some(Some(query)) => query.clone(),
        _ => o.value.clone(),
    };
    let typed = on_text.clone();
    let mut field = text_input(o.label, &shown, move |text| typed(Some(text))).prop("placeholder", o.placeholder).prop("maxLength", 2000);
    if o.auto_focus {
        field = field.prop("autoFocus", true);
    }
    if !free_text {
        let left = on_text.clone();
        field = field.prop("selectOnFocus", true).on("onBlur", move |_| left(None));
    }
    let options = (!done && !found.is_empty()).then(|| {
        row().prop("gap", "s").prop("wrap", true).children(found.iter().map(|choice| {
            let (pick, value) = (on_pick.clone(), choice.value.clone());
            let chip = Node::new("Button").key(choice.value.clone()).prop("chip", true).prop("pressed", false).on("onClick", move |_| pick(&value)).text(choice.label.clone());
            match &choice.detail {
                Some(detail) => chip.child(muted(detail.clone()).prop("size", "small")),
                None => chip,
            }
        }))
    });
    let nothing = (!done && found.is_empty() && !search.is_empty()).then(|| muted(o.empty).prop("size", "small"));
    stack().prop("gap", "s").child(field).maybe(options.or(nothing))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_imported_export_draws_the_same_and_hands_its_clicks_back_to_the_exported_handlers() {
        let clicked = Rc::new(Cell::new(0));
        let counted = clicked.clone();
        let drawn = || stack().prop("gap", "s").child(text("Changes")).child(button("Stop", "ghost", false, || {}).key("task-1"));
        let (tree, handlers) = export(vec![drawn().child(button("Open", "ghost", false, move || counted.set(counted.get() + 1)).key("open"))]);
        // Drawn again in another page, it has the same nodes, props and text.
        let heard = Rc::new(RefCell::new(Vec::new()));
        let kept = heard.clone();
        let on: Rc<dyn Fn(&str, &Value)> = Rc::new(move |name, payload| kept.borrow_mut().push((name.to_owned(), payload.clone())));
        let shown = import(&tree, &on);
        assert_eq!(shown.len(), 1);
        let again = render(shown);
        assert!(again.contains("\"Changes\"") && again.contains("\"Open\"") && again.contains("\"gap\":\"s\""), "{again}");
        // A click there names the handler, which runs here.
        let open = HANDLERS.with(|handlers| handlers.borrow().keys().filter(|name| name.ends_with(":onClick")).cloned().collect::<Vec<_>>());
        for name in open {
            dispatch(&name, json!({}));
        }
        let names: Vec<String> = heard.borrow().iter().map(|(name, _)| name.clone()).collect();
        assert_eq!(names.len(), 2);
        for name in &names {
            assert!(handlers.call(name, &json!({})));
        }
        assert_eq!(clicked.get(), 1);
        assert!(!handlers.call("1:onClick", &json!({})));
        // The same nodes exported again name their handlers the same, so a click on an older drawing still lands.
        let (_, later) = export(vec![drawn().child(button("Open", "ghost", false, || {}).key("open"))]);
        assert!(names.iter().all(|name| later.call(name, &json!({}))));
    }

    /// The names of the slots `node` sends, in order.
    fn slots(node: Node) -> Vec<String> {
        tree_json(node)["children"].as_array().unwrap().iter().filter(|child| child["type"] == "Slot").map(|slot| slot["props"]["name"].as_str().unwrap().to_owned()).collect()
    }

    #[test]
    fn an_entry_a_section_and_actions_send_their_parts_as_the_slots_the_app_reads() {
        let full = Entry { lead: Some(spinner()), title: text("Build"), detail: Some(muted("Running")), trailing: Some(badge("2", None)) };
        assert_eq!(slots(entry(full)), ["lead", "title", "detail", "trailing"]);
        assert_eq!(slots(entry(Entry { lead: None, title: text("Earlier"), detail: None, trailing: None })), ["title"]);
        assert_eq!(slots(section("Folders", Some("Added with /add-dir."), [text("a")])), ["title", "description"]);
        assert_eq!(slots(section("Folders", None, Vec::new())), ["title"]);
        let folding = tree_json(section_folding("Folders", None, false, |_| {}, [text("a")]));
        assert_eq!((folding["props"]["collapsible"].clone(), folding["props"]["open"].clone()), (json!(true), json!(false)));
        let drawn = tree_json(actions(Some(button("Cancel", "outline", false, || {})), [button("Save", "default", false, || {})]));
        assert_eq!(drawn["type"], "Actions");
        assert_eq!(drawn["children"][0]["props"]["name"], "dismiss");
        assert_eq!(drawn["children"][1]["children"][0]["text"], "Save");
    }
}
