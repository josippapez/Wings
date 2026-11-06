//! Where a pane's shell runs. The app talks to its terminals through `PtyBackend`; `LocalBackend` keeps them inside
//! this process, as Wings always has. With `WINGS_PTYD=1`, `RemoteBackend` keeps them in a terminal host process,
//! `wings ptyd` (`host.rs`, run as `launch.rs` says), that the app talks to over a socket (`proto.rs`).

#[cfg(target_os = "macos")]
pub mod host;
#[cfg(target_os = "macos")]
pub mod launch;
pub mod local;
pub mod output;
pub mod proto;
#[cfg(target_os = "macos")]
pub mod remote;
mod shell;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use tauri::ipc::{Channel, InvokeResponseBody};

pub use local::LocalBackend;

/// The terminal host's own version, apart from Wings', in its `welcome`. Bump it when `host.rs` changes, so an app
/// can tell an older host that still runs from before an update.
pub const HOST_VERSION: u32 = 2;

/// The terminal host's socket, in a folder only this user can open, next to the MCP socket. A dev build has its own,
/// so it never talks to the installed Wings' host.
pub fn socket_path(app_data: &Path) -> PathBuf {
    app_data.join("ptyd").join(if cfg!(debug_assertions) { "ptyd-dev.sock" } else { "ptyd.sock" })
}

pub use wings_agents::NO_PROJECT;

/// A pane's id, from the number its backend gave it. The terminal host numbers its panes, so the ids Wings saved
/// still name the same panes after it restarts.
pub fn pane_id(number: u32) -> String {
    format!("p{number}")
}

/// The number in a pane's id.
pub fn pane_number(id: &str) -> Option<u32> {
    id.strip_prefix('p')?.parse().ok()
}

pub struct SpawnRequest<'a> {
    pub space_id: String,
    pub cwd: &'a Path,
    pub cols: u16,
    pub rows: u16,
    pub history_id: Option<String>,
    /// Output saved from this pane before Wings restarted, as `local::restored` gives it. It goes in the kept output
    /// only: the UI writes it into the terminal itself, before it listens for input.
    pub history: Vec<u8>,
    /// The config folder of the Claude Code account the pane's project uses, for its `CLAUDE_CONFIG_DIR`. `None` is the
    /// default account.
    pub claude_config_dir: Option<PathBuf>,
}

/// What stays true of a pane from the moment it starts.
pub struct PaneMeta {
    pub id: String,
    pub space_id: String,
    pub shell_pid: Option<u32>,
    /// The folder the shell started in.
    pub cwd: PathBuf,
    /// Names the file its output is saved to when Wings quits, and read back from when it starts again.
    pub history_id: Option<String>,
}

/// A pane as the app sees it now: what it was started with, plus the two things that change as it runs.
pub struct PaneMirror<'a> {
    pub meta: &'a Arc<PaneMeta>,
    /// The running command's process (the shell itself at a prompt), from the PTY's foreground process group.
    pub foreground_pid: Option<u32>,
    /// Latest OSC 0/2 window title the program set. Claude Code puts its state glyph here.
    pub title: &'a str,
    pub codex_status: Option<crate::program_status::CodexReport>,
}

/// Called once with the pane's id when its output ends: the shell exited, or it was killed.
pub type OnExit = Box<dyn FnOnce(&str) + Send>;

/// The terminals Wings runs, by pane id. Every method is safe to call from any thread.
pub trait PtyBackend: Send + Sync {
    /// Starts a pane, under an id the backend picks.
    fn spawn(&self, req: SpawnRequest<'_>, output: Channel<InvokeResponseBody>, on_exit: OnExit) -> anyhow::Result<Arc<PaneMeta>>;

    /// Sends the UI that loaded since (a webview reload) the recent output, then streams to it instead.
    fn attach(&self, id: &str, output: Channel<InvokeResponseBody>) -> anyhow::Result<()>;

    fn write(&self, id: &str, data: &[u8]) -> anyhow::Result<()>;

    fn resize(&self, id: &str, cols: u16, rows: u16) -> anyhow::Result<()>;

    /// Ends the pane's process and forgets the pane.
    fn kill(&self, id: &str);

    fn meta(&self, id: &str) -> Option<Arc<PaneMeta>>;

    fn list(&self) -> Vec<Arc<PaneMeta>>;

    /// Calls `visit` for each pane. The detection loop runs this every 500 ms, so it reads each pane once, under one
    /// lock, and hands out borrows instead of copies.
    fn each_mirror(&self, visit: &mut dyn FnMut(PaneMirror<'_>));

    fn foreground_pid(&self, id: &str) -> Option<u32>;

    /// The raw output kept for this pane, oldest first, for `read_pane` and for saving history when Wings quits.
    fn recent_output(&self, id: &str) -> Option<Vec<u8>>;
}
