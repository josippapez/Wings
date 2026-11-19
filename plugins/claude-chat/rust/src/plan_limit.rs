//! Tells Claude to pause when your plan's usage nears its limit, in every Claude Code session Wings runs: a note with
//! each message you send (`UserPromptSubmit`) and before a command it runs (`PreToolUse`), from the limits Claude Code
//! reports to Wings' status line. Past the limit work is billed as extra usage, which Claude Code doesn't stop for.

use serde_json::Value;

/// The share of a window's limit at which Claude is told to pause, in percent.
pub const NEAR: f64 = 90.0;

/// How close a window is: near its limit, or used up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Level {
    Near,
    Over,
}

/// What Claude is told for the session `session`, from what the status line last heard (`{ rateLimits, sessions }`), at
/// `now` (epoch seconds), with `clock` saying when a window resets: the window, how close it is, and the note. None while
/// both windows are under `NEAR`, or the numbers are of a window that has reset since.
pub fn note(statusline: &Value, session: &str, now: f64, clock: impl Fn(f64) -> String) -> Option<(&'static str, Level, String)> {
    let account = statusline["sessions"][session]["account"].clone();
    let limits = statusline["rateLimits"].as_array()?.iter().find(|limits| limits["account"] == account)?;
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

#[cfg(any(feature = "main", all(test, not(feature = "chat"))))]
mod hooks {
    use std::{cell::RefCell, collections::HashMap};

    use wings_plugin::{
        hooks::{Say, on_hook},
        host, local_time,
    };

    use super::{Level, note};
    use crate::prefs;

    thread_local! {
        /// What each session was last told before a command, so a run of commands carries the note once per level.
        static TOLD: RefCell<HashMap<String, (&'static str, Level)>> = RefCell::new(HashMap::new());
    }

    async fn say(input: serde_json::Value, before_command: bool) -> Option<Say> {
        if !prefs::pause_near_limit().get() {
            return None;
        }
        let session = input["session_id"].as_str()?.to_owned();
        let heard = host::statusline().await.ok()?;
        let clock = |at: f64| {
            let local = local_time::local((at * 1000.0) as i64);
            format!("{:02}:{:02}", local.hour, local.minute)
        };
        let Some((window, level, text)) = note(&heard, &session, host::now() / 1000.0, clock) else {
            TOLD.with(|told| told.borrow_mut().remove(&session));
            return None;
        };
        if before_command && TOLD.with(|told| told.borrow_mut().insert(session, (window, level)) == Some((window, level))) {
            return None;
        }
        Some(Say::Context(text))
    }

    pub fn listen() {
        on_hook("UserPromptSubmit", |input, _| say(input, false));
        on_hook("PreToolUse", |input, _| say(input, true));
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

    #[test]
    fn under_the_limit_says_nothing_and_near_it_says_to_pause_with_when_it_resets() {
        assert_eq!(note(&heard(50.0, 80.0), "s1", 1000.0, clock), None);
        let (window, level, text) = note(&heard(92.4, 80.0), "s1", 1000.0, clock).unwrap();
        assert_eq!((window, level), ("fiveHour", Level::Near));
        assert!(text.contains("5-hour limit is at 92%") && text.contains("resets at t2000") && text.contains("Pause"), "{text}");
    }

    #[test]
    fn a_used_up_window_says_to_stop_and_the_closer_window_is_the_one_named() {
        let (window, level, text) = note(&heard(91.0, 100.0), "s1", 1000.0, clock).unwrap();
        assert_eq!((window, level), ("sevenDay", Level::Over));
        assert!(text.contains("weekly limit is used up") && text.contains("extra usage") && text.contains("Stop now"), "{text}");
    }

    #[test]
    fn each_session_reads_its_own_accounts_limits_and_a_reset_window_is_not_near() {
        // `s2` runs under the default account, whose limits are low.
        assert_eq!(note(&heard(99.0, 99.0), "s2", 1000.0, clock), None);
        // Past its reset time the 5-hour window's numbers are old; the weekly one still counts.
        assert_eq!(note(&heard(99.0, 50.0), "s1", 3000.0, clock), None);
        assert_eq!(note(&json!({}), "s1", 1000.0, clock), None);
    }
}
