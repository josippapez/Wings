//! Fakes for the host's tests: an app that records what plugins did to it, and frames that make the calls a test asks for
//! and report what came of them.

use std::{
    path::PathBuf,
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

use serde_json::{Value, json};
use wings_plugins::manifest::Plugin;
use wings_spaces::{claude_accounts::Accounts, spaces::WorktreeSetup};

use super::{
    Access, Env, PaneEntry, PanePort, PluginHost, Project, Running, ToolCall,
    tests::Root,
    ui::{Badge, PaneStatus, SidebarLabel},
};
use crate::{
    page::runtime::{
        Event, PageRuntime,
        fake::{self, Cx, Guest},
    },
    projects::Projects,
};

pub(super) const WAIT: Duration = Duration::from_secs(10);

/// What a plugin did to the app, in order.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Seen {
    DiffOpen(crate::diff::View),
    DiffUpdate(String, String, crate::diff::Update),
    DiffClose(String, String),
    OpenPane(super::OpenPane),
    OpenView(super::OpenView),
    FocusPane(String),
    CloseView(String, String),
    Badge(String, String, Option<Badge>),
    Status(String, String, Option<PaneStatus>),
    Url(String),
    Notice(String, String),
    Copied(String),
    SendToAgent(String, String),
    OpenSettings(String),
    /// A plugin's section under a project: plugin, project, section, rows.
    ProjectSection(String, String, String, Option<Vec<super::SectionRow>>),
    /// A plugin's confirmed sort: plugin, section, sort.
    SectionSort(String, String, String),
    /// A named Claude Code account got ready for a terminal.
    PreparedAccount(String),
    /// A named account's share of the default account's setup turned on or off.
    SharedSetup(String, bool),
    /// A plugin's label by a title bar button: plugin, sidebar or view, label.
    SidebarLabel(String, String, Option<SidebarLabel>),
}

pub(super) struct FakeEnv {
    pub projects: Mutex<Vec<PathBuf>>,
    /// The list `createWorktree` and `worktreeSetup` work on, which tests replace with one of their own.
    pub spaces: Mutex<Projects>,
    /// The folders of the Claude Code accounts, which `transcript` reads.
    pub claude: Mutex<Vec<PathBuf>>,
    pub ports: Mutex<Result<Vec<PanePort>, String>>,
    /// The OpenCode sessions the service knows, and the ones `agentActivity` was asked about.
    opencode: Mutex<Vec<String>>,
    asked: Mutex<Vec<String>>,
    /// Why opening a URL or showing a notification fails, if it should.
    pub refuse: Mutex<Option<String>>,
    /// The other plugins running in the app, which `callTool` reaches.
    pub running: Running,
    /// The secrets plugins kept, by plugin, scope and name.
    pub secrets: Mutex<std::collections::HashMap<(String, String, String), String>>,
    /// What Claude Code told `wings statusline`.
    pub statusline: Mutex<wings_connect::statusline::Statusline>,
    log: Mutex<Vec<Seen>>,
    changed: Condvar,
}

impl FakeEnv {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            projects: Mutex::default(),
            spaces: Mutex::new(Projects::open(None, "/nowhere".into())),
            claude: Mutex::default(),
            ports: Mutex::new(Ok(Vec::new())),
            opencode: Mutex::default(),
            asked: Mutex::default(),
            refuse: Mutex::default(),
            running: Running::default(),
            secrets: Mutex::default(),
            statusline: Mutex::default(),
            log: Mutex::default(),
            changed: Condvar::new(),
        })
    }

    fn record(&self, seen: Seen) {
        self.log.lock().unwrap().push(seen);
        self.changed.notify_all();
    }

    pub fn opencode_knows(&self, session: &str) {
        self.opencode.lock().unwrap().push(session.into());
    }

    pub fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.log.lock().unwrap().clone()
    }

    /// Waits for what the plugin did to be what `done` likes, and returns it.
    pub fn wait(&self, what: &str, done: impl Fn(&[Seen]) -> bool) -> Vec<Seen> {
        let mut log = self.log.lock().unwrap();
        loop {
            if done(&log) {
                return log.clone();
            }
            let (next, timeout) = self.changed.wait_timeout(log, WAIT).unwrap();
            log = next;
            assert!(
                !timeout.timed_out() || done(&log),
                "waiting for {what}: the plugin did {log:?}"
            );
        }
    }
}

impl Env for FakeEnv {
    fn open_diff(&self, view: crate::diff::View) -> Result<(), String> {
        self.record(Seen::DiffOpen(view));
        Ok(())
    }
    fn update_diff(
        &self,
        plugin: &str,
        id: &str,
        update: crate::diff::Update,
    ) -> Result<(), String> {
        self.record(Seen::DiffUpdate(plugin.into(), id.into(), update));
        Ok(())
    }
    fn close_diff(&self, plugin: &str, id: &str) {
        self.record(Seen::DiffClose(plugin.into(), id.into()));
    }
    fn current_project(&self) -> Option<PathBuf> {
        self.projects.lock().unwrap().first().cloned()
    }

    fn open_pane(&self, plan: super::OpenPane, reply: crate::page::runtime::Reply) {
        self.record(Seen::OpenPane(plan));
        reply.ok(json!({ "paneId": "3" }).to_string());
    }

    fn open_view(&self, plan: super::OpenView, reply: crate::page::runtime::Reply) {
        self.record(Seen::OpenView(plan));
        reply.ok("null".into());
    }
    fn focus_pane(&self, pane: &str, reply: crate::page::runtime::Reply) {
        self.record(Seen::FocusPane(pane.into()));
        reply.ok("null".into());
    }
    fn close_view(&self, plugin: &str, view: &str) {
        self.record(Seen::CloseView(plugin.into(), view.into()));
    }

    fn builtin(&self, plugin: &str) -> Option<bool> {
        self.running.builtin(plugin)
    }

    fn call_tool(&self, call: ToolCall, reply: crate::page::runtime::Reply) {
        self.running.call_tool(call, reply);
    }

    fn create_worktree(
        &self,
        project: &str,
        branch: &str,
        base: Option<&str>,
    ) -> Result<Project, String> {
        let made = self
            .spaces
            .lock()
            .unwrap()
            .create_worktree(project, branch, base)?;
        Ok(made.into())
    }

    fn worktree_setup(&self, project: &str) -> Result<WorktreeSetup, String> {
        self.spaces.lock().unwrap().worktree_setup(project)
    }

    fn set_worktree_setup(
        &self,
        project: &str,
        include: Option<Vec<String>>,
        setup: Option<String>,
    ) -> Result<(), String> {
        self.spaces
            .lock()
            .unwrap()
            .set_worktree_setup(project, include, setup)
    }

    fn emit(&self, from: &str, name: &str, data: &Value) {
        self.running.emit(from, name, data);
    }

    fn secret_get(&self, plugin: &str, scope: &str, name: &str) -> Result<Option<String>, String> {
        let key = (plugin.into(), scope.into(), name.into());
        Ok(self.secrets.lock().unwrap().get(&key).cloned())
    }

    fn secret_set(&self, plugin: &str, scope: &str, name: &str, value: &str) -> Result<(), String> {
        let key = (plugin.into(), scope.into(), name.into());
        self.secrets.lock().unwrap().insert(key, value.into());
        Ok(())
    }

    fn secret_delete(&self, plugin: &str, scope: &str, name: &str) -> Result<(), String> {
        let key = (plugin.into(), scope.into(), name.into());
        self.secrets.lock().unwrap().remove(&key);
        Ok(())
    }

    fn send_to_agent(&self, pane: &str, text: &str, reply: crate::page::runtime::Reply) {
        self.record(Seen::SendToAgent(pane.into(), text.into()));
        reply.ok(json!({ "submitted": false }).to_string());
    }

    fn open_settings(&self, plugin: &str) -> Result<(), String> {
        self.record(Seen::OpenSettings(plugin.into()));
        Ok(())
    }

    fn projects(&self) -> Vec<PathBuf> {
        self.projects.lock().unwrap().clone()
    }

    fn claude_dirs(&self) -> Vec<PathBuf> {
        self.claude.lock().unwrap().clone()
    }

    fn claude_accounts(&self) -> Result<Accounts, String> {
        Ok(self
            .spaces
            .lock()
            .unwrap()
            .claude_accounts(|_| Default::default()))
    }

    fn set_claude_account(&self, project: &str, name: Option<&str>) -> Result<(), String> {
        // Not `claude_accounts::create_dir`, which would make a folder in the user's home.
        self.spaces
            .lock()
            .unwrap()
            .set_claude_account(project, name, |_| Ok(()))
    }

    fn prepare_claude_account(&self, name: &str) -> Result<(), String> {
        self.record(Seen::PreparedAccount(name.into()));
        Ok(())
    }

    fn share_claude_setup(&self, name: &str, on: bool) -> Result<(), String> {
        self.record(Seen::SharedSetup(name.into(), on));
        Ok(())
    }

    fn agent_activity(&self, session_id: &str) -> Result<Value, String> {
        self.asked.lock().unwrap().push(session_id.into());
        if !self
            .opencode
            .lock()
            .unwrap()
            .iter()
            .any(|id| id == session_id)
        {
            return Err("OpenCode doesn't know that session".into());
        }
        // What the desktop's `plugin_agent_activity` answers: a prompt, then a finished `git commit`.
        Ok(json!({
            "model": "gpt-x",
            "context": 100,
            "tokens": { "input": 100, "output": 20, "reasoning": 3, "cacheRead": 300, "cacheWrite": 7 },
            "cost": 0.25,
            "items": [
                { "kind": "prompt", "id": "m1", "at": 1000, "text": "fix the bug" },
                { "kind": "tool", "id": "c1", "at": 2000, "name": "shell", "input": { "command": "git commit -m 'Fix it'" },
                  "status": "completed", "ms": 400, "result": "1 file changed", "files": [] },
            ],
        }))
    }

    fn statusline(&self) -> Value {
        self.statusline.lock().unwrap().view()
    }
    fn ports(&self) -> Result<Vec<PanePort>, String> {
        self.ports.lock().unwrap().clone()
    }

    fn open_url(&self, url: &str) -> Result<(), String> {
        if let Some(why) = self.refuse.lock().unwrap().clone() {
            return Err(why);
        }
        self.record(Seen::Url(url.into()));
        Ok(())
    }

    fn notify(&self, notification: super::Notification) -> Result<(), String> {
        if let Some(why) = self.refuse.lock().unwrap().clone() {
            return Err(why);
        }
        self.record(Seen::Notice(notification.system_title(), notification.body));
        Ok(())
    }

    fn copy_text(&self, text: String) -> Result<(), String> {
        self.record(Seen::Copied(text));
        Ok(())
    }

    fn set_badge(&self, plugin: &str, pane: &str, badge: Option<Badge>) {
        self.record(Seen::Badge(plugin.into(), pane.into(), badge));
    }

    fn set_pane_status(&self, plugin: &str, pane: &str, status: Option<PaneStatus>) {
        self.record(Seen::Status(plugin.into(), pane.into(), status));
    }

    fn set_sidebar_label(&self, plugin: &str, page: &str, label: Option<SidebarLabel>) {
        self.record(Seen::SidebarLabel(plugin.into(), page.into(), label));
    }

    fn set_project_section(
        &self,
        plugin: &str,
        project: &str,
        section: &str,
        rows: Option<Vec<super::SectionRow>>,
    ) -> Result<(), String> {
        self.record(Seen::ProjectSection(
            plugin.into(),
            project.into(),
            section.into(),
            rows,
        ));
        Ok(())
    }

    fn set_section_sort(&self, plugin: &str, section: &str, sort: &str) {
        self.record(Seen::SectionSort(
            plugin.into(),
            section.into(),
            sort.into(),
        ));
    }
}

/// A plugin of id `probe` with what `manifest` adds (`permissions`, `contributes`, `name`).
pub(super) fn plugin(manifest: Value) -> Plugin {
    let mut whole =
        json!({ "id": "probe", "name": "Probe", "version": "1", "api": 1, "main": "main.js" });
    whole
        .as_object_mut()
        .unwrap()
        .extend(manifest.as_object().unwrap().clone());
    Plugin {
        manifest: serde_json::from_value(whole).unwrap(),
        dir: PathBuf::from("/tmp"),
        icon_svg: None,
    }
}

/// A pane, as the app lists it, in `cwd`.
pub(super) fn pane(id: &str, cwd: &str) -> PaneEntry {
    PaneEntry {
        pane_id: id.into(),
        cwd: cwd.into(),
        command: "zsh".into(),
        project: "proj".into(),
        project_id: "/proj".into(),
        session: None,
        focused: false,
    }
}

/// A host for `plugin`, in a fake app, with the panes `panes`.
pub(super) struct Rig {
    pub host: PluginHost,
    pub env: Arc<FakeEnv>,
    pub root: Root,
}

impl Rig {
    pub fn new(manifest: Value, panes: Vec<PaneEntry>) -> Self {
        Self::with(plugin(manifest), false, panes)
    }

    pub fn with(plugin: Plugin, builtin: bool, panes: Vec<PaneEntry>) -> Self {
        let root = Root::new();
        let env = FakeEnv::new();
        let config = root.config(&plugin.manifest.id);
        let host = PluginHost::with_access(
            config,
            Access {
                plugin,
                builtin,
                env: env.clone(),
            },
        );
        host.publish_panes(panes);
        Self { host, env, root }
    }

    /// A page of the plugin that makes the call `method` with `params`, and what it came to: the result, or the error.
    pub fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        Caller::start(&self.host).call(method, params)
    }

    /// The call's error, which the test expects.
    pub fn refuses(&self, method: &str, params: Value) -> String {
        match self.call(method, params) {
            Err(error) => error,
            Ok(result) => panic!("expected an error, got {result}"),
        }
    }

    /// The call's result, which the test expects.
    pub fn allows(&self, method: &str, params: Value) -> Value {
        match self.call(method, params) {
            Ok(result) => result,
            Err(error) => panic!("expected a result, got the error {error}"),
        }
    }
}

/// A page of the plugin that makes the calls it is asked to, and hands the answers to the test.
pub(super) struct Caller {
    frame: Frame,
    answers: std::sync::mpsc::Receiver<Heard>,
}

enum Heard {
    Line(String),
    Done(Result<Value, String>),
}

/// A frame that calls `first` when it starts, if it has one, and a `call` event's method after, and tells `heard` what comes back.
struct Calls {
    first: Option<(String, Value)>,
    heard: std::sync::mpsc::Sender<Heard>,
}

impl Guest for Calls {
    fn start(&mut self, cx: &mut Cx<'_>) {
        if let Some((method, params)) = self.first.take() {
            cx.call(&method, params);
        }
    }

    fn event(&mut self, cx: &mut Cx<'_>, handler: &str, payload: &Value) {
        if handler == "call" {
            cx.call(payload["method"].as_str().unwrap(), payload["params"].clone());
        }
    }

    fn deliver(&mut self, _cx: &mut Cx<'_>, kind: &str, _id: u32, text: &str) {
        let _ = self.heard.send(match kind {
            "resolve" => Heard::Done(Ok(serde_json::from_str(text).unwrap())),
            "reject" => Heard::Done(Err(text.into())),
            "output" => Heard::Line(text.into()),
            _ => return,
        });
    }
}

fn calls(first: Option<(String, Value)>) -> (String, std::sync::mpsc::Receiver<Heard>) {
    let (heard, answers) = std::sync::mpsc::channel();
    let source = fake::source("caller", move || {
        Box::new(Calls { first: first.clone(), heard: heard.clone() })
    });
    (source, answers)
}

impl Caller {
    pub fn start(host: &PluginHost) -> Self {
        let (source, answers) = calls(None);
        Self { frame: Frame::start(host, &source), answers }
    }

    /// The call `method`, and what it came to.
    pub fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        self.streamed(method, params).0
    }

    /// The call, and the lines it printed while it ran.
    pub fn streamed(&self, method: &str, params: Value) -> (Result<Value, String>, Vec<String>) {
        assert!(self.frame._runtime.dispatch("call", Some(&json!({ "method": method, "params": params }))));
        self.finish()
    }

    /// The call, made right after a click of yours, as a page may copy.
    pub fn call_after_click(&self, method: &str, params: Value) -> Result<Value, String> {
        assert!(self.frame._runtime.dispatch_gesture("call", Some(&json!({ "method": method, "params": params }))));
        self.finish().0
    }

    fn finish(&self) -> (Result<Value, String>, Vec<String>) {
        let mut lines = Vec::new();
        loop {
            match self.answers.recv_timeout(WAIT) {
                Ok(Heard::Line(line)) => lines.push(line),
                Ok(Heard::Done(result)) => return (result, lines),
                Err(_) => panic!("waiting for the call to finish"),
            }
        }
    }
}

/// The call `method`, made by the plugin's main, which is the only frame some calls are for.
pub(super) fn call_from_main(host: &PluginHost, method: &str, params: Value) -> Result<Value, String> {
    let (source, answers) = calls(Some((method.into(), params)));
    host.start_main(&source, |_| {}).unwrap();
    first_answer(&answers)
}

/// The call `method`, made by a page of the plugin as it starts, before anything of yours.
pub(super) fn call_at_start(host: &PluginHost, method: &str, params: Value) -> Result<Value, String> {
    let (source, answers) = calls(Some((method.into(), params)));
    let _page = Frame::start(host, &source);
    first_answer(&answers)
}

fn first_answer(answers: &std::sync::mpsc::Receiver<Heard>) -> Result<Value, String> {
    match answers.recv_timeout(WAIT) {
        Ok(Heard::Done(result)) => result,
        _ => panic!("waiting for the first call to finish"),
    }
}

/// What `wings.exec(program, args)` sends the host.
pub(super) fn exec(program: &str, args: &[&str]) -> Value {
    json!({ "program": program, "args": args, "cwd": null, "timeoutMs": null, "stream": false })
}

/// What `wings.openPane(options)` sends the host: the defaults it fills in, and `extra` over them.
pub(super) fn open_pane(extra: Value) -> Value {
    let mut params = json!({ "command": null, "args": [], "cwd": null, "placement": "tab" });
    params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    params
}

/// A page that broadcasts as it starts, which is how a test waits for the frames it started before to have heard what was sent.
pub(super) fn barrier() -> String {
    struct Barrier;
    impl Guest for Barrier {
        fn start(&mut self, cx: &mut Cx<'_>) {
            cx.link().broadcast("{}".into()).unwrap();
        }
    }
    fake::source("barrier", || Box::new(Barrier))
}

pub(super) use crate::page::runtime::fake::idle;

/// A page that shows `{ heard }`, every message of the kind `kind` the host told it, as JSON, from the start.
pub(super) fn hears(kind: &'static str) -> String {
    struct Hears(&'static str, Vec<Value>);
    impl Guest for Hears {
        fn start(&mut self, cx: &mut Cx<'_>) {
            show(cx, json!({ "heard": self.1 }));
        }
        fn deliver(&mut self, cx: &mut Cx<'_>, kind: &str, _id: u32, text: &str) {
            if kind == self.0 {
                self.1.push(serde_json::from_str(text).unwrap());
                show(cx, json!({ "heard": self.1 }));
            }
        }
    }
    fake::source("hears", move || Box::new(Hears(kind, Vec::new())))
}

/// A page that shows each message another frame broadcasts, as JSON.
pub(super) fn echoes() -> String {
    struct Echoes;
    impl Guest for Echoes {
        fn deliver(&mut self, cx: &mut Cx<'_>, kind: &str, _id: u32, text: &str) {
            if kind == "broadcast" {
                show(cx, serde_json::from_str(text).unwrap());
            }
        }
    }
    fake::source("echoes", || Box::new(Echoes))
}

/// Renders `value` for a test to read with `Frame::report`.
pub(super) fn show(cx: &mut Cx<'_>, value: Value) {
    cx.render(&json!({ "id": 0, "type": "#root", "props": {}, "children": [
        { "id": 1, "type": "#text", "text": value.to_string() }] }));
}

/// A page of the plugin and what it has shown.
pub(super) struct Frame {
    /// The page runs while this is held.
    _runtime: PageRuntime,
    events: std::sync::mpsc::Receiver<Event>,
}

impl Frame {
    pub fn in_window(host: &PluginHost, window: u64, source: &str) -> Self {
        let (sender, events) = std::sync::mpsc::channel();
        let runtime = host
            .open_window_page(source, window, move |event| {
                let _ = sender.send(event);
            })
            .unwrap();
        Self {
            _runtime: runtime,
            events,
        }
    }
    pub fn start(host: &PluginHost, source: &str) -> Self {
        Self::with_visibility(host, source, true)
    }

    pub fn hidden(host: &PluginHost, source: &str) -> Self {
        Self::with_visibility(host, source, false)
    }

    pub fn set_visible(&self, visible: bool) {
        self._runtime.set_visible(visible);
    }

    fn with_visibility(host: &PluginHost, source: &str, visible: bool) -> Self {
        let (sender, events) = std::sync::mpsc::channel();
        let report = move |event| {
            let _ = sender.send(event);
        };
        let runtime = if visible {
            host.open_page(source, report)
        } else {
            host.open_hidden_page(source, report)
        }
        .unwrap();
        Self {
            _runtime: runtime,
            events,
        }
    }

    /// The JSON the page shows, once `done` likes it. A page that errors fails the test.
    pub fn report(&self, what: &str, done: impl Fn(&Value) -> bool) -> Value {
        let mut last = String::new();
        loop {
            match self.events.recv_timeout(WAIT) {
                Ok(Event::Tree(tree)) => {
                    let text = tree.text();
                    if let Ok(report) = serde_json::from_str::<Value>(&text)
                        && done(&report)
                    {
                        return report;
                    }
                    last = text;
                }
                Ok(Event::Error { message, .. }) => panic!("waiting for {what}: {message}"),
                Err(_) => panic!("waiting for {what}: the page last showed {last:?}"),
            }
        }
    }
}
