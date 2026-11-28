//! What a plugin's scripts, pictures and logs do on a Chromium page, over the page's own DevTools channel (`devtools`),
//! by WebKit's rules (`web::guard`, `mac.rs`): the plugin's helper runs in a world of its own (`PLUGIN_WORLD`), an agent's
//! script in the page's own world only where the guard lets it, a picture has the secret fields painted over, and the
//! logs are the main frame's console and requests, never a site's signed in with imported cookies.
//!
//! Real input (`input`) and the page's requests (`network`, kept by `net` from the moment the page lives) are read and
//! acted on under the same rules as a script: not on a site signed in with imported cookies, and no input or response body
//! on a page that shows a secret field.
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
    time::{Duration, Instant},
};

use base64::Engine;
use serde_json::{Value, json};

use super::{
    devtools::{Channel, Incoming},
    input, net,
    page::{Shared, weak_of},
};
use crate::{
    page::tree::{WebFile, WebInput, WebNetwork, WebRecord},
    web::{
        diff, guard::{self, FUNCTIONS, WATCH_FUNCTION, WORLD},
        mac::{LOGS_KEPT, MOVED_ON},
        policy::{self, Files},
        record::Recording,
        views::Heard,
    },
};

/// The plugin's own world, where its helper runs: it sees the page's document and none of its scripts.
pub const PLUGIN_WORLD: &str = "wings-plugin";
/// The bindings Wings' world and the page's tell.
const GUARD_BINDING: &str = "wingsGuard";

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
    /// The main frame's recent console lines and uncaught exceptions (`net::console_line`), the newest last.
    logs: RefCell<VecDeque<Value>>,
    /// Its recent requests, which a thread of their own fills (`net::feed`).
    net: net::Log,
    /// The page's recording, running or ended and not yet asked about (`record`).
    recording: RefCell<Option<Recording>>,
    /// Its frames waiting for a check of the page that began after them (`Held`).
    held: RefCell<Held>,
}

/// A recording's frames, held until a check of the page that began after each found no password or card field: only then
/// is a frame written, so a page that showed one is never in the video. Frames were written until a check ran, and none ran
/// in a page the recording went on to, which recorded a sign-in page's card field.
#[derive(Default)]
struct Held {
    /// When each came, its time in the video, and its picture, the oldest first.
    frames: VecDeque<(Instant, f64, Vec<u8>)>,
    checking: bool,
    last_check: Option<Instant>,
}

impl Held {
    /// Holds a frame that came `at`, the oldest going past `HELD_MOST`.
    fn hold(&mut self, at: Instant, at_ms: f64, jpeg: Vec<u8>) {
        self.frames.push_back((at, at_ms, jpeg));
        while self.frames.len() > HELD_MOST {
            self.frames.pop_front();
        }
    }

    /// The frames that came before a check that began `began` and found no secret field, to write; later ones wait.
    fn cleared_by(&mut self, began: Instant) -> Vec<(f64, Vec<u8>)> {
        let mut cleared = Vec::new();
        while let Some((_, at_ms, jpeg)) = self.frames.pop_front_if(|(came, _, _)| *came <= began) {
            cleared.push((at_ms, jpeg));
        }
        cleared
    }

    /// Drops the frames that came before a check that began `began` and couldn't clear them (the page moved).
    fn drop_before(&mut self, began: Instant) {
        self.frames.retain(|(came, _, _)| *came > began);
    }
}

/// How often a recording's page is checked for a password or card field, at most.
const HELD_CHECK_EVERY: Duration = Duration::from_millis(200);
/// The most frames held for a check: older ones go, unwritten.
const HELD_MOST: usize = 120;

impl Agent {
    /// The page closed: a recording of it ends, and its file is kept.
    pub fn page_closed(&self) {
        if let Some(recording) = self.recording.take() {
            recording.end("the page closed");
        }
    }

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

/// What readies a page's session, in order, before it loads anything: the main frame's id first, so the contexts that
/// come next are known by it. Chromium runs a new document's scripts in the page's own world only for a session that
/// enabled `Page` (measured in 156: the log script never ran without it).
pub fn setup() -> Vec<(&'static str, Value)> {
    vec![
        ("Page.getFrameTree", json!({})),
        ("Page.enable", json!({})),
        ("Runtime.enable", json!({})),
        // On for as long as the page lives, with the bodies it keeps for `Network.getResponseBody` bounded.
        ("Network.enable", json!({ "maxTotalBufferSize": net::BUFFER_TOTAL, "maxResourceBufferSize": net::BUFFER_ONE })),
        ("Runtime.addBinding", json!({ "name": GUARD_BINDING, "executionContextName": WORLD })),
        ("Page.addScriptToEvaluateOnNewDocument", json!({ "source": watch(), "worldName": WORLD, "runImmediately": true })),
        ("Page.addScriptToEvaluateOnNewDocument", json!({ "source": format!("//# sourceURL={PLUGIN_WORLD}"), "worldName": PLUGIN_WORLD, "runImmediately": true })),
        ("Target.setAutoAttach", auto_attach()),
    ]
}

/// A window a page opens (`window.open`, a link to a new one) waits for a debugger before it runs, which nothing attached
/// to it lets go: the opener's `window.open` never returned and the window never loaded until this did (measured in
/// Chromium 158, where Wings' sessions auto-attach to workers and frames with `waitForDebuggerOnStart`). Its own channel
/// sends it once the watch is in, so its first page is watched too.
pub fn popup_waits() -> (&'static str, Value) {
    ("Runtime.runIfWaitingForDebugger", json!({}))
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

/// Opens `page`'s channel on `browser` and readies it (`setup`); `done` hears whether all of it took.
pub fn open(page: &Shared, browser: &cef::Browser, done: impl FnOnce(Result<(), String>) + 'static) {
    let Some(channel) = Channel::open(browser, weak_of(page)) else { return done(Err("Chromium's DevTools didn't open".into())) };
    let channel = Rc::new(channel);
    page.agent.channel.replace(Some(channel.clone()));
    let mut steps = setup();
    if page.is_popup() {
        steps.push(popup_waits());
    }
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
            "Runtime.consoleAPICalled" => console(page, session, &params, net::console_line),
            "Runtime.exceptionThrown" => console(page, session, &params, net::exception_line),
            "Page.screencastFrame" => screencast_frame(page, session, &params),
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

/// A binding was called: the watch in a frame saw a secret field.
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
            // A recording can't paint a field over: it ends where one shows.
            if let Some(recording) = page.agent.recording.borrow().as_ref() {
                recording.end(RECORDING_SECRET);
            }
        }
        _ => {}
    }
}

/// A console call or an uncaught exception of the page's main frame, in its own world, kept as `line` makes it. Nothing of a
/// frame of another site, a world of Wings or an extension, which have no say here; what is read goes through `gate`, which
/// refuses a site signed in with imported cookies, and a new document starts the lines again (`clear_logs`).
fn console(page: &Shared, session: Option<&str>, params: &Value, line: fn(&Value) -> Value) {
    if session.is_some() {
        return;
    }
    let id = params["executionContextId"].as_i64().or_else(|| params["exceptionDetails"]["executionContextId"].as_i64());
    if let Some(id) = id {
        let contexts = page.agent.contexts.borrow();
        if !contexts.get(None, id).is_some_and(|context| context.default && contexts.is_main(None, &context.frame)) {
            return;
        }
    }
    // Every value the page's requests showed to be a credential is out of its console too.
    let line = page.agent.net.lock().unwrap_or_else(std::sync::PoisonError::into_inner).console(line(params));
    let mut logs = page.agent.logs.borrow_mut();
    logs.push_back(line);
    while logs.len() > LOGS_KEPT * 2 {
        logs.pop_front();
    }
}

/// Hands a `Network` event's bytes to the thread that reads them (`net::feed`), off the main thread.
pub fn feed_net(page: &Shared, message: &[u8]) {
    net::feed(&page.agent.net, message);
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
    let shots = page.web_dir.join("screenshots");
    shoot(page, move |shot| told.tell(Heard::Result { token, result: shot.and_then(|(png, _)| crate::web::snapshot::save(&shots, &png)) }));
}

/// What `shoot` hears: the picture, PNG as Chromium made it with the secret fields painted over, and how wide the page's
/// viewport is in CSS pixels, or why there is none.
type Shot = Result<(Vec<u8>, f64), String>;

/// Takes the picture `screenshot` and `screenshot_diff` use. `then` hears it on a thread of its own, painted off the main
/// thread: a big picture took the window's frames with it; an error it hears on the main thread.
fn shoot(page: &Shared, then: impl FnOnce(Shot) + Send + 'static) {
    let Some(channel) = page.agent.channel() else { return then(Err(guard::UNCHECKED.into())) };
    let (generation, doc) = (page.generation.get(), page.agent.contexts.borrow().doc());
    let Some(wings) = page.agent.contexts.borrow().of_main(Some(WORLD)).map(|context| context.unique.clone()) else {
        return then(Err(crate::web::LOADING.into()));
    };
    let (weak, asked) = (weak_of(page), channel.clone());
    channel.ask(None, "Runtime.evaluate", evaluate(&check(), &wings), move |before| {
        let Some(page) = weak.upgrade() else { return };
        let before = value_of(before);
        let width = before.as_ref().ok().and_then(|found| found["width"].as_f64()).unwrap_or(0.0);
        let before = match guard::rects(&before, page.agent.seen.foreign(doc)) {
            Ok(before) => before,
            Err(why) => return then(Err(why.into())),
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
                std::thread::spawn(move || {
                    let shot = rects.and_then(|rects| picture.and_then(|png| guard::masked(&png, &rects, width, 1.0))).map(|png| (png, width));
                    then(shot);
                });
            });
        });
    });
}

/// The error a file command answers when it can't have the page's own plugin world.
const NO_WORLD: &str = "The page's helper isn't ready: wait for the page to load";

/// What `upload` runs in the plugin's world to find the input: the element the helper's `ref` names, or the one `selector`
/// matches, a `<label>`'s control, which has to be a file input, one that takes `count` files.
pub fn upload_target(reference: Option<&str>, selector: Option<&str>, count: usize) -> String {
    let find = match (reference, selector) {
        (Some(reference), _) => format!(
            "const helper = globalThis.__wingsAgent;\nif (!helper || typeof helper.element !== 'function') throw new Error(\"The page's helper isn't loaded: take a snapshot first\");\nlet el = helper.element({});",
            serde_json::to_string(reference).expect("a string is JSON")
        ),
        (None, selector) => {
            let selector = serde_json::to_string(selector.unwrap_or_default()).expect("a string is JSON");
            format!(
                "const found = document.querySelectorAll({selector});\nif (found.length === 0) throw new Error('No element matches ' + {selector});\nif (found.length > 1) throw new Error(found.length + ' elements match ' + {selector} + ': use one that matches exactly one');\nlet el = found[0];"
            )
        }
    };
    format!(
        "(() => {{\n{find}\nif (el && el.tagName === 'LABEL' && el.control) el = el.control;\nif (!(el instanceof HTMLInputElement) || el.type !== 'file') throw new Error('That element is not a file input: ' + (el && el.tagName ? el.tagName.toLowerCase() : String(el)));\nif ({count} > 1 && !el.multiple) throw new Error('That file input takes one file, and {count} were given');\nreturn el;\n}})()"
    )
}

/// Puts files in a file input of the page: the paths checked, inline files written to the plugin's own folder, the input
/// found in the plugin's world, and Chromium's `DOM.setFileInputFiles` sets them (it fires `input` and `change` itself).
pub fn upload(page: &Shared, reference: Option<&str>, selector: Option<&str>, files: &[WebFile], token: u64) {
    let told = page.told.clone();
    let tell = move |result| told.tell(Heard::Result { token, result });
    let Some(channel) = page.agent.channel() else { return tell(Err(guard::UNCHECKED.into())) };
    // Written on this thread: at most 10 MB of inline files.
    let access = Files::of(&page.web_dir);
    let paths = files.iter().filter_map(|file| if let WebFile::Path(path) = file { Some(path.clone()) } else { None }).collect::<Vec<_>>();
    let inline = files.iter().filter_map(|file| if let WebFile::Inline { name, data } = file { Some((name.clone(), data.clone())) } else { None }).collect::<Vec<_>>();
    let found = access.uploads(&paths).and_then(|mut found| access.inline(&inline).map(|more| found.extend(more)).map(|()| found));
    let found: Vec<String> = match found {
        Ok(found) => found.iter().map(|path| path.to_string_lossy().into_owned()).collect(),
        Err(why) => return tell(Err(why)),
    };
    let (generation, weak) = (page.generation.get(), weak_of(page));
    let (doc, world) = {
        let contexts = page.agent.contexts.borrow();
        (contexts.doc(), contexts.of_main(Some(PLUGIN_WORLD)).map(|context| context.unique.clone()))
    };
    let Some(world) = world else { return tell(Err(NO_WORLD.into())) };
    let script = upload_target(reference, selector, found.len());
    let params = json!({ "expression": script, "uniqueContextId": world, "awaitPromise": false, "returnByValue": false });
    let asked = channel.clone();
    channel.ask(None, "Runtime.evaluate", params, move |answer| {
        let Some(page) = weak.upgrade() else { return };
        if moved(&page, generation, doc) {
            return tell(Err(MOVED_ON.into()));
        }
        let object = match answer {
            Ok(answer) => match answer.get("exceptionDetails") {
                Some(details) => Err(exception_text(details)),
                None => answer["result"]["objectId"].as_str().map(str::to_owned).ok_or_else(|| "The input wasn't found".to_owned()),
            },
            Err(why) => Err(why),
        };
        let object = match object {
            Ok(object) => object,
            Err(why) => return tell(Err(why)),
        };
        let (count, release, weak) = (found.len(), asked.clone(), weak_of(&page));
        let object_id = object.clone();
        asked.ask(None, "DOM.setFileInputFiles", json!({ "files": found, "objectId": object }), move |set| {
            release.ask(None, "Runtime.releaseObject", json!({ "objectId": object_id }), |_| {});
            let gone = weak.upgrade().is_none_or(|page| moved(&page, generation, doc));
            tell(if gone { Err(MOVED_ON.into()) } else { set.map(|_| json!({ "files": count })) });
        });
    });
}

/// Compares `png`, a picture of the page `css_wide` CSS pixels wide, cut to `clip` (CSS pixels) if it has one and fitted to
/// the size a saved picture is (`snapshot::LONGEST`), with the baseline `baseline_png`.
pub fn compare_pictures(baseline_png: &[u8], png: &[u8], css_wide: f64, clip: Option<[f64; 4]>, threshold: f64) -> Result<(diff::Diff, image::RgbaImage), String> {
    let baseline = image::load_from_memory(baseline_png).map_err(|error| format!("The baseline isn't a picture: {error}"))?.into_rgba8();
    let mut current = image::load_from_memory(png).map_err(|error| error.to_string())?.into_rgba8();
    if let Some([x, y, width, height]) = clip {
        let scale = f64::from(current.width()) / css_wide.max(1.0);
        let (left, top) = ((x * scale).round() as u32, (y * scale).round() as u32);
        if left >= current.width() || top >= current.height() {
            return Err("The clip is outside the page".into());
        }
        let (wide, high) = (((width * scale).round() as u32).min(current.width() - left).max(1), ((height * scale).round() as u32).min(current.height() - top).max(1));
        current = image::imageops::crop_imm(&current, left, top, wide, high).to_image();
    }
    let longest = crate::web::snapshot::LONGEST;
    if current.width().max(current.height()) > longest {
        current = image::DynamicImage::ImageRgba8(current).resize(longest, longest, image::imageops::FilterType::Triangle).into_rgba8();
    }
    diff::compare(&baseline, &current, threshold).map(|found| (found, current))
}

fn png_bytes(image: &image::RgbaImage) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    image.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).map_err(|error| error.to_string())?;
    Ok(bytes)
}

/// A fresh picture of the page against the baseline's: the answer of the `screenshotDiff` command.
pub fn screenshot_diff(page: &Shared, baseline: &str, threshold: f64, clip: Option<[f64; 4]>, current: Option<&str>, diff_to: Option<&str>, token: u64) {
    let told = page.told.clone();
    let access = Files::of(&page.web_dir);
    // Every path is checked before the picture is taken.
    let paths = (|| {
        let baseline = access.input(baseline, policy::SCREENSHOTS)?;
        let current = current.map(|path| access.output(path, policy::SCREENSHOTS)).transpose()?;
        let diff_to = diff_to.map(|path| access.output(path, policy::SCREENSHOTS)).transpose()?;
        Ok::<_, String>((baseline, current, diff_to))
    })();
    let (baseline, current, diff_to) = match paths {
        Ok(paths) => paths,
        Err(why) => return told.tell(Heard::Result { token, result: Err(why) }),
    };
    let (web, shots) = (page.web_dir.clone(), page.web_dir.join(policy::SCREENSHOTS));
    shoot(page, {
        let told = told.clone();
        move |shot| {
            let result = shot.and_then(|(png, wide)| {
                let access = Files::of(&web);
                let baseline_png = access.read(&baseline.to_string_lossy(), policy::SCREENSHOTS, policy::UPLOAD_MAX_BYTES)?;
                let (found, now) = compare_pictures(&baseline_png, &png, wide, clip, threshold)?;
                let saved_current = match &current {
                    Some(path) => {
                        access.write(path, &png_bytes(&now)?)?;
                        Some(path.clone())
                    }
                    None => None,
                };
                let saved_diff = if found.changed == 0 {
                    None
                } else if let Some(path) = &diff_to {
                    access.write(path, &png_bytes(&found.image)?)?;
                    Some(path.clone())
                } else {
                    let saved = crate::web::snapshot::save(&shots, &png_bytes(&found.image)?)?;
                    saved["path"].as_str().map(std::path::PathBuf::from)
                };
                Ok(json!({
                    "matches": found.changed == 0,
                    "changedPixels": found.changed,
                    "totalPixels": found.total,
                    "changedRatio": found.changed as f64 / found.total.max(1) as f64,
                    "width": found.image.width(),
                    "height": found.image.height(),
                    "diff": saved_diff,
                    "current": saved_current,
                }))
            });
            told.tell(Heard::Result { token, result });
        }
    });
}

const RECORDING_SECRET: &str = "the page showed a password or card field";
const NO_RECORDING: &str = "No recording is running";

fn stop_screencast(page: &Shared) {
    if let Some(channel) = page.agent.channel() {
        channel.ask(None, "Page.stopScreencast", json!({}), |_| {});
    }
}

/// Starts, asks about, stops or cancels the page's recording (`WebRecord`).
pub fn record(page: &Shared, record: &WebRecord, token: u64) {
    let told = page.told.clone();
    let tell = move |result| told.tell(Heard::Result { token, result });
    match record {
        WebRecord::Status => tell(Ok(page.agent.recording.borrow().as_ref().map_or_else(|| json!({ "recording": false, "durationMs": 0, "frames": 0 }), Recording::status))),
        WebRecord::Stop => match page.agent.recording.take() {
            Some(recording) => {
                // What no check has cleared yet, the last moments, isn't written.
                page.agent.held.take();
                stop_screencast(page);
                recording.stop(Box::new(tell));
            }
            None => tell(Err(NO_RECORDING.into())),
        },
        WebRecord::Cancel => match page.agent.recording.take() {
            Some(recording) => {
                page.agent.held.take();
                stop_screencast(page);
                recording.cancel();
                tell(Ok(json!({ "cancelled": true })));
            }
            None => tell(Err(NO_RECORDING.into())),
        },
        WebRecord::Start { path, fps, max_ms } => {
            if page.agent.recording.borrow().as_ref().is_some_and(|recording| !recording.ended()) {
                return tell(Err("The page is being recorded already: stop or cancel that first".into()));
            }
            let access = Files::of(&page.web_dir);
            let path = match access.output(path, policy::RECORDINGS).and_then(|path| access.prepare(&path).map(|_| path)) {
                Ok(path) => path,
                Err(why) => return tell(Err(why)),
            };
            let Some(channel) = page.agent.channel() else { return tell(Err(guard::UNCHECKED.into())) };
            let (generation, doc) = (page.generation.get(), page.agent.contexts.borrow().doc());
            let Some(wings) = page.agent.contexts.borrow().of_main(Some(WORLD)).map(|context| context.unique.clone()) else { return tell(Err(crate::web::LOADING.into())) };
            let (weak, asked, started) = (weak_of(page), channel.clone(), Instant::now());
            let (fps, max_ms) = (*fps, *max_ms);
            // A page that shows a password or card field isn't recorded, as no script runs on it.
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
                if page.agent.recording.borrow().as_ref().is_some_and(|recording| !recording.ended()) {
                    return tell(Err("The page is being recorded already: stop or cancel that first".into()));
                }
                page.agent.held.take();
                page.agent.recording.replace(Some(Recording::start(page.web_dir.clone(), path.clone(), fps, max_ms)));
                let (weak, again) = (weak_of(&page), asked.clone());
                // At the page's size in CSS pixels, as the browse MCP records an .mp4: frames at the screen's own pixels
                // made a 3012x1858 video of a window's page.
                asked.ask(None, "Page.getLayoutMetrics", json!({}), move |metrics| {
                    let viewport = metrics.ok().map(|metrics| metrics["cssVisualViewport"].clone()).unwrap_or_default();
                    let mut cast = json!({ "format": "jpeg", "quality": 90, "everyNthFrame": 1 });
                    if let (Some(width), Some(height)) = (viewport["clientWidth"].as_f64(), viewport["clientHeight"].as_f64()) {
                        cast["maxWidth"] = json!(width.round() as u64);
                        cast["maxHeight"] = json!(height.round() as u64);
                    }
                    again.ask(None, "Page.startScreencast", cast, move |started| match started {
                        Ok(_) => tell(Ok(json!({ "recording": true, "path": path }))),
                        Err(why) => {
                            if let Some(page) = weak.upgrade() {
                                page.agent.recording.take();
                            }
                            tell(Err(why));
                        }
                    });
                });
            });
        }
    }
}

/// One frame of the page's screencast: acknowledged at once, or Chromium sends no more, and given to the recording unless
/// that ended, or the page moved where it mustn't be recorded.
fn screencast_frame(page: &Shared, session: Option<&str>, params: &Value) {
    if session.is_some() {
        return;
    }
    if let (Some(channel), Some(id)) = (page.agent.channel(), params["sessionId"].as_i64()) {
        channel.ask(None, "Page.screencastFrameAck", json!({ "sessionId": id }), |_| {});
    }
    let borrowed = page.agent.recording.borrow();
    let Some(recording) = borrowed.as_ref() else { return };
    if !recording.ended() {
        let doc = page.agent.contexts.borrow().doc();
        if crate::web::holds_imported(&page.web_dir, &page.committed.borrow()) {
            recording.end(RECORDING_IMPORTED);
        } else if page.agent.seen.secret(doc) {
            recording.end(RECORDING_SECRET);
        } else if let Some(jpeg) = params["data"].as_str().and_then(|data| base64::engine::general_purpose::STANDARD.decode(data).ok()) {
            let at_ms = params["metadata"]["timestamp"].as_f64().map_or_else(|| recording.now_ms(), |seconds| seconds * 1000.0);
            drop(borrowed);
            page.agent.held.borrow_mut().hold(Instant::now(), at_ms, jpeg);
            check_held(page);
            return;
        }
    }
    drop(borrowed);
    page.agent.held.take();
    stop_screencast(page);
}

/// Checks the page for a password or card field, at most every `HELD_CHECK_EVERY`, and writes the frames that came before
/// the check began once it finds none; one found ends the recording with none of them. A check that can't answer (the page
/// is loading) writes nothing, and the next frame asks again.
fn check_held(page: &Shared) {
    let now = Instant::now();
    {
        let mut held = page.agent.held.borrow_mut();
        if held.checking || held.last_check.is_some_and(|last| now.duration_since(last) < HELD_CHECK_EVERY) {
            return;
        }
        held.checking = true;
        held.last_check = Some(now);
    }
    let (generation, doc) = (page.generation.get(), page.agent.contexts.borrow().doc());
    let wings = page.agent.contexts.borrow().of_main(Some(WORLD)).map(|context| context.unique.clone());
    let (Some(channel), Some(wings)) = (page.agent.channel(), wings) else {
        page.agent.held.borrow_mut().checking = false;
        return;
    };
    let weak = weak_of(page);
    channel.ask(None, "Runtime.evaluate", evaluate(&check(), &wings), move |found| {
        let Some(page) = weak.upgrade() else { return };
        page.agent.held.borrow_mut().checking = false;
        let found = value_of(found);
        if found.as_ref().is_ok_and(|found| found["secret"] == true) {
            page.agent.seen.note(doc, false);
            page.agent.held.take();
            if let Some(recording) = page.agent.recording.borrow().as_ref() {
                recording.end(RECORDING_SECRET);
            }
            return stop_screencast(&page);
        }
        // The page moved while it was checked: what came before is of a page no check cleared.
        if moved(&page, generation, doc) {
            return page.agent.held.borrow_mut().drop_before(now);
        }
        if guard::may_run(&found, now.elapsed(), page.agent.seen.secret(doc)).is_err() {
            return;
        }
        let cleared = page.agent.held.borrow_mut().cleared_by(now);
        if let Some(recording) = page.agent.recording.borrow().as_ref().filter(|recording| !recording.ended()) {
            for (at_ms, jpeg) in cleared {
                recording.frame(jpeg, at_ms);
            }
        }
    });
}

const RECORDING_IMPORTED: &str = "the page is signed in with cookies you imported";

/// Whether `host` is of a site signed in with imported cookies, asked once for each host.
fn hider(page: &Shared) -> impl Fn(&str) -> bool + '_ {
    let known: RefCell<HashMap<String, bool>> = RefCell::default();
    move |host: &str| *known.borrow_mut().entry(host.to_owned()).or_insert_with(|| crate::web::holds_imported_host(&page.web_dir, host))
}

/// The main frame's recent console messages and requests, as WebKit answers `logs`.
pub fn logs(page: &Shared) -> Result<Value, String> {
    if page.agent.channel().is_none() {
        return Err(guard::UNCHECKED.into());
    }
    let net = page.agent.net.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    // Scrubbed again as it is read: a credential learned after a line was kept (a response body read since) is out too.
    let console: Vec<Value> = page.agent.logs.borrow().iter().map(|line| net.console(line.clone())).collect();
    let network = net.legacy(&hider(page), LOGS_KEPT);
    Ok(json!({ "console": console, "network": network }))
}

fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |since| since.as_millis() as f64)
}

/// The page's requests: a list, or with `detail` one request's headers and, with `body`, its response body, which a page
/// that shows a secret field never gives (checked as a script's run is) and only for a text type up to 1 MB.
pub fn network(page: &Shared, wanted: &WebNetwork, token: u64) {
    let told = page.told.clone();
    let tell = move |result| told.tell(Heard::Result { token, result });
    let Some(channel) = page.agent.channel() else { return tell(Err(guard::UNCHECKED.into())) };
    let hide = hider(page);
    let log = page.agent.net.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(id) = wanted.detail.as_deref() else { return tell(Ok(log.list(wanted, &hide, now_ms()))) };
    let Some((request, mut shown)) = log.detail(id, &hide) else { return tell(Err(format!("No request with the id {id}: network lists them."))) };
    drop(log);
    if !wanted.body {
        return tell(Ok(shown));
    }
    if let Some(why) = net::body_refusal(&request) {
        return tell(Err(why.into()));
    }
    let (generation, weak) = (page.generation.get(), weak_of(page));
    let (doc, wings) = {
        let contexts = page.agent.contexts.borrow();
        (contexts.doc(), contexts.of_main(Some(WORLD)).map(|context| context.unique.clone()))
    };
    let Some(wings) = wings else { return tell(Err(crate::web::LOADING.into())) };
    let (started, asked) = (Instant::now(), channel.clone());
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
        asked.ask(None, "Network.getResponseBody", json!({ "requestId": request.id }), move |answer| {
            let body = answer.map_err(net::body_lost).and_then(|answer| net::body_text(&answer).map_err(str::to_owned));
            let Some(page) = weak.upgrade() else { return };
            // Its credential fields are scrubbed, by the type the response said, before it leaves.
            tell(body.map(|body| {
                shown["body"] = json!(page.agent.net.lock().unwrap_or_else(std::sync::PoisonError::into_inner).protect_body(&body, &request.mime));
                shown
            }));
        });
    });
}

/// Sends `requests` one after another, each once the one before was answered, and `done` hears how it went.
/// `still` says before each whether the page may still take it: one that went to another document, or showed a secret
/// field, while the earlier ones were sent stops the rest.
fn in_order(channel: Rc<Channel>, mut requests: VecDeque<(&'static str, Value)>, still: Rc<dyn Fn() -> Result<(), &'static str>>, done: Box<dyn FnOnce(Result<(), String>)>) {
    let Some((method, params)) = requests.pop_front() else { return done(Ok(())) };
    if let Err(why) = still() {
        return done(Err(why.into()));
    }
    let next = channel.clone();
    channel.ask(None, method, params, move |answer| match answer {
        Ok(_) => in_order(next, requests, still, done),
        Err(why) => done(Err(why)),
    });
}

/// Whether the page that was checked as the document `doc` of `generation` may still take input: it is that document yet
/// and showed no secret field since.
fn still_clear(page: &Shared, generation: u64, doc: u64) -> Result<(), &'static str> {
    if moved(page, generation, doc) {
        return Err(MOVED_ON);
    }
    if page.agent.seen.secret(doc) {
        return Err(guard::SECRET_FIELDS);
    }
    Ok(())
}

/// Real mouse and keyboard input (`input`), once the page is checked for a password, card or one-time code field as a
/// script in the page's own world is: trusted events could fill and submit one.
pub fn input(page: &Shared, events: &[WebInput], token: u64) {
    let told = page.told.clone();
    let tell = move |result| told.tell(Heard::Result { token, result });
    let Some(channel) = page.agent.channel() else { return tell(Err(guard::UNCHECKED.into())) };
    let (generation, weak) = (page.generation.get(), weak_of(page));
    let (doc, wings) = {
        let contexts = page.agent.contexts.borrow();
        (contexts.doc(), contexts.of_main(Some(WORLD)).map(|context| context.unique.clone()))
    };
    let Some(wings) = wings else { return tell(Err(crate::web::LOADING.into())) };
    let (requests, count, started, asked) = (input::requests(events).into_iter().collect::<VecDeque<_>>(), events.len(), Instant::now(), channel.clone());
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
        let still = Rc::new(move || weak.upgrade().map_or(Err(MOVED_ON), |page| still_clear(&page, generation, doc)));
        in_order(asked, requests, still, Box::new(move |done| tell(done.map(|()| json!({ "events": count })))));
    });
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
        // The requests are kept from the start, with the bodies they keep bounded.
        let network = steps.iter().find(|(method, _)| *method == "Network.enable").unwrap();
        assert_eq!((network.1["maxTotalBufferSize"].as_u64(), network.1["maxResourceBufferSize"].as_u64()), (Some(net::BUFFER_TOTAL), Some(net::BUFFER_ONE)));
        assert!(!steps.iter().any(|(_, params)| params["name"] == "wingsLog"), "the console comes over the protocol, no script of the page's own world");
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

    #[test]
    fn the_upload_script_finds_one_file_input_by_ref_or_selector() {
        let by_ref = upload_target(Some("e12"), None, 1);
        assert!(by_ref.contains(r#"helper.element("e12")"#) && by_ref.contains("el.control") && by_ref.contains("el.type !== 'file'"), "{by_ref}");
        assert!(by_ref.contains("if (1 > 1 && !el.multiple)"));
        let by_selector = upload_target(None, Some("input[name=\"a\"]"), 3);
        assert!(by_selector.contains(&format!("querySelectorAll({})", serde_json::to_string("input[name=\"a\"]").unwrap())), "{by_selector}");
        assert!(by_selector.contains("found.length > 1") && by_selector.contains("found.length === 0"));
        assert!(by_selector.contains("if (3 > 1 && !el.multiple)"));
    }

    fn png(image: &image::RgbaImage) -> Vec<u8> {
        png_bytes(image).unwrap()
    }

    #[test]
    fn a_picture_is_compared_whole_or_cut_to_a_clip_in_css_pixels() {
        let flat = |width, height| image::RgbaImage::from_pixel(width, height, image::Rgba([10, 20, 30, 255]));
        let mut changed = flat(40, 20);
        changed.put_pixel(30, 5, image::Rgba([250, 20, 30, 255]));
        // A page 20 CSS pixels wide, in a picture twice as wide.
        let (found, _) = compare_pictures(&png(&flat(40, 20)), &png(&changed), 20.0, None, 0.1).unwrap();
        assert_eq!((found.changed, found.total), (1, 800));
        let (found, now) = compare_pictures(&png(&flat(20, 10)), &png(&changed), 20.0, Some([0.0, 0.0, 10.0, 5.0]), 0.1).unwrap();
        assert_eq!((found.changed, now.dimensions()), (0, (20, 10)), "the clip (10x5 CSS pixels) is 20x10 pixels and holds no change");
        let (found, _) = compare_pictures(&png(&flat(10, 10)), &png(&changed), 20.0, Some([11.0, 0.0, 5.0, 5.0]), 0.1).unwrap();
        assert_eq!(found.changed, 1, "a clip over the changed pixel");
        assert!(compare_pictures(&png(&flat(40, 21)), &png(&changed), 20.0, None, 0.1).err().unwrap().contains("40x21"));
        assert!(compare_pictures(&png(&flat(4, 4)), &png(&changed), 20.0, Some([100.0, 0.0, 5.0, 5.0]), 0.1).err().unwrap().contains("outside"));
        assert!(compare_pictures(b"not a picture", &png(&changed), 20.0, None, 0.1).err().unwrap().contains("baseline"));
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

    /// A recording's frame is written only once a check of the page that began after it cleared it: frames that came later
    /// wait for the next check, and a page that moved under a check drops what came before it.
    #[test]
    fn a_recordings_frames_wait_for_a_check_that_began_after_them() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut held = Held::default();
        held.hold(at(0), 0.0, vec![1]);
        held.hold(at(10), 10.0, vec![2]);
        held.hold(at(30), 30.0, vec![3]);
        assert_eq!(held.cleared_by(at(20)), [(0.0, vec![1]), (10.0, vec![2])]);
        assert_eq!(held.cleared_by(at(20)), [], "nothing twice");
        held.hold(at(40), 40.0, vec![4]);
        held.drop_before(at(35));
        assert_eq!(held.cleared_by(at(50)), [(40.0, vec![4])], "the frame of a page no check cleared is gone");
        for ms in 0..(HELD_MOST as u64 + 5) {
            held.hold(at(100 + ms), ms as f64, vec![]);
        }
        assert_eq!(held.frames.len(), HELD_MOST);
    }

}
