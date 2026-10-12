//! macOS keeps another app's data, like Chrome's folder, from anything Wings runs, and for that permission it
//! never asks (tccd: "does not allow prompting; recording denied"). So Wings watches the kernel's refusals and
//! asks you itself, pointing at the switch in Privacy & Security > Files & Folders.

use std::{
    collections::HashSet,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Mutex,
};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// Filtered by `logd`, so the watch costs nothing while nothing is refused.
const PREDICATE: &str = r#"process == "kernel" AND sender == "Sandbox" AND eventMessage BEGINSWITH "System Policy:""#;

pub const SETTINGS_URL: &str = "x-apple.systempreferences:com.apple.preference.security?Privacy_FilesAndFolders";

static WATCH: Mutex<Option<Child>> = Mutex::new(None);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Blocked {
    /// The protected folder, like `~/Library/Application Support/Google/Chrome`.
    pub folder: String,
    /// What Privacy & Security calls it, near enough: the folder's name.
    pub name: String,
    /// The program macOS stopped.
    pub program: String,
}

/// `System Policy: ls(49492) deny(1) file-read-data /Users/you/Library/Application Support/Firefox`
fn parse(message: &str) -> Option<(String, u32, PathBuf)> {
    let rest = message.strip_prefix("System Policy: ")?;
    let open = rest.find('(')?;
    let program = rest[..open].to_string();
    let pid = rest[open + 1..open + rest[open..].find(')')?].parse().ok()?;
    let path = &rest[rest.find(" /")? + 1..];
    Some((program, pid, PathBuf::from(path)))
}

/// The folder under Application Support that Wings itself can't open, or `None` when Wings can read the path,
/// so the refusal was another app's. Chrome keeps its data one level down, in `Google/Chrome`.
fn blocked_folder(support: &Path, path: &Path, readable: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let mut parts = path.strip_prefix(support).ok()?.components();
    let first = support.join(parts.next()?);
    if !readable(&first) {
        return Some(first);
    }
    let second = first.join(parts.next()?);
    (!readable(&second)).then_some(second)
}

/// The program and the folder for a refusal Wings should ask about. Wings' own refusals don't count: checking a
/// folder is itself refused and logged, so they would feed the watch, thousands a second.
fn refused(message: &str, own_pid: u32, support: &Path, readable: impl Fn(&Path) -> bool) -> Option<(String, PathBuf)> {
    let (program, pid, path) = parse(message)?;
    if pid == own_pid {
        return None;
    }
    Some((program, blocked_folder(support, &path, readable)?))
}

pub fn readable(folder: &Path) -> bool {
    std::fs::read_dir(folder).is_ok()
}

pub fn watch(app: AppHandle) {
    let Some(support) = dirs::home_dir().map(|h| h.join("Library/Application Support")) else { return };
    let child = Command::new("/usr/bin/log")
        .args(["stream", "--style", "ndjson", "--predicate", PREDICATE])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else { return };
    let Some(stdout) = child.stdout.take() else { return };
    *WATCH.lock().unwrap() = Some(child);
    std::thread::spawn(move || {
        let mut seen = HashSet::new();
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
            let Some(message) = event["eventMessage"].as_str() else { continue };
            let Some((program, folder)) = refused(message, std::process::id(), &support, readable) else { continue };
            if !seen.insert(folder.clone()) {
                continue;
            }
            let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let _ = app.emit("privacy-blocked", Blocked { folder: folder.to_string_lossy().into_owned(), name, program });
        }
    });
}

/// `log stream` would outlive Wings otherwise.
pub fn stop() {
    if let Some(mut child) = WATCH.lock().unwrap().take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_program_and_path() {
        let line = "System Policy: ls(49492) deny(1) file-read-data /Users/a/Library/Application Support/Google/Chrome/Local State";
        assert_eq!(parse(line), Some(("ls".into(), 49492, PathBuf::from("/Users/a/Library/Application Support/Google/Chrome/Local State"))));
        assert_eq!(parse("3 duplicate reports for System Policy: ls(1) deny(1) file-read-data /x"), None);
    }

    #[test]
    fn ignores_its_own_refusals_without_checking_again() {
        let support = Path::new("/Users/a/Library/Application Support");
        let checked = std::cell::Cell::new(0);
        let readable = |_: &Path| {
            checked.set(checked.get() + 1);
            false
        };
        let own = "System Policy: wings(7) deny(1) file-read-data /Users/a/Library/Application Support/Firefox";
        assert_eq!(refused(own, 7, support, readable), None);
        assert_eq!(checked.get(), 0, "checking would be refused and logged again");
        let other = "System Policy: ls(8) deny(1) file-read-data /Users/a/Library/Application Support/Firefox";
        assert_eq!(refused(other, 7, support, readable), Some(("ls".into(), support.join("Firefox"))));
    }

    #[test]
    fn names_the_folder_wings_cant_open() {
        let support = Path::new("/Users/a/Library/Application Support");
        let locked = |p: &Path| p.ends_with("Google/Chrome") || p.ends_with("Firefox");
        let readable = |p: &Path| !locked(p);
        let chrome = support.join("Google/Chrome/DevToolsActivePort");
        assert_eq!(blocked_folder(support, &chrome, readable), Some(support.join("Google/Chrome")));
        assert_eq!(blocked_folder(support, &support.join("Firefox/Profiles"), readable), Some(support.join("Firefox")));
        assert_eq!(blocked_folder(support, &support.join("Slack/x"), readable), None, "Wings can read it, so another app was refused");
        assert_eq!(blocked_folder(support, Path::new("/private/var/db/x"), readable), None);
    }
}
