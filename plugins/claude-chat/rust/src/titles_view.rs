//! The header's Rename and its dialog (`titles.rs`): a box with the session's name, Generate a name, Cancel and Rename.
//! Rename sends `rename_session`, so the name is the one Claude Code's `/resume` shows, and the tab follows it once Claude
//! Code says the title changed.

use wings_plugin::ui::{Node, button, dialog as panel, muted, notice, row, stack, text_input};

use crate::{
    chat::{self, Chat, State},
    session::Item,
};

const WIDTH: u32 = 480;

/// The "…" menu's Rename, while Claude Code runs in the chat.
pub fn menu_item(c: &Chat) -> Option<Node> {
    (c.state == State::Running).then(|| crate::view::menu_item("Rename session", open).prop("title", "Rename this session: the name Claude Code lists it by"))
}

/// The dialog, while it is open.
pub fn dialog(c: &Chat) -> Option<Node> {
    let naming = &c.session.naming;
    if !naming.open {
        return None;
    }
    let blank = naming.draft.trim().is_empty();
    let field = text_input("Name", &naming.draft, set).prop("placeholder", "A short name").prop("autoFocus", true).on("onKeyDown", |event| {
        if event["key"] == "Enter" {
            save();
        }
    });
    let can_suggest = about(c).is_some();
    let actions = row()
        .prop("gap", "s")
        .prop("wrap", true)
        .prop("align", "center")
        .child(button("Generate a name", "outline", naming.suggesting.is_some(), suggest).prop("disabled", !can_suggest || naming.suggesting.is_some()))
        .child(row().prop("grow", true).prop("justify", "end").prop("gap", "s").child(button("Cancel", "ghost", false, close)).child(button("Rename", "default", false, save).prop("disabled", blank)));
    Some(
        panel("Rename session", WIDTH, close)
            .child(stack().prop("gap", "m").child(field).maybe(naming.error.as_deref().map(|why| notice("warn", why))).child(muted("Past sessions and /resume show this name.").prop("size", "small")).child(actions)),
    )
}

/// What the conversation began with, for a suggested name.
fn about(c: &Chat) -> Option<String> {
    c.session.items.iter().find_map(|item| match item {
        Item::User(said) | Item::UserImages(said, _) => Some(said.trim().to_owned()).filter(|said| !said.is_empty()),
        _ => None,
    })
}

pub fn open() {
    chat::with(|c| {
        let current = chat::name(c);
        c.session.naming.show(c.session.title.as_deref().or(current.as_deref()));
    });
}

pub fn close() {
    chat::with(|c| c.session.naming.close());
}

fn set(said: String) {
    chat::with(|c| c.session.naming.draft = said);
}

/// Sends the name in the box as `rename_session`.
pub fn save() {
    chat::write(|c, n| {
        let session = c.session.id.clone();
        c.session.naming.rename(n, session.as_deref())
    });
}

/// Asks Claude Code for a name from the start of the conversation, for the box.
fn suggest() {
    chat::write(|c, _| {
        let about = about(c)?;
        c.session.naming.suggest(&about)
    });
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use wings_plugin::{host, task};

    use super::*;
    use crate::chat::with;

    type Calls = std::rc::Rc<std::cell::RefCell<Vec<(String, Value)>>>;

    fn chat_running() -> Calls {
        let calls: Calls = Default::default();
        let kept = calls.clone();
        host::testing::answer(move |method, params| {
            kept.borrow_mut().push((method.to_owned(), params.clone()));
            Ok(Value::Null)
        });
        let process = host::spawn("claude", &[], "/work/shop", "", |_| {});
        with(|c| {
            c.session = crate::session::Session::default();
            c.input = Some(process.input());
            c.state = State::Running;
            c.session.id = Some("s1".into());
            c.session.items.push(Item::User("Fix the flaky login test".into()));
        });
        calls
    }

    fn writes(calls: &Calls) -> Vec<Value> {
        task::block_on(async {});
        calls
            .borrow()
            .iter()
            .filter(|(method, _)| method == "processWrite")
            .flat_map(|(_, params)| params["text"].as_str().unwrap().lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()).collect::<Vec<_>>())
            .collect()
    }

    #[test]
    fn rename_sends_rename_session_and_the_title_and_the_tab_follow_once_it_is_taken() {
        let calls = chat_running();
        open();
        assert!(with(|c| dialog(c).is_some()));
        set("Fix the login".into());
        save();
        let sent = writes(&calls);
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert_eq!(sent[0]["request"], json!({ "subtype": "rename_session", "title": "Fix the login", "source": "host", "session_id": "s1" }));
        let id = sent[0]["request_id"].as_str().unwrap().to_owned();
        assert!(with(|c| dialog(c).is_none()), "the dialog closes when the name goes");
        // Not renamed until Claude Code takes it; then the title and the tab's name follow.
        assert_eq!(with(|c| chat::name(c)).as_deref(), Some("Fix the flaky login test"));
        with(|c| c.session.hear(&json!({ "type": "control_response", "response": { "subtype": "success", "request_id": id } })));
        chat::report_agent();
        assert_eq!(with(|c| chat::name(c)).as_deref(), Some("Fix the login"));
        let titled = calls.borrow().iter().rev().find(|(method, _)| method == "setTabTitle").map(|(_, params)| params["title"].clone());
        assert_eq!(titled, Some(json!("Fix the login")));
    }

    #[test]
    fn generate_a_name_asks_for_one_without_naming_and_puts_it_in_the_box() {
        let calls = chat_running();
        open();
        suggest();
        let sent = writes(&calls);
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert_eq!(sent[0]["request"], json!({ "subtype": "generate_session_title", "description": "Fix the flaky login test" }));
        with(|c| c.session.hear(&json!({ "type": "control_response", "response": { "subtype": "success", "request_id": "suggest-title", "response": { "title": "Fix flaky login test" } } })));
        assert_eq!(with(|c| c.session.naming.draft.clone()), "Fix flaky login test");
        assert_eq!(with(|c| c.session.title.clone()), None, "only the box changes until Rename");
    }

    #[test]
    fn a_blank_box_sends_nothing() {
        let calls = chat_running();
        open();
        with(|c| c.session.naming.draft.clear());
        save();
        assert!(writes(&calls).is_empty());
        assert!(with(|c| dialog(c).is_some()));
    }
}
