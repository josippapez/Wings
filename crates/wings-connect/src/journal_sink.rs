//! Makes Wings the sink of the agents' change journal, as the desktop's `journal_sink.rs` does, so either Wings app
//! leaves it set up the same way: `~/.config/journal/sink.json` holds the command the journal runs at the end of each
//! turn, and Wings' own is `<wings> journal`. A sink that is anything else is yours and is never touched. Other keys in
//! the file are kept. What `wings journal` then does with a turn's journal is `cli::journal`.

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

pub fn sink_path() -> PathBuf {
    std::env::home_dir()
        .unwrap_or_default()
        .join(".config")
        .join("journal")
        .join("sink.json")
}

/// What the file holds: nothing, or an object. Anything else (not JSON, not an object) is left alone.
fn read(path: &Path) -> Result<Map<String, Value>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(Value::Object(config)) => Ok(config),
            _ => Err(format!(
                "{} isn't a JSON object, so Wings left it alone",
                path.display()
            )),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(e.to_string()),
    }
}

/// Whole or not at all, so the journal never reads half a file, keeping the file's permissions and a link to it.
fn write(path: &Path, config: &Map<String, Value>) -> Result<(), String> {
    let mut text = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    text.push('\n');
    let dir = path.parent().ok_or("no folder for the journal sink")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    crate::write_whole(path, text.as_bytes()).map_err(|e| e.to_string())
}

/// Whether `command` is a Wings binary running `journal`, at whatever path. Cargo's own copy of a build,
/// `target/release/deps/wings-<hash>`, counts too, so the installed app repoints a sink a build once wrote.
fn is_wings(command: Option<&Value>) -> bool {
    let Some(list) = command.and_then(Value::as_array) else {
        return false;
    };
    let wings_stem = |s: &str| {
        s == "wings"
            || s.strip_prefix("wings-")
                .is_some_and(|h| !h.is_empty() && h.chars().all(|c| c.is_ascii_hexdigit()))
    };
    matches!(list.as_slice(), [exe, sub] if sub.as_str() == Some("journal")
        && exe.as_str().is_some_and(|e| Path::new(e).file_stem().and_then(|s| s.to_str()).is_some_and(wings_stem)))
}

/// Makes `exe journal` the sink when there is none or the one there is Wings' (repointed). Returns whether the file
/// changed. A sink of yours, or a file that isn't a JSON object, is left as it is.
pub fn add(path: &Path, exe: &Path) -> Result<bool, String> {
    let mut config = read(path)?;
    if config.get("command").is_some_and(|c| !is_wings(Some(c))) {
        return Ok(false);
    }
    let ours = json!([exe.to_string_lossy(), "journal"]);
    if config.get("command") == Some(&ours) {
        return Ok(false);
    }
    config.insert("command".into(), ours);
    write(path, &config).map(|_| true)
}

/// Takes Wings' own sink out, and the file with it when nothing else is in it. A file that is a link stays a link, with
/// an empty object. Returns whether anything changed.
pub fn remove(path: &Path) -> Result<bool, String> {
    let mut config = read(path)?;
    if !is_wings(config.get("command")) {
        return Ok(false);
    }
    config.shift_remove("command");
    let linked = std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink());
    if config.is_empty() && !linked {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
    } else {
        write(path, &config)?;
    }
    Ok(true)
}

/// What the desktop's `journal_sync` does: Wings is the sink while either agent is connected, and takes its own entry
/// out once you disconnected both. `exe` is the `wings` program to point it at; with none (not the installed app) a
/// sink is only ever taken out. Returns whether the file changed.
pub fn sync(path: &Path, exe: Option<&Path>, both_disconnected: bool) -> Result<bool, String> {
    match exe {
        _ if both_disconnected => remove(path),
        Some(exe) => add(path, exe),
        None => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str, text: Option<&str>) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "wings-connect-journal-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("journal").join("sink.json");
        if let Some(text) = text {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, text).unwrap();
        }
        path
    }

    fn json_of(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn an_absent_file_is_written_with_its_folder() {
        let path = temp("absent", None);
        let exe = Path::new("/Applications/Wings.app/Contents/MacOS/wings");
        assert!(add(&path, exe).unwrap());
        assert_eq!(
            json_of(&path),
            json!({ "command": ["/Applications/Wings.app/Contents/MacOS/wings", "journal"] })
        );
        assert!(!add(&path, exe).unwrap(), "a second add changes nothing");
        let leftovers = std::fs::read_dir(path.parent().unwrap()).unwrap().count();
        assert_eq!(leftovers, 1, "no temp file stays");
    }

    #[test]
    fn wings_entry_at_an_old_path_is_repointed_and_other_keys_stay() {
        let path = temp(
            "moved",
            Some(
                r#"{"command":["/old/Wings.app/Contents/MacOS/wings","journal"],"other":{"a":1}}"#,
            ),
        );
        assert!(add(&path, Path::new("/new/wings")).unwrap());
        assert_eq!(
            json_of(&path),
            json!({ "command": ["/new/wings", "journal"], "other": { "a": 1 } })
        );
        // Written once by a release build from cargo's deps folder.
        let path = temp(
            "deps",
            Some(r#"{"command":["/repo/target/release/deps/wings-224b4daf414bfdd7","journal"]}"#),
        );
        assert!(add(&path, Path::new("/new/wings")).unwrap());
        assert_eq!(
            json_of(&path),
            json!({ "command": ["/new/wings", "journal"] })
        );
        // Other keys without a command: Wings adds its own and keeps them, in their order.
        let path = temp("keys", Some(r#"{"other":true}"#));
        assert!(add(&path, Path::new("/new/wings")).unwrap());
        assert_eq!(
            json_of(&path),
            json!({ "other": true, "command": ["/new/wings", "journal"] })
        );
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.find("\"other\"").unwrap() < text.find("\"command\"").unwrap());
    }

    #[test]
    fn a_sink_of_yours_is_left_alone() {
        for text in [
            r#"{"command":["my-sink"]}"#,
            r#"{"command":["/x/wings","hook"]}"#,
            r#"{"command":["/x/other","journal"]}"#,
            r#"{"command":"wings journal"}"#,
        ] {
            let path = temp("foreign", Some(text));
            assert!(!add(&path, Path::new("/new/wings")).unwrap(), "{text}");
            assert!(!remove(&path).unwrap(), "{text}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        }
        let broken = temp("broken", Some("{ nope"));
        assert!(add(&broken, Path::new("/new/wings")).is_err());
        assert_eq!(std::fs::read_to_string(&broken).unwrap(), "{ nope");
    }

    #[test]
    fn removing_takes_only_wings_own_command() {
        let path = temp(
            "remove",
            Some(r#"{"command":["/a/wings","journal"],"other":1}"#),
        );
        assert!(remove(&path).unwrap());
        assert_eq!(json_of(&path), json!({ "other": 1 }));
        let alone = temp("alone", Some(r#"{"command":["/a/wings","journal"]}"#));
        assert!(remove(&alone).unwrap());
        assert!(!alone.exists());
        assert!(!remove(&alone).unwrap());
    }

    #[test]
    fn syncing_adds_while_an_agent_is_connected_and_removes_once_both_are_not() {
        let path = temp("sync", None);
        let exe = Path::new("/A.app/Contents/MacOS/wings");
        assert!(sync(&path, Some(exe), false).unwrap());
        assert!(!sync(&path, Some(exe), false).unwrap());
        // Not the installed app: nothing to point at, and an entry there stays while an agent is connected.
        assert!(!sync(&path, None, false).unwrap());
        assert!(path.exists());
        assert!(sync(&path, None, true).unwrap());
        assert!(!path.exists());
    }

    /// The file keeps its mode, a new one is yours only, and a link into your dotfiles stays a link.
    #[cfg(unix)]
    #[test]
    fn the_file_keeps_its_mode_and_a_link_stays_a_link() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        let path = temp("mode", None);
        assert!(add(&path, Path::new("/w/wings")).unwrap());
        assert_eq!(mode(&path), 0o600);
        std::fs::write(&path, r#"{"other":1}"#).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(add(&path, Path::new("/w/wings")).unwrap());
        assert_eq!(mode(&path), 0o644);
        assert!(remove(&path).unwrap());
        assert_eq!(mode(&path), 0o644);

        let link = temp("link", None);
        let real = link.with_file_name("dotfiles.json");
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::fs::write(&real, "{}").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let is_link = |p: &Path| {
            std::fs::symlink_metadata(p)
                .unwrap()
                .file_type()
                .is_symlink()
        };
        assert!(add(&link, Path::new("/w/wings")).unwrap());
        assert!(is_link(&link));
        assert_eq!(
            json_of(&real),
            json!({ "command": ["/w/wings", "journal"] })
        );
        assert!(remove(&link).unwrap());
        assert!(is_link(&link), "taking Wings out keeps your link");
        assert_eq!(json_of(&real), json!({}));
    }
}
