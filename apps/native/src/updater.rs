//! Native-only signed releases. No feed value becomes a command or an installation path.
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use base64::{Engine, engine::general_purpose::STANDARD};
use minisign_verify::{PublicKey, Signature};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub type Result<T> = std::result::Result<T, String>;
#[cfg(test)]
#[path = "updater_tests.rs"]
mod tests;
/// The side-by-side app has its own version. The replacement carries the desktop's (`build.rs`), so the desktop's
/// updater offers it and its own updater goes on from there.
pub const VERSION: &str = if crate::data_dir::REPLACEMENT {
    env!("WINGS_DESKTOP_VERSION")
} else {
    env!("CARGO_PKG_VERSION")
};
const RESTART_ACK: &str = "--update-restarted";

/// Which app a release is for, as its signed feed, archive and bundle name it.
struct Identity {
    bundle_identifier: &'static str,
    /// The bundle's folder name, the archive's only top-level entry.
    root: &'static str,
    /// The archive is `<archive>-<version>-<platform>.app.tar.gz`.
    archive: &'static str,
    /// The feed is `<feed>-<platform>.json` in the latest release.
    feed: &'static str,
    /// The release holding the archive is tagged `<tag><version>`.
    tag: &'static str,
}

const APP: Identity = if crate::data_dir::REPLACEMENT {
    Identity {
        bundle_identifier: "dev.wings.app",
        root: "Wings.app",
        archive: "Wings",
        feed: "wings-latest",
        tag: "v",
    }
} else {
    Identity {
        bundle_identifier: "dev.wings.native",
        root: "Wings-native.app",
        archive: "Wings-native",
        feed: "native-latest",
        tag: "native-v",
    }
};
const ROOT: &str = APP.root;
const EXECUTABLES: [&str; 4] = [
    "wings-native",
    "wings-plugin-worker",
    "wings",
    "wings-terminal-server",
];
const REPO: &str = "https://github.com/josippapez/Wings/releases";
const MAX_META: u64 = 64 * 1024;
const MAX_ARCHIVE: u64 = 512 * 1024 * 1024;
const MAX_EXPANDED: u64 = 2 * 1024 * 1024 * 1024;

pub fn platform() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "darwin-aarch64"
    } else {
        "darwin-x86_64"
    }
}

fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn public_key() -> Result<String> {
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../../desktop/src-tauri/tauri.conf.json"))
            .map_err(err)?;
    config["plugins"]["updater"]["pubkey"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "Missing updater public key".into())
}

fn authenticate(bytes: &[u8], signature: &[u8], key: &str) -> Result<()> {
    if bytes.len() as u64 > MAX_META || signature.len() > 4096 {
        return Err("Update metadata exceeds its size limit".into());
    }
    let decode = |value: &[u8]| -> Result<String> {
        String::from_utf8(STANDARD.decode(value).map_err(err)?).map_err(err)
    };
    let key = PublicKey::decode(&decode(key.trim().as_bytes())?).map_err(err)?;
    let signature = Signature::decode(&decode(signature.trim_ascii())?).map_err(err)?;
    key.verify(bytes, &signature, true)
        .map_err(|e| format!("Update authentication failed: {e}"))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Release {
    pub bundle_identifier: String,
    pub version: String,
    pub platform: String,
    pub artifact: String,
    pub size: u64,
    pub sha256: String,
    #[serde(default)]
    pub notes: String,
}

impl Release {
    fn validate(&self, current: &str, arch: &str) -> Result<bool> {
        if self.bundle_identifier != APP.bundle_identifier || self.platform != arch {
            return Err("The update is not for this native app and architecture".into());
        }
        let version = semver::Version::parse(&self.version).map_err(err)?;
        if version.to_string() != self.version
            || !version.pre.is_empty()
            || !version.build.is_empty()
        {
            return Err("The update version must be a stable semantic version".into());
        }
        if self.artifact != format!("{}-{}-{}.app.tar.gz", APP.archive, self.version, arch)
            || self.size == 0
            || self.size > MAX_ARCHIVE
            || self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        {
            return Err("Invalid native update archive, size or digest".into());
        }
        Ok(version > semver::Version::parse(current).map_err(err)?)
    }

    fn url(&self) -> String {
        format!(
            "{REPO}/download/{}{}/{}",
            APP.tag, self.version, self.artifact
        )
    }

    /// The release's page on GitHub, where the dialog sends you when it has no notes.
    pub fn page(&self) -> String {
        format!("{REPO}/tag/{}{}", APP.tag, self.version)
    }
}

#[derive(Clone, Debug)]
pub struct Available {
    pub release: Release,
    manifest: Vec<u8>,
    signature: Vec<u8>,
}

impl Available {
    /// A release to draw, never to install: it has no signed metadata.
    #[cfg(feature = "test-support")]
    pub fn sample(version: &str, notes: &str) -> Self {
        Available {
            release: Release {
                bundle_identifier: APP.bundle_identifier.into(),
                version: version.into(),
                platform: platform().into(),
                artifact: format!("{}-{version}-{}.app.tar.gz", APP.archive, platform()),
                size: 13_529_763,
                sha256: "0".repeat(64),
                notes: notes.into(),
            },
            manifest: Vec::new(),
            signature: Vec::new(),
        }
    }
}

trait Transport: Sync {
    fn get(&self, url: &str, max: u64, output: &mut dyn Write) -> Result<()>;
}

/// What a refused request says. GitHub answers 5xx, mostly 504, for a while after a release is published: like the
/// desktop's updater, the update isn't asked for again on its own, the dialog says so and offers Retry.
fn status_error(code: u16) -> String {
    if code >= 500 {
        format!(
            "GitHub couldn't send the update just now (HTTP {code}). A new release can take a few minutes to become \
             available. Try again shortly."
        )
    } else {
        format!("Update request failed: HTTP {code}")
    }
}

/// The archive as it arrives, telling `progress` how many bytes have so far.
struct Counting<'a, W> {
    inner: W,
    received: u64,
    progress: &'a mut dyn FnMut(u64),
}

impl<W: Write> Write for Counting<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(bytes)?;
        self.received += written as u64;
        (self.progress)(self.received);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

struct Https;
fn trusted_url(url: &str) -> Result<()> {
    let uri: ureq::http::Uri = url.parse().map_err(err)?;
    if uri.scheme_str() != Some("https")
        || !matches!(
            uri.authority().map(|a| a.as_str()),
            Some(
                "github.com"
                    | "release-assets.githubusercontent.com"
                    | "objects.githubusercontent.com"
            )
        )
    {
        return Err("Update redirect is not a trusted HTTPS GitHub asset host".into());
    }
    Ok(())
}

impl Transport for Https {
    fn get(&self, url: &str, max: u64, output: &mut dyn Write) -> Result<()> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .https_only(true)
            .timeout_global(Some(Duration::from_secs(300)))
            .max_redirects(0)
            .build()
            .into();
        let mut url = url.to_owned();
        for _ in 0..6 {
            trusted_url(&url)?;
            let mut response = agent.get(&url).call().map_err(|error| match error {
                ureq::Error::StatusCode(code) => status_error(code),
                error => err(error),
            })?;
            if response.status().is_redirection() {
                let location = response
                    .headers()
                    .get("location")
                    .ok_or("Update redirect has no location")?
                    .to_str()
                    .map_err(err)?;
                url = if location.starts_with('/') && !location.starts_with("//") {
                    let uri: ureq::http::Uri = url.parse().map_err(err)?;
                    format!(
                        "https://{}{}",
                        uri.authority().ok_or("Missing redirect host")?,
                        location
                    )
                } else {
                    location.to_owned()
                };
                continue;
            }
            if !response.status().is_success() {
                return Err(format!("Update request failed: {}", response.status()));
            }
            let count = std::io::copy(&mut response.body_mut().as_reader().take(max + 1), output)
                .map_err(err)?;
            if count > max {
                return Err("Update download exceeds its size limit".into());
            }
            return Ok(());
        }
        Err("Too many update redirects".into())
    }
}

fn check_with(
    transport: &impl Transport,
    key: &str,
    current: &str,
    arch: &str,
) -> Result<Option<Available>> {
    let url = format!("{REPO}/latest/download/{}-{arch}.json", APP.feed);
    let fetch = |url: &str, max: u64| -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        transport.get(url, max, &mut bytes)?;
        Ok(bytes)
    };
    // The feed and its signature are asked for together, so a check waits for one trip through GitHub's redirects, as
    // the desktop's single `latest.json` does, not two in a row.
    let signature_url = format!("{url}.sig");
    let (manifest, signature) = std::thread::scope(|scope| {
        let signature = scope.spawn(|| fetch(&signature_url, 4096));
        let manifest = fetch(&url, MAX_META);
        (manifest, signature.join().unwrap())
    });
    let (manifest, signature) = (manifest?, signature?);
    authenticate(&manifest, &signature, key)?;
    let release: Release = serde_json::from_slice(&manifest).map_err(err)?;
    if !release.validate(current, arch)? {
        return Ok(None);
    }
    Ok(Some(Available {
        release,
        manifest,
        signature,
    }))
}

pub fn check() -> Result<Option<Available>> {
    check_with(&Https, &public_key()?, VERSION, platform())
}

/// Only the actual running bundle decides where to install. Debug/source runs never auto-install.
pub fn destination() -> Result<PathBuf> {
    if !cfg!(target_os = "macos") || cfg!(debug_assertions) {
        return Err("Auto-update is available only in a packaged macOS release build. Build or install the native app bundle first.".into());
    }
    bundle_from_executable(&std::env::current_exe().map_err(err)?, VERSION)
}

fn bundle_from_executable(exe: &Path, version: &str) -> Result<PathBuf> {
    let exe = exe.canonicalize().map_err(err)?;
    let bundle = exe
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .filter(|p| p.extension().is_some_and(|x| x == "app"))
        .ok_or("This executable is not inside a native app bundle")?;
    if exe != bundle.join("Contents/MacOS/wings-native") {
        return Err("Unexpected native executable path".into());
    }
    validate_bundle(bundle, version)?;
    Ok(bundle.to_owned())
}

fn real_directory(bundle: &Path) -> Result<()> {
    if !fs::symlink_metadata(bundle)
        .map_err(err)?
        .file_type()
        .is_dir()
        || bundle.canonicalize().map_err(err)? != bundle
    {
        return Err("Native app must be a real directory, not a symbolic link".into());
    }
    Ok(())
}

fn validate_bundle(bundle: &Path, version: &str) -> Result<()> {
    real_directory(bundle)?;
    let info = plist::Value::from_file(bundle.join("Contents/Info.plist")).map_err(err)?;
    let info = info.as_dictionary().ok_or("Invalid native bundle plist")?;
    for (key, expected) in [
        ("CFBundleIdentifier", APP.bundle_identifier),
        ("CFBundleExecutable", "wings-native"),
        ("CFBundleShortVersionString", version),
        ("CFBundleVersion", version),
    ] {
        if info.get(key).and_then(plist::Value::as_string) != Some(expected) {
            return Err(format!(
                "Native bundle {key} does not match the signed update"
            ));
        }
    }
    for name in EXECUTABLES {
        let meta = fs::symlink_metadata(bundle.join("Contents/MacOS").join(name)).map_err(err)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if !meta.is_file() || meta.permissions().mode() & 0o111 == 0 {
                return Err(format!("Missing executable {name}"));
            }
        }
    }
    Ok(())
}

fn codesign(bundle: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let arch = if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "x86_64"
        };
        for name in EXECUTABLES {
            verify_binary_architecture(&bundle.join("Contents/MacOS").join(name), arch)?;
        }
        let result = Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(bundle)
            .output()
            .map_err(err)?;
        if !result.status.success() {
            return Err(format!(
                "Native bundle code signature is invalid: {}",
                String::from_utf8_lossy(&result.stderr)
            ));
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = bundle;
        return Err("Native updates require macOS".into());
    }
    #[allow(unreachable_code)]
    Ok(())
}

#[cfg(target_os = "macos")]
fn verify_binary_architecture(executable: &Path, arch: &str) -> Result<()> {
    let result = Command::new("/usr/bin/lipo")
        .arg(executable)
        .args(["-verify_arch", arch])
        .output()
        .map_err(err)?;
    if !result.status.success() {
        return Err(format!(
            "Native executable {} does not support {arch}: {}",
            executable.display(),
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    Ok(())
}

fn verify_archive(path: &Path, release: &Release) -> Result<()> {
    let mut file = File::open(path).map_err(err)?;
    if file.metadata().map_err(err)?.len() != release.size {
        return Err("Update archive size does not match its signature".into());
    }
    let mut hash = Sha256::new();
    std::io::copy(&mut file, &mut hash).map_err(err)?;
    if format!("{:x}", hash.finalize()) != release.sha256 {
        return Err("Update archive digest does not match its signature".into());
    }
    Ok(())
}

fn extract(archive: &Path, directory: &Path) -> Result<PathBuf> {
    use std::collections::HashSet;
    let decoder = flate2::read::GzDecoder::new(File::open(archive).map_err(err)?);
    // Limit the entire decompressed stream, including tar headers and extensions.
    let mut tar = tar::Archive::new(decoder.take(MAX_EXPANDED + 1));
    let mut paths = HashSet::new();
    let mut total = 0u64;
    for entry in tar.entries().map_err(err)? {
        let mut entry = entry.map_err(err)?;
        let path = entry.path().map_err(err)?.into_owned();
        let mut components = path.components();
        if components.next() != Some(Component::Normal(ROOT.as_ref()))
            || components.any(|c| !matches!(c, Component::Normal(_)))
            || !paths.insert(path.clone())
            || paths.len() > 20_000
        {
            return Err("Unsafe or duplicate path in native update".into());
        }
        let kind = entry.header().entry_type();
        if !kind.is_file() && !kind.is_dir() {
            return Err("Links and special files are not allowed in native updates".into());
        }
        total = total
            .checked_add(entry.size())
            .ok_or("Update expands beyond its size limit")?;
        if total > MAX_EXPANDED {
            return Err("Update expands beyond its size limit".into());
        }
        let dest = directory.join(path);
        if kind.is_dir() {
            fs::create_dir_all(&dest).map_err(err)?;
        } else {
            fs::create_dir_all(dest.parent().ok_or("Invalid archive path")?).map_err(err)?;
            let mut file = File::options()
                .write(true)
                .create_new(true)
                .open(&dest)
                .map_err(err)?;
            std::io::copy(&mut entry, &mut file).map_err(err)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(
                    &dest,
                    fs::Permissions::from_mode(entry.header().mode().map_err(err)? & 0o755),
                )
                .map_err(err)?;
            }
        }
    }
    Ok(directory.join(ROOT))
}

#[derive(Debug)]
pub struct Prepared {
    directory: tempfile::TempDir,
    target: PathBuf,
    pub version: String,
}

/// `progress` hears how many bytes of the archive have arrived each time more do, out of `release.size`.
fn prepare_with(
    available: &Available,
    target: &Path,
    transport: &impl Transport,
    verify_code: impl Fn(&Path) -> Result<()>,
    mut progress: impl FnMut(u64),
) -> Result<Prepared> {
    let directory = tempfile::Builder::new()
        .prefix(".wings-update-")
        .tempdir_in(target.parent().ok_or("Missing app parent directory")?)
        .map_err(err)?;
    let archive = directory.path().join("update.tar.gz");
    let mut file = File::create(&archive).map_err(err)?;
    transport.get(
        &available.release.url(),
        available.release.size,
        &mut Counting {
            inner: &mut file,
            received: 0,
            progress: &mut progress,
        },
    )?;
    file.sync_all().map_err(err)?;
    verify_archive(&archive, &available.release)?;
    let bundle = extract(&archive, directory.path())?;
    validate_bundle(&bundle, &available.release.version)?;
    verify_code(&bundle)?;
    fs::write(directory.path().join("manifest.json"), &available.manifest).map_err(err)?;
    fs::write(directory.path().join("manifest.sig"), &available.signature).map_err(err)?;
    Ok(Prepared {
        directory,
        target: target.to_owned(),
        version: available.release.version.clone(),
    })
}

pub fn prepare(
    available: &Available,
    target: &Path,
    progress: impl FnMut(u64),
) -> Result<Prepared> {
    prepare_with(available, target, &Https, codesign, progress)
}

#[derive(Serialize, Deserialize)]
struct InstallPlan {
    target: PathBuf,
    args: Vec<std::ffi::OsString>,
    parent: u32,
}

impl Prepared {
    /// Runs a copy of this trusted executable, never code from the downloaded bundle.
    /// Returns only once the helper has registered its parent-exit event.
    pub fn launch_installer(self) -> Result<()> {
        let actual = destination()?;
        if self.target != actual {
            return Err("The running app moved. Check for updates again.".into());
        }
        let helper = self.directory.path().join("installer");
        fs::copy(std::env::current_exe().map_err(err)?, &helper).map_err(err)?;
        let plan = InstallPlan {
            target: actual,
            args: startup_args().0,
            parent: std::process::id(),
        };
        fs::write(
            self.directory.path().join("plan.json"),
            serde_json::to_vec(&plan).map_err(err)?,
        )
        .map_err(err)?;
        let log = File::create(self.directory.path().join("install.log")).map_err(err)?;
        let mut child = Command::new(&helper)
            .arg("--install-update")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(log)
            .spawn()
            .map_err(err)?;
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        let mut output = child.stdout.take().ok_or("Missing installer handshake")?;
        std::thread::spawn(move || {
            let mut byte = [0];
            let result = output.read_exact(&mut byte).map(|_| byte[0]);
            let _ = send.send(result);
        });
        if !matches!(receive.recv_timeout(Duration::from_secs(15)), Ok(Ok(b'R'))) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "The installer could not start: {}",
                fs::read_to_string(self.directory.path().join("install.log")).unwrap_or_default()
            ));
        }
        // The helper owns cleanup from here. Its running executable is outside both app bundles.
        let _ = self.directory.keep();
        Ok(())
    }
}

/// The restart handshake is private installer state, not a workspace option to save or replay.
pub fn startup_args() -> (Vec<std::ffi::OsString>, Option<PathBuf>) {
    let mut args: Vec<_> = std::env::args_os().skip(1).collect();
    let ack = if args.len() >= 2 && args[args.len() - 2] == RESTART_ACK {
        let path = args.pop().map(PathBuf::from);
        args.pop();
        path
    } else {
        None
    };
    (args, ack)
}

pub fn acknowledge_startup(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let target = destination()?;
        let directory = path.parent().ok_or("Missing restart handshake directory")?;
        if directory.parent() != target.parent()
            || path.file_name().is_none_or(|p| p != "restart.sock")
            || !directory
                .file_name()
                .is_some_and(|p| p.to_string_lossy().starts_with(".wings-update-"))
        {
            return Err("Invalid restart handshake location".into());
        }
        std::os::unix::net::UnixStream::connect(path)
            .map_err(err)?
            .write_all(b"R")
            .map_err(err)?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn restart_app(target: &Path, args: &[std::ffi::OsString], directory: &Path) -> Result<()> {
    restart_and_confirm(target, args, directory, Duration::from_secs(30))
}

#[cfg(target_os = "macos")]
fn restart_and_confirm(
    target: &Path,
    args: &[std::ffi::OsString],
    directory: &Path,
    timeout: Duration,
) -> Result<()> {
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixListener;
    let socket = directory.join("restart.sock");
    let listener = UnixListener::bind(&socket).map_err(err)?;
    let result = (|| {
        let mut child = Command::new(target.join("Contents/MacOS/wings-native"))
            .args(args)
            .arg(RESTART_ACK)
            .arg(&socket)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(err)?;
        let confirmed = (|| -> Result<()> {
            let mut poll = libc::pollfd {
                fd: listener.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: poll references one live descriptor owned by the listener.
            let count = unsafe {
                libc::poll(
                    &mut poll,
                    1,
                    timeout.as_millis().min(i32::MAX as u128) as i32,
                )
            };
            if count < 0 {
                return Err(err(std::io::Error::last_os_error()));
            }
            if count == 0 {
                return Err("The app did not confirm opening before the restart deadline".into());
            }
            if poll.revents & libc::POLLIN == 0 {
                return Err("The app restart handshake socket failed".into());
            }
            let (connection, _) = listener.accept().map_err(err)?;
            read_confirmation(connection, timeout)
        })();
        if confirmed.is_err() {
            let _ = child.kill();
            let _ = child.wait();
        }
        confirmed
    })();
    drop(listener);
    if let Err(cleanup) = fs::remove_file(&socket) {
        if let Err(error) = result {
            return Err(format!(
                "{error}. Cannot remove restart handshake: {cleanup}"
            ));
        }
        eprintln!("The app opened, but its restart handshake could not be removed: {cleanup}");
    }
    result
}

/// The new app's one-byte startup confirmation. The app writes it and hangs up at once, and macOS refuses a read
/// timeout on a socket whose peer has gone (EINVAL): failing on that killed every new app that confirmed, and rolled
/// the update back. Then the byte is already buffered, so the read needs no timeout.
#[cfg(target_os = "macos")]
fn read_confirmation(
    mut connection: std::os::unix::net::UnixStream,
    timeout: Duration,
) -> Result<()> {
    let _ = connection.set_read_timeout(Some(timeout));
    let mut byte = [0];
    connection.read_exact(&mut byte).map_err(err)?;
    if byte[0] != b'R' {
        return Err("Invalid app startup confirmation".into());
    }
    Ok(())
}

/// Called before GPUI or any app services start.
pub fn installer_entry() -> Option<Result<()>> {
    if std::env::args_os().nth(1).as_deref() != Some(std::ffi::OsStr::new("--install-update")) {
        return None;
    }
    Some(run_installer())
}

#[cfg(target_os = "macos")]
fn run_installer() -> Result<()> {
    let directory = std::env::current_exe()
        .map_err(err)?
        .parent()
        .ok_or("Missing installer directory")?
        .to_owned();
    let plan: InstallPlan =
        serde_json::from_slice(&fs::read(directory.join("plan.json")).map_err(err)?)
            .map_err(err)?;
    // Only a direct child of the still-running app may wait and replace it.
    if unsafe { libc::getppid() } as u32 != plan.parent {
        return Err("Installer parent changed".into());
    }
    let mut parent_path = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let count = unsafe {
        libc::proc_pidpath(
            plan.parent as i32,
            parent_path.as_mut_ptr().cast(),
            parent_path.len() as u32,
        )
    };
    if count <= 0 {
        return Err("Cannot identify the running parent app".into());
    }
    use std::os::unix::ffi::OsStrExt;
    let end = parent_path
        .iter()
        .position(|&b| b == 0)
        .ok_or("Invalid parent executable path")?;
    let parent_bundle = bundle_from_executable(
        Path::new(std::ffi::OsStr::from_bytes(&parent_path[..end])),
        VERSION,
    )?;
    if parent_bundle != plan.target {
        return Err("Installer target is not the running parent app".into());
    }
    let manifest = fs::read(directory.join("manifest.json")).map_err(err)?;
    authenticate(
        &manifest,
        &fs::read(directory.join("manifest.sig")).map_err(err)?,
        &public_key()?,
    )?;
    let release: Release = serde_json::from_slice(&manifest).map_err(err)?;
    if !release.validate(VERSION, platform())? {
        return Err("Update is not newer than this app".into());
    }
    validate_bundle(&plan.target, VERSION)?;
    if directory.parent() != plan.target.parent() {
        return Err("Update staging must be beside the running app".into());
    }
    verify_archive(&directory.join("update.tar.gz"), &release)?;
    // Re-extract from authenticated bytes instead of trusting staged files between processes.
    fs::remove_dir_all(directory.join(ROOT)).map_err(err)?;
    let staged = extract(&directory.join("update.tar.gz"), &directory)?;
    validate_bundle(&staged, &release.version)?;
    codesign(&staged)?;
    wait_for_exit(plan.parent, || {
        std::io::stdout().write_all(b"R").map_err(err)?;
        std::io::stdout().flush().map_err(err)
    })?;
    let result = replace_and_restart(&plan.target, &staged, swap, |target| {
        restart_app(target, &plan.args, &directory)
    });
    match &result {
        Ok(()) => fs::remove_dir_all(&directory).map_err(err)?,
        Err(error) => eprintln!(
            "Update failed. The original bundle is restored or retained at {}. {error}",
            staged.display()
        ),
    }
    result
}

#[cfg(target_os = "macos")]
fn wait_for_exit(parent: u32, ready: impl FnOnce() -> Result<()>) -> Result<()> {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    let fd = unsafe { libc::kqueue() };
    if fd < 0 {
        return Err(err(std::io::Error::last_os_error()));
    }
    // SAFETY: kqueue returned a fresh, owned descriptor.
    let queue = unsafe { OwnedFd::from_raw_fd(fd) };
    let event = libc::kevent {
        ident: parent as usize,
        filter: libc::EVFILT_PROC,
        flags: libc::EV_ADD | libc::EV_ENABLE | libc::EV_ONESHOT,
        fflags: libc::NOTE_EXIT,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    // SAFETY: all pointers reference live kevent structures, and the descriptor is owned above.
    if unsafe {
        libc::kevent(
            queue.as_raw_fd(),
            &event,
            1,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
        )
    } < 0
    {
        return Err(err(std::io::Error::last_os_error()));
    }
    ready()?;
    let mut exited = event;
    // SAFETY: the output buffer holds one event. No timeout means actual process exit, not a delay.
    if unsafe {
        libc::kevent(
            queue.as_raw_fd(),
            std::ptr::null(),
            0,
            &mut exited,
            1,
            std::ptr::null(),
        )
    } != 1
    {
        return Err(err(std::io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn run_installer() -> Result<()> {
    Err("Native updates require macOS".into())
}

#[cfg(target_os = "macos")]
fn swap(a: &Path, b: &Path) -> Result<()> {
    use std::os::unix::ffi::OsStrExt;
    real_directory(a)?;
    real_directory(b)?;
    let a = std::ffi::CString::new(a.as_os_str().as_bytes()).map_err(err)?;
    let b = std::ffi::CString::new(b.as_os_str().as_bytes()).map_err(err)?;
    if unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), libc::RENAME_SWAP) } != 0 {
        return Err(err(std::io::Error::last_os_error()));
    }
    Ok(())
}

fn replace_and_restart(
    target: &Path,
    staged: &Path,
    swap: impl Fn(&Path, &Path) -> Result<()>,
    restart: impl Fn(&Path) -> Result<()>,
) -> Result<()> {
    // Atomic exchange leaves a usable app at target even if the helper is interrupted.
    swap(target, staged)?;
    if let Err(error) = restart(target) {
        swap(target, staged).map_err(|rollback| {
            format!(
                "Restart failed: {error}. Restore the old app from {}: {rollback}",
                staged.display()
            )
        })?;
        if let Err(restored) = restart(target) {
            return Err(format!(
                "Update restart failed: {error}. The original app was restored, but restarting it also failed: {restored}"
            ));
        }
        return Err(format!(
            "Update restart failed: {error}. The original app was restored and reopened."
        ));
    }
    Ok(())
}
