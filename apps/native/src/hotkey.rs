//! The global shortcut that brings Wings forward from any app and goes to the agent that needs you, as the desktop's
//! `hotkey.rs` and `src/lib/hotkey.ts`. The chord is held only while the setting is on, and at most one at a time.
//! The system registration sits behind [`Registrar`], so tests and screenshots never take a key from your Mac.

use std::rc::Rc;

use gpui::{App, Global, Keystroke};

/// Makes the shortcut `accelerator`, like `Ctrl+Alt+KeyJ`, or none with `None`, releasing the one held before.
pub trait Registrar {
    fn set(&self, accelerator: Option<&str>) -> Result<(), String>;
}

/// The app's global shortcut: what holds it, and each press of it, which the window waits on.
pub struct Hotkeys {
    pub registrar: Rc<dyn Registrar>,
    pub presses: async_channel::Receiver<()>,
}

impl Global for Hotkeys {}

impl Hotkeys {
    /// The system's, on macOS: Carbon's `RegisterEventHotKey` through the `global-hotkey` crate the desktop uses too.
    /// Must be made on the main thread, inside the app's run loop.
    #[cfg(target_os = "macos")]
    pub fn system() -> Result<Self, String> {
        let (send, presses) = async_channel::unbounded();
        let manager = global_hotkey::GlobalHotKeyManager::new().map_err(|e| e.to_string())?;
        global_hotkey::GlobalHotKeyEvent::set_event_handler(Some(
            move |event: global_hotkey::GlobalHotKeyEvent| {
                // Pressed and released both arrive; one press is one jump.
                if event.state == global_hotkey::HotKeyState::Pressed {
                    let _ = send.try_send(());
                }
            },
        ));
        Ok(Self {
            registrar: Rc::new(System {
                manager,
                held: std::cell::Cell::new(None),
            }),
            presses,
        })
    }
}

#[cfg(target_os = "macos")]
struct System {
    manager: global_hotkey::GlobalHotKeyManager,
    held: std::cell::Cell<Option<global_hotkey::hotkey::HotKey>>,
}

#[cfg(target_os = "macos")]
impl Registrar for System {
    fn set(&self, accelerator: Option<&str>) -> Result<(), String> {
        if let Some(held) = self.held.take() {
            self.manager.unregister(held).map_err(|e| e.to_string())?;
        }
        let Some(accelerator) = accelerator else {
            return Ok(());
        };
        let hotkey: global_hotkey::hotkey::HotKey = accelerator
            .parse()
            .map_err(|e| format!("{accelerator} can't be used: {e}"))?;
        self.manager
            .register(hotkey)
            .map_err(|e| format!("{accelerator} can't be used: {e}"))?;
        self.held.set(Some(hotkey));
        Ok(())
    }
}

/// Shows the app and its window from anywhere: unhidden, out of the Dock, and in front, as the desktop's `raise`.
pub fn raise(window: &mut gpui::Window, cx: &mut App) {
    #[cfg(target_os = "macos")]
    if let Some(main) = objc2::MainThreadMarker::new() {
        let app = objc2_app_kit::NSApplication::sharedApplication(main);
        app.unhide(None);
        for window in app.windows().iter() {
            if window.isMiniaturized() {
                window.deminiaturize(None);
            }
        }
    }
    cx.activate(true);
    window.activate_window();
}

/// A registrar that only remembers what it was told, and a way to press the shortcut, for tests and screenshots.
#[cfg(any(test, feature = "test-support"))]
pub mod fake {
    use std::{cell::RefCell, rc::Rc};

    /// Every `set` it was given, in order. An accelerator in `refuse` fails, as one another app holds would.
    #[derive(Default)]
    pub struct Fake {
        pub calls: RefCell<Vec<Option<String>>>,
        pub refuse: RefCell<Vec<String>>,
    }

    impl super::Registrar for Fake {
        fn set(&self, accelerator: Option<&str>) -> Result<(), String> {
            self.calls.borrow_mut().push(accelerator.map(str::to_owned));
            match accelerator {
                Some(taken) if self.refuse.borrow().iter().any(|r| r == taken) => {
                    Err(format!("{taken} can't be used: another app holds it"))
                }
                _ => Ok(()),
            }
        }
    }

    /// The fake, and the sender that presses the shortcut.
    pub fn hotkeys() -> (super::Hotkeys, Rc<Fake>, async_channel::Sender<()>) {
        let fake = Rc::new(Fake::default());
        let (press, presses) = async_channel::unbounded();
        let hotkeys = super::Hotkeys {
            registrar: fake.clone(),
            presses,
        };
        (hotkeys, fake, press)
    }
}

/// The accelerator for a key press, like `Ctrl+Alt+KeyJ`, or `None` when it can't be a global shortcut: a key the
/// desktop's recorder doesn't take, or no Ctrl, Alt or Cmd, which would take a plain letter or Shift-letter over in
/// every app. The desktop's `acceleratorFromEvent`, from the key GPUI reports instead of `KeyboardEvent.code`.
pub fn accelerator_from(keystroke: &Keystroke) -> Option<String> {
    let mods = &keystroke.modifiers;
    if !(mods.control || mods.alt || mods.platform) {
        return None;
    }
    let (code, shifted) = code(&keystroke.key)?;
    let shift = mods.shift || shifted;
    let parts = [
        (mods.control, "Ctrl"),
        (mods.alt, "Alt"),
        (shift, "Shift"),
        (mods.platform, "Cmd"),
    ];
    let mut out: Vec<String> = parts
        .iter()
        .filter(|(held, _)| *held)
        .map(|(_, name)| (*name).to_owned())
        .collect();
    out.push(code);
    Some(out.join("+"))
}

/// The `KeyboardEvent.code` of a key GPUI names, and whether the name is the key's shifted character (GPUI reports
/// Shift-1 as `!` with Shift taken off). A US layout's keys, as the desktop's recorder knows them.
fn code(key: &str) -> Option<(String, bool)> {
    const SYMBOLS: [(&str, &str, &str); 11] = [
        ("`", "~", "Backquote"),
        ("-", "_", "Minus"),
        ("=", "+", "Equal"),
        (",", "<", "Comma"),
        (".", ">", "Period"),
        ("/", "?", "Slash"),
        (";", ":", "Semicolon"),
        ("'", "\"", "Quote"),
        ("\\", "|", "Backslash"),
        ("[", "{", "BracketLeft"),
        ("]", "}", "BracketRight"),
    ];
    const SHIFTED_DIGITS: &str = ")!@#$%^&*(";
    let mut chars = key.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_alphabetic() {
            return Some((
                format!("Key{}", c.to_ascii_uppercase()),
                c.is_ascii_uppercase(),
            ));
        }
        if c.is_ascii_digit() {
            return Some((format!("Digit{c}"), false));
        }
        if let Some(digit) = SHIFTED_DIGITS.find(c) {
            return Some((format!("Digit{digit}"), true));
        }
        return SYMBOLS.iter().find_map(|(plain, shifted, code)| {
            (key == *plain || key == *shifted).then(|| ((*code).to_owned(), key == *shifted))
        });
    }
    let named = match key {
        "space" => "Space",
        "up" => "ArrowUp",
        "down" => "ArrowDown",
        "left" => "ArrowLeft",
        "right" => "ArrowRight",
        _ => {
            let n: u8 = key.strip_prefix('f')?.parse().ok()?;
            return (1..=12).contains(&n).then(|| (format!("F{n}"), false));
        }
    };
    Some((named.to_owned(), false))
}

/// `Ctrl+Alt+KeyJ` as the platform writes it: `⌃⌥J` on macOS, `Ctrl+Alt+J` elsewhere, as the desktop's `hotkeyLabel`.
pub fn label(accelerator: &str, mac: bool) -> String {
    let mut parts: Vec<&str> = accelerator.split('+').collect();
    let code = parts.pop().unwrap_or_default();
    let key = match code {
        "ArrowUp" => "↑",
        "ArrowDown" => "↓",
        "ArrowLeft" => "←",
        "ArrowRight" => "→",
        "Backquote" => "`",
        "Minus" => "-",
        "Equal" => "=",
        "Comma" => ",",
        "Period" => ".",
        "Slash" => "/",
        "Semicolon" => ";",
        "Quote" => "'",
        "Backslash" => "\\",
        "BracketLeft" => "[",
        "BracketRight" => "]",
        other => other
            .strip_prefix("Key")
            .or_else(|| other.strip_prefix("Digit"))
            .unwrap_or(other),
    };
    if !mac {
        let mut out: Vec<&str> = parts
            .into_iter()
            .map(|m| if m == "Cmd" { "Win" } else { m })
            .collect();
        out.push(key);
        return out.join("+");
    }
    let mut out: String = parts
        .into_iter()
        .map(|m| match m {
            "Ctrl" => "⌃",
            "Alt" => "⌥",
            "Shift" => "⇧",
            "Cmd" => "⌘",
            other => other,
        })
        .collect();
    out.push_str(key);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(text: &'static str) -> Option<String> {
        accelerator_from(&Keystroke::parse(text).unwrap())
    }

    /// The same cases as the desktop's `hotkey.test.ts`.
    #[test]
    fn a_press_becomes_the_desktops_accelerator() {
        assert_eq!(press("ctrl-alt-j").as_deref(), Some("Ctrl+Alt+KeyJ"));
        assert_eq!(press("cmd-shift-k").as_deref(), Some("Shift+Cmd+KeyK"));
        assert_eq!(press("ctrl-`").as_deref(), Some("Ctrl+Backquote"));
        assert_eq!(press("alt-f5").as_deref(), Some("Alt+F5"));
        assert_eq!(press("ctrl-alt-space").as_deref(), Some("Ctrl+Alt+Space"));
        assert_eq!(press("cmd-up").as_deref(), Some("Cmd+ArrowUp"));
        assert_eq!(press("ctrl-alt-7").as_deref(), Some("Ctrl+Alt+Digit7"));
        // GPUI reports Shift-1 as `!` with Shift taken off.
        assert_eq!(press("ctrl-!").as_deref(), Some("Ctrl+Shift+Digit1"));
        assert_eq!(press("cmd-{").as_deref(), Some("Shift+Cmd+BracketLeft"));
    }

    #[test]
    fn a_plain_or_shifted_key_or_an_unknown_one_is_not_a_shortcut() {
        assert_eq!(press("j"), None);
        assert_eq!(press("shift-j"), None);
        assert_eq!(press("ctrl-enter"), None);
        assert_eq!(press("ctrl-f13"), None);
        assert_eq!(press("ctrl-escape"), None);
    }

    #[test]
    fn labels_read_as_the_desktops() {
        assert_eq!(label("Ctrl+Alt+KeyJ", true), "⌃⌥J");
        assert_eq!(label("Shift+Cmd+Digit1", true), "⇧⌘1");
        assert_eq!(label("Ctrl+Backquote", true), "⌃`");
        assert_eq!(label("Ctrl+Alt+ArrowUp", false), "Ctrl+Alt+↑");
        assert_eq!(label("Cmd+KeyK", false), "Win+K");
    }

    /// Whatever the recorder makes, the system registration can read: the desktop's `the_recorded_key_names_parse`.
    #[cfg(target_os = "macos")]
    #[test]
    fn every_recorded_accelerator_parses_for_the_system() {
        let keys = [
            "a", "z", "0", "9", "f1", "f12", "space", "up", "left", "`", "-", "=", ",", ".", "/",
            ";", "'", "\\", "[", "]",
        ];
        for key in keys {
            for mods in ["ctrl", "alt", "cmd", "ctrl-alt", "cmd-shift-alt-ctrl"] {
                let text: &'static str = format!("{mods}-{key}").leak();
                let accelerator = press(text).unwrap();
                accelerator
                    .parse::<global_hotkey::hotkey::HotKey>()
                    .unwrap_or_else(|e| panic!("{accelerator}: {e}"));
            }
        }
        let default: global_hotkey::hotkey::HotKey = crate::saved::DEFAULT_HOTKEY.parse().unwrap();
        assert!(!default.mods.is_empty());
    }

    #[test]
    fn the_default_is_the_desktops() {
        let commands: serde_json::Value =
            serde_json::from_str(include_str!("../../desktop/src/lib/commands.json")).unwrap();
        assert_eq!(commands["hotkey"]["default"], crate::saved::DEFAULT_HOTKEY);
    }
}
