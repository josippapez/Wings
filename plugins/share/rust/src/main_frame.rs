//! The main script: it answers the tools, asks the user to allow each port, puts a badge on the pane of a program that is
//! shared, and takes a link down when its port has closed.

use crate::{
    ASK, DONE, STATE, WANT, approvals,
    shares::{self, Share},
    tools,
    tunnel::{self, Fail},
};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::{BTreeSet, HashMap},
};
use wings_plugin::{
    host, messages,
    task::{SingleFlight, spawn},
};

/// A port that closes takes its link down within two of these, and only looks are made while something is shared.
const POLL_MS: u32 = 15_000;
/// A badge has buttons for each of its first three links.
const MAX_ACTIONS: usize = 3;

#[derive(Default)]
pub struct State {
    pub panes: Vec<String>,
    /// For each link, the looks in a row its port was gone.
    pub gone: HashMap<String, u32>,
    /// The pane Claude asked from when opentunnel wasn't installed, which shows the Install badge.
    pub install_pane: Option<String>,
}

thread_local! {
    static STATE_OF: RefCell<State> = RefCell::new(State::default());
    /// Looks at the ports. A call while a look is running makes it look once more when it is done.
    static REFRESH: SingleFlight = SingleFlight::new(look);
    /// Whether `opentunnel` was missing the last time a tool ran it, so the modal offers to install it.
    static MISSING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Notes whether `opentunnel` is missing, and tells the modal when that changed.
pub fn set_missing(missing: bool) {
    if MISSING.with(|was| was.replace(missing)) != missing {
        changed();
    }
}

pub fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE_OF.with(|state| f(&mut state.borrow_mut()))
}

pub fn refresh() -> impl Future<Output = ()> {
    REFRESH.with(|flight| flight.call())
}

/// Tells the modal what waits for an answer and what is shared.
pub fn changed() {
    messages::tell(STATE, &state_message());
}

/// What the modal is told: the asks that wait, the links, and whether OpenTunnel is missing.
pub fn state_message() -> Value {
    let missing = MISSING.with(std::cell::Cell::get);
    json!({ "approvals": approvals::waiting(), "shares": shares::all().iter().map(Share::to_json).collect::<Vec<_>>(), "missing": missing })
}

/// The programs in Wings' terminals that listen on a local port, as (pane, port). None when Wings can't read ports.
pub async fn listening_ports() -> Option<Vec<(String, u16)>> {
    let ports = host::ports().await.ok()?;
    Some(ports.as_array().into_iter().flatten().filter_map(|port| Some((port["paneId"].as_str()?.to_owned(), u16::try_from(port["port"].as_u64()?).ok()?))).collect())
}

/// Removes the routes called `names`, and forgets the links of them that were Wings'. A route that is already gone counts
/// as removed. Gives the names that came down, or what stopped the first one that didn't.
pub async fn take_down(names: &[String]) -> Result<Vec<String>, Fail> {
    let mut ended = Vec::new();
    for name in names {
        match tunnel::run(tunnel::remove_args(name), tunnel::QUICK_MS).await {
            Ok(_) => {}
            Err(Fail::Failed(message)) if tunnel::route_gone(&message) => {}
            Err(fail) => {
                changed();
                return Err(fail);
            }
        }
        shares::forget(std::slice::from_ref(name));
        ended.push(name.clone());
    }
    changed();
    Ok(ended)
}

fn share_badge(list: &[&Share]) -> Value {
    let first = list[0];
    let mut actions = vec![
        json!({ "id": format!("open:{}", first.name), "label": "Open", "primary": true }),
        json!({ "id": "links", "label": "Copy link" }),
        json!({ "id": format!("stop:{}", first.name), "label": "Stop sharing" }),
    ];
    for share in list.iter().take(MAX_ACTIONS).skip(1) {
        actions.push(json!({ "id": format!("open:{}", share.name), "label": format!("Open :{}", share.port) }));
        actions.push(json!({ "id": format!("stop:{}", share.name), "label": format!("Stop :{}", share.port) }));
    }
    json!({
        "label": if list.len() > 1 { format!("Shared :{} +{}", first.port, list.len() - 1) } else { format!("Shared :{}", first.port) },
        "tone": "warning",
        "title": "Anyone with the link can open this",
        "rows": list.iter().take(10).map(|share| json!({ "label": format!(":{}", share.port), "value": share.url })).collect::<Vec<_>>(),
        "actions": actions,
    })
}

fn install_badge() -> Value {
    json!({
        "label": "Install OpenTunnel",
        "tone": "warning",
        "title": "OpenTunnel isn't installed",
        "subtitle": "Share needs it to make a public link.",
        "actions": [{ "id": "install", "label": "Install", "primary": true }],
    })
}

/// Takes down the links whose port closed, and sets the badge of every open pane: the ones whose program is shared get one,
/// the others lose theirs. A badge a pane already shows isn't sent again, and Wings drops the badges of panes that close.
pub async fn look() {
    let listening = listening_ports().await;
    // Wings couldn't read the ports this time: nothing is known, so nothing comes down.
    if let Some(open) = &listening {
        let ports: BTreeSet<u16> = open.iter().map(|(_, port)| *port).collect();
        let plan = with(|state| shares::plan(&shares::all(), &ports, &mut state.gone));
        if !plan.seen.is_empty() {
            shares::mark_seen(&plan.seen);
        }
        // One that can't come down now is asked for again at the next look.
        if !plan.remove.is_empty() {
            drop(take_down(&plan.remove).await);
        }
    }
    let (list, panes, install) = (shares::all(), with(|state| state.panes.clone()), with(|state| state.install_pane.clone()));
    let open = listening.unwrap_or_default();
    for pane in panes {
        let mut here: Vec<&Share> = list.iter().filter(|share| open.iter().any(|(owner, port)| *owner == pane && *port == share.port)).collect();
        here.sort_by_key(|share| share.port);
        let badge = if !here.is_empty() {
            share_badge(&here)
        } else if install.as_deref() == Some(pane.as_str()) {
            install_badge()
        } else {
            Value::Null
        };
        drop(host::set_badge(&pane, &badge).await);
    }
}

async fn on_action(action: Value) -> Result<(), String> {
    let id = action["actionId"].as_str().unwrap_or_default();
    let (kind, name) = id.split_once(':').unwrap_or((id, ""));
    // A click can come from a badge a few seconds old: only act on a link that is still there.
    let share = shares::all().into_iter().find(|share| share.name == name);
    match (kind, share) {
        ("open", Some(share)) => host::call("openUrl", json!({ "url": share.url })).await.map(drop),
        ("stop", Some(share)) => {
            take_down(&[share.name]).await.map_err(|fail| fail.message())?;
            drop(refresh());
            Ok(())
        }
        ("open" | "stop", None) => Err("That link is already gone.".into()),
        ("links", _) => {
            approvals::opened_by_user();
            host::open_view_with(approvals::VIEW, "modal", &Value::Null).await.map(drop)
        }
        ("install", _) => install().await,
        _ => Err("Unknown action".into()),
    }
}

/// Install, on the badge or in the modal: runs the Homebrew command in a terminal tab, where you see it work.
async fn install() -> Result<(), String> {
    host::call("openPane", json!({ "command": tunnel::INSTALL, "args": [], "placement": "tab" })).await?;
    with(|state| state.install_pane = None);
    set_missing(false);
    drop(refresh());
    Ok(())
}

/// What the modal asks for: the user's answer to an ask, a link to stop, or OpenTunnel to install.
async fn serve(ask: Value) {
    let result = match ask["op"].as_str().unwrap_or_default() {
        "install" => install().await,
        "answer" => approvals::answer(ask["approval"].as_str().unwrap_or_default(), ask["allow"] == true),
        "stop" => {
            let name = ask["name"].as_str().unwrap_or_default().to_owned();
            if shares::all().iter().any(|share| share.name == name) {
                let done = take_down(&[name]).await.map(drop).map_err(|fail| fail.message());
                drop(refresh());
                done
            } else {
                Err("That link is already gone.".into())
            }
        }
        other => Err(format!("Share's main doesn't do {other}.")),
    };
    messages::tell(DONE, &json!({ "id": ask["id"], "error": result.err() }));
}

#[unsafe(no_mangle)]
pub fn start() {
    messages::on(ASK, |ask| spawn(serve(ask.clone())));
    messages::on(WANT, |_| changed());
    tools::register_all();
    host::on_panes(|list| {
        with(|state| state.panes = list.iter().filter_map(|pane| pane["paneId"].as_str().map(str::to_owned)).collect());
        if busy() {
            drop(refresh());
        }
    });
    host::on_action(on_action);
    // Not paused while Wings is in the background: a link has to come down when its program stops, whoever is looking.
    spawn(async {
        loop {
            host::sleep(POLL_MS).await;
            if busy() {
                refresh().await;
            }
        }
    });
}

/// Whether anything needs looking at: a link, or an Install badge.
fn busy() -> bool {
    !shares::all().is_empty() || with(|state| state.install_pane.is_some())
}
