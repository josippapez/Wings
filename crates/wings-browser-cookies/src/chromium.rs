//! Chromium's cookie store (Chrome, Arc, Brave, Edge, Chromium, Vivaldi): row mapping and value decryption.
//! The system side (finding profiles, copying the database, the Keychain) is in `system.rs`.

use crate::crypto::{self, Key};
use crate::{Cookie, Read, ReadError, SameSite};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Microseconds from 1601-01-01 to 1970-01-01 (base/time/time.h: kTimeTToMicrosecondsOffset).
const WINDOWS_TO_UNIX_MICROS: i64 = 11_644_473_600_000_000;

/// From this `meta` version on, the encrypted value starts with the SHA-256 of `host_key`
/// (net/extras/sqlite/sqlite_persistent_cookie_store.cc: "Version 24 adds a SHA256 hash of the domain value to front
/// of the the encrypted_value").
const HASH_PREFIX_VERSION: i64 = 24;

/// The SQL both queries run. Partitioned cookies (a non-empty `top_frame_site_key`) belong to one embedding site, so
/// they are not imported into the main store. The blob goes through hex() because sqlite3's JSON cannot carry bytes.
pub(crate) const ROWS_SQL: &str = "SELECT host_key, name, value, hex(encrypted_value) AS encrypted_hex, path, \
     expires_utc, has_expires, is_secure, is_httponly, samesite FROM cookies WHERE top_frame_site_key = ''";
pub(crate) const META_SQL: &str = "SELECT value FROM meta WHERE key = 'version'";

pub(crate) struct Row {
    host_key: String,
    name: String,
    value: String,
    encrypted: Vec<u8>,
    path: String,
    expires_utc: i64,
    has_expires: bool,
    secure: bool,
    http_only: bool,
    samesite: i64,
}

fn text(row: &Value, key: &str) -> Result<String, String> {
    row.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("column {key} is missing or not text"))
}

fn int(row: &Value, key: &str) -> Result<i64, String> {
    row.get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("column {key} is missing or not a number"))
}

pub(crate) fn from_hex(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) || !hex.is_ascii() {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

/// `sqlite3 -json` output of [`ROWS_SQL`]; empty output means no rows.
pub(crate) fn rows_from_json(json: &str) -> Result<Vec<Row>, String> {
    if json.trim().is_empty() {
        return Ok(Vec::new());
    }
    let parsed: Value = serde_json::from_str(json).map_err(|e| format!("unexpected sqlite3 output: {e}"))?;
    let items = parsed.as_array().ok_or("unexpected sqlite3 output")?;
    items
        .iter()
        .map(|row| {
            Ok(Row {
                host_key: text(row, "host_key")?,
                name: text(row, "name")?,
                value: text(row, "value")?,
                encrypted: from_hex(&text(row, "encrypted_hex")?).ok_or("encrypted_value is not hex")?,
                path: text(row, "path")?,
                expires_utc: int(row, "expires_utc")?,
                has_expires: int(row, "has_expires")? != 0,
                secure: int(row, "is_secure")? != 0,
                http_only: int(row, "is_httponly")? != 0,
                samesite: int(row, "samesite")?,
            })
        })
        .collect()
}

/// `sqlite3 -json` output of [`META_SQL`]. A missing row reads as version 0: a store too old to carry the hash.
pub(crate) fn meta_version_from_json(json: &str) -> i64 {
    serde_json::from_str::<Value>(json)
        .ok()
        .and_then(|v| v.get(0)?.get("value").cloned())
        .and_then(|v| match v {
            Value::String(s) => s.trim().parse().ok(),
            other => other.as_i64(),
        })
        .unwrap_or(0)
}

/// `None` for a session cookie (`has_expires` 0, or the 0 Chromium writes).
pub(crate) fn expires_at(expires_utc: i64, has_expires: bool) -> Option<f64> {
    if !has_expires || expires_utc == 0 {
        return None;
    }
    Some((expires_utc - WINDOWS_TO_UNIX_MICROS) as f64 / 1_000_000.0)
}

/// `samesite` column (sqlite_persistent_cookie_store.cc: DBCookieSameSite): -1 unspecified, 0 none, 1 lax, 2 strict;
/// the deprecated 3 ("extended") is read as unspecified there too.
pub(crate) fn same_site(value: i64) -> Option<SameSite> {
    match value {
        0 => Some(SameSite::None),
        1 => Some(SameSite::Lax),
        2 => Some(SameSite::Strict),
        _ => None,
    }
}

enum Decoded {
    Value(String),
    Undecryptable,
}

fn decode_value(row: &Row, key: Option<&Key>, meta_version: i64) -> Decoded {
    if row.encrypted.is_empty() {
        return Decoded::Value(row.value.clone());
    }
    // Without the v10 prefix the bytes are an old, unencrypted value (what Chromium's os_crypt_mac does too).
    let Some(sealed) = row.encrypted.strip_prefix(b"v10") else {
        return String::from_utf8(row.encrypted.clone()).map_or(Decoded::Undecryptable, Decoded::Value);
    };
    let Some(plain) = key.and_then(|key| crypto::decrypt(key, sealed)) else {
        return Decoded::Undecryptable;
    };
    let plain: &[u8] = &plain;
    let value = if meta_version >= HASH_PREFIX_VERSION {
        // Chromium drops a cookie whose hash does not match its host_key, and so do we: the key was wrong.
        match plain.strip_prefix(Sha256::digest(row.host_key.as_bytes()).as_slice()) {
            Some(rest) => rest,
            None => return Decoded::Undecryptable,
        }
    } else {
        plain
    };
    std::str::from_utf8(value).map_or(Decoded::Undecryptable, |v| Decoded::Value(v.to_owned()))
}

/// Turns the rows into cookies. `key` is asked for once, and only when a row needs it, so a store with no encrypted
/// value never reaches the Keychain. Its error ends the whole read.
pub(crate) fn cookies_from_rows(
    rows: Vec<Row>,
    meta_version: i64,
    key: &mut dyn FnMut() -> Result<Key, ReadError>,
    now: f64,
) -> Result<Read, ReadError> {
    let mut key_cache: Option<Key> = None;
    let mut out = Read::default();
    for row in rows {
        let expires = expires_at(row.expires_utc, row.has_expires);
        if expires.is_some_and(|at| at <= now) {
            continue;
        }
        if key_cache.is_none() && row.encrypted.starts_with(b"v10") {
            key_cache = Some(key()?);
        }
        match decode_value(&row, key_cache.as_ref(), meta_version) {
            Decoded::Value(value) => out.cookies.push(Cookie {
                domain: row.host_key,
                name: row.name,
                value,
                path: row.path,
                expires,
                secure: row.secure,
                http_only: row.http_only,
                same_site: same_site(row.samesite),
            }),
            Decoded::Undecryptable => out.skipped += 1,
        }
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::crypto::derive_key;

    /// The value a v24 store holds for `host_key`: v10, then AES-CBC of sha256(host_key) + value.
    pub(crate) fn sealed_v24(key: &Key, host_key: &str, value: &str) -> Vec<u8> {
        let mut plain = Sha256::digest(host_key.as_bytes()).to_vec();
        plain.extend_from_slice(value.as_bytes());
        let mut out = b"v10".to_vec();
        out.extend(crypto_encrypt(key, &plain));
        out
    }

    pub(crate) fn crypto_encrypt(key: &Key, plain: &[u8]) -> Vec<u8> {
        use aes::Aes128;
        use cbc::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
        let mut buf = plain.to_vec();
        buf.resize(plain.len() + 16, 0);
        cbc::Encryptor::<Aes128>::new(&(**key).into(), &[b' '; 16].into())
            .encrypt_padded_mut::<Pkcs7>(&mut buf, plain.len())
            .unwrap()
            .to_vec()
    }

    pub(crate) fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn json_row(host: &str, name: &str, value: &str, enc: &[u8], expires_utc: i64, samesite: i64) -> String {
        format!(
            r#"{{"host_key":"{host}","name":"{name}","value":"{value}","encrypted_hex":"{}","path":"/","expires_utc":{expires_utc},"has_expires":{},"is_secure":1,"is_httponly":0,"samesite":{samesite}}}"#,
            hex(enc),
            i32::from(expires_utc != 0),
        )
    }

    const NOW: f64 = 1_800_000_000.0;
    // 2030-01-01 and 2001-01-01 in Chromium's clock.
    const FUTURE: i64 = 1_893_456_000_000_000 + WINDOWS_TO_UNIX_MICROS;
    const PAST: i64 = 978_307_200_000_000 + WINDOWS_TO_UNIX_MICROS;

    fn no_key() -> Result<Key, ReadError> {
        panic!("the Keychain must not be asked for")
    }

    #[test]
    fn time_conversion_and_session_cookies() {
        assert_eq!(expires_at(WINDOWS_TO_UNIX_MICROS, true), Some(0.0));
        assert_eq!(expires_at(FUTURE, true), Some(1_893_456_000.0));
        assert_eq!(expires_at(0, true), None);
        assert_eq!(expires_at(FUTURE, false), None);
    }

    #[test]
    fn same_site_values() {
        assert_eq!(same_site(-1), None);
        assert_eq!(same_site(0), Some(SameSite::None));
        assert_eq!(same_site(1), Some(SameSite::Lax));
        assert_eq!(same_site(2), Some(SameSite::Strict));
        assert_eq!(same_site(3), None);
    }

    #[test]
    fn meta_version_parses_text_or_number() {
        assert_eq!(meta_version_from_json(r#"[{"value":"24"}]"#), 24);
        assert_eq!(meta_version_from_json(r#"[{"value":24}]"#), 24);
        assert_eq!(meta_version_from_json(""), 0);
    }

    #[test]
    fn plaintext_rows_never_ask_for_the_key() {
        let json = format!("[{}]", json_row(".example.com", "a", "plain", b"", FUTURE, 1));
        let rows = rows_from_json(&json).unwrap();
        let read = cookies_from_rows(rows, 24, &mut no_key, NOW).unwrap();
        assert_eq!(read.skipped, 0);
        assert_eq!(
            read.cookies,
            vec![Cookie {
                domain: ".example.com".into(),
                name: "a".into(),
                value: "plain".into(),
                path: "/".into(),
                expires: Some(1_893_456_000.0),
                secure: true,
                http_only: false,
                same_site: Some(SameSite::Lax),
            }]
        );
    }

    #[test]
    fn v24_values_are_decrypted_and_their_hash_stripped() {
        let key = derive_key(b"a known password");
        let enc = sealed_v24(&key, ".example.com", "secret");
        let json = format!("[{}]", json_row(".example.com", "sid", "", &enc, 0, -1));
        let mut asked = 0;
        let read = cookies_from_rows(
            rows_from_json(&json).unwrap(),
            24,
            &mut || {
                asked += 1;
                Ok(derive_key(b"a known password"))
            },
            NOW,
        )
        .unwrap();
        assert_eq!(asked, 1);
        assert_eq!(read.cookies[0].value, "secret");
        assert_eq!(read.cookies[0].expires, None);
        assert_eq!(read.cookies[0].same_site, None);
    }

    #[test]
    fn older_stores_have_no_hash_prefix() {
        let key = derive_key(b"pw");
        let enc = [b"v10".to_vec(), crypto_encrypt(&key, b"old")].concat();
        let json = format!("[{}]", json_row("example.com", "sid", "", &enc, FUTURE, 0));
        let read = cookies_from_rows(rows_from_json(&json).unwrap(), 23, &mut || Ok(derive_key(b"pw")), NOW).unwrap();
        assert_eq!(read.cookies[0].value, "old");
    }

    #[test]
    fn wrong_key_or_wrong_host_hash_is_skipped_and_counted() {
        let key = derive_key(b"right");
        let good = sealed_v24(&key, "a.com", "v");
        let other_host = sealed_v24(&key, "b.com", "v");
        let json = format!(
            "[{},{}]",
            json_row("a.com", "x", "", &good, FUTURE, 0),
            json_row("a.com", "y", "", &other_host, FUTURE, 0)
        );
        let read = cookies_from_rows(rows_from_json(&json).unwrap(), 24, &mut || Ok(derive_key(b"right")), NOW).unwrap();
        assert_eq!((read.cookies.len(), read.skipped), (1, 1));
        let read = cookies_from_rows(rows_from_json(&json).unwrap(), 24, &mut || Ok(derive_key(b"wrong")), NOW).unwrap();
        assert_eq!((read.cookies.len(), read.skipped), (0, 2));
    }

    #[test]
    fn expired_cookies_are_dropped_without_asking_for_the_key() {
        let key = derive_key(b"pw");
        let enc = sealed_v24(&key, "a.com", "v");
        let json = format!("[{}]", json_row("a.com", "x", "", &enc, PAST, 0));
        let read = cookies_from_rows(rows_from_json(&json).unwrap(), 24, &mut no_key, NOW).unwrap();
        assert_eq!((read.cookies.len(), read.skipped), (0, 0));
    }

    #[test]
    fn a_keychain_error_ends_the_read() {
        let key = derive_key(b"pw");
        let json = format!("[{}]", json_row("a.com", "x", "", &sealed_v24(&key, "a.com", "v"), FUTURE, 0));
        let err = cookies_from_rows(
            rows_from_json(&json).unwrap(),
            24,
            &mut || Err(ReadError::KeychainDenied),
            NOW,
        )
        .unwrap_err();
        assert_eq!(err, ReadError::KeychainDenied);
    }

    #[test]
    fn bad_json_is_an_error_not_a_panic() {
        assert!(rows_from_json("not json").is_err());
        assert!(rows_from_json(r#"[{"host_key":"a"}]"#).is_err());
        assert_eq!(rows_from_json("").unwrap().len(), 0);
    }
}
