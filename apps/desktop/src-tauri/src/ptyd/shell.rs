//! The program a pane runs and the environment it gets. Built from the app's own environment, so a pane starts the
//! same whether it runs in Wings or in a terminal host that Wings didn't start itself.

use std::{ffi::OsString, path::Path};

use portable_pty::CommandBuilder;

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

/// This process's environment, minus the dev host's session, plus what Wings tells every pane.
pub fn pane_env(pane_id: &str) -> Vec<(OsString, OsString)> {
    let mut env: Vec<(OsString, OsString)> =
        std::env::vars_os().filter(|(key, _)| !key.to_str().is_some_and(is_host_session_var)).collect();
    for (key, value) in [
        ("TERM", "xterm-256color"),
        ("COLORTERM", "truecolor"),
        ("TERM_PROGRAM", "Wings"),
        ("WINGS_ENV", "1"),
        ("WINGS_PANE_ID", pane_id),
    ] {
        env.retain(|(k, _)| k != key);
        env.push((key.into(), value.into()));
    }
    env
}

/// The user's login shell in `cwd`, with `env` as its whole environment, so their PATH and profile load as in any
/// terminal.
pub fn pane_command(cwd: &Path, env: impl IntoIterator<Item = (OsString, OsString)>) -> CommandBuilder {
    let mut cmd = CommandBuilder::new_default_prog();
    cmd.env_clear();
    for (key, value) in env {
        cmd.env(key, value);
    }
    cmd.cwd(cwd);
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrubs_only_host_session_vars() {
        assert!(is_host_session_var("CLAUDE_CODE_CHILD_SESSION"));
        assert!(is_host_session_var("CLAUDECODE"));
        assert!(is_host_session_var("HERDR_PANE_ID"));
        assert!(!is_host_session_var("CLAUDE_CONFIG_DIR"));
        assert!(!is_host_session_var("PATH"));
    }
}
