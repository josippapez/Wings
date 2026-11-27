//! What a plugin's frames can ask of the host with `wings.*`: the desktop's `handle` in `apps/desktop/src/lib/plugins.ts`
//! and the `plugin_*` commands in `apps/desktop/src-tauri/src/lib.rs`, with their argument rules, limits and messages.
//! What a call may do is `wings-plugins`' to say (`permissions`, `exec`, `fetch::may_open_url`, `transcript`), and
//! nothing here decides it: this reads the manifest's grant with those functions and refuses with the desktop's words.
//!
//! A call that only moves data answers on the frame's thread. One that reads the disk or runs a program runs on a thread
//! of its own and answers when it is done, as the desktop's async commands do, so a slow call holds up no one.

use std::{path::PathBuf, sync::{Arc, Mutex}, thread, time::Instant};

use serde::Deserialize;
use serde_json::{Value, json};
use wings_plugins::{
    exec,
    fetch::may_open_url,
    manifest::Plugin,
    panes::{exec_cwd, worktrees_call_in_any_repo},
    permissions::{
        NOTIFY_LIMIT, may_call_tool, may_create_worktrees, may_open_view, may_read_pane_output, may_read_ports,
        may_read_statusline, may_use_claude_accounts, notification,
    },
    transcript::agent_entries,
};
use wings_spaces::claude_accounts::{AccountPick, valid_name};

use super::{
    Shared,
    account::AccountRequest,
    env::{Access, ToolCall},
    ui::{clean_badge, clean_pane_status, clean_sidebar_label},
};
use crate::page::runtime::Reply;

/// The most a page may put on the clipboard at once, in UTF-8 bytes: far more than a branch name or a code block.
const COPY_MAX: usize = 16 * 1024 * 1024;
/// The most threads a plugin's calls run on at once (`Shared::off_thread`). Each call had a thread of its own, so a
/// frame calling in a loop started thousands, past the 4,096 a macOS process may have, and the app's own then failed.
/// The busiest plugins ask about each project at once (`exec` of `gh` or `git`), tens at a time.
const CALL_THREADS: usize = 64;
/// The most calls that wait for one of those threads. More are refused at once.
const CALLS_QUEUED: usize = 1024;

type Job = Box<dyn FnOnce() + Send>;

/// The threads a plugin's calls run on, and the calls waiting for one, oldest first.
#[derive(Default)]
pub(super) struct Workers {
    running: usize,
    queued: std::collections::VecDeque<Job>,
}

/// Runs `job`, then each call waiting, until none waits.
fn work_on(pool: &Mutex<Workers>, mut job: Job) {
    loop {
        // A call that panics loses its answer, as on a thread of its own, and not the thread the others wait for.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
        let mut workers = pool.lock().unwrap();
        match workers.queued.pop_front() {
            Some(next) => job = next,
            None => {
                workers.running -= 1;
                return;
            }
        }
    }
}
/// The lines `readPane` reads when it isn't told, and the most lines and bytes it answers, so a frame's turn stays small.
const READ_PANE_LINES: u64 = 100;
const READ_PANE_LINES_MAX: u64 = 500;
const READ_PANE_BYTES_MAX: usize = 64 * 1024;

/// The text JS makes of a value with `String(value)`, for the arguments the desktop turns into strings.
fn js_string(value: Option<&Value>) -> String {
    match value {
        None => "undefined".into(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) => "null".into(),
        Some(Value::Number(number)) => match number.as_f64() {
            Some(float) if float.fract() == 0.0 && float.abs() < 1e21 => format!("{float:.0}"),
            _ => number.to_string(),
        },
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item {
                Value::Null => String::new(),
                item => js_string(Some(item)),
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".into(),
        Some(Value::Bool(flag)) => flag.to_string(),
    }
}

/// A plugin may draw a kind of UI only when its manifest lists it in `contributes.ui`.
pub(super) fn needs(plugin: &Plugin, kind: &str) -> Result<(), String> {
    if plugin.manifest.contributes.ui.iter().any(|ui| ui == kind) {
        Ok(())
    } else {
        Err(format!(
            "{} must declare \"{kind}\" in contributes.ui",
            plugin.manifest.id
        ))
    }
}

/// The ids of a plugin's sidebars, then its views: what a status may `open`.
fn pages(plugin: &Plugin) -> Vec<String> {
    let contributes = &plugin.manifest.contributes;
    let sidebars = contributes.sidebars.iter().map(|page| page.id.clone());
    sidebars
        .chain(contributes.views.iter().map(|view| view.id.clone()))
        .collect()
}

fn to_json(value: &impl serde::Serialize) -> Result<String, String> {
    serde_json::to_string(value).map_err(|error| error.to_string())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecRequest {
    program: String,
    args: Vec<String>,
    cwd: Option<String>,
    timeout_ms: Option<f64>,
    /// Send each output line to the frame while the program runs.
    stream: bool,
    /// Runs under a Claude Code account: see `account`.
    #[serde(default)]
    account: Option<AccountRequest>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TranscriptRequest {
    session_id: String,
    types: Vec<String>,
    last: Option<f64>,
    /// `"register"`: the pictures in the entries are kept by the app, and the entries name their ids.
    images: Option<String>,
    /// A subagent of the session, by Claude Code's agent id: its own transcript, in the session's folder.
    agent: Option<String>,
    /// Each string of an entry cut to this many characters, which lets `last` ask for more entries.
    cut: Option<f64>,
}

/// What the desktop's wrapper makes of an `exec`'s `timeoutMs`: a number of milliseconds, whole and not negative. The
/// desktop's command takes a `u64`, so anything else never reaches the runner.
fn timeout_ms(value: Option<f64>) -> Result<Option<u64>, String> {
    match value {
        Some(ms) if ms.is_finite() && ms >= 0.0 && ms.fract() == 0.0 && ms < u64::MAX as f64 => {
            Ok(Some(ms as u64))
        }
        Some(ms) if ms.is_finite() => Err(format!(
            "exec timeoutMs must be a whole number of milliseconds, not {ms}"
        )),
        _ => Ok(None),
    }
}

impl Shared {
    /// `openPane` from the frame `tab`, when that frame's page is shown as a tab: `"replace"` puts the new tab in that tab's
    /// place.
    fn open_pane(&self, access: &Access, params: &Value, tab: Option<u64>, reply: Reply) -> Result<(), String> {
        const PLACEMENTS: &str = "openPane placement is \"tab\", \"right\", \"down\" or \"replace\"";
        let mut placement = match params.get("placement") {
            None | Some(Value::Null) => "tab",
            Some(Value::String(value)) => value,
            _ => return Err(PLACEMENTS.into()),
        };
        if !["tab", "right", "down", "replace"].contains(&placement) {
            return Err(PLACEMENTS.into());
        }
        let mut replace = None;
        if placement == "replace" {
            let frame = tab.ok_or("openPane placement \"replace\" works only from a page shown as a tab")?;
            replace = Some((access.plugin.manifest.id.clone(), frame));
            placement = "tab";
        }
        // In its project without showing it: the project on screen, its tab and the keyboard stay as they are.
        match params.get("background") {
            None | Some(Value::Null | Value::Bool(false)) => {}
            Some(Value::Bool(true)) if placement == "tab" && replace.is_none() => placement = "background",
            Some(Value::Bool(true)) => return Err("openPane background only works with placement \"tab\"".into()),
            _ => return Err("openPane background must be true or false".into()),
        }
        let text = |key: &str| match params.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(value)) => Ok(Some(value.as_str())),
            _ => Err(format!("openPane {key} must be a string")),
        };
        let args: Vec<String> = match params.get("args") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(values)) => values
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or("openPane args must be a list of strings".to_owned())
                })
                .collect::<Result<_, _>>()?,
            _ => return Err("openPane args must be a list of strings".into()),
        };
        let input = wings_plugins::panes::pane_input(&access.plugin, text("command")?, &args)?;
        // Absent is the project's own account, null the default account, a string a named one.
        let claude_account = match params.get("claudeAccount") {
            None => None,
            Some(Value::Null) => Some(AccountPick { name: None }),
            Some(Value::String(name)) => Some(AccountPick {
                name: Some(name.clone()),
            }),
            Some(_) => {
                return Err(
                    "openPane claudeAccount is an account name, or null for the default account"
                        .into(),
                );
            }
        };
        if let Some(pick) = &claude_account {
            may_use_accounts(access)?;
            if let Some(name) = &pick.name {
                account_name(name)?;
            }
        }
        let projects = access.env.projects();
        let current_path = access.env.current_project();
        let current = current_path
            .as_ref()
            .and_then(|path| projects.iter().position(|root| root == path));
        let (index, cwd) = match text("cwd")? {
            Some(cwd) => {
                wings_plugins::panes::pane_cwd(std::path::Path::new(cwd), &projects, current)?
            }
            None => {
                let index = current.ok_or("No project is open in Wings")?;
                (index, projects[index].clone())
            }
        };
        let name = claude_account.as_ref().and_then(|pick| pick.name.clone());
        let plan = super::OpenPane {
            cwd,
            project: projects[index].clone(),
            input: input.map(|line| line + "\r"),
            placement: placement.into(),
            claude_account,
            replace,
        };
        let Some(name) = name else {
            access.env.open_pane(plan, reply);
            return Ok(());
        };
        // A named account has its folder, the default account's setup and Wings by the time its terminal starts, as
        // when a project is assigned to it. That may run `claude`, so not on the frame's thread.
        let env = access.env.clone();
        thread::Builder::new()
            .name("plugin-call".into())
            .spawn(move || match env.prepare_claude_account(&name) {
                Ok(()) => env.open_pane(plan, reply),
                Err(error) => reply.err(error),
            })
            .map(|_| ())
            .map_err(|error| format!("The call couldn't start: {error}"))
    }

    /// `openView` from a frame, which opens a new tab right after the tab of the frame `after` when it is given.
    /// `openView`; `tab_project` is the project of the tab whose page asks, when a tab's page asks.
    pub(super) fn open_view_call(&self, params: Value, after: Option<u64>, tab_project: Option<std::path::PathBuf>, reply: Reply) {
        let Some(access) = self.access.clone() else {
            return reply.err(format!("{} is turned off or not installed", self.config.id));
        };
        reply_if_error(reply.clone(), self.open_view(&access, &params, after, tab_project, reply));
    }

    /// `openPane` and `openHistory`, which can act on the tab of the page that asks: `tab` is its frame when its page is
    /// shown as a tab.
    pub(super) fn tab_call(&self, method: &str, params: Value, tab: Option<u64>, reply: Reply) {
        let Some(access) = self.access.clone() else {
            return reply.err(format!("{} is turned off or not installed", self.config.id));
        };
        let called = match method {
            "openPane" => self.open_pane(&access, &params, tab, reply.clone()),
            _ => open_history(&access, &params, tab, reply.clone()),
        };
        reply_if_error(reply, called);
    }

    fn open_view(&self, access: &Access, params: &Value, after: Option<u64>, tab_project: Option<std::path::PathBuf>, reply: Reply) -> Result<(), String> {
        let id = js_string(params.get("id"));
        let contributes = &access.plugin.manifest.contributes;
        // A view of another plugin: only a tab, only when the manifest names `<plugin> <view>` and that plugin is one of
        // Wings' own and on. Its sidebars and its other places stay its own.
        let other = match params.get("plugin") {
            None | Some(Value::Null) => None,
            Some(plugin) => Some(js_string(Some(plugin))).filter(|plugin| *plugin != access.plugin.manifest.id),
        };
        let owner = other.clone().unwrap_or_else(|| access.plugin.manifest.id.clone());
        let opens = if let Some(target) = &other {
            let caller = &access.plugin.manifest.id;
            if !access.builtin || access.env.builtin(target) != Some(true) {
                return Err("Only Wings' own plugins can open each other's views".into());
            }
            if !may_open_view(&access.plugin, target, &id) {
                return Err(format!("{caller} may not open {target}'s {id}"));
            }
            match access.env.view_opens(target, &id) {
                Some(opens) if opens.iter().any(|place| place == "tab") => vec!["tab".to_owned()],
                _ => return Err(format!("{target} has no view {id} that opens as a tab")),
            }
        } else if contributes.sidebars.iter().any(|page| page.id == id) {
            vec!["sidebar".to_owned()]
        } else if let Some(view) = contributes.views.iter().find(|view| view.id == id) {
            view.opens.clone()
        } else {
            return Err(format!("{} has no view {id}", access.plugin.manifest.id));
        };
        if other.is_some() && after.is_some() {
            return Err("openView after only works for the plugin's own views".into());
        }
        let place = params
            .get("as")
            .filter(|v| !v.is_null())
            .map(|v| js_string(Some(v)))
            .unwrap_or_else(|| opens[0].clone());
        if !opens.contains(&place) {
            return Err(format!("{id} opens as {}, not {place}", opens.join(" or ")));
        }
        let args = params.get("args").filter(|v| !v.is_null()).cloned();
        if args.is_some() && place != "tab" && place != "window" {
            return Err("openView args only work with as: \"tab\" or \"window\"".into());
        }
        if args
            .as_ref()
            .is_some_and(|args| args.to_string().encode_utf16().count() > 16 * 1024)
        {
            return Err("openView args is limited to 16 KB".into());
        }
        let new_tab = match params.get("newTab") {
            None => false,
            Some(Value::Bool(value)) => *value,
            _ => return Err("openView newTab must be true or false".into()),
        };
        if new_tab && place != "tab" {
            return Err("openView newTab only works with as: \"tab\"".into());
        }
        if after.is_some() && place != "tab" {
            return Err("openView after only works with as: \"tab\"".into());
        }
        // A window may ask for its size, an agent checking a page at a phone's or a tablet's width say.
        let points = |key: &str| params.get(key).filter(|value| !value.is_null()).map(|value| value.as_f64().filter(|points| points.is_finite() && *points > 0.0));
        let size = match (points("width"), points("height")) {
            (None, None) => None,
            (Some(Some(width)), Some(Some(height))) if place == "window" => Some((width, height)),
            _ => return Err("openView width and height are points, both, with as: \"window\"".into()),
        };
        // The project the tab opens in is one of Wings' projects, by its id, as `pastSessions`'s.
        let project = match params.get("projectId").filter(|value| !value.is_null()) {
            None => None,
            Some(project) => {
                if place != "tab" || !new_tab || after.is_some() {
                    return Err("openView projectId only works with as: \"tab\" and newTab: true, and not after".into());
                }
                let project = js_string(Some(project));
                let known = self.projects.lock().unwrap();
                let found = known.iter().find(|known| known.id == project).ok_or_else(|| format!("no project {project}"))?;
                Some(std::path::PathBuf::from(&found.path))
            }
        };
        let tab_project = tab_project.filter(|_| place == "sidebar");
        access.env.open_view(
            super::OpenView {
                plugin: owner,
                id,
                place,
                args,
                size,
                new_tab,
                project,
                after,
                tab_project,
            },
            reply,
        );
        Ok(())
    }

    /// Runs a tool of another of Wings' own plugins: the desktop's `callTool` and `plugin_call_tool_check`.
    fn call_tool(&self, access: &Access, params: &Value, reply: Reply) -> Result<(), String> {
        let name = match params.get("name") {
            None | Some(Value::Null) => String::new(),
            name => js_string(name),
        };
        let mut parts = name.split("__");
        let (Some(target), Some(tool)) = (
            parts.next().filter(|part| !part.is_empty()),
            parts.next().filter(|part| !part.is_empty()),
        ) else {
            return Err(r#"callTool takes a name like "tasks__list""#.into());
        };
        let input = match params.get("arguments") {
            Some(arguments @ Value::Object(_)) => arguments.clone(),
            _ => json!({}),
        };
        let caller = &access.plugin.manifest.id;
        if !access.builtin || access.env.builtin(target) == Some(false) {
            return Err("Only Wings' own plugins can call each other's tools".into());
        }
        if !may_call_tool(&access.plugin, target, tool) {
            return Err(format!("{caller} may not call {target}'s {tool}"));
        }
        let projects = self.projects.lock().unwrap();
        let project = match params.get("project").filter(|value| !value.is_null()) {
            Some(id) => {
                let id = js_string(Some(id));
                Some(
                    projects
                        .iter()
                        .find(|project| project.id == id)
                        .ok_or_else(|| format!("no project {id}"))?,
                )
            }
            None => access.env.current_project().and_then(|path| {
                projects
                    .iter()
                    .find(|project| std::path::Path::new(&project.path) == path)
            }),
        }
        .cloned();
        drop(projects);
        access.env.call_tool(
            ToolCall {
                target: target.to_owned(),
                tool: tool.to_owned(),
                input,
                project,
            },
            reply,
        );
        Ok(())
    }

    /// `project` for a page shown as a tab of the project at `path`: that project, not the one on screen.
    pub(super) fn project_of(&self, path: std::path::PathBuf, reply: Reply) {
        if self.access.is_none() {
            return reply.err(format!("{} is turned off or not installed", self.config.id));
        }
        reply.done(self.project_at(Some(path)));
    }

    /// The project on screen, as the desktop's `project` call gives the project a page is about.
    fn project(&self, access: &Access) -> Result<String, String> {
        self.project_at(access.env.current_project())
    }

    fn project_at(&self, at: Option<std::path::PathBuf>) -> Result<String, String> {
        let projects = self.projects.lock().unwrap();
        to_json(&at.and_then(|path| {
            projects
                .iter()
                .find(|project| std::path::Path::new(&project.path) == path)
        }))
    }

    /// `closeView`: the plugin's sidebar or view, closed wherever the app shows it.
    fn close_view(&self, access: &Access, params: &Value) -> Result<String, String> {
        let id = js_string(params.get("id"));
        let plugin = &access.plugin.manifest.id;
        if !pages(&access.plugin).contains(&id) {
            return Err(format!("{plugin} has no view {id}"));
        }
        access.env.close_view(plugin, &id);
        Ok("null".into())
    }

    /// A frame's call of `wings.<method>`, answered through `reply`.
    pub(super) fn call(self: &Arc<Self>, method: &str, params: Value, reply: Reply) {
        let Some(access) = self.access.clone() else {
            // Nothing runs a plugin that isn't on and approved, so a host without a manifest refuses every call.
            return reply.err(format!("{} is turned off or not installed", self.config.id));
        };
        let this = self.clone();
        match method {
            "focusPane" | "closePane" => {
                let pane = js_string(params.get("paneId"));
                let verb = if method == "focusPane" { "focus" } else { "close" };
                if !wings_plugins::permissions::may_use_panes(&access.plugin) {
                    return reply.err(format!("{} may not {verb} panes", self.config.id));
                }
                if let Err(error) = self.require_pane(&pane) {
                    return reply.err(error);
                }
                // Closing ends what runs there, so only as the answer to your click on that pane's badge, once.
                let clicked = || {
                    let mut actions = self.actions.lock().unwrap();
                    let token = actions.panes.iter().find(|(_, clicked)| **clicked == pane).map(|(token, _)| *token);
                    token.and_then(|token| actions.panes.remove(&token)).is_some()
                };
                match method {
                    "focusPane" => access.env.focus_pane(&pane, reply),
                    _ if clicked() => access.env.close_pane(&pane, reply),
                    _ => reply.err(format!(
                        "{} may close a pane only while it answers your click on that pane's badge",
                        self.config.id
                    )),
                }
            }
            "projects" => reply.done(to_json(&*self.projects.lock().unwrap())),
            "disks" | "diskFind" | "diskUsage" | "diskUsageCancel" => {
                self.disk_call(method, params, reply, access)
            }
            "openView" => reply_if_error(reply.clone(), self.open_view(&access, &params, None, None, reply)),
            "project" => reply.done(self.project(&access)),
            "closeView" => reply.done(self.close_view(&access, &params)),
            "callTool" => reply_if_error(reply.clone(), self.call_tool(&access, &params, reply)),
            "toolDone" => {
                let token = js_string(params.get("token")).parse().unwrap_or(0);
                let text = params.get("text").and_then(Value::as_str).unwrap_or("");
                let is_error = params.get("isError") == Some(&Value::Bool(true));
                let image = params.get("image").and_then(Value::as_str);
                self.settle_tool(token, text, is_error, image);
                reply.ok("null".into());
            }
            "createWorktree" => {
                self.off_thread(reply, move |_| create_worktree(&access, &params));
            }
            "worktreeSetup" => reply.done(worktree_setup(&access, &params)),
            "setWorktreeSetup" => reply.done(set_worktree_setup(&access, &params)),
            "claudeAccounts" => {
                self.off_thread(reply, move |_| claude_accounts(&access));
            }
            "claudeAccountChoices" => {
                self.off_thread(reply, move |_| claude_account_choices(&access, &params));
            }
            "setClaudeAccount" => {
                self.off_thread(reply, move |_| set_claude_account(&access, &params));
            }
            "shareClaudeSetup" => {
                self.off_thread(reply, move |_| share_claude_setup(&access, &params));
            }
            "setSidebarLabel" => reply.done(set_sidebar_label(&access, &params)),
            "setLinkWords" => reply.done(set_link_words(&access, &params)),
            "fetch" | "secretSet" | "secretDelete" | "secretHas" | "post" | "sendToAgent"
            | "openSettings" | "emit" => self.forge_call(method, params, reply, access),
            "setProjectSection" => reply.done(self.set_project_section(&access, &params)),
            "setSectionSort" => reply.done(self.set_section_sort(&access, &params)),
            "setBadge" => reply.done(self.set_badge(&access, &params)),
            "setPaneStatus" => reply.done(self.set_pane_status(&access, &params)),
            "actionDone" => {
                let token = js_string(params.get("token")).parse().unwrap_or(0);
                let error = params
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                self.settle_action(token, error);
                reply.ok("null".into());
            }
            "statusline" => reply.done(statusline(&access)),
            "reportUsage" => reply.done(report_usage(&access, &params)),
            "exec" => {
                self.off_thread(reply, move |reply| this.exec(&access, &params, reply));
            }
            "pastSessions" => self.off_thread(reply, move |_| this.past_sessions(&access, &params)),
            "transcript" => {
                self.off_thread(reply, move |_| transcript(&access, &params));
            }
            "agentActivity" => {
                self.off_thread(reply, move |_| {
                    session_detail::agent_activity(&access, &params)
                });
            }
            "ports" => self.off_thread(reply, move |_| ports(&access)),
            "readPane" => match self.pane_to_read(&access, &params) {
                Ok((pane, lines)) => self.off_thread(reply, move |_| read_pane(&access, &pane, lines)),
                Err(error) => reply.err(error),
            },
            "openUrl" => self.off_thread(reply, move |_| open_url(&access, &params)),
            // Right after your click or key, which the frame has checked, as a link in a page is clicked.
            "openFile" => self.off_thread(reply, move |_| open_file(&access, &params)),
            "openLink" => self.off_thread(reply, move |_| open_link(&access, &params)),
            "notify" => {
                self.off_thread(reply, move |_| this.notify(&access, &params));
            }
            // Any page may copy right after your click or key, as a desktop page's frame may use the clipboard API;
            // the frame has checked that (`FrameLink::call`).
            // Any page may keep a picture for its `Image` nodes: the app's store of them is bounded (`page::images`).
            "registerImage" => reply.done(pictures::register_image(&self.config.id, &params)),
            "copyText" => {
                let text = js_string(params.get("text"));
                if text.len() > COPY_MAX {
                    return reply.err("copyText is limited to 16 MB of text".into());
                }
                reply.done(access.env.copy_text(text).map(|_| "null".into()))
            }
            // Any page may open the file picker right after your click or key; the frame has checked that. Only for
            // pictures, given as a paste gives them.
            "pickFiles" => {
                if params.get("kinds") != Some(&json!(["image"])) {
                    return reply.err(r#"pickFiles takes kinds: ["image"]"#.into());
                }
                let multiple = params.get("multiple") == Some(&Value::Bool(true));
                access.env.pick_files(multiple, reply)
            }
            "webData" | "webExtensions" | "webEngine" | "webFolders" | "browserCookies" => self.web_call(access, method, params, reply),
            _ => reply.err(format!("unknown method {method}")),
        }
    }

    /// Runs `work` off the frame's thread and answers with what it returns: on a thread of the plugin's, of which there
    /// are at most `CALL_THREADS`, or after the calls waiting for one. More than `CALLS_QUEUED` waiting are refused.
    pub(super) fn off_thread(
        &self,
        reply: Reply,
        work: impl FnOnce(&Reply) -> Result<String, String> + Send + 'static,
    ) {
        let spare = reply.clone();
        let job: Job = Box::new(move || {
            let result = work(&reply);
            reply.done(result);
        });
        let mut workers = self.workers.lock().unwrap();
        if workers.running == CALL_THREADS {
            if workers.queued.len() == CALLS_QUEUED {
                drop(workers);
                return spare.err(format!("{} has {CALLS_QUEUED} calls waiting already", self.config.id));
            }
            return workers.queued.push_back(job);
        }
        let pool = self.workers.clone();
        match thread::Builder::new().name("plugin-call".into()).spawn(move || work_on(&pool, job)) {
            Ok(_) => workers.running += 1,
            Err(error) => {
                drop(workers);
                spare.err(format!("The call couldn't start: {error}"));
            }
        }
    }

    /// Where each pane's program is now.
    pub(super) fn pane_folders(&self) -> Vec<PathBuf> {
        let panes = self.panes.lock().unwrap();
        panes
            .list
            .iter()
            .filter(|pane| !pane.cwd.is_empty())
            .map(|pane| PathBuf::from(&pane.cwd))
            .collect()
    }

    /// The pane must be one the app last listed, so a plugin can't draw on a pane that isn't there.
    fn require_pane(&self, pane: &str) -> Result<(), String> {
        let panes = self.panes.lock().unwrap();
        if panes.list.iter().any(|entry| entry.pane_id == pane) {
            Ok(())
        } else {
            Err(format!("no pane {pane}"))
        }
    }

    /// `readPane`'s pane and lines: the plugin needs `paneOutput`, and the pane is one the app listed.
    fn pane_to_read(&self, access: &Access, params: &Value) -> Result<(String, usize), String> {
        if !may_read_pane_output(&access.plugin) {
            return Err(format!("{} may not read terminal output", access.plugin.manifest.id));
        }
        let pane = js_string(params.get("paneId"));
        self.require_pane(&pane)?;
        let lines = match params.get("lines") {
            None | Some(Value::Null) => READ_PANE_LINES,
            Some(lines) => lines.as_u64().filter(|lines| *lines > 0).ok_or("readPane lines is a whole number from 1")?,
        };
        Ok((pane, lines.min(READ_PANE_LINES_MAX) as usize))
    }

    fn set_badge(&self, access: &Access, params: &Value) -> Result<String, String> {
        let plugin = &access.plugin;
        needs(plugin, "badges")?;
        let pane = js_string(params.get("paneId"));
        self.require_pane(&pane)?;
        let badge = params.get("badge").and_then(clean_badge);
        access.env.set_badge(&plugin.manifest.id, &pane, badge);
        Ok("null".into())
    }

    fn set_pane_status(&self, access: &Access, params: &Value) -> Result<String, String> {
        let plugin = &access.plugin;
        needs(plugin, "status")?;
        let pane = js_string(params.get("paneId"));
        self.require_pane(&pane)?;
        let status = match params.get("status") {
            Some(Value::Null) => None,
            Some(raw) => Some(clean_pane_status(raw, &pages(plugin))?),
            None => return Err("setPaneStatus: status must be an object or null".into()),
        };
        access
            .env
            .set_pane_status(&plugin.manifest.id, &pane, status);
        Ok("null".into())
    }

    fn exec(&self, access: &Access, params: &Value, reply: &Reply) -> Result<String, String> {
        let request: ExecRequest =
            serde_json::from_value(params.clone()).map_err(|error| error.to_string())?;
        let ExecRequest {
            program,
            args,
            cwd,
            timeout_ms: requested,
            stream,
            account,
        } = request;
        let timeout = exec::exec_timeout(timeout_ms(requested)?);
        let on_line = stream.then(|| -> exec::OnLine { reply.lines() });
        let any_repo = worktrees_call_in_any_repo(access.builtin, &program, &args);
        let cwd = exec_cwd(
            &program,
            cwd.as_deref(),
            &access.env.projects(),
            &self.pane_folders(),
            any_repo,
        )?;
        let config_dir = access.claude_config_dir(account.as_ref(), &cwd)?;
        let result = exec::exec_under(
            &access.plugin,
            &program,
            &args,
            Some(&cwd),
            timeout,
            on_line,
            config_dir.as_deref(),
        )?;
        to_json(&result)
    }

    fn notify(&self, access: &Access, params: &Value) -> Result<String, String> {
        let plugin = &access.plugin;
        let id = &plugin.manifest.id;
        let text = |key: &str| params.get(key).and_then(Value::as_str).unwrap_or("");
        let buttons = notify_buttons(plugin, params.get("actions"))?;
        // `projectId` is the project the inbox opens the notification's tabs in; it must still be a real one.
        let project = params.get("projectId").filter(|value| !value.is_null());
        let project = project.map(|project| js_string(Some(project)));
        if let Some(project) = &project {
            let known = access
                .env
                .projects()
                .iter()
                .any(|path| path.to_string_lossy() == *project);
            if !known {
                return Err(format!("no project {project}"));
            }
        }
        let (title, body) = notification(plugin, text("title"), text("body"))?;
        let name = &plugin.manifest.name;
        let title = title
            .strip_prefix(&format!("{name}: "))
            .unwrap_or(&title)
            .to_owned();
        self.notified
            .lock()
            .unwrap()
            .allow(id, Instant::now())
            .map_err(|wait| {
                format!(
                    "{id} already showed {NOTIFY_LIMIT} notifications this minute. Try again in {} s.",
                    wait.as_secs() + 1
                )
            })?;
        access.env.notify(super::Notification {
            plugin: id.clone(),
            name: name.clone(),
            title,
            body,
            project,
            buttons,
        })?;
        Ok("null".into())
    }
}

/// The desktop's `cleanButtons`: plugin data is untrusted, so it keeps up to 3 buttons with a label of up to 32
/// characters, each opening one of the plugin's views that open as a tab (with args of up to 16 KB) or an https link.
fn notify_buttons(
    plugin: &Plugin,
    buttons: Option<&Value>,
) -> Result<Vec<super::NotifyButton>, String> {
    let buttons = match buttons {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(buttons)) => buttons,
        Some(_) => return Err("notify actions must be a list".into()),
    };
    let tab_views: Vec<&str> = plugin
        .manifest
        .contributes
        .views
        .iter()
        .filter(|view| view.opens.iter().any(|place| place == "tab"))
        .map(|view| view.id.as_str())
        .collect();
    buttons
        .iter()
        .take(3)
        .enumerate()
        .map(|(index, button)| {
            let label: String = button
                .get("label")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .chars()
                .take(32)
                .collect();
            if label.is_empty() {
                return Err(format!("notify action {index} needs a label"));
            }
            if let Some(url) = button.get("url").and_then(Value::as_str) {
                if !url.starts_with("https://") || url.len() > 2048 {
                    return Err(format!(
                        "notify action {label} needs an https link of up to 2048 characters"
                    ));
                }
                return Ok(super::NotifyButton::Url {
                    label,
                    url: url.to_owned(),
                });
            }
            let open = button.get("open");
            let view = open.and_then(|open| open.get("view"));
            let Some(view) = view
                .and_then(Value::as_str)
                .filter(|view| tab_views.contains(view))
            else {
                return Err(format!(
                    "notify action {label} opens {}, which isn't one of the plugin's views that open as a tab",
                    js_string(view)
                ));
            };
            let args = open
                .and_then(|open| open.get("args"))
                .filter(|args| !args.is_null())
                .cloned();
            if args
                .as_ref()
                .is_some_and(|args| args.to_string().encode_utf16().count() > 16 * 1024)
            {
                return Err("notify action args is limited to 16 KB".into());
            }
            Ok(super::NotifyButton::Open {
                label,
                view: view.to_owned(),
                args,
            })
        })
        .collect()
}

/// The project a worktree call is about: its `projectId`, else the one on screen.
fn worktree_project(access: &Access, params: &Value) -> Result<String, String> {
    let id = match params.get("projectId").filter(|id| !id.is_null()) {
        Some(id) => js_string(Some(id)),
        None => access
            .env
            .current_project()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default(),
    };
    if id.is_empty() {
        Err("No project is open in Wings".into())
    } else {
        Ok(id)
    }
}

/// The desktop's `plugin_*` worktree commands all start with this.
fn may_make_worktrees(access: &Access, what: &str) -> Result<(), String> {
    if may_create_worktrees(&access.plugin) {
        Ok(())
    } else {
        Err(format!("{} may not {what}", access.plugin.manifest.id))
    }
}

/// `createWorktree`: a worktree of a project on a new branch, listed under that project. Answers `{ projectId, path }`.
fn create_worktree(access: &Access, params: &Value) -> Result<String, String> {
    let branch = params.get("branch").and_then(Value::as_str).unwrap_or("");
    let base = match params.get("base") {
        None | Some(Value::Null) => None,
        Some(Value::String(base)) => Some(base.as_str()),
        _ => return Err("createWorktree base must be a branch name".into()),
    };
    let project = worktree_project(access, params)?;
    may_make_worktrees(access, "create worktrees")?;
    let made = access.env.create_worktree(&project, branch, base)?;
    to_json(&json!({ "projectId": made.id, "path": made.path }))
}

/// `worktreeSetup`: what a project's new worktrees get.
fn worktree_setup(access: &Access, params: &Value) -> Result<String, String> {
    let project = worktree_project(access, params)?;
    may_make_worktrees(access, "read worktree setup")?;
    to_json(&access.env.worktree_setup(&project)?)
}

/// `setWorktreeSetup`: absent leaves a setting as it is; `null` resets the include list or clears the setup line. The
/// setup line runs in every new worktree, which is more than making them, so only Wings' own plugins set it.
fn set_worktree_setup(access: &Access, params: &Value) -> Result<String, String> {
    let project = worktree_project(access, params)?;
    let include = match params.get("include") {
        None => None,
        Some(Value::Null) => Some(Vec::new()),
        Some(Value::Array(lines)) => Some(
            lines
                .iter()
                .map(|line| line.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
                .ok_or(INCLUDE_LINES)?,
        ),
        Some(_) => return Err(INCLUDE_LINES.into()),
    };
    let setup = match params.get("setup") {
        None => None,
        Some(Value::Null) => Some(String::new()),
        Some(Value::String(line)) => Some(line.clone()),
        Some(_) => {
            return Err("setWorktreeSetup setup is a shell line, or null to clear it".into());
        }
    };
    may_make_worktrees(access, "change worktree setup")?;
    if setup.is_some() && !access.builtin {
        return Err("Only Wings' own plugins can change a project's setup command".into());
    }
    access.env.set_worktree_setup(&project, include, setup)?;
    Ok("null".into())
}

const INCLUDE_LINES: &str =
    "setWorktreeSetup include is an array of strings, or null for the default list";

/// The desktop's `plugin_claude_accounts` and `plugin_set_claude_account` both start with this.
fn may_use_accounts(access: &Access) -> Result<(), String> {
    if may_use_claude_accounts(&access.plugin) {
        Ok(())
    } else {
        Err(format!(
            "{} may not use Claude Code accounts",
            access.plugin.manifest.id
        ))
    }
}

/// `claudeAccounts`: `{ accounts, projects }`.
fn claude_accounts(access: &Access) -> Result<String, String> {
    may_use_accounts(access)?;
    to_json(&access.env.claude_accounts()?)
}

/// `claudeAccountChoices`: the accounts a plugin may start Claude Code under, `[{ name, email, signedIn, current }]`, the
/// default one first with a `null` name. Without their folders, and quick: whether one is signed in is what its config
/// says. Given `{ account, cwd }`, as a `spawn` would be, `current` marks the account that program runs under, so a chat
/// can name the account it is in before you pick one.
fn claude_account_choices(access: &Access, params: &Value) -> Result<String, String> {
    may_use_accounts(access)?;
    let accounts = access.env.claude_accounts_quick()?.accounts;
    let current = match (params.get("account"), params.get("cwd").and_then(Value::as_str)) {
        (Some(account), Some(cwd)) => {
            let request: AccountRequest =
                serde_json::from_value(account.clone()).map_err(|error| error.to_string())?;
            let dir = access.claude_config_dir(Some(&request), std::path::Path::new(cwd))?;
            let dirs = access.env.claude_dirs();
            let name = match (&dir, dirs.first()) {
                (Some(dir), Some(default)) => wings_spaces::claude_accounts::label(dir, default),
                _ => None,
            };
            Some(name.map(str::to_owned))
        }
        _ => None,
    };
    let choices: Vec<Value> = accounts
        .into_iter()
        .map(|account| {
            let is_current = current.as_ref() == Some(&account.name);
            json!({ "name": account.name, "email": account.email, "signedIn": account.signed_in, "current": is_current })
        })
        .collect();
    to_json(&choices)
}

/// `setClaudeAccount`: the account a project's new terminals use, or the default one with a `null` name.
fn set_claude_account(access: &Access, params: &Value) -> Result<String, String> {
    let project = match params.get("projectId") {
        Some(Value::String(id)) if !id.is_empty() => id.as_str(),
        _ => return Err("setClaudeAccount needs a project id".into()),
    };
    let name = match params.get("name") {
        None | Some(Value::Null) => None,
        Some(Value::String(name)) => Some(name.as_str()),
        _ => {
            return Err(
                "setClaudeAccount name is an account name, or null for the default account".into(),
            );
        }
    };
    may_use_accounts(access)?;
    if let Some(name) = name {
        account_name(name)?;
    }
    access.env.set_claude_account(project, name)?;
    Ok("null".into())
}

/// The desktop's `claude_account_name`: nothing but an account name becomes part of a folder name.
fn account_name(name: &str) -> Result<(), String> {
    if valid_name(name) {
        Ok(())
    } else {
        Err(format!(
            "{name:?} isn't an account name: up to 32 lowercase letters, digits and dashes, starting with a letter or digit"
        ))
    }
}

/// `shareClaudeSetup`: whether a named account uses the default account's setup.
fn share_claude_setup(access: &Access, params: &Value) -> Result<String, String> {
    let (Some(Value::String(name)), Some(Value::Bool(on))) = (params.get("name"), params.get("on"))
    else {
        return Err("shareClaudeSetup takes an account name and true or false".into());
    };
    may_use_accounts(access)?;
    account_name(name)?;
    access.env.share_claude_setup(name, *on)?;
    Ok("null".into())
}

/// `setSidebarLabel`: the label by one of the plugin's sidebar or view buttons, or none with a `null` label.
fn set_sidebar_label(access: &Access, params: &Value) -> Result<String, String> {
    let page = js_string(params.get("sidebarId"));
    let plugin = &access.plugin.manifest.id;
    if !pages(&access.plugin).contains(&page) {
        return Err(format!("{plugin} has no sidebar or view {page}"));
    }
    access
        .env
        .set_sidebar_label(plugin, &page, clean_sidebar_label(params));
    Ok("null".into())
}

/// `setLinkWords`: one of the plugin's links finds only these words from now on, like the issue keys there are, or with
/// `null` whatever its pattern matches.
fn set_link_words(access: &Access, params: &Value) -> Result<String, String> {
    let link = js_string(params.get("id"));
    let plugin = &access.plugin.manifest.id;
    if !access.plugin.manifest.contributes.links.iter().any(|declared| declared.id == link) {
        return Err(format!("{plugin} has no link {link}"));
    }
    const WORDS: &str = "setLinkWords words is a list of strings, or null";
    let words = match params.get("words") {
        None | Some(Value::Null) => None,
        Some(Value::Array(words)) => {
            let words: Vec<String> = words
                .iter()
                .map(|word| word.as_str().map(str::to_owned))
                .collect::<Option<_>>()
                .ok_or(WORDS)?;
            Some(wings_plugins::links::words(&words)?)
        }
        Some(_) => return Err(WORDS.into()),
    };
    access.env.set_link_words(plugin, &link, words);
    Ok("null".into())
}

fn reply_if_error(reply: Reply, result: Result<(), String>) {
    if let Err(error) = result {
        reply.err(error);
    }
}

/// `openHistory`: the Past sessions sheet, whose Resume continues a session in the tab of the page that asks, for the
/// agents the plugin's views resume. `reply` waits for the sheet: `{ resume, cwd }`, or null.
fn open_history(access: &Access, params: &Value, tab: Option<u64>, reply: Reply) -> Result<(), String> {
    if params.get("resumeIn").and_then(Value::as_str) != Some("this tab") {
        return Err("openHistory resumeIn is \"this tab\"".into());
    }
    tab.ok_or("openHistory works only from a page shown as a tab")?;
    let views = &access.plugin.manifest.contributes.views;
    let mut agents: Vec<String> = views.iter().flat_map(|view| view.resumes.iter().cloned()).collect();
    agents.sort();
    agents.dedup();
    if agents.is_empty() {
        return Err(format!("{} has no view that resumes sessions", access.plugin.manifest.id));
    }
    access.env.open_history(agents, reply);
    Ok(())
}

/// What Claude Code last told `wings statusline`, which the app keeps.
fn statusline(access: &Access) -> Result<String, String> {
    if !may_read_statusline(&access.plugin) {
        return Err(format!(
            "{} may not read the status line",
            access.plugin.manifest.id
        ));
    }
    Ok(access.env.statusline().to_string())
}

/// `wings.reportUsage`: the limits of a Claude Code account that a plugin heard from Claude Code itself, as a chat hears
/// them with its replies: `{ account, fiveHour?, sevenDay? }`, each window `{ usedPercentage, resetsAt? }` (0 to 100, epoch
/// seconds). Wings keeps them as it keeps the status line's, and then doesn't ask Anthropic for that account for a
/// minute. Needs the status line permission, as reading them does.
fn report_usage(access: &Access, params: &Value) -> Result<String, String> {
    if !may_read_statusline(&access.plugin) {
        return Err(format!("{} may not report usage", access.plugin.manifest.id));
    }
    let account = match params.get("account") {
        None | Some(Value::Null) => None,
        Some(Value::String(name)) if valid_name(name) => Some(name.as_str()),
        _ => return Err("reportUsage account must be null or an account's name".into()),
    };
    let window = |key: &str| {
        let window = params.get(key)?;
        let used = window.get("usedPercentage")?.as_f64().filter(|used| used.is_finite())?;
        let resets_at = window.get("resetsAt").and_then(Value::as_f64).filter(|at| at.is_finite() && *at > 0.0).map(|at| at as i64);
        Some(wings_connect::statusline::Window { used_percentage: used.clamp(0.0, 100.0), resets_at })
    };
    access.env.record_usage(account, window("fiveHour"), window("sevenDay"));
    Ok("null".into())
}

fn transcript(access: &Access, params: &Value) -> Result<String, String> {
    let request: TranscriptRequest =
        serde_json::from_value(params.clone()).map_err(|error| error.to_string())?;
    let last = request.last.map(|last| last as usize);
    let cut = request.cut.filter(|cut| cut.is_finite() && *cut >= 0.0).map(|cut| cut as usize);
    let mut entries = agent_entries(
        &access.plugin,
        &access.env.claude_dirs(),
        &request.session_id,
        request.agent.as_deref(),
        &request.types,
        last,
        cut,
    )?;
    if request.images.as_deref() == Some("register") {
        pictures::register_in(&access.plugin.manifest.id, &mut entries);
    }
    to_json(&entries)
}

/// The most sessions one `pastSessions` lists.
const PAST_SESSIONS_MAX: u64 = 200;

impl Shared {
    /// `pastSessions`: the Claude Code sessions of a project, most recent first, matching `query`. Each one shows your
    /// first prompt, so it needs the `user` entries of `permissions.transcript`. The project is the one on screen, or
    /// the one `projectId` names, which has to be one of Wings' projects: a plugin never reads the sessions of any
    /// other folder.
    fn past_sessions(&self, access: &Access, params: &Value) -> Result<String, String> {
        let id = &access.plugin.manifest.id;
        if !access.plugin.manifest.permissions.transcript.iter().any(|kind| kind == "user") {
            return Err(format!("{id} may not list your Claude sessions: it needs \"user\" in permissions.transcript"));
        }
        let project = match params.get("projectId").filter(|value| !value.is_null()) {
            Some(project) => {
                let project = js_string(Some(project));
                let known = self.projects.lock().unwrap();
                let found = known.iter().find(|known| known.id == project).ok_or_else(|| format!("no project {project}"))?;
                std::path::PathBuf::from(&found.path)
            }
            None => access.env.current_project().ok_or("No project is open in Wings")?,
        };
        let query = params.get("query").and_then(Value::as_str).unwrap_or_default();
        let limit = params.get("limit").and_then(Value::as_u64).unwrap_or(50).clamp(1, PAST_SESSIONS_MAX);
        to_json(&access.env.past_sessions(&project, query, limit as usize))
    }
}

/// `readPane`: `{ text, cut }`, the pane's last lines as plain text, as Wings' `read_pane` tool reads them, and whether
/// older output was left out.
fn read_pane(access: &Access, pane: &str, lines: usize) -> Result<String, String> {
    let output = access.env.pane_output(pane)?;
    let (text, cut) = wings_connect::panes::pane_text(&output, lines, READ_PANE_BYTES_MAX);
    to_json(&json!({ "text": text, "cut": cut }))
}

fn ports(access: &Access) -> Result<String, String> {
    if !may_read_ports(&access.plugin) {
        return Err(format!(
            "{} may not read listening ports",
            access.plugin.manifest.id
        ));
    }
    to_json(&access.env.ports()?)
}

/// `openFile`: `path`, a file in one of your Wings projects, in your editor, at `line` when there is one.
fn open_file(access: &Access, params: &Value) -> Result<String, String> {
    let path = params.get("path").and_then(Value::as_str).ok_or("openFile needs the file's full path")?;
    let line = match params.get("line") {
        None | Some(Value::Null) => None,
        Some(line) => Some(line.as_u64().and_then(|line| u32::try_from(line).ok()).filter(|line| *line > 0).ok_or("openFile line is a number from 1")?),
    };
    let asked = std::path::Path::new(path);
    if !asked.is_absolute() {
        return Err("openFile needs the file's full path".into());
    }
    // Where it really is, past `..` and links, so the check below is of the file that opens.
    let file = asked.canonicalize().ok().filter(|file| file.is_file()).ok_or_else(|| format!("{path} isn't a file"))?;
    let folder = file.parent().ok_or_else(|| format!("{path} isn't a file"))?;
    if !is_claude_file(&file, &access.env.claude_dirs()) {
        wings_plugins::panes::pane_cwd(folder, &access.env.projects(), None).map_err(|_| format!("{path} isn't inside any of your Wings projects"))?;
    }
    access.env.open_file(&file, line)?;
    Ok("null".into())
}

/// Claude Code's own files a page may open outside your projects: `CLAUDE.md`, `settings.json` and `settings.local.json`
/// in an account's folder, and the memories (`.md`) in `projects/<project>/memory/`. `file` is already past `..` and
/// links, and so are the folders, so a link can't lead out of them.
pub(super) fn is_claude_file(file: &std::path::Path, claude_dirs: &[std::path::PathBuf]) -> bool {
    claude_dirs.iter().filter_map(|dir| dir.canonicalize().ok()).any(|dir| {
        let Ok(inside) = file.strip_prefix(&dir) else { return false };
        let parts: Vec<_> = inside.components().map(|part| part.as_os_str().to_str().unwrap_or_default()).collect();
        match parts[..] {
            [name] => ["CLAUDE.md", "settings.json", "settings.local.json"].contains(&name),
            [projects, _, memory, ..] => projects == "projects" && memory == "memory" && file.extension().is_some_and(|ext| ext == "md"),
            _ => false,
        }
    })
}

/// `openLink`: what a link in a page opens for `text`, as a click on it would: one of the plugins' `contributes.links`
/// must match the whole text. Right after your click or key, which the frame has checked.
fn open_link(access: &Access, params: &Value) -> Result<String, String> {
    let text = params.get("text").and_then(Value::as_str).filter(|text| !text.is_empty() && text.len() <= 2048).ok_or("openLink needs the link's text")?;
    access.env.open_link(text)?;
    Ok("null".into())
}

fn open_url(access: &Access, params: &Value) -> Result<String, String> {
    let url = js_string(params.get("url"));
    if !may_open_url(&access.plugin, &url) {
        return Err(format!("{} may not open {url}", access.plugin.manifest.id));
    }
    access.env.open_url(&url)?;
    Ok("null".into())
}

#[cfg(test)]
mod claude;
mod pictures;
mod pr_tracker;
mod sections;
pub use sections::{SectionIcon, SectionRow};
#[cfg(test)]
mod disk_usage;
#[cfg(all(test, unix))]
mod jobs;
#[cfg(test)]
mod prerequisites;
#[cfg(test)]
mod page_names;
#[cfg(test)]
mod processes;
#[cfg(test)]
mod schedules;
mod session_detail;
mod web;
pub(super) use web::needs_click as web_needs_click;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod worktrees;
