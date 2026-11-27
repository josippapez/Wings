//! What an extension's manifest asks for, cut to what the person allowed, for an engine that grants whatever the manifest
//! asks for (Chromium): match patterns as Chrome reads them
//! (<https://developer.chrome.com/docs/extensions/develop/concepts/match-patterns>), and the manifest rewritten to name
//! no more.

use std::{fs, path::Path};

use serde_json::Value;

use crate::{manifest::is_host_pattern, Error};

/// A match pattern read: `<all_urls>`, or a scheme (`*` for http or https), a host (`*`, `*.example.com` or a name), a
/// port (none for any) and a path.
#[derive(Debug, PartialEq)]
enum Pattern<'a> {
    All,
    Url { scheme: &'a str, host: String, port: Option<&'a str>, path: &'a str },
}

fn parse(pattern: &str) -> Option<Pattern<'_>> {
    if pattern == "<all_urls>" {
        return Some(Pattern::All);
    }
    let (scheme, rest) = pattern.split_once("://")?;
    let at = rest.find('/')?;
    let (authority, path) = rest.split_at(at);
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if port == "*" || (!port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit())) => (host, Some(port).filter(|port| *port != "*")),
        _ => (authority, None),
    };
    let wildcard_ok = host == "*" || !host.contains('*') || (host.starts_with("*.") && !host[2..].contains('*'));
    (wildcard_ok && (scheme == "file" || !host.is_empty())).then(|| Pattern::Url { scheme, host: host.to_ascii_lowercase(), port, path })
}

/// Whether the host part `allowed` of a pattern reaches the host `host` (or every host the pattern host `host` names).
fn host_covers(allowed: &str, host: &str) -> bool {
    match allowed.strip_prefix("*.") {
        _ if allowed == "*" => true,
        Some(domain) => {
            let named = host.strip_prefix("*.").unwrap_or(host);
            named == domain || named.ends_with(&format!(".{domain}"))
        }
        None => allowed == host,
    }
}

/// Whether the match pattern `allowed` reaches every address the pattern `pattern` reaches, as WebKit's
/// `matchesPattern:` answers. A pattern either can't read reaches nothing. Paths count only as far as `allowed`'s is the
/// same or ends in its one `*`: it may say no where Chrome would say yes, never the other way.
pub fn pattern_covers(allowed: &str, pattern: &str) -> bool {
    let (Some(allowed), Some(pattern)) = (parse(allowed), parse(pattern)) else { return false };
    let (Pattern::Url { scheme: a_scheme, host: a_host, port: a_port, path: a_path }, Pattern::Url { scheme, host, port, path }) = (&allowed, &pattern) else {
        return allowed == Pattern::All;
    };
    let scheme_ok = a_scheme == scheme || (*a_scheme == "*" && matches!(*scheme, "http" | "https"));
    let path_ok = match a_path.strip_suffix('*') {
        Some(prefix) if !prefix.contains('*') => path.starts_with(prefix),
        _ => a_path == path,
    };
    scheme_ok && host_covers(a_host, host) && (a_port.is_none() || a_port == port) && path_ok
}

/// Whether the match pattern `pattern`, as a host permission (its path ignored), reaches the page at `url`.
pub fn pattern_reaches(pattern: &str, url: &str) -> bool {
    let (Some(pattern), Ok(url)) = (parse(pattern), url::Url::parse(url)) else { return false };
    let scheme = url.scheme();
    let Pattern::Url { scheme: wanted, host, port, .. } = pattern else {
        return matches!(scheme, "http" | "https" | "ws" | "wss" | "ftp" | "data" | "file" | "urn");
    };
    let scheme_ok = wanted == scheme || (wanted == "*" && matches!(scheme, "http" | "https"));
    // `Url` writes an IPv6 host in brackets, as a pattern does.
    let at = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let port_ok = port.is_none_or(|port| url.port_or_known_default().is_some_and(|at| at.to_string() == port));
    scheme_ok && !at.contains('*') && host_covers(&host, &at) && port_ok
}

/// Rewrites the manifest of the extension in `dir` so it asks for no more than the person allowed: of its `permissions`,
/// the names `permitted` grants and the host patterns `hosts` cover; of its `host_permissions`, those `hosts` cover; its
/// content scripts only on the pages `hosts` cover (a script left on none goes); of what it may ask for later
/// (`optional_permissions`), only the names `permitted` grants, moved into its `permissions` so it has them from the
/// start and its `permissions.request` answers yes with no prompt, the rest and `optional_host_permissions` gone; no
/// shortcuts (`commands`, which may work in every app on the
/// Mac, `"global": true`, or beat Wings' own in its pages); and no address to update itself from (`update_url`): Wings
/// decides which version runs. Answers the host patterns it kept, those of its content scripts too.
pub fn narrow_manifest(dir: &Path, permitted: &dyn Fn(&str) -> bool, hosts: &[String]) -> Result<Vec<String>, Error> {
    let file = dir.join("manifest.json");
    let raw = fs::read(&file)?;
    let mut manifest: Value =
        serde_json::from_str(String::from_utf8_lossy(&raw).trim_start_matches('\u{feff}')).map_err(|e| Error::Manifest(e.to_string()))?;
    let object = manifest.as_object_mut().ok_or_else(|| Error::Manifest("manifest.json is not an object".into()))?;
    let allowed = |pattern: &str| hosts.iter().any(|host| pattern_covers(host, pattern));
    let mut kept = Vec::new();
    let mut keep = |pattern: &str| {
        let ok = allowed(pattern);
        if ok && !kept.iter().any(|kept| kept == pattern) {
            kept.push(pattern.to_owned());
        }
        ok
    };
    if let Some(list) = object.get_mut("permissions").and_then(Value::as_array_mut) {
        list.retain(|entry| entry.as_str().is_some_and(|name| if is_host_pattern(name) { keep(name) } else { permitted(name) }));
    }
    let later: Vec<Value> = object
        .get("optional_permissions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|entry| entry.as_str().is_some_and(|name| !is_host_pattern(name) && permitted(name)))
        .cloned()
        .collect();
    if !later.is_empty() {
        let list = object.entry("permissions").or_insert_with(|| Value::Array(Vec::new()));
        if let Some(list) = list.as_array_mut() {
            for entry in later {
                if !list.contains(&entry) {
                    list.push(entry);
                }
            }
        }
    }
    if let Some(list) = object.get_mut("host_permissions").and_then(Value::as_array_mut) {
        list.retain(|entry| entry.as_str().is_some_and(&mut keep));
    }
    if let Some(scripts) = object.get_mut("content_scripts").and_then(Value::as_array_mut) {
        for script in scripts.iter_mut() {
            if let Some(matches) = script.get_mut("matches").and_then(Value::as_array_mut) {
                matches.retain(|entry| entry.as_str().is_some_and(&mut keep));
            }
        }
        scripts.retain(|script| script.get("matches").and_then(Value::as_array).is_some_and(|matches| !matches.is_empty()));
    }
    object.remove("optional_permissions");
    object.remove("optional_host_permissions");
    object.remove("commands");
    object.remove("update_url");
    fs::write(&file, serde_json::to_vec_pretty(&manifest).map_err(|e| Error::Manifest(e.to_string()))?)?;
    Ok(kept)
}
