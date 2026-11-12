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
