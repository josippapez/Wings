//! Remote Control: the chat's session on claude.ai/code and in the Claude app, while Claude Code goes on running here.
//! Claude Code turns it on and off on a `remote_control` request, as the Desktop app's and VS Code's `/remote-control`
//! do, and answers with the session's link.

use serde_json::{Value, json};

use crate::session::request;

/// The id of the chat's requests to turn Remote Control on or off.
pub const REQUEST: &str = "remote-control";

#[derive(Debug, Default, Clone, PartialEq)]
pub enum Remote {
    #[default]
    Off,
    Connecting,
    /// Connected, with the session's address on claude.ai.
    On(String),
    /// Turning off, with the address it had.
    Disconnecting(String),
}

/// The line that turns it on, under `name` when given, else the chat's own title on claude.ai.
pub fn connect(remote: &mut Remote, name: Option<&str>) -> String {
    *remote = Remote::Connecting;
    let mut asked = json!({ "subtype": "remote_control", "enabled": true });
    if let Some(name) = name {
        asked["name"] = json!(name);
    }
    request(REQUEST, asked)
}

/// The line that turns it off. The chat goes on here.
pub fn disconnect(remote: &mut Remote) -> String {
    if let Remote::On(url) = remote {
        *remote = Remote::Disconnecting(std::mem::take(url));
    }
    request(REQUEST, json!({ "subtype": "remote_control", "enabled": false }))
}

/// Claude Code's answer to one of these requests, with what to tell you when it refused. None for any other answer.
pub fn answered(remote: &mut Remote, id: &str, response: &Value) -> Option<Option<String>> {
    if id != REQUEST {
        return None;
    }
    if response["subtype"] == "error" {
        let why = response["error"].as_str().unwrap_or("Claude Code didn't say why.");
        // Turning it off failed: it is still on.
        *remote = match std::mem::take(remote) {
            Remote::Disconnecting(url) => Remote::On(url),
            _ => Remote::Off,
        };
        return Some(Some(format!("Remote Control: {why}")));
    }
    *remote = match (&*remote, response["response"]["session_url"].as_str()) {
        (Remote::Disconnecting(_), _) => Remote::Off,
        (_, Some(url)) if !url.is_empty() => Remote::On(url.to_owned()),
        _ => Remote::Off,
    };
    Some(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn success(response: Value) -> Value {
        json!({ "subtype": "success", "request_id": REQUEST, "response": response })
    }

    #[test]
    fn it_connects_with_the_link_claude_code_answers_and_disconnects() {
        let mut remote = Remote::Off;
        let line: Value = serde_json::from_str(&connect(&mut remote, Some("Shop"))).unwrap();
        assert_eq!(line["request"], json!({ "subtype": "remote_control", "enabled": true, "name": "Shop" }));
        assert_eq!(remote, Remote::Connecting);
        let url = "https://claude.ai/code/session_01";
        assert_eq!(answered(&mut remote, REQUEST, &success(json!({ "session_url": url, "bridge_session_id": "b1" }))), Some(None));
        assert_eq!(remote, Remote::On(url.into()));

        let line: Value = serde_json::from_str(&disconnect(&mut remote)).unwrap();
        assert_eq!(line["request"], json!({ "subtype": "remote_control", "enabled": false }));
        assert_eq!(answered(&mut remote, REQUEST, &success(Value::Null)), Some(None));
        assert_eq!(remote, Remote::Off);
        assert_eq!(answered(&mut remote, "usage", &success(Value::Null)), None);
    }

    #[test]
    fn a_refusal_says_why_and_leaves_it_off() {
        let mut remote = Remote::Off;
        drop(connect(&mut remote, None));
        let refused = json!({ "subtype": "error", "request_id": REQUEST, "error": "Remote Control requires a claude.ai subscription" });
        assert_eq!(answered(&mut remote, REQUEST, &refused), Some(Some("Remote Control: Remote Control requires a claude.ai subscription".into())));
        assert_eq!(remote, Remote::Off);
    }
}
