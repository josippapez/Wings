//! The page as a tree of the components `wings-plugin-ui/native` has, by the names and props the app reads
//! (`apps/native/src/page/tree.rs`). A node's id is made from its place: its parent's id, its key or its index among
//! its parent's children, and its kind. So a field keeps its text and the keyboard from one build of the page to the
//! next as long as it stays where it is, and a row of a list keeps them when it is keyed.

use serde_json::{Map, Value, json};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

type Handler = Rc<dyn Fn(&Value)>;

pub struct Node {
    kind: &'static str,
    key: Option<String>,
    props: Map<String, Value>,
    handlers: Vec<(&'static str, Handler)>,
    children: Vec<Child>,
}

enum Child {
    Node(Node),
    Text(String),
    /// A list of nodes in one child's place, as an array among a React element's children, or nothing.
    Group(Vec<Node>),
}

impl Node {
    pub fn new(kind: &'static str) -> Self {
        Self { kind, key: None, props: Map::new(), handlers: Vec::new(), children: Vec::new() }
    }

    /// Names the node among its siblings, so it keeps its id when they change order or number.
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    pub fn prop(mut self, name: &str, value: impl Into<Value>) -> Self {
        self.props.insert(name.to_owned(), value.into());
        self
    }

    /// The handler the app calls for `name` (`onClick`, `onChange`...), with the event's payload.
    pub fn on(mut self, name: &'static str, handler: impl Fn(&Value) + 'static) -> Self {
        self.handlers.push((name, Rc::new(handler)));
        self
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
        let id = id_of(parent, &place(node.key.as_ref(), index), node.kind);
        let mut props = node.props;
        for (name, handler) in node.handlers {
            let named = format!("{id}:{name}");
            props.insert(name.to_owned(), named.clone().into());
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

/// The page's JSON, as `wings.render` takes it. The handlers of this build replace the last one's.
pub(crate) fn render(page: Vec<Node>) -> String {
    let mut handlers = HashMap::new();
    let children = build(page.into_iter().map(Child::Node).collect(), 0, &mut handlers);
    HANDLERS.with(|slot| *slot.borrow_mut() = handlers);
    json!({ "id": 0, "type": "#root", "props": {}, "children": children }).to_string()
}

pub(crate) fn dispatch(handler: &str, payload: Value) {
    if let Some(handler) = HANDLERS.with(|handlers| handlers.borrow().get(handler).cloned()) {
        handler(&payload);
    }
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

/// What a page shows when it has nothing to list.
pub fn empty(title: &str, description: &str) -> Node {
    Node::new("Empty").child(Node::new("Slot").prop("name", "title").text(title)).child(Node::new("Slot").prop("name", "description").text(description))
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
