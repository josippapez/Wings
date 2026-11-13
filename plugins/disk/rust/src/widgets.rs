//! The small pieces the tab's views are made of, over the components `wings_plugin::ui` doesn't build.

use wings_plugin::ui::{Node, muted, row, stack, text};

/// `node` with `gap` between its children.
pub fn gap(node: Node, gap: &str) -> Node {
    node.prop("gap", gap)
}

/// A button of `variant` and `size` labelled `label`, which a screen reader calls `name` when it is only a symbol.
pub fn button(label: &str, variant: &str, size: &str, on_click: impl Fn() + 'static) -> Node {
    Node::new("Button").prop("variant", variant).prop("size", size).on("onClick", move |_| on_click()).text(label)
}

/// A button that reads as `label`, named `name`.
pub fn named_button(label: &str, name: &str, variant: &str, size: &str, on_click: impl Fn() + 'static) -> Node {
    button(label, variant, size, on_click).prop("label", name)
}

pub fn icon(name: &str) -> Node {
    Node::new("Icon").prop("name", name)
}

/// A page's title.
pub fn title(text_: impl Into<String>) -> Node {
    text(text_).prop("weight", "strong")
}

/// A message with a button that takes it away, for one that stays until you've read it.
pub fn dismissible(tone: &str, message: &str, on_dismiss: impl Fn() + 'static) -> Node {
    let close = Node::new("Button").prop("variant", "ghost").prop("size", "icon-xs").prop("label", "Dismiss").on("onClick", move |_| on_dismiss()).text("×");
    gap(row(), "s").prop("align", "start").child(wings_plugin::ui::notice(tone, message)).child(close)
}

/// Small figures side by side, label over value.
pub fn stats(items: Vec<(&str, String)>) -> Node {
    gap(row(), "l").prop("wrap", true).children(items.into_iter().map(|(label, value)| {
        gap(stack(), "none").key(label).child(muted(label).prop("size", "tiny")).child(text(value).prop("weight", "medium"))
    }))
}

/// A button that shows its `items`, as `(label, handler)`, under it when pressed: the app has no popup menu yet.
pub fn menu(label: &str, name: &str, variant: &str, size: &str, open: bool, toggle: impl Fn() + 'static, items: Vec<(String, Box<dyn Fn()>)>) -> Node {
    let items = items.into_iter().map(|(label, on_select)| {
        let key = label.clone();
        Node::new("Button").key(key).prop("variant", "ghost").on("onClick", move |_| on_select()).text(label)
    });
    gap(stack(), "xs").child(named_button(label, name, variant, size, toggle)).children(open.then_some(items).into_iter().flatten())
}

pub fn section(title_: &str, children: Vec<Node>) -> Node {
    gap(stack(), "m").key(title_).child(wings_plugin::ui::heading(2, title_)).children(children)
}
