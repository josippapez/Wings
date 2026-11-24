//! `cargo wings-plugin pack`: zips what a Rust plugin ships into `<id>.wings-plugin`, the file Wings installs. The
//! WebAssembly modules the manifest names must already be built (`cargo wings-plugin build`, see `frames.rs`).

use crate::frames::{missing_frames, read_manifest};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

/// Names at the top of a plugin folder that are for building it, not for Wings. The README stays: Wings shows it in Settings.
fn source_only(name: &str) -> bool {
    matches!(
        name,
        "src" | "preview" | "node_modules" | "package.json" | "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock" | "bun.lock" | "bun.lockb" | "rust" | "target" | "Cargo.toml" | "Cargo.lock"
    ) || (name.starts_with("tsconfig") && name.ends_with(".json"))
        || name.starts_with("HANDOVER")
        || name.ends_with(".wings-plugin")
}

/// What the plugin ships under `dir`, as `(path inside the package with / between folders, file)`: everything but its
/// source, dependencies and dotfiles, sorted by path so the same plugin packs the same way.
fn shipped(dir: &Path, prefix: &str, top: bool, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
    let mut entries: Vec<_> = fs::read_dir(dir).map_err(|e| format!("Couldn't read {}: {e}", dir.display()))?.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == "node_modules" || (top && source_only(&name)) {
            continue;
        }
        let path = entry.path();
        let kind = fs::metadata(&path).map_err(|e| e.to_string())?;
        if kind.is_dir() {
            shipped(&path, &format!("{prefix}{name}/"), false, out)?;
        } else if kind.is_file() {
            out.push((format!("{prefix}{name}"), path));
        }
    }
    Ok(())
}

/// Zips what the plugin in `dir` ships as `<id>.wings-plugin` in `dir`, and returns its path. The package is the
/// manifest, README, icon and the WebAssembly modules in `dist/native/`; the Rust source and its build folder stay
/// out. Fails when a module the manifest names isn't built.
pub fn pack(dir: &Path) -> Result<PathBuf, String> {
    let manifest = read_manifest(dir)?;
    let id = manifest["id"].as_str().ok_or("wings-plugin.json has no id.")?;
    let mut files = Vec::new();
    shipped(dir, "", true, &mut files)?;
    let main = manifest["main"].as_str().map(|m| crate::frames::in_folder(m));
    let missing = missing_frames(dir)?.into_iter().next().or_else(|| main.filter(|m| !files.iter().any(|(path, _)| path == m)));
    if let Some(missing) = missing {
        return Err(format!("{missing} is missing. Build the plugin's Rust frames first."));
    }
    let out = dir.join(format!("{id}.wings-plugin"));
    let mut zip = ZipWriter::new(fs::File::create(&out).map_err(|e| format!("Couldn't write {}: {e}", out.display()))?);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated).compression_level(Some(9)).unix_permissions(0o644);
    for (path, file) in files {
        zip.start_file(path, options).map_err(|e| e.to_string())?;
        zip.write_all(&fs::read(&file).map_err(|e| format!("Couldn't read {}: {e}", file.display()))?).map_err(|e| e.to_string())?;
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::{build_rust, tests::made};
    use std::io::Read;

    fn names(file: &Path) -> Vec<String> {
        let mut zip = zip::ZipArchive::new(fs::File::open(file).unwrap()).unwrap();
        let mut names: Vec<String> = (0..zip.len()).map(|i| zip.by_index(i).unwrap().name().to_string()).collect();
        names.sort();
        let mut main = String::new();
        zip.by_name("dist/native/main.wasm").unwrap().read_to_string(&mut main).unwrap();
        assert_eq!(main, "module main v1");
        names
    }

    #[test]
    fn pack_ships_the_manifest_readme_icon_and_modules_and_leaves_the_crate_and_its_build_folder_out() {
        let m = made(true, true, None);
        build_rust(&m.dir, false, &m.fake).unwrap();
        fs::create_dir_all(m.dir.join("rust/target/release")).unwrap();
        fs::write(m.dir.join("rust/target/release/big"), "x").unwrap();
        fs::write(m.dir.join("rust/Cargo.lock"), "x").unwrap();
        let file = pack(&m.dir).unwrap();
        assert_eq!(file, m.dir.join("demo.wings-plugin"));
        assert_eq!(names(&file), ["README.md", "dist/native/main.wasm", "dist/native/sidebar.wasm", "icon.svg", "wings-plugin.json"]);
        // Packing again leaves the first package out of the second.
        assert_eq!(names(&pack(&m.dir).unwrap()).len(), 5);
    }

    #[test]
    fn pack_stops_at_a_module_the_manifest_names_that_isnt_built() {
        let m = made(true, true, None);
        build_rust(&m.dir, false, &m.fake).unwrap();
        fs::remove_file(m.dir.join("dist/native/sidebar.wasm")).unwrap();
        let error = pack(&m.dir).err().unwrap();
        assert_eq!(error, "dist/native/sidebar.wasm is missing. Build the plugin's Rust frames first.");
        assert!(!m.dir.join("demo.wings-plugin").exists());
    }
}
