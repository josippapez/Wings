//! Terminals that outlive the app. Panes run in `wings-terminal-server` (the `wings-ptyd` crate), a process of its own
//! that the app reaches over a socket in its data folder. When the app comes back after a quit, a crash or an update,
//! each saved pane attaches to its program again by its history id. Settings turns it off and on for new panes; it's on
//! unless turned off, except in a debug build (`enabled`).
//!
//! The server never runs from the app bundle: an update deletes the old bundle, and macOS can't attribute a program
//! whose file is gone, so its panes would lose the privacy grants (Screen Recording, hard links into Desktop). The app
//! copies the server out to `terminal-server/bin/`, always under the same name, since macOS keys a grant to a program
//! without a bundle by its path: Full Disk Access, which macOS never asks for (`full_disk_access`), lasts across
//! updates. A release app on macOS starts that copy as a
//! one-shot launchd job that forks the server off and ends (`wings_ptyd::launchd`), so the server is in no coalition of
//! the app's and lives on when the app quits; no job stays loaded. The server owns its privacy responsibility, and the
//! shells it runs inherit it (`wings_ptyd::responsibility`). When launchd fails, the app starts the server directly in
//! a session of its own, which may end with the app, and says so in the server's log. A debug build runs it as its own
//! child instead, which ends with the app and keeps the app's privacy responsibility.
//!
//! A server is versioned apart from the app (`wings_ptyd::SERVER_VERSION`), with a socket and a binary copy per version.
//! One older than the app's is told to drain: it keeps its panes, which the app still attaches, and exits once they have
//! ended, while new panes go to the app's version. Nothing kills a pane on an upgrade. The copy of an older version is
//! deleted once no server of that version answers.
//!
//! When no server answers, or the server is turned off, the app runs new panes in its own process, and saves them under
//! the same history ids. Panes a server still runs are attached either way, and keep running until they're closed.

use std::{
    fs,
    io::{self, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use wings_ptyd::{SERVER_VERSION, client::Control, proto::PaneInfo};

/// `1` runs new panes in the terminal server and `0` runs them in the app, whatever Settings says: for tests and
/// debugging.
pub const ENV: &str = "WINGS_TERMINAL_SERVER";

/// The server binary's name, beside the app's own in its bundle and in the build folder.
pub const BINARY: &str = "wings-terminal-server";

/// The `space_id` of the sidebar's terminals and the quick terminal. Like the desktop's they're in no tab, so nothing
/// saves them: they end when the app quits, and one an earlier app left running (it crashed) ends when the next starts.
pub const LOOSE: &str = "wings:loose";

/// A server nobody uses for this long exits, and the next app to want one starts it.
pub const IDLE: Duration = Duration::from_secs(10 * 60);

/// How long the app waits for a server it started to answer.
const STARTUP: Duration = Duration::from_secs(5);

/// Whether new panes start in the server: `ENV` when set, else what Settings saved (`saved`), on unless turned off. A
/// debug build keeps them in the app unless `ENV` is `1`, since it shares the side-by-side app's settings.
pub fn enabled(saved: Option<bool>) -> bool {
    enabled_with(
        std::env::var_os(ENV).as_deref(),
        saved,
        cfg!(debug_assertions),
    )
}

/// `enabled` for the value of `ENV`, the saved setting and the kind of build.
pub fn enabled_with(env: Option<&std::ffi::OsStr>, saved: Option<bool>, debug: bool) -> bool {
    match env.and_then(|value| value.to_str()) {
        Some("1") => true,
        Some("0") => false,
        _ => !debug && saved.unwrap_or(true),
    }
}

/// The server's folder in the app's data folder `data`: its sockets, its log and the copy of its binary. A debug build
/// has its own, so it never talks to the installed app's server.
pub fn dir(data: &Path) -> PathBuf {
    data.join(if cfg!(debug_assertions) {
        "terminal-server-dev"
    } else {
        "terminal-server"
    })
}

pub fn socket(dir: &Path, version: u32) -> PathBuf {
    dir.join(format!("v{version}.sock"))
}

/// Makes `dir` a folder only this user can open, so nobody else reaches the sockets in it.
fn private_dir(dir: &Path) -> io::Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    if fs::metadata(dir)?.uid() != unsafe { libc::getuid() } {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the folder belongs to another user",
        ));
    }
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
}

/// How a server starts when none answers.
#[derive(Clone)]
pub enum Start {
    /// A release app: copy `bundled` out of the bundle and start the copy with a one-shot launchd job, or directly
    /// in a session of its own when launchd fails.
    Launchd { bundled: PathBuf },
    /// A plain child of this process, which ends with it: debug builds and tests.
    Child(PathBuf),
    /// `Launchd` without launchd: the copy of `bundled` starts directly in a session of its own, as when launchd
    /// fails. Tests of an update use it, which must never load a launchd job.
    Direct { bundled: PathBuf },
}

impl Start {
    /// What this app does: launchd for a release build, else a child; from the server binary beside the app's.
    pub fn for_this_app() -> Option<Start> {
        let bundled = std::env::current_exe().ok()?.with_file_name(BINARY);
        if !bundled.is_file() {
            return None;
        }
        if cfg!(debug_assertions) {
            return Some(Start::Child(bundled));
        }
        Some(Start::Launchd { bundled })
    }

    fn run(&self, dir: &Path, version: u32) -> io::Result<()> {
        let socket = socket(dir, version);
        let log = dir.join(format!("v{version}.log"));
        match self {
            Start::Child(bin) => {
                let mut child = Command::new(bin)
                    .arg("serve")
                    .arg("--socket")
                    .arg(&socket)
                    .args(play_version(version))
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(fs::OpenOptions::new().create(true).append(true).open(log)?)
                    .spawn()?;
                // Reaped once it exits (drained or idle), so it doesn't linger as a zombie of the app.
                thread::spawn(move || child.wait());
                Ok(())
            }
            Start::Launchd { bundled } => {
                let cmd = copied(bundled, dir, version, &socket)?;
                #[cfg(target_os = "macos")]
                let launch = wings_ptyd::launchd::spawn;
                #[cfg(not(target_os = "macos"))]
                let launch = |_: &Command, _: &Path| Err(io::ErrorKind::Unsupported.into());
                start_detached(cmd, &log, launch)
            }
            Start::Direct { bundled } => {
                start_in_own_session(copied(bundled, dir, version, &socket)?, &log)
            }
        }
    }
}

/// The command that serves `socket` from the copy of `bundled` for `version`, as a release app runs it.
fn copied(bundled: &Path, dir: &Path, version: u32, socket: &Path) -> io::Result<Command> {
    let mut cmd = Command::new(install(bundled, dir)?);
    cmd.arg("serve")
        .arg("--socket")
        .arg(socket)
        .arg("--idle-secs")
        .arg(IDLE.as_secs().to_string())
        .args(play_version(version));
    Ok(cmd)
}

/// A debug server's flag to say `version` in its `welcome`, for an app that plays a newer server than this build's
/// (`Servers::connect_as`). Nothing for this build's own.
fn play_version(version: u32) -> Vec<String> {
    if version == SERVER_VERSION {
        Vec::new()
    } else {
        vec!["--server-version".into(), version.to_string()]
    }
}

/// Starts the server `cmd` with `launch`, else directly in a session of its own, which may end with the app: the
/// reason goes to stderr and the server's `log`. Either way it owns its privacy responsibility.
pub fn start_detached(
    cmd: Command,
    log: &Path,
    launch: impl FnOnce(&Command, &Path) -> io::Result<()>,
) -> io::Result<()> {
    let Err(error) = launch(&cmd, log) else {
        return Ok(());
    };
    let reason = format!(
        "Couldn't start the terminal server with launchd, so it starts directly and may end with the app: {error}"
    );
    eprintln!("{reason}");
    writeln!(
        &fs::OpenOptions::new().create(true).append(true).open(log)?,
        "{reason}"
    )?;
    start_in_own_session(cmd, log)
}

/// Starts the server `cmd` in a session of its own, owning its privacy responsibility, with its stderr in `log`.
fn start_in_own_session(mut cmd: Command, log: &Path) -> io::Result<()> {
    let stderr = fs::OpenOptions::new().create(true).append(true).open(log)?;
    #[cfg(target_os = "macos")]
    cmd.arg(wings_ptyd::responsibility::FLAG);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr);
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = cmd.spawn()?;
    // Reaped if it exits while the app runs, so it doesn't linger as a zombie of the app.
    thread::spawn(move || child.wait());
    Ok(())
}

/// The version a socket or an earlier app's binary copy in the server's folder is named for: `v4.sock`,
/// `wings-terminal-server-v4`.
fn version_of(name: &str, prefix: &str, suffix: &str) -> Option<u32> {
    name.strip_prefix(prefix)?
        .strip_suffix(suffix)?
        .parse()
        .ok()
}

/// Deletes the copies earlier apps made under their version, `wings-terminal-server-v4`, of versions older than `ours`
/// that no server in `live` runs from, so each is kept only while its server still has panes.
fn remove_old_copies(dir: &Path, ours: u32, live: &[u32]) {
    let prefix = format!("{BINARY}-v");
    for entry in fs::read_dir(dir.join("bin"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let name = entry.file_name();
        let Some(version) = version_of(&name.to_string_lossy(), &prefix, "") else {
            continue;
        };
        if version < ours
            && !live.contains(&version)
            && let Err(error) = fs::remove_file(entry.path())
        {
            eprintln!(
                "Couldn't delete an older terminal server's copy {}: {error}",
                entry.path().display()
            );
        }
    }
}

/// Copies the bundle's server to `dir/bin/wings-terminal-server`, the same path every time, when the copy there isn't
/// already this one. A server still running from the copy it replaces keeps that file until it exits, as the rename
/// leaves it in place.
pub fn install(bundled: &Path, dir: &Path) -> io::Result<PathBuf> {
    let bin = dir.join("bin");
    private_dir(&bin)?;
    let program = bin.join(BINARY);
    let bytes = fs::read(bundled)?;
    if fs::read(&program).is_ok_and(|copied| copied == bytes) {
        return Ok(program);
    }
    // A file of its own: the app checks access and starts a server on other threads, which may install at once.
    let mut partial = tempfile::NamedTempFile::new_in(&bin)?;
    partial.write_all(&bytes)?;
    fs::set_permissions(partial.path(), fs::Permissions::from_mode(0o700))?;
    partial.persist(&program).map_err(|error| error.error)?;
    Ok(program)
}

/// Whether programs in the server's panes can read what macOS keeps for Full Disk Access, such as other apps' data:
/// the server's copy checks under its own privacy responsibility, which its panes have (`check-access` in its `main`).
/// `None` when this app's server keeps the app's responsibility, or the check couldn't tell. It starts a process, so
/// call it off the main thread.
#[cfg(target_os = "macos")]
pub fn full_disk_access(start: &Start, dir: &Path) -> Option<bool> {
    let (Start::Launchd { bundled } | Start::Direct { bundled }) = start else {
        return None;
    };
    let program = install(bundled, dir).ok()?;
    let status = Command::new(program)
        .arg("check-access")
        .arg(wings_ptyd::responsibility::FLAG)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()?;
    match status.code() {
        Some(0) => Some(true),
        Some(1) => Some(false),
        _ => None,
    }
}

/// The servers this app uses: the one new panes start in, and the panes all of them ran when the app started, until
/// a saved pane claims each.
pub struct Servers {
    /// Where new panes go while `on`. `None` runs them in the app.
    current: Mutex<Option<PathBuf>>,
    /// New panes start in a server. Off, they run in the app, and the panes servers already run are still attached.
    on: AtomicBool,
    found: Mutex<Vec<(PathBuf, PaneInfo)>>,
    /// The folder, the app's server version and how a server starts, to start another when the current one is gone.
    dir: PathBuf,
    version: u32,
    start: Option<Start>,
}

impl Servers {
    /// Every server in `dir`: older ones are told to drain, and the app's own version is started with `start` when
    /// no server as new answers.
    pub fn connect(dir: &Path, start: Option<&Start>) -> Servers {
        Servers::open(dir, start, SERVER_VERSION, true)
    }

    /// `connect` for an app whose server is `version`.
    pub fn connect_as(dir: &Path, start: Option<&Start>, version: u32) -> Servers {
        Servers::open(dir, start, version, true)
    }

    /// `connect` for an app with the server turned off: it finds the panes servers run, and starts none until it's
    /// turned on (`set_on`, then `start_if_none`).
    pub fn connect_off(dir: &Path, start: Option<&Start>) -> Servers {
        Servers::open(dir, start, SERVER_VERSION, false)
    }

    fn open(dir: &Path, start: Option<&Start>, version: u32, on: bool) -> Servers {
        let ours = version;
        let mut found = Vec::new();
        let mut current: Option<(u32, PathBuf)> = None;
        let mut live = Vec::new();
        if let Err(error) = private_dir(dir) {
            eprintln!(
                "Couldn't make the terminal server's folder {}: {error}",
                dir.display()
            );
            return Servers {
                current: Mutex::new(None),
                on: AtomicBool::new(on),
                found: Mutex::new(found),
                dir: dir.to_owned(),
                version,
                start: start.cloned(),
            };
        }
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !(name.starts_with('v') && name.ends_with(".sock")) {
                continue;
            }
            // A socket left by a server that crashed answers nothing; the next server there replaces it.
            let Ok(mut control) = Control::connect(&path) else {
                continue;
            };
            let version = control.welcome().server_version;
            live.extend(version_of(&name, "v", ".sock"));
            if version < ours {
                if let Err(error) = control.drain() {
                    eprintln!(
                        "Couldn't drain the terminal server at {}: {error}",
                        path.display()
                    );
                }
            } else if current.as_ref().is_none_or(|(newest, _)| version > *newest) {
                current = Some((version, path.clone()));
            }
            match control.list() {
                Ok(panes) => {
                    for pane in panes {
                        if pane.space_id != LOOSE {
                            found.push((path.clone(), pane));
                        } else if let Err(error) = control.kill(pane.pane) {
                            eprintln!(
                                "Couldn't end a terminal an earlier app left running: {error}"
                            );
                        }
                    }
                }
                Err(error) => eprintln!(
                    "Couldn't list the terminal server's panes at {}: {error}",
                    path.display()
                ),
            }
        }
        remove_old_copies(dir, ours, &live);
        let servers = Servers {
            current: Mutex::new(current.map(|(_, path)| path)),
            on: AtomicBool::new(on),
            found: Mutex::new(found),
            dir: dir.to_owned(),
            version: ours,
            start: start.cloned(),
        };
        if on && servers.current().is_none() {
            *servers.current.lock().unwrap() = servers.start_one();
        }
        servers
    }

    /// Whether new panes start in a server.
    pub fn is_on(&self) -> bool {
        self.on.load(Ordering::Relaxed)
    }

    /// Turns the server on or off for new panes. Turned off, the panes servers run keep running until they're closed.
    /// Turned on, new panes run in the app until a server answers: `start_if_none` starts one.
    pub fn set_on(&self, on: bool) {
        self.on.store(on, Ordering::Relaxed);
    }

    /// Starts the app's own server when none is current, as an app turned off at launch has none. It can take a few
    /// seconds, so call it off the main thread.
    pub fn start_if_none(&self) {
        if self.current.lock().unwrap().is_some() {
            return;
        }
        let started = self.start_one();
        let mut current = self.current.lock().unwrap();
        if current.is_none() {
            *current = started;
        }
    }

    /// Starts the app's own version, if this app starts servers. `None` when it doesn't, or the server didn't come up.
    fn start_one(&self) -> Option<PathBuf> {
        let start = self.start.as_ref()?;
        let socket = socket(&self.dir, self.version);
        match start
            .run(&self.dir, self.version)
            .and_then(|()| wait(&socket))
        {
            Ok(()) => Some(socket),
            Err(error) => {
                eprintln!("Couldn't start the terminal server, so panes run in the app: {error}");
                None
            }
        }
    }

    /// The copy of the server that Full Disk Access is granted to, when the server owns its privacy responsibility.
    pub fn program(&self) -> Option<PathBuf> {
        matches!(
            self.start,
            Some(Start::Launchd { .. } | Start::Direct { .. })
        )
        .then(|| self.dir.join("bin").join(BINARY))
    }

    /// `full_disk_access` for this app's server.
    #[cfg(target_os = "macos")]
    pub fn full_disk_access(&self) -> Option<bool> {
        full_disk_access(self.start.as_ref()?, &self.dir)
    }

    /// The server new panes start in, if any. `None` while turned off.
    pub fn current(&self) -> Option<PathBuf> {
        if !self.is_on() {
            return None;
        }
        self.current.lock().unwrap().clone()
    }

    /// The server at `gone` didn't answer: a new one starts, as when the app found none, and new panes go there. Panes
    /// that ran in the old server are gone with it. `None` when this app starts no server or the new one didn't come
    /// up; panes then run in the app.
    pub fn replace(&self, gone: &Path) -> Option<PathBuf> {
        let mut current = self.current.lock().unwrap();
        // Another pane found it gone first and started the next one, at the same path when it's the same version.
        if current
            .as_deref()
            .is_some_and(|now| now != gone || Control::connect(now).is_ok())
        {
            return current.clone();
        }
        *current = self.start_one();
        current.clone()
    }

    /// Takes the pane a server runs for the saved pane `history_id`, once.
    pub fn claim(&self, history_id: &str) -> Option<(PathBuf, PaneInfo)> {
        let mut found = self.found.lock().unwrap();
        let at = found
            .iter()
            .position(|(_, pane)| pane.history_id.as_deref() == Some(history_id))?;
        Some(found.remove(at))
    }

    /// `claim`, only while the pane's program still runs: a pane that ended is left for `claim`, which shows its output
    /// above a new shell.
    pub fn claim_running(&self, history_id: &str) -> Option<(PathBuf, PaneInfo)> {
        let mut found = self.found.lock().unwrap();
        let at = found
            .iter()
            .position(|(_, pane)| !pane.exited && pane.history_id.as_deref() == Some(history_id))?;
        Some(found.remove(at))
    }

    /// The history ids of the panes of project `space_id` still running that no saved pane names, and so no saved
    /// pane will claim: a pane started just before the app crashed, or closed in an app that ran without the server.
    pub fn orphans(&self, space_id: &str, saved: &[String]) -> Vec<String> {
        self.found
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, pane)| !pane.exited && pane.space_id == space_id)
            .filter_map(|(_, pane)| pane.history_id.clone())
            .filter(|id| !saved.contains(id))
            .collect()
    }
}

/// Waits for a server to answer at `socket`.
fn wait(socket: &Path) -> io::Result<()> {
    let deadline = Instant::now() + STARTUP;
    loop {
        match Control::connect(socket) {
            Ok(_) => return Ok(()),
            Err(error) if Instant::now() >= deadline => return Err(error),
            Err(_) => thread::sleep(Duration::from_millis(20)),
        }
    }
}

/// Ends the servers in `dir` and every pane they run: for a folder no app reads any more, like the side-by-side app's
/// once the replacement took over its data. Their panes can't come back, so they'd otherwise run until a reboot.
pub fn end_all(dir: &Path) {
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !(name.starts_with('v') && name.ends_with(".sock")) {
            continue;
        }
        let Ok(mut control) = Control::connect(&entry.path()) else {
            continue;
        };
        // Drained first, so it starts no pane while this ends the others, and exits once it has none.
        let ended = control
            .drain()
            .and_then(|()| control.list())
            .and_then(|panes| panes.iter().try_for_each(|pane| control.kill(pane.pane)));
        if let Err(error) = ended {
            eprintln!(
                "Couldn't end the terminal server at {}: {error}",
                entry.path().display()
            );
        }
    }
}

/// Ends a pane's program in its server. The pane is already gone from the app.
pub fn kill(socket: &Path, pane: u32) {
    if let Err(error) = Control::connect(socket).and_then(|mut control| control.kill(pane)) {
        eprintln!("Couldn't end a pane in the terminal server: {error}");
    }
}

/// The output a server kept for a pane, oldest first.
pub fn read(socket: &Path, pane: u32) -> io::Result<Vec<u8>> {
    Control::connect(socket)?.read(pane)
}
