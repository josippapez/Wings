//! The tree a page sends, parsed from its JSON and checked, so the view draws only what it knows and a page can't
//! make it hold or draw more than these limits.
//!
//! A page's host components are `wings-plugin-ui/native`'s: a component with named parts (a page's toolbar, an empty
//! state's description) sends each part as a `Slot` child, and a component the host doesn't know is dropped with
//! everything in it.

use serde::Deserialize;
use serde_json::{Map, Value};
use std::fmt;

/// The largest tree JSON the host parses.
pub const MAX_JSON_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_NODES: usize = 5_000;
pub const MAX_DEPTH: usize = 32;
/// A longer text is cut to this many characters.
pub const MAX_TEXT_CHARS: usize = 2_000;
const MAX_HANDLER_BYTES: usize = 64;
/// A select shows this many of its options; the rest are dropped.
pub const MAX_OPTIONS: usize = 200;

#[derive(Debug, PartialEq, Eq)]
pub enum TreeError {
    TooLarge,
    TooManyNodes,
    TooDeep,
    Invalid(String),
}

impl fmt::Display for TreeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TreeError::TooLarge => write!(f, "the page's tree is over {MAX_JSON_BYTES} bytes"),
            TreeError::TooManyNodes => write!(f, "the page's tree has over {MAX_NODES} nodes"),
            TreeError::TooDeep => write!(f, "the page's tree is nested over {MAX_DEPTH} deep"),
            TreeError::Invalid(why) => write!(f, "the page's tree isn't valid: {why}"),
        }
    }
}

impl std::error::Error for TreeError {}

/// A string a component prop can pick from, by its name in ui.d.ts.
trait Choice: Sized {
    fn choose(text: &str) -> Option<Self>;
}

macro_rules! choice {
    ($(#[$doc:meta])* $name:ident { $($variant:ident = $text:literal),+ $(,)? }) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        pub enum $name {
            #[default]
            $($variant),+
        }

        impl Choice for $name {
            fn choose(text: &str) -> Option<Self> {
                match text {
                    $($text => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

// The first variant of each is what a missing or unknown value means, as in ui.d.ts.
choice!(Gap { M = "m", None = "none", Xs = "xs", S = "s", L = "l" });
choice!(Align { Center = "center", Start = "start", Baseline = "baseline" });
choice!(Justify { Start = "start", End = "end", Between = "between" });
choice!(TextSize { Normal = "normal", Small = "small", Tiny = "tiny" });
choice!(Weight { Normal = "normal", Medium = "medium", Strong = "strong" });
choice!(
    /// The variants of both `Button` and `Badge`.
    Variant { Default = "default", Secondary = "secondary", Outline = "outline", Ghost = "ghost", Destructive = "destructive", Link = "link" }
);

/// A colour that says something. A text or badge without one has the plain colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Muted,
    Info,
    Success,
    Warn,
    Danger,
}

impl Choice for Tone {
    fn choose(text: &str) -> Option<Self> {
        Some(match text {
            "muted" => Tone::Muted,
            "info" => Tone::Info,
            "success" => Tone::Success,
            "warn" => Tone::Warn,
            "danger" => Tone::Danger,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tree {
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    /// A string a component holds, like a button's label.
    Str {
        id: u64,
        text: String,
    },
    Slot(Slot),
    Page(Page),
    Toolbar(Toolbar),
    Stack(Stack),
    Row(Row),
    Text(Text),
    Button(Button),
    Notice(Notice),
    Spinner(Spinner),
    Checkbox(Checkbox),
    Confirm(Confirm),
    Badge(Badge),
    Empty(Empty),
    Loading(Loading),
    Card(Card),
    Heading(Heading),
    Dot(Dot),
    Gauge(Gauge),
    List(List),
    ListItem(ListItem),
    Disclosure(Disclosure),
    Select(Select),
    Chips(Select),
    ListButton(ListButton),
    SearchField(SearchField),
    Graph(Graph),
}

/// A named part of the component it's in.
#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    pub name: String,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub id: u64,
    pub toolbar: Vec<Node>,
    pub flush: bool,
    pub body: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Toolbar {
    pub id: u64,
    pub title: Vec<Node>,
    pub filters: Vec<Node>,
    pub actions: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stack {
    pub id: u64,
    pub gap: Gap,
    pub grow: bool,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: u64,
    pub gap: Gap,
    pub align: Align,
    pub justify: Justify,
    pub wrap: bool,
    pub grow: bool,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub id: u64,
    pub tone: Option<Tone>,
    pub size: TextSize,
    pub weight: Weight,
    pub mono: bool,
    pub truncate: bool,
    pub clamp: bool,
    pub grow: bool,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Button {
    pub id: u64,
    pub variant: Variant,
    pub disabled: bool,
    /// What to send back to the page when it's clicked. A disabled button has none.
    pub on_click: Option<String>,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub id: u64,
    pub tone: Option<Tone>,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Spinner {
    pub id: u64,
    pub large: bool,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Checkbox {
    pub id: u64,
    pub checked: bool,
    pub disabled: bool,
    pub on_change: Option<String>,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Confirm {
    pub id: u64,
    pub title: Vec<Node>,
    pub body: Vec<Node>,
    pub actions: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Badge {
    pub id: u64,
    pub variant: Variant,
    pub tone: Option<Tone>,
    pub small: bool,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Empty {
    pub id: u64,
    pub title: Vec<Node>,
    pub description: Vec<Node>,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Loading {
    pub id: u64,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub id: u64,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Heading {
    pub id: u64,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Dot {
    pub id: u64,
    /// Green, the plain colour, when there is none.
    pub tone: Option<Tone>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Gauge {
    pub id: u64,
    pub label: String,
    /// From 0 to 100: a bar can't be fuller than full, so what the page sends is cut to that.
    pub percent: f64,
    /// Where the bar turns warn and danger.
    pub levels: Option<(f64, f64)>,
    pub note: Vec<Node>,
}

/// How full a gauge is, by its `levels`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Normal,
    Warn,
    Danger,
}

impl Gauge {
    pub fn level(&self) -> Level {
        match self.levels {
            Some((_, danger)) if self.percent >= danger => Level::Danger,
            Some((warn, _)) if self.percent >= warn => Level::Warn,
            _ => Level::Normal,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct List {
    pub id: u64,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ListItem {
    pub id: u64,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Disclosure {
    pub id: u64,
    pub summary: Vec<Node>,
    pub body: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
    pub disabled: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Select {
    pub id: u64,
    /// The select's accessible name.
    pub label: String,
    pub options: Vec<SelectOption>,
    pub value: Option<String>,
    /// What to send back to the page, with the value picked. A select with none can still be opened.
    pub on_change: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ListButton {
    pub button: Button,
    pub indent: f32,
    pub expanded: Option<bool>,
    pub vertical: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SearchField {
    pub id: u64,
    pub label: String,
    pub placeholder: String,
    pub value: Option<String>,
    pub on_input: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphRow {
    pub hash: String,
    pub subject: String,
    pub author: String,
    pub date: String,
    pub refs: Vec<String>,
    pub lane: usize,
    pub color: usize,
    pub merge: bool,
    pub lines: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Graph {
    pub id: u64,
    pub total: usize,
    pub lanes: usize,
    pub selected: i64,
    pub offset: usize,
    pub rows: Vec<GraphRow>,
    pub on_range: Option<String>,
    pub on_select: Option<String>,
}

fn graph(id: u64, props: &Map<String, Value>) -> Result<Graph, TreeError> {
    let invalid = || {
        TreeError::Invalid(
            "invalid graph window (10,000 total, 256 window rows, bounded fields required)".into(),
        )
    };
    let integer = |key| {
        props
            .get(key)
            .and_then(Value::as_u64)
            .and_then(|v| usize::try_from(v).ok())
            .ok_or_else(invalid)
    };
    let total = integer("total")?;
    let lanes = integer("lanes")?;
    let offset = integer("offset")?;
    let selected = props
        .get("selected")
        .and_then(Value::as_i64)
        .ok_or_else(invalid)?;
    let rows: Vec<GraphRow> =
        serde_json::from_value(props.get("rows").cloned().ok_or_else(invalid)?)
            .map_err(|_| invalid())?;
    let text_ok = |s: &str, max| s.chars().count() <= max;
    if total > 10_000
        || !(1..=24).contains(&lanes)
        || offset > total
        || rows.len() > 256
        || rows.len() > total - offset
        || selected < -1
        || selected >= total as i64
        || rows.iter().any(|r| {
            r.hash.is_empty()
                || !text_ok(&r.hash, 64)
                || !text_ok(&r.subject, 4096)
                || !text_ok(&r.author, 512)
                || !text_ok(&r.date, 64)
                || r.refs.len() > 64
                || r.refs.iter().any(|s| !text_ok(s, 1024))
                || r.lane > 10_000
                || r.color > 100_000
                || r.lines.len() > 4096
                || !r.lines.len().is_multiple_of(4)
                || r.lines
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|l| l[0] > 10_000 || l[1] > 10_000 || l[2] > 2 || l[3] > 100_000)
        })
    {
        return Err(invalid());
    }
    let handler = |key| {
        str_prop(props, key)
            .filter(|s| valid_handler(s))
            .map(str::to_owned)
    };
    Ok(Graph {
        id,
        total,
        lanes,
        selected,
        offset,
        rows,
        on_range: handler("onRangeChange"),
        on_select: handler("onSelect"),
    })
}

impl Select {
    /// The value the select is on: the one the page set, or the first option's when it set none.
    pub fn current(&self) -> Option<&str> {
        self.value
            .as_deref()
            .or_else(|| self.options.first().map(|option| option.value.as_str()))
    }

    /// What the select shows: its current option's label, or the page's value itself when no option has it.
    pub fn shown(&self) -> &str {
        let current = self.current().unwrap_or_default();
        self.options
            .iter()
            .find(|option| option.value == current)
            .map_or(current, |option| option.label.as_str())
    }
}

/// What a page sends: `{id, type, props, children}`, or `{id, type: "#text", text}`.
#[derive(Deserialize)]
struct Raw {
    id: u64,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    props: Map<String, Value>,
    #[serde(default)]
    children: Vec<Raw>,
    #[serde(default)]
    text: String,
}

impl Tree {
    pub fn parse(json: &str) -> Result<Tree, TreeError> {
        if json.len() > MAX_JSON_BYTES {
            return Err(TreeError::TooLarge);
        }
        let root: Raw =
            serde_json::from_str(json).map_err(|error| TreeError::Invalid(error.to_string()))?;
        if root.kind != "#root" {
            return Err(TreeError::Invalid("no root".into()));
        }
        let mut walk = Walk { nodes: 0 };
        Ok(Tree {
            children: walk.children(root.children, 0)?,
        })
    }

    /// Every string in the tree, in order: what a reader would see.
    pub fn text(&self) -> String {
        plain_text(&self.children)
    }
}

/// The strings under `nodes`: what a reader would see. Strings next to each other run together, and the parts of a
/// component (a page's toolbar and body) are told apart by a space.
pub fn plain_text(nodes: &[Node]) -> String {
    nodes
        .iter()
        .map(|node| match node {
            Node::Str { text, .. } => text.clone(),
            other => other
                .parts()
                .into_iter()
                .map(plain_text)
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join(" "),
        })
        .collect()
}

impl Node {
    /// What the component holds, one list for each of its named parts.
    pub fn parts(&self) -> Vec<&[Node]> {
        match self {
            Node::Str { .. } => vec![],
            Node::Slot(n) => vec![&n.children],
            Node::Page(n) => vec![&n.toolbar, &n.body],
            Node::Toolbar(n) => vec![&n.title, &n.filters, &n.actions],
            Node::Stack(n) => vec![&n.children],
            Node::Row(n) => vec![&n.children],
            Node::Text(n) => vec![&n.children],
            Node::Button(n) => vec![&n.children],
            Node::ListButton(n) => vec![&n.button.children],
            Node::Notice(n) => vec![&n.children],
            Node::Spinner(_) => vec![],
            Node::Checkbox(n) => vec![&n.children],
            Node::Confirm(n) => vec![&n.title, &n.body, &n.actions],
            Node::Badge(n) => vec![&n.children],
            Node::Empty(n) => vec![&n.title, &n.description, &n.children],
            Node::Loading(n) => vec![&n.children],
            Node::Card(n) => vec![&n.children],
            Node::Heading(n) => vec![&n.children],
            Node::Dot(_)
            | Node::Select(_)
            | Node::Chips(_)
            | Node::SearchField(_)
            | Node::Graph(_) => vec![],
            Node::Gauge(n) => vec![&n.note],
            Node::List(n) => vec![&n.children],
            Node::ListItem(n) => vec![&n.children],
            Node::Disclosure(n) => vec![&n.summary, &n.body],
        }
    }
}

struct Walk {
    nodes: usize,
}

impl Walk {
    fn children(&mut self, raw: Vec<Raw>, depth: usize) -> Result<Vec<Node>, TreeError> {
        let mut out = Vec::with_capacity(raw.len());
        for child in raw {
            if let Some(node) = self.node(child, depth + 1)? {
                out.push(node);
            }
        }
        Ok(out)
    }

    fn node(&mut self, raw: Raw, depth: usize) -> Result<Option<Node>, TreeError> {
        if depth > MAX_DEPTH {
            return Err(TreeError::TooDeep);
        }
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(TreeError::TooManyNodes);
        }
        let Raw {
            id,
            kind,
            props,
            children,
            text,
        } = raw;
        if kind == "#text" {
            return Ok(Some(Node::Str {
                id,
                text: cut(text),
            }));
        }
        // Checked before the children are walked, so a component nobody draws costs nothing.
        if !matches!(
            kind.as_str(),
            "Slot"
                | "Page"
                | "Toolbar"
                | "Stack"
                | "Row"
                | "Text"
                | "Button"
                | "Notice"
                | "Spinner"
                | "Checkbox"
                | "Confirm"
                | "Badge"
                | "Empty"
                | "Loading"
                | "Card"
                | "Heading"
                | "Dot"
                | "Gauge"
                | "List"
                | "ListItem"
                | "Disclosure"
                | "Select"
                | "Chips"
                | "ListButton"
                | "SearchField"
                | "Graph"
        ) {
            return Ok(None);
        }
        let children = self.children(children, depth)?;
        let on = |name: &str| props.get(name) == Some(&Value::Bool(true));
        let tone = str_prop(&props, "tone").and_then(Tone::choose);
        Ok(Some(match kind.as_str() {
            "Slot" => Node::Slot(Slot {
                name: str_prop(&props, "name").unwrap_or_default().to_owned(),
                children,
            }),
            "Page" => {
                let mut slots = Slots(children);
                Node::Page(Page {
                    id,
                    toolbar: slots.take("toolbar"),
                    flush: on("flush"),
                    body: slots.take("body"),
                })
            }
            "Toolbar" => {
                let mut slots = Slots(children);
                Node::Toolbar(Toolbar {
                    id,
                    title: slots.take("title"),
                    filters: slots.take("children"),
                    actions: slots.take("actions"),
                })
            }
            "Stack" => Node::Stack(Stack {
                id,
                gap: choice(&props, "gap"),
                grow: on("grow"),
                children: parts(children),
            }),
            "Row" => Node::Row(Row {
                id,
                gap: choice(&props, "gap"),
                align: choice(&props, "align"),
                justify: choice(&props, "justify"),
                wrap: on("wrap"),
                grow: on("grow"),
                children: parts(children),
            }),
            "Text" => Node::Text(Text {
                id,
                tone,
                size: choice(&props, "size"),
                weight: choice(&props, "weight"),
                mono: on("mono"),
                truncate: on("truncate"),
                clamp: on("clamp"),
                grow: on("grow"),
                children: parts(children),
            }),
            "Button" => {
                let disabled = on("disabled");
                Node::Button(Button {
                    id,
                    variant: choice(&props, "variant"),
                    disabled,
                    on_click: str_prop(&props, "onClick")
                        .filter(|handler| !disabled && valid_handler(handler))
                        .map(str::to_owned),
                    children: parts(children),
                })
            }
            "Notice" => Node::Notice(Notice {
                id,
                tone,
                children: parts(children),
            }),
            "Spinner" => Node::Spinner(Spinner {
                id,
                large: str_prop(&props, "size") == Some("large"),
                label: cut(str_prop(&props, "label").unwrap_or_default().to_owned()),
            }),
            "Checkbox" => {
                let disabled = on("disabled");
                Node::Checkbox(Checkbox {
                    id,
                    checked: on("checked"),
                    disabled,
                    on_change: str_prop(&props, "onChange")
                        .filter(|handler| !disabled && valid_handler(handler))
                        .map(str::to_owned),
                    children: parts(children),
                })
            }
            "Confirm" => {
                let mut slots = Slots(children);
                Node::Confirm(Confirm {
                    id,
                    title: slots.take("title"),
                    body: slots.take("body"),
                    actions: slots.take("actions"),
                })
            }
            "Badge" => Node::Badge(Badge {
                id,
                variant: choice(&props, "variant"),
                tone,
                small: str_prop(&props, "size") == Some("sm"),
                children: parts(children),
            }),
            "Empty" => {
                let mut slots = Slots(children);
                Node::Empty(Empty {
                    id,
                    title: slots.take("title"),
                    description: slots.take("description"),
                    children: slots.take("children"),
                })
            }
            "Loading" => Node::Loading(Loading {
                id,
                children: parts(children),
            }),
            "Card" => Node::Card(Card {
                id,
                children: parts(children),
            }),
            "Heading" => Node::Heading(Heading {
                id,
                children: parts(children),
            }),
            "Dot" => Node::Dot(Dot { id, tone }),
            "Gauge" => {
                let mut slots = Slots(children);
                Node::Gauge(Gauge {
                    id,
                    label: cut(str_prop(&props, "label").unwrap_or_default().to_owned()),
                    percent: number(&props, "percent").unwrap_or(0.0).clamp(0.0, 100.0),
                    levels: levels(&props),
                    note: slots.take("note"),
                })
            }
            "List" => Node::List(List {
                id,
                children: parts(children),
            }),
            "ListItem" => Node::ListItem(ListItem {
                id,
                children: parts(children),
            }),
            "Disclosure" => {
                let mut slots = Slots(children);
                Node::Disclosure(Disclosure {
                    id,
                    summary: slots.take("summary"),
                    body: slots.take("body"),
                })
            }
            "Graph" => Node::Graph(graph(id, &props)?),
            "SearchField" => Node::SearchField(SearchField {
                id,
                label: cut(str_prop(&props, "label").unwrap_or_default().to_owned()),
                placeholder: cut(str_prop(&props, "placeholder")
                    .unwrap_or_default()
                    .to_owned()),
                value: str_prop(&props, "defaultValue").map(str::to_owned),
                on_input: str_prop(&props, "onInput")
                    .filter(|s| valid_handler(s))
                    .map(str::to_owned),
            }),
            "ListButton" => Node::ListButton(ListButton {
                button: Button {
                    id,
                    variant: Variant::Ghost,
                    disabled: on("disabled"),
                    on_click: str_prop(&props, "onClick")
                        .filter(|s| !on("disabled") && valid_handler(s))
                        .map(str::to_owned),
                    children: parts(children),
                },
                indent: number(&props, "indent")
                    .unwrap_or_default()
                    .clamp(0.0, 240.0) as f32,
                expanded: props.get("aria-expanded").and_then(Value::as_bool),
                vertical: str_prop(&props, "layout") == Some("column"),
            }),
            "Select" | "Chips" => {
                let select = Select {
                    id,
                    label: cut(str_prop(&props, "label").unwrap_or_default().to_owned()),
                    options: options(&props),
                    value: str_prop(&props, "value").map(|value| cut(value.to_owned())),
                    on_change: str_prop(&props, "onChange")
                        .filter(|handler| valid_handler(handler))
                        .map(str::to_owned),
                };
                if kind == "Chips" {
                    Node::Chips(select)
                } else {
                    Node::Select(select)
                }
            }
            _ => return Ok(None),
        }))
    }
}

fn cut(text: String) -> String {
    if text.len() <= MAX_TEXT_CHARS {
        return text;
    }
    text.chars().take(MAX_TEXT_CHARS).collect()
}

fn str_prop<'a>(props: &'a Map<String, Value>, name: &str) -> Option<&'a str> {
    props.get(name).and_then(Value::as_str)
}

fn number(props: &Map<String, Value>, name: &str) -> Option<f64> {
    props.get(name).and_then(Value::as_f64)
}

/// `[warn, danger]`: anything but two numbers is no levels.
fn levels(props: &Map<String, Value>) -> Option<(f64, f64)> {
    match props.get("levels")?.as_array()?.as_slice() {
        [warn, danger] => Some((warn.as_f64()?, danger.as_f64()?)),
        _ => None,
    }
}

/// `[{value, label, disabled?}]`: an entry with no string `value` isn't one, and a missing label is the value.
fn options(props: &Map<String, Value>) -> Vec<SelectOption> {
    let Some(Value::Array(all)) = props.get("options") else {
        return Vec::new();
    };
    all.iter()
        .filter_map(|option| {
            let option = option.as_object()?;
            let value = cut(str_prop(option, "value")?.to_owned());
            Some(SelectOption {
                label: str_prop(option, "label")
                    .map_or_else(|| value.clone(), |l| cut(l.to_owned())),
                disabled: option.get("disabled") == Some(&Value::Bool(true)),
                value,
            })
        })
        .take(MAX_OPTIONS)
        .collect()
}

fn choice<T: Choice + Default>(props: &Map<String, Value>, name: &str) -> T {
    str_prop(props, name)
        .and_then(T::choose)
        .unwrap_or_default()
}

/// An id the page made: `<instance id>:<prop>`. Anything else isn't one, and a click on it would reach nothing.
fn valid_handler(handler: &str) -> bool {
    handler.len() <= MAX_HANDLER_BYTES
        && handler
            .split_once(':')
            .is_some_and(|(id, name)| !id.is_empty() && !name.is_empty())
        && handler
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b':' || b == b'_')
}

/// A component's named parts, taken out by name.
struct Slots(Vec<Node>);

impl Slots {
    fn take(&mut self, name: &str) -> Vec<Node> {
        match self
            .0
            .iter()
            .position(|node| matches!(node, Node::Slot(slot) if slot.name == name))
        {
            Some(at) => match self.0.remove(at) {
                Node::Slot(slot) => slot.children,
                _ => unreachable!("position found a slot"),
            },
            None => Vec::new(),
        }
    }
}

/// What a component without named parts holds: a slot out of place has nothing to belong to, so it's dropped.
fn parts(children: Vec<Node>) -> Vec<Node> {
    children
        .into_iter()
        .filter(|node| !matches!(node, Node::Slot(_)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tree(children: Value) -> Result<Tree, TreeError> {
        Tree::parse(&json!({"id":0,"type":"#root","props":{},"children":children}).to_string())
    }

    fn button(props: Value) -> Value {
        json!({"id":2,"type":"Button","props":props,"children":[{"id":3,"type":"#text","text":"Go"}]})
    }

    #[test]
    fn parses_a_page_into_its_parts() {
        let parsed = tree(json!([{"id":1,"type":"Page","props":{"flush":true},"children":[
            {"id":4,"type":"Slot","props":{"name":"toolbar"},"children":[
                {"id":5,"type":"Toolbar","props":{},"children":[
                    {"id":6,"type":"Slot","props":{"name":"title"},"children":[{"id":7,"type":"#text","text":"Pilot"}]}]}]},
            {"id":8,"type":"Slot","props":{"name":"body"},"children":[
                button(json!({"variant":"outline","onClick":"2:onClick"}))]}]}]))
        .unwrap();
        let Node::Page(page) = &parsed.children[0] else {
            panic!("a page")
        };
        assert!(page.flush);
        let Node::Toolbar(toolbar) = &page.toolbar[0] else {
            panic!("a toolbar")
        };
        assert_eq!(plain_text(&toolbar.title), "Pilot");
        let Node::Button(button) = &page.body[0] else {
            panic!("a button")
        };
        assert_eq!(button.variant, Variant::Outline);
        assert_eq!(button.on_click.as_deref(), Some("2:onClick"));
        assert_eq!(parsed.text(), "Pilot Go");
    }

    #[test]
    fn unknown_components_are_dropped_with_what_is_in_them() {
        let parsed = tree(json!([
            {"id":1,"type":"Hologram","props":{},"children":[{"id":2,"type":"#text","text":"never"}]},
            {"id":3,"type":"Stack","props":{},"children":[
                {"id":4,"type":"script","props":{},"children":[]},
                {"id":5,"type":"#text","text":"kept"}]},
        ]))
        .unwrap();
        assert_eq!(parsed.children.len(), 1);
        assert_eq!(parsed.text(), "kept");
    }

    #[test]
    fn unknown_values_are_the_defaults_and_a_bad_handler_is_none() {
        let parsed = tree(json!([
            button(json!({"variant":"flashy","onClick":"../../etc"})),
            button(json!({"onClick":"2:onClick","disabled":true})),
            {"id":9,"type":"Stack","props":{"gap":"huge"},"children":[]},
        ]))
        .unwrap();
        let Node::Button(first) = &parsed.children[0] else {
            panic!("a button")
        };
        assert_eq!(
            (first.variant, first.on_click.clone()),
            (Variant::Default, None)
        );
        let Node::Button(disabled) = &parsed.children[1] else {
            panic!("a button")
        };
        assert!(disabled.disabled && disabled.on_click.is_none());
        let Node::Stack(stack) = &parsed.children[2] else {
            panic!("a stack")
        };
        assert_eq!(stack.gap, Gap::M);
    }

    #[test]
    fn notices_spinners_checkboxes_and_confirms_have_only_their_supported_parts() {
        let parsed = tree(json!([
            {"id":1,"type":"Notice","props":{"tone":"warn"},"children":[text(2, "Careful")]},
            {"id":3,"type":"Spinner","props":{"size":"large","label":"Refreshing"},"children":[]},
            {"id":4,"type":"Checkbox","props":{"checked":true,"onChange":"4:onChange"},"children":[text(5, "Delete branch")]},
            {"id":6,"type":"Confirm","props":{},"children":[
                slot("title", json!([text(7, "Remove?")])),
                slot("body", json!([text(8, "This cannot be undone.")])),
                slot("actions", json!([button(json!({"onClick":"9:onClick"}))]))]}]))
        .unwrap();
        let Node::Notice(notice) = &parsed.children[0] else {
            panic!("a notice")
        };
        assert_eq!(
            (notice.tone, plain_text(&notice.children)),
            (Some(Tone::Warn), "Careful".to_owned())
        );
        let Node::Spinner(spinner) = &parsed.children[1] else {
            panic!("a spinner")
        };
        assert!(spinner.large && spinner.label == "Refreshing");
        let Node::Checkbox(checkbox) = &parsed.children[2] else {
            panic!("a checkbox")
        };
        assert_eq!(
            (
                checkbox.checked,
                checkbox.on_change.as_deref(),
                plain_text(&checkbox.children)
            ),
            (true, Some("4:onChange"), "Delete branch".to_owned())
        );
        let Node::Confirm(confirm) = &parsed.children[3] else {
            panic!("a confirm")
        };
        assert_eq!(plain_text(&confirm.title), "Remove?");
        assert_eq!(plain_text(&confirm.body), "This cannot be undone.");
        assert_eq!(plain_text(&confirm.actions), "Go");
    }

    fn slot(name: &str, children: Value) -> Value {
        json!({"id":90,"type":"Slot","props":{"name":name},"children":children})
    }

    fn text(id: u64, text: &str) -> Value {
        json!({"id":id,"type":"#text","text":text})
    }

    fn parse_one(node: Value) -> Node {
        tree(json!([node])).unwrap().children.remove(0)
    }

    #[test]
    fn card_heading_list_and_list_item_hold_their_children() {
        let Node::Card(card) = parse_one(json!({"id":1,"type":"Card","props":{},"children":[
            {"id":2,"type":"Heading","props":{},"children":[text(3, "Usage")]},
            {"id":4,"type":"List","props":{},"children":[
                {"id":5,"type":"ListItem","props":{},"children":[text(6, "one")]},
                // A slot out of place has nothing to belong to.
                slot("note", json!([text(7, "lost")]))]}]}))
        else {
            panic!("a card")
        };
        assert_eq!(plain_text(&card.children), "Usageone");
        let [Node::Heading(_), Node::List(list)] = card.children.as_slice() else {
            panic!("a heading and a list")
        };
        assert!(matches!(list.children.as_slice(), [Node::ListItem(_)]));
    }

    #[test]
    fn a_dot_has_a_tone_or_is_green() {
        let dot = |props| match parse_one(json!({"id":1,"type":"Dot","props":props,"children":[]}))
        {
            Node::Dot(dot) => dot.tone,
            _ => panic!("a dot"),
        };
        assert_eq!(dot(json!({})), None);
        assert_eq!(dot(json!({"tone":"danger"})), Some(Tone::Danger));
        assert_eq!(dot(json!({"tone":"muted"})), Some(Tone::Muted));
        assert_eq!(dot(json!({"tone":"purple"})), None);
        assert_eq!(dot(json!({"tone":7})), None);
    }

    fn gauge(props: Value, children: Value) -> Gauge {
        match parse_one(json!({"id":1,"type":"Gauge","props":props,"children":children})) {
            Node::Gauge(gauge) => gauge,
            _ => panic!("a gauge"),
        }
    }

    #[test]
    fn a_gauge_has_a_label_a_bounded_percent_levels_and_a_note() {
        let full = gauge(
            json!({"label":"Memory","percent":42.5,"levels":[70,90]}),
            json!([slot("note", json!([text(2, "Resets in 2h")]))]),
        );
        assert_eq!(full.label, "Memory");
        assert_eq!(full.percent, 42.5);
        assert_eq!(full.levels, Some((70.0, 90.0)));
        assert_eq!(plain_text(&full.note), "Resets in 2h");

        // A page that sends nonsense gets a bar that is empty or full, not one that draws outside its box.
        let bounded =
            |percent: Value| gauge(json!({"label":"x","percent":percent}), json!([])).percent;
        assert_eq!(bounded(json!(-5)), 0.0);
        assert_eq!(bounded(json!(1e300)), 100.0);
        assert_eq!(bounded(json!("40")), 0.0);
        assert_eq!(bounded(Value::Null), 0.0);

        let none = gauge(json!({}), json!([]));
        assert_eq!(
            (none.label.as_str(), none.percent, none.levels),
            ("", 0.0, None)
        );
        assert!(none.note.is_empty());
        // Levels are two numbers or nothing.
        for levels in [
            json!([70]),
            json!([1, 2, 3]),
            json!(["a", "b"]),
            json!(70),
            json!([70, null]),
        ] {
            assert_eq!(
                gauge(json!({"levels":levels}), json!([])).levels,
                None,
                "{levels}"
            );
        }
    }

    #[test]
    fn a_gauge_turns_warn_and_danger_at_its_levels() {
        let level = |percent: f64, levels: Value| {
            gauge(json!({"percent":percent,"levels":levels}), json!([])).level()
        };
        assert_eq!(level(40.0, json!([70, 90])), Level::Normal);
        assert_eq!(level(69.0, json!([70, 90])), Level::Normal);
        assert_eq!(level(70.0, json!([70, 90])), Level::Warn);
        assert_eq!(level(85.0, json!([70, 90])), Level::Warn);
        assert_eq!(level(90.0, json!([70, 90])), Level::Danger);
        assert_eq!(level(100.0, json!([70, 90])), Level::Danger);
        assert_eq!(level(99.0, json!(null)), Level::Normal);
    }

    #[test]
    fn a_disclosure_has_a_summary_and_a_body() {
        let Node::Disclosure(disclosure) =
            parse_one(json!({"id":1,"type":"Disclosure","props":{},"children":[
            slot("body", json!([text(2, "details")])),
            slot("summary", json!([text(3, "2 need sign-in")]))]}))
        else {
            panic!("a disclosure")
        };
        assert_eq!(plain_text(&disclosure.summary), "2 need sign-in");
        assert_eq!(plain_text(&disclosure.body), "details");
        assert_eq!(disclosure.id, 1);
    }

    fn select(props: Value) -> Select {
        match parse_one(json!({"id":1,"type":"Select","props":props,"children":[]})) {
            Node::Select(select) => select,
            _ => panic!("a select"),
        }
    }

    #[test]
    fn a_select_has_options_a_value_and_a_handler() {
        let select = select(
            json!({"label":"Session","value":"b","onChange":"1:onChange","options":[
            {"value":"a","label":"Alpha"},
            {"value":"b","label":"Beta","disabled":true},
            {"value":"c"},
            {"label":"no value"},
            "not an object",
            {"value":3,"label":"not a string"}]}),
        );
        assert_eq!(select.label, "Session");
        assert_eq!(select.on_change.as_deref(), Some("1:onChange"));
        assert_eq!(
            select.options,
            [
                SelectOption {
                    value: "a".into(),
                    label: "Alpha".into(),
                    disabled: false
                },
                SelectOption {
                    value: "b".into(),
                    label: "Beta".into(),
                    disabled: true
                },
                // No label is the value, so the option still shows something.
                SelectOption {
                    value: "c".into(),
                    label: "c".into(),
                    disabled: false
                },
            ]
        );
        assert_eq!((select.current(), select.shown()), (Some("b"), "Beta"));
    }

    #[test]
    fn a_select_without_a_value_shows_its_first_option_and_a_value_nobody_has_shows_itself() {
        let options = json!([{"value":"a","label":"Alpha"},{"value":"b","label":"Beta"}]);
        let first = select(json!({"options":options}));
        assert_eq!((first.current(), first.shown()), (Some("a"), "Alpha"));
        let unknown = select(json!({"options":options,"value":"zzz"}));
        assert_eq!(unknown.shown(), "zzz");
        let empty = select(json!({}));
        assert_eq!((empty.current(), empty.shown()), (None, ""));
    }

    #[test]
    fn a_select_is_bounded_and_its_bad_handler_is_none() {
        let many: Vec<Value> = (0..MAX_OPTIONS * 2)
            .map(|n| json!({"value":n.to_string(),"label":"x".repeat(MAX_TEXT_CHARS * 2)}))
            .collect();
        let bounded = select(json!({"options":many,"onChange":"../../etc"}));
        assert_eq!(bounded.options.len(), MAX_OPTIONS);
        assert!(
            bounded
                .options
                .iter()
                .all(|o| o.label.chars().count() == MAX_TEXT_CHARS)
        );
        assert_eq!(bounded.on_change, None);
    }

    #[test]
    fn sizes_are_bounded() {
        let long = "x".repeat(MAX_TEXT_CHARS * 2);
        let parsed = tree(json!([{"id":1,"type":"#text","text":long}])).unwrap();
        assert_eq!(parsed.text().chars().count(), MAX_TEXT_CHARS);

        let many: Vec<Value> = (0..=MAX_NODES)
            .map(|id| json!({"id":id,"type":"#text","text":"a"}))
            .collect();
        assert_eq!(tree(Value::Array(many)), Err(TreeError::TooManyNodes));

        let mut deep = json!([]);
        for id in 0..=MAX_DEPTH {
            deep = json!([{"id":id,"type":"Stack","props":{},"children":deep}]);
        }
        assert_eq!(tree(deep), Err(TreeError::TooDeep));

        let huge = format!(
            r##"{{"id":0,"type":"#root","children":[{{"id":1,"type":"#text","text":"{}"}}]}}"##,
            "x".repeat(MAX_JSON_BYTES)
        );
        assert_eq!(Tree::parse(&huge), Err(TreeError::TooLarge));
    }

    #[test]
    fn what_is_not_a_tree_is_refused() {
        for json in [
            "",
            "[]",
            "null",
            r#"{"id":0,"type":"Page"}"#,
            r##"{"type":"#root"}"##,
        ] {
            assert!(Tree::parse(json).is_err(), "{json}");
        }
    }

    #[test]
    fn graph_windows_preserve_geometry_and_reject_oversized_or_malformed_data() {
        let mut props = serde_json::json!({ "total": 10000, "lanes": 2, "selected": -1, "offset": 5000,
            "rows": [{"hash":"abc", "subject":"x".repeat(3000), "author":"Ada", "date":"2026-11-06", "refs":["H:main"], "lane":1, "color":2, "merge":true, "lines":[0,1,0,2,1,1,1,2]}]
        }).as_object().unwrap().clone();
        let parsed = graph(1, &props).unwrap();
        assert_eq!(
            parsed.rows[0].subject.len(),
            3000,
            "graph data is not generic truncated text"
        );
        assert_eq!(parsed.rows[0].lines, [0, 1, 0, 2, 1, 1, 1, 2]);
        assert!(parsed.rows[0].merge);
        let row = props["rows"][0].clone();
        props.insert("rows".into(), serde_json::json!(vec![row.clone(); 257]));
        assert!(graph(1, &props).is_err());
        props.insert("rows".into(), serde_json::json!([row]));
        props["rows"][0]["lines"] = serde_json::json!([0, 1, 3, 0]);
        assert!(graph(1, &props).is_err());
        props["rows"][0]["lines"] = serde_json::json!([0, 1, 0, 0]);
        props["rows"][0]["subject"] = serde_json::json!("x".repeat(4097));
        assert!(graph(1, &props).is_err());
    }
}
