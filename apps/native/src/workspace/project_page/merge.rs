//! What the Project page decides without drawing: how long ago a section was written, what an open editor takes of a
//! change in storage, a new section's title and the search. These are `ago.ts`, `merge.ts` and `search.ts` of
//! `apps/desktop/src/components/project-page/`, with their tests.

use wings_spaces::project_info::{Fact, Info, Item};

/// "2 hours ago", "yesterday", "just now": how long before `now` (both in ms) a time was.
pub(super) fn ago(at: u64, now: u64) -> String {
    const UNITS: [(&str, u64); 5] = [
        ("year", 365 * 86_400_000),
        ("month", 30 * 86_400_000),
        ("day", 86_400_000),
        ("hour", 3_600_000),
        ("minute", 60_000),
    ];
    let (past, diff) = if at <= now {
        (true, now - at)
    } else {
        (false, at - now)
    };
    let Some((unit, ms)) = UNITS.iter().find(|(_, ms)| diff >= *ms) else {
        return "just now".into();
    };
    let n = diff / ms;
    // `Intl.RelativeTimeFormat` with `numeric: "auto"`: one day back is "yesterday".
    match (n, past, *unit) {
        (1, true, "day") => "yesterday".into(),
        (1, false, "day") => "tomorrow".into(),
        (1, true, "month") => "last month".into(),
        (1, false, "month") => "next month".into(),
        (1, true, "year") => "last year".into(),
        (1, false, "year") => "next year".into(),
        (n, true, unit) => format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" }),
        (n, false, unit) => format!("in {n} {unit}{}", if n == 1 { "" } else { "s" }),
    }
}

/// What a section editor holds, apart from the text fields themselves.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct EditorState {
    /// The section's text as storage last confirmed it, to tell an agent's append from your own typing.
    pub base: String,
    /// What is in the text field.
    pub text: String,
    /// Typed since the last save went out.
    pub dirty: bool,
    /// A save is out and not answered yet.
    pub inflight: bool,
    /// Whether this editor ever sent a save. After that the stored text can lag behind what it saved, so only an
    /// extension of `base` is taken.
    pub sent: bool,
    /// The stored text when the last save went out: what it shows until it catches up with that save.
    pub stale: String,
}

/// What to do when storage reports `stored` for the section being edited, or `None` to leave the editor alone: the new
/// text and base. An agent's append goes after what you typed and never replaces it; an older text (the stored value
/// catching up with a save this editor made) is ignored; and nothing is taken while a save is out, since its answer
/// brings the rest.
pub(super) fn take_stored(s: &EditorState, stored: &str) -> Option<(String, String)> {
    if s.inflight || stored == s.base || (s.sent && stored == s.stale) {
        return None;
    }
    let extension = stored.starts_with(&s.base);
    if s.sent && !extension {
        return None;
    }
    if !s.dirty {
        return Some((stored.to_owned(), stored.to_owned()));
    }
    extension.then(|| {
        (
            format!("{}{}", s.text, &stored[s.base.len()..]),
            stored.to_owned(),
        )
    })
}

/// A title for a new section that no other section has: "Untitled", "Untitled 2"... Titles compare ignoring case.
pub(super) fn fresh_title<'a>(taken: impl IntoIterator<Item = &'a str>) -> String {
    let used: Vec<String> = taken.into_iter().map(|t| t.trim().to_lowercase()).collect();
    (1..)
        .map(|n| {
            if n == 1 {
                "Untitled".to_owned()
            } else {
                format!("Untitled {n}")
            }
        })
        .find(|title| !used.contains(&title.to_lowercase()))
        .unwrap()
}

/// The fact with this key, ignoring case and surrounding spaces, as storage matches keys.
pub(super) fn find_fact<'a>(facts: &'a [Fact], key: &str) -> Option<&'a Fact> {
    let key = key.trim().to_lowercase();
    facts.iter().find(|f| f.key.trim().to_lowercase() == key)
}

/// One page entry that matches: a fact (by key) or an item (by id).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Match {
    Fact(String),
    Item(String),
}

/// Entries whose text holds `query`, case-insensitive, plain text, in page order (facts, then items). Facts match on key
/// and value; sections on title and Markdown source; blocks on their block id. A blank query matches nothing.
pub(super) fn find_matches(info: &Info, query: &str) -> Vec<Match> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let has = |texts: &[&str]| texts.iter().any(|t| t.to_lowercase().contains(&q));
    let facts = info
        .facts
        .iter()
        .filter(|f| has(&[&f.key, &f.value]))
        .map(|f| Match::Fact(f.key.clone()));
    let items = info.items.iter().filter_map(|item| {
        let hit = match item {
            Item::Section { title, body, .. } => has(&[title, body]),
            Item::Block { block_id, .. } => has(&[block_id]),
        };
        hit.then(|| Match::Item(item.id().to_owned()))
    });
    facts.chain(items).collect()
}

/// The match Enter (`forward`) or Shift+Enter goes to from `at`, wrapping around; 0 when there are none.
pub(super) fn step_match(at: usize, forward: bool, count: usize) -> usize {
    if count == 0 {
        0
    } else if forward {
        (at + 1) % count
    } else {
        (at + count - 1) % count
    }
}

/// `at` kept inside the matches, for when they shrink under it (an edit while searching).
pub(super) fn clamp_match(at: usize, count: usize) -> usize {
    at.min(count.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wings_spaces::project_info::Who;

    const NOW: u64 = 1_792_000_000_000;

    #[test]
    fn counts_minutes_hours_and_days_back_and_one_day_is_yesterday() {
        assert_eq!(ago(NOW - 20_000, NOW), "just now");
        assert_eq!(ago(NOW - 5 * 60_000, NOW), "5 minutes ago");
        assert_eq!(ago(NOW - 3_600_000, NOW), "1 hour ago");
        assert_eq!(ago(NOW - 3 * 3_600_000, NOW), "3 hours ago");
        assert_eq!(ago(NOW - 86_400_000, NOW), "yesterday");
        assert_eq!(ago(NOW - 2 * 86_400_000, NOW), "2 days ago");
    }

    fn state(base: &str, text: &str) -> EditorState {
        EditorState {
            base: base.into(),
            text: text.into(),
            ..EditorState::default()
        }
    }

    fn took(text: &str) -> Option<(String, String)> {
        Some((text.into(), text.into()))
    }

    #[test]
    fn an_untouched_editor_takes_whatever_storage_holds() {
        let s = state("abc", "abc");
        assert_eq!(
            take_stored(&s, "abc\n\nfrom an agent"),
            took("abc\n\nfrom an agent")
        );
        assert_eq!(take_stored(&s, "rewritten"), took("rewritten"));
    }

    #[test]
    fn an_agents_append_goes_after_what_you_typed_and_a_rewrite_is_not_taken() {
        let s = EditorState {
            dirty: true,
            ..state("abc", "abc and mine")
        };
        assert_eq!(
            take_stored(&s, "abc\n\nappended"),
            Some(("abc and mine\n\nappended".into(), "abc\n\nappended".into()))
        );
        assert_eq!(take_stored(&s, "something else"), None);
    }

    #[test]
    fn nothing_is_taken_while_a_save_is_out_or_when_storage_still_shows_the_base() {
        let out = EditorState {
            inflight: true,
            ..state("abc", "abc")
        };
        assert_eq!(take_stored(&out, "abc more"), None);
        assert_eq!(take_stored(&state("abc", "abc"), "abc"), None);
    }

    #[test]
    fn after_a_save_storage_catching_up_is_ignored_but_an_extension_is_taken() {
        let s = EditorState {
            sent: true,
            stale: "abc".into(),
            ..state("abc mine", "abc mine")
        };
        assert_eq!(take_stored(&s, "abc"), None);
        assert_eq!(
            take_stored(&s, "abc mine\n\nmore"),
            took("abc mine\n\nmore")
        );
        assert_eq!(take_stored(&s, "other"), None);
    }

    #[test]
    fn a_new_sections_title_is_one_nobody_has_ignoring_case() {
        assert_eq!(fresh_title([]), "Untitled");
        assert_eq!(fresh_title(["untitled", "Untitled 2"]), "Untitled 3");
    }

    fn info() -> Info {
        let fact = |key: &str, value: &str| Fact {
            key: key.into(),
            value: value.into(),
            by: Who::You,
            updated_at: 0,
        };
        let section = |id: &str, title: &str, body: &str| Item::Section {
            id: id.into(),
            title: title.into(),
            body: body.into(),
            by: Who::You,
            updated_at: 0,
        };
        Info {
            facts: vec![
                fact("Dev server", "pnpm dev, port 5173"),
                fact("Deploy", "Run `PNPM build` first"),
            ],
            items: vec![
                section("s1", "Setup", "Install with **pnpm** install"),
                section("s2", "PNPM tips", ""),
                Item::Block {
                    id: "b1".into(),
                    plugin_id: "tasks".into(),
                    block_id: "board".into(),
                    state: None,
                    height: None,
                },
                section("s3", "Other", "nothing here"),
            ],
            ..Info::default()
        }
    }

    #[test]
    fn matches_are_case_insensitive_across_facts_sections_and_blocks_in_page_order() {
        let fact = |k: &str| Match::Fact(k.into());
        let item = |i: &str| Match::Item(i.into());
        assert_eq!(
            find_matches(&info(), "pnpm"),
            [fact("Dev server"), fact("Deploy"), item("s1"), item("s2")]
        );
        assert_eq!(find_matches(&info(), "DEPLOY"), [fact("Deploy")]);
        assert_eq!(find_matches(&info(), "board"), [item("b1")]);
        // The Markdown source is searched as plain text.
        assert_eq!(find_matches(&info(), "**pnpm**"), [item("s1")]);
        assert!(find_matches(&info(), "  ").is_empty());
        assert!(find_matches(&info(), "zzz").is_empty());
    }

    #[test]
    fn stepping_wraps_and_a_position_past_the_end_clamps() {
        assert_eq!(step_match(0, true, 3), 1);
        assert_eq!(step_match(2, true, 3), 0);
        assert_eq!(step_match(0, false, 3), 2);
        assert_eq!(step_match(0, true, 0), 0);
        assert_eq!(clamp_match(4, 3), 2);
        assert_eq!(clamp_match(2, 0), 0);
    }
}
