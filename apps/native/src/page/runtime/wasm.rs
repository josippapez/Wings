//! A plugin frame built to WebAssembly, for plugins written in Rust: a page is a module that hands over its tree and
//! hears events, a main is one that only runs, and both reach the host through a `Link`.
//!
//! The module imports, from `wings`: `render(ptr, len)` with the tree's JSON (a page's alone: a main that calls it
//! stops), `call(id, method, params)` for a host call that is answered later, `storage(op, key, json)` for one of
//! `wings.storage`'s (`get`, `set`, `delete`, `keys`, `entries`), answered at once with the pointer and length, packed
//! in an i64, of `{"ok": <answer>}` or `{"error": "<message>"}` in memory it asked `wings_alloc` for,
//! `broadcast(ptr, len)`, `timer(id, ms)`, `now()` in ms since the epoch,
//! `utc_offset(ms)` in minutes for this Mac's timezone at that instant, `focused()`, and `view_args()`, the JSON its tab
//! was opened with, handed over as `storage`'s answer is. It exports its `memory`,
//! `wings_alloc(len)` for the host to write a string into, `wings_start()`, `wings_event(handler, payload)`,
//! `wings_deliver(kind, id, text)` (see `Message`) and `wings_timer(id)`. Strings are UTF-8, as a pointer and a length;
//! one the host wrote is the module's to free.
//!
//! Each frame runs on a thread of its own, under the limits of its role (a page's or a main's): a plugin's manifest
//! declares none. What a frame may hold of the host is bounded too: its memory and table, its timers
//! (`MAX_TIMERS`), the calls it waits on (`MAX_WAITING_CALLS`) and their arguments (`MAX_PARAMS_BYTES`), and each host
//! call takes fuel from its turn (`charge`). A module is compiled once for as long as the app runs, and its compiled
//! code is kept on disk for the next launch, signed so that only code this install compiled is read back
//! (`keep_compiled_in`); the code of modules no installed plugin has is removed as the app starts (`prune_compiled`).

use super::{
    Command, Event, HEAP_BYTES, Inbox, Job, Link, Message, PAGE_HEAP_BYTES, PageRuntime, QUEUED_COMMANDS, Queue,
    Reply, Role, Store as StorageCall, THREAD_STACK_BYTES, Tree, message, overran,
};
use std::{
    cell::RefCell,
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use wasmtime::{Caller, Config, Extern, Instance, Linker, Module, Store, StoreLimits, StoreLimitsBuilder, Trap, TypedFunc};

/// The first line of the source of a page that is a WebAssembly module, before the module's path.
pub const MARKER: &str = "// wings-wasm ";

/// What one turn of a plugin's main may run: about a second of a busy loop, as a JS page's turn has a second.
const TURN_FUEL: u64 = 1_000_000_000;
/// What one turn of a page may run, ten times a main's: a page reads back what it shows in one turn, and a Claude chat
/// reading back a 96 MB session with 138 pictures ran out of a main's turn and was stopped. A page stuck in a loop is
/// stopped after about ten seconds, on its own thread.
const PAGE_TURN_FUEL: u64 = 10 * TURN_FUEL;
/// What a host call costs a turn, besides a unit for each byte it reads from or writes into the frame's memory, as a
/// `memory.copy` costs: some microseconds of a busy loop. Fuel counts the module's own instructions alone, so a loop of
/// host calls paid a few units a call, whatever the host did for it, and ran far past its turn.
const CALL_FUEL: u64 = 10_000;
/// What an import that only reads a value (`now`, `focused`, `utc_offset`) or sets a timer costs a turn.
const QUICK_FUEL: u64 = 100;
/// The most timers a frame may have set and not yet run: a frame setting them in a loop grew the host's list without
/// bound.
const MAX_TIMERS: usize = 10_000;
/// The most calls a frame may wait on, counted from the call to its answer reaching the frame: a frame calling in a
/// loop had the host hold an answer, and for some calls a thread, for each. A page waits on tens at most (one per
/// project or pane it asks about).
const MAX_WAITING_CALLS: usize = 4_096;
/// The most JSON a call's arguments may be. They are parsed into the host's memory before any call looks at them, and
/// `[0,0,...]` takes some sixteen times its length parsed. The longest a call takes is `processWrite`'s 8 MB of text.
const MAX_PARAMS_BYTES: usize = 16 * 1024 * 1024;
/// The call a module makes from its panic hook, `{ "message": <the panic, as Rust words it> }`, just before it traps:
/// the host keeps the message for the frame's fatal error, which the plugins' log keeps. A host before this one refuses
/// it as a method it doesn't know.
pub const PANICKED: &str = "panicked";
/// The most of a panic's message that is kept.
const MAX_PANIC_CHARS: usize = 1000;
/// The most elements a frame's table may have. A Rust module's one table holds a slot for each function called by
/// pointer: 726 in the largest of Wings' own (Claude chat's page). Unbounded, a table of four billion could take 32 GB.
const TABLE_ELEMENTS: usize = 65_536;

type Emit = Rc<RefCell<Box<dyn FnMut(Event)>>>;

pub(super) struct Host {
    /// A page's: a main draws nothing.
    render: bool,
    pub(super) link: Arc<dyn Link>,
    queue: Queue,
    emit: Emit,
    /// When each timer the page set is due, with the id it gave it.
    pub(super) timers: Vec<(Instant, u32)>,
    /// The calls the frame made whose answer hasn't reached it yet.
    waiting: usize,
    /// What the frame said as it panicked (`PANICKED`), for the trap that follows.
    panicked: Option<String>,
    limits: StoreLimits,
}

/// What one turn of a frame may run: a page's more than a main's.
fn turn_fuel(page: bool) -> u64 {
    if page { PAGE_TURN_FUEL } else { TURN_FUEL }
}

/// What a frame's memory may grow to, a page's more than a main's, and its one memory and one table, as a Rust module
/// has. The size is each memory's, and wasmtime takes modules with the multi-memory proposal by default: a module could
/// have declared ten thousand memories of that size.
fn limits(page: bool) -> StoreLimits {
    StoreLimitsBuilder::new()
        .memory_size(if page { PAGE_HEAP_BYTES } else { HEAP_BYTES })
        .memories(1)
        .tables(1)
        .table_elements(TABLE_ELEMENTS)
        .instances(1)
        .build()
}

/// The compiler's settings. No GC proposal, which no plugin uses: in builds without wasmtime's `gc` feature, as this
/// one, a module using its types could abort the app before 48.0.9 (GHSA-rc4p-x33f-vpr2).
fn config() -> Config {
    let mut config = Config::new();
    config.consume_fuel(true).wasm_gc(false);
    config
}

impl Host {
    pub(super) fn new(render: bool, link: Arc<dyn Link>, queue: Queue, emit: Emit) -> Self {
        Self {
            render,
            link,
            queue,
            emit,
            timers: Vec::new(),
            waiting: 0,
            panicked: None,
            limits: limits(render),
        }
    }

    /// The `render` import: the page's tree as JSON.
    pub(super) fn render(&self, json: &str) -> wasmtime::Result<()> {
        if !self.render {
            return Err(wasmtime::Error::msg("a plugin's main can't render"));
        }
        let event = match Tree::parse(json) {
            Ok(tree) => Event::Tree(tree),
            Err(error) => Event::Error {
                message: error.to_string(),
                fatal: false,
            },
        };
        (self.emit.borrow_mut())(event);
        Ok(())
    }

    /// The `call` import: a host call the frame named `id`, answered later through its queue. One made while
    /// `MAX_WAITING_CALLS` are unanswered stops the frame, and arguments past `MAX_PARAMS_BYTES` are refused unparsed.
    pub(super) fn call(&mut self, id: u32, method: &str, params: &str) -> wasmtime::Result<()> {
        if self.waiting >= MAX_WAITING_CALLS {
            return Err(wasmtime::Error::msg(format!(
                "the frame made a call while {MAX_WAITING_CALLS} of its calls were still unanswered"
            )));
        }
        self.waiting += 1;
        let reply = Reply {
            call: id,
            queue: self.queue.clone(),
            note: None,
        };
        if params.len() > MAX_PARAMS_BYTES {
            reply.err(format!("A call's arguments are limited to {} MB", MAX_PARAMS_BYTES >> 20));
            return Ok(());
        }
        let params: serde_json::Value = serde_json::from_str(params)?;
        if method == PANICKED {
            self.panicked = params["message"].as_str().map(|message| message.chars().take(MAX_PANIC_CHARS).collect());
            reply.ok("null".into());
            return Ok(());
        }
        self.link.call(method, params, reply);
        Ok(())
    }

    /// `error`, the frame's last, with what it said as it panicked before it: a Rust module that panics traps on an
    /// `unreachable`, and its message was lost.
    pub(super) fn fatal(&mut self, error: wasmtime::Error) -> wasmtime::Error {
        match self.panicked.take() {
            Some(message) => error.context(message),
            None => error,
        }
    }

    /// An answer to one of the frame's calls is about to reach it.
    fn answered(&mut self) {
        self.waiting = self.waiting.saturating_sub(1);
    }

    /// The `timer` import. One past `MAX_TIMERS` set and not yet run stops the frame.
    pub(super) fn timer(&mut self, id: u32, ms: u32) -> wasmtime::Result<()> {
        if self.timers.len() >= MAX_TIMERS {
            return Err(wasmtime::Error::msg(format!("the frame set more than {MAX_TIMERS} timers")));
        }
        let due = Instant::now() + Duration::from_millis(ms.into());
        self.timers.push((due, id));
        Ok(())
    }
}

/// A frame the serve loop runs: a module, or a test's stand-in for one.
pub(super) trait Frame {
    fn host(&mut self) -> &mut Host;
    fn command(&mut self, command: Command) -> wasmtime::Result<()>;
    fn fire(&mut self, id: u32) -> wasmtime::Result<()>;
}

/// The frame's thread by its plugin and module, `page-wasm tasks/board` for `<plugins>/tasks/dist/native/board.wasm`,
/// so a profile of the app tells which plugin a busy thread runs.
fn thread_name(path: &str) -> String {
    let path = std::path::Path::new(path);
    let plugin = path.ancestors().nth(3).and_then(|dir| dir.file_name()).map(|name| name.to_string_lossy());
    let module = path.file_stem().map(|name| name.to_string_lossy());
    match (plugin, module) {
        (Some(plugin), Some(module)) => format!("page-wasm {plugin}/{module}"),
        _ => "page-wasm".into(),
    }
}

/// Starts the frame whose module is the file at `path`. `on_event` is called from the frame's thread.
pub(super) fn open(
    path: &str,
    role: Role,
    link: Arc<dyn Link>,
    on_event: impl FnMut(Event) + Send + 'static,
) -> PageRuntime {
    let (jobs, received) = mpsc::sync_channel(QUEUED_COMMANDS);
    let (inputs, input_received) = mpsc::sync_channel(QUEUED_COMMANDS);
    let closed = Arc::new(AtomicBool::new(false));
    let stopped = Arc::new(AtomicBool::new(false));
    let queue = Queue::new(jobs.clone());
    let path = path.to_owned();
    let spawned = thread::Builder::new()
        .name(thread_name(&path))
        .stack_size(THREAD_STACK_BYTES)
        .spawn({
            let (closed, stopped) = (closed.clone(), stopped.clone());
            let (link, queue) = (link.clone(), queue.clone());
            move || {
                let emit: Emit = Rc::new(RefCell::new(Box::new(on_event)));
                let host = Host::new(role == Role::Page, link, queue, emit.clone());
                serve(&path, host, &emit, &Received { inputs: input_received, jobs: received }, &closed, &stopped)
            }
        });
    if spawned.is_err() {
        stopped.store(true, Ordering::Relaxed);
    }
    PageRuntime {
        link: Some(link),
        inbox: Inbox { queue, stopped },
        closed,
        _attached: None,
        jobs,
        inputs,
    }
}

#[cfg(unix)]
mod cache;

/// No compiled code is kept where folders can't be made this user's alone.
#[cfg(not(unix))]
mod cache {
    pub(super) struct Kept;
    impl Kept {
        pub(super) fn read(&self) -> Option<Vec<u8>> {
            None
        }
        pub(super) fn write(&self, _: &[u8]) {}
    }
    pub(super) fn keep_in(_: std::path::PathBuf) {}
    pub(super) fn kept(_: &[u8]) -> Option<Kept> {
        None
    }
    pub(super) fn prune(_: &[std::path::PathBuf]) -> std::io::Result<usize> {
        Ok(0)
    }
}

/// Keeps each module's compiled code in `dir`, so the next launch loads it in place of compiling. Said once, when the
/// app starts; without it every launch compiles. The folder is made for this user alone, and what is read back from it
/// is only what this install compiled and signed (`cache`): compiled code runs as it is, unchecked.
pub fn keep_compiled_in(dir: PathBuf) {
    cache::keep_in(dir);
}

/// Removes the compiled code of every module but `modules`, the `.wasm` files of the installed plugins, from the folder
/// `keep_compiled_in` was given. Said once, as the app starts, off the window's thread: it reads every module.
pub fn prune_compiled(modules: &[PathBuf]) {
    if let Err(error) = cache::prune(modules) {
        eprintln!("[plugins] couldn't clear compiled code no plugin has: {error}");
    }
}

/// The module `bytes`: its code from `kept` when this install compiled and signed it, and otherwise compiled, and
/// kept. Code from another version of the compiler is refused when it is read, and compiled again.
fn load(engine: &wasmtime::Engine, bytes: &[u8], kept: Option<&cache::Kept>) -> wasmtime::Result<Module> {
    if let Some(code) = kept.and_then(cache::Kept::read)
        // SAFETY: `code` is what `Module::serialize` gave this install for this very module: it carries the install's
        // signature over the module's hash and the code (`cache`), which only this app can make.
        && let Ok(module) = unsafe { Module::deserialize(engine, code) }
    {
        return Ok(module);
    }
    let module = Module::new(engine, bytes)?;
    if let Some(kept) = kept
        && let Ok(code) = module.serialize()
    {
        kept.write(&code);
    }
    Ok(module)
}

/// The compiler and its settings, one for every page: compiling is what a page's start costs, so a module's code is
/// kept by its file and that file's size and time, for the next page of it. Two pages of one module compile it once,
/// and pages of different modules compile side by side.
fn compiled(path: &str) -> wasmtime::Result<(wasmtime::Engine, Module)> {
    type Compiled = HashMap<(PathBuf, u64, Option<SystemTime>), Arc<Mutex<Option<Module>>>>;
    static ENGINE: OnceLock<(wasmtime::Engine, Mutex<Compiled>)> = OnceLock::new();
    let engine = match ENGINE.get() {
        Some(engine) => engine,
        None => {
            let engine = wasmtime::Engine::new(&config())?;
            ENGINE.get_or_init(|| (engine, Mutex::default()))
        }
    };
    let file = std::fs::metadata(path)?;
    let key = (PathBuf::from(path), file.len(), file.modified().ok());
    let slot = engine.1.lock().unwrap().entry(key).or_default().clone();
    let mut slot = slot.lock().unwrap();
    let module = match &*slot {
        Some(module) => module.clone(),
        None => {
            let bytes = std::fs::read(path)?;
            slot.insert(load(&engine.0, &bytes, cache::kept(&bytes).as_ref())?).clone()
        }
    };
    Ok((engine.0.clone(), module))
}

/// Compiles the module at `path` now, and waits for it. A test build compiles a large module for minutes, far past what
/// a test waits for a page to draw, and a test that gave up left nothing kept for the next one.
#[cfg(any(test, feature = "test-support"))]
pub fn compile_now(path: &str) -> wasmtime::Result<()> {
    compiled(path).map(|_| ())
}

/// The frame of `module`, under its limits and with its first turn's fuel, before it starts.
fn instantiate(engine: &wasmtime::Engine, module: &Module, host: Host) -> wasmtime::Result<(Store<Host>, Instance)> {
    let mut store = Store::new(engine, host);
    store.limiter(|host| &mut host.limits);
    store.set_fuel(turn_fuel(store.data().render))?;
    let instance = imports(engine)?.instantiate(&mut store, module)?;
    Ok((store, instance))
}

/// What the page exports for the host to call.
struct Page {
    store: Store<Host>,
    instance: Instance,
    alloc: TypedFunc<u32, u32>,
    event: TypedFunc<(u32, u32, u32, u32), ()>,
    deliver: TypedFunc<(u32, u32, u32, u32, u32), ()>,
    timer: TypedFunc<u32, ()>,
}

impl Page {
    fn start(path: &str, host: Host) -> wasmtime::Result<Self> {
        let (engine, module) = compiled(path)?;
        let (mut store, instance) = instantiate(&engine, &module, host)?;
        let start = instance.get_typed_func::<(), ()>(&mut store, "wings_start")?;
        let mut page = Page {
            alloc: instance.get_typed_func(&mut store, "wings_alloc")?,
            event: instance.get_typed_func(&mut store, "wings_event")?,
            deliver: instance.get_typed_func(&mut store, "wings_deliver")?,
            timer: instance.get_typed_func(&mut store, "wings_timer")?,
            store,
            instance,
        };
        if let Err(error) = start.call(&mut page.store, ()) {
            return Err(page.store.data_mut().fatal(error));
        }
        Ok(page)
    }

    /// `text` in the page's memory, as its pointer and length.
    fn give(&mut self, text: &str) -> wasmtime::Result<(u32, u32)> {
        let len = u32::try_from(text.len())?;
        let at = self.alloc.call(&mut self.store, len)?;
        let memory = self
            .instance
            .get_memory(&mut self.store, "memory")
            .ok_or_else(|| wasmtime::Error::msg("the page exports no memory"))?;
        memory.write(&mut self.store, at as usize, text.as_bytes())?;
        Ok((at, len))
    }
}

impl Frame for Page {
    fn host(&mut self) -> &mut Host {
        self.store.data_mut()
    }

    fn command(&mut self, command: Command) -> wasmtime::Result<()> {
        self.store.set_fuel(turn_fuel(self.store.data().render))?;
        match command {
            Command::Event { handler, payload } => {
                let (handler, handler_len) = self.give(&handler)?;
                let (payload, payload_len) = self.give(&payload)?;
                self.event
                    .call(&mut self.store, (handler, handler_len, payload, payload_len))
            }
            Command::Deliver {
                message: kind,
                id,
                text,
            } => {
                let (kind, kind_len) = self.give(kind.name())?;
                let (text, text_len) = self.give(&text)?;
                self.deliver
                    .call(&mut self.store, (kind, kind_len, id, text, text_len))
            }
        }
    }

    fn fire(&mut self, id: u32) -> wasmtime::Result<()> {
        self.store.set_fuel(turn_fuel(self.store.data().render))?;
        self.timer.call(&mut self.store, id)
    }
}

/// A frame's two queues: what you do on the page, and everything else.
struct Received {
    inputs: mpsc::Receiver<Command>,
    jobs: mpsc::Receiver<Job>,
}

fn serve(
    path: &str,
    host: Host,
    emit: &Emit,
    received: &Received,
    closed: &AtomicBool,
    stopped: &AtomicBool,
) {
    let fail = |error: wasmtime::Error| {
        stopped.store(true, Ordering::Relaxed);
        let ran_out = error.downcast_ref::<Trap>() == Some(&Trap::OutOfFuel);
        (emit.borrow_mut())(Event::Error {
            message: if ran_out { overran() } else { fatal_message(&error) },
            fatal: true,
        });
    };
    host.link.wait_to_join();
    #[cfg(any(test, feature = "test-support"))]
    if let Some(guest) = super::fake::claim(path) {
        return match super::fake::start(guest, host) {
            Ok(frame) => run(frame, received, closed, fail),
            Err(error) => fail(error),
        };
    }
    match Page::start(path, host) {
        Ok(page) => run(page, received, closed, fail),
        Err(error) => fail(error),
    }
}

/// A frame's last error as one short text: what happened (the panic and the trap, or a host call's own error), then
/// where. wasmtime puts where first, and `message` keeps the first 500 characters: each of the 115 traps in a plugins'
/// log said only where, never what.
fn fatal_message(error: &wasmtime::Error) -> String {
    let backtrace = error.downcast_ref::<wasmtime::WasmBacktrace>().map(ToString::to_string);
    let mut said: Vec<String> =
        error.chain().map(ToString::to_string).filter(|said| Some(said) != backtrace.as_ref()).collect();
    said.extend(backtrace);
    message(said.join(": "))
}

fn run(
    mut page: impl Frame,
    received: &Received,
    closed: &AtomicBool,
    fail: impl Fn(wasmtime::Error),
) {
    let jobs = &received.jobs;
    loop {
        let wait = page
            .host()
            .timers
            .iter()
            .map(|(due, _)| due.saturating_duration_since(Instant::now()))
            .min();
        let job = match received.inputs.try_recv() {
            Ok(input) => Some(Job::Frame(input)),
            Err(_) => match wait {
                Some(wait) => match jobs.recv_timeout(wait) {
                    Ok(job) => Some(job),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                },
                None => match jobs.recv() {
                    Ok(job) => Some(job),
                    Err(_) => return,
                },
            },
        };
        if closed.load(Ordering::Relaxed) {
            return;
        }
        if let Some(Job::Frame(command)) = job {
            if let Command::Deliver { message: Message::Resolve | Message::Reject, .. } = command {
                page.host().answered();
            }
            if let Err(error) = page.command(command) {
                return fail(page.host().fatal(error));
            }
        }
        let now = Instant::now();
        let timers = &mut page.host().timers;
        let due: Vec<u32> = timers.iter().filter(|(at, _)| *at <= now).map(|(_, id)| *id).collect();
        timers.retain(|(at, _)| *at > now);
        for id in due {
            if let Err(error) = page.fire(id) {
                return fail(page.host().fatal(error));
            }
        }
    }
}

/// The page's string at `at`, `len` bytes long.
fn text(caller: &mut Caller<'_, Host>, at: u32, len: u32) -> wasmtime::Result<String> {
    let memory = caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| wasmtime::Error::msg("the page exports no memory"))?;
    let bytes = memory
        .data(&caller)
        .get(at as usize..at as usize + len as usize)
        .ok_or_else(|| wasmtime::Error::msg("a string outside the page's memory"))?;
    Ok(std::str::from_utf8(bytes)?.to_owned())
}

/// Writes `text` into memory the frame allocates for it, and gives where it is: the pointer in the high half, the
/// length in the low. The frame frees it.
fn hand(caller: &mut Caller<'_, Host>, text: &str) -> wasmtime::Result<u64> {
    let len = u32::try_from(text.len())?;
    let alloc = caller
        .get_export("wings_alloc")
        .and_then(Extern::into_func)
        .ok_or_else(|| wasmtime::Error::msg("the frame exports no wings_alloc"))?
        .typed::<u32, u32>(&*caller)?;
    let at = alloc.call(&mut *caller, len)?;
    let memory = caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| wasmtime::Error::msg("the page exports no memory"))?;
    memory.write(&mut *caller, at as usize, text.as_bytes())?;
    Ok(u64::from(at) << 32 | u64::from(len))
}

/// Takes what a host call costs from the turn's fuel: `CALL_FUEL` and a unit for each of the `bytes` it moves in or out
/// of the frame's memory, or `QUICK_FUEL` for one that only reads a value. A turn that can't pay stops as one that ran
/// out.
fn charge(caller: &mut Caller<'_, Host>, cost: u64) -> wasmtime::Result<()> {
    let fuel = caller.get_fuel()?;
    if fuel < cost {
        caller.set_fuel(0)?;
        return Err(Trap::OutOfFuel.into());
    }
    caller.set_fuel(fuel - cost)
}

fn imports(engine: &wasmtime::Engine) -> wasmtime::Result<Linker<Host>> {
    let mut linker = Linker::new(engine);
    linker.func_wrap("wings", "render", |mut caller: Caller<'_, Host>, at: u32, len: u32| {
        charge(&mut caller, CALL_FUEL + u64::from(len))?;
        let json = text(&mut caller, at, len)?;
        caller.data().render(&json)
    })?;
    linker.func_wrap(
        "wings",
        "call",
        |mut caller: Caller<'_, Host>, id: u32, method: u32, method_len: u32, params: u32, params_len: u32| {
            charge(&mut caller, CALL_FUEL + u64::from(method_len) + u64::from(params_len))?;
            let method = text(&mut caller, method, method_len)?;
            let params = text(&mut caller, params, params_len)?;
            caller.data_mut().call(id, &method, &params)
        },
    )?;
    linker.func_wrap(
        "wings",
        "storage",
        |mut caller: Caller<'_, Host>, op: u32, op_len: u32, key: u32, key_len: u32, json: u32, json_len: u32| {
            charge(&mut caller, CALL_FUEL + u64::from(op_len) + u64::from(key_len) + u64::from(json_len))?;
            let op = text(&mut caller, op, op_len)?;
            let key = text(&mut caller, key, key_len)?;
            let json = text(&mut caller, json, json_len)?;
            let call = match op.as_str() {
                "get" => StorageCall::Get(key),
                "set" => StorageCall::Set(key, json),
                "delete" => StorageCall::Delete(key),
                "keys" => StorageCall::Keys,
                "entries" => StorageCall::Entries(key),
                _ => return Err(wasmtime::Error::msg("unknown storage call")),
            };
            let answer = match caller.data().link.storage(call) {
                Ok(json) => format!("{{\"ok\":{json}}}"),
                Err(error) => serde_json::json!({ "error": error }).to_string(),
            };
            charge(&mut caller, answer.len() as u64)?;
            hand(&mut caller, &answer)
        },
    )?;
    // What the page's tab was opened with, as JSON. Nothing (a length of 0) for a page opened without args.
    linker.func_wrap("wings", "view_args", |mut caller: Caller<'_, Host>| {
        let args = caller.data().link.view_args();
        charge(&mut caller, CALL_FUEL + args.as_ref().map_or(0, |args| args.len() as u64))?;
        match args {
            Some(args) => hand(&mut caller, &args),
            None => Ok(0),
        }
    })?;
    linker.func_wrap("wings", "broadcast", |mut caller: Caller<'_, Host>, at: u32, len: u32| {
        charge(&mut caller, CALL_FUEL + u64::from(len))?;
        let json = text(&mut caller, at, len)?;
        // A broadcast the host refuses (too long) is the page's loss, as a JS page's rejected promise nobody awaits.
        let _ = caller.data().link.broadcast(json);
        Ok(())
    })?;
    linker.func_wrap("wings", "timer", |mut caller: Caller<'_, Host>, id: u32, ms: u32| {
        charge(&mut caller, QUICK_FUEL)?;
        caller.data_mut().timer(id, ms)
    })?;
    linker.func_wrap("wings", "now", |mut caller: Caller<'_, Host>| {
        charge(&mut caller, QUICK_FUEL)?;
        Ok(SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |since| since.as_secs_f64() * 1000.0))
    })?;
    // debt: UTC where `localtime_r` isn't (Windows).
    #[cfg(not(unix))]
    linker.func_wrap("wings", "utc_offset", |mut caller: Caller<'_, Host>, _: f64| -> wasmtime::Result<i32> {
        charge(&mut caller, QUICK_FUEL)?;
        Ok(0)
    })?;
    #[cfg(unix)]
    linker.func_wrap("wings", "utc_offset", |mut caller: Caller<'_, Host>, ms: f64| -> wasmtime::Result<i32> {
        charge(&mut caller, QUICK_FUEL)?;
        let at = (ms / 1000.0).floor() as libc::time_t;
        // SAFETY: `localtime_r` writes the `tm` it is given and keeps neither pointer.
        let local = unsafe {
            let mut local = std::mem::zeroed::<libc::tm>();
            libc::localtime_r(&at, &mut local);
            local
        };
        Ok((local.tm_gmtoff / 60) as i32)
    })?;
    linker.func_wrap("wings", "focused", |mut caller: Caller<'_, Host>| {
        charge(&mut caller, QUICK_FUEL)?;
        Ok(caller.data().link.focused() as i32)
    })?;
    Ok(linker)
}

#[cfg(test)]
mod tests {
    use super::{cache::tests::kept_in, *};

    #[test]
    fn a_frames_thread_is_named_by_its_plugin_and_module() {
        assert_eq!(thread_name("/data/plugins/tasks/dist/native/board.wasm"), "page-wasm tasks/board");
        assert_eq!(thread_name("board.wasm"), "page-wasm");
    }

    /// A module with nothing in it: the header alone.
    const EMPTY: &[u8] = b"\0asm\x01\0\0\0";

    /// A module of `sections`, each its id and contents.
    fn module(sections: &[(u8, Vec<u8>)]) -> Vec<u8> {
        let mut bytes = EMPTY.to_vec();
        for (id, contents) in sections {
            bytes.push(*id);
            leb(&mut bytes, contents.len() as u32);
            bytes.extend(contents);
        }
        bytes
    }

    fn leb(out: &mut Vec<u8>, mut n: u32) {
        loop {
            let byte = (n & 0x7f) as u8;
            n >>= 7;
            if n == 0 {
                return out.push(byte);
            }
            out.push(byte | 0x80);
        }
    }

    fn name(out: &mut Vec<u8>, name: &str) {
        leb(out, name.len() as u32);
        out.extend(name.as_bytes());
    }

    /// A module that exports one function, `export`, that does nothing.
    fn exporting(export: &str) -> Vec<u8> {
        let mut exports = vec![1];
        name(&mut exports, export);
        exports.extend([0, 0]);
        module(&[(1, vec![1, 0x60, 0, 0]), (3, vec![1, 0]), (7, exports), (10, vec![1, 2, 0, 0x0b])])
    }

    /// A link that refuses what a frame asks of it.
    struct Quiet;

    impl Link for Quiet {
        fn broadcast(&self, _: String) -> Result<(), String> {
            Ok(())
        }
        fn storage(&self, _: StorageCall) -> Result<String, String> {
            Err("no storage".into())
        }
        fn call(&self, method: &str, _: serde_json::Value, reply: Reply) {
            reply.err(format!("no {method}"));
        }
    }

    fn host(page: bool) -> Host {
        let (jobs, _) = mpsc::sync_channel(1);
        Host::new(page, Arc::new(Quiet), Queue::new(jobs), Rc::new(RefCell::new(Box::new(|_| {}))))
    }

    fn engine() -> wasmtime::Engine {
        wasmtime::Engine::new(&config()).unwrap()
    }

    #[test]
    fn a_modules_compiled_code_is_kept_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let kept = kept_in(dir.path(), EMPTY);
        let engine = engine();
        load(&engine, EMPTY, Some(&kept)).unwrap();
        let written = std::fs::read(&kept.file).expect("the compiled code, kept");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2, "the code and the secret, and no part file");
        // Bytes that aren't a module compile to nothing, so a module from them can only be the kept code.
        load(&engine, b"not a module", Some(&kept)).expect("the kept code, read back");
        assert_eq!(std::fs::read(&kept.file).unwrap(), written);
        assert!(load(&engine, b"not a module", None).is_err());
    }

    /// `Module::deserialize` runs any compiled code it is handed. A cache file another program swapped in, here a
    /// module's code as `Module::serialize` gives it, ran in the app in place of the module's own.
    #[test]
    fn a_cache_file_swapped_from_outside_is_compiled_again_and_never_run() {
        let dir = tempfile::tempdir().unwrap();
        let engine = engine();
        let (own, other) = (exporting("own"), exporting("other"));
        let kept = kept_in(dir.path(), &own);
        load(&engine, &own, Some(&kept)).unwrap();

        std::fs::write(&kept.file, Module::new(&engine, &other).unwrap().serialize().unwrap()).unwrap();
        let loaded = load(&engine, &own, Some(&kept)).unwrap();
        let exports: Vec<_> = loaded.exports().map(|export| export.name().to_owned()).collect();
        assert_eq!(exports, ["own"]);
        // And what it compiled in its place is kept, signed.
        assert!(load(&engine, b"not a module", Some(&kept)).is_ok());
    }

    /// No plugin uses the GC proposal, and in builds without wasmtime's `gc` feature, as this one, a module using its
    /// function subtypes could abort the app before 48.0.9 (GHSA-rc4p-x33f-vpr2): such a module isn't compiled at all.
    #[test]
    fn a_module_using_the_gc_proposal_is_not_compiled() {
        // (type (sub (func))): a function type that isn't final, which only the GC proposal has.
        let subtype = module(&[(1, vec![1, 0x50, 0, 0x60, 0, 0])]);
        let default = wasmtime::Engine::new(Config::new().consume_fuel(true)).unwrap();
        assert!(Module::new(&default, &subtype).is_ok(), "the compiler's own settings take it");
        assert!(Module::new(&engine(), &subtype).is_err());
    }

    /// One memory and one table of a size a plugin's has: a memory's limit is each memory's, and a module could have
    /// declared ten thousand of them, or a table of four billion slots.
    #[test]
    fn a_module_with_a_second_memory_or_a_huge_table_does_not_start() {
        let engine = engine();
        let start = |bytes: &[u8]| instantiate(&engine, &Module::new(&engine, bytes).unwrap(), host(true)).map(|_| ());
        let mut table = vec![1, 0x70, 0];
        leb(&mut table, 726);
        assert!(start(&module(&[(4, table), (5, vec![1, 0, 17])])).is_ok(), "Claude chat's page's table and memory");
        assert!(start(&module(&[(5, vec![2, 0, 1, 0, 1])])).is_err(), "a second memory");
        let mut huge = vec![1, 0x70, 0];
        leb(&mut huge, 100_000_000);
        assert!(start(&module(&[(4, huge)])).is_err(), "a table of 100 million slots");
        assert!(start(&module(&[(4, vec![2, 0x70, 0, 1, 0x70, 0, 1])])).is_err(), "a second table");
    }

    /// The log kept the first 500 characters of a trap, and wasmtime words where it happened first: every trap logged
    /// said where, and not what.
    #[test]
    fn a_traps_message_says_what_happened_before_where() {
        // (func (export "boom") unreachable), called through a chain of functions, as a panic deep in a module is.
        let mut exports = vec![1];
        name(&mut exports, "boom");
        exports.extend([0, 0]);
        let mut code = vec![40];
        for function in 0..40u8 {
            // Each calls the next, and the last traps.
            let body: Vec<u8> = if function == 39 { vec![0, 0x00, 0x0b] } else { vec![0, 0x10, function + 1, 0x0b] };
            code.push(body.len() as u8);
            code.extend(body);
        }
        let mut functions = vec![40];
        functions.extend([0; 40]);
        let boom = module(&[(1, vec![1, 0x60, 0, 0]), (3, functions), (7, exports), (10, code)]);
        let engine = engine();
        let (mut store, instance) = instantiate(&engine, &Module::new(&engine, &boom).unwrap(), host(true)).unwrap();
        let error = instance.get_typed_func::<(), ()>(&mut store, "boom").unwrap().call(&mut store, ()).unwrap_err();
        let said = fatal_message(&store.data_mut().fatal(error));
        assert!(said.starts_with("wasm trap: wasm `unreachable` instruction executed: error while executing at wasm backtrace:"), "{said}");
    }

    /// Fuel counts the module's instructions alone: a main calling the host in a loop, each call a few instructions,
    /// ran for minutes past the second its turn has. Each call now costs the turn `CALL_FUEL`.
    #[test]
    fn a_frame_calling_the_host_in_a_loop_runs_out_of_its_turn() {
        // (import "wings" "broadcast" (func (param i32 i32))) (memory (export "memory") 1)
        // (func (export "spin") (loop (call 0 (i32.const 0) (i32.const 0)) (br 0)))
        let mut imports = vec![1];
        name(&mut imports, "wings");
        name(&mut imports, "broadcast");
        imports.extend([0, 0]);
        let mut exports = vec![2];
        name(&mut exports, "memory");
        exports.extend([2, 0]);
        name(&mut exports, "spin");
        exports.extend([0, 1]);
        let body = [0, 0x03, 0x40, 0x41, 0, 0x41, 0, 0x10, 0, 0x0c, 0, 0x0b, 0x0b];
        let mut code = vec![1, body.len() as u8];
        code.extend(body);
        let spin = module(&[
            (1, vec![2, 0x60, 2, 0x7f, 0x7f, 0, 0x60, 0, 0]),
            (2, imports),
            (3, vec![1, 1]),
            (5, vec![1, 0, 1]),
            (7, exports),
            (10, code),
        ]);
        let (done, ended) = mpsc::channel();
        thread::spawn(move || {
            let engine = engine();
            let (mut store, instance) = instantiate(&engine, &Module::new(&engine, &spin).unwrap(), host(false)).unwrap();
            let spin = instance.get_typed_func::<(), ()>(&mut store, "spin").unwrap();
            let _ = done.send(spin.call(&mut store, ()).unwrap_err().downcast_ref::<Trap>().copied());
        });
        let stopped = ended.recv_timeout(Duration::from_secs(20)).expect("the loop to be stopped within its turn");
        assert_eq!(stopped, Some(Trap::OutOfFuel));
    }
}

#[cfg(test)]
mod heap_tests {
    use wasmtime::ResourceLimiter;

    use super::{limits, turn_fuel};

    /// A page may grow past the 64 MB a main keeps to: a long chat's page ran out there and stopped.
    #[test]
    fn a_page_may_grow_to_512_mb_and_a_main_to_64() {
        const MB: usize = 1024 * 1024;
        let (mut page, mut main) = (limits(true), limits(false));
        assert!(page.memory_growing(0, 500 * MB, None).unwrap());
        assert!(!page.memory_growing(0, 513 * MB, None).unwrap());
        assert!(main.memory_growing(0, 64 * MB, None).unwrap());
        assert!(!main.memory_growing(0, 65 * MB, None).unwrap());
    }

    /// A page's turn may run ten times a main's: a long chat's read-back passed a main's second and was stopped.
    #[test]
    fn a_pages_turn_may_run_ten_times_a_mains() {
        assert_eq!(turn_fuel(false), 1_000_000_000);
        assert_eq!(turn_fuel(true), 10_000_000_000);
    }
}
