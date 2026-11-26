use std::{
    fs,
    io::{self, Cursor},
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

/// What looking in the other browsers came to: the extensions found, and the browsers whose folders macOS refused to
/// open (EPERM or EACCES), which is not the same as having none.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Detected {
    pub found: Vec<Installed>,
    /// Browser names, each once.
    pub refused: Vec<String>,
}

impl Detected {
    /// Plain words for the person: what macOS refused and what to turn on.
    pub fn refusal_message(&self) -> Option<String> {
        let (last, rest) = self.refused.split_last()?;
        let what = if rest.is_empty() { format!("{last}'s extensions") } else { format!("the extensions of {} and {last}", rest.join(", ")) };
        Some(format!(
            "macOS doesn't let Wings read {what}. Turn on Full Disk Access for Wings in System Settings, Privacy & Security, then try again."
        ))
    }
}

/// Extensions installed in the user's other browsers, Chromium family first, then Firefox.
pub fn detect_installed() -> Detected {
    match std::env::var_os("HOME") {
        Some(home) => scan(&Path::new(&home).join("Library/Application Support")),
        None => Detected::default(),
    }
}

#[cfg(test)]
pub(crate) fn detect_in(app_support: &Path) -> Vec<Installed> {
    scan(app_support).found
}

pub(crate) fn scan(app_support: &Path) -> Detected {
    let mut out = Detected::default();
    for (browser, parts) in CHROMIUM_BROWSERS {
        let base = parts.iter().fold(app_support.to_path_buf(), |p, s| p.join(s));
        let mut refused = false;
        chromium_profiles(browser, &base, &mut out.found, &mut refused);
        if refused {
            out.refused.push((*browser).to_string());
        }
    }
    let mut refused = false;
    firefox_profiles(&app_support.join("Firefox/Profiles"), &mut out.found, &mut refused);
    if refused {
        out.refused.push("Firefox".into());
    }
    out
}

/// Whether macOS said no: `EPERM` (a privacy-protected folder) and `EACCES` both are `PermissionDenied`.
fn denied(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::PermissionDenied
}

/// The entries of `dir`, noting in `refused` when macOS wouldn't open it.
fn entries(dir: &Path, refused: &mut bool) -> Vec<PathBuf> {
    match fs::read_dir(dir) {
        Ok(read) => read.flatten().map(|e| e.path()).collect(),
        Err(e) => {
            *refused |= denied(&e);
            Vec::new()
        }
    }
}

/// `is_dir`, noting in `refused` when macOS wouldn't even say.
fn is_dir(path: &Path, refused: &mut bool) -> bool {
    match fs::metadata(path) {
        Ok(meta) => meta.is_dir(),
        Err(e) => {
            *refused |= denied(&e);
            false
        }
    }
}

fn sorted_dirs_noting(dir: &Path, refused: &mut bool) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = entries(dir, refused).into_iter().filter(|p| p.is_dir()).collect();
    v.sort();
    v
}

fn sorted_dirs(dir: &Path) -> Vec<PathBuf> {
    sorted_dirs_noting(dir, &mut false)
}

fn chromium_profiles(browser: &str, base: &Path, out: &mut Vec<Installed>, refused: &mut bool) {
    // `Local State` holds the names people gave their profiles; the folder is `Default` or `Profile 3`.
    let names: Option<Value> = fs::read(base.join("Local State")).ok().and_then(|b| serde_json::from_slice(&b).ok());
    for profile_dir in sorted_dirs_noting(base, refused) {
        let extensions = profile_dir.join("Extensions");
        if !is_dir(&extensions, refused) {
            continue;
        }
        let folder = profile_dir.file_name().unwrap_or_default().to_string_lossy().to_string();
        let profile = names
            .as_ref()
            .and_then(|v| v.pointer("/profile/info_cache")?.get(&folder)?.get("name")?.as_str())
            .map_or_else(|| folder.clone(), String::from);
        for ext_dir in sorted_dirs_noting(&extensions, refused) {
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
fn firefox_profiles(base: &Path, out: &mut Vec<Installed>, refused: &mut bool) {
    for profile_dir in sorted_dirs_noting(base, refused) {
        let folder = profile_dir.file_name().unwrap_or_default().to_string_lossy().to_string();
        // `a1b2c3d4.default-release` is shown as `default-release`.
        let profile = folder.split_once('.').map_or(folder.clone(), |(_, n)| n.to_string());
        let mut xpis = entries(&profile_dir.join("extensions"), refused);
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
    let archive = ZipArchive::new(Cursor::new(crate::read_package(path)?))?;
    let parsed = parse(&mut ZipFiles(archive), None)?;
    let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
    Ok((parsed.gecko_id.unwrap_or(stem), parsed.info))
}
