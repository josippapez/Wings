//! Fakes for the host's tests: an app that records what plugins did to it, and frames that run a snippet of `wings` code
//! and report what it came to.

use std::{
    path::PathBuf,
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

use serde_json::{Value, json};
use wings_plugins::manifest::Plugin;

use super::{
    Access, Env, PaneEntry, PanePort, PluginHost,
    tests::Root,
    ui::{Badge, PaneStatus},
};
use crate::page::runtime::{Event, PageRuntime};

pub(super) const WAIT: Duration = Duration::from_secs(10);

/// What a plugin did to the app, in order.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Seen {
    Badge(String, String, Option<Badge>),
    Status(String, String, Option<PaneStatus>),
    Url(String),
    Notice(String, String),
}

pub(super) struct FakeEnv {
    pub projects: Mutex<Vec<PathBuf>>,
    pub claude: Mutex<PathBuf>,
    pub ports: Mutex<Result<Vec<PanePort>, String>>,
    /// Why opening a URL or showing a notification fails, if it should.
    pub refuse: Mutex<Option<String>>,
    log: Mutex<Vec<Seen>>,
    changed: Condvar,
}

impl FakeEnv {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            projects: Mutex::default(),
            claude: Mutex::default(),
            ports: Mutex::new(Ok(Vec::new())),
            refuse: Mutex::default(),
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
    fn projects(&self) -> Vec<PathBuf> {
        self.projects.lock().unwrap().clone()
    }

    fn claude_dir(&self) -> PathBuf {
        self.claude.lock().unwrap().clone()
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
    pub fn start(host: &PluginHost, source: &str) -> Self {
        let (sender, events) = std::sync::mpsc::channel();
        let runtime = host
            .open_page(source, move |event| {
                let _ = sender.send(event);
            })
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
