//! The local log: what the app prints and its panics, with a timestamp, in `~/Library/Logs/Wings/wings.log`. A
//! release app's stderr goes nowhere, so without this its `[plugin] ... took N ms` lines and its errors are lost. Nothing
//! leaves the machine. At `MAX_BYTES` the file moves to `wings.log.1` (replacing the last one), so the pair
//! never holds more than twice that. Dev builds don't open the file: they print to the terminal as before.

use std::{
    fmt::Arguments,
    fs::{self, File, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_BYTES: u64 = 2 * 1024 * 1024;
const NAME: &str = "wings.log";

struct Sink {
    dir: PathBuf,
    file: File,
    size: u64,
    max: u64,
}

// Only the app process opens it. The CLI subcommands never call `init`, so their lines stay on stderr.
static SINK: OnceLock<Mutex<Sink>> = OnceLock::new();

/// `~/Library/Logs/Wings` (Tauri's own log folder would be named after the bundle id).
fn log_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        dirs::home_dir().map(|h| h.join("Library/Logs/Wings"))
    } else {
        dirs::data_local_dir().map(|d| d.join("Wings/logs"))
    }
}

/// Starts writing to the log file, panics included. Does nothing when the folder can't be made.
pub fn init() {
    if let Some(dir) = log_dir() {
        start(dir);
    }
}

fn start(dir: PathBuf) {
    let Some(sink) = Sink::open(dir, MAX_BYTES) else { return };
    let _ = SINK.set(Mutex::new(sink));
    // The release app aborts on a panic, and the hook runs before that, so it's the one place the panic gets written down.
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        to_file(format_args!("[panic] thread '{}' {info}", thread.name().unwrap_or("<unnamed>")));
        default(info);
    }));
}

/// Prints one line to stderr and, once `init` ran, appends it to the file with a timestamp.
pub fn line(message: Arguments) {
    eprintln!("{message}");
    to_file(message);
}

fn to_file(message: Arguments) {
    if let Some(sink) = SINK.get() {
        let text = format!("{} {message}\n", timestamp(SystemTime::now()));
        if let Ok(mut sink) = sink.lock() {
            sink.write(text.as_bytes());
        }
    }
}

macro_rules! log_line {
    ($($arg:tt)*) => { $crate::applog::line(format_args!($($arg)*)) };
}
pub(crate) use log_line;

impl Sink {
    fn open(dir: PathBuf, max: u64) -> Option<Sink> {
        fs::create_dir_all(&dir).ok()?;
        let file = OpenOptions::new().create(true).append(true).open(dir.join(NAME)).ok()?;
        let size = file.metadata().ok()?.len();
        Some(Sink { dir, file, size, max })
    }

    /// One write per line, no flush to disk: a crash may lose the last lines, which is fine for a log.
    fn write(&mut self, text: &[u8]) {
        if self.size > 0 && self.size + text.len() as u64 > self.max {
            let current = self.dir.join(NAME);
            let _ = fs::rename(&current, self.dir.join(format!("{NAME}.1")));
            // If the new file can't be opened, keep appending to the old handle rather than dropping lines.
            if let Ok(file) = OpenOptions::new().create(true).append(true).open(&current) {
                self.file = file;
                self.size = 0;
            }
        }
        if self.file.write_all(text).is_ok() {
            self.size += text.len() as u64;
        }
    }
}

/// `2026-10-30T12:34:56.789Z`, in UTC.
fn timestamp(time: SystemTime) -> String {
    let since = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = since.as_secs() as i64;
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Days since 1970-01-01 to a calendar date (Howard Hinnant's civil_from_days).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60,
        since.subsec_millis()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn timestamp_is_utc_with_millis() {
        assert_eq!(timestamp(UNIX_EPOCH + Duration::from_millis(1_700_000_000_123)), "2023-11-14T22:13:20.123Z");
        assert_eq!(timestamp(UNIX_EPOCH + Duration::from_secs(951_782_400)), "2000-02-29T00:00:00.000Z");
    }

    #[test]
    fn full_file_moves_aside_and_only_one_previous_is_kept() {
        let dir = std::env::temp_dir().join(format!("wings-applog-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut sink = Sink::open(dir.clone(), 20).unwrap();
        for line in ["aaaaaaaaaaaa\n", "bbbbbbbbbbbb\n", "cccccccccccc\n"] {
            sink.write(line.as_bytes());
        }
        assert_eq!(fs::read_to_string(dir.join(NAME)).unwrap(), "cccccccccccc\n");
        assert_eq!(fs::read_to_string(dir.join("wings.log.1")).unwrap(), "bbbbbbbbbbbb\n");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_panic_is_written_to_the_file() {
        let dir = std::env::temp_dir().join(format!("wings-applog-panic-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        start(dir.clone());
        let _ = std::thread::Builder::new().name("boom".into()).spawn(|| panic!("kaboom")).unwrap().join();
        let log = fs::read_to_string(dir.join(NAME)).unwrap();
        assert!(log.contains("[panic] thread 'boom' panicked at") && log.contains("kaboom"), "{log}");
        let _ = fs::remove_dir_all(&dir);
    }
}
