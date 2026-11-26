//! The pages that finished loading, newest first, kept in the plugin's storage where every tab reads it.

use serde_json::{Value, json};
use wings_plugin::host::storage;

const KEY: &str = "history";
/// The most entries kept: the oldest go first.
pub const MAX: usize = 500;

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub url: String,
    pub title: String,
    pub at: f64,
}

/// Puts the page first, once: an older entry for the same address goes. Only web pages are kept.
pub fn record(list: &mut Vec<Entry>, url: &str, title: &str, at: f64) {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return;
    }
    list.retain(|entry| entry.url != url);
    list.insert(0, Entry { url: url.to_owned(), title: title.to_owned(), at });
    list.truncate(MAX);
}

/// The host and port of an address, as typed after the scheme.
fn host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.split(['/', '?', '#']).next().unwrap_or_default()
}

/// Up to `limit` entries for what is typed, in the order kept: those whose host starts with it (with or without
/// `www.`) or whose title or address holds it, whatever the case.
pub fn suggest<'a>(list: &'a [Entry], typed: &str, limit: usize) -> Vec<&'a Entry> {
    let typed = typed.trim().to_lowercase();
    if typed.is_empty() {
        return Vec::new();
    }
    list.iter()
        .filter(|entry| {
            let host = host(&entry.url).to_lowercase();
            host.starts_with(&typed) || host.strip_prefix("www.").is_some_and(|bare| bare.starts_with(&typed)) || entry.title.to_lowercase().contains(&typed) || entry.url.to_lowercase().contains(&typed)
        })
        .take(limit)
        .collect()
}

pub fn load() -> Vec<Entry> {
    let stored = storage::get(KEY).unwrap_or(Value::Null);
    let entries = stored.as_array().map(Vec::as_slice).unwrap_or_default();
    entries
        .iter()
        .filter_map(|entry| Some(Entry { url: entry["url"].as_str()?.to_owned(), title: entry["title"].as_str().unwrap_or_default().to_owned(), at: entry["at"].as_f64().unwrap_or_default() }))
        .take(MAX)
        .collect()
}

fn save(list: &[Entry]) -> Result<(), String> {
    storage::set(KEY, &Value::Array(list.iter().map(|entry| json!({ "url": entry.url, "title": entry.title, "at": entry.at })).collect()))
}

/// Adds a finished page to what is kept. Storage calls are answered at once, so two tabs can't lose each other's entry.
pub fn add(url: &str, title: &str, at: f64) -> Result<(), String> {
    let mut list = load();
    record(&mut list, url, title, at);
    save(&list)
}

pub fn clear() -> Result<(), String> {
    storage::delete(KEY)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kept(list: &[Entry]) -> Vec<&str> {
        list.iter().map(|entry| entry.url.as_str()).collect()
    }

    fn urls(found: Vec<&Entry>) -> Vec<&str> {
        found.into_iter().map(|entry| entry.url.as_str()).collect()
    }

    #[test]
    fn a_page_goes_first_and_an_older_one_at_the_same_address_goes() {
        let mut list = Vec::new();
        record(&mut list, "https://a.com/", "A", 1.0);
        record(&mut list, "https://b.com/", "B", 2.0);
        record(&mut list, "https://a.com/", "A again", 3.0);
        assert_eq!(kept(&list), ["https://a.com/", "https://b.com/"]);
        assert_eq!(list[0], Entry { url: "https://a.com/".into(), title: "A again".into(), at: 3.0 });
    }

    #[test]
    fn only_the_newest_500_are_kept() {
        let mut list = Vec::new();
        for n in 0..MAX + 20 {
            record(&mut list, &format!("https://site{n}.com/"), "", n as f64);
        }
        assert_eq!(list.len(), MAX);
        assert_eq!(list[0].url, format!("https://site{}.com/", MAX + 19));
        assert_eq!(list[MAX - 1].url, "https://site20.com/");
    }

    #[test]
    fn what_is_not_a_web_page_is_not_kept() {
        let mut list = Vec::new();
        record(&mut list, "about:blank", "", 1.0);
        record(&mut list, "file:///x", "", 1.0);
        assert!(list.is_empty());
    }

    fn sample() -> Vec<Entry> {
        let entry = |url: &str, title: &str| Entry { url: url.into(), title: title.into(), at: 0.0 };
        vec![
            entry("https://www.rust-lang.org/learn", "Learn Rust"),
            entry("https://docs.rs/serde", "serde - Rust"),
            entry("http://localhost:3000/", "My app"),
            entry("https://example.com/rust-news", "News"),
        ]
    }

    #[test]
    fn a_host_prefix_matches_with_or_without_www_and_the_port_counts() {
        let list = sample();
        assert_eq!(urls(suggest(&list, "rust-l", 6)), ["https://www.rust-lang.org/learn"]);
        assert_eq!(urls(suggest(&list, "www.ru", 6)), ["https://www.rust-lang.org/learn"]);
        assert_eq!(urls(suggest(&list, "localhost:30", 6)), ["http://localhost:3000/"]);
    }

    #[test]
    fn a_title_or_address_holds_what_is_typed_whatever_the_case() {
        let list = sample();
        assert_eq!(urls(suggest(&list, "RUST", 6)), ["https://www.rust-lang.org/learn", "https://docs.rs/serde", "https://example.com/rust-news"]);
        assert_eq!(urls(suggest(&list, "my app", 6)), ["http://localhost:3000/"]);
    }

    #[test]
    fn at_most_the_limit_come_back_and_nothing_for_nothing() {
        let list = sample();
        assert_eq!(suggest(&list, "rust", 2).len(), 2);
        assert!(suggest(&list, "  ", 6).is_empty());
        assert!(suggest(&list, "zzz", 6).is_empty());
    }

    #[test]
    fn the_scheme_is_not_part_of_the_host() {
        assert_eq!(host("https://a.com:81/x?y"), "a.com:81");
        assert_eq!(host("http://a.com#z"), "a.com");
    }
}
