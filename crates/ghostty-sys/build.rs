//! Builds libghostty-vt from `vendor/ghostty` (a git submodule pinned to one Ghostty commit) with Zig, and
//! generates bindings from its headers.
//!
//! Adapted from Herdr's `crates/ghostty-sys/build.rs` (https://github.com/herdrdev/herdr-app, Apache-2.0).

use std::path::{Path, PathBuf};
use std::process::Command;

/// Zig builds for the CPU it runs on unless told otherwise, whose newest instructions crash older CPUs.
fn zig_target(target: &str) -> &'static str {
    match target {
        "aarch64-apple-darwin" => "aarch64-macos",
        "x86_64-apple-darwin" => "x86_64-macos",
        "x86_64-unknown-linux-gnu" => "x86_64-linux-gnu",
        "aarch64-unknown-linux-gnu" => "aarch64-linux-gnu",
        other => panic!("ghostty-sys: no Zig target for {other}"),
    }
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let src = manifest.join("vendor/ghostty");
    // A new git worktree has the submodule's folder but not its files.
    if !src.join("build.zig").exists() {
        let status = Command::new("git")
            .current_dir(&manifest)
            .args(["submodule", "update", "--init", "--depth", "1", "vendor/ghostty"])
            .status()
            .expect("cannot run git to fetch vendor/ghostty");
        assert!(
            status.success(),
            "vendor/ghostty missing: run `git submodule update --init --depth 1` in crates/ghostty-sys"
        );
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    for path in ["src", "build.zig", "build.zig.zon", "include"] {
        println!("cargo:rerun-if-changed={}", src.join(path).display());
    }
    println!("cargo:rerun-if-env-changed=ZIG");

    let zig = std::env::var("ZIG").unwrap_or_else(|_| "zig".into());
    // Zig's cache stays in the submodule's ignored `.zig-cache`, so debug and release builds share it.
    let status = Command::new(&zig)
        .current_dir(&src)
        // Only the static library is linked. Ghostty builds the xcframework whenever it finds xcodebuild,
        // which fails with only the Command Line Tools.
        .args([
            "build",
            "-Demit-lib-vt",
            "-Demit-xcframework=false",
            "-Doptimize=ReleaseFast",
        ])
        .arg(format!(
            "-Dtarget={}",
            zig_target(&std::env::var("TARGET").unwrap())
        ))
        .arg("--prefix")
        .arg(out.join("zig-out"))
        .status()
        .unwrap_or_else(|e| panic!("cannot run {zig:?} (Zig 0.17 required, `brew install zig`): {e}"));
    assert!(status.success(), "zig build of libghostty-vt failed: {status}");

    // Only the static archive in the search path, so the linker can't pick the shared one.
    let lib = out.join("lib");
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::copy(
        out.join("zig-out/lib/libghostty-vt.a"),
        lib.join("libghostty-vt.a"),
    )
    .unwrap();
    println!("cargo:rustc-link-search=native={}", lib.display());
    println!("cargo:rustc-link-lib=static=ghostty-vt");

    bindings(&src.join("include"), &out);
}

fn bindings(include: &Path, out: &Path) {
    let mut builder = bindgen::Builder::default()
        .header(include.join("ghostty/vt.h").to_string_lossy())
        .clang_arg(format!("-I{}", include.display()));
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        // Apple's libclang has its builtin headers but needs the SDK for the C library.
        if let Ok(sdk) = Command::new("xcrun").args(["--show-sdk-path"]).output() {
            builder = builder.clang_arg(format!(
                "-isysroot{}",
                String::from_utf8_lossy(&sdk.stdout).trim()
            ));
        }
    }
    builder
        .allowlist_function("ghostty_.*")
        .allowlist_type("Ghostty.*")
        .allowlist_var("GHOSTTY_.*")
        .prepend_enum_name(false)
        .derive_default(true)
        .layout_tests(false)
        .generate()
        .expect("bindgen failed on ghostty/vt.h")
        .write_to_file(out.join("bindings.rs"))
        .unwrap();
}
