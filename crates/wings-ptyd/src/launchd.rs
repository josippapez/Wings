//! How a release app starts the terminal server on macOS so it outlives the app: a one-shot launchd job. A process the
//! app starts itself stays in the app's coalition even after `setsid`, and macOS can end that coalition when the app
//! quits. A launchd job that stays the server died about a second after the app quit in a spike; one that forks the
//! server and exits survives. So the job runs the server binary with `SENTINEL`, which forks, lets the job's process
//! exit, and in the child calls `setsid` and execs the server again (owning its own privacy responsibility,
//! `responsibility.rs`). The new server connects to a `ready` socket the app made, the app removes the one-shot job
//! and sends it the app's environment and resource limits (launchd would give it its own), and the server acks and
//! serves. No job stays loaded and no file stays in `~/Library/LaunchAgents`.
//!
//! Adapted from herdr-core's `platform/macos_launchd.rs` (Apache-2.0, see THIRD-PARTY-NOTICES).

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStringExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Set by the job's plist to the `ready` socket: the job's process forks and detaches.
const DETACH: &str = "WINGS_TERMINAL_SERVER_LAUNCHD_DETACH";
/// The job's first argument. A server binary that predates it rejects the job instead of serving with launchd's
/// environment.
pub const SENTINEL: &str = "--wings-launchd";
/// After `SENTINEL` in the detached server, before the `ready` socket.
const READY: &str = "--ready";
const LAUNCHCTL: &str = "/bin/launchctl";
/// Every job's label starts with this, then the app's pid and the start's nanoseconds.
pub const PREFIX: &str = "dev.wings.native.terminal-server.";
/// The folders with a job's plist and `ready` socket, in `/tmp`.
const DIR_PREFIX: &str = "wings-ts-launchd-";
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// A job or folder this old whose starter is gone was abandoned (the app crashed during a start).
const STALE: Duration = Duration::from_secs(60);

/// What the server gets from the app instead of launchd's defaults: the app's environment with the command's changes,
/// and every resource limit.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Inherited {
    pub env: Vec<(Vec<u8>, Vec<u8>)>,
    pub limits: Vec<(libc::c_int, libc::rlim_t, libc::rlim_t)>,
}

impl Inherited {
    pub fn capture(cmd: &Command) -> io::Result<Self> {
        let mut env: BTreeMap<_, _> = std::env::vars_os().collect();
        for (key, value) in cmd.get_envs() {
            match value {
                Some(value) => {
                    env.insert(key.to_owned(), value.to_owned());
                }
                None => {
                    env.remove(key);
                }
            }
        }
        env.remove(OsStr::new(DETACH));
        env.insert("XPC_SERVICE_NAME".into(), "0".into());
        let mut limits = Vec::new();
        // Darwin's RLIM_NLIMITS is not stable across OS versions. Ask the running kernel for every slot rather than
        // baking in a header's count.
        for resource in 0.. {
            let mut limit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
            if unsafe { libc::getrlimit(resource, &mut limit) } != 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::EINVAL) {
                    break;
                }
                return Err(error);
            }
            limits.push((resource, limit.rlim_cur, limit.rlim_max));
        }
        Ok(Self { env: env.into_iter().map(|(k, v)| (k.into_vec(), v.into_vec())).collect(), limits })
    }

    /// Runs only in the newly detached server, before it starts any thread.
    fn restore(self) -> io::Result<()> {
        for (resource, soft, hard) in self.limits {
            let mut current = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
            if unsafe { libc::getrlimit(resource, &mut current) } != 0 {
                return Err(io::Error::last_os_error());
            }
            // Darwin can clamp NPROC even when setting its inherited value. Don't rewrite a limit that launchd
            // already preserved exactly.
            if (current.rlim_cur, current.rlim_max) == (soft, hard) {
                continue;
            }
            let limit = libc::rlimit { rlim_cur: soft, rlim_max: hard };
            if unsafe { libc::setrlimit(resource, &limit) } != 0 {
                return Err(io::Error::other(format!("cannot restore resource limit {resource}: {}", io::Error::last_os_error())));
            }
            if unsafe { libc::getrlimit(resource, &mut current) } != 0 {
                return Err(io::Error::last_os_error());
            }
            if (current.rlim_cur, current.rlim_max) != (soft, hard) {
                return Err(io::Error::other(format!("resource limit {resource} was clamped instead of inherited")));
            }
        }
        for (key, _) in std::env::vars_os() {
            std::env::remove_var(key);
        }
        for (key, value) in self.env {
            std::env::set_var(OsString::from_vec(key), OsString::from_vec(value));
        }
        Ok(())
    }
}

/// The first thing the server binary does. `None` when launchd didn't start it. In the job's process it detaches and
/// never returns. In the detached server it takes the app's environment and returns the arguments to serve with.
pub fn startup() -> Option<Vec<OsString>> {
    let args: Vec<OsString> = std::env::args_os().collect();
    if args.get(1).map(OsString::as_os_str) != Some(OsStr::new(SENTINEL)) {
        return None;
    }
    let result = if let Some(socket) = std::env::var_os(DETACH) {
        detach(Path::new(&socket), &args[2..]).map(|()| Vec::new())
    } else if args.get(2).map(OsString::as_os_str) == Some(OsStr::new(READY)) && args.len() > 3 {
        receive(Path::new(&args[3])).map(|()| args[4..].to_vec())
    } else {
        Err(io::Error::new(io::ErrorKind::InvalidInput, "started by launchd without its socket"))
    };
    match result {
        Ok(args) => Some(args),
        Err(error) => {
            eprintln!("wings-terminal-server: launchd detach failed: {error}");
            std::process::exit(1);
        }
    }
}

/// Starts `cmd` (the server binary and its arguments) as a one-shot launchd job that detaches from it. Returns once
/// the server has its environment, before it answers on its socket. `log` gets the server's stderr.
pub fn spawn(cmd: &Command, log: &Path) -> io::Result<()> {
    let domain = format!("gui/{}", unsafe { libc::getuid() });
    launch(cmd, log, Path::new("/tmp"), |job, file| {
        remove_stale(&domain, job.started);
        let bootstrapped = launchctl(&[OsStr::new("bootstrap"), OsStr::new(&domain), file.as_os_str()]);
        if let Err(error) = bootstrapped {
            // A failed bootstrap normally creates no job; clean a partial one quietly without hiding the error.
            let _ = launchctl(&[OsStr::new("bootout"), OsStr::new(&format!("{domain}/{}", job.label))]);
            return Err(error);
        }
        job.target = Some(format!("{domain}/{}", job.label));
        Ok(())
    })
}

/// `spawn` with the job's program run as a plain child instead of under launchd, with launchd's empty environment:
/// the same fork, detach and handshake, for tests that mustn't load a launchd job.
pub fn spawn_without_launchd(cmd: &Command, log: &Path, root: &Path) -> io::Result<()> {
    launch(cmd, log, root, |job, _| {
        let mut job_cmd = Command::new(cmd.get_program());
        if let Some(dir) = cmd.get_current_dir() {
            job_cmd.current_dir(dir);
        }
        let mut child = job_cmd
            .arg(SENTINEL)
            .args(cmd.get_args())
            .env_clear()
            .env(DETACH, &job.socket)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::OpenOptions::new().append(true).open(log)?)
            .spawn()?;
        std::thread::spawn(move || child.wait());
        Ok(())
    })
}

/// A start's folder with its plist and `ready` socket, and the job launchd loaded from it.
struct Job {
    dir: PathBuf,
    socket: PathBuf,
    label: String,
    started: Duration,
    /// The loaded job, `gui/<uid>/<label>`, until it is booted out.
    target: Option<String>,
}

impl Job {
    fn remove(&mut self) -> io::Result<()> {
        if let Some(target) = self.target.take() {
            launchctl(&[OsStr::new("bootout"), OsStr::new(&target)])?;
        }
        Ok(())
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        if let Err(error) = self.remove() {
            eprintln!("Couldn't remove the terminal server's launchd job {}: {error}", self.label);
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn launch(cmd: &Command, log: &Path, root: &Path, start: impl FnOnce(&mut Job, &Path) -> io::Result<()>) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let inherited = Inherited::capture(cmd)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(io::Error::other)?;
    remove_stale_dirs(root, now);
    let pid = std::process::id();
    let dir = root.join(format!("{DIR_PREFIX}{pid}-{}-{}", now.as_nanos(), random_hex()));
    std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
    let socket = dir.join("ready");
    let mut job = Job { dir, socket, label: label(pid, now), started: now, target: None };
    let listener = UnixListener::bind(&job.socket)?;
    listener.set_nonblocking(true)?;
    let mask = unsafe {
        let mask = libc::umask(0o077);
        libc::umask(mask);
        mask
    };
    // Create the log here: the same owner, mask and append as a direct start, before launchd opens it.
    std::fs::OpenOptions::new().create(true).append(true).open(log)?;
    let contents = plist(cmd, log, &job.label, &job.socket, mask)?;
    let file = job.dir.join("job.plist");
    std::fs::OpenOptions::new().create_new(true).write(true).mode(0o600).open(&file)?.write_all(contents.as_bytes())?;
    let started = start(&mut job, &file);
    let _ = std::fs::remove_file(&file);
    started?;
    let deadline = Instant::now() + HANDSHAKE_TIMEOUT;
    loop {
        match listener.accept() {
            Ok((mut ready, _)) => {
                // Darwin inherits O_NONBLOCK from the listener on accept.
                ready.set_nonblocking(false)?;
                ready.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
                ready.set_write_timeout(Some(HANDSHAKE_TIMEOUT))?;
                // The server has forked and seen the job's process exit. Remove the one-shot job before it serves.
                job.remove()?;
                serde_json::to_writer(&mut ready, &inherited)?;
                ready.shutdown(std::net::Shutdown::Write)?;
                let mut ack = [0];
                ready.read_exact(&mut ack)?;
                return Ok(());
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "the launchd job didn't complete the detach handshake within 10 s (it may be an older binary)",
                ));
            }
            Err(e) => return Err(e),
        }
    }
}

/// In the job's process: fork, let the job's process exit, and exec the server again in a session of its own.
fn detach(socket: &Path, args: &[OsString]) -> io::Result<()> {
    let mut cmd = Command::new(std::env::current_exe()?);
    cmd.arg(SENTINEL).arg(READY).arg(socket).args(args).env_remove(DETACH);
    match unsafe { libc::fork() } {
        -1 => return Err(io::Error::last_os_error()),
        0 => {}
        _ => unsafe { libc::_exit(0) },
    }
    if unsafe { libc::setsid() } < 0 {
        return Err(io::Error::last_os_error());
    }
    // libSystem can initialize libnotify before main. fork disables it in the child; exec gives the detached server
    // fresh libnotify/libdispatch state.
    crate::responsibility::exec(cmd)
}

/// In the detached server: wait for the job's process to be gone, then take the app's environment.
fn receive(socket: &Path) -> io::Result<()> {
    let deadline = Instant::now() + HANDSHAKE_TIMEOUT;
    while unsafe { libc::getppid() } != 1 {
        if Instant::now() >= deadline {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "the launcher didn't exit"));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let mut ready = UnixStream::connect(socket)?;
    ready.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
    let inherited: Inherited = serde_json::from_reader(&mut ready)?;
    inherited.restore()?;
    ready.write_all(&[1])
}

/// launchd's name for the job of the start at `started` by the app with `pid`.
pub fn label(pid: u32, started: Duration) -> String {
    format!("{PREFIX}{pid}.{}", started.as_nanos())
}

fn launchctl(args: &[&OsStr]) -> io::Result<String> {
    let out = Command::new(LAUNCHCTL).args(args).stdin(Stdio::null()).output()?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(io::Error::other(format!("launchctl {}: {} ({})", args[0].to_string_lossy(), String::from_utf8_lossy(&out.stderr).trim(), out.status)))
    }
}

fn remove_stale(domain: &str, now: Duration) {
    let Ok(list) = launchctl(&[OsStr::new("list")]) else { return };
    for label in stale_labels(&list, now) {
        let target = format!("{domain}/{label}");
        if let Err(error) = launchctl(&[OsStr::new("bootout"), OsStr::new(&target)]) {
            eprintln!("Couldn't remove the finished launchd job {target}: {error}");
        }
    }
}

/// Removes the folders in `root` of starts whose app is gone, a minute or more after they began.
pub fn remove_stale_dirs(root: &Path, now: Duration) {
    use std::os::unix::fs::MetadataExt;
    for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        if !meta.is_dir() || meta.uid() != unsafe { libc::getuid() } {
            continue;
        }
        let name = entry.file_name();
        let Some(attempt) = name.to_str().and_then(|name| name.strip_prefix(DIR_PREFIX)) else { continue };
        let mut fields = attempt.splitn(3, '-');
        let Some(pid) = fields.next().and_then(|p| p.parse::<libc::pid_t>().ok()).filter(|pid| *pid > 0) else {
            continue;
        };
        let Some(stamp) = fields.next().and_then(|s| s.parse::<u128>().ok()) else { continue };
        if !fields.next().is_some_and(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()))
            || now.as_nanos().saturating_sub(stamp) < STALE.as_nanos()
        {
            continue;
        }
        // ESRCH only: a live or reused pid, or a process we can't signal, never shows the start has ended.
        if unsafe { libc::kill(pid, 0) } == 0 || io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH) {
            continue;
        }
        if let Err(error) = std::fs::remove_dir_all(&path) {
            eprintln!("Couldn't remove the abandoned launchd folder {}: {error}", path.display());
        }
    }
}

/// The jobs in `launchctl list` output that are ours, not running, and a minute or more old.
pub fn stale_labels(list: &str, now: Duration) -> Vec<&str> {
    list.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            if fields.next()? != "-" {
                return None;
            }
            let label = fields.nth(1)?;
            let (pid, stamp) = label.strip_prefix(PREFIX)?.split_once('.')?;
            pid.parse::<u32>().ok()?;
            let stamp = stamp.parse::<u128>().ok()?;
            (now.as_nanos().saturating_sub(stamp) >= STALE.as_nanos()).then_some(label)
        })
        .collect()
}

/// The one-shot job: run once at load (`RunAtLoad`), never again (`KeepAlive` false), and leave the detached server
/// alone when the job's process exits (`AbandonProcessGroup`). `Interactive`, since launchd throttles the CPU and
/// disk of other jobs, and the builds in its panes with them.
pub fn plist(cmd: &Command, log: &Path, label: &str, socket: &Path, mask: libc::mode_t) -> io::Result<String> {
    let mut result = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>",
    );
    let pair = |key: &str, value: &OsStr| -> io::Result<String> { Ok(format!("<key>{key}</key><string>{}</string>", xml(value)?)) };
    result.push_str(&pair("Label", OsStr::new(label))?);
    result.push_str("<key>ProgramArguments</key><array>");
    // The sentinel first: a server binary that predates it rejects the job instead of serving with launchd's
    // environment before it can receive the app's.
    for arg in [cmd.get_program(), OsStr::new(SENTINEL)].into_iter().chain(cmd.get_args()) {
        result.push_str(&format!("<string>{}</string>", xml(arg)?));
    }
    result.push_str("</array><key>EnvironmentVariables</key><dict>");
    result.push_str(&pair(DETACH, socket.as_os_str())?);
    result.push_str("</dict>");
    let cwd = cmd.get_current_dir().map(Path::to_path_buf).map_or_else(std::env::current_dir, Ok)?;
    for (key, value) in [
        ("WorkingDirectory", cwd.as_os_str()),
        ("StandardInPath", OsStr::new("/dev/null")),
        ("StandardOutPath", OsStr::new("/dev/null")),
        ("StandardErrorPath", log.as_os_str()),
        ("ProcessType", OsStr::new("Interactive")),
    ] {
        result.push_str(&pair(key, value)?);
    }
    result.push_str(&format!(
        "<key>RunAtLoad</key><true/><key>KeepAlive</key><false/><key>AbandonProcessGroup</key><true/>\
         <key>Umask</key><integer>{mask}</integer></dict></plist>\n"
    ));
    Ok(result)
}

/// Paths and arguments need an exact XML form. The environment's bytes travel over the private socket instead, so
/// escapes and colors in variables work too.
fn xml(value: &OsStr) -> io::Result<String> {
    let text = value.to_str().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "launchd value is not UTF-8"))?;
    let mut result = String::new();
    for c in text.chars() {
        match c {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '"' => result.push_str("&quot;"),
            '\t' | '\n' => result.push(c),
            c if c < ' ' || c == '\u{fffe}' || c == '\u{ffff}' => {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "launchd value contains an XML control character"));
            }
            c => result.push(c),
        }
    }
    Ok(result)
}

/// 32 random hex digits.
fn random_hex() -> String {
    let mut bytes = [0u8; 16];
    unsafe { libc::arc4random_buf(bytes.as_mut_ptr().cast(), bytes.len()) };
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
#[path = "launchd_tests.rs"]
mod tests;
