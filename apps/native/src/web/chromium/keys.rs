//! Which key presses in a Chromium page go to Wings. The page's window has the keyboard while you type in the page, so
//! without this Cmd-T opened a Chrome tab and Cmd-Q quit Chromium instead of Wings. A press goes to Wings when it holds
//! Cmd or Ctrl and Wings binds it app-wide (`actions::bind`, no context), unless it is one of the editing keys a page
//! needs for itself; everything else stays with the page.

/// A press, as GPUI names keys: `"t"`, `"1"`, `"["`, `"left"`, `"` + "`" + `"`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Press {
    pub key: String,
    pub cmd: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

/// The editing keys every page handles itself, which stay with it even when Wings binds them somewhere.
const PAGE_KEYS: [&str; 7] = ["cmd-c", "cmd-v", "cmd-x", "cmd-a", "cmd-z", "cmd-shift-z", "cmd-f"];

impl Press {
    /// Reads GPUI's way of writing one keystroke: modifiers then the key, joined by `-` (`"cmd-shift-d"`, `"cmd--"`).
    pub fn parse(text: &str) -> Option<Self> {
        let (modifiers, key) = match text.strip_suffix("--") {
            Some(rest) => (rest, "-"),
            None => text.rsplit_once('-').unwrap_or(("", text)),
        };
        let mut press = Press { key: key.to_owned(), cmd: false, ctrl: false, alt: false, shift: false };
        for modifier in modifiers.split('-').filter(|part| !part.is_empty()) {
            match modifier {
                "cmd" => press.cmd = true,
                "ctrl" => press.ctrl = true,
                "alt" => press.alt = true,
                "shift" => press.shift = true,
                _ => return None,
            }
        }
        (!press.key.is_empty()).then_some(press)
    }

    /// Whether this press is the keystroke `bound`. Shift counts only with a letter or a named key: a binding of
    /// `cmd-+` is the `=` key with Shift on a US keyboard, which macOS reads as `+` with Shift.
    fn is(&self, bound: &Press) -> bool {
        let shift_counts = bound.key.chars().count() > 1 || bound.key.chars().all(|c| c.is_ascii_alphabetic());
        self.key.eq_ignore_ascii_case(&bound.key)
            && self.cmd == bound.cmd
            && self.ctrl == bound.ctrl
            && self.alt == bound.alt
            && (!shift_counts || self.shift == bound.shift)
    }
}

/// Whether `press` goes to Wings rather than the page, given Wings' app-wide keystrokes.
pub fn goes_to_wings(press: &Press, app_wide: &[Press]) -> bool {
    if !press.cmd && !press.ctrl {
        return false;
    }
    if PAGE_KEYS.iter().filter_map(|key| Press::parse(key)).any(|key| press.is(&key)) {
        return false;
    }
    app_wide.iter().any(|bound| press.is(bound))
}

/// The name GPUI gives the key of a macOS key code that types no character of its own (an arrow, Escape), else none.
pub fn named_key(key_code: u16) -> Option<&'static str> {
    Some(match key_code {
        36 | 76 => "enter",
        48 => "tab",
        49 => "space",
        51 => "backspace",
        53 => "escape",
        115 => "home",
        116 => "pageup",
        117 => "delete",
        119 => "end",
        121 => "pagedown",
        123 => "left",
        124 => "right",
        125 => "down",
        126 => "up",
        _ => return None,
    })
}

thread_local! {
    /// Wings' app-wide keystrokes, as `actions::bind` bound them.
    static APP_WIDE: std::cell::RefCell<Vec<Press>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Notes Wings' app-wide keystrokes, which `goes_to_wings_now` reads.
pub fn remember(app_wide: Vec<Press>) {
    APP_WIDE.with_borrow_mut(|kept| *kept = app_wide);
}

/// `goes_to_wings` with the keystrokes `remember` noted.
pub fn goes_to_wings_now(press: &Press) -> bool {
    APP_WIDE.with_borrow(|app_wide| goes_to_wings(press, app_wide))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(text: &str) -> Press {
        Press::parse(text).unwrap()
    }

    fn wings() -> Vec<Press> {
        ["cmd-t", "cmd-w", "cmd-1", "cmd-9", "cmd-k", "ctrl-`", "cmd-q", "cmd-,", "cmd-+", "cmd--", "cmd-shift-d", "cmd-alt-left", "cmd-]"]
            .iter()
            .map(|text| press(text))
            .collect()
    }

    /// Cmd-T, Cmd-W, Cmd-number, Cmd-K and Ctrl-` pressed in a page are Wings' (a new Wings tab, not a Chrome one).
    #[test]
    fn wings_shortcuts_pressed_in_a_page_go_to_wings() {
        for text in ["cmd-t", "cmd-w", "cmd-1", "cmd-9", "cmd-k", "ctrl-`", "cmd-q", "cmd-,", "cmd-shift-d", "cmd-alt-left", "cmd-]", "cmd--"] {
            assert!(goes_to_wings(&press(text), &wings()), "{text}");
        }
        // The = key with Shift reads as "+" with Shift.
        assert!(goes_to_wings(&press("cmd-shift-+"), &wings()));
        assert!(goes_to_wings(&press("cmd-T"), &wings()), "Caps Lock on");
    }

    /// Copy, paste, select all, undo and Find stay with the page, and so does anything Wings doesn't bind app-wide.
    #[test]
    fn editing_keys_and_unbound_keys_stay_with_the_page() {
        let mut wings = wings();
        // Wings binds Copy and Paste in terminals, and Find too: in a page they are the page's.
        wings.extend(["cmd-c", "cmd-v", "cmd-f"].map(press));
        for text in ["cmd-c", "cmd-v", "cmd-x", "cmd-a", "cmd-z", "cmd-shift-z", "cmd-f", "cmd-l", "cmd-r", "t", "shift-t", "alt-t", "enter", "cmd-shift-t"] {
            assert!(!goes_to_wings(&press(text), &wings), "{text}");
        }
    }

    #[test]
    fn gpui_keystrokes_are_read_with_their_modifiers() {
        assert_eq!(press("cmd--"), Press { key: "-".into(), cmd: true, ctrl: false, alt: false, shift: false });
        assert_eq!(press("ctrl-shift-alt-["), Press { key: "[".into(), cmd: false, ctrl: true, alt: true, shift: true });
        assert_eq!(press("down").key, "down");
        assert_eq!(Press::parse("hyper-x"), None);
    }
}
