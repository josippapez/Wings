//! What a Browser page does for the agent tools: it tells main who and where it is, and runs what main asks (load an address,
//! run the JavaScript helper, take a picture, pop out). Main picks the page; the page does the work, because only a page
//! can draw a `WebView` and run its commands. The tools themselves are in `agent_tools.rs`, in main.
//!
//! Each agent has a group of tabs in the page, found by the key main sends with every op (`caller`). An op acts on that
//! group's tab, never on the person's tabs or another agent's, and whatever tab is shown. A call with no key (the CLI) works in
//! the person's own tab.

use crate::{
    agent_demo::{self, Recorder},
    agent_input,
    agent_js::{self, SNAPSHOT_MAX},
    agent_webmcp,
    address::resolve,
    browser_page::{self, tabs_changed, with},
    settings,
    protocol::{ANNOUNCE, WAIT_POLL_MS, WHO, cmd_name},
    wait::within,
};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::HashMap};
use wings_plugin::{
    host, messages,
    task::{Sender, oneshot, spawn},
    ui::{Node, WebInput, WebNetwork, WebState, bar, button, card, dot, row, small, stack, title_with_context},
};

/// How long a page waits for what the web view answers to a script or a picture.
const RESULT_MS: u32 = 30_000;
/// How often a wait looks again: the page's state changes by itself, a script has to be asked.
const LOAD_POLL_MS: u32 = 100;
/// A load that hasn't started this long after its command is taken to be none (back with nowhere to go).
const LOAD_START_MS: f64 = 1000.0;
const LOAD_MAX_MS: f64 = 20_000.0;

const NO_TAB: &str = "You have no Browser tab yet. Call browser__open with an address first.";

#[derive(Default)]
struct Agent {
    id: String,
    /// In a window of its own, which the page learns from the args it was opened with.
    popped: bool,
    project: String,
    /// What was last sent to main, so a change that doesn't matter to it isn't sent.
    sent: Option<Value>,
    /// The scripts and pictures the views are asked for, by token (one count for all the tabs).
    waiting: HashMap<u64, Sender<Result<Value, String>>>,
    /// The step the person was asked to do, the agent that asked, and who waits for them to say it is done.
    handoff: Option<(String, String, Option<Sender<()>>)>,
    demo: Option<Demo>,
}

/// What the person was asked to show, while they show it.
struct Demo {
    message: String,
    /// The name of the agent that asked, which the banner gives.
    agent: String,
    /// The steps recorded so far, which the banner counts.
    steps: usize,
    /// Done (true) or Cancel (false) pressed.
    ended: Option<bool>,
}

thread_local! {
    static AGENT: RefCell<Agent> = RefCell::new(Agent::default());
}

fn agent<R>(change: impl FnOnce(&mut Agent) -> R) -> R {
    AGENT.with(|agent| change(&mut agent.borrow_mut()))
}

/// A page's id: the clock to the microsecond when it began, with the address and project it began in. Pages restored at launch
/// start in the same millisecond, so the clock alone isn't enough.
fn id_for(url: &str) -> String {
    let hash = url.bytes().fold(host::now().to_bits(), |hash, byte| hash.rotate_left(5) ^ u64::from(byte)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    format!("p{:x}", hash >> 20)
}

pub fn start(args: Option<&Value>) {
    agent(|agent| {
        agent.id = id_for(args.and_then(|args| args["url"].as_str()).unwrap_or_default());
        agent.popped = args.is_some_and(|args| args["window"] == true);
    });
    let name = cmd_name(&agent(|agent| agent.id.clone()));
    messages::answer(&name, |op| async move { run(op).await });
    messages::on(WHO, |_| announce(true));
    host::on_focus(|_| announce(false));
    spawn(async {
        if let Ok(project) = host::project().await {
            agent(|agent| agent.project = project["id"].as_str().unwrap_or_default().to_owned());
        }
        announce(true);
    });
}

/// What a tab is as main wants it: who and where, and what it shows. Null for a tab that isn't there.
fn tab_status(tab: &str) -> Value {
    let (id, popped, project) = agent(|agent| (agent.id.clone(), agent.popped, agent.project.clone()));
    with(|page| {
        let Some(tab) = page.tabs.get(tab) else { return Value::Null };
        json!({
            "id": id, "project": project, "popped": popped, "tab": tab.id, "group": page.tabs.group_name(tab).unwrap_or_default(),
            "url": agent_url(&tab.state), "title": if tab.state.imported { "" } else { tab.state.title.as_str() },
            "loading": tab.state.loading, "canGoBack": tab.state.can_go_back, "canGoForward": tab.state.can_go_forward,
            "dialog": tab.state.dialog, "waiting": waiting_on(&tab.state).map(|(what, site)| json!({ "for": what, "site": site })), "blockedPopup": tab.blocked.as_deref().map(origin_of),
        })
    })
}

/// What the page waits on the person for, and the site: a certificate to choose about (the notice's buttons) or a name and a
/// password (Wings' sheet). No agent tool answers either or clicks through the page meanwhile.
fn waiting_on(state: &WebState) -> Option<(&'static str, String)> {
    match (&state.certificate, &state.auth) {
        (Some((site, _)), _) => Some(("certificate", site.clone())),
        (None, Some(site)) => Some(("auth", site.clone())),
        _ => None,
    }
}

/// The ops that only look at the page or move its window, which go on while it waits on the person.
const WHILE_WAITING: [&str; 8] = ["status", "screenshot", "wait_for", "handoff", "pop_out", "pop_in", "resize", "logs"];

/// This page as main wants it: who, where, which groups it has, and what the person's own tab shows.
fn status() -> Value {
    let mut now = with(|page| page.tabs.target("")).map_or(Value::Null, |tab| tab_status(&tab));
    if now.is_null() {
        let (id, popped, project) = agent(|agent| (agent.id.clone(), agent.popped, agent.project.clone()));
        now = json!({ "id": id, "project": project, "popped": popped });
    }
    now["groups"] = with(|page| page.tabs.groups.iter().map(|group| json!(group.key)).collect::<Vec<_>>()).into();
    now["focused"] = json!(host::focused());
    now
}

/// Tells main about this page, when it differs from the last time or when `force`d (main asked, or is new).
pub fn announce(force: bool) {
    let now = status();
    if agent(|agent| !force && agent.sent.as_ref() == Some(&now)) {
        return;
    }
    agent(|agent| agent.sent = Some(now.clone()));
    messages::tell(ANNOUNCE, &now);
}

/// A view's state changed.
pub fn changed() {
    announce(false);
}

/// A view's answer to a script or a picture, by its token.
pub fn result(event: &Value) {
    let Some(token) = event["token"].as_u64() else { return };
    let Some(sender) = agent(|agent| agent.waiting.remove(&token)) else { return };
    sender.send(match event["error"].as_str() {
        Some(error) => Err(error.to_owned()),
        None => Ok(event["value"].clone()),
    });
}

/// A tab closed with these commands unanswered: their waiters hear it now, not after the wait is out.
pub fn closed(tokens: &[u64]) {
    for token in tokens {
        result(&json!({ "token": token, "error": "The tab was closed before it answered." }));
    }
}

/// Sends a tab's view a command made from its token and waits for what it answers.
async fn command(tab: &str, make: impl FnOnce(u64) -> Value) -> Result<Value, String> {
    let (sender, receiver) = oneshot();
    let token = with(|page| page.tabs.run(tab, make)).ok_or("That tab is closed.")?;
    agent(|agent| agent.waiting.insert(token, sender));
    let answer = within(RESULT_MS, receiver).await;
    agent(|agent| agent.waiting.remove(&token));
    answer.ok_or("The page took too long to answer. It may be busy loading: wait_for it, then try again.")?.ok_or_else(|| "The page closed before it answered.".to_owned())?
}

/// Runs a call of the JavaScript helper in the tab's isolated world.
async fn helper(tab: &str, func: &str, args: &[Value]) -> Result<Value, String> {
    let script = agent_js::body(func, args);
    command(tab, |token| wings_plugin::ui::web_eval(&script, false, token)).await
}

/// What the app answers to real input on WebKit (`web::NEEDS_CHROMIUM`), and on a page that shows a password or card field
/// (`guard::SECRET_FIELDS`): the tool goes on with the helper, as it did before there was real input. WebKit gets no new
/// way to act: the helper is its fallback for reading and searching pages.
fn helper_instead(why: &str) -> bool {
    why.contains("needs the Chromium engine") || why.contains("password or card field")
}

/// Sends real input to the tab, or none when the page can't take it (`helper_instead`).
async fn real(tab: &str, events: Vec<WebInput>) -> Option<Result<Value, String>> {
    match command(tab, |token| wings_plugin::ui::web_input(&events, token)).await {
        Err(why) if helper_instead(&why) => None,
        other => Some(other),
    }
}

/// `click` or `hover` of a ref: the helper says where the element is (it is scrolled to first), and real input goes there.
/// What a real click would open a system picker on, or an element with nothing to click, goes to the helper.
async fn pointer(tab: &str, tool: &str, reference: &str) -> Result<Value, String> {
    let at = helper(tab, "locate", &[json!(reference)]).await?;
    if let (true, Some(x), Some(y)) = (at["plain"] == true, at["x"].as_f64(), at["y"].as_f64()) {
        let events = if tool == "hover" { agent_input::hover(x, y) } else { agent_input::click(x, y) };
        if let Some(done) = real(tab, events).await {
            return done.map(|_| json!(format!("{} {reference}", if tool == "hover" { "Hovered" } else { "Clicked" })));
        }
    }
    helper(tab, tool, &[json!(reference)]).await
}

/// `fill`: the field is focused and its content selected, then the value is typed over it, so the page sees the typing.
async fn fill(tab: &str, reference: &str, value: &str) -> Result<Value, String> {
    let field = helper(tab, "focus_all", &[json!(reference)]).await?;
    if field["plain"] == true
        && let Some(done) = real(tab, agent_input::replace(value)).await
    {
        return done.map(|_| json!(format!("Filled {reference}")));
    }
    helper(tab, "fill", &[json!(reference), json!(value)]).await
}

/// `type`: into what has the focus.
async fn type_text(tab: &str, typed: &str) -> Result<Value, String> {
    let events = agent_input::type_text(typed)?;
    helper(tab, "focused", &[]).await?;
    match real(tab, events).await {
        Some(done) => done.map(|_| json!(format!("Typed {} characters", typed.chars().count()))),
        None => helper(tab, "type", &[json!(typed)]).await,
    }
}

/// `press`: a key the table knows is real input; any other goes to the helper.
async fn press(tab: &str, combo: &str) -> Result<Value, String> {
    if let Ok(events) = agent_input::press(combo)
        && let Some(done) = real(tab, events).await
    {
        return done.map(|_| json!(format!("Pressed {combo}")));
    }
    helper(tab, "press", &[json!(combo)]).await
}

/// `drag` from one ref to another with the pointer, unless either has nothing to point at or the source would start the
/// system's own drag (a link, an image, anything `draggable`), which the helper's page events do without.
async fn drag(tab: &str, from: &str, to: &str) -> Result<Value, String> {
    let (a, b) = (helper(tab, "locate", &[json!(from)]).await?, helper(tab, "locate", &[json!(to)]).await?);
    let point = |at: &Value| Some((at["x"].as_f64()?, at["y"].as_f64()?));
    if let (true, true, false, Some(start), Some(end)) = (a["plain"] == true, b["plain"] == true, a["drag"] == true, point(&a), point(&b))
        && let Some(done) = real(tab, agent_input::drag(start, end)).await
    {
        return done.map(|_| json!(format!("Dragged {from} to {to}")));
    }
    helper(tab, "drag", &[json!(from), json!(to)]).await
}

/// The page's requests, kept by Chromium; WebKit answers that it needs Chromium.
async fn network(tab: &str, op: &Value) -> Result<Value, String> {
    let wanted = WebNetwork {
        url: op["url"].as_str(),
        status: op["status"].as_str(),
        kind: op["type"].as_str(),
        since: op["since"].as_f64(),
        limit: op["limit"].as_u64().map(|limit| limit.min(500) as u32),
        detail: op["detail"].as_str(),
        body: op["body"] == true,
    };
    command(tab, |token| wings_plugin::ui::web_network(&wanted, token)).await
}

fn text<'a>(op: &'a Value, key: &str) -> Result<&'a str, String> {
    op[key].as_str().ok_or_else(|| format!("{key} is missing"))
}

/// Waits for the tab's view to start loading what a command asked for, and to finish it.
async fn loaded(tab: &str) {
    let updates = || with(|page| page.tabs.get(tab).map_or(0, |tab| tab.updates));
    let (before, begun) = (updates(), host::now());
    loop {
        host::sleep(LOAD_POLL_MS).await;
        let (changed, loading) = (updates() > before, with(|page| page.tabs.get(tab).is_some_and(|tab| tab.state.loading)));
        let waited = host::now() - begun;
        if (changed || waited > LOAD_START_MS) && !loading || waited > LOAD_MAX_MS {
            return;
        }
    }
}

/// The caller's key and name: an empty key for the person and the CLI, who have no group.
fn caller(op: &Value) -> (String, String) {
    let key = op["caller"]["key"].as_str().unwrap_or_default().to_owned();
    let name = op["caller"]["name"].as_str().filter(|name| !name.is_empty()).unwrap_or("Agent").to_owned();
    (key, name)
}

/// The tab a call from `key` works in, or the answer for one that has no tab yet.
fn target(key: &str) -> Result<String, String> {
    with(|page| page.tabs.target(key)).ok_or_else(|| NO_TAB.to_owned())
}

async fn navigate(op: &Value, key: &str, name: &str) -> Result<Value, String> {
    let tab = match (op["url"].as_str(), op["action"].as_str()) {
        (Some(url), None) => {
            let url = resolve(url, settings::engine().get()).ok_or("That isn't an address or words to search for.")?;
            let (new_tab, group) = (op["new_tab"] == true, (!key.is_empty()).then_some((key, name)));
            with(|page| {
                let known = if new_tab { None } else { page.tabs.target(key) };
                match known {
                    Some(tab) => {
                        page.tabs.send(&tab, "load", Some(&url));
                        if group.is_some() {
                            page.tabs.ensure_group(key, name);
                            page.tabs.follow(&tab);
                        }
                        tab
                    }
                    None => page.tabs.open(group, Some(&url)),
                }
            })
        }
        (None, Some(action @ ("back" | "forward" | "reload"))) => {
            let tab = target(key)?;
            with(|page| page.tabs.send(&tab, action, None));
            tab
        }
        _ => return Err("navigate takes a url, or an action of back, forward or reload, not both".into()),
    };
    with(|page| page.tabs.touch(&tab, host::now()));
    tabs_changed();
    loaded(&tab).await;
    Ok(tab_status(&tab))
}

async fn wait_for(tab: &str, op: &Value) -> Result<Value, String> {
    let (words, selector, url) = (op["text"].as_str(), op["selector"].as_str(), op["url"].as_str());
    if words.is_none() && selector.is_none() && url.is_none() {
        return Err("wait_for takes text, selector or url".into());
    }
    let limit = op["timeout_ms"].as_f64().unwrap_or(10_000.0);
    let begun = host::now();
    loop {
        if url.is_some_and(|url| with(|page| page.tabs.get(tab).is_some_and(|tab| agent_url(&tab.state).contains(url)))) {
            return Ok(tab_status(tab));
        }
        // A page that is loading has no document to ask yet, so a failed ask is "not yet".
        if (words.is_some() || selector.is_some()) && helper(tab, "has", &[json!({ "text": words, "selector": selector })]).await.is_ok_and(|found| found == true) {
            return Ok(tab_status(tab));
        }
        if host::now() - begun >= limit {
            let what = words.map(|w| format!("text {w:?}")).or(selector.map(|s| format!("selector {s}"))).or(url.map(|u| format!("an address with {u:?}")));
            return Err(format!("Timed out after {} s waiting for {}.", (limit / 1000.0).round(), what.unwrap_or_default()));
        }
        host::sleep(WAIT_POLL_MS).await;
    }
}

/// The page's address as an agent may hear it: on a site signed in with imported cookies only its origin, as the rest
/// (an account's name, a code in a redirect) is the person's.
fn agent_url(state: &wings_plugin::ui::WebState) -> String {
    if !state.imported {
        return state.url.clone();
    }
    let Some((scheme, rest)) = state.url.split_once("://") else { return String::new() };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    format!("{scheme}://{}", host.rsplit_once('@').map_or(host, |(_, host)| host))
}

/// The scheme and host of `url`, all an agent hears of a window a page opened by itself or a file it downloaded on a site
/// signed in with imported cookies.
fn origin_of(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else { return String::new() };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    format!("{scheme}://{}", host.rsplit_once('@').map_or(host, |(_, host)| host))
}

/// The caller's downloads, newest first: what each is, where it was saved, and how far it is.
fn downloads(key: &str) -> Value {
    with(|page| {
        let list = page.downloads.of(key).into_iter().map(|download| {
            // As its tab was when it began, or is now: a closed tab doesn't show the agent the whole address.
            let imported = download.imported || page.tabs.get(&download.tab).is_some_and(|tab| tab.state.imported);
            let event = &download.event;
            json!({
                "name": event.name, "path": event.path, "state": event.state, "received": event.received, "total": event.total,
                "error": event.error, "url": if imported { origin_of(&event.url) } else { event.url.clone() }, "tab": download.tab,
            })
        });
        Value::Array(list.collect())
    })
}

/// Closing the tab or window ends this frame, so the answer to main goes out first.
fn close_soon() {
    spawn(async {
        host::sleep(100).await;
        drop(host::close_tab().await);
    });
}

fn size_of(op: &Value) -> Option<(f64, f64)> {
    Some((op["width"].as_f64()?, op["height"].as_f64()?))
}

/// What the tab reopens with: the address it shows, or none for one that shows nothing yet, and its group.
fn reopen_args(tab: &str, extra: Value) -> Value {
    let (url, group) = with(|page| {
        let tab = page.tabs.get(tab);
        let group = tab.and_then(|tab| Some(json!({ "key": tab.group.as_deref()?, "name": page.tabs.group_name(tab)? })));
        (tab.map(|tab| tab.url().to_owned()).unwrap_or_default(), group)
    });
    let mut args = json!({ "url": url });
    if let Some(group) = group {
        args["group"] = group;
    }
    if let (Some(args), Some(extra)) = (args.as_object_mut(), extra.as_object()) {
        args.extend(extra.clone());
    }
    args
}

/// Opens the tab's address in a window of its own and takes it out of this page: when it was the page's last tab the Wings tab
/// closes too. The same cookie store keeps the person signed in; the back and forward history doesn't come along. The answer
/// says whether the page is gone, for main to know whether to look for it.
pub async fn pop_out(tab: &str, size: Option<(f64, f64)>) -> Result<Value, String> {
    if agent(|agent| agent.popped) {
        return Err("This page is already in a window of its own.".into());
    }
    let mut answer = host::open_window("browser", &reopen_args(tab, json!({ "window": true })), size).await?;
    let last = with(|page| page.tabs.list.len() <= 1);
    if last {
        close_soon();
    } else {
        browser_page::close_tab(tab);
    }
    match answer.as_object_mut() {
        Some(answer) => drop(answer.insert("closed".into(), json!(last))),
        None => answer = json!({ "closed": last }),
    }
    Ok(answer)
}

/// Opens the tab's address as a tab and closes the window.
pub async fn pop_in(tab: &str) -> Result<Value, String> {
    if !agent(|agent| agent.popped) {
        return Err("This page is in a tab already.".into());
    }
    host::open_view_with("browser", "tab", &reopen_args(tab, json!({}))).await?;
    close_soon();
    Ok(Value::Null)
}

async fn resize(size: Option<(f64, f64)>) -> Result<Value, String> {
    let (width, height) = size.ok_or("resize takes a width and a height")?;
    if !agent(|agent| agent.popped) {
        return Err("This page is in a tab, which has no window of its own to resize. Call pop_out first.".into());
    }
    host::resize_window(width, height).await
}

/// Asks the person for a step (a sign-in, a captcha) in a banner on the page, and waits for them to press Done. It needs the
/// person, so the agent's tab comes forward.
async fn handoff(tab: &str, op: &Value, name: &str) -> Result<Value, String> {
    let message = text(op, "message")?.to_owned();
    let (sender, receiver) = oneshot();
    with(|page| page.tabs.activate(tab));
    tabs_changed();
    agent(|agent| agent.handoff = Some((message, name.to_owned(), Some(sender))));
    let done = within(op["timeout_ms"].as_f64().unwrap_or(120_000.0) as u32, receiver).await;
    agent(|agent| agent.handoff = None);
    match done {
        Some(_) => Ok(tab_status(tab)),
        None => Err("The person didn't press Done in time. Ask them again, or carry on without that step.".into()),
    }
}

/// The caller's tabs, for `browser__tabs`.
fn tab_list(key: &str) -> Value {
    let (now, mine) = with(|page| (page.tabs.target(key), page.tabs.of(key).into_iter().map(|tab| (tab.id.clone(), agent_url(&tab.state), tab.state.title.clone(), page.tabs.group(key).map(|group| group.name.clone()))).collect::<Vec<_>>()));
    Value::Array(mine.into_iter().map(|(id, url, title, group)| json!({ "id": id, "url": url, "title": title, "active": now.as_deref() == Some(id.as_str()), "group": group })).collect())
}

/// Lists, selects or closes the caller's own tabs, never anyone else's.
fn tabs(op: &Value, key: &str) -> Result<Value, String> {
    if !key.is_empty() && with(|page| page.tabs.group(key).is_none()) {
        return Err(NO_TAB.into());
    }
    let own = |id: &str| with(|page| page.tabs.get(id).is_some_and(|tab| tab.group.as_deref().unwrap_or_default() == key));
    match op["action"].as_str().unwrap_or("list") {
        "list" => {}
        action @ ("select" | "close") => {
            let id = text(op, "tab").map_err(|_| format!("{action} takes the tab's id: browser__tabs list shows them."))?;
            if !own(id) {
                return Err(format!("You have no tab {id}: browser__tabs list shows yours."));
            }
            if action == "select" {
                with(|page| {
                    page.tabs.select(id);
                    page.tabs.touch(id, host::now());
                });
                tabs_changed();
            } else {
                browser_page::close_tab(id);
            }
        }
        other => return Err(format!("tabs takes an action of list, select or close, not {other}.")),
    }
    Ok(tab_list(key))
}

/// Runs one thing main asked.
pub async fn run(op: Value) -> Result<Value, String> {
    let (key, name) = caller(&op);
    let string = |key: &str| text(&op, key).map(|value| json!(value));
    let name_of = op["op"].as_str().unwrap_or_default();
    // Main's own look at the page has no caller; a tool's look is at the caller's tab.
    match name_of {
        "status" if op.get("caller").is_none() => return Ok(status()),
        "navigate" => return navigate(&op, &key, &name).await,
        "tabs" => return tabs(&op, &key),
        "downloads" => return Ok(downloads(&key)),
        _ => {}
    }
    let tab = target(&key)?;
    with(|page| page.tabs.touch(&tab, host::now()));
    let waiting = with(|page| page.tabs.get(&tab).and_then(|tab| waiting_on(&tab.state)));
    if let (Some((what, _)), false) = (&waiting, WHILE_WAITING.contains(&name_of)) {
        return Err(match *what {
            "certificate" => "The page waits for the person to choose about its certificate. Use handoff to ask them, and don't click through it.".into(),
            _ => "The page waits for the person to type a name and a password in the sheet over the window. Use handoff to ask them.".into(),
        });
    }
    match name_of {
        "status" => Ok(tab_status(&tab)),
        "screenshot" => {
            let mut shot = command(&tab, |token| wings_plugin::ui::web_snapshot(token)).await?;
            shot["url"] = json!(with(|page| page.tabs.get(&tab).map(|tab| agent_url(&tab.state)).unwrap_or_default()));
            Ok(shot)
        }
        "snapshot" => helper(&tab, "snapshot", &[json!(SNAPSHOT_MAX)]).await,
        "click" | "hover" => pointer(&tab, name_of, text(&op, "ref")?).await,
        "fill" => fill(&tab, text(&op, "ref")?, text(&op, "value")?).await,
        "type" => type_text(&tab, text(&op, "text")?).await,
        "press" => press(&tab, text(&op, "key")?).await,
        "select" => helper(&tab, "select", &[string("ref")?, op["values"].clone()]).await,
        "scroll" => helper(&tab, "scroll", &[json!({ "ref": op["ref"], "to": op["to"], "x": op["x"], "y": op["y"] })]).await,
        "drag" => drag(&tab, text(&op, "from")?, text(&op, "to")?).await,
        "network" => network(&tab, &op).await,
        "eval" => command(&tab, |token| wings_plugin::ui::web_eval(op["script"].as_str().unwrap_or_default(), true, token)).await,
        "logs" => command(&tab, |token| json!({ "action": "logs", "token": token })).await,
        "dialog" => {
            let accept = op["accept"].as_bool().ok_or("dialog takes accept: true for OK or false for Cancel")?;
            command(&tab, |token| json!({ "action": "dialog", "accept": accept, "text": op["text"], "token": token })).await
        }
        "wait_for" => wait_for(&tab, &op).await,
        "pop_out" => pop_out(&tab, size_of(&op)).await,
        "pop_in" => pop_in(&tab).await,
        "resize" => resize(size_of(&op)).await,
        "handoff" => handoff(&tab, &op, &name).await,
        "upload" => command(&tab, |token| json!({ "action": "upload", "ref": op["ref"], "selector": op["selector"], "files": op["files"], "token": token })).await,
        "screenshot_diff" => command(&tab, |token| json!({ "action": "screenshotDiff", "baseline": op["baseline"], "threshold": op["threshold"], "clip": op["clip"], "current": op["current"], "diff": op["diff"], "token": token })).await,
        "recording" => command(&tab, |token| json!({ "action": "record", "do": op["do"], "path": op["path"], "fps": op["fps"], "maxMs": op["maxMs"], "token": token })).await,
        "demonstrate" => demonstrate(&tab, &op, &name).await,
        "webmcp" => webmcp(&tab, &op).await,
        other => Err(format!("The page doesn't know {other}")),
    }
}

/// Done pressed on the banner.
fn done() {
    if let Some(sender) = agent(|agent| agent.handoff.as_mut().and_then(|(_, _, sender)| sender.take())) {
        sender.send(());
    }
}

/// Done (`send` true) or Cancel pressed on the banner of a demonstration.
fn demo_end(send: bool) {
    agent(|agent| {
        if let Some(demo) = agent.demo.as_mut() {
            demo.ended.get_or_insert(send);
        }
    });
}

/// The person shows the agent a flow in the tab, which comes forward with a banner and Done and Cancel buttons. What they do
/// is recorded (`agent_demo.rs`) by asking the tab's page for it twice a second, until they press one or the time is out.
async fn demonstrate(tab: &str, op: &Value, name: &str) -> Result<Value, String> {
    let message = text(op, "message")?.to_owned();
    if agent(|agent| agent.demo.is_some()) {
        return Err("The person is already showing something in this page: try again once they have pressed Done.".into());
    }
    let limit = op["timeout_ms"].as_f64().unwrap_or(600_000.0);
    let url_of = || with(|page| page.tabs.get(tab).map(|tab| agent_url(&tab.state)));
    let mut recorder = Recorder::new(&url_of().unwrap_or_default());
    with(|page| page.tabs.activate(tab));
    tabs_changed();
    agent(|agent| agent.demo = Some(Demo { message, agent: name.to_owned(), steps: 0, ended: None }));
    let begun = host::now();
    let (mut ended, mut refused) = (None, None);
    loop {
        host::sleep(agent_demo::POLL_MS).await;
        let Some(url) = url_of() else {
            ended = Some("the tab was closed");
            break;
        };
        let pressed = agent(|agent| agent.demo.as_ref().is_none_or(|demo| demo.ended.is_some()));
        let late = host::now() - begun >= limit;
        // The last ask also ends the recorder, so it stops listening in the page.
        let last = pressed || late;
        let script = agent_demo::script(last);
        match within(3000, command(tab, |token| wings_plugin::ui::web_eval(&script, false, token))).await {
            Some(Ok(events)) => recorder.events(&events),
            // A page with a password field runs no script, so its steps go unseen: the agent hears why.
            Some(Err(why)) => refused = Some(why),
            None => {}
        }
        recorder.navigated(&url);
        agent(|agent| agent.demo.iter_mut().for_each(|demo| demo.steps = recorder.steps().len()));
        with(|page| page.tabs.touch(tab, host::now()));
        if last {
            if !pressed {
                ended = Some("the time was out");
            }
            break;
        }
    }
    if agent(|agent| agent.demo.take().is_some_and(|demo| demo.ended == Some(false))) {
        return Err("The person pressed Cancel: they didn't show you this. Ask them what they'd like, or carry on another way.".into());
    }
    let mut answer = recorder.answer();
    if let Some(why) = ended {
        answer["endedBecause"] = json!(why);
    }
    if let Some(why) = refused {
        answer["notRecorded"] = json!(format!("Some steps in the page couldn't be recorded: {why}"));
    }
    Ok(answer)
}

/// The tools a page offers through WebMCP, or one run by name. The tab's isolated world is asked first, which is where a
/// browser's own WebMCP is; a page that defines `modelContext` itself (a polyfill) is only seen from its own world.
async fn webmcp(tab: &str, op: &Value) -> Result<Value, String> {
    let action = op["action"].as_str().unwrap_or("list");
    let name = op["name"].as_str().unwrap_or_default();
    let timeout = op["timeout"].as_u64().unwrap_or(agent_webmcp::TIMEOUT_MS);
    let script = agent_webmcp::script(action, name, &op["input"], op["frame"].as_str(), timeout);
    let ask = |page_world: bool| {
        let script = script.clone();
        command(tab, move |token| wings_plugin::ui::web_eval(&script, page_world, token))
    };
    let first = ask(false).await?;
    if !webmcp_wants_page_world(action, &first) {
        return Ok(first);
    }
    // A page that refuses the page world (a site signed in with imported cookies) leaves what the isolated world said.
    match ask(true).await {
        Ok(second) if !webmcp_wants_page_world(action, &second) || (action == "list" && second["available"] == true) => Ok(second),
        _ if action == "call" => Err(format!("No WebMCP tool named {name}: webmcp with action list shows what the page offers.")),
        _ => Ok(first),
    }
}

/// Whether the isolated world's answer is not the whole story: no WebMCP there, or no such tool.
fn webmcp_wants_page_world(action: &str, answer: &Value) -> bool {
    if action == "call" { answer["found"] != true } else { answer["tools"].as_array().is_none_or(|tools| tools.is_empty()) }
}

/// The banner over an agent's tab while it waits on the person: to show it a flow (`demonstrate`), which is recorded until
/// they press Done, or to do a step only they can (`handoff`). It says which agent asks, as its tab group is named.
pub fn handoff_banner() -> Option<Node> {
    // In a bar, which sets it in from the page's sides as the toolbar is: a row alone met the page's edge.
    let banner = |key: &'static str, tone: &str, title: &str, context: String, message: String, note: String, buttons: Vec<Node>| {
        let heading = row().prop("gap", "s").prop("align", "center").child(dot().prop("tone", tone)).child(title_with_context(title, &context));
        let words = stack().prop("grow", true).prop("gap", "xs").child(heading).child(wings_plugin::ui::text(message)).child(small(note).prop("tone", "muted"));
        bar("bottom", [card().child(row().prop("gap", "m").prop("align", "center").child(words).children(buttons))]).key(key)
    };
    let demo = agent(|agent| agent.demo.as_ref().filter(|demo| demo.ended.is_none()).map(|demo| (demo.message.clone(), demo.agent.clone(), demo.steps)));
    if let Some((message, name, steps)) = demo {
        let count = match steps {
            0 => "Nothing recorded yet.".to_owned(),
            1 => "1 step recorded.".to_owned(),
            n => format!("{n} steps recorded."),
        };
        let note = format!("{count} Do it in the page below, then press Done. Passwords are never recorded.");
        let buttons = vec![button("Cancel", "ghost", false, || demo_end(false)), button("Done", "default", false, || demo_end(true))];
        return Some(banner("demo", "danger", "Recording your steps", format!("for {name}"), message, note, buttons));
    }
    let (message, name) = agent(|agent| agent.handoff.as_ref().map(|(message, name, _)| (message.clone(), name.clone())))?;
    let note = "Do it in the page below, then press Done.".to_owned();
    Some(banner("handoff", "warn", "Your turn", format!("{name} is waiting"), message, note, vec![button("Done", "default", false, done)]))
}

/// Pop out, or put back when this page is a window: its toolbar button, for the tab shown.
pub fn pop_button_action() {
    spawn(async {
        let (popped, tab) = (agent(|agent| agent.popped), with(|page| page.active_id()));
        if let Err(error) = if popped { pop_in(&tab).await } else { pop_out(&tab, None).await } {
            browser_page::problem(error);
        }
    });
}

pub fn popped() -> bool {
    agent(|agent| agent.popped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};
    use wings_plugin::host::testing;

    fn record() -> Rc<RefCell<Vec<(String, Value)>>> {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let kept = calls.clone();
        testing::answer(move |method, params| {
            kept.borrow_mut().push((method.to_owned(), params.clone()));
            Ok(if method == "openView" { json!({ "width": 390, "height": 844 }) } else { Value::Null })
        });
        calls
    }

    fn run_op(op: Value) -> Rc<RefCell<Option<Result<Value, String>>>> {
        let out = Rc::new(RefCell::new(None));
        let slot = out.clone();
        spawn(async move { *slot.borrow_mut() = Some(run(op).await) });
        testing::settle();
        out
    }

    fn shown(url: &str, popped: bool) {
        with(|page| page.tabs.active_mut().state.url = url.into());
        agent(|agent| agent.popped = popped);
    }

    #[test]
    fn popping_out_opens_the_same_address_in_a_window_of_that_size_and_only_then_closes_the_tab() {
        let calls = record();
        shown("https://a.com/x", false);
        let out = run_op(json!({ "op": "pop_out", "width": 390.0, "height": 844.0 }));
        assert_eq!(out.borrow().clone(), Some(Ok(json!({ "width": 390, "height": 844, "closed": true }))));
        assert_eq!(*calls.borrow(), [("openView".to_owned(), json!({ "id": "browser", "as": "window", "args": { "url": "https://a.com/x", "window": true }, "newTab": false, "width": 390.0, "height": 844.0 }))]);
        // The answer is out before the tab goes, which ends the frame.
        testing::fire_timers();
        testing::settle();
        assert_eq!(calls.borrow().last().map(|call| call.0.as_str()), Some("closeTab"));
    }

    #[test]
    fn putting_back_opens_a_tab_with_the_address_then_closes_the_window_and_a_tab_cant_be_put_back() {
        let calls = record();
        shown("https://a.com/x", true);
        assert_eq!(run_op(json!({ "op": "pop_in" })).borrow().clone(), Some(Ok(Value::Null)));
        assert_eq!(calls.borrow()[0], ("openView".to_owned(), json!({ "id": "browser", "as": "tab", "args": { "url": "https://a.com/x" }, "newTab": false })));
        testing::fire_timers();
        testing::settle();
        assert_eq!(calls.borrow().last().map(|call| call.0.as_str()), Some("closeTab"));
        assert!(run_op(json!({ "op": "pop_out" })).borrow().clone().unwrap().unwrap_err().contains("already in a window"));
        shown("https://a.com/x", false);
        assert!(run_op(json!({ "op": "pop_in" })).borrow().clone().unwrap().is_err());
    }

    #[test]
    fn resize_sizes_a_window_and_says_a_tab_has_none() {
        let calls = record();
        shown("https://a.com/", false);
        assert!(run_op(json!({ "op": "resize", "width": 500.0, "height": 600.0 })).borrow().clone().unwrap().unwrap_err().contains("pop_out"));
        shown("https://a.com/", true);
        run_op(json!({ "op": "resize", "width": 500.0, "height": 600.0 }));
        assert_eq!(*calls.borrow(), [("resizeWindow".to_owned(), json!({ "width": 500.0, "height": 600.0 }))]);
    }

    #[test]
    fn an_agent_hears_only_the_origin_and_no_title_of_a_page_signed_in_with_imported_cookies() {
        let state = |imported: bool| wings_plugin::ui::WebState {
            url: "https://me@mail.example.com:8443/u/0/inbox?code=secret".into(),
            title: "Inbox (3) - ana@example.com".into(),
            imported,
            ..Default::default()
        };
        assert_eq!(agent_url(&state(true)), "https://mail.example.com:8443");
        assert_eq!(agent_url(&state(false)), state(false).url);
        record();
        with(|page| page.tabs.active_mut().state = state(true));
        let told = status();
        assert_eq!((told["url"].as_str(), told["title"].as_str()), (Some("https://mail.example.com:8443"), Some("")));
    }

    #[test]
    fn a_script_runs_as_a_command_with_a_token_and_its_result_comes_back_by_that_token() {
        record();
        let out = run_op(json!({ "op": "eval", "script": "return 1" }));
        let command = with(|page| page.tabs.active_tab().commands.last().cloned()).unwrap();
        assert_eq!(command["action"], "eval");
        assert_eq!(command["world"], "page");
        assert!(out.borrow().is_none(), "waiting for the view");
        result(&json!({ "token": command["token"], "value": 1 }));
        testing::settle();
        assert_eq!(out.borrow().clone(), Some(Ok(json!(1))));
        // The helper's calls run in the isolated world, and a failure is the view's error: a click asks where the element is.
        let out = run_op(json!({ "op": "click", "ref": "e1" }));
        let command = with(|page| page.tabs.active_tab().commands.last().cloned()).unwrap();
        assert!(command.get("world").is_none() && command["script"].as_str().unwrap().ends_with("return w.locate(\"e1\");"));
        result(&json!({ "token": command["token"], "error": "Ref e1 is stale" }));
        testing::settle();
        assert_eq!(out.borrow().clone(), Some(Err("Ref e1 is stale".into())));
    }

    #[test]
    fn an_op_without_what_it_needs_or_unknown_is_an_error_and_a_navigation_takes_one_of_url_or_action() {
        record();
        assert!(run_op(json!({ "op": "click" })).borrow().clone().unwrap().is_err());
        assert!(run_op(json!({ "op": "nope" })).borrow().clone().unwrap().unwrap_err().contains("doesn't know"));
        assert!(run_op(json!({ "op": "navigate", "url": "a.com", "action": "back" })).borrow().clone().unwrap().is_err());
        assert!(run_op(json!({ "op": "wait_for" })).borrow().clone().unwrap().is_err());
    }

    #[test]
    fn a_handoff_shows_the_banner_until_done_is_pressed_and_gives_up_when_time_is_out() {
        record();
        let out = run_op(json!({ "op": "handoff", "message": "Sign in", "timeout_ms": 5000 }));
        assert!(handoff_banner().is_some());
        done();
        testing::settle();
        assert!(out.borrow().clone().unwrap().is_ok() && handoff_banner().is_none());
        let out = run_op(json!({ "op": "handoff", "message": "Sign in", "timeout_ms": 5000 }));
        testing::fire_timers();
        testing::settle();
        assert!(out.borrow().clone().unwrap().unwrap_err().contains("didn't press Done"));
    }

    fn caller_for(key: &str, name: &str) -> Value {
        json!({ "key": key, "name": name })
    }

    /// Runs `op` as `key` and lets the load it starts finish, as the view's state events and the clock would.
    fn run_loaded(op: Value) -> Result<Value, String> {
        let out = run_op(op);
        testing::set_now(host::now() + 2000.0);
        testing::fire_timers();
        testing::settle();
        let done = out.borrow().clone();
        done.expect("the op finished")
    }

    fn navigate_as(key: &str, name: &str, url: &str, new_tab: bool) -> Result<Value, String> {
        run_loaded(json!({ "op": "navigate", "url": url, "new_tab": new_tab, "caller": caller_for(key, name) }))
    }

    /// The person's tab t1 is showing a page.
    fn person_at(url: &str) {
        with(|page| {
            *page = browser_page::Page::default();
            page.tabs.send("t1", "load", Some(url));
            page.tabs.active_mut().state.url = url.into();
        });
    }

    #[test]
    fn two_agents_each_get_a_tab_group_of_their_own_and_the_persons_tab_is_left_alone() {
        record();
        person_at("https://mine.com/");
        let a = navigate_as("ses-a", "Fix login bug", "https://a.com/", false).unwrap();
        let b = navigate_as("ses-b", "Write docs", "https://b.com/", false).unwrap();
        assert_eq!((a["tab"].as_str(), a["group"].as_str(), b["tab"].as_str(), b["group"].as_str()), (Some("t2"), Some("Fix login bug"), Some("t3"), Some("Write docs")));
        assert_eq!(with(|page| page.tabs.active.clone()), "t1", "the person still sees their own tab");
        // Another address for A goes to A's tab, not B's and not the person's.
        navigate_as("ses-a", "Fix login bug", "https://a.com/next", false).unwrap();
        let starts = with(|page| page.tabs.list.iter().map(|tab| (tab.id.clone(), tab.start.clone(), tab.commands.last().map(|command| command["url"].clone()))).collect::<Vec<_>>());
        assert_eq!(starts[0], ("t1".into(), "https://mine.com/".into(), None));
        assert_eq!(starts[1], ("t2".into(), "https://a.com/".into(), Some(json!("https://a.com/next"))));
        assert_eq!(starts[2], ("t3".into(), "https://b.com/".into(), None));
        // The page tells main about its groups.
        assert_eq!(status()["groups"], json!(["ses-a", "ses-b"]));
    }

    #[test]
    fn a_call_with_no_key_works_in_the_persons_own_tab_as_before() {
        record();
        person_at("https://mine.com/");
        navigate_as("ses-a", "Agent", "https://a.com/", false).unwrap();
        let out = run_loaded(json!({ "op": "navigate", "url": "https://next.com/" }));
        assert_eq!(out.unwrap()["tab"], "t1");
        assert_eq!(with(|page| page.tabs.get("t1").unwrap().commands.last().map(|command| command["url"].clone())), Some(json!("https://next.com/")));
        // And a tab shown that is an agent's doesn't move the person's tab for it.
        with(|page| page.tabs.activate("t2"));
        assert_eq!(with(|page| page.tabs.target("")), Some("t1".into()));
    }

    #[test]
    fn a_call_from_a_key_with_no_group_is_told_to_open_one_except_an_open_which_makes_it() {
        record();
        person_at("https://mine.com/");
        let nobody = caller_for("ses-x", "Agent");
        for op in [json!({ "op": "snapshot" }), json!({ "op": "click", "ref": "e1" }), json!({ "op": "status" }), json!({ "op": "navigate", "action": "reload" }), json!({ "op": "tabs", "action": "list" }), json!({ "op": "eval", "script": "1" }), json!({ "op": "screenshot" })] {
            let mut op = op;
            op["caller"] = nobody.clone();
            assert_eq!(run_op(op.clone()).borrow().clone(), Some(Err(NO_TAB.into())), "{op}");
        }
        assert_eq!(with(|page| page.tabs.list.len()), 1, "none of them made a tab");
        assert!(navigate_as("ses-x", "Agent", "https://x.com/", false).is_ok());
        assert_eq!(with(|page| page.tabs.list.len()), 2);
    }

    #[test]
    fn an_op_acts_on_the_callers_tab_whichever_tab_is_shown() {
        record();
        person_at("https://mine.com/");
        navigate_as("ses-a", "A", "https://a.com/", false).unwrap();
        navigate_as("ses-b", "B", "https://b.com/", false).unwrap();
        let out = run_op(json!({ "op": "click", "ref": "e1", "caller": caller_for("ses-b", "B") }));
        let sent = |id: &str| with(|page| page.tabs.get(id).unwrap().commands.last().cloned());
        assert!(sent("t3").is_some_and(|command| command["script"].as_str().unwrap().contains("e1")), "B's tab got it");
        assert!(sent("t2").is_none(), "A's tab did not");
        assert!(sent("t1").is_none(), "nor the person's");
        drop(out);
    }

    #[test]
    fn results_come_back_to_the_tab_and_the_waiter_that_asked_in_any_order() {
        record();
        person_at("https://mine.com/");
        navigate_as("ses-a", "A", "https://a.com/", false).unwrap();
        navigate_as("ses-b", "B", "https://b.com/", false).unwrap();
        let one = run_op(json!({ "op": "eval", "script": "return 'a'", "caller": caller_for("ses-a", "A") }));
        let two = run_op(json!({ "op": "eval", "script": "return 'b'", "caller": caller_for("ses-b", "B") }));
        let token = |id: &str| with(|page| page.tabs.get(id).unwrap().commands.last().unwrap()["token"].as_u64().unwrap());
        let (a, b) = (token("t2"), token("t3"));
        assert_ne!(a, b, "one count for all the tabs");
        assert_eq!(with(|page| (page.tabs.tab_of_token(a).map(str::to_owned), page.tabs.tab_of_token(b).map(str::to_owned))), (Some("t2".into()), Some("t3".into())));
        result(&json!({ "token": b, "value": "from b" }));
        testing::settle();
        assert_eq!((one.borrow().clone(), two.borrow().clone()), (None, Some(Ok(json!("from b")))));
        result(&json!({ "token": a, "value": "from a" }));
        testing::settle();
        assert_eq!(one.borrow().clone(), Some(Ok(json!("from a"))));
    }

    #[test]
    fn closing_a_tab_ends_what_waited_on_it_at_once() {
        record();
        person_at("https://mine.com/");
        navigate_as("ses-a", "A", "https://a.com/", false).unwrap();
        let waiting = run_op(json!({ "op": "eval", "script": "return 1", "caller": caller_for("ses-a", "A") }));
        assert!(waiting.borrow().is_none());
        browser_page::close_tab("t2");
        testing::settle();
        assert!(waiting.borrow().clone().unwrap().unwrap_err().contains("tab was closed"));
    }

    #[test]
    fn open_goes_to_the_current_tab_or_with_new_tab_to_another_of_the_callers_group_and_follows_only_where_watched() {
        record();
        person_at("https://mine.com/");
        navigate_as("ses-a", "A", "https://a.com/", false).unwrap();
        navigate_as("ses-b", "B", "https://b.com/", false).unwrap();
        assert_eq!(with(|page| page.tabs.list.len()), 3);
        // A new tab in A's group; the person watches their own, so it stays behind.
        let more = navigate_as("ses-a", "A", "https://a.com/two", true).unwrap();
        assert_eq!((more["tab"].as_str(), with(|page| page.tabs.active.clone()).as_str()), (Some("t4"), "t1"));
        assert_eq!(with(|page| page.tabs.target("ses-a")), Some("t4".into()), "the new tab is A's current one");
        // The person looks at A's group: its next new tab comes forward, B's does not.
        with(|page| page.tabs.activate("t4"));
        navigate_as("ses-b", "B", "https://b.com/two", true).unwrap();
        assert_eq!(with(|page| page.tabs.active.clone()), "t4");
        navigate_as("ses-a", "A", "https://a.com/three", true).unwrap();
        assert_eq!(with(|page| page.tabs.active.clone()), "t6");
    }

    #[test]
    fn tabs_lists_selects_and_closes_only_the_callers_own_tabs() {
        record();
        person_at("https://mine.com/");
        navigate_as("ses-a", "A", "https://a.com/", false).unwrap();
        navigate_as("ses-a", "A", "https://a.com/two", true).unwrap();
        navigate_as("ses-b", "B", "https://b.com/", false).unwrap();
        let tabs = |key: &str, extra: Value| {
            let mut op = json!({ "op": "tabs", "caller": caller_for(key, "X") });
            op.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            run_op(op).borrow().clone().unwrap()
        };
        let listed = tabs("ses-a", json!({ "action": "list" })).unwrap();
        assert_eq!(listed.as_array().unwrap().iter().map(|tab| (tab["id"].as_str().unwrap(), tab["active"].as_bool().unwrap())).collect::<Vec<_>>(), [("t2", false), ("t3", true)]);
        assert!(tabs("ses-a", json!({ "action": "select", "tab": "t4" })).unwrap_err().contains("no tab t4"), "B's tab is not A's");
        assert!(tabs("ses-a", json!({ "action": "close", "tab": "t1" })).is_err(), "nor the person's");
        assert_eq!(with(|page| page.tabs.list.len()), 4);
        let selected = tabs("ses-a", json!({ "action": "select", "tab": "t2" })).unwrap();
        assert_eq!(selected[0]["active"], true);
        assert_eq!(with(|page| page.tabs.target("ses-a")), Some("t2".into()));
        assert_eq!(with(|page| page.tabs.active.clone()), "t1", "selecting does not move the person");
        tabs("ses-a", json!({ "action": "close", "tab": "t2" })).unwrap();
        assert_eq!(with(|page| page.tabs.target("ses-a")), Some("t3".into()));
        // The person's own list, for a call with no key.
        let mine = tabs("", json!({ "action": "list" })).unwrap();
        assert_eq!(mine.as_array().unwrap().len(), 1);
        assert!(tabs("ses-a", json!({ "action": "nope" })).is_err());
    }

    #[test]
    fn a_handoff_brings_the_agents_tab_forward_for_the_person() {
        record();
        person_at("https://mine.com/");
        navigate_as("ses-a", "A", "https://a.com/", false).unwrap();
        assert_eq!(with(|page| page.tabs.active.clone()), "t1");
        let out = run_op(json!({ "op": "handoff", "message": "Sign in", "timeout_ms": 5000, "caller": caller_for("ses-a", "A") }));
        assert_eq!(with(|page| page.tabs.active.clone()), "t2");
        assert!(handoff_banner().is_some());
        done();
        testing::settle();
        assert!(out.borrow().clone().unwrap().is_ok());
    }

    #[test]
    fn popping_out_one_tab_of_several_moves_only_it_and_keeps_the_page_and_the_agents_group() {
        let calls = record();
        person_at("https://mine.com/");
        navigate_as("ses-a", "A", "https://a.com/", false).unwrap();
        let out = run_op(json!({ "op": "pop_out", "width": 390.0, "height": 844.0, "caller": caller_for("ses-a", "A") }));
        assert_eq!(out.borrow().clone(), Some(Ok(json!({ "width": 390, "height": 844, "closed": false }))));
        assert_eq!(calls.borrow().iter().find(|call| call.0 == "openView").unwrap().1["args"], json!({ "url": "https://a.com/", "window": true, "group": { "key": "ses-a", "name": "A" } }));
        assert_eq!(with(|page| page.tabs.list.iter().map(|tab| tab.id.clone()).collect::<Vec<_>>()), ["t1"], "the tab left this page");
        assert!(with(|page| page.tabs.group("ses-a").is_none()));
        testing::fire_timers();
        testing::settle();
        assert!(calls.borrow().iter().all(|call| call.0 != "closeTab"), "the page still has the person's tab");
    }

    #[test]
    fn putting_a_window_page_back_reopens_its_address_with_no_group_for_the_person() {
        let calls = record();
        shown("https://a.com/x", true);
        with(|page| page.tabs.active_mut().group = None);
        assert_eq!(run_op(json!({ "op": "pop_in", "caller": caller_for("", "") })).borrow().clone(), Some(Ok(Value::Null)));
        assert_eq!(calls.borrow()[0].1["args"], json!({ "url": "https://a.com/x" }));
    }

    /// The last command sent to `tab`, answered with `value`.
    fn answer_last(tab: &str, value: Value) -> Value {
        let command = with(|page| page.tabs.get(tab).unwrap().commands.last().cloned()).unwrap();
        result(&json!({ "token": command["token"], "value": value }));
        testing::settle();
        command
    }

    #[test]
    fn upload_diff_and_recording_send_the_view_the_commands_the_app_takes_and_return_its_answer() {
        record();
        person_at("https://mine.com/");
        navigate_as("ses-a", "A", "https://a.com/", false).unwrap();
        let a = caller_for("ses-a", "A");
        let files = json!(["/tmp/a.png", { "name": "b.png", "data": "AAAA" }]);
        let out = run_op(json!({ "op": "upload", "ref": "e12", "selector": null, "files": files, "caller": a }));
        let sent = answer_last("t2", json!({ "files": 2 }));
        assert_eq!((sent["action"].clone(), sent["ref"].clone(), sent["selector"].clone(), sent["files"].clone()), (json!("upload"), json!("e12"), json!(null), files));
        assert_eq!(out.borrow().clone(), Some(Ok(json!({ "files": 2 }))));
        let out = run_op(json!({ "op": "screenshot_diff", "baseline": "/p/base.png", "threshold": 0.2, "clip": { "x": 0, "y": 0, "width": 10, "height": 10 }, "current": null, "diff": "/p/d.png", "caller": a }));
        let sent = answer_last("t2", json!({ "matches": true }));
        assert_eq!((sent["action"].clone(), sent["baseline"].clone(), sent["diff"].clone(), sent["threshold"].clone()), (json!("screenshotDiff"), json!("/p/base.png"), json!("/p/d.png"), json!(0.2)));
        assert_eq!(out.borrow().clone(), Some(Ok(json!({ "matches": true }))));
        let out = run_op(json!({ "op": "recording", "do": "start", "path": "/p/r.mp4", "fps": 30, "maxMs": 900000, "caller": a }));
        let sent = answer_last("t2", json!({ "recording": true }));
        assert_eq!((sent["action"].clone(), sent["do"].clone(), sent["fps"].clone(), sent["maxMs"].clone()), (json!("record"), json!("start"), json!(30), json!(900000)));
        assert!(out.borrow().clone().unwrap().is_ok());
        // None of them reached the person's tab or anyone else's.
        assert!(with(|page| page.tabs.get("t1").unwrap().commands.is_empty()));
    }

    #[test]
    fn a_demonstration_shows_a_banner_on_the_agents_tab_records_until_done_and_answers_steps_and_a_summary() {
        record();
        person_at("https://mine.com/");
        navigate_as("ses-a", "A", "https://a.com/login", false).unwrap();
        let out = run_op(json!({ "op": "demonstrate", "message": "Show me how to sign in", "timeout_ms": 60000, "caller": caller_for("ses-a", "A") }));
        assert_eq!(with(|page| page.tabs.active.clone()), "t2", "the tab comes forward");
        assert!(handoff_banner().is_some());
        // A second demonstration waits.
        assert!(run_op(json!({ "op": "demonstrate", "message": "x", "caller": caller_for("ses-a", "A") })).borrow().clone().unwrap().is_err());
        testing::fire_timers();
        testing::settle();
        let ask = answer_last("t2", json!([{ "t": "fill", "selector": "#pw", "role": "textbox", "name": "Password", "value": "hunter2", "inputType": "password" }]));
        assert!(ask["script"].as_str().unwrap().ends_with("return d.drain(false);"), "recording goes on");
        with(|page| page.tabs.get_mut("t2").unwrap().state.url = "https://a.com/home".into());
        // The banner says it records, for which agent, what it asked and how much it has.
        let banner = wings_plugin::ui::tree_json(handoff_banner().unwrap()).to_string();
        for words in ["Recording your steps", "for A", "Show me how to sign in", "1 step recorded.", "Cancel", "Done"] {
            assert!(banner.contains(words), "{words}: {banner}");
        }
        demo_end(true);
        assert!(handoff_banner().is_none(), "Done takes the banner away at once");
        testing::fire_timers();
        testing::settle();
        let last = answer_last("t2", json!([{ "t": "click", "selector": "#go", "role": "button", "name": "Sign in" }]));
        assert!(last["script"].as_str().unwrap().ends_with("return d.drain(true);"), "the last ask ends the recorder");
        let answer = out.borrow().clone().unwrap().unwrap();
        assert_eq!(answer["summary"], "1. fill textbox \"Password\" (#pw) with \"(hidden)\"\n2. click button \"Sign in\" (#go)\n3. go to https://a.com/home");
        assert!(!answer.to_string().contains("hunter2") && answer.get("endedBecause").is_none());
        assert!(handoff_banner().is_none());
    }

    #[test]
    fn cancel_on_a_demonstration_ends_the_recorder_and_tells_the_agent_nothing_was_shown() {
        record();
        person_at("https://mine.com/");
        navigate_as("ses-a", "A", "https://a.com/", false).unwrap();
        let out = run_op(json!({ "op": "demonstrate", "message": "Show me", "caller": caller_for("ses-a", "A") }));
        demo_end(false);
        assert!(handoff_banner().is_none(), "Cancel takes the banner away at once");
        testing::fire_timers();
        testing::settle();
        let last = answer_last("t2", json!([{ "t": "click", "selector": "#go", "role": "button", "name": "Go" }]));
        assert!(last["script"].as_str().unwrap().ends_with("return d.drain(true);"), "the recorder stops");
        let said = out.borrow().clone().unwrap().unwrap_err();
        assert!(said.contains("Cancel") && !said.contains("#go"), "{said}");
        // The next demonstration may start.
        assert!(agent(|agent| agent.demo.is_none()));
    }

    #[test]
    fn a_handoff_banner_names_the_agent_waiting_and_done_ends_it() {
        record();
        person_at("https://mine.com/");
        navigate_as("ses-a", "Fix login", "https://a.com/", false).unwrap();
        let out = run_op(json!({ "op": "handoff", "message": "Sign in, then press Done.", "caller": caller_for("ses-a", "Fix login") }));
        let banner = wings_plugin::ui::tree_json(handoff_banner().unwrap()).to_string();
        for words in ["Your turn", "Fix login is waiting", "Sign in, then press Done.", "Done"] {
            assert!(banner.contains(words), "{words}: {banner}");
        }
        done();
        testing::settle();
        assert!(out.borrow().clone().unwrap().is_ok() && handoff_banner().is_none());
    }

    #[test]
    fn a_demonstration_ends_itself_when_the_time_is_out() {
        record();
        person_at("https://mine.com/");
        navigate_as("ses-a", "A", "https://a.com/", false).unwrap();
        let out = run_op(json!({ "op": "demonstrate", "message": "Show me", "timeout_ms": 5000, "caller": caller_for("ses-a", "A") }));
        testing::set_now(host::now() + 6000.0);
        testing::fire_timers();
        testing::settle();
        answer_last("t2", json!([]));
        assert_eq!(out.borrow().clone().unwrap().unwrap()["endedBecause"], "the time was out");
        assert!(handoff_banner().is_none());
    }

    #[test]
    fn webmcp_asks_the_isolated_world_first_and_the_pages_own_world_when_it_finds_nothing() {
        record();
        person_at("https://mine.com/");
        navigate_as("ses-a", "A", "https://a.com/", false).unwrap();
        let a = caller_for("ses-a", "A");
        let out = run_op(json!({ "op": "webmcp", "action": "list", "caller": a }));
        let first = answer_last("t2", json!({ "available": false, "tools": [] }));
        assert!(first.get("world").is_none() && first["script"].as_str().unwrap().contains(r#""action":"list""#));
        let second = answer_last("t2", json!({ "available": true, "tools": [{ "name": "addTodo", "frame": "https://a.com/" }] }));
        assert_eq!(second["world"], "page");
        assert_eq!(out.borrow().clone().unwrap().unwrap()["tools"][0]["name"], "addTodo");
        // A tool found in the first world is not asked for again.
        let out = run_op(json!({ "op": "webmcp", "action": "call", "name": "addTodo", "input": { "text": "x" }, "timeout": 2000, "caller": a }));
        let sent = answer_last("t2", json!({ "found": true, "result": "ok" }));
        assert!(sent.get("world").is_none() && sent["script"].as_str().unwrap().contains(r#""timeout":2000"#));
        assert_eq!(out.borrow().clone().unwrap().unwrap()["result"], "ok");
        // No such tool in either world says so.
        let out = run_op(json!({ "op": "webmcp", "action": "call", "name": "nope", "caller": a }));
        answer_last("t2", json!({ "found": false }));
        answer_last("t2", json!({ "found": false }));
        assert!(out.borrow().clone().unwrap().unwrap_err().contains("No WebMCP tool named nope"));
    }

    /// An agent hears its own group's downloads and the window its page didn't open, never the person's downloads, and only
    /// the origin of one from a site signed in with imported cookies.
    #[test]
    fn an_agent_hears_its_own_downloads_and_the_window_its_page_could_not_open() {
        record();
        person_at("https://mine.com/");
        navigate_as("ses-a", "A", "https://a.com/", false).unwrap();
        let download = |id: u64, url: &str| wings_plugin::ui::WebDownload { id, url: url.into(), name: format!("f{id}.pdf"), path: format!("/Users/me/Downloads/f{id}.pdf"), state: "done".into(), received: 5, total: Some(5), error: None };
        with(|page| {
            page.downloads.heard("t1", None, false, download(1, "https://mine.com/private.pdf"));
            page.downloads.heard("t2", Some("ses-a"), false, download(2, "https://a.com/report.pdf?token=1"));
        });
        let heard = run_op(json!({ "op": "downloads", "caller": caller_for("ses-a", "A") })).borrow().clone().unwrap().unwrap();
        assert_eq!(heard, json!([{ "name": "f2.pdf", "path": "/Users/me/Downloads/f2.pdf", "state": "done", "received": 5, "total": 5, "error": null, "url": "https://a.com/report.pdf?token=1", "tab": "t2" }]));
        with(|page| page.tabs.get_mut("t2").unwrap().state.imported = true);
        let heard = run_op(json!({ "op": "downloads", "caller": caller_for("ses-a", "A") })).borrow().clone().unwrap().unwrap();
        assert_eq!(heard[0]["url"], "https://a.com", "only the origin on a site signed in with imported cookies");
        // Begun on such a site, it stays that way once its tab is gone.
        with(|page| page.downloads.heard("t9", Some("ses-a"), true, download(3, "https://b.com/secret.pdf?token=2")));
        let heard = run_op(json!({ "op": "downloads", "caller": caller_for("ses-a", "A") })).borrow().clone().unwrap().unwrap();
        assert_eq!(heard[0]["url"], "https://b.com");
        with(|page| page.tabs.get_mut("t2").unwrap().blocked = Some("https://ads.example/x?y".into()));
        let status = run_op(json!({ "op": "status", "caller": caller_for("ses-a", "A") })).borrow().clone().unwrap().unwrap();
        assert_eq!(status["blockedPopup"], "https://ads.example");
    }

    #[test]
    fn an_op_routed_to_a_tab_counts_as_using_it() {
        record();
        person_at("https://mine.com/");
        navigate_as("ses-a", "A", "https://a.com/", false).unwrap();
        testing::set_now(host::now() + 5000.0);
        run_op(json!({ "op": "snapshot", "caller": caller_for("ses-a", "A") }));
        assert_eq!(with(|page| page.tabs.get("t2").unwrap().used), Some(host::now()));
    }

    /// The last command sent to `tab`, answered with the error `why`.
    fn fail_last(tab: &str, why: &str) -> Value {
        let command = with(|page| page.tabs.get(tab).unwrap().commands.last().cloned()).unwrap();
        result(&json!({ "token": command["token"], "error": why }));
        testing::settle();
        command
    }

    fn last_command(tab: &str) -> Value {
        with(|page| page.tabs.get(tab).unwrap().commands.last().cloned()).unwrap()
    }

    fn kinds_of(command: &Value) -> Vec<String> {
        command["events"].as_array().unwrap().iter().map(|event| event["type"].as_str().unwrap_or("text").to_owned()).collect()
    }

    /// On a Chromium page a click is the helper finding the element, then real mouse events at its centre, and the answer is
    /// the helper's own words; the helper's synthetic click isn't sent.
    #[test]
    fn a_click_finds_the_element_then_sends_real_mouse_events_at_its_centre() {
        record();
        person_at("https://mine.com/");
        let out = run_op(json!({ "op": "click", "ref": "e1" }));
        assert!(last_command("t1")["script"].as_str().unwrap().ends_with("return w.locate(\"e1\");"));
        answer_last("t1", json!({ "x": 120.5, "y": 44.0, "plain": true, "drag": false }));
        let sent = last_command("t1");
        assert_eq!(sent["action"], "input");
        assert_eq!(kinds_of(&sent), ["mouseMoved", "mousePressed", "mouseReleased"]);
        assert_eq!((sent["events"][1]["x"].clone(), sent["events"][1]["y"].clone(), sent["events"][1]["clickCount"].clone()), (json!(120.5), json!(44.0), json!(1)));
        answer_last("t1", json!({ "events": 3 }));
        assert_eq!(out.borrow().clone(), Some(Ok(json!("Clicked e1"))));
        let out = run_op(json!({ "op": "hover", "ref": "e1" }));
        answer_last("t1", json!({ "x": 1.0, "y": 2.0, "plain": true }));
        assert_eq!(kinds_of(&last_command("t1")), ["mouseMoved"]);
        answer_last("t1", json!({ "events": 1 }));
        assert_eq!(out.borrow().clone(), Some(Ok(json!("Hovered e1"))));
    }

    /// WebKit says it needs Chromium, and a page with a password field refuses real input: the helper clicks, as before.
    /// An element a real click would open a picker on, or that shows nowhere, goes to the helper without asking.
    #[test]
    fn a_click_the_page_cant_take_as_real_input_is_the_helpers_as_before() {
        for why in ["This needs the Chromium engine: turn it on in Browser settings.", "This page has a password or card field, so scripts can't run on it. Use handoff for sign-ins."] {
            record();
            person_at("https://mine.com/");
            let out = run_op(json!({ "op": "click", "ref": "e1" }));
            answer_last("t1", json!({ "x": 5.0, "y": 5.0, "plain": true }));
            fail_last("t1", why);
            assert!(last_command("t1")["script"].as_str().unwrap().ends_with("return w.click(\"e1\");"), "{why}");
            answer_last("t1", json!("Clicked e1"));
            assert_eq!(out.borrow().clone(), Some(Ok(json!("Clicked e1"))));
        }
        record();
        person_at("https://mine.com/");
        let out = run_op(json!({ "op": "click", "ref": "e3" }));
        answer_last("t1", json!({ "x": 5.0, "y": 5.0, "plain": false }));
        assert!(last_command("t1")["script"].as_str().unwrap().ends_with("return w.click(\"e3\");"), "a select or a file input: no real click");
        answer_last("t1", json!("Clicked e3"));
        assert!(out.borrow().clone().unwrap().is_ok());
        // An error that isn't one of those two is the answer.
        let out = run_op(json!({ "op": "click", "ref": "e1" }));
        answer_last("t1", json!({ "x": 5.0, "y": 5.0, "plain": true }));
        fail_last("t1", "Wings couldn't check this page for password and card fields, so nothing ran: try again");
        assert!(out.borrow().clone().unwrap().unwrap_err().contains("try again"));
    }

    #[test]
    fn fill_selects_the_field_then_types_over_it_and_type_and_press_send_text_and_keys() {
        record();
        person_at("https://mine.com/");
        let out = run_op(json!({ "op": "fill", "ref": "e2", "value": "ana@example.com" }));
        assert!(last_command("t1")["script"].as_str().unwrap().ends_with("return w.focus_all(\"e2\");"));
        answer_last("t1", json!({ "plain": true }));
        assert_eq!(last_command("t1")["events"], json!([{ "t": "text", "text": "ana@example.com" }]));
        answer_last("t1", json!({ "events": 1 }));
        assert_eq!(out.borrow().clone(), Some(Ok(json!("Filled e2"))));
        // A field real typing can't fill (a date) is filled by the helper.
        let out = run_op(json!({ "op": "fill", "ref": "e4", "value": "2026-01-02" }));
        answer_last("t1", json!({ "plain": false }));
        assert!(last_command("t1")["script"].as_str().unwrap().ends_with("return w.fill(\"e4\", \"2026-01-02\");"));
        answer_last("t1", json!("Filled e4"));
        assert!(out.borrow().clone().unwrap().is_ok());
        // Typing needs a focused element first, then inserts the text.
        let out = run_op(json!({ "op": "type", "text": "héllo" }));
        assert!(last_command("t1")["script"].as_str().unwrap().ends_with("return w.focused();"));
        answer_last("t1", json!(true));
        assert_eq!(last_command("t1")["events"], json!([{ "t": "text", "text": "héllo" }]));
        answer_last("t1", json!({ "events": 1 }));
        assert_eq!(out.borrow().clone(), Some(Ok(json!("Typed 5 characters"))));
        let out = run_op(json!({ "op": "type", "text": "x" }));
        fail_last("t1", "Nothing has focus: click or fill a field first");
        assert!(out.borrow().clone().unwrap().unwrap_err().contains("Nothing has focus"));
        // A key goes down and up; a combo with a modifier holds it between; one the table doesn't know is the helper's.
        let out = run_op(json!({ "op": "press", "key": "Control+a" }));
        assert_eq!(kinds_of(&last_command("t1")), ["rawKeyDown", "rawKeyDown", "keyUp", "keyUp"]);
        answer_last("t1", json!({ "events": 4 }));
        assert_eq!(out.borrow().clone(), Some(Ok(json!("Pressed Control+a"))));
        run_op(json!({ "op": "press", "key": "Control+Enterr" }));
        assert!(last_command("t1")["script"].as_str().unwrap().ends_with("return w.press(\"Control+Enterr\");"));
    }

    /// A drag between two plain elements is the pointer's; one from a link, an image or a draggable would start the system's
    /// own drag, so it is the helper's.
    #[test]
    fn a_drag_is_the_pointers_unless_the_source_would_start_the_systems_own_drag() {
        record();
        person_at("https://mine.com/");
        let out = run_op(json!({ "op": "drag", "from": "e1", "to": "e2" }));
        answer_last("t1", json!({ "x": 10.0, "y": 10.0, "plain": true, "drag": false }));
        answer_last("t1", json!({ "x": 90.0, "y": 50.0, "plain": true, "drag": false }));
        let kinds = kinds_of(&last_command("t1"));
        assert_eq!((kinds.first().map(String::as_str), kinds.get(1).map(String::as_str), kinds.last().map(String::as_str)), (Some("mouseMoved"), Some("mousePressed"), Some("mouseReleased")));
        answer_last("t1", json!({ "events": kinds.len() }));
        assert_eq!(out.borrow().clone(), Some(Ok(json!("Dragged e1 to e2"))));
        run_op(json!({ "op": "drag", "from": "e1", "to": "e2" }));
        answer_last("t1", json!({ "x": 10.0, "y": 10.0, "plain": true, "drag": true }));
        answer_last("t1", json!({ "x": 90.0, "y": 50.0, "plain": true, "drag": false }));
        assert!(last_command("t1")["script"].as_str().unwrap().ends_with("return w.drag(\"e1\", \"e2\");"));
    }

    /// The network tool asks the app with its filters, and WebKit's answer that it needs Chromium is the tool's.
    #[test]
    fn network_asks_the_app_with_its_filters_and_passes_its_answer_on() {
        record();
        person_at("https://mine.com/");
        let out = run_op(json!({ "op": "network", "url": "/api", "status": "4xx", "type": "XHR", "since": 5.0, "limit": 20, "detail": null, "body": false }));
        let sent = last_command("t1");
        assert_eq!((sent["action"].clone(), sent["url"].clone(), sent["status"].clone(), sent["type"].clone(), sent["since"].clone(), sent["limit"].clone()), (json!("network"), json!("/api"), json!("4xx"), json!("XHR"), json!(5.0), json!(20)));
        answer_last("t1", json!({ "requests": [], "matching": 0, "kept": 0, "now": 1.0 }));
        assert_eq!(out.borrow().clone().unwrap().unwrap()["matching"], 0);
        let out = run_op(json!({ "op": "network", "detail": "1.1", "body": true }));
        assert_eq!(last_command("t1")["detail"], "1.1");
        fail_last("t1", "This needs the Chromium engine: turn it on in Browser settings.");
        assert!(out.borrow().clone().unwrap().unwrap_err().contains("needs the Chromium engine"));
    }
}
