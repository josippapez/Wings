//! Puts the Wings MCP server in OpenCode's global config, as the desktop's `opencode_config.rs` does, so either Wings
//! app leaves OpenCode set up the same way. Wings edits the file itself, since `opencode mcp add` replaces the whole
//! entry and OpenCode has no command to remove one. Fields you set on the entry, like `enabled` or `timeout`, are
//! kept; Wings owns `type` and `command`. A file with comments is left alone rather than rewritten without them.

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

/// You disconnected OpenCode, so Wings doesn't connect it again when it starts: the name of the file in the app's data
/// folder that says so, the desktop's `OPENCODE_DISCONNECTED`.
pub const DISCONNECTED: &str = "opencode-disconnected";

/// Where OpenCode stands with Wings, as the desktop's `OpencodeMcpStatus`.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Status {
    /// OpenCode's CLI is installed.
    pub opencode: bool,
    /// Its global config has the Wings MCP server.
    pub connected: bool,
}

/// OpenCode's global config under `XDG_CONFIG_HOME`, else `~/.config`: `opencode.jsonc` when you have one, the way
/// `opencode mcp add --global` picks it.
pub fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::home_dir().unwrap_or_default().join(".config"));
    config_in(&base)
}

/// The config in this config folder, the one `config_path` uses.
pub fn config_in(base: &Path) -> PathBuf {
    let dir = base.join("opencode");
    let jsonc = dir.join("opencode.jsonc");
    if jsonc.exists() {
        jsonc
    } else {
        dir.join("opencode.json")
    }
}

fn read(path: &Path) -> Result<Map<String, Value>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(Value::Object(config)) => Ok(config),
            _ => Err(format!(
                "{} isn't plain JSON (comments, say), so Wings left it alone",
                path.display()
            )),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(e.to_string()),
    }
}

/// OpenCode writes this file with two-space indents and keeps its key order, and so does Wings. Written beside it and
/// renamed, so a crash mid-write can't leave half your config.
fn write(path: &Path, config: &Map<String, Value>) -> Result<(), String> {
    let mut text = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    text.push('\n');
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    crate::write_whole(path, text.as_bytes()).map_err(|e| e.to_string())
}

/// The Wings entry under `mcp.servers`, if there is one.
pub fn entry(path: &Path) -> Option<Value> {
    read(path)
        .ok()?
        .get("mcp")?
        .get("servers")?
        .get("wings")
        .cloned()
}

/// Whether the Wings entry starts this program.
pub fn points_at(path: &Path, exe: &Path) -> bool {
    entry(path).is_some_and(|e| e.pointer("/command/0").and_then(Value::as_str) == exe.to_str())
}

/// Adds the Wings server, `exe --mcp socket`, or repoints the one there. Returns whether the file changed.
pub fn add(path: &Path, exe: &Path, socket: &Path) -> Result<bool, String> {
    let mut config = read(path)?;
    let mcp = config.entry("mcp").or_insert_with(|| json!({}));
    let servers = mcp
        .as_object_mut()
        .ok_or("`mcp` in OpenCode's config isn't an object")?
        .entry("servers")
        .or_insert_with(|| json!({}));
    let servers = servers
        .as_object_mut()
        .ok_or("`mcp.servers` in OpenCode's config isn't an object")?;
    let before = servers.get("wings").cloned();
    let mut wings = before
        .as_ref()
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    wings.insert("type".into(), "local".into());
    wings.insert(
        "command".into(),
        json!([exe.to_string_lossy(), "--mcp", socket.to_string_lossy()]),
    );
    let wings = Value::Object(wings);
    if before.as_ref() == Some(&wings) {
        return Ok(false);
    }
    servers.insert("wings".into(), wings);
    write(path, &config).map(|_| true)
}

/// Takes the Wings server out. Returns whether the file changed.
pub fn remove(path: &Path) -> Result<bool, String> {
    let mut config = read(path)?;
    let removed = config
        .get_mut("mcp")
        .and_then(|m| m.get_mut("servers"))
        .and_then(Value::as_object_mut)
        .and_then(|s| s.shift_remove("wings"))
        .is_some();
    if !removed {
        return Ok(false);
    }
    write(path, &config).map(|_| true)
}

/// OpenCode's background service reads its config only on start or `opencode reload`. Without a running service
/// there's nothing to reload, and the next one reads the file. Returns why it failed, if it did.
pub fn reload(opencode: &Path) -> Result<(), String> {
    let out = std::process::Command::new(opencode)
        .arg("reload")
        .current_dir(std::env::home_dir().unwrap_or_default())
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str, text: Option<&str>) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "wings-connect-opencode-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let path = config_in(&dir);
        if let Some(text) = text {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, text).unwrap();
        }
        path
    }

    #[test]
    fn repointing_keeps_your_fields_and_removing_takes_only_wings() {
        let path = temp(
            "keep",
            Some(
                r#"{"model":"m","mcp":{"servers":{"x":{"type":"local","command":["a"]},"wings":{"type":"local","command":["/old","--mcp","/s"],"enabled":false}}}}"#,
            ),
        );
        assert!(add(&path, Path::new("/new"), Path::new("/s")).unwrap());
        let wings = entry(&path).unwrap();
        assert_eq!(wings["command"], json!(["/new", "--mcp", "/s"]));
        assert_eq!(wings["enabled"], json!(false));
        assert!(points_at(&path, Path::new("/new")) && !points_at(&path, Path::new("/old")));
        // Adding again changes nothing, not even the file.
        let before = std::fs::read_to_string(&path).unwrap();
        assert!(!add(&path, Path::new("/new"), Path::new("/s")).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        assert!(remove(&path).unwrap());
        assert!(!remove(&path).unwrap());
        let config: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            config,
            json!({"model":"m","mcp":{"servers":{"x":{"type":"local","command":["a"]}}}})
        );
        // Your keys stay in the order you wrote them.
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.find("\"model\"").unwrap() < text.find("\"mcp\"").unwrap());
        assert!(!path.with_extension("json.wings-tmp").exists());
    }

    #[test]
    fn a_missing_config_gets_one_with_only_wings() {
        let path = temp("new", None);
        assert_eq!(entry(&path), None);
        assert!(add(&path, Path::new("/Apps/wings"), Path::new("/data/mcp.sock")).unwrap());
        let config: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            config,
            json!({"mcp":{"servers":{"wings":{"type":"local","command":["/Apps/wings","--mcp","/data/mcp.sock"]}}}})
        );
        assert!(path.ends_with("opencode/opencode.json"));
    }

    /// OpenCode's config can hold provider keys: one only you can read stays so, and a new one is yours only.
    #[cfg(unix)]
    #[test]
    fn the_config_keeps_its_permissions_and_a_new_one_is_yours_only() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        let path = temp(
            "mode",
            Some(r#"{"provider":{"x":{"options":{"apiKey":"secret"}}}}"#),
        );
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(add(&path, Path::new("/w"), Path::new("/s")).unwrap());
        assert_eq!(mode(&path), 0o600);
        assert!(remove(&path).unwrap());
        assert_eq!(mode(&path), 0o600);
        // A config others may read keeps that too: Wings doesn't change what you chose.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(add(&path, Path::new("/w"), Path::new("/s")).unwrap());
        assert_eq!(mode(&path), 0o644);
        let new = temp("mode-new", None);
        assert!(add(&new, Path::new("/w"), Path::new("/s")).unwrap());
        assert_eq!(mode(&new), 0o600);
    }

    /// A config that is a link into your dotfiles stays a link, and the file it points at gets the change.
    #[cfg(unix)]
    #[test]
    fn a_linked_config_stays_a_link() {
        let path = temp("link", None);
        let real = path.with_file_name("dotfiles.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&real, r#"{"model":"m"}"#).unwrap();
        std::os::unix::fs::symlink(&real, &path).unwrap();
        assert!(add(&path, Path::new("/w"), Path::new("/s")).unwrap());
        assert!(std::fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(entry(&real).is_some());
    }

    #[test]
    fn leaves_a_file_with_comments_alone() {
        let text = "{\n  // mine\n  \"mcp\": {}\n}\n";
        let dir = std::env::temp_dir().join(format!(
            "wings-connect-opencode-{}-comments",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("opencode")).unwrap();
        std::fs::write(dir.join("opencode/opencode.jsonc"), text).unwrap();
        // With a `.jsonc` there, that is the one Wings reads, as `opencode mcp add --global` does.
        let path = config_in(&dir);
        assert!(path.ends_with("opencode.jsonc"));
        assert!(add(&path, Path::new("/new"), Path::new("/s")).is_err());
        assert!(remove(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }
}
