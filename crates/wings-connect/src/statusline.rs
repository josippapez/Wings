//! What Claude Code tells its status line, passed on by `wings statusline` (cli.rs) once you make that your
//! `statusLine` command. Plugins with `permissions.statusline` read it with `wings.statusline()`: your usage
//! limits and each session's context window and prompt cache, as Claude Code reports them, rather than worked
//! out from transcripts. Only these fields are kept, checked and bounded, since any process of yours can
//! write to the socket. Each Claude Code account has limits of its own, so they are kept per account.

use std::collections::HashMap;

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

impl Statusline {
    /// Keeps what one status line run of `account` reported, at `now` in ms.
    pub fn record(&mut self, status: &Value, account: &Value, now: u64) {
        let account = account.as_str().filter(|name| wings_spaces::claude_accounts::valid_name(name)).map(str::to_string);
        let limits = &status["rate_limits"];
        let (five_hour, seven_day) = (window(&limits["five_hour"]), window(&limits["seven_day"]));
        // Absent before a session's first response, so that says nothing about the account.
        if five_hour.is_some() || seven_day.is_some() {
            self.limits.retain(|l| l.account != account);
            // Newest first. Names are checked, and there are only as many as you made.
            self.limits.insert(0, Limits { account: account.clone(), five_hour, seven_day, at: now });
            self.limits.truncate(ACCOUNTS_MAX);
        }
        let Some(id) = session_id(&status["session_id"]) else { return };
        let context = &status["context_window"];
        let usage = &context["current_usage"];
        let current_usage = usage.is_object().then(|| Usage {
            input_tokens: count(&usage["input_tokens"]).unwrap_or(0),
            output_tokens: count(&usage["output_tokens"]).unwrap_or(0),
            cache_creation_input_tokens: count(&usage["cache_creation_input_tokens"]).unwrap_or(0),
            cache_read_input_tokens: count(&usage["cache_read_input_tokens"]).unwrap_or(0),
        });
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
            total_input_tokens: count(&context["total_input_tokens"]),
            total_output_tokens: count(&context["total_output_tokens"]),
            current_usage,
            prompt_cache,
            at: now,
        };
        self.sessions.insert(id.to_string(), session);
        if self.sessions.len() > SESSIONS_MAX {
            let oldest = self.sessions.iter().min_by_key(|(_, s)| s.at).map(|(id, _)| id.clone());
            self.sessions.remove(&oldest.unwrap_or_default());
        }
    }

    /// What `wings.statusline()` resolves.
    pub fn view(&self) -> Value {
        serde_json::json!({ "rateLimits": self.limits, "sessions": self.sessions })
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
        s.record(&status(ID), &Value::Null, 1000);
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

    #[test]
    fn a_status_without_limits_keeps_the_last_ones() {
        let mut s = Statusline::default();
        s.record(&status(ID), &Value::Null, 1000);
        // A new session before its first response: no limits, and no context yet.
        let other = "4c21b0c5-8113-4f55-9493-fa156c3fa369";
        s.record(&json!({ "session_id": other, "context_window": { "context_window_size": 200000, "used_percentage": null, "current_usage": null } }), &Value::Null, 2000);
        let view = s.view();
        assert_eq!(view["rateLimits"][0]["at"], 1000);
        assert_eq!(view["sessions"][other]["usedPercentage"], Value::Null);
        assert_eq!(view["sessions"][other]["currentUsage"], Value::Null);
        // Claude Code drops a window once it resets; the newest report wins.
        s.record(&json!({ "session_id": ID, "rate_limits": { "five_hour": { "used_percentage": 1, "resets_at": 1738443600 } } }), &Value::Null, 3000);
        assert_eq!(s.view()["rateLimits"], json!([{ "account": null, "fiveHour": { "usedPercentage": 1.0, "resetsAt": 1738443600 }, "sevenDay": null, "at": 3000 }]));
    }

    /// Two accounts each keep their own limits, so they don't take turns and alert again and again.
    #[test]
    fn keeps_each_accounts_limits() {
        let mut s = Statusline::default();
        let other = "4c21b0c5-8113-4f55-9493-fa156c3fa369";
        s.record(&status(ID), &Value::Null, 1000);
        let mut work = status(other);
        work["rate_limits"]["five_hour"] = json!({ "used_percentage": 90, "resets_at": 1738420000 });
        s.record(&work, &json!("work"), 2000);
        s.record(&status(ID), &Value::Null, 3000);
        let view = s.view();
        let accounts: Vec<_> = view["rateLimits"].as_array().unwrap().iter().map(|l| (l["account"].clone(), l["fiveHour"]["usedPercentage"].clone())).collect();
        assert_eq!(accounts, [(Value::Null, json!(23.5)), (json!("work"), json!(90.0))]);
        assert_eq!((view["sessions"][ID]["account"].clone(), view["sessions"][other]["account"].clone()), (Value::Null, json!("work")));
        // A name that couldn't be an account's folder counts as the default account.
        s.record(&status(ID), &json!("../x"), 4000);
        assert_eq!(s.view()["rateLimits"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn untrusted_input_is_checked_and_bounded() {
        let mut s = Statusline::default();
        for bad in [json!(null), json!("x"), json!({ "session_id": "../../etc" }), json!({ "session_id": 7 })] {
            s.record(&bad, &Value::Null, 1);
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
            s.record(&status(&format!("{i:08x}-8113-4f55-9493-fa156c3fa369")), &Value::Null, i);
        }
        assert_eq!(s.sessions.len(), SESSIONS_MAX);
        assert!(!s.sessions.contains_key("00000000-8113-4f55-9493-fa156c3fa369"));
        assert!(s.sessions.contains_key(&format!("{:08x}-8113-4f55-9493-fa156c3fa369", SESSIONS_MAX + 4)));
    }
}
