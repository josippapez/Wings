//! Fakes for the host's tests: an app that records what plugins did to it, and frames that run a snippet of `wings` code
//! and report what it came to.

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
    ui::{Badge, PaneStatus},
};
use crate::{
    page::runtime::{Event, PageRuntime},
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
    CloseView(String, String),
    Badge(String, String, Option<Badge>),
    Status(String, String, Option<PaneStatus>),
    Url(String),
    Notice(String, String),
}

pub(super) struct FakeEnv {
    pub projects: Mutex<Vec<PathBuf>>,
    /// The list `createWorktree` and `worktreeSetup` work on, which tests replace with one of their own.
    pub spaces: Mutex<Projects>,
    /// The folders of the Claude Code accounts, which `transcript` reads.
    pub claude: Mutex<Vec<PathBuf>>,
    pub ports: Mutex<Result<Vec<PanePort>, String>>,
    /// Why opening a URL or showing a notification fails, if it should.
    pub refuse: Mutex<Option<String>>,
    /// The other plugins running in the app, which `callTool` reaches.
    pub running: Running,
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
            refuse: Mutex::default(),
            running: Running::default(),
            log: Mutex::default(),
            changed: Condvar::new(),
        })
    }

    fn record(&self, seen: Seen) {
        self.log.lock().unwrap().push(seen);
        self.changed.notify_all();
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

    fn projects(&self) -> Vec<PathBuf> {
        self.projects.lock().unwrap().clone()
    }

    fn claude_dirs(&self) -> Vec<PathBuf> {
        self.claude.lock().unwrap().clone()
    }

    fn claude_accounts(&self) -> Result<Accounts, String> {
        Ok(self.spaces.lock().unwrap().claude_accounts())
    }

    fn set_claude_account(&self, project: &str, name: Option<&str>) -> Result<(), String> {
        // Not `claude_accounts::create_dir`, which would make a folder in the user's home.
        self.spaces
            .lock()
            .unwrap()
            .set_claude_account(project, name, |_| Ok(()))
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

    fn notify(&self, title: &str, body: &str) -> Result<(), String> {
        if let Some(why) = self.refuse.lock().unwrap().clone() {
            return Err(why);
        }
        self.record(Seen::Notice(title.into(), body.into()));
        Ok(())
    }

    fn set_badge(&self, plugin: &str, pane: &str, badge: Option<Badge>) {
        self.record(Seen::Badge(plugin.into(), pane.into(), badge));
    }

    fn set_pane_status(&self, plugin: &str, pane: &str, status: Option<PaneStatus>) {
        self.record(Seen::Status(plugin.into(), pane.into(), status));
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

    /// Runs `body` as an async function in a page of the plugin and returns `{ ok }` or `{ err }`: what it returned,
    /// or the message it was rejected with.
    pub fn run(&self, body: &str) -> Value {
        let frame = Frame::start(&self.host, &script(body));
        frame.report("the call to finish", |report| {
            report.get("ok").is_some() || report.get("err").is_some()
        })
    }

    /// The call's error, which the test expects.
    pub fn refused(&self, body: &str) -> String {
        match self.run(body) {
            Value::Object(report) => match report.get("err") {
                Some(Value::String(err)) => err.clone(),
                _ => panic!("expected an error, got {report:?}"),
            },
            other => panic!("{other}"),
        }
    }

    /// The call's result, which the test expects.
    pub fn allowed(&self, body: &str) -> Value {
        let report = self.run(body);
        match report.get("ok") {
            Some(ok) => ok.clone(),
            None => panic!("expected a result, got {report}"),
        }
    }
}

/// The shell of a probe page: `show` renders any JSON, and `attempt` shows what an async function returns.
fn script(body: &str) -> String {
    format!("{PRELUDE}\nattempt(async () => {{ {body} }});")
}

pub(super) const PRELUDE: &str = r#"
const show = (value) => wings.render(JSON.stringify({ id: 0, type: '#root', props: {}, children: [
  { id: 1, type: '#text', text: JSON.stringify(value) }] }));
const attempt = async (work) => {
  try { show({ ok: (await work()) ?? null }); } catch (error) { show({ err: String(error?.message ?? error) }); }
};
"#;

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
