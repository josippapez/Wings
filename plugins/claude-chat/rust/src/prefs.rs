//! What you set for every chat, kept in the plugin's storage: the page in Wings Settings (`settings_page.rs`) changes
//! it, and each chat page reads it. They are not a chat's own, so a new chat and another tab follow them too.

use serde_json::{Value, json};
use wings_plugin::settings::Setting;

use crate::session::MODES;

/// The permission mode a chat starts in.
pub const MODE: &str = "default-mode";
/// Every run of tool calls shows open with each call's output, as Claude Code's own verbose transcript does.
pub const DETAILED: &str = "detailed-output";

/// A mode the chat knows, else the stored value is ignored and Claude Code's own default stands.
fn mode_of(stored: &Value) -> Option<String> {
    stored.as_str().filter(|mode| MODES.iter().any(|(value, _)| value == mode)).map(str::to_owned)
}

pub fn mode() -> Setting<String> {
    Setting::new(MODE, "default".to_owned(), mode_of, |mode| json!(mode))
}

pub fn detailed() -> Setting<bool> {
    Setting::boolean(DETAILED, false)
}

/// The mode to ask Claude Code for as a chat starts. None while it is Claude Code's own default, which it starts in.
pub fn start_mode() -> Option<String> {
    Some(mode().get()).filter(|mode| mode != "default")
}

/// Whether a fold shows open: Compact folds everything and Detailed opens everything, and each click on one flips that.
pub fn is_open(detailed: bool, clicked: bool) -> bool {
    detailed != clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chat_starts_in_claude_codes_own_mode_unless_you_picked_another() {
        // Outside Wings nothing is kept, so nothing is picked.
        assert_eq!(start_mode(), None);
        assert!(!detailed().get());
        assert_eq!(mode_of(&json!("auto")).as_deref(), Some("auto"));
        assert_eq!(mode_of(&json!("bypassEverything")), None);
        assert_eq!(mode_of(&json!(3)), None);
    }

    #[test]
    fn a_click_flips_what_the_output_setting_shows() {
        assert!(!is_open(false, false));
        assert!(is_open(false, true));
        assert!(is_open(true, false));
        assert!(!is_open(true, true));
    }
}
