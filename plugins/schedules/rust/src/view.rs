//! The pieces both pages draw with, as the nodes `wings-plugin-ui/native` makes of them.

use wings_plugin::ui::{Node, muted, row, spinner, stack, text};

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
