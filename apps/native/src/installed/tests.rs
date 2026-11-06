//! Which plugins the native app runs: the ones the desktop runs, and no others.

use std::fs;

use super::{
    DEV_SCOPE, FoundPage, WINGS_VERSION, discover,
    fixture::{Approval::*, Data},
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
    assert!(ready.main.ends_with("plugins/ready/dist/native/main.js"));
    // Every view that opens as a tab and has a native page can open; only one that sets `newTab` is offered as a new tab.
    assert_eq!(
        ready.pages,
        [
            FoundPage {
                id: "fixture".into(),
                title: "Fixture".into(),
                bundle: ready.main.with_file_name("fixture.js"),
                multiple: false,
                new_tab: true,
            },
            FoundPage {
                id: "plain".into(),
                title: "Plain".into(),
                bundle: ready.main.with_file_name("plain.js"),
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
    fs::write(dir.join("dist/native/main.js"), "").unwrap();
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
    fs::remove_file(dev.join("sided/dist/native/hud.js")).ok();
    fs::write(dev.join("sided/dist/native/board.js"), "").unwrap();
    let found = discover(&PluginPaths {
        dev: Some(dev.clone()),
        ..data.paths()
    });
    assert_eq!(found.len(), 1);
    // `dist/board.html` is `board.js` beside the main, as for a view; `hud` has none and is skipped.
    assert_eq!(
        found[0].sidebars,
        [FoundPage {
            id: "board".into(),
            title: "BOARD".into(),
            bundle: found[0].main.with_file_name("board.js"),
            multiple: false,
            new_tab: false,
        }]
    );
    assert!(found[0].pages.is_empty());
}
