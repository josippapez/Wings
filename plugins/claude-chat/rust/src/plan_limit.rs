//! Tells Claude to pause when your plan's usage nears its limit, in every Claude Code session Wings runs: a note with
//! each message you send (`UserPromptSubmit`) and with a tool call, its subagents' too (`PreToolUse`), from the limits
//! Wings keeps for each account (`wings.statusline()`). Past the limit work is billed as extra usage, which Claude Code
//! doesn't stop for.

use serde_json::Value;

/// The share of a window's limit at which Claude is told to pause, in percent.
pub const NEAR: f64 = 90.0;

/// How close a window is: near its limit, or used up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Level {
    Near,
    Over,
}

/// The Claude Code account the session `session` runs under, null being the default one: as its status line said, else as
/// the hook's `caller` session says (a chat's Claude Code never runs the status line, and Wings names the account its tab
/// runs).
pub fn account_of(statusline: &Value, session: &str, caller: &Value) -> Value {
    let reported = &statusline["sessions"][session];
    if reported.is_object() { reported["account"].clone() } else { caller["account"].clone() }
}

/// What Claude is told in a session of `account`, from the limits Wings keeps (`{ rateLimits, sessions }`), at `now`
/// (epoch seconds), with `clock` saying when a window resets: the window, how close it is, and the note. None while both
/// windows are under `NEAR`, or the numbers are of a window that has reset since.
pub fn note(statusline: &Value, account: &Value, now: f64, clock: impl Fn(f64) -> String) -> Option<(&'static str, Level, String)> {
    let limits = statusline["rateLimits"].as_array()?.iter().find(|limits| limits["account"] == *account)?;
    let windows = [("fiveHour", "5-hour"), ("sevenDay", "weekly")];
    let (key, name, used, resets) = windows
        .iter()
        .filter_map(|(key, name)| {
            let window = &limits[*key];
            let used = window["usedPercentage"].as_f64()?;
            let resets = window["resetsAt"].as_f64();
            // Numbers from before a reset say nothing of the window now.
            (used >= NEAR && resets.is_none_or(|at| at > now)).then_some((*key, *name, used, resets))
        })
        .max_by(|a, b| a.2.total_cmp(&b.2))?;
    let when = resets.map(|at| format!(" (it resets at {})", clock(at))).unwrap_or_default();
    Some(if used >= 100.0 {
        (key, Level::Over, format!(
            "Your Claude plan's {name} limit is used up{when}: anything more is billed as extra usage. Stop now: finish nothing \
             else, start no subagents or builds, tell the user where things stand and ask whether to go on."
        ))
    } else {
        (key, Level::Near, format!(
            "Your Claude plan's {name} limit is at {used:.0}%{when}. Pause: finish the step you are on, start no subagents, \
             builds or other long work, tell the user where things stand and ask whether to go on, unless they already \
             said to go on after this note."
        ))
    })
}

/// A window a chat stopped its turn for: its label in the chat's limits ("5h", "week"), how close it was, and when it
/// resets (ms).
pub type Stopped = (&'static str, Level, Option<f64>);

/// Whether a chat stops Claude's turn, as its Stop button does, from the limits Claude Code reported to it (`limits`: label,
/// percent used, reset in ms), with the setting on: the window at `NEAR` or past it it hasn't stopped for yet at that level
/// and reset, and what the chat says of it. Each stops once, so the message you send next goes on.
pub fn stop_for(limits: &[(&'static str, u64, Option<f64>)], stopped: &[Stopped]) -> Option<(Stopped, String)> {
    limits.iter().find_map(|&(label, used, resets)| {
        let level = match used as f64 {
            used if used >= 100.0 => Level::Over,
            used if used >= NEAR => Level::Near,
            _ => return None,
        };
        let stop = (label, level, resets);
        if stopped.contains(&stop) {
            return None;
        }
        let name = if label == "5h" { "5-hour" } else { "weekly" };
        let said = match level {
            Level::Over => format!("Stopped: your plan's {name} limit is used up, so more work is billed as extra usage. Send a message to go on."),
            Level::Near => format!("Stopped: your plan's {name} limit is at {used}%. Send a message to go on."),
        };
        Some((stop, said))
    })
}

#[cfg(any(feature = "main", all(test, not(feature = "chat"))))]
mod hooks {
    use std::{cell::RefCell, collections::HashMap};

    use wings_plugin::{
        hooks::{Say, on_hook},
        host, local_time,
    };

    use super::{Level, account_of, note};
    use crate::prefs;

    thread_local! {
        /// What each agent of a session (the main one, or a subagent by its id) was last told with a tool call, so a run of
        /// calls carries the note once per level, and each subagent hears it once in its own conversation.
        static TOLD: RefCell<HashMap<(String, String), (&'static str, Level)>> = RefCell::new(HashMap::new());
    }

    async fn say(input: serde_json::Value, context: serde_json::Value, tool_call: bool) -> Option<Say> {
        if !prefs::pause_near_limit().get() {
            return None;
        }
        let session = input["session_id"].as_str()?.to_owned();
        let agent = (session.clone(), input["agent_id"].as_str().unwrap_or_default().to_owned());
        let heard = host::statusline().await.ok()?;
        let clock = |at: f64| {
            let local = local_time::local((at * 1000.0) as i64);
            format!("{:02}:{:02}", local.hour, local.minute)
        };
        let account = account_of(&heard, &session, &context["session"]);
        let Some((window, level, text)) = note(&heard, &account, host::now() / 1000.0, clock) else {
            TOLD.with(|told| told.borrow_mut().remove(&agent));
            return None;
        };
        if tool_call && TOLD.with(|told| told.borrow_mut().insert(agent, (window, level)) == Some((window, level))) {
            return None;
        }
        Some(Say::Context(text))
    }

    pub fn listen() {
        on_hook("UserPromptSubmit", |input, context| say(input, context, false));
        on_hook("PreToolUse", |input, context| say(input, context, true));
    }
}

#[cfg(any(feature = "main", all(test, not(feature = "chat"))))]
pub use hooks::listen;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn heard(five: f64, week: f64) -> Value {
        json!({
            "rateLimits": [
                { "account": null, "fiveHour": { "usedPercentage": 10, "resetsAt": 2000 }, "sevenDay": { "usedPercentage": 10, "resetsAt": 9000 } },
                { "account": "work", "fiveHour": { "usedPercentage": five, "resetsAt": 2000 }, "sevenDay": { "usedPercentage": week, "resetsAt": 9000 } },
            ],
            "sessions": { "s1": { "account": "work" }, "s2": {} },
        })
    }

    fn clock(at: f64) -> String {
        format!("t{at}")
    }

    fn work() -> Value {
        json!("work")
    }

    #[test]
    fn under_the_limit_says_nothing_and_near_it_says_to_pause_with_when_it_resets() {
        assert_eq!(note(&heard(50.0, 80.0), &work(), 1000.0, clock), None);
        let (window, level, text) = note(&heard(92.4, 80.0), &work(), 1000.0, clock).unwrap();
        assert_eq!((window, level), ("fiveHour", Level::Near));
        assert!(text.contains("5-hour limit is at 92%") && text.contains("resets at t2000") && text.contains("Pause"), "{text}");
    }

    #[test]
    fn a_used_up_window_says_to_stop_and_the_closer_window_is_the_one_named() {
        let (window, level, text) = note(&heard(91.0, 100.0), &work(), 1000.0, clock).unwrap();
        assert_eq!((window, level), ("sevenDay", Level::Over));
        assert!(text.contains("weekly limit is used up") && text.contains("extra usage") && text.contains("Stop now"), "{text}");
    }

    #[test]
    fn each_session_reads_its_own_accounts_limits_and_a_reset_window_is_not_near() {
        // `s2` runs under the default account, whose limits are low.
        assert_eq!(note(&heard(99.0, 99.0), &account_of(&heard(99.0, 99.0), "s2", &Value::Null), 1000.0, clock), None);
        // Past its reset time the 5-hour window's numbers are old; the weekly one still counts.
        assert_eq!(note(&heard(99.0, 50.0), &work(), 3000.0, clock), None);
        assert_eq!(note(&json!({}), &work(), 1000.0, clock), None);
    }

    #[test]
    fn a_chat_stops_once_as_a_window_passes_ninety_percent_and_once_more_when_it_is_used_up() {
        let mut stopped = Vec::new();
        assert_eq!(stop_for(&[("5h", 89, Some(5.0)), ("week", 40, Some(9.0))], &stopped), None);
        let (stop, said) = stop_for(&[("5h", 92, Some(5.0)), ("week", 40, Some(9.0))], &stopped).unwrap();
        assert_eq!(stop, ("5h", Level::Near, Some(5.0)));
        assert_eq!(said, "Stopped: your plan's 5-hour limit is at 92%. Send a message to go on.");
        stopped.push(stop);
        // The message you send next goes on.
        assert_eq!(stop_for(&[("5h", 95, Some(5.0))], &stopped), None);
        let (stop, said) = stop_for(&[("5h", 100, Some(5.0))], &stopped).unwrap();
        assert_eq!(stop.1, Level::Over);
        assert!(said.contains("used up") && said.contains("extra usage"), "{said}");
        // A window that reset is a new one.
        assert_eq!(stop_for(&[("week", 91, Some(20.0))], &[("week", Level::Near, Some(9.0))]).map(|(stop, _)| stop), Some(("week", Level::Near, Some(20.0))));
    }

    /// A chat's Claude Code never runs the status line, so its session isn't among the status line's: before, it read the
    /// default account's limits whatever account its tab ran.
    #[test]
    fn a_chat_session_reads_the_limits_of_the_account_its_tab_runs() {
        let heard = heard(95.0, 50.0);
        let chat = json!({ "id": "c1", "account": "work" });
        assert_eq!(account_of(&heard, "c1", &chat), "work");
        assert_eq!(note(&heard, &account_of(&heard, "c1", &chat), 1000.0, clock).map(|(_, level, _)| level), Some(Level::Near));
        // One the status line knows goes by what it said.
        assert_eq!(account_of(&heard, "s1", &json!({ "account": "other" })), "work");
        // One nobody names runs under the default account.
        assert_eq!(account_of(&heard, "c2", &json!({ "id": "c2" })), Value::Null);
    }
}
