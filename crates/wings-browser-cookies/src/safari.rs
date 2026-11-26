//! Safari's `Cookies.binarycookies`: a big-endian file header, then pages of little-endian cookie records
//! (libyal/dtformats "Safari Cookies" and yt-dlp's parser).

use crate::Cookie;

/// Seconds from the Unix epoch to 2001-01-01, the zero of Mac absolute time.
const MAC_TO_UNIX: f64 = 978_307_200.0;

const FLAG_SECURE: u32 = 0x1;
const FLAG_HTTP_ONLY: u32 = 0x4;

/// The bytes of the file ended early, or a field pointed outside its page.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Malformed;

fn u32_at(data: &[u8], at: usize, big_endian: bool) -> Result<u32, Malformed> {
    let bytes: [u8; 4] = data.get(at..at + 4).ok_or(Malformed)?.try_into().map_err(|_| Malformed)?;
    Ok(if big_endian { u32::from_be_bytes(bytes) } else { u32::from_le_bytes(bytes) })
}

fn f64_at(data: &[u8], at: usize) -> Result<f64, Malformed> {
    let bytes: [u8; 8] = data.get(at..at + 8).ok_or(Malformed)?.try_into().map_err(|_| Malformed)?;
    Ok(f64::from_le_bytes(bytes))
}

fn cstring_at(record: &[u8], at: usize) -> Option<String> {
    let rest = record.get(at..)?;
    let end = rest.iter().position(|&b| b == 0)?;
    String::from_utf8(rest[..end].to_vec()).ok()
}

/// A record's cookie, or `None` for one whose strings are cut off or not UTF-8 (counted as skipped).
fn record(record: &[u8]) -> Result<Option<(Cookie, f64)>, Malformed> {
    let flags = u32_at(record, 8, false)?;
    let (domain, name, path, value) = (
        u32_at(record, 16, false)? as usize,
        u32_at(record, 20, false)? as usize,
        u32_at(record, 24, false)? as usize,
        u32_at(record, 28, false)? as usize,
    );
    let expires = f64_at(record, 40)? + MAC_TO_UNIX;
    let (Some(domain), Some(name), Some(path), Some(value)) =
        (cstring_at(record, domain), cstring_at(record, name), cstring_at(record, path), cstring_at(record, value))
    else {
        return Ok(None);
    };
    let cookie = Cookie {
        domain,
        name,
        value,
        path,
        expires: Some(expires),
        secure: flags & FLAG_SECURE != 0,
        http_only: flags & FLAG_HTTP_ONLY != 0,
        // Safari's format has no SameSite field.
        same_site: None,
    };
    Ok(Some((cookie, expires)))
}

fn page(page: &[u8], now: f64, cookies: &mut Vec<Cookie>, skipped: &mut usize) -> Result<(), Malformed> {
    if page.get(..4) != Some(&[0, 0, 1, 0]) {
        return Err(Malformed);
    }
    let count = u32_at(page, 4, false)? as usize;
    for i in 0..count {
        let offset = u32_at(page, 8 + i * 4, false)? as usize;
        let size = u32_at(page, offset, false)? as usize;
        let bytes = page.get(offset..offset.checked_add(size).ok_or(Malformed)?).ok_or(Malformed)?;
        match record(bytes)? {
            Some((cookie, expires)) if expires > now => cookies.push(cookie),
            Some(_) => {}
            None => *skipped += 1,
        }
    }
    Ok(())
}

/// Every unexpired cookie in the file, and how many records could not be decoded.
pub(crate) fn parse(data: &[u8], now: f64) -> Result<(Vec<Cookie>, usize), Malformed> {
    if data.get(..4) != Some(b"cook") {
        return Err(Malformed);
    }
    let pages = u32_at(data, 4, true)? as usize;
    let mut offset = 8 + pages.checked_mul(4).ok_or(Malformed)?;
    let (mut cookies, mut skipped) = (Vec::new(), 0);
    for i in 0..pages {
        let size = u32_at(data, 8 + i * 4, true)? as usize;
        let bytes = data.get(offset..offset.checked_add(size).ok_or(Malformed)?).ok_or(Malformed)?;
        page(bytes, now, &mut cookies, &mut skipped)?;
        offset += size;
    }
    Ok((cookies, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A record laid out like Safari's: 56-byte header, then the four strings.
    fn record_bytes(domain: &str, name: &str, path: &str, value: &str, flags: u32, expires_mac: f64) -> Vec<u8> {
        let strings = [domain, name, path, value];
        let mut offsets = [0u32; 4];
        let mut at = 56u32;
        for (i, s) in strings.iter().enumerate() {
            offsets[i] = at;
            at += s.len() as u32 + 1;
        }
        let mut out = Vec::new();
        out.extend(at.to_le_bytes());
        out.extend(0u32.to_le_bytes());
        out.extend(flags.to_le_bytes());
        out.extend(0u32.to_le_bytes());
        for o in offsets {
            out.extend(o.to_le_bytes());
        }
        out.extend(0u64.to_le_bytes());
        out.extend(expires_mac.to_le_bytes());
        out.extend(0f64.to_le_bytes());
        for s in strings {
            out.extend(s.as_bytes());
            out.push(0);
        }
        out
    }

    fn page_bytes(records: &[Vec<u8>]) -> Vec<u8> {
        let mut out = vec![0, 0, 1, 0];
        out.extend((records.len() as u32).to_le_bytes());
        let mut at = 8 + records.len() as u32 * 4;
        for r in records {
            out.extend(at.to_le_bytes());
            at += r.len() as u32;
        }
        for r in records {
            out.extend(r);
        }
        out.extend([0, 0, 0, 0]);
        out
    }

    fn file_bytes(pages: &[Vec<u8>]) -> Vec<u8> {
        let mut out = b"cook".to_vec();
        out.extend((pages.len() as u32).to_be_bytes());
        for p in pages {
            out.extend((p.len() as u32).to_be_bytes());
        }
        for p in pages {
            out.extend(p);
        }
        out.extend([0u8; 8]);
        out
    }

    // 2030-01-01 as Mac absolute time, and "now" in 2027.
    const FUTURE_MAC: f64 = 1_893_456_000.0 - MAC_TO_UNIX;
    const NOW: f64 = 1_800_000_000.0;

    #[test]
    fn parses_pages_records_flags_and_time() {
        let file = file_bytes(&[
            page_bytes(&[
                record_bytes(".example.com", "sid", "/", "abc", 0x5, FUTURE_MAC),
                record_bytes("old.com", "x", "/", "v", 0, 1.0),
            ]),
            page_bytes(&[record_bytes("b.com", "n", "/p", "v2", 0x1, FUTURE_MAC)]),
        ]);
        let (cookies, skipped) = parse(&file, NOW).unwrap();
        assert_eq!(skipped, 0);
        assert_eq!(cookies.len(), 2);
        assert_eq!(
            (cookies[0].domain.as_str(), cookies[0].name.as_str(), cookies[0].value.as_str()),
            (".example.com", "sid", "abc")
        );
        assert!(cookies[0].secure && cookies[0].http_only);
        assert_eq!(cookies[0].expires, Some(1_893_456_000.0));
        assert!(cookies[1].secure && !cookies[1].http_only);
        assert_eq!(cookies[1].path, "/p");
    }

    #[test]
    fn rejects_a_wrong_signature_and_truncation() {
        assert_eq!(parse(b"nope", NOW), Err(Malformed));
        assert_eq!(parse(b"", NOW), Err(Malformed));
        let file = file_bytes(&[page_bytes(&[record_bytes("a.com", "n", "/", "v", 0, FUTURE_MAC)])]);
        assert_eq!(parse(&file[..file.len() - 40], NOW), Err(Malformed));
    }

    #[test]
    fn a_record_with_invalid_utf8_is_skipped() {
        let mut r = record_bytes("a.com", "n", "/", "v", 0, FUTURE_MAC);
        let at = r.len() - 2;
        r[at] = 0xff;
        let file = file_bytes(&[page_bytes(&[r])]);
        let (cookies, skipped) = parse(&file, NOW).unwrap();
        assert_eq!((cookies.len(), skipped), (0, 1));
    }
}
