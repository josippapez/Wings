//! What the address bar loads for what was typed: an address as it is, a host with its scheme added, or a search.

/// The search engine that gets what isn't an address.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Engine {
    #[default]
    DuckDuckGo,
    Google,
}

impl Engine {
    pub const ALL: [Engine; 2] = [Engine::DuckDuckGo, Engine::Google];

    /// What is stored, and the value of its menu entry.
    pub fn id(self) -> &'static str {
        match self {
            Engine::DuckDuckGo => "duckduckgo",
            Engine::Google => "google",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Engine::DuckDuckGo => "DuckDuckGo",
            Engine::Google => "Google",
        }
    }

    pub fn from_id(id: &str) -> Option<Engine> {
        Engine::ALL.into_iter().find(|engine| engine.id() == id)
    }

    /// Where a tab with no address of its own starts.
    pub fn home(self) -> &'static str {
        match self {
            Engine::DuckDuckGo => "https://duckduckgo.com/",
            Engine::Google => "https://www.google.com/",
        }
    }

    fn search(self, query: &str) -> String {
        let base = match self {
            Engine::DuckDuckGo => "https://duckduckgo.com/?q=",
            Engine::Google => "https://www.google.com/search?q=",
        };
        format!("{base}{}", percent_encode(query))
    }
}

/// Every byte but the unreserved characters of RFC 3986 as `%XX`, so a query can't end or add to the address.
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The address to load for `input`, or none for an empty one. Only http, https and `about:blank` come out: anything
/// else (a `file:` or `javascript:` address, words) is searched for, never loaded.
pub fn resolve(input: &str, engine: Engine) -> Option<String> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    if input.eq_ignore_ascii_case("about:blank") {
        return Some("about:blank".to_owned());
    }
    if let Some(query) = input.strip_prefix('?') {
        let query = query.trim();
        return (!query.is_empty()).then(|| engine.search(query));
    }
    if has_web_scheme(input) {
        return Some(input.to_owned());
    }
    Some(host_address(input).unwrap_or_else(|| engine.search(input)))
}

fn has_web_scheme(input: &str) -> bool {
    let lower = input.to_ascii_lowercase();
    let Some(rest) = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://")) else {
        return false;
    };
    !rest.starts_with('/') && !rest.is_empty() && !input.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// `input` with the scheme a host gets: http for this computer, https for the rest. None when it isn't `host[:port][/path]`.
fn host_address(input: &str) -> Option<String> {
    if input.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let end = input.find(['/', '?', '#']).unwrap_or(input.len());
    let authority = &input[..end];
    let (host, port) = if authority.starts_with('[') {
        let close = authority.find(']')?;
        let inside = &authority[1..close];
        if inside.is_empty() || !inside.chars().all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.') {
            return None;
        }
        (&authority[..=close], authority[close + 1..].strip_prefix(':').or(authority[close + 1..].is_empty().then_some("")))
    } else {
        match authority.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, Some("")),
        }
    };
    // `Some("")` is no port, and `None` is something after the bracket that isn't one.
    let port = port?;
    if !port.is_empty() && (port.len() > 5 || port.parse::<u16>().is_err() || !port.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    if authority.ends_with(':') && port.is_empty() {
        return None;
    }
    let local = matches!(host, "localhost" | "127.0.0.1" | "[::1]");
    if !(local || host.starts_with('[') || is_ipv4(host) || is_dotted_name(host)) {
        return None;
    }
    Some(format!("{}://{input}", if local { "http" } else { "https" }))
}

fn is_ipv4(host: &str) -> bool {
    let labels: Vec<&str> = host.split('.').collect();
    labels.len() == 4 && labels.iter().all(|label| !label.is_empty() && label.len() <= 3 && label.parse::<u8>().is_ok())
}

/// `a.b` with letters, digits and hyphens in each label and a last label that isn't a number, so `1.5` and `v2.` are words.
fn is_dotted_name(host: &str) -> bool {
    let labels: Vec<&str> = host.split('.').collect();
    let plain = |label: &&str| !label.is_empty() && !label.starts_with('-') && !label.ends_with('-') && label.chars().all(|c| c.is_alphanumeric() || c == '-');
    let last = labels.last().copied().unwrap_or_default();
    labels.len() >= 2 && labels.iter().all(plain) && last.chars().count() >= 2 && !last.chars().all(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn go(input: &str) -> Option<String> {
        resolve(input, Engine::DuckDuckGo)
    }

    #[test]
    fn an_address_with_a_web_scheme_loads_as_typed() {
        assert_eq!(go("https://example.com/a?b=1"), Some("https://example.com/a?b=1".into()));
        assert_eq!(go("  http://localhost:3000  "), Some("http://localhost:3000".into()));
        assert_eq!(go("HTTPS://Example.com"), Some("HTTPS://Example.com".into()));
    }

    #[test]
    fn a_host_gets_https_and_this_computer_gets_http() {
        assert_eq!(go("example.com"), Some("https://example.com".into()));
        assert_eq!(go("sub.example.co.uk/a/b?c#d"), Some("https://sub.example.co.uk/a/b?c#d".into()));
        assert_eq!(go("example.com:8443/x"), Some("https://example.com:8443/x".into()));
        assert_eq!(go("localhost:3000"), Some("http://localhost:3000".into()));
        assert_eq!(go("localhost"), Some("http://localhost".into()));
        assert_eq!(go("127.0.0.1:8080/x"), Some("http://127.0.0.1:8080/x".into()));
        assert_eq!(go("[::1]:3000"), Some("http://[::1]:3000".into()));
        assert_eq!(go("[::1]"), Some("http://[::1]".into()));
        assert_eq!(go("192.168.1.5:80"), Some("https://192.168.1.5:80".into()));
        assert_eq!(go("münchen.de"), Some("https://münchen.de".into()));
    }

    #[test]
    fn words_are_searched_for_with_the_engine_percent_encoded() {
        assert_eq!(go("rust lang"), Some("https://duckduckgo.com/?q=rust%20lang".into()));
        assert_eq!(go("cats"), Some("https://duckduckgo.com/?q=cats".into()));
        assert_eq!(resolve("rust lang", Engine::Google), Some("https://www.google.com/search?q=rust%20lang".into()));
        assert_eq!(go("a&b=c#d"), Some("https://duckduckgo.com/?q=a%26b%3Dc%23d".into()));
    }

    #[test]
    fn a_question_mark_forces_a_search_even_for_a_host() {
        assert_eq!(go("?example.com"), Some("https://duckduckgo.com/?q=example.com".into()));
        assert_eq!(go("? two words "), Some("https://duckduckgo.com/?q=two%20words".into()));
        assert_eq!(go("?"), None);
    }

    #[test]
    fn unicode_is_encoded_as_utf8_bytes() {
        assert_eq!(go("café au lait"), Some("https://duckduckgo.com/?q=caf%C3%A9%20au%20lait".into()));
        assert_eq!(go("日本語"), Some("https://duckduckgo.com/?q=%E6%97%A5%E6%9C%AC%E8%AA%9E".into()));
    }

    #[test]
    fn about_blank_loads_and_any_other_scheme_is_searched_never_loaded() {
        assert_eq!(go("about:blank"), Some("about:blank".into()));
        assert_eq!(go("ABOUT:BLANK"), Some("about:blank".into()));
        assert_eq!(go("file:///x"), Some("https://duckduckgo.com/?q=file%3A%2F%2F%2Fx".into()));
        assert_eq!(go("javascript:alert(1)"), Some("https://duckduckgo.com/?q=javascript%3Aalert%281%29".into()));
        assert_eq!(go("about:config"), Some("https://duckduckgo.com/?q=about%3Aconfig".into()));
        assert_eq!(go("http:///x"), Some("https://duckduckgo.com/?q=http%3A%2F%2F%2Fx".into()));
    }

    #[test]
    fn what_only_looks_like_a_host_is_searched() {
        for input in ["1.5", "v2.", "a. b.com", "example.com/a b", "foo:bar", "host:99999", "host.com:", "-a.com", "[zz]:1", "user@example.com"] {
            assert!(go(input).unwrap().starts_with("https://duckduckgo.com/?q="), "{input}");
        }
    }

    #[test]
    fn nothing_typed_loads_nothing() {
        assert_eq!(go(""), None);
        assert_eq!(go("   "), None);
    }

    #[test]
    fn an_engine_is_found_by_the_id_it_is_stored_under() {
        for engine in Engine::ALL {
            assert_eq!(Engine::from_id(engine.id()), Some(engine));
        }
        assert_eq!(Engine::from_id("bing"), None);
    }
}
