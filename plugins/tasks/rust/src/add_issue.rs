//! A line to type a title in. Enter or Add sends it to main; the input clears when main accepts it. On the Issues tab it
//! opens from the header's New issue button (`button`) as a bar under the filters (`bar`); the Project page's block shows
//! the line itself (`node`).

use crate::{
    domain::{Op, limits},
    feed,
    parts::{error_text, row},
};
use std::cell::RefCell;
use wings_plugin::{
    task::spawn,
    ui::{Node, icon},
};

/// Lucide's `plus`, as Claude chat's New chat button draws it.
const PLUS: [&str; 2] = ["M5 12h14", "M12 5v14"];

#[derive(Default)]
struct Add {
    /// The Issues tab's line to add one shows, under its filters.
    open: bool,
    title: String,
    busy: bool,
    error: Option<String>,
}

thread_local! {
    static ADD: RefCell<Add> = RefCell::new(Add::default());
}

fn with<T>(change: impl FnOnce(&mut Add) -> T) -> T {
    ADD.with(|add| change(&mut add.borrow_mut()))
}

fn submit() {
    let title = with(|add| {
        if add.title.trim().is_empty() || add.busy {
            return None;
        }
        add.busy = true;
        add.error = None;
        Some(add.title.clone())
    });
    let Some(title) = title else {
        return;
    };
    spawn(async move {
        let done = feed::apply(Op::Create(Op::create(&title))).await;
        with(|add| {
            add.busy = false;
            match done {
                Ok(()) => add.title.clear(),
                Err(error) => add.error = Some(error),
            }
        });
    });
}

/// The header's New issue button: opens the line to add one, and puts it away again.
pub fn button() -> Node {
    let open = with(|add| add.open);
    Node::new("Button")
        .prop("variant", if open { "secondary" } else { "ghost" })
        .prop("size", "icon-sm")
        .prop("label", "New issue")
        .prop("title", "New issue")
        .on("onClick", |_| with(|add| add.open = !add.open))
        .child(icon(24, 2.0, &PLUS).prop("size", 16))
}

/// The line to add an issue, as a bar under the Issues tab's filters, while the New issue button has it open. Escape puts
/// it away.
pub fn bar() -> Option<Node> {
    with(|add| add.open).then(|| Node::new("Bar").prop("edge", "bottom").child(line(true)))
}

/// The line to add an issue, as the Project page's block shows it.
pub fn node() -> Node {
    line(false)
}

fn line(wide: bool) -> Node {
    let (title, busy, error) = with(|add| (add.title.clone(), add.busy, add.error.clone()));
    let field = Node::new("TextInput")
        .prop("label", "New issue")
        .prop("placeholder", "Add an issue")
        .prop("value", title.as_str())
        .prop("maxLength", limits::TITLE)
        .on("onChange", |event| with(|add| add.title = event["value"].as_str().unwrap_or_default().to_owned()))
        .on("onKeyDown", |event| {
            if event["key"] == "Enter" {
                submit();
            } else if event["key"] == "Escape" {
                with(|add| add.open = false);
            }
        });
    let add = Node::new("Button").prop("disabled", busy || title.trim().is_empty()).on("onClick", |_| submit());
    let add = if busy { add.child(Node::new("Spinner")).text("Add") } else { add.text("Add") };
    if !wide {
        return row("s").child(field).child(add).maybe(error.map(|error| error_text(&error).prop("size", "small")));
    }
    // The field takes the bar's width, and has the keyboard as it opens.
    let field = Node::new("Stack").prop("grow", true).child(field.prop("autoFocus", true));
    let cancel = Node::new("Button").prop("variant", "ghost").on("onClick", |_| with(|add| add.open = false)).text("Cancel");
    let line = row("s").prop("align", "center").child(field).child(add).child(cancel);
    Node::new("Stack").prop("gap", "xs").child(line).maybe(error.map(|error| error_text(&error).prop("size", "small")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// New issue opens the line to add one as a bar, which takes the keyboard and its width; Escape puts it away.
    #[test]
    fn new_issue_opens_a_wide_line_that_escape_puts_away() {
        assert!(bar().is_none());
        with(|add| add.open = true);
        let open = wings_plugin::ui::tree_json(bar().expect("the line")).to_string();
        assert!(open.contains(r#""autoFocus":true"#) && open.contains(r#""grow":true"#) && open.contains("Cancel"), "{open}");
        assert!(wings_plugin::ui::tree_json(button()).to_string().contains(r#""variant":"secondary""#), "the button shows it's open");
        with(|add| add.open = false);
        assert!(bar().is_none());
        // The Project page's block keeps its own line, without Cancel.
        assert!(!wings_plugin::ui::tree_json(node()).to_string().contains("Cancel"));
    }
}
