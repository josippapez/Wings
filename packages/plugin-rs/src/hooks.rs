//! The Claude Code hooks a plugin answers: `wings.onHook`. The host sends a hook to the same `tool` message a tool call
//! comes in, so `tools::register` has to have been called for the plugin to hear one.

use serde_json::{Map, Value, json};
use std::{cell::RefCell, collections::HashMap, future::Future, pin::Pin, rc::Rc};

/// What a hook says back to Claude Code. A hook that says nothing lets it go on.
pub enum Say {
    /// Added to what Claude is told (the session start line, or a line with each message).
    Context(String),
    /// Stops a tool call, and why: Claude is asked to change what it did.
    Deny(String),
}

type Run = Rc<dyn Fn(Value, Value) -> Pin<Box<dyn Future<Output = Option<Say>>>>>;

thread_local! {
    static HOOKS: RefCell<HashMap<String, Run>> = RefCell::new(HashMap::new());
}

/// Answers the hook `event` (`SessionStart`, `PreToolUse`, `UserPromptSubmit`, `Journal`...) with `run`, which gets the
/// hook's input and `{ paneId, project, session }` of the terminal it came from.
pub fn on_hook<F, Fut>(event: &str, run: F)
where
    F: Fn(Value, Value) -> Fut + 'static,
    Fut: Future<Output = Option<Say>> + 'static,
{
    HOOKS.with(|hooks| hooks.borrow_mut().insert(event.to_owned(), Rc::new(move |input, context| Box::pin(run(input, context)))));
}

/// The JSON a hook call is answered with: `{ context?, deny? }`, empty for a hook the plugin has no handler for.
pub(crate) async fn answer(call: &Value) -> String {
    let name = call["name"].as_str().unwrap_or_default();
    let mut out = Map::new();
    if let Some(run) = HOOKS.with(|hooks| hooks.borrow().get(name).cloned()) {
        let input = if call["arguments"].is_null() { json!({}) } else { call["arguments"].clone() };
        let context = json!({ "paneId": call["paneId"], "project": call["project"], "session": call["session"] });
        match run(input, context).await {
            Some(Say::Context(text)) if !text.is_empty() => out.insert("context".into(), text.into()),
            Some(Say::Deny(reason)) if !reason.is_empty() => out.insert("deny".into(), reason.into()),
            _ => None,
        };
    }
    Value::Object(out).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::block_on;

    #[test]
    fn a_hook_is_answered_with_what_it_says_and_a_hook_nobody_handles_says_nothing() {
        on_hook("SessionStart", |input, context| async move { Some(Say::Context(format!("{} in {}", input["session_id"], context["project"]["name"]))) });
        on_hook("PreToolUse", |_, _| async { Some(Say::Deny("no".into())) });
        on_hook("UserPromptSubmit", |_, _| async { None });
        let hook = |name: &str| {
            let call = json!({ "hook": true, "name": name, "arguments": { "session_id": "s" }, "project": { "name": "web" } });
            block_on(async move { answer(&call).await })
        };
        assert_eq!(hook("SessionStart"), r#"{"context":"\"s\" in \"web\""}"#);
        assert_eq!(hook("PreToolUse"), r#"{"deny":"no"}"#);
        assert_eq!(hook("UserPromptSubmit"), "{}");
        assert_eq!(hook("Journal"), "{}");
    }
}
