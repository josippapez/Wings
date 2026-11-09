use super::{Access, Shared};
use crate::page::runtime::Reply;
use serde_json::Value;
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
};

struct Scan {
    host: Weak<Shared>,
    token: String,
}

impl Drop for Scan {
    fn drop(&mut self) {
        if let Some(host) = self.host.upgrade() {
            host.scans.lock().unwrap().remove(&self.token);
        }
    }
}

fn names(params: &Value, key: &str, min: usize, max: usize) -> Result<Vec<String>, String> {
    let empty = Vec::new();
    let list = match params.get(key) {
        None | Some(Value::Null) => &empty,
        Some(Value::Array(list)) => list,
        _ => {
            return Err(format!(
                "diskFind {key} must be a list of {min} to {max} names"
            ));
        }
    };
    if !(min..=max).contains(&list.len()) {
        return Err(format!(
            "diskFind {key} must be a list of {min} to {max} names"
        ));
    }
    list.iter()
        .map(|value| {
            value
                .as_str()
                .filter(|s| (1..=255).contains(&s.encode_utf16().count()) && !s.contains('/'))
                .map(str::to_owned)
                .ok_or_else(|| {
                    format!("diskFind {key} must be names of 1 to 255 characters with no /")
                })
        })
        .collect()
}

/// A number `diskUsage` was given, 0 or more and at most `max`, or `None` when it gave none: the desktop's `diskCount`.
/// A size is whole bytes, so a floor of 1.5 is 2.
fn count(params: &Value, name: &str, max: f64, whole: bool) -> Result<Option<f64>, String> {
    let bad = || {
        format!(
            "diskUsage {name} must be {}, 0 or more",
            if whole { "a whole number" } else { "a number" }
        )
    };
    match params.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => {
            let value = number.as_f64().filter(|v| *v >= 0.0).ok_or_else(bad)?;
            if whole && (value.fract() != 0.0 || value > MAX_SAFE_INTEGER) {
                return Err(bad());
            }
            Ok(Some(value.ceil().min(max)))
        }
        _ => Err(bad()),
    }
}

/// `Number.MAX_SAFE_INTEGER`.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// What a scan does once it has its token.
enum Work {
    Usage(wings_disk_usage::Options),
    Find {
        names: Vec<String>,
        skip: Vec<String>,
    },
}

impl Work {
    /// The call as the desktop's `diskUsageRequest` and `diskFindRequest` read it. Rust clamps the ranges.
    fn of(method: &str, params: &Value) -> Result<Self, String> {
        if method == "diskUsage" {
            let defaults = wings_disk_usage::Options::default();
            let min_bytes = count(params, "minBytes", MAX_SAFE_INTEGER, false)?;
            let depth = count(params, "depth", u32::MAX as f64, true)?;
            let max_entries = count(params, "maxEntries", u32::MAX as f64, true)?;
            let reuse = match params.get("reuse") {
                None | Some(Value::Null) => false,
                Some(Value::Bool(reuse)) => *reuse,
                _ => return Err("diskUsage reuse must be true or false".into()),
            };
            return Ok(Self::Usage(wings_disk_usage::Options {
                reuse,
                min_bytes: min_bytes.map_or(defaults.min_bytes, |v| v as u64),
                depth: depth.map_or(defaults.depth, |d| {
                    (d as u32).min(wings_disk_usage::MAX_DEPTH)
                }),
                max_entries: max_entries.map_or(defaults.max_entries, |n| {
                    (n as usize).clamp(1, wings_disk_usage::MAX_ENTRIES)
                }),
            }));
        }
        Ok(Self::Find {
            names: names(params, "names", 1, 16)?,
            skip: names(params, "skip", 0, 32)?,
        })
    }

    fn run(
        self,
        path: &str,
        cancel: &AtomicBool,
        progress: impl Fn(&wings_disk_usage::Progress) + Sync,
    ) -> Result<String, String> {
        match self {
            Self::Usage(options) => wings_disk_usage::scan(path, options, cancel, progress)
                .and_then(|scan| serde_json::to_string(&scan).map_err(|e| e.to_string())),
            Self::Find { names, skip } => {
                wings_disk_usage::find(path, &names, &skip, cancel, progress)
                    .and_then(|found| serde_json::to_string(&found).map_err(|e| e.to_string()))
            }
        }
    }
}

impl Shared {
    pub(super) fn disk_call(
        self: &Arc<Self>,
        method: &str,
        params: Value,
        reply: Reply,
        access: Access,
    ) {
        let result = (|| {
            if !wings_plugins::permissions::may_scan_disks(&access.plugin) {
                return Err(format!(
                    "{} may not read disk usage",
                    access.plugin.manifest.id
                ));
            }
            if method == "disks" {
                self.off_thread(reply.clone(), |_| {
                    serde_json::to_string(&wings_disk_usage::disks()).map_err(|e| e.to_string())
                });
                return Ok(());
            }
            let token = params
                .get("token")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or("A scan needs a token of 1 to 64 characters")?
                .to_owned();
            if method == "diskUsageCancel" {
                if let Some(flag) = self.scans.lock().unwrap().get(&token) {
                    flag.store(true, Ordering::Relaxed);
                }
                reply.clone().ok("null".into());
                return Ok(());
            }
            let path = params
                .get("path")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| format!("{method} needs a path"))?
                .to_owned();
            let work = Work::of(method, &params)?;
            let stream = params.get("stream") == Some(&Value::Bool(true));
            let flag = Arc::new(AtomicBool::new(false));
            self.scans
                .lock()
                .unwrap()
                .insert(token.clone(), flag.clone());
            let scan = Scan {
                host: Arc::downgrade(self),
                token,
            };
            self.off_thread(reply.clone(), move |reply| {
                let _scan = scan;
                let lines = reply.lines();
                work.run(&path, &flag, |progress| {
                    if stream && let Ok(line) = serde_json::to_string(progress) {
                        lines(&line);
                    }
                })
            });
            Ok(())
        })();
        if let Err(error) = result {
            reply.err(error);
        }
    }
}
