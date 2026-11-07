//! Connecting OpenCode, as the desktop's `opencode_mcp_*` commands do: Wings' MCP server goes in OpenCode's global
//! config (`wings_connect::opencode`), pointing at this app's `wings` program and the socket Claude Code uses too, so
//! OpenCode's sessions get Wings' and plugin tools. The app adds it on start unless you disconnected OpenCode in
//! Settings, Agents, which keeps a marker beside the app's state the way Claude Code's disconnect does.

use std::path::{Path, PathBuf};

use gpui::Context;
use wings_connect::opencode::{self, Status};

use super::Workspace;

/// The file that says you disconnected OpenCode, beside the app's own state.
pub(super) fn disconnected_marker(state: &Path) -> PathBuf {
    state.with_file_name(opencode::DISCONNECTED)
}

/// What a Connect or Disconnect works on, so tests can point it at their own folders.
pub(super) struct Target {
    /// OpenCode's global config.
    pub(super) config: PathBuf,
    /// The disconnected marker; `None` keeps none.
    pub(super) marker: Option<PathBuf>,
    /// `opencode`, to reload its service after a change; `None` when it isn't installed.
    pub(super) opencode: Option<PathBuf>,
}

impl Target {
    /// Reloads OpenCode's service after the file changed, so its next session sees the change.
    fn reload(&self, changed: bool) {
        if let (true, Some(opencode)) = (changed, &self.opencode)
            && let Err(error) = opencode::reload(opencode)
        {
            eprintln!("OpenCode didn't reload its config: {error}");
        }
    }

    /// Points the Wings server in OpenCode's config at `exe --mcp socket`, and forgets an earlier disconnect.
    pub(super) fn connect(&self, exe: &Path, socket: &Path) -> Result<(), String> {
        let changed = opencode::add(&self.config, exe, socket)?;
        self.reload(changed);
        if let Some(marker) = &self.marker {
            let _ = std::fs::remove_file(marker);
        }
        Ok(())
    }

    /// Takes the Wings server out, and keeps the app from adding it back when it starts.
    pub(super) fn disconnect(&self) -> Result<(), String> {
        let changed = opencode::remove(&self.config)?;
        self.reload(changed);
        match &self.marker {
            Some(marker) => std::fs::write(marker, "").map_err(|e| e.to_string()),
            None => Ok(()),
        }
    }

    /// What starting the app does: connects OpenCode unless you disconnected it or it already runs `exe`.
    pub(super) fn connect_on_start(&self, exe: &Path, socket: &Path) -> Result<(), String> {
        if self.marker.as_ref().is_some_and(|m| m.exists())
            || opencode::points_at(&self.config, exe)
        {
            return Ok(());
        }
        self.connect(exe, socket)
    }

    pub(super) fn status(&self) -> Status {
        Status {
            opencode: self.opencode.is_some(),
            connected: opencode::entry(&self.config).is_some(),
        }
    }
}

/// The real OpenCode: its config under `~/.config`, and the marker beside the app's state.
fn target(state: Option<&Path>) -> Target {
    Target {
        config: opencode::config_path(),
        marker: state.map(disconnected_marker),
        opencode: wings_plugins::exec::find_program("opencode"),
    }
}

/// On start, as the desktop's `connect_opencode_on_start`: only a release build of the installed app, so a build run
/// from `target/` never points OpenCode at the build folder, and only when OpenCode is installed. Off the window's
/// thread, since a reload runs `opencode`.
pub(super) fn connect_on_start(socket: PathBuf, state: Option<PathBuf>) {
    if cfg!(debug_assertions) || state.is_none() {
        return;
    }
    let Some(exe) = std::env::current_exe()
        .ok()
        .and_then(|exe| wings_connect::register::installed_wings(&exe))
    else {
        return;
    };
    std::thread::spawn(move || {
        let target = target(state.as_deref());
        if target.opencode.is_none() {
            return;
        }
        if let Err(error) = target.connect_on_start(&exe, &socket) {
            eprintln!("Couldn't connect OpenCode: {error}");
        }
    });
}

/// OpenCode's connection as Settings, Agents shows it, and what it is doing.
#[derive(Default)]
pub(super) struct Link {
    /// `None` until read.
    pub(super) status: Option<Status>,
    pub(super) busy: bool,
    pub(super) error: Option<String>,
    task: Option<gpui::Task<()>>,
}

impl Workspace {
    /// Reads OpenCode's state again, off the window's thread. Only an app that serves the socket connects OpenCode, as
    /// with Claude Code, so tests and screenshots read none of yours.
    pub(super) fn load_opencode_status(&mut self, cx: &mut Context<Self>) {
        if self.config.socket.is_none() || self.opencode.busy {
            return;
        }
        self.run_opencode(None, cx);
    }

    /// Connect (`true`) or Disconnect (`false`), then reads the state again.
    pub(super) fn change_opencode(&mut self, connect: bool, cx: &mut Context<Self>) {
        if self.opencode.busy {
            return;
        }
        self.run_opencode(Some(connect), cx);
    }

    fn run_opencode(&mut self, change: Option<bool>, cx: &mut Context<Self>) {
        let Some(socket) = self.config.socket.clone() else {
            return;
        };
        let state = self.config.state.clone();
        self.opencode.busy = true;
        if change.is_some() {
            self.opencode.error = None;
        }
        cx.notify();
        let work = cx.background_executor().spawn(async move {
            let target = target(state.as_deref());
            let result = match change {
                None => Ok(()),
                Some(true) => super::claude::wings_program()
                    .ok_or_else(|| {
                        "The `wings` program isn't beside this app, so OpenCode would have nothing to run".to_owned()
                    })
                    .and_then(|exe| target.connect(&exe, &socket)),
                Some(false) => target.disconnect(),
            };
            if change.is_some() && result.is_ok() {
                super::journal::sync(state.as_deref());
            }
            (result, target.status())
        });
        self.opencode.task = Some(cx.spawn(async move |this, cx| {
            let (result, status) = work.await;
            let _ = this.update(cx, |this, cx| {
                this.opencode.busy = false;
                this.opencode.status = Some(status);
                if let Err(error) = result {
                    this.opencode.error = Some(error);
                }
                cx.notify();
            });
        }));
    }

    /// Shows this OpenCode state in Settings, Agents instead of reading yours, for screenshots.
    #[cfg(feature = "test-support")]
    pub fn show_opencode_status(&mut self, status: Status, cx: &mut Context<Self>) {
        self.opencode.status = Some(status);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    /// A made-up home: OpenCode's config folder and the app's state folder, never your own.
    fn target(name: &str, config: Option<&str>) -> (Target, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "wings-native-opencode-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let config_dir = dir.join("config");
        let path = opencode::config_in(&config_dir);
        if let Some(text) = config {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, text).unwrap();
        }
        let state = dir.join("Wings Native/workspace.json");
        std::fs::create_dir_all(state.parent().unwrap()).unwrap();
        let target = Target {
            config: path,
            marker: Some(disconnected_marker(&state)),
            opencode: None,
        };
        (target, dir)
    }

    fn config(target: &Target) -> Value {
        serde_json::from_str(&std::fs::read_to_string(&target.config).unwrap()).unwrap()
    }

    #[test]
    fn connecting_adds_wings_beside_your_servers_once_and_disconnecting_takes_only_it() {
        let mine = r#"{"$schema":"https://opencode.ai/config.json","model":"anthropic/claude","mcp":{"servers":{"context7":{"type":"remote","url":"https://mcp.context7.com/mcp"}}}}"#;
        let (target, dir) = target("connect", Some(mine));
        let (exe, socket) = (
            Path::new("/Applications/Wings Native.app/Contents/MacOS/wings"),
            Path::new("/data/mcp.sock"),
        );
        target.connect(exe, socket).unwrap();
        let wings = json!({"type": "local", "command": [exe, "--mcp", socket]});
        assert_eq!(config(&target)["mcp"]["servers"]["wings"], wings);
        assert_eq!(
            config(&target)["mcp"]["servers"]["context7"]["url"],
            "https://mcp.context7.com/mcp"
        );
        assert_eq!(config(&target)["model"], "anthropic/claude");
        assert_eq!(
            target.status(),
            Status {
                opencode: false,
                connected: true
            }
        );
        // A second Connect leaves the file as it was.
        let before = std::fs::read_to_string(&target.config).unwrap();
        target.connect(exe, socket).unwrap();
        assert_eq!(std::fs::read_to_string(&target.config).unwrap(), before);
        target.disconnect().unwrap();
        let mut expected: Value = serde_json::from_str(mine).unwrap();
        assert_eq!(config(&target), expected);
        assert!(target.marker.as_ref().unwrap().exists());
        // Connecting again forgets the disconnect.
        target.connect(exe, socket).unwrap();
        assert!(!target.marker.as_ref().unwrap().exists());
        expected["mcp"]["servers"]["wings"] = wings;
        assert_eq!(config(&target), expected);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn starting_the_app_connects_unless_you_disconnected_and_repoints_a_moved_app() {
        let (target, dir) = target("start", None);
        let socket = Path::new("/data/mcp.sock");
        target
            .connect_on_start(Path::new("/Old.app/Contents/MacOS/wings"), socket)
            .unwrap();
        let new = Path::new("/New.app/Contents/MacOS/wings");
        target.connect_on_start(new, socket).unwrap();
        assert!(opencode::points_at(&target.config, new));
        target.disconnect().unwrap();
        target.connect_on_start(new, socket).unwrap();
        assert!(!target.status().connected);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_config_with_comments_is_left_alone_and_says_why() {
        let text = "{\n  // mine\n  \"model\": \"m\"\n}\n";
        let (target, dir) = target("comments", Some(text));
        let error = target
            .connect(Path::new("/w"), Path::new("/s"))
            .unwrap_err();
        assert!(error.contains("isn't plain JSON"), "{error}");
        assert_eq!(std::fs::read_to_string(&target.config).unwrap(), text);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
