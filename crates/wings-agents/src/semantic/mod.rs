//! Semantic search of past sessions: every Claude Code transcript on this Mac, the subagents' too (counted toward the
//! session that ran them), embedded in the background with a small local model, so a search worded differently from the
//! session still finds it. `History::search` fuses what this ranks with its keyword matches (`fuse`).
//!
//! The indexer runs on a thread of its own (`start`) and embeds a message at a time in the helper process
//! (`model::Helper`, at utility priority, one thread). It stops feeding the helper when a key is pressed and goes on a
//! few seconds after the last one; the helper ends after a minute with nothing to do. Its progress is a few numbers
//! (`Progress`) the app's sidebar reads when it draws, so a step of it draws nothing. Nothing leaves this Mac but the
//! one download of the model.

pub mod index;
pub mod model;

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering},
    },
    time::{Duration, Instant, UNIX_EPOCH},
};

use index::{Best, DIM, Header, Index, OnDisk, Row};
use model::Embed;

/// A key pressed this recently pauses the indexing.
const TYPING: Duration = Duration::from_secs(3);
/// The helper ends after this long with nothing to embed.
const HELPER_IDLE: Duration = Duration::from_secs(60);
/// How often the transcripts are looked at again once all are indexed.
const RESCAN: Duration = Duration::from_secs(60);
/// After a failure (the model wouldn't download, the helper wouldn't start), the next try waits this long.
const RETRY: Duration = Duration::from_secs(600);
/// While indexing, the index is saved this often, so a quit loses little: once per sessions, a first round wrote its
/// file over a thousand times.
const SAVE_EVERY: Duration = Duration::from_secs(60);
/// Fewer sessions to go than this is a catch-up: it shows no progress.
pub const SHOW_AT: u32 = 10;
/// A transcript is embedded once it has gone this long without a change: while an agent works, it changes every few
/// seconds, and each look re-read it and embedded its last message again.
const QUIET: Duration = Duration::from_secs(300);
/// A message's text the helper is given at most, in bytes: a chunk is cut well under it.
const MAX_TEXT: usize = 4 * index::CHUNK_BYTES;

/// What the indexer is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum State {
    /// Turned off in Settings.
    Off = 0,
    /// Looking at the transcripts, or waiting for the next look.
    Ready = 1,
    /// Fetching the model.
    Downloading = 2,
    /// Embedding the sessions that changed.
    Indexing = 3,
    /// The model or the helper failed; it tries again later.
    Failed = 4,
}

impl State {
    fn from(n: u8) -> Self {
        match n {
            0 => State::Off,
            2 => State::Downloading,
            3 => State::Indexing,
            4 => State::Failed,
            _ => State::Ready,
        }
    }
}

/// The indexer's progress, which the app reads whenever it draws: setting it draws nothing.
#[derive(Default)]
pub struct Progress {
    state: AtomicU8,
    done: AtomicU32,
    total: AtomicU32,
}

impl Progress {
    pub fn state(&self) -> State {
        State::from(self.state.load(Ordering::Relaxed))
    }
    /// Sessions indexed so far in this round, and in all of it.
    pub fn counts(&self) -> (u32, u32) {
        (self.done.load(Ordering::Relaxed), self.total.load(Ordering::Relaxed))
    }
    /// A round big enough to show, and how far it is, as a share.
    pub fn shown(&self) -> Option<f32> {
        let (done, total) = self.counts();
        (self.state() == State::Indexing && total >= SHOW_AT).then(|| (done as f32 / total as f32).clamp(0.0, 1.0))
    }
    /// Sets it, as the indexer does, and for screenshots and tests.
    pub fn set(&self, state: State, done: u32, total: u32) {
        self.done.store(done, Ordering::Relaxed);
        self.total.store(total, Ordering::Relaxed);
        self.state.store(state as u8, Ordering::Relaxed);
    }
}

/// What Settings shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    pub enabled: bool,
    pub state: State,
    pub done: u32,
    pub total: u32,
    /// Sessions with vectors, and the index's size on disk.
    pub sessions: usize,
    pub bytes: u64,
    pub error: Option<String>,
}

/// Makes the embedder when one is needed. `download`: it may fetch the model first (the indexer may, a search may not).
pub type MakeEmbedder = Box<dyn Fn(bool) -> Result<Box<dyn Embed>, String> + Send + Sync>;

pub struct Semantic {
    dir: PathBuf,
    /// Read from its file by the indexer's first round, off the window's thread: empty until then.
    index: Mutex<Index>,
    loaded: AtomicBool,
    embedder: Mutex<Option<Box<dyn Embed>>>,
    make: MakeEmbedder,
    progress: Arc<Progress>,
    enabled: AtomicBool,
    error: Mutex<Option<String>>,
    /// When a key was last pressed and the embedder last used, in ms since `epoch`.
    typed: AtomicU64,
    used: AtomicU64,
    epoch: Instant,
    /// Wakes the indexer early: turned on again, or quitting a wait.
    wake: (Mutex<bool>, Condvar),
    /// What `changed` was last called for (`signal`).
    signalled: Mutex<(State, bool)>,
    /// The helper is starting for a search that didn't wait for it (`rank`).
    warming: AtomicBool,
    /// How long a transcript goes unchanged before it is embedded (`QUIET`).
    quiet: Duration,
}

impl Semantic {
    /// The index kept in `dir`, with the model it was made with: the helper at `helper` running the model fetched into
    /// `dir/model`.
    pub fn open(dir: PathBuf, helper: PathBuf) -> Arc<Self> {
        let model_dir = dir.join("model");
        let make: MakeEmbedder = Box::new(move |download| {
            let (model, tokenizer) = if download {
                model::ensure(&model_dir, &model::repo_docs_models())?
            } else {
                model::present(&model_dir).ok_or("The search model isn't downloaded yet")?
            };
            Ok(Box::new(model::Helper::spawn(&helper, &model, &tokenizer)?) as Box<dyn Embed>)
        });
        Self::build(dir, model::model_id(), make, QUIET)
    }

    /// `open` with an embedder of the caller's, taking transcripts as soon as they change: for tests.
    pub fn with(dir: PathBuf, model: u64, make: MakeEmbedder) -> Arc<Self> {
        Self::build(dir, model, make, Duration::ZERO)
    }

    fn build(dir: PathBuf, model: u64, make: MakeEmbedder, quiet: Duration) -> Arc<Self> {
        let enabled = fs::read(dir.join(SETTINGS))
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .and_then(|v| v.get("enabled").and_then(serde_json::Value::as_bool))
            .unwrap_or(true);
        let progress = Arc::new(Progress::default());
        progress.set(if enabled { State::Ready } else { State::Off }, 0, 0);
        Arc::new(Self {
            dir,
            index: Mutex::new(Index::empty(Header::current(model))),
            loaded: AtomicBool::new(false),
            embedder: Mutex::new(None),
            make,
            progress,
            enabled: AtomicBool::new(enabled),
            error: Mutex::new(None),
            typed: AtomicU64::new(0),
            used: AtomicU64::new(0),
            epoch: Instant::now(),
            wake: (Mutex::new(false), Condvar::new()),
            signalled: Mutex::new((State::Ready, false)),
            warming: AtomicBool::new(false),
            quiet,
        })
    }

    pub fn progress(&self) -> Arc<Progress> {
        self.progress.clone()
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Turns it on or off, and remembers the choice.
    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
        let _ = fs::create_dir_all(&self.dir);
        let _ = fs::write(self.dir.join(SETTINGS), serde_json::json!({ "enabled": on }).to_string());
        if !on {
            self.progress.set(State::Off, 0, 0);
        }
        // The indexer ends the helper when it sees it off: here, the window's thread would wait for an embedding.
        self.nudge();
    }

    /// A key was pressed: the indexing waits until a few seconds pass without one.
    pub fn typed(&self) {
        // 0 is never.
        self.typed.store(self.now_ms().max(1), Ordering::Relaxed);
    }

    pub fn status(&self) -> Status {
        let (done, total) = self.progress.counts();
        Status {
            enabled: self.enabled(),
            state: self.progress.state(),
            done,
            total,
            sessions: self.index.lock().unwrap().sessions(),
            bytes: fs::metadata(self.dir.join(INDEX)).map(|m| m.len()).unwrap_or(0),
            error: self.error.lock().unwrap().clone(),
        }
    }

    /// Each session's best chunk for `query`, best first, with its transcript as it was indexed. None while it's off,
    /// has nothing indexed, or the model can't run: the search is then by words alone. Unless it may `wait`, a search
    /// doesn't wait for the helper to start (about a second): it starts it for the next one (`warming`).
    pub fn rank(self: &Arc<Self>, query: &str, wait: bool) -> Option<Vec<(String, Ranked)>> {
        if !self.enabled() || self.index.lock().unwrap().rows.is_empty() {
            return None;
        }
        if !wait && self.embedder.lock().unwrap().is_none() {
            self.warm();
            return None;
        }
        let (scale, q) = self.embed(query, true, false).ok()?;
        let query: Vec<f32> = q.iter().map(|&x| x as f32 * scale).collect();
        let index = self.index.lock().unwrap();
        Some(
            index
                .rank(&query)
                .into_iter()
                .map(|(session, best)| {
                    let t = &index.transcripts[best.transcript as usize];
                    (session, Ranked { best, path: t.path.clone(), len: t.len })
                })
                .collect(),
        )
    }

    /// The helper is starting for the next search.
    pub fn warming(&self) -> bool {
        self.warming.load(Ordering::Relaxed)
    }

    fn warm(self: &Arc<Self>) {
        if self.warming.swap(true, Ordering::Relaxed) {
            return;
        }
        let this = self.clone();
        let spawned = std::thread::Builder::new().name("semantic-warm".into()).spawn(move || {
            let mut embedder = this.embedder.lock().unwrap();
            if embedder.is_none() {
                *embedder = (this.make)(false).ok();
                this.used.store(this.now_ms(), Ordering::Relaxed);
            }
            drop(embedder);
            this.warming.store(false, Ordering::Relaxed);
        });
        if spawned.is_err() {
            self.warming.store(false, Ordering::Relaxed);
        }
    }

    fn now_ms(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }

    fn since(&self, at: &AtomicU64) -> Duration {
        Duration::from_millis(self.now_ms().saturating_sub(at.load(Ordering::Relaxed)))
    }

    fn embed(&self, text: &str, query: bool, download: bool) -> Result<(f32, [i8; DIM]), String> {
        let mut embedder = self.embedder.lock().unwrap();
        if embedder.is_none() {
            *embedder = Some((self.make)(download)?);
        }
        self.used.store(self.now_ms(), Ordering::Relaxed);
        let text = &text[..text.floor_char_boundary(MAX_TEXT)];
        let result = embedder.as_mut().map(|e| e.embed(text, query)).unwrap_or(Err("no embedder".into()));
        if result.is_err() {
            *embedder = None;
        }
        result
    }

    fn nudge(&self) {
        *self.wake.0.lock().unwrap() = true;
        self.wake.1.notify_all();
    }

    /// Waits up to `most`, or until nudged; ends the helper once it has been idle for long enough.
    fn rest(&self, most: Duration) {
        let until = Instant::now() + most;
        let mut woken = self.wake.0.lock().unwrap();
        while !*woken {
            let now = Instant::now();
            if now >= until {
                break;
            }
            let (guard, _) = self.wake.1.wait_timeout(woken, (until - now).min(Duration::from_secs(5))).unwrap();
            woken = guard;
            if self.since(&self.used) >= HELPER_IDLE {
                *self.embedder.lock().unwrap() = None;
            }
        }
        *woken = false;
    }

    /// Waits while a key was pressed in the last few seconds.
    fn wait_for_quiet(&self) {
        while self.enabled() && self.typed.load(Ordering::Relaxed) != 0 && self.since(&self.typed) < TYPING {
            std::thread::sleep(Duration::from_millis(250));
            if self.since(&self.used) >= HELPER_IDLE {
                *self.embedder.lock().unwrap() = None;
            }
        }
    }

    fn save(&self) {
        let bytes = self.index.lock().unwrap().encode();
        let (path, part) = (self.dir.join(INDEX), self.dir.join(format!("{INDEX}.part")));
        let _ = fs::create_dir_all(&self.dir);
        if fs::write(&part, bytes).and_then(|()| fs::rename(&part, &path)).is_err() {
            let _ = fs::remove_file(&part);
        }
    }

    /// Runs the indexer on a thread of its own, for as long as the app runs. `dirs` gives the Claude folders to read
    /// (every account's), and `changed` is called when the state changes, not at each step.
    pub fn start(self: &Arc<Self>, dirs: impl Fn() -> Vec<PathBuf> + Send + 'static, changed: impl Fn() + Send + 'static) {
        let this = self.clone();
        let spawned = std::thread::Builder::new().name("semantic".into()).spawn(move || {
            loop {
                let wait = this.round(&dirs(), &|| this.signal(&changed));
                this.signal(&changed);
                this.rest(wait);
            }
        });
        if let Err(e) = spawned {
            crate::log_line!("Couldn't start indexing past sessions: {e}");
        }
    }

    /// Calls `changed` when what the app shows changes: the state, and whether the progress shows. A catch-up of a few
    /// sessions, as every minute while agents work, counts as ready, so it draws nothing.
    fn signal(&self, changed: &dyn Fn()) {
        let shown = self.progress.shown().is_some();
        let state = match self.progress.state() {
            State::Indexing if !shown => State::Ready,
            state => state,
        };
        let mut last = self.signalled.lock().unwrap();
        if *last != (state, shown) {
            *last = (state, shown);
            drop(last);
            changed();
        }
    }

    /// One look at the transcripts in `dirs` and the embedding of what changed. How long to wait before the next.
    pub fn round(&self, dirs: &[PathBuf], changed: &dyn Fn()) -> Duration {
        if !self.loaded.swap(true, Ordering::Relaxed) {
            let header = self.index.lock().unwrap().header;
            let read = fs::read(self.dir.join(INDEX)).map(|b| Index::decode(&b, header));
            if let Ok(index) = read {
                *self.index.lock().unwrap() = index;
            }
        }
        if !self.enabled() {
            self.progress.set(State::Off, 0, 0);
            *self.embedder.lock().unwrap() = None;
            return Duration::from_secs(3600);
        }
        let disk = on_disk(dirs);
        let now = std::time::SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
        let plan = index::plan(&self.index.lock().unwrap(), &disk, now.saturating_sub(self.quiet.as_millis() as u64));
        if !plan.gone.is_empty() {
            let mut index = self.index.lock().unwrap();
            for path in &plan.gone {
                index.drop_transcript(path);
            }
            drop(index);
            self.save();
        }
        if plan.embed.is_empty() {
            self.progress.set(State::Ready, 0, 0);
            return RESCAN;
        }
        // The model first, which may download.
        if self.embedder.lock().unwrap().is_none() {
            self.progress.set(State::Downloading, 0, 0);
            changed();
            let made = (self.make)(true);
            match made {
                Ok(embedder) => *self.embedder.lock().unwrap() = Some(embedder),
                Err(e) => return self.failed(e),
            }
            self.used.store(self.now_ms(), Ordering::Relaxed);
        }
        // By session, the most recently changed first, so recent work is found soonest.
        let mut work = plan.embed;
        work.sort_by(|a, b| b.0.modified.cmp(&a.0.modified).then_with(|| a.0.path.cmp(&b.0.path)));
        let mut sessions: Vec<&str> = Vec::new();
        for (file, _) in &work {
            if !sessions.contains(&file.session.as_str()) {
                sessions.push(&file.session);
            }
        }
        let total = sessions.len() as u32;
        self.progress.set(State::Indexing, 0, total);
        changed();
        let mut saved = Instant::now();
        for (done, session) in sessions.iter().enumerate() {
            for (file, from) in work.iter().filter(|(f, _)| f.session == *session) {
                if let Err(e) = self.embed_transcript(file, *from) {
                    return self.failed(e);
                }
                if !self.enabled() {
                    return Duration::ZERO;
                }
            }
            self.progress.set(State::Indexing, done as u32 + 1, total);
            if saved.elapsed() >= SAVE_EVERY {
                self.save();
                saved = Instant::now();
            }
        }
        self.save();
        *self.error.lock().unwrap() = None;
        self.progress.set(State::Ready, total, total);
        RESCAN
    }

    fn failed(&self, error: String) -> Duration {
        crate::log_line!("Indexing past sessions: {error}");
        *self.error.lock().unwrap() = Some(error);
        self.progress.set(State::Failed, 0, 0);
        self.save();
        RETRY
    }

    /// Embeds `file`'s messages from `from` on and puts them in the index in place of what it had from there.
    fn embed_transcript(&self, file: &OnDisk, from: u32) -> Result<(), String> {
        let messages = crate::history::transcript_messages(&file.path, is_subagent(&file.path));
        let mut rows = Vec::new();
        for (i, text) in messages.iter().enumerate().skip(from as usize) {
            for (start, len) in index::chunks(text) {
                self.wait_for_quiet();
                if !self.enabled() {
                    return Ok(());
                }
                let (scale, vector) = self.embed(&text[start..start + len], false, true)?;
                rows.push(Row { transcript: 0, message: i as u32, start: start as u32, len: len as u32, scale, vector });
            }
        }
        self.index.lock().unwrap().update(file, from, messages.len() as u32, rows);
        Ok(())
    }
}

/// A session's best chunk, and its transcript as the index knew it.
#[derive(Clone, Debug, PartialEq)]
pub struct Ranked {
    pub best: Best,
    pub path: PathBuf,
    /// The transcript's size when it was indexed: one that is smaller now was rewritten, and its rows say nothing true.
    pub len: u64,
}

const INDEX: &str = "index.bin";
const SETTINGS: &str = "settings.json";

/// A subagent's transcript: `<project>/<session>/subagents/<agent>.jsonl`.
pub fn is_subagent(path: &Path) -> bool {
    path.parent().and_then(Path::file_name).is_some_and(|name| name == "subagents")
}

/// Every transcript in the Claude folders `dirs`: each session's, and its subagents' as its own.
pub fn on_disk(dirs: &[PathBuf]) -> Vec<OnDisk> {
    let mut out = Vec::new();
    let mut add = |path: PathBuf, session: String| {
        if let Ok(meta) = fs::metadata(&path) {
            let modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64);
            out.push(OnDisk { path, session, len: meta.len(), modified });
        }
    };
    for folder in dirs.iter().flat_map(|dir| fs::read_dir(dir.join("projects")).into_iter().flatten().flatten()) {
        for entry in fs::read_dir(folder.path()).into_iter().flatten().flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(id) = name.strip_suffix(".jsonl") {
                add(path, id.to_owned());
            } else if entry.file_type().is_ok_and(|t| t.is_dir()) {
                for agent in fs::read_dir(path.join("subagents")).into_iter().flatten().flatten() {
                    if agent.path().extension().is_some_and(|x| x == "jsonl") {
                        add(agent.path(), name.clone());
                    }
                }
            }
        }
    }
    out
}

/// The chunk `best` stands for in a message's `text`, cut to a snippet: None when the message no longer holds it.
pub fn passage(text: &str, best: &Best) -> Option<String> {
    let (start, end) = (best.start as usize, best.start as usize + best.len as usize);
    let chunk = text.get(start..end)?;
    let cut = chunk.floor_char_boundary(SNIPPET);
    let body = chunk[..cut].split_whitespace().collect::<Vec<_>>().join(" ");
    Some(format!("{}{body}{}", if start > 0 { "…" } else { "" }, if cut < chunk.len() || end < text.len() { "…" } else { "" }))
}

/// The characters of a chunk a snippet shows.
const SNIPPET: usize = 240;
/// Reciprocal rank fusion's constant: a rank's weight is 1 / (K + rank).
const RRF_K: f32 = 60.0;

/// Lists of ids, each best first, as one, by reciprocal rank fusion: an id high in either list, or in both, comes first.
/// Ties keep the order in which the ids first appear.
pub fn fuse<'a>(lists: &[&[&'a str]]) -> Vec<&'a str> {
    let mut scores: Vec<(&'a str, f32)> = Vec::new();
    for list in lists {
        for (rank, &id) in list.iter().enumerate() {
            let weight = 1.0 / (RRF_K + rank as f32 + 1.0);
            match scores.iter_mut().find(|(s, _)| *s == id) {
                Some((_, score)) => *score += weight,
                None => scores.push((id, weight)),
            }
        }
    }
    let mut order: Vec<(usize, &(&str, f32))> = scores.iter().enumerate().collect();
    order.sort_by(|a, b| b.1.1.total_cmp(&a.1.1).then(a.0.cmp(&b.0)));
    order.into_iter().map(|(_, (id, _))| *id).collect()
}

#[cfg(test)]
pub(crate) mod tests;
