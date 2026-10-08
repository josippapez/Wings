//! Installing, turning on and off, and removing plugins. A package is a `.wings-plugin` file: a zip with
//! `wings-plugin.json` at its root, or one folder down like GitHub's source zips.

use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::plugins::{self, Contributes, Manifest, Permissions, Plugin};

const MAX_FILES: usize = 2_000;
const MAX_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Source {
    /// Installed from a `.wings-plugin` file.
    File,
    /// Installed from a public GitHub repo, `owner/name`.
    Github { repo: String },
}

/// What the user agreed to: the access a plugin asks for and what it adds, like MCP tools.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Grant {
    permissions: Permissions,
    contributes: Contributes,
}

impl Grant {
    fn of(manifest: &Manifest) -> Self {
        Self { permissions: manifest.permissions.clone(), contributes: manifest.contributes.clone() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    enabled: bool,
    /// The plugin only runs while its manifest asks for exactly what was approved.
    approved: Option<Grant>,
    source: Source,
}

/// A plugin as the manager shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginView {
    #[serde(flatten)]
    pub manifest: Manifest,
    pub enabled: bool,
    /// False until the user approves this version's permissions, and again after an update changes them.
    pub approved: bool,
    pub source: Option<Source>,
    /// Loaded from the repo's `plugins/` folder by a dev build: always approved, can't be removed.
    pub dev: bool,
}

pub struct Store {
    root: PathBuf,
    state_file: PathBuf,
    plugins: HashMap<String, Plugin>,
    entries: HashMap<String, Entry>,
    dev: HashSet<String>,
}

impl Store {
    /// Installed plugins from `root`, plus the repo's plugins in dev builds, which replace installed copies.
    pub fn load(root: PathBuf, state_file: PathBuf, dev_root: Option<&Path>) -> Self {
        let entries = fs::read_to_string(&state_file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let mut store = Self { plugins: HashMap::new(), root, state_file, entries, dev: HashSet::new() };
        // debt: on Windows WebView2 runs Tauri's IPC script in child frames too, and pages on the
        // wings-plugin scheme count as local, so a plugin frame could call any command. Plugins stay
        // off there until each one runs in its own webview with only plugin commands allowed.
        if cfg!(windows) {
            return store;
        }
        store.plugins = plugins::discover(std::slice::from_ref(&store.root));
        if let Some(dev) = dev_root {
            for (id, plugin) in plugins::discover(&[dev.to_path_buf()]) {
                store.dev.insert(id.clone());
                store.plugins.insert(id, plugin);
            }
        }
        store
    }

    pub fn list(&self) -> Vec<PluginView> {
        let mut list: Vec<PluginView> = self.plugins.keys().filter_map(|id| self.view(id)).collect();
        list.sort_by_key(|p| p.manifest.name.to_lowercase());
        list
    }

    pub fn view(&self, id: &str) -> Option<PluginView> {
        let plugin = self.plugins.get(id)?;
        let entry = self.entries.get(id);
        let dev = self.dev.contains(id);
        Some(PluginView {
            manifest: plugin.manifest.clone(),
            // Dev plugins start on; everything else starts off until you approve it.
            enabled: entry.map_or(dev, |e| e.enabled),
            approved: dev || entry.and_then(|e| e.approved.as_ref()) == Some(&Grant::of(&plugin.manifest)),
            source: entry.map(|e| e.source.clone()),
            dev,
        })
    }

    /// The plugin, if it's turned on and its permissions are approved. Everything a plugin does goes through this.
    pub fn active(&self, id: &str) -> Option<Plugin> {
        let view = self.view(id)?;
        (view.enabled && view.approved).then(|| self.plugins[id].clone())
    }

    pub fn source(&self, id: &str) -> Option<Source> {
        self.entries.get(id).map(|e| e.source.clone())
    }

    /// Unpacks a package and installs it, replacing an older version. The previous approval carries over only
    /// for a reinstall from the same GitHub repo, and still only counts if the new version asks for the same
    /// access and adds the same things. A file can come from anyone, so a file install always asks again.
    /// `expected_id` makes an update fail if the package turns out to be a different plugin.
    pub fn install(&mut self, bytes: &[u8], source: Source, expected_id: Option<&str>) -> Result<PluginView, String> {
        if cfg!(windows) {
            return Err("Plugins aren't available on Windows yet".into());
        }
        let staging = self.root.with_file_name(format!(".plugin-install-{}", std::process::id()));
        let _ = fs::remove_dir_all(&staging);
        fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
        let checked = unpack(bytes, &staging).and_then(|()| plugins::load(&staging));
        let id = match checked {
            Ok(plugin) if self.dev.contains(&plugin.manifest.id) => Err(format!("{} is loaded from the repo in this dev build", plugin.manifest.id)),
            Ok(plugin) if expected_id.is_some_and(|id| id != plugin.manifest.id) => {
                Err(format!("The update is a different plugin ({}), so it wasn't installed", plugin.manifest.id))
            }
            Ok(plugin) => Ok(plugin.manifest.id),
            Err(e) => Err(e),
        }
        .inspect_err(|_| {
            let _ = fs::remove_dir_all(&staging);
        })?;
        let dest = self.root.join(&id);
        fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
        let _ = fs::remove_dir_all(&dest);
        fs::rename(&staging, &dest).map_err(|e| e.to_string())?;
        let plugin = plugins::load(&dest)?;
        let old = self.entries.remove(&id).filter(|e| matches!(source, Source::Github { .. }) && e.source == source);
        let approved = old.as_ref().and_then(|e| e.approved.clone());
        let enabled = old.is_some_and(|e| e.enabled) && approved.as_ref() == Some(&Grant::of(&plugin.manifest));
        self.entries.insert(id.clone(), Entry { enabled, approved, source });
        self.plugins.insert(id.clone(), plugin);
        self.save()?;
        Ok(self.view(&id).expect("just installed"))
    }

    /// Turning a plugin on approves `shown`, what the user was shown, and only if that's still what the
    /// installed manifest asks for. A plugin replaced while the dialog was open isn't approved by mistake.
    pub fn set_enabled(&mut self, id: &str, enabled: bool, shown: Option<Grant>) -> Result<PluginView, String> {
        let plugin = self.plugins.get(id).ok_or_else(|| format!("unknown plugin {id}"))?;
        let grant = Grant::of(&plugin.manifest);
        if enabled && shown.as_ref() != Some(&grant) {
            return Err("This plugin changed since you looked at it. Review it again to turn it on.".into());
        }
        let entry = self.entries.entry(id.to_string()).or_insert(Entry { enabled: false, approved: None, source: Source::File });
        entry.enabled = enabled;
        if enabled {
            entry.approved = Some(grant);
        }
        self.save()?;
        Ok(self.view(id).expect("known plugin"))
    }

    pub fn remove(&mut self, id: &str) -> Result<(), String> {
        if self.dev.contains(id) {
            return Err(format!("{id} is loaded from the repo in this dev build"));
        }
        let plugin = self.plugins.remove(id).ok_or_else(|| format!("unknown plugin {id}"))?;
        self.entries.remove(id);
        fs::remove_dir_all(&plugin.dir).map_err(|e| e.to_string())?;
        self.save()
    }

    fn save(&self) -> Result<(), String> {
        let json = serde_json::to_string_pretty(&self.entries).map_err(|e| e.to_string())?;
        let tmp = self.state_file.with_extension("json.tmp");
        fs::write(&tmp, json).and_then(|()| fs::rename(&tmp, &self.state_file)).map_err(|e| e.to_string())
    }
}

/// Extracts the folder that holds `wings-plugin.json`. Refuses paths that leave the folder and oversized packages.
fn unpack(bytes: &[u8], into: &Path) -> Result<(), String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("Not a .wings-plugin file: {e}"))?;
    if zip.len() > MAX_FILES {
        return Err(format!("The package has more than {MAX_FILES} files"));
    }
    let mut base: Option<PathBuf> = None;
    for i in 0..zip.len() {
        let file = zip.by_index(i).map_err(|e| e.to_string())?;
        let Some(path) = file.enclosed_name() else { continue };
        if path.file_name().is_some_and(|n| n == "wings-plugin.json") && path.components().count() <= 2 {
            let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
            if base.as_ref().is_none_or(|b| parent.components().count() < b.components().count()) {
                base = Some(parent);
            }
        }
    }
    let base = base.ok_or("No wings-plugin.json at the top of the package")?;
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(|e| e.to_string())?;
        let path = file.enclosed_name().ok_or_else(|| format!("Unsafe path in package: {}", file.name()))?;
        let Ok(relative) = path.strip_prefix(&base) else { continue };
        if file.is_dir() || relative.as_os_str().is_empty() {
            continue;
        }
        // Count what actually comes out, since the size in the header can lie.
        let mut data = Vec::new();
        (&mut file).take(MAX_BYTES - total + 1).read_to_end(&mut data).map_err(|e| e.to_string())?;
        total += data.len() as u64;
        if total > MAX_BYTES {
            return Err("The package is larger than 50 MB".into());
        }
        let out = into.join(relative);
        if let Some(dir) = out.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        fs::write(&out, data).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// `owner/name` from a GitHub URL (https, ssh or `github.com/owner/name`) or from `owner/name` itself.
pub fn parse_repo(input: &str) -> Option<String> {
    let s = input.trim().trim_end_matches('/');
    let s = s.strip_prefix("git@github.com:").or_else(|| s.split_once("github.com/").map(|(_, rest)| rest)).unwrap_or(s);
    let mut parts = s.split('/');
    let owner = parts.next()?;
    let name = parts.next()?.trim_end_matches(".git");
    let ok = |p: &str| !matches!(p, "" | "." | "..") && p.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b));
    (ok(owner) && ok(name) && parts.next().is_none_or(|p| p == "tree" || p == "releases")).then(|| format!("{owner}/{name}"))
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    zipball_url: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

fn get(url: &str) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
    ureq::get(url).header("User-Agent", "Wings").header("Accept", "application/vnd.github+json").call()
}

fn github_error(repo: &str, error: ureq::Error) -> String {
    match error {
        ureq::Error::StatusCode(404) => format!("Couldn't find {repo} on GitHub. If it's private, install its .wings-plugin file instead."),
        ureq::Error::StatusCode(403 | 429) => "GitHub's rate limit was hit. Try again in a few minutes.".into(),
        e => format!("Couldn't reach GitHub: {e}"),
    }
}

fn latest_release(repo: &str) -> Result<Option<Release>, String> {
    match get(&format!("https://api.github.com/repos/{repo}/releases/latest")) {
        Ok(mut response) => {
            let text = response.body_mut().read_to_string().map_err(|e| e.to_string())?;
            serde_json::from_str(&text).map(Some).map_err(|e| e.to_string())
        }
        // No releases yet, which is fine: the default branch is installed instead.
        Err(ureq::Error::StatusCode(404)) => Ok(None),
        Err(e) => Err(github_error(repo, e)),
    }
}

/// The newest release's `.wings-plugin` file, or its source zip, or the default branch when there's no release.
pub fn download_github(repo: &str) -> Result<Vec<u8>, String> {
    let url = match latest_release(repo)? {
        Some(release) => match release.assets.into_iter().find(|a| a.name.ends_with(".wings-plugin")) {
            Some(asset) => asset.browser_download_url,
            None => release.zipball_url,
        },
        None => format!("https://api.github.com/repos/{repo}/zipball"),
    };
    let mut response = get(&url).map_err(|e| github_error(repo, e))?;
    response.body_mut().with_config().limit(MAX_BYTES).read_to_vec().map_err(|e| e.to_string())
}

/// The newest release's version, without a leading `v`, if the repo has releases.
pub fn latest_version(repo: &str) -> Result<Option<String>, String> {
    Ok(latest_release(repo)?.map(|r| r.tag_name.trim_start_matches('v').to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn package(files: &[(&str, &str)]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, body) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn manifest(id: &str, exec: &[&str]) -> String {
        serde_json::json!({ "id": id, "name": id, "version": "1.0.0", "api": 1, "main": "main.js", "permissions": { "exec": exec } }).to_string()
    }

    fn manifest_with_tools(id: &str, tools: &[&str]) -> String {
        let tools: Vec<_> = tools.iter().map(|t| serde_json::json!({ "name": t })).collect();
        serde_json::json!({ "id": id, "name": id, "version": "1.0.0", "api": 1, "main": "main.js", "contributes": { "mcpTools": tools } }).to_string()
    }

    fn store() -> (Store, PathBuf) {
        let dir = std::env::temp_dir().join(format!("wings-store-{}-{}", std::process::id(), rand_suffix()));
        fs::create_dir_all(&dir).unwrap();
        (Store::load(dir.join("plugins"), dir.join("plugins.json"), None), dir)
    }

    fn rand_suffix() -> u128 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    }

    #[test]
    fn installs_off_and_needs_approval_again_when_permissions_grow() {
        let (mut s, dir) = store();
        let repo = || Source::Github { repo: "owner/demo".into() };
        let v1 = s.install(&package(&[("wings-plugin.json", &manifest("demo", &["git status"])), ("main.js", "")]), repo(), None).unwrap();
        assert!(!v1.enabled && !v1.approved && s.active("demo").is_none());
        let stale = Grant::of(&s.plugins["demo"].manifest);
        assert!(s.set_enabled("demo", true, None).is_err());
        assert!(s.set_enabled("demo", true, Some(stale.clone())).unwrap().approved && s.active("demo").is_some());

        // Same repo, same permissions: stays on. More permissions: off until approved again.
        s.install(&package(&[("wings-plugin.json", &manifest("demo", &["git status"])), ("main.js", "")]), repo(), Some("demo")).unwrap();
        assert!(s.active("demo").is_some());
        let v2 = s.install(&package(&[("wings-plugin.json", &manifest("demo", &["git status", "gh api"])), ("main.js", "")]), repo(), Some("demo")).unwrap();
        assert!(!v2.enabled && !v2.approved && s.active("demo").is_none());
        // What the dialog showed before the update doesn't approve the new permissions.
        assert!(s.set_enabled("demo", true, Some(stale)).unwrap_err().contains("changed since"));

        // A file or another repo with the same id doesn't inherit the approval.
        s.set_enabled("demo", true, Some(Grant::of(&s.plugins["demo"].manifest))).unwrap();
        let same = |s: &mut Store, source| s.install(&package(&[("wings-plugin.json", &manifest("demo", &["git status", "gh api"])), ("main.js", "")]), source, None).unwrap();
        assert!(!same(&mut s, Source::File).approved);
        s.set_enabled("demo", true, Some(Grant::of(&s.plugins["demo"].manifest))).unwrap();
        assert!(!same(&mut s, Source::Github { repo: "someone/else".into() }).approved);

        // An update that turns out to be another plugin is refused.
        let other = package(&[("wings-plugin.json", &manifest("other", &[])), ("main.js", "")]);
        assert!(s.install(&other, repo(), Some("demo")).unwrap_err().contains("different plugin"));
        assert!(s.view("other").is_none());

        // State survives a restart.
        s.set_enabled("demo", true, Some(Grant::of(&s.plugins["demo"].manifest))).unwrap();
        let again = Store::load(dir.join("plugins"), dir.join("plugins.json"), None);
        assert!(again.active("demo").is_some());
        s.remove("demo").unwrap();
        assert!(s.view("demo").is_none() && !dir.join("plugins/demo").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn adding_mcp_tools_needs_approval_again() {
        let (mut s, dir) = store();
        let repo = || Source::Github { repo: "owner/demo".into() };
        s.install(&package(&[("wings-plugin.json", &manifest_with_tools("demo", &[])), ("main.js", "")]), repo(), None).unwrap();
        s.set_enabled("demo", true, Some(Grant::of(&s.plugins["demo"].manifest))).unwrap();
        let v2 = s.install(&package(&[("wings-plugin.json", &manifest_with_tools("demo", &["get_status"])), ("main.js", "")]), repo(), Some("demo")).unwrap();
        assert!(!v2.approved && s.active("demo").is_none());
        assert!(s.install(&package(&[("wings-plugin.json", &manifest_with_tools("demo", &["Bad Name"])), ("main.js", "")]), repo(), None).is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn unpacks_from_one_top_folder_and_refuses_escapes() {
        let (mut s, dir) = store();
        let github = package(&[("owner-demo-abc123/wings-plugin.json", &manifest("demo", &[])), ("owner-demo-abc123/main.js", "x")]);
        s.install(&github, Source::Github { repo: "owner/demo".into() }, None).unwrap();
        assert_eq!(fs::read_to_string(dir.join("plugins/demo/main.js")).unwrap(), "x");

        let escape = package(&[("wings-plugin.json", &manifest("evil", &[])), ("main.js", ""), ("../../outside.txt", "x")]);
        assert!(s.install(&escape, Source::File, None).unwrap_err().contains("Unsafe path"));
        assert!(!dir.join("outside.txt").exists() && s.view("evil").is_none());
        assert!(s.install(b"not a zip", Source::File, None).unwrap_err().contains("Not a .wings-plugin"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn parses_github_repo_urls() {
        for input in ["owner/repo", "https://github.com/owner/repo", "https://github.com/owner/repo.git", "github.com/owner/repo/", "git@github.com:owner/repo.git", "https://github.com/owner/repo/tree/main"] {
            assert_eq!(parse_repo(input).as_deref(), Some("owner/repo"), "{input}");
        }
        for input in ["", "owner", "https://github.com/owner/repo/blob/x", "owner/re po", "../etc/passwd", "../etc"] {
            assert_eq!(parse_repo(input), None, "{input}");
        }
    }
}

