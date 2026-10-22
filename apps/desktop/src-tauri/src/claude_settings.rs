//! Puts `wings statusline` in Claude Code's `statusLine` setting without taking it over: the status line you
//! already had runs after Wings, on the same input, so it still shows. It also adds `wings hook` to the
//! `SessionStart` and `PreToolUse` hooks, after the hooks you have. Removing Wings puts yours back as it was.

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

/// Whether `exe` is a Wings program: `wings`, or a copy Cargo keeps in `target/*/deps` as `wings-<hash>`. Such a copy
/// once wrote itself in, and read as someone else's command it stayed there, wrapped, after `cargo clean` removed it.
fn is_wings(exe: &str) -> bool {
    let Some(stem) = Path::new(exe).file_stem().and_then(|s| s.to_str()) else { return false };
    stem == "wings" || stem.strip_prefix("wings-").is_some_and(|hash| !hash.is_empty() && hash.bytes().all(|b| b.is_ascii_hexdigit()))
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
    is_wings(&exe).then_some((exe, yours))
}

/// Your status line with any Wings wrappers peeled off it: one Wings in front is all it needs.
fn yours_only(mut yours: Option<String>) -> Option<String> {
    while let Some((_, inner)) = yours.as_deref().and_then(unwrap) {
        yours = inner;
    }
    yours
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

/// The hooks Wings adds, with the matcher each one has. `PreToolUse` is only for Bash: Wings starts in 20 to
/// 60 ms, which every tool call shouldn't wait for.
const HOOKS: [(&str, Option<&str>); 2] = [("SessionStart", None), ("PreToolUse", Some("Bash"))];
/// Claude Code's own cap is 600 s; Wings answers within 2 s or says nothing.
const HOOK_TIMEOUT: u64 = 5;

/// The Wings program in a hook command Wings wrote.
fn hook_exe(command: &str) -> Option<String> {
    let exe = unquote(command.strip_suffix(" hook")?)?;
    is_wings(&exe).then_some(exe)
}

/// The hooks of `event`'s entries that run `wings hook`.
fn hook_commands<'a>(settings: &'a mut Map<String, Value>, event: &str) -> Vec<&'a mut Value> {
    let Some(Value::Array(entries)) = settings.get_mut("hooks").and_then(|h| h.get_mut(event)) else { return vec![] };
    entries
        .iter_mut()
        .filter_map(|entry| entry.get_mut("hooks")?.as_array_mut())
        .flatten()
        .filter(|hook| hook.get("command").and_then(Value::as_str).and_then(hook_exe).is_some())
        .collect()
}

/// Whether Claude Code's status line and hooks go through Wings.
pub fn connected(path: &Path) -> bool {
    read(path).is_ok_and(|mut s| {
        command(&s).and_then(unwrap).is_some() && HOOKS.iter().all(|(event, _)| !hook_commands(&mut s, event).is_empty())
    })
}

fn add_statusline(settings: &mut Map<String, Value>, exe: &Path) -> Result<bool, String> {
    let next = match settings.get("statusLine") {
        None => wrap(exe, None),
        Some(_) => {
            let current = command(settings).ok_or("Claude Code's statusLine isn't a command, so Wings left it alone")?;
            match unwrap(current) {
                Some((was, yours)) if Path::new(&was) == exe && yours_only(yours.clone()) == yours => return Ok(false),
                Some((_, yours)) => wrap(exe, yours_only(yours).as_deref()),
                None => wrap(exe, Some(current)),
            }
        }
    };
    let line = settings.entry("statusLine").or_insert_with(|| json!({ "type": "command" }));
    line["command"] = Value::String(next);
    Ok(true)
}

/// Adds `wings hook` after the hooks you have for each event, or repoints it when Wings moved.
fn add_hooks(settings: &mut Map<String, Value>, exe: &Path) -> Result<bool, String> {
    let ours = format!("{} hook", quote(&exe.to_string_lossy()));
    let mut changed = false;
    for (event, matcher) in HOOKS {
        let found = hook_commands(settings, event);
        if !found.is_empty() {
            // The first stays, repointed; any other Wings hook would run Wings twice, or a program that is gone.
            let extra = found.len() > 1;
            if let Some(first) = found.into_iter().next() {
                if first["command"] != ours.as_str() {
                    first["command"] = Value::String(ours.clone());
                    changed = true;
                }
            }
            if extra {
                drop_extra_hooks(settings, event);
                changed = true;
            }
            continue;
        }
        let hooks = settings.entry("hooks").or_insert_with(|| json!({}));
        let entries = hooks
            .as_object_mut()
            .ok_or("Claude Code's hooks isn't an object, so Wings left it alone")?
            .entry(event)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or_else(|| format!("Claude Code's {event} hooks isn't a list, so Wings left it alone"))?;
        let mut entry = Map::new();
        if let Some(matcher) = matcher {
            entry.insert("matcher".into(), json!(matcher));
        }
        entry.insert("hooks".into(), json!([{ "type": "command", "command": ours, "timeout": HOOK_TIMEOUT }]));
        entries.push(Value::Object(entry));
        changed = true;
    }
    Ok(changed)
}

/// Leaves only the first `wings hook` of `event`, and drops an entry that held only one of the others.
fn drop_extra_hooks(settings: &mut Map<String, Value>, event: &str) {
    let Some(Value::Array(entries)) = settings.get_mut("hooks").and_then(|h| h.get_mut(event)) else { return };
    let mut seen = false;
    for entry in entries.iter_mut() {
        if let Some(list) = entry.get_mut("hooks").and_then(Value::as_array_mut) {
            list.retain(|hook| {
                let ours = hook.get("command").and_then(Value::as_str).and_then(hook_exe).is_some();
                let keep = !ours || !seen;
                seen |= ours;
                keep
            });
        }
    }
    entries.retain(|entry| entry.get("hooks").and_then(Value::as_array).is_none_or(|list| !list.is_empty()));
}

/// Takes `wings hook` out of each event, and an entry or event that held only that. Yours stay as they were.
fn remove_hooks(settings: &mut Map<String, Value>) -> bool {
    let mut changed = false;
    for (event, _) in HOOKS {
        let Some(Value::Array(entries)) = settings.get_mut("hooks").and_then(|h| h.get_mut(event)) else { continue };
        let is_ours = |hook: &Value| hook.get("command").and_then(Value::as_str).and_then(hook_exe).is_some();
        for entry in entries.iter_mut() {
            if let Some(list) = entry.get_mut("hooks").and_then(Value::as_array_mut) {
                let had = list.len();
                list.retain(|hook| !is_ours(hook));
                changed |= list.len() != had;
            }
        }
        entries.retain(|entry| entry.get("hooks").and_then(Value::as_array).is_none_or(|list| !list.is_empty()));
        if entries.is_empty() {
            if let Some(Value::Object(hooks)) = settings.get_mut("hooks") {
                hooks.shift_remove(event);
            }
        }
    }
    if settings.get("hooks").and_then(Value::as_object).is_some_and(|h| h.is_empty()) && changed {
        settings.shift_remove("hooks");
    }
    changed
}

/// Puts Wings in front of your status line and after your hooks, or repoints both when Wings moved. Returns
/// whether the file changed.
pub fn add(path: &Path, exe: &Path) -> Result<bool, String> {
    let mut settings = read(path)?;
    let line = add_statusline(&mut settings, exe)?;
    let hooks = add_hooks(&mut settings, exe)?;
    if !(line || hooks) {
        return Ok(false);
    }
    write(path, &settings)?;
    Ok(true)
}

/// Puts back the status line and hooks you had before Wings, or removes the ones Wings added. Returns whether the
/// file changed.
pub fn remove(path: &Path) -> Result<bool, String> {
    let mut settings = read(path)?;
    let mut changed = false;
    if let Some((_, yours)) = command(&settings).and_then(unwrap) {
        match yours {
            Some(yours) => settings["statusLine"]["command"] = Value::String(yours),
            None => {
                settings.shift_remove("statusLine");
            }
        }
        changed = true;
    }
    changed |= remove_hooks(&mut settings);
    if changed {
        write(path, &settings)?;
    }
    Ok(changed)
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
        assert_eq!(settings.keys().collect::<Vec<_>>(), ["model", "statusLine", "outputStyle", "hooks"]);
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
    fn hooks_go_after_yours_once_and_come_off_again() {
        let before = format!(
            "{}\n",
            serde_json::to_string_pretty(&json!({
                "model": "opus",
                "hooks": {
                    "PreToolUse": [
                        { "matcher": "Bash", "hooks": [{ "type": "command", "command": "rtk hook claude" }] },
                        { "matcher": "Edit", "hooks": [{ "type": "command", "command": "fmt" }] }
                    ],
                    "PostToolUse": [{ "hooks": [{ "type": "command", "command": "after" }] }]
                },
                "outputStyle": "concise"
            }))
            .unwrap()
        );
        let path = file("hooks", &before);
        let exe = Path::new("/Applications/Wings.app/Contents/MacOS/wings");

        assert!(add(&path, exe).unwrap());
        let settings = read(&path).unwrap();
        assert_eq!(settings.keys().collect::<Vec<_>>(), ["model", "hooks", "outputStyle", "statusLine"]);
        let pre = settings["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 3);
        assert_eq!(pre[0]["hooks"][0]["command"], "rtk hook claude");
        assert_eq!(pre[1]["hooks"][0]["command"], "fmt");
        assert_eq!(pre[2], json!({ "matcher": "Bash", "hooks": [{ "type": "command", "command": format!("'{}' hook", exe.display()), "timeout": 5 }] }));
        assert_eq!(settings["hooks"]["SessionStart"].as_array().unwrap().len(), 1);
        assert_eq!(settings["hooks"]["SessionStart"][0].get("matcher"), None);
        assert_eq!(settings["hooks"]["PostToolUse"], json!([{ "hooks": [{ "type": "command", "command": "after" }] }]));
        // `Journal` is a plugin hook event, not a Claude Code one: Wings never registers it there.
        assert_eq!(settings["hooks"].as_object().unwrap().keys().collect::<Vec<_>>(), ["PreToolUse", "PostToolUse", "SessionStart"]);
        assert!(connected(&path));
        assert!(!add(&path, exe).unwrap(), "a second add changes nothing");

        // Moved: repointed, not duplicated.
        let moved = Path::new("/new place/Wings.app/Contents/MacOS/wings");
        assert!(add(&path, moved).unwrap());
        let settings = read(&path).unwrap();
        assert_eq!(settings["hooks"]["PreToolUse"].as_array().unwrap().len(), 3);
        assert_eq!(settings["hooks"]["PreToolUse"][2]["hooks"][0]["command"], format!("'{}' hook", moved.display()));

        assert!(remove(&path).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        assert!(!connected(&path));
    }

    #[test]
    fn hooks_added_to_a_file_without_any_leave_no_trace() {
        let path = file("nohooks", "{\n  \"model\": \"opus\"\n}\n");
        add(&path, Path::new("/a/wings")).unwrap();
        assert_eq!(read(&path).unwrap()["hooks"]["SessionStart"][0]["hooks"][0]["command"], "'/a/wings' hook");
        remove(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\n  \"model\": \"opus\"\n}\n");
    }

    /// What a build in `target/release/deps` left behind: wrapped inside the app's status line and in a hook of
    /// its own, both running a program `cargo clean` had removed.
    #[test]
    fn a_build_copy_left_in_the_settings_is_taken_out() {
        let app = Path::new("/Applications/Wings.app/Contents/MacOS/wings");
        let build = "/w/target/release/deps/wings-224b4daf414bfdd7";
        let nested = format!("'{}'{PASS}'{build}'{PASS}{HUD}\n}}\n}}", app.display());
        let hook = |exe: &str| json!({ "hooks": [{ "type": "command", "command": format!("'{exe}' hook"), "timeout": 5 }] });
        let bash = |exe: &str| json!({ "matcher": "Bash", "hooks": [{ "type": "command", "command": format!("'{exe}' hook"), "timeout": 5 }] });
        let path = file(
            "build-copy",
            &json!({
                "statusLine": { "type": "command", "command": nested },
                "hooks": { "SessionStart": [hook(build), hook(&app.to_string_lossy())], "PreToolUse": [bash(build), bash(&app.to_string_lossy())] }
            })
            .to_string(),
        );
        assert!(add(&path, app).unwrap());
        let settings = read(&path).unwrap();
        assert_eq!(command(&settings), Some(format!("'{}'{PASS}{HUD}\n}}", app.display()).as_str()));
        assert_eq!(settings["hooks"]["SessionStart"], json!([hook(&app.to_string_lossy())]));
        assert_eq!(settings["hooks"]["PreToolUse"], json!([bash(&app.to_string_lossy())]));
        assert!(!add(&path, app).unwrap(), "a second start changes nothing");
        assert!(!is_wings("/usr/bin/wingspan") && !is_wings("/x/wings-tool") && is_wings("/x/wings-0af"));
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
