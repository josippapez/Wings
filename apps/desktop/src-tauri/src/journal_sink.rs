//! Makes Wings the sink of an agent's change journal: `~/.config/journal/sink.json` holds the command the journal
//! runs at the end of each turn, and Wings' own is `<wings> journal`. A sink that is anything else is yours and is
//! never touched. Other keys in the file are kept.

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

pub fn sink_path() -> PathBuf {
    std::env::home_dir().unwrap_or_default().join(".config").join("journal").join("sink.json")
}

/// What the file holds: nothing, or an object. Anything else (not JSON, not an object) is left alone.
fn read(path: &Path) -> Result<Map<String, Value>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(Value::Object(config)) => Ok(config),
            _ => Err(format!("{} isn't a JSON object, so Wings left it alone", path.display())),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(e.to_string()),
    }
}

/// Temp file and rename, so the journal never reads half a file.
fn write(path: &Path, config: &Map<String, Value>) -> Result<(), String> {
    let mut text = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    text.push('\n');
    let dir = path.parent().ok_or("no folder for the journal sink")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let temp = dir.join(format!(".sink.json.{}.tmp", std::process::id()));
    std::fs::write(&temp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&temp, path).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        e.to_string()
    })
}

/// Whether `command` is a Wings binary running `journal`, at whatever path.
fn is_wings(command: Option<&Value>) -> bool {
    let Some(list) = command.and_then(Value::as_array) else { return false };
    matches!(list.as_slice(), [exe, sub] if sub.as_str() == Some("journal") && exe.as_str().is_some_and(|e| Path::new(e).file_stem().is_some_and(|s| s == "wings")))
}

/// Whether the sink is Wings' own, at any path.
pub fn is_ours(path: &Path) -> bool {
    read(path).is_ok_and(|c| is_wings(c.get("command")))
}

/// Makes `exe journal` the sink when there is none or the one there is Wings' (repointed). Returns whether the file changed.
/// A sink of yours, or a file that isn't a JSON object, is left as it is.
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

/// Takes Wings' own sink out, and the file with it when nothing else is in it. Returns whether anything changed.
pub fn remove(path: &Path) -> Result<bool, String> {
    let mut config = read(path)?;
    if !is_wings(config.get("command")) {
        return Ok(false);
    }
    config.shift_remove("command");
    if config.is_empty() {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
    } else {
        write(path, &config)?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str, text: Option<&str>) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wings-journal-{}-{name}", std::process::id()));
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
        assert!(add(&path, Path::new("/Applications/Wings.app/Contents/MacOS/wings")).unwrap());
        assert_eq!(json_of(&path), json!({ "command": ["/Applications/Wings.app/Contents/MacOS/wings", "journal"] }));
        assert!(is_ours(&path));
        assert!(!add(&path, Path::new("/Applications/Wings.app/Contents/MacOS/wings")).unwrap(), "a second add changes nothing");
        let leftovers = std::fs::read_dir(path.parent().unwrap()).unwrap().count();
        assert_eq!(leftovers, 1, "no temp file stays");
    }

    #[test]
    fn wings_entry_at_an_old_path_is_repointed_and_other_keys_stay() {
        let path = temp("moved", Some(r#"{"command":["/old/Wings.app/Contents/MacOS/wings","journal"],"other":{"a":1}}"#));
        assert!(add(&path, Path::new("/new/wings")).unwrap());
        assert_eq!(json_of(&path), json!({ "command": ["/new/wings", "journal"], "other": { "a": 1 } }));
        // Other keys without a command: Wings adds its own and keeps them.
        let path = temp("keys", Some(r#"{"other":true}"#));
        assert!(add(&path, Path::new("/new/wings")).unwrap());
        assert_eq!(json_of(&path), json!({ "other": true, "command": ["/new/wings", "journal"] }));
    }

    #[test]
    fn a_sink_of_yours_is_left_alone() {
        for text in [r#"{"command":["my-sink"]}"#, r#"{"command":["/x/wings","hook"]}"#, r#"{"command":["/x/other","journal"]}"#, r#"{"command":"wings journal"}"#] {
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
        let path = temp("remove", Some(r#"{"command":["/a/wings","journal"],"other":1}"#));
        assert!(remove(&path).unwrap());
        assert_eq!(json_of(&path), json!({ "other": 1 }));
        let alone = temp("alone", Some(r#"{"command":["/a/wings","journal"]}"#));
        assert!(remove(&alone).unwrap());
        assert!(!alone.exists());
        assert!(!remove(&alone).unwrap());
    }
}
