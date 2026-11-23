//! `MenuKeys`: the keyboard of an open menu, written once (UI SDK's menu keys over `createListbox`). Up and Down move
//! the highlighted item and wrap, Home and End jump, typing a letter goes to the item that starts with it, Enter picks
//! the highlighted one and Space does too, while nothing is being typed. Escape is the caller's, through `bind_escape`
//! or `bind_layer`. Items that are off are skipped. The caller draws the highlight (`ui::menu_highlight`) and runs
//! the pick.

use std::time::Instant;

use gpui::KeyDownEvent;

use super::listbox::{self, Focus, Typeahead};

/// What a key did to the menu.
#[derive(Debug, PartialEq, Eq)]
pub enum MenuKey {
    /// The highlight moved, or a letter went to an item: draw the menu again.
    Moved,
    /// Enter, or Space, on the highlighted item, by its place in the menu.
    Pick(usize),
    /// Not a menu key: leave it to whoever else hears it.
    Ignored,
}

/// The highlighted item of an open menu and what was typed.
#[derive(Default)]
pub struct MenuKeys {
    pub highlight: Option<usize>,
    typeahead: Typeahead,
}

impl MenuKeys {
    /// What `event` does to a menu of items given as their labels, `None` for one that can't be picked.
    pub fn press(&mut self, event: &KeyDownEvent, labels: &[Option<String>]) -> MenuKey {
        if event.keystroke.modifiers.modified() {
            return MenuKey::Ignored;
        }
        let key = event.keystroke.key.as_str();
        let count = labels.len();
        if let Some(moved) = listbox::highlight_after(key, Focus::Real, self.highlight, count, |at| labels[at].is_some()) {
            self.highlight = moved;
            return MenuKey::Moved;
        }
        let typing = key == "space" && self.typeahead.is_typing(Instant::now());
        if matches!(key, "enter" | "space") && !typing {
            return match self.highlight.filter(|at| labels.get(*at).is_some_and(Option::is_some)) {
                Some(at) => MenuKey::Pick(at),
                None => MenuKey::Ignored,
            };
        }
        match listbox::typed_text(event)
            .and_then(|text| self.typeahead.press(text, Instant::now(), self.highlight, count, |at| labels[at].clone()))
        {
            Some(at) => {
                self.highlight = Some(at);
                MenuKey::Moved
            }
            None => MenuKey::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui::Keystroke;

    use super::*;

    fn press(keys: &mut MenuKeys, key: &'static str, labels: &[Option<String>]) -> MenuKey {
        let keystroke = Keystroke::parse(key).unwrap();
        keys.press(&KeyDownEvent { keystroke, is_held: false, prefer_character_input: false }, labels)
    }

    #[test]
    fn arrows_letters_and_enter_drive_a_menu() {
        let labels = ["Move left", "Move right", "Close tab"].map(|label| Some(label.to_owned()));
        let mut keys = MenuKeys::default();
        assert_eq!(press(&mut keys, "enter", &labels), MenuKey::Ignored);
        assert_eq!(press(&mut keys, "down", &labels), MenuKey::Moved);
        assert_eq!(keys.highlight, Some(0));
        assert_eq!(press(&mut keys, "up", &labels), MenuKey::Moved);
        assert_eq!(keys.highlight, Some(2));
        assert_eq!(press(&mut keys, "enter", &labels), MenuKey::Pick(2));
        assert_eq!(press(&mut keys, "home", &labels), MenuKey::Moved);
        assert_eq!(keys.highlight, Some(0));
        assert_eq!(press(&mut keys, "c->c", &labels), MenuKey::Moved);
        assert_eq!(keys.highlight, Some(2));
        assert_eq!(press(&mut keys, "cmd-down", &labels), MenuKey::Ignored);
    }

    #[test]
    fn off_items_are_skipped_and_never_picked() {
        let labels = [None, Some("Close".to_owned())];
        let mut keys = MenuKeys::default();
        press(&mut keys, "down", &labels);
        assert_eq!(keys.highlight, Some(1));
        keys.highlight = Some(0);
        assert_eq!(press(&mut keys, "enter", &labels), MenuKey::Ignored);
    }
}
