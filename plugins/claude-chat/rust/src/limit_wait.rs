//! Waiting out a plan's usage limit, then going on. When a turn ends because the 5-hour or the weekly limit was hit, the
//! chat shows a card with when it resets and a button to continue then: one sleep until the reset, then one message, as
//! Claude Code's terminal does with its `autoContinueAtUsageLimit` ("Continuing automatically at 18:00 · Esc to cancel").
//!
//! Shapes are from Claude Code 2.1.328:
//! - `rate_limit_event`: `rate_limit_info: { status: "rejected", resetsAt: <epoch seconds>, rateLimitType: "five_hour",
//!   unifiedWindows: { five_hour: { utilization, resetsAt, status } } }`. `status` is `rejected` once the window is used
//!   up, `allowed_warning` before.
//! - The turn's `result` has `is_error: true` and the limit as its `result` text: "You've hit your 5-hour limit · resets
//!   8:30pm (Europe/Zagreb)". The terminal checks its replies for the starts "You've hit your" and "You've reached your".
//! - What the terminal sends at the reset is its message `H`: [`CONTINUE`], word for word. It arms only when the reset is
//!   at most a day away ("the usage limit now resets more than 24 hours out, so this task will not resume on its own"), and
//!   a message of the person's own cancels it ("Automatic continue cancelled").

use serde_json::Value;
use wings_plugin::local_time::local;

/// What Claude Code's terminal sends when a usage limit has reset.
pub const CONTINUE: &str = "Your claude.ai usage limit has reset. Continue the task you were working on when the limit was reached; do not repeat work that is already complete.";

/// The longest wait the chat arms, as the terminal's.
const WAIT_MOST_MS: f64 = 24.0 * 3_600_000.0;
/// Slept past the reset, so the limit is over when the message goes.
const AFTER_MS: f64 = 2_000.0;

/// The limit that was hit: its window ("5-hour", "weekly") and when it resets, in ms.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hit {
    pub window: Option<String>,
    pub resets_at: Option<f64>,
}

/// Why the armed continue did not send.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Skipped {
    /// There was something in the message box.
    Typing,
    /// Claude was busy with something of its own.
    Busy,
}

#[derive(Debug, Default)]
pub struct LimitWait {
    pub hit: Option<Hit>,
    /// The reset (ms) the chat waits for, once you armed it.
    pub armed: Option<f64>,
    /// Counts the arms, so a sleep that outlived its arm knows.
    pub generation: u32,
    pub skipped: Option<Skipped>,
}

/// The name a limit's window is called by.
fn window_name(key: &str) -> String {
    match key {
        "five_hour" => "5-hour".to_owned(),
        "seven_day" => "weekly".to_owned(),
        other => other.replace('_', " "),
    }
}

/// The window in "You've hit your 5-hour limit · resets 8:30pm": what stands between "hit your" and " limit".
fn window_in(said: &str) -> Option<String> {
    let rest = said.strip_prefix("You've hit your ").or_else(|| said.strip_prefix("You've reached your "))?;
    let name = rest.split(" limit").next().filter(|_| rest.contains(" limit"))?.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// Whether `said` is Claude Code's line for a hit limit.
pub fn is_limit_text(said: &str) -> bool {
    let said = said.trim_start();
    said.starts_with("You've hit your") || said.starts_with("You've reached your")
}

impl LimitWait {
    /// Takes in a line Claude Code printed: a window gone `rejected`, or a turn that ended on a limit. A turn that
    /// ended well says the limit is over.
    pub fn heard(&mut self, line: &Value) {
        match line["type"].as_str().unwrap_or_default() {
            "rate_limit_event" => {
                let info = &line["rate_limit_info"];
                if info["status"] != "rejected" {
                    return;
                }
                let key = info["rateLimitType"].as_str();
                let resets = info["resetsAt"].as_f64().or_else(|| key.and_then(|key| info["unifiedWindows"][key]["resetsAt"].as_f64()));
                let window = key.map(window_name).or_else(|| self.hit.as_ref().and_then(|hit| hit.window.clone()));
                self.hit = Some(Hit { window, resets_at: resets.map(|at| at * 1000.0) });
                self.skipped = None;
            }
            "result" => {
                let said = line["result"].as_str().filter(|_| line["is_error"] == true).filter(|said| is_limit_text(said));
                match said {
                    Some(said) => {
                        let window = window_in(said.trim_start());
                        let known = self.hit.take().unwrap_or_default();
                        self.hit = Some(Hit { window: known.window.or(window), resets_at: known.resets_at });
                    }
                    None if line["is_error"] != true && self.armed.is_none() => self.clear(),
                    None => {}
                }
            }
            _ => {}
        }
    }

    /// Drops the card and the wait.
    pub fn clear(&mut self) {
        self.hit = None;
        self.skipped = None;
        if self.armed.take().is_some() {
            self.generation += 1;
        }
    }

    /// A message of yours went to Claude Code: the wait is over, as the terminal's is.
    pub fn user_spoke(&mut self) {
        self.clear();
    }

    /// When the hit limit resets (ms): what Claude Code said, else the limits the chat shows (`("5h", percent, resets)`)
    /// for a window used up.
    pub fn resets_at(&self, limits: &[(&'static str, u64, Option<f64>)]) -> Option<f64> {
        let hit = self.hit.as_ref()?;
        hit.resets_at.or_else(|| {
            let label = match hit.window.as_deref()? {
                "5-hour" | "session" => "5h",
                "weekly" | "7-day" => "week",
                _ => return None,
            };
            limits.iter().find(|(shown, used, _)| *shown == label && *used >= 100).and_then(|(.., resets)| *resets)
        })
    }

    /// The reset (ms) the tab keeps for when the chat starts again, armed or not: Claude Code's transcript has no line that
    /// says a limit was hit, so a restarted chat knows of it only from this.
    pub fn kept(&self, limits: &[(&'static str, u64, Option<f64>)]) -> Option<f64> {
        self.armed.or_else(|| self.resets_at(limits))
    }

    /// Arms the continue at the reset `resets` (ms), at `now`: the number of this arm and how long to sleep (ms). None
    /// when there is no limit hit, the reset has passed, or it is more than a day away.
    pub fn arm(&mut self, resets: f64, now: f64) -> Option<(u32, u32)> {
        if self.hit.is_none() || resets <= now || resets - now > WAIT_MOST_MS {
            return None;
        }
        self.hit.as_mut()?.resets_at = Some(resets);
        self.armed = Some(resets);
        self.skipped = None;
        self.generation += 1;
        Some((self.generation, (resets - now + AFTER_MS) as u32))
    }

    /// The card for a reset the tab kept across a restart of Wings, not armed: the card knows the window no more. None
    /// for a reset more than a day gone.
    pub fn offer(&mut self, resets: f64, now: f64) {
        if self.hit.is_none() && resets + WAIT_MOST_MS > now {
            self.hit = Some(Hit { window: None, resets_at: Some(resets) });
        }
    }

    /// "Not now": no more wait, and no card.
    pub fn dismiss(&mut self) {
        self.clear();
    }

    /// Takes the arm back and keeps the card.
    pub fn unarm(&mut self) {
        if self.armed.take().is_some() {
            self.generation += 1;
        }
    }

    /// The sleep of arm `generation` ended: the message to send, if it is still to go. Not when the arm was taken back or
    /// you sent something meanwhile (both drop `armed`), the chat isn't open, something is in the message box, or Claude
    /// is busy.
    pub fn fire(&mut self, generation: u32, open: bool, idle: bool, draft_empty: bool) -> Option<&'static str> {
        if generation != self.generation || self.armed.take().is_none() {
            return None;
        }
        if !open {
            return None;
        }
        if !draft_empty {
            self.skipped = Some(Skipped::Typing);
            return None;
        }
        if !idle {
            self.skipped = Some(Skipped::Busy);
            return None;
        }
        self.hit = None;
        Some(CONTINUE)
    }
}

/// What the card says at `now`.
#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    /// Resets within a day: the button arms.
    Offer,
    /// Armed: it goes at the reset.
    Armed,
    /// It has reset, and nothing was sent.
    Passed,
    /// Resets more than a day away.
    Far,
    /// No reset time known, as for a monthly spend limit.
    Unknown,
    Skipped(Skipped),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub window: Option<String>,
    pub resets_at: Option<f64>,
    pub phase: Phase,
}

impl LimitWait {
    /// The card to show at `now` (ms), when a limit was hit.
    pub fn card(&self, now: f64, limits: &[(&'static str, u64, Option<f64>)]) -> Option<Card> {
        let hit = self.hit.as_ref()?;
        let resets_at = self.resets_at(limits);
        let phase = match (self.armed, self.skipped, resets_at) {
            (Some(_), ..) => Phase::Armed,
            (None, Some(skipped), _) => Phase::Skipped(skipped),
            (None, None, Some(at)) if at <= now => Phase::Passed,
            (None, None, Some(at)) if at - now > WAIT_MOST_MS => Phase::Far,
            (None, None, Some(_)) => Phase::Offer,
            (None, None, None) => Phase::Unknown,
        };
        Some(Card { window: hit.window.clone(), resets_at, phase })
    }
}

/// A moment as the card names it: "18:00", with the weekday when it isn't today.
pub fn at_text(at: f64, now: f64) -> String {
    let (when, today) = (local(at as i64), local(now as i64));
    let time = format!("{:02}:{:02}", when.hour, when.minute);
    if (when.year, when.month, when.day) == (today.year, today.month, today.day) {
        return time;
    }
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    format!("{} {time}", DAYS[when.weekday as usize % 7])
}

/// How long until `at`: "2h 14m", "14m", "under a minute".
pub fn left_text(at: f64, now: f64) -> String {
    if at - now < 60_000.0 {
        return "under a minute".to_owned();
    }
    let minutes = ((at - now) / 60_000.0).ceil() as u64;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

/// The card's words: the headline, and the line under it for what happens next.
pub fn lines(card: &Card, now: f64) -> (String, Option<String>) {
    let limit = match &card.window {
        Some(window) => format!("You've hit your {window} limit."),
        None => "You've hit your usage limit.".to_owned(),
    };
    let resets = |at: f64| format!("It resets at {} (in {}).", at_text(at, now), left_text(at, now));
    let (head, small) = match (&card.phase, card.resets_at) {
        (Phase::Passed, _) => (format!("{limit} It has reset."), None),
        (Phase::Far, Some(at)) => (format!("{limit} It resets at {}.", at_text(at, now)), Some("That is more than a day away, so Wings won't wait for it.".to_owned())),
        (Phase::Skipped(why), at) => {
            let small = match why {
                Skipped::Typing => "It has reset. You had started a message, so nothing was sent.",
                Skipped::Busy => "It has reset. Claude was busy, so nothing was sent.",
            };
            (at.map_or(limit.clone(), |at| format!("{limit} It reset at {}.", at_text(at, now))), Some(small.to_owned()))
        }
        (Phase::Armed, Some(at)) => (format!("{limit} {}", resets(at)), Some("Continues by itself then, if this chat is open and nothing has been typed. Send a message to cancel.".to_owned())),
        (_, Some(at)) => (format!("{limit} {}", resets(at)), None),
        (_, None) => (limit, None),
    };
    (head, small)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wings_plugin::local_time;

    use super::*;

    const NOW: f64 = 1_795_000_000_000.0;
    const RESET: f64 = NOW + 2.0 * 3_600_000.0 + 14.0 * 60_000.0 - 30_000.0;

    fn rejected(at_seconds: f64) -> Value {
        json!({ "type": "rate_limit_event", "rate_limit_info": { "status": "rejected", "resetsAt": at_seconds, "rateLimitType": "five_hour",
            "unifiedWindows": { "five_hour": { "utilization": 1, "resetsAt": at_seconds, "status": "rejected" } } } })
    }

    fn limit_result(said: &str) -> Value {
        json!({ "type": "result", "subtype": "success", "is_error": true, "result": said })
    }

    #[test]
    fn a_rejected_window_makes_the_card_with_its_name_and_reset() {
        let mut wait = LimitWait::default();
        wait.heard(&json!({ "type": "rate_limit_event", "rate_limit_info": { "status": "allowed_warning", "resetsAt": 1, "rateLimitType": "seven_day" } }));
        assert_eq!(wait.card(NOW, &[]), None);
        wait.heard(&rejected(RESET / 1000.0));
        let card = wait.card(NOW, &[]).unwrap();
        assert_eq!((card.window.as_deref(), card.resets_at, &card.phase), (Some("5-hour"), Some(RESET), &Phase::Offer));
    }

    #[test]
    fn a_limit_result_makes_the_card_and_takes_the_reset_from_the_limits_the_chat_shows() {
        let mut wait = LimitWait::default();
        wait.heard(&limit_result("You've hit your 5-hour limit · resets 8:30pm (Europe/Zagreb)"));
        let limits = [("5h", 100, Some(RESET)), ("week", 40, Some(RESET * 2.0))];
        let card = wait.card(NOW, &limits).unwrap();
        assert_eq!((card.window.as_deref(), card.resets_at, &card.phase), (Some("5-hour"), Some(RESET), &Phase::Offer));
        // Without a reset to go by, nothing can be armed.
        let card = wait.card(NOW, &[("5h", 80, Some(RESET))]).unwrap();
        assert_eq!((card.resets_at, card.phase), (None, Phase::Unknown));
        assert_eq!(wait.arm(RESET, NOW).map(|(_, ms)| ms), Some((RESET - NOW + AFTER_MS) as u32));
    }

    #[test]
    fn another_error_or_a_good_turn_is_not_a_limit_and_a_good_turn_ends_the_card() {
        let mut wait = LimitWait::default();
        wait.heard(&limit_result("Overloaded"));
        assert_eq!(wait.card(NOW, &[]), None);
        wait.heard(&limit_result("You've hit your monthly spend limit."));
        assert_eq!(wait.card(NOW, &[]).unwrap().window.as_deref(), Some("monthly spend"));
        wait.heard(&json!({ "type": "result", "subtype": "success", "is_error": false, "result": "done" }));
        assert_eq!(wait.card(NOW, &[]), None);
    }

    #[test]
    fn a_reset_that_is_over_a_day_away_or_past_is_not_armed() {
        let mut wait = LimitWait::default();
        wait.heard(&rejected((NOW + 25.0 * 3_600_000.0) / 1000.0));
        assert_eq!(wait.card(NOW, &[]).unwrap().phase, Phase::Far);
        assert_eq!(wait.arm(NOW + 25.0 * 3_600_000.0, NOW), None);
        assert_eq!(wait.arm(NOW - 1.0, NOW), None);
        assert_eq!(wait.armed, None);
        wait.heard(&rejected((NOW - 5_000.0) / 1000.0));
        assert_eq!(wait.card(NOW, &[]).unwrap().phase, Phase::Passed);
    }

    fn armed() -> (LimitWait, u32) {
        let mut wait = LimitWait::default();
        wait.heard(&rejected(RESET / 1000.0));
        let (generation, _) = wait.arm(RESET, NOW).unwrap();
        (wait, generation)
    }

    #[test]
    fn the_arm_sends_exactly_one_continue_at_the_reset() {
        let (mut wait, generation) = armed();
        assert_eq!(wait.card(NOW, &[]).unwrap().phase, Phase::Armed);
        assert_eq!(wait.fire(generation, true, true, true), Some(CONTINUE));
        assert_eq!(wait.fire(generation, true, true, true), None, "a second sleep ending sends nothing");
        assert_eq!(wait.card(NOW, &[]), None);
    }

    #[test]
    fn a_message_you_sent_meanwhile_or_not_now_or_a_newer_arm_stops_the_old_sleep() {
        let (mut wait, generation) = armed();
        wait.user_spoke();
        assert_eq!(wait.fire(generation, true, true, true), None);
        let (mut wait, generation) = armed();
        wait.dismiss();
        assert_eq!(wait.fire(generation, true, true, true), None);
        assert_eq!(wait.card(NOW, &[]), None);
        // Armed again: only the newest sleep sends.
        let (mut wait, first) = armed();
        let (second, _) = wait.arm(RESET, NOW).unwrap();
        assert_eq!(wait.fire(first, true, true, true), None);
        assert_eq!(wait.fire(second, true, true, true), Some(CONTINUE));
    }

    #[test]
    fn nothing_is_sent_when_the_chat_closed_you_typed_or_claude_is_busy() {
        let (mut wait, generation) = armed();
        assert_eq!(wait.fire(generation, false, true, true), None);
        let (mut wait, generation) = armed();
        assert_eq!(wait.fire(generation, true, true, false), None);
        assert_eq!(wait.card(RESET + 1.0, &[]).unwrap().phase, Phase::Skipped(Skipped::Typing));
        let (mut wait, generation) = armed();
        assert_eq!(wait.fire(generation, true, false, true), None);
        assert_eq!(wait.card(RESET + 1.0, &[]).unwrap().phase, Phase::Skipped(Skipped::Busy));
    }

    #[test]
    fn a_reset_kept_across_a_restart_offers_the_button_again_unarmed() {
        let mut wait = LimitWait::default();
        wait.offer(RESET, NOW);
        let card = wait.card(NOW, &[]).unwrap();
        assert_eq!((card.window, card.phase, wait.armed), (None, Phase::Offer, None));
        // Reset while Wings was closed: "Continue" now.
        let mut wait = LimitWait::default();
        wait.offer(NOW - 1.0, NOW);
        assert_eq!(wait.card(NOW, &[]).unwrap().phase, Phase::Passed);
        // Long gone: no card.
        let mut wait = LimitWait::default();
        wait.offer(NOW - WAIT_MOST_MS - 1.0, NOW);
        assert_eq!(wait.card(NOW, &[]), None);
    }

    #[test]
    fn the_card_says_the_window_the_reset_and_how_long_is_left() {
        local_time::testing::zone(|_| 60);
        // NOW is 2026-11-18T11:06:40Z, 12:06 in the zone an hour ahead.
        let card = |phase: Phase, resets_at: Option<f64>| Card { window: Some("5-hour".into()), resets_at, phase };
        let (head, small) = lines(&card(Phase::Offer, Some(RESET)), NOW);
        assert_eq!(head, "You've hit your 5-hour limit. It resets at 14:20 (in 2h 14m).");
        assert_eq!(small, None);
        let (head, small) = lines(&card(Phase::Armed, Some(RESET)), NOW);
        assert!(head.contains("It resets at 14:20") && small.unwrap().contains("Send a message to cancel"));
        // Another day names its weekday.
        assert_eq!(at_text(NOW + 20.0 * 3_600_000.0, NOW), "Thu 08:06");
        assert_eq!(left_text(NOW + 10_000.0, NOW), "under a minute");
        assert_eq!(left_text(NOW + 3.0 * 3_600_000.0, NOW), "3h");
    }
}
