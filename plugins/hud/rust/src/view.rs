//! What the strip and the sidebar show, worked out from the sessions and what Claude Code reports.

use crate::{
    CONTEXT_CRITICAL, CONTEXT_WARN, LIMIT_CRITICAL, LIMIT_WARN,
    format::{names, text, until},
    session::{Session, servers},
};
use serde_json::{Value, json};
use std::cell::RefCell;
use wings_plugin::{format::{clock, tokens}, host};

#[derive(Default)]
pub struct State {
    /// What `wings.statusline` last answered, or none when it failed.
    pub reported: Option<Value>,
    /// The last limits Claude Code reported, kept in storage so the strip shows them before this run's first reply.
    pub saved_limits: Vec<Value>,
    /// Bytes in use, and all there is.
    pub memory: Option<(f64, f64)>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

pub fn state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|state| f(&mut state.borrow_mut()))
}

struct Context {
    percent: f64,
    tokens: f64,
    window: f64,
    cache: Value,
}

/// Claude Code's own numbers once it has reported this run, else the same sum from the transcript.
fn context(state: &State, s: &Session) -> Option<Context> {
    let none = Value::Null;
    let live = state.reported.as_ref().map_or(&none, |reported| &reported["sessions"][&s.id]);
    let (used, window) = (live["usedPercentage"].as_f64(), live["contextWindowSize"].as_f64().filter(|window| *window > 0.0));
    if let (Some(used), Some(window)) = (used, window)
        && live["at"].as_f64().is_some_and(|at| at >= s.started_at)
    {
        let u = &live["currentUsage"];
        let count = |name: &str| u[name].as_f64().unwrap_or(0.0);
        let tokens = if u.is_object() { count("inputTokens") + count("cacheCreationInputTokens") + count("cacheReadInputTokens") } else { (used / 100.0 * window).round() };
        return Some(Context { percent: used.round(), tokens, window, cache: live["promptCache"].clone() });
    }
    let tokens = s.tokens?;
    let window = s.window.unwrap_or(200_000.0);
    Some(Context { percent: (tokens / window * 100.0).round().min(100.0), tokens, window, cache: Value::Null })
}

struct Limit {
    percent: f64,
    resets: Option<String>,
    left: Option<String>,
    resets_at: Option<f64>,
}

impl Limit {
    fn json(&self) -> Value {
        json!({ "percent": self.percent, "resets": self.resets, "left": self.left, "resetsAt": self.resets_at })
    }
}

/// A limit window, unless it has already reset: Claude Code reports the new one with the next reply.
fn limit(window: &Value, now: f64) -> Option<Limit> {
    if !window.is_object() {
        return None;
    }
    let resets_at = window["resetsAt"].as_f64().filter(|at| *at != 0.0);
    // Past its reset the window is empty until the next message starts a new one.
    if resets_at.is_some_and(|at| at * 1000.0 <= now) {
        return Some(Limit { percent: 0.0, resets: None, left: None, resets_at: None });
    }
    Some(Limit {
        percent: window["usedPercentage"].as_f64().unwrap_or(0.0).round(),
        resets: resets_at.map(|at| until(at, now, false)),
        left: resets_at.map(|at| until(at, now, true)),
        resets_at,
    })
}

fn cache_text(cache: &Value, now: f64) -> Option<String> {
    if !cache.is_object() {
        return None;
    }
    let expires = cache["expiresAt"].as_f64().filter(|at| cache["warm"] == true && at * 1000.0 > now);
    Some(expires.map_or_else(|| "Expired".to_owned(), |at| format!("Warm until {}", clock(at * 1000.0))))
}

/// Each account's limits, newest first: the last report, else the ones saved from an earlier run.
fn all_limits(state: &State) -> Vec<Value> {
    let reported = state.reported.as_ref().and_then(|reported| reported["rateLimits"].as_array()).filter(|limits| !limits.is_empty());
    reported.cloned().unwrap_or_else(|| state.saved_limits.clone())
}

fn tone(percent: f64, warn: f64, critical: f64) -> &'static str {
    if percent >= critical {
        "danger"
    } else if percent >= warn {
        "warning"
    } else {
        "success"
    }
}

/// Wings cuts nothing from a status, it rejects one over its limits, so row text is cut here.
const ROW_VALUE: usize = 200;

/// The strip under a Claude session's terminal, always shown: both usage limits (percent used, then time to reset),
/// the session's context, and a failed MCP server when there is one. The card on hover has the details, a click opens
/// the sidebar.
pub fn pane_status(state: &State, s: &Session, now: f64) -> Value {
    let ctx = context(state, s);
    // The limits of the account the session runs as, named when there is more than one. Before Claude Code has
    // reported the session, the newest account's.
    let all = all_limits(state);
    let account = state.reported.as_ref().and_then(|reported| reported["sessions"][&s.id].get("account"));
    let own = account.and_then(|account| all.iter().find(|l| l["account"] == *account)).or(all.first());
    let (five_hour, seven_day) = own.map_or((None, None), |l| (limit(&l["fiveHour"], now), limit(&l["sevenDay"], now)));
    let named = own.filter(|_| all.len() > 1).map(|l| l["account"].as_str().unwrap_or("default").to_owned());

    let all_servers = servers(s);
    let of = |state: &str| all_servers.iter().filter(|x| x["state"] == state).collect::<Vec<_>>();
    let (failed, on, sign_in) = (of("failed"), of("connected"), of("sign-in").len());
    let name = |server: &&Value| server["name"].as_str().unwrap_or_default().to_owned();
    let last_failed: Vec<String> = all_servers.iter().filter(|x| x["lastFailed"] == true).map(|x| name(&x)).collect();
    let cache = ctx.as_ref().and_then(|ctx| cache_text(&ctx.cache, now));
    // Green while there's room, then amber and red, so the line reads at a glance.
    let context_tone = ctx.as_ref().map(|ctx| tone(ctx.percent, CONTEXT_WARN, CONTEXT_CRITICAL));
    // `8% · 4h19m`: usage, then the time until that limit resets. A dash until Claude Code has reported the limit.
    let limit_item = |label: &str, l: &Option<Limit>| match l {
        Some(l) => {
            let left = l.left.as_ref().map(|left| format!(" · {left}")).unwrap_or_default();
            json!({ "label": label, "value": format!("{}%{left}", l.percent), "percent": l.percent.min(100.0), "tone": tone(l.percent, LIMIT_WARN, LIMIT_CRITICAL) })
        }
        None => json!({ "label": label, "value": "–" }),
    };
    let mut rows: Vec<Value> = Vec::new();
    for (label, l) in [("5-hour limit", &five_hour), ("Weekly limit", &seven_day)] {
        if let Some(l) = l {
            let resets = l.resets.as_ref().map(|resets| format!(", resets in {resets}")).unwrap_or_default();
            rows.push(json!({ "label": label, "value": format!("{}% used{resets}", l.percent), "tone": tone(l.percent, LIMIT_WARN, LIMIT_CRITICAL) }));
        }
    }
    if let Some(account) = &named {
        rows.push(json!({ "label": "Account", "value": account }));
    }
    if !statusline(state) {
        rows.push(json!({ "label": "Usage limits", "value": "After Claude Code's next reply" }));
    }
    rows.push(match &ctx {
        Some(ctx) => json!({ "label": "Context", "value": format!("{} of {} tokens ({}%)", tokens(ctx.tokens), tokens(ctx.window), ctx.percent), "tone": context_tone }),
        None => json!({ "label": "Context", "value": "No reply yet" }),
    });
    if let Some(cache) = cache {
        rows.push(json!({ "label": "Prompt cache", "value": cache }));
    }
    let mut counts = vec![format!("{} connected", on.len())];
    if !failed.is_empty() {
        counts.push(format!("{} failed", failed.len()));
    }
    if sign_in > 0 {
        counts.push(format!("{sign_in} need sign-in"));
    }
    rows.push(json!({ "label": "MCP servers", "value": counts.join(", ") }));
    for server in failed.iter().take(3) {
        let value = match server["error"].as_str().filter(|error| !error.is_empty()) {
            Some(error) => format!("{}: {error}", name(server)),
            None => name(server),
        };
        rows.push(json!({ "label": "Failed", "value": value, "tone": "danger" }));
    }
    if !last_failed.is_empty() {
        rows.push(json!({ "label": "Last call failed", "value": names(&last_failed, 3), "tone": "warning" }));
    }
    let used: Vec<String> = on.iter().filter(|x| x["used"] == true).map(name).collect();
    if !used.is_empty() {
        rows.push(json!({ "label": "MCP used", "value": names(&used, 4) }));
    }
    rows.push(json!({ "label": format!("Skills ({})", s.skills.len()), "value": if s.skills.is_empty() { "None used yet".to_owned() } else { names(&s.skills, 4) } }));
    for row in &mut rows {
        row["value"] = text(&row["value"], ROW_VALUE).into();
    }
    rows.truncate(12);

    let mut items = vec![limit_item("5h", &five_hour), limit_item("wk", &seven_day)];
    items.push(match &ctx {
        Some(ctx) => json!({ "label": "ctx", "value": format!("{}%", ctx.percent), "percent": ctx.percent.min(100.0), "tone": context_tone }),
        None => json!({ "label": "ctx", "value": "–" }),
    });
    if !failed.is_empty() {
        items.push(json!({ "label": "MCP", "value": format!("{} failed", failed.len()), "tone": "danger" }));
    }
    json!({ "items": items, "rows": rows, "open": "hud" })
}

/// Without the status line Wings can't get the limits, so the sidebar explains how to turn it on.
fn statusline(state: &State) -> bool {
    state.reported.as_ref().is_some_and(|reported| !all_limits(state).is_empty() || reported["sessions"].as_object().is_some_and(|sessions| !sessions.is_empty()))
}

/// Each account's limits with its name, null for the default one, as the sidebar and the alerts read them.
pub fn limits(state: &State, now: f64) -> Vec<Value> {
    all_limits(state)
        .iter()
        .map(|l| json!({ "account": l["account"], "fiveHour": limit(&l["fiveHour"], now).map(|l| l.json()), "sevenDay": limit(&l["sevenDay"], now).map(|l| l.json()) }))
        .collect()
}

/// What the sidebar shows: the account-wide part (usage limits and memory), then the sessions that run in panes,
/// sorted by project, kept well under the 64 KB a broadcast may carry.
pub fn snapshot(state: &State, sessions: &[&Session]) -> Value {
    let now = host::now();
    let mut shown = sessions.to_vec();
    shown.sort_by_key(|s| s.project.to_lowercase());
    let sessions: Vec<Value> = shown
        .into_iter()
        .take(8)
        .map(|s| {
            let ctx = context(state, s);
            let mut view = json!({
                "id": s.id,
                "project": s.project,
                "name": s.name,
                "focused": s.focused,
                "context": ctx.map(|ctx| json!({ "percent": ctx.percent, "tokens": ctx.tokens, "window": ctx.window, "cache": cache_text(&ctx.cache, now) })),
                "servers": servers(s).into_iter().take(80).collect::<Vec<_>>(),
                "skills": s.skills.iter().take(60).collect::<Vec<_>>(),
            });
            // Left out until Claude Code has reported the session.
            if let Some(account) = state.reported.as_ref().and_then(|reported| reported["sessions"][&s.id].get("account")) {
                view["account"] = account.clone();
            }
            view
        })
        .collect();
    let memory = state.memory.map(|(used, total)| json!({ "used": used, "total": total, "percent": (used / total * 100.0).round() }));
    json!({ "overview": { "limits": limits(state, now), "statusline": statusline(state), "memory": memory }, "sessions": sessions })
}

/// The session the sidebar shows: the one in the selected pane, which it follows from pane to pane. `followed` is the
/// last such session, kept while the selected pane runs no Claude. A session picked from the list shows in its place
/// until another pane is selected: `picked` is its id, and what was followed when it was picked.
pub fn shown<'a>(sessions: &'a [Value], followed: Option<&str>, picked: Option<&(String, Option<String>)>) -> Option<&'a Value> {
    let choice = picked.filter(|(_, over)| over.as_deref() == followed).map(|(id, _)| id.as_str()).or(followed);
    sessions.iter().find(|s| s["id"].as_str() == choice).or(sessions.first())
}

/// The accounts' limits with the shown session's account first, and whether that account has none: before a session's
/// account has reported its limits, another account's must not pass for them. `account` is none until Claude Code
/// has said which account the session runs as.
pub fn limits_for<'a>(limits: &'a [Value], account: Option<&Value>) -> (Vec<&'a Value>, bool) {
    let Some(account) = account else {
        return (limits.iter().collect(), false);
    };
    let (own, others): (Vec<&Value>, Vec<&Value>) = limits.iter().partition(|l| l["account"] == *account);
    let missing = own.is_empty() && !limits.is_empty();
    (own.into_iter().chain(others).collect(), missing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::session;

    #[test]
    fn the_sidebar_shows_the_session_of_the_selected_pane_and_one_picked_from_the_list_until_another_pane_is_selected() {
        let sessions = [json!({ "id": "a" }), json!({ "id": "b" }), json!({ "id": "c" })];
        let id = |shown: Option<&Value>| shown.map(|s| s["id"].as_str().unwrap().to_owned());
        // Nothing selected yet: the first.
        assert_eq!(id(shown(&sessions, None, None)).as_deref(), Some("a"));
        assert_eq!(id(shown(&sessions, Some("b"), None)).as_deref(), Some("b"));
        // Picked while b's pane was selected: it shows, until the selection moves on.
        let picked = ("c".to_owned(), Some("b".to_owned()));
        assert_eq!(id(shown(&sessions, Some("b"), Some(&picked))).as_deref(), Some("c"));
        assert_eq!(id(shown(&sessions, Some("a"), Some(&picked))).as_deref(), Some("a"));
        // The followed session's pane closed.
        assert_eq!(id(shown(&sessions, Some("gone"), None)).as_deref(), Some("a"));
        assert_eq!(shown(&[], Some("a"), None), None);
    }

    #[test]
    fn the_shown_sessions_accounts_limits_come_first_and_another_accounts_never_pass_for_its_own() {
        let limit = |account: Value| json!({ "account": account, "fiveHour": null, "sevenDay": null });
        let both = [limit(Value::Null), limit("work".into())];
        let accounts = |(limits, missing): (Vec<&Value>, bool)| (limits.iter().map(|l| l["account"].clone()).collect::<Vec<_>>(), missing);
        assert_eq!(accounts(limits_for(&both, Some(&"work".into()))), (vec!["work".into(), Value::Null], false));
        assert_eq!(accounts(limits_for(&both, Some(&Value::Null))), (vec![Value::Null, "work".into()], false));
        // Claude Code hasn't said which account the session runs as: the limits as they are.
        assert_eq!(accounts(limits_for(&both, None)), (vec![Value::Null, "work".into()], false));
        assert_eq!(accounts(limits_for(&both[..1], Some(&"work".into()))), (vec![Value::Null], true));
        assert_eq!(accounts(limits_for(&[], Some(&"work".into()))), (vec![], false));
    }

    #[test]
    fn the_snapshot_says_which_sessions_pane_is_selected_and_the_account_each_runs_as() {
        let (mut one, mut two) = (session("one"), session("two"));
        one.project = "a".into();
        two.project = "b".into();
        two.focused = true;
        let state = State { reported: Some(json!({ "rateLimits": [], "sessions": { "two": { "account": "work", "at": 0 } } })), ..State::default() };
        let views = snapshot(&state, &[&two, &one]);
        let said: Vec<_> = views["sessions"].as_array().unwrap().iter().map(|s| (s["id"].clone(), s["focused"].clone(), s.get("account").cloned())).collect();
        assert_eq!(said, [("one".into(), false.into(), None), ("two".into(), true.into(), Some("work".into()))]);
    }
}
