//! The Claude chat's review: the diff viewer it asks for lets the user comment on lines, and each comment (or a draft
//! taken back) goes to that chat as the `review-comment` event, which the chat's main hands to the chat's page.

use serde_json::{Value, json};
use std::cell::RefCell;
use wings_plugin::{diff::on_comment, host};

/// The event the chat hears (`claude-chat`'s `events` in its manifest).
pub const EVENT: &str = "review-comment";
/// The most of a quoted line and of a comment one event carries: an event is up to 16 KB of JSON.
const QUOTE_MAX: usize = 2_000;
const BODY_MAX: usize = 6_000;

thread_local! {
    /// The chat the open viewer is a review for, by the key it sent with its `open-changes` event, and when the viewer
    /// opened. A viewer numbers its drafts from -1, so the chat tells those of one viewer from another's by that time.
    static CHAT: RefCell<Option<(String, u64)>> = const { RefCell::new(None) };
}

/// A viewer opens: a review for the chat `chat` names, or for none.
pub fn start(chat: Option<String>) {
    let opened = host::now() as u64;
    CHAT.with(|kept| *kept.borrow_mut() = chat.map(|chat| (chat, opened)));
}

/// The key of the chat an `open-changes` event's data asks for a review for.
pub fn chat_of(data: &Value) -> Option<String> {
    data["chat"].as_str().filter(|chat| !chat.is_empty() && chat.len() <= 64).map(str::to_owned)
}

fn cut(text: &Value, max: usize) -> String {
    text.as_str().unwrap_or_default().chars().take(max).collect()
}

/// The event for a comment the viewer handed over (`{ path, side, line, quote, body, id }`, or `{ delete: true, id }`),
/// for the chat `chat`, from the viewer opened at `opened`. Its `id` is the viewer's with that time before it.
pub fn event_of(chat: &str, opened: u64, comment: &Value) -> Result<Value, String> {
    let id = format!("{opened}:{}", comment["id"].as_i64().ok_or("The comment has no id")?);
    let data = if comment["delete"] == true {
        json!({ "chat": chat, "delete": true, "id": id })
    } else {
        let path = comment["path"].as_str().filter(|path| !path.is_empty()).ok_or("The comment has no file")?;
        let line = comment["line"].as_u64().filter(|line| *line > 0).ok_or("The comment has no line")?;
        let side = if comment["side"] == "old" { "old" } else { "new" };
        let mut data = json!({ "chat": chat, "path": path, "side": side, "line": line, "quote": cut(&comment["quote"], QUOTE_MAX), "body": cut(&comment["body"], BODY_MAX), "id": id });
        if let Some(end) = comment["endLine"].as_u64().filter(|end| *end > line) {
            data["endLine"] = end.into();
        }
        data
    };
    Ok(json!({ "name": EVENT, "data": data }))
}

/// Sends a comment on to the chat the open viewer is a review for.
async fn forward(comment: Value) -> Result<(), String> {
    let (chat, opened) = CHAT.with(|kept| kept.borrow().clone()).ok_or("No chat is waiting for comments from this viewer")?;
    host::call("emit", event_of(&chat, opened, &comment)?).await.map(drop)
}

/// Hears the comments typed in the viewer.
pub fn listen() {
    on_comment(forward);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_comment_becomes_an_event_for_its_chat_with_its_line_side_and_quote() {
        let comment = json!({ "diffId": "changes:1", "path": "src/a.rs", "side": "old", "line": 4, "quote": "let a = 1;", "body": "Why?", "id": -2 });
        assert_eq!(
            event_of("chat-7", 55, &comment).unwrap(),
            json!({ "name": "review-comment", "data": { "chat": "chat-7", "path": "src/a.rs", "side": "old", "line": 4, "quote": "let a = 1;", "body": "Why?", "id": "55:-2" } })
        );
        // Anything but "old" is the new side; a range's end is kept only when it is past the line; long texts are cut.
        let range = json!({ "path": "a", "side": "new", "line": 4, "endLine": 6, "quote": "q".repeat(3_000), "body": "b".repeat(9_000), "id": -3 });
        let data = &event_of("c", 1, &range).unwrap()["data"];
        assert_eq!((data["side"].as_str(), data["endLine"].as_u64()), (Some("new"), Some(6)));
        assert_eq!((data["quote"].as_str().unwrap().len(), data["body"].as_str().unwrap().len()), (QUOTE_MAX, BODY_MAX));
        assert!(event_of("c", 1, &json!({ "path": "a", "line": 4, "endLine": 3, "id": 1 })).unwrap()["data"]["endLine"].is_null());
    }

    #[test]
    fn a_draft_taken_back_and_a_comment_with_no_place_are_told_apart() {
        assert_eq!(event_of("c", 9, &json!({ "delete": true, "id": -2 })).unwrap()["data"], json!({ "chat": "c", "delete": true, "id": "9:-2" }));
        assert!(event_of("c", 1, &json!({ "path": "a", "side": "new", "line": 0, "id": 1 })).is_err());
        assert!(event_of("c", 1, &json!({ "path": "", "line": 2, "id": 1 })).is_err());
        assert!(event_of("c", 1, &json!({ "path": "a", "line": 2 })).is_err());
    }

    #[test]
    fn a_comment_is_emitted_to_the_chat_the_viewer_is_a_review_for_and_to_none_otherwise() {
        use std::{cell::RefCell, rc::Rc};
        use wings_plugin::task::block_on;
        let calls: Rc<RefCell<Vec<(String, Value)>>> = Rc::default();
        let seen = calls.clone();
        host::testing::answer(move |method, params| {
            seen.borrow_mut().push((method.to_owned(), params.clone()));
            Ok(Value::Null)
        });
        let comment = json!({ "diffId": "changes:1", "path": "a.rs", "side": "new", "line": 2, "quote": "x", "body": "Why?", "id": -1 });
        start(None);
        assert!(block_on(forward(comment.clone())).is_err());
        assert!(calls.borrow().is_empty());
        start(Some("chat-1".into()));
        block_on(forward(comment)).unwrap();
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "emit");
        assert_eq!((calls[0].1["name"].as_str(), calls[0].1["data"]["chat"].as_str(), calls[0].1["data"]["line"].as_u64()), (Some("review-comment"), Some("chat-1"), Some(2)));
    }

    #[test]
    fn the_chat_is_the_key_its_event_carried() {
        assert_eq!(chat_of(&json!({ "review": true, "chat": "k1" })), Some("k1".to_owned()));
        assert_eq!(chat_of(&json!({ "review": true })), None);
        assert_eq!(chat_of(&json!({ "chat": "" })), None);
        assert_eq!(chat_of(&json!({ "chat": "x".repeat(65) })), None);
    }
}
