//! A chat's name and how its turn ended: Rename (and a name Claude Code suggests) and the state its last turn left it in.
//!
//! Shapes are from Claude Code 2.1.328:
//! - Rename: the control request `{ "subtype": "rename_session", "title": <string>, "source": "remote" | "host",
//!   "session_id"?: <string> }`. `source: "host"` is "one the user made in the hosting application (an IDE), which the CLI
//!   counts as a user rename". `session_id`, when given and the process has since moved to another session (`/clear`),
//!   refuses the request. It answers an empty success, or an `error`. Claude Code then prints
//!   `{ "type": "system", "subtype": "session_title_changed", "title": ... }` (`notices.rs`), which names the tab.
//! - Suggest a name: `{ "subtype": "generate_session_title", "description": <string>, "persist"?: <bool> }` answers
//!   `{ "title": <string> | null }`. Without `persist` it only returns the title; with it the session is named too.
//! - `{ "type": "system", "subtype": "post_turn_summary", "summarizes_uuid", "status_category", "status_detail",
//!   "needs_action" }`, "background post-turn summary emitted after each assistant turn". `status_category` is `blocked`,
//!   `waiting`, `completed`, `review_ready` or `failed` ("blocked" arrives as `need_input` in some paths); `needs_action`
//!   is empty unless it is `blocked`.

use serde_json::{Value, json};

use crate::session::{Item, request};

/// The id of a rename request, followed by its number, and of the one that suggests a name.
pub const RENAME: &str = "rename-session-";
pub const SUGGEST: &str = "suggest-title";
/// The most of the conversation's start the suggestion is asked from.
const ABOUT_KEPT: usize = 1_000;

/// How the last turn ended, from `post_turn_summary`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TurnSummary {
    pub category: String,
    pub detail: String,
    pub needs_action: String,
}

impl TurnSummary {
    /// The state the sidebar's Agents shows for it (`working`, `blocked`, `done`, `idle`) and the line beside it. A turn
    /// that needs you, or failed, is `blocked`; one that finished is `done`, which the caller shows as `idle` while you
    /// are looking; one that waits on something is `working`. None for a category this chat doesn't know.
    pub fn state(&self) -> Option<(&'static str, Option<String>)> {
        let said = |text: &str| Some(text.trim().to_owned()).filter(|text| !text.is_empty());
        let (state, line) = match self.category.as_str() {
            "blocked" | "need_input" => ("blocked", said(&self.needs_action).or_else(|| said(&self.detail))),
            "failed" => ("blocked", said(&self.detail)),
            "waiting" => ("working", said(&self.detail)),
            "completed" | "review_ready" => ("done", said(&self.detail)),
            _ => return None,
        };
        Some((state, line))
    }
}

/// The rename dialog, what it asks for, and the last turn's summary.
#[derive(Debug, Default)]
pub struct Naming {
    /// The rename dialog is open, with the name in its box.
    pub open: bool,
    pub draft: String,
    /// A suggestion is being asked for, by the number of the ask.
    pub suggesting: Option<u32>,
    /// What went wrong, shown in the dialog.
    pub error: Option<String>,
    /// The name asked for, by the number of its request, until Claude Code answers.
    renaming: Option<(u32, String)>,
    pub summary: Option<TurnSummary>,
    asked: u32,
}

impl Naming {
    /// Takes in a line: the turn's summary.
    pub fn heard(&mut self, line: &Value) {
        if line["type"] == "system" && line["subtype"] == "post_turn_summary" {
            let text = |key: &str| line[key].as_str().unwrap_or_default().to_owned();
            self.summary = Some(TurnSummary { category: text("status_category"), detail: text("status_detail"), needs_action: text("needs_action") });
        }
    }

    /// A message of yours starts the next turn, which the last one's summary no longer describes.
    pub fn user_spoke(&mut self) {
        self.summary = None;
    }

    /// Opens the dialog with the name the session has now.
    pub fn show(&mut self, current: Option<&str>) {
        self.open = true;
        self.draft = current.unwrap_or_default().to_owned();
        self.error = None;
        self.suggesting = None;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.suggesting = None;
        self.error = None;
    }

    /// The request that names the session what the dialog holds, `session` being the one the chat is in. None for a blank
    /// name. The dialog closes; the name shows once Claude Code says so.
    pub fn rename(&mut self, n: u32, session: Option<&str>) -> Option<String> {
        let title = self.draft.trim().to_owned();
        if title.is_empty() {
            return None;
        }
        self.open = false;
        self.suggesting = None;
        let mut asked = json!({ "subtype": "rename_session", "title": title, "source": "host" });
        if let Some(session) = session {
            asked["session_id"] = session.into();
        }
        self.renaming = Some((n, title));
        Some(request(&format!("{RENAME}{n}"), asked))
    }

    /// The request that asks Claude Code for a name from the start of the conversation, without naming the session: the
    /// dialog's box gets it. None when there is nothing said yet to name it from.
    pub fn suggest(&mut self, about: &str) -> Option<String> {
        let about = about.trim();
        if about.is_empty() || self.suggesting.is_some() {
            return None;
        }
        self.asked += 1;
        self.suggesting = Some(self.asked);
        self.error = None;
        let about: String = about.chars().take(ABOUT_KEPT).collect();
        Some(request(SUGGEST, json!({ "subtype": "generate_session_title", "description": about })))
    }

    /// An answer to a rename or a suggestion: whether it was one. A name taken sets `title` (and holds it against the
    /// one Claude Code makes up); a refusal is an error in the conversation.
    pub fn answered(&mut self, id: &str, response: &Value, title: &mut Option<String>, titled: &mut bool, items: &mut Vec<Item>) -> bool {
        let failed = response["subtype"] == "error";
        let why = || response["error"].as_str().or(response["response"]["error"].as_str()).unwrap_or("Claude Code didn't say why.").to_owned();
        if id == SUGGEST {
            self.suggesting = None;
            match response["response"]["title"].as_str().map(str::trim).filter(|name| !name.is_empty()) {
                Some(name) if !failed => self.draft = name.to_owned(),
                _ if failed => self.error = Some(format!("Couldn't suggest a name: {}", why())),
                _ => self.error = Some("Claude Code had no name to suggest.".to_owned()),
            }
            return true;
        }
        let Some(n) = id.strip_prefix(RENAME).and_then(|n| n.parse::<u32>().ok()) else {
            return false;
        };
        if let Some((_, name)) = self.renaming.take().filter(|(asked, _)| *asked == n) {
            if failed {
                items.push(Item::Error(format!("Couldn't rename the session: {}", why())));
            } else {
                *title = Some(name);
                *titled = true;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(category: &str, detail: &str, needs: &str) -> TurnSummary {
        TurnSummary { category: category.into(), detail: detail.into(), needs_action: needs.into() }
    }

    #[test]
    fn a_post_turn_summary_is_kept_and_the_next_message_drops_it() {
        let mut naming = Naming::default();
        naming.heard(&json!({ "type": "system", "subtype": "post_turn_summary", "summarizes_uuid": "u", "status_category": "blocked",
            "status_detail": "Waiting on permission: Bash", "needs_action": "Approve or deny Bash", "uuid": "x", "session_id": "s" }));
        assert_eq!(naming.summary, Some(summary("blocked", "Waiting on permission: Bash", "Approve or deny Bash")));
        naming.user_spoke();
        assert_eq!(naming.summary, None);
    }

    #[test]
    fn each_status_category_maps_to_the_state_the_sidebar_shows() {
        let state = |category: &str, detail: &str, needs: &str| summary(category, detail, needs).state();
        assert_eq!(state("blocked", "Waiting on permission: Bash", "Approve or deny Bash"), Some(("blocked", Some("Approve or deny Bash".into()))));
        assert_eq!(state("need_input", "Which branch?", ""), Some(("blocked", Some("Which branch?".into()))));
        assert_eq!(state("failed", "The build broke", ""), Some(("blocked", Some("The build broke".into()))));
        assert_eq!(state("waiting", "Waiting on 2 subagents", ""), Some(("working", Some("Waiting on 2 subagents".into()))));
        assert_eq!(state("completed", "Fixed the login test", ""), Some(("done", Some("Fixed the login test".into()))));
        assert_eq!(state("review_ready", " ", ""), Some(("done", None)));
        assert_eq!(state("something_new", "x", ""), None);
    }

    #[test]
    fn rename_sends_the_title_as_the_host_with_the_session_and_the_name_shows_once_it_is_taken() {
        let mut naming = Naming::default();
        naming.show(Some("Old name"));
        assert_eq!(naming.draft, "Old name");
        naming.draft = "  Fix the login  ".into();
        let line = naming.rename(3, Some("s1")).unwrap();
        let sent: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(sent["type"], "control_request");
        assert_eq!(sent["request_id"], "rename-session-3");
        assert_eq!(sent["request"], json!({ "subtype": "rename_session", "title": "Fix the login", "source": "host", "session_id": "s1" }));
        assert!(!naming.open);
        let (mut title, mut titled, mut items) = (Some("Old name".to_owned()), false, Vec::new());
        let ok = json!({ "subtype": "success", "request_id": "rename-session-3" });
        assert!(naming.answered("rename-session-3", &ok, &mut title, &mut titled, &mut items));
        assert_eq!((title.as_deref(), titled, items.len()), (Some("Fix the login"), true, 0));
    }

    #[test]
    fn a_blank_name_sends_nothing_and_a_refused_rename_says_so_and_keeps_the_old_name() {
        let mut naming = Naming::default();
        naming.show(None);
        naming.draft = "   ".into();
        assert_eq!(naming.rename(1, None), None);
        assert!(naming.open, "the dialog stays for a blank name");
        naming.draft = "New".into();
        let line: Value = serde_json::from_str(&naming.rename(2, None).unwrap()).unwrap();
        assert!(line["request"].get("session_id").is_none());
        let (mut title, mut titled, mut items) = (Some("Old".to_owned()), false, Vec::new());
        let refused = json!({ "subtype": "error", "request_id": "rename-session-2", "error": "rename_session is not supported in this context" });
        assert!(naming.answered("rename-session-2", &refused, &mut title, &mut titled, &mut items));
        assert_eq!((title.as_deref(), titled), (Some("Old"), false));
        assert_eq!(items, [Item::Error("Couldn't rename the session: rename_session is not supported in this context".into())]);
        assert!(!naming.answered("get-usage", &refused, &mut title, &mut titled, &mut items));
    }

    #[test]
    fn a_suggestion_asks_without_naming_the_session_and_fills_the_box() {
        let mut naming = Naming::default();
        naming.show(None);
        assert_eq!(naming.suggest("  "), None, "nothing said yet to name it from");
        let line: Value = serde_json::from_str(&naming.suggest("Fix the flaky login test").unwrap()).unwrap();
        assert_eq!(line["request_id"], SUGGEST);
        assert_eq!(line["request"], json!({ "subtype": "generate_session_title", "description": "Fix the flaky login test" }));
        assert!(naming.suggesting.is_some() && naming.suggest("again").is_none(), "one ask at a time");
        let (mut title, mut titled, mut items) = (None, false, Vec::new());
        let answer = json!({ "subtype": "success", "request_id": SUGGEST, "response": { "title": " Fix flaky login test " } });
        assert!(naming.answered(SUGGEST, &answer, &mut title, &mut titled, &mut items));
        assert_eq!((naming.draft.as_str(), naming.suggesting, title.clone()), ("Fix flaky login test", None, None));
        // No title is said in the dialog, not in the conversation.
        naming.suggest("x");
        let empty = json!({ "subtype": "success", "request_id": SUGGEST, "response": { "title": null } });
        naming.answered(SUGGEST, &empty, &mut title, &mut titled, &mut items);
        assert_eq!(naming.error.as_deref(), Some("Claude Code had no name to suggest."));
        assert!(items.is_empty());
    }
}
