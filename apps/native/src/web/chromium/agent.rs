//! What a plugin's scripts, pictures and logs do on a Chromium page, over the page's own DevTools channel (`devtools`),
//! by WebKit's rules (`web::guard`, `mac.rs`): the plugin's helper runs in a world of its own (`PLUGIN_WORLD`), an agent's
//! script in the page's own world only where the guard lets it, a picture has the secret fields painted over, and the
//! logs are the main frame's console and requests, never a site's signed in with imported cookies.
//!
//! The guard's watch runs in Wings' own world of every frame as it starts (`Page.addScriptToEvaluateOnNewDocument`),
//! the frames of other sites too, each in a process of its own that the page's session attaches to before it runs
//! (`Target.setAutoAttach`), and tells a binding only that world has (`Runtime.addBinding` by its name). Each script
//! runs in the context of the document it was meant for (`uniqueContextId`): one that went with its document fails
//! rather than run in the next one.

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
    rc::Rc,
    time::Instant,
};

use base64::Engine;
use serde_json::{Value, json};

use super::{
    devtools::{Channel, Incoming},
    page::{Shared, weak_of},
};
use crate::web::{
    guard::{self, FUNCTIONS, WATCH_FUNCTION, WORLD},
    mac::{LOGS, LOGS_KEPT, MOVED_ON},
    views::Heard,
};

/// The plugin's own world, where its helper runs: it sees the page's document and none of its scripts.
pub const PLUGIN_WORLD: &str = "wings-plugin";
/// The bindings Wings' world and the page's tell.
const GUARD_BINDING: &str = "wingsGuard";
const LOG_BINDING: &str = "wingsLog";

/// What a script returned that isn't JSON, as WebKit says it (`mac::json_of`).
const NOT_JSON: &str = "The script returned something that isn't JSON: return plain values, arrays and objects";

/// A page's side of the agent's commands.
#[derive(Default)]
pub struct Agent {
    channel: RefCell<Option<Rc<Channel>>>,
    /// The channel was readied whole (`setup`): without that, nothing reads or acts on the page.
    guarded: Cell<bool>,
    contexts: RefCell<Contexts>,
    /// The secret fields its documents showed, by `Contexts::doc`.
    seen: guard::Seen,
    /// The main frame's recent console lines and requests (`mac::LOGS`), the newest last.
    logs: RefCell<VecDeque<Value>>,
}

impl Agent {
    fn channel(&self) -> Option<Rc<Channel>> {
        self.channel.borrow().clone().filter(|_| self.guarded.get())
    }

    /// A new document came: its logs start empty.
    pub fn clear_logs(&self) {
        self.logs.borrow_mut().clear();
    }

    /// What a debug build's checks read of it (`debug`).
    #[cfg(debug_assertions)]
    pub fn describe(&self) -> String {
        let contexts = self.contexts.borrow();
        let main: Vec<String> = [Some(PLUGIN_WORLD), Some(WORLD), None].iter().map(|world| format!("{world:?}={}", contexts.of_main(*world).is_some())).collect();
        format!(
            "guarded {}, doc {}, main frame {:?}, contexts {}, main's {}, secret seen {}, logs {}",
            self.guarded.get(),
            contexts.doc,
            contexts.main_frame,
            contexts.all.len(),
            main.join(" "),
            self.seen.secret(contexts.doc),
            self.logs.borrow().len()
        )
    }
}

/// One JavaScript context of the page or of a frame of another site, as `Runtime.executionContextCreated` described it.
#[derive(Clone, Debug, PartialEq)]
pub struct Context {
    /// `uniqueId`: unique in all of Chromium, so a script sent to it never runs in another.
    pub unique: String,
    /// Its world's name, empty for the page's own.
    pub name: String,
    pub frame: String,
    /// The frame's own world.
    pub default: bool,
    pub origin: String,
    /// The main frame's document it came with (`Contexts::doc`).
    pub doc: u64,
}

/// The contexts alive now, by their session (none for the page's own) and id.
#[derive(Default)]
pub struct Contexts {
    all: HashMap<(Option<String>, i64), Context>,
    main_frame: Option<String>,
    /// Counts the main frame's documents, by the contexts of its own world each one made: the guard keeps what each
    /// showed by it. It comes over the same channel as what the document's scripts tell, in order.
    doc: u64,
}

impl Contexts {
    pub fn doc(&self) -> u64 {
        self.doc
    }

    fn is_main(&self, session: Option<&str>, frame: &str) -> bool {
        session.is_none() && self.main_frame.as_deref() == Some(frame)
    }

    /// The main frame is `frame`: the document there now is a new one to the guard.
    pub fn main_frame_is(&mut self, frame: String) {
        self.doc += 1;
        let doc = self.doc;
        for ((session, _), context) in &mut self.all {
            if session.is_none() && context.frame == frame {
                context.doc = doc;
            }
        }
        self.main_frame = Some(frame);
    }

    /// A context was made, from its description.
    pub fn created(&mut self, session: Option<&str>, context: &Value) {
        let (Some(id), Some(unique)) = (context["id"].as_i64(), context["uniqueId"].as_str()) else { return };
        let frame = context["auxData"]["frameId"].as_str().unwrap_or_default().to_owned();
        let default = context["auxData"]["isDefault"] == true;
        if default && self.is_main(session, &frame) {
            self.doc += 1;
        }
        let context = Context {
            unique: unique.to_owned(),
            name: context["name"].as_str().unwrap_or_default().to_owned(),
            frame,
            default,
            origin: context["origin"].as_str().unwrap_or_default().to_owned(),
            doc: self.doc,
        };
        self.all.insert((session.map(str::to_owned), id), context);
    }

    pub fn destroyed(&mut self, session: Option<&str>, id: i64) {
        self.all.remove(&(session.map(str::to_owned), id));
    }

    /// Every context of `session` went (`Runtime.executionContextsCleared`), or the session itself.
    pub fn cleared(&mut self, session: Option<&str>) {
        self.all.retain(|(of, _), _| of.as_deref() != session);
    }

    pub fn get(&self, session: Option<&str>, id: i64) -> Option<&Context> {
        self.all.get(&(session.map(str::to_owned), id))
    }

    /// The main frame's context of `world` (its own for none) in the document there now. An extension's content scripts
    /// run in worlds of their own too, named by the extension, which may be any name: those are never Wings'.
    pub fn of_main(&self, world: Option<&str>) -> Option<&Context> {
        self.all.iter().find_map(|((session, _), context)| {
            let wanted = match world {
                None => context.default,
                Some(name) => !context.default && context.name == name && !context.origin.starts_with("chrome-extension://"),
            };
            (wanted && context.doc == self.doc && self.is_main(session.as_deref(), &context.frame)).then_some(context)
        })
    }

    /// Whether `context` of `session` is in a frame of the page rather than its main frame, or isn't known: a frame of
    /// another site runs in a session of its own.
    fn in_frame(&self, session: Option<&str>, context: Option<&Context>) -> bool {
        context.is_none_or(|context| !self.is_main(session, &context.frame))
    }
}

/// `script`, the body of an async function, as an expression `Runtime.evaluate` runs: called at once, it answers what the
/// body returns.
pub fn expression(script: &str) -> String {
    format!("(async function () {{\n{script}\n}})()")
}

/// An agent's `script` for the page's own world: the check runs again first, in front of the function and outside it,
/// so a field that came after Wings' check refuses it too, whatever the script holds (`guard::checked_again`).
pub fn in_page(script: &str) -> String {
    guard::checked_again(&expression(script))
}

/// The check, which also answers how wide the page's viewport is in CSS pixels, the picture's scale.
pub fn check() -> String {
    expression(&format!("{FUNCTIONS}\nconst found = wingsScan(document);\nfound.width = innerWidth;\nreturn found;"))
}

/// What every frame runs in Wings' world as it starts: the watch, telling the binding only that world has.
pub fn watch() -> String {
    format!(
        "(() => {{\nconst webkit = {{ messageHandlers: {{ {GUARD_BINDING}: {{ postMessage: () => globalThis.{GUARD_BINDING}(\"1\") }} }} }};\n{FUNCTIONS}\n{WATCH_FUNCTION}\n}})();"
    )
}

/// What the main frame runs in the page's own world as it starts: WebKit's log script, posting to the binding. A frame in
/// the page runs none, as in WebKit.
pub fn log_script() -> String {
    format!(
        "(() => {{\nif (window.top !== window) return;\nconst webkit = {{ messageHandlers: {{ {LOG_BINDING}: {{ postMessage: (m) => globalThis.{LOG_BINDING}(JSON.stringify(m)) }} }} }};\n{LOGS}\n}})();"
    )
}

/// What readies a page's session, in order, before it loads anything: the main frame's id first, so the contexts that
/// come next are known by it. Chromium runs a new document's scripts in the page's own world only for a session that
/// enabled `Page` (measured in 156: the log script never ran without it).
pub fn setup() -> Vec<(&'static str, Value)> {
    vec![
        ("Page.getFrameTree", json!({})),
        ("Page.enable", json!({})),
        ("Runtime.enable", json!({})),
        ("Runtime.addBinding", json!({ "name": GUARD_BINDING, "executionContextName": WORLD })),
        ("Runtime.addBinding", json!({ "name": LOG_BINDING })),
        ("Page.addScriptToEvaluateOnNewDocument", json!({ "source": watch(), "worldName": WORLD, "runImmediately": true })),
        ("Page.addScriptToEvaluateOnNewDocument", json!({ "source": format!("//# sourceURL={PLUGIN_WORLD}"), "worldName": PLUGIN_WORLD, "runImmediately": true })),
        ("Page.addScriptToEvaluateOnNewDocument", json!({ "source": log_script(), "runImmediately": true })),
        ("Target.setAutoAttach", auto_attach()),
    ]
}

/// What readies a frame of another site, waiting to start, before it runs: the watch, and the same for its own frames.
/// `Runtime.runIfWaitingForDebugger` follows, whatever these answer.
pub fn frame_setup() -> Vec<(&'static str, Value)> {
    vec![
        ("Page.enable", json!({})),
        ("Runtime.enable", json!({})),
        ("Runtime.addBinding", json!({ "name": GUARD_BINDING, "executionContextName": WORLD })),
        ("Page.addScriptToEvaluateOnNewDocument", json!({ "source": watch(), "worldName": WORLD, "runImmediately": true })),
        ("Target.setAutoAttach", auto_attach()),
    ]
}

/// Frames of other sites are attached to as they come, and wait to start until they are readied.
fn auto_attach() -> Value {
    json!({ "autoAttach": true, "waitForDebuggerOnStart": true, "flatten": true, "filter": [{ "type": "iframe" }, { "exclude": true }] })
}

/// `Runtime.evaluate` of `expression` in the context `unique`, waiting for its promise and answering its value.
fn evaluate(expression: &str, unique: &str) -> Value {
    json!({ "expression": expression, "uniqueContextId": unique, "awaitPromise": true, "returnByValue": true })
}

/// What `Runtime.evaluate` answered, as a script's answer: its value as JSON, `null` for none, or why it failed.
pub fn value_of(answer: Result<Value, String>) -> Result<Value, String> {
    let answer = answer?;
    if let Some(details) = answer.get("exceptionDetails") {
        return Err(exception_text(details));
    }
    let result = &answer["result"];
    if result.get("unserializableValue").is_some() {
        return Err(NOT_JSON.into());
    }
    match (result["type"].as_str(), result["subtype"].as_str()) {
        (Some("undefined"), _) => Ok(Value::Null),
        (Some("string" | "number" | "boolean"), _) | (Some("object"), None | Some("array" | "null")) => Ok(result.get("value").cloned().unwrap_or(Value::Null)),
        _ => Err(NOT_JSON.into()),
    }
}

/// Why a script threw: its error's message, or what it threw.
fn exception_text(details: &Value) -> String {
    let exception = &details["exception"];
    if let Some(thrown) = exception["value"].as_str() {
        return thrown.to_owned();
    }
    // An error's description is its name, message and stack: the first line, without the plain `Error: `.
    if let Some(description) = exception["description"].as_str() {
        let line = description.lines().next().unwrap_or_default();
        return line.strip_prefix("Error: ").unwrap_or(line).to_owned();
    }
    details["text"].as_str().unwrap_or("The script threw").to_owned()
}

/// One line the log script posted, in WebKit's shape (`mac::Observer::heard`), or none for what isn't one.
pub fn log_line(payload: &str) -> Option<Value> {
    let body: Value = serde_json::from_str(payload).ok()?;
    let text = |key: &str| body[key].as_str().map(str::to_owned);
    let number = |key: &str| body[key].as_i64();
    match body["kind"].as_str()? {
        "console" => Some(json!({ "kind": "console", "level": text("level"), "text": text("text") })),
        "network" => Some(json!({ "kind": "network", "method": text("method"), "url": text("url"), "status": number("status"), "error": text("error") })),
        _ => None,
    }
}

/// The host of a context's origin (`https://host:port`), empty for one with none.
fn host_of(origin: &str) -> String {
    crate::web::origin(origin)
        .and_then(|origin| origin.split_once("://").map(|(_, host)| host.split(':').next().unwrap_or_default().to_owned()))
        .unwrap_or_default()
}

/// Opens `page`'s channel on `browser` and readies it (`setup`); `done` hears whether all of it took.
pub fn open(page: &Shared, browser: &cef::Browser, done: impl FnOnce(Result<(), String>) + 'static) {
    let Some(channel) = Channel::open(browser, weak_of(page)) else { return done(Err("Chromium's DevTools didn't open".into())) };
    let channel = Rc::new(channel);
    page.agent.channel.replace(Some(channel.clone()));
    let steps = setup();
    let left = Rc::new(Cell::new(steps.len()));
    let failed: Rc<RefCell<Option<String>>> = Rc::default();
    let done: Rc<RefCell<Option<Box<dyn FnOnce(Result<(), String>)>>>> = Rc::new(RefCell::new(Some(Box::new(done))));
    for (method, params) in steps {
        let (left, failed, done, weak) = (left.clone(), failed.clone(), done.clone(), weak_of(page));
        channel.ask(None, method, params, move |answer| {
            match &answer {
                Ok(tree) if method == "Page.getFrameTree" => match (tree["frameTree"]["frame"]["id"].as_str(), weak.upgrade()) {
                    (Some(frame), Some(page)) => page.agent.contexts.borrow_mut().main_frame_is(frame.to_owned()),
                    _ => *failed.borrow_mut() = Some("no main frame".into()),
                },
                Ok(_) => {}
                Err(why) => *failed.borrow_mut() = Some(format!("{method}: {why}")),
            }
            left.set(left.get() - 1);
            if left.get() == 0 {
                let result = failed.borrow_mut().take().map_or(Ok(()), Err);
                if let Some(page) = weak.upgrade() {
                    page.agent.guarded.set(result.is_ok());
                }
                if let Some(done) = done.borrow_mut().take() {
                    done(result);
                }
            }
        });
    }
}

/// What the page's channel heard.
pub fn heard(page: &Shared, incoming: Incoming) {
    let session = match &incoming {
        Incoming::Answer { .. } => None,
        Incoming::Event { session, .. } => session.clone(),
    };
    let session = session.as_deref();
    match incoming {
        Incoming::Answer { id, result } => {
            let channel = page.agent.channel.borrow().clone();
            if let Some(channel) = channel {
                channel.answered(id, result);
            }
        }
        Incoming::Event { method, params, .. } => match method.as_str() {
            "Runtime.executionContextCreated" => page.agent.contexts.borrow_mut().created(session, &params["context"]),
            "Runtime.executionContextDestroyed" => {
                if let Some(id) = params["executionContextId"].as_i64() {
                    page.agent.contexts.borrow_mut().destroyed(session, id);
                }
            }
            "Runtime.executionContextsCleared" => page.agent.contexts.borrow_mut().cleared(session),
            "Runtime.bindingCalled" => bound(page, session, &params),
            // On a turn of its own: Chromium tells of a frame waiting to start before it can hear that it may, so a
            // `Runtime.runIfWaitingForDebugger` sent while it tells is lost and the frame never loads (measured in 156).
            "Target.attachedToTarget" => {
                let weak = weak_of(page);
                super::next_turn(move || {
                    if let Some(page) = weak.upgrade() {
                        attached(&page, &params);
                    }
                });
            }
            "Target.detachedFromTarget" => {
                if let Some(gone) = params["sessionId"].as_str() {
                    page.agent.contexts.borrow_mut().cleared(Some(gone));
                }
            }
            _ => {}
        },
    }
}

/// A binding was called: the watch in a frame saw a secret field, or the main frame's log script posted a line.
fn bound(page: &Shared, session: Option<&str>, params: &Value) {
    let (Some(name), Some(id)) = (params["name"].as_str(), params["executionContextId"].as_i64()) else { return };
    let contexts = page.agent.contexts.borrow();
    let context = contexts.get(session, id).cloned();
    match name {
        // Only Wings' world has it. Kept for the rest of the main frame's document, and apart when it is a frame in the
        // page, which may be of another origin, so the frames the check can't look into are painted over in a picture.
        GUARD_BINDING => {
            let (doc, in_frame) = (contexts.doc(), contexts.in_frame(session, context.as_ref()));
            drop(contexts);
            page.agent.seen.note(doc, in_frame);
        }
        // The page's own world of its main frame only, and nothing from a site signed in with imported cookies or a
        // context with no host of its own.
        LOG_BINDING => {
            let Some(context) = context.filter(|context| context.default && contexts.is_main(session, &context.frame)) else { return };
            drop(contexts);
            if crate::web::holds_imported_host(&page.web_dir, &host_of(&context.origin)) {
                return;
            }
            let Some(line) = params["payload"].as_str().and_then(log_line) else { return };
            let mut logs = page.agent.logs.borrow_mut();
            logs.push_back(line);
            while logs.len() > LOGS_KEPT * 2 {
                logs.pop_front();
            }
        }
        _ => {}
    }
}

/// A frame of another site, in a process of its own, waits to start: it gets the watch first. One that couldn't be
/// readied counts as holding a secret field, as nothing watches it.
fn attached(page: &Shared, params: &Value) {
    let (Some(session), Some(channel)) = (params["sessionId"].as_str(), page.agent.channel.borrow().clone()) else { return };
    if params["targetInfo"]["type"] == "iframe" {
        for (method, params) in frame_setup() {
            let weak = weak_of(page);
            channel.ask(Some(session), method, params, move |answer| {
                if answer.is_err()
                    && let Some(page) = weak.upgrade()
                {
                    let doc = page.agent.contexts.borrow().doc();
                    page.agent.seen.note(doc, true);
                }
            });
        }
    }
    channel.ask(Some(session), "Runtime.runIfWaitingForDebugger", json!({}), |_| {});
}

/// Whether the page moved on since a command started: another document committed, or came to its main frame.
fn moved(page: &Shared, generation: u64, doc: u64) -> bool {
    page.generation.get() != generation || page.agent.contexts.borrow().doc() != doc
}

/// Runs `script`, the body of an async function, in the plugin's own world, or in the page's (`in_page`) where the guard
/// lets it, and tells its answer as the command `token`'s.
pub fn eval(page: &Shared, script: &str, in_page_world: bool, token: u64) {
    let told = page.told.clone();
    let tell = move |result| told.tell(Heard::Result { token, result });
    let Some(channel) = page.agent.channel() else { return tell(Err(guard::UNCHECKED.into())) };
    let (generation, weak) = (page.generation.get(), weak_of(page));
    let (doc, unique) = {
        let contexts = page.agent.contexts.borrow();
        let unique = |world| contexts.of_main(world).map(|context| context.unique.clone());
        (contexts.doc(), [unique(Some(PLUGIN_WORLD)), unique(Some(WORLD)), unique(None)])
    };
    if !in_page_world {
        let [Some(world), _, _] = unique else { return tell(Err(crate::web::LOADING.into())) };
        return channel.ask(None, "Runtime.evaluate", evaluate(&expression(script), &world), move |answer| {
            let gone = weak.upgrade().is_none_or(|page| moved(&page, generation, doc));
            tell(if gone { Err(MOVED_ON.into()) } else { value_of(answer) });
        });
    }
    // The page's own world, where an agent's script runs: only on a page with no password, card or one-time code field
    // (`guard`), checked right before in Wings' world, and again by the script itself as it starts.
    let [_, Some(wings), Some(own)] = unique else { return tell(Err(crate::web::LOADING.into())) };
    let (script, started) = (in_page(script), Instant::now());
    let asked = channel.clone();
    channel.ask(None, "Runtime.evaluate", evaluate(&check(), &wings), move |found| {
        let Some(page) = weak.upgrade() else { return };
        let found = value_of(found);
        if found.as_ref().is_ok_and(|found| found["secret"] == true) {
            page.agent.seen.note(doc, false);
        }
        let allowed = if moved(&page, generation, doc) { Err(MOVED_ON) } else { guard::may_run(&found, started.elapsed(), page.agent.seen.secret(doc)) };
        if let Err(why) = allowed {
            return tell(Err(why.into()));
        }
        let weak = weak_of(&page);
        asked.ask(None, "Runtime.evaluate", evaluate(&script, &own), move |answer| {
            let gone = weak.upgrade().is_none_or(|page| moved(&page, generation, doc));
            tell(if gone { Err(MOVED_ON.into()) } else { value_of(answer) });
        });
    });
}

/// A picture of what shows of the page, as a PNG saved in its plugin's web folder, with its secret fields painted over
/// where they were just before and just after it was taken, and the frames the check can't look into too once a frame
/// in the page said it holds one.
pub fn screenshot(page: &Shared, token: u64) {
    let told = page.told.clone();
    let Some(channel) = page.agent.channel() else { return told.tell(Heard::Result { token, result: Err(guard::UNCHECKED.into()) }) };
    let (generation, doc) = (page.generation.get(), page.agent.contexts.borrow().doc());
    let Some(wings) = page.agent.contexts.borrow().of_main(Some(WORLD)).map(|context| context.unique.clone()) else {
        return told.tell(Heard::Result { token, result: Err(crate::web::LOADING.into()) });
    };
    let (weak, shots, asked) = (weak_of(page), page.web_dir.join("screenshots"), channel.clone());
    channel.ask(None, "Runtime.evaluate", evaluate(&check(), &wings), move |before| {
        let Some(page) = weak.upgrade() else { return };
        let before = value_of(before);
        let width = before.as_ref().ok().and_then(|found| found["width"].as_f64()).unwrap_or(0.0);
        let before = match guard::rects(&before, page.agent.seen.foreign(doc)) {
            Ok(before) => before,
            Err(why) => return told.tell(Heard::Result { token, result: Err(why.into()) }),
        };
        let (weak, again) = (weak_of(&page), asked.clone());
        asked.ask(None, "Page.captureScreenshot", json!({ "format": "png" }), move |shot| {
            let Some(page) = weak.upgrade() else { return };
            // As a script's answer: none of a page that came while it was taken, or one signed in with imports.
            let picture = if moved(&page, generation, doc) {
                Err(MOVED_ON.to_owned())
            } else if crate::web::holds_imported(&page.web_dir, &page.committed.borrow()) {
                Err(crate::web::IMPORTED.to_owned())
            } else {
                shot.and_then(|shot| {
                    let data = shot["data"].as_str().ok_or("Chromium made no picture")?;
                    base64::engine::general_purpose::STANDARD.decode(data).map_err(|error| error.to_string())
                })
            };
            let weak = weak_of(&page);
            again.ask(None, "Runtime.evaluate", evaluate(&check(), &wings), move |after| {
                let Some(page) = weak.upgrade() else { return };
                let rects = guard::rects(&value_of(after), page.agent.seen.foreign(doc)).map(|after| [before, after].concat()).map_err(str::to_owned);
                // Painted, scaled and written off the main thread: a big picture took the window's frames with it.
                std::thread::spawn(move || {
                    let result = rects
                        .and_then(|rects| picture.and_then(|png| guard::masked(&png, &rects, width, 1.0)))
                        .and_then(|png| crate::web::snapshot::save(&shots, &png));
                    told.tell(Heard::Result { token, result });
                });
            });
        });
    });
}

/// The main frame's recent console messages and requests, as WebKit answers `logs`.
pub fn logs(page: &Shared) -> Result<Value, String> {
    if page.agent.channel().is_none() {
        return Err(guard::UNCHECKED.into());
    }
    let logs = page.agent.logs.borrow();
    let of = |kind: &str| logs.iter().filter(|line| line["kind"] == kind).cloned().collect::<Vec<_>>();
    Ok(json!({ "console": of("console"), "network": of("network") }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn context(id: i64, unique: &str, frame: &str, default: bool, name: &str) -> Value {
        json!({ "id": id, "uniqueId": unique, "name": name, "origin": "http://127.0.0.1:8765", "auxData": { "frameId": frame, "isDefault": default } })
    }

    /// A document's own world and Wings' worlds are found by the main frame's id, each of the document there now: those of
    /// a frame in the page, or of the document before, are never used.
    #[test]
    fn the_main_frames_contexts_are_those_of_its_document_now() {
        let mut contexts = Contexts::default();
        // The first document's contexts, reported before the main frame was known.
        contexts.created(None, &context(1, "u1", "MAIN", true, ""));
        contexts.main_frame_is("MAIN".into());
        contexts.created(None, &context(2, "u2", "MAIN", false, WORLD));
        contexts.created(None, &context(3, "u3", "SUB", true, ""));
        contexts.created(None, &context(4, "u4", "SUB", false, PLUGIN_WORLD));
        assert_eq!(contexts.of_main(None).map(|context| context.unique.as_str()), Some("u1"));
        assert_eq!(contexts.of_main(Some(WORLD)).map(|context| context.unique.as_str()), Some("u2"));
        assert_eq!(contexts.of_main(Some(PLUGIN_WORLD)), None, "only a frame in the page has it");
        let first = contexts.doc();
        // The next document: its own world comes first, and Wings' world of the last one isn't its.
        contexts.created(None, &context(5, "u5", "MAIN", true, ""));
        assert_eq!(contexts.doc(), first + 1);
        assert_eq!(contexts.of_main(None).map(|context| context.unique.as_str()), Some("u5"));
        assert_eq!(contexts.of_main(Some(WORLD)), None, "Wings' world of the document before");
        // An extension's content scripts, in a world named as Wings' is.
        let mut extension = context(7, "x7", "MAIN", false, WORLD);
        extension["origin"] = json!("chrome-extension://nngceckbapebfimnlniiiahkandclblb");
        contexts.created(None, &extension);
        assert_eq!(contexts.of_main(Some(WORLD)), None, "an extension's world is never Wings'");
        contexts.created(None, &context(6, "u6", "MAIN", false, WORLD));
        assert_eq!(contexts.of_main(Some(WORLD)).map(|context| context.unique.as_str()), Some("u6"));
        // A frame of another site has the same frame id in a session of its own: never the main frame.
        contexts.created(Some("F1"), &context(1, "f1", "MAIN", true, ""));
        assert!(contexts.in_frame(Some("F1"), contexts.get(Some("F1"), 1)));
        assert!(!contexts.in_frame(None, contexts.get(None, 6)));
        assert!(contexts.in_frame(None, contexts.get(None, 3)), "a frame of the page's own process");
        assert!(contexts.in_frame(None, None), "a context it doesn't know");
        contexts.destroyed(None, 6);
        assert_eq!(contexts.of_main(Some(WORLD)), None);
        contexts.cleared(Some("F1"));
        assert_eq!(contexts.get(Some("F1"), 1), None);
    }

    /// The guard's binding is only in Wings' world, the watch runs there in every frame, frames of other sites are
    /// attached to and wait until they are readied the same way, and all of it is set before the page loads anything.
    #[test]
    fn a_page_and_each_frame_of_another_site_are_readied_with_the_watch_in_wings_world() {
        let steps = setup();
        assert_eq!(steps[0].0, "Page.getFrameTree", "the main frame is known before its contexts come");
        let guard_binding = steps.iter().find(|(method, params)| *method == "Runtime.addBinding" && params["name"] == GUARD_BINDING).unwrap();
        assert_eq!(guard_binding.1["executionContextName"], WORLD, "the page's world can't reach it");
        let watch_step = steps.iter().find(|(_, params)| params["worldName"] == WORLD).unwrap();
        assert_eq!((watch_step.0, watch_step.1["source"].as_str()), ("Page.addScriptToEvaluateOnNewDocument", Some(watch().as_str())));
        assert!(steps.iter().any(|(_, params)| params["worldName"] == PLUGIN_WORLD));
        let attach = steps.iter().find(|(method, _)| *method == "Target.setAutoAttach").unwrap();
        assert_eq!((attach.1["waitForDebuggerOnStart"].as_bool(), attach.1["flatten"].as_bool()), (Some(true), Some(true)));
        let frame: Vec<&str> = frame_setup().iter().map(|(method, _)| *method).collect();
        assert_eq!(frame, ["Page.enable", "Runtime.enable", "Runtime.addBinding", "Page.addScriptToEvaluateOnNewDocument", "Target.setAutoAttach"]);
        assert!(steps.iter().any(|(method, _)| *method == "Page.enable"), "a new document's scripts run in its own world only then");
        assert!(frame_setup().iter().any(|(_, params)| params["source"].as_str() == Some(watch().as_str()) && params["worldName"] == WORLD));
        // The log script names only the page's own binding, in the main frame.
        assert!(log_script().contains("window.top !== window") && log_script().contains(LOG_BINDING) && !log_script().contains(GUARD_BINDING));
    }

    /// An agent's script in the page's own world has the check in front of the function it runs in: a script that closes
    /// that function early can't get ahead of it.
    #[test]
    fn the_check_runs_before_an_agents_script_whatever_the_script_holds() {
        let breaks_out = "}); window.evil = 1; (async function () {";
        let script = in_page(breaks_out);
        let check_at = script.find("throw new Error(").unwrap();
        assert!(check_at < script.find(breaks_out).unwrap(), "{script}");
        assert!(script.starts_with("if ((() => {") && script.ends_with(&format!("(async function () {{\n{breaks_out}\n}})()")));
        assert!(check().contains("wingsScan(document)") && check().contains("innerWidth"));
    }

    #[test]
    fn a_scripts_answer_is_its_json_value_and_anything_else_says_why() {
        let answer = |result: Value| value_of(Ok(json!({ "result": result })));
        assert_eq!(answer(json!({ "type": "string", "value": "Docs" })), Ok(json!("Docs")));
        assert_eq!(answer(json!({ "type": "object", "value": { "a": [1] } })), Ok(json!({ "a": [1] })));
        assert_eq!(answer(json!({ "type": "object", "subtype": "null", "value": null })), Ok(Value::Null));
        assert_eq!(answer(json!({ "type": "undefined" })), Ok(Value::Null));
        for odd in [json!({ "type": "number", "unserializableValue": "NaN" }), json!({ "type": "object", "subtype": "node", "value": {} }), json!({ "type": "function" })] {
            assert_eq!(answer(odd.clone()), Err(NOT_JSON.into()), "{odd}");
        }
        let threw = |exception: Value| value_of(Ok(json!({ "result": {}, "exceptionDetails": { "text": "Uncaught", "exception": exception } })));
        assert_eq!(threw(json!({ "type": "object", "description": format!("Error: {}\n    at <anonymous>:3:9", guard::SECRET_FIELDS) })), Err(guard::SECRET_FIELDS.into()));
        assert_eq!(threw(json!({ "type": "object", "description": "TypeError: x is not a function\n    at f" })), Err("TypeError: x is not a function".into()));
        assert_eq!(threw(json!({ "type": "string", "value": "plain" })), Err("plain".into()));
        assert_eq!(value_of(Err("Cannot find context with specified id".into())), Err("Cannot find context with specified id".into()));
    }

    /// The log script's posts become WebKit's lines, field by field, and nothing else is kept.
    #[test]
    fn the_logs_are_kept_in_webkits_shape() {
        assert_eq!(log_line(r#"{"kind":"console","level":"error","text":"boom","extra":1}"#), Some(json!({ "kind": "console", "level": "error", "text": "boom" })));
        assert_eq!(
            log_line(r#"{"kind":"network","method":"GET","url":"https://a.com/x","status":500}"#),
            Some(json!({ "kind": "network", "method": "GET", "url": "https://a.com/x", "status": 500, "error": null }))
        );
        assert_eq!(log_line(r#"{"kind":"cookie","text":"x"}"#), None);
        assert_eq!(log_line("not json"), None);
        assert_eq!(host_of("http://127.0.0.1:8765"), "127.0.0.1");
        assert_eq!(host_of("null"), "");
    }

    /// Runs `script` after a made-up document of one field of `kind`, in JavaScriptCore (`osascript`), and answers what the
    /// binding heard.
    fn watched(kind: &str, then: &str) -> Value {
        let fake = format!(
            r#"
            const field = {{ tagName: "INPUT", type: "{kind}", nodeType: 1, children: [], getAttribute: () => null, getClientRects: () => [] }};
            const document = {{ nodeType: 9, children: [field], querySelectorAll(sel) {{ return sel === "*" ? [field] : [field]; }} }};
            const told = [], observed = [];
            globalThis.{GUARD_BINDING} = (m) => told.push(m);
            class MutationObserver {{ constructor(heard) {{ observed.push(heard); }} observe() {{}} disconnect() {{}} }}
            const addEventListener = () => {{}}, removeEventListener = () => {{}};
            "#
        );
        let script = format!("{fake}\n{}\n{then}\nJSON.stringify(told)", watch());
        let mut run = std::process::Command::new("osascript")
            .args(["-l", "JavaScript", "-"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        std::io::Write::write_all(&mut run.stdin.take().unwrap(), script.as_bytes()).unwrap();
        let out = run.wait_with_output().unwrap();
        serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
    }

    /// Chromium's watch tells its binding, as WebKit's tells its handler: once for a page that holds a password field, and
    /// once a field turns into one.
    #[test]
    fn chromiums_watch_tells_its_binding_of_a_secret_field() {
        assert_eq!(watched("password", ""), json!(["1"]));
        assert_eq!(watched("text", ""), json!([]));
        assert_eq!(watched("text", "field.type = 'password'; observed[0]([{ type: 'attributes', target: field, addedNodes: [] }]);"), json!(["1"]));
    }
}
