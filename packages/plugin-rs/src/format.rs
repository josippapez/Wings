//! How Wings writes numbers and times, as `wings-plugin-sdk/format` does.

use crate::host;

/// `950`, `12k`, `1.2M`.
pub fn tokens(n: f64) -> String {
    if n >= 1e6 {
        let millions = (n / 1e6 * 10.0).round() / 10.0;
        format!("{millions}M")
    } else if n >= 1e3 {
        format!("{}k", (n / 1e3).round())
    } else {
        n.to_string()
    }
}

/// The time of day of `ms` on this Mac, as `09:41`.
pub fn clock(ms: f64) -> String {
    let minutes = ((ms + f64::from(host::utc_offset(ms)) * 60_000.0) / 60_000.0).floor() as i64;
    format!("{:02}:{:02}", minutes.div_euclid(60).rem_euclid(24), minutes.rem_euclid(60))
}

/// The instant of an ISO 8601 time with `Z` or an offset, like `2026-11-11T10:00:00.000Z`, in ms since the epoch.
/// None for anything else.
pub fn parse_time(iso: &str) -> Option<f64> {
    let (date, time) = iso.split_once(['T', 't'])?;
    let mut ymd = date.split('-').map(str::parse::<i64>);
    let (y, m, d) = (ymd.next()?.ok()?, ymd.next()?.ok()?, ymd.next()?.ok()?);
    let zone = time.find(['Z', 'z', '+', '-'])?;
    let (clock, zone) = time.split_at(zone);
    let mut hms = clock.split(':');
    let (hh, mm) = (hms.next()?.parse::<f64>().ok()?, hms.next()?.parse::<f64>().ok()?);
    let ss = hms.next().map_or(Some(0.0), |seconds| seconds.parse::<f64>().ok())?;
    let ahead = match zone.split_once(':') {
        Some((hours, minutes)) => {
            let ahead = hours[1..].parse::<f64>().ok()? * 60.0 + minutes.parse::<f64>().ok()?;
            if hours.starts_with('-') { -ahead } else { ahead }
        }
        None => 0.0,
    };
    // Days from 1970 to a civil date (Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    Some((days as f64 * 86_400.0 + hh * 3600.0 + mm * 60.0 + ss - ahead * 60.0) * 1000.0)
}

/// `n` with its noun: "1 file", "2 files".
pub fn plural(n: usize, one: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {one}s") }
}

/// `text` cut to `max` characters, ending in "…" when it was longer.
pub fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() > max { format!("{}…", text.chars().take(max.saturating_sub(1)).collect::<String>()) } else { text.to_owned() }
}

/// `1234567` as `1,234,567`.
pub fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// The last part of a path, like `repo` for `/Users/me/repo/`. A path with no parts, like `/`, comes back as it is.
pub fn basename(path: &str) -> &str {
    path.split('/').rfind(|part| !part.is_empty()).unwrap_or(path)
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// The day of `ms` on this Mac, as `Nov 12, 2026`.
pub fn day(ms: f64) -> String {
    let days = ((ms + f64::from(host::utc_offset(ms)) * 60_000.0) / 86_400_000.0).floor() as i64;
    // A civil date from days since 1970 (Hinnant's algorithm).
    let z = days + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{} {d}, {y}", MONTHS[(m - 1) as usize])
}

/// How long ago `ms` was at `now`: "just now", "5 min ago", "3 h ago", "2 d ago", and its day once a month has passed.
pub fn ago(ms: f64, now: f64) -> String {
    let minutes = ((now - ms) / 60_000.0).round().max(0.0);
    if minutes < 1.0 {
        "just now".into()
    } else if minutes < 60.0 {
        format!("{minutes} min ago")
    } else if minutes < 1440.0 {
        format!("{} h ago", (minutes / 60.0).round())
    } else if minutes < 43_200.0 {
        format!("{} d ago", (minutes / 1440.0).round())
    } else {
        day(ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_paths_and_ages_read_as_the_js_sdk_writes_them() {
        assert_eq!([plural(1, "repo"), plural(0, "repo"), plural(12, "worktree")], ["1 repo", "0 repos", "12 worktrees"]);
        assert_eq!([grouped(0), grouped(999), grouped(1000), grouped(1_234_567)], ["0", "999", "1,000", "1,234,567"]);
        assert_eq!([basename("/Users/me/repo/"), basename("repo"), basename("/")], ["repo", "repo", "/"]);
        let now = 1_800_000_000_000.0;
        let minutes = |n: f64| ago(now - n * 60_000.0, now);
        assert_eq!([minutes(0.2), minutes(5.0), minutes(90.0), minutes(60.0 * 50.0)], ["just now", "5 min ago", "2 h ago", "2 d ago"]);
        // 2026-11-12T10:00:00Z, read in UTC as the tests' clock is.
        assert_eq!(day(1_794_477_600_000.0), "Nov 12, 2026");
        assert_eq!(ago(1_794_477_600_000.0 - 86_400_000.0 * 400.0, 1_794_477_600_000.0), "Oct 8, 2025");
        assert_eq!(day(951_782_400_000.0), "Feb 29, 2000");
    }
}

/// `ms` since the epoch as `new Date(ms).toISOString()` writes it: UTC, to the millisecond, like `2026-11-13T09:05:00.000Z`.
pub fn iso_of(ms: f64) -> String {
    let total = ms.floor() as i64;
    let (days, in_day) = (total.div_euclid(86_400_000), total.rem_euclid(86_400_000));
    // Days since the epoch to a calendar date: the days-to-civil algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let (h, m, s, milli) = (in_day / 3_600_000, in_day / 60_000 % 60, in_day / 1000 % 60, in_day % 1000);
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}.{milli:03}Z")
}

#[cfg(test)]
mod iso_tests {
    use super::iso_of;

    #[test]
    fn a_time_is_written_as_to_iso_string_writes_it() {
        assert_eq!(iso_of(0.0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_of(1_700_000_000_123.0), "2023-11-14T22:13:20.123Z");
        assert_eq!(iso_of(951_782_400_000.0), "2000-02-29T00:00:00.000Z");
    }
}
