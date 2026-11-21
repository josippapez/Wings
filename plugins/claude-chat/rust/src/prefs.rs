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

/// A chat stops Claude's turn, as its Stop button does, when your plan's usage nears its limit (`plan_limit::stop_for`).
pub const STOP_NEAR_LIMIT: &str = "stop-near-limit";

pub fn stop_near_limit() -> Setting<bool> {
    Setting::boolean(STOP_NEAR_LIMIT, false)
}

/// The share of a limit (percent) at which Claude is told to pause, and at which a chat stops its turn.
pub const PAUSE_AT: &str = "pause-at";
pub const STOP_AT: &str = "stop-at";
pub const LIMIT_SHARES: [(&str, &str); 5] = [("70", "70%"), ("80", "80%"), ("90", "90%"), ("95", "95%"), ("100", "100%")];

fn share_of(stored: &Value) -> Option<String> {
    stored.as_str().filter(|share| LIMIT_SHARES.iter().any(|(value, _)| value == share)).map(str::to_owned)
}

pub fn pause_at() -> Setting<String> {
    Setting::new(PAUSE_AT, "90".to_owned(), share_of, |share| json!(share))
}

pub fn stop_at() -> Setting<String> {
    Setting::new(STOP_AT, "90".to_owned(), share_of, |share| json!(share))
}

/// The percent a value of those settings stands for.
pub fn percent(share: &str) -> f64 {
    share.parse().unwrap_or(90.0)
}

/// The share an account has of its own, where it differs from the shared `PAUSE_AT`/`STOP_AT`: by the account's name ("" for
/// the default account), each `{ "pause": "80", "stop": "95" }` with only the ones set. With more than one account Settings
/// shows a row for each (`settings_page.rs`).
pub const ACCOUNT_AT: &str = "account-at";

/// Which of an account's two shares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limit {
    Pause,
    Stop,
}

impl Limit {
    fn key(self) -> &'static str {
        match self {
            Limit::Pause => "pause",
            Limit::Stop => "stop",
        }
    }
}

/// An account's own value that turns the limit off for it, whatever the shared one is.
pub const OFF: &str = "off";

pub type AccountShares = std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>;

fn account_shares_of(stored: &Value) -> Option<AccountShares> {
    let accounts = stored.as_object()?;
    Some(
        accounts
            .iter()
            .map(|(name, shares)| {
                let valid = shares.as_object().into_iter().flatten().filter_map(|(key, share)| Some((key.clone(), share_of(share).or_else(|| (share.as_str() == Some(OFF)).then(|| OFF.to_owned()))?))).collect();
                (name.clone(), valid)
            })
            .collect(),
    )
}

pub fn account_at() -> Setting<AccountShares> {
    Setting::new(ACCOUNT_AT, AccountShares::new(), account_shares_of, |shares| json!(shares))
}

/// The share `account` (None for the default one) has of its own for `limit`, if it was picked.
pub fn own_share(shares: &AccountShares, account: Option<&str>, limit: Limit) -> Option<String> {
    shares.get(account.unwrap_or_default())?.get(limit.key()).cloned()
}

/// The percent of `limit` an account is held to, or None when it is off for it: its own pick when there is one ("off", or a
/// share), else the shared one (`shared_on`, the shared checkbox, and `shared`, a value of `pause_at()` or `stop_at()`).
pub fn percent_for(shares: &AccountShares, account: Option<&str>, limit: Limit, shared_on: bool, shared: &str) -> Option<f64> {
    match own_share(shares, account, limit) {
        Some(own) if own == OFF => None,
        Some(own) => Some(percent(&own)),
        None => shared_on.then(|| percent(shared)),
    }
}

/// `percent_for` from what is kept.
pub fn pause_percent(account: Option<&str>) -> Option<f64> {
    percent_for(&account_at().get(), account, Limit::Pause, pause_near_limit().get(), &pause_at().get())
}

pub fn stop_percent(account: Option<&str>) -> Option<f64> {
    percent_for(&account_at().get(), account, Limit::Stop, stop_near_limit().get(), &stop_at().get())
}

/// Keeps `share` as `account`'s own for `limit`, or None to follow the shared one again.
pub fn set_own_share(account: Option<&str>, limit: Limit, share: Option<String>) {
    let mut shares = account_at().get();
    let own = shares.entry(account.unwrap_or_default().to_owned()).or_default();
    match share {
        Some(share) => drop(own.insert(limit.key().to_owned(), share)),
        None => drop(own.remove(limit.key())),
    }
    shares.retain(|_, own| !own.is_empty());
    drop(account_at().set(shares));
}

/// Every new chat turns Remote Control on as it starts (`remote.rs`), so you can go on with it on claude.ai or in the Claude
/// app.
/// Whether a subagent's report to the chat starts folded to its first line. Off: a report shows in full, in every view
/// mode, as Claude Code says to show it.
pub fn fold_reports() -> Setting<bool> {
    Setting::boolean("fold-reports", false)
}

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

/// What lies behind the conversation, in every chat: the page's plain ground, a slow pattern of pixels Wings draws there
/// (`backdrop: "pixels"` on the page, Wings 0.2.59, or `"pixels:<pattern>"`, a Wings after 0.2.60, which an older one
/// leaves plain; `mono`, `ocean`, `meadow`, `dusk`, `blocks`, `bayer`, `grain` and `grain-lagoon` need Wings 0.2.75, and an older Wings draws
/// the plain drift for them, or the plain ground before 0.2.61), or a moving gradient Wings draws with a shader (`"gradient:<preset>"`, Wings 0.2.60), whose colours are
/// shadergradient's presets'.
pub const BACKGROUND: &str = "background";
pub const BACKGROUNDS: [(&str, &str); 24] = [
    ("plain", "Plain"),
    ("pixels", "Pixel drift"),
    ("pixels:rain", "Pixel rain"),
    ("pixels:checker", "Checker shimmer"),
    ("pixels:embers", "Embers"),
    ("pixels:mono", "Black and white"),
    ("pixels:ocean", "Ocean pixels"),
    ("pixels:meadow", "Meadow pixels"),
    ("pixels:dusk", "Dusk pixels"),
    ("pixels:blocks", "Pixel blocks"),
    ("pixels:bayer", "Ordered dither"),
    ("pixels:grain", "Grainy gradient"),
    ("pixels:grain-lagoon", "Grainy lagoon"),
    ("gradient:halo", "Halo"),
    ("gradient:pensive", "Pensive"),
    ("gradient:mint", "Mint"),
    ("gradient:interstella", "Interstella"),
    ("gradient:nighty-night", "Nighty night"),
    ("gradient:topo", "Topography"),
    ("gradient:flow", "Flow lines"),
    ("gradient:grid", "Grid"),
    ("gradient:dots", "Dot matrix"),
    ("gradient:silk", "Silk"),
    ("gradient:cells", "Cells"),
];

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

/// Whether the session panel, while it shows in the chat, is collapsed to a slim rail (true) or open (false). Unset until
/// you press the panel's own collapse or expand button: then a narrow tab, where the panel would cover the conversation,
/// starts with the rail and a wide one with the panel open.
pub const SIDE_RAIL: &str = "side-rail";

pub fn side_rail() -> Setting<Option<bool>> {
    Setting::new(SIDE_RAIL, None, |stored| Some(stored.as_bool()), |rail| json!(rail))
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

/// The page's `backdrop` for a value of the background setting: none for the plain ground, else the value itself.
pub fn backdrop(kind: &str) -> Option<String> {
    (kind != "plain").then(|| kind.to_owned())
}

/// Diffs in the chat (a permission card's, a finished edit's) show the old lines on the left and the new on the right, in every
/// chat. Off, they are one column.
pub const DIFF_SPLIT: &str = "diff-split";

pub fn diff_split() -> Setting<bool> {
    Setting::boolean(DIFF_SPLIT, false)
}

/// The session panel's sections you folded, by key (`view::fold`): one list for every chat, so a section stays as you left
/// it across chats and restarts. Empty reads as all open.
pub const PANEL_CLOSED: &str = "panel-closed";

fn closed_of(stored: &Value) -> Option<Vec<String>> {
    stored.as_array().map(|keys| keys.iter().filter_map(Value::as_str).map(str::to_owned).collect())
}

pub fn panel_closed() -> Setting<Vec<String>> {
    Setting::new(PANEL_CLOSED, Vec::new(), closed_of, |keys| json!(keys))
}

pub fn detailed() -> Setting<bool> {
    Setting::boolean(DETAILED, false)
}

/// What Claude thought shows open, with tool calls as they are: Claude Code's Thinking view mode (`view_mode.rs`). The other
/// modes are `DETAILED` off (Normal) and on (Verbose).
pub const THINKING: &str = "show-thinking";

pub fn thinking() -> Setting<bool> {
    Setting::boolean(THINKING, false)
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
    fn the_background_is_plain_unless_you_picked_the_pixels_or_a_gradient_and_a_change_reaches_the_chats() {
        assert_eq!(background().get(), "plain");
        assert_eq!(backdrop(&background().get()), None);
        assert_eq!(background_of(&json!("pixels")).as_deref(), Some("pixels"));
        assert_eq!(background_of(&json!("plain")).as_deref(), Some("plain"));
        assert_eq!(background_of(&json!("gradient:mint")).as_deref(), Some("gradient:mint"));
        for pattern in ["pixels:rain", "pixels:checker", "pixels:embers", "pixels:mono", "pixels:ocean", "pixels:meadow", "pixels:dusk", "pixels:blocks", "pixels:bayer", "pixels:grain", "pixels:grain-lagoon"] {
            assert_eq!(background_of(&json!(pattern)).as_deref(), Some(pattern));
            assert_eq!(backdrop(pattern).as_deref(), Some(pattern), "the page asks Wings for the pattern by name");
        }
        assert_eq!(background_of(&json!("pixels:snow")), None);
        assert_eq!(background_of(&json!("gradient:aurora")), None);
        // Moire is gone: a stored choice of it reads as the default, the plain ground.
        assert_eq!(background_of(&json!("gradient:moire")), None);
        assert_eq!(background_of(&json!("aurora")), None);
        assert_eq!(background_of(&json!(true)), None);
        assert_eq!(backdrop("pixels").as_deref(), Some("pixels"));
        assert_eq!(backdrop("gradient:halo").as_deref(), Some("gradient:halo"));
        assert_eq!(backdrop("plain"), None);
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
    fn the_folded_panel_sections_read_back_as_a_list_of_keys_and_anything_else_as_none_folded() {
        assert_eq!(closed_of(&json!(["tasks", "refs", 3])), Some(vec!["tasks".to_owned(), "refs".to_owned()]));
        assert_eq!(closed_of(&json!("tasks")), None);
        assert!(panel_closed().get().is_empty());
    }

    #[test]
    fn an_account_follows_the_shared_share_unless_it_has_its_own() {
        let mut shares = AccountShares::new();
        assert_eq!(percent_for(&shares, Some("work"), Limit::Pause, true, "90"), Some(90.0));
        shares.entry("work".to_owned()).or_default().insert("pause".to_owned(), "70".to_owned());
        shares.entry(String::new()).or_default().insert("stop".to_owned(), "100".to_owned());
        // The account's own wins for it, and only for the limit it set.
        assert_eq!(percent_for(&shares, Some("work"), Limit::Pause, true, "90"), Some(70.0));
        assert_eq!(percent_for(&shares, Some("work"), Limit::Stop, true, "95"), Some(95.0));
        // Other accounts fall back to the shared one; the default account is kept under "".
        assert_eq!(percent_for(&shares, Some("home"), Limit::Pause, true, "90"), Some(90.0));
        assert_eq!(percent_for(&shares, None, Limit::Stop, true, "80"), Some(100.0));
        assert_eq!(percent_for(&shares, None, Limit::Pause, true, "80"), Some(80.0));
        // What is kept reads back, dropping shares that are not one of the listed, and keeping "off".
        let read = account_shares_of(&json!({ "work": { "pause": "70", "stop": "55" }, "": 3 })).unwrap();
        assert_eq!(own_share(&read, Some("work"), Limit::Pause).as_deref(), Some("70"));
        assert_eq!(own_share(&read, Some("work"), Limit::Stop), None);
        assert_eq!(account_shares_of(&json!("x")), None);
        let read = account_shares_of(&json!({ "work": { "pause": "off" } })).unwrap();
        assert_eq!(own_share(&read, Some("work"), Limit::Pause).as_deref(), Some("off"));
    }

    #[test]
    fn an_account_can_turn_a_limit_off_or_on_whatever_the_shared_one_is() {
        let mut shares = AccountShares::new();
        // Following: the shared one's on/off and percent, both ways.
        assert_eq!(percent_for(&shares, Some("work"), Limit::Pause, true, "95"), Some(95.0));
        assert_eq!(percent_for(&shares, Some("work"), Limit::Pause, false, "95"), None);
        shares.entry("work".to_owned()).or_default().insert("pause".to_owned(), OFF.to_owned());
        shares.entry("work".to_owned()).or_default().insert("stop".to_owned(), "80".to_owned());
        // Off wins over the shared one being on; a percent wins over it being off.
        assert_eq!(percent_for(&shares, Some("work"), Limit::Pause, true, "95"), None);
        assert_eq!(percent_for(&shares, Some("work"), Limit::Stop, false, "95"), Some(80.0));
        // Another account still follows.
        assert_eq!(percent_for(&shares, Some("home"), Limit::Pause, true, "95"), Some(95.0));
    }

    #[test]
    fn a_click_flips_what_the_output_setting_shows() {
        assert!(!is_open(false, false));
        assert!(is_open(false, true));
        assert!(is_open(true, false));
        assert!(!is_open(true, true));
    }
}
