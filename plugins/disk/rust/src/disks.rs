//! The disks: what Wings lists for each one, `df`'s numbers on top, and the Time Machine snapshots of a disk.

use crate::df::{DfRow, DiskRow, Disk, Hidden, available_of, disk_rows, hidden_space, parse_df};
use std::cell::RefCell;
use wings_plugin::host;

/// A local Time Machine snapshot's date, as `tmutil` writes it: `2026-10-22-101010`.
pub fn is_snapshot_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 17 && bytes.iter().enumerate().all(|(at, byte)| if matches!(at, 4 | 7 | 10) { *byte == b'-' } else { byte.is_ascii_digit() })
}

/// The disks as read now, and the rows of `df` they were worked out from.
#[derive(Clone)]
pub struct Disks {
    pub at: f64,
    pub rows: Vec<DiskRow>,
    pub df: Vec<DfRow>,
}

thread_local! {
    static LAST: RefCell<Option<Disks>> = const { RefCell::new(None) };
}

async fn run_df() -> Vec<DfRow> {
    // Without df the disks still show, with no purgeable space and no other volumes.
    match host::exec("df", &["-k", "-P"]).await {
        Ok(out) => parse_df(out["stdout"].as_str().unwrap_or_default()),
        Err(_) => Vec::new(),
    }
}

/// Reads the disks now.
pub async fn load_disks() -> Result<Disks, String> {
    let (disks, df) = wings_plugin::task::join(host::disks(), run_df()).await;
    let list: Vec<Disk> = disks?.as_array().map_or_else(Vec::new, |list| list.iter().map(Disk::from_value).collect());
    let loaded = Disks { at: host::now(), rows: disk_rows(&list, &df), df };
    LAST.with(|last| *last.borrow_mut() = Some(loaded.clone()));
    Ok(loaded)
}

/// The disks as read in the last `max_age_ms`, reading them if that is older.
pub async fn disks_now(max_age_ms: f64) -> Result<Disks, String> {
    if let Some(last) = LAST.with(|last| last.borrow().clone()).filter(|last| host::now() - last.at <= max_age_ms) {
        return Ok(last);
    }
    load_disks().await
}

/// The dates of the local snapshots on the disk mounted at `mount`: none when `tmutil` can't list them.
async fn snapshots_of(mount: &str) -> Vec<String> {
    match host::exec("tmutil", &["listlocalsnapshotdates", mount]).await {
        Ok(out) if out["code"] == 0 => out["stdout"].as_str().unwrap_or_default().lines().map(str::trim).filter(|line| is_snapshot_date(line)).map(str::to_owned).collect(),
        _ => Vec::new(),
    }
}

/// The hidden space of the disk at `mount` for a scan that found `found` bytes, from numbers read now. None when it isn't a disk.
pub async fn hidden_of(mount: &str, found: u64) -> Result<Option<Hidden>, String> {
    let Disks { rows, df, .. } = load_disks().await?;
    let Some(disk) = rows.iter().find(|d| d.disk.mount == mount) else { return Ok(None) };
    Ok(Some(hidden_space(disk, available_of(&df, mount), found, snapshots_of(mount).await)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snapshot_date_is_what_tmutil_writes() {
        assert!(is_snapshot_date("2026-10-22-101010"));
        for text in ["", "2026-10-22-10101", "2026-10-22 101010", "2026-10-22-10101x", "com.apple.TimeMachine.2026-10-22-101010"] {
            assert!(!is_snapshot_date(text), "{text}");
        }
    }
}
