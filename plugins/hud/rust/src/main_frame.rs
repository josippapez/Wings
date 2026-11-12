//! The main script: follows the Claude sessions in panes, draws each one's strip, reads the limits and the memory, and
//! tells the sidebar what to show.
//!
//! The usage limits, and the context and cache Claude Code itself reports, come from `wings.statusline()`, once
//! `wings statusline` is Claude Code's status line. Without it the context is worked out from the transcript the same
//! way Claude Code does. MCP servers come from the tool lists Claude Code records as it connects to them.

use crate::{
    LIMIT_CRITICAL, LIMIT_WARN, SNAPSHOT, WANT,
    format::MINUTE,
    session::{Session, ingest, session, start_run},
    view::{limits, pane_status, snapshot, state},
};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::HashMap};
use wings_plugin::{
    host::{self, storage},
    messages, poll,
    sessions::{self, Handlers, Sessions},
    task,
};

/// Claude's replies hold the skills and MCP tools it used.
const REPLIES: u32 = 500;
/// Tool results are most of a transcript, and only the newest show whether an MCP server's calls fail.
const RESULTS: u32 = 100;
/// How often the limits are read while you're in Wings. A status line with `refreshInterval` reports this often.
const STATUS_EVERY: u32 = 5_000;

#[derive(Default)]
struct Main {
    follow: Option<Sessions<Session>>,
    /// A draw of the strips and the sidebar is on its way.
    drawing: bool,
    /// Which wait for a reply's status line is the live one.
    status_wait: u64,
    /// The last status read, so a check that finds nothing new draws nothing.
    last_status: String,
    total_memory: Option<f64>,
    /// The highest level each account's limit windows have alerted at, so a refresh doesn't repeat it. A new resetsAt
    /// is a new window. Each Claude Code account has limits of its own.
    alerted: HashMap<String, (Option<f64>, f64)>,
}

thread_local! {
    static MAIN: RefCell<Main> = RefCell::new(Main::default());
}

fn main<R>(f: impl FnOnce(&mut Main) -> R) -> R {
    MAIN.with(|main| f(&mut main.borrow_mut()))
}

fn tell_sidebar() {
    let Some(follow) = main(|m| m.follow.clone()) else {
        return;
    };
    messages::tell(SNAPSHOT, &follow.all(|sessions| state(|state| snapshot(state, sessions))));
}

/// Changes come in bursts while Claude works, so the strips and the sidebar are drawn at most every 300 ms. Wings
/// skips a strip that is unchanged, so each pass can go through every session. A run whose backlog isn't in has
/// nothing to show: its strip stays as it was, and this runs again when the backlog lands.
fn changed() {
    if main(|m| std::mem::replace(&mut m.drawing, true)) {
        return;
    }
    task::spawn(async {
        host::sleep(300).await;
        main(|m| m.drawing = false);
        let Some(follow) = main(|m| m.follow.clone()) else {
            return;
        };
        let now = host::now();
        let strips = follow.each(|s, loaded| loaded.then(|| (s.pane_id.clone(), state(|state| pane_status(state, s, now)))));
        for (pane, status) in strips.into_iter().flatten() {
            task::spawn(async move { drop(host::set_pane_status(&pane, &status).await) });
        }
        tell_sidebar();
    });
}

fn alert_limits() {
    let all = state(|state| limits(state, host::now()));
    for a in &all {
        for (key, name) in [("fiveHour", "5-hour"), ("sevenDay", "weekly")] {
            let l = &a[key];
            let Some(percent) = l["percent"].as_f64() else {
                continue;
            };
            let resets_at = l["resetsAt"].as_f64();
            let level = if percent >= LIMIT_CRITICAL {
                LIMIT_CRITICAL
            } else if percent >= LIMIT_WARN {
                LIMIT_WARN
            } else {
                0.0
            };
            let risen = main(|m| {
                let seen = m.alerted.entry(format!("{}:{key}", a["account"].as_str().unwrap_or_default())).or_insert((resets_at, 0.0));
                if seen.0 != resets_at {
                    *seen = (resets_at, 0.0);
                }
                let risen = level > seen.1;
                if risen {
                    seen.1 = level;
                }
                risen
            });
            if !risen {
                continue;
            }
            let account = if all.len() > 1 { format!(" ({} account)", a["account"].as_str().unwrap_or("default")) } else { String::new() };
            let body = l["resets"].as_str().map(|resets| format!("Resets in {resets}")).unwrap_or_default();
            drop(host::notify(&format!("{percent}% of your {name} Claude limit used{account}"), &body));
        }
    }
}

/// Without `redraw`, only a status that changed is drawn.
fn refresh_status(redraw: bool) {
    task::spawn(async move {
        match host::statusline().await {
            Ok(reported) => {
                let text = reported.to_string();
                if !redraw && main(|m| m.last_status == text) {
                    return;
                }
                main(|m| m.last_status = text);
                let fresh = state(|state| {
                    let limits = reported["rateLimits"].as_array().filter(|limits| !limits.is_empty()).cloned();
                    state.reported = Some(reported);
                    limits.filter(|limits| *limits != state.saved_limits).inspect(|limits| state.saved_limits = limits.clone())
                });
                if let Some(limits) = fresh {
                    let _ = storage::set("limits", &Value::Array(limits));
                }
            }
            Err(_) => {
                state(|state| state.reported = None);
                main(|m| m.last_status.clear());
            }
        }
        alert_limits();
        changed();
    });
}

/// The number after `label` on a line of `vm_stat`.
fn pages(out: &str, label: &str) -> Option<f64> {
    out.split_once(label)?.1.trim_start().split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
}

/// Active and wired pages, as claude-hud counts them: what macOS can't hand to another app right away.
fn refresh_memory() {
    task::spawn(async {
        let memory: Result<Option<(f64, f64)>, String> = async {
            let total = match main(|m| m.total_memory) {
                Some(total) => Some(total),
                None => host::exec("sysctl", &["-n", "hw.memsize"]).await?["stdout"].as_str().and_then(|out| out.trim().parse().ok()).filter(|total| *total > 0.0),
            };
            main(|m| m.total_memory = total);
            let stat = host::exec("vm_stat", &[]).await?;
            let out = stat["stdout"].as_str().unwrap_or_default();
            let (page, active, wired) = (pages(out, "page size of"), pages(out, "Pages active:"), pages(out, "Pages wired down:"));
            Ok(match (total, page, active, wired) {
                (Some(total), Some(page), Some(active), Some(wired)) if page > 0.0 && active > 0.0 && wired > 0.0 => Some((total.min((active + wired) * page), total)),
                _ => None,
            })
        }
        .await;
        // Not macOS, or vm_stat is missing: the HUD just leaves memory out.
        state(|state| state.memory = memory.unwrap_or(None));
        tell_sidebar();
    });
}

fn backlog(id: String) -> std::pin::Pin<Box<dyn Future<Output = Result<Vec<Value>, String>>>> {
    Box::pin(async move {
        // These attachment kinds are about 340 KB of a 44 MB transcript and tell what this run connected to, so all
        // of them; the conversation only from its end.
        let attachments = host::transcript(&id, &["attachment:model", "attachment:deferred_tools_delta", "attachment:invoked_skills"], None);
        let replies = host::transcript(&id, &["assistant"], Some(REPLIES));
        let results = host::transcript(&id, &["user"], Some(RESULTS));
        // Replies before results, so each tool result finds the call it answers.
        let mut entries = Vec::new();
        for part in [attachments.await?, replies.await?, results.await?] {
            entries.extend(part.as_array().cloned().unwrap_or_default());
        }
        Ok(entries)
    })
}

#[unsafe(no_mangle)]
pub fn start() {
    let follow = sessions::follow(Handlers {
        create: session,
        pane: |s, pane| {
            s.pane_id = pane["paneId"].as_str().unwrap_or_default().to_owned();
            s.project = pane["project"].as_str().unwrap_or_default().to_owned();
            s.name = pane["session"]["name"].as_str().map(str::to_owned);
            s.focused = pane["focused"] == true;
        },
        start_run,
        backlog,
        entry: |s, entry, live, this_run| {
            ingest(s, entry, this_run);
            // The status line runs right after each reply, so look for its numbers just after the reply lands.
            if live && entry["type"] == "assistant" {
                let wait = main(|m| {
                    m.status_wait += 1;
                    m.status_wait
                });
                task::spawn(async move {
                    host::sleep(1500).await;
                    if main(|m| m.status_wait == wait) {
                        refresh_status(true);
                    }
                });
            }
        },
        // A closed pane's strip goes with it; one that runs something else now is cleared here.
        gone: |_, pane, still_open| {
            if still_open {
                let pane = pane.to_owned();
                task::spawn(async move { drop(host::set_pane_status(&pane, &json!(null)).await) });
            }
        },
        changed,
    });
    main(|m| m.follow = Some(follow));
    // The sidebar opened: what there is to show, at once.
    messages::on(WANT, |_| tell_sidebar());

    // Saved before limits were kept per account, it is a single object: start over.
    if let Ok(Value::Array(saved)) = storage::get("limits") {
        state(|state| state.saved_limits = saved);
    }
    changed();
    refresh_status(true);
    refresh_memory();
    // The limits are read every 5 seconds while you're in Wings, and once a minute behind it, which also moves the
    // time to reset along. The status check also raises limit alerts, which matter most when you're in another app,
    // so it keeps running. Memory is only drawn for someone looking, and costs a `vm_stat` run, so it pauses while
    // Wings is in the background.
    task::spawn(async {
        loop {
            host::sleep(MINUTE as u32).await;
            refresh_status(true);
        }
    });
    poll::while_focused(STATUS_EVERY, false, || refresh_status(false));
    poll::while_focused(MINUTE as u32, false, refresh_memory);
}
