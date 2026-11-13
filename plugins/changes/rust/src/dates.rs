//! Dates as the History list and the graph show them. Every `at` is Unix seconds, as git prints it.

/// The day and time of day of `at`, in UTC.
fn parts(at: f64) -> (i64, i64, i64, i64, i64, i64) {
    let seconds = at.floor() as i64;
    let (days, rest) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    // A civil date from days since 1970 (Hinnant's algorithm).
    let z = days + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    (yoe + era * 400 + i64::from(m <= 2), m, d, rest / 3600, rest % 3600 / 60, rest % 60)
}

/// Like `2026-10-15`.
pub fn date_of(at: f64) -> String {
    let (y, m, d, ..) = parts(at);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Like `2026-10-15T14:03:00.000Z`.
pub fn date_time_of(at: f64) -> String {
    let (y, m, d, hh, mm, ss) = parts(at);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}.000Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_commit_time_reads_as_its_utc_day_and_time() {
        assert_eq!(date_of(1_794_477_780.0), "2026-11-12");
        assert_eq!(date_time_of(1_794_477_780.0), "2026-11-12T10:03:00.000Z");
        assert_eq!(date_time_of(0.0), "1970-01-01T00:00:00.000Z");
    }
}
