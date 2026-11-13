//! Frames written in Rust, for tests of the host. A test names what the frame does in a `Guest`, and `source` gives the
//! source to open it with: a file that holds a number instead of a module, which `wasm::serve` claims before it
//! compiles anything. From there a fake runs on the loop a module does, with the same queue, `Reply` and host
//! functions, so the host can't tell which one it is talking to. It has no fuel, as it has no WebAssembly to run out.

use super::{
    Command, Link, Store,
    wasm::{self, Frame, Host},
};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs,
    io::Read,
    sync::{Arc, Mutex},
    time::Instant,
};

/// What a fake frame does, as the module's exports are what a real one does. Each is one turn.
pub trait Guest: Send + 'static {
    fn start(&mut self, _cx: &mut Cx<'_>) {}
    /// What `PageRuntime::dispatch` sends: the handler and its payload, `Null` when it had none.
    fn event(&mut self, _cx: &mut Cx<'_>, _handler: &str, _payload: &Value) {}
    /// What the host tells the frame: `Message::name()` ("panes", "action", "tool", "resolve", "output" ...).
    fn deliver(&mut self, _cx: &mut Cx<'_>, _kind: &str, _id: u32, _text: &str) {}
    fn timer(&mut self, _cx: &mut Cx<'_>, _id: u32) {}
}

/// The imports of the module, for a guest to call.
pub struct Cx<'a> {
    host: &'a mut Host,
    next: &'a mut u32,
    failed: Option<wasmtime::Error>,
}

impl Cx<'_> {
    /// The `render` import. A tree the host refuses is a non-fatal error, and a main that renders stops.
    pub fn render(&mut self, tree: &Value) {
        if let Err(error) = self.host.render(&tree.to_string()) {
            self.failed = Some(error);
        }
    }

    /// The `call` import. The answer comes as `deliver("resolve" | "reject", id, ..)`, and a call's lines as "output".
    pub fn call(&mut self, method: &str, params: Value) -> u32 {
        let id = self.id();
        if let Err(error) = self.host.call(id, method, &params.to_string()) {
            self.failed = Some(error);
        }
        id
    }

    /// What storage, `broadcast`, `view_args` and `focused` are.
    pub fn link(&self) -> &Arc<dyn Link> {
        &self.host.link
    }

    /// `wings.storage.set` of `value`.
    pub fn save(&self, key: &str, value: &Value) {
        let set = Store::Set(key.into(), value.to_string());
        self.link().storage(set).unwrap();
    }

    /// The `timer` import: `Guest::timer` is called with the id in `ms`.
    pub fn later(&mut self, ms: u32) -> u32 {
        let id = self.id();
        self.host.timer(id, ms);
        id
    }

    /// Stops the frame as a trap does: the last event is a fatal error with `message`.
    pub fn stop(&mut self, message: &str) {
        self.failed = Some(wasmtime::Error::msg(message.to_owned()));
    }

    fn id(&mut self) -> u32 {
        *self.next += 1;
        *self.next
    }
}

type Make = Arc<dyn Fn() -> Box<dyn Guest> + Send + Sync>;

static GUESTS: Mutex<Vec<Make>> = Mutex::new(Vec::new());

/// What a fake's file starts with, then the number of its `make`.
const MAGIC: &str = "wings-fake-frame ";

/// The source of a frame that runs what `make` returns, a new one each time the frame starts. `name` is the file's, which
/// `PageOpener::name` and the logs show.
pub fn source(name: &str, make: impl Fn() -> Box<dyn Guest> + Send + Sync + 'static) -> String {
    let mut guests = GUESTS.lock().unwrap();
    let n = guests.len();
    guests.push(Arc::new(make));
    let dir = std::env::temp_dir()
        .join(format!("wings-fake-frames-{}", std::process::id()))
        .join(n.to_string());
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.wasm"));
    fs::write(&path, format!("{MAGIC}{n}")).unwrap();
    format!("{}{}", wasm::MARKER, path.display())
}

/// A page that renders `render(None)` when it starts and `render(Some((handler, payload)))` after each event.
pub fn page(
    name: &str,
    render: impl FnMut(Option<(&str, &Value)>) -> Value + Clone + Send + Sync + 'static,
) -> String {
    struct Drawn<F>(F);
    impl<F: FnMut(Option<(&str, &Value)>) -> Value + Send + 'static> Guest for Drawn<F> {
        fn start(&mut self, cx: &mut Cx<'_>) {
            cx.render(&(self.0)(None));
        }
        fn event(&mut self, cx: &mut Cx<'_>, handler: &str, payload: &Value) {
            cx.render(&(self.0)(Some((handler, payload))));
        }
    }
    source(name, move || Box::new(Drawn(render.clone())))
}

/// What a `script` hears: the turns of a `Guest`.
pub enum Hear<'a> {
    Start,
    Event(&'a str, &'a Value),
    Deliver(&'a str, u32, &'a str),
}

/// A frame that is one closure, which `make` returns anew each time the frame starts, and which hears each turn.
pub fn script<F>(name: &str, make: impl Fn() -> F + Send + Sync + 'static) -> String
where
    F: FnMut(&mut Cx<'_>, Hear<'_>) + Send + 'static,
{
    struct Script<F>(F);
    impl<F: FnMut(&mut Cx<'_>, Hear<'_>) + Send + 'static> Guest for Script<F> {
        fn start(&mut self, cx: &mut Cx<'_>) {
            (self.0)(cx, Hear::Start);
        }
        fn event(&mut self, cx: &mut Cx<'_>, handler: &str, payload: &Value) {
            (self.0)(cx, Hear::Event(handler, payload));
        }
        fn deliver(&mut self, cx: &mut Cx<'_>, kind: &str, id: u32, text: &str) {
            (self.0)(cx, Hear::Deliver(kind, id, text));
        }
    }
    source(name, move || Box::new(Script(make())))
}

/// A frame that does nothing: a main that never answers what it is told.
pub fn idle(name: &str) -> String {
    script(name, || |_: &mut Cx<'_>, _: Hear<'_>| {})
}

/// The calls a script is waiting on, each with the `N` it goes on to when answered, as an `await` goes on.
pub struct Pending<N>(HashMap<u32, N>);

impl<N> Default for Pending<N> {
    fn default() -> Self {
        Self(HashMap::new())
    }
}

impl<N> Pending<N> {
    /// Makes the call `method`, to go on with `next` once it is answered.
    pub fn call(&mut self, cx: &mut Cx<'_>, method: &str, params: Value, next: N) {
        let id = cx.call(method, params);
        self.0.insert(id, next);
    }

    /// What `Hear::Deliver(kind, id, text)` answers: the `N` of that call, and its result or its error.
    pub fn answered(
        &mut self,
        kind: &str,
        id: u32,
        text: &str,
    ) -> Option<(N, Result<Value, String>)> {
        let answer = match kind {
            "resolve" => Ok(serde_json::from_str(text).unwrap()),
            "reject" => Err(text.to_owned()),
            _ => return None,
        };
        Some((self.0.remove(&id)?, answer))
    }
}

/// The guest for the file at `path`, when it is a fake's. A module starts with `\0asm`, so it never is.
pub(super) fn claim(path: &str) -> Option<Box<dyn Guest>> {
    let mut head = Vec::new();
    fs::File::open(path).ok()?.take(64).read_to_end(&mut head).ok()?;
    let n: usize = std::str::from_utf8(head.strip_prefix(MAGIC.as_bytes())?).ok()?.parse().ok()?;
    let make = GUESTS.lock().unwrap().get(n)?.clone();
    Some(make())
}

pub(super) struct Fake {
    guest: Box<dyn Guest>,
    host: Host,
    next: u32,
}

/// Starts `guest`, which can stop at once.
pub(super) fn start(guest: Box<dyn Guest>, host: Host) -> wasmtime::Result<Fake> {
    let mut fake = Fake { guest, host, next: 0 };
    fake.turn(|guest, cx| guest.start(cx))?;
    Ok(fake)
}

impl Fake {
    fn turn(&mut self, run: impl FnOnce(&mut dyn Guest, &mut Cx<'_>)) -> wasmtime::Result<()> {
        let mut cx = Cx { host: &mut self.host, next: &mut self.next, failed: None };
        run(&mut *self.guest, &mut cx);
        cx.failed.map_or(Ok(()), Err)
    }
}

impl Frame for Fake {
    fn timers(&mut self) -> &mut Vec<(Instant, u32)> {
        &mut self.host.timers
    }

    fn command(&mut self, command: Command) -> wasmtime::Result<()> {
        match command {
            Command::Event { handler, payload } => {
                let payload = serde_json::from_str(&payload).unwrap_or(Value::Null);
                self.turn(|guest, cx| guest.event(cx, &handler, &payload))
            }
            Command::Deliver { message, id, text } => {
                self.turn(|guest, cx| guest.deliver(cx, message.name(), id, &text))
            }
        }
    }

    fn fire(&mut self, id: u32) -> wasmtime::Result<()> {
        self.turn(|guest, cx| guest.timer(cx, id))
    }
}
