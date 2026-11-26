//! The Browser's tab and settings.
use super::*;

/// The Browser (`plugins/browser/rust`): its tab, whose web page WebKit draws over the empty place the page leaves for it
/// (no web page is drawn offscreen), and its settings page. The other browsers it lists are made-up profiles in a home
/// folder of the scene's own, never this Mac's.
pub(super) fn browser(out: PathBuf) {
    let project = out.join("fixture-project");
    std::fs::create_dir_all(&project).unwrap();
    let project_dir = project.clone();
    let home = out.join("fixture-home");
    let chrome = home.join("Library/Application Support/Google/Chrome");
    for profile in ["Default", "Profile 1"] {
        std::fs::create_dir_all(chrome.join(profile)).unwrap();
        // Only that it is there: nothing reads it in a screenshot.
        std::fs::write(chrome.join(profile).join("Cookies"), b"").unwrap();
    }
    let names = json!({ "profile": { "info_cache": { "Default": { "name": "Work" }, "Profile 1": { "name": "Personal" } } } });
    std::fs::write(chrome.join("Local State"), names.to_string()).unwrap();
    // Extensions of the Work profile, which Look lists: a folder each with a manifest and a 128 point icon.
    let extensions = chrome.join("Default/Extensions");
    let made = [
        ("Bitwarden Password Manager", "At home, at work, or on the go, Bitwarden easily secures all your passwords.\nSecond line.", [30, 90, 200]),
        ("uBlock Origin", "Finally, an efficient wide-spectrum content blocker. Easy on CPU and memory.", [170, 40, 40]),
        ("Dark Reader", "Dark mode for every website. Take care of your eyes, use dark theme for night and daily browsing.", [40, 40, 60]),
        ("React Developer Tools", "Adds React debugging tools to the Chrome Developer Tools.", [20, 150, 180]),
    ];
    for (at, (name, description, colour)) in made.iter().enumerate() {
        let folder = extensions.join(format!("{}{}", "abcdefghijklmnopabcdefghijklmn", ["ab", "bc", "cd", "de"][at])).join("1.0_0");
        std::fs::create_dir_all(&folder).unwrap();
        let manifest = json!({ "manifest_version": 3, "name": name, "version": "1.0", "description": description, "icons": { "128": "icon.png" } });
        std::fs::write(folder.join("manifest.json"), manifest.to_string()).unwrap();
        image::RgbaImage::from_pixel(128, 128, image::Rgba([colour[0], colour[1], colour[2], 255])).save(folder.join("icon.png")).unwrap();
    }
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
    // Pages visited, newest first, for the start page.
    let visited = [
        ("https://github.com/acme/web/pull/482", "acme/web \u{b7} Pull request #482"),
        ("https://vite.dev/config/", "Configuring Vite"),
        ("http://localhost:5173/", "acme web"),
        ("https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Intl/NumberFormat", "Intl.NumberFormat - JavaScript | MDN"),
        ("https://docs.stripe.com/testing", "Test cards"),
    ];
    let history: Vec<_> = visited.iter().map(|(url, title)| json!({ "url": url, "title": title, "at": 1.0 })).collect();
    wings_plugin_storage::Storage::new(storage.clone()).set("browser", "shot", "history", json!(history)).unwrap();
    let host = PluginHost::with_access(
        PluginConfig { id: "browser".into(), root: storage, scope: "shot".into() },
        Access {
            plugin: wings_plugins::manifest::Plugin { manifest, dir: project.clone(), icon_svg: None },
            builtin: true,
            env: std::sync::Arc::new(FixtureEnv(project)),
        },
    );
    let id = project_dir.display().to_string();
    host.publish_projects(vec![wings_native::page::plugin::Project { hidden_sections: Vec::new(), id: id.clone(), name: "acme-web".into(), parent_id: None, path: id }]);
    // The terminal the start page's dev servers listen in, which is the project's own.
    host.publish_panes(vec![PaneEntry {
        pane_id: "shot-web".into(),
        cwd: project_dir.display().to_string(),
        command: "pnpm".into(),
        project: "acme-web".into(),
        project_id: project_dir.display().to_string(),
        session: None,
        focused: false,
    }]);
    // Two extensions' buttons on a page, as WebKit tells the tab of them: a password manager with a count and one with
    // no count. Their icons are drawn here, never taken from an extension on this Mac.
    let icons = [("ext-shot-vault", [23, 93, 220]), ("ext-shot-react", [88, 196, 220])];
    for (id, colour) in icons {
        let picture = image::RgbaImage::from_fn(32, 32, |x, y| {
            let (dx, dy) = (x as f32 - 15.5, y as f32 - 15.5);
            let inside = if id == "ext-shot-vault" { dx.abs() < 13.0 && dy.abs() < 13.0 } else { dx * dx + dy * dy < 15.0 * 15.0 };
            let dot = dx * dx + dy * dy < 5.0 * 5.0;
            match (inside, dot) {
                (true, true) => image::Rgba([255, 255, 255, 255]),
                (true, false) => image::Rgba([colour[0], colour[1], colour[2], 255]),
                _ => image::Rgba([0, 0, 0, 0]),
            }
        });
        let mut png = Vec::new();
        picture.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        wings_native::page::images::register_bytes("browser", id, png).unwrap();
    }
    let page_state = json!({ "url": "https://github.com/acme/web/pull/482", "title": "acme/web · Pull request #482", "extensions": [
        { "id": "18f2a9c0b1", "name": "Bitwarden Password Manager", "label": "Bitwarden", "icon": "ext-shot-vault", "badge": "3", "enabled": true },
        { "id": "18f2a9c0b2", "name": "React Developer Tools", "label": "React Developer Tools", "icon": "ext-shot-react", "badge": "", "enabled": true },
    ] });
    let page_args = json!({ "url": "https://github.com/acme/web/pull/482" });
    let mut cx = app();
    let pages: [(&str, String, &str, f32); 3] =
        [("tab", module("browser"), "", 640.0), ("page", module("browser"), "", 640.0), ("settings", module("settings"), "Search engine", 1100.0)];
    for width in [1000.0, 640.0] {
        for (name, source, expected, height) in &pages {
            let args = (*name == "page").then_some(&page_args);
            let handle = cx
                .open_offscreen_window(size(px(width), px(*height)), |_, cx| cx.new(|cx| PageView::in_plugin_with_args(&host, source, args, cx)))
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
            // A page's web view tells it where it is and its extensions' buttons.
            if args.is_some() {
                while !handle.read_with(&cx, |view, _| view.tell_web_state("page", &page_state)).unwrap() {
                    assert!(Instant::now() < deadline, "the page has no web view");
                    wait(&mut cx, window, Duration::from_millis(20));
                }
            }
            // The settings ask for the other browsers and extensions as they open.
            wait(&mut cx, window, Duration::from_millis(1500));
            save(&mut cx, window, out.join(format!("browser-{name}-{width}.png")));
            if *name == "settings" {
                // Look, as a click on it does, then Add on the first extension it lists.
                assert!(handle.read_with(&cx, |view, _| view.press("Look", 0)).unwrap(), "no Look button");
                wait(&mut cx, window, Duration::from_millis(1500));
                save(&mut cx, window, out.join(format!("browser-settings-look-{width}.png")));
            }
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
    }
}
