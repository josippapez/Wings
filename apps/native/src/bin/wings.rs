//! The `wings` command, the desktop app's: what Claude Code runs (`wings --mcp <socket>`, `wings statusline`, `wings
//! hook`), the journal sink and the `wings` CLI. Each talks to the running app over its socket.

fn main() {
    #[cfg(unix)]
    {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if let Some(code) = wings_connect::run_command(&args, env!("WINGS_DESKTOP_VERSION")) {
            std::process::exit(code);
        }
    }
    eprintln!(
        "Usage: wings <command>\n\n  plugin ...        Manage plugins\n  tools             The commands the plugins that are on add\n  <plugin> <tool>   Run one of them\n  wait <pane>       Wait for a pane's command to finish\n  worktree-setup    What a project's new worktrees get\n\nClaude Code runs `wings --mcp`, `wings statusline` and `wings hook` itself. Wings has to be open."
    );
    std::process::exit(2);
}
