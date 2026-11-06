//! Refreshes a project's git status as soon as its repo changes, instead of waiting for the next poll.
//! The watching itself is `wings_gitwatch`; this tells the main window which projects moved (`git-changed`),
//! which forwards it to the plugins that may read git. Edits to files in the working tree are not watched;
//! the slow poll in `start_git_status` still picks those up.

use std::path::PathBuf;
use std::sync::OnceLock;
use wings_gitwatch::Watcher;

static WATCHER: OnceLock<Watcher> = OnceLock::new();

/// Tells the watcher the list of projects changed.
pub fn resync() {
    if let Some(watcher) = WATCHER.get() {
        watcher.resync();
    }
}

/// Starts the watcher thread for the app's projects.
pub fn start(app: tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    WATCHER.get_or_init(|| {
        let spaces = {
            let app = app.clone();
            move || {
                let state = app.state::<crate::AppState>();
                let list = state.spaces.lock().unwrap().spaces.iter().map(|s| (s.id.clone(), PathBuf::from(&s.path))).collect();
                list
            }
        };
        Watcher::start(spaces, move |ids| {
            crate::git_refresh_spaces(&app, &ids);
            // After the refresh, so a plugin that reads git then sees what the status shows. One per settled batch.
            let _ = app.emit_to("main", "git-changed", &ids);
        })
    });
}
