//! Puts the Wings MCP server in OpenCode's global config. Wings edits the file itself, since `opencode mcp add`
//! replaces the whole entry and OpenCode has no command to remove one. Fields you set on the entry, like
//! `enabled` or `timeout`, are kept; Wings owns `type` and `command`.

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

/// OpenCode's global config, `opencode.jsonc` when you have one, the way `opencode mcp add --global` picks it.
pub fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| std::env::home_dir().unwrap_or_default().join(".config"));
    let dir = base.join("opencode");
    let jsonc = dir.join("opencode.jsonc");
    if jsonc.exists() { jsonc } else { dir.join("opencode.json") }
}

fn read(path: &Path) -> Result<Map<String, Value>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(Value::Object(config)) => Ok(config),
            _ => Err(format!("{} isn't plain JSON (comments, say), so Wings left it alone", path.display())),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(e.to_string()),
    }
}

/// OpenCode writes this file with two-space indents and keeps its key order, and so does Wings.
fn write(path: &Path, config: &Map<String, Value>) -> Result<(), String> {
    let mut text = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    text.push('\n');
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, text).map_err(|e| e.to_string())
}

/// The Wings entry under `mcp.servers`, if there is one.
pub fn entry(path: &Path) -> Option<Value> {
    read(path).ok()?.get("mcp")?.get("servers")?.get("wings").cloned()
}

/// Whether the Wings entry starts this binary.
pub fn points_at(path: &Path, exe: &Path) -> bool {
    entry(path).is_some_and(|e| e.pointer("/command/0").and_then(Value::as_str) == exe.to_str())
}

/// Adds the Wings server, or repoints the one there. Returns whether the file changed.
pub fn add(path: &Path, exe: &Path, socket: &Path) -> Result<bool, String> {
    let mut config = read(path)?;
    let mcp = config.entry("mcp").or_insert_with(|| json!({}));
    let servers = mcp.as_object_mut().ok_or("`mcp` in OpenCode's config isn't an object")?.entry("servers").or_insert_with(|| json!({}));
    let servers = servers.as_object_mut().ok_or("`mcp.servers` in OpenCode's config isn't an object")?;
    let before = servers.get("wings").cloned();
    let mut wings = before.as_ref().and_then(Value::as_object).cloned().unwrap_or_default();
    wings.insert("type".into(), "local".into());
    wings.insert("command".into(), json!([exe.to_string_lossy(), "--mcp", socket.to_string_lossy()]));
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
    let removed = config.get_mut("mcp").and_then(|m| m.get_mut("servers")).and_then(Value::as_object_mut).and_then(|s| s.shift_remove("wings")).is_some();
    if !removed {
        return Ok(false);
    }
    write(path, &config).map(|_| true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str, text: Option<&str>) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wings-opencode-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("opencode.json");
        let _ = std::fs::remove_file(&path);
        if let Some(text) = text {
            std::fs::write(&path, text).unwrap();
        }
        path
    }

    #[test]
    fn repointing_keeps_your_fields_and_removing_takes_only_wings() {
        let path = temp("keep", Some(r#"{"model":"m","mcp":{"servers":{"x":{"type":"local","command":["a"]},"wings":{"type":"local","command":["/old","--mcp","/s"],"enabled":false}}}}"#));
        assert!(add(&path, Path::new("/new"), Path::new("/s")).unwrap());
        let wings = entry(&path).unwrap();
        assert_eq!(wings["command"], json!(["/new", "--mcp", "/s"]));
        assert_eq!(wings["enabled"], json!(false));
        assert!(!add(&path, Path::new("/new"), Path::new("/s")).unwrap());
        assert!(remove(&path).unwrap());
        let config: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(config, json!({"model":"m","mcp":{"servers":{"x":{"type":"local","command":["a"]}}}}));
    }

    #[test]
    fn leaves_a_file_with_comments_alone() {
        let text = "{\n  // mine\n  \"mcp\": {}\n}\n";
        let path = temp("comments", Some(text));
        assert!(add(&path, Path::new("/new"), Path::new("/s")).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }
}
