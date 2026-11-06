//! Keyboard and text input: key encoding, paste, and the native input-method handler.

use std::ops::Range;

use gpui::{
    Bounds, Context, EntityInputHandler, Focusable, KeyDownEvent, Keystroke, Pixels, Point,
    UTF16Selection, Window, point, px, size,
};
use wings_terminal::alacritty_terminal::{grid::Scroll, term::TermMode};

use super::{TerminalView, worker::Command};

fn key_bytes(key: &Keystroke, mode: TermMode, prefer_character: bool) -> Option<Vec<u8>> {
    let mods = key.modifiers;
    if mods.platform || prefer_character {
        return None;
    }
    let modifier = 1 + mods.shift as u8 + 2 * mods.alt as u8 + 4 * mods.control as u8;
    let cursor = match key.key.as_str() {
        "up" => Some('A'),
        "down" => Some('B'),
        "right" => Some('C'),
        "left" => Some('D'),
        "home" => Some('H'),
        "end" => Some('F'),
        _ => None,
    };
    if let Some(final_byte) = cursor {
        return Some(
            if modifier != 1 {
                format!("\x1b[1;{modifier}{final_byte}")
            } else if mode.contains(TermMode::APP_CURSOR) {
                format!("\x1bO{final_byte}")
            } else {
                format!("\x1b[{final_byte}")
            }
            .into_bytes(),
        );
    }
    let mut bytes = match key.key.as_str() {
        "enter" => {
            if mode.contains(TermMode::LINE_FEED_NEW_LINE) {
                b"\r\n".to_vec()
            } else {
                b"\r".to_vec()
            }
        }
        "backspace" => vec![0x7f],
        "escape" => vec![0x1b],
        "tab" if mods.shift => b"\x1b[Z".to_vec(),
        "tab" => vec![b'\t'],
        "insert" | "delete" | "pageup" | "pagedown" => {
            let code = match key.key.as_str() {
                "insert" => 2,
                "delete" => 3,
                "pageup" => 5,
                _ => 6,
            };
            return Some(
                if modifier == 1 {
                    format!("\x1b[{code}~")
                } else {
                    format!("\x1b[{code};{modifier}~")
                }
                .into_bytes(),
            );
        }
        _ if mods.control => {
            let lowered = key.key.to_ascii_lowercase();
            let code = match lowered.as_str() {
                "space" | "@" | "2" => 0,
                "[" | "3" => 0x1b,
                "\\" | "4" => 0x1c,
                "]" | "5" => 0x1d,
                "^" | "6" => 0x1e,
                "_" | "-" | "7" => 0x1f,
                "?" | "8" => 0x7f,
                _ if lowered.len() == 1 && lowered.as_bytes()[0].is_ascii_lowercase() => {
                    lowered.as_bytes()[0] & 0x1f
                }
                _ => return None,
            };
            vec![code]
        }
        // Printable text, including Option-generated characters, comes only from native text input.
        _ => return None,
    };
    if mods.alt {
        bytes.insert(0, 0x1b);
    }
    Some(bytes)
}

fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed {
        // An escape in pasted text could end the bracket early and run the rest as typed keys.
        format!("\x1b[200~{}\x1b[201~", text.replace('\x1b', "")).into_bytes()
    } else {
        text.into_bytes()
    }
}

/// Keys the desktop terminal sends differently from xterm (`src/lib/edit-keys.ts` and its
/// Shift-Return setting, on by default). Shift-Return is Meta-Return, a new line in Claude Code.
/// On macOS, Option-arrows move a word and Cmd-arrows and Cmd-Backspace edit the line.
fn edit_key(key: &Keystroke, shift_return_meta: bool) -> Option<&'static [u8]> {
    let mods = key.modifiers;
    if shift_return_meta
        && key.key == "enter"
        && mods.shift
        && !mods.alt
        && !mods.platform
        && !mods.control
    {
        return Some(b"\x1b\r");
    }
    if !cfg!(target_os = "macos") || mods.control || mods.shift || mods.platform == mods.alt {
        return None;
    }
    match (mods.alt, key.key.as_str()) {
        (true, "left") => Some(b"\x1bb"),
        (true, "right") => Some(b"\x1bf"),
        (false, "left") => Some(b"\x01"),
        (false, "right") => Some(b"\x05"),
        (false, "backspace") => Some(b"\x15"),
        _ => None,
    }
}

impl TerminalView {
    pub(super) fn key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (mode, rows) = {
            let latest = self.latest.lock().unwrap();
            (latest.mode, latest.rows)
        };
        if self
            .find
            .as_ref()
            .is_some_and(|find| find.field.focus_handle(cx).is_focused(window))
        {
            return;
        }
        let key = &event.keystroke;
        let command = match () {
            // Shift-Page Up and Down scroll the history by a screen less one line, as in the desktop terminal.
            () if key.modifiers.shift && matches!(key.key.as_str(), "pageup" | "pagedown") => {
                let lines = rows.saturating_sub(1).max(1) as i32;
                Some(Command::Scroll(Scroll::Delta(if key.key == "pageup" {
                    lines
                } else {
                    -lines
                })))
            }
            () => edit_key(key, super::look(cx).shift_return_meta)
                .map(<[u8]>::to_vec)
                .or_else(|| key_bytes(key, mode, event.prefer_character_input))
                .map(Command::Input),
        };
        if let Some(command) = command {
            self.send(command, cx);
            window.prevent_default();
            cx.stop_propagation();
        }
    }

    pub(super) fn paste(&mut self, cx: &mut Context<Self>) {
        let bracketed = self
            .latest
            .lock()
            .unwrap()
            .mode
            .contains(TermMode::BRACKETED_PASTE);
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.send(Command::Input(paste_bytes(&text, bracketed)), cx);
        }
    }
}

impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let utf16: Vec<_> = self.marked.encode_utf16().collect();
        let text = String::from_utf16(utf16.get(range.clone())?).ok()?;
        *adjusted = Some(range);
        Some(text)
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let end = self.marked.encode_utf16().count();
        Some(UTF16Selection {
            range: end..end,
            reversed: false,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        (!self.marked.is_empty()).then(|| 0..self.marked.encode_utf16().count())
    }
    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.marked.clear();
        window.notify_text_input_changed(&self.focus);
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if range.is_some_and(|range| !range.is_empty()) && self.marked.is_empty() {
            self.error =
                Some("Replacing previously committed terminal text is not supported.".into());
            cx.notify();
            return;
        }
        self.marked.clear();
        if !text.is_empty() {
            self.send(Command::Input(text.as_bytes().to_vec()), cx);
        }
        window.notify_text_input_changed(&self.focus);
        cx.notify();
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked = text.to_string();
        window.notify_text_input_changed(&self.focus);
        cx.notify();
    }
    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let bounds = self.bounds?;
        let (row, col, width, _) = self.snapshot.cursor?;
        Some(Bounds::new(
            point(
                bounds.left() + self.cell_width * col,
                bounds.top() + px(self.line_height) * row,
            ),
            size(self.cell_width * width, px(self.line_height)),
        ))
    }
    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
    fn accepts_text_input(&self, _: &mut Window, _: &mut Context<Self>) -> bool {
        !self.snapshot.closed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(binding: &'static str, mode: TermMode) -> Option<Vec<u8>> {
        key_bytes(&Keystroke::parse(binding).unwrap(), mode, false)
    }

    #[test]
    fn printable_text_is_left_to_native_input_and_controls_are_encoded_once() {
        let mode = TermMode::default();
        assert_eq!(key("a", mode), None);
        assert_eq!(key("shift-a", mode), None);
        assert_eq!(key("alt-e", mode), None);
        assert_eq!(key("cmd-c", mode), None);
        assert_eq!(key("ctrl-c", mode), Some(vec![3]));
        assert_eq!(key("ctrl-d", mode), Some(vec![4]));
        assert_eq!(key("ctrl-z", mode), Some(vec![26]));
        assert_eq!(key("ctrl-space", mode), Some(vec![0]));
        assert_eq!(key("ctrl-\\", mode), Some(vec![28]));
        assert_eq!(key("enter", mode), Some(b"\r".to_vec()));
        assert_eq!(key("backspace", mode), Some(vec![127]));
        assert_eq!(key("shift-tab", mode), Some(b"\x1b[Z".to_vec()));
        assert_eq!(
            key_bytes(&Keystroke::parse("ctrl-a").unwrap(), mode, true),
            None
        );
    }

    #[test]
    fn cursor_keys_follow_terminal_modes_and_modifiers() {
        let mode = TermMode::default();
        assert_eq!(key("up", mode), Some(b"\x1b[A".to_vec()));
        assert_eq!(
            key("left", mode | TermMode::APP_CURSOR),
            Some(b"\x1bOD".to_vec())
        );
        assert_eq!(key("ctrl-right", mode), Some(b"\x1b[1;5C".to_vec()));
        assert_eq!(key("shift-home", mode), Some(b"\x1b[1;2H".to_vec()));
        assert_eq!(key("delete", mode), Some(b"\x1b[3~".to_vec()));
        assert_eq!(
            key("enter", mode | TermMode::LINE_FEED_NEW_LINE),
            Some(b"\r\n".to_vec())
        );
    }

    #[test]
    fn paste_uses_carriage_returns_and_cannot_close_the_bracket_early() {
        assert_eq!(paste_bytes("a\nb\r\nc", false), b"a\rb\rc");
        assert_eq!(
            paste_bytes("x\x1b[201~echo injected\n", true),
            b"\x1b[200~x[201~echo injected\r\x1b[201~"
        );
    }

    #[test]
    fn shift_return_and_mac_editing_keys_match_the_desktop_terminal() {
        let edit = |binding| edit_key(&Keystroke::parse(binding).unwrap(), true);
        assert_eq!(edit("shift-enter"), Some(b"\x1b\r".as_slice()));
        assert_eq!(edit("enter"), None);
        assert_eq!(edit("ctrl-left"), None);
        // With the setting off, Shift-Return is left to the usual encoding.
        assert_eq!(
            edit_key(&Keystroke::parse("shift-enter").unwrap(), false),
            None
        );
        if cfg!(target_os = "macos") {
            assert_eq!(edit("alt-left"), Some(b"\x1bb".as_slice()));
            assert_eq!(edit("alt-right"), Some(b"\x1bf".as_slice()));
            assert_eq!(edit("cmd-left"), Some(b"\x01".as_slice()));
            assert_eq!(edit("cmd-right"), Some(b"\x05".as_slice()));
            assert_eq!(edit("cmd-backspace"), Some(b"\x15".as_slice()));
            assert_eq!(edit("cmd-alt-left"), None);
            assert_eq!(edit("shift-alt-left"), None);
        } else {
            assert_eq!(edit("alt-left"), None);
        }
    }
}
