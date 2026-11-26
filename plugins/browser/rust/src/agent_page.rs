//! What a Browser page does for the agent tools: it tells main who and where it is, and runs what main asks (load an address,
//! run the JavaScript helper, take a picture, pop out). Main picks the page; the page does the work, because only a page
//! can draw a `WebView` and run its commands. The tools themselves are in `agent_tools.rs`, in main.

use crate::{
    agent_js::{self, SNAPSHOT_MAX},
    address::resolve,
    browser_page::{self, with},
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

#[derive(Default)]
struct Agent {
    id: String,
    /// In a window of its own, which the page learns from the args it was opened with.
    popped: bool,
    project: String,
    /// What was last sent to main, so a change that doesn't matter to it isn't sent.
    sent: Option<Value>,
    /// How many times the view's state changed.
    updates: u64,
    /// The scripts and pictures the view is asked for, by token.
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

pub fn start(args: Option<&Value>, start_url: &str) {
    agent(|agent| {
        agent.id = id_for(start_url);
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

/// This page as main wants it: who, where, and what it shows.
fn status() -> Value {
    let (id, popped, project) = agent(|agent| (agent.id.clone(), agent.popped, agent.project.clone()));
    with(|page| {
        json!({
            "id": id, "project": project, "popped": popped, "url": page.state.url, "title": page.state.title,
            "loading": page.state.loading, "canGoBack": page.state.can_go_back, "canGoForward": page.state.can_go_forward,
            "focused": host::focused(),
        })
    })
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

/// The view's state changed.
pub fn changed() {
    agent(|agent| agent.updates += 1);
    announce(false);
}

/// The view's answer to a script or a picture.
pub fn result(event: &Value) {
    let Some(token) = event["token"].as_u64() else { return };
    let Some(sender) = agent(|agent| agent.waiting.remove(&token)) else { return };
    sender.send(match event["error"].as_str() {
        Some(error) => Err(error.to_owned()),
        None => Ok(event["value"].clone()),
    });
}

/// Sends the view a command made from its token and waits for what it answers.
async fn command(make: impl FnOnce(u64) -> Value) -> Result<Value, String> {
    let (sender, receiver) = oneshot();
    let token = with(|page| page.run(make));
    agent(|agent| agent.waiting.insert(token, sender));
    let answer = within(RESULT_MS, receiver).await;
    agent(|agent| agent.waiting.remove(&token));
    answer.ok_or("The page took too long to answer. It may be busy loading: wait_for it, then try again.")?.ok_or_else(|| "The page closed before it answered.".to_owned())?
}

/// Runs a call of the JavaScript helper in the page's isolated world.
async fn helper(func: &str, args: &[Value]) -> Result<Value, String> {
    let script = agent_js::body(func, args);
    command(|token| wings_plugin::ui::web_eval(&script, false, token)).await
}

fn text<'a>(op: &'a Value, key: &str) -> Result<&'a str, String> {
    op[key].as_str().ok_or_else(|| format!("{key} is missing"))
}

/// Waits for the view to start loading what a command asked for, and to finish it.
async fn loaded() {
    let (before, begun) = (agent(|agent| agent.updates), host::now());
    loop {
        host::sleep(LOAD_POLL_MS).await;
        let (changed, loading) = (agent(|agent| agent.updates) > before, with(|page| page.state.loading));
        let waited = host::now() - begun;
        if (changed || waited > LOAD_START_MS) && !loading || waited > LOAD_MAX_MS {
            return;
        }
    }
}

async fn navigate(op: &Value) -> Result<Value, String> {
    let engine = settings::engine().get();
    match (op["url"].as_str(), op["action"].as_str()) {
        (Some(url), None) => {
            let url = resolve(url, engine).ok_or("That isn't an address or words to search for.")?;
            with(|page| page.send("load", Some(&url)));
        }
        (None, Some(action @ ("back" | "forward" | "reload"))) => with(|page| page.send(action, None)),
        _ => return Err("navigate takes a url, or an action of back, forward or reload, not both".into()),
    }
    loaded().await;
    Ok(status())
}

async fn wait_for(op: &Value) -> Result<Value, String> {
    let (words, selector, url) = (op["text"].as_str(), op["selector"].as_str(), op["url"].as_str());
    if words.is_none() && selector.is_none() && url.is_none() {
        return Err("wait_for takes text, selector or url".into());
    }
    let limit = op["timeout_ms"].as_f64().unwrap_or(10_000.0);
    let begun = host::now();
    loop {
        if url.is_some_and(|url| with(|page| page.state.url.contains(url))) {
            return Ok(status());
        }
        // A page that is loading has no document to ask yet, so a failed ask is "not yet".
        if (words.is_some() || selector.is_some()) && helper("has", &[json!({ "text": words, "selector": selector })]).await.is_ok_and(|found| found == true) {
            return Ok(status());
        }
        if host::now() - begun >= limit {
            let what = words.map(|w| format!("text {w:?}")).or(selector.map(|s| format!("selector {s}"))).or(url.map(|u| format!("an address with {u:?}")));
            return Err(format!("Timed out after {} s waiting for {}.", (limit / 1000.0).round(), what.unwrap_or_default()));
        }
        host::sleep(WAIT_POLL_MS).await;
    }
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

/// The address this page reopens with: the one it shows, or none for a page that shows nothing yet.
fn reopen_args(extra: Value) -> Value {
    let url = with(|page| if page.state.url.is_empty() { page.start.clone() } else { page.state.url.clone() });
    let mut args = json!({ "url": url });
    if let (Some(args), Some(extra)) = (args.as_object_mut(), extra.as_object()) {
        args.extend(extra.clone());
    }
    args
}

/// Opens this page's address in a window of its own and closes the tab. The same cookie store keeps the person signed in; the
/// back and forward history doesn't come along.
pub async fn pop_out(size: Option<(f64, f64)>) -> Result<Value, String> {
    if agent(|agent| agent.popped) {
        return Err("This page is already in a window of its own.".into());
    }
    let answer = host::open_window("browser", &reopen_args(json!({ "window": true })), size).await?;
    close_soon();
    Ok(answer)
}

/// Opens this page's address as a tab and closes the window.
pub async fn pop_in() -> Result<Value, String> {
    if !agent(|agent| agent.popped) {
        return Err("This page is in a tab already.".into());
    }
    host::open_view_with("browser", "tab", &reopen_args(json!({}))).await?;
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

/// Asks the person for a step (a sign-in, a captcha) in a banner on the page, and waits for them to press Done.
async fn handoff(op: &Value) -> Result<Value, String> {
    let message = text(op, "message")?.to_owned();
    let (sender, receiver) = oneshot();
    agent(|agent| agent.handoff = Some((message, Some(sender))));
    let done = within(op["timeout_ms"].as_f64().unwrap_or(120_000.0) as u32, receiver).await;
    agent(|agent| agent.handoff = None);
    match done {
        Some(_) => Ok(status()),
        None => Err("The person didn't press Done in time. Ask them again, or carry on without that step.".into()),
    }
}

/// Runs one thing main asked.
pub async fn run(op: Value) -> Result<Value, String> {
    let string = |key: &str| text(&op, key).map(|value| json!(value));
    match op["op"].as_str().unwrap_or_default() {
        "status" => Ok(status()),
        "navigate" => navigate(&op).await,
        "screenshot" => {
            let mut shot = command(|token| wings_plugin::ui::web_snapshot(token)).await?;
            shot["url"] = json!(with(|page| page.state.url.clone()));
            Ok(shot)
        }
        "snapshot" => helper("snapshot", &[json!(SNAPSHOT_MAX)]).await,
        "click" | "hover" => helper(op["op"].as_str().unwrap_or_default(), &[string("ref")?]).await,
        "fill" => helper("fill", &[string("ref")?, string("value")?]).await,
        "type" => helper("type", &[string("text")?]).await,
        "press" => helper("press", &[string("key")?]).await,
        "select" => helper("select", &[string("ref")?, op["values"].clone()]).await,
        "scroll" => helper("scroll", &[json!({ "ref": op["ref"], "to": op["to"], "x": op["x"], "y": op["y"] })]).await,
        "drag" => helper("drag", &[string("from")?, string("to")?]).await,
        "eval" => command(|token| wings_plugin::ui::web_eval(op["script"].as_str().unwrap_or_default(), true, token)).await,
        "wait_for" => wait_for(&op).await,
        "pop_out" => pop_out(size_of(&op)).await,
        "pop_in" => pop_in().await,
        "resize" => resize(size_of(&op)).await,
        "handoff" => handoff(&op).await,
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

/// Pop out, or put back when this page is a window: its toolbar button.
pub fn pop_button_action() {
    spawn(async {
        let popped = agent(|agent| agent.popped);
        if let Err(error) = if popped { pop_in().await } else { pop_out(None).await } {
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
        with(|page| page.state.url = url.into());
        agent(|agent| agent.popped = popped);
    }

    #[test]
    fn popping_out_opens_the_same_address_in_a_window_of_that_size_and_only_then_closes_the_tab() {
        let calls = record();
        shown("https://a.com/x", false);
        let out = run_op(json!({ "op": "pop_out", "width": 390.0, "height": 844.0 }));
        assert_eq!(out.borrow().clone(), Some(Ok(json!({ "width": 390, "height": 844 }))));
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
    fn a_script_runs_as_a_command_with_a_token_and_its_result_comes_back_by_that_token() {
        record();
        let out = run_op(json!({ "op": "eval", "script": "return 1" }));
        let command = with(|page| page.command.clone()).unwrap();
        assert_eq!(command["action"], "eval");
        assert_eq!(command["world"], "page");
        assert!(out.borrow().is_none(), "waiting for the view");
        result(&json!({ "token": command["token"], "value": 1 }));
        testing::settle();
        assert_eq!(out.borrow().clone(), Some(Ok(json!(1))));
        // The helper's calls run in the isolated world, and a failure is the view's error.
        let out = run_op(json!({ "op": "click", "ref": "e1" }));
        let command = with(|page| page.command.clone()).unwrap();
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
}
