//! What Claude Code answers to `get_usage` and `get_context_usage`, cut down to what the usage and context dialogs show.
//! Only what the answer has is kept: a window or a section the answer lacks is not here, so the dialog doesn't draw it.

use serde_json::Value;

/// The dialogs `/usage`, `/cost` and `/context` open over the chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Usage,
    Context,
}

/// One of the plan's limits: its name, how much of it is used (percent) and when it resets (as Claude Code wrote it).
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    pub name: String,
    pub used: u64,
    pub resets: String,
}

/// Usage credits, in the major unit of `currency` (euros, not cents).
#[derive(Debug, Clone, PartialEq)]
pub struct Credits {
    pub spent: f64,
    pub limit: Option<f64>,
    pub currency: String,
    pub used: Option<u64>,
}

/// What the local transcripts say fills the limits over a day or a week: each a name and its share (percent).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Share {
    /// What Claude Code recommends, as (share, title, detail).
    pub tips: Vec<(u64, String, String)>,
    pub skills: Vec<(String, u64)>,
    pub plugins: Vec<(String, u64)>,
    pub mcp_servers: Vec<(String, u64)>,
    pub agents: Vec<(String, u64)>,
}

impl Share {
    pub fn is_empty(&self) -> bool {
        self.tips.is_empty() && self.skills.is_empty() && self.plugins.is_empty() && self.mcp_servers.is_empty() && self.agents.is_empty()
    }
}

/// The session's own totals.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Totals {
    pub cost: f64,
    pub api_ms: u64,
    pub wall_ms: u64,
    pub added: u64,
    pub removed: u64,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Usage {
    pub plan: Option<String>,
    pub windows: Vec<Window>,
    pub credits: Option<Credits>,
    /// The last 24 hours and the last 7 days. None when Claude Code didn't scan (an API key, or a failed scan).
    pub day: Option<Share>,
    pub week: Option<Share>,
    pub totals: Totals,
}

fn percent(value: &Value) -> Option<u64> {
    Some(value.as_f64()?.round().max(0.0) as u64)
}

impl Usage {
    /// None when `answer` isn't the usage answer at all.
    pub fn parse(answer: &Value) -> Option<Usage> {
        let session = answer.get("session")?;
        let number = |key: &str| session[key].as_u64().unwrap_or_else(|| session[key].as_f64().unwrap_or(0.0) as u64);
        let mut totals = Totals {
            cost: session["total_cost_usd"].as_f64().unwrap_or(0.0),
            api_ms: number("total_api_duration_ms"),
            wall_ms: number("total_duration_ms"),
            added: number("total_lines_added"),
            removed: number("total_lines_removed"),
            ..Totals::default()
        };
        for usage in session["model_usage"].as_object().into_iter().flat_map(|models| models.values()) {
            totals.input += usage["inputTokens"].as_u64().unwrap_or(0);
            totals.output += usage["outputTokens"].as_u64().unwrap_or(0);
            totals.cache_read += usage["cacheReadInputTokens"].as_u64().unwrap_or(0);
            totals.cache_write += usage["cacheCreationInputTokens"].as_u64().unwrap_or(0);
        }
        let limits = &answer["rate_limits"];
        let window = |name: &str, limit: &Value| Some(Window { name: name.to_owned(), used: percent(&limit["utilization"])?, resets: limit["resets_at"].as_str().unwrap_or_default().to_owned() });
        let mut windows: Vec<Window> = [("Current session", "five_hour"), ("Current week (all models)", "seven_day")].into_iter().filter_map(|(name, key)| window(name, &limits[key])).collect();
        // The per-model windows the server lists, else the fixed Opus and Sonnet ones.
        let scoped: Vec<Window> = limits["model_scoped"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|model| window(&format!("Current week ({})", model["display_name"].as_str()?), model))
            .collect();
        if scoped.is_empty() {
            windows.extend([("Current week (Opus)", "seven_day_opus"), ("Current week (Sonnet)", "seven_day_sonnet")].into_iter().filter_map(|(name, key)| window(name, &limits[key])));
        } else {
            windows.extend(scoped);
        }
        let extra = &limits["extra_usage"];
        let credits = (extra["is_enabled"] == true).then(|| {
            // Claude Code counts credits in the currency's minor unit (cents).
            let unit = 10f64.powi(extra["decimal_places"].as_i64().unwrap_or(2) as i32);
            Credits {
                spent: extra["used_credits"].as_f64().unwrap_or(0.0) / unit,
                limit: extra["monthly_limit"].as_f64().map(|limit| limit / unit),
                currency: extra["currency"].as_str().unwrap_or("USD").to_owned(),
                used: percent(&extra["utilization"]),
            }
        });
        let behaviors = &answer["behaviors"];
        Some(Usage {
            plan: answer["subscription_type"].as_str().map(str::to_owned),
            windows,
            credits,
            day: share(&behaviors["day"]),
            week: share(&behaviors["week"]),
            totals,
        })
    }
}

fn share(window: &Value) -> Option<Share> {
    if !window.is_object() {
        return None;
    }
    let named = |key: &str| -> Vec<(String, u64)> {
        window[key].as_array().into_iter().flatten().filter_map(|item| Some((item["name"].as_str()?.to_owned(), percent(&item["pct"])?))).collect()
    };
    let tips = window["recommendations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tip| Some((percent(&tip["pct"])?, tip["title"].as_str()?.to_owned(), tip["detail"].as_str().unwrap_or_default().to_owned())))
        .collect();
    Some(Share { tips, skills: named("skills"), plugins: named("plugins"), mcp_servers: named("mcp_servers"), agents: named("agents") })
}

/// How the context window is spent: the model, the total and each category.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContextDetail {
    pub model: String,
    pub used: u64,
    pub most: u64,
    /// (name, tokens) of what fills the window, then the compaction reserve and the free space, in Claude Code's order.
    pub categories: Vec<(String, u64, Kind)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Used,
    Free,
    Buffer,
}

impl ContextDetail {
    pub fn parse(answer: &Value) -> Option<ContextDetail> {
        let categories = answer["categories"]
            .as_array()?
            .iter()
            .filter_map(|category| {
                // Deferred rows aren't in the window, so they take none of it.
                let kind = match category["kind"].as_str() {
                    Some("used") => Kind::Used,
                    Some("free") => Kind::Free,
                    Some("buffer") => Kind::Buffer,
                    _ => return None,
                };
                Some((category["name"].as_str()?.to_owned(), category["tokens"].as_u64()?, kind))
            })
            .collect();
        Some(ContextDetail { model: answer["model"].as_str().unwrap_or_default().to_owned(), used: answer["totalTokens"].as_u64()?, most: answer["maxTokens"].as_u64()?, categories })
    }
}
