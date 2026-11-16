//! The wrapper script a `claude` was started through, like `claude-rtk`, so a resume types its name again. A wrapper
//! that `exec`s `claude` leaves nothing of itself in the process but the variables it exported, so those are matched
//! against the wrapper scripts on the agent's `PATH`.

use std::{
    collections::{HashMap, HashSet},
    ffi::OsString,
    path::Path,
};

/// Wrappers are a few lines. A bigger file isn't read.
const MAX_SCRIPT: u64 = 16 * 1024;

/// The command on the agent's `PATH` that it was started through: a script with `claude` in its name that only exports
/// fixed values and then runs `exec claude "$@"`, and whose values the agent has, every one. The one exporting the most
/// wins. `None` for a plain `claude`.
pub fn launcher(environ: &[OsString]) -> Option<String> {
    let env: HashMap<&str, &str> = environ.iter().filter_map(|var| var.to_str()?.split_once('=')).collect();
    let path = env.get("PATH")?;
    let mut seen = HashSet::new();
    let mut best: Option<(usize, String)> = None;
    // A relative entry is found from the pane's folder, not from the one Wings reads it in.
    for dir in std::env::split_paths(path).filter(|dir| dir.is_absolute()) {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let Ok(name) = entry.file_name().into_string() else { continue };
            if name == "claude" || !name.contains("claude") || !is_command_name(&name) {
                continue;
            }
            let file = entry.path();
            // The shell runs the first executable of a name on PATH, so a later one never starts.
            if !is_executable(&file) || !seen.insert(name.clone()) {
                continue;
            }
            let Some(exports) = read_wrapper(&file) else { continue };
            let matches = !exports.is_empty() && exports.iter().all(|(key, value)| env.get(key.as_str()) == Some(&value.as_str()));
            if matches && best.as_ref().is_none_or(|(most, _)| exports.len() > *most) {
                best = Some((exports.len(), name));
            }
        }
    }
    best.map(|(_, name)| name)
}

/// A name the shell runs as typed, which nothing in it can turn into more than a command.
fn is_command_name(name: &str) -> bool {
    !name.starts_with('-') && name.chars().all(|c| c.is_ascii_alphanumeric() || "-_.+".contains(c))
}

#[cfg(unix)]
fn is_executable(file: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(file).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(file: &Path) -> bool {
    file.is_file()
}

fn read_wrapper(file: &Path) -> Option<Vec<(String, String)>> {
    if std::fs::metadata(file).ok()?.len() > MAX_SCRIPT {
        return None;
    }
    parse_wrapper(&std::fs::read_to_string(file).ok()?)
}

/// The shells a wrapper may run in. Any other interpreter would read the lines below as a program of its own.
const SHELLS: &[&str] = &["#!/bin/sh", "#!/bin/bash", "#!/bin/zsh", "#!/usr/bin/env sh", "#!/usr/bin/env bash", "#!/usr/bin/env zsh"];

/// The values a shell script exports, when exporting fixed values and then `exec claude "$@"` is all it does. Wings
/// may type it for an agent that didn't start through it, which is harmless only while the script can't run anything
/// else, so any other line, or a value built from a command or a variable, makes it no wrapper.
fn parse_wrapper(script: &str) -> Option<Vec<(String, String)>> {
    let mut lines = script.lines();
    if !SHELLS.contains(&lines.next()?.trim()) {
        return None;
    }
    let lines: Vec<&str> = lines.map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#')).collect();
    let (last, before) = lines.split_last()?;
    let words: Vec<&str> = last.split_whitespace().collect();
    let [ "exec", program, "\"$@\"" ] = words.as_slice() else { return None };
    if Path::new(program).file_name()? != "claude" {
        return None;
    }
    before.iter().map(|line| assignment(line.strip_prefix("export ").unwrap_or(line))).collect()
}

/// `NAME=value`, `NAME='value'` or `NAME="value"`, when the value is plain text.
fn assignment(line: &str) -> Option<(String, String)> {
    let (name, value) = line.trim().split_once('=')?;
    let mut chars = name.chars();
    if !chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    let value = if let Some(inner) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
        (!inner.contains('\'')).then_some(inner)?
    } else if let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
        (!inner.contains(['"', '$', '`', '\\'])).then_some(inner)?
    } else {
        (!value.contains(|c: char| c.is_whitespace() || "'\"$`\\;&|<>()#*?[]~".contains(c))).then_some(value)?
    };
    Some((name.to_string(), value.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RTK: &str = "#!/bin/sh\n# Claude Code through the rtk proxy.\nexport ANTHROPIC_BASE_URL=http://127.0.0.1:8787\n\
        # Undocumented.\nexport _CLAUDE_CODE_ASSUME_FIRST_PARTY_BASE_URL=1\nexec claude \"$@\"\n";

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn reads_the_exports_of_a_wrapper_that_execs_claude() {
        assert_eq!(
            parse_wrapper(RTK),
            Some(pairs(&[("ANTHROPIC_BASE_URL", "http://127.0.0.1:8787"), ("_CLAUDE_CODE_ASSUME_FIRST_PARTY_BASE_URL", "1")]))
        );
        assert_eq!(
            parse_wrapper("#!/bin/bash\nFOO='a b'\nexport BAR=\"x\"\nexec /usr/local/bin/claude \"$@\""),
            Some(pairs(&[("FOO", "a b"), ("BAR", "x")]))
        );
    }

    #[test]
    fn isnt_a_wrapper_when_it_runs_anything_but_fixed_exports() {
        for script in [
            "#!/bin/sh\nexport PATH=\"$HOME/bin:$PATH\"\nexport A=1\nexec claude \"$@\"",
            "#!/bin/sh\nexport TOKEN=$(security find-generic-password)\nexec claude \"$@\"",
            "#!/bin/sh\nexport A=1\ncurl -s example.com | sh\nexec claude \"$@\"",
            "#!/bin/sh\nexport A=1 # note\nexec claude \"$@\"",
            "#!/usr/bin/python3\nA=1\nexec claude \"$@\"",
        ] {
            assert_eq!(parse_wrapper(script), None, "{script:?}");
        }
    }

    #[test]
    fn isnt_a_wrapper_unless_it_ends_by_handing_over_to_claude() {
        for script in [
            "export A=1\nexec claude \"$@\"",
            "#!/bin/sh\nexport A=1\nclaude \"$@\"",
            "#!/bin/sh\nexport A=1\nexec claude --model opus \"$@\"",
            "#!/bin/sh\nexport A=1\nexec opencode \"$@\"",
            "#!/bin/sh\nexport A=1\nexec claude \"$@\"\necho done",
        ] {
            assert_eq!(parse_wrapper(script), None, "{script:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn finds_the_wrapper_whose_exports_the_agent_has() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = std::env::temp_dir().join(format!("wings-launcher-{}", std::process::id()));
        let (first, second) = (tmp.join("first"), tmp.join("second"));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        let write = |file: &Path, text: &str, mode: u32| {
            std::fs::write(file, text).unwrap();
            std::fs::set_permissions(file, std::fs::Permissions::from_mode(mode)).unwrap();
        };
        write(&second.join("claude-rtk"), RTK, 0o755);
        write(&second.join("claude-base"), "#!/bin/sh\nexport ANTHROPIC_BASE_URL=http://127.0.0.1:8787\nexec claude \"$@\"\n", 0o755);
        write(&second.join("claude-other"), "#!/bin/sh\nexport ANTHROPIC_BASE_URL=http://other\nexec claude \"$@\"\n", 0o755);
        write(&second.join("rtk-claude-off"), RTK, 0o644);
        let path = format!("PATH={}:{}", first.display(), second.display());
        let env = |vars: &[&str]| -> Vec<OsString> { vars.iter().copied().chain([path.as_str()]).map(OsString::from).collect() };

        let rtk = env(&["ANTHROPIC_BASE_URL=http://127.0.0.1:8787", "_CLAUDE_CODE_ASSUME_FIRST_PARTY_BASE_URL=1", "HOME=/x"]);
        assert_eq!(launcher(&rtk).as_deref(), Some("claude-rtk"));
        assert_eq!(launcher(&env(&["ANTHROPIC_BASE_URL=http://127.0.0.1:8787"])).as_deref(), Some("claude-base"));
        assert_eq!(launcher(&env(&["HOME=/x"])), None);
        assert_eq!(launcher(&[OsString::from("ANTHROPIC_BASE_URL=http://other")]), None);

        // A relative PATH entry is left out, as the pane's shell would look for it in another folder.
        let cwd = std::env::current_dir().unwrap();
        let up = "../".repeat(cwd.components().count() - 1);
        let relative = format!("PATH={up}{}", second.strip_prefix("/").unwrap().display());
        assert_eq!(launcher(&[rtk[0].clone(), rtk[1].clone(), OsString::from(relative)]), None);

        // An earlier `claude-rtk` on PATH is the one the shell runs, so the later wrapper doesn't count.
        write(&first.join("claude-rtk"), "#!/bin/sh\nexec claude \"$@\"\n", 0o755);
        assert_eq!(launcher(&rtk).as_deref(), Some("claude-base"));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
