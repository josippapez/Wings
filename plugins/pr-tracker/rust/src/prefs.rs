//! The settings the Settings page changes and main and the pages read, each kept in storage under the key the TypeScript
//! used.

use crate::watch::{WATCH_KEY, notify_of, sidebar_of, watch_of};
use serde_json::json;
use wings_plugin::settings::Setting;

/// How often the lists are checked while Wings is in the background, in minutes.
pub fn watch_minutes() -> Setting<u32> {
    Setting::new(WATCH_KEY, 5, watch_of, |minutes| json!(minutes))
}

/// What to notify about: `off`, `review` or `all`.
pub fn notify_pulls() -> Setting<&'static str> {
    Setting::new("notify-pulls", "off", notify_of, |notify| json!(notify))
}

/// What the sidebar lists: `mine` or `all`.
pub fn sidebar_pulls() -> Setting<&'static str> {
    Setting::new("sidebar-pulls", "mine", sidebar_of, |sidebar| json!(sidebar))
}

/// Notify about comments that mention you or reply in a thread you wrote in.
pub fn mentions_on() -> Setting<bool> {
    Setting::boolean("notify-mentions", false)
}

/// Tell the agent on a PR's branch about its news.
pub fn tell_agents() -> Setting<bool> {
    Setting::boolean("tell-agents", false)
}
