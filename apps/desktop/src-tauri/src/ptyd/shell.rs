//! The program a pane runs and the environment it gets. Built from the app's own environment, so a pane starts the
//! same whether it runs in Wings or in a terminal host that Wings didn't start itself.

use std::{
    ffi::{OsStr, OsString},
    path::Path,
};

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

/// This process's environment, minus the dev host's session, plus what Wings tells every pane but its id, which
/// `pane_command` adds: in the terminal host, the host numbers the pane. `claude_config_dir` is the folder of the
/// Claude Code account the pane's project uses; without one the pane keeps the app's own, the default account.
pub fn pane_env(claude_config_dir: Option<&Path>) -> Vec<(OsString, OsString)> {
    let mut env: Vec<(OsString, OsString)> =
        std::env::vars_os().filter(|(key, _)| !key.to_str().is_some_and(is_host_session_var)).collect();
    for (key, value) in [("TERM", "xterm-256color"), ("COLORTERM", "truecolor"), ("TERM_PROGRAM", "Wings"), ("WINGS_ENV", "1")] {
        set(&mut env, key, OsStr::new(value));
    }
    if let Some(dir) = claude_config_dir {
        set(&mut env, "CLAUDE_CONFIG_DIR", dir.as_os_str());
    }
    env
}

fn set(env: &mut Vec<(OsString, OsString)>, key: &str, value: &OsStr) {
    env.retain(|(k, _)| k != key);
    env.push((key.into(), value.into()));
}

/// The user's login shell in `cwd`, with `env` and the pane's id as its whole environment, so their PATH and profile
/// load as in any terminal.
pub fn pane_command(cwd: &Path, env: impl IntoIterator<Item = (OsString, OsString)>, pane_id: &str) -> CommandBuilder {
    let mut cmd = CommandBuilder::new_default_prog();
    cmd.env_clear();
    for (key, value) in env {
        cmd.env(key, value);
    }
    cmd.env("WINGS_PANE_ID", pane_id);
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

    #[test]
    fn a_project_account_replaces_the_apps_own_claude_config_dir() {
        let value = |env: &[(OsString, OsString)]| env.iter().filter(|(k, _)| k == "CLAUDE_CONFIG_DIR").map(|(_, v)| v.clone()).collect::<Vec<_>>();
        let account = Path::new("/home/me/.claude-work");
        assert_eq!(value(&pane_env(Some(account))), [OsString::from(account)], "once, whatever the app itself runs under");
        assert!(pane_env(Some(account)).iter().any(|(k, v)| k == "TERM_PROGRAM" && v == "Wings"));
        // Without an account the pane gets what the app has, the default account.
        assert_eq!(value(&pane_env(None)), std::env::var_os("CLAUDE_CONFIG_DIR").into_iter().collect::<Vec<_>>());
    }
}
