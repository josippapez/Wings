//! Real input for the agent's tools: a plugin's `input` command as the requests of the DevTools protocol's `Input` domain,
//! which Chromium handles as the person's own mouse and keyboard (`isTrusted` is true), in the order they came.

use serde_json::{Value, json};

use crate::page::tree::WebInput;

/// The request each event of `events` is, in order.
pub fn requests(events: &[WebInput]) -> Vec<(&'static str, Value)> {
    events
        .iter()
        .map(|event| match event {
            WebInput::Mouse { kind, x, y, button, buttons, click_count, modifiers } => {
                ("Input.dispatchMouseEvent", json!({ "type": kind, "x": x, "y": y, "button": button, "buttons": buttons, "clickCount": click_count, "modifiers": modifiers }))
            }
            WebInput::Key { kind, key, code, vk, text, modifiers, commands } => {
                let mut params = json!({ "type": kind, "key": key, "code": code, "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk, "modifiers": modifiers });
                if let Some(text) = text {
                    params["text"] = json!(text);
                    params["unmodifiedText"] = json!(text);
                }
                if !commands.is_empty() {
                    params["commands"] = json!(commands);
                }
                ("Input.dispatchKeyEvent", params)
            }
            WebInput::Text(text) => ("Input.insertText", json!({ "text": text })),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A click is a move, a press and a release at one point, and a typed text is one `insertText`: each becomes the request
    /// the protocol names, with its fields as the protocol spells them.
    #[test]
    fn events_become_the_protocols_requests_in_order() {
        let events = [
            WebInput::Mouse { kind: "mouseMoved", x: 5.0, y: 6.0, button: "none", buttons: 0, click_count: 0, modifiers: 0 },
            WebInput::Mouse { kind: "mousePressed", x: 5.0, y: 6.0, button: "left", buttons: 1, click_count: 1, modifiers: 0 },
            WebInput::Key { kind: "keyDown", key: "Enter".into(), code: "Enter".into(), vk: 13, text: Some("\r".into()), modifiers: 8, commands: vec![] },
            WebInput::Key { kind: "rawKeyDown", key: "a".into(), code: "KeyA".into(), vk: 65, text: None, modifiers: 4, commands: vec!["selectAll"] },
            WebInput::Text("hi".into()),
        ];
        let sent = requests(&events);
        assert_eq!(sent.iter().map(|(method, _)| *method).collect::<Vec<_>>(), ["Input.dispatchMouseEvent", "Input.dispatchMouseEvent", "Input.dispatchKeyEvent", "Input.dispatchKeyEvent", "Input.insertText"]);
        assert_eq!(sent[1].1, json!({ "type": "mousePressed", "x": 5.0, "y": 6.0, "button": "left", "buttons": 1, "clickCount": 1, "modifiers": 0 }));
        assert_eq!(sent[2].1["text"], "\r");
        assert_eq!(sent[2].1["windowsVirtualKeyCode"], 13);
        assert!(sent[3].1.get("text").is_none() && sent[3].1["commands"] == json!(["selectAll"]), "a shortcut types nothing");
        assert_eq!(sent[4].1, json!({ "text": "hi" }));
    }
}
