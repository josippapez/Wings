//! A script of the app's own that runs before an extension's background service worker, for what the browser engine
//! lacks that the worker uses. It goes into the extension's folder before the folder is hashed and allowed, so the hash
//! and the person's yes cover it, and the extension's own files stay as they were: only its manifest names another
//! worker, which loads the script and then the extension's own.

use std::{fs, path::Path};

use serde_json::Value;

use crate::{limits::safe_relative, Error};

/// The worker the manifest names instead of the extension's own, beside it, and the script that worker runs first.
pub const SHIM_WORKER: &str = "__wings_background.js";
pub const SHIM_SCRIPT: &str = "__wings_compat.js";

/// Puts `source` in front of the background service worker of the extension in `dir`, and says whether it did. Only a
/// classic worker gets it (a module worker can't `importScripts`), and only one whose folder has no file by those names.
pub fn shim_service_worker(dir: &Path, source: &str) -> Result<bool, Error> {
    let file = dir.join("manifest.json");
    let raw = fs::read(&file)?;
    let mut manifest: Value =
        serde_json::from_str(String::from_utf8_lossy(&raw).trim_start_matches('\u{feff}')).map_err(|e| Error::Manifest(e.to_string()))?;
    let Some(background) = manifest.get_mut("background").and_then(Value::as_object_mut) else { return Ok(false) };
    if background.get("type").and_then(Value::as_str) == Some("module") {
        return Ok(false);
    }
    let Some(worker) = background.get("service_worker").and_then(Value::as_str).and_then(safe_relative) else { return Ok(false) };
    let Some(name) = worker.file_name().and_then(|name| name.to_str()) else { return Ok(false) };
    // Beside the extension's worker, so what it finds by its own address (its chunks) is where it was.
    let folder = worker.parent().unwrap_or(Path::new(""));
    let beside = dir.join(folder);
    if !dir.join(&worker).is_file() || beside.join(SHIM_WORKER).exists() || beside.join(SHIM_SCRIPT).exists() {
        return Ok(false);
    }
    let load = serde_json::to_string(&url_segment(name)).map_err(|e| Error::Manifest(e.to_string()))?;
    let named = folder.join(SHIM_WORKER).to_string_lossy().into_owned();
    background.insert("service_worker".into(), Value::String(named));
    fs::write(beside.join(SHIM_SCRIPT), source)?;
    fs::write(beside.join(SHIM_WORKER), format!("importScripts(\"{SHIM_SCRIPT}\", {load});\n"))?;
    fs::write(&file, serde_json::to_vec_pretty(&manifest).map_err(|e| Error::Manifest(e.to_string()))?)?;
    Ok(true)
}

/// `name` as one segment of a URL path, so `importScripts` loads that file whatever characters it has (`#`, `?`, `%`).
fn url_segment(name: &str) -> String {
    name.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (byte as char).to_string(),
            _ => format!("%{byte:02X}"),
        })
        .collect()
}
