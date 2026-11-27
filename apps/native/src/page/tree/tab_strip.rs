//! A `TabStrip`: a browser-style row of tabs a plugin page shows, drawn as Wings' own title-bar tabs are. Its children
//! are `Tab`s and `TabGroup` labels in display order, read here into plain items (no node of their own), so a page can
//! only send what the strip knows how to draw.

use serde_json::{Map, Value};

use super::{Choice, MAX_NODES, Node, Raw, TreeError, Walk, choice, cut, str_prop, text_prop, valid_handler};

/// The most tabs and group labels a strip shows; the rest are dropped.
pub const MAX_TAB_ITEMS: usize = 500;
/// A tab's or group's `id` is at most this many bytes.
const MAX_ID_BYTES: usize = 128;

/// A group's colour, from the theme's own: a page can't send a colour of its own. Neutral is a missing or unknown one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GroupTone {
    #[default]
    Neutral,
    Info,
    Success,
    Warning,
    Danger,
    Accent,
}

impl Choice for GroupTone {
    fn choose(text: &str) -> Option<Self> {
        Some(match text {
            "neutral" => Self::Neutral,
            "info" => Self::Info,
            "success" => Self::Success,
            "warning" => Self::Warning,
            "danger" => Self::Danger,
            "accent" => Self::Accent,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tab {
    pub id: String,
    pub title: String,
    pub url: String,
    pub active: bool,
    /// A spinner stands where a page's icon would.
    pub loading: bool,
    /// The `id` of the `TabGroup` it belongs to.
    pub group: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TabGroup {
    pub id: String,
    pub name: String,
    pub tone: GroupTone,
    /// Its tabs are not drawn, and its label says how many there are.
    pub collapsed: bool,
    pub count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TabItem {
    Tab(Tab),
    Group(TabGroup),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TabStrip {
    pub id: u64,
    /// The strip's accessible name.
    pub label: String,
    /// A click on a tab: `{ id }`.
    pub on_select: Option<String>,
    /// A tab's close button or a middle click on it: `{ id }`.
    pub on_close: Option<String>,
    /// The "+" at the strip's end: `{}`.
    pub on_new: Option<String>,
    /// A click on a group's label: `{ id }`.
    pub on_group: Option<String>,
    pub items: Vec<TabItem>,
}

impl TabStrip {
    /// The tabs a strip draws: those of a collapsed group are left out.
    pub fn shown(&self) -> impl Iterator<Item = &TabItem> {
        self.items.iter().filter(|item| match item {
            TabItem::Tab(tab) => !self.is_collapsed(tab),
            TabItem::Group(_) => true,
        })
    }

    fn is_collapsed(&self, tab: &Tab) -> bool {
        let Some(group) = &tab.group else { return false };
        self.items.iter().any(|item| matches!(item, TabItem::Group(g) if g.id == *group && g.collapsed))
    }
}

fn key(props: &Map<String, Value>) -> Option<String> {
    str_prop(props, "id").filter(|id| !id.is_empty() && id.len() <= MAX_ID_BYTES).map(str::to_owned)
}

/// A strip of `children`, the `Tab`s and `TabGroup`s of it. Anything else among them, or one with no `id`, is dropped.
pub(super) fn parse(walk: &mut Walk, id: u64, props: &Map<String, Value>, children: Vec<Raw>) -> Result<Node, TreeError> {
    let handler = |name| str_prop(props, name).filter(|value| valid_handler(value)).map(str::to_owned);
    let mut items = Vec::new();
    for child in children {
        walk.nodes += 1;
        if walk.nodes > MAX_NODES {
            return Err(TreeError::TooManyNodes);
        }
        if items.len() >= MAX_TAB_ITEMS {
            continue;
        }
        let on = |name: &str| child.props.get(name) == Some(&Value::Bool(true));
        let Some(item_id) = key(&child.props) else { continue };
        match child.kind.as_str() {
            "Tab" => items.push(TabItem::Tab(Tab {
                id: item_id,
                title: text_prop(&child.props, "title"),
                url: text_prop(&child.props, "url"),
                active: on("active"),
                loading: on("loading"),
                group: str_prop(&child.props, "group").filter(|g| !g.is_empty() && g.len() <= MAX_ID_BYTES).map(str::to_owned),
            })),
            "TabGroup" => items.push(TabItem::Group(TabGroup {
                id: item_id,
                name: cut(str_prop(&child.props, "name").unwrap_or_default().to_owned()),
                tone: choice(&child.props, "tone"),
                collapsed: on("collapsed"),
                count: child.props.get("count").and_then(Value::as_u64).unwrap_or_default().min(100_000) as usize,
            })),
            _ => {}
        }
    }
    Ok(Node::TabStrip(TabStrip {
        id,
        label: text_prop(props, "label"),
        on_select: handler("onSelect"),
        on_close: handler("onClose"),
        on_new: handler("onNew"),
        on_group: handler("onGroup"),
        items,
    }))
}
