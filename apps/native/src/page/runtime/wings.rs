//! What a frame of a plugin reaches of the host: the `Link` the host implements, and the storage calls it takes.

use super::Reply;

/// What a frame of a plugin reaches through `wings`. The host that runs the frame implements it; each call is made on
/// the frame's own thread, so it must not wait for another frame.
pub trait Link: Send + Sync + 'static {
    fn focused(&self) -> bool {
        true
    }
    fn set_visible(&self, _visible: bool) {}
    /// The app is handing the frame a click, key press or pick of yours.
    fn user_event(&self) {}
    /// The frame broadcast `json`: the host sends it to the plugin's other frames. An error rejects the frame's promise.
    fn broadcast(&self, json: String) -> Result<(), String>;
    /// One call of `wings.storage`. The answer is JSON: the value, a list of keys or of `[key, value]`, or `null`.
    fn storage(&self, call: Store) -> Result<String, String>;
    /// What `wings.openView` opened this page with, as JSON, for `wings.viewArgs`; none for a page opened without args.
    fn view_args(&self) -> Option<String> {
        None
    }
    /// A call of `wings.<method>` with the JSON the frame's wrapper made of its arguments. The host answers once through
    /// `reply`, now or later and from any thread: a call that takes a while must not run on this one.
    fn call(&self, method: &str, params: serde_json::Value, reply: Reply);
}

/// A `wings.storage` call, with the key and value already as the desktop's frame hands them over: `String(key)`, and
/// the JSON text of a value that `JSON.stringify` could make one of.
#[derive(Debug, PartialEq, Eq)]
pub enum Store {
    Get(String),
    Set(String, String),
    Delete(String),
    Keys,
    Entries(String),
}
