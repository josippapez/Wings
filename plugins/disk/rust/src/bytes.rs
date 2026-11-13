//! Sizes, counts, times and the sentences the plugin writes with them.

use wings_plugin::format::grouped;

/// `x` to one decimal as JavaScript's `toFixed(1)` writes it: a tie that a binary number can hold, like 12.25, goes up.
fn fixed1(x: f64) -> String {
    let quarters = x * 4.0;
    if quarters.fract() == 0.0 && quarters as u64 % 2 == 1 { format!("{:.1}", x + 0.01) } else { format!("{x:.1}") }
}

/// Bytes the way Finder writes them, in powers of 1000: `980 KB`, `12.3 GB`, `1 byte`.
pub fn bytes(n: u64) -> String {
    if n < 1000 {
        return if n == 1 { "1 byte".into() } else { format!("{n} bytes") };
    }
    let units = ["KB", "MB", "GB", "TB", "PB"];
    let (mut value, mut unit) = (n as f64 / 1000.0, 0);
    while value >= 999.95 && unit < units.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    let number = if value >= 100.0 { format!("{}", value.round()) } else { fixed1(value).trim_end_matches(".0").to_owned() };
    format!("{number} {}", units[unit])
}

/// `n` and its noun with the thousands told apart: "1 item", "90,000 items".
pub fn count(n: u64, one: &str) -> String {
    format!("{} {one}{}", grouped(n), if n == 1 { "" } else { "s" })
}

/// `ms` as `m:ss`, or `h:mm:ss` from an hour on.
pub fn duration(ms: f64) -> String {
    let total = (ms / 1000.0).floor().max(0.0) as u64;
    let (m, s) = (total / 60, total % 60);
    if m < 60 { format!("{m}:{s:02}") } else { format!("{}:{:02}:{s:02}", m / 60, m % 60) }
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// A Time Machine snapshot's date, `2026-10-22-101010`, as `Oct 22, 2026, 10:10`. What isn't one stays as it is.
pub fn snapshot_label(date: &str) -> String {
    if !crate::disks::is_snapshot_date(date) {
        return date.to_owned();
    }
    let number = |from: usize, to: usize| date[from..to].parse::<usize>().unwrap_or(0);
    let month = MONTHS.get(number(5, 7).wrapping_sub(1)).copied().unwrap_or("");
    format!("{month} {}, {}, {}:{}", number(8, 10), number(0, 4), &date[11..13], &date[13..15])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_are_written_the_way_finder_writes_them() {
        let said: Vec<String> = [0, 1, 999, 1000, 12_250, 980_000, 999_950, 12_300_000_000, 150_000_000, 1_150, 2_000_000_000_000_000_000].into_iter().map(bytes).collect();
        assert_eq!(said, ["0 bytes", "1 byte", "999 bytes", "1 KB", "12.3 KB", "980 KB", "1 MB", "12.3 GB", "150 MB", "1.1 KB", "2000 PB"]);
    }

    #[test]
    fn counts_and_clocks_read_as_the_tab_writes_them() {
        assert_eq!([count(1, "item"), count(90_000, "item"), count(0, "smaller item")], ["1 item", "90,000 items", "0 smaller items"]);
        assert_eq!([duration(0.0), duration(65_000.0), duration(3_605_000.0), duration(-5.0)], ["0:00", "1:05", "1:00:05", "0:00"]);
        assert_eq!(snapshot_label("2026-10-22-101010"), "Oct 22, 2026, 10:10");
        assert_eq!(snapshot_label("com.apple.TimeMachine"), "com.apple.TimeMachine");
    }
}
