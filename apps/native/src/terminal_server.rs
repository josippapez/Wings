//! Terminals that outlive the app. Panes run in `wings-terminal-server` (the `wings-ptyd` crate), a process of its own
//! that the app reaches over a socket in its data folder. When the app comes back after a quit, a crash or an update,
//! each saved pane attaches to its program again by its history id. Settings turns it off and on for new panes; it's on
//! unless turned off, except in a debug build (`enabled`).
//!
//! A release app carries the server in a helper app of its own, `Contents/Library/Wings Terminals.app`
//! (`scripts/build-mac-app.sh`), which macOS names by its bundle id: in System Settings, Files & Folders lists it with a
//! switch for each app whose data its terminals asked for, as it never does for a bare program (`app_data_access`). The
//! server never runs from the app bundle: an update deletes the old bundle, and macOS can't attribute a program whose
//! file is gone, so its panes would lose the privacy grants (Screen Recording, hard links into Desktop). The app copies
//! the helper out to `terminal-server/`, always under the same name (`install`). A release app on macOS starts that
//! copy as a
//! one-shot launchd job that forks the server off and ends (`wings_ptyd::launchd`), so the server is in no coalition of
//! the app's and lives on when the app quits; no job stays loaded. The server owns its privacy responsibility, and the
//! shells it runs inherit it (`wings_ptyd::responsibility`). When launchd fails, the app starts the server directly in
//! a session of its own, which may end with the app, and says so in the server's log. A debug build runs it as its own
//! child instead, which ends with the app and keeps the app's privacy responsibility.
//!
//! A server is versioned apart from the app (`wings_ptyd::SERVER_VERSION`), with a socket per version. One older than
//! the app's is told to drain: it keeps its panes, which the app still attaches, and exits once they have ended, while
//! new panes go to the app's version. Nothing kills a pane that runs a program on an upgrade: only idle shells on an
//! older server end at launch (`Servers::tidy_older`), and the app moves its panes off one once nothing else runs
//! there (`workspace/older_servers.rs`). A copy the app replaced is put aside, and deleted once no server runs from it
//! (`remove_old_copies`).
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

/// The server binary's name: the helper app's executable, and beside the app's own in the build folder.
pub const BINARY: &str = "wings-terminal-server";

/// The helper app's name, `<HELPER>.app` in the app's `Contents/Library`, as System Settings shows it. The side-by-side
/// app's has its own, as it has its own bundle id. `scripts/build-mac-app.sh` names it the same.
pub const HELPER: &str = if crate::data_dir::REPLACEMENT {
    "Wings Terminals"
} else {
    "Wings-native Terminals"
};

/// The server's executable in the helper app `bundle`.
pub fn executable(bundle: &Path) -> PathBuf {
    bundle.join("Contents/MacOS").join(BINARY)
}

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
    /// A release app: copy the helper app `bundled` out of the bundle and start the copy with a one-shot launchd job,
    /// or directly in a session of its own when launchd fails.
    Launchd { bundled: PathBuf },
    /// A plain child of this process, which ends with it: debug builds and tests.
    Child(PathBuf),
    /// `Launchd` without launchd: the copy of `bundled` starts directly in a session of its own, as when launchd
    /// fails. Tests of an update use it, which must never load a launchd job.
    Direct { bundled: PathBuf },
}

impl Start {
    /// What this app does: a release build starts the helper app in its bundle with launchd, a debug build the server
    /// binary beside its own as a child. `None` without one, as a release build run outside its bundle.
    pub fn for_this_app() -> Option<Start> {
        let exe = std::env::current_exe().ok()?;
        if cfg!(debug_assertions) {
            let bin = exe.with_file_name(BINARY);
            return bin.is_file().then_some(Start::Child(bin));
        }
        // `Contents/MacOS/wings-native` to `Contents/Library/<HELPER>.app`.
        let bundled = exe
            .parent()?
            .parent()?
            .join("Library")
            .join(format!("{HELPER}.app"));
        executable(&bundled)
            .is_file()
            .then_some(Start::Launchd { bundled })
    }

    fn run(&self, dir: &Path, version: u32) -> io::Result<()> {
        let socket = socket(dir, version);
        let log = dir.join(format!("v{version}.log"));
        // A marker from a server that ran here before says nothing about this one.
        let _ = fs::remove_file(direct_marker(&log));
        match self {
            Start::Child(bin) => {
                let mut child = Command::new(bin)
                    .arg("serve")
                    .arg("--socket")
                    .arg(&socket)
                    .args(play_version(version))
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(
                        fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&log)?,
                    )
                    .spawn()?;
                // A child of the app, it may end with it.
                fs::write(direct_marker(&log), child.id().to_string())?;
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
    // Its pid stays the same when it execs itself to disclaim (`wings_ptyd::responsibility`).
    fs::write(direct_marker(log), child.id().to_string())?;
    // Reaped if it exits while the app runs, so it doesn't linger as a zombie of the app.
    thread::spawn(move || child.wait());
    Ok(())
}

/// The file a server started without launchd leaves beside its `log`, holding its process id, so an app can tell
/// it may end with the app that started it (`started_directly`).
fn direct_marker(log: &Path) -> PathBuf {
    log.with_extension("direct")
}

/// Whether the server at `socket` was started without launchd, as a child of an app or in a session of its own, so it
/// may end when that app quits. Told by the marker its start left (`direct_marker`), when it names this server.
pub fn started_directly(socket: &Path) -> bool {
    let Some(pid) = fs::read_to_string(direct_marker(socket))
        .ok()
        .and_then(|pid| pid.trim().parse::<u32>().ok())
    else {
        return false;
    };
    #[cfg(target_os = "macos")]
    return Control::connect(socket)
        .and_then(|control| control.server_pid())
        .is_ok_and(|running| running == pid);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pid;
        true
    }
}

/// The server version a socket is for, from its name, `v<version>.sock` (`socket`).
pub fn socket_version(socket: &Path) -> Option<u32> {
    socket
        .file_name()?
        .to_str()?
        .strip_prefix('v')?
        .strip_suffix(".sock")?
        .parse()
        .ok()
}

/// Whether the pane's shell sits at its prompt with nothing running: no program in front, and no job sent to the
/// background. Anything that can't be told counts as running.
pub fn idle_shell(pane: &PaneInfo) -> bool {
    !pane.exited
        && pane.shell_pid.is_some()
        && pane.fg == pane.shell_pid
        && pane.shell_pid.is_some_and(|shell| !has_jobs(shell))
}

/// Whether the shell `pid` has a child in a process group other than its own: a job it sent to the background. What its
/// prompt runs stays in its own group and comes and goes as the prompt draws, so it doesn't count.
#[cfg(target_os = "macos")]
fn has_jobs(pid: u32) -> bool {
    let mut pids = [0 as libc::pid_t; 64];
    let size = std::mem::size_of_val(&pids) as libc::c_int;
    let got =
        unsafe { libc::proc_listchildpids(pid as libc::pid_t, pids.as_mut_ptr().cast(), size) };
    if got < 0 {
        return true;
    }
    pids.iter()
        .take(got as usize)
        .filter(|&&child| child > 0)
        .any(|&child| {
            let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
            let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
            let read = unsafe {
                libc::proc_pidinfo(
                    child,
                    libc::PROC_PIDTBSDINFO,
                    0,
                    (&raw mut info).cast(),
                    size,
                )
            };
            // A child that ended meanwhile can't be read, and is no job.
            read == size && info.pbi_pgid != pid
        })
}

#[cfg(not(target_os = "macos"))]
fn has_jobs(_: u32) -> bool {
    true
}

/// The file the server answering `control` runs from. `None` when it can't be told.
#[cfg(target_os = "macos")]
fn running_from(control: &Control) -> Option<PathBuf> {
    let pid = control.server_pid().ok()?;
    let mut path = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let count =
        unsafe { libc::proc_pidpath(pid as i32, path.as_mut_ptr().cast(), path.len() as u32) };
    use std::os::unix::ffi::OsStrExt;
    (count > 0).then(|| PathBuf::from(std::ffi::OsStr::from_bytes(&path[..count as usize])))
}

#[cfg(not(target_os = "macos"))]
fn running_from(_: &Control) -> Option<PathBuf> {
    None
}

/// Deletes the copies of the server no server in `running` runs from: the bundles `install` put aside in `replaced/`,
/// and the bare binaries apps before the helper app copied to `bin/`. Each is kept while a server still runs from it,
/// as a drained one does until its panes end. `None`, when a running server's file couldn't be told, keeps them all.
fn remove_old_copies(dir: &Path, running: Option<&[PathBuf]>) {
    let Some(running) = running else {
        return;
    };
    for folder in ["bin", "replaced"].map(|folder| dir.join(folder)) {
        for entry in fs::read_dir(&folder).into_iter().flatten().flatten() {
            let path = entry.path();
            let path = path.canonicalize().unwrap_or(path);
            if running.iter().any(|exe| exe.starts_with(&path)) {
                continue;
            }
            let removed = if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
            if let Err(error) = removed {
                eprintln!(
                    "Couldn't delete an older terminal server's copy {}: {error}",
                    path.display()
                );
            }
        }
        // Gone once empty.
        let _ = fs::remove_dir(&folder);
    }
}

/// One install at a time: the app checks access and starts a server on other threads, which may install at once.
static INSTALLING: Mutex<()> = Mutex::new(());

/// Copies the helper app `bundled` to `dir`, under its own name, the same path every time, when the copy there isn't
/// already this one, and returns the server's executable in it. A server still running from the copy it replaces keeps
/// its files: the copy is moved aside to `replaced/`, and deleted once no server runs from it (`remove_old_copies`).
pub fn install(bundled: &Path, dir: &Path) -> io::Result<PathBuf> {
    let _installing = INSTALLING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    private_dir(dir)?;
    let name = bundled
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the helper app has no name"))?;
    let copy = dir.join(name);
    // The executable carries the signature, which covers the rest of the bundle; `Info.plist` names it to macOS.
    let same = |file: &Path| fs::read(bundled.join(file)).ok() == fs::read(copy.join(file)).ok();
    if same(&Path::new("Contents/MacOS").join(BINARY)) && same(Path::new("Contents/Info.plist")) {
        return Ok(executable(&copy));
    }
    let staging = tempfile::Builder::new()
        .prefix(".install-")
        .tempdir_in(dir)?;
    let staged = staging.path().join(name);
    copy_dir(bundled, &staged)?;
    if copy.exists() {
        let replaced = dir.join("replaced");
        fs::create_dir_all(&replaced)?;
        let aside = tempfile::Builder::new().tempdir_in(&replaced)?.keep();
        fs::rename(&copy, aside.join(name))?;
    }
    fs::rename(&staged, &copy)?;
    Ok(executable(&copy))
}

/// Copies the folder `from` to `to`, which mustn't exist yet.
fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Whether programs in the server's panes can read other apps' data, which macOS refuses unless that app's data is
/// turned on for the helper app in Files & Folders: the server's copy checks Chrome's under its own privacy
/// responsibility, which its panes have (`check-access` in its `main`). `None` when this app's server keeps the app's
/// responsibility, or the check couldn't tell, as without Chrome. It starts a process, so call it off the main thread.
#[cfg(target_os = "macos")]
pub fn app_data_access(start: &Start, dir: &Path) -> Option<bool> {
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
    /// What the saved panes whose idle shells an older server ran printed, by history id, once `tidy_older` ended
    /// them: each pane starts again in the current server below it.
    retired: Mutex<Vec<(String, Vec<u8>)>>,
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
        // The file each answering server runs from, `None` once one can't be told.
        let mut running = Some(Vec::new());
        if let Err(error) = private_dir(dir) {
            eprintln!(
                "Couldn't make the terminal server's folder {}: {error}",
                dir.display()
            );
            return Servers {
                current: Mutex::new(None),
                on: AtomicBool::new(on),
                found: Mutex::new(found),
                retired: Mutex::default(),
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
            match (running_from(&control), running.as_mut()) {
                (Some(exe), Some(running)) => running.push(exe),
                _ => running = None,
            }
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
        remove_old_copies(dir, running.as_deref());
        let servers = Servers {
            current: Mutex::new(current.map(|(_, path)| path)),
            on: AtomicBool::new(on),
            found: Mutex::new(found),
            retired: Mutex::default(),
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

    /// Whether this app's server owns its privacy responsibility, so Files & Folders lists it as the helper app.
    pub fn owns_responsibility(&self) -> bool {
        matches!(
            self.start,
            Some(Start::Launchd { .. } | Start::Direct { .. })
        )
    }

    /// `app_data_access` for this app's server.
    #[cfg(target_os = "macos")]
    pub fn app_data_access(&self) -> Option<bool> {
        app_data_access(self.start.as_ref()?, &self.dir)
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

    /// The output `tidy_older` kept of the saved pane `history`, once, for its new shell.
    pub fn claim_retired(&self, history_id: &str) -> Option<Vec<u8>> {
        let mut retired = self.retired.lock().unwrap();
        let at = retired.iter().position(|(id, _)| id == history_id)?;
        Some(retired.remove(at).1)
    }

    /// Whether the server at `socket` is older than this app's: one told to drain.
    pub fn is_older(&self, socket: &Path) -> bool {
        socket_version(socket).is_some_and(|version| version < self.version)
    }

    /// Every server that answers in the folder, newest first, with the panes whose programs still run.
    pub fn running(&self) -> Vec<Running> {
        let mut running: Vec<Running> = fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let socket = entry.path();
                socket_version(&socket)?;
                let mut control = Control::connect(&socket).ok()?;
                let version = control.welcome().server_version;
                let panes = control
                    .list()
                    .ok()?
                    .into_iter()
                    .filter(|pane| !pane.exited)
                    .collect();
                Some(Running {
                    socket,
                    version,
                    panes,
                })
            })
            .collect();
        running.sort_by_key(|server| std::cmp::Reverse(server.version));
        running
    }

    /// Ends an older server and every pane it runs (`end`). The current one is never stopped this way.
    pub fn stop(&self, socket: &Path) -> io::Result<()> {
        if !self.is_older(socket) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "only an older terminal server can be stopped",
            ));
        }
        end(socket)?;
        self.found.lock().unwrap().retain(|(at, _)| at != socket);
        Ok(())
    }

    /// At launch, before any pane claims its own: ends what older servers run that nobody would miss, so they drain
    /// and exit. An idle shell (`idle_shell`) no saved pane in `saved` names ends. When every pane an older server
    /// runs is an idle shell, they all end, and the saved panes among them start again in the current server, below
    /// what they printed (`claim_retired`). A pane that runs a program is never ended. Nothing ends without a current
    /// server to start them in.
    pub fn tidy_older(&self, saved: &[String]) {
        if self.current().is_none() {
            return;
        }
        let mut found = self.found.lock().unwrap();
        let mut older: Vec<PathBuf> = found
            .iter()
            .map(|(socket, _)| socket.clone())
            .filter(|socket| self.is_older(socket))
            .collect();
        older.sort();
        older.dedup();
        for socket in older {
            let running: Vec<&PaneInfo> = found
                .iter()
                .filter(|(at, pane)| *at == socket && !pane.exited)
                .map(|(_, pane)| pane)
                .collect();
            let all_idle = running.iter().all(|pane| idle_shell(pane));
            let ending: Vec<PaneInfo> = running
                .into_iter()
                .filter(|pane| idle_shell(pane))
                .filter(|pane| {
                    all_idle
                        || !pane
                            .history_id
                            .as_ref()
                            .is_some_and(|id| saved.contains(id))
                })
                .cloned()
                .collect();
            if ending.is_empty() {
                continue;
            }
            let Ok(mut control) = Control::connect(&socket) else {
                continue;
            };
            for pane in &ending {
                let kept = pane
                    .history_id
                    .as_ref()
                    .filter(|id| saved.contains(id))
                    .map(|id| (id.clone(), control.read(pane.pane).unwrap_or_default()));
                if let Err(error) = control.kill(pane.pane) {
                    eprintln!("Couldn't end an idle terminal on an older terminal server: {error}");
                    continue;
                }
                self.retired.lock().unwrap().extend(kept);
                found.retain(|(at, other)| !(*at == socket && other.pane == pane.pane));
            }
        }
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
        if let Err(error) = end(&entry.path()) {
            eprintln!(
                "Couldn't end the terminal server at {}: {error}",
                entry.path().display()
            );
        }
    }
}

/// Ends the server at `socket` and every pane it runs. It exits once its panes have ended.
pub fn end(socket: &Path) -> io::Result<()> {
    let mut control = Control::connect(socket)?;
    // Drained first, so it starts no pane while this ends the others, and exits once it has none.
    control.drain()?;
    for pane in control.list()? {
        control.kill(pane.pane)?;
    }
    Ok(())
}

/// A server that answers in the app's folder, as Settings lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Running {
    pub socket: PathBuf,
    pub version: u32,
    /// The panes whose programs still run.
    pub panes: Vec<PaneInfo>,
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
