use std::{
    io::{Read, Write},
    path::Path,
    sync::{Arc, Mutex},
    thread,
};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use tauri::ipc::{Channel, InvokeResponseBody};

/// A shell running in a pseudo-terminal, bound to one Wings tab.
pub struct Pane {
    pub id: String,
    pub space_id: String,
    pub shell_pid: Option<u32>,
    /// Latest OSC 0/2 window title the program set. Claude Code puts its state glyph here.
    pub title: Arc<Mutex<String>>,
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
}

pub struct SpawnRequest<'a> {
    pub id: String,
    pub space_id: String,
    pub cwd: &'a Path,
    pub cols: u16,
    pub rows: u16,
}

/// Variables the dev host (a Claude Code or Herdr session) would otherwise leak into every pane.
/// `CLAUDE_CODE_CHILD_SESSION`, for one, hides a nested `claude` from `--resume` and the agents list.
fn is_host_session_var(key: &str) -> bool {
    key == "CLAUDECODE"
        || key.starts_with("CLAUDE_CODE_")
        || matches!(
            key,
            "CLAUDE_PID" | "CLAUDE_PROJECT_DIR" | "CLAUDE_ENV_FILE" | "CLAUDE_JOB_DIR" | "CLAUDE_EFFORT"
        )
        || key.starts_with("HERDR_")
        || key.starts_with("TERM_PROGRAM")
}

impl Pane {
    pub fn spawn(
        req: SpawnRequest,
        output: Channel<InvokeResponseBody>,
        on_exit: impl FnOnce(&str) + Send + 'static,
    ) -> anyhow::Result<Arc<Pane>> {
        let pair = native_pty_system().openpty(PtySize {
            rows: req.rows,
            cols: req.cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        // Default prog = the user's login shell, so their PATH and profile load as in any terminal.
        let mut cmd = CommandBuilder::new_default_prog();
        cmd.cwd(req.cwd);
        for (key, _) in std::env::vars_os() {
            if key.to_str().is_some_and(is_host_session_var) {
                cmd.env_remove(key);
            }
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "Wings");
        cmd.env("WINGS_ENV", "1");
        cmd.env("WINGS_PANE_ID", &req.id);

        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);

        let pane = Arc::new(Pane {
            id: req.id,
            space_id: req.space_id,
            shell_pid: child.process_id(),
            title: Arc::new(Mutex::new(String::new())),
            writer: Mutex::new(pair.master.take_writer()?),
            child: Mutex::new(child),
            master: Mutex::new(pair.master),
        });

        let mut reader = pane.master.lock().unwrap().try_clone_reader()?;
        let title = pane.title.clone();
        let pane_id = pane.id.clone();
        thread::Builder::new()
            .name(format!("pty-{pane_id}"))
            .spawn(move || {
                let mut parser = vte::Parser::new();
                let mut tap = TitleTap { title };
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            parser.advance(&mut tap, &buf[..n]);
                            // debt: one IPC message per read; coalesce per frame if many busy panes show lag.
                            if output.send(InvokeResponseBody::Raw(buf[..n].to_vec())).is_err() {
                                break;
                            }
                        }
                    }
                }
                on_exit(&pane_id);
            })?;

        Ok(pane)
    }

    pub fn write(&self, data: &[u8]) -> anyhow::Result<()> {
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(data)?;
        writer.flush()?;
        Ok(())
    }

    pub fn resize(&self, cols: u16, rows: u16) -> anyhow::Result<()> {
        self.master.lock().unwrap().resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
    }

    pub fn kill(&self) {
        let _ = self.child.lock().unwrap().kill();
    }

    /// The running command's process (the shell itself at a prompt), from the PTY's foreground process group.
    #[cfg(unix)]
    pub fn foreground_pid(&self) -> Option<u32> {
        self.master.lock().unwrap().process_group_leader().map(|pid| pid as u32)
    }

    #[cfg(not(unix))]
    pub fn foreground_pid(&self) -> Option<u32> {
        None
    }

    pub fn title(&self) -> String {
        self.title.lock().unwrap().clone()
    }
}

/// Watches the byte stream for OSC 0/2 title changes without keeping a screen model.
struct TitleTap {
    title: Arc<Mutex<String>>,
}

impl vte::Perform for TitleTap {
    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if let [kind, rest @ ..] = params {
            if matches!(*kind, b"0" | b"2") && !rest.is_empty() {
                *self.title.lock().unwrap() = String::from_utf8_lossy(&rest.join(&b';')).into_owned();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn title_after(bytes: &[u8]) -> String {
        let title = Arc::new(Mutex::new(String::new()));
        let mut tap = TitleTap { title: title.clone() };
        vte::Parser::new().advance(&mut tap, bytes);
        let out = title.lock().unwrap().clone();
        out
    }

    #[test]
    fn reads_osc_titles_including_semicolons() {
        assert_eq!(title_after(b"\x1b]0;\xe2\x9c\xb3 Fix login\x07"), "✳ Fix login");
        assert_eq!(title_after(b"\x1b]2;a;b\x1b\\"), "a;b");
        assert_eq!(title_after(b"\x1b]7;file:///tmp\x07"), "");
    }

    #[test]
    fn scrubs_only_host_session_vars() {
        assert!(is_host_session_var("CLAUDE_CODE_CHILD_SESSION"));
        assert!(is_host_session_var("CLAUDECODE"));
        assert!(is_host_session_var("HERDR_PANE_ID"));
        assert!(!is_host_session_var("CLAUDE_CONFIG_DIR"));
        assert!(!is_host_session_var("PATH"));
    }
}
