//! @-mentions of files in the message box: which word of the draft is being mentioned, which of the project's files
//! match it, and what the draft becomes once you pick one. It makes no host calls, so its tests run.

/// The most files kept. A big monorepo lists far more, and every keystroke ranks them all; the first ones git lists
/// (its order is by path) are enough for a menu of eight.
pub const MOST_FILES: usize = 20_000;
/// How many files the menu offers.
pub const MENU: usize = 8;

/// The project's files, with each path lowercased once so ranking a keystroke doesn't allocate per file.
#[derive(Default)]
pub struct Files {
    paths: Vec<String>,
    lower: Vec<String>,
}

impl Files {
    /// From what `git ls-files -z` printed: paths ending in a NUL.
    pub fn from_git(stdout: &str) -> Files {
        let paths: Vec<String> = stdout.split('\0').filter(|path| !path.is_empty()).take(MOST_FILES).map(str::to_owned).collect();
        let lower = paths.iter().map(|path| path.to_lowercase()).collect();
        Files { paths, lower }
    }

    /// Up to `MENU` files that match `query`, best first.
    pub fn matches(&self, query: &str) -> Vec<&str> {
        let query = query.to_lowercase();
        let mut found: Vec<(u8, usize, usize)> = self
            .lower
            .iter()
            .enumerate()
            .filter_map(|(at, path)| rank(path, &query).map(|tier| (tier, path.len(), at)))
            .collect();
        found.sort_unstable();
        found.into_iter().take(MENU).map(|(.., at)| self.paths[at].as_str()).collect()
    }
}

/// How well a lowercased `path` matches the lowercased `query`: 0 when its file name starts with it, 1 when the path
/// holds it, 2 when its characters come in order, None when not.
fn rank(path: &str, query: &str) -> Option<u8> {
    let name = path.rsplit('/').next().unwrap_or(path);
    if name.starts_with(query) {
        Some(0)
    } else if path.contains(query) {
        Some(1)
    } else {
        let mut rest = path.chars();
        query.chars().all(|ch| rest.any(|have| have == ch)).then_some(2)
    }
}

/// The `@word` the draft ends in: where it starts and what follows the `@`. None when the last word isn't one, or the
/// draft ends in a space (that word is done).
pub fn active(draft: &str) -> Option<(usize, &str)> {
    let start = draft.char_indices().rev().find(|(_, ch)| ch.is_whitespace()).map_or(0, |(at, ch)| at + ch.len_utf8());
    draft[start..].strip_prefix('@').map(|query| (start, query))
}

/// The draft with its last `@word` (from `start`) replaced by a mention of `path`. A path with a space is quoted, as
/// Claude Code reads it.
pub fn insert(draft: &str, start: usize, path: &str) -> String {
    let mention = if path.contains(char::is_whitespace) { format!("@\"{path}\"") } else { format!("@{path}") };
    format!("{}{mention} ", &draft[..start])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(paths: &[&str]) -> Files {
        Files::from_git(&paths.join("\0"))
    }

    #[test]
    fn the_last_word_is_the_mention() {
        assert_eq!(active("fix @src/che"), Some((4, "src/che")));
        assert_eq!(active("@a"), Some((0, "a")));
        assert_eq!(active("fix @"), Some((4, "")));
        assert_eq!(active("see\n@x"), Some((4, "x")));
    }

    #[test]
    fn a_finished_or_plain_word_is_not() {
        assert_eq!(active("fix @src/che "), None);
        assert_eq!(active("fix src"), None);
        assert_eq!(active("mail a@b"), None);
        assert_eq!(active(""), None);
    }

    #[test]
    fn picking_replaces_the_word() {
        assert_eq!(insert("fix @src/che", 4, "src/chat.rs"), "fix @src/chat.rs ");
        assert_eq!(insert("@", 0, "a b.txt"), "@\"a b.txt\" ");
    }

    #[test]
    fn a_file_name_start_beats_a_path_part_beats_scattered_letters() {
        let all = files(&["docs/chat/readme.md", "src/chat.rs", "c/h/a/t.rs", "src/other.rs"]);
        assert_eq!(all.matches("chat"), ["src/chat.rs", "docs/chat/readme.md", "c/h/a/t.rs"]);
    }

    #[test]
    fn ties_go_to_the_shorter_path() {
        let all = files(&["a/b/main.rs", "main.rs", "a/main.rs"]);
        assert_eq!(all.matches("main"), ["main.rs", "a/main.rs", "a/b/main.rs"]);
    }

    #[test]
    fn matching_ignores_case() {
        let all = files(&["src/README.md"]);
        assert_eq!(all.matches("readme"), ["src/README.md"]);
        assert_eq!(all.matches("SRC/rd"), ["src/README.md"]);
    }

    #[test]
    fn letters_must_come_in_order() {
        let all = files(&["src/chat.rs"]);
        assert_eq!(all.matches("scr"), ["src/chat.rs"]);
        assert!(all.matches("tsc").is_empty());
    }

    #[test]
    fn the_menu_has_at_most_eight() {
        let paths: Vec<String> = (0..20).map(|n| format!("f{n}.rs")).collect();
        let all = Files::from_git(&paths.join("\0"));
        assert_eq!(all.matches("").len(), MENU);
    }

    #[test]
    fn the_list_is_capped() {
        let paths: Vec<String> = (0..MOST_FILES + 5).map(|n| format!("f{n}")).collect();
        assert_eq!(Files::from_git(&paths.join("\0")).paths.len(), MOST_FILES);
    }
}
