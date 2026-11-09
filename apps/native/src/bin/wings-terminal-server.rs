//! `wings-terminal-server serve --socket <path> [--idle-secs <n>] [--own-responsibility]`: the process that runs the native app's terminals so
//! they outlive it (`terminal_server.rs` says who starts it). It exits once nothing used it for `--idle-secs`, or once
//! it was told to drain and its panes have ended.
//!
//! On macOS it first owns its privacy responsibility, so the shells it runs keep their grants without the app: started
//! by launchd it detaches into a server that does (`wings_ptyd::launchd`), and with `--own-responsibility` it execs
//! itself to (`responsibility`). A debug app's plain child keeps the app's.
//!
//! A debug build also takes `--server-version <n>`, to play a newer server in tests of an update.
//!
//! `wings-terminal-server check-access --own-responsibility` says whether the shells it runs could read other apps'
//! data, which macOS refuses unless it's turned on for the server's helper app in Files & Folders, and exits: 0 if they
//! could, 1 if not, 3 when it can't tell. Run, it also has Files & Folders list Chrome under the helper app.

#[cfg(unix)]
fn main() {
    #[cfg(target_os = "macos")]
    let launched = wings_ptyd::launchd::startup();
    #[cfg(target_os = "macos")]
    if launched.is_none() && std::env::args_os().any(|arg| arg == wings_ptyd::responsibility::FLAG)
    {
        wings_ptyd::responsibility::startup();
    }
    #[cfg(not(target_os = "macos"))]
    let launched: Option<Vec<std::ffi::OsString>> = None;
    let args: Vec<String> = match launched {
        Some(args) => args
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect(),
        None => std::env::args().skip(1).collect(),
    };
    if args.first().map(String::as_str) == Some("check-access") {
        std::process::exit(check_access());
    }
    let value = |flag: &str| args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone());
    let (Some("serve"), Some(socket)) = (args.first().map(String::as_str), value("--socket"))
    else {
        eprintln!(
            "Usage: wings-terminal-server serve --socket <path> [--idle-secs <n>] [--own-responsibility]"
        );
        std::process::exit(2);
    };
    let idle = value("--idle-secs")
        .and_then(|secs| secs.parse().ok())
        .map(std::time::Duration::from_secs);
    let version = if cfg!(debug_assertions) {
        value("--server-version").and_then(|version| version.parse().ok())
    } else {
        None
    };
    let version = version.unwrap_or(wings_ptyd::SERVER_VERSION);
    match wings_ptyd::host::main(std::path::Path::new(&socket), idle, version) {
        Ok(stopped) => eprintln!("wings-terminal-server: stopped ({stopped:?})"),
        // Another server answers there already.
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            eprintln!("wings-terminal-server: {socket}: {error}")
        }
        Err(error) => {
            eprintln!("wings-terminal-server: {socket}: {error}");
            std::process::exit(1);
        }
    }
}

/// Lists Chrome's data folder, the other app's data the Chrome DevTools MCP reads. Without Chrome, it can't tell.
#[cfg(unix)]
fn check_access() -> i32 {
    let Some(home) = std::env::var_os("HOME") else {
        return 3;
    };
    let chrome = std::path::Path::new(&home).join("Library/Application Support/Google/Chrome");
    match std::fs::read_dir(chrome) {
        Ok(_) => 0,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => 1,
        Err(_) => 3,
    }
}

#[cfg(not(unix))]
fn main() {
    eprintln!("wings-terminal-server runs on macOS and Linux only");
    std::process::exit(2);
}
