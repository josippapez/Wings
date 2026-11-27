//! `cargo wings-plugin new`: writes the starter plugin, a Rust crate that builds to WebAssembly, into a new folder and
//! names it after the folder. The template is in the binary, so this works from a crate installed off crates.io.

use std::{fs, path::Path};

/// The `wings-plugin` requirement a new plugin's `rust/Cargo.toml` gets: the series this tool was made for.
pub const SDK_REQUIREMENT: &str = "0.2";

/// The template's files, by where they go in the new folder. `gitignore` and `github/` are written with their dot, and
/// the crate's manifest is `Cargo.toml.tmpl`, which cargo would otherwise take for a package of its own and leave out.
const FILES: &[(&str, &str)] = &[
    ("README.md", include_str!("../template/README.md")),
    (".gitignore", include_str!("../template/gitignore")),
    ("icon.svg", include_str!("../template/icon.svg")),
    ("wings-plugin.json", include_str!("../template/wings-plugin.json")),
    ("rust/Cargo.toml", include_str!("../template/rust/Cargo.toml.tmpl")),
    ("rust/src/lib.rs", include_str!("../template/rust/src/lib.rs")),
    ("rust/src/main_frame.rs", include_str!("../template/rust/src/main_frame.rs")),
    ("rust/src/sidebar.rs", include_str!("../template/rust/src/sidebar.rs")),
    (".github/workflows/release.yml", include_str!("../template/github/workflows/release.yml")),
];

/// The id the template's manifest and README start with, and the name they show.
const STARTER_ID: &str = "hello-wings";
const STARTER_NAME: &str = "Hello Wings";

/// The id a folder name makes: lowercase letters, digits and dashes, up to 64, which is what Wings accepts.
pub fn id_of(folder: &Path) -> String {
    let base = folder.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let mut id = String::new();
    for c in base.chars() {
        if c.is_ascii_alphanumeric() {
            id.push(c);
        } else if !id.ends_with('-') {
            id.push('-');
        }
    }
    let id = id.trim_matches('-');
    id.chars().take(64).collect::<String>().trim_end_matches('-').to_string()
}

/// A readable name from an id: "my-plugin" is "My plugin".
fn name_of(id: &str) -> String {
    let words = id.replace('-', " ");
    let mut chars = words.chars();
    chars.next().map(|first| first.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
}

/// Creates the plugin in `folder`, which has to be new or empty. The crate takes the SDK from crates.io; with `sdk`, the
/// `packages/plugin-rs` folder of a Wings checkout, it takes it from there.
pub fn new_plugin(folder: &Path, sdk: Option<&Path>, log: &mut dyn FnMut(&str)) -> Result<String, String> {
    let dir = std::path::absolute(folder).map_err(|e| e.to_string())?;
    let id = id_of(&dir);
    if id.is_empty() {
        let base = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        return Err(format!("Can't make a plugin id from the folder name \"{base}\": use letters or digits."));
    }
    if dir.is_dir() && fs::read_dir(&dir).map_err(|e| e.to_string())?.next().is_some() {
        return Err(format!("{} already has files. Run cargo wings-plugin new with a new or empty folder.", dir.display()));
    }
    let sdk = sdk.map(std::path::absolute).transpose().map_err(|e| e.to_string())?;
    if let Some(sdk) = &sdk {
        if !sdk.join("Cargo.toml").exists() {
            return Err(format!("--sdk {} has no Cargo.toml: it is the packages/plugin-rs folder of a Wings checkout.", sdk.display()));
        }
    }
    let name = name_of(&id);
    // Cargo wants a package name that doesn't start with a digit.
    let crate_name = if id.starts_with(|c: char| c.is_ascii_digit()) { format!("plugin-{id}") } else { id.clone() };
    let sdk_line = match &sdk {
        Some(path) => format!("wings-plugin = {{ path = {:?} }}", path.to_string_lossy()),
        None => format!("wings-plugin = \"{SDK_REQUIREMENT}\""),
    };
    for (path, text) in FILES {
        let mut text = text.to_string();
        match *path {
            "wings-plugin.json" | "README.md" => text = text.replace(STARTER_ID, &id).replace(STARTER_NAME, &name),
            "rust/Cargo.toml" => {
                text = text.replace(&format!("name = \"{STARTER_ID}\""), &format!("name = \"{crate_name}\""));
                text = text.lines().map(|l| if l.starts_with("wings-plugin = ") { sdk_line.as_str() } else { l }).collect::<Vec<_>>().join("\n") + "\n";
            }
            _ => {}
        }
        let file = dir.join(path);
        fs::create_dir_all(file.parent().unwrap_or(&dir)).map_err(|e| e.to_string())?;
        fs::write(&file, text).map_err(|e| format!("Couldn't write {}: {e}", file.display()))?;
    }
    log(&format!("Created {name} ({id}) in {}", dir.display()));
    log(&format!(
        "Next: in {}, rustup target add wasm32-unknown-unknown, then cargo wings-plugin build, or cargo wings-plugin dev to rebuild on each save and have the running Wings restart the plugin.",
        dir.display()
    ));
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::{frames_of, manifest_problems, read_manifest, tests::Scratch};

    fn read(dir: &Path, file: &str) -> String {
        fs::read_to_string(dir.join(file)).unwrap()
    }

    #[test]
    fn new_names_the_plugin_and_its_crate_after_the_folder_and_the_manifest_agrees_with_the_crate() {
        let root = Scratch::new("new");
        let dir = root.0.join("My Cool_Plugin");
        let mut lines = Vec::new();
        new_plugin(&dir, None, &mut |l| lines.push(l.to_string())).unwrap();
        let manifest = read_manifest(&dir).unwrap();
        assert_eq!(manifest["id"], "my-cool-plugin");
        assert_eq!(manifest["name"], "My cool plugin");
        let instructions = manifest["contributes"]["mcpInstructions"].as_str().unwrap();
        assert!(instructions.starts_with("My cool plugin: ") && instructions.contains("wings my-cool-plugin pane_folders"), "{instructions}");
        assert!(read(&dir, "rust/Cargo.toml").lines().any(|l| l == "name = \"my-cool-plugin\""));
        assert!(read(&dir, ".gitignore").lines().any(|l| l == "target/"));
        assert!(dir.join(".github/workflows/release.yml").exists());
        assert!(!read(&dir, "README.md").contains("hello-wings"));
        // The SDK comes from crates.io.
        assert!(read(&dir, "rust/Cargo.toml").lines().any(|l| l == "wings-plugin = \"0.2\""));
        // A main, a sidebar page and a tool, with a Cargo feature each frame.
        assert_eq!(frames_of(&manifest), ["dist/native/main.wasm", "dist/native/sidebar.wasm"]);
        assert_eq!(manifest_problems(&dir).unwrap(), Vec::<String>::new());
        assert_eq!(manifest["contributes"]["mcpTools"][0]["name"], "pane_folders");
        assert!(dir.join("rust/src/main_frame.rs").exists() && dir.join("rust/src/sidebar.rs").exists());
        assert!(lines[0].starts_with("Created My cool plugin (my-cool-plugin) in "));
    }

    #[test]
    fn the_release_workflow_installs_the_cargo_subcommand() {
        let root = Scratch::new("ci");
        let dir = root.0.join("ci");
        new_plugin(&dir, None, &mut |_| {}).unwrap();
        let workflow = read(&dir, ".github/workflows/release.yml");
        for line in ["cargo install cargo-wings-plugin --locked", "cargo wings-plugin build", "cargo wings-plugin check", "cargo wings-plugin pack"] {
            assert!(workflow.contains(line), "{line}");
        }
        assert!(!workflow.contains("npx") && !workflow.contains("node"));
    }

    #[test]
    fn sdk_points_the_crate_at_a_local_sdk_and_a_folder_that_isnt_one_is_refused() {
        let root = Scratch::new("sdk");
        let sdk = root.0.join("plugin-rs");
        fs::create_dir(&sdk).unwrap();
        fs::write(sdk.join("Cargo.toml"), "[package]\nname = \"wings-plugin\"\n").unwrap();
        let dir = root.0.join("app");
        new_plugin(&dir, Some(&sdk), &mut |_| {}).unwrap();
        assert!(read(&dir, "rust/Cargo.toml").contains(&format!("wings-plugin = {{ path = {:?} }}", sdk.to_string_lossy())));
        let error = new_plugin(&root.0.join("other"), Some(&root.0.join("nope")), &mut |_| {}).err().unwrap();
        assert!(error.contains("has no Cargo.toml"), "{error}");
        assert!(!root.0.join("other").exists());
    }

    #[test]
    fn an_id_that_starts_with_a_digit_gets_a_crate_name_cargo_takes() {
        let root = Scratch::new("digit");
        let dir = root.0.join("9lives");
        new_plugin(&dir, None, &mut |_| {}).unwrap();
        assert_eq!(read_manifest(&dir).unwrap()["id"], "9lives");
        assert!(read(&dir, "rust/Cargo.toml").lines().any(|l| l == "name = \"plugin-9lives\""));
    }

    #[test]
    fn new_refuses_a_folder_that_has_files_and_a_name_that_makes_no_id() {
        let root = Scratch::new("refuse");
        let full = root.0.join("full");
        fs::create_dir(&full).unwrap();
        fs::write(full.join("notes.txt"), "mine").unwrap();
        assert!(new_plugin(&full, None, &mut |_| {}).err().unwrap().contains("already has files"));
        assert_eq!(read(&full, "notes.txt"), "mine");
        assert!(new_plugin(&root.0.join("___"), None, &mut |_| {}).err().unwrap().contains("Can't make a plugin id"));
        let empty = root.0.join("empty");
        fs::create_dir(&empty).unwrap();
        assert_eq!(new_plugin(&empty, None, &mut |_| {}).unwrap(), "empty");
    }

    #[test]
    fn an_id_has_only_lowercase_letters_digits_and_dashes_up_to_64() {
        assert_eq!(id_of(Path::new("/x/Hello, World!")), "hello-world");
        assert_eq!(id_of(Path::new("/x/-a--b-")), "a-b");
        assert_eq!(id_of(Path::new(&format!("/x/{}", "a".repeat(70)))).len(), 64);
    }

    #[test]
    fn the_sdk_requirement_matches_the_sdk_in_this_repo() {
        // Only in the Wings checkout: the published crate has no packages/plugin-rs next to it.
        let Ok(toml) = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../plugin-rs/Cargo.toml")) else { return };
        let version = toml.lines().find_map(|l| l.strip_prefix("version = \"")).unwrap().trim_end_matches('"');
        assert!(version.starts_with(SDK_REQUIREMENT), "plugin-rs is {version}, new plugins ask for {SDK_REQUIREMENT}");
    }
}
