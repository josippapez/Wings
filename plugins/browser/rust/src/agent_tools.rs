//! The tools Claude uses to drive a Browser page (`browser__open`, `browser__click`...). Main answers them: it knows the pages
//! because each one tells it who and where it is (`agent_page.rs`), picks the page of the caller's project that had the
//! focus last, and asks it to do the work, since only a page can run a `WebView`'s commands.

use crate::{
    protocol::{ANNOUNCE, WAIT_POLL_MS, WHO, cmd_name},
    wait::within,
};
use serde_json::{Value, json};
use std::cell::RefCell;
use wings_plugin::{
    host, messages,
    task::join_all,
    tools::{self, with_image},
};

const MANIFEST: &str = include_str!("../../wings-plugin.json");

/// A page that doesn't answer a status ask in this long is gone: pages can't say when they close.
const PING_MS: u32 = 1500;
/// Most of what a page does answers in seconds; a wait or a handoff says how long it will take.
const OP_MS: u32 = 40_000;
/// How long a window or tab takes to open and tell main it is there.
const OPEN_MS: f64 = 10_000.0;
/// The most characters of an `eval` answer.
const EVAL_MAX: usize = 20_000;

const NO_PAGE: &str = "No Browser page is open for this project. Call browser__open with an address first.";

/// What the agent can ask a window to be. Wings keeps it on the screen and no smaller than 320 by 240.
const PRESETS: [(&str, (f64, f64)); 3] = [("desktop", (1440.0, 900.0)), ("tablet", (834.0, 1194.0)), ("phone", (390.0, 844.0))];

#[derive(Clone, Debug, PartialEq)]
struct Info {
    id: String,
    project: String,
    popped: bool,
    url: String,
    title: String,
    loading: bool,
    /// When this page last had the focus: the page of a project that has it most recently is the one a tool uses.
    focused_at: f64,
    /// The size last set by a tool, since the person can drag a window to another.
    size: Option<(f64, f64)>,
}

thread_local! {
    static PAGES: RefCell<Vec<Info>> = const { RefCell::new(Vec::new()) };
}

fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_owned()
}

/// Keeps what a page told: new pages are the most recent; a page keeps its size and, unless it has the focus now, its time.
fn hear(page: &Value) {
    let id = text(page, "id");
    if id.is_empty() {
        return;
    }
    PAGES.with(|pages| {
        let mut pages = pages.borrow_mut();
        let old = pages.iter().position(|kept| kept.id == id).map(|at| pages.remove(at));
        let focused_at = match (&old, page["focused"] == true) {
            (Some(old), false) => old.focused_at,
            _ => host::now(),
        };
        pages.push(Info {
            id,
            project: text(page, "project"),
            popped: page["popped"] == true,
            url: text(page, "url"),
            title: text(page, "title"),
            loading: page["loading"] == true,
            focused_at,
            size: old.and_then(|old| old.size),
        });
    });
}

fn forget(id: &str) {
    PAGES.with(|pages| pages.borrow_mut().retain(|page| page.id != id));
}

fn set_size(id: &str, size: Option<(f64, f64)>) {
    PAGES.with(|pages| pages.borrow_mut().iter_mut().filter(|page| page.id == id).for_each(|page| page.size = size));
}

fn get(id: &str) -> Option<Info> {
    PAGES.with(|pages| pages.borrow().iter().find(|page| page.id == id).cloned())
}

fn ids() -> Vec<String> {
    PAGES.with(|pages| pages.borrow().iter().map(|page| page.id.clone()).collect())
}

/// The pages of a project, the one with the focus last first. A page that doesn't know its project yet counts for any.
fn candidates(project: &str) -> Vec<Info> {
    let mut pages: Vec<Info> = PAGES.with(|pages| pages.borrow().iter().filter(|page| page.project.is_empty() || project.is_empty() || page.project == project).cloned().collect());
    pages.sort_by(|a, b| b.focused_at.total_cmp(&a.focused_at));
    pages
}

pub fn start() {
    messages::on(ANNOUNCE, hear);
    // Pages that were there before this main started tell it again.
    messages::tell(WHO, &Value::Null);
    for (name, schema) in schemas() {
        let tool = name.clone();
        tools::register(&name, schema, move |input, context| run(tool.clone(), input, context));
    }
}

fn schemas() -> Vec<(String, Value)> {
    let manifest: Value = serde_json::from_str(MANIFEST).expect("the manifest is JSON");
    manifest["contributes"]["mcpTools"].as_array().into_iter().flatten().map(|tool| (text(tool, "name"), tool["inputSchema"].clone())).collect()
}

pub fn preset(name: &str) -> Option<(f64, f64)> {
    PRESETS.iter().find(|(known, _)| *known == name).map(|(_, size)| *size)
}

/// The window size a call asks for: its `width` and `height`, which win over a `preset`, or none.
pub fn size_of(input: &Value) -> Result<Option<(f64, f64)>, String> {
    match (input["width"].as_f64(), input["height"].as_f64(), input["preset"].as_str()) {
        (Some(width), Some(height), _) => Ok(Some((width, height))),
        (None, None, Some(name)) => preset(name).map(Some).ok_or_else(|| format!("Unknown preset {name}.")),
        (None, None, None) => Ok(None),
        _ => Err("width and height go together: give both, or a preset instead.".into()),
    }
}

fn sized(size: (f64, f64)) -> String {
    format!("{}x{}", size.0, size.1)
}

fn describe(page: &Info) -> String {
    let place = match (page.popped, page.size) {
        (true, Some(size)) => format!("in a window (set to {})", sized(size)),
        (true, None) => "in a window".to_owned(),
        (false, _) => "in a tab".to_owned(),
    };
    let title = if page.title.is_empty() { String::new() } else { format!(" \"{}\"", page.title) };
    format!("Browser page {} {place}: {}{title}{}", page.id, if page.url.is_empty() { "(no address yet)" } else { &page.url }, if page.loading { " (still loading)" } else { "" })
}

/// What a tool that takes a ref or text sends the page, and how long the page may take: nothing for the tools main does itself.
fn op_for(tool: &str, input: &Value) -> Result<(Value, u32), String> {
    let has = |key: &str| input.get(key).is_some();
    let op = |name: &str, keys: &[&str]| {
        let mut op = json!({ "op": name });
        for key in keys {
            op[*key] = input[*key].clone();
        }
        op
    };
    Ok(match tool {
        "navigate" => {
            if has("url") == has("action") {
                return Err("navigate takes a url, or an action of back, forward or reload, and not both.".into());
            }
            (op("navigate", &["url", "action"]), OP_MS)
        }
        "screenshot" | "snapshot" => (op(tool, &[]), OP_MS),
        "click" | "hover" => (op(tool, &["ref"]), OP_MS),
        "fill" => (op("fill", &["ref", "value"]), OP_MS),
        "type" => (op("type", &["text"]), OP_MS),
        "press" => (op("press", &["key"]), OP_MS),
        "select" => (op("select", &["ref", "values"]), OP_MS),
        "scroll" => {
            if !has("ref") && !has("to") && !has("x") && !has("y") {
                return Err("scroll takes a ref, a to of top or bottom, or x and y pixels.".into());
            }
            (op("scroll", &["ref", "to", "x", "y"]), OP_MS)
        }
        "drag" => (op("drag", &["from", "to"]), OP_MS),
        "eval" => (op("eval", &["script"]), OP_MS),
        // One command answers both; the tool keeps its half.
        "console" | "network" => (op("logs", &[]), OP_MS),
        "dialog" => (op("dialog", &["accept", "text"]), OP_MS),
        "wait_for" => {
            if !has("text") && !has("selector") && !has("url") {
                return Err("wait_for takes text, selector or url.".into());
            }
            let ms = input["timeout_ms"].as_u64().unwrap_or(10_000);
            let mut wait = op("wait_for", &["text", "selector", "url"]);
            wait["timeout_ms"] = json!(ms);
            // The page gives up first, so its message says what it waited for.
            (wait, ms as u32 + 5000)
        }
        "handoff" => {
            let ms = input["timeout_ms"].as_u64().unwrap_or(120_000);
            let mut handoff = op("handoff", &["message"]);
            handoff["timeout_ms"] = json!(ms);
            (handoff, ms as u32 + 5000)
        }
        other => return Err(format!("Browser has no tool {other}.")),
    })
}

async fn ask(id: &str, op: Value, ms: u32) -> Result<Value, String> {
    messages::ask_within(&cmd_name(id), op, ms).await
}

/// The page of the project that had the focus last and is still there, from its own answer. A page that doesn't answer is dropped.
async fn live_page(project: &str) -> Option<Info> {
    for page in candidates(project) {
        match ask(&page.id, json!({ "op": "status" }), PING_MS).await {
            Ok(status) => {
                hear(&status);
                return get(&page.id);
            }
            Err(_) => forget(&page.id),
        }
    }
    None
}

/// The first page that wasn't in `known`, once it has told main about itself.
async fn new_page(known: &[String], limit_ms: f64) -> Option<Info> {
    let begun = host::now();
    loop {
        if let Some(page) = PAGES.with(|pages| pages.borrow().iter().find(|page| !known.contains(&page.id)).cloned()) {
            return Some(page);
        }
        if host::now() - begun >= limit_ms {
            return None;
        }
        host::sleep(WAIT_POLL_MS / 2).await;
    }
}

fn project_of(context: &Value) -> String {
    text(&context["project"], "id")
}

async fn run(tool: String, input: Value, context: Value) -> Result<Value, String> {
    let project = project_of(&context);
    match tool.as_str() {
        "open" => open(&project, &input).await,
        "pop_out" | "resize" | "emulate" => sizing(&tool, &project, &input).await,
        "pop_in" => pop_in(&project).await,
        "status" => status(&project).await,
        // With no page, a navigation is an open.
        "navigate" if input["url"].is_string() && live_page(&project).await.is_none() => open(&project, &input).await,
        _ => forward(&tool, &project, &input).await,
    }
}

/// Opens a page, in a window when asked, at `url` (or the engine's home for none), and gives it once it is there.
async fn open_new(url: Option<&str>, size: Option<(f64, f64)>, window: bool) -> Result<Info, String> {
    let known = ids();
    let args = match url {
        Some(url) => json!({ "url": url, "window": window }),
        None => json!({ "window": window }),
    };
    let answer = if window { host::open_window("browser", &args, size).await? } else { host::open_view_with("browser", "tab", &args).await? };
    let page = new_page(&known, OPEN_MS).await.ok_or("The Browser page didn't open. Is the Browser plugin on?")?;
    set_size(&page.id, window.then(|| size_answered(&answer).or(size)).flatten());
    Ok(get(&page.id).unwrap_or(page))
}

/// The size the app gave a window, which it keeps on the screen.
fn size_answered(answer: &Value) -> Option<(f64, f64)> {
    Some((answer["width"].as_f64()?, answer["height"].as_f64()?))
}

async fn open(project: &str, input: &Value) -> Result<Value, String> {
    let url = text(input, "url");
    let size = size_of(input)?;
    let pop = input["pop_out"] == true || size.is_some();
    let Some(page) = live_page(project).await else {
        return Ok(describe(&open_new(Some(&url), size, pop).await?).into());
    };
    ask(&page.id, json!({ "op": "navigate", "url": url }), OP_MS).await?;
    let id = if pop && !page.popped { move_out(&page, size).await?.id } else { page.id.clone() };
    if pop && page.popped && size.is_some() {
        resize_window(&page.id, size).await?;
    }
    Ok(describe(&get(&id).ok_or(NO_PAGE)?).into())
}

async fn resize_window(id: &str, size: Option<(f64, f64)>) -> Result<(), String> {
    let (width, height) = size.ok_or("A width and a height, or a preset, is needed.")?;
    let answer = ask(id, json!({ "op": "resize", "width": width, "height": height }), OP_MS).await?;
    set_size(id, size_answered(&answer).or(size));
    Ok(())
}

/// A page from a tab to a window: the page opens it and closes its tab, and the window's page is the one that tells main next.
async fn move_out(page: &Info, size: Option<(f64, f64)>) -> Result<Info, String> {
    let known = ids();
    let op = match size {
        Some((width, height)) => json!({ "op": "pop_out", "width": width, "height": height }),
        None => json!({ "op": "pop_out" }),
    };
    let answer = ask(&page.id, op, OP_MS).await?;
    forget(&page.id);
    let window = new_page(&known, OPEN_MS).await.ok_or("The window didn't open.")?;
    set_size(&window.id, size_answered(&answer).or(size));
    Ok(get(&window.id).unwrap_or(window))
}

async fn sizing(tool: &str, project: &str, input: &Value) -> Result<Value, String> {
    let size = size_of(input)?;
    if tool != "pop_out" && size.is_none() {
        return Err(format!("{tool} takes a width and a height, or a preset of desktop, tablet or phone."));
    }
    let Some(page) = live_page(project).await else {
        if tool == "pop_out" {
            return Ok(describe(&open_new(None, size, true).await?).into());
        }
        return Err(NO_PAGE.into());
    };
    if page.popped {
        if size.is_some() {
            resize_window(&page.id, size).await?;
        }
        return Ok(describe(&get(&page.id).unwrap_or(page)).into());
    }
    if tool != "pop_out" {
        return Err("This page is in a tab, so it has no window to resize. Call pop_out first (it takes a width and height or a preset), then resize.".into());
    }
    Ok(describe(&move_out(&page, size).await?).into())
}

async fn pop_in(project: &str) -> Result<Value, String> {
    let page = live_page(project).await.ok_or(NO_PAGE)?;
    if !page.popped {
        return Ok(format!("Already in a tab. {}", describe(&page)).into());
    }
    let known = ids();
    ask(&page.id, json!({ "op": "pop_in" }), OP_MS).await?;
    forget(&page.id);
    // A tab with the same address that is open already is shown instead of a new one, so no new page may come.
    let tab = new_page(&known, 3000.0).await;
    Ok(tab.map_or_else(|| "Put back in a tab.".to_owned(), |tab| describe(&tab)).into())
}

async fn status(project: &str) -> Result<Value, String> {
    let pages = candidates(project);
    let asks: Vec<_> = pages.iter().map(|page| { let id = page.id.clone(); Box::pin(async move { ask(&id, json!({ "op": "status" }), PING_MS).await }) } as std::pin::Pin<Box<dyn std::future::Future<Output = _>>>).collect();
    for (page, answer) in pages.iter().zip(join_all(asks).await) {
        match answer {
            Ok(status) => hear(&status),
            Err(_) => forget(&page.id),
        }
    }
    let lines: Vec<String> = candidates(project).iter().map(describe).collect();
    Ok(if lines.is_empty() { "No Browser page is open for this project. browser__open opens one.".into() } else { lines.join("\n") }.into())
}

fn cut(text: String, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((at, _)) => format!("{}\n... cut at {max} characters", &text[..at]),
        None => text,
    }
}

/// What the tool answers from what its page did.
fn present(tool: &str, answer: Value) -> Value {
    match tool {
        "screenshot" => {
            let (path, width, height) = (text(&answer, "path"), answer["width"].as_u64().unwrap_or(0), answer["height"].as_u64().unwrap_or(0));
            let note = format!("Screenshot of the visible area of {} ({width}x{height} px), saved to {path}. It shows only what is on screen: scroll to see more.", text(&answer, "url"));
            with_image(&note, &path)
        }
        "navigate" | "wait_for" | "handoff" => {
            let page = Info { id: text(&answer, "id"), project: String::new(), popped: answer["popped"] == true, url: text(&answer, "url"), title: text(&answer, "title"), loading: answer["loading"] == true, focused_at: 0.0, size: get(&text(&answer, "id")).and_then(|page| page.size) };
            describe(&page).into()
        }
        "eval" => cut(serde_json::to_string_pretty(&answer).unwrap_or_default(), EVAL_MAX).into(),
        "console" | "network" => cut(serde_json::to_string_pretty(&answer[tool]).unwrap_or_default(), EVAL_MAX).into(),
        "dialog" => "Answered the page's dialog.".into(),
        _ => answer,
    }
}

async fn forward(tool: &str, project: &str, input: &Value) -> Result<Value, String> {
    let (op, ms) = op_for(tool, input)?;
    let page = live_page(project).await.ok_or(NO_PAGE)?;
    // A wait that outlasts the ask's own limit is the page's to end, so a hung page still answers.
    let answer = within(ms + 1000, ask(&page.id, op, ms)).await.ok_or("The page took too long to answer.")??;
    Ok(present(tool, answer))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_js::SNAPSHOT_MAX;
    use std::{cell::RefCell, rc::Rc};
    use wings_plugin::host::testing;

    #[test]
    fn presets_are_the_three_sizes_and_explicit_sizes_win() {
        assert_eq!(preset("desktop"), Some((1440.0, 900.0)));
        assert_eq!(preset("tablet"), Some((834.0, 1194.0)));
        assert_eq!(preset("phone"), Some((390.0, 844.0)));
        assert_eq!(size_of(&json!({ "preset": "phone" })), Ok(Some((390.0, 844.0))));
        assert_eq!(size_of(&json!({ "preset": "phone", "width": 500, "height": 600 })), Ok(Some((500.0, 600.0))));
        assert_eq!(size_of(&json!({})), Ok(None));
        assert!(size_of(&json!({ "width": 500 })).unwrap_err().contains("together"));
        assert!(size_of(&json!({ "preset": "watch" })).unwrap_err().contains("Unknown preset"));
    }

    #[test]
    fn every_tool_in_the_manifest_has_a_schema_and_a_route() {
        let names: Vec<String> = schemas().into_iter().map(|(name, _)| name).collect();
        for tool in ["open", "pop_out", "pop_in", "resize", "navigate", "status", "screenshot", "snapshot", "click", "fill", "type", "press", "wait_for", "eval", "hover", "select", "scroll", "drag", "handoff", "emulate", "console", "network", "dialog"] {
            assert!(names.iter().any(|name| name == tool), "{tool} is in the manifest");
        }
        let manifest: Value = serde_json::from_str(MANIFEST).unwrap();
        assert!(manifest["contributes"]["mcpInstructions"].as_str().unwrap().chars().count() <= 500);
        // Each tool that main forwards turns its input into an op without a page.
        for (tool, input) in [("click", json!({ "ref": "e1" })), ("snapshot", json!({})), ("eval", json!({ "script": "return 1" }))] {
            assert!(op_for(tool, &input).is_ok(), "{tool}");
        }
        // The console and the requests come from one ask of the page, and each tool answers its own half.
        assert_eq!(op_for("console", &json!({})).unwrap().0, json!({ "op": "logs" }));
        assert_eq!(op_for("network", &json!({})).unwrap().0, json!({ "op": "logs" }));
        let logs = json!({ "console": [{ "level": "error", "text": "boom" }], "network": [{ "method": "GET", "url": "https://a.com/x", "status": 500 }] });
        assert!(present("console", logs.clone()).as_str().unwrap().contains("boom"));
        assert!(!present("network", logs).as_str().unwrap().contains("boom"));
        assert_eq!(op_for("dialog", &json!({ "accept": false })).unwrap().0, json!({ "op": "dialog", "accept": false, "text": null }));
    }

    #[test]
    fn a_tool_sends_the_page_the_fields_it_takes_and_refuses_a_call_that_is_unclear() {
        assert_eq!(op_for("fill", &json!({ "ref": "e2", "value": "hi" })).unwrap().0, json!({ "op": "fill", "ref": "e2", "value": "hi" }));
        assert_eq!(op_for("navigate", &json!({ "action": "back" })).unwrap().0, json!({ "op": "navigate", "url": null, "action": "back" }));
        assert!(op_for("navigate", &json!({})).is_err());
        assert!(op_for("navigate", &json!({ "url": "a.com", "action": "back" })).is_err());
        assert!(op_for("scroll", &json!({})).is_err());
        assert!(op_for("wait_for", &json!({})).is_err());
        let (wait, ms) = op_for("wait_for", &json!({ "text": "Done", "timeout_ms": 2000 })).unwrap();
        assert_eq!((wait["timeout_ms"].clone(), ms), (json!(2000), 7000));
        assert!(SNAPSHOT_MAX > 1000);
    }

    #[test]
    fn the_page_with_the_latest_focus_in_the_callers_project_is_the_one_used() {
        PAGES.with(|pages| pages.borrow_mut().clear());
        testing::set_now(100.0);
        hear(&json!({ "id": "a", "project": "/p", "url": "https://a", "focused": true }));
        testing::set_now(200.0);
        hear(&json!({ "id": "b", "project": "/p", "url": "https://b", "focused": true }));
        hear(&json!({ "id": "c", "project": "/other", "url": "https://c", "focused": true }));
        testing::set_now(300.0);
        // An announcement without focus keeps its time.
        hear(&json!({ "id": "a", "project": "/p", "url": "https://a2", "focused": false }));
        let order: Vec<String> = candidates("/p").into_iter().map(|page| page.id).collect();
        assert_eq!(order, ["b", "a"]);
        hear(&json!({ "id": "a", "project": "/p", "focused": true }));
        assert_eq!(candidates("/p")[0].id, "a");
        assert_eq!(describe(&get("c").unwrap()), "Browser page c in a tab: https://c");
    }

    /// A page that answers main's asks as the page frame would, with `reply` for all but status, and records them.
    fn fake_page(id: &str, popped: bool, reply: impl Fn(&Value) -> Result<Value, String> + 'static) -> Rc<RefCell<Vec<Value>>> {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let log = seen.clone();
        let status = json!({ "id": id, "project": "/p", "popped": popped, "url": "https://a.com/", "title": "A", "loading": false, "focused": true });
        messages::answer(&cmd_name(id), move |op| {
            log.borrow_mut().push(op.clone());
            let answer = if op["op"] == "status" { Ok(status.clone()) } else { reply(&op) };
            async move { answer }
        });
        // Each page that comes is the one that had the focus last.
        testing::set_now(host::now() + 1.0);
        hear(&json!({ "id": id, "project": "/p", "popped": popped, "focused": true }));
        seen
    }

    fn call(tool: &str, input: Value) -> Result<Value, String> {
        let out = Rc::new(RefCell::new(None));
        let slot = out.clone();
        let tool = tool.to_owned();
        let context = json!({ "project": { "id": "/p" } });
        wings_plugin::task::spawn(async move { *slot.borrow_mut() = Some(run(tool, input, context).await) });
        testing::settle();
        let done = out.borrow_mut().take();
        done.expect("the call finished")
    }

    fn fresh() {
        PAGES.with(|pages| pages.borrow_mut().clear());
        testing::answer(|_, _| Ok(Value::Null));
    }

    #[test]
    fn a_tool_call_goes_to_the_page_of_the_project_and_its_answer_comes_back() {
        fresh();
        let seen = fake_page("a", false, |op| Ok(json!(format!("did {}", op["op"].as_str().unwrap()))));
        assert_eq!(call("click", json!({ "ref": "e1" })), Ok(json!("did click")));
        assert_eq!(seen.borrow().last(), Some(&json!({ "op": "click", "ref": "e1" })));
        assert_eq!(call("fill", json!({ "ref": "e1", "value": "x" })), Ok(json!("did fill")));
        // The page's own error is the tool's.
        let _ = fake_page("b", false, |_| Err("Ref e9 is stale".into()));
        assert_eq!(call("click", json!({ "ref": "e9" })), Err("Ref e9 is stale".into()));
        assert!(call("click", json!({})).is_err(), "no ref, no op");
    }

    #[test]
    fn a_screenshot_answers_with_the_picture_and_its_path() {
        fresh();
        fake_page("a", false, |_| Ok(json!({ "path": "/w/s.png", "width": 800, "height": 600, "url": "https://a.com/" })));
        let answer = call("screenshot", json!({})).unwrap();
        assert_eq!(answer["image"], "/w/s.png");
        assert!(answer["text"].as_str().unwrap().contains("/w/s.png") && answer["text"].as_str().unwrap().contains("visible area"));
    }

    #[test]
    fn an_eval_answer_is_json_and_cut_when_long() {
        fresh();
        fake_page("a", false, |_| Ok(json!("x".repeat(30_000))));
        let answer = call("eval", json!({ "script": "return 1" })).unwrap();
        let text = answer.as_str().unwrap();
        assert!(text.starts_with("\"x") && text.ends_with("cut at 20000 characters") && text.len() < 20_100);
    }

    #[test]
    fn resize_says_to_pop_out_first_for_a_page_in_a_tab_and_resizes_a_window() {
        fresh();
        let seen = fake_page("a", false, |_| Ok(Value::Null));
        let error = call("resize", json!({ "preset": "phone" })).unwrap_err();
        assert!(error.contains("pop_out first"), "{error}");
        assert!(call("resize", json!({})).unwrap_err().contains("width and a height"));
        assert!(seen.borrow().iter().all(|op| op["op"] == "status"), "nothing but a status ask reached the page");
        fresh();
        let seen = fake_page("w", true, |_| Ok(json!({ "width": 390, "height": 844 })));
        let answer = call("resize", json!({ "preset": "phone" })).unwrap();
        assert_eq!(seen.borrow().last(), Some(&json!({ "op": "resize", "width": 390.0, "height": 844.0 })));
        assert!(answer.as_str().unwrap().contains("in a window (set to 390x844)"), "{answer}");
    }

    #[test]
    fn popping_out_asks_the_tab_to_open_the_window_then_follows_the_page_that_takes_its_place() {
        fresh();
        // The tab's page opens the window, and the window's page tells main once it has started.
        messages::on(ANNOUNCE, hear);
        let seen = fake_page("tab", false, |_| {
            messages::tell(ANNOUNCE, &json!({ "id": "win", "project": "/p", "popped": true, "url": "https://a.com/", "focused": true }));
            Ok(json!({ "width": 390, "height": 844 }))
        });
        let answer = call("pop_out", json!({ "preset": "phone" })).unwrap();
        assert_eq!(seen.borrow().last(), Some(&json!({ "op": "pop_out", "width": 390.0, "height": 844.0 })));
        assert!(get("tab").is_none() && get("win").is_some(), "the tab's page is forgotten, the window's is known");
        assert!(answer.as_str().unwrap().contains("Browser page win in a window (set to 390x844)"), "{answer}");
    }

    #[test]
    fn with_no_page_a_tool_says_to_open_one_and_status_says_none_is_open() {
        fresh();
        assert_eq!(call("click", json!({ "ref": "e1" })), Err(NO_PAGE.into()));
        assert!(call("status", json!({})).unwrap().as_str().unwrap().contains("No Browser page is open"));
        assert!(call("pop_in", json!({})).is_err());
    }

    #[test]
    fn a_page_that_stops_answering_is_dropped_and_the_next_is_used() {
        fresh();
        fake_page("alive", false, |_| Ok(Value::Null));
        testing::set_now(host::now() + 1.0);
        hear(&json!({ "id": "gone", "project": "/p", "focused": true }));
        // `gone` has no answerer, so its ask waits out the ping.
        let out = Rc::new(RefCell::new(None));
        let slot = out.clone();
        wings_plugin::task::spawn(async move { *slot.borrow_mut() = Some(live_page("/p").await) });
        testing::settle();
        assert!(out.borrow().is_none(), "still waiting on the page that doesn't answer");
        testing::fire_timers();
        testing::settle();
        assert_eq!(out.borrow().clone().flatten().map(|page| page.id), Some("alive".to_owned()));
        assert!(get("gone").is_none());
    }
}
