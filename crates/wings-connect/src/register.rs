//! Adding Wings to Claude Code and taking it out again, per account: the MCP server `wings --mcp <socket>` registered at
//! user scope with `claude mcp`, and the status line and hooks in its settings (settings.rs). Every Wings app does this
//! the same way, so Claude Code's config doesn't change when one Wings app replaces another.

use std::{
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use serde_json::{json, Value};
use wings_spaces::{claude_accounts, spaces::Space};

use crate::settings;

/// Where Claude Code stands with Wings, in the default account.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// Claude Code's CLI is installed.
    pub claude: bool,
    /// The Wings MCP server is registered with it.
    pub connected: bool,
    /// Claude Code's status line and hooks go through Wings, in front of yours.
    pub statusline: bool,
    /// The server's tools load at session start instead of waiting behind Claude's tool search.
    pub always_load: bool,
}

/// You disconnected Claude Code, so Wings doesn't connect it again when it starts: the name of the file in the app's
/// data folder that says so.
pub const DISCONNECTED: &str = "claude-disconnected";

/// `claude` for an account: a named one runs under its own config folder, the default one under whatever Wings runs under.
pub fn claude_command(claude: &Path, account: Option<&str>) -> Command {
    let mut command = Command::new(claude);
    if let Some(name) = account {
        command.env("CLAUDE_CONFIG_DIR", claude_accounts::dir(Some(name)));
    }
    command
}

pub fn claude_mcp(claude: &Path, account: Option<&str>, args: &[&str]) -> std::io::Result<Output> {
    claude_command(claude, account).arg("mcp").args(args).stdin(Stdio::null()).output()
}

/// Whether Claude Code has the Wings MCP server and status line. Reading Claude's own list keeps Wings from guessing.
pub fn status(account: Option<&str>) -> Status {
    let statusline = settings::connected(&settings::settings_path(account));
    let always_load = wings_mcp_entry(&claude_accounts::user_config(account)).is_some_and(|e| e["alwaysLoad"] == true);
    let Some(claude) = wings_plugins::exec::find_program("claude") else { return Status { claude: false, connected: false, statusline, always_load } };
    let connected = claude_mcp(&claude, account, &["get", "wings"]).is_ok_and(|out| out.status.success());
    Status { claude: true, connected, statusline, always_load }
}

/// The JSON for re-adding the Wings server with the fields you set on it kept, and `alwaysLoad` set when given,
/// or `None` when there are none and a plain `claude mcp add` will do. Wings owns `type`, `command`, `args` and `env`.
pub fn mcp_add_json(existing: Option<&Value>, always_load: Option<bool>, exe: &Path, socket: &Path) -> Option<Value> {
    let mut entry = existing.and_then(|e| e.as_object()).cloned().unwrap_or_default();
    for owned in ["type", "command", "args", "env"] {
        entry.remove(owned);
    }
    match always_load {
        Some(true) => _ = entry.insert("alwaysLoad".into(), true.into()),
        Some(false) => _ = entry.remove("alwaysLoad"),
        None => {}
    }
    if entry.is_empty() {
        return None;
    }
    entry.insert("type".into(), "stdio".into());
    entry.insert("command".into(), exe.to_string_lossy().into());
    entry.insert("args".into(), json!(["--mcp", socket.to_string_lossy()]));
    entry.insert("env".into(), json!({}));
    Some(entry.into())
}

pub fn wings_mcp_entry(config: &Path) -> Option<Value> {
    let config: Value = serde_json::from_str(&std::fs::read_to_string(config).ok()?).ok()?;
    config.pointer("/mcpServers/wings").cloned()
}

/// Registers `wings --mcp` with Claude Code at user scope, so every session gets the plugin tools, and puts `wings
/// statusline` in front of your status line, in each of `accounts`. Every one is tried, and the first to fail is the one
/// reported.
pub fn connect(claude: &Path, exe: &Path, socket: &Path, accounts: &[Option<String>], always_load: Option<bool>) -> Result<(), String> {
    each(accounts, |account| connect_account(claude, exe, socket, account, always_load))
}

/// `connect` for one account. The server is removed first so a moved app is repointed rather than left dangling.
pub fn connect_account(claude: &Path, exe: &Path, socket: &Path, account: Option<&str>, always_load: Option<bool>) -> Result<(), String> {
    // Read before the remove, which drops the entry: what you set on it yourself (`alwaysLoad`, say) goes back on.
    let kept = mcp_add_json(wings_mcp_entry(&claude_accounts::user_config(account)).as_ref(), always_load, exe, socket);
    let _ = claude_mcp(claude, account, &["remove", "--scope", "user", "wings"]);
    let mut add = claude_command(claude, account);
    match &kept {
        Some(entry) => add.args(["mcp", "add-json", "--scope", "user", "wings", &entry.to_string()]),
        None => add.args(["mcp", "add", "--scope", "user", "--transport", "stdio", "wings", "--"]).arg(exe).arg("--mcp").arg(socket),
    };
    let out = add.stdin(Stdio::null()).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    settings::add(&settings::settings_path(account), exe)?;
    Ok(())
}

/// Makes sure Claude Code, under `account`, has Wings: registered, and in front of your status line and after your hooks.
pub fn ensure_connected(claude: &Path, exe: &Path, socket: &Path, account: Option<&str>) -> Result<(), String> {
    let registered = claude_mcp(claude, account, &["get", "wings"])
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).lines().any(|l| l.trim() == format!("Command: {}", exe.display())));
    if registered {
        settings::add(&settings::settings_path(account), exe).map(|_| ())
    } else {
        connect_account(claude, exe, socket, account, None)
    }
}

/// Takes Wings back out of Claude Code: the MCP server goes, and your own status line and hooks are put back as they
/// were, in every account it is in. `claude` is `None` when Claude Code isn't installed any more.
pub fn disconnect(claude: Option<&Path>, spaces: &[Space]) -> Result<(), String> {
    each(&disconnect_targets(spaces), |account| disconnect_account(claude, account))
}

/// The accounts to take Wings out of: those it connects to, and any other with Wings registered, like one that no
/// project uses any more.
pub fn disconnect_targets(spaces: &[Space]) -> Vec<Option<String>> {
    let mut accounts = claude_accounts::connect_targets(spaces);
    for name in claude_accounts::existing_names() {
        let account = Some(name);
        if !accounts.contains(&account) && wings_mcp_entry(&claude_accounts::user_config(account.as_deref())).is_some() {
            accounts.push(account);
        }
    }
    accounts
}

pub fn disconnect_account(claude: Option<&Path>, account: Option<&str>) -> Result<(), String> {
    // `claude mcp` creates its config files in a folder that has none, so a named account without the server is left alone.
    let registered = account.is_none() || wings_mcp_entry(&claude_accounts::user_config(account)).is_some();
    if let Some(claude) = claude.filter(|_| registered) {
        claude_mcp(claude, account, &["remove", "--scope", "user", "wings"]).map_err(|e| e.to_string())?;
    }
    settings::remove(&settings::settings_path(account))?;
    Ok(())
}

/// Runs `task` for each account, and reports the first that failed, named.
fn each(accounts: &[Option<String>], task: impl Fn(Option<&str>) -> Result<(), String>) -> Result<(), String> {
    let mut failed = None;
    for account in accounts {
        if let Err(e) = task(account.as_deref()) {
            failed.get_or_insert(match account {
                Some(name) => format!("{name}: {e}"),
                None => e,
            });
        }
    }
    failed.map_or(Ok(()), Err)
}

/// Whether this is an installed app, `<name>.app/Contents/MacOS/<binary>`, the only copy that may write itself into
/// the agents' configs on start. A release binary run from `target/` would point them at the build folder.
pub fn installed_app(exe: &Path) -> bool {
    let mut parents = exe.ancestors().skip(1);
    let (macos, contents, bundle) = (parents.next(), parents.next(), parents.next());
    macos.and_then(|p| p.file_name()).is_some_and(|n| n == "MacOS")
        && contents.and_then(|p| p.file_name()).is_some_and(|n| n == "Contents")
        && bundle.and_then(|p| p.extension()).is_some_and(|e| e == "app")
}

/// The `wings` program beside `exe` in an installed app, the one Claude Code's config points at.
pub fn installed_wings(exe: &Path) -> Option<PathBuf> {
    Some(exe.with_file_name("wings")).filter(|wings| installed_app(wings))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_installed_app_connects_the_agents_on_start() {
        assert!(installed_app(Path::new("/Applications/Wings.app/Contents/MacOS/wings")));
        // A release binary in the build folder, the test harness, or a binary copied anywhere else does not.
        for other in ["/w/apps/desktop/src-tauri/target/release/deps/wings-224b4daf414bfdd7", "/w/target/release/wings", "/usr/local/bin/wings", "/x/MacOS/wings", "wings"] {
            assert!(!installed_app(Path::new(other)), "{other}");
        }
        assert_eq!(installed_wings(Path::new("/Applications/W.app/Contents/MacOS/wings-native")), Some("/Applications/W.app/Contents/MacOS/wings".into()));
        assert_eq!(installed_wings(Path::new("/w/target/release/wings-native")), None);
    }

    #[test]
    fn re_registering_keeps_what_you_set_on_the_server() {
        let (exe, socket) = (Path::new("/Apps/Wings"), Path::new("/data/mcp.sock"));
        let old = json!({ "type": "stdio", "command": "/Old/Wings", "args": ["--mcp", "/old.sock"], "env": {}, "alwaysLoad": true });
        assert_eq!(
            mcp_add_json(Some(&old), None, exe, socket),
            Some(json!({ "type": "stdio", "command": "/Apps/Wings", "args": ["--mcp", "/data/mcp.sock"], "env": {}, "alwaysLoad": true }))
        );
        // Nothing of yours on it, or no entry at all: the plain `claude mcp add`.
        let plain = json!({ "type": "stdio", "command": "/Old/Wings", "args": ["--mcp", "/old.sock"], "env": {} });
        assert_eq!(mcp_add_json(Some(&plain), None, exe, socket), None);
        assert_eq!(mcp_add_json(None, None, exe, socket), None);
    }

    #[test]
    fn the_always_load_switch_sets_or_clears_only_that_field() {
        let (exe, socket) = (Path::new("/Apps/Wings"), Path::new("/data/mcp.sock"));
        let plain = json!({ "type": "stdio", "command": "/Old/Wings", "args": ["--mcp", "/old.sock"], "env": {}, "timeout": 9 });
        assert_eq!(
            mcp_add_json(Some(&plain), Some(true), exe, socket),
            Some(json!({ "type": "stdio", "command": "/Apps/Wings", "args": ["--mcp", "/data/mcp.sock"], "env": {}, "timeout": 9, "alwaysLoad": true }))
        );
        assert_eq!(mcp_add_json(None, Some(true), exe, socket).unwrap()["alwaysLoad"], true);
        let on = json!({ "type": "stdio", "command": "/Old/Wings", "args": [], "env": {}, "alwaysLoad": true });
        assert_eq!(mcp_add_json(Some(&on), Some(false), exe, socket), None);
    }
}
