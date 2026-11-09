//! The tree a page sends, parsed from its JSON and checked, so the view draws only what it knows and a page can't
//! make it hold or draw more than these limits.
//!
//! A page's host components are `wings-plugin-ui/native`'s: a component with named parts (a page's toolbar, an empty
//! state's description) sends each part as a `Slot` child, and a component the host doesn't know is dropped with
//! everything in it.

use serde::Deserialize;
use serde_json::{Map, Value};
use std::fmt;

mod sunburst;
mod svg;
pub use sunburst::{Geometry, Hit, Sunburst, Wedge};
pub use svg::Svg;

/// The largest tree JSON the host parses.
pub const MAX_JSON_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_NODES: usize = 5_000;
pub const MAX_DEPTH: usize = 32;
/// A longer text is cut to this many characters.
pub const MAX_TEXT_CHARS: usize = 2_000;
const MAX_HANDLER_BYTES: usize = 64;
/// A select shows this many of its options; the rest are dropped.
pub const MAX_OPTIONS: usize = 200;
/// A text input holds this many characters of what the page gave it. The rest of the page's text is cut to
/// `MAX_TEXT_CHARS`, which would cut what the person typed in a long field.
pub const MAX_INPUT_CHARS: usize = 20_000;

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
choice!(Edge { None = "none", Top = "top", Bottom = "bottom" });
choice!(
    /// `field` is native.js's own, for a `Field`'s label and hint.
    TextSize { Normal = "normal", Small = "small", Tiny = "tiny", Large = "large", Field = "field" }
);
choice!(Weight { Normal = "normal", Medium = "medium", Strong = "strong" });
choice!(
    /// The names `Icon` draws.
    IconName { Check = "check", ChevronDown = "chevron-down", Copy = "copy", Trash = "trash" }
);
choice!(
    /// A button's `size`, as plugin-ui.css's `.w-button[data-size]`.
    ButtonSize { Default = "default", Xs = "xs", Sm = "sm", Lg = "lg", Icon = "icon", IconXs = "icon-xs", IconSm = "icon-sm", IconLg = "icon-lg" }
);
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
    /// The page lays itself out edge to edge with no padding, as a `{ fill: true }` page does on the desktop.
    pub fill: bool,
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
    Meter(Meter),
    Board(Board),
    BoardRow(List),
    BoardColumn(BoardColumn),
    List(List),
    ListItem(ListItem),
    Disclosure(Disclosure),
    Select(Select),
    Chips(Select),
    Combobox(Combobox),
    ListButton(ListButton),
    SearchField(SearchField),
    Input(Input),
    Icon(Icon),
    Svg(Svg),
    Graph(Graph),
    Avatar(Avatar),
    Skeleton(Skeleton),
    Scroll(Scroll),
    Split(Split),
    Bar(Bar),
    Sunburst(Sunburst),
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
    /// A fixed width in points, for a column beside others.
    pub width: Option<f32>,
    /// At most this wide, centred: a reading column, as plugin-ui.css's `.w-stack[data-max-width]`.
    pub max_width: Option<f32>,
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
    /// What to send back to the page when the text is clicked, for a link inside a line of text.
    pub on_click: Option<String>,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Button {
    pub id: u64,
    pub variant: Variant,
    pub size: ButtonSize,
    /// A `Chip`: plugin-ui.css's `.w-chip`, tinted while `pressed`.
    pub chip: bool,
    pub pressed: bool,
    pub disabled: bool,
    /// The name a screen reader says, when it isn't the button's text (an icon button's).
    pub label: Option<String>,
    /// Its text wraps onto the lines it needs, from its start, and it's as tall as they are: an issue's title that is
    /// a button, as plugin-ui.css's `.w-button[data-wrap]`.
    pub wrap: bool,
    /// What to send back to the page when it's clicked. A disabled button has none.
    pub on_click: Option<String>,
    pub on_key_down: Option<String>,
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
    /// A badge that's a button, as a `SummaryChip` with an `onClick` is: plugin-ui.css draws it as the same pill.
    pub on_click: Option<String>,
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
    /// Level 3 names a section of a page, as `h3.w-heading` does.
    pub section: bool,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Dot {
    pub id: u64,
    /// Green, the plain colour, when there is none.
    pub tone: Option<Tone>,
    /// `0xRRGGBB`, a colour from the page's data, which wins over the tone.
    pub color: Option<u32>,
}

/// A column that scrolls and fills what is left of its parent.
#[derive(Clone, Debug, PartialEq)]
pub struct Scroll {
    pub id: u64,
    pub children: Vec<Node>,
}

/// A strip across its parent, a page's header or footer: padded like a toolbar, with a hairline on one edge.
#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    pub id: u64,
    pub edge: Edge,
    pub gap: Gap,
    pub children: Vec<Node>,
}

/// Two panes side by side: `start` is `size` points wide, `end` takes the rest.
#[derive(Clone, Debug, PartialEq)]
pub struct Split {
    pub id: u64,
    pub size: f32,
    pub start: Vec<Node>,
    pub end: Vec<Node>,
}

/// A round avatar with the initials of a name, on a colour that depends only on the name.
#[derive(Clone, Debug, PartialEq)]
pub struct Avatar {
    pub id: u64,
    pub name: String,
    pub large: bool,
}

/// How wide a skeleton is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    Points(f32),
    Percent(f32),
}

/// A block that stands in for content that is loading.
#[derive(Clone, Debug, PartialEq)]
pub struct Skeleton {
    pub id: u64,
    /// The whole width when there is none.
    pub width: Option<Length>,
    pub height: f32,
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

#[derive(Clone, Debug, PartialEq)]
pub struct Meter {
    pub id: u64,
    pub label: String,
    pub percent: f64,
    pub tone: Option<Tone>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Board {
    pub id: u64,
    pub label: String,
    pub lanes: bool,
    pub header: Vec<Node>,
    pub body: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BoardColumn {
    pub id: u64,
    pub label: String,
    pub cell: bool,
    pub count: Option<u64>,
    pub title: Vec<Node>,
    pub actions: Vec<Node>,
    pub body: Vec<Node>,
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
    /// Buttons that show over the row's end while it is pointed at or one of them has the keyboard.
    pub actions: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Disclosure {
    pub id: u64,
    pub open: Option<bool>,
    pub on_open_change: Option<String>,
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
    /// Chips drawn as `Tabs` of the "line" variant are: labels with a line under the picked one.
    pub line: bool,
}

/// wings-plugin-ui/native's `Combobox`, which has no node of its own: the page sends a stack of its field and, under
/// it, the options that match the field as chips, or a muted line saying none do. The app draws them as a list over
/// the page, as the desktop does, instead of in the page's flow, where they pushed everything under the field down.
#[derive(Clone, Debug, PartialEq)]
pub struct Combobox {
    pub id: u64,
    /// The field, one `Node::Input`.
    pub field: Vec<Node>,
    /// The chips (`Node::Button`): a click on one picks its option.
    pub options: Vec<Node>,
    /// What the line saying nothing matches holds, when nothing does.
    pub empty: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ListButton {
    pub button: Button,
    pub indent: f32,
    pub expanded: Option<bool>,
    pub vertical: bool,
    /// `align="baseline"`: its parts line up on their first lines, as a prompt's name and time beside its text.
    pub baseline: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SearchField {
    pub id: u64,
    pub label: String,
    pub placeholder: String,
    pub value: Option<String>,
    pub on_input: Option<String>,
}

/// A `TextInput`, or with `rows` a `Textarea`. The page holds its text: the one in the tree is what to show.
#[derive(Clone, Debug, PartialEq)]
pub struct Input {
    pub id: u64,
    pub label: String,
    pub placeholder: String,
    pub value: String,
    pub rows: Option<usize>,
    pub disabled: bool,
    pub max_length: Option<usize>,
    /// What to send back to the page, with the text after each edit.
    pub on_change: Option<String>,
    pub on_key_down: Option<String>,
    pub on_blur: Option<String>,
    pub auto_focus: bool,
    pub select_on_focus: bool,
    /// A combobox's field shows its options when it takes the keyboard, unless the page sends `openOnFocus: false`.
    pub open_on_focus: bool,
    /// `type="password"`: the text shows as dots and can't be copied. Any other type is plain text here, but time.
    pub password: bool,
    /// `type="time"`: digits alone make HH:MM, and the page hears "" until it is a whole time, as from a browser.
    pub time: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Icon {
    pub id: u64,
    pub name: IconName,
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
            fill: root.props.get("fill").and_then(Value::as_bool) == Some(true),
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
            | Node::Input(_)
            | Node::Icon(_)
            | Node::Svg(_)
            | Node::Graph(_)
            | Node::Avatar(_)
            | Node::Skeleton(_)
            | Node::Sunburst(_) => vec![],
            Node::Scroll(n) => vec![&n.children],
            Node::Split(n) => vec![&n.start, &n.end],
            Node::Bar(n) => vec![&n.children],
            Node::Gauge(n) => vec![&n.note],
            Node::Meter(_) => vec![],
            Node::Board(n) => vec![&n.header, &n.body],
            Node::BoardRow(n) => vec![&n.children],
            Node::BoardColumn(n) => vec![&n.title, &n.actions, &n.body],
            Node::List(n) => vec![&n.children],
            Node::ListItem(n) => vec![&n.children, &n.actions],
            Node::Disclosure(n) => vec![&n.summary, &n.body],
            Node::Combobox(n) => vec![&n.field, &n.options, &n.empty],
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
                | "Meter"
                | "Board"
                | "BoardRow"
                | "BoardColumn"
                | "List"
                | "ListItem"
                | "Disclosure"
                | "Select"
                | "Chips"
                | "ListButton"
                | "SearchField"
                | "TextInput"
                | "Textarea"
                | "Icon"
                | "Svg"
                | "Graph"
                | "Avatar"
                | "Skeleton"
                | "Scroll"
                | "Split"
                | "Bar"
                | "Sunburst"
        ) {
            return Ok(None);
        }
        let children = self.children(children, depth)?;
        let on = |name: &str| props.get(name) == Some(&Value::Bool(true));
        let tone = str_prop(&props, "tone").and_then(Tone::choose);
        let handler = |name| {
            str_prop(&props, name)
                .filter(|value| !on("disabled") && valid_handler(value))
                .map(str::to_owned)
        };
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
            "Stack" => combobox(Stack {
                id,
                gap: choice(&props, "gap"),
                grow: on("grow"),
                width: number(&props, "width")
                    .filter(|width| *width > 0.0)
                    .map(|width| width.min(2_000.0) as f32),
                max_width: number(&props, "maxWidth")
                    .filter(|width| *width > 0.0)
                    .map(|width| width as f32),
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
                on_click: str_prop(&props, "onClick")
                    .filter(|handler| valid_handler(handler))
                    .map(str::to_owned),
                children: parts(children),
            }),
            "Avatar" => Node::Avatar(Avatar {
                id,
                name: text_prop(&props, "name"),
                large: str_prop(&props, "size") == Some("large"),
            }),
            "Skeleton" => Node::Skeleton(Skeleton {
                id,
                width: match props.get("width") {
                    Some(Value::Number(width)) => width
                        .as_f64()
                        .filter(|width| *width > 0.0)
                        .map(|width| Length::Points(width.min(2_000.0) as f32)),
                    Some(Value::String(width)) => width
                        .strip_suffix('%')
                        .and_then(|percent| percent.trim().parse::<f32>().ok())
                        .filter(|percent| *percent > 0.0)
                        .map(|percent| Length::Percent(percent.min(100.0))),
                    _ => None,
                },
                height: number(&props, "height").unwrap_or(14.0).clamp(1.0, 400.0) as f32,
            }),
            "Button" => Node::Button(Button {
                id,
                variant: choice(&props, "variant"),
                size: choice(&props, "size"),
                chip: on("chip"),
                pressed: on("pressed"),
                disabled: on("disabled"),
                label: str_prop(&props, "label").map(|label| cut(label.to_owned())),
                wrap: on("wrap"),
                on_click: handler("onClick"),
                on_key_down: handler("onKeyDown"),
                children: parts(children),
            }),
            "Notice" => Node::Notice(Notice {
                id,
                tone,
                children: parts(children),
            }),
            "Spinner" => Node::Spinner(Spinner {
                id,
                large: str_prop(&props, "size") == Some("large"),
                label: text_prop(&props, "label"),
            }),
            "Checkbox" => Node::Checkbox(Checkbox {
                id,
                checked: on("checked"),
                disabled: on("disabled"),
                on_change: handler("onChange"),
                children: parts(children),
            }),
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
                on_click: handler("onClick"),
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
                section: number(&props, "level") == Some(3.0),
                children: parts(children),
            }),
            "Dot" => Node::Dot(Dot {
                id,
                tone,
                color: number(&props, "color")
                    .filter(|color| (0.0..=16_777_215.0).contains(color))
                    .map(|color| color as u32),
            }),
            "Scroll" => Node::Scroll(Scroll {
                id,
                children: parts(children),
            }),
            "Bar" => Node::Bar(Bar {
                id,
                edge: choice(&props, "edge"),
                gap: choice(&props, "gap"),
                children: parts(children),
            }),
            "Split" => {
                let mut slots = Slots(children);
                Node::Split(Split {
                    id,
                    size: number(&props, "defaultSize")
                        .unwrap_or(280.0)
                        .clamp(120.0, 1_000.0) as f32,
                    start: slots.take("start"),
                    end: slots.take("end"),
                })
            }
            "Sunburst" => Node::Sunburst(sunburst::parse(id, &props)?),
            "Gauge" => {
                let mut slots = Slots(children);
                Node::Gauge(Gauge {
                    id,
                    label: text_prop(&props, "label"),
                    percent: number(&props, "percent").unwrap_or(0.0).clamp(0.0, 100.0),
                    levels: levels(&props),
                    note: slots.take("note"),
                })
            }
            "Meter" => Node::Meter(Meter {
                id,
                label: text_prop(&props, "label"),
                percent: number(&props, "percent").unwrap_or(0.0).clamp(0.0, 100.0),
                tone,
            }),
            "Board" => {
                let lanes = on("lanes");
                let (header, body) = if lanes {
                    let mut slots = Slots(children);
                    (slots.take("header"), slots.take("body"))
                } else {
                    (Vec::new(), parts(children))
                };
                Node::Board(Board {
                    id,
                    label: text_prop(&props, "label"),
                    lanes,
                    header,
                    body,
                })
            }
            "BoardRow" => Node::BoardRow(List {
                id,
                children: parts(children),
            }),
            "BoardColumn" => {
                let mut slots = Slots(children);
                Node::BoardColumn(BoardColumn {
                    id,
                    label: text_prop(&props, "label"),
                    cell: on("cell"),
                    count: props.get("count").and_then(Value::as_u64),
                    title: slots.take("title"),
                    actions: slots.take("actions"),
                    body: slots.take("body"),
                })
            }
            "List" => Node::List(List {
                id,
                children: parts(children),
            }),
            "ListItem" => {
                let mut slots = Slots(children);
                let actions = slots.take("actions");
                Node::ListItem(ListItem {
                    id,
                    children: parts(slots.0),
                    actions,
                })
            }
            "Disclosure" => {
                let mut slots = Slots(children);
                Node::Disclosure(Disclosure {
                    id,
                    open: props.get("open").and_then(Value::as_bool),
                    on_open_change: handler("onOpenChange"),
                    summary: slots.take("summary"),
                    body: slots.take("body"),
                })
            }
            "Graph" => Node::Graph(graph(id, &props)?),
            "SearchField" => Node::SearchField(SearchField {
                id,
                label: text_prop(&props, "label"),
                placeholder: text_prop(&props, "placeholder"),
                value: str_prop(&props, "defaultValue").map(str::to_owned),
                on_input: str_prop(&props, "onInput")
                    .filter(|s| valid_handler(s))
                    .map(str::to_owned),
            }),
            "TextInput" | "Textarea" => Node::Input(Input {
                id,
                label: text_prop(&props, "label"),
                placeholder: text_prop(&props, "placeholder"),
                value: str_prop(&props, "value")
                    .unwrap_or_default()
                    .chars()
                    .take(MAX_INPUT_CHARS)
                    .collect(),
                rows: (kind == "Textarea")
                    .then(|| number(&props, "rows").unwrap_or(2.0).clamp(1.0, 24.0) as usize),
                disabled: on("disabled"),
                max_length: number(&props, "maxLength")
                    .filter(|max| *max >= 0.0)
                    .map(|max| max as usize),
                on_change: handler("onChange"),
                on_key_down: handler("onKeyDown"),
                on_blur: handler("onBlur"),
                auto_focus: on("autoFocus"),
                select_on_focus: on("selectOnFocus"),
                open_on_focus: props.get("openOnFocus") != Some(&Value::Bool(false)),
                password: kind == "TextInput" && str_prop(&props, "type") == Some("password"),
                time: kind == "TextInput" && str_prop(&props, "type") == Some("time"),
            }),
            "Icon" => match str_prop(&props, "name").and_then(IconName::choose) {
                Some(name) => Node::Icon(Icon { id, name }),
                None => return Ok(None),
            },
            // One that holds anything the app doesn't draw is left out, as an icon it has no name for is.
            "Svg" => match svg::parse(id, &props) {
                Some(drawing) => Node::Svg(drawing),
                None => return Ok(None),
            },
            "ListButton" => Node::ListButton(ListButton {
                button: Button {
                    id,
                    variant: Variant::Ghost,
                    size: ButtonSize::Default,
                    chip: false,
                    pressed: false,
                    disabled: on("disabled"),
                    label: None,
                    wrap: false,
                    on_click: handler("onClick"),
                    on_key_down: handler("onKeyDown"),
                    children: parts(children),
                },
                indent: number(&props, "indent")
                    .unwrap_or_default()
                    .clamp(0.0, 240.0) as f32,
                expanded: props.get("aria-expanded").and_then(Value::as_bool),
                vertical: str_prop(&props, "layout") == Some("column"),
                baseline: str_prop(&props, "align") == Some("baseline"),
            }),
            "Select" | "Chips" => {
                let select = Select {
                    id,
                    label: text_prop(&props, "label"),
                    options: options(&props),
                    value: str_prop(&props, "value").map(|value| cut(value.to_owned())),
                    on_change: str_prop(&props, "onChange")
                        .filter(|handler| valid_handler(handler))
                        .map(str::to_owned),
                    line: kind == "Chips" && on("line"),
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

/// A string prop as text the page shows, cut to `MAX_TEXT_CHARS`, or nothing when it isn't one.
fn text_prop(props: &Map<String, Value>, name: &str) -> String {
    cut(str_prop(props, name).unwrap_or_default().to_owned())
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

/// `stack` as a `Combobox` when it is exactly what wings-plugin-ui/native's `Combobox` renders: a field with no key or
/// blur handler of its own, then its options as plain chips in a wrapping row, or a small muted line when none match.
/// When the field holds an option, or is empty with none to suggest, the stack is the field alone and stays a stack.
fn combobox(stack: Stack) -> Node {
    let plain =
        stack.gap == Gap::S && !stack.grow && stack.width.is_none() && stack.max_width.is_none();
    // A picker's field selects its text when it takes the keyboard, and hears when it loses it.
    let field = |node: &Node| {
        matches!(node, Node::Input(input) if input.rows.is_none()
            && !input.password
            && input.on_key_down.is_none())
    };
    // An option's label, and its detail after it as a small muted text.
    let chip = |node: &Node| {
        matches!(node, Node::Button(button) if button.chip
        && !button.pressed
        && button.variant == Variant::Default
        && button.size == ButtonSize::Default
        && button.label.is_none()
        && button.on_click.is_some()
        && button.on_key_down.is_none()
        && match button.children.split_last() {
            Some((Node::Text(detail), label)) => {
                option_detail(detail) && label.iter().all(|child| matches!(child, Node::Str { .. }))
            }
            _ => button.children.iter().all(|child| matches!(child, Node::Str { .. })),
        })
    };
    let chips = |node: &Node| {
        matches!(node, Node::Row(row) if row.gap == Gap::S
            && row.wrap
            && !row.grow
            && row.align == Align::Center
            && row.justify == Justify::Start
            && !row.children.is_empty()
            && row.children.iter().all(chip))
    };
    let none_match = |node: &Node| matches!(node, Node::Text(text) if option_detail(text));
    if !(plain
        && stack.children.len() == 2
        && field(&stack.children[0])
        && (chips(&stack.children[1]) || none_match(&stack.children[1])))
    {
        return Node::Stack(stack);
    }
    let mut children = stack.children;
    let (options, empty) = match children.pop() {
        Some(Node::Row(row)) => (row.children, Vec::new()),
        Some(Node::Text(text)) => (Vec::new(), text.children),
        _ => unreachable!("matched above"),
    };
    Node::Combobox(Combobox {
        id: stack.id,
        field: children,
        options,
        empty,
    })
}

/// A small muted text with nothing else to it: a combobox option's detail, or its line saying nothing matches.
fn option_detail(text: &Text) -> bool {
    text.size == TextSize::Small
        && text.tone == Some(Tone::Muted)
        && text.weight == Weight::Normal
        && !text.mono
        && !text.truncate
        && !text.clamp
        && !text.grow
        && text.on_click.is_none()
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
    fn a_button_keeps_its_size_and_a_chip_its_pressed_state() {
        let parsed = tree(json!([
            button(json!({"size":"sm"})),
            button(json!({"chip":true,"pressed":true})),
            button(json!({"size":"huge"})),
        ]))
        .unwrap();
        let buttons: Vec<_> = parsed
            .children
            .iter()
            .map(|node| match node {
                Node::Button(button) => (button.size, button.chip, button.pressed),
                _ => panic!("a button"),
            })
            .collect();
        assert_eq!(
            buttons,
            [
                (ButtonSize::Sm, false, false),
                (ButtonSize::Default, true, true),
                (ButtonSize::Default, false, false),
            ]
        );
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
    fn a_list_items_actions_slot_is_its_actions_and_not_one_of_its_parts() {
        let Node::ListItem(item) = parse_one(
            json!({"id":1,"type":"ListItem","props":{},"children":[
            text(2, "Code review"),
            slot("actions", json!([{"id":4,"type":"Button","props":{"onClick":"4:click"},"children":[text(5, "Edit")]}]))]}),
        ) else {
            panic!("a list item")
        };
        assert_eq!(plain_text(&item.children), "Code review");
        assert!(matches!(item.actions.as_slice(), [Node::Button(_)]));
    }

    #[test]
    fn a_heading_of_level_3_names_a_section() {
        let section = |props: Value| match parse_one(
            json!({"id":1,"type":"Heading","props":props,"children":[text(2, "Entries")]}),
        ) {
            Node::Heading(heading) => heading.section,
            other => panic!("a heading, not {other:?}"),
        };
        // It was drawn as every other heading, small and muted, where the desktop draws a section's 14px title.
        assert!(section(json!({"level":3})));
        assert!(!section(json!({"level":2})));
        assert!(!section(json!({})));
    }

    #[test]
    fn only_a_text_input_of_type_password_is_a_password() {
        let password = |kind: &str, kind_prop: &str| match parse_one(
            json!({"id":1,"type":kind,"props":{"label":"Token","type":kind_prop},"children":[]}),
        ) {
            Node::Input(input) => input.password,
            other => panic!("{other:?}"),
        };
        assert!(password("TextInput", "password"));
        assert!(!password("TextInput", "time"));
        assert!(!password("TextInput", "text"));
        assert!(!password("Textarea", "password"));
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

    /// Tabs of the "line" variant are chips the app underlines; a select has no such look.
    #[test]
    fn chips_can_be_line_tabs_and_a_select_cannot() {
        let props = json!({"label":"Pull request","line":true,"options":[{"value":"a"}]});
        let Node::Chips(tabs) =
            parse_one(json!({"id":1,"type":"Chips","props":props,"children":[]}))
        else {
            panic!("chips")
        };
        assert!(tabs.line);
        assert!(!select(props).line);
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
    fn svg(props: Value) -> Option<Svg> {
        match tree(json!([{"id":1,"type":"Svg","props":props,"children":[]}]))
            .unwrap()
            .children
            .pop()
        {
            Some(Node::Svg(drawing)) => Some(drawing),
            None => None,
            Some(other) => panic!("an svg, not {other:?}"),
        }
    }

    #[test]
    fn an_svg_is_drawn_from_markup_the_app_writes_from_its_checked_shapes() {
        let drawing = svg(json!({
            "viewBox":"0 0 16 16","size":18,"tone":"warn","label":"Epic",
            "fill":"none","stroke":"currentColor","strokeWidth":"1.4","strokeLinecap":"round",
            "shapes":[
                ["rect",{"x":"2.5","y":2.5,"width":11,"height":"11","rx":2.5}],
                ["path",{"d":"M9 1.8 3.5 9H8l-1 5.2L12.5 7H8Z","fillRule":"evenodd"}],
                ["g",{"opacity":0.3},[["circle",{"cx":8,"cy":8,"r":2}],["line",{"x1":0,"y1":0,"x2":-4,"y2":4}]]],
                ["polygon",{"points":"0,0 4,0 4,4"}]]
        }))
        .unwrap();
        assert_eq!(
            (drawing.size, drawing.tone, drawing.label.as_deref()),
            (18.0, Some(Tone::Warn), Some("Epic"))
        );
        assert_eq!(
            drawing.markup,
            concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round">"#,
                r#"<rect x="2.5" y="2.5" width="11" height="11" rx="2.5"/>"#,
                r#"<path d="M9 1.8 3.5 9H8l-1 5.2L12.5 7H8Z" fill-rule="evenodd"/>"#,
                r#"<g opacity="0.3"><circle cx="8" cy="8" r="2"/><line x1="0" y1="0" x2="-4" y2="4"/></g>"#,
                r#"<polygon points="0,0 4,0 4,4"/>"#,
                "</svg>"
            )
        );
        // Without a size it is 16 points, and a size past an icon's is cut to 64.
        let plain = |props: Value| svg(props).unwrap();
        assert_eq!(plain(json!({"viewBox":"0 0 24 24","shapes":[]})).size, 16.0);
        assert_eq!(
            plain(json!({"viewBox":"0,0,24,24","size":1e9,"shapes":[]})).size,
            64.0
        );
    }

    #[test]
    fn an_svg_with_anything_the_app_does_not_draw_is_left_out() {
        let with = |shapes: Value| json!({"viewBox":"0 0 16 16","shapes":shapes});
        let mut nested = json!([["path",{"d":"M0 0h1"}]]);
        for _ in 0..5 {
            nested = json!([["g", {}, nested]]);
        }
        let many: Vec<Value> = (0..=svg::MAX_SHAPES)
            .map(|_| json!(["path",{"d":"M0 0h1"}]))
            .collect();
        // At the limits, it is drawn: what is left out below is left out for the one thing wrong with it.
        let mut deepest = json!([["path",{"d":"M0 0h1"}]]);
        for _ in 0..4 {
            deepest = json!([["g", {}, deepest]]);
        }
        let most = Value::Array(many[1..].to_vec());
        for drawn in [
            with(json!([["path",{"d":"M0 0h1"}]])),
            with(deepest),
            with(most),
        ] {
            assert!(svg(drawn.clone()).is_some(), "{drawn}");
        }
        for refused in [
            // Elements that load files, fonts or styles, and attributes outside the allowlist.
            with(json!([["image",{"href":"/etc/passwd","width":16,"height":16}]])),
            with(json!([["text",{"x":0,"y":0},[]]])),
            with(json!([["style", {}]])),
            with(json!([["use",{"href":"#a"}]])),
            with(json!([["path",{"d":"M0 0h1","href":"x"}]])),
            with(json!([["path",{"d":"M0 0h1","style":"fill:red"}]])),
            with(json!([["path",{"d":"M0 0h1","transform":"scale(9)"}]])),
            with(json!([["rect",{"d":"M0 0h1"}]])),
            // Values that aren't what the attribute takes, or would end it.
            with(json!([["path",{"d":"M0 0\" onload=\"x"}]])),
            with(json!([["path",{"d":"M0 0<script>"}]])),
            with(json!([["polygon",{"points":"0,0 M 4,4"}]])),
            with(json!([["path",{"d":"M0 0h1","fill":"red"}]])),
            with(json!([["path",{"d":"M0 0h1","stroke":"url(#paint)"}]])),
            with(json!([["path",{"d":"M0 0h1","strokeLinecap":"pointy"}]])),
            with(json!([["rect",{"width":-1}]])),
            with(json!([["rect",{"width":"10px"}]])),
            with(json!([["rect",{"x":"NaN"}]])),
            with(json!([["rect",{"x":1e9}]])),
            with(json!([["rect",{"x":true}]])),
            with(json!([["g",{"opacity":2}]])),
            // Shapes that aren't shapes, or hold shapes when only a g does.
            with(json!([["path",{"d":"M0 0h1"},[["path",{"d":"M0 0h1"}]]]])),
            with(json!([[7, {}]])),
            with(json!([["path"]])),
            with(json!(["path"])),
            with(json!({"path":{}})),
            json!({"viewBox":"0 0 16 16"}),
            // Over the limits.
            with(Value::Array(many)),
            with(nested),
            with(json!([["path",{"d":"M0 0".repeat(svg::MAX_MARKUP_BYTES)}]])),
            // A view box that isn't four numbers of a square.
            json!({"shapes":[]}),
            json!({"viewBox":"0 0 16","shapes":[]}),
            json!({"viewBox":"0 0 16 8","shapes":[]}),
            json!({"viewBox":"0 0 0 0","shapes":[]}),
            json!({"viewBox":"0 0 1e9 1e9","shapes":[]}),
            json!({"viewBox":"a b c d","shapes":[]}),
            json!({"viewBox":"0 0 16 16","fill":"blue","shapes":[]}),
        ] {
            assert_eq!(svg(refused.clone()), None, "{refused}");
        }
    }

    #[test]
    fn parses_a_split_a_scroll_a_bar_and_a_dot_with_a_colour() {
        let parsed = tree(json!([
            {"id":1,"type":"Split","props":{"defaultSize":320},"children":[
                {"id":2,"type":"Slot","props":{"name":"start"},"children":[
                    {"id":3,"type":"Bar","props":{"edge":"bottom","gap":"xs"},"children":[{"id":4,"type":"#text","text":"Head"}]},
                    {"id":5,"type":"Scroll","props":{},"children":[{"id":6,"type":"Dot","props":{"color":1122867},"children":[]}]}]},
                {"id":7,"type":"Slot","props":{"name":"end"},"children":[{"id":8,"type":"Dot","props":{"color":99999999},"children":[]}]}]}
        ]))
        .unwrap();
        let Node::Split(split) = &parsed.children[0] else {
            panic!("a split")
        };
        assert_eq!(split.size, 320.0);
        let (Node::Bar(bar), Node::Scroll(scroll)) = (&split.start[0], &split.start[1]) else {
            panic!("a bar over a scroll")
        };
        assert_eq!((bar.edge, bar.gap), (Edge::Bottom, Gap::Xs));
        assert!(matches!(
            &scroll.children[0],
            Node::Dot(Dot {
                color: Some(0x112233),
                ..
            })
        ));
        // A colour that isn't one is no colour.
        assert!(matches!(&split.end[0], Node::Dot(Dot { color: None, .. })));
    }
}
