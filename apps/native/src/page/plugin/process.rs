//! `spawn`, `processWrite` and `processStop`: programs a frame keeps running and talks to, under the manifest's
//! `processes`, like an agent run headless. The frame names each with a token, which its link has prefixed with the
//! frame's id, so a frame reaches only its own. They stop when the frame closes or the plugin is turned off.

use super::{Access, Shared};
use crate::page::runtime::Reply;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use wings_plugins::{
    panes::exec_cwd,
    process::{self, Process},
};

/// The most programs one plugin keeps running at once.
const MAX_RUNNING: usize = 8;
/// The most text one `processWrite` sends: a message with a pasted image fits.
const WRITE_MAX: usize = process::LINE_LIMIT;

#[derive(Deserialize)]
struct SpawnRequest {
    program: String,
    args: Vec<String>,
    cwd: String,
}

impl Shared {
    pub(super) fn process_call(
        self: &Arc<Self>,
        method: &str,
        params: Value,
        reply: Reply,
        access: Access,
    ) {
        // The frame's link has checked it and put the frame's id in front.
        let token = params["token"].as_str().unwrap_or_default().to_owned();
        let result = match method {
            "spawn" => self.spawn(token, &params, reply.clone(), access),
            "processWrite" => self.write(&token, &params, reply.clone()),
            _ => {
                if let Some(running) = self.processes.lock().unwrap().get(&token) {
                    running.stop();
                }
                reply.clone().ok("null".into());
                Ok(())
            }
        };
        if let Err(error) = result {
            reply.err(error);
        }
    }

    fn spawn(
        self: &Arc<Self>,
        token: String,
        params: &Value,
        reply: Reply,
        access: Access,
    ) -> Result<(), String> {
        let SpawnRequest { program, args, cwd } =
            serde_json::from_value(params.clone()).map_err(|error| error.to_string())?;
        let cwd = exec_cwd(&program, Some(&cwd), &access.env.projects(), &self.pane_folders(), false)?;
        // Kept from now, before it starts, so that the page can write to it at once.
        let running = Process::new();
        {
            let mut processes = self.processes.lock().unwrap();
            room(&processes, &token)?;
            processes.insert(token.clone(), running.clone());
        }
        let host = Arc::downgrade(self);
        self.off_thread(reply, move |reply| {
            let lines = reply.lines_in_order();
            let exit = process::run(&access.plugin, &running, &program, &args, &cwd, |line| lines(line));
            if let Some(host) = host.upgrade() {
                host.processes.lock().unwrap().remove(&token);
            }
            serde_json::to_string(&exit?).map_err(|error| error.to_string())
        });
        Ok(())
    }

    fn write(&self, token: &str, params: &Value, reply: Reply) -> Result<(), String> {
        let text = params["text"]
            .as_str()
            .ok_or("processWrite text must be a string")?
            .to_owned();
        if text.len() > WRITE_MAX {
            return Err("processWrite is limited to 8 MB of text".into());
        }
        let running = self
            .processes
            .lock()
            .unwrap()
            .get(token)
            .cloned()
            .ok_or_else(|| format!("No program is running as {}", shown(token)))?;
        running.write(text, move |result| reply.done(result.map(|()| "null".into())));
        Ok(())
    }

    /// Stops the programs whose token starts with `prefix`: a frame's, or with an empty one, all of them.
    pub(super) fn stop_processes(&self, prefix: &str) {
        for (token, running) in self.processes.lock().unwrap().iter() {
            if token.starts_with(prefix) {
                running.stop();
            }
        }
    }
}

fn room(processes: &std::collections::HashMap<String, Arc<Process>>, token: &str) -> Result<(), String> {
    if processes.contains_key(token) {
        return Err(format!("A program is already running as {}", shown(token)));
    }
    if processes.len() >= MAX_RUNNING {
        return Err(format!("A plugin can keep at most {MAX_RUNNING} programs running"));
    }
    Ok(())
}

/// The token as the frame named it, without the frame's id in front.
fn shown(token: &str) -> &str {
    token.split_once(':').map_or(token, |(_, own)| own)
}
