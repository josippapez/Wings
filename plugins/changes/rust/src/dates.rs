//! Dates as the History list and the graph show them. Every `at` is Unix seconds, as git prints it.

use wings_plugin::format::iso_of;

/// Like `2026-10-15`.
pub fn date_of(at: f64) -> String {
    date_time_of(at).split_once('T').map(|(date, _)| date.to_owned()).unwrap_or_default()
}

/// Like `2026-10-15T14:03:00.000Z`.
pub fn date_time_of(at: f64) -> String {
    iso_of(at.floor() * 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_commit_time_reads_as_its_utc_day_and_time() {
        assert_eq!(date_of(1_794_477_780.0), "2026-11-12");
        assert_eq!(date_time_of(1_794_477_780.0), "2026-11-12T10:03:00.000Z");
        assert_eq!(date_time_of(0.0), "1970-01-01T00:00:00.000Z");
        // A fraction of a second is dropped, as git's whole seconds have it.
        assert_eq!(date_time_of(1_794_477_780.9), "2026-11-12T10:03:00.000Z");
        assert_eq!(date_time_of(-1.0), "1969-12-31T23:59:59.000Z");
    }
}
