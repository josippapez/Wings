use std::{
    collections::HashMap,
    fs,
    io::{Read, Seek},
    path::{Path, PathBuf},
};

use serde_json::Value;
use zip::ZipArchive;

use crate::{limits::safe_relative, Error};

/// What an extension is and what it asks for, as shown before it is added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    pub name: String,
    pub description: String,
    pub version: String,
    /// The best icon as a file inside the unpacked folder; `None` when the manifest names none that exists, and always
    /// `None` for an `.xpi`, whose files are inside a ZIP.
    pub icon: Option<PathBuf>,
    /// 0 when the manifest has none (WebKit reports that as an error).
    pub manifest_version: u32,
    /// API permissions such as `storage`, without host patterns.
    pub permissions: Vec<String>,
    /// Sites it reaches: `host_permissions`, MV2 host patterns found in `permissions`, and content scripts' `matches`.
    pub host_permissions: Vec<String>,
}

/// `Info` plus what only a Firefox package says.
pub(crate) struct Parsed {
    pub info: Info,
    pub gecko_id: Option<String>,
}

/// Read access to an extension's files, by path relative to its root, so a folder and a ZIP share one parser.
pub(crate) trait Files {
    fn read(&mut self, rel: &str) -> Option<Vec<u8>>;
}

pub(crate) struct DirFiles<'a>(pub &'a Path);

impl Files for DirFiles<'_> {
    fn read(&mut self, rel: &str) -> Option<Vec<u8>> {
        fs::read(self.0.join(safe_relative(rel)?)).ok()
    }
}

pub(crate) struct ZipFiles<R>(pub ZipArchive<R>);

impl<R: Read + Seek> Files for ZipFiles<R> {
    fn read(&mut self, rel: &str) -> Option<Vec<u8>> {
        let mut f = self.0.by_name(rel).ok()?;
        let mut out = Vec::new();
        // A manifest or messages file is small; the cap keeps a hostile ZIP from filling memory just to be read.
        f.by_ref().take(8 * 1024 * 1024).read_to_end(&mut out).ok()?;
        Some(out)
    }
}

/// Reads an unpacked extension folder (one with `manifest.json` at its top).
///
/// Themes and Chrome apps are refused with `Error::Unsupported`.
pub fn read_manifest(dir: &Path) -> Result<Info, Error> {
    Ok(parse(&mut DirFiles(dir), Some(dir))?.info)
}

/// `icon_root` is the folder icon paths are joined to, when the files are on disk.
pub(crate) fn parse(files: &mut dyn Files, icon_root: Option<&Path>) -> Result<Parsed, Error> {
    let raw = files.read("manifest.json").ok_or_else(|| Error::Manifest("no manifest.json".into()))?;
    let text = String::from_utf8_lossy(&raw);
    // Chrome accepts a byte order mark that serde_json does not.
    let m: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| Error::Manifest(e.to_string()))?;
    let m = m.as_object().ok_or_else(|| Error::Manifest("manifest.json is not an object".into()))?;
    if m.contains_key("theme") {
        return Err(Error::Unsupported("theme".into()));
    }
    if m.contains_key("app") {
        return Err(Error::Unsupported("app".into()));
    }

    let messages = Messages::load(files, m.get("default_locale").and_then(Value::as_str));
    let text_of = |key: &str| m.get(key).and_then(Value::as_str).map(|s| messages.resolve(s));
    let name = text_of("name").filter(|n| !n.trim().is_empty()).ok_or_else(|| Error::Manifest("no name".into()))?;

    let manifest_version = m.get("manifest_version").and_then(Value::as_u64).unwrap_or(0) as u32;
    let mut permissions = Vec::new();
    let mut host_permissions = Vec::new();
    let strings = |key: &str| -> Vec<String> { m.get(key).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(String::from).collect() };
    // MV2 lists hosts among the API permissions.
    for p in strings("permissions") {
        if is_host_pattern(&p) {
            push_unique(&mut host_permissions, p);
        } else {
            push_unique(&mut permissions, p);
        }
    }
    for p in strings("host_permissions") {
        push_unique(&mut host_permissions, p);
    }
    for script in m.get("content_scripts").and_then(Value::as_array).into_iter().flatten() {
        for p in script.get("matches").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
            push_unique(&mut host_permissions, p.to_string());
        }
    }

    let icon = icon_root.and_then(|root| best_icon(m, root));
    let gecko_id = ["browser_specific_settings", "applications"]
        .iter()
        .find_map(|k| m.get(*k)?.get("gecko")?.get("id")?.as_str())
        .map(String::from);

    Ok(Parsed {
        info: Info {
            name,
            description: text_of("description").unwrap_or_default(),
            version: m.get("version").and_then(Value::as_str).unwrap_or_default().to_string(),
            icon,
            manifest_version,
            permissions,
            host_permissions,
        },
        gecko_id,
    })
}

fn push_unique(list: &mut Vec<String>, item: String) {
    if !list.contains(&item) {
        list.push(item);
    }
}

fn is_host_pattern(p: &str) -> bool {
    p == "<all_urls>" || p.contains("://")
}

/// `__MSG_key__` placeholders from `_locales/<locale>/messages.json`.
struct Messages(HashMap<String, String>);

impl Messages {
    /// The default locale, else `en`, else `en_US`: Chrome requires `default_locale` once `_locales` exists, but
    /// hand-made folders often leave it out.
    fn load(files: &mut dyn Files, default_locale: Option<&str>) -> Messages {
        let mut tries: Vec<String> = Vec::new();
        if let Some(l) = default_locale {
            // Chrome writes `en-US` in manifests and `en_US` in folder names.
            tries.push(l.to_string());
            tries.push(l.replace('-', "_"));
        }
        tries.extend(["en".to_string(), "en_US".to_string()]);
        for locale in tries {
            let Some(raw) = files.read(&format!("_locales/{locale}/messages.json")) else { continue };
            let text = String::from_utf8_lossy(&raw);
            let Ok(Value::Object(map)) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) else { continue };
            // Message names are case-insensitive in Chrome.
            let map = map.iter().filter_map(|(k, v)| Some((k.to_lowercase(), v.get("message")?.as_str()?.to_string()))).collect();
            return Messages(map);
        }
        Messages(HashMap::new())
    }

    /// Replaces every `__MSG_name__` it has a message for and leaves the others as written.
    fn resolve(&self, s: &str) -> String {
        let mut out = String::new();
        let mut rest = s;
        while let Some(start) = rest.find("__MSG_") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 6..];
            match after.find("__") {
                Some(end) => match self.0.get(&after[..end].to_lowercase()) {
                    Some(msg) => out.push_str(msg),
                    None => out.push_str(&rest[start..start + 6 + end + 2]),
                },
                None => {
                    out.push_str(&rest[start..]);
                    return out;
                }
            }
            rest = &after[after.find("__").map_or(after.len(), |e| e + 2)..];
        }
        out.push_str(rest);
        out
    }
}

/// The largest icon up to 128 px from `icons`, else the action's, else the smallest bigger one.
fn best_icon(m: &serde_json::Map<String, Value>, root: &Path) -> Option<PathBuf> {
    let exists = |rel: &str| -> Option<PathBuf> {
        let p = root.join(safe_relative(rel.trim_start_matches('/'))?);
        p.is_file().then_some(p)
    };
    let pick = |v: &Value| -> Option<PathBuf> {
        match v {
            Value::String(s) => exists(s),
            Value::Object(map) => {
                let mut sized: Vec<(u32, &str)> = map.iter().filter_map(|(k, v)| Some((k.parse().ok()?, v.as_str()?))).collect();
                sized.sort_by_key(|(n, _)| std::cmp::Reverse(*n));
                // Largest first, but only up to 128, and only a file that exists.
                sized.iter().filter(|(n, _)| *n <= 128).find_map(|(_, p)| exists(p))
            }
            _ => None,
        }
    };
    if let Some(p) = m.get("icons").and_then(pick) {
        return Some(p);
    }
    for key in ["action", "browser_action", "page_action"] {
        if let Some(p) = m.get(key).and_then(|a| a.get("default_icon")).and_then(pick) {
            return Some(p);
        }
    }
    let Value::Object(map) = m.get("icons")? else { return None };
    let mut sized: Vec<(u32, &str)> = map.iter().filter_map(|(k, v)| Some((k.parse().ok()?, v.as_str()?))).collect();
    sized.sort_by_key(|(n, _)| *n);
    sized.iter().find_map(|(_, p)| exists(p))
}
