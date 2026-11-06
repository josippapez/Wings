//! `wings-terminal-server serve --socket <path> [--idle-secs <n>] [--plist <path>]`: the process that runs the native
//! app's terminals so they outlive it (`terminal_server.rs` says who starts it). It exits once nothing used it for
//! `--idle-secs`, or once it was told to drain and its panes have ended; then it removes its LaunchAgent (`--plist`),
//! so launchd doesn't start it at the next login.

#[cfg(unix)]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |flag: &str| args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone());
    let (Some("serve"), Some(socket)) = (args.first().map(String::as_str), value("--socket"))
    else {
        eprintln!(
            "Usage: wings-terminal-server serve --socket <path> [--idle-secs <n>] [--plist <path>]"
        );
        std::process::exit(2);
    };
    let idle = value("--idle-secs")
        .and_then(|secs| secs.parse().ok())
        .map(std::time::Duration::from_secs);
    match wings_ptyd::host::main(std::path::Path::new(&socket), idle) {
        Ok(stopped) => {
            eprintln!("wings-terminal-server: stopped ({stopped:?})");
            if stopped == wings_ptyd::host::Stopped::Drained
                && let Some(plist) = value("--plist")
            {
                let _ = std::fs::remove_file(plist);
            }
        }
        // Another server answers there already. Exiting 0 keeps launchd from starting this one again and again.
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
