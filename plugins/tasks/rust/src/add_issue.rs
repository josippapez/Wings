//! A line to type a title in. Enter or Add sends it to main; the input clears when main accepts it.

use crate::{
    domain::{Op, limits},
    feed,
    parts::{error_text, row},
};
use std::cell::RefCell;
use wings_plugin::{task::spawn, ui::Node};

#[derive(Default)]
struct Add {
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

pub fn node() -> Node {
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
            }
        });
    let add = Node::new("Button").prop("disabled", busy || title.trim().is_empty()).on("onClick", |_| submit());
    let add = if busy { add.child(Node::new("Spinner")).text("Add") } else { add.text("Add") };
    row("s").child(field).child(add).maybe(error.map(|error| error_text(&error).prop("size", "small")))
}
