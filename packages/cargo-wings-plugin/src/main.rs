//! cargo wings-plugin new <name> [--sdk <path>]   starts a Rust plugin in a new folder (--sdk takes the SDK from a Wings
//!                                                checkout's packages/plugin-rs instead of crates.io)
//! cargo wings-plugin build                       builds the crate in rust/ into the .wasm modules the manifest names (dist/native/)
//! cargo wings-plugin check                       fails if a built module differs from what rust/ builds, or the manifest names what isn't there
//! cargo wings-plugin pack                        zips what the plugin ships into <id>.wings-plugin, the file Wings installs (run build first)
//! cargo wings-plugin dev                         builds on each save and has the running Wings restart the plugin, until you stop it

mod dev;
mod frames;
mod pack;
mod scaffold;

use frames::{System, build_rust, is_rust};
use std::{
    env,
    path::{Path, PathBuf},
    process::ExitCode,
};

const USAGE: &str = "Usage: cargo wings-plugin new <name> [--sdk <path>] | build | check | pack | dev";

fn main() -> ExitCode {
    // `cargo wings-plugin new` runs `cargo-wings-plugin wings-plugin new`.
    let mut args: Vec<String> = env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "wings-plugin") {
        args.remove(0);
    }
    match run(&args) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    let Some((command, flags)) = args.split_first() else {
        eprintln!("{USAGE}");
        return Ok(ExitCode::from(2));
    };
    if matches!(command.as_str(), "--help" | "-h" | "help") {
        println!("{USAGE}");
        return Ok(ExitCode::SUCCESS);
    }
    if matches!(command.as_str(), "--version" | "-V") {
        println!("cargo-wings-plugin {}", env!("CARGO_PKG_VERSION"));
        return Ok(ExitCode::SUCCESS);
    }
    if command == "new" {
        return new(flags);
    }
    if !["build", "check", "pack", "dev"].contains(&command.as_str()) {
        eprintln!("{USAGE}");
        return Ok(ExitCode::from(2));
    }
    let dir = env::current_dir().map_err(|e| e.to_string())?;
    // pack zips the built modules of a plugin, so it needs only the manifest and dist/native/.
    if command == "pack" {
        println!("{}", relative(&dir, &pack::pack(&dir)?));
        return Ok(ExitCode::SUCCESS);
    }
    if !is_rust(&dir) {
        return Err(format!("Nothing to {command} here: Wings runs only Rust/WebAssembly plugins, and a plugin is a folder with wings-plugin.json and a Rust crate in rust/. Run cargo wings-plugin new to make one."));
    }
    if command == "dev" {
        dev::watch(&dir, &System);
    }
    let check = command == "check" || flags.iter().any(|f| f == "--check");
    let built = build_rust(&dir, check, &System)?;
    for (file, bytes) in &built.built {
        println!("{}  {:.1} KB", relative(&dir, file), *bytes as f64 / 1024.0);
    }
    for file in &built.stale {
        eprintln!("{} is missing or out of date. Run cargo wings-plugin build.", relative(&dir, file));
    }
    if !built.stale.is_empty() {
        return Ok(ExitCode::FAILURE);
    }
    if check {
        println!("The built modules match rust/, and the manifest names files that exist.");
    }
    Ok(ExitCode::SUCCESS)
}

fn new(flags: &[String]) -> Result<ExitCode, String> {
    let mut flags = flags.to_vec();
    let sdk = match flags.iter().position(|f| f == "--sdk") {
        Some(at) => {
            let value = flags.get(at + 1).filter(|v| !v.starts_with("--")).cloned();
            flags.drain(at..(at + 2).min(flags.len()));
            Some(PathBuf::from(value.ok_or("--sdk needs the path of packages/plugin-rs in a Wings checkout.")?))
        }
        None => None,
    };
    let Some(name) = flags.iter().find(|f| !f.starts_with("--")) else {
        eprintln!("{USAGE}");
        return Ok(ExitCode::from(2));
    };
    scaffold::new_plugin(Path::new(name), sdk.as_deref(), &mut |line| println!("{line}"))?;
    Ok(ExitCode::SUCCESS)
}

fn relative(dir: &Path, file: &Path) -> String {
    file.strip_prefix(dir).unwrap_or(file).display().to_string()
}
