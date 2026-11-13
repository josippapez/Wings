//! Why plugins stopped, kept in a file: the installed app's stderr goes nowhere, so a main or a page that stopped left
//! nothing to read afterwards. One line each: the time (UTC), the plugin, the frame, and what happened.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

/// A log this long is moved aside (`plugins.log.old`, replacing the last one) before the next line.
const MOST_BYTES: u64 = 1024 * 1024;
/// A line is cut to this many characters: an error can carry a whole stack.
const MOST_CHARS: usize = 2000;

static FILE: OnceLock<Mutex<PathBuf>> = OnceLock::new();

/// Keeps the log in `file` from now on. Said once, when the app starts; without it nothing is kept.
pub fn keep_in(file: PathBuf) {
    let _ = FILE.set(Mutex::new(file));
}

/// Adds a line for `plugin`'s `frame` ("main", or a page's name), and prints it on stderr as before.
pub fn line(plugin: &str, frame: &str, message: &str) {
    eprintln!("[plugins] {plugin} {frame}: {message}");
    if let Some(file) = FILE.get() {
        // One writer at a time, so two lines never interleave and the move aside happens once.
        let file = file.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let _ = append(&file, plugin, frame, message, SystemTime::now());
    }
}

fn append(file: &Path, plugin: &str, frame: &str, message: &str, now: SystemTime) -> std::io::Result<()> {
    if fs::metadata(file).is_ok_and(|kept| kept.len() >= MOST_BYTES) {
        fs::rename(file, file.with_extension("log.old"))?;
    }
    let message: String = message.chars().take(MOST_CHARS).map(|c| if c == '\n' { ' ' } else { c }).collect();
    let mut log = fs::OpenOptions::new().create(true).append(true).open(file)?;
    writeln!(log, "{} {plugin} {frame}: {message}", stamp(now))
}

/// `now` as `2026-11-13T09:05:00Z`.
fn stamp(now: SystemTime) -> String {
    let secs = now.duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs());
    let (days, rest) = (secs / 86_400, secs % 86_400);
    // Days since 1970 to a date, by Howard Hinnant's `civil_from_days`.
    let shifted = days as i64 + 719_468;
    let era = shifted.div_euclid(146_097);
    let of_era = shifted.rem_euclid(146_097);
    let year_of_era = (of_era - of_era / 1_460 + of_era / 36_524 - of_era / 146_096) / 365;
    let day_of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rest / 3600, rest % 3600 / 60, rest % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_line_has_the_time_the_plugin_and_the_frame_and_a_full_log_is_moved_aside() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("plugins.log");
        // 2026-11-13 09:05:07 UTC, and a leap day.
        let at = UNIX_EPOCH + Duration::from_secs(1_794_560_707);
        append(&file, "worktrees", "sidebar.wasm", "the page ran for too long\nand was stopped", at).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "2026-11-13T09:05:07Z worktrees sidebar.wasm: the page ran for too long and was stopped\n");
        assert_eq!(stamp(UNIX_EPOCH + Duration::from_secs(1_709_164_800)), "2024-02-29T00:00:00Z");

        fs::write(&file, vec![b'x'; MOST_BYTES as usize]).unwrap();
        append(&file, "hud", "main", "started again", at).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "2026-11-13T09:05:07Z hud main: started again\n");
        assert_eq!(fs::metadata(dir.path().join("plugins.log.old")).unwrap().len(), MOST_BYTES);
    }
}
