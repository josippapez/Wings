//! @-mentions of files and folders in the message box: which word of the draft is being mentioned, which of the
//! project's files and folders match it, and what the draft becomes once you pick one. A line range follows a picked
//! file as `#L10` or `#L10-20`, which Claude Code reads as lines of that file. It makes no host calls, so its tests run.

/// The most files kept. A big monorepo lists more, and every keystroke ranks them all: 200,000 paths rank in about 15 ms (native release)
/// (`a_keystroke_ranks_two_hundred_thousand_paths_quickly`), and the first 20,000 that git listed (its order
/// is by path) left every file after them out of the menu.
pub const MOST_FILES: usize = 200_000;
/// The most folders kept, derived from the files' paths.
pub const MOST_DIRS: usize = 50_000;
/// How many entries, files and folders together, the menu offers.
pub const MENU: usize = 8;

/// The project's files, with each path lowercased once so ranking a keystroke doesn't allocate per file, and the folders
/// they sit in, each ending in `/`.
#[derive(Default)]
pub struct Files {
    paths: Vec<String>,
    lower: Vec<String>,
    dirs: Vec<String>,
    dirs_lower: Vec<String>,
    /// The same paths, for whether a reply names one of them, word by word as it shows.
    set: std::collections::HashSet<String>,
}

impl Files {
    /// From what `git ls-files -z` printed: paths ending in a NUL.
    pub fn from_git(stdout: &str) -> Files {
        let paths: Vec<String> = stdout.split('\0').filter(|path| !path.is_empty()).take(MOST_FILES).map(str::to_owned).collect();
        let lower = paths.iter().map(|path| path.to_lowercase()).collect();
        let set = paths.iter().cloned().collect();
        let dirs = folders(&paths);
        let dirs_lower = dirs.iter().map(|dir| dir.to_lowercase()).collect();
        Files { paths, lower, dirs, dirs_lower, set }
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// Whether `path`, relative to the project, is one of its files.
    pub fn contains(&self, path: &str) -> bool {
        self.set.contains(path)
    }

    /// Up to `MENU` files and folders (a folder ends in `/`) that match `query`, best first. A `#` that starts a line range
    /// means the file is already picked, so nothing matches.
    pub fn matches(&self, query: &str) -> Vec<&str> {
        if range_at(query).is_some() {
            return Vec::new();
        }
        let query = query.to_lowercase();
        let all = self.lower.iter().chain(&self.dirs_lower);
        let mut found: Vec<(u8, usize, usize)> =
            all.enumerate().filter_map(|(at, path)| rank(path, &query).map(|tier| (tier, path.len(), at))).collect();
        found.sort_unstable();
        found
            .into_iter()
            .take(MENU)
            .map(|(.., at)| self.paths.get(at).unwrap_or_else(|| &self.dirs[at - self.paths.len()]).as_str())
            .collect()
    }
}

/// The folders the `paths` sit in, with their parents, sorted, each ending in `/`.
fn folders(paths: &[String]) -> Vec<String> {
    let mut found: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for path in paths {
        let mut dir = path.as_str();
        // A folder already seen has had its parents added too.
        while let Some((parent, _)) = dir.rsplit_once('/') {
            if !found.insert(&path[..parent.len() + 1]) {
                break;
            }
            dir = parent;
        }
    }
    let mut dirs: Vec<String> = found.into_iter().map(str::to_owned).collect();
    dirs.sort_unstable();
    dirs.truncate(MOST_DIRS);
    dirs
}

/// Where a line range starts in what follows the `@`: the `#` of `#`, `#L`, `#L10`, `#L10-20` or `#10-20`, being typed
/// or done. None for a `#` inside a name, like `a#b.rs`.
pub fn range_at(query: &str) -> Option<usize> {
    let at = query.rfind('#')?;
    let rest = query[at + 1..].strip_prefix(['L', 'l']).unwrap_or(&query[at + 1..]);
    rest.chars().all(|ch| ch.is_ascii_digit() || ch == '-').then_some(at)
}

/// A finished range, as Claude Code reads one: the text after the `#` of `#L10`, `#l10-20` or `#10` as `L10`, `L10-20`.
/// Claude Code takes the lines from `#L…` only, and takes anything else after a `#` for part of no range.
fn range_form(range: &str) -> Option<String> {
    let numbers = range.strip_prefix(['L', 'l']).unwrap_or(range);
    let (start, end) = match numbers.split_once('-') {
        Some((start, end)) => (start, Some(end)),
        None => (numbers, None),
    };
    let digits = |text: &str| !text.is_empty() && text.chars().all(|ch| ch.is_ascii_digit());
    (digits(start) && end.is_none_or(digits)).then(|| end.map_or(format!("L{start}"), |end| format!("L{start}-{end}")))
}

/// The message as it goes to Claude Code: a line range after a mention in the form it reads (`@a.rs#10-20` is
/// `@a.rs#L10-20`), and one typed after a space (`@a.rs #L10`) joined to its mention. A `#12` after a space stays, since
/// that is an issue.
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut after_mention = false;
    let mut rest = text;
    while !rest.is_empty() {
        let (space, tail) = rest.split_at(rest.len() - rest.trim_start().len());
        let end = tail.find(char::is_whitespace).unwrap_or(tail.len());
        let (word, next) = tail.split_at(end);
        rest = next;
        if word.starts_with('@') && word.len() > 1 {
            after_mention = true;
            out.push_str(space);
            match range_at(word).and_then(|at| range_form(&word[at + 1..]).map(|form| (at, form))) {
                Some((at, form)) => {
                    out.push_str(&word[..at]);
                    out.push('#');
                    out.push_str(&form);
                }
                None => out.push_str(word),
            }
            continue;
        }
        let joined = word.strip_prefix(['#']).filter(|range| after_mention && space == " " && range.starts_with(['L', 'l']));
        match joined.and_then(range_form) {
            Some(form) => {
                out.push('#');
                out.push_str(&form);
            }
            None => {
                out.push_str(space);
                out.push_str(word);
                after_mention = false;
            }
        }
    }
    out
}

/// Whether the last word of `draft` is a line range typed after a mention and a space (`@a.rs #L1`), which isn't an
/// issue's `#`.
pub fn range_after_mention(draft: &str, start: usize) -> bool {
    let word = &draft[start..];
    let before = draft[..start].trim_end_matches(' ');
    draft[..start].ends_with(' ')
        && word.strip_prefix('#').is_some_and(|rest| rest.starts_with(['L', 'l']) && range_at(word).is_some())
        && before.rsplit(char::is_whitespace).next().is_some_and(|prev| prev.starts_with('@'))
}

/// How well a lowercased `path` matches the lowercased `query`: 0 when its file name starts with it, 1 when the path
/// holds it, 2 when its characters come in order, None when not.
fn rank(path: &str, query: &str) -> Option<u8> {
    // A folder's name is the part before its trailing `/`.
    let name = path.trim_end_matches('/').rsplit('/').next().unwrap_or(path);
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
/// Claude Code reads it. A folder (ending in `/`) has no space after it, so the menu goes on into it.
pub fn insert(draft: &str, start: usize, path: &str) -> String {
    let quoted = path.contains(char::is_whitespace);
    let mention = if quoted { format!("@\"{path}\"") } else { format!("@{path}") };
    let end = if path.ends_with('/') && !quoted { "" } else { " " };
    format!("{}{mention}{end}", &draft[..start])
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
        assert_eq!(all.matches("chat"), ["docs/chat/", "src/chat.rs", "docs/chat/readme.md", "c/h/a/t.rs"]);
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
    fn folders_come_from_the_paths_with_their_parents() {
        let all = files(&["src/a/b.rs", "src/c.rs", "README.md"]);
        assert_eq!(all.dirs, ["src/", "src/a/"]);
        assert_eq!(files(&["x.rs"]).dirs, Vec::<String>::new());
    }

    #[test]
    fn a_folder_is_offered_before_what_is_in_it() {
        let all = files(&["src/chat.rs", "src/view/mod.rs", "docs/src.md"]);
        assert_eq!(all.matches("src"), ["src/", "docs/src.md", "src/view/", "src/chat.rs", "src/view/mod.rs"]);
        assert_eq!(all.matches("src/"), ["src/", "src/view/", "src/chat.rs", "src/view/mod.rs"]);
        assert_eq!(all.matches("view/"), ["src/view/", "src/view/mod.rs"]);
    }

    #[test]
    fn picking_a_folder_keeps_the_word_open() {
        assert_eq!(insert("see @sr", 4, "src/"), "see @src/");
        assert_eq!(insert("@", 0, "my dir/"), "@\"my dir/\" ");
        assert_eq!(active("see @src/"), Some((4, "src/")));
    }

    #[test]
    fn a_line_range_after_a_file_hides_the_menu() {
        let all = files(&["src/a.rs"]);
        assert!(!all.matches("src/a.rs").is_empty());
        for typed in ["src/a.rs#", "src/a.rs#L", "src/a.rs#L10", "src/a.rs#L10-", "src/a.rs#L10-20", "src/a.rs#10-20"] {
            assert!(all.matches(typed).is_empty(), "{typed}");
        }
        // A `#` in a name isn't a range.
        assert_eq!(files(&["a#b.rs"]).matches("a#b"), ["a#b.rs"]);
        // The mention is still the last word, so picking another file replaces the range with it.
        assert_eq!(active("see @src/a.rs#L10"), Some((4, "src/a.rs#L10")));
    }

    #[test]
    fn a_range_is_sent_as_claude_code_reads_it() {
        assert_eq!(normalize("see @src/a.rs#L10 now"), "see @src/a.rs#L10 now");
        assert_eq!(normalize("see @src/a.rs#L10-20"), "see @src/a.rs#L10-20");
        assert_eq!(normalize("see @src/a.rs#10-20"), "see @src/a.rs#L10-20");
        assert_eq!(normalize("see @a.rs#l5"), "see @a.rs#L5");
        assert_eq!(normalize("see @a.rs #L5-6 and more"), "see @a.rs#L5-6 and more");
        assert_eq!(normalize("a\n@a.rs\n#L5"), "a\n@a.rs\n#L5");
    }

    #[test]
    fn what_is_not_a_range_is_left_alone() {
        assert_eq!(normalize("fix @a.rs #12 and C# @b.rs#"), "fix @a.rs #12 and C# @b.rs#");
        assert_eq!(normalize("@a.rs#L"), "@a.rs#L");
        assert_eq!(normalize("@a.rs#L10- x"), "@a.rs#L10- x");
        assert_eq!(normalize("two  spaces @a.rs"), "two  spaces @a.rs");
        assert_eq!(normalize("see #L5"), "see #L5");
    }

    #[test]
    fn a_range_typed_after_a_mention_is_not_an_issue() {
        assert!(range_after_mention("see @a.rs #L", 10));
        assert!(range_after_mention("see @a.rs #L10-2", 10));
        assert!(!range_after_mention("see a.rs #L", 9));
        assert!(!range_after_mention("see @a.rs #12", 10));
        assert!(!range_after_mention("see @a.rs\n#L1", 10));
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

    /// 200,000 files are all known, the last one included, and one keystroke's ranking of them is quick.
    #[test]
    fn a_keystroke_ranks_two_hundred_thousand_paths_quickly() {
        let path = |n: u32| format!("packages/pkg{:04}/src/module{}/file{n}.ts", n % 4_000, n % 37);
        let listed: String = (0..200_000).map(|n| path(n) + "\0").collect();
        let known = Files::from_git(&listed);
        assert!(known.contains(&path(199_999)), "the last file is known");
        assert_eq!(known.matches("file199999"), [path(199_999).as_str()]);
        // The worst keystroke, one letter that every path holds, and a longer one.
        for query in ["e", "module3", "file1999"] {
            let start = std::time::Instant::now();
            let found = known.matches(query);
            let took = start.elapsed();
            eprintln!("ranking {query:?} on 200,000 paths: {took:?}");
            assert_eq!(found.len(), MENU);
            assert!(took.as_millis() < 1_000, "{query}: {took:?}");
        }
    }
}
