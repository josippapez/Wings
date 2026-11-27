//! The Chromium engine: whether it is downloaded, which engine each profile's web views use, and the steps that fetch,
//! check, unpack and remove it. No CEF in here: the engine is a bundle (`Wings Chromium.app`) in a folder of this app's
//! own, loaded by `crates/wings-chromium` only when a profile chose it.
//!
//! ```text
//! <data folder>/chromium/choice.json        which engine each profile uses
//! <data folder>/chromium/downloads/         the archive while it downloads and until it is installed
//! <data folder>/chromium/<version>/         one installed engine: `Wings Chromium.app` and a `finished` marker written last
//! <data folder>/chromium/profiles/          what Chromium keeps for sites (the backend makes it)
//! ```
//!
//! The choice lives here, not in the plugin's web folder: `plugin-web` is shared with the side-by-side build, which may
//! not have the engine. The archive is pinned by size and SHA-256, so whoever serves it can't change what is installed,
//! and the bundle it unpacks to by its tree hash (`tree_hash`), which every start of Chromium checks again
//! (`verify_bundle`): the engine runs inside Wings' own process, so a file changed in it would run as Wings.

use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

use serde::{Deserialize, Serialize};
use sha2_0_11::{Digest, Sha256};

use super::Profile;

/// The engine a profile's web views use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    WebKit,
    Chromium,
}

/// The one engine release this build installs.
pub struct Pinned {
    pub version: &'static str,
    pub file: &'static str,
    pub url: &'static str,
    /// The archive's size in bytes: the most the download may be.
    pub size: u64,
    pub sha256: &'static str,
    /// The bytes of the files in the archive: the most it may unpack to (plus `MARGIN`).
    pub unpacked: u64,
    /// The `tree_hash` of the bundle the archive unpacks to, as `install` unpacks it: what `verify_bundle` checks.
    pub tree: &'static str,
}

pub const PINNED: Pinned = Pinned {
    version: "156.0.31",
    file: "wings-chromium-156.0.31-darwin-aarch64.tar.gz",
    url: "https://github.com/josippapez/Wings/releases/download/chromium-156.0.31/wings-chromium-156.0.31-darwin-aarch64.tar.gz",
    size: 147_328_901,
    sha256: "fa275c5921611e274965793715f6f21927e8b01c16bfa9421d457f996fad668a",
    unpacked: 343_012_256,
    tree: "38d65af53edd49714bac7895e364d25a5b9c8a2c87dbea0706a2cc867787aaca",
};

/// What the unpacked archive may hold beyond `Pinned::unpacked`: tar's headers and padding.
const MARGIN: u64 = 8 << 20;
/// The most entries an archive may hold: the CEF framework has a few hundred.
const MAX_ENTRIES: usize = 20_000;
/// The deepest a folder in the bundle may be: the framework's resources are about ten down.
const MAX_DEPTH: usize = 32;
const BUNDLE: &str = "Wings Chromium.app";
/// Written last: a folder without it is an install that didn't finish.
const FINISHED: &str = "finished";
const WRONG_DOWNLOAD: &str = "The download didn't match what Wings expects, so it was deleted. Try again.";
/// Why Chromium doesn't start when its bundle isn't, file for file, the one Wings installed.
pub const CHANGED: &str = "Chromium's files were changed after install. Remove and download it again.";
const UNPACKED_WRONG: &str = "Chromium didn't unpack to the files Wings expects, so it wasn't installed. Try again.";

// ---- Where things are ----

/// This build's folder for the engine, beside its workspace file.
pub fn root() -> Option<PathBuf> {
    Some(crate::data_dir::state_file()?.parent()?.join("chromium"))
}

/// The installed engine's bundle: only when an install finished.
pub fn bundle() -> Option<PathBuf> {
    installed_in(&root()?, &PINNED)
}

/// Where Chromium keeps site data.
pub fn profiles() -> Option<PathBuf> {
    Some(root()?.join("profiles"))
}

fn installed_in(root: &Path, pin: &Pinned) -> Option<PathBuf> {
    let folder = root.join(pin.version);
    (folder.join(FINISHED).is_file() && folder.join(BUNDLE).is_dir()).then(|| folder.join(BUNDLE))
}

fn downloads_in(root: &Path) -> PathBuf {
    root.join("downloads")
}

// ---- Can this Mac run it ----

/// Why Chromium can't run here, if it can't.
pub fn available() -> Result<(), &'static str> {
    if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return Err("Chromium needs a Mac with Apple silicon");
    }
    if macos_major().is_some_and(|major| major < 12) {
        return Err("Chromium needs macOS 12 or later");
    }
    Ok(())
}

fn macos_major() -> Option<u32> {
    static MAJOR: OnceLock<Option<u32>> = OnceLock::new();
    *MAJOR.get_or_init(|| {
        let out = std::process::Command::new("/usr/bin/sw_vers").arg("-productVersion").output().ok()?;
        String::from_utf8_lossy(&out.stdout).trim().split('.').next()?.parse().ok()
    })
}

// ---- The choice ----

static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Bumps on every choice, install and removal, so a view that placed its pages with one engine knows to place them again.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

fn bump() {
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

fn key(profile: &Profile) -> String {
    format!("{}.{}", profile.plugin, profile.scope)
}

fn read_choices(root: &Path) -> BTreeMap<String, Kind> {
    fs::read(root.join("choice.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

/// Written whole beside the file and renamed over it, so a crash leaves the old choice or the new, never half.
fn write_choices(root: &Path, choices: &BTreeMap<String, Kind>) -> Result<(), String> {
    fs::create_dir_all(root).map_err(|error| format!("Couldn't make {}: {error}", root.display()))?;
    let part = root.join("choice.json.part");
    let bytes = serde_json::to_vec_pretty(choices).map_err(|error| error.to_string())?;
    fs::write(&part, bytes).map_err(|error| format!("Couldn't save the engine choice: {error}"))?;
    fs::rename(&part, root.join("choice.json")).map_err(|error| {
        let _ = fs::remove_file(&part);
        format!("Couldn't save the engine choice: {error}")
    })
}

/// The engine `profile` uses in `root`: Chromium only when chosen, installed and runnable here.
#[cfg(test)]
fn choice_in(root: &Path, pin: &Pinned, runnable: bool, profile: &Profile) -> Kind {
    let chosen = read_choices(root).get(&key(profile)) == Some(&Kind::Chromium);
    if chosen && runnable && installed_in(root, pin).is_some() { Kind::Chromium } else { Kind::WebKit }
}

/// What the disk held at the first ask of `root`: the choices and whether the engine is installed. `set_choice`,
/// `install` and `remove` keep it current, so the web views' every frame doesn't read the disk. It is for one data
/// folder: another folder (a screenshot's fixture) reads its own.
struct Cache {
    root: PathBuf,
    choices: BTreeMap<String, Kind>,
    installed: bool,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

fn with_cache<T>(root: &Path, use_cache: impl FnOnce(&mut Cache) -> T) -> T {
    let mut guard = CACHE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.as_ref().is_none_or(|cache| cache.root != root) {
        *guard = Some(Cache { root: root.to_owned(), choices: read_choices(root), installed: installed_in(root, &PINNED).is_some() });
    }
    use_cache(guard.as_mut().expect("just filled"))
}

/// The engine `profile`'s web views use: Chromium only when it was chosen for that profile, is installed and runs here.
pub fn choice(profile: &Profile) -> Kind {
    let Some(root) = root() else { return Kind::WebKit };
    let chosen = with_cache(&root, |cache| cache.installed && cache.choices.get(&key(profile)) == Some(&Kind::Chromium));
    if chosen && available().is_ok() { Kind::Chromium } else { Kind::WebKit }
}

pub fn set_choice(profile: &Profile, kind: Kind) -> Result<(), String> {
    let root = root().ok_or("Wings has no data folder")?;
    set_choice_in(&root, profile, kind)?;
    with_cache(&root, |cache| {
        cache.choices.insert(key(profile), kind);
    });
    bump();
    Ok(())
}

fn set_choice_in(root: &Path, profile: &Profile, kind: Kind) -> Result<(), String> {
    let mut choices = read_choices(root);
    choices.insert(key(profile), kind);
    write_choices(root, &choices)
}

// ---- Download, verify, install, remove ----

/// Where `download` gets the archive from, besides the pinned address: a local file, in a debug build or one made with
/// `--features chromium-local`. The pin still applies to it.
fn local_archive() -> Option<PathBuf> {
    (cfg!(debug_assertions) || cfg!(feature = "chromium-local"))
        .then(|| std::env::var_os("WINGS_CHROMIUM_ARCHIVE").filter(|path| !path.is_empty()).map(PathBuf::from))
        .flatten()
}

/// Fetches the archive into `<root>/downloads`. Run it off the UI thread.
pub fn download() -> Result<(), String> {
    let root = root().ok_or("Wings has no data folder")?;
    if let Some(path) = local_archive() {
        let file = fs::File::open(&path).map_err(|error| format!("Couldn't read {}: {error}", path.display()))?;
        return download_from(&root, &PINNED, file);
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(std::time::Duration::from_secs(30 * 60)))
        .build()
        .into();
    // GitHub answers with a redirect to its storage, which ureq follows.
    let mut response = agent.get(PINNED.url).call().map_err(|error| format!("Couldn't download Chromium: {error}"))?;
    download_from(&root, &PINNED, response.body_mut().as_reader())
}

/// Streams `source` to `<root>/downloads/<file>.part`, refusing more than the pinned size, and renames it to
/// `<root>/downloads/<file>` once it is all there.
fn download_from(root: &Path, pin: &Pinned, mut source: impl Read) -> Result<(), String> {
    let dir = downloads_in(root);
    fs::create_dir_all(&dir).map_err(|error| format!("Couldn't make {}: {error}", dir.display()))?;
    let (part, done) = (dir.join(format!("{}.part", pin.file)), dir.join(pin.file));
    let _ = fs::remove_file(&done);
    let streamed = (|| -> Result<(), String> {
        let mut out = fs::File::create(&part).map_err(|error| error.to_string())?;
        let (mut buffer, mut total) = (vec![0u8; 1 << 16], 0u64);
        loop {
            let read = source.read(&mut buffer).map_err(|error| format!("The download failed: {error}"))?;
            if read == 0 {
                break;
            }
            total += read as u64;
            if total > pin.size {
                return Err(WRONG_DOWNLOAD.into());
            }
            out.write_all(&buffer[..read]).map_err(|error| error.to_string())?;
        }
        out.sync_all().map_err(|error| error.to_string())
    })();
    match streamed.and_then(|()| fs::rename(&part, &done).map_err(|error| error.to_string())) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&part);
            Err(error)
        }
    }
}

/// Checks the downloaded archive against the pin; one that doesn't match is deleted.
pub fn verify() -> Result<(), String> {
    verify_in(&root().ok_or("Wings has no data folder")?, &PINNED)
}

fn verify_in(root: &Path, pin: &Pinned) -> Result<(), String> {
    let path = downloads_in(root).join(pin.file);
    let file = fs::File::open(&path).map_err(|_| "There is no download to check. Try again.".to_owned())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    let mut source = Hashing { inner: file, hash: Sha256::new(), read: 0 };
    let hashed = if metadata.len() == pin.size { std::io::copy(&mut source, &mut std::io::sink()).map(drop) } else { Ok(()) };
    let digest: String = source.hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    drop(source.inner);
    if hashed.is_ok() && metadata.len() == pin.size && digest == pin.sha256 {
        return Ok(());
    }
    let _ = fs::remove_file(&path);
    Err(WRONG_DOWNLOAD.into())
}

/// What the last `verify_bundle` of this run found wrong, for the Engine section to say (`failed_check`).
static FAILED_CHECK: Mutex<Option<String>> = Mutex::new(None);

/// Why Chromium didn't start this run, if its files failed their check.
pub fn failed_check() -> Option<String> {
    FAILED_CHECK.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}

fn note_check(failed: Option<String>) {
    *FAILED_CHECK.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = failed;
}

/// Whether the engine at `bundle` is, file for file, the one this build pins (`Pinned::tree`): what Wings unpacked from
/// the archive it checked, with nothing changed, added or taken away since. Run it off the main thread before every
/// start of Chromium: it reads every byte of the bundle (343 MB, 0.22 s in a release build on an M-series Mac).
///
/// It proves the files were right as it read them. A program running as you can still change one between this check and
/// CEF loading it; only an engine signed with a Developer ID, the hardened runtime and library validation closes that.
pub fn verify_bundle(bundle: &Path) -> Result<(), String> {
    verify_against(bundle, &PINNED)
}

fn verify_against(bundle: &Path, pin: &Pinned) -> Result<(), String> {
    let checked = if matches_pin(bundle, pin) { Ok(()) } else { Err(CHANGED.to_owned()) };
    note_check(checked.clone().err());
    checked
}

/// The name `web::chromium::runtime` calls `verify_bundle` by, from when the engine's signature was all it checked.
pub use self::verify_bundle as verify_signature;

fn matches_pin(bundle: &Path, pin: &Pinned) -> bool {
    tree_hash(bundle, pin.unpacked).is_ok_and(|tree| tree.hash == pin.tree)
}

/// What `tree_hash` read.
pub struct Tree {
    /// The SHA-256, as hex.
    pub hash: String,
    /// The bytes of its files, and how many files and folders it holds.
    pub bytes: u64,
    pub entries: usize,
}

/// The SHA-256 of the bundle folder `bundle`: each entry's kind and path in it, and for a file its mode, size and bytes,
/// in the order of their names, so the same files give the same hash on any Mac and a file edited, added, removed,
/// renamed or made executable gives another. A folder's mode is left out, as `extract` makes folders with the umask of
/// whoever installs. Only files and folders: a link or anything else is refused, as `extract` never makes one. Stops at
/// more than `max` bytes of files, `MAX_ENTRIES` entries or `MAX_DEPTH` folders down, which no bundle that matches has.
pub fn tree_hash(bundle: &Path, max: u64) -> Result<Tree, String> {
    let is_folder = fs::symlink_metadata(bundle).map_err(|error| format!("Couldn't read {}: {error}", bundle.display()))?.is_dir();
    if !is_folder {
        return Err(format!("{} is not a folder", bundle.display()));
    }
    let mut walk = Walk { hash: Sha256::new(), bytes: 0, entries: 0, max, buffer: vec![0; 1 << 20] };
    walk.folder(bundle, Path::new(""), 0)?;
    let hash = walk.hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(Tree { hash, bytes: walk.bytes, entries: walk.entries })
}

struct Walk {
    hash: Sha256,
    bytes: u64,
    entries: usize,
    max: u64,
    buffer: Vec<u8>,
}

impl Walk {
    /// Each entry goes in as its kind, its path's length and the path, then for a file its mode, its length and its
    /// bytes, so no two trees hash the same stream.
    fn folder(&mut self, dir: &Path, rel: &Path, depth: usize) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return Err("folders nested too deep".into());
        }
        let failed = |path: &Path, error: std::io::Error| format!("Couldn't read {}: {error}", path.display());
        let mut entries: Vec<fs::DirEntry> = fs::read_dir(dir).and_then(|entries| entries.collect()).map_err(|error| failed(dir, error))?;
        entries.sort_by_key(fs::DirEntry::file_name);
        for entry in entries {
            self.entries += 1;
            if self.entries > MAX_ENTRIES {
                return Err("too many files".into());
            }
            let path = rel.join(entry.file_name());
            let kind = entry.file_type().map_err(|error| failed(&path, error))?;
            let tag = if kind.is_dir() {
                b'd'
            } else if kind.is_file() {
                b'f'
            } else {
                return Err(format!("{} is a link or a special file", path.display()));
            };
            let name = path.as_os_str().as_encoded_bytes();
            self.hash.update([tag]);
            self.hash.update((name.len() as u64).to_le_bytes());
            self.hash.update(name);
            if kind.is_dir() {
                self.folder(&entry.path(), &path, depth + 1)?;
                continue;
            }
            // Its mode, size and bytes are what this one open found.
            let mut file = fs::File::open(entry.path()).map_err(|error| failed(&path, error))?;
            let meta = file.metadata().map_err(|error| failed(&path, error))?;
            if !meta.is_file() {
                return Err(format!("{} is a link or a special file", path.display()));
            }
            #[cfg(unix)]
            let mode = std::os::unix::fs::MetadataExt::mode(&meta) & 0o7777;
            #[cfg(not(unix))]
            let mode = 0u32;
            self.bytes = self.bytes.saturating_add(meta.len());
            if self.bytes > self.max {
                return Err("more bytes than the engine has".into());
            }
            self.hash.update(mode.to_le_bytes());
            self.hash.update(meta.len().to_le_bytes());
            let mut left = meta.len();
            while left > 0 {
                let want = self.buffer.len().min(usize::try_from(left).unwrap_or(usize::MAX));
                let read = file.read(&mut self.buffer[..want]).map_err(|error| failed(&path, error))?;
                if read == 0 {
                    return Err(format!("{} changed while it was read", path.display()));
                }
                self.hash.update(&self.buffer[..read]);
                left -= read as u64;
            }
        }
        Ok(())
    }
}

/// What `archive` installs as, unpacked by `install`'s own steps into the empty folder `scratch` with nothing to check it
/// against, and its bundle's `tree_hash`: the package script prints it for `Pinned::tree`
/// (`examples/chromium_engine_pin.rs`). Only for an archive Wings' own script made.
pub fn tree_of(archive: &Path, scratch: &Path) -> Result<(PathBuf, Tree), String> {
    let file = fs::File::open(archive).map_err(|error| format!("Couldn't read {}: {error}", archive.display()))?;
    extract(file, scratch, 4 << 30)?;
    let bundle = scratch.join(BUNDLE);
    let tree = tree_hash(&bundle, u64::MAX)?;
    Ok((bundle, tree))
}

/// Unpacks the verified download and keeps it as the installed engine. Run it off the UI thread.
pub fn install() -> Result<(), String> {
    let root = root().ok_or("Wings has no data folder")?;
    install_in(&root, &PINNED, |bundle| if matches_pin(bundle, &PINNED) { Ok(()) } else { Err(UNPACKED_WRONG.into()) })?;
    note_check(None);
    with_cache(&root, |cache| cache.installed = true);
    bump();
    Ok(())
}

/// Installs the download in `root` (`install_from`). One that isn't the pinned archive is deleted, as `verify` deletes it.
fn install_in(root: &Path, pin: &Pinned, check: impl FnOnce(&Path) -> Result<(), String>) -> Result<(), String> {
    let path = downloads_in(root).join(pin.file);
    let file = fs::File::open(&path).map_err(|_| "There is no download to check. Try again.".to_owned())?;
    let installed = install_from(root, pin, file, check);
    if matches!(&installed, Err(error) if error == WRONG_DOWNLOAD) {
        let _ = fs::remove_file(&path);
    }
    installed
}

/// Unpacks `archive` into a fresh folder in `root` (the same volume), hashing every byte as it reads it, and keeps what
/// it unpacked only when those bytes were the pinned archive's: what is installed is what was hashed, even if the file
/// changes while it is read. Then checks the bundle with `check`, moves the folder to `<root>/<version>`, writes the
/// finished marker last, and deletes the download and any other version. Any failure deletes the fresh folder.
fn install_from(root: &Path, pin: &Pinned, archive: impl Read, check: impl FnOnce(&Path) -> Result<(), String>) -> Result<(), String> {
    let fresh = tempfile::Builder::new().prefix(".installing-").tempdir_in(root).map_err(|error| format!("Couldn't unpack Chromium: {error}"))?;
    // A byte past the pinned size is enough to tell a longer file.
    let mut source = Hashing { inner: archive.take(pin.size + 1), hash: Sha256::new(), read: 0 };
    let unpacked = extract(&mut source, fresh.path(), pin.unpacked + MARGIN);
    // The pin covers the whole file, past the end of the archive in it too.
    let rest = std::io::copy(&mut source, &mut std::io::sink());
    let digest: String = source.hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    if rest.is_err() || source.read != pin.size || digest != pin.sha256 {
        return Err(WRONG_DOWNLOAD.into());
    }
    unpacked?;
    check(&fresh.path().join(BUNDLE))?;
    let target = root.join(pin.version);
    let _ = fs::remove_dir_all(&target);
    let unpacked = fresh.keep();
    if let Err(error) = fs::rename(&unpacked, &target) {
        let _ = fs::remove_dir_all(&unpacked);
        return Err(format!("Couldn't install Chromium: {error}"));
    }
    fs::write(target.join(FINISHED), pin.version).map_err(|error| format!("Couldn't install Chromium: {error}"))?;
    let _ = fs::remove_dir_all(downloads_in(root));
    // The install is done; an old version that won't go is only wasted space.
    let _ = remove_versions(root, Some(pin.version));
    Ok(())
}

/// A reader that hashes and counts what it reads from `inner`.
struct Hashing<R> {
    inner: R,
    hash: Sha256,
    read: u64,
}

impl<R: Read> Read for Hashing<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.hash.update(&buffer[..read]);
        self.read += read as u64;
        Ok(read)
    }
}

/// Every version folder in `root` but `keep`.
fn remove_versions(root: &Path, keep: Option<&str>) -> Result<(), String> {
    for entry in fs::read_dir(root).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_version = name.starts_with(|c: char| c.is_ascii_digit());
        if is_version && Some(name.as_str()) != keep {
            fs::remove_dir_all(entry.path()).map_err(|error| format!("Couldn't delete {}: {error}", entry.path().display()))?;
        }
    }
    Ok(())
}

/// Unpacks the archive `source` into `into`, as the updater does (`updater::extract`): only files and folders, each under
/// the bundle's folder, none twice, and no more than `max` bytes in all, which includes what the gzip stream expands to.
fn extract(source: impl Read, into: &Path, max: u64) -> Result<(), String> {
    let refused = |what: &str| format!("Chromium's download holds {what}, so it wasn't installed");
    let decoder = flate2::read::GzDecoder::new(source);
    // The whole decompressed stream, tar headers included.
    let mut tar = tar::Archive::new(decoder.take(max + 1));
    let (mut paths, mut total) = (HashSet::new(), 0u64);
    for entry in tar.entries().map_err(|error| error.to_string())? {
        let mut entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path().map_err(|error| error.to_string())?.into_owned();
        let mut components = path.components();
        if components.next() != Some(Component::Normal(BUNDLE.as_ref())) || components.any(|c| !matches!(c, Component::Normal(_))) {
            return Err(refused("a path outside the bundle"));
        }
        if !paths.insert(path.clone()) || paths.len() > MAX_ENTRIES {
            return Err(refused("a path twice or too many files"));
        }
        let kind = entry.header().entry_type();
        if !kind.is_file() && !kind.is_dir() {
            return Err(refused("a link or special file"));
        }
        total = total.checked_add(entry.size()).filter(|total| *total <= max).ok_or_else(|| refused("more than it should unpack to"))?;
        let dest = into.join(&path);
        if kind.is_dir() {
            fs::create_dir_all(&dest).map_err(|error| error.to_string())?;
            continue;
        }
        fs::create_dir_all(dest.parent().ok_or("Invalid path in the download")?).map_err(|error| error.to_string())?;
        let mut file = fs::File::options().write(true).create_new(true).open(&dest).map_err(|error| error.to_string())?;
        std::io::copy(&mut entry, &mut file).map_err(|error| error.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = entry.header().mode().map_err(|error| error.to_string())? & 0o755;
            fs::set_permissions(&dest, fs::Permissions::from_mode(mode)).map_err(|error| error.to_string())?;
        }
    }
    if !into.join(BUNDLE).is_dir() {
        return Err(refused("no bundle"));
    }
    Ok(())
}

/// Deletes the engine, its download and what Chromium kept for sites, and puts every profile back on WebKit.
pub fn remove() -> Result<(), String> {
    let root = root().ok_or("Wings has no data folder")?;
    let result = remove_in(&root);
    note_check(None);
    with_cache(&root, |cache| {
        cache.installed = false;
        for kind in cache.choices.values_mut() {
            *kind = Kind::WebKit;
        }
    });
    bump();
    result
}

fn remove_in(root: &Path) -> Result<(), String> {
    // The choice first: whatever fails after it, no profile points at an engine that is gone.
    let mut choices = read_choices(root);
    if choices.values().any(|kind| *kind != Kind::WebKit) {
        choices.values_mut().for_each(|kind| *kind = Kind::WebKit);
        write_choices(root, &choices)?;
    }
    remove_versions(root, None)?;
    for dir in [downloads_in(root), root.join("profiles")] {
        match fs::remove_dir_all(&dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Couldn't delete {}: {error}", dir.display())),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
