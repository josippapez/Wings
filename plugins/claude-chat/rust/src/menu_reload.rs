//! Asks Claude Code to look for skills on disk again when the slash menu opens, so one installed from a terminal outside
//! the chat shows up without /reload-plugins or a new chat. Typing in a field isn't a gesture a page may write on, so
//! Wings lets this one request go unasked, at most one every 10 s (`RELOAD_EVERY_MS` in the app's `process.rs`).

/// The shortest time between two asks: Wings refuses a request sooner, which the chat would show as an error.
const EVERY_MS: u64 = 10_000;

#[derive(Default)]
pub struct MenuReload {
    /// Whether this opening of the menu (a draft that starts with `/`) has asked already.
    asked: bool,
    last: Option<u64>,
}

impl MenuReload {
    /// What the draft is now: true when the request should go, at most once per opening of the menu and never within
    /// `EVERY_MS` of the last one. The menu itself shows the list it has at once and changes in place when `commands_changed` comes.
    pub fn typed(&mut self, draft: &str, now: u64) -> bool {
        if !draft.starts_with('/') {
            self.asked = false;
            return false;
        }
        if std::mem::replace(&mut self.asked, true) || self.last.is_some_and(|last| now.saturating_sub(last) < EVERY_MS) {
            return false;
        }
        self.last = Some(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_once_when_the_menu_opens_and_again_only_after_it_closed() {
        let mut reload = MenuReload::default();
        assert!(!reload.typed("", 0));
        assert!(!reload.typed("hello", 1));
        assert!(reload.typed("/", 20_000));
        assert!(!reload.typed("/de", 20_100), "typing on in the same opening");
        assert!(!reload.typed("/dep", 40_000), "still the same opening, however long");
        assert!(!reload.typed("", 41_000));
        assert!(reload.typed("/", 60_000), "a new opening");
    }

    #[test]
    fn does_not_ask_again_within_ten_seconds() {
        let mut reload = MenuReload::default();
        assert!(reload.typed("/", 20_000));
        assert!(!reload.typed("", 21_000));
        assert!(!reload.typed("/", 25_000), "Wings would refuse it");
        assert!(!reload.typed("", 26_000));
        assert!(reload.typed("/", 30_000));
    }
}
