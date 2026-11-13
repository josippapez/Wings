//! Which plugins the native app runs: the ones the desktop runs, and no others.

use std::fs;

use super::{
    DEV_SCOPE, FoundPage, WINGS_VERSION, discover,
    fixture::{Approval::*, Data},
    frame_file, page_source,
};
use crate::host::PluginPaths;

/// `WINGS_VERSION` with its last number raised by one: a plugin that needs a newer desktop than this one.
fn newer() -> String {
    let (head, patch) = WINGS_VERSION.rsplit_once('.').unwrap();
    format!("{head}.{}", patch.parse::<u64>().unwrap() + 1)
}

#[test]
fn the_version_plugins_are_checked_against_is_the_desktops() {
    let manifest = fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../desktop/src-tauri/Cargo.toml"),
    )
    .unwrap();
    assert!(
        manifest.contains(&format!("version = \"{WINGS_VERSION}\"")),
        "{WINGS_VERSION} isn't the desktop's version"
    );
    assert_ne!(WINGS_VERSION, env!("CARGO_PKG_VERSION"));
}

#[test]
fn only_plugins_that_are_on_approved_and_built_for_native_are_found() {
    let data = Data::temp();
    // `ready` asks for exactly the desktop's version, which is enough.
    data.install("ready", WINGS_VERSION, true, true, Current, "scope-ready");
    data.install("off", WINGS_VERSION, true, false, Current, "scope-off");
    data.install("unapproved", WINGS_VERSION, true, true, Never, "scope-un");
    data.install("stale", WINGS_VERSION, true, true, Stale, "scope-stale");
    data.install("web-only", WINGS_VERSION, false, true, Current, "scope-web");
    data.install("too-new", &newer(), true, true, Current, "scope-new");
    let state = fs::read(data.paths().state).unwrap();
    let before = fs::read_dir(data.path()).unwrap().count();

    let found = discover(&data.paths());

    assert_eq!(found.len(), 1, "{found:?}");
    let ready = &found[0];
    assert_eq!(
        (ready.id.as_str(), ready.name.as_str()),
        ("ready", "Plugin ready")
    );
    // The desktop's scope, so the storage is the desktop's.
    assert_eq!(ready.scope, "scope-ready");
    assert!(ready.main.ends_with("plugins/ready/dist/native/main.wasm"));
    // Every view that opens as a tab and has a native page can open; only one that sets `newTab` is offered as a new tab.
    assert_eq!(
        ready.pages,
        [
            FoundPage {
                id: "fixture".into(),
                title: "Fixture".into(),
                bundle: ready.main.with_file_name("fixture.wasm"),
                multiple: false,
                new_tab: true,
            },
            FoundPage {
                id: "plain".into(),
                title: "Plain".into(),
                bundle: ready.main.with_file_name("plain.wasm"),
                multiple: false,
                new_tab: false,
            }
        ]
    );
    // Reading never writes the desktop's state or leaves a file beside it.
    assert_eq!(fs::read(data.paths().state).unwrap(), state);
    assert_eq!(fs::read_dir(data.path()).unwrap().count(), before);
}

#[test]
fn a_folder_the_desktop_never_turned_on_is_off() {
    let data = Data::temp();
    let dir = data.install("fresh", WINGS_VERSION, true, true, Current, "scope");
    // Installed, but `plugins.json` has no entry for it: the desktop shows it off, awaiting approval.
    fs::write(data.paths().state, "{}").unwrap();
    assert!(dir.is_dir());
    assert!(discover(&data.paths()).is_empty());
    // And with no `plugins.json` at all, nothing runs.
    fs::remove_file(data.paths().state).unwrap();
    assert!(discover(&data.paths()).is_empty());
}

/// A plugin folder of the dev root: a manifest, a web main and a native one, and a sidebar page with a native build.
fn dev_plugin(root: &std::path::Path, id: &str, sidebars: &[&str]) {
    let dir = root.join(id);
    fs::create_dir_all(dir.join("dist/native")).unwrap();
    let sidebar_list: Vec<_> = sidebars
        .iter()
        .map(|name| serde_json::json!({ "id": name, "title": name.to_uppercase(), "page": format!("dist/{name}.html") }))
        .collect();
    let manifest = serde_json::json!({
        "id": id, "name": format!("Dev {id}"), "version": "1.0.0", "api": 1, "main": "dist/main.js",
        "permissions": { "ports": true }, "contributes": { "sidebars": sidebar_list },
    });
    fs::write(dir.join("wings-plugin.json"), manifest.to_string()).unwrap();
    fs::write(dir.join("dist/main.js"), "").unwrap();
    fs::write(dir.join("dist/native/main.wasm"), b"\0asm\x01\0\0\0").unwrap();
    for name in sidebars {
        fs::write(dir.join("dist").join(format!("{name}.html")), "").unwrap();
    }
}

#[test]
fn plugins_of_the_dev_root_run_with_no_approval_and_a_scope_the_desktop_never_wrote() {
    let data = Data::temp();
    let dev = data.path().join("dev-plugins");
    dev_plugin(&dev, "fresh", &[]);
    // The desktop knows this one and turned it on: it runs under the scope the desktop gave it, so it shares its storage.
    dev_plugin(&dev, "known", &[]);
    data.install("known", WINGS_VERSION, true, true, Current, "desktop-scope");
    // And this one it turned off, which a dev build respects as the desktop's own does.
    dev_plugin(&dev, "off", &[]);
    data.install("off", WINGS_VERSION, true, false, Current, "off-scope");
    let paths = PluginPaths {
        dev: Some(dev),
        ..data.paths()
    };
    let state = fs::read(paths.state.clone()).unwrap();

    let found = discover(&paths);
    let by_id = |id: &str| {
        found
            .iter()
            .find(|plugin| plugin.id == id)
            .unwrap_or_else(|| panic!("{found:?}"))
    };
    assert_eq!(by_id("fresh").scope, DEV_SCOPE);
    assert_eq!(by_id("known").scope, "desktop-scope");
    assert!(found.iter().all(|plugin| plugin.id != "off"), "{found:?}");
    // Wings' own, as the desktop counts a plugin of the repo it loads: it may make the calls those may.
    assert!(by_id("fresh").builtin && by_id("known").builtin);
    // Nothing was written to the desktop's state, and without the dev root neither of them runs.
    assert_eq!(fs::read(&paths.state).unwrap(), state);
    assert_eq!(
        discover(&data.paths()).len(),
        1,
        "only the plugin the desktop runs"
    );
}

#[test]
fn a_plugin_that_is_installed_but_not_from_the_dev_root_is_not_builtin() {
    let data = Data::temp();
    data.install("ready", WINGS_VERSION, true, true, Current, "scope");
    assert!(!discover(&data.paths())[0].builtin);
}

#[test]
fn a_plugin_that_ships_native_builds_of_its_sidebars_has_them_found_and_one_without_is_left_out() {
    let data = Data::temp();
    let dev = data.path().join("dev-plugins");
    dev_plugin(&dev, "sided", &["board", "hud"]);
    fs::remove_file(dev.join("sided/dist/native/hud.wasm")).ok();
    fs::write(dev.join("sided/dist/native/board.wasm"), b"\0asm\x01\0\0\0").unwrap();
    let found = discover(&PluginPaths {
        dev: Some(dev.clone()),
        ..data.paths()
    });
    assert_eq!(found.len(), 1);
    // `dist/board.html` is `board.wasm` beside the main, as for a view; `hud` has none and is skipped.
    assert_eq!(
        found[0].sidebars,
        [FoundPage {
            id: "board".into(),
            title: "BOARD".into(),
            bundle: found[0].main.with_file_name("board.wasm"),
            multiple: false,
            new_tab: false,
        }]
    );
    assert!(found[0].pages.is_empty());
}

/// A page is a WebAssembly module only when its `.wasm` is in the plugin's folder: a script that starts with the marker,
/// naming a module anywhere on disk, is refused, and so is a `.wasm` that is a link out of the folder.
#[cfg(unix)]
#[test]
fn only_a_module_in_the_plugin_folder_runs_as_a_webassembly_page() {
    use crate::page::runtime::wasm::MARKER;
    let dir = tempfile::tempdir().unwrap();
    let native = dir.path().join("dist/native");
    std::fs::create_dir_all(&native).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let elsewhere = outside.path().join("other.wasm");
    std::fs::write(&elsewhere, b"\0asm\x01\0\0\0").unwrap();

    let faked = native.join("faked.js");
    std::fs::write(&faked, format!("{MARKER}{}", elsewhere.display())).unwrap();
    assert!(page_source(dir.path(), &faked).is_err());

    let linked = native.join("linked.wasm");
    std::os::unix::fs::symlink(&elsewhere, &linked).unwrap();
    assert!(page_source(dir.path(), &linked).is_err());

    let built = native.join("built.wasm");
    std::fs::write(&built, b"\0asm\x01\0\0\0").unwrap();
    let source = page_source(dir.path(), &built).unwrap();
    assert!(source.starts_with(MARKER) && source.ends_with("built.wasm"), "{source}");
}

/// A plugin written in Rust alone ships `main.wasm` and no `main.js`: it is found by that file, which is what runs. A
/// `main.wasm` that is a link out of the plugin's folder is no build at all.
#[cfg(unix)]
#[test]
fn a_frame_with_only_a_webassembly_build_is_found_by_it() {
    use crate::page::runtime::wasm::MARKER;
    let dir = tempfile::tempdir().unwrap();
    let native = dir.path().join("dist/native");
    std::fs::create_dir_all(&native).unwrap();
    assert_eq!(frame_file(dir.path(), "main"), None);

    let outside = tempfile::tempdir().unwrap();
    let elsewhere = outside.path().join("main.wasm");
    std::fs::write(&elsewhere, b"\0asm\x01\0\0\0").unwrap();
    std::os::unix::fs::symlink(&elsewhere, native.join("main.wasm")).unwrap();
    assert_eq!(frame_file(dir.path(), "main"), None);

    std::fs::remove_file(native.join("main.wasm")).unwrap();
    std::fs::write(native.join("main.wasm"), b"\0asm\x01\0\0\0").unwrap();
    let main = frame_file(dir.path(), "main").expect("the module");
    let source = page_source(dir.path(), &main).unwrap();
    assert!(source.starts_with(MARKER) && source.ends_with("main.wasm"), "{source}");

    // A script beside it is no frame.
    std::fs::write(native.join("main.js"), "").unwrap();
    assert!(frame_file(dir.path(), "main").unwrap().ends_with("main.wasm"));
    std::fs::remove_file(native.join("main.wasm")).unwrap();
    assert_eq!(frame_file(dir.path(), "main"), None);
}

/// A plugin with only `dist/native/main.js` is not found, and so starts no frame.
#[test]
fn a_plugin_with_only_a_javascript_build_is_not_found() {
    let data = Data::temp();
    let dir = data.install("old", WINGS_VERSION, false, true, Current, "scope");
    fs::write(dir.join("dist/native/main.js"), "wings.render('{}');").unwrap();
    assert!(discover(&data.paths()).is_empty());
}

/// A plugin written in Rust alone ships no JavaScript at all: its manifest names its main and its pages by their
/// modules, which the desktop's loading takes as the files they are, and those are the frames that run.
#[test]
fn a_plugin_with_no_javascript_is_loaded_and_its_frames_are_its_modules() {
    use crate::page::{plugin::wasm_tests::module, runtime::wasm::MARKER};
    use std::{path::PathBuf, sync::OnceLock};
    static MAIN: OnceLock<PathBuf> = OnceLock::new();
    static PAGE: OnceLock<PathBuf> = OnceLock::new();
    let built = |source: String| PathBuf::from(source.strip_prefix(MARKER).unwrap());
    let package = tempfile::tempdir().unwrap();
    let native = package.path().join("dist/native");
    fs::create_dir_all(&native).unwrap();
    fs::copy(built(module("fixture-wasm", "fixture_main.wasm", &MAIN)), native.join("main.wasm")).unwrap();
    fs::copy(built(module("fixture-wasm-page", "fixture_page.wasm", &PAGE)), native.join("side.wasm")).unwrap();
    fs::write(
        package.path().join("wings-plugin.json"),
        serde_json::json!({
            "id": "rusty", "name": "Rusty", "version": "1.0.0", "api": 1, "main": "dist/native/main.wasm",
            "contributes": { "sidebars": [{ "id": "side", "title": "Side", "icon": "clock", "page": "dist/native/side.wasm" }] },
        })
        .to_string(),
    )
    .unwrap();

    let data = Data::temp();
    let dir = data.install_folder(package.path(), "scope");
    let found = discover(&data.paths());
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].main.ends_with("dist/native/main.wasm"), "{:?}", found[0].main);
    let [side] = &found[0].sidebars[..] else { panic!("{found:?}") };
    assert!(side.bundle.ends_with("dist/native/side.wasm"), "{:?}", side.bundle);
    for frame in [&found[0].main, &side.bundle] {
        let source = page_source(&dir, frame).unwrap();
        assert!(source.starts_with(MARKER) && source.ends_with(".wasm"), "{source}");
    }
}
