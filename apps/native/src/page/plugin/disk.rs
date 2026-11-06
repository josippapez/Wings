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
                .ok_or("diskFind needs a path")?
                .to_owned();
            let find_names = names(&params, "names", 1, 16)?;
            let skip = names(&params, "skip", 0, 32)?;
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
                let found = wings_disk_usage::find(&path, &find_names, &skip, &flag, |progress| {
                    if stream && let Ok(line) = serde_json::to_string(progress) {
                        lines(&line);
                    }
                });
                found.and_then(|found| serde_json::to_string(&found).map_err(|e| e.to_string()))
            });
            Ok(())
        })();
        if let Err(error) = result {
            reply.err(error);
        }
    }
}
