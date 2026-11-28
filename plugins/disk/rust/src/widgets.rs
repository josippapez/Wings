//! The small pieces the tab's views are made of, over the components `wings_plugin::ui` doesn't build.

pub use wings_plugin::ui::title;
use wings_plugin::ui::{Node, muted, row, stack, text};

/// `node` with `gap` between its children.
pub fn gap(node: Node, gap: &str) -> Node {
    node.prop("gap", gap)
}

/// A button of `variant` and `size` labelled `label`, which a screen reader calls `name` when it is only a symbol.
pub fn button(label: &str, variant: &str, size: &str, on_click: impl Fn() + 'static) -> Node {
    wings_plugin::ui::button(label, variant, false, on_click).size(size)
}

/// A button that reads as `label`, named `name`.
pub fn named_button(label: &str, name: &str, variant: &str, size: &str, on_click: impl Fn() + 'static) -> Node {
    button(label, variant, size, on_click).aria_label(name)
}

pub fn icon(name: &str) -> Node {
    Node::new("Icon").prop("name", name)
}

/// A message with a button that takes it away, for one that stays until you've read it.
pub fn dismissible(tone: &str, message: &str, on_dismiss: impl Fn() + 'static) -> Node {
    let close = wings_plugin::ui::icon_button("Dismiss", &wings_plugin::ui::icons::CLOSE, on_dismiss);
    gap(row(), "s").prop("align", "start").child(wings_plugin::ui::notice(tone, message)).child(close)
}

/// Small figures side by side, label over value.
pub fn stats(items: Vec<(&str, String)>) -> Node {
    gap(row(), "l").prop("wrap", true).children(items.into_iter().map(|(label, value)| {
        gap(stack(), "none").key(label).child(muted(label).prop("size", "tiny")).child(text(value).prop("weight", "medium"))
    }))
}

/// An ellipsis icon button, named `name`, that opens `items`, as `(label, handler)`, in a menu under it. `end` lines
/// the menu up with the button's right edge.
pub fn menu(name: &str, end: bool, items: Vec<(String, Box<dyn Fn()>)>) -> Node {
    let trigger = wings_plugin::ui::icon_button(name, &wings_plugin::ui::icons::ELLIPSIS, || {});
    let items = items.into_iter().map(|(label, on_select)| Node::new("Button").key(label.clone()).on("onClick", move |_| on_select()).text(label));
    wings_plugin::ui::menu(trigger, items, end)
}

pub fn section(title_: &str, children: Vec<Node>) -> Node {
    gap(stack(), "m").key(title_).child(wings_plugin::ui::heading(2, title_)).children(children)
}
