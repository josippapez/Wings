//! One plugin running natively: its main script and any number of its pages, talking through `wings.broadcast` and
//! keeping what they save in `wings.storage`. Main runs on an engine of its own, and the pages share one (see
//! `runtime::Engine`), which ends when the last page closes: one thread and runtime for them all, and, for pages built
//! to share React and `wings-plugin-ui` (`runtime::Library`), one compiled copy of those.
//!
//! This does what the desktop's plugin host does for its frames (`apps/desktop/src/lib/plugins.ts`, the `broadcast` and
//! `storage*` cases, and `plugin_storage_*` in `src-tauri/src/lib.rs`):
//!
//! - **Broadcast.** The message is JSON (`JSON.stringify(message ?? null)`) of at most 64,000 characters, counted in
//!   JS string units; a longer one rejects the sender's promise with "A broadcast is limited to 64 KB" and goes to no
//!   one (`wings-plugin-sdk/messages` sends a bigger message in parts). It goes to every *other* open frame of the same
//!   plugin, never to the sender and never to another plugin's frames, in the order the frames were opened, and each
//!   frame gets its own parsed copy. A frame still starting up gets it once its bundle has run, which is when its
//!   listeners exist. The sender doesn't wait for the listeners. A frame that is too far behind, or has stopped,
//!   misses it, and nothing else is held up.
//! - **Storage.** One folder of keys per plugin and install scope, kept by `wings-plugin-storage` (the desktop's own
//!   crate, so the files are the same): keys of 1 to 128 letters, digits and `- _ . : /`, JSON values of at most 1 MB
//!   and 50 MB together, all frames of the plugin sharing them. Only what `JSON.stringify` keeps is stored, so a value
//!   with nothing to stringify is refused (`delete` removes a key).
//!
//! - **The rest of `wings`.** A host built with `with_access` has the plugin's manifest and the app it runs in (`Env`),
//!   and answers the calls `calls` lists: `exec`, `openUrl`, `ports`, `notify`, `transcript`, `statusline`, `setBadge`
//!   and `setPaneStatus`, each under the permission `wings-plugins` says the manifest grants. The app tells it the panes
//!   (`publish_panes`), which every frame hears with `onPanes`, and the transcript entries Claude writes
//!   (`publish_transcript`); a click on a badge's button is `send_action`, which the main frame's `onAction` listeners
//!   answer. Projects, focus and permission-gated git events follow the workspace. Disk searches and opening panes or
//!   views use the desktop's contracts too. A plugin's main answers the tools another of Wings' own plugins calls (`onTool`;
//!   `callTool` hands the call over with `run_tool`). A host without access refuses calls.

use super::runtime::{
    Delivery, Engine, Event, Inbox, Library, Limits, Link, Message, PageRuntime, Reply, Role, Store,
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use wings_plugin_storage::Storage;
use wings_plugins::permissions::{NotifyLog, may_read_agent_activity};

mod calls;
mod diff;
mod disk;
mod env;
mod state;
pub use state::{FrameState, StatePlace};
#[cfg(test)]
mod focus_tests;
pub mod ui;

pub use calls::{SectionIcon, SectionRow};
pub use env::{
    Access, ClaudeCaller, Env, Notification, NotifyButton, OpenPane, OpenView, PaneEntry, PanePort,
    Project, Running, SessionEntry, ToolCall,
};

/// The most JSON a broadcast may hold, in JS string units: what the desktop's host allows.
pub const MAX_BROADCAST_CHARS: usize = 64_000;

fn effective_focus(windows: &HashMap<u64, bool>, role: Role, window: u64) -> bool {
    if role == Role::Main {
        windows.values().any(|active| *active)
    } else {
        windows.get(&window).copied().unwrap_or(false)
    }
}

pub struct PluginWindow {
    pub id: u64,
    shared: Arc<Shared>,
}

impl PluginWindow {
    pub fn set_active(&self, active: bool) {
        self.shared.window_focus(self.id, Some(active));
    }
}

impl Drop for PluginWindow {
    fn drop(&mut self) {
        self.shared.window_focus(self.id, None);
    }
}

/// Which plugin this is and where it keeps what it saves. The storage root and scope come from the app, as the Issues
/// tab's do, never from the plugin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginConfig {
    /// The plugin's id, as in its manifest: ASCII letters, digits and `-`.
    pub id: String,
    /// The folder the app keeps every plugin's storage in.
    pub root: PathBuf,
    /// The install scope, which a plugin that is removed and added again doesn't share: ASCII letters, digits and `-`.
    pub scope: String,
}

fn json(value: &impl serde::Serialize) -> Result<String, String> {
    serde_json::to_string(value).map_err(|error| error.to_string())
}

/// An open frame, as the plugin's other frames and the host reach it.
struct Open {
    id: u64,
    role: Role,
    inbox: Inbox,
    visible: Arc<AtomicBool>,
    window: u64,
}

/// The pane list the app last published, and its JSON, which is what each frame is sent.
#[derive(Default)]
struct Panes {
    list: Vec<PaneEntry>,
    json: Option<String>,
}

/// What a badge button's click comes to: the plugin settled it, or the error one of its listeners threw.
type Done = Box<dyn FnOnce(Result<(), String>) + Send>;

/// Badge buttons clicked and not yet answered by the plugin's main, by the token the frame answers with.
#[derive(Default)]
struct Actions {
    last: u32,
    waiting: HashMap<u32, Done>,
}

/// Tool calls from other plugins that this plugin's main hasn't answered, by the token it answers with.
#[derive(Default)]
struct Tools {
    last: u32,
    waiting: HashMap<u32, Reply>,
}

/// What all of a plugin's frames share.
struct Shared {
    config: PluginConfig,
    storage: Storage,
    /// The plugin's manifest and the app, which the calls of `calls` are checked against and answered by.
    access: Option<Access>,
    next_frame: AtomicU64,
    /// The open frames in the order they opened.
    frames: Mutex<Vec<Open>>,
    panes: Mutex<Panes>,
    actions: Mutex<Actions>,
    tools: Mutex<Tools>,
    notified: Mutex<NotifyLog>,
    projects: Mutex<Vec<Project>>,
    /// The JSON of the project on screen (or `null`) that the frames were last told, once the app has said.
    current: Mutex<Option<String>>,
    focused: Mutex<HashMap<u64, bool>>,
    next_window: AtomicU64,
    diffs: Mutex<HashMap<String, (u64, Option<u32>)>>,
    next_diff: AtomicU64,
    scans: Mutex<HashMap<String, Arc<std::sync::atomic::AtomicBool>>>,
}

impl Shared {
    fn window_focus(&self, id: u64, active: Option<bool>) {
        let mut windows = self.focused.lock().unwrap();
        if windows.get(&id).copied() == active {
            return;
        }
        let previous_main = windows.values().any(|active| *active);
        match active {
            Some(active) => {
                windows.insert(id, active);
            }
            None => {
                windows.remove(&id);
            }
        }
        let main = windows.values().any(|active| *active);
        for frame in self.frames.lock().unwrap().iter() {
            if (frame.role == Role::Main && previous_main != main)
                || (frame.role == Role::Page && frame.window == id)
            {
                frame.inbox.send(Message::Focus, 0, serde_json::json!({ "focused": effective_focus(&windows, frame.role, frame.window) && frame.visible.load(Ordering::Relaxed) }).to_string());
            }
        }
    }
    fn broadcast(&self, from: u64, json: String) -> Result<(), String> {
        if json.encode_utf16().count() > MAX_BROADCAST_CHARS {
            return Err("A broadcast is limited to 64 KB".into());
        }
        // A frame that has stopped is dropped from the list as it's found. Delivering never waits for a frame.
        self.frames.lock().unwrap().retain(|frame| {
            frame.id == from || frame.inbox.deliver(json.clone()) != Delivery::Stopped
        });
        Ok(())
    }

    /// Tells every open frame `message`. A frame that is behind misses it, and one that has stopped is let go of.
    fn tell_all(&self, message: Message, text: &str) {
        self.frames
            .lock()
            .unwrap()
            .retain(|frame| frame.inbox.send(message, 0, text.to_owned()) != Delivery::Stopped);
    }

    /// The plugin's main answered a badge button, or failed to: ends the wait for `token`.
    fn settle_action(&self, token: u32, error: Option<String>) {
        for (_, pending) in self.diffs.lock().unwrap().values_mut() {
            if *pending == Some(token) {
                *pending = None;
            }
        }
        let done = self.actions.lock().unwrap().waiting.remove(&token);
        if let Some(done) = done {
            done(error.map_or(Ok(()), Err));
        }
    }

    /// The plugin's main answered a tool call: the caller's promise resolves with `{ text, isError }`.
    fn settle_tool(&self, token: u32, text: &str, is_error: bool) {
        let waiting = self.tools.lock().unwrap().waiting.remove(&token);
        if let Some(reply) = waiting {
            reply.ok(serde_json::json!({ "text": text, "isError": is_error }).to_string());
        }
    }

    /// Answers every tool call still waiting as failed, as the desktop does for a plugin that is turned off.
    fn fail_tools(&self, why: &str) {
        let waiting = std::mem::take(&mut self.tools.lock().unwrap().waiting);
        for reply in waiting.into_values() {
            reply.ok(serde_json::json!({ "text": why, "isError": true }).to_string());
        }
    }

    /// Fails every button click still waiting for an answer.
    fn fail_actions(&self, why: &str) {
        let waiting = std::mem::take(&mut self.actions.lock().unwrap().waiting);
        for done in waiting.into_values() {
            done(Err(why.to_owned()));
        }
    }

    fn store(&self, call: Store) -> Result<String, String> {
        let PluginConfig { id, scope, .. } = &self.config;
        let storage = &self.storage;
        match call {
            Store::Get(key) => json(&storage.get(id, scope, &key)?),
            Store::Set(key, text) => {
                let value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
                storage.set(id, scope, &key, value)?;
                json(&())
            }
            Store::Delete(key) => {
                storage.delete(id, scope, &key)?;
                json(&())
            }
            Store::Keys => json(&storage.keys(id, scope)?),
            Store::Entries(prefix) => json(&storage.entries(id, scope, &prefix)?),
        }
    }
}

/// What one frame of the plugin is given to reach the others.
struct FrameLink {
    id: u64,
    shared: Arc<Shared>,
    visible: Arc<AtomicBool>,
    role: Role,
    window: u64,
    /// The focus the frame was first told, set once it's registered. Its bundle reads this, not the live value,
    /// so `wings.focused` agrees with the first focus message it hears.
    ready: Arc<(Mutex<Option<bool>>, Condvar)>,
    /// What the page was opened with, as JSON, for `wings.viewArgs`.
    args: Option<String>,
    state: Option<FrameState>,
    /// When the app last handed the page a click, key or other event of yours.
    acted: Mutex<Option<Instant>>,
}

/// How long after your click or key a page may still put text on the clipboard: a browser's transient user activation,
/// which is what lets a desktop page's `navigator.clipboard.writeText` work.
const USER_ACTION: Duration = Duration::from_secs(5);

impl Link for FrameLink {
    fn user_event(&self) {
        *self.acted.lock().unwrap() = Some(Instant::now());
    }

    fn focused(&self) -> bool {
        let (ready, wake) = &*self.ready;
        let mut started = ready.lock().unwrap();
        loop {
            if let Some(focused) = *started {
                return focused;
            }
            started = wake.wait(started).unwrap();
        }
    }

    fn set_visible(&self, visible: bool) {
        if self.visible.load(Ordering::Relaxed) == visible {
            return;
        }
        let focused = self.shared.focused.lock().unwrap();
        let frames = self.shared.frames.lock().unwrap();
        if self.visible.swap(visible, Ordering::Relaxed) == visible {
            return;
        }
        if let Some(frame) = frames.iter().find(|frame| frame.id == self.id) {
            frame.inbox.send(
                Message::Focus,
                0,
                serde_json::json!({ "focused": effective_focus(&focused, self.role, self.window) && visible }).to_string(),
            );
        }
    }
    fn broadcast(&self, json: String) -> Result<(), String> {
        self.shared.broadcast(self.id, json)
    }

    fn view_args(&self) -> Option<String> {
        self.args.clone()
    }

    fn storage(&self, call: Store) -> Result<String, String> {
        self.shared.store(call)
    }

    fn call(&self, method: &str, mut params: serde_json::Value, reply: Reply) {
        if matches!(
            method,
            "tabState" | "setTabState" | "blockState" | "setBlockState"
        ) {
            let place = if method.contains("Tab") || method == "tabState" {
                StatePlace::Tab
            } else {
                StatePlace::Block
            };
            if !method.starts_with("set") {
                return reply.ok(self
                    .state
                    .as_ref()
                    .map_or(serde_json::Value::Null, |state| state.read(place))
                    .to_string());
            }
            let Some(state) = self.state.clone() else {
                return reply
                    .err("State can only be saved by a page shown as a tab or block".into());
            };
            let Some(value) = params.get("state").cloned() else {
                return reply.err("State must be a JSON value".into());
            };
            std::thread::spawn(move || {
                reply.done(state.write(place, value).map(|()| "null".into()))
            });
            return;
        }
        if method == "copyText" {
            // Each click or key lets the page try to copy once.
            let acted = self
                .acted
                .lock()
                .unwrap()
                .take()
                .is_some_and(|at| at.elapsed() < USER_ACTION);
            if self.role != Role::Page || !acted {
                return reply
                    .err("A page can copy only right after you click or type in it".into());
            }
        }
        if matches!(method, "openDiff" | "updateDiff") {
            return self.shared.diff_call(self.id, method, params, reply);
        }
        if matches!(method, "diskFind" | "diskUsage" | "diskUsageCancel") {
            let Some(token) = params
                .get("token")
                .and_then(serde_json::Value::as_str)
                .filter(|s| (1..=64).contains(&s.encode_utf16().count()))
            else {
                return reply.err("A scan needs a token of 1 to 64 characters".into());
            };
            let token = format!("{}:{token}", self.id);
            params["token"] = token.into();
        }
        self.shared.call(method, params, reply)
    }
}

/// A frame's place among the plugin's frames, until the frame is let go of.
struct Membership {
    id: u64,
    shared: Arc<Shared>,
}

impl Drop for Membership {
    fn drop(&mut self) {
        self.shared.close_frame_diffs(self.id);
        let prefix = format!("{}:", self.id);
        for (token, flag) in self.shared.scans.lock().unwrap().iter() {
            if token.starts_with(&prefix) {
                flag.store(true, Ordering::Relaxed);
            }
        }
        self.shared
            .frames
            .lock()
            .unwrap()
            .retain(|frame| frame.id != self.id);
    }
}

/// A plugin: its main script, and the pages opened from it. Frames hear and answer each other until they're dropped;
/// a frame that stops for good (it threw while starting, or ran too long) leaves the others going.
pub struct PluginHost {
    shared: Arc<Shared>,
    limits: Limits,
    main: Mutex<Option<PageRuntime>>,
    /// The engine the open pages run on, while one is open.
    pages: Mutex<Weak<Engine>>,
    /// The plugin's `dist/native/shared.js`, which its pages built to share it need.
    library: Mutex<Option<Arc<Library>>>,
}

impl PluginHost {
    pub fn new(config: PluginConfig) -> Self {
        Self::with_limits(config, Limits::default())
    }

    pub fn with_limits(config: PluginConfig, limits: Limits) -> Self {
        Self::build(config, None, limits)
    }

    /// A host that runs `access.plugin` in `access.env`: its frames can make the calls its manifest grants.
    pub fn with_access(config: PluginConfig, access: Access) -> Self {
        Self::build(config, Some(access), Limits::default())
    }

    fn build(config: PluginConfig, access: Option<Access>, limits: Limits) -> Self {
        let storage = Storage::new(config.root.clone());
        Self {
            shared: Arc::new(Shared {
                config,
                storage,
                access,
                next_frame: AtomicU64::new(0),
                frames: Mutex::default(),
                panes: Mutex::default(),
                actions: Mutex::default(),
                tools: Mutex::default(),
                notified: Mutex::default(),
                projects: Mutex::default(),
                current: Mutex::default(),
                focused: Mutex::new(HashMap::from([(0, true)])),
                next_window: AtomicU64::new(1),
                diffs: Mutex::default(),
                next_diff: AtomicU64::new(1),
                scans: Mutex::default(),
            }),
            limits,
            main: Mutex::default(),
            pages: Mutex::default(),
            library: Mutex::default(),
        }
    }

    /// The React and `wings-plugin-ui` the plugin's pages share, read from its `dist/native/shared.js` (see
    /// `runtime::Library`). A page opened after this that was built with it runs on it.
    pub fn set_library(&self, library: Option<Arc<Library>>) {
        *self.library.lock().unwrap() = library;
    }

    /// The library set with `set_library`.
    pub fn library(&self) -> Option<Arc<Library>> {
        self.library.lock().unwrap().clone()
    }

    /// Starts the plugin's main script, in place of the one that ran before, which is let go of. Its errors, and the
    /// last one when it stops, go to `on_event`, which is called from the frame's thread.
    pub fn start_main(
        &self,
        source: &str,
        on_event: impl FnMut(Event) + Send + 'static,
    ) -> std::io::Result<()> {
        let main = self.spawn(Role::Main, true, 0, None, source, on_event)?;
        // The old main leaves the list as it drops, after the new one joined: a message in between reaches the new one.
        let old = self.main.lock().unwrap().replace(main);
        if old.is_some() {
            // Whatever it was asked is lost with it.
            self.shared.fail_actions("The plugin was turned off");
            self.shared.fail_tools(&format!(
                "{} was turned off in Wings",
                self.shared.config.id
            ));
        }
        Ok(())
    }

    /// The panes the app has open now. Every frame hears the list with `onPanes`, now and as a frame starts, and nothing
    /// is sent when it is what the frames heard last.
    pub fn publish_panes(&self, panes: Vec<PaneEntry>) {
        let Ok(json) = serde_json::to_string(&panes) else {
            return;
        };
        {
            let mut state = self.shared.panes.lock().unwrap();
            if state.json.as_deref() == Some(json.as_str()) {
                return;
            }
            *state = Panes {
                list: panes,
                json: Some(json.clone()),
            };
        }
        self.shared.tell_all(Message::Panes, &json);
    }

    pub fn publish_projects(&self, projects: Vec<Project>) {
        let mut old = self.shared.projects.lock().unwrap();
        if *old != projects {
            self.shared
                .tell_all(Message::Projects, &json(&projects).unwrap());
            *old = projects;
        }
    }

    /// The project on screen, by its folder: every frame hears it with `onProject` when it is not the one they heard
    /// last, which is when the user switched projects or that project was renamed. Call after `publish_projects`.
    pub fn publish_project(&self, path: Option<&std::path::Path>) {
        let json = {
            let projects = self.shared.projects.lock().unwrap();
            json(&path.and_then(|path| projects.iter().find(|project| std::path::Path::new(&project.path) == path)))
                .unwrap()
        };
        let mut current = self.shared.current.lock().unwrap();
        if current.as_deref() != Some(json.as_str()) {
            self.shared.tell_all(Message::Project, &json);
            *current = Some(json);
        }
    }

    pub fn publish_focus(&self, focused: bool) {
        self.shared.window_focus(0, Some(focused));
    }

    pub fn publish_git(&self, project_ids: &[String]) {
        if self.shared.access.as_ref().is_some_and(|access| {
            access.plugin.manifest.permissions.exec.iter().any(|entry| {
                entry
                    .split('[')
                    .next()
                    .unwrap_or("")
                    .split_ascii_whitespace()
                    .next()
                    == Some("git")
            })
        }) {
            self.shared.tell_all(
                Message::Git,
                &serde_json::json!({ "projectIds": project_ids }).to_string(),
            );
        }
    }

    /// A transcript entry Claude wrote, as the JSON `{ sessionId, paneId, entry }`, for the frames' `onTranscript`. The
    /// app has checked the plugin's `permissions.transcript` names the entry's type.
    pub fn publish_transcript(&self, json: &str) {
        self.shared.tell_all(Message::Transcript, json);
    }

    /// An OpenCode session did something, for the frames' `onAgentActivity`: only a plugin whose manifest has
    /// `permissions.agentActivity` hears it.
    pub fn publish_agent_activity(&self, session_id: &str) {
        if self
            .shared
            .access
            .as_ref()
            .is_some_and(|access| may_read_agent_activity(&access.plugin))
        {
            self.shared.tell_all(
                Message::AgentActivity,
                &serde_json::json!({ "sessionId": session_id }).to_string(),
            );
        }
    }

    /// A badge button was clicked: the main frame's `onAction` listeners get it, and `done` is called when they have
    /// all settled, with the error of the first that threw. It fails at once if the plugin has no main running.
    pub fn send_action(
        &self,
        pane_id: &str,
        action_id: &str,
        done: impl FnOnce(Result<(), String>) + Send + 'static,
    ) {
        let text = serde_json::json!({ "paneId": pane_id, "actionId": action_id }).to_string();
        self.send_action_message(Message::Action, text, done);
    }

    /// Sends the main frame `message`, which it answers with `actionDone`, and calls `done` with the answer.
    fn send_action_message(
        &self,
        message: Message,
        text: String,
        done: impl FnOnce(Result<(), String>) + Send + 'static,
    ) {
        let main = self
            .shared
            .frames
            .lock()
            .unwrap()
            .iter()
            .find(|frame| frame.role == Role::Main)
            .map(|frame| frame.inbox.clone());
        let Some(main) = main else {
            return done(Err(format!(
                "plugin {} is not running",
                self.shared.config.id
            )));
        };
        let token = {
            let mut actions = self.shared.actions.lock().unwrap();
            actions.last += 1;
            let token = actions.last;
            actions.waiting.insert(token, Box::new(done));
            token
        };
        if main.send(message, token, text) != Delivery::Queued {
            self.shared
                .settle_action(token, Some("The plugin didn't respond".into()));
        }
    }

    /// Another plugin calls this one's tool `name`: its main runs the handler `onTool` registered, and `reply` gets
    /// `{ text, isError }`. It fails at once if the plugin has no main running yet.
    pub fn run_tool(
        &self,
        name: &str,
        input: serde_json::Value,
        project: Option<&Project>,
        reply: Reply,
    ) {
        let call = serde_json::json!({ "name": name, "arguments": input, "project": project });
        self.send_tool(call, reply);
    }

    /// Claude Code calls this plugin's tool `name` (`hook` false), or one of its hooks answered with `onHook` (`hook`
    /// true), from the pane `caller` names, as the desktop's `mcp-call` does. `reply` gets `{ text, isError }`; a hook's
    /// text is the JSON `{ context?, deny? }`.
    pub fn run_for_claude(
        &self,
        name: &str,
        input: serde_json::Value,
        caller: &ClaudeCaller,
        hook: bool,
        reply: Reply,
    ) {
        let call = serde_json::json!({
            "name": name, "arguments": input, "hook": hook,
            "paneId": caller.pane_id, "project": caller.project, "session": caller.session,
        });
        self.send_tool(call, reply);
    }

    fn send_tool(&self, call: serde_json::Value, reply: Reply) {
        let main = self
            .shared
            .frames
            .lock()
            .unwrap()
            .iter()
            .find(|frame| frame.role == Role::Main)
            .map(|frame| frame.inbox.clone());
        let Some(main) = main else {
            return reply.err(format!(
                "{} is still starting in Wings. Try again in a moment.",
                self.shared.config.id
            ));
        };
        let token = {
            let mut tools = self.shared.tools.lock().unwrap();
            tools.last += 1;
            let token = tools.last;
            tools.waiting.insert(token, reply);
            token
        };
        if main.send(Message::Tool, token, call.to_string()) != Delivery::Queued {
            let waiting = self.shared.tools.lock().unwrap().waiting.remove(&token);
            if let Some(reply) = waiting {
                reply.err("The plugin didn't respond".into());
            }
        }
    }

    /// How many frames the plugin has open, its main included: a frame leaves when what holds it is let go of.
    #[cfg(all(test, feature = "test-support"))]
    pub(crate) fn frames(&self) -> usize {
        self.shared.frames.lock().unwrap().len()
    }

    /// Opens one of the plugin's pages. It lives as long as what's returned: the view that shows it holds that.
    pub fn open_page(
        &self,
        source: &str,
        on_event: impl FnMut(Event) + Send + 'static,
    ) -> std::io::Result<PageRuntime> {
        self.spawn(Role::Page, true, 0, None, source, on_event)
    }

    /// Starts unfocused even during bundle evaluation, until its view is shown.
    pub fn open_hidden_page(
        &self,
        source: &str,
        on_event: impl FnMut(Event) + Send + 'static,
    ) -> std::io::Result<PageRuntime> {
        self.spawn(Role::Page, false, 0, None, source, on_event)
    }

    /// Like `open_hidden_page`, for a tab `wings.openView` opened with `args`, which the page reads with `viewArgs`.
    pub fn open_hidden_page_with_args(
        &self,
        source: &str,
        args: Option<&serde_json::Value>,
        on_event: impl FnMut(Event) + Send + 'static,
    ) -> std::io::Result<PageRuntime> {
        self.spawn(
            Role::Page,
            false,
            0,
            args.map(|args| args.to_string()),
            source,
            on_event,
        )
    }

    /// Another plugin's event, as the JSON `{ from, name, data }`, for the `onEvent` listeners of this plugin's main.
    pub fn hear_event(&self, json: &str) {
        let main = self
            .shared
            .frames
            .lock()
            .unwrap()
            .iter()
            .find(|frame| frame.role == Role::Main)
            .map(|frame| frame.inbox.clone());
        if let Some(main) = main {
            main.send(Message::PluginEvent, 0, json.to_owned());
        }
    }

    /// A sort the user picked in the plugin's project section menu, for the `onSectionSort` listeners of its main.
    pub fn hear_section_sort(&self, section: &str, sort: &str) {
        let main = self
            .shared
            .frames
            .lock()
            .unwrap()
            .iter()
            .find(|frame| frame.role == Role::Main)
            .map(|frame| frame.inbox.clone());
        if let Some(main) = main {
            let json = serde_json::json!({ "sectionId": section, "sort": sort });
            main.send(Message::SectionSort, 0, json.to_string());
        }
    }

    pub fn register_window(&self) -> PluginWindow {
        let id = self.shared.next_window.fetch_add(1, Ordering::Relaxed);
        self.shared.window_focus(id, Some(false));
        PluginWindow {
            id,
            shared: self.shared.clone(),
        }
    }

    pub fn open_window_page(
        &self,
        source: &str,
        window_id: u64,
        on_event: impl FnMut(Event) + Send + 'static,
    ) -> std::io::Result<PageRuntime> {
        self.spawn(Role::Page, true, window_id, None, source, on_event)
    }

    fn spawn(
        &self,
        role: Role,
        visible: bool,
        window: u64,
        args: Option<String>,
        source: &str,
        on_event: impl FnMut(Event) + Send + 'static,
    ) -> std::io::Result<PageRuntime> {
        self.spawn_with_state(role, visible, window, args, None, source, on_event)
    }

    pub fn open_hidden_page_with_state(
        &self,
        source: &str,
        args: Option<&serde_json::Value>,
        state: FrameState,
        on_event: impl FnMut(Event) + Send + 'static,
    ) -> std::io::Result<PageRuntime> {
        self.spawn_with_state(
            Role::Page,
            false,
            0,
            args.map(|args| args.to_string()),
            Some(state),
            source,
            on_event,
        )
    }

    /// The engine the plugin's pages run on: the one its open pages use, or a new one when none is open.
    fn page_engine(&self) -> std::io::Result<Arc<Engine>> {
        let mut pages = self.pages.lock().unwrap();
        if let Some(engine) = pages.upgrade() {
            return Ok(engine);
        }
        let engine = Engine::start(self.limits)?;
        *pages = Arc::downgrade(&engine);
        Ok(engine)
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_with_state(
        &self,
        role: Role,
        visible: bool,
        window: u64,
        args: Option<String>,
        state: Option<FrameState>,
        source: &str,
        on_event: impl FnMut(Event) + Send + 'static,
    ) -> std::io::Result<PageRuntime> {
        let id = self.shared.next_frame.fetch_add(1, Ordering::Relaxed);
        let visible = Arc::new(AtomicBool::new(visible));
        let ready = Arc::new((Mutex::new(None), Condvar::new()));
        let link = Arc::new(FrameLink {
            id,
            shared: self.shared.clone(),
            visible: visible.clone(),
            role,
            window,
            ready: ready.clone(),
            args,
            state,
            acted: Mutex::new(None),
        });
        let shared = Arc::downgrade(&self.shared);
        let mut on_event = on_event;
        let on_event = move |event| {
            if matches!(&event, Event::Error { fatal: true, .. })
                && let Some(shared) = shared.upgrade()
            {
                shared.close_frame_diffs(id);
            }
            on_event(event);
        };
        let runtime = match role {
            Role::Main => PageRuntime::spawn_plugin(source, role, self.limits, link, on_event)?,
            Role::Page => PageRuntime::open_plugin(
                &self.page_engine()?,
                source,
                role,
                link,
                self.library(),
                on_event,
            ),
        };
        let inbox = runtime.inbox();
        let focused = self.shared.focused.lock().unwrap();
        self.shared.frames.lock().unwrap().push(Open {
            id,
            role,
            inbox: inbox.clone(),
            visible: visible.clone(),
            window,
        });
        // The frame hears the panes once its bundle has run, which is when its listeners exist.
        let panes = self.shared.panes.lock().unwrap().json.clone();
        if let Some(json) = panes {
            inbox.send(Message::Panes, 0, json);
        }
        inbox.send(
            Message::Projects,
            0,
            json(&*self.shared.projects.lock().unwrap()).unwrap(),
        );
        if let Some(current) = self.shared.current.lock().unwrap().clone() {
            inbox.send(Message::Project, 0, current);
        }
        let initial = effective_focus(&focused, role, window) && visible.load(Ordering::Relaxed);
        inbox.send(
            Message::Focus,
            0,
            serde_json::json!({ "focused": initial }).to_string(),
        );
        *ready.0.lock().unwrap() = Some(initial);
        ready.1.notify_all();
        Ok(runtime.attach(Membership {
            id,
            shared: self.shared.clone(),
        }))
    }
}

impl Drop for PluginHost {
    fn drop(&mut self) {
        self.shared.close_all_diffs();
        for flag in self.shared.scans.lock().unwrap().values() {
            flag.store(true, Ordering::Relaxed);
        }
        self.shared.fail_actions("The plugin was turned off");
        self.shared.fail_tools(&format!(
            "{} was turned off in Wings",
            self.shared.config.id
        ));
    }
}

#[cfg(test)]
mod shipped;
#[cfg(test)]
mod testing;
#[cfg(test)]
pub(super) mod tests;
