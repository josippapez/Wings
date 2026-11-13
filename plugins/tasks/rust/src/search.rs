//! Searching issues.

use crate::domain::Issue;

/// The words of a search, lowercased.
pub fn query_words(query: &str) -> Vec<String> {
    query.to_lowercase().split_whitespace().map(str::to_owned).collect()
}

/// What a search looks at, lowercased: key, title, description and the text of every log entry.
pub fn haystack(issue: &Issue) -> String {
    let mut text = format!("{}\n{}\n{}", issue.key, issue.title, issue.description.as_deref().unwrap_or_default());
    for entry in &issue.log {
        text.push('\n');
        text.push_str(&entry.text);
    }
    text.to_lowercase()
}

/// Whether an issue matches a search: every word is somewhere in its key, title, description or log, in any case. No words
/// matches all.
pub fn matches_query(issue: &Issue, query: &str) -> bool {
    let words = query_words(query);
    if words.is_empty() {
        return true;
    }
    let text = haystack(issue);
    words.iter().all(|w| text.contains(w))
}

/// Each issue's text lowercased once, so a keystroke is a substring test per issue and not a pass over every log.
pub struct Index(Vec<String>);

impl Index {
    pub fn new(issues: &[Issue]) -> Self {
        Index(issues.iter().map(haystack).collect())
    }

    /// The positions of the issues the words all match.
    pub fn matching(&self, query: &str) -> Vec<usize> {
        let words = query_words(query);
        self.0.iter().enumerate().filter(|(_, text)| words.iter().all(|w| text.contains(w))).map(|(at, _)| at).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{Actor, Op},
        ops::apply_op,
        testing::{create_with, fresh},
    };

    #[test]
    fn every_word_any_case_over_key_title_description_and_log_notes() {
        let mut s = create_with(&fresh(), "Checkout summary", |c| { c.description = Some("Round the Totals once".into()); c.rel.kind = Some("bug".into()); }, &Actor::You, 1.0).store;
        s = create_with(&s, "Login page", |c| c.status = Some("in_progress".into()), &Actor::You, 1.0).store;
        s = apply_op(&s, &Op::Note { key: "WEB-2".into(), text: "Waiting on the Checkout API".into() }, &Actor::You, 3.0).unwrap().store;
        let (a, b) = (&s.issues[0], &s.issues[1]);
        assert!(matches_query(a, "") && matches_query(a, "   "), "no words matches all");
        assert!(matches_query(a, "CHECKOUT"), "title, any case");
        assert!(matches_query(a, "round totals"), "description, words in any order");
        assert!(matches_query(a, "totals round"));
        assert!(!matches_query(a, "totals login"), "every word has to match");
        assert!(matches_query(b, "checkout api"), "log note");
        assert!(matches_query(b, "web-2") && matches_query(a, "web-1"), "key");
        assert!(!matches_query(b, "zebra"));
        let index = Index::new(&s.issues);
        assert_eq!(index.matching("checkout"), [0, 1]);
        assert_eq!(index.matching("checkout api"), [1]);
        assert_eq!(index.matching("  "), [0, 1]);
    }
}
