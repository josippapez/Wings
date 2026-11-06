//! Hands the crate the desktop app's version as `WINGS_DESKTOP_VERSION`.
//!
//! Native shows the plugins the desktop installed, and a plugin's `minWingsVersion` is a promise about the desktop
//! release it was written for, so it is checked against the desktop's version (`apps/desktop/src-tauri/Cargo.toml`),
//! never against this crate's own `0.1.0`. Reading it here means a desktop release needs no edit in this crate.

use std::{env, fs, path::Path};

/// The `version` of the `[package]` table.
fn package_version(manifest: &str) -> Option<&str> {
    let mut in_package = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if in_package && let Some(rest) = line.strip_prefix("version") {
            return rest
                .trim_start()
                .strip_prefix('=')?
                .trim()
                .strip_prefix('"')?
                .split('"')
                .next();
        }
    }
    None
}

fn main() {
    let dir = env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    let manifest = Path::new(&dir).join("../desktop/src-tauri/Cargo.toml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let text = fs::read_to_string(&manifest)
        .unwrap_or_else(|error| panic!("can't read {}: {error}", manifest.display()));
    let version = package_version(&text)
        .unwrap_or_else(|| panic!("{} has no [package] version", manifest.display()));
    println!("cargo:rustc-env=WINGS_DESKTOP_VERSION={version}");
}
