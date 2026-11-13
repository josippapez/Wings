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
//! debt: each such frame has a thread of its own, and `Limits::default()` whatever its plugin's are. A module is compiled once for as long
//! as the app runs, and its compiled code is kept on disk for the next launch (`keep_compiled_in`), never pruned: each
//! build of a module leaves a file a few times its size. Clear the folder of the ones no installed plugin has if it grows.

use super::{
    Command, Event, HEAP_BYTES, Inbox, Job, Link, PageRuntime, QUEUED_COMMANDS, Queue,
    Reply, Role, THREAD_STACK_BYTES, Tree, message, overran,
};
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    path::PathBuf,
};
use super::Store as StorageCall;
use wasmtime::{Caller, Config, Extern, Instance, Linker, Module, Store, StoreLimits, StoreLimitsBuilder, Trap, TypedFunc};

/// The first line of the source of a page that is a WebAssembly module, before the module's path.
pub const MARKER: &str = "// wings-wasm ";

/// What one turn may run: about a second of a busy loop, as a JS page's turn has a second.
const TURN_FUEL: u64 = 1_000_000_000;

type Emit = Rc<RefCell<Box<dyn FnMut(Event)>>>;

pub(super) struct Host {
    /// A page's: a main draws nothing.
    render: bool,
    pub(super) link: Arc<dyn Link>,
    queue: Queue,
    emit: Emit,
    /// When each timer the page set is due, with the id it gave it.
    pub(super) timers: Vec<(Instant, u32)>,
    limits: StoreLimits,
}

impl Host {
    pub(super) fn new(render: bool, link: Arc<dyn Link>, queue: Queue, emit: Emit) -> Self {
        Self {
            render,
            link,
            queue,
            emit,
            timers: Vec::new(),
            limits: StoreLimitsBuilder::new()
                .memory_size(HEAP_BYTES)
                .build(),
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

    /// The `call` import: a host call the frame named `id`, answered later through its queue.
    pub(super) fn call(&self, id: u32, method: &str, params: &str) -> wasmtime::Result<()> {
        self.link.call(
            method,
            serde_json::from_str(params)?,
            Reply {
                call: id,
                queue: self.queue.clone(),
            },
        );
        Ok(())
    }

    /// The `timer` import.
    pub(super) fn timer(&mut self, id: u32, ms: u32) {
        let due = Instant::now() + Duration::from_millis(ms.into());
        self.timers.push((due, id));
    }
}

/// A frame the serve loop runs: a module, or a test's stand-in for one.
pub(super) trait Frame {
    fn timers(&mut self) -> &mut Vec<(Instant, u32)>;
    fn command(&mut self, command: Command) -> wasmtime::Result<()>;
    fn fire(&mut self, id: u32) -> wasmtime::Result<()>;
}

/// Starts the frame whose module is the file at `path`. `on_event` is called from the frame's thread.
pub(super) fn open(
    path: &str,
    role: Role,
    link: Arc<dyn Link>,
    on_event: impl FnMut(Event) + Send + 'static,
) -> PageRuntime {
    let (jobs, received) = mpsc::sync_channel(QUEUED_COMMANDS);
    let closed = Arc::new(AtomicBool::new(false));
    let stopped = Arc::new(AtomicBool::new(false));
    let queue = Queue {
        jobs: jobs.clone(),
    };
    let path = path.to_owned();
    let spawned = thread::Builder::new()
        .name("page-wasm".into())
        .stack_size(THREAD_STACK_BYTES)
        .spawn({
            let (closed, stopped) = (closed.clone(), stopped.clone());
            let (link, queue) = (link.clone(), queue.clone());
            move || {
                let emit: Emit = Rc::new(RefCell::new(Box::new(on_event)));
                let host = Host::new(role == Role::Page, link, queue, emit.clone());
                serve(&path, host, &emit, &received, &closed, &stopped)
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
    }
}

static KEPT: OnceLock<PathBuf> = OnceLock::new();

/// Keeps each module's compiled code in `dir`, so the next launch loads it in place of compiling. Said once, when the
/// app starts; without it every launch compiles. The folder is made for this user alone, and has to stay the app's own:
/// what is read back from it runs as it is, unchecked, so nothing a plugin can write may ever land there.
pub fn keep_compiled_in(dir: PathBuf) {
    let _ = KEPT.set(dir);
}

/// Where the compiled code of the module `bytes` is kept: a file named by the module's hash, so another build of the
/// page is another file. Code from another version of the compiler is refused when it is read, and compiled again.
#[cfg(unix)]
fn kept_file(bytes: &[u8]) -> Option<PathBuf> {
    use sha2::{Digest, Sha256};
    let dir = KEPT.get()?;
    crate::terminal_server::private_dir(dir).ok()?;
    let hash: String = Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect();
    Some(dir.join(format!("{hash}.cwasm")))
}

#[cfg(not(unix))]
fn kept_file(_: &[u8]) -> Option<PathBuf> {
    None
}

/// The module `bytes`: its code from `kept` when it is there and this compiler's, and otherwise compiled, and kept.
fn load(engine: &wasmtime::Engine, bytes: &[u8], kept: Option<&std::path::Path>) -> wasmtime::Result<Module> {
    if let Some(code) = kept.and_then(|file| std::fs::read(file).ok())
        // SAFETY: the file is in the folder `keep_compiled_in` was given, which only this user can open
        // (`private_dir`) and only `load` writes, with what `Module::serialize` gave it.
        && let Ok(module) = unsafe { Module::deserialize(engine, code) }
    {
        return Ok(module);
    }
    let module = Module::new(engine, bytes)?;
    if let Some(file) = kept
        && let Ok(code) = module.serialize()
    {
        // Whole or not at all: a launch that reads while this one writes sees the old file or none.
        let part = file.with_extension(format!("{}.part", std::process::id()));
        if std::fs::write(&part, code).and_then(|()| std::fs::rename(&part, file)).is_err() {
            let _ = std::fs::remove_file(&part);
        }
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
            let engine = wasmtime::Engine::new(Config::new().consume_fuel(true))?;
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
            slot.insert(load(&engine.0, &bytes, kept_file(&bytes).as_deref())?).clone()
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
        let mut store = Store::new(&engine, host);
        store.limiter(|host| &mut host.limits);
        store.set_fuel(TURN_FUEL)?;
        let instance = imports(&engine)?.instantiate(&mut store, &module)?;
        let start = instance.get_typed_func::<(), ()>(&mut store, "wings_start")?;
        let mut page = Page {
            alloc: instance.get_typed_func(&mut store, "wings_alloc")?,
            event: instance.get_typed_func(&mut store, "wings_event")?,
            deliver: instance.get_typed_func(&mut store, "wings_deliver")?,
            timer: instance.get_typed_func(&mut store, "wings_timer")?,
            store,
            instance,
        };
        start.call(&mut page.store, ())?;
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
    fn timers(&mut self) -> &mut Vec<(Instant, u32)> {
        &mut self.store.data_mut().timers
    }

    fn command(&mut self, command: Command) -> wasmtime::Result<()> {
        self.store.set_fuel(TURN_FUEL)?;
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
        self.store.set_fuel(TURN_FUEL)?;
        self.timer.call(&mut self.store, id)
    }
}

fn serve(
    path: &str,
    host: Host,
    emit: &Emit,
    jobs: &mpsc::Receiver<Job>,
    closed: &AtomicBool,
    stopped: &AtomicBool,
) {
    let fail = |error: wasmtime::Error| {
        stopped.store(true, Ordering::Relaxed);
        let ran_out = error.downcast_ref::<Trap>() == Some(&Trap::OutOfFuel);
        (emit.borrow_mut())(Event::Error {
            message: if ran_out { overran() } else { message(format!("{error:#}")) },
            fatal: true,
        });
    };
    #[cfg(any(test, feature = "test-support"))]
    if let Some(guest) = super::fake::claim(path) {
        return match super::fake::start(guest, host) {
            Ok(frame) => run(frame, jobs, closed, fail),
            Err(error) => fail(error),
        };
    }
    match Page::start(path, host) {
        Ok(page) => run(page, jobs, closed, fail),
        Err(error) => fail(error),
    }
}

fn run(
    mut page: impl Frame,
    jobs: &mpsc::Receiver<Job>,
    closed: &AtomicBool,
    fail: impl Fn(wasmtime::Error),
) {
    loop {
        let wait = page
            .timers()
            .iter()
            .map(|(due, _)| due.saturating_duration_since(Instant::now()))
            .min();
        let job = match wait {
            Some(wait) => match jobs.recv_timeout(wait) {
                Ok(job) => Some(job),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            },
            None => match jobs.recv() {
                Ok(job) => Some(job),
                Err(_) => return,
            },
        };
        if closed.load(Ordering::Relaxed) {
            return;
        }
        if let Some(Job::Frame(command)) = job
            && let Err(error) = page.command(command)
        {
            return fail(error);
        }
        let now = Instant::now();
        let timers = page.timers();
        let due: Vec<u32> = timers.iter().filter(|(at, _)| *at <= now).map(|(_, id)| *id).collect();
        timers.retain(|(at, _)| *at > now);
        for id in due {
            if let Err(error) = page.fire(id) {
                return fail(error);
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

fn imports(engine: &wasmtime::Engine) -> wasmtime::Result<Linker<Host>> {
    let mut linker = Linker::new(engine);
    linker.func_wrap("wings", "render", |mut caller: Caller<'_, Host>, at: u32, len: u32| {
        let json = text(&mut caller, at, len)?;
        caller.data().render(&json)
    })?;
    linker.func_wrap(
        "wings",
        "call",
        |mut caller: Caller<'_, Host>, id: u32, method: u32, method_len: u32, params: u32, params_len: u32| {
            let method = text(&mut caller, method, method_len)?;
            let params = text(&mut caller, params, params_len)?;
            caller.data().call(id, &method, &params)
        },
    )?;
    linker.func_wrap(
        "wings",
        "storage",
        |mut caller: Caller<'_, Host>, op: u32, op_len: u32, key: u32, key_len: u32, json: u32, json_len: u32| {
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
            hand(&mut caller, &answer)
        },
    )?;
    // What the page's tab was opened with, as JSON. Nothing (a length of 0) for a page opened without args.
    linker.func_wrap("wings", "view_args", |mut caller: Caller<'_, Host>| match caller.data().link.view_args() {
        Some(args) => hand(&mut caller, &args),
        None => Ok(0),
    })?;
    linker.func_wrap("wings", "broadcast", |mut caller: Caller<'_, Host>, at: u32, len: u32| {
        let json = text(&mut caller, at, len)?;
        // A broadcast the host refuses (too long) is the page's loss, as a JS page's rejected promise nobody awaits.
        let _ = caller.data().link.broadcast(json);
        Ok(())
    })?;
    linker.func_wrap("wings", "timer", |mut caller: Caller<'_, Host>, id: u32, ms: u32| {
        caller.data_mut().timer(id, ms);
    })?;
    linker.func_wrap("wings", "now", || {
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |since| since.as_secs_f64() * 1000.0)
    })?;
    // debt: UTC where `localtime_r` isn't (Windows).
    #[cfg(not(unix))]
    linker.func_wrap("wings", "utc_offset", |_: f64| -> i32 { 0 })?;
    #[cfg(unix)]
    linker.func_wrap("wings", "utc_offset", |ms: f64| -> i32 {
        let at = (ms / 1000.0).floor() as libc::time_t;
        // SAFETY: `localtime_r` writes the `tm` it is given and keeps neither pointer.
        let local = unsafe {
            let mut local = std::mem::zeroed::<libc::tm>();
            libc::localtime_r(&at, &mut local);
            local
        };
        (local.tm_gmtoff / 60) as i32
    })?;
    linker.func_wrap("wings", "focused", |caller: Caller<'_, Host>| caller.data().link.focused() as i32)?;
    Ok(linker)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A module with nothing in it: the header alone.
    const EMPTY: &[u8] = b"\0asm\x01\0\0\0";

    #[test]
    fn a_modules_compiled_code_is_kept_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let kept = dir.path().join("empty.cwasm");
        let engine = wasmtime::Engine::new(Config::new().consume_fuel(true)).unwrap();
        load(&engine, EMPTY, Some(&kept)).unwrap();
        let written = std::fs::read(&kept).expect("the compiled code, kept");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "no part file is left");
        // Bytes that aren't a module compile to nothing, so a module from them can only be the kept code.
        load(&engine, b"not a module", Some(&kept)).expect("the kept code, read back");
        assert_eq!(std::fs::read(&kept).unwrap(), written);
        assert!(load(&engine, b"not a module", None).is_err());
    }
}
