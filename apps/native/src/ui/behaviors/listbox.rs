//! `listbox`: one option highlighted, moved with the arrow keys (UI SDK's `createListbox`). Up and Down move it and wrap
//! at the ends; Home and End jump to the first and last option when the list has the keyboard itself (`Focus::Real`,
//! a menu), not when a field keeps it and Home and End move its caret (`Focus::Virtual`, a combobox). Disabled options
//! are skipped. Enter is the caller's: it picks the highlighted option. `clamp_after` is the same for a long list of
//! rows that stops at its ends and pages (a graph, a file list). `Typeahead` jumps to the option whose label starts with
//! what is typed.

use std::time::{Duration, Instant};

/// Where the keyboard is while the list is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    /// On the list.
    Real,
    /// On a field, which keeps it while the list follows the arrows.
    Virtual,
}

/// The option highlighted after `key` (a gpui key name) from `highlighted` among `count` options, or `None` when the
/// key isn't the list's. `enabled` says which options can be highlighted.
pub fn highlight_after(
    key: &str,
    focus: Focus,
    highlighted: Option<usize>,
    count: usize,
    enabled: impl Fn(usize) -> bool,
) -> Option<Option<usize>> {
    let forward = |start: usize| (0..count).map(|step| (start + step) % count).find(|at| enabled(*at));
    let moved = match key {
        _ if count == 0 => return None,
        "down" => forward(highlighted.map_or(0, |at| (at + 1) % count)),
        "up" => {
            let start = highlighted.map_or(count - 1, |at| (at + count - 1) % count);
            (0..count).map(|step| (start + count - step) % count).find(|at| enabled(*at))
        }
        "home" if focus == Focus::Real => forward(0),
        "end" if focus == Focus::Real => (0..count).rev().find(|at| enabled(*at)),
        _ => return None,
    };
    Some(moved)
}

/// How long a pause ends what was typed, so the next letter starts a new word.
const TYPEAHEAD_PAUSE: Duration = Duration::from_millis(1000);

/// What was typed in a list or menu, kept while the letters come close together.
#[derive(Default)]
pub struct Typeahead {
    typed: String,
    last: Option<Instant>,
}

impl Typeahead {
    /// Whether letters typed at `now` are still a word, so a space belongs to it.
    pub fn is_typing(&self, now: Instant) -> bool {
        !self.typed.is_empty() && self.last.is_some_and(|last| now.duration_since(last) <= TYPEAHEAD_PAUSE)
    }


    /// The option to highlight after `text` was typed at `now`, among `count` options starting from `current`, or
    /// `None` when none starts with what is typed. `label` gives an option's text, `None` for one that can't be
    /// picked. The same letter pressed again goes through the options with that letter, as a native menu does; a
    /// space only counts inside a word.
    pub fn press(
        &mut self,
        text: &str,
        now: Instant,
        current: Option<usize>,
        count: usize,
        label: impl Fn(usize) -> Option<String>,
    ) -> Option<usize> {
        if self.last.is_some_and(|last| now.duration_since(last) > TYPEAHEAD_PAUSE) {
            self.typed.clear();
        }
        if text.chars().any(char::is_control) || (text == " " && self.typed.is_empty()) {
            return None;
        }
        self.last = Some(now);
        self.typed.push_str(&text.to_lowercase());
        let first = self.typed.chars().next()?;
        // One letter, or the same one again, goes one option on at a time from the current one; a word starts at it.
        let (word, from) = if self.typed.chars().all(|c| c == first) {
            (&self.typed[..first.len_utf8()], current.map_or(0, |at| at + 1))
        } else {
            (self.typed.as_str(), current.unwrap_or(0))
        };
        (0..count).map(|step| (from + step) % count.max(1)).find(|at| {
            label(*at).is_some_and(|label| label.to_lowercase().starts_with(word))
        })
    }
}

/// The text a key press types, none when it is a shortcut (Ctrl, Cmd or Alt held) or a key with no text.
pub fn typed_text(event: &gpui::KeyDownEvent) -> Option<&str> {
    let mods = event.keystroke.modifiers;
    (!mods.control && !mods.platform && !mods.alt)
        .then(|| event.keystroke.key_char.as_deref())
        .flatten()
}

/// The row selected after `key` from `current` among `count` rows: Up and Down stop at the ends, Page Up and Page Down
/// move `page` rows, Home and End jump to the first and last. With nothing selected, a key toward the end (Down, Page
/// Down, Home) picks the first row and one toward the start (Up, Page Up, End) the last. `None` when the key isn't the
/// list's.
pub fn clamp_after(key: &str, current: Option<usize>, count: usize, page: usize) -> Option<usize> {
    let last = count.checked_sub(1)?;
    Some(match (key, current) {
        ("down", Some(at)) => (at + 1).min(last),
        ("up", Some(at)) => at.saturating_sub(1),
        ("pagedown", Some(at)) => at.saturating_add(page).min(last),
        ("pageup", Some(at)) => at.saturating_sub(page),
        ("down" | "pagedown" | "home", _) => 0,
        ("up" | "pageup" | "end", _) => last,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_letters_jump_to_the_option_they_start() {
        let names = ["Copy", "Cut", "Paste", "Delete"];
        let label = |at: usize| Some(names[at].to_owned());
        let start = Instant::now();
        let after = |ms| start + Duration::from_millis(ms);
        let mut typed = Typeahead::default();
        assert_eq!(typed.press("p", start, None, 4, label), Some(2));
        // A pause starts a new word.
        assert_eq!(typed.press("d", after(2_000), Some(2), 4, label), Some(3));
        // Letters close together make one word: "cu" skips "Copy".
        assert_eq!(typed.press("c", after(5_000), Some(3), 4, label), Some(0));
        assert_eq!(typed.press("u", after(5_200), Some(0), 4, label), Some(1));
        // The same letter again goes on to the next option with it, and wraps.
        assert_eq!(typed.press("c", after(8_000), Some(1), 4, label), Some(0));
        assert_eq!(typed.press("c", after(8_200), Some(0), 4, label), Some(1));
        assert_eq!(typed.press("c", after(8_400), Some(1), 4, label), Some(0));
        // Nothing starts with it, a space opens no word, and a control character is no letter.
        assert_eq!(typed.press("x", after(12_000), Some(0), 4, label), None);
        assert_eq!(typed.press(" ", after(15_000), Some(0), 4, label), None);
        assert_eq!(typed.press("\u{8}", after(15_100), Some(0), 4, label), None);
    }

    #[test]
    fn typeahead_skips_options_that_cannot_be_picked() {
        let label = |at: usize| (at != 0).then(|| ["Apple", "Avocado"][at].to_owned());
        let mut typed = Typeahead::default();
        assert_eq!(typed.press("a", Instant::now(), None, 2, label), Some(1));
    }

    #[test]
    fn clamped_rows_stop_at_the_ends_and_page() {
        assert_eq!(clamp_after("end", Some(0), 10_000, 20), Some(9_999));
        assert_eq!(clamp_after("down", Some(9_999), 10_000, 20), Some(9_999));
        assert_eq!(clamp_after("up", Some(0), 10_000, 20), Some(0));
        assert_eq!(clamp_after("pageup", Some(12), 10_000, 20), Some(0));
        assert_eq!(clamp_after("pagedown", Some(12), 10_000, 20), Some(32));
        assert_eq!(clamp_after("pagedown", Some(9_990), 10_000, 20), Some(9_999));
        assert_eq!(clamp_after("home", Some(500), 10_000, 20), Some(0));
        assert_eq!(clamp_after("down", None, 3, 20), Some(0));
        assert_eq!(clamp_after("up", None, 3, 20), Some(2));
        assert_eq!(clamp_after("down", Some(0), 0, 20), None);
        assert_eq!(clamp_after("enter", Some(0), 3, 20), None);
    }

    #[test]
    fn arrows_move_and_wrap() {
        let all = |_| true;
        assert_eq!(highlight_after("down", Focus::Real, None, 3, all), Some(Some(0)));
        assert_eq!(highlight_after("down", Focus::Real, Some(2), 3, all), Some(Some(0)));
        assert_eq!(highlight_after("up", Focus::Real, None, 3, all), Some(Some(2)));
        assert_eq!(highlight_after("up", Focus::Real, Some(0), 3, all), Some(Some(2)));
    }

    #[test]
    fn skips_disabled_options() {
        let enabled = |at| at != 1;
        assert_eq!(highlight_after("down", Focus::Real, Some(0), 3, enabled), Some(Some(2)));
        assert_eq!(highlight_after("up", Focus::Real, Some(2), 3, enabled), Some(Some(0)));
        assert_eq!(highlight_after("home", Focus::Real, Some(2), 3, |at| at != 0), Some(Some(1)));
        assert_eq!(highlight_after("end", Focus::Real, None, 3, |at| at != 2), Some(Some(1)));
        assert_eq!(highlight_after("down", Focus::Real, None, 2, |_| false), Some(None));
    }

    #[test]
    fn home_and_end_stay_with_a_field() {
        assert_eq!(highlight_after("home", Focus::Virtual, Some(1), 3, |_| true), None);
        assert_eq!(highlight_after("end", Focus::Virtual, Some(1), 3, |_| true), None);
        assert_eq!(highlight_after("enter", Focus::Real, Some(1), 3, |_| true), None);
        assert_eq!(highlight_after("down", Focus::Real, None, 0, |_| true), None);
    }
}
