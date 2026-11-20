//! What you set for every chat, kept in the plugin's storage: the page in Wings Settings (`settings_page.rs`) changes
//! it, and each chat page reads it. They are not a chat's own, so a new chat and another tab follow them too.

use serde_json::{Value, json};
use wings_plugin::settings::Setting;

use crate::{folds::Kind, session::MODES};

/// The permission mode a chat starts in.
pub const MODE: &str = "default-mode";
/// Every run of tool calls shows open with each call's output, as Claude Code's own verbose transcript does.
pub const DETAILED: &str = "detailed-output";

/// Claude is told to pause in every Claude Code session when your plan's usage nears its limit (`plan_limit.rs`).
pub const PAUSE_NEAR_LIMIT: &str = "pause-near-limit";

pub fn pause_near_limit() -> Setting<bool> {
    Setting::boolean(PAUSE_NEAR_LIMIT, true)
}

/// Every new chat turns Remote Control on as it starts (`remote.rs`), so you can go on with it on claude.ai or in the Claude
/// app.
pub const REMOTE_ON_START: &str = "remote-on-start";

pub fn remote_on_start() -> Setting<bool> {
    Setting::boolean(REMOTE_ON_START, true)
}

/// The session panel shows, in every chat: over the chat's right side, or as Wings' sidebar that opens with a chat
/// (`session_side.rs`).
pub const SIDE_PANEL: &str = "side-panel";

/// Where the session panel is: in the chat, or in Wings' right sidebar.
pub const SESSION_PANEL: &str = "session-panel";
pub const SESSION_PANELS: [(&str, &str); 2] = [("chat", "In the chat"), ("sidebar", "Wings sidebar")];

/// What lies behind the conversation, in every chat: the page's plain ground, or a slow drift of pixels Wings draws there
/// (`backdrop: "pixels"` on the page, Wings 0.2.59).
pub const BACKGROUND: &str = "background";
pub const BACKGROUNDS: [(&str, &str); 2] = [("plain", "Plain"), ("pixels", "Pixel drift")];

/// How big the conversation's text is, in every chat.
pub const TEXT_SIZE: &str = "text-size";
pub const TEXT_SIZES: [(&str, &str); 3] = [("small", "Small"), ("default", "Default"), ("large", "Large")];

/// A mode the chat knows, else the stored value is ignored and Claude Code's own default stands.
fn mode_of(stored: &Value) -> Option<String> {
    stored.as_str().filter(|mode| MODES.iter().any(|(value, _)| value == mode)).map(str::to_owned)
}

pub fn mode() -> Setting<String> {
    Setting::new(MODE, "default".to_owned(), mode_of, |mode| json!(mode))
}

pub fn side_panel() -> Setting<bool> {
    Setting::boolean(SIDE_PANEL, true)
}

fn session_panel_of(stored: &Value) -> Option<String> {
    stored.as_str().filter(|place| SESSION_PANELS.iter().any(|(value, _)| value == place)).map(str::to_owned)
}

pub fn session_panel() -> Setting<String> {
    Setting::new(SESSION_PANEL, "chat".to_owned(), session_panel_of, |place| json!(place))
}

/// Whether a value of the session panel setting is Wings' sidebar.
pub fn in_sidebar(place: &str) -> bool {
    place == "sidebar"
}

fn background_of(stored: &Value) -> Option<String> {
    stored.as_str().filter(|kind| BACKGROUNDS.iter().any(|(value, _)| value == kind)).map(str::to_owned)
}

pub fn background() -> Setting<String> {
    Setting::new(BACKGROUND, "plain".to_owned(), background_of, |kind| json!(kind))
}

/// Whether a value of the background setting is the drift of pixels.
pub fn pixels(kind: &str) -> bool {
    kind == "pixels"
}

/// Diffs in the chat (a permission card's, a finished edit's) show the old lines on the left and the new on the right, in every
/// chat. Off, they are one column.
pub const DIFF_SPLIT: &str = "diff-split";

pub fn diff_split() -> Setting<bool> {
    Setting::boolean(DIFF_SPLIT, false)
}

pub fn detailed() -> Setting<bool> {
    Setting::boolean(DETAILED, false)
}

/// Whether a kind of fold starts open in Detailed (`folds.rs`): on, as Detailed has always opened everything.
pub fn opens(kind: Kind) -> Setting<bool> {
    Setting::boolean(kind.key(), true)
}

fn text_size_of(stored: &Value) -> Option<String> {
    stored.as_str().filter(|size| TEXT_SIZES.iter().any(|(value, _)| value == size)).map(str::to_owned)
}

pub fn text_size() -> Setting<String> {
    Setting::new(TEXT_SIZE, "default".to_owned(), text_size_of, |size| json!(size))
}

/// What a text size multiplies the conversation's text by.
pub fn scale_of(size: &str) -> f32 {
    match size {
        "small" => 0.9,
        "large" => 1.2,
        _ => 1.0,
    }
}

/// The text size Wings' font keys take `size` to while a chat has the keyboard: one size up or down, held at the ends,
/// or back to the default.
pub fn stepped(size: &str, step: &str) -> &'static str {
    let at = TEXT_SIZES.iter().position(|(value, _)| *value == size).unwrap_or(1);
    let to = match step {
        "bigger" => (at + 1).min(TEXT_SIZES.len() - 1),
        "smaller" => at.saturating_sub(1),
        _ => 1,
    };
    TEXT_SIZES[to].0
}

/// Cmd-+, Cmd-- and Cmd-0 in a chat, as the page's `onTextScale` hears them: the text size of every chat, as in Settings.
pub fn step_text_size(step: &str) {
    drop(text_size().set(stepped(&text_size().get(), step).to_owned()));
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
    fn the_session_panel_is_in_the_chat_unless_you_picked_wings_sidebar() {
        assert_eq!(session_panel().get(), "chat");
        assert!(!in_sidebar(&session_panel().get()));
        assert_eq!(session_panel_of(&json!("sidebar")).as_deref(), Some("sidebar"));
        assert_eq!(session_panel_of(&json!("floating")), None);
        assert!(in_sidebar("sidebar"));
    }

    #[test]
    fn the_background_is_plain_unless_you_picked_the_pixels_and_a_change_reaches_the_chats() {
        assert_eq!(background().get(), "plain");
        assert!(!pixels(&background().get()));
        assert_eq!(background_of(&json!("pixels")).as_deref(), Some("pixels"));
        assert_eq!(background_of(&json!("plain")).as_deref(), Some("plain"));
        assert_eq!(background_of(&json!("aurora")), None);
        assert_eq!(background_of(&json!(true)), None);
        assert!(pixels("pixels") && !pixels("plain"));
        // What a set stores (`json!(kind)`) reads back as it was, and a listener hears the set.
        let setting = background();
        let heard = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let seen = heard.clone();
        setting.on_change(move |kind| seen.borrow_mut().push(kind.clone()));
        setting.set("pixels".to_owned()).unwrap();
        assert_eq!(*heard.borrow(), ["pixels"]);
    }

    #[test]
    fn a_text_size_is_one_the_chat_knows_and_scales_the_text() {
        assert_eq!(text_size().get(), "default");
        assert_eq!(text_size_of(&json!("large")).as_deref(), Some("large"));
        assert_eq!(text_size_of(&json!("huge")), None);
        assert_eq!(text_size_of(&json!(1)), None);
        assert_eq!(scale_of("default"), 1.0);
        assert!(scale_of("small") < 1.0 && scale_of("large") > 1.0);
        assert_eq!(scale_of("anything else"), 1.0);
    }

    #[test]
    fn the_font_keys_step_the_text_size_and_stop_at_its_ends() {
        assert_eq!(stepped("default", "bigger"), "large");
        assert_eq!(stepped("large", "bigger"), "large");
        assert_eq!(stepped("default", "smaller"), "small");
        assert_eq!(stepped("small", "smaller"), "small");
        assert_eq!(stepped("large", "reset"), "default");
    }

    #[test]
    fn a_click_flips_what_the_output_setting_shows() {
        assert!(!is_open(false, false));
        assert!(is_open(false, true));
        assert!(is_open(true, false));
        assert!(!is_open(true, true));
    }
}
