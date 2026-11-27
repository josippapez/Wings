//! The settings the browser tabs and the settings page share. A change reaches every open frame.

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

/// Whether sign-ins imported from another browser last past this run (in a kept store): off, they go as Wings quits.
pub fn keep_signins() -> Setting<bool> {
    Setting::boolean("keepSignIns", false)
}

/// The minutes an agent's tab can sit unused before it is closed (0 is never).
pub fn idle() -> Setting<u32> {
    Setting::new("idleMinutes", IDLE_MINUTES, |stored| stored.as_u64().and_then(|minutes| u32::try_from(minutes).ok()), |minutes| json!(minutes))
}

/// What `idle` is until the person chooses.
pub const IDLE_MINUTES: u32 = 10;

pub fn idle_minutes() -> u32 {
    idle().get()
}
