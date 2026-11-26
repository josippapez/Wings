//! Firefox: `profiles.ini` and the unencrypted `moz_cookies` table.

use crate::{Cookie, SameSite};
use serde_json::Value;

/// From this `PRAGMA user_version` on, `expiry` is in milliseconds, not seconds (yt-dlp cookies.py, citing Firefox
/// 142's schema 16; netwerk/cookie/CookiePersistentStorage.cpp binds `ExpiryInMSec()` at schema 17).
const MILLISECOND_EXPIRY_VERSION: i64 = 16;

/// Only cookies outside any container tab and any partition: those carry non-empty `originAttributes`
/// (`^userContextId=`, `^partitionKey=`), and importing them into one store would mix identities.
pub(crate) const ROWS_SQL: &str = "SELECT host, name, value, path, expiry, isSecure, isHttpOnly, sameSite \
     FROM moz_cookies WHERE originAttributes = ''";
pub(crate) const VERSION_SQL: &str = "PRAGMA user_version";

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Profile {
    pub name: String,
    /// As written in the file; relative to the Firefox folder when `relative`.
    pub path: String,
    pub relative: bool,
}

/// The `[ProfileN]` sections of profiles.ini, counted up from 0 until one has no `IsRelative`, with the same
/// requirements as nsToolkitProfileService.cpp (Path and Name both present).
pub(crate) fn parse_profiles_ini(text: &str) -> Vec<Profile> {
    let mut sections: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            sections.push((name.to_owned(), Vec::new()));
        } else if let (Some((_, pairs)), Some((k, v))) = (sections.last_mut(), line.split_once('=')) {
            pairs.push((k.trim().to_owned(), v.trim().to_owned()));
        }
    }
    let get = |pairs: &[(String, String)], key: &str| pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    let mut profiles = Vec::new();
    for n in 0.. {
        let Some((_, pairs)) = sections.iter().find(|(name, _)| *name == format!("Profile{n}")) else {
            break;
        };
        let Some(relative) = get(pairs, "IsRelative") else { break };
        if let (Some(path), Some(name)) = (get(pairs, "Path"), get(pairs, "Name")) {
            profiles.push(Profile { name, path, relative: relative == "1" });
        }
    }
    profiles
}

/// The `user_version` from `PRAGMA user_version` JSON; 0 when absent.
pub(crate) fn schema_version_from_json(json: &str) -> i64 {
    serde_json::from_str::<Value>(json)
        .ok()
        .and_then(|v| v.get(0)?.get("user_version")?.as_i64())
        .unwrap_or(0)
}

/// `nsICookie.idl`: SAMESITE_NONE 0, LAX 1, STRICT 2, UNSET 256.
fn same_site(value: i64) -> Option<SameSite> {
    match value {
        0 => Some(SameSite::None),
        1 => Some(SameSite::Lax),
        2 => Some(SameSite::Strict),
        _ => None,
    }
}

/// Returns the cookies and how many rows could not be mapped. Firefox only stores persistent cookies, so a
/// zero or missing expiry reads as a session cookie rather than an expired one.
pub(crate) fn cookies_from_json(json: &str, schema_version: i64, now: f64) -> Result<(Vec<Cookie>, usize), String> {
    if json.trim().is_empty() {
        return Ok((Vec::new(), 0));
    }
    let parsed: Value = serde_json::from_str(json).map_err(|e| format!("unexpected sqlite3 output: {e}"))?;
    let rows = parsed.as_array().ok_or("unexpected sqlite3 output")?;
    let divisor = if schema_version >= MILLISECOND_EXPIRY_VERSION { 1000.0 } else { 1.0 };
    let (mut cookies, mut skipped) = (Vec::new(), 0);
    for row in rows {
        let text = |key: &str| row.get(key).and_then(Value::as_str).map(str::to_owned);
        let int = |key: &str| row.get(key).and_then(Value::as_i64);
        let (Some(domain), Some(name), Some(value), Some(path)) = (text("host"), text("name"), text("value"), text("path"))
        else {
            skipped += 1;
            continue;
        };
        let expires = int("expiry").filter(|&e| e > 0).map(|e| e as f64 / divisor);
        if expires.is_some_and(|at| at <= now) {
            continue;
        }
        cookies.push(Cookie {
            domain,
            name,
            value,
            path,
            expires,
            secure: int("isSecure").unwrap_or(0) != 0,
            http_only: int("isHttpOnly").unwrap_or(0) != 0,
            same_site: int("sameSite").and_then(same_site),
        });
    }
    Ok((cookies, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_ini_sections() {
        let ini = "[Install4F96D1932A9F858E]\nDefault=Profiles/abc.default-release\nLocked=1\n\n\
                   [Profile1]\nName=work\nIsRelative=0\nPath=/Volumes/x/work\n\n\
                   [Profile0]\nName=default-release\nIsRelative=1\nPath=Profiles/abc.default-release\nDefault=1\n\n\
                   [General]\nStartWithLastProfile=1\nVersion=2\n";
        assert_eq!(
            parse_profiles_ini(ini),
            vec![
                Profile { name: "default-release".into(), path: "Profiles/abc.default-release".into(), relative: true },
                Profile { name: "work".into(), path: "/Volumes/x/work".into(), relative: false },
            ]
        );
        assert!(parse_profiles_ini("").is_empty());
    }

    const ROW: &str = r#"[{"host":".example.com","name":"a","value":"v","path":"/","expiry":1893456000000,"isSecure":1,"isHttpOnly":1,"sameSite":2},
        {"host":"old.com","name":"b","value":"v","path":"/","expiry":1000000,"isSecure":0,"isHttpOnly":0,"sameSite":256},
        {"host":"s.com","name":"c","value":"v","path":"/","expiry":0,"isSecure":0,"isHttpOnly":0,"sameSite":0}]"#;

    #[test]
    fn rows_map_with_millisecond_expiry() {
        let (cookies, skipped) = cookies_from_json(ROW, 17, 1_800_000_000.0).unwrap();
        assert_eq!(skipped, 0);
        // "old.com" expired 1970-01-12 in ms: dropped.
        assert_eq!(cookies.len(), 2);
        assert_eq!(cookies[0].domain, ".example.com");
        assert_eq!(cookies[0].expires, Some(1_893_456_000.0));
        assert!(cookies[0].secure && cookies[0].http_only);
        assert_eq!(cookies[0].same_site, Some(SameSite::Strict));
        assert_eq!(cookies[1].expires, None);
        assert_eq!(cookies[1].same_site, Some(SameSite::None));
    }

    #[test]
    fn older_schema_counts_seconds() {
        let json = r#"[{"host":"a.com","name":"a","value":"v","path":"/","expiry":1893456000,"isSecure":0,"isHttpOnly":0,"sameSite":256}]"#;
        let (cookies, _) = cookies_from_json(json, 15, 1_800_000_000.0).unwrap();
        assert_eq!(cookies[0].expires, Some(1_893_456_000.0));
        assert_eq!(cookies[0].same_site, None);
    }

    #[test]
    fn malformed_rows_are_counted() {
        let (cookies, skipped) = cookies_from_json(r#"[{"host":"a.com"}]"#, 17, 0.0).unwrap();
        assert_eq!((cookies.len(), skipped), (0, 1));
        assert!(cookies_from_json("oops", 17, 0.0).is_err());
    }

    #[test]
    fn schema_version_pragma() {
        assert_eq!(schema_version_from_json(r#"[{"user_version":17}]"#), 17);
        assert_eq!(schema_version_from_json(""), 0);
    }
}
