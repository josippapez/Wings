//! What a plugin may open or fetch: URL prefixes, pinned POST bodies, secrets bound to a prefix, and fetch headers.

use serde::Deserialize;
use serde_json::Value;

use crate::manifest::Plugin;

/// A fetch prefix has to name a host and end its host part with `/`, so `https://api.example.com` can't
/// also match `https://api.example.com.evil.net`.
pub(crate) fn fetch_prefix_ok(prefix: &str) -> bool {
    prefix.strip_prefix("https://").is_some_and(|rest| rest.split_once('/').is_some_and(|(host, _)| !host.is_empty() && !host.contains(['@', '*'])))
}

/// An `openUrl` entry: `https://` for any page, an https prefix that ends its host with `/` like a fetch entry, or http
/// to this computer with a port or path next. `https://dev.azure.com` would match `https://dev.azure.com.example.com/` too.
pub(crate) fn open_url_prefix_ok(prefix: &str) -> bool {
    let loopback =
        ["http://localhost", "http://127.0.0.1", "http://[::1]"].iter().any(|host| prefix.strip_prefix(host).is_some_and(|rest| rest.starts_with([':', '/'])));
    prefix == "https://" || fetch_prefix_ok(prefix) || loopback
}

/// One `permissions.fetch` entry.
pub(crate) struct FetchRule<'a> {
    /// `None` is a bare prefix for any method.
    method: Option<&'a str>,
    target: &'a str,
    pins: Vec<Pin<'a>>,
}

/// A body field that must be one of these values: `{event=APPROVE}` or `{vote=10|5|0}`.
pub(crate) type Pin<'a> = (&'a str, Vec<&'a str>);

/// Splits a trailing ` {field=a|b,other=c}` off an entry. `None` if it is there but malformed.
pub(crate) fn split_pins(entry: &str) -> Option<(&str, Vec<Pin<'_>>)> {
    let entry = entry.trim_end();
    if !entry.ends_with('}') {
        return Some((entry, Vec::new()));
    }
    let at = entry.rfind(" {")?;
    let mut pins = Vec::new();
    for item in entry[at + 2..entry.len() - 1].split(',') {
        let (key, values) = item.split_once('=')?;
        let word = |w: &str| !w.is_empty() && w.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b));
        let values: Vec<&str> = values.split('|').collect();
        if !word(key) || key.contains(['-', '.']) || !values.iter().all(|v| word(v)) {
            return None;
        }
        pins.push((key, values));
    }
    (!pins.is_empty()).then_some((&entry[..at], pins))
}

/// Whether a body's top-level keys can't be read two ways. Go and .NET servers match JSON keys without regard to
/// case (Go also folds `ſ` to `s` and the Kelvin sign to `k`), so with a pinned field `{"event":"APPROVED",
/// "EVENT":"REQUEST_CHANGES"}` would pass the pin and act on the other key. A body that pins anything must use ASCII
/// keys, none repeated, none equal to another ignoring ASCII case.
pub(crate) fn keys_unambiguous<'a>(keys: impl IntoIterator<Item = &'a str>) -> bool {
    let mut seen = std::collections::HashSet::new();
    keys.into_iter().all(|k| k.is_ascii() && seen.insert(k.to_ascii_lowercase()))
}

/// The top-level keys of a JSON object in order, repeats included, which `serde_json::Value` would merge.
fn json_keys(text: &str) -> Option<Vec<String>> {
    struct Keys(Vec<String>);
    impl<'de> Deserialize<'de> for Keys {
        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            struct V;
            impl<'de> serde::de::Visitor<'de> for V {
                type Value = Keys;
                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    f.write_str("a JSON object")
                }
                fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Keys, A::Error> {
                    let mut keys = Vec::new();
                    while let Some((k, _)) = map.next_entry::<String, serde::de::IgnoredAny>()? {
                        keys.push(k);
                    }
                    Ok(Keys(keys))
                }
            }
            d.deserialize_map(V)
        }
    }
    serde_json::from_str::<Keys>(text).ok().map(|k| k.0)
}

/// Whether the body's fields hold every pinned value, as text (a whole number counts as its digits).
pub(crate) fn pins_hold(pins: &[Pin<'_>], body: &serde_json::Map<String, Value>) -> bool {
    pins.iter().all(|(key, values)| {
        let got = match body.get(*key) {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            _ => return false,
        };
        values.contains(&got.as_str())
    })
}

pub(crate) fn fetch_rule(entry: &str) -> Option<FetchRule<'_>> {
    let (core, pins) = split_pins(entry)?;
    let (method, target) = match core.split_once(' ') {
        Some((m @ ("GET" | "POST"), rest)) => (Some(m), rest),
        Some(_) => return None,
        None => (None, core),
    };
    if method != Some("POST") && (!pins.is_empty() || target.contains('*')) {
        return None;
    }
    if method == Some("POST") && (target.contains(['?', '#']) || !target.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.~*/:%".contains(&b))) {
        return None;
    }
    fetch_prefix_ok(target).then_some(FetchRule { method, target, pins })
}

/// Whether a `POST` rule's URL pattern covers `url`: the same segments, where `*` takes one plain segment.
fn post_url_matches(pattern: &str, url: &str) -> bool {
    let (want, got): (Vec<&str>, Vec<&str>) = (pattern.split('/').collect(), url.split('/').collect());
    want.len() == got.len() && want.iter().zip(&got).enumerate().all(|(i, (w, g))| if *w == "*" { i > 2 && post_segment_ok(g) } else { w == g })
}

/// The URL to send, if the plugin may call it with this method and body: https, no credentials, under a
/// `permissions.fetch` entry, and already in normal form. The path may only use letters, digits and `-_.~/`, with
/// no `;` or percent escapes, so no server can read it as a different path (`..;/`, `..%2F`, double encoding) than
/// the one that was checked. A `POST` entry also needs the body to be a JSON object that holds its pinned fields.
pub fn fetch_url(plugin: &Plugin, method: &str, url: &str, body: Option<&str>) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let normal = parsed.as_str();
    let plain_path = parsed.path().bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.~/".contains(&b));
    let ok =
        parsed.scheme() == "https" && parsed.username().is_empty() && parsed.password().is_none() && parsed.fragment().is_none() && normal == url && plain_path;
    if !ok {
        return None;
    }
    let method = method.to_ascii_uppercase();
    let allowed = plugin.manifest.permissions.fetch.iter().filter_map(|e| fetch_rule(e)).any(|rule| match rule.method {
        None => normal.starts_with(rule.target),
        Some("GET") => matches!(method.as_str(), "GET" | "HEAD") && normal.starts_with(rule.target),
        _ => {
            method == "POST"
                && parsed.query().is_none()
                && post_url_matches(rule.target, normal)
                && match body.filter(|b| !b.is_empty()) {
                    // A POST that pins nothing may send no body, like Bitbucket's approve.
                    None => rule.pins.is_empty(),
                    Some(b) => {
                        b.len() <= POST_BODY_LIMIT
                            && (rule.pins.is_empty() || json_keys(b).is_some_and(|k| keys_unambiguous(k.iter().map(String::as_str))))
                            && matches!(serde_json::from_str::<Value>(b), Ok(Value::Object(o)) if pins_hold(&rule.pins, &o))
                    }
                }
        }
    });
    allowed.then(|| normal.to_string())
}

/// Whether a pinned `POST` rule covers this call. Its body was checked as JSON, so it must go out as JSON: with a
/// form or UTF-16 content type, `{"event":"APPROVED","x":"&event=REQUEST_CHANGES&"}` reads as the other event.
pub fn fetch_needs_json(plugin: &Plugin, method: &str, url: &str) -> bool {
    method.eq_ignore_ascii_case("POST")
        && plugin
            .manifest
            .permissions
            .fetch
            .iter()
            .filter_map(|e| fetch_rule(e))
            .any(|rule| rule.method == Some("POST") && !rule.pins.is_empty() && post_url_matches(rule.target, url))
}

/// Whether `wings.secrets.set` may keep a secret of this name: only one `permissions.secrets` binds to a URL prefix.
pub fn declares_secret(plugin: &Plugin, name: &str) -> bool {
    plugin.manifest.permissions.secrets.contains_key(name)
}

/// Whether `wings.fetch` may send this secret as the bearer to `url`, the normal form `fetch_url` returned: only under
/// the prefix the manifest binds it to. Its host part ends in `/` (`fetch_prefix_ok`), so a longer host can't match.
pub fn may_send_secret(plugin: &Plugin, name: &str, url: &str) -> bool {
    plugin.manifest.permissions.secrets.get(name).is_some_and(|prefix| url.starts_with(prefix.as_str()))
}

/// The only headers a plugin may set. Anything else could route a request, and the token on it, to a
/// different site or path behind the same server (`Host`, `X-Forwarded-*`, `X-HTTP-Host-Override`).
pub fn fetch_header_ok(name: &str) -> bool {
    ["accept", "accept-language", "content-type", "if-none-match", "if-modified-since", "cache-control"].contains(&name.to_ascii_lowercase().as_str())
}

/// A URL segment matched by `*`. Percent escapes stay, since GitLab project paths (`g%2Fp`) and Azure project
/// names (`My%20Project`) need them, but no decoding of it may step out of its place (`%2e%2e`, `%252e`, `%5c`).
pub(crate) fn post_segment_ok(segment: &str) -> bool {
    if segment.is_empty() || !segment.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.~%".contains(&b)) {
        return false;
    }
    percent_decoded(segment).is_some_and(|decoded| !decoded.contains(['%', '\\', '?', '#']) && !decoded.split('/').any(|part| part == "." || part == ".."))
}

/// A segment with its `%XX` escapes decoded, or `None` if one is malformed.
pub(crate) fn percent_decoded(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let (mut decoded, mut i) = (Vec::new(), 0);
    while i < bytes.len() {
        if bytes[i] == b'%' {
            decoded.push(segment.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())?);
            i += 3;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }
    Some(String::from_utf8_lossy(&decoded).into_owned())
}

pub(crate) const POST_BODY_LIMIT: usize = 64 * 1024;

pub fn may_open_url(plugin: &Plugin, url: &str) -> bool {
    (url.starts_with("https://") || is_loopback_http(url)) && plugin.manifest.permissions.open_url.iter().any(|prefix| url.starts_with(prefix))
}

/// An http URL that can only reach this computer: host `localhost`, `127.0.0.1` or `[::1]`, no credentials,
/// and already in the normal form the browser would read, so no backslash or odd escape can move the host.
fn is_loopback_http(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|u| {
        u.scheme() == "http"
            && matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
            && u.username().is_empty()
            && u.password().is_none()
            && u.as_str() == url
    })
}

/// A secret's name: letters, digits, `-` and `_`, up to 64 characters.
pub fn valid_secret_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{loading::load, post::may_post, test_support::plugin};
    use std::{fs, path::Path};

    #[test]
    fn fetch_only_reaches_declared_https_prefixes() {
        let p = plugin(Path::new("/tmp"), &[], &[]);
        assert_eq!(fetch_url(&p, "GET", "https://api.example.com/v1/timers", None).as_deref(), Some("https://api.example.com/v1/timers"));
        for url in [
            "http://api.example.com/v1/timers",
            "https://api.example.com/v2/x",
            "https://api.example.com.evil.net/v1/",
            "https://user:pw@api.example.com/v1/",
            "not a url",
            // Paths a server might decode or normalise to somewhere outside /v1/.
            "https://api.example.com/v1/../admin",
            "https://api.example.com/v1/..%2Fadmin",
            "https://api.example.com/v1/%2e%2e/admin",
            "https://api.example.com/v1\\..\\admin",
            "https://API.example.com/v1/x",
            "https://api.example.com/v1/..;/admin",
            "https://api.example.com/v1/%252e%252e%252fadmin",
            "https://api.example.com/v1/a%20b",
            "https://api.example.com/v1/x#frag",
        ] {
            assert!(fetch_url(&p, "GET", url, None).is_none(), "{url}");
        }
        assert_eq!(
            fetch_url(&p, "GET", "https://api.example.com/v1/time-entries?from=2026-10-05&to=2026-10-08", None).as_deref(),
            Some("https://api.example.com/v1/time-entries?from=2026-10-05&to=2026-10-08")
        );
        for name in ["Accept", "content-type", "If-None-Match"] {
            assert!(fetch_header_ok(name), "{name}");
        }
        for name in [
            "Host",
            "x-forwarded-host",
            "X-Forwarded",
            "Forwarded",
            "Cookie",
            "Transfer-Encoding",
            "Authorization",
            "X-Real-IP",
            "X-HTTP-Host-Override",
            "X-HTTP-Method-Override",
            "X-Override-URL",
            "X-Api-Key",
        ] {
            assert!(!fetch_header_ok(name), "{name}");
        }
        assert!(fetch_prefix_ok("https://api.example.com/"));
        assert!(!fetch_prefix_ok("https://api.example.com") && !fetch_prefix_ok("http://x.com/") && !fetch_prefix_ok("https:///x"));
    }

    /// A plugin with two fetch hosts, loaded from a manifest that binds its secrets as given.
    fn with_secrets(dir: &Path, secrets: Value) -> Result<Plugin, String> {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("main.js"), "").unwrap();
        let permissions = serde_json::json!({ "fetch": ["https://api.example.com/v1/", "https://other.example.com/"], "secrets": secrets });
        let manifest = serde_json::json!({ "id": "t", "name": "T", "version": "0", "api": 1, "main": "main.js", "permissions": permissions });
        fs::write(dir.join("wings-plugin.json"), manifest.to_string()).unwrap();
        load(dir)
    }

    #[test]
    fn a_bearer_secret_only_goes_to_the_prefix_it_is_bound_to() {
        let dir = std::env::temp_dir().join(format!("wings-bearer-{}", std::process::id()));
        let p = with_secrets(&dir, serde_json::json!({ "api": "https://api.example.com/v1/" })).unwrap();
        let (own, other) = ("https://api.example.com/v1/timers", "https://other.example.com/collect");
        // The plugin may call both hosts, but its token only goes to the one the manifest names.
        assert_eq!(fetch_url(&p, "GET", own, None).as_deref(), Some(own));
        assert_eq!(fetch_url(&p, "GET", other, None).as_deref(), Some(other));
        assert!(may_send_secret(&p, "api", own));
        assert!(!may_send_secret(&p, "api", other));
        assert!(!may_send_secret(&p, "api", "https://api.example.com/v2/x"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn only_declared_secrets_can_be_kept_or_sent() {
        let dir = std::env::temp_dir().join(format!("wings-secret-names-{}", std::process::id()));
        let p = with_secrets(&dir, serde_json::json!({ "api": "https://api.example.com/v1/" })).unwrap();
        assert!(declares_secret(&p, "api"));
        assert!(!declares_secret(&p, "other"));
        assert!(!may_send_secret(&p, "other", "https://api.example.com/v1/timers"));
        // A manifest that declares no secrets sends none, whatever its fetch entries allow.
        assert!(!may_send_secret(&with_secrets(&dir, serde_json::json!({})).unwrap(), "api", "https://api.example.com/v1/timers"));
        for bad in [
            serde_json::json!({ "api": "https://api.example.com" }),
            serde_json::json!({ "api": "https://api.example.com.evil.net" }),
            serde_json::json!({ "api": "http://api.example.com/" }),
            serde_json::json!({ "a b": "https://api.example.com/" }),
            serde_json::json!({ "api": ["https://api.example.com/"] }),
        ] {
            assert!(with_secrets(&dir, bad.clone()).is_err(), "{bad}");
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn open_url_entries_are_checked_on_load() {
        for entry in ["https://", "https://dev.azure.com/", "http://localhost:", "http://127.0.0.1:8080/", "http://[::1]:"] {
            assert!(open_url_prefix_ok(entry), "{entry}");
        }
        // Each of these would also match another host: dev.azure.com.example.com, localhost.example.com, user@host.
        for entry in
            ["https://dev.azure.com", "http://localhost", "https://dev.azure.com@example.com/", "https://*.example.com/", "http://example.com/", "ftp://x/", ""]
        {
            assert!(!open_url_prefix_ok(entry), "{entry}");
        }
        let dir = std::env::temp_dir().join(format!("wings-open-url-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.js"), "").unwrap();
        let manifest =
            serde_json::json!({ "id": "t", "name": "T", "version": "0", "api": 1, "main": "main.js", "permissions": { "openUrl": ["https://dev.azure.com"] } });
        fs::write(dir.join("wings-plugin.json"), manifest.to_string()).unwrap();
        assert!(load(&dir).err().unwrap().contains("openUrl entry"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn fetch_writes_need_their_own_pinned_entry() {
        let mut p = plugin(Path::new("/tmp"), &[], &[]);
        p.manifest.permissions.fetch = vec![
            "GET https://api.example.com/2.0/".into(),
            "POST https://api.example.com/2.0/repos/*/*/pulls/*/comments".into(),
            "POST https://api.example.com/2.0/repos/*/*/pulls/*/approve".into(),
            "POST https://api.example.com/v1/repos/*/*/pulls/*/reviews {event=APPROVED}".into(),
        ];
        let comments = "https://api.example.com/2.0/repos/w/r/pulls/3/comments";
        let body = Some(r#"{"content":{"raw":"Thanks"},"parent":{"id":7}}"#);
        assert!(fetch_url(&p, "GET", "https://api.example.com/2.0/repos/w/r/pulls/3", None).is_some());
        assert!(fetch_url(&p, "head", "https://api.example.com/2.0/user", None).is_some());
        assert!(fetch_url(&p, "POST", comments, body).is_some());
        // A GET prefix doesn't let a write through, anywhere under it.
        for method in ["POST", "PUT", "PATCH", "DELETE"] {
            assert!(fetch_url(&p, method, "https://api.example.com/2.0/repos/w/r/pulls/3/merge", Some("{}")).is_none(), "{method}");
        }
        // Only POST, only that path, no query, a JSON object body.
        assert!(fetch_url(&p, "PUT", comments, body).is_none());
        assert!(fetch_url(&p, "DELETE", comments, None).is_none());
        assert!(fetch_url(&p, "POST", "https://api.example.com/2.0/repos/w/r/pulls/3/comments/9", body).is_none());
        assert!(fetch_url(&p, "POST", "https://api.example.com/2.0/repos/w/r/pulls/3/comments?x=1", body).is_none());
        assert!(fetch_url(&p, "POST", "https://api.example.com/2.0/repos/w/r/pulls/../comments", body).is_none());
        assert!(fetch_url(&p, "POST", "https://api.example.com/2.0/repos/w/r/pulls/%2e%2e/comments", body).is_none());
        assert!(fetch_url(&p, "POST", comments, Some("not json")).is_none());
        assert!(fetch_url(&p, "POST", comments, Some("[1]")).is_none());
        let huge = format!(r#"{{"content":"{}"}}"#, "x".repeat(POST_BODY_LIMIT));
        assert!(fetch_url(&p, "POST", comments, Some(&huge)).is_none());
        // No body is fine where nothing is pinned (Bitbucket's approve), but not where a field is.
        assert!(fetch_url(&p, "POST", "https://api.example.com/2.0/repos/w/r/pulls/3/approve", None).is_some());
        let review = "https://api.example.com/v1/repos/o/r/pulls/3/reviews";
        assert!(fetch_url(&p, "POST", review, Some(r#"{"event":"APPROVED"}"#)).is_some());
        assert!(fetch_url(&p, "POST", review, Some(r#"{"event":"REQUEST_CHANGES","body":"x"}"#)).is_none());
        assert!(fetch_url(&p, "POST", review, Some(r#"{"body":"x"}"#)).is_none());
        assert!(fetch_url(&p, "POST", review, None).is_none());
        // A host's own path can't be reached by a `*` (the host is literal).
        assert!(fetch_url(&p, "POST", "https://evil.example.com/2.0/repos/w/r/pulls/3/comments", body).is_none());
    }

    #[test]
    fn fetch_entries_are_checked_on_load() {
        for entry in [
            "https://api.example.com/",
            "GET https://api.example.com/2.0/",
            "POST https://api.example.com/x/*/y",
            "POST https://api.example.com/x/*/reviews {event=APPROVED}",
        ] {
            assert!(fetch_rule(entry).is_some(), "{entry}");
        }
        for entry in [
            "PUT https://api.example.com/x",
            "DELETE https://api.example.com/",
            "GET https://api.example.com/x/*/y",
            "GET https://api.example.com/ {a=b}",
            "https://api.example.com/*/y",
            "POST https://*/x",
            "POST https://api.example.com/x?y=1",
            "POST https://api.example.com {a=b}",
            "POST https://api.example.com/x {a=}",
            "POST http://api.example.com/x",
            "GET",
        ] {
            assert!(fetch_rule(entry).is_none(), "{entry}");
        }
    }

    #[test]
    fn a_pinned_post_goes_out_as_json_only() {
        let mut p = plugin(Path::new("/tmp"), &[], &[]);
        p.manifest.permissions.fetch = vec![
            "POST https://api.example.com/v1/repos/*/*/pulls/*/reviews {event=APPROVED}".into(),
            "POST https://api.example.com/v1/repos/*/*/issues/*/comments".into(),
        ];
        assert!(fetch_needs_json(&p, "POST", "https://api.example.com/v1/repos/o/r/pulls/1/reviews"));
        assert!(fetch_needs_json(&p, "post", "https://api.example.com/v1/repos/o/r/pulls/1/reviews"));
        // An unpinned rule has nothing to read two ways, and other methods carry no checked body.
        assert!(!fetch_needs_json(&p, "POST", "https://api.example.com/v1/repos/o/r/issues/1/comments"));
        assert!(!fetch_needs_json(&p, "GET", "https://api.example.com/v1/repos/o/r/pulls/1/reviews"));
    }

    #[test]
    fn a_pinned_body_cannot_hide_a_second_key() {
        let mut p = plugin(Path::new("/tmp"), &[], &[]);
        p.manifest.permissions.post = vec!["gh repos/*/*/pulls/*/reviews {event=APPROVE}".into()];
        p.manifest.permissions.fetch = vec!["POST https://api.example.com/v1/repos/*/*/pulls/*/reviews {event=APPROVED}".into()];
        let fields = |v: serde_json::Value| v.as_object().unwrap().clone();
        let url = "repos/o/r/pulls/1/reviews";
        for bad in [
            serde_json::json!({ "event": "APPROVE", "EVENT": "REQUEST_CHANGES" }),
            serde_json::json!({ "event": "APPROVE", "Event": "REQUEST_CHANGES" }),
            serde_json::json!({ "event": "APPROVE", "ſ": "x" }),
            serde_json::json!({ "event": "APPROVE", "\u{212A}ey": "x" }),
        ] {
            assert!(!may_post(&p, "gh", url, &fields(bad.clone())), "{bad}");
        }
        assert!(may_post(&p, "gh", url, &fields(serde_json::json!({ "event": "APPROVE", "body": "ok" }))));
        let review = "https://api.example.com/v1/repos/o/r/pulls/1/reviews";
        for bad in [
            r#"{"event":"APPROVED","EVENT":"REQUEST_CHANGES"}"#,
            r#"{"event":"REQUEST_CHANGES","event":"APPROVED"}"#,
            r#"{"event":"APPROVED","event":"REQUEST_CHANGES"}"#,
            r#"{"event":"APPROVED","ſ":"x"}"#,
            r#"{"event":"APPROVED","\u212Aey":"x"}"#,
            r#"{"ev\u0065nt":"APPROVED","EVENT":"REQUEST_CHANGES"}"#,
        ] {
            assert!(fetch_url(&p, "POST", review, Some(bad)).is_none(), "{bad}");
        }
        assert!(fetch_url(&p, "POST", review, Some(r#"{"event":"APPROVED","body":"ok"}"#)).is_some());
        // An entry that pins nothing is not affected.
        p.manifest.permissions.fetch = vec!["POST https://api.example.com/v1/c/*/comments".into()];
        assert!(fetch_url(&p, "POST", "https://api.example.com/v1/c/1/comments", Some(r#"{"a":1,"A":2}"#)).is_some());
    }

    #[test]
    fn open_url_needs_https_and_a_declared_prefix() {
        let p = plugin(Path::new("/tmp"), &[], &[]);
        assert!(may_open_url(&p, "https://github.com/a/b/pull/1"));
        assert!(!may_open_url(&p, "https://evil.example/"));
        assert!(!may_open_url(&p, "file:///etc/passwd"));
    }

    #[test]
    fn open_url_allows_http_only_to_this_computer() {
        let mut p = plugin(Path::new("/tmp"), &[], &[]);
        p.manifest.permissions.open_url.push("http://localhost:".into());
        p.manifest.permissions.open_url.push("http://127.0.0.1:".into());
        assert!(may_open_url(&p, "http://localhost:5173/"));
        assert!(may_open_url(&p, "http://localhost:8765/a/b?c=d"));
        assert!(may_open_url(&p, "http://127.0.0.1:3000/"));
        for url in [
            "http://localhost.evil.com:80/",
            "http://localhost:80@evil.com/",
            "http://localhost:80\\@evil.com/",
            "http://evil.com:80/",
            "http://localhost@evil.com/",
            "http://[::1]:5173/",
            "https://localhost:5173/",
            "http://LOCALHOST:5173/",
        ] {
            assert!(!may_open_url(&p, url), "{url}");
        }
        // Not declared: a plugin with only https prefixes can't open localhost.
        assert!(!may_open_url(&plugin(Path::new("/tmp"), &[], &[]), "http://localhost:5173/"));
        // A broad prefix still can't reach past this computer.
        p.manifest.permissions.open_url.push("http://".into());
        assert!(!may_open_url(&p, "http://example.com/"));
        assert!(may_open_url(&p, "http://[::1]:5173/"));
    }
}
