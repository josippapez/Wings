//! Live state of opencode agents: working, waiting for you, or idle, plus the session's id and title.
//!
//! opencode runs one background service per user and registers it in `service.json` (url, pid, password).
//! That file is the whole discovery contract, so this only reads it: it never starts opencode. The service
//! pushes events on `GET /api/event` (server-sent events). One long-lived connection is open only while a
//! pane runs opencode, so an idle agent costs no wakeups. The stream is volatile by contract (events during
//! a disconnection are lost), so every (re)connect starts from a snapshot of `/api/session/active` and the
//! pending permission and form lists, and events change that picture from then on.
//!
//! Which session a pane runs is a guess: the pane's cwd and its terminal title (`OC | <session title>`,
//! see `title_matches`) against the folder's root sessions, else the one running root session in that folder.
//!
//! The service password goes only into the `Authorization` header of a request to the loopback url in
//! `service.json`. It is wrapped in `Secret`, which prints as `<redacted>`, and no error carries it.

use std::{
    collections::{HashMap, HashSet},
    fmt,
    io::{self, BufRead, BufReader},
    path::PathBuf,
    sync::{Arc, Condvar, Mutex, MutexGuard},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::Deserialize;
use serde_json::Value;

/// The TUI sets the terminal title to this plus the session title.
const TITLE_PREFIX: &str = "OC | ";
/// Longer titles are cut to 37 characters and an ellipsis (tui/src/app.tsx in opencode).
const TITLE_LIMIT: usize = 40;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(1);
const HISTORY_TIMEOUT: Duration = Duration::from_secs(20);
/// ureq's own default.
const BODY_LIMIT: u64 = 10 * 1024 * 1024;
const HISTORY_BODY_LIMIT: u64 = 512 * 1024 * 1024;
/// How many root sessions of a folder are listed to find the pane's by title.
const LIST_LIMIT: &str = "20";
const BACKOFF: [u64; 5] = [1, 2, 5, 10, 30];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Working,
    /// Running, but asking you for a permission or a form answer.
    Waiting,
    Idle,
}

/// A value that must never be printed. Not `Display`, and `Debug` hides it.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

/// Why a request or the discovery failed. Carries no url, header or body, so it is safe to print.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    NoService,
    BadService,
    NotLoopback,
    Unreachable,
    Timeout,
    Unauthorized,
    Status(u16),
    BadBody,
    /// The service that answered is not the one `service.json` names.
    Mismatch,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NoService => f.write_str("no service.json"),
            Error::BadService => f.write_str("service.json is not what was expected"),
            Error::NotLoopback => f.write_str("service url is not on this machine"),
            Error::Unreachable => f.write_str("service unreachable"),
            Error::Timeout => f.write_str("service timed out"),
            Error::Unauthorized => f.write_str("service refused the credentials"),
            Error::Status(code) => write!(f, "service answered {code}"),
            Error::BadBody => f.write_str("unexpected answer from the service"),
            Error::Mismatch => f.write_str("the service is not the registered one"),
        }
    }
}

fn from_ureq(e: ureq::Error) -> Error {
    match e {
        ureq::Error::StatusCode(401) => Error::Unauthorized,
        ureq::Error::StatusCode(code) => Error::Status(code),
        ureq::Error::Timeout(_) => Error::Timeout,
        _ => Error::Unreachable,
    }
}

// ---- discovery ------------------------------------------------------------------------------------------

/// `service.json`: the running service's address and password.
#[derive(Debug, PartialEq)]
pub struct Service {
    pub url: String,
    pub pid: u32,
    password: Option<Secret>,
}

impl Service {
    pub fn parse(text: &str) -> Result<Service, Error> {
        #[derive(Deserialize)]
        struct File {
            url: String,
            pid: u32,
            password: Option<Secret>,
        }
        // The serde message is dropped: for a wrong type it quotes the value, and that could be the password.
        let file: File = serde_json::from_str(text).map_err(|_| Error::BadService)?;
        let url = file.url.trim_end_matches('/').to_string();
        if !is_loopback(&url) {
            return Err(Error::NotLoopback);
        }
        Ok(Service { url, pid: file.pid, password: file.password })
    }
}

/// The password is only ever sent to this machine.
fn is_loopback(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://") else { return false };
    let authority = rest.split('/').next().unwrap_or_default();
    let host = if authority.starts_with('[') { authority.split(']').next().map(|h| format!("{h}]")) } else { authority.split(':').next().map(str::to_string) };
    matches!(host.as_deref(), Some("127.0.0.1" | "localhost" | "[::1]"))
}

/// Where opencode registers its service, the way its own client finds it.
pub fn service_path() -> Option<PathBuf> {
    let state = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).or_else(|| dirs::home_dir().map(|h| h.join(".local").join("state")))?;
    Some(state.join("opencode").join("service.json"))
}

fn read_service() -> Result<Service, Error> {
    let text = std::fs::read_to_string(service_path().ok_or(Error::NoService)?).map_err(|_| Error::NoService)?;
    Service::parse(&text)
}

/// A client for the service `service.json` names, for the history search. Never starts opencode. A page of
/// a long session's messages is tens of MB of tool output (78 MB for one session here) and takes the service
/// up to a second, so a request gets more time and room than the status polling's.
pub fn connect() -> Result<Client, Error> {
    read_service().map(|service| Client::with_limits(&service, HISTORY_TIMEOUT, HISTORY_BODY_LIMIT))
}

// ---- http -----------------------------------------------------------------------------------------------

pub trait Api {
    fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value, Error>;
}

#[derive(Debug)]
pub struct Client {
    agent: ureq::Agent,
    /// No overall timeout: the event stream stays open for as long as opencode runs.
    stream_agent: ureq::Agent,
    base: String,
    auth: Option<Secret>,
    /// The most bytes read from one answer.
    max_body: u64,
}

fn agent(global: Option<Duration>) -> ureq::Agent {
    ureq::Agent::config_builder()
        // ureq reads HTTP_PROXY from the environment by default; the password must not leave this machine.
        .proxy(None)
        .max_redirects(0)
        .timeout_connect(Some(REQUEST_TIMEOUT))
        .timeout_global(global)
        .build()
        .into()
}

impl Client {
    pub fn new(service: &Service) -> Client {
        Client::with_limits(service, REQUEST_TIMEOUT, BODY_LIMIT)
    }

    fn with_limits(service: &Service, timeout: Duration, max_body: u64) -> Client {
        let auth = service.password.as_ref().map(|p| Secret(format!("Basic {}", base64(format!("opencode:{}", p.0).as_bytes()))));
        Client { agent: agent(Some(timeout)), stream_agent: agent(None), base: service.url.clone(), auth, max_body }
    }

    fn request(&self, agent: &ureq::Agent, path: &str) -> ureq::RequestBuilder<ureq::typestate::WithoutBody> {
        let request = agent.get(format!("{}{path}", self.base));
        match &self.auth {
            Some(auth) => request.header("Authorization", &auth.0),
            None => request,
        }
    }

    /// The event stream, read line by line as it arrives.
    fn stream(&self) -> Result<impl BufRead, Error> {
        let response = self.request(&self.stream_agent, "/api/event").header("Accept", "text/event-stream").call().map_err(from_ureq)?;
        Ok(BufReader::new(response.into_body().into_reader()))
    }
}

impl Api for Client {
    fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value, Error> {
        let mut request = self.request(&self.agent, path);
        for (key, value) in query {
            request = request.query(*key, *value);
        }
        let text = request.call().map_err(from_ureq)?.body_mut().with_config().limit(self.max_body).read_to_string().map_err(|_| Error::BadBody)?;
        serde_json::from_str(&text).map_err(|_| Error::BadBody)
    }
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

// ---- server-sent events ---------------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
pub enum Frame {
    /// One event: its `data:` lines joined with newlines.
    Data(String),
    /// A `:` comment line. opencode sends one every 15 s to keep the connection alive.
    Comment,
}

/// Reads frames off a stream one line at a time.
pub struct Sse<R> {
    reader: R,
    data: Option<String>,
}

impl<R: BufRead> Sse<R> {
    pub fn new(reader: R) -> Self {
        Sse { reader, data: None }
    }

    /// The next frame; `None` when the stream ends.
    pub fn next_frame(&mut self) -> io::Result<Option<Frame>> {
        loop {
            let mut line = String::new();
            if self.reader.read_line(&mut line)? == 0 {
                return Ok(None);
            }
            let line = line.trim_end_matches(['\n', '\r']);
            if line.is_empty() {
                if let Some(data) = self.data.take() {
                    return Ok(Some(Frame::Data(data)));
                }
            } else if line.starts_with(':') {
                return Ok(Some(Frame::Comment));
            } else if let Some(value) = line.strip_prefix("data") {
                // `data:x`, `data: x` (one space is dropped) and a bare `data` (an empty line).
                let value = value.strip_prefix(':').map(|v| v.strip_prefix(' ').unwrap_or(v)).unwrap_or("");
                match &mut self.data {
                    Some(data) => {
                        data.push('\n');
                        data.push_str(value);
                    }
                    None => self.data = Some(value.to_string()),
                }
            }
            // `event:`, `id:` and `retry:` carry nothing this needs.
        }
    }
}

// ---- events and responses -------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Update {
    /// `session.execution.started`
    Started(String),
    /// `session.execution.succeeded`, `.failed` or `.interrupted`
    Ended(String),
    /// `permission.asked`, `form.created`
    Pending { session: String, id: String },
    /// `permission.replied`, `form.replied`, `form.cancelled`
    Settled { session: String, id: String },
    /// `session.renamed`
    Renamed { session: String, title: String },
    /// A tool call or a step of a reply started or finished: the session's activity changed.
    Activity(String),
    /// Any other event.
    Other,
    /// An event of a kind that is used, without the fields it should have: the picture can't be trusted.
    Malformed,
}

/// What one event means, and when it happened (ms since 1970).
pub fn parse_event(data: &str) -> (Update, Option<u64>) {
    #[derive(Deserialize)]
    struct Head {
        #[serde(rename = "type")]
        kind: String,
        created: Option<f64>,
    }
    let Ok(head) = serde_json::from_str::<Head>(data) else { return (Update::Malformed, None) };
    let created = head.created.filter(|c| *c >= 0.0).map(|c| c as u64);
    let used = matches!(
        head.kind.as_str(),
        "session.execution.started"
            | "session.execution.succeeded"
            | "session.execution.failed"
            | "session.execution.interrupted"
            | "permission.asked"
            | "permission.replied"
            | "form.created"
            | "form.replied"
            | "form.cancelled"
            | "session.renamed"
    );
    if matches!(head.kind.as_str(), "session.tool.called" | "session.tool.success" | "session.tool.failed" | "session.step.ended") {
        // Only a signal to look again, so one without a session is dropped rather than distrusting the picture.
        let session = serde_json::from_str::<Value>(data).ok().and_then(|e| e["data"]["sessionID"].as_str().map(str::to_string));
        return (session.map_or(Update::Other, Update::Activity), created);
    }
    if !used {
        return (Update::Other, created);
    }
    let Ok(event) = serde_json::from_str::<Value>(data) else { return (Update::Malformed, created) };
    let text = |v: &Value| v.as_str().map(str::to_string);
    let body = &event["data"];
    let update = (|| {
        Some(match head.kind.as_str() {
            "session.execution.started" => Update::Started(text(&body["sessionID"])?),
            "permission.asked" => Update::Pending { session: text(&body["sessionID"])?, id: text(&body["id"])? },
            "permission.replied" => Update::Settled { session: text(&body["sessionID"])?, id: text(&body["requestID"])? },
            "form.created" => Update::Pending { session: text(&body["form"]["sessionID"])?, id: text(&body["form"]["id"])? },
            "form.replied" | "form.cancelled" => Update::Settled { session: text(&body["sessionID"])?, id: text(&body["id"])? },
            "session.renamed" => Update::Renamed { session: text(&body["sessionID"])?, title: text(&body["title"])? },
            _ => Update::Ended(text(&body["sessionID"])?),
        })
    })();
    (update.unwrap_or(Update::Malformed), created)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub id: String,
    pub parent: Option<String>,
    pub title: Option<String>,
    pub directory: Option<String>,
}

/// `title` unless it is the placeholder opencode gives a session before it has one, "New session - <ISO time>".
/// Matching a pane by its title still uses the placeholder, since the TUI shows it.
pub fn real_title(title: Option<String>) -> Option<String> {
    title.filter(|t| t.strip_prefix("New session - ").is_none_or(|rest| !rest.starts_with(|c: char| c.is_ascii_digit())))
}

fn session_info(v: &Value) -> Option<SessionInfo> {
    Some(SessionInfo {
        id: v["id"].as_str()?.to_string(),
        parent: v["parentID"].as_str().map(str::to_string),
        title: v["title"].as_str().map(str::to_string),
        directory: v["location"]["directory"].as_str().map(str::to_string),
    })
}

/// `GET /api/session/:id` -> `{"data": {...}}`
pub fn parse_session(v: &Value) -> Option<SessionInfo> {
    session_info(&v["data"])
}

/// `GET /api/session?...` -> `{"data": [...]}`
pub fn parse_sessions(v: &Value) -> Option<Vec<SessionInfo>> {
    v["data"].as_array()?.iter().map(session_info).collect()
}

/// `GET /api/session/active` -> `{"data": {"<id>": {"type": "running"}}}`; a session that is absent is idle.
pub fn parse_active(v: &Value) -> Option<HashSet<String>> {
    Some(v["data"].as_object()?.iter().filter(|(_, s)| s["type"] == "running").map(|(id, _)| id.clone()).collect())
}

/// `GET /api/session/:id/permission` and `/form` -> `{"data": [{"id": ...}]}`
pub fn parse_pending(v: &Value) -> Option<HashSet<String>> {
    v["data"].as_array()?.iter().map(|r| r["id"].as_str().map(str::to_string)).collect()
}

/// `GET /api/info` -> the pid of the process that answers.
pub fn parse_info_pid(v: &Value) -> Option<u32> {
    u32::try_from(v["pid"].as_u64()?).ok()
}

/// Does the pane's terminal title name this session title? The TUI cuts a title over 40 characters to 37 and "…".
pub fn title_matches(pane_title: &str, session_title: &str) -> bool {
    let Some(shown) = pane_title.strip_prefix(TITLE_PREFIX) else { return false };
    if session_title.chars().count() > TITLE_LIMIT {
        shown == format!("{}…", session_title.chars().take(TITLE_LIMIT - 3).collect::<String>())
    } else {
        shown == session_title
    }
}

// ---- what is known --------------------------------------------------------------------------------------

/// One pane that runs opencode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneRef {
    pub pane_id: String,
    pub pid: u32,
    pub cwd: Option<String>,
    pub title: String,
}

/// What the session list of a folder was read for.
type ListKey = (Option<String>, String);

fn list_key(pane: &PaneRef) -> ListKey {
    (pane.cwd.clone(), pane.title.clone())
}

/// Only a pane with a session title can be matched by it, and only with a folder to look in.
fn needs_list(pane: &PaneRef) -> bool {
    pane.cwd.is_some() && pane.title.starts_with(TITLE_PREFIX)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneView {
    pub session_id: String,
    pub title: Option<String>,
    pub state: State,
    /// When Wings first saw this state, in ms since 1970.
    pub since: u64,
}

#[derive(Debug, Default)]
struct Model {
    running: HashSet<String>,
    /// Permission and form ids waiting for an answer, by session.
    pending: HashMap<String, HashSet<String>>,
    infos: HashMap<String, SessionInfo>,
    lists: HashMap<ListKey, Vec<SessionInfo>>,
    /// The state each root session was last seen in, and when.
    states: HashMap<String, (State, u64)>,
}

fn same_dir(a: &str, b: &str) -> bool {
    a.trim_end_matches('/') == b.trim_end_matches('/')
}

impl Model {
    /// The top of a session's parent chain; `None` while something on the way is not known yet.
    fn root_of(&self, id: &str) -> Option<String> {
        let mut current = id;
        for _ in 0..16 {
            match &self.infos.get(current)?.parent {
                Some(parent) => current = parent,
                None => return Some(current.to_string()),
            }
        }
        None
    }

    fn apply(&mut self, update: Update) {
        match update {
            Update::Started(session) => {
                self.running.insert(session);
            }
            Update::Ended(session) => {
                self.running.remove(&session);
                self.pending.remove(&session);
            }
            Update::Pending { session, id } => {
                self.pending.entry(session).or_default().insert(id);
            }
            Update::Settled { session, id } => {
                if let Some(ids) = self.pending.get_mut(&session) {
                    ids.remove(&id);
                    if ids.is_empty() {
                        self.pending.remove(&session);
                    }
                }
            }
            Update::Renamed { session, title } => {
                if let Some(info) = self.infos.get_mut(&session) {
                    info.title = Some(title);
                }
            }
            Update::Activity(_) | Update::Other | Update::Malformed => {}
        }
    }

    /// Sessions that run or wait, and the parents above them, that nothing is known about yet.
    fn missing_infos(&self) -> Vec<String> {
        let mut missing = HashSet::new();
        for id in self.running.iter().chain(self.pending.keys()) {
            let mut current = id.as_str();
            for _ in 0..16 {
                match self.infos.get(current) {
                    None => {
                        missing.insert(current.to_string());
                        break;
                    }
                    Some(info) => match &info.parent {
                        Some(parent) => current = parent,
                        None => break,
                    },
                }
            }
        }
        missing.into_iter().collect()
    }

    /// The root session each pane runs. A title match against the folder's sessions wins. Failing that, a pane
    /// with nothing matched takes the one root session running in its folder that no pane has matched, as long
    /// as it is the only such pane there. Anything else is ambiguous, and the pane has no session.
    fn assign(&self, panes: &[PaneRef]) -> HashMap<String, String> {
        let mut out: HashMap<String, String> = HashMap::new();
        for pane in panes {
            let Some(list) = self.lists.get(&list_key(pane)) else { continue };
            let mut named = list.iter().filter(|s| s.title.as_deref().is_some_and(|t| title_matches(&pane.title, t)));
            if let (Some(only), None) = (named.next(), named.next()) {
                out.insert(pane.pane_id.clone(), only.id.clone());
            }
        }
        let claimed: HashSet<&String> = out.values().collect();
        let running_roots: Vec<(String, &SessionInfo)> = self
            .running
            .iter()
            .filter_map(|id| self.root_of(id))
            .collect::<HashSet<_>>()
            .into_iter()
            .filter_map(|root| self.infos.get(&root).map(|info| (root, info)))
            .filter(|(root, _)| !claimed.contains(root))
            .collect();
        let mut fallback = Vec::new();
        for pane in panes.iter().filter(|p| !out.contains_key(&p.pane_id)) {
            let Some(cwd) = pane.cwd.as_deref() else { continue };
            let alone = panes.iter().filter(|p| !out.contains_key(&p.pane_id) && p.cwd.as_deref().is_some_and(|c| same_dir(c, cwd))).count() == 1;
            let mut here = running_roots.iter().filter(|(_, info)| info.directory.as_deref().is_some_and(|d| same_dir(d, cwd)));
            if let (true, Some((root, _)), None) = (alone, here.next(), here.next()) {
                fallback.push((pane.pane_id.clone(), root.clone()));
            }
        }
        out.extend(fallback);
        out
    }

    fn state_of(&self, root: &str) -> State {
        let mut state = State::Idle;
        for id in self.running.iter().filter(|id| self.root_of(id).as_deref() == Some(root)) {
            if self.pending.get(id).is_some_and(|p| !p.is_empty()) {
                return State::Waiting;
            }
            state = State::Working;
        }
        state
    }

    fn views(&mut self, panes: &[PaneRef], at: u64) -> HashMap<String, PaneView> {
        let assigned = self.assign(panes);
        let mut views = HashMap::new();
        let mut seen = HashSet::new();
        for (pane_id, root) in assigned {
            let state = self.state_of(&root);
            let entry = self.states.entry(root.clone()).or_insert((state, at));
            if entry.0 != state {
                *entry = (state, at);
            }
            let title = self.infos.get(&root).and_then(|i| i.title.clone()).or_else(|| self.lists.values().flatten().find(|s| s.id == root).and_then(|s| s.title.clone()));
            views.insert(pane_id, PaneView { session_id: root.clone(), title, state, since: entry.1 });
            seen.insert(root);
        }
        self.states.retain(|root, _| seen.contains(root));
        views
    }
}

// ---- shared with the detector and the threads -----------------------------------------------------------

/// Whether what the views say is current.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Link {
    /// Not connected yet, and nothing is known.
    #[default]
    Connecting,
    /// Connected, and the views follow the stream.
    Up,
    /// The stream dropped and is being reopened. The views are the last known ones, and stay until the
    /// snapshot of the new connection says what became of them.
    Reconnecting,
    /// The service stayed away through every backoff step. Nothing is known, and no state is claimed.
    Gone,
}

#[derive(Default)]
struct Inner {
    link: Link,
    panes: Vec<PaneRef>,
    model: Model,
    view: HashMap<String, PaneView>,
    client: Option<Arc<Client>>,
    /// Looked up once; asked again only after an event names the session, or a reconnect.
    tried_infos: HashSet<String>,
    tried_lists: HashSet<ListKey>,
}

/// What the resolver thread has to ask for.
#[derive(Debug, Default, PartialEq, Eq)]
struct Work {
    infos: Vec<String>,
    lists: Vec<ListKey>,
}

#[derive(Default)]
struct Resolved {
    infos: Vec<SessionInfo>,
    /// Sessions the service no longer knows: noted, so they are not asked for again.
    gone: Vec<String>,
    lists: Vec<(ListKey, Vec<SessionInfo>)>,
}

impl Inner {
    fn refresh(&mut self, at: u64) {
        self.view = self.model.views(&self.panes, at);
    }

    fn work(&self) -> Work {
        let infos = self.model.missing_infos().into_iter().filter(|id| !self.tried_infos.contains(id)).collect();
        let lists = self
            .panes
            .iter()
            .filter(|p| needs_list(p))
            .map(list_key)
            .filter(|k| !self.model.lists.contains_key(k) && !self.tried_lists.contains(k))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        Work { infos, lists }
    }
}

type ActivityHook = Box<dyn Fn(&str) + Send + Sync>;

#[derive(Default)]
pub struct Shared {
    inner: Mutex<Inner>,
    wake: Condvar,
    /// Told the root session whenever it starts, ends or does a tool call, for plugins that show its activity.
    on_activity: Mutex<Option<ActivityHook>>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The panes that run opencode, as the detector sees them now. Wakes the threads only when that changed.
    pub fn set_panes(&self, panes: Vec<PaneRef>) {
        let mut inner = self.lock();
        if inner.panes == panes {
            return;
        }
        if panes.is_empty() {
            Self::forget(&mut inner, Link::Connecting);
        }
        let keys: HashSet<ListKey> = panes.iter().map(list_key).collect();
        inner.model.lists.retain(|k, _| keys.contains(k));
        inner.tried_lists.retain(|k| keys.contains(k));
        inner.panes = panes;
        inner.refresh(now_ms());
        self.wake.notify_all();
    }

    /// Stands in for what the threads would work out, for tests of the detector.
    #[cfg(test)]
    pub(crate) fn seed(&self, pane_id: &str, view: PaneView) {
        self.lock().view.insert(pane_id.to_string(), view);
    }

    pub fn link(&self) -> Link {
        self.lock().link
    }

    pub fn view(&self, pane_id: &str) -> Option<PaneView> {
        self.lock().view.get(pane_id).cloned()
    }

    pub fn on_activity(&self, f: impl Fn(&str) + Send + Sync + 'static) {
        *self.on_activity.lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(f));
    }

    fn activity(&self, session: &str) {
        let root = self.lock().model.root_of(session).unwrap_or_else(|| session.to_string());
        if let Some(f) = &*self.on_activity.lock().unwrap_or_else(|e| e.into_inner()) {
            f(&root);
        }
    }

    fn apply(&self, update: Update, at: u64) {
        let signal = match &update {
            Update::Started(s) | Update::Ended(s) => Some(s.clone()),
            _ => None,
        };
        let mut inner = self.lock();
        let touched = match &update {
            Update::Started(s) | Update::Ended(s) | Update::Pending { session: s, .. } | Update::Settled { session: s, .. } | Update::Renamed { session: s, .. } => Some(s.clone()),
            _ => None,
        };
        if let Some(session) = touched {
            // A session that fell out of reach earlier gets another try now that it did something.
            inner.tried_infos.remove(&session);
        }
        inner.model.apply(update);
        inner.refresh(at);
        drop(inner);
        self.wake.notify_all();
        if let Some(session) = signal {
            self.activity(&session);
        }
    }

    /// Replaces what is known about running and pending sessions with a fresh snapshot.
    fn install(&self, snapshot: Snapshot, at: u64) {
        let mut inner = self.lock();
        inner.model.running = snapshot.running;
        inner.model.pending = snapshot.pending;
        for info in snapshot.infos {
            inner.model.infos.insert(info.id.clone(), info);
        }
        inner.tried_infos.clear();
        inner.link = Link::Up;
        inner.refresh(at);
        self.wake.notify_all();
    }

    fn connected(&self, client: Arc<Client>) {
        self.lock().client = Some(client);
        self.wake.notify_all();
    }

    /// The stream dropped. What was last known is kept: a restart of the service is not the end of a turn,
    /// and the snapshot of the next connection settles it.
    pub(crate) fn disconnected(&self) {
        let mut inner = self.lock();
        inner.client = None;
        inner.link = Link::Reconnecting;
        inner.tried_infos.clear();
        inner.tried_lists.clear();
    }

    /// The service is not coming back: nothing is known, so every pane reads as idle.
    pub(crate) fn gone(&self) {
        Self::forget(&mut self.lock(), Link::Gone);
    }

    fn forget(inner: &mut Inner, link: Link) {
        inner.client = None;
        inner.model = Model::default();
        inner.view.clear();
        inner.tried_infos.clear();
        inner.tried_lists.clear();
        inner.link = link;
    }

    fn has_panes(&self) -> bool {
        !self.lock().panes.is_empty()
    }

    fn wait_for_panes(&self) {
        let guard = self.lock();
        drop(self.wake.wait_while(guard, |i| i.panes.is_empty()).unwrap_or_else(|e| e.into_inner()));
    }

    /// Waits, but gives up the moment no pane runs opencode.
    fn pause(&self, time: Duration) {
        let guard = self.lock();
        drop(self.wake.wait_timeout_while(guard, time, |i| !i.panes.is_empty()).unwrap_or_else(|e| e.into_inner()));
    }

    fn wait_work(&self) -> (Arc<Client>, Work) {
        let guard = self.lock();
        let mut inner = self
            .wake
            .wait_while(guard, |i| i.client.is_none() || i.work() == Work::default())
            .unwrap_or_else(|e| e.into_inner());
        let work = inner.work();
        inner.tried_infos.extend(work.infos.iter().cloned());
        inner.tried_lists.extend(work.lists.iter().cloned());
        (inner.client.clone().expect("waited for a client"), work)
    }

    fn resolved(&self, resolved: Resolved, at: u64) {
        let mut inner = self.lock();
        for info in resolved.infos {
            inner.model.infos.insert(info.id.clone(), info);
        }
        for id in resolved.gone {
            // A tombstone that is its own root, so the session counts without a folder or a title.
            inner.model.infos.entry(id.clone()).or_insert(SessionInfo { id, parent: None, title: None, directory: None });
        }
        for (key, list) in resolved.lists {
            inner.model.lists.insert(key, list);
        }
        inner.refresh(at);
        self.wake.notify_all();
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

// ---- talking to the service ----------------------------------------------------------------------------

struct Snapshot {
    running: HashSet<String>,
    pending: HashMap<String, HashSet<String>>,
    infos: Vec<SessionInfo>,
}

fn session_path(id: &str, tail: &str) -> String {
    format!("/api/session/{id}{tail}")
}

/// A session id goes into a url path, so anything but the characters opencode ids are made of is refused.
pub(crate) fn plausible_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn snapshot(api: &dyn Api) -> Result<Snapshot, Error> {
    let running = parse_active(&api.get("/api/session/active", &[])?).ok_or(Error::BadBody)?;
    let mut pending: HashMap<String, HashSet<String>> = HashMap::new();
    let mut infos = Vec::new();
    for id in running.iter().filter(|id| plausible_id(id)) {
        infos.push(parse_session(&api.get(&session_path(id, ""), &[])?).ok_or(Error::BadBody)?);
        for tail in ["/permission", "/form"] {
            let ids = parse_pending(&api.get(&session_path(id, tail), &[])?).ok_or(Error::BadBody)?;
            if !ids.is_empty() {
                pending.entry(id.clone()).or_default().extend(ids);
            }
        }
    }
    Ok(Snapshot { running, pending, infos })
}

fn resolve(api: &dyn Api, work: Work) -> Resolved {
    let mut out = Resolved::default();
    for id in work.infos.iter().filter(|id| plausible_id(id)) {
        match api.get(&session_path(id, ""), &[]) {
            Ok(v) => out.infos.extend(parse_session(&v)),
            Err(Error::Status(404)) => out.gone.push(id.clone()),
            Err(_) => {}
        }
    }
    for key in work.lists {
        let (Some(cwd), _) = (&key.0, &key.1) else { continue };
        let query = [("directory", cwd.as_str()), ("parentID", "null"), ("limit", LIST_LIMIT)];
        if let Some(list) = api.get("/api/session", &query).ok().as_ref().and_then(parse_sessions) {
            out.lists.push((key, list));
        }
    }
    out
}

/// One connection: finds the service, checks it is the registered one, takes a snapshot, then follows events.
/// Returns whether the stream was ever up.
fn connection(shared: &Shared) -> Result<bool, (bool, Error)> {
    let fail = |connected| move |e| (connected, e);
    let service = read_service().map_err(fail(false))?;
    let client = Client::new(&service);
    // The same check opencode's own client makes before trusting `service.json`.
    let info = client.get("/api/info", &[]).map_err(fail(false))?;
    if parse_info_pid(&info) != Some(service.pid) {
        return Err((false, Error::Mismatch));
    }
    let mut sse = Sse::new(client.stream().map_err(fail(false))?);
    let client = Arc::new(client);
    // The stream is open before the snapshot, so nothing that happens in between is lost.
    shared.install(snapshot(&*client).map_err(fail(true))?, now_ms());
    shared.connected(client.clone());
    while shared.has_panes() {
        match sse.next_frame() {
            Ok(Some(Frame::Data(data))) => match parse_event(&data) {
                (Update::Malformed, _) => shared.install(snapshot(&*client).map_err(fail(true))?, now_ms()),
                (Update::Other, _) => {}
                (Update::Activity(session), _) => shared.activity(&session),
                (update, at) => shared.apply(update, at.unwrap_or_else(now_ms)),
            },
            Ok(Some(Frame::Comment)) => {}
            Ok(None) => return Ok(true),
            Err(_) => return Err((true, Error::Unreachable)),
        }
    }
    Ok(true)
}

/// Failed attempts in a row after which the service counts as gone: every backoff step has been tried.
fn gone_after(failures: usize) -> bool {
    failures >= BACKOFF.len()
}

fn backoff(attempt: usize) -> Duration {
    Duration::from_secs(BACKOFF[attempt.min(BACKOFF.len() - 1)])
}

/// Starts the two threads: one holds the event stream while a pane runs opencode, the other looks sessions up.
pub fn spawn(shared: Arc<Shared>) {
    let stream = shared.clone();
    thread::spawn(move || {
        let mut attempt: usize = 0;
        loop {
            stream.wait_for_panes();
            let up = match connection(&stream) {
                Ok(up) => up,
                Err((up, e)) => {
                    if cfg!(debug_assertions) {
                        eprintln!("[opencode] {e}");
                    }
                    up
                }
            };
            attempt = if up { 0 } else { attempt + 1 };
            if !stream.has_panes() {
                continue;
            }
            if gone_after(attempt) {
                stream.gone();
            } else {
                stream.disconnected();
            }
            stream.pause(backoff(attempt.saturating_sub(1)));
        }
    });
    thread::spawn(move || loop {
        let (client, work) = shared.wait_work();
        let resolved = resolve(&*client, work);
        shared.resolved(resolved, now_ms());
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        io::{Cursor, Read, Write},
        net::{TcpListener, TcpStream},
    };

    const SERVICE: &str = r#"{"id":"253f","version":"2.0.27","url":"http://127.0.0.1:49374","pid":31204,"password":"hunter2-secret"}"#;

    #[test]
    fn a_placeholder_title_is_no_title() {
        assert_eq!(real_title(Some("New session - 2026-09-15T10:19:53.552Z".into())), None);
        assert_eq!(real_title(Some("New session - fix the login".into())).as_deref(), Some("New session - fix the login"));
        assert_eq!(real_title(Some("Wire up the MCP config".into())).as_deref(), Some("Wire up the MCP config"));
        assert_eq!(real_title(None), None);
    }

    fn session(id: &str, parent: Option<&str>, title: &str, dir: &str) -> SessionInfo {
        SessionInfo { id: id.into(), parent: parent.map(Into::into), title: Some(title.into()), directory: Some(dir.into()) }
    }

    fn pane(id: &str, cwd: &str, title: &str) -> PaneRef {
        PaneRef { pane_id: id.into(), pid: 1, cwd: Some(cwd.into()), title: title.into() }
    }

    // ---- fixtures

    #[test]
    fn parses_service_json() {
        let s = Service::parse(SERVICE).unwrap();
        assert_eq!((s.url.as_str(), s.pid), ("http://127.0.0.1:49374", 31204));
        assert!(s.password.is_some());
        // Only url and pid are required.
        assert!(Service::parse(r#"{"url":"http://localhost:1/","pid":5}"#).unwrap().password.is_none());
        assert_eq!(Service::parse("nope"), Err(Error::BadService));
        assert_eq!(Service::parse(r#"{"url":"http://127.0.0.1:1"}"#), Err(Error::BadService));
    }

    #[test]
    fn the_password_goes_only_to_this_machine() {
        for url in ["http://example.com:80", "https://127.0.0.1:1", "http://127.0.0.1.evil.com:1", "ftp://127.0.0.1", "http://[::2]:1"] {
            let text = format!(r#"{{"url":"{url}","pid":1,"password":"p"}}"#);
            assert_eq!(Service::parse(&text), Err(Error::NotLoopback), "{url}");
        }
        for url in ["http://127.0.0.1:1", "http://localhost:9", "http://[::1]:9"] {
            assert!(Service::parse(&format!(r#"{{"url":"{url}","pid":1}}"#)).is_ok(), "{url}");
        }
    }

    #[test]
    fn nothing_prints_the_password() {
        let service = Service::parse(SERVICE).unwrap();
        let client = Client::new(&service);
        let shown = format!("{service:?} {client:?} {:?}", service.password);
        assert!(!shown.contains("hunter2"), "{shown}");
        assert!(!shown.contains(&base64(b"opencode:hunter2-secret")), "{shown}");
        assert!(shown.contains("<redacted>"));
        // A wrong type makes serde quote the value; the error must not.
        let bad = Service::parse(r#"{"url":"http://127.0.0.1:1","pid":1,"password":12345678}"#).unwrap_err();
        assert!(!format!("{bad:?} {bad}").contains("12345678"));
        for e in [Error::Unauthorized, Error::Status(500), Error::Timeout, Error::Mismatch] {
            assert!(!format!("{e:?} {e}").contains("hunter2"));
        }
    }

    #[test]
    fn encodes_base64() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"opencode:secret"), "b3BlbmNvZGU6c2VjcmV0");
    }

    #[test]
    fn parses_responses() {
        assert_eq!(parse_info_pid(&json!({"version":"2.0.27","pid":31204,"urls":["http://127.0.0.1:49374"]})), Some(31204));
        assert_eq!(parse_info_pid(&json!({"version":"x"})), None);
        let active = parse_active(&json!({"data":{"ses_a":{"type":"running"},"ses_b":{"type":"running"}}})).unwrap();
        assert_eq!(active, HashSet::from(["ses_a".to_string(), "ses_b".to_string()]));
        assert_eq!(parse_active(&json!({"data":{}})), Some(HashSet::new()));
        assert_eq!(parse_active(&json!({"error":1})), None);
        let one = json!({"data":{"id":"ses_a","parentID":"ses_r","title":"Fix it","location":{"directory":"/p"},"time":{"created":1,"updated":2}}});
        assert_eq!(parse_session(&one), Some(session("ses_a", Some("ses_r"), "Fix it", "/p")));
        let root = json!({"data":{"id":"ses_r","location":{"directory":"/p"}}});
        assert_eq!(parse_session(&root), Some(SessionInfo { id: "ses_r".into(), parent: None, title: None, directory: Some("/p".into()) }));
        let list = json!({"data":[{"id":"ses_a","title":"A","location":{"directory":"/p"}},{"id":"ses_b","title":"B","location":{"directory":"/p"}}]});
        assert_eq!(parse_sessions(&list).unwrap().len(), 2);
        assert_eq!(parse_sessions(&json!({"data":[{"title":"no id"}]})), None);
        assert_eq!(parse_pending(&json!({"data":[]})), Some(HashSet::new()));
        assert_eq!(parse_pending(&json!({"data":[{"id":"per_1"},{"id":"per_2"}]})).unwrap().len(), 2);
    }

    #[test]
    fn matches_the_terminal_title_a_session_gets() {
        assert!(title_matches("OC | Fix the login bug", "Fix the login bug"));
        assert!(!title_matches("OC | Fix the login bug", "Fix the login"));
        assert!(!title_matches("OpenCode", "OpenCode"));
        assert!(!title_matches("zsh", "zsh"));
        let long = "Refactor the whole authentication module and its tests";
        let shown = format!("OC | {}…", long.chars().take(37).collect::<String>());
        assert!(title_matches(&shown, long));
        // Exactly 40 characters is shown whole.
        let forty = "a".repeat(40);
        assert!(title_matches(&format!("OC | {forty}"), &forty));
        assert!(!title_matches(&format!("OC | {}…", "a".repeat(37)), &forty));
        // Characters, not bytes.
        let wide = "é".repeat(45);
        assert!(title_matches(&format!("OC | {}…", "é".repeat(37)), &wide));
    }

    // ---- sse

    fn frames(text: &str) -> Vec<Frame> {
        let mut sse = Sse::new(Cursor::new(text.as_bytes().to_vec()));
        let mut out = Vec::new();
        while let Some(frame) = sse.next_frame().unwrap() {
            out.push(frame);
        }
        out
    }

    #[test]
    fn reads_sse_frames() {
        let text = ": heartbeat\n\ndata: {\"a\":1}\n\ndata:{\"b\":2}\r\n\r\ndata: one\ndata: two\n\nevent: x\nid: 7\ndata: kept\n\n";
        assert_eq!(
            frames(text),
            [
                Frame::Comment,
                Frame::Data("{\"a\":1}".into()),
                Frame::Data("{\"b\":2}".into()),
                Frame::Data("one\ntwo".into()),
                Frame::Data("kept".into()),
            ]
        );
        // An event cut off before its blank line is never delivered.
        assert_eq!(frames("data: {\"a\":1}\n"), []);
        // Blank lines with nothing before them are nothing.
        assert_eq!(frames("\n\n\n"), []);
    }

    // ---- events

    fn event(kind: &str, data: Value) -> String {
        json!({"id":"evt_1","created":1791810000123u64,"type":kind,"data":data}).to_string()
    }

    #[test]
    fn reads_the_events_that_matter() {
        let at = Some(1791810000123);
        assert_eq!(parse_event(&event("session.execution.started", json!({"sessionID":"ses_a"}))), (Update::Started("ses_a".into()), at));
        for kind in ["session.execution.succeeded", "session.execution.failed", "session.execution.interrupted"] {
            assert_eq!(parse_event(&event(kind, json!({"sessionID":"ses_a"}))).0, Update::Ended("ses_a".into()), "{kind}");
        }
        let asked = event("permission.asked", json!({"id":"per_1","sessionID":"ses_a","action":"bash","resources":["ls"]}));
        assert_eq!(parse_event(&asked).0, Update::Pending { session: "ses_a".into(), id: "per_1".into() });
        let replied = event("permission.replied", json!({"sessionID":"ses_a","requestID":"per_1","reply":"once"}));
        assert_eq!(parse_event(&replied).0, Update::Settled { session: "ses_a".into(), id: "per_1".into() });
        let form = event("form.created", json!({"form":{"id":"frm_1","sessionID":"ses_a","title":"Q","fields":[]}}));
        assert_eq!(parse_event(&form).0, Update::Pending { session: "ses_a".into(), id: "frm_1".into() });
        for kind in ["form.replied", "form.cancelled"] {
            assert_eq!(parse_event(&event(kind, json!({"id":"frm_1","sessionID":"ses_a"}))).0, Update::Settled { session: "ses_a".into(), id: "frm_1".into() });
        }
        let renamed = event("session.renamed", json!({"sessionID":"ses_a","title":"New"}));
        assert_eq!(parse_event(&renamed).0, Update::Renamed { session: "ses_a".into(), title: "New".into() });
        assert_eq!(parse_event(&event("session.text.delta", json!({"sessionID":"ses_a","delta":"hi"}))).0, Update::Other);
        assert_eq!(parse_event(r#"{"id":"e","type":"server.connected","data":{}}"#), (Update::Other, None));
    }

    #[test]
    fn tool_events_signal_activity_and_never_distrust_the_picture() {
        for kind in ["session.tool.called", "session.tool.success", "session.tool.failed", "session.step.ended"] {
            assert_eq!(parse_event(&event(kind, json!({"sessionID":"ses_a","callID":"c"}))).0, Update::Activity("ses_a".into()), "{kind}");
            assert_eq!(parse_event(&event(kind, json!({}))).0, Update::Other, "{kind} without a session is dropped, not Malformed");
        }
    }

    #[test]
    fn activity_reaches_the_hook_with_the_root_session() {
        let shared = Shared::default();
        shared.lock().model.infos.insert("ses_child".into(), SessionInfo { id: "ses_child".into(), parent: Some("ses_root".into()), title: None, directory: None });
        shared.lock().model.infos.insert("ses_root".into(), SessionInfo { id: "ses_root".into(), parent: None, title: None, directory: None });
        let heard = Arc::new(Mutex::new(Vec::new()));
        let sink = heard.clone();
        shared.on_activity(move |s| sink.lock().unwrap().push(s.to_string()));
        shared.activity("ses_child");
        shared.apply(Update::Ended("ses_root".into()), 1);
        assert_eq!(*heard.lock().unwrap(), ["ses_root", "ses_root"]);
    }

    #[test]
    fn a_used_event_without_its_fields_is_malformed() {
        assert_eq!(parse_event(&event("session.execution.started", json!({}))).0, Update::Malformed);
        assert_eq!(parse_event(&event("permission.asked", json!({"sessionID":"ses_a"}))).0, Update::Malformed);
        assert_eq!(parse_event(&event("form.created", json!({"form":{}}))).0, Update::Malformed);
        assert_eq!(parse_event("not json").0, Update::Malformed);
        assert_eq!(parse_event(r#"{"data":{}}"#).0, Update::Malformed);
    }

    // ---- state

    fn model(infos: Vec<SessionInfo>) -> Model {
        Model { infos: infos.into_iter().map(|i| (i.id.clone(), i)).collect(), ..Default::default() }
    }

    #[test]
    fn running_waiting_and_idle() {
        let mut m = model(vec![session("ses_a", None, "A", "/p")]);
        m.lists.insert((Some("/p".into()), "OC | A".into()), vec![session("ses_a", None, "A", "/p")]);
        let panes = [pane("p1", "/p", "OC | A")];
        let state = |m: &mut Model, at| m.views(&panes, at)["p1"].clone();
        assert_eq!(state(&mut m, 10).state, State::Idle, "absent from the active list is idle");
        m.apply(Update::Started("ses_a".into()));
        assert_eq!(state(&mut m, 20).state, State::Working);
        m.apply(Update::Pending { session: "ses_a".into(), id: "per_1".into() });
        let v = state(&mut m, 30);
        assert_eq!((v.state, v.since, v.session_id.as_str(), v.title.as_deref()), (State::Waiting, 30, "ses_a", Some("A")));
        m.apply(Update::Pending { session: "ses_a".into(), id: "frm_1".into() });
        m.apply(Update::Settled { session: "ses_a".into(), id: "per_1".into() });
        assert_eq!(state(&mut m, 40).state, State::Waiting, "a form is still open");
        m.apply(Update::Settled { session: "ses_a".into(), id: "frm_1".into() });
        let v = state(&mut m, 50);
        assert_eq!((v.state, v.since), (State::Working, 50));
        assert_eq!(state(&mut m, 60).since, 50, "since stays while the state does");
        m.apply(Update::Ended("ses_a".into()));
        assert_eq!(state(&mut m, 70).state, State::Idle);
    }

    #[test]
    fn a_waiting_session_that_ends_stops_waiting() {
        let mut m = model(vec![session("ses_a", None, "A", "/p")]);
        m.apply(Update::Started("ses_a".into()));
        m.apply(Update::Pending { session: "ses_a".into(), id: "per_1".into() });
        m.apply(Update::Ended("ses_a".into()));
        m.apply(Update::Started("ses_a".into()));
        assert_eq!(m.state_of("ses_a"), State::Working);
    }

    #[test]
    fn a_child_running_or_asking_counts_for_its_root() {
        let mut m = model(vec![session("ses_r", None, "R", "/p"), session("ses_c", Some("ses_r"), "child", "/p"), session("ses_o", None, "Other", "/p")]);
        m.apply(Update::Started("ses_c".into()));
        assert_eq!((m.state_of("ses_r"), m.state_of("ses_o")), (State::Working, State::Idle));
        m.apply(Update::Pending { session: "ses_c".into(), id: "per_1".into() });
        assert_eq!(m.state_of("ses_r"), State::Waiting);
    }

    #[test]
    fn missing_parents_are_asked_for() {
        let mut m = model(vec![session("ses_c", Some("ses_r"), "child", "/p")]);
        m.apply(Update::Started("ses_c".into()));
        m.apply(Update::Started("ses_new".into()));
        let mut missing = m.missing_infos();
        missing.sort();
        assert_eq!(missing, ["ses_new", "ses_r"]);
    }

    // ---- which session a pane runs

    #[test]
    fn a_title_picks_the_session() {
        let mut m = model(vec![]);
        let list = vec![session("ses_a", None, "Alpha", "/p"), session("ses_b", None, "Beta", "/p")];
        m.lists.insert((Some("/p".into()), "OC | Beta".into()), list);
        assert_eq!(m.assign(&[pane("p1", "/p", "OC | Beta")])["p1"], "ses_b");
    }

    #[test]
    fn the_same_title_twice_is_ambiguous() {
        let mut m = model(vec![session("ses_a", None, "Same", "/p")]);
        m.lists.insert((Some("/p".into()), "OC | Same".into()), vec![session("ses_a", None, "Same", "/p"), session("ses_b", None, "Same", "/p")]);
        assert!(m.assign(&[pane("p1", "/p", "OC | Same")]).is_empty());
        // Two sessions running there do not settle it either.
        m.infos.insert("ses_b".into(), session("ses_b", None, "Same", "/p"));
        m.running = HashSet::from(["ses_a".to_string(), "ses_b".to_string()]);
        assert!(m.assign(&[pane("p1", "/p", "OC | Same")]).is_empty());
    }

    #[test]
    fn the_one_running_session_in_the_folder_is_the_fallback() {
        let mut m = model(vec![session("ses_a", None, "A", "/p"), session("ses_b", None, "B", "/q")]);
        m.running = HashSet::from(["ses_a".to_string(), "ses_b".to_string()]);
        let a = m.assign(&[pane("p1", "/p", "OpenCode")]);
        assert_eq!(a["p1"], "ses_a");
        assert_eq!(a.len(), 1);
        // Another folder's session does not count, and a trailing slash does not matter.
        assert_eq!(m.assign(&[pane("p1", "/p/", "OpenCode")])["p1"], "ses_a");
        assert!(m.assign(&[pane("p1", "/elsewhere", "OpenCode")]).is_empty());
    }

    #[test]
    fn two_running_sessions_in_a_folder_are_ambiguous() {
        let mut m = model(vec![session("ses_a", None, "A", "/p"), session("ses_b", None, "B", "/p")]);
        m.running = HashSet::from(["ses_a".to_string(), "ses_b".to_string()]);
        assert!(m.assign(&[pane("p1", "/p", "OpenCode")]).is_empty());
    }

    #[test]
    fn a_title_match_is_not_taken_again_by_another_pane() {
        let mut m = model(vec![session("ses_a", None, "A", "/p")]);
        m.lists.insert((Some("/p".into()), "OC | A".into()), vec![session("ses_a", None, "A", "/p")]);
        m.running = HashSet::from(["ses_a".to_string()]);
        let got = m.assign(&[pane("p1", "/p", "OC | A"), pane("p2", "/p", "OpenCode")]);
        assert_eq!(got.get("p1").map(String::as_str), Some("ses_a"));
        assert!(!got.contains_key("p2"));
    }

    #[test]
    fn two_unmatched_panes_in_a_folder_share_nothing() {
        let mut m = model(vec![session("ses_a", None, "A", "/p")]);
        m.running = HashSet::from(["ses_a".to_string()]);
        assert!(m.assign(&[pane("p1", "/p", "OpenCode"), pane("p2", "/p", "OpenCode")]).is_empty());
    }

    // ---- shared state

    #[derive(Default)]
    struct Fake(Mutex<HashMap<String, Value>>, Mutex<Vec<String>>);

    impl Fake {
        fn with(routes: Vec<(&str, Value)>) -> Fake {
            Fake(Mutex::new(routes.into_iter().map(|(k, v)| (k.to_string(), v)).collect()), Mutex::default())
        }
    }

    impl Api for Fake {
        fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value, Error> {
            let key = if query.is_empty() { path.to_string() } else { format!("{path}?{}", query.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&")) };
            self.1.lock().unwrap().push(key.clone());
            self.0.lock().unwrap().get(&key).cloned().ok_or(Error::Status(404))
        }
    }

    #[test]
    fn a_snapshot_reads_running_sessions_and_what_they_wait_for() {
        let api = Fake::with(vec![
            ("/api/session/active", json!({"data":{"ses_a":{"type":"running"},"ses_b":{"type":"running"}}})),
            ("/api/session/ses_a", json!({"data":{"id":"ses_a","title":"A","location":{"directory":"/p"}}})),
            ("/api/session/ses_a/permission", json!({"data":[{"id":"per_1"}]})),
            ("/api/session/ses_a/form", json!({"data":[]})),
            ("/api/session/ses_b", json!({"data":{"id":"ses_b","parentID":"ses_a","location":{"directory":"/p"}}})),
            ("/api/session/ses_b/permission", json!({"data":[]})),
            ("/api/session/ses_b/form", json!({"data":[{"id":"frm_1"}]})),
        ]);
        let snap = snapshot(&api).unwrap();
        assert_eq!(snap.running.len(), 2);
        assert_eq!(snap.pending["ses_a"], HashSet::from(["per_1".to_string()]));
        assert_eq!(snap.pending["ses_b"], HashSet::from(["frm_1".to_string()]));
        let shared = Shared::default();
        shared.set_panes(vec![pane("p1", "/p", "OpenCode")]);
        shared.install(snap, 5);
        // Two running roots? No: ses_b is a child, so there is one root, and it is waiting.
        let v = shared.view("p1").unwrap();
        assert_eq!((v.session_id.as_str(), v.state, v.since), ("ses_a", State::Waiting, 5));
        shared.gone();
        assert!(shared.view("p1").is_none());
    }

    #[test]
    fn a_snapshot_that_cannot_be_read_fails() {
        assert_eq!(snapshot(&Fake::with(vec![("/api/session/active", json!({"nope":1}))])).err(), Some(Error::BadBody));
        assert_eq!(snapshot(&Fake::default()).err(), Some(Error::Status(404)));
    }

    #[test]
    fn events_and_lookups_change_what_a_pane_shows() {
        let shared = Shared::default();
        shared.set_panes(vec![pane("p1", "/p", "OC | Alpha")]);
        // An event for a session nothing is known about asks for it, and the pane's list.
        shared.apply(Update::Started("ses_a".into()), 100);
        let work = shared.lock().work();
        assert_eq!(work.infos, ["ses_a"]);
        assert_eq!(work.lists, [(Some("/p".to_string()), "OC | Alpha".to_string())]);
        assert!(shared.view("p1").is_none());

        let api = Fake::with(vec![
            ("/api/session/ses_a", json!({"data":{"id":"ses_a","title":"Alpha","location":{"directory":"/p"}}})),
            ("/api/session?directory=/p&parentID=null&limit=20", json!({"data":[{"id":"ses_a","title":"Alpha","location":{"directory":"/p"}}]})),
        ]);
        shared.resolved(resolve(&api, work), 200);
        let v = shared.view("p1").unwrap();
        assert_eq!((v.session_id.as_str(), v.state, v.since), ("ses_a", State::Working, 200));
        assert_eq!(shared.lock().work(), Work::default(), "nothing left to ask");

        shared.apply(Update::Pending { session: "ses_a".into(), id: "per_1".into() }, 300);
        assert_eq!(shared.view("p1").unwrap().state, State::Waiting);
        shared.apply(Update::Settled { session: "ses_a".into(), id: "per_1".into() }, 400);
        assert_eq!(shared.view("p1").unwrap().state, State::Working);
        shared.apply(Update::Renamed { session: "ses_a".into(), title: "Beta".into() }, 450);
        assert_eq!(shared.view("p1").unwrap().title.as_deref(), Some("Beta"));
        shared.apply(Update::Ended("ses_a".into()), 500);
        let v = shared.view("p1").unwrap();
        assert_eq!((v.state, v.since), (State::Idle, 500));
    }

    #[test]
    fn a_lookup_is_not_repeated_until_the_session_does_something() {
        let shared = Shared::default();
        shared.set_panes(vec![pane("p1", "/p", "OpenCode")]);
        shared.apply(Update::Started("ses_a".into()), 1);
        let (_, work) = {
            let mut inner = shared.lock();
            let work = inner.work();
            inner.tried_infos.extend(work.infos.iter().cloned());
            ((), work)
        };
        assert_eq!(work.infos, ["ses_a"]);
        assert_eq!(shared.lock().work(), Work::default(), "a failed lookup does not loop");
        shared.apply(Update::Pending { session: "ses_a".into(), id: "per_1".into() }, 2);
        assert_eq!(shared.lock().work().infos, ["ses_a"]);
    }

    #[test]
    fn a_pane_that_goes_forgets_its_list() {
        let shared = Shared::default();
        shared.set_panes(vec![pane("p1", "/p", "OC | A")]);
        shared.lock().model.lists.insert((Some("/p".into()), "OC | A".into()), vec![]);
        shared.set_panes(vec![pane("p1", "/p", "OC | B")]);
        assert!(shared.lock().model.lists.is_empty());
    }

    fn snap(running: &[&str]) -> Snapshot {
        Snapshot {
            running: running.iter().map(|s| s.to_string()).collect(),
            pending: HashMap::new(),
            infos: vec![session("ses_a", None, "A", "/p")],
        }
    }

    #[test]
    fn a_dropped_stream_keeps_the_last_known_state_until_the_snapshot() {
        let shared = Shared::default();
        shared.set_panes(vec![pane("p1", "/p", "OpenCode")]);
        shared.install(snap(&["ses_a"]), 10);
        assert_eq!(shared.view("p1").unwrap().state, State::Working);
        shared.disconnected();
        assert_eq!(shared.link(), Link::Reconnecting);
        let v = shared.view("p1").unwrap();
        assert_eq!((v.state, v.since), (State::Working, 10));
        // A pane that changes meanwhile is still worked out from what is known.
        shared.set_panes(vec![pane("p1", "/p", "OpenCode"), pane("p2", "/q", "zsh")]);
        assert_eq!(shared.view("p1").unwrap().state, State::Working);
        // Back, and the session still runs: nothing changed.
        shared.install(snap(&["ses_a"]), 99);
        let v = shared.view("p1").unwrap();
        assert_eq!((shared.link(), v.state, v.since), (Link::Up, State::Working, 10));
        // Back, and it finished while away.
        shared.disconnected();
        shared.install(snap(&[]), 120);
        // Nothing runs there, and a pane named only "OpenCode" has no session to show: idle.
        assert!(shared.view("p1").is_none());
    }

    #[test]
    fn the_service_is_gone_after_every_backoff_step() {
        assert!(!gone_after(BACKOFF.len() - 1));
        assert!(gone_after(BACKOFF.len()));
    }

    #[test]
    fn no_panes_forgets_everything() {
        let shared = Shared::default();
        shared.set_panes(vec![pane("p1", "/p", "OpenCode")]);
        shared.install(snap(&["ses_a"]), 10);
        shared.disconnected();
        shared.set_panes(vec![]);
        assert_eq!(shared.link(), Link::Connecting);
        shared.set_panes(vec![pane("p1", "/p", "OpenCode")]);
        assert!(shared.view("p1").is_none(), "no state carried over from before");
    }

    #[test]
    fn backs_off_and_stays_at_the_cap() {
        let secs: Vec<u64> = (0..7).map(|n| backoff(n).as_secs()).collect();
        assert_eq!(secs, [1, 2, 5, 10, 30, 30, 30]);
    }

    // ---- the real client against a local stub

    /// Answers each connection with the next canned response after reading its request head; returns the heads.
    fn stub(responses: Vec<Vec<u8>>) -> (u16, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let mut heads = Vec::new();
            for response in responses {
                let (mut socket, _) = listener.accept().unwrap();
                heads.push(read_head(&mut socket));
                let _ = socket.write_all(&response);
            }
            heads
        });
        (port, handle)
    }

    fn read_head(socket: &mut TcpStream) -> String {
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") && socket.read(&mut byte).unwrap_or(0) == 1 {
            head.push(byte[0]);
        }
        String::from_utf8_lossy(&head).into_owned()
    }

    fn client_for(port: u16) -> Client {
        Client::new(&Service::parse(&format!(r#"{{"url":"http://127.0.0.1:{port}","pid":1,"password":"secret"}}"#)).unwrap())
    }

    fn ok(body: &str) -> Vec<u8> {
        format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).into_bytes()
    }

    #[test]
    fn sends_basic_auth_and_reads_json() {
        let (port, server) = stub(vec![ok(r#"{"data":{}}"#)]);
        let got = client_for(port).get("/api/session/active", &[("directory", "/a b")]).unwrap();
        assert_eq!(got, json!({"data":{}}));
        let head = server.join().unwrap().remove(0).to_lowercase();
        assert!(head.starts_with("get /api/session/active?directory=%2fa%20b http/1.1"), "{head}");
        assert!(head.contains("authorization: basic b3blbmnvzgu6c2vjcmv0"), "{head}");
    }

    #[test]
    fn a_refusal_is_an_error_without_the_password() {
        let (port, server) = stub(vec![b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()]);
        let e = client_for(port).get("/api/info", &[]).unwrap_err();
        server.join().unwrap();
        assert_eq!(e, Error::Unauthorized);
        assert!(!format!("{e:?} {e}").contains("secret"));
    }

    #[test]
    fn a_service_that_never_answers_times_out() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let started = std::time::Instant::now();
        let e = client_for(port).get("/api/info", &[]).unwrap_err();
        assert_eq!(e, Error::Timeout);
        assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
        drop(listener);
    }

    #[test]
    fn nothing_listening_is_unreachable() {
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        assert_eq!(client_for(port).get("/api/info", &[]).unwrap_err(), Error::Unreachable);
    }

    /// The stream is read as it arrives: the first event is in hand while the response is still open.
    #[test]
    fn follows_a_stream_without_waiting_for_it_to_end() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (release, gate) = std::sync::mpsc::channel::<()>();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let head = read_head(&mut socket).to_lowercase();
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").unwrap();
            let chunk = |s: &str| format!("{:x}\r\n{s}\r\n", s.len());
            socket.write_all(chunk("data: {\"type\":\"server.connected\",\"data\":{}}\n\n").as_bytes()).unwrap();
            socket.flush().unwrap();
            // The rest only after the client has read the first event.
            gate.recv().unwrap();
            socket.write_all(chunk(": heartbeat\n\n").as_bytes()).unwrap();
            socket.write_all(chunk("data: {\"type\":\"session.execution.started\",\"data\":{\"sessionID\":\"ses_a\"}}\n\n").as_bytes()).unwrap();
            socket.write_all(b"0\r\n\r\n").unwrap();
            head
        });
        let mut sse = Sse::new(client_for(port).stream().unwrap());
        assert_eq!(sse.next_frame().unwrap(), Some(Frame::Data("{\"type\":\"server.connected\",\"data\":{}}".into())));
        release.send(()).unwrap();
        assert_eq!(sse.next_frame().unwrap(), Some(Frame::Comment));
        let Some(Frame::Data(data)) = sse.next_frame().unwrap() else { panic!("no event") };
        assert_eq!(parse_event(&data).0, Update::Started("ses_a".into()));
        assert_eq!(sse.next_frame().unwrap(), None);
        let head = server.join().unwrap();
        assert!(head.starts_with("get /api/event "), "{head}");
        assert!(head.contains("authorization: basic "), "{head}");
    }
}
