//! What a Browser page does for the agent tools: it tells main who and where it is, and runs what main asks (load an address,
//! run the JavaScript helper, take a picture, pop out). Main picks the page; the page does the work, because only a page
//! can draw a `WebView` and run its commands. The tools themselves are in `agent_tools.rs`, in main.
//!
//! Each agent has a group of tabs in the page, found by the key main sends with every op (`caller`). An op acts on that
//! group's tab, never on the person's tabs or another agent's, and whatever tab is shown. A call with no key (the CLI) works in
//! the person's own tab.

use crate::{
    agent_js::{self, SNAPSHOT_MAX},
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
    ui::{button, notice, row},
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
    /// The step the person was asked to do, and who waits for them to say it is done.
    handoff: Option<(String, Option<Sender<()>>)>,
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
            "dialog": tab.state.dialog,
        })
    })
}

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
async fn handoff(tab: &str, op: &Value) -> Result<Value, String> {
    let message = text(op, "message")?.to_owned();
    let (sender, receiver) = oneshot();
    with(|page| page.tabs.activate(tab));
    tabs_changed();
    agent(|agent| agent.handoff = Some((message, Some(sender))));
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
                with(|page| page.tabs.select(id));
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
        _ => {}
    }
    let tab = target(&key)?;
    match name_of {
        "status" => Ok(tab_status(&tab)),
        "screenshot" => {
            let mut shot = command(&tab, |token| wings_plugin::ui::web_snapshot(token)).await?;
            shot["url"] = json!(with(|page| page.tabs.get(&tab).map(|tab| agent_url(&tab.state)).unwrap_or_default()));
            Ok(shot)
        }
        "snapshot" => helper(&tab, "snapshot", &[json!(SNAPSHOT_MAX)]).await,
        "click" | "hover" => helper(&tab, name_of, &[string("ref")?]).await,
        "fill" => helper(&tab, "fill", &[string("ref")?, string("value")?]).await,
        "type" => helper(&tab, "type", &[string("text")?]).await,
        "press" => helper(&tab, "press", &[string("key")?]).await,
        "select" => helper(&tab, "select", &[string("ref")?, op["values"].clone()]).await,
        "scroll" => helper(&tab, "scroll", &[json!({ "ref": op["ref"], "to": op["to"], "x": op["x"], "y": op["y"] })]).await,
        "drag" => helper(&tab, "drag", &[string("from")?, string("to")?]).await,
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
        "handoff" => handoff(&tab, &op).await,
        other => Err(format!("The page doesn't know {other}")),
    }
}

/// Done pressed on the banner.
fn done() {
    if let Some(sender) = agent(|agent| agent.handoff.as_mut().and_then(|(_, sender)| sender.take())) {
        sender.send(());
    }
}

/// The banner that asks the person for a step, when an agent did.
pub fn handoff_banner() -> Option<wings_plugin::ui::Node> {
    let message = agent(|agent| agent.handoff.as_ref().map(|(message, _)| message.clone()))?;
    Some(row().prop("gap", "s").prop("align", "center").key("handoff").child(notice("warn", message).prop("grow", true)).child(button("Done", "default", false, done)))
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
        // The helper's calls run in the isolated world, and a failure is the view's error.
        let out = run_op(json!({ "op": "click", "ref": "e1" }));
        let command = with(|page| page.tabs.active_tab().commands.last().cloned()).unwrap();
        assert!(command.get("world").is_none() && command["script"].as_str().unwrap().ends_with("return w.click(\"e1\");"));
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
}
