//! What main and the Disk usage tab say to each other. Main runs every scan and keeps the Collector, so each tab and
//! Claude's tools see the same ones; a tab asks main for anything that touches the disk, and asks Wings itself for the
//! folder it shows. Only what describes a scan crosses over, never its tree.

use crate::{df::Hidden, entry::Kind};
use serde_json::{Value, json};

/// A tab tells main it needs the scans and the Collector, and main tells everyone what it holds on every change.
pub const WANT: &str = "disk-want";
pub const STATE: &str = "disk-state";
/// A tab asks main to do something: `{ id, op, ... }`. Main answers with `{ id, error, result }`.
pub const ASK: &str = "disk-ask";
pub const DONE: &str = "disk-done";
/// Main tells every tab to read the disks again: space was freed.
pub const REFRESH: &str = "disk-refresh";

/// How far a scan has got.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    pub entries: u64,
    pub bytes: u64,
    pub path: String,
}

impl Progress {
    pub fn from_value(value: &Value) -> Self {
        Self { entries: value["entries"].as_u64().unwrap_or(0), bytes: value["bytes"].as_u64().unwrap_or(0), path: value["path"].as_str().unwrap_or_default().to_owned() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Done {
    /// The scan's root as Wings named it: a full path.
    pub name: String,
    /// What the root holds now: what the scan found, less what moved to the Trash since.
    pub size: u64,
    /// What the scan found.
    pub found: u64,
    /// The least size the scan kept entries at, which a folder in it is looked at in more detail below.
    pub min_bytes: u64,
    pub restricted: u64,
    pub at: f64,
    /// The mount point when the scan is of a whole disk.
    pub disk: Option<String>,
    pub hidden: Option<Hidden>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Scanning {
        progress: Progress,
        /// The bytes it should find, when known: the disk's used space, or the folder's size from an earlier scan.
        expected: Option<u64>,
        started_at: f64,
    },
    Done(Done),
    Failed(String),
}

/// A scan by the root a tab or a tool asked for, like `~` or `/`.
#[derive(Clone, Debug, PartialEq)]
pub struct ScanInfo {
    pub root: String,
    pub phase: Phase,
    /// When the scan last changed state, for one that has no time of its own.
    pub stamp: f64,
}

impl ScanInfo {
    /// When the scan began, or finished.
    pub fn at(&self) -> f64 {
        match &self.phase {
            Phase::Scanning { started_at, .. } => *started_at,
            Phase::Done(done) => done.at,
            Phase::Failed(_) => self.stamp,
        }
    }

    pub fn done(&self) -> Option<&Done> {
        match &self.phase {
            Phase::Done(done) => Some(done),
            _ => None,
        }
    }

    pub fn to_value(&self) -> Value {
        let mut out = json!({ "root": self.root, "stamp": self.stamp });
        match &self.phase {
            Phase::Scanning { progress, expected, started_at } => {
                out["state"] = "scanning".into();
                out["progress"] = json!({ "entries": progress.entries, "bytes": progress.bytes, "path": progress.path });
                out["expected"] = json!(expected);
                out["startedAt"] = (*started_at).into();
            }
            Phase::Done(done) => {
                out["state"] = "done".into();
                out["name"] = done.name.clone().into();
                out["size"] = done.size.into();
                out["found"] = done.found.into();
                out["minBytes"] = done.min_bytes.into();
                out["restricted"] = done.restricted.into();
                out["at"] = done.at.into();
                out["disk"] = json!(done.disk);
                out["hidden"] = done.hidden.as_ref().map_or(Value::Null, Hidden::to_value);
            }
            Phase::Failed(message) => {
                out["state"] = "error".into();
                out["message"] = message.clone().into();
            }
        }
        out
    }

    pub fn from_value(value: &Value) -> Option<Self> {
        let phase = match value["state"].as_str()? {
            "scanning" => Phase::Scanning { progress: Progress::from_value(&value["progress"]), expected: value["expected"].as_u64(), started_at: value["startedAt"].as_f64().unwrap_or(0.0) },
            "done" => Phase::Done(Done {
                name: value["name"].as_str()?.to_owned(),
                size: value["size"].as_u64()?,
                found: value["found"].as_u64().unwrap_or(0),
                min_bytes: value["minBytes"].as_u64().unwrap_or(0),
                restricted: value["restricted"].as_u64().unwrap_or(0),
                at: value["at"].as_f64().unwrap_or(0.0),
                disk: value["disk"].as_str().map(str::to_owned),
                hidden: value["hidden"].is_object().then(|| Hidden::from_value(&value["hidden"])),
            }),
            "error" => Phase::Failed(value["message"].as_str().unwrap_or_default().to_owned()),
            _ => return None,
        };
        Some(Self { root: value["root"].as_str()?.to_owned(), phase, stamp: value["stamp"].as_f64().unwrap_or(0.0) })
    }
}

/// A file or folder gathered in the Collector.
#[derive(Clone, Debug, PartialEq)]
pub struct Collected {
    pub path: String,
    pub size: u64,
    pub kind: Kind,
}

impl Collected {
    pub fn to_value(&self) -> Value {
        json!({ "path": self.path, "size": self.size, "kind": self.kind.name() })
    }

    /// An item as stored or told, or none for what isn't one.
    pub fn from_value(value: &Value) -> Option<Self> {
        Some(Self { path: value["path"].as_str()?.to_owned(), size: value["size"].as_f64().filter(|size| size.is_finite() && *size >= 0.0)? as u64, kind: Kind::parse(value["kind"].as_str()?)? })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Collector {
    pub items: Vec<Collected>,
}

impl Collector {
    pub fn total(&self) -> u64 {
        self.items.iter().map(|item| item.size).sum()
    }

    pub fn to_value(&self) -> Value {
        json!({ "items": self.items.iter().map(Collected::to_value).collect::<Vec<_>>(), "total": self.total() })
    }

    pub fn from_value(value: &Value) -> Self {
        Self { items: value["items"].as_array().map_or_else(Vec::new, |list| list.iter().filter_map(Collected::from_value).collect()) }
    }
}

/// What came of moving the Collector to the Trash.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrashResult {
    pub trashed: Vec<String>,
    pub failed: Vec<(String, String)>,
    /// The bytes of what moved.
    pub freed: u64,
}

impl TrashResult {
    pub fn to_value(&self) -> Value {
        json!({ "trashed": self.trashed, "failed": self.failed.iter().map(|(path, message)| json!({ "path": path, "message": message })).collect::<Vec<_>>(), "freed": self.freed })
    }

    pub fn from_value(value: &Value) -> Self {
        Self {
            trashed: value["trashed"].as_array().map_or_else(Vec::new, |list| list.iter().filter_map(|path| path.as_str().map(str::to_owned)).collect()),
            failed: value["failed"].as_array().map_or_else(Vec::new, |list| list.iter().map(|f| (f["path"].as_str().unwrap_or_default().to_owned(), f["message"].as_str().unwrap_or_default().to_owned())).collect()),
            freed: value["freed"].as_u64().unwrap_or(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scan_crosses_from_main_to_a_tab_in_each_of_its_states() {
        let done = Done {
            name: "/Users/ada".into(),
            size: 10,
            found: 12,
            min_bytes: 1 << 20,
            restricted: 2,
            at: 1000.0,
            disk: Some("/".into()),
            hidden: Some(Hidden { size: 5, purgeable: 2, others: vec![], snapshots: vec!["2026-10-22-101010".into()], rest: 3 }),
        };
        for phase in [
            Phase::Scanning { progress: Progress { entries: 5, bytes: 6, path: "/x".into() }, expected: Some(9), started_at: 3.0 },
            Phase::Scanning { progress: Progress::default(), expected: None, started_at: 3.0 },
            Phase::Done(done.clone()),
            Phase::Done(Done { disk: None, hidden: None, ..done }),
            Phase::Failed("No access".into()),
        ] {
            let scan = ScanInfo { root: "~".into(), phase, stamp: 7.0 };
            assert_eq!(ScanInfo::from_value(&scan.to_value()), Some(scan));
        }
    }

    #[test]
    fn the_collector_keeps_the_items_that_are_one_and_drops_the_rest() {
        let stored = json!({ "items": [{ "path": "/a", "size": 5, "kind": "dir" }, { "path": "/b", "size": "x", "kind": "file" }, { "path": "/c", "size": 1, "kind": "socket" }, 4, { "path": "/d", "size": 2, "kind": "file" }] });
        let collector = Collector::from_value(&stored);
        assert_eq!(collector.items.iter().map(|i| i.path.as_str()).collect::<Vec<_>>(), ["/a", "/d"]);
        assert_eq!(collector.total(), 7);
        assert_eq!(Collector::from_value(&collector.to_value()), collector);
    }
}
