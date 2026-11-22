//! The pieces both pages draw with, as the nodes `wings-plugin-ui/native` makes of them.

use wings_plugin::ui::{Node, icon, muted, row, spinner, stack, text};

/// A button of `variant` and `size` ("sm", "icon-sm") that a screen reader names `aria`.
pub fn button(label: &str, variant: &str, size: Option<&str>, aria: Option<String>, disabled: bool, on_click: impl Fn() + 'static) -> Node {
    let mut button = Node::new("Button").prop("variant", variant).on("onClick", move |_| on_click());
    if let Some(size) = size {
        button = button.prop("size", size);
    }
    if let Some(aria) = aria {
        button = button.prop("label", aria);
    }
    if disabled {
        button = button.prop("disabled", true);
    }
    button.text(label)
}

/// `button` that shows a spinner before its label while `busy`, and takes no clicks then.
pub fn busy_button(label: &str, variant: &str, size: Option<&str>, aria: Option<String>, busy: bool, disabled: bool, on_click: impl Fn() + 'static) -> Node {
    let button = button(label, variant, size, aria, disabled || busy, on_click);
    if busy { Node::new("Button").prop("variant", variant).prop("disabled", true).child(spinner()).text(label) } else { button }
}

/// Lucide icons, as the paths `icon` draws on a 24-point grid.
pub const PLUS: [&str; 2] = ["M5 12h14", "M12 5v14"];
pub const PLAY: [&str; 1] = ["m6 3 14 9-14 9z"];
pub const RESUME: [&str; 2] = ["M22 12a10 10 0 1 1-20 0 10 10 0 0 1 20 0z", "m10 8 6 4-6 4z"];
pub const PAUSE: [&str; 2] = ["M8 5v14", "M16 5v14"];
pub const PENCIL: [&str; 2] = ["M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z", "m15 5 4 4"];
pub const TRASH: [&str; 3] = ["M3 6h18", "M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6", "M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"];
pub const REFRESH: [&str; 2] = ["M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8", "M21 3v5h-5"];
pub const EXTERNAL: [&str; 3] = ["M15 3h6v6", "M10 14 21 3", "M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"];
pub const CLOSE: [&str; 2] = ["M18 6 6 18", "m6 6 12 12"];

/// A quiet square button holding one of those icons, named by `label`, which is also its tooltip. It shows a spinner
/// instead of the icon, and takes no clicks, while `busy`.
pub fn icon_button(label: &str, paths: &[&str], busy: bool, on_click: impl Fn() + 'static) -> Node {
    let glyph = if busy { spinner() } else { icon(24, 2.0, paths).prop("size", 16) };
    button("", "ghost", Some("icon-sm"), Some(label.to_owned()), busy, on_click).prop("title", label).child(glyph)
}

pub fn small_badge(label: &str, tone: Option<&str>, variant: Option<&str>) -> Node {
    let mut badge = Node::new("Badge").prop("size", "sm");
    if let Some(tone) = tone {
        badge = badge.prop("tone", tone);
    }
    if let Some(variant) = variant {
        badge = badge.prop("variant", variant);
    }
    badge.text(label)
}

/// A control with its label above it and a hint under it, in the size of a field's text.
pub fn field(label: &str, hint: Option<String>, control: Node) -> Node {
    stack().prop("gap", "s").child(text(label).prop("weight", "medium").prop("size", "field")).child(control).maybe(hint.map(|hint| muted(hint).prop("size", "field")))
}

/// Several controls under one label, with a hint under them.
pub fn field_group(label: &str, hint: Option<String>, content: Node) -> Node {
    stack().prop("gap", "m").child(text(label).prop("weight", "medium").prop("size", "field")).child(content).maybe(hint.map(|hint| muted(hint).prop("size", "field")))
}

/// A multi-line text field, `rows` high at first.
pub fn textarea(label: &str, value: &str, rows: u32, disabled: bool, on_change: impl Fn(String) + 'static) -> Node {
    let area = Node::new("Textarea").prop("label", label).prop("value", value).prop("rows", rows).on("onChange", move |event| on_change(event["value"].as_str().unwrap_or_default().to_owned()));
    if disabled { area.prop("disabled", true) } else { area }
}

/// Label and value pairs, one per line.
pub fn properties(items: Vec<(String, Node)>) -> Node {
    stack().prop("gap", "s").children(items.into_iter().map(|(label, value)| {
        row().key(label.clone()).prop("gap", "m").prop("align", "baseline").child(muted(label).prop("size", "small")).child(value.prop("grow", true))
    }))
}
