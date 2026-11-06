//! `wings-terminal-server serve --socket <path> [--idle-secs <n>] [--own-responsibility]`: the process that runs the native app's terminals so
//! they outlive it (`terminal_server.rs` says who starts it). It exits once nothing used it for `--idle-secs`, or once
//! it was told to drain and its panes have ended.
//!
//! On macOS it first owns its privacy responsibility, so the shells it runs keep their grants without the app: started
//! by launchd it detaches into a server that does (`wings_ptyd::launchd`), and with `--own-responsibility` it execs
//! itself to (`responsibility`). A debug app's plain child keeps the app's.

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
    match wings_ptyd::host::main(std::path::Path::new(&socket), idle) {
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

#[cfg(not(unix))]
fn main() {
    eprintln!("wings-terminal-server runs on macOS and Linux only");
    std::process::exit(2);
}
