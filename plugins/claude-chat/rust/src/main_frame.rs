//! Puts a Chat badge on every terminal Claude Code runs in. Its Open as chat button ends that terminal and continues the
//! session in a chat tab, started the way the terminal started it (`claude` or a wrapper like `claude-rtk`).

use std::cell::RefCell;

use serde_json::{Value, json};
use wings_plugin::{host, task};

thread_local! {
    static PANES: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
}

/// The badge of a pane Claude Code runs in, or none.
fn badge(pane: &Value) -> Value {
    if pane["session"]["kind"] != "claude" {
        return Value::Null;
    }
    json!({
        "label": "Chat",
        "tone": "neutral",
        "title": "Claude chat (experimental)",
        "rows": [{ "label": "Open as chat", "value": "Ends this terminal and goes on with the session in a chat tab" }],
        "actions": [{ "id": "open", "label": "Open as chat", "primary": true }],
    })
}

/// What the chat tab opens with, to go on with the session in `pane`.
fn chat_args(pane: &Value) -> Option<Value> {
    let session = &pane["session"];
    let program = session["launcher"].as_str().unwrap_or("claude");
    Some(json!({ "resume": session["sessionId"].as_str()?, "cwd": pane["cwd"], "program": program }))
}

#[unsafe(no_mangle)]
pub fn start() {
    host::on_panes(|list| {
        PANES.with(|panes| *panes.borrow_mut() = list.to_vec());
        let badges: Vec<_> = list.iter().map(|pane| (pane["paneId"].as_str().unwrap_or_default().to_owned(), badge(pane))).collect();
        task::spawn(async move {
            for (pane, badge) in badges {
                let _ = host::set_badge(&pane, &badge).await;
            }
        });
    });
    host::on_action(|action| async move {
        let id = action["paneId"].as_str().unwrap_or_default().to_owned();
        let pane = PANES.with(|panes| panes.borrow().iter().find(|pane| pane["paneId"] == id.as_str()).cloned());
        let args = pane.as_ref().and_then(chat_args).ok_or("Claude Code isn't running in this terminal any more")?;
        // The terminal's Claude Code ends first, so the chat's is the only one writing to the session.
        host::close_pane(&id).await?;
        host::open_view_with("chat", "tab", &args).await.map(drop)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_claude_panes_get_the_badge_and_it_resumes_with_their_wrapper() {
        let claude = json!({ "paneId": "3", "cwd": "/repo", "session": { "kind": "claude", "sessionId": "s1", "launcher": "claude-rtk" } });
        assert_eq!(badge(&claude)["actions"][0]["id"], "open");
        assert_eq!(chat_args(&claude), Some(json!({ "resume": "s1", "cwd": "/repo", "program": "claude-rtk" })));
        let plain = json!({ "paneId": "4", "cwd": "/repo", "session": { "kind": "claude", "sessionId": "s2" } });
        assert_eq!(chat_args(&plain).unwrap()["program"], "claude");
        assert_eq!(badge(&json!({ "paneId": "5", "session": { "kind": "opencode" } })), Value::Null);
        assert_eq!(badge(&json!({ "paneId": "6", "session": null })), Value::Null);
    }
}
