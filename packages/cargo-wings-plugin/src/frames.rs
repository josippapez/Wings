//! Builds and checks a plugin written in Rust: the crate in `rust/` with one Cargo feature per frame, built to
//! `dist/native/<frame>.wasm`, which the manifest names as its `main` and its pages. This is `plugins/build-rust.sh` of
//! the Wings repo for a plugin in its own repo: that script builds the repo's workspace with jq and sh, this needs only
//! Rust, and the crate has its own target folder in `rust/target`.

use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub const TARGET: &str = "wasm32-unknown-unknown";

/// What a program printed and how it ended. `status` is `None` when the program isn't there or was killed.
pub struct Output {
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// Runs a program. `capture` returns its output instead of letting it print. The tests replace it.
pub trait Runner {
    fn run(&self, program: &str, args: &[String], cwd: &Path, capture: bool) -> Output;
}

pub struct System;

impl Runner for System {
    fn run(&self, program: &str, args: &[String], cwd: &Path, capture: bool) -> Output {
        let mut command = Command::new(program);
        command.args(args).current_dir(cwd);
        if capture {
            return match command.stdin(Stdio::null()).output() {
                Ok(out) => Output {
                    status: out.status.code(),
                    stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
                },
                Err(_) => Output { status: None, stdout: String::new(), stderr: String::new() },
            };
        }
        let status = command.status().ok().and_then(|s| s.code());
        Output { status, stdout: String::new(), stderr: String::new() }
    }
}

/// Whether the plugin in `dir` has a Rust crate.
pub fn is_rust(dir: &Path) -> bool {
    dir.join("rust").join("Cargo.toml").exists()
}

pub fn read_manifest(dir: &Path) -> Result<Value, String> {
    let file = dir.join("wings-plugin.json");
    let text = fs::read_to_string(&file).map_err(|e| format!("Couldn't read {}: {e}", file.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{} isn't valid JSON: {e}", file.display()))
}

/// A manifest value as a path: none for null, text for anything else.
fn path_of(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

/// The manifest's fields that name a module, as `(field, path)`: its main and each page.
fn modules_named(manifest: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut add = |field: String, value: &Value| {
        if let Some(path) = path_of(value) {
            out.push((field, path));
        }
    };
    add("main".into(), &manifest["main"]);
    let contributes = &manifest["contributes"];
    let list = |key: &str| contributes[key].as_array().cloned().unwrap_or_default();
    for (i, sidebar) in list("sidebars").iter().enumerate() {
        add(format!("contributes.sidebars[{i}].page"), &sidebar["page"]);
        add(format!("contributes.sidebars[{i}].popover.page"), &sidebar["popover"]["page"]);
    }
    for (i, view) in list("views").iter().enumerate() {
        add(format!("contributes.views[{i}].page"), &view["page"]);
    }
    for (i, block) in list("blocks").iter().enumerate() {
        add(format!("contributes.blocks[{i}].page"), &block["page"]);
    }
    add("contributes.settings.page".into(), &contributes["settings"]["page"]);
    out
}

/// A manifest path as Wings reads it: from the plugin folder, without a leading `/`, `.` and `..` resolved.
pub fn in_folder(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.trim_start_matches('/').split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|last| *last != "..") => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() { ".".into() } else { parts.join("/") }
}

/// The WebAssembly modules the manifest names, each once, as paths from the plugin folder.
pub fn frames_of(manifest: &Value) -> Vec<String> {
    let mut seen = BTreeSet::new();
    modules_named(manifest)
        .into_iter()
        .map(|(_, path)| in_folder(&path))
        .filter(|path| path.ends_with(".wasm") && seen.insert(path.clone()))
        .collect()
}

/// The lines of the `[header]` section of a Cargo.toml, up to the next section.
fn toml_section<'a>(text: &'a str, header: &str) -> Option<Vec<&'a str>> {
    let mut lines = text.lines();
    let open = format!("[{header}]");
    lines.find(|line| line.starts_with(&open))?;
    Some(lines.take_while(|line| !line.starts_with('[')).collect())
}

fn cargo_toml(dir: &Path) -> Result<String, String> {
    fs::read_to_string(dir.join("rust").join("Cargo.toml")).map_err(|e| format!("Couldn't read rust/Cargo.toml: {e}"))
}

/// The package name in `rust/Cargo.toml`: `name = "..."` under `[package]`.
pub fn crate_name(dir: &Path) -> Result<String, String> {
    let text = cargo_toml(dir)?;
    toml_section(&text, "package")
        .and_then(|lines| {
            lines.iter().find_map(|line| {
                let rest = line.strip_prefix("name")?.trim_start().strip_prefix('=')?.trim_start().strip_prefix('"')?;
                rest.split_once('"').map(|(name, _)| name.to_string()).filter(|name| !name.is_empty())
            })
        })
        .ok_or_else(|| "rust/Cargo.toml has no [package] name.".to_string())
}

/// The feature names `rust/Cargo.toml` declares under `[features]`.
fn features_of(dir: &Path) -> Result<BTreeSet<String>, String> {
    let text = cargo_toml(dir)?;
    Ok(toml_section(&text, "features")
        .unwrap_or_default()
        .iter()
        .filter_map(|line| {
            let (key, _) = line.split_once('=')?;
            let key = key.trim_end();
            (!key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')).then(|| key.to_string())
        })
        .collect())
}

fn is_file(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file())
}

/// What the manifest says that a Rust plugin can't bear out, as a sentence each: none when it all fits. Reads no built file.
pub fn manifest_problems(dir: &Path) -> Result<Vec<String>, String> {
    let manifest = read_manifest(dir)?;
    let features = features_of(dir)?;
    let mut problems = Vec::new();
    for (field, path) in modules_named(&manifest) {
        if !path.ends_with(".wasm") {
            let example = if field == "main" { "main" } else { "sidebar" };
            problems.push(format!(
                "{field} is \"{path}\", but Wings runs only WebAssembly modules now: name a .wasm built from rust/, like dist/native/{example}.wasm."
            ));
            continue;
        }
        let name = Path::new(&path).file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        if !features.contains(name) {
            problems.push(format!(
                "{field} is \"{path}\", which is built with the Cargo feature \"{name}\", but rust/Cargo.toml has no [features] {name} = []."
            ));
        }
    }
    if !manifest["main"].is_string() {
        problems.push("main is missing: name the main module, \"dist/native/main.wasm\".".into());
    }
    if let Some(icon) = path_of(&manifest["icon"]) {
        if !is_file(&dir.join(in_folder(&icon))) {
            problems.push(format!("icon is \"{icon}\", which the plugin folder doesn't have."));
        }
    }
    Ok(problems)
}

/// The modules the manifest names that aren't in the folder.
pub fn missing_frames(dir: &Path) -> Result<Vec<String>, String> {
    Ok(frames_of(&read_manifest(dir)?).into_iter().filter(|frame| !is_file(&dir.join(frame))).collect())
}

/// Fails with a line that says how to get cargo or the WebAssembly target when either is missing.
fn preflight(dir: &Path, runner: &dyn Runner) -> Result<(), String> {
    if runner.run("cargo", &["--version".into()], dir, true).status != Some(0) {
        return Err(format!("cargo isn't installed or isn't on your PATH. Install Rust from https://rustup.rs, then run: rustup target add {TARGET}"));
    }
    let sysroot = runner.run("rustc", &["--print".into(), "sysroot".into()], dir, true);
    if sysroot.status != Some(0) || !Path::new(sysroot.stdout.trim()).join("lib").join("rustlib").join(TARGET).exists() {
        return Err(format!("The {TARGET} target isn't installed for this Rust. Run: rustup target add {TARGET}"));
    }
    Ok(())
}

pub struct Built {
    /// The modules written, with their size in bytes.
    pub built: Vec<(PathBuf, u64)>,
    /// With `check`, the modules that are missing or differ from what `rust/` builds.
    pub stale: Vec<PathBuf>,
}

/// Builds the Rust plugin in `dir`: each module the manifest names, with the Cargo feature of its file name, into the
/// place the manifest names. With `check`, writes nothing and returns the modules that differ from what `rust/` builds as
/// `stale`. Fails when the manifest and the crate disagree, or cargo fails.
pub fn build_rust(dir: &Path, check: bool, runner: &dyn Runner) -> Result<Built, String> {
    let problems = manifest_problems(dir)?;
    if !problems.is_empty() {
        return Err(problems.iter().map(|p| format!("wings-plugin.json: {p}")).collect::<Vec<_>>().join("\n"));
    }
    preflight(dir, runner)?;
    let target_dir = dir.join("rust").join("target");
    // Frames share the folder: each build replaces the module the last one left, so it is copied out right away.
    let module = target_dir.join(TARGET).join("release").join(format!("{}.wasm", crate_name(dir)?.replace('-', "_")));
    let mut result = Built { built: Vec::new(), stale: Vec::new() };
    for frame in frames_of(&read_manifest(dir)?) {
        let name = Path::new(&frame).file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
        let args: Vec<String> = [
            "build",
            "--release",
            "--target",
            TARGET,
            "--manifest-path",
            &dir.join("rust").join("Cargo.toml").to_string_lossy(),
            "--target-dir",
            &target_dir.to_string_lossy(),
            "--features",
            &name,
        ]
        .map(String::from)
        .into();
        if runner.run("cargo", &args, dir, false).status != Some(0) {
            return Err(format!("cargo build failed for the \"{name}\" frame (--features {name})."));
        }
        let out = dir.join(&frame);
        let fresh = fs::read(&module).map_err(|e| format!("cargo built no {}: {e}", module.display()))?;
        if check {
            if fs::read(&out).map_or(true, |old| old != fresh) {
                result.stale.push(out);
            }
        } else {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::write(&out, &fresh).map_err(|e| format!("Couldn't write {}: {e}", out.display()))?;
            result.built.push((out, fresh.len() as u64));
        }
    }
    Ok(result)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::scaffold;
    use std::cell::{Cell, RefCell};

    /// A new empty folder in the temp dir that goes when the value does.
    pub struct Scratch(pub PathBuf);
    impl Scratch {
        pub fn new(label: &str) -> Scratch {
            static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("cwp-{label}-{}-{n}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Stands in for cargo and rustc: writes a module named after the feature.
    pub struct Fake {
        pub calls: RefCell<Vec<(String, Vec<String>)>>,
        pub version: Cell<u32>,
        pub cargo: bool,
        pub sysroot: PathBuf,
        pub fails: Option<&'static str>,
    }

    impl Runner for Fake {
        fn run(&self, program: &str, args: &[String], _cwd: &Path, _capture: bool) -> Output {
            self.calls.borrow_mut().push((program.to_string(), args.to_vec()));
            let ok = |stdout: String| Output { status: Some(0), stdout, stderr: String::new() };
            if program == "cargo" && !self.cargo {
                return Output { status: None, stdout: String::new(), stderr: String::new() };
            }
            if args[0] == "--version" {
                return ok("cargo 1.99.0".into());
            }
            if program == "rustc" {
                return ok(format!("{}\n", self.sysroot.display()));
            }
            let arg = |flag: &str| args[args.iter().position(|a| a == flag).unwrap() + 1].clone();
            let feature = arg("--features");
            if Some(feature.as_str()) == self.fails {
                return Output { status: Some(101), stdout: String::new(), stderr: String::new() };
            }
            let out = Path::new(&arg("--target-dir")).join(TARGET).join("release");
            fs::create_dir_all(&out).unwrap();
            fs::write(out.join("demo.wasm"), format!("module {feature} v{}", self.version.get())).unwrap();
            ok(String::new())
        }
    }

    pub struct Made {
        pub _root: Scratch,
        pub dir: PathBuf,
        pub fake: Fake,
    }

    /// A plugin made by `new` in a temp folder, and a runner that stands in for cargo.
    pub fn made(target: bool, cargo: bool, fails: Option<&'static str>) -> Made {
        let root = Scratch::new("frames");
        let dir = root.0.join("demo");
        scaffold::new_plugin(&dir, None, &mut |_| {}).unwrap();
        let sysroot = root.0.join("sysroot");
        if target {
            fs::create_dir_all(sysroot.join("lib").join("rustlib").join(TARGET)).unwrap();
        }
        let fake = Fake { calls: RefCell::new(Vec::new()), version: Cell::new(1), cargo, sysroot, fails };
        Made { _root: root, dir, fake }
    }

    fn rel(dir: &Path, file: &Path) -> String {
        file.strip_prefix(dir).unwrap().to_string_lossy().into_owned()
    }

    #[test]
    fn build_puts_each_frames_module_where_the_manifest_names_it_built_with_that_frames_feature() {
        let m = made(true, true, None);
        let built = build_rust(&m.dir, false, &m.fake).unwrap().built;
        assert_eq!(built.iter().map(|(f, _)| rel(&m.dir, f)).collect::<Vec<_>>(), ["dist/native/main.wasm", "dist/native/sidebar.wasm"]);
        assert_eq!(fs::read_to_string(m.dir.join("dist/native/main.wasm")).unwrap(), "module main v1");
        assert_eq!(fs::read_to_string(m.dir.join("dist/native/sidebar.wasm")).unwrap(), "module sidebar v1");
        let calls = m.fake.calls.borrow();
        let builds: Vec<&Vec<String>> = calls.iter().filter(|(_, a)| a[0] == "build").map(|(_, a)| a).collect();
        let value = |a: &Vec<String>, flag: &str| a[a.iter().position(|x| x == flag).unwrap() + 1].clone();
        assert_eq!(builds.iter().map(|a| value(a, "--features")).collect::<Vec<_>>(), ["main", "sidebar"]);
        let target_dir = m.dir.join("rust").join("target").to_string_lossy().into_owned();
        assert!(builds.iter().all(|a| a.contains(&"--release".to_string()) && value(a, "--target") == TARGET && value(a, "--target-dir") == target_dir));
    }

    #[test]
    fn build_says_what_to_install_when_cargo_or_the_webassembly_target_is_missing() {
        let no_cargo = made(true, false, None);
        let error = build_rust(&no_cargo.dir, false, &no_cargo.fake).err().unwrap();
        assert!(error.contains("cargo isn't installed") && error.contains("rustup.rs"), "{error}");
        let no_target = made(false, true, None);
        let error = build_rust(&no_target.dir, false, &no_target.fake).err().unwrap();
        assert!(error.contains("wasm32-unknown-unknown target isn't installed") && error.contains("rustup target add wasm32-unknown-unknown"), "{error}");
        assert!(!no_target.dir.join("dist").exists());
    }

    #[test]
    fn a_failed_cargo_build_names_the_frame() {
        let m = made(true, true, Some("sidebar"));
        let error = build_rust(&m.dir, false, &m.fake).err().unwrap();
        assert!(error.contains("cargo build failed for the \"sidebar\" frame"), "{error}");
    }

    #[test]
    fn build_refuses_a_manifest_the_crate_cant_bear_out() {
        let m = made(true, true, None);
        let file = m.dir.join("wings-plugin.json");
        let mut manifest: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        manifest["contributes"]["sidebars"][0]["page"] = "dist/native/other.wasm".into();
        manifest["contributes"]["views"] = serde_json::json!([{ "id": "old", "title": "Old", "page": "dist/old.html" }]);
        manifest["icon"] = "missing.svg".into();
        fs::write(&file, manifest.to_string()).unwrap();
        let error = build_rust(&m.dir, false, &m.fake).err().unwrap();
        assert!(error.contains("\"dist/native/other.wasm\", which is built with the Cargo feature \"other\""), "{error}");
        assert!(error.contains("contributes.views[0].page is \"dist/old.html\", but Wings runs only WebAssembly"), "{error}");
        assert!(error.contains("icon is \"missing.svg\""), "{error}");
        assert!(m.fake.calls.borrow().is_empty(), "nothing runs before the manifest fits");
    }

    #[test]
    fn check_passes_on_what_build_wrote_and_fails_on_a_module_that_is_out_of_date_or_gone() {
        let m = made(true, true, None);
        assert_eq!(build_rust(&m.dir, false, &m.fake).unwrap().built.len(), 2);
        let checked = build_rust(&m.dir, true, &m.fake).unwrap();
        assert!(checked.built.is_empty() && checked.stale.is_empty());
        m.fake.version.set(2);
        let stale = build_rust(&m.dir, true, &m.fake).unwrap().stale;
        assert_eq!(stale.iter().map(|f| rel(&m.dir, f)).collect::<Vec<_>>(), ["dist/native/main.wasm", "dist/native/sidebar.wasm"]);
        fs::remove_file(m.dir.join("dist/native/sidebar.wasm")).unwrap();
        assert_eq!(fs::read_to_string(m.dir.join("dist/native/main.wasm")).unwrap(), "module main v1", "check writes nothing");
        assert!(!m.dir.join("dist/native/sidebar.wasm").exists());
    }

    #[test]
    fn manifest_paths_are_read_as_wings_reads_them() {
        assert_eq!(in_folder("/dist/./native/../native/main.wasm"), "dist/native/main.wasm");
        assert_eq!(in_folder("../x.wasm"), "../x.wasm");
        let manifest = serde_json::json!({
            "main": "dist/native/main.wasm",
            "contributes": {
                "sidebars": [{ "page": "dist/native/a.wasm", "popover": { "page": "dist/native/b.wasm" } }],
                "views": [{ "page": "dist/native/a.wasm" }],
                "blocks": [{ "page": "dist/native/c.wasm" }],
                "settings": { "page": "dist/native/d.wasm" }
            }
        });
        assert_eq!(frames_of(&manifest), ["dist/native/main.wasm", "dist/native/a.wasm", "dist/native/b.wasm", "dist/native/c.wasm", "dist/native/d.wasm"]);
    }

    #[test]
    fn the_crate_name_and_features_come_from_cargo_toml() {
        let m = made(true, true, None);
        assert_eq!(crate_name(&m.dir).unwrap(), "demo");
        assert_eq!(features_of(&m.dir).unwrap().into_iter().collect::<Vec<_>>(), ["main", "sidebar"]);
    }
}
