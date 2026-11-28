//! The pieces both pages draw with, as the nodes `wings-plugin-ui/native` makes of them.

use wings_plugin::ui::{self, Node, muted, row, spinner, stack, text};

/// A button of `variant` and `size` ("sm", "icon-sm") that a screen reader names `aria`.
pub fn button(label: &str, variant: &str, size: Option<&str>, aria: Option<String>, disabled: bool, on_click: impl Fn() + 'static) -> Node {
    let mut button = ui::button(label, variant, false, on_click).disabled(disabled);
    if let Some(size) = size {
        button = button.size(size);
    }
    if let Some(aria) = aria {
        button = button.aria_label(&aria);
    }
    button
}

/// `button` that shows a spinner before its label while `busy`, and takes no clicks then.
pub fn busy_button(label: &str, variant: &str, size: Option<&str>, aria: Option<String>, busy: bool, disabled: bool, on_click: impl Fn() + 'static) -> Node {
    let button = button(label, variant, size, aria, disabled || busy, on_click);
    if busy { Node::new("Button").prop("variant", variant).prop("disabled", true).child(spinner()).text(label) } else { button }
}

/// Lucide's `circle-play`, which the Resume button draws; the other icons are in `ui::icons`.
pub const RESUME: [&str; 2] = ["M22 12a10 10 0 1 1-20 0 10 10 0 0 1 20 0z", "m10 8 6 4-6 4z"];

/// Lucide's `zap`, which Run now draws: a bolt, so it is not taken for Resume's circled play beside it on a paused row.
pub const RUN_NOW: [&str; 1] = ["M4 14a1 1 0 0 1-.78-1.63l9.9-10.2a.5.5 0 0 1 .86.46l-1.92 6.02A1 1 0 0 0 13 10h7a1 1 0 0 1 .78 1.63l-9.9 10.2a.5.5 0 0 1-.86-.46l1.92-6.02A1 1 0 0 0 11 14z"];

/// A quiet square button holding one of the icons, named by `label`, which is also its tooltip. It shows a spinner
/// instead of the icon, and takes no clicks, while `busy`.
pub fn icon_button(label: &str, paths: &[&str], busy: bool, on_click: impl Fn() + 'static) -> Node {
    if busy {
        button("", "ghost", Some("icon-sm"), Some(label.to_owned()), true, on_click).tooltip(label).child(spinner())
    } else {
        ui::icon_button(label, paths, on_click)
    }
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
    ui::textarea(label, value, rows, on_change).disabled(disabled)
}

/// Label and value pairs, one per line.
pub fn properties(items: Vec<(String, Node)>) -> Node {
    stack().prop("gap", "s").children(items.into_iter().map(|(label, value)| {
        row().key(label.clone()).prop("gap", "m").prop("align", "baseline").child(muted(label).prop("size", "small")).child(value.prop("grow", true))
    }))
}
