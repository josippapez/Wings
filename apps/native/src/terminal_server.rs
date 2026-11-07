//! Terminals that outlive the app. With `WINGS_TERMINAL_SERVER=1`, panes run in `wings-terminal-server` (the
//! `wings-ptyd` crate), a process of its own that the app reaches over a socket in its data folder. When the app comes
//! back after a quit, a crash or an update, each saved pane attaches to its program again by its history id.
//!
//! The server never runs from the app bundle: an update deletes the old bundle, and macOS can't attribute a program
//! whose file is gone, so its panes would lose the privacy grants (Screen Recording, hard links into Desktop). The app
//! copies the server out to `terminal-server/bin/` under its version. A release app on macOS starts that copy as a
//! one-shot launchd job that forks the server off and ends (`wings_ptyd::launchd`), so the server is in no coalition of
//! the app's and lives on when the app quits; no job stays loaded. The server owns its privacy responsibility, and the
//! shells it runs inherit it (`wings_ptyd::responsibility`). When launchd fails, the app starts the server directly in
//! a session of its own, which may end with the app, and says so in the server's log. A debug build runs it as its own
//! child instead, which ends with the app and keeps the app's privacy responsibility.
//!
//! A server is versioned apart from the app (`wings_ptyd::SERVER_VERSION`), with a socket per version. One older than
//! the app's is told to drain: it keeps its panes, which the app still attaches, and exits once they have ended, while
//! new panes go to the app's version. Nothing kills a pane on an upgrade.
//!
//! When no server answers, the app runs panes in its own process as before, and saves them under the same history
//! ids, so the server's panes are still found by the next app that reaches it.

use std::{
    fs,
    io::{self, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

use wings_ptyd::{SERVER_VERSION, client::Control, proto::PaneInfo};

/// Set to `1` to run panes in the terminal server.
pub const ENV: &str = "WINGS_TERMINAL_SERVER";

/// The server binary's name, beside the app's own in its bundle and in the build folder.
pub const BINARY: &str = "wings-terminal-server";

/// A server nobody uses for this long exits, and the next app to want one starts it.
pub const IDLE: Duration = Duration::from_secs(10 * 60);

/// How long the app waits for a server it started to answer.
const STARTUP: Duration = Duration::from_secs(5);

pub fn enabled() -> bool {
    std::env::var_os(ENV).is_some_and(|value| value == "1")
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
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(fs::OpenOptions::new().create(true).append(true).open(log)?)
                    .spawn()?;
                // Reaped once it exits (drained or idle), so it doesn't linger as a zombie of the app.
                thread::spawn(move || child.wait());
                Ok(())
            }
            Start::Launchd { bundled } => {
                let mut cmd = Command::new(install(bundled, dir)?);
                cmd.arg("serve")
                    .arg("--socket")
                    .arg(&socket)
                    .arg("--idle-secs")
                    .arg(IDLE.as_secs().to_string());
                #[cfg(target_os = "macos")]
                let launch = wings_ptyd::launchd::spawn;
                #[cfg(not(target_os = "macos"))]
                let launch = |_: &Command, _: &Path| Err(io::ErrorKind::Unsupported.into());
                start_detached(cmd, &log, launch)
            }
        }
    }
}

/// Starts the server `cmd` with `launch`, else directly in a session of its own, which may end with the app: the
/// reason goes to stderr and the server's `log`. Either way it owns its privacy responsibility.
pub fn start_detached(
    mut cmd: Command,
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
    let stderr = fs::OpenOptions::new().create(true).append(true).open(log)?;
    writeln!(&stderr, "{reason}")?;
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

/// Copies the bundle's server to `dir/bin/` under its version, once: a server that still runs from an earlier copy
/// keeps its file, and its version never names other code (bump `SERVER_VERSION` when the server changes).
pub fn install(bundled: &Path, dir: &Path) -> io::Result<PathBuf> {
    let bin = dir.join("bin");
    private_dir(&bin)?;
    let program = bin.join(format!("{BINARY}-v{SERVER_VERSION}"));
    if program.is_file() {
        return Ok(program);
    }
    let partial = bin.join(format!(
        ".{BINARY}-v{SERVER_VERSION}.{}",
        std::process::id()
    ));
    fs::copy(bundled, &partial)?;
    fs::set_permissions(&partial, fs::Permissions::from_mode(0o700))?;
    fs::rename(&partial, &program)?;
    Ok(program)
}

/// The servers this app uses: the one new panes start in, and the panes all of them ran when the app started, until
/// a saved pane claims each.
pub struct Servers {
    /// Where new panes go. `None` runs them in the app.
    current: Mutex<Option<PathBuf>>,
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
        Servers::connect_as(dir, start, SERVER_VERSION)
    }

    /// `connect` for an app whose server is `version`.
    pub fn connect_as(dir: &Path, start: Option<&Start>, version: u32) -> Servers {
        let ours = version;
        let mut found = Vec::new();
        let mut current: Option<(u32, PathBuf)> = None;
        if let Err(error) = private_dir(dir) {
            eprintln!(
                "Couldn't make the terminal server's folder {}: {error}",
                dir.display()
            );
            return Servers {
                current: Mutex::new(None),
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
                Ok(panes) => found.extend(panes.into_iter().map(|pane| (path.clone(), pane))),
                Err(error) => eprintln!(
                    "Couldn't list the terminal server's panes at {}: {error}",
                    path.display()
                ),
            }
        }
        let servers = Servers {
            current: Mutex::new(current.map(|(_, path)| path)),
            found: Mutex::new(found),
            dir: dir.to_owned(),
            version: ours,
            start: start.cloned(),
        };
        if servers.current().is_none() {
            *servers.current.lock().unwrap() = servers.start_one();
        }
        servers
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

    /// The server new panes start in, if any.
    pub fn current(&self) -> Option<PathBuf> {
        self.current.lock().unwrap().clone()
    }

    /// The server at `gone` didn't answer: a new one starts, as when the app found none, and new panes go there. Panes
    /// that ran in the old server are gone with it. `None` when this app starts no server or the new one didn't come
    /// up; panes then run in the app.
    pub fn replace(&self, gone: &Path) -> Option<PathBuf> {
        let mut current = self.current.lock().unwrap();
        // Another pane found it gone first and started the next one.
        if current.as_deref().is_some_and(|now| now != gone) {
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
