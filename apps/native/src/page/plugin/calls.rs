//! What a plugin's frames can ask of the host with `wings.*`: the desktop's `handle` in `apps/desktop/src/lib/plugins.ts`
//! and the `plugin_*` commands in `apps/desktop/src-tauri/src/lib.rs`, with their argument rules, limits and messages.
//! What a call may do is `wings-plugins`' to say (`permissions`, `exec`, `fetch::may_open_url`, `transcript`), and
//! nothing here decides it: this reads the manifest's grant with those functions and refuses with the desktop's words.
//!
//! A call that only moves data answers on the frame's thread. One that reads the disk or runs a program runs on a thread
//! of its own and answers when it is done, as the desktop's async commands do, so a slow call holds up no one.

use std::{path::PathBuf, sync::Arc, thread, time::Instant};

use serde::Deserialize;
use serde_json::{Value, json};
use wings_plugins::{
    exec,
    fetch::may_open_url,
    manifest::Plugin,
    panes::{exec_cwd, worktrees_call_in_any_repo},
    permissions::{NOTIFY_LIMIT, may_read_ports, may_read_statusline, notification},
    transcript::transcript_entries,
};

use super::{
    Shared,
    env::Access,
    ui::{clean_badge, clean_pane_status},
};
use crate::page::runtime::Reply;

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
fn needs(plugin: &Plugin, kind: &str) -> Result<(), String> {
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
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TranscriptRequest {
    session_id: String,
    types: Vec<String>,
    last: Option<f64>,
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
    /// A frame's call of `wings.<method>`, answered through `reply`.
    pub(super) fn call(self: &Arc<Self>, method: &str, params: Value, reply: Reply) {
        let Some(access) = self.access.clone() else {
            // Nothing runs a plugin that isn't on and approved, so a host without a manifest refuses every call.
            return reply.err(format!("{} is turned off or not installed", self.config.id));
        };
        let this = self.clone();
        match method {
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
            "statusline" => reply.done(statusline(&access.plugin)),
            "exec" => {
                self.off_thread(reply, move |reply| this.exec(&access, &params, reply));
            }
            "transcript" => {
                self.off_thread(reply, move |_| transcript(&access, &params));
            }
            "ports" => self.off_thread(reply, move |_| ports(&access)),
            "openUrl" => self.off_thread(reply, move |_| open_url(&access, &params)),
            "notify" => {
                self.off_thread(reply, move |_| this.notify(&access, &params));
            }
            _ => reply.err(format!("unknown method {method}")),
        }
    }

    /// Runs `work` on a thread of its own and answers with what it returns.
    fn off_thread(
        &self,
        reply: Reply,
        work: impl FnOnce(&Reply) -> Result<String, String> + Send + 'static,
    ) {
        let spare = reply.clone();
        let spawned = thread::Builder::new()
            .name("plugin-call".into())
            .spawn(move || {
                let result = work(&reply);
                reply.done(result);
            });
        if let Err(error) = spawned {
            spare.err(format!("The call couldn't start: {error}"));
        }
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
        } = request;
        let timeout = exec::exec_timeout(timeout_ms(requested)?);
        let on_line = stream.then(|| -> exec::OnLine { reply.lines() });
        let projects = access.env.projects();
        // Where each pane's program is now.
        let panes: Vec<PathBuf> = {
            let panes = self.panes.lock().unwrap();
            panes
                .list
                .iter()
                .filter(|pane| !pane.cwd.is_empty())
                .map(|pane| PathBuf::from(&pane.cwd))
                .collect()
        };
        let any_repo = worktrees_call_in_any_repo(access.builtin, &program, &args);
        let cwd = exec_cwd(&program, cwd.as_deref(), &projects, &panes, any_repo)?;
        let result = exec::exec(
            &access.plugin,
            &program,
            &args,
            Some(&cwd),
            timeout,
            on_line,
        )?;
        to_json(&result)
    }

    fn notify(&self, access: &Access, params: &Value) -> Result<String, String> {
        let plugin = &access.plugin;
        let id = &plugin.manifest.id;
        let text = |key: &str| params.get(key).and_then(Value::as_str).unwrap_or("");
        // `projectId` is the notification's project, which the desktop's inbox opens; here it must still be a real one.
        if let Some(project) = params.get("projectId").filter(|value| !value.is_null()) {
            let project = js_string(Some(project));
            let known = access
                .env
                .projects()
                .iter()
                .any(|path| path.to_string_lossy() == project);
            if !known {
                return Err(format!("no project {project}"));
            }
        }
        let (title, body) = notification(plugin, text("title"), text("body"))?;
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
        access.env.notify(&title, &body)?;
        Ok("null".into())
    }
}

/// What the desktop's `wings statusline` holds before Claude Code has told it anything. The native app doesn't receive
/// status lines yet, so this is all there is: no limits, and no session's context.
fn statusline(plugin: &Plugin) -> Result<String, String> {
    if !may_read_statusline(plugin) {
        return Err(format!(
            "{} may not read the status line",
            plugin.manifest.id
        ));
    }
    Ok(json!({ "rateLimits": null, "sessions": {} }).to_string())
}

fn transcript(access: &Access, params: &Value) -> Result<String, String> {
    let request: TranscriptRequest =
        serde_json::from_value(params.clone()).map_err(|error| error.to_string())?;
    let last = request.last.map(|last| last as usize);
    let entries = transcript_entries(
        &access.plugin,
        &access.env.claude_dir(),
        &request.session_id,
        &request.types,
        last,
    )?;
    to_json(&entries)
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

fn open_url(access: &Access, params: &Value) -> Result<String, String> {
    let url = js_string(params.get("url"));
    if !may_open_url(&access.plugin, &url) {
        return Err(format!("{} may not open {url}", access.plugin.manifest.id));
    }
    access.env.open_url(&url)?;
    Ok("null".into())
}

#[cfg(test)]
mod tests;
