//! What Claude Code tells its status line, passed on by `wings statusline` (cli.rs) once you make that your
//! `statusLine` command. Plugins with `permissions.statusline` read it with `wings.statusline()`: your usage
//! limits and each session's context window and prompt cache, as Claude Code reports them, rather than worked
//! out from transcripts. Only these fields are kept, checked and bounded, since any process of yours can
//! write to the socket. Each Claude Code account has limits of its own, so they are kept per account.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::Serialize;
use serde_json::Value;

/// Sessions kept. Older ones have long since stopped.
const SESSIONS_MAX: usize = 32;
const MODEL_MAX: usize = 80;
const ACCOUNTS_MAX: usize = 16;

/// A usage limit window, like the 5-hour one.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    /// 0 to 100.
    pub used_percentage: f64,
    /// Unix epoch seconds.
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    /// The Claude Code account, `None` being the default one.
    pub account: Option<String>,
    /// Who is signed in to it, when known. Two accounts signed in to one login have one set of limits.
    #[serde(skip)]
    login: Option<String>,
    pub five_hour: Option<Window>,
    pub seven_day: Option<Window>,
    /// When Wings got them, in ms.
    pub at: u64,
}

/// Token counts from the session's last API response.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cache {
    pub warm: bool,
    /// `5m` or `1h`.
    pub ttl: Option<String>,
    /// Unix epoch seconds.
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    /// The Claude Code account it runs as, `None` being the default one.
    pub account: Option<String>,
    /// Like `Opus 5.5 (1M context)`.
    pub model: Option<String>,
    pub context_window_size: Option<u64>,
    pub used_percentage: Option<f64>,
    pub total_input_tokens: Option<u64>,
    pub total_output_tokens: Option<u64>,
    pub current_usage: Option<Usage>,
    pub prompt_cache: Option<Cache>,
    /// When Wings got it, in ms.
    pub at: u64,
    /// The limits it last sent, to tell a status that repeats them from one with news.
    #[serde(skip)]
    limits: (Option<Window>, Option<Window>),
}

#[derive(Debug, Default)]
pub struct Statusline {
    /// Each account's newest. A status without them keeps the last ones.
    limits: Vec<Limits>,
    sessions: HashMap<String, Session>,
}

fn number(v: &Value) -> Option<f64> {
    v.as_f64().filter(|n| n.is_finite())
}

fn count(v: &Value) -> Option<u64> {
    number(v).filter(|n| *n >= 0.0).map(|n| n as u64)
}

fn epoch(v: &Value) -> Option<i64> {
    number(v).filter(|n| *n > 0.0).map(|n| n as i64)
}

fn percent(v: &Value) -> Option<f64> {
    number(v).map(|n| n.clamp(0.0, 100.0))
}

fn window(v: &Value) -> Option<Window> {
    Some(Window { used_percentage: percent(&v["used_percentage"])?, resets_at: epoch(&v["resets_at"]) })
}

/// Same rule as transcripts: Claude Code's session ids are UUIDs.
fn session_id(v: &Value) -> Option<&str> {
    v.as_str().filter(|id| id.len() == 36 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'))
}

/// Reset times this close are one period's. The usage endpoint (`usage.rs`) gives them to a fraction of a second and a
/// status line in whole seconds, and periods start hours apart.
const SAME_PERIOD: i64 = 60;

/// Of a window already kept and one a status line reports later, the one with newer data. A session's limits are
/// from its own last response, so one that has been idle, or has just started, reports numbers that were true
/// when it last heard, whatever the order the reports come in. The window's `resets_at` says which period it is:
/// a later one is a newer period (the usage was reset), an earlier one is an old period that already ended. Within
/// one period usage only goes up, so the lower number is the one that is out of date, and the reset time stays the
/// one kept: the HUD alerts again when it changes. A report that has no window (Claude Code drops one once it resets)
/// or no `resets_at` to compare is taken as it is.
fn newer(kept: &Option<Window>, reported: &Option<Window>) -> Option<Window> {
    match (kept, reported) {
        (Some(kept), Some(reported)) => match (kept.resets_at, reported.resets_at) {
            (Some(k), Some(r)) if (r - k).abs() <= SAME_PERIOD => {
                Some(Window { used_percentage: kept.used_percentage.max(reported.used_percentage), resets_at: Some(k) })
            }
            (Some(k), Some(r)) if r < k => Some(kept.clone()),
            _ => Some(reported.clone()),
        },
        _ => reported.clone(),
    }
}

impl Statusline {
    /// Keeps what one status line run of `account` reported, at `now` in ms. `login` is who is signed in to that
    /// account, when known.
    pub fn record(&mut self, status: &Value, account: &Value, login: Option<&str>, now: u64) {
        let account = account.as_str().filter(|name| wings_spaces::claude_accounts::valid_name(name)).map(str::to_string);
        let limits = &status["rate_limits"];
        let (five_hour, seven_day) = (window(&limits["five_hour"]), window(&limits["seven_day"]));
        let id = session_id(&status["session_id"]);
        let context = &status["context_window"];
        let usage = &context["current_usage"];
        let current_usage = usage.is_object().then(|| Usage {
            input_tokens: count(&usage["input_tokens"]).unwrap_or(0),
            output_tokens: count(&usage["output_tokens"]).unwrap_or(0),
            cache_creation_input_tokens: count(&usage["cache_creation_input_tokens"]).unwrap_or(0),
            cache_read_input_tokens: count(&usage["cache_read_input_tokens"]).unwrap_or(0),
        });
        let (total_input_tokens, total_output_tokens) = (count(&context["total_input_tokens"]), count(&context["total_output_tokens"]));
        // A session's limits are from its own last response, and its status line can run on a timer, so an idle
        // session sends the same old numbers again and again. One that sends what it sent before, with no tokens
        // used since, has no news: taking it would put old limits over a working session's new ones.
        let repeats = id.and_then(|id| self.sessions.get(id)).is_some_and(|last| {
            last.limits == (five_hour.clone(), seven_day.clone())
                && (last.total_input_tokens, last.total_output_tokens) == (total_input_tokens, total_output_tokens)
                && last.current_usage == current_usage
        });
        // Absent before a session's first response, so that says nothing about the account.
        if (five_hour.is_some() || seven_day.is_some()) && !repeats {
            // Whichever session reports, what is kept is the newest data of each window, not the last report.
            let kept = self.limits.iter().find(|l| l.account == account);
            let five_hour = kept.map_or(five_hour.clone(), |k| newer(&k.five_hour, &five_hour));
            let seven_day = kept.map_or(seven_day.clone(), |k| newer(&k.seven_day, &seven_day));
            self.limits.retain(|l| l.account != account);
            // Newest first. Names are checked, and there are only as many as you made.
            self.limits.insert(0, Limits { account: account.clone(), login: login.map(str::to_string), five_hour, seven_day, at: now });
            self.limits.truncate(ACCOUNTS_MAX);
        }
        let Some(id) = id else { return };
        let cache = &status["prompt_cache"];
        let prompt_cache = cache.is_object().then(|| Cache {
            warm: cache["warm"].as_bool().unwrap_or(false),
            ttl: cache["ttl"].as_str().filter(|t| ["5m", "1h"].contains(t)).map(str::to_string),
            expires_at: epoch(&cache["expires_at"]),
        });
        let model = status["model"]["display_name"].as_str().map(|m| m.chars().filter(|c| !c.is_control()).take(MODEL_MAX).collect());
        let session = Session {
            account,
            model,
            context_window_size: count(&context["context_window_size"]),
            used_percentage: percent(&context["used_percentage"]),
            total_input_tokens,
            total_output_tokens,
            current_usage,
            prompt_cache,
            at: now,
            limits: (five_hour, seven_day),
        };
        self.sessions.insert(id.to_string(), session);
        if self.sessions.len() > SESSIONS_MAX {
            let oldest = self.sessions.iter().min_by_key(|(_, s)| s.at).map(|(id, _)| id.clone());
            self.sessions.remove(&oldest.unwrap_or_default());
        }
    }

    /// Keeps the limits of `account` that Wings read itself (`usage.rs`), at `now` in ms, by the same newest-data rule
    /// as a status line's. A window the answer didn't have leaves the one kept. Unless something changed, the kept
    /// limits stay as they are, `at` too, so the HUD has nothing to redraw.
    pub fn record_usage(&mut self, account: Option<&str>, login: Option<&str>, five_hour: Option<Window>, seven_day: Option<Window>, now: u64) {
        let account = account.filter(|name| wings_spaces::claude_accounts::valid_name(name)).map(str::to_string);
        let fetched = |kept: &Option<Window>, fetched: Option<Window>| if fetched.is_some() { newer(kept, &fetched) } else { kept.clone() };
        match self.limits.iter_mut().find(|l| l.account == account) {
            Some(kept) => {
                let (five_hour, seven_day) = (fetched(&kept.five_hour, five_hour), fetched(&kept.seven_day, seven_day));
                if (&five_hour, &seven_day) != (&kept.five_hour, &kept.seven_day) {
                    (kept.five_hour, kept.seven_day, kept.at) = (five_hour, seven_day, now);
                }
            }
            None if five_hour.is_some() || seven_day.is_some() => {
                self.limits.insert(0, Limits { account, login: login.map(str::to_string), five_hour, seven_day, at: now });
                self.limits.truncate(ACCOUNTS_MAX);
            }
            None => {}
        }
    }

    /// The ids of the sessions that reported, by account.
    pub fn sessions_by_account(&self) -> BTreeMap<Option<String>, BTreeSet<String>> {
        let mut accounts: BTreeMap<Option<String>, BTreeSet<String>> = BTreeMap::new();
        for (id, session) in &self.sessions {
            accounts.entry(session.account.clone()).or_default().insert(id.clone());
        }
        accounts
    }

    /// What `wings.statusline()` resolves.
    pub fn view(&self) -> Value {
        // An account signed in to the same login as one that reported later has that one's limits: they are one
        // login's limits, and an account whose sessions report none would keep showing old ones.
        let limits: Vec<Limits> = self
            .limits
            .iter()
            .map(|own| {
                let Some(login) = own.login.as_ref() else { return own.clone() };
                // Oldest report first, so each window ends up as the newest data of any account on the login.
                let mut shared = self.limits.iter().rev().filter(|other| other.login.as_ref() == Some(login));
                let first = shared.next().unwrap_or(own);
                let (mut five_hour, mut seven_day, mut at) = (first.five_hour.clone(), first.seven_day.clone(), first.at);
                for other in shared {
                    (five_hour, seven_day, at) = (newer(&five_hour, &other.five_hour), newer(&seven_day, &other.seven_day), other.at);
                }
                Limits { account: own.account.clone(), login: own.login.clone(), five_hour, seven_day, at }
            })
            .collect();
        serde_json::json!({ "rateLimits": limits, "sessions": self.sessions })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ID: &str = "3c21b0c5-8113-4f55-9493-fa156c3fa369";

    /// The example from code.claude.com/docs/en/statusline, trimmed.
    fn status(id: &str) -> Value {
        json!({
            "session_id": id,
            "model": { "id": "claude-opus-5-5", "display_name": "Opus 5.5 (1M context)" },
            "context_window": {
                "total_input_tokens": 15500, "total_output_tokens": 1200, "context_window_size": 1000000,
                "used_percentage": 8, "remaining_percentage": 92,
                "current_usage": { "input_tokens": 8500, "output_tokens": 1200, "cache_creation_input_tokens": 5000, "cache_read_input_tokens": 2000 }
            },
            "prompt_cache": { "warm": true, "caching_observed": true, "ttl": "1h", "expires_at": 1738429200, "hit_ratio": 0.91 },
            "rate_limits": {
                "five_hour": { "used_percentage": 23.5, "resets_at": 1738425600 },
                "seven_day": { "used_percentage": 41.2, "resets_at": 1738857600 }
            },
            "cost": { "total_cost_usd": 0.01 },
            "transcript_path": "/Users/me/.claude/projects/x/y.jsonl"
        })
    }

    #[test]
    fn keeps_the_limits_and_each_sessions_context_and_cache() {
        let mut s = Statusline::default();
        s.record(&status(ID), &Value::Null, None, 1000);
        let view = s.view();
        assert_eq!(view["rateLimits"], json!([{ "account": null, "fiveHour": { "usedPercentage": 23.5, "resetsAt": 1738425600 }, "sevenDay": { "usedPercentage": 41.2, "resetsAt": 1738857600 }, "at": 1000 }]));
        assert_eq!(
            view["sessions"][ID],
            json!({
                "account": null, "model": "Opus 5.5 (1M context)", "contextWindowSize": 1000000, "usedPercentage": 8.0,
                "totalInputTokens": 15500, "totalOutputTokens": 1200,
                "currentUsage": { "inputTokens": 8500, "outputTokens": 1200, "cacheCreationInputTokens": 5000, "cacheReadInputTokens": 2000 },
                "promptCache": { "warm": true, "ttl": "1h", "expiresAt": 1738429200 },
                "at": 1000
            })
        );
        // Only what's listed: no paths, costs or anything else Claude Code sends.
        assert!(!view.to_string().contains("transcript") && !view.to_string().contains("cost"));
    }

    /// Every open session's status line can run every few seconds. One that has been idle since the limits were reset
    /// kept sending the old numbers, which replaced a working session's new ones each time.
    #[test]
    fn a_session_that_repeats_its_old_limits_does_not_replace_a_working_sessions_new_ones() {
        // A reset moves `resets_at` on: the old numbers are of the period before, the new ones of the one after.
        let limits = |five: f64, week: f64, period: i64| json!({ "five_hour": { "used_percentage": five, "resets_at": 1738425600 + period }, "seven_day": { "used_percentage": week, "resets_at": 1738857600 + period } });
        let shown = |s: &Statusline| (s.view()["rateLimits"][0]["fiveHour"]["usedPercentage"].clone(), s.view()["rateLimits"][0]["sevenDay"]["usedPercentage"].clone());
        let (idle, working) = (status(ID), status("4c21b0c5-8113-4f55-9493-fa156c3fa369"));
        let mut s = Statusline::default();
        let mut old = idle.clone();
        old["rate_limits"] = limits(93.0, 38.0, 0);
        s.record(&old, &Value::Null, None, 1000);
        let mut new = working.clone();
        new["rate_limits"] = limits(1.0, 11.0, 1000);
        s.record(&new, &Value::Null, None, 2000);
        s.record(&old, &Value::Null, None, 3000);
        assert_eq!(shown(&s), (json!(1.0), json!(11.0)));
        // The working session's next response has the same numbers and more tokens: still its, and still the newest.
        new["context_window"]["total_output_tokens"] = json!(1300);
        s.record(&new, &Value::Null, None, 4000);
        assert_eq!(s.view()["rateLimits"][0]["at"], 4000);
        // The idle one answers again: it has news now.
        old["context_window"]["total_output_tokens"] = json!(1300);
        old["rate_limits"] = limits(2.0, 11.0, 1000);
        s.record(&old, &Value::Null, None, 5000);
        assert_eq!(shown(&s), (json!(2.0), json!(11.0)));
    }

    /// After Wings restarts every session's first report is new to it, so an idle session's old numbers arrive as
    /// "news", and a working session that then repeats its own is dropped as no news.
    #[test]
    fn a_stale_first_report_does_not_replace_newer_numbers_of_the_same_period() {
        let limits = |five: f64, week: f64, reset: i64| json!({ "five_hour": { "used_percentage": five, "resets_at": reset }, "seven_day": { "used_percentage": week, "resets_at": 1738857600 } });
        let shown = |s: &Statusline| (s.view()["rateLimits"][0]["fiveHour"]["usedPercentage"].clone(), s.view()["rateLimits"][0]["sevenDay"]["usedPercentage"].clone());
        let (working, idle) = (status(ID), status("4c21b0c5-8113-4f55-9493-fa156c3fa369"));
        let mut s = Statusline::default();
        let mut new = working.clone();
        new["rate_limits"] = limits(55.0, 87.0, 1738425600);
        s.record(&new, &Value::Null, None, 1000);
        let mut old = idle.clone();
        old["rate_limits"] = limits(50.0, 85.0, 1738425600);
        s.record(&old, &Value::Null, None, 2000);
        s.record(&new, &Value::Null, None, 3000);
        assert_eq!(shown(&s), (json!(55.0), json!(87.0)));
        // A report of an older period, after the usage was reset, is not newer either.
        let mut reset = working.clone();
        reset["context_window"]["total_output_tokens"] = json!(1300);
        reset["rate_limits"] = limits(2.0, 88.0, 1738443600);
        s.record(&reset, &Value::Null, None, 4000);
        assert_eq!(shown(&s), (json!(2.0), json!(88.0)));
        old["context_window"]["total_output_tokens"] = json!(1400);
        s.record(&old, &Value::Null, None, 5000);
        assert_eq!(shown(&s), (json!(2.0), json!(88.0)));
    }

    #[test]
    fn a_status_without_limits_keeps_the_last_ones() {
        let mut s = Statusline::default();
        s.record(&status(ID), &Value::Null, None, 1000);
        // A new session before its first response: no limits, and no context yet.
        let other = "4c21b0c5-8113-4f55-9493-fa156c3fa369";
        s.record(&json!({ "session_id": other, "context_window": { "context_window_size": 200000, "used_percentage": null, "current_usage": null } }), &Value::Null, None, 2000);
        let view = s.view();
        assert_eq!(view["rateLimits"][0]["at"], 1000);
        assert_eq!(view["sessions"][other]["usedPercentage"], Value::Null);
        assert_eq!(view["sessions"][other]["currentUsage"], Value::Null);
        // Claude Code drops a window once it resets; the newest report wins.
        s.record(&json!({ "session_id": ID, "rate_limits": { "five_hour": { "used_percentage": 1, "resets_at": 1738443600 } } }), &Value::Null, None, 3000);
        assert_eq!(s.view()["rateLimits"], json!([{ "account": null, "fiveHour": { "usedPercentage": 1.0, "resetsAt": 1738443600 }, "sevenDay": null, "at": 3000 }]));
    }

    /// Two accounts each keep their own limits, so they don't take turns and alert again and again.
    #[test]
    fn keeps_each_accounts_limits() {
        let mut s = Statusline::default();
        let other = "4c21b0c5-8113-4f55-9493-fa156c3fa369";
        s.record(&status(ID), &Value::Null, None, 1000);
        let mut work = status(other);
        work["rate_limits"]["five_hour"] = json!({ "used_percentage": 90, "resets_at": 1738420000 });
        s.record(&work, &json!("work"), None, 2000);
        // After another response, or it would be a repeat with no news.
        let mut again = status(ID);
        again["context_window"]["total_output_tokens"] = json!(1300);
        s.record(&again, &Value::Null, None, 3000);
        let view = s.view();
        let accounts: Vec<_> = view["rateLimits"].as_array().unwrap().iter().map(|l| (l["account"].clone(), l["fiveHour"]["usedPercentage"].clone())).collect();
        assert_eq!(accounts, [(Value::Null, json!(23.5)), (json!("work"), json!(90.0))]);
        assert_eq!((view["sessions"][ID]["account"].clone(), view["sessions"][other]["account"].clone()), (Value::Null, json!("work")));
        // A name that couldn't be an account's folder counts as the default account.
        s.record(&status(ID), &json!("../x"), None, 4000);
        assert_eq!(s.view()["rateLimits"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn accounts_signed_in_to_one_login_share_its_newest_limits() {
        let limits = |percent: u64| json!({ "session_id": ID, "rate_limits": { "five_hour": { "used_percentage": percent, "resets_at": 1738443600 } } });
        let mut s = Statusline::default();
        s.record(&limits(16), &Value::Null, Some("me"), 1000);
        s.record(&limits(90), &json!("work"), Some("them"), 2000);
        s.record(&limits(41), &json!("personal"), Some("me"), 3000);
        // An account whose login isn't known keeps its own.
        s.record(&limits(5), &json!("other"), None, 4000);
        s.record(&limits(7), &json!("another"), None, 5000);
        let view = s.view();
        let shown: Vec<_> = view["rateLimits"].as_array().unwrap().iter().map(|l| (l["account"].clone(), l["fiveHour"]["usedPercentage"].clone(), l["at"].clone())).collect();
        assert_eq!(
            shown,
            [
                (json!("another"), json!(7.0), json!(5000)),
                (json!("other"), json!(5.0), json!(4000)),
                (json!("personal"), json!(41.0), json!(3000)),
                (json!("work"), json!(90.0), json!(2000)),
                (Value::Null, json!(41.0), json!(3000)),
            ]
        );
        assert!(view["rateLimits"][0].get("login").is_none());
    }

    /// Limits Wings read itself (`usage.rs`) and a status line's join by the newest-data rule, whichever comes first.
    #[test]
    fn read_limits_and_status_lines_keep_the_newest_data_both_ways() {
        let window = |used: f64, resets_at: i64| Some(Window { used_percentage: used, resets_at: Some(resets_at) });
        let shown = |s: &Statusline| {
            let view = s.view();
            let l = &view["rateLimits"][0];
            (l["fiveHour"]["usedPercentage"].clone(), l["fiveHour"]["resetsAt"].clone(), l["sevenDay"]["usedPercentage"].clone(), l["at"].clone())
        };
        let (idle, working) = (status(ID), status("4c21b0c5-8113-4f55-9493-fa156c3fa369"));

        // A status line, then a read with more used. Its reset time, given to a fraction of a second, is a second later
        // than the status line's: the same period, so the reset time stays and the HUD doesn't alert again.
        let mut s = Statusline::default();
        s.record(&working, &Value::Null, None, 1000);
        s.record_usage(None, None, window(30.0, 1738425601), window(41.0, 1738857600), 2000);
        assert_eq!(shown(&s), (json!(30.0), json!(1738425600), json!(41.2), json!(2000)));
        // A session that hasn't heard since sends its older numbers: the read ones stay.
        s.record(&idle, &Value::Null, None, 3000);
        assert_eq!(shown(&s).0, json!(30.0));
        // A read in the next period wins, and a window it doesn't have stays as it was.
        s.record_usage(None, None, window(2.0, 1738443600), None, 4000);
        assert_eq!(shown(&s), (json!(2.0), json!(1738443600), json!(41.2), json!(4000)));

        // A read first, then status lines: an old period's report doesn't replace it, the next period's does.
        let mut s = Statusline::default();
        s.record_usage(Some("work"), Some("me"), window(50.0, 1738443600), window(60.0, 1738857600), 1000);
        let mut old = working.clone();
        old["rate_limits"]["five_hour"] = json!({ "used_percentage": 80, "resets_at": 1738425600 });
        s.record(&old, &json!("work"), Some("me"), 2000);
        assert_eq!(shown(&s), (json!(50.0), json!(1738443600), json!(60.0), json!(2000)));
        let mut next = working.clone();
        next["context_window"]["total_output_tokens"] = json!(1300);
        next["rate_limits"]["five_hour"] = json!({ "used_percentage": 1, "resets_at": 1738461600 });
        s.record(&next, &json!("work"), Some("me"), 3000);
        assert_eq!(shown(&s), (json!(1.0), json!(1738461600), json!(60.0), json!(3000)));
    }

    /// The HUD redraws when what `wings.statusline()` resolves changes.
    #[test]
    fn a_read_with_no_news_changes_nothing() {
        let mut s = Statusline::default();
        s.record(&status(ID), &Value::Null, None, 1000);
        let before = s.view();
        let same = |used: f64, resets_at: i64| Some(Window { used_percentage: used, resets_at: Some(resets_at) });
        s.record_usage(None, None, same(23.5, 1738425600), same(41.2, 1738857600), 2000);
        s.record_usage(None, None, None, None, 3000);
        assert_eq!(s.view(), before);
        // An account with no limits yet gets them; one whose name couldn't be a folder counts as the default.
        s.record_usage(Some("work"), None, same(5.0, 1738425600), None, 4000);
        s.record_usage(Some("../x"), None, same(24.0, 1738425600), None, 5000);
        let accounts: Vec<_> = s.view()["rateLimits"].as_array().unwrap().iter().map(|l| (l["account"].clone(), l["fiveHour"]["usedPercentage"].clone())).collect();
        assert_eq!(accounts, [(json!("work"), json!(5.0)), (Value::Null, json!(24.0))]);
    }

    #[test]
    fn lists_the_sessions_that_reported_by_account() {
        let mut s = Statusline::default();
        let other = "4c21b0c5-8113-4f55-9493-fa156c3fa369";
        s.record(&status(ID), &Value::Null, None, 1000);
        s.record(&status(other), &json!("work"), None, 2000);
        let by: Vec<_> = s.sessions_by_account().into_iter().map(|(account, ids)| (account, ids.into_iter().collect::<Vec<_>>())).collect();
        assert_eq!(by, [(None, vec![ID.to_string()]), (Some("work".to_string()), vec![other.to_string()])]);
    }

    #[test]
    fn untrusted_input_is_checked_and_bounded() {
        let mut s = Statusline::default();
        for bad in [json!(null), json!("x"), json!({ "session_id": "../../etc" }), json!({ "session_id": 7 })] {
            s.record(&bad, &Value::Null, None, 1);
        }
        assert_eq!(s.view(), json!({ "rateLimits": [], "sessions": {} }));
        s.record(
            &json!({
                "session_id": ID,
                "model": { "display_name": format!("\u{1b}[31m{}", "x".repeat(500)) },
                "context_window": { "used_percentage": 250, "context_window_size": -5 },
                "prompt_cache": { "warm": "yes", "ttl": "forever", "expires_at": "soon" },
                "rate_limits": { "five_hour": { "used_percentage": "lots" }, "seven_day": { "used_percentage": -3 } }
            }),
            &Value::Null,
            None,
            1,
        );
        let view = s.view();
        let session = &view["sessions"][ID];
        assert_eq!(session["model"].as_str().unwrap().chars().count(), MODEL_MAX);
        assert!(!session["model"].as_str().unwrap().contains('\u{1b}'));
        assert_eq!((session["usedPercentage"].clone(), session["contextWindowSize"].clone()), (json!(100.0), Value::Null));
        assert_eq!(session["promptCache"], json!({ "warm": false, "ttl": null, "expiresAt": null }));
        assert_eq!(view["rateLimits"], json!([{ "account": null, "fiveHour": null, "sevenDay": { "usedPercentage": 0.0, "resetsAt": null }, "at": 1 }]));
    }

    #[test]
    fn keeps_only_the_newest_sessions() {
        let mut s = Statusline::default();
        for i in 0..SESSIONS_MAX as u64 + 5 {
            s.record(&status(&format!("{i:08x}-8113-4f55-9493-fa156c3fa369")), &Value::Null, None, i);
        }
        assert_eq!(s.sessions.len(), SESSIONS_MAX);
        assert!(!s.sessions.contains_key("00000000-8113-4f55-9493-fa156c3fa369"));
        assert!(s.sessions.contains_key(&format!("{:08x}-8113-4f55-9493-fa156c3fa369", SESSIONS_MAX + 4)));
    }
}
