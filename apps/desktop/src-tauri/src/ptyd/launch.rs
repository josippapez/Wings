//! Who runs the terminal host.
//!
//! An installed Wings registers it as a LaunchAgent with SMAppService, from `Contents/Library/LaunchAgents/
//! dev.wings.ptyd.plist` in its bundle (`LaunchAgents/` here), so launchd runs `wings ptyd` from the bundle
//! (`BundleProgram`) apart from the app and it outlives Wings: a child of Wings ends when Wings quits, even after
//! `setsid`. launchd starts it when it is registered and at each login, and again whenever it exits (`RunAtLoad`,
//! `KeepAlive`), as it does after an update once it has no pane (`host::serve`). It runs as `Interactive`: launchd
//! throttles the CPU and disk of a job without a `ProcessType`, and the builds in its panes would be throttled with it.
//!
//! A dev build runs the host as its own child, which ends with the app.

use std::{
    io,
    os::unix::net::UnixStream,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use objc2::runtime::AnyClass;
use objc2_foundation::NSString;
use objc2_service_management::{kSMErrorAlreadyRegistered, SMAppService, SMAppServiceStatus};

/// In `Contents/Library/LaunchAgents`, where SMAppService looks for it.
const PLIST: &str = "dev.wings.ptyd.plist";

/// A release Wings run from its app bundle, which holds the plist, on macOS 13 or later, which has SMAppService.
fn bundled() -> bool {
    !cfg!(debug_assertions)
        && std::env::current_exe().is_ok_and(|exe| {
            exe.parent().is_some_and(|dir| dir.ends_with("Contents/MacOS")) && exe.ancestors().nth(3).is_some_and(|app| app.extension().is_some_and(|e| e == "app"))
        })
        && AnyClass::get(c"SMAppService").is_some()
}

/// Registers the host's LaunchAgent, which launchd then starts. One already registered stays as it is.
pub fn register() {
    if !bundled() {
        return;
    }
    let service = unsafe { SMAppService::agentServiceWithPlistName(&NSString::from_str(PLIST)) };
    if let Err(e) = unsafe { service.registerAndReturnError() } {
        if e.code() != kSMErrorAlreadyRegistered as isize {
            crate::applog::log_line!("[ptyd] couldn't register the terminal host: {} ({})", e.localizedDescription(), e.code());
        }
    }
    // Where it lands when you turn Wings off in System Settings, Login Items; launchd doesn't run it then.
    if unsafe { service.status() } == SMAppServiceStatus::RequiresApproval {
        crate::applog::log_line!("[ptyd] the terminal host waits for your approval in System Settings, Login Items");
    }
}

/// Removes the host's LaunchAgent, so launchd stops starting it. Nothing happens when it isn't registered.
pub fn unregister() {
    if !bundled() {
        return;
    }
    let service = unsafe { SMAppService::agentServiceWithPlistName(&NSString::from_str(PLIST)) };
    if unsafe { service.status() } == SMAppServiceStatus::NotRegistered {
        return;
    }
    if let Err(e) = unsafe { service.unregisterAndReturnError() } {
        crate::applog::log_line!("[ptyd] couldn't unregister the terminal host: {} ({})", e.localizedDescription(), e.code());
    }
}

/// A connection to the host at `socket`: launchd's, which may take a moment to start, or a child this starts.
pub fn connect(socket: &Path) -> io::Result<UnixStream> {
    if let Ok(stream) = UnixStream::connect(socket) {
        return Ok(stream);
    }
    if bundled() {
        return answer(socket, || Ok(()));
    }
    let mut child = Command::new(std::env::current_exe()?).arg("ptyd").arg("--socket").arg(socket).stdin(Stdio::null()).stdout(Stdio::null()).spawn()?;
    let stream = answer(socket, || match child.try_wait()? {
        Some(status) => Err(io::Error::other(format!("the terminal host exited: {status}"))),
        None => Ok(()),
    });
    // Reaped when it exits, since nothing else waits for it.
    thread::spawn(move || child.wait());
    stream
}

/// Connects every 10 ms until the host answers, `alive` fails, or 5 s pass.
fn answer(socket: &Path, mut alive: impl FnMut() -> io::Result<()>) -> io::Result<UnixStream> {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(stream) = UnixStream::connect(socket) {
            return Ok(stream);
        }
        alive()?;
        if Instant::now() > until {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "the terminal host didn't start"));
        }
        thread::sleep(Duration::from_millis(10));
    }
}
