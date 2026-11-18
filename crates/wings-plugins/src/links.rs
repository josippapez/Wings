//! Words in a page that open something of a plugin's, like an issue key: the patterns a manifest's `contributes.links`
//! declares, checked when the plugin loads, and found in a page's text once when the text arrives. `regex` runs in time
//! linear in the text whatever the pattern, which a plugin writes.

use std::{collections::HashSet, sync::Arc};

use regex::{Regex, RegexBuilder};

use crate::manifest::{Link, MAX_LINK_PATTERN};

/// The most words `setLinkWords` keeps for one link, and the longest of them.
pub const MAX_WORDS: usize = 20_000;
pub const MAX_WORD: usize = 64;

/// A link's pattern, compiled, or why it can't be one.
pub fn compile(pattern: &str) -> Result<Regex, String> {
    if pattern.is_empty() || pattern.chars().count() > MAX_LINK_PATTERN {
        return Err(format!("a pattern is 1 to {MAX_LINK_PATTERN} characters"));
    }
    let regex = RegexBuilder::new(pattern)
        .size_limit(1 << 20)
        .dfa_size_limit(1 << 20)
        .build()
        .map_err(|error| format!("the pattern isn't a regular expression: {error}"))?;
    if regex.is_match("") {
        return Err("the pattern matches empty text".into());
    }
    Ok(regex)
}

/// The words a plugin narrows a link to, checked: each 1 to `MAX_WORD` characters with no space in it.
pub fn words(list: &[String]) -> Result<HashSet<String>, String> {
    if list.len() > MAX_WORDS {
        return Err(format!("A link takes at most {MAX_WORDS} words"));
    }
    let bad = |word: &String| word.is_empty() || word.chars().count() > MAX_WORD || word.chars().any(|c| c.is_whitespace() || c.is_control());
    if let Some(word) = list.iter().find(|word| bad(word)) {
        return Err(format!("A link's word is 1 to {MAX_WORD} characters with no spaces, not {word:?}"));
    }
    Ok(list.iter().cloned().collect())
}

/// One running plugin's link, ready to find: whose it is, its pattern, and the words it is narrowed to, if any.
#[derive(Clone, Debug)]
pub struct Rule {
    pub plugin: String,
    pub link: String,
    pub title: String,
    regex: Regex,
    words: Option<Arc<HashSet<String>>>,
}

impl Rule {
    /// `None` for a pattern that doesn't compile, which a plugin that loaded can't have.
    pub fn new(plugin: &str, link: &Link, words: Option<Arc<HashSet<String>>>) -> Option<Rule> {
        Some(Rule { plugin: plugin.to_owned(), link: link.id.clone(), title: link.title.clone(), regex: compile(&link.pattern).ok()?, words })
    }
}

/// A link found in a text: its byte range and the index of its rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Found {
    pub start: usize,
    pub end: usize,
    pub rule: usize,
}

/// The address a `url` link's text opens: the text in the normal form a browser reads it in, which is what `may_open_url`
/// checks, so `http://localhost:5173` opens as `http://localhost:5173/`.
pub fn url_of(text: &str) -> Option<String> {
    url::Url::parse(text).ok().map(|url| url.as_str().to_owned())
}

fn word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The most of a text, in bytes, that links are looked for in: more than a chat's paragraph, and a bound on what one costs.
pub const MAX_TEXT: usize = 20_000;

/// Every link in the first `MAX_TEXT` bytes of `text`, in order and none overlapping another: what a rule's pattern matches
/// with no letter, digit or `_` right before or after it and, for a rule narrowed to words, is one of them. Where two
/// overlap, the one that starts first wins, and at the same start the earlier rule.
pub fn find(text: &str, rules: &[Rule]) -> Vec<Found> {
    let mut cut = text.len().min(MAX_TEXT);
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut found = Vec::new();
    for (index, rule) in rules.iter().enumerate() {
        // One pass a rule: a match that isn't a whole word is passed over, never searched again from inside it, so a text
        // full of near misses costs one search.
        for hit in rule.regex.find_iter(&text[..cut]) {
            let (start, end) = (hit.start(), hit.end());
            let whole = end > start && !text[..start].chars().next_back().is_some_and(word) && !text[end..].chars().next().is_some_and(word);
            if whole && rule.words.as_ref().map_or(true, |words| words.contains(&text[start..end])) {
                found.push(Found { start, end, rule: index });
            }
        }
    }
    found.sort_by_key(|found| (found.start, found.rule));
    let mut kept: Vec<Found> = Vec::new();
    for link in found {
        if kept.last().map_or(true, |last| link.start >= last.end) {
            kept.push(link);
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(id: &str, pattern: &str) -> Link {
        Link { id: id.into(), title: "Issue".into(), pattern: pattern.into(), view: "issue".into(), arg: "key".into(), url: false }
    }

    fn texts<'a>(text: &'a str, found: &[Found]) -> Vec<&'a str> {
        found.iter().map(|found| &text[found.start..found.end]).collect()
    }

    #[test]
    fn keys_are_found_as_whole_words_and_near_misses_are_not() {
        let rules = [Rule::new("tasks", &link("issue", "[A-Z]{1,3}-[0-9]{1,9}"), None).unwrap()];
        let text = "Fixed ABC-12 and `XY-3`; see (Q-400). Not XABC-12, abc-12, ABC-, ABC-12a, ABCD-1 or ABC_12, but ABC-7.";
        assert_eq!(texts(text, &find(text, &rules)), ["ABC-12", "XY-3", "Q-400", "ABC-7"]);
        assert!(find("", &rules).is_empty());
        // Byte ranges stay on character boundaries around other scripts.
        let wide = "Über ABC-1 ✓";
        assert_eq!(texts(wide, &find(wide, &rules)), ["ABC-1"]);
    }

    #[test]
    fn a_link_narrowed_to_words_finds_only_those() {
        let known = Arc::new(words(&["ABC-12".to_owned()]).unwrap());
        let rules = [Rule::new("tasks", &link("issue", "[A-Z]{1,3}-[0-9]{1,9}"), Some(known)).unwrap()];
        let text = "UTF-8 and SHA-256 aren't issues, ABC-12 is, ABC-13 isn't one yet.";
        assert_eq!(texts(text, &find(text, &rules)), ["ABC-12"]);
    }

    /// A text full of near misses costs one search a rule, not one from each place inside a near miss, and only its first
    /// `MAX_TEXT` bytes are searched.
    #[test]
    fn a_text_of_near_misses_is_searched_once() {
        let rules = [
            Rule::new("tasks", &link("issue", "[A-Z]{1,3}-[0-9]{1,9}"), None).unwrap(),
            Rule::new("other", &link("words", "[a-z]+"), None).unwrap(),
        ];
        let started = std::time::Instant::now();
        assert!(find(&"ABC-".repeat(25_000), &rules).is_empty());
        // One word of 19,999 letters with a digit after it: each place in it was searched again, a quadratic cost.
        assert!(find(&format!("{}1", "a".repeat(MAX_TEXT - 1)), &rules).is_empty());
        assert!(find(&"XABC-1".repeat(20_000), &rules).is_empty());
        assert!(started.elapsed() < std::time::Duration::from_secs(2), "{:?}", started.elapsed());
        // Past `MAX_TEXT`, nothing is looked for.
        let late = format!("{} ABC-7", " ".repeat(MAX_TEXT));
        assert!(find(&late, &rules).is_empty());
    }

    /// `may_open_url` takes only an address in its normal form, which a port with nothing after it isn't.
    #[test]
    fn a_url_link_opens_the_address_in_its_normal_form() {
        assert_eq!(url_of("http://localhost:5173").as_deref(), Some("http://localhost:5173/"));
        assert_eq!(url_of("http://localhost:5173/cart?step=2").as_deref(), Some("http://localhost:5173/cart?step=2"));
        assert_eq!(url_of("no address"), None);
    }

    #[test]
    fn overlapping_links_keep_the_first() {
        let rules = [
            Rule::new("dev-servers", &link("local", "http://localhost:[0-9]{2,5}"), None).unwrap(),
            Rule::new("other", &link("port", "localhost:[0-9]{2,5}"), None).unwrap(),
        ];
        let text = "open http://localhost:5173 now";
        let found = find(text, &rules);
        assert_eq!((texts(text, &found), found[0].rule), (vec!["http://localhost:5173"], 0));
    }

    #[test]
    fn patterns_and_words_are_checked() {
        assert!(compile("[A-Z]+-[0-9]+").is_ok());
        assert!(compile("").is_err());
        assert!(compile("x*").unwrap_err().contains("empty"));
        assert!(compile("(unclosed").unwrap_err().contains("isn't a regular expression"));
        assert!(compile(&"a".repeat(MAX_LINK_PATTERN + 1)).is_err());
        assert!(words(&["two words".to_owned()]).is_err());
        assert!(words(&[String::new()]).is_err());
        assert_eq!(words(&["A-1".to_owned(), "A-1".to_owned()]).unwrap().len(), 1);
    }
}
