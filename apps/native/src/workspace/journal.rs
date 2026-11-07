//! Keeps Wings the agents' change journal sink while Claude Code or OpenCode is connected, as the desktop's
//! `journal_sync`: on start and after each Connect or Disconnect of either, Wings adds itself to
//! `~/.config/journal/sink.json` (`wings_connect::journal_sink`), or repoints its entry after the app moved, and takes
//! its entry out once you have disconnected both. A sink of yours is never touched. What a turn's journal then does is
//! `wings journal`'s: it reaches the plugins as their `Journal` hook.

use std::path::{Path, PathBuf};

use wings_connect::journal_sink;

use super::{claude, opencode};

/// Both agents are disconnected when both markers beside the app's state say so, as the desktop's two marker files.
pub(super) fn sync_in(sink: &Path, state: &Path, exe: Option<&Path>) -> Result<bool, String> {
    let both_off = claude::disconnected_marker(state).exists()
        && opencode::disconnected_marker(state).exists();
    journal_sink::sync(sink, exe, both_off)
}

/// The `wings` program the sink runs: only the installed app's, so a build run from `target/` never points the journal
/// at the build folder, and tests never change yours.
fn installed_wings() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| wings_connect::register::installed_wings(&exe))
}

/// Syncs the real sink. Reads and writes a small file, so it runs where the Connect or Disconnect ran, off the window's
/// thread.
pub(super) fn sync(state: Option<&Path>) {
    let (Some(state), Some(exe)) = (state, installed_wings()) else {
        return;
    };
    if let Err(error) = sync_in(&journal_sink::sink_path(), state, Some(&exe)) {
        eprintln!("Couldn't set the journal sink: {error}");
    }
}

/// On start, in a release build only, like the agent connections.
pub(super) fn sync_on_start(state: Option<PathBuf>) {
    if cfg!(debug_assertions) || state.is_none() {
        return;
    }
    std::thread::spawn(move || sync(state.as_deref()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn the_sink_stays_while_either_agent_is_connected_and_goes_once_both_are_disconnected() {
        let dir = std::env::temp_dir().join(format!("wings-native-journal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let state = dir.join("Wings Native/workspace.json");
        std::fs::create_dir_all(state.parent().unwrap()).unwrap();
        let sink = dir.join("config/journal/sink.json");
        std::fs::create_dir_all(sink.parent().unwrap()).unwrap();
        std::fs::write(&sink, r#"{"retention":7}"#).unwrap();
        let exe = Path::new("/Applications/Wings Native.app/Contents/MacOS/wings");
        let read =
            || serde_json::from_str::<Value>(&std::fs::read_to_string(&sink).unwrap()).unwrap();
        let (claude_off, opencode_off) = (
            claude::disconnected_marker(&state),
            opencode::disconnected_marker(&state),
        );

        // Connecting either agent adds Wings beside what was there.
        assert!(sync_in(&sink, &state, Some(exe)).unwrap());
        assert_eq!(
            read(),
            json!({ "retention": 7, "command": [exe, "journal"] })
        );
        // One disconnected: the other still uses it.
        std::fs::write(&claude_off, "").unwrap();
        assert!(!sync_in(&sink, &state, Some(exe)).unwrap());
        assert_eq!(read()["command"], json!([exe, "journal"]));
        // Both: Wings takes only its own entry out.
        std::fs::write(&opencode_off, "").unwrap();
        assert!(sync_in(&sink, &state, Some(exe)).unwrap());
        assert_eq!(read(), json!({ "retention": 7 }));
        // Connecting OpenCode again brings it back.
        std::fs::remove_file(&opencode_off).unwrap();
        assert!(sync_in(&sink, &state, Some(exe)).unwrap());
        assert_eq!(read()["command"], json!([exe, "journal"]));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
