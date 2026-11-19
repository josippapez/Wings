//! `listbox`: one option highlighted, moved with the arrow keys (UI SDK's `createListbox`). Up and Down move it and wrap
//! at the ends; Home and End jump to the first and last option when the list has the keyboard itself (`Focus::Real`,
//! a menu), not when a field keeps it and Home and End move its caret (`Focus::Virtual`, a combobox). Disabled options
//! are skipped. Enter is the caller's: it picks the highlighted option.

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

#[cfg(test)]
mod tests {
    use super::*;

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
