//! How Wings connects to Claude Code, the same in every Wings app so one can replace another without a change to
//! Claude Code's config: the status line and hooks in its settings (`settings`), the MCP server registered with it
//! (`register`), the socket the app serves (`server`), and the `wings` commands Claude Code and you run, which talk to
//! that socket (`cli`, and `bridge` for `wings --mcp`). `statusline` keeps what Claude Code tells its status line, `usage`
//! reads the usage limits as Claude Code's `/usage` does, and `panes` and `project_info` are Wings' own tools.

#[cfg(unix)]
pub mod bridge;
#[cfg(unix)]
pub mod cli;
pub mod harnesses;
pub mod journal_sink;
pub mod opencode;
pub mod panes;
pub mod project_info;
pub mod register;
#[cfg(unix)]
pub mod server;
pub mod settings;
pub mod statusline;
pub mod usage;

/// Runs the `wings` command `args` (without the program's name), or `None` when they aren't one, for the app to start.
/// `version` is the Wings version the MCP server reports.
#[cfg(unix)]
pub fn run_command(args: &[String], version: &'static str) -> Option<i32> {
    Some(match args.first().map(String::as_str) {
        Some("--mcp") => bridge::run(args.get(1).cloned(), version),
        Some("plugin") => cli::run(&args[1..]),
        Some("hook") => cli::hook(),
        Some("wait") => cli::wait(&args[1..]),
        Some("journal") => cli::journal(),
        Some("statusline") => cli::statusline(args.get(1).is_some_and(|a| a == "--pass")),
        Some("worktree-setup") => cli::worktree_setup(&args[1..]),
        Some("tools") => cli::tools(),
        // A plugin's id, like `tasks`: one of its commands. Anything starting with `-` is left to the app.
        Some(id) if id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') && !id.starts_with('-') => cli::plugin_tool(args),
        _ => return None,
    })
}

/// Writes `bytes` to `path` whole or not at all: a temporary file made for you only, given the permissions the file
/// already has, synced, then renamed over it. An agent's config can hold provider keys, so a file only you could read
/// stays that way, and a new one is yours only. Through a symlink, the file it points at is the one replaced. As
/// `write_whole` in `wings-spaces`' `claude_accounts.rs`.
pub(crate) fn write_whole(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut tmp = path.clone().into_os_string();
    tmp.push(".wings-tmp");
    let tmp = std::path::PathBuf::from(tmp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let written = (|| {
        let mut file = options.open(&tmp)?;
        file.write_all(bytes)?;
        if let Ok(meta) = std::fs::metadata(&path) {
            file.set_permissions(meta.permissions())?;
        }
        file.sync_all()?;
        std::fs::rename(&tmp, &path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}
