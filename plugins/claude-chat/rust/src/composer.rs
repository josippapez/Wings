//! What the message box's keys and the model menu add to typing: Up and Down recall the prompts you sent, Shift-Tab goes
//! to the next permission mode, Escape stops the turn, and the menu also holds fast mode and the output style, as a
//! terminal's Claude Code has them. Each is one request to Claude Code, from the key or click that asked for it.

use std::cell::RefCell;

use serde_json::{Value, json};

use crate::session::{Item, MODES, request};

/// Where Up has taken the message box among the prompts you sent: how many back from the newest, and the text it held
/// before the first Up, which Down brings back after the newest.
#[derive(Debug, PartialEq)]
pub struct Recall {
    back: usize,
    draft: String,
}

thread_local! {
    static RECALL: RefCell<Option<Recall>> = const { RefCell::new(None) };
}

/// Forgets where Up has taken you: the message was sent, or the session is another one.
pub fn forget() {
    RECALL.with(|recall| *recall.borrow_mut() = None);
}

/// The prompts you sent, oldest first, as the conversation holds them, so a resumed session's come from its transcript.
/// The same one twice in a row counts once, as a terminal's history does.
pub fn prompts(items: &[Item]) -> Vec<&str> {
    let mut found: Vec<&str> = Vec::new();
    for item in items {
        if let Item::User(said) | Item::UserImages(said, _) = item
            && !said.trim().is_empty()
            && found.last() != Some(&said.as_str())
        {
            found.push(said);
        }
    }
    found
}

/// What Up puts in the box: the prompt before the one it shows, the newest to start with. None when there is none
/// before, and the box stays as it is.
pub fn older(draft: &str, prompts: &[&str]) -> Option<String> {
    RECALL.with(|recall| older_in(&mut recall.borrow_mut(), draft, prompts))
}

/// What Down puts in the box: the next prompt, or after the newest the text the box held before the first Up. None when
/// Up hasn't taken it anywhere.
pub fn newer(prompts: &[&str]) -> Option<String> {
    RECALL.with(|recall| newer_in(&mut recall.borrow_mut(), prompts))
}

fn older_in(recall: &mut Option<Recall>, draft: &str, prompts: &[&str]) -> Option<String> {
    let back = match recall {
        Some(at) => at.back + 1,
        None => 0,
    };
    let said = prompts.len().checked_sub(back + 1).map(|at| prompts[at])?;
    match recall {
        Some(at) => at.back = back,
        None => *recall = Some(Recall { back, draft: draft.to_owned() }),
    }
    Some(said.to_owned())
}

fn newer_in(recall: &mut Option<Recall>, prompts: &[&str]) -> Option<String> {
    let at = recall.as_mut()?;
    if at.back == 0 {
        return recall.take().map(|at| at.draft);
    }
    at.back -= 1;
    prompts.len().checked_sub(at.back + 1).map(|at| prompts[at].to_owned())
}

/// The permission mode after `current`, as the mode menu lists them, round to the first. One that isn't listed goes to
/// the first.
pub fn next_mode(current: &str) -> &'static str {
    let at = MODES.iter().position(|(value, _)| *value == current);
    MODES[at.map_or(0, |at| (at + 1) % MODES.len())].0
}

#[derive(Debug, PartialEq)]
pub enum Escape {
    /// A dialog or sheet is open: it closes first.
    Close,
    /// Claude is working.
    Stop,
    Nothing,
}

/// What Escape in the message box does.
pub fn escape(dialog_open: bool, working: bool) -> Escape {
    if dialog_open {
        Escape::Close
    } else if working {
        Escape::Stop
    } else {
        Escape::Nothing
    }
}

/// Fast mode and the output style, as `initialize` says them and as the chat last set them.
#[derive(Debug, Default)]
pub struct Options {
    pub fast: bool,
    /// Why fast mode can't be used here, when `initialize` gives a reason the chat can't get round.
    pub fast_blocked: Option<String>,
    /// The output styles Claude Code has, and the one in use.
    pub styles: Vec<String>,
    pub style: Option<String>,
}

/// The reason `initialize` gives for fast mode being off in a session an SDK started: it wants an opt in, which the flag
/// setting is. `preference` is what it says once the setting is on.
const GET_ROUND: [&str; 2] = ["sdk_opt_in_required", "preference"];

impl Options {
    /// Takes in `initialize`'s answer.
    pub fn initialized(&mut self, answer: &Value) {
        self.fast = answer["fast_mode_state"] == "on";
        self.fast_blocked = answer["fast_mode_disabled_reason"].as_str().filter(|reason| !GET_ROUND.contains(reason)).map(str::to_owned);
        self.styles = answer["available_output_styles"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
        self.style = answer["output_style"].as_str().map(str::to_owned);
    }

    /// The line that turns fast mode on or off for this session, which the menu then shows. It is a setting of the
    /// session, not saved to yours.
    pub fn set_fast(&mut self, on: bool, n: u32) -> Option<String> {
        if self.fast_blocked.is_some() {
            return None;
        }
        self.fast = on;
        Some(request(&format!("fast-{n}"), json!({ "subtype": "apply_flag_settings", "settings": { "fastMode": on } })))
    }

    /// The line that switches the output style, which Claude Code applies from the next message. None for a name it
    /// didn't list.
    pub fn set_style(&mut self, name: &str, n: u32) -> Option<String> {
        if !self.styles.iter().any(|style| style == name) {
            return None;
        }
        self.style = Some(name.to_owned());
        Some(request(&format!("style-{n}"), json!({ "subtype": "apply_flag_settings", "settings": { "outputStyle": name } })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(text: &str) -> Item {
        Item::User(text.to_owned())
    }

    fn lines(line: &str) -> Value {
        serde_json::from_str(line.trim()).unwrap()
    }

    #[test]
    fn escape_stops_only_while_claude_works_and_closes_a_dialog_first() {
        assert_eq!(escape(false, true), Escape::Stop);
        assert_eq!(escape(false, false), Escape::Nothing);
        assert_eq!(escape(true, true), Escape::Close);
        assert_eq!(escape(true, false), Escape::Close);
    }

    #[test]
    fn up_goes_back_through_the_prompts_down_comes_forward_and_ends_at_the_draft() {
        let items = [said("first"), Item::Text("ok".into()), said("second"), said("second"), Item::UserImages(String::new(), vec!["p1".into()]), said("third")];
        let all = prompts(&items);
        assert_eq!(all, ["first", "second", "third"], "a repeat counts once and a picture alone says nothing");
        let mut recall = None;
        assert_eq!(older_in(&mut recall, "half typed", &all).as_deref(), Some("third"));
        assert_eq!(older_in(&mut recall, "third", &all).as_deref(), Some("second"));
        assert_eq!(older_in(&mut recall, "second", &all).as_deref(), Some("first"));
        assert_eq!(older_in(&mut recall, "first", &all), None, "there is none before the first");
        assert_eq!(newer_in(&mut recall, &all).as_deref(), Some("second"));
        assert_eq!(newer_in(&mut recall, &all).as_deref(), Some("third"));
        assert_eq!(newer_in(&mut recall, &all).as_deref(), Some("half typed"));
        assert_eq!(newer_in(&mut recall, &all), None, "Down means nothing once the draft is back");
        assert_eq!(recall, None);
    }

    #[test]
    fn up_with_nothing_sent_leaves_the_box_alone_and_a_recalled_prompt_is_not_history() {
        let mut recall = None;
        assert_eq!(older_in(&mut recall, "draft", &[]), None);
        assert_eq!(recall, None);
        // Editing what was recalled changes the box, and the prompts are what was sent: they stay as they were.
        let items = [said("run the tests")];
        let all = prompts(&items);
        assert_eq!(older_in(&mut recall, "", &all).as_deref(), Some("run the tests"));
        assert_eq!(prompts(&items), ["run the tests"]);
        assert_eq!(newer_in(&mut recall, &all).as_deref(), Some(""));
    }

    #[test]
    fn shift_tab_cycles_ask_first_accept_edits_plan_auto_and_round() {
        assert_eq!(next_mode("default"), "acceptEdits");
        assert_eq!(next_mode("acceptEdits"), "plan");
        assert_eq!(next_mode("plan"), "auto");
        assert_eq!(next_mode("auto"), "default");
        assert_eq!(next_mode("bypassPermissions"), "default");
    }

    #[test]
    fn initialize_tells_the_styles_and_fast_mode_and_a_setting_changes_each_by_one_line() {
        let mut options = Options::default();
        options.initialized(&json!({
            "fast_mode_state": "off", "fast_mode_disabled_reason": "sdk_opt_in_required",
            "output_style": "default", "available_output_styles": ["default", "Concise", "Explanatory"],
        }));
        assert_eq!((options.fast, options.fast_blocked.as_deref(), options.style.as_deref()), (false, None, Some("default")));
        assert_eq!(options.styles, ["default", "Concise", "Explanatory"]);
        let on = lines(&options.set_fast(true, 3).unwrap());
        assert_eq!(on["request_id"], "fast-3");
        assert_eq!(on["request"], json!({ "subtype": "apply_flag_settings", "settings": { "fastMode": true } }));
        assert!(options.fast);
        let off = lines(&options.set_fast(false, 4).unwrap());
        assert_eq!(off["request"], json!({ "subtype": "apply_flag_settings", "settings": { "fastMode": false } }));
        let style = lines(&options.set_style("Concise", 5).unwrap());
        assert_eq!(style["request"], json!({ "subtype": "apply_flag_settings", "settings": { "outputStyle": "Concise" } }));
        assert_eq!(options.style.as_deref(), Some("Concise"));
        assert_eq!(options.set_style("Made up", 6), None);
        assert_eq!(options.style.as_deref(), Some("Concise"));
    }

    #[test]
    fn a_reason_fast_mode_cannot_be_got_round_turns_nothing_on() {
        let mut options = Options::default();
        options.initialized(&json!({ "fast_mode_state": "off", "fast_mode_disabled_reason": "org_disabled" }));
        assert_eq!(options.fast_blocked.as_deref(), Some("org_disabled"));
        assert_eq!(options.set_fast(true, 1), None);
        assert!(!options.fast);
        options.initialized(&json!({ "fast_mode_state": "on" }));
        assert!(options.fast && options.fast_blocked.is_none());
    }
}
