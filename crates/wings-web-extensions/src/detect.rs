use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};

use serde_json::Value;
use zip::ZipArchive;

use crate::{
    manifest::{parse, read_manifest, ZipFiles},
    Error,
};

/// An extension another browser already has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub browser_name: String,
    pub profile: String,
    /// The Chromium extension id, or the Firefox add-on id.
    pub id: String,
    pub version: String,
    pub name: String,
    pub description: String,
    pub icon: Option<PathBuf>,
    /// The version folder of a Chromium extension, or the `.xpi` of a Firefox one.
    pub source: PathBuf,
    pub manifest_version: u32,
    pub permissions: Vec<String>,
    pub host_permissions: Vec<String>,
}

/// Each Chromium-family browser's user data folder under `~/Library/Application Support`. Chrome, Brave, Edge, Arc,
/// Chromium and Vivaldi match the table Firefox's own importer uses (`ChromeMigrationUtils.getDataPath`, macOS).
const CHROMIUM_BROWSERS: &[(&str, &[&str])] = &[
    ("Chrome", &["Google", "Chrome"]),
    ("Chrome Beta", &["Google", "Chrome Beta"]),
    ("Chrome Canary", &["Google", "Chrome Canary"]),
    ("Arc", &["Arc", "User Data"]),
    ("Brave", &["BraveSoftware", "Brave-Browser"]),
    ("Edge", &["Microsoft Edge"]),
    ("Chromium", &["Chromium"]),
    ("Vivaldi", &["Vivaldi"]),
];

/// Extensions installed in the user's other browsers, Chromium family first, then Firefox.
pub fn detect_installed() -> Vec<Installed> {
    match std::env::var_os("HOME") {
        Some(home) => detect_in(&Path::new(&home).join("Library/Application Support")),
        None => Vec::new(),
    }
}

pub(crate) fn detect_in(app_support: &Path) -> Vec<Installed> {
    let mut out = Vec::new();
    for (browser, parts) in CHROMIUM_BROWSERS {
        let base = parts.iter().fold(app_support.to_path_buf(), |p, s| p.join(s));
        chromium_profiles(browser, &base, &mut out);
    }
    firefox_profiles(&app_support.join("Firefox/Profiles"), &mut out);
    out
}

fn sorted_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    v.sort();
    v
}

fn chromium_profiles(browser: &str, base: &Path, out: &mut Vec<Installed>) {
    // `Local State` holds the names people gave their profiles; the folder is `Default` or `Profile 3`.
    let names: Option<Value> = fs::read(base.join("Local State")).ok().and_then(|b| serde_json::from_slice(&b).ok());
    for profile_dir in sorted_dirs(base) {
        let extensions = profile_dir.join("Extensions");
        if !extensions.is_dir() {
            continue;
        }
        let folder = profile_dir.file_name().unwrap_or_default().to_string_lossy().to_string();
        let profile = names
            .as_ref()
            .and_then(|v| v.pointer("/profile/info_cache")?.get(&folder)?.get("name")?.as_str())
            .map_or_else(|| folder.clone(), String::from);
        for ext_dir in sorted_dirs(&extensions) {
            let id = ext_dir.file_name().unwrap_or_default().to_string_lossy().to_string();
            // Chrome keeps a `Temp` folder beside the extensions; ids are 32 letters a to p.
            if id.len() != 32 {
                continue;
            }
            let Some(version_dir) = newest_version(&ext_dir) else { continue };
            // Themes, apps and folders without a manifest fail to read and are left out.
            let Ok(info) = read_manifest(&version_dir) else { continue };
            out.push(Installed {
                browser_name: browser.to_string(),
                profile: profile.clone(),
                id,
                version: info.version,
                name: info.name,
                description: info.description,
                icon: info.icon,
                source: version_dir,
                manifest_version: info.manifest_version,
                permissions: info.permissions,
                host_permissions: info.host_permissions,
            });
        }
    }
}

/// The highest `1.2.3_0` style folder, compared number by number.
fn newest_version(ext_dir: &Path) -> Option<PathBuf> {
    let key = |p: &Path| -> Vec<u64> { p.file_name().unwrap_or_default().to_string_lossy().split(['.', '_']).map(|s| s.parse().unwrap_or(0)).collect() };
    sorted_dirs(ext_dir).into_iter().filter(|d| d.join("manifest.json").is_file()).max_by_key(|d| key(d))
}

/// Firefox keeps a profile's installed add-ons as `<id>.xpi` in `extensions/`. Add-ons that ship inside the app are not
/// there, and the profile folder is behind macOS privacy rules for some processes, so an empty result is normal.
fn firefox_profiles(base: &Path, out: &mut Vec<Installed>) {
    for profile_dir in sorted_dirs(base) {
        let folder = profile_dir.file_name().unwrap_or_default().to_string_lossy().to_string();
        // `a1b2c3d4.default-release` is shown as `default-release`.
        let profile = folder.split_once('.').map_or(folder.clone(), |(_, n)| n.to_string());
        let mut xpis: Vec<PathBuf> = fs::read_dir(profile_dir.join("extensions")).into_iter().flatten().flatten().map(|e| e.path()).collect();
        xpis.retain(|p| p.extension().is_some_and(|e| e == "xpi") && p.is_file());
        xpis.sort();
        for xpi in xpis {
            if let Ok((id, info)) = read_xpi(&xpi) {
                out.push(Installed {
                    browser_name: "Firefox".into(),
                    profile: profile.clone(),
                    id,
                    version: info.version,
                    name: info.name,
                    description: info.description,
                    icon: None,
                    source: xpi,
                    manifest_version: info.manifest_version,
                    permissions: info.permissions,
                    host_permissions: info.host_permissions,
                });
            }
        }
    }
}

fn read_xpi(path: &Path) -> Result<(String, crate::Info), Error> {
    let archive = ZipArchive::new(Cursor::new(fs::read(path)?))?;
    let parsed = parse(&mut ZipFiles(archive), None)?;
    let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
    Ok((parsed.gecko_id.unwrap_or(stem), parsed.info))
}
