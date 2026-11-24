//! A tab asks main to do something and waits for its answer: `{ op, ...args }` over the SDK's `messages::ask`.

use crate::state::ASK;
use serde_json::{Value, json};
use wings_plugin::messages;

/// Asks main to do `op` with `args`, and gives what it answers, or why it couldn't.
pub fn ask(op: &str, args: Value) -> impl Future<Output = Result<Value, String>> + use<> {
    let mut asked = if args.is_null() { json!({}) } else { args };
    asked["op"] = op.into();
    messages::ask(ASK, asked)
}
