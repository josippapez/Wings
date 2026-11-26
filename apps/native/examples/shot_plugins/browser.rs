//! The Browser's tab and settings.
use super::*;

/// The Browser (`plugins/browser/rust`): its tab, whose web page WebKit draws over the empty place the page leaves for it
/// (no web page is drawn offscreen), and its settings page. The other browsers it lists are made-up profiles in a home
/// folder of the scene's own, never this Mac's.
pub(super) fn browser(out: PathBuf) {
    let project = out.join("fixture-project");
    std::fs::create_dir_all(&project).unwrap();
    let home = out.join("fixture-home");
    let chrome = home.join("Library/Application Support/Google/Chrome");
    for profile in ["Default", "Profile 1"] {
        std::fs::create_dir_all(chrome.join(profile)).unwrap();
        // Only that it is there: nothing reads it in a screenshot.
        std::fs::write(chrome.join(profile).join("Cookies"), b"").unwrap();
    }
    let names = json!({ "profile": { "info_cache": { "Default": { "name": "Work" }, "Profile 1": { "name": "Personal" } } } });
    std::fs::write(chrome.join("Local State"), names.to_string()).unwrap();
    // The scene runs alone in its process, so its home is the browsers' only home there.
    unsafe { std::env::set_var("HOME", &home) };
    let manifest: wings_plugins::manifest::Manifest =
        serde_json::from_str(include_str!("../../../../plugins/browser/wings-plugin.json")).unwrap();
    let module = |frame: &str| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../plugins/browser/dist/native/{frame}.wasm"));
        format!(
            "{}{}",
            wings_native::page::runtime::wasm::MARKER,
            path.canonicalize().expect("build the plugin: cd plugins && sh build-rust.sh browser").display()
        )
    };
    let storage = out.join("fixture-storage-browser");
    let _ = std::fs::remove_dir_all(&storage);
    let host = PluginHost::with_access(
        PluginConfig { id: "browser".into(), root: storage, scope: "shot".into() },
        Access {
            plugin: wings_plugins::manifest::Plugin { manifest, dir: project.clone(), icon_svg: None },
            builtin: true,
            env: std::sync::Arc::new(FixtureEnv(project)),
        },
    );
    let mut cx = app();
    let pages: [(&str, String, &str, f32); 2] = [("tab", module("browser"), "", 640.0), ("settings", module("settings"), "Search engine", 1100.0)];
    for width in [1000.0, 640.0] {
        for (name, source, expected, height) in &pages {
            let handle = cx
                .open_offscreen_window(size(px(width), px(*height)), |_, cx| cx.new(|cx| PageView::in_plugin(&host, source, cx)))
                .unwrap();
            let window = handle.into();
            handle.read_with(&cx, |view, _| view.set_visible(true)).unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            while handle.read_with(&cx, |view, _| view.trees_sent()).unwrap() == 0
                || !handle.read_with(&cx, |view, _| view.text().contains(expected)).unwrap()
            {
                assert!(Instant::now() < deadline, "page did not show {expected:?}: {}", handle.read_with(&cx, |view, _| view.text()).unwrap());
                wait(&mut cx, window, Duration::from_millis(20));
            }
            // The settings ask for the other browsers and extensions as they open.
            wait(&mut cx, window, Duration::from_millis(1500));
            save(&mut cx, window, out.join(format!("browser-{name}-{width}.png")));
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
    }
}
