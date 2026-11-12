//! Dev servers: when a program in a terminal pane listens on a local port, the pane gets a `localhost:<port>` badge
//! with buttons to open it in the browser. The badge goes when the port closes. Wings finds the ports (`wings.ports`,
//! one process-tree walk and one `lsof`, about 20 ms), so this only polls.

use serde_json::{Value, json};
use std::{cell::RefCell, collections::HashMap};
use wings_plugin::{
    host, poll,
    task::{self, SingleFlight},
};

const POLL_MS: u32 = 15_000;
/// A server started in a pane takes a few seconds to listen: look again this long after the pane changes.
const SETTLE_MS: u32 = 4_000;
/// A pane's badge has a browser button for each of its first three ports.
const MAX_ACTION_PORTS: usize = 3;
const MINUTE: f64 = 60_000.0;

#[derive(Default)]
struct State {
    panes: Vec<String>,
    /// Pane id → the ports its badge has buttons for, as of the last look.
    buttons: HashMap<String, Vec<u64>>,
    last_check: f64,
    /// Which wait for a pane to settle is the live one.
    settling: u64,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
    /// Looks at the ports. A call while a look is running makes it look once more when it is done.
    static REFRESH: SingleFlight = SingleFlight::new(look);
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|state| f(&mut state.borrow_mut()))
}

fn refresh() -> impl Future<Output = ()> {
    REFRESH.with(|flight| flight.call())
}

/// `3 min`, `2 h 5 min`, `1 d 4 h`. Minutes are the finest it shows, so the badge changes once a minute.
fn uptime(started_at: f64, now: f64) -> String {
    if started_at == 0.0 {
        return String::new();
    }
    let minutes = ((now - started_at) / MINUTE).floor().max(0.0) as u64;
    let hours = minutes / 60;
    match (minutes, hours) {
        (0, _) => "just started".into(),
        (1..60, _) => format!("{minutes} min"),
        (_, 1..24) if minutes.is_multiple_of(60) => format!("{hours} h"),
        (_, 1..24) => format!("{hours} h {} min", minutes % 60),
        _ if hours.is_multiple_of(24) => format!("{} d", hours / 24),
        _ => format!("{} d {} h", hours / 24, hours % 24),
    }
}

fn address(port: u64) -> String {
    format!("http://localhost:{port}/")
}

/// The badge for one pane: its lowest port as the label, `+N` for the others, a row for each program, and a browser
/// button for each of the first three.
fn badge_for(list: &[Value], now: f64) -> Value {
    let port = |p: &Value| p["port"].as_u64().unwrap_or(0);
    let first = port(&list[0]);
    let mut actions = vec![json!({ "id": format!("browser:{first}"), "label": "Open in browser", "primary": true })];
    for p in list.iter().take(MAX_ACTION_PORTS).skip(1) {
        actions.push(json!({ "id": format!("browser:{}", port(p)), "label": format!("Open :{} in browser", port(p)), "primary": false }));
    }
    actions.push(json!({ "id": "refresh", "label": "Refresh", "primary": false }));
    let rows: Vec<Value> = list
        .iter()
        .take(10)
        .map(|p| {
            let age = uptime(p["startedAt"].as_f64().unwrap_or(0.0), now);
            let up = if age.is_empty() { String::new() } else { format!(" · up {age}") };
            json!({ "label": p["name"], "value": format!(":{}{up}", port(p)) })
        })
        .collect();
    json!({
        "label": if list.len() > 1 { format!("localhost:{first} +{}", list.len() - 1) } else { format!("localhost:{first}") },
        "tone": "neutral",
        "title": if list.len() == 1 { "Listening on this computer".to_owned() } else { format!("Listening on {} ports", list.len()) },
        "rows": rows,
        "actions": actions,
    })
}

/// Sets the badge of every open pane: the ones that listen get one, the others lose theirs. A badge a pane already
/// shows isn't sent again, and Wings drops the badges of panes that close, so each look sets them all.
async fn look() {
    with(|s| s.last_check = host::now());
    // Wings couldn't read ports this time (no lsof): keep the badges as they are and look again on the next tick.
    let Ok(ports) = host::ports().await else {
        return;
    };
    let now = host::now();
    let mut by_pane: HashMap<String, Vec<Value>> = HashMap::new();
    for port in ports.as_array().into_iter().flatten() {
        by_pane.entry(port["paneId"].as_str().unwrap_or_default().to_owned()).or_default().push(port.clone());
    }
    for list in by_pane.values_mut() {
        list.sort_by_key(|p| p["port"].as_u64());
    }
    let panes = with(|s| {
        s.buttons = by_pane.iter().map(|(pane, list)| (pane.clone(), list.iter().take(MAX_ACTION_PORTS).filter_map(|p| p["port"].as_u64()).collect())).collect();
        s.panes.clone()
    });
    task::join_all(
        panes
            .into_iter()
            .map(|pane| {
                let badge = by_pane.get(&pane).map_or(Value::Null, |list| badge_for(list, now));
                Box::pin(async move { drop(host::set_badge(&pane, &badge).await) }) as std::pin::Pin<Box<dyn Future<Output = ()>>>
            })
            .collect(),
    )
    .await;
}

#[unsafe(no_mangle)]
pub fn start() {
    host::on_panes(|list| {
        let (stale, settling) = with(|s| {
            s.panes = list.iter().filter_map(|pane| pane["paneId"].as_str().map(str::to_owned)).collect();
            s.settling += 1;
            (host::now() - s.last_check > 1_000.0, s.settling)
        });
        // A new pane or a `cd` shouldn't wait for the next tick, but a burst of changes costs one lookup.
        if stale {
            drop(refresh());
        }
        task::spawn(async move {
            host::sleep(SETTLE_MS).await;
            if with(|s| s.settling) == settling {
                refresh().await;
            }
        });
    });
    // Paused while Wings is in the background: lsof once every 15 s for a badge nobody is looking at.
    poll::while_focused(POLL_MS, false, || {
        if with(|s| !s.panes.is_empty()) {
            drop(refresh());
        }
    });
    host::on_action(|action| async move {
        let (pane, id) = (action["paneId"].as_str().unwrap_or_default(), action["actionId"].as_str().unwrap_or_default());
        if id == "refresh" {
            refresh().await;
            return Ok(());
        }
        let (kind, port) = id.split_once(':').unwrap_or((id, ""));
        // A click can come from a badge a few seconds old: only open a port that had a button at the last look.
        let port = port.parse::<u64>().ok().filter(|port| with(|s| s.buttons.get(pane).is_some_and(|ports| ports.contains(port))));
        let Some(port) = port else {
            return Err("That port isn't open any more.".into());
        };
        if kind != "browser" {
            return Err("Unknown action".into());
        }
        host::call("openUrl", json!({ "url": address(port) })).await.map(drop)
    });
}
