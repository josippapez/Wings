//! How Wings connects to Claude Code, the same in every Wings app so one can replace another without a change to
//! Claude Code's config: the status line and hooks in its settings (`settings`), the MCP server registered with it
//! (`register`), the socket the app serves (`server`), and the `wings` commands Claude Code and you run, which talk to
//! that socket (`cli`, and `bridge` for `wings --mcp`). `statusline` keeps what Claude Code tells its status line, and
//! `panes` and `project_info` are Wings' own tools.

#[cfg(unix)]
pub mod bridge;
#[cfg(unix)]
pub mod cli;
pub mod panes;
pub mod project_info;
pub mod register;
#[cfg(unix)]
pub mod server;
pub mod settings;
pub mod statusline;

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
