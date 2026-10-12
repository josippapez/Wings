//! The global shortcut that brings Wings forward from any app and jumps to the agent that needs you. It is
//! registered only while the setting is on: the window tells this module when it changes (`hotkey_set`).

#[cfg(test)]
use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

#[cfg(test)]
#[derive(Deserialize)]
struct File {
    hotkey: Hotkey,
}

#[cfg(test)]
#[derive(Deserialize)]
struct Hotkey {
    default: String,
}

/// The chord Wings starts with, from `src/lib/commands.json`. The window picks it, so only the tests read it here.
#[cfg(test)]
pub fn default_accelerator() -> String {
    serde_json::from_str::<File>(include_str!("../../src/lib/commands.json")).expect("commands.json").hotkey.default
}

/// Brings the window to the front, then tells it to go to the agent that needs you.
fn raise(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    let _ = app.emit("global-hotkey", ());
}

/// Makes `accelerator` the global shortcut, like "Ctrl+Alt+J", or none with `None`. Anything registered before is
/// released first, so at most one is ever held.
#[tauri::command]
pub fn hotkey_set(app: AppHandle, accelerator: Option<String>) -> Result<(), String> {
    let shortcuts = app.global_shortcut();
    shortcuts.unregister_all().map_err(|e| e.to_string())?;
    let Some(accelerator) = accelerator else { return Ok(()) };
    shortcuts
        .on_shortcut(accelerator.as_str(), |app, _, event| {
            // Pressed and released both arrive; one press is one jump.
            if event.state == ShortcutState::Pressed {
                raise(app);
            }
        })
        .map_err(|e| format!("{accelerator} can't be used: {e}"))
}

#[cfg(test)]
mod tests {
    use tauri_plugin_global_shortcut::Shortcut;

    /// What the window records for a key press (`acceleratorFromEvent` in `src/lib/hotkey.ts`): a modifier and a
    /// `KeyboardEvent.code`, joined by +. The parser has to know each code the recorder can produce.
    #[test]
    fn the_recorded_key_names_parse() {
        let codes = ["KeyA", "KeyZ", "Digit0", "Digit9", "F1", "F12", "Space", "ArrowUp", "ArrowLeft", "Backquote", "Minus", "Equal", "Comma", "Period", "Slash", "Semicolon", "Quote", "Backslash", "BracketLeft", "BracketRight"];
        for code in codes {
            for mods in ["Ctrl", "Alt", "Shift", "Cmd", "Ctrl+Alt", "Cmd+Shift+Alt+Ctrl"] {
                let accelerator = format!("{mods}+{code}");
                accelerator.parse::<Shortcut>().unwrap_or_else(|e| panic!("{accelerator}: {e}"));
            }
        }
    }

    #[test]
    fn the_default_is_a_chord_with_modifiers() {
        let default = super::default_accelerator();
        let shortcut: Shortcut = default.parse().unwrap();
        assert!(!shortcut.mods.is_empty(), "{default} has no modifier");
    }

    #[test]
    fn a_bare_or_malformed_name_is_refused() {
        assert!("Ctrl+".parse::<Shortcut>().is_err());
        assert!("Ctrl+Alt+Nope".parse::<Shortcut>().is_err());
    }
}
