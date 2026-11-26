//! The two settings the browser tabs and the settings page share. A change reaches every open frame.

use crate::address::Engine;
use serde_json::json;
use wings_plugin::settings::Setting;

/// The search engine for what isn't an address.
pub fn engine() -> Setting<Engine> {
    Setting::new("engine", Engine::default(), |stored| stored.as_str().and_then(Engine::from_id), |engine| json!(engine.id()))
}

/// Whether the web views keep cookies and site data between runs.
pub fn keep_data() -> Setting<bool> {
    Setting::boolean("keepData", true)
}
