//! What a turn spends, as the chat shows it quietly: the tokens so far in the status line, a finished turn's tokens and
//! cost under it, how far each subagent has got, and how long the prompt cache stays warm.
//!
//! Shapes are from Claude Code 2.1.328 (its bundled schema):
//! - `usage` of an assistant message, of the `message_start` stream event's `message` and of the `result`:
//!   `{ input_tokens, output_tokens, cache_creation_input_tokens, cache_read_input_tokens,
//!   cache_creation: { ephemeral_5m_input_tokens, ephemeral_1h_input_tokens } }` ("Cache writes by TTL"). The
//!   `message_delta` event carries `usage.output_tokens`, a running total of that message.
//! - `{ type: "system", subtype: "thinking_tokens", estimated_tokens, estimated_tokens_delta }` while Claude thinks.
//! - `{ type: "system", subtype: "task_progress", task_id, tool_use_id, description, last_tool_name,
//!   usage: { total_tokens, tool_uses, duration_ms } }` for a running subagent (or background task).
//! - `result` has `duration_ms`, `duration_api_ms` and `total_cost_usd`, which is the session's running total.
//! - `system`/`init` has `apiKeySource`: `ANTHROPIC_API_KEY`, `apiKeyHelper` or `/login managed key` mean an API key
//!   (billed per token), `none` means a claude.ai login, a bearer token or a cloud provider (not told apart).
//! - Prompt cache lifetime: the setting `promptCacheTtl` is "5m" or "1h"; unset is "1 hour on a Claude subscription
//!   within its usage limits, 5 minutes on an API key, Bedrock, Vertex or Foundry". A request's own `usage.cache_creation`
//!   says which one it wrote, so that is what the clock follows.

use std::collections::HashMap;

use serde_json::Value;

/// How long the prompt cache lasts after the last request that used it.
pub const CACHE_5M: u64 = 5 * 60_000;
pub const CACHE_1H: u64 = 60 * 60_000;
/// How often the cache clock is redrawn, in ms, while Wings is in front.
pub const CACHE_TICK_MS: u32 = 15_000;

/// The tokens and cost of one turn, as its `result` or its messages said them.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Spend {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    /// This turn's share of the session's cost, in US dollars.
    pub cost: Option<f64>,
}

impl Spend {
    fn add(&mut self, other: &Spend) {
        self.input += other.input;
        self.output += other.output;
        self.cache_read += other.cache_read;
        self.cache_write += other.cache_write;
    }

    /// The four counts of a `usage` object.
    pub fn of(usage: &Value) -> Spend {
        let get = |key: &str| usage[key].as_u64().unwrap_or(0);
        Spend {
            input: get("input_tokens"),
            output: get("output_tokens"),
            cache_read: get("cache_read_input_tokens"),
            cache_write: get("cache_creation_input_tokens"),
            ..Spend::default()
        }
    }

    /// Keeps the larger of each count: a message's usage only grows as it streams.
    fn grow(&mut self, usage: &Spend) {
        self.input = self.input.max(usage.input);
        self.output = self.output.max(usage.output);
        self.cache_read = self.cache_read.max(usage.cache_read);
        self.cache_write = self.cache_write.max(usage.cache_write);
    }
}

/// How far a subagent has got, from its `task_progress` lines.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct AgentProgress {
    pub tokens: u64,
    pub tool_uses: u64,
    pub duration_ms: u64,
}

/// What a session counts as it hears the stream.
#[derive(Debug, Default)]
pub struct Meter {
    /// The turn's messages that are finished.
    total: Spend,
    /// The message being written: its id (Claude Code prints one line per content block, all with that id) and usage.
    id: Option<String>,
    current: Spend,
    /// Claude Code's estimate of the thinking so far in the message.
    think: u64,
    /// How far each subagent has got, by the id of the call that started it.
    pub agents: HashMap<String, AgentProgress>,
    /// When the last request was sent (ms), and whether it wrote a one-hour cache. 0: none heard yet.
    pub requested: u64,
    pub hour: bool,
    /// The credential is an API key, which bills per token.
    pub per_token: bool,
}

impl Meter {
    /// A message begins (`message_start`) or arrives whole (`assistant`): a new id folds the last message into the turn.
    pub fn message(&mut self, id: Option<&str>, usage: &Value) {
        let same = id.is_some() && self.id.as_deref() == id;
        if !same {
            let done = std::mem::take(&mut self.current);
            self.total.add(&done);
            self.think = 0;
            self.id = id.map(str::to_owned);
        }
        self.update(usage);
    }

    /// A running total of the message being written (`message_delta`, or a later line of the same message).
    pub fn update(&mut self, usage: &Value) {
        self.current.grow(&Spend::of(usage));
    }

    /// `thinking_tokens`.
    pub fn thinking(&mut self, estimated: u64) {
        self.think = self.think.max(estimated);
    }

    /// The output tokens of the turn so far, counting the thinking Claude Code estimates while the message's own count is
    /// still at its start.
    pub fn tokens(&self) -> u64 {
        self.total.output + self.current.output.max(self.think)
    }

    /// The turn's counts from its messages, and starts the next turn.
    pub fn end_turn(&mut self) -> Spend {
        let mut spend = std::mem::take(&mut self.total);
        spend.add(&std::mem::take(&mut self.current));
        self.id = None;
        self.think = 0;
        spend
    }

    /// A request was sent at `at` (ms) with this `usage`: the cache clock restarts, and the lifetime is the one it wrote.
    pub fn requested_at(&mut self, at: u64, usage: &Value) {
        self.requested = at;
        self.note_cache(usage);
    }

    /// What a request wrote to the cache says how long it lasts; one that only read it says nothing.
    pub fn note_cache(&mut self, usage: &Value) {
        let written = &usage["cache_creation"];
        if written["ephemeral_1h_input_tokens"].as_u64().unwrap_or(0) > 0 {
            self.hour = true;
        } else if written["ephemeral_5m_input_tokens"].as_u64().unwrap_or(0) > 0 {
            self.hour = false;
        }
    }

    /// `task_progress`.
    pub fn progress(&mut self, line: &Value) {
        let Some(call) = line["tool_use_id"].as_str() else {
            return;
        };
        let usage = &line["usage"];
        self.agents.insert(
            call.to_owned(),
            AgentProgress {
                tokens: usage["total_tokens"].as_u64().unwrap_or(0),
                tool_uses: usage["tool_uses"].as_u64().unwrap_or(0),
                duration_ms: usage["duration_ms"].as_u64().unwrap_or(0),
            },
        );
    }

    /// "Cache warm · 3m left" or "Cache expired", None before any request.
    pub fn cache(&self, now: u64) -> Option<String> {
        cache_line(self.requested, self.hour, now)
    }
}

/// Counts as the status line and the worked line say them: "950", "2.1k", "12.3k", "123k", "1.2M".
pub fn count(n: u64) -> String {
    match n {
        0..1000 => n.to_string(),
        1000..99_950 => format!("{:.1}k", n as f64 / 1000.0),
        99_950..999_500 => format!("{}k", (n as f64 / 1000.0).round()),
        _ => format!("{:.1}M", n as f64 / 1_000_000.0),
    }
}

/// "2.1k tokens", for the status line while a turn runs. None until there is something to count.
pub fn live_tokens(n: u64) -> Option<String> {
    (n > 0).then(|| format!("{} tokens", count(n)))
}

fn dollars(cost: f64) -> String {
    if cost >= 0.01 || cost == 0.0 { format!("${cost:.2}") } else { format!("${cost:.3}") }
}

/// The line under a finished turn, and its hover: "Worked for 14s · 12.3k tokens · $0.04". Tokens are what Claude wrote;
/// the cost shows only when `per_token` (an API key), else only in the hover, as a subscription pays no such price.
pub fn worked(time: &str, spend: &Spend, per_token: bool) -> (String, Option<String>) {
    let mut line = format!("Worked for {time}");
    if spend.output > 0 {
        line.push_str(&format!(" · {} tokens", count(spend.output)));
    }
    let cost = spend.cost.filter(|cost| *cost > 0.0);
    if let (true, Some(cost)) = (per_token, cost) {
        line.push_str(&format!(" · {}", dollars(cost)));
    }
    let mut hover = Vec::new();
    if spend.input + spend.output + spend.cache_read + spend.cache_write > 0 {
        hover.push(format!("In {}", count(spend.input)));
        hover.push(format!("out {}", count(spend.output)));
        hover.push(format!("cache read {}", count(spend.cache_read)));
        hover.push(format!("cache write {}", count(spend.cache_write)));
    }
    if let Some(cost) = cost {
        hover.push(format!("{} estimated", dollars(cost)));
    }
    let hover = (!hover.is_empty()).then(|| hover.join(" · "));
    (line, hover)
}

/// The prompt cache's clock from the last request at `requested` (ms): "Cache warm · 3m left" counting down, "Cache
/// expired" after its lifetime. None when no request was heard.
pub fn cache_line(requested: u64, hour: bool, now: u64) -> Option<String> {
    if requested == 0 {
        return None;
    }
    let ttl = if hour { CACHE_1H } else { CACHE_5M };
    let left = ttl.saturating_sub(now.saturating_sub(requested));
    Some(match left {
        0 => "Cache expired".to_owned(),
        1..60_000 => "Cache warm · <1m left".to_owned(),
        _ => format!("Cache warm · {}m left", left.div_ceil(60_000)),
    })
}

/// Whether an `init`'s `apiKeySource` is a key billed per token.
pub fn bills_per_token(source: &str) -> bool {
    matches!(source, "ANTHROPIC_API_KEY" | "apiKeyHelper" | "/login managed key")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn counts_read_like_the_status_line() {
        assert_eq!([count(950), count(2_100), count(12_345), count(123_400), count(1_300_000)], ["950", "2.1k", "12.3k", "123k", "1.3M"]);
        assert_eq!(live_tokens(0), None);
        assert_eq!(live_tokens(2_100).as_deref(), Some("2.1k tokens"));
    }

    #[test]
    fn the_live_counter_counts_each_message_once_and_thinking_until_its_message_reports() {
        let mut meter = Meter::default();
        // message_start, then thinking estimates beat its tiny count.
        meter.message(Some("m1"), &json!({ "output_tokens": 1 }));
        meter.thinking(800);
        assert_eq!(meter.tokens(), 800);
        // message_delta gives the message's running total; the assistant lines of its blocks repeat it.
        meter.update(&json!({ "output_tokens": 1_000 }));
        meter.message(Some("m1"), &json!({ "output_tokens": 1_000 }));
        meter.message(Some("m1"), &json!({ "output_tokens": 1_000 }));
        assert_eq!(meter.tokens(), 1_000);
        // The next message adds, and the estimate of the last one does not carry over.
        meter.message(Some("m2"), &json!({ "output_tokens": 1 }));
        meter.update(&json!({ "output_tokens": 1_100 }));
        assert_eq!(meter.tokens(), 2_100);
        let spend = meter.end_turn();
        assert_eq!(spend.output, 2_100);
        assert_eq!(meter.tokens(), 0);
    }

    #[test]
    fn a_subagent_row_reads_tool_calls_and_time_from_task_progress() {
        let mut meter = Meter::default();
        meter.progress(&json!({ "type": "system", "subtype": "task_progress", "task_id": "a1", "tool_use_id": "toolu_1", "description": "scan", "usage": { "total_tokens": 4_200, "tool_uses": 7, "duration_ms": 12_000 } }));
        assert_eq!(meter.agents["toolu_1"], AgentProgress { tokens: 4_200, tool_uses: 7, duration_ms: 12_000 });
        // A line without a call id names no row.
        meter.progress(&json!({ "usage": { "tool_uses": 1 } }));
        assert_eq!(meter.agents.len(), 1);
    }

    #[test]
    fn the_worked_line_shows_cost_only_for_an_api_key_and_keeps_it_in_the_hover_otherwise() {
        let spend = Spend { input: 1_200, output: 12_300, cache_read: 50_000, cache_write: 2_000, cost: Some(0.0412) };
        let (line, hover) = worked("14s", &spend, true);
        assert_eq!(line, "Worked for 14s · 12.3k tokens · $0.04");
        assert_eq!(hover.as_deref(), Some("In 1.2k · out 12.3k · cache read 50.0k · cache write 2.0k · $0.04 estimated"));
        let (line, hover) = worked("14s", &spend, false);
        assert_eq!(line, "Worked for 14s · 12.3k tokens");
        assert!(hover.unwrap().contains("$0.04 estimated"));
        // Nothing counted: the plain line it was.
        assert_eq!(worked("5s", &Spend::default(), true), ("Worked for 5s".to_owned(), None));
        assert!(bills_per_token("ANTHROPIC_API_KEY") && bills_per_token("/login managed key") && !bills_per_token("none"));
    }

    #[test]
    fn the_cache_clock_counts_down_from_the_last_request_and_expires() {
        assert_eq!(cache_line(0, false, 10_000), None);
        let at = 1_000_000;
        assert_eq!(cache_line(at, false, at + 1_000).as_deref(), Some("Cache warm · 5m left"));
        assert_eq!(cache_line(at, false, at + 2 * 60_000).as_deref(), Some("Cache warm · 3m left"));
        assert_eq!(cache_line(at, false, at + 4 * 60_000 + 30_000).as_deref(), Some("Cache warm · <1m left"));
        assert_eq!(cache_line(at, false, at + CACHE_5M).as_deref(), Some("Cache expired"));
        assert_eq!(cache_line(at, true, at + CACHE_5M).as_deref(), Some("Cache warm · 55m left"));
        assert_eq!(cache_line(at, true, at + CACHE_1H + 1).as_deref(), Some("Cache expired"));
    }

    #[test]
    fn the_cache_lifetime_follows_what_the_request_wrote() {
        let mut meter = Meter::default();
        meter.requested_at(5, &json!({ "cache_creation": { "ephemeral_5m_input_tokens": 100, "ephemeral_1h_input_tokens": 0 } }));
        assert!(!meter.hour);
        meter.requested_at(6, &json!({ "cache_creation": { "ephemeral_5m_input_tokens": 0, "ephemeral_1h_input_tokens": 100 } }));
        assert!(meter.hour);
        // A request that only read the cache says nothing new.
        meter.requested_at(7, &json!({ "cache_read_input_tokens": 500 }));
        assert!(meter.hour && meter.requested == 7);
    }
}
