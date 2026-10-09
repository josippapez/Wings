//! Puts `wings statusline` in Claude Code's `statusLine` setting without taking it over: the status line you
//! already had runs after Wings, on the same input, so it still shows. Removing Wings puts yours back as it was.

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

const PASS: &str = " statusline --pass | {\n";

pub fn settings_path() -> PathBuf {
    crate::claude::claude_dir().join("settings.json")
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn unquote(s: &str) -> Option<String> {
    Some(s.strip_prefix('\'')?.strip_suffix('\'')?.replace(r"'\''", "'"))
}

/// The command Wings writes. Yours goes in a `{ }` group so it runs in Claude Code's own shell, unchanged.
fn wrap(exe: &Path, yours: Option<&str>) -> String {
    let wings = quote(&exe.to_string_lossy());
    match yours {
        Some(yours) => format!("{wings}{PASS}{yours}\n}}"),
        None => format!("{wings} statusline"),
    }
}

/// The Wings program in a command Wings wrote, and the status line it runs after itself.
fn unwrap(command: &str) -> Option<(String, Option<String>)> {
    let (wings, yours) = match command.split_once(PASS) {
        Some((wings, rest)) => (wings, Some(rest.strip_suffix("\n}")?.to_string())),
        None => (command.strip_suffix(" statusline")?, None),
    };
    let exe = unquote(wings)?;
    (Path::new(&exe).file_stem()? == "wings").then_some((exe, yours))
}

fn read(path: &Path) -> Result<Map<String, Value>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(Value::Object(settings)) => Ok(settings),
            _ => Err(format!("{} isn't valid JSON, so Wings left it alone", path.display())),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(e.to_string()),
    }
}

/// Claude Code writes this file with two-space indents and keeps its key order, and so does Wings.
fn write(path: &Path, settings: &Map<String, Value>) -> Result<(), String> {
    let mut text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    text.push('\n');
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, text).map_err(|e| e.to_string())
}

fn command(settings: &Map<String, Value>) -> Option<&str> {
    let line = settings.get("statusLine")?;
    (line.get("type")?.as_str()? == "command").then_some(())?;
    line.get("command")?.as_str()
}

/// Whether Claude Code's status line goes through Wings.
pub fn connected(path: &Path) -> bool {
    read(path).is_ok_and(|s| command(&s).and_then(unwrap).is_some())
}

/// Puts Wings in front of your status line, or repoints it when Wings moved. Returns whether the file changed.
pub fn add(path: &Path, exe: &Path) -> Result<bool, String> {
    let mut settings = read(path)?;
    let next = match settings.get("statusLine") {
        None => wrap(exe, None),
        Some(_) => {
            let current = command(&settings).ok_or("Claude Code's statusLine isn't a command, so Wings left it alone")?;
            match unwrap(current) {
                Some((was, _)) if Path::new(&was) == exe => return Ok(false),
                Some((_, yours)) => wrap(exe, yours.as_deref()),
                None => wrap(exe, Some(current)),
            }
        }
    };
    let line = settings.entry("statusLine").or_insert_with(|| json!({ "type": "command" }));
    line["command"] = Value::String(next);
    write(path, &settings)?;
    Ok(true)
}

/// Puts back the status line you had before Wings, or removes the one Wings added. Returns whether the file changed.
pub fn remove(path: &Path) -> Result<bool, String> {
    let mut settings = read(path)?;
    let Some((_, yours)) = command(&settings).and_then(unwrap) else { return Ok(false) };
    match yours {
        Some(yours) => settings["statusLine"]["command"] = Value::String(yours),
        None => {
            settings.shift_remove("statusLine");
        }
    }
    write(path, &settings)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HUD: &str = r#"/opt/homebrew/bin/bun --env-file /dev/null "$HOME/.claude/plugins/claude-hud/statusline.mjs""#;

    fn file(name: &str, text: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wings-claude-settings-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn keeps_your_status_line_and_puts_it_back() {
        let before = format!(
            "{}\n",
            serde_json::to_string_pretty(&json!({
                "model": "opus",
                "statusLine": { "type": "command", "command": HUD, "refreshInterval": 5 },
                "outputStyle": "concise"
            }))
            .unwrap()
        );
        let path = file("wrap", &before);
        let exe = Path::new("/Applications/Wings.app/Contents/MacOS/wings");

        assert!(add(&path, exe).unwrap());
        let settings = read(&path).unwrap();
        assert_eq!(settings.keys().collect::<Vec<_>>(), ["model", "statusLine", "outputStyle"]);
        assert_eq!(settings["statusLine"]["refreshInterval"], 5);
        assert_eq!(command(&settings), Some(format!("'{}'{PASS}{HUD}\n}}", exe.display()).as_str()));
        assert!(connected(&path));
        assert!(!add(&path, exe).unwrap(), "a second start changes nothing");

        assert!(remove(&path).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn repoints_a_moved_wings_without_wrapping_twice() {
        let path = file("move", &json!({ "statusLine": { "type": "command", "command": "it's mine" } }).to_string());
        add(&path, Path::new("/old/wings")).unwrap();
        add(&path, Path::new("/new place/Wings.app/Contents/MacOS/wings")).unwrap();
        let settings = read(&path).unwrap();
        assert_eq!(unwrap(command(&settings).unwrap()), Some(("/new place/Wings.app/Contents/MacOS/wings".into(), Some("it's mine".into()))));
    }

    #[test]
    fn adds_one_when_you_had_none_and_removes_it_again() {
        let path = file("none", "{\n  \"model\": \"opus\"\n}\n");
        add(&path, Path::new("/a/wings")).unwrap();
        assert_eq!(command(&read(&path).unwrap()), Some("'/a/wings' statusline"));
        remove(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\n  \"model\": \"opus\"\n}\n");
    }

    #[test]
    fn leaves_a_file_it_cant_read_alone() {
        let path = file("broken", "{ not json");
        assert!(add(&path, Path::new("/a/wings")).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
    }

    #[test]
    fn your_status_line_still_runs_and_gets_the_input() {
        let wings = std::env::temp_dir().join(format!("wings-pass-{}", std::process::id())).join("wings");
        std::fs::create_dir_all(wings.parent().unwrap()).unwrap();
        // Stands in for `wings statusline --pass`, which hands its input on.
        std::fs::write(&wings, "#!/bin/sh\ncat\n").unwrap();
        std::process::Command::new("chmod").arg("+x").arg(&wings).status().unwrap();
        let line = wrap(&wings, Some(r#"read input; echo "yours: $input""#));
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(&line)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                child.stdin.take().unwrap().write_all(b"{\"model\":1}\n")?;
                child.wait_with_output()
            })
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "yours: {\"model\":1}\n");
    }
}
