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
    // Brave, readable when Chrome's folder is locked in the refused-access states.
    let brave = home.join("Library/Application Support/BraveSoftware/Brave-Browser");
    let folder = brave.join("Default/Extensions/abcdefghijklmnopabcdefghijklmnef/1.0_0");
    std::fs::create_dir_all(&folder).unwrap();
    let manifest = json!({ "manifest_version": 3, "name": "Privacy Badger", "version": "1.0", "description": "Automatically learns to block invisible trackers." });
    std::fs::write(folder.join("manifest.json"), manifest.to_string()).unwrap();
    // An extension already added, to ask before removing it. Unsealed, as nothing here loads it.
    let profile = out.join("plugin-web/browser.shot");
    std::fs::create_dir_all(&profile).unwrap();
    let added = json!([{ "id": "shotvault", "name": "Bitwarden Password Manager", "version": "1.0", "description": "A password manager", "icon": null,
        "permissions": [], "hosts": [], "enabled": true }]);
    std::fs::write(profile.join("extensions.json"), added.to_string()).unwrap();
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
    // What a tab kept: the person's own tabs, an agent's group of two, and another agent's, folded.
    let tabs_kept = json!({
        "tabs": [
            { "id": "t1", "url": "https://github.com/acme/web/pull/482", "title": "acme/web \u{b7} Pull request #482", "group": null },
            { "id": "t2", "url": "https://vite.dev/config/", "title": "Configuring Vite", "group": null },
            { "id": "t3", "url": "", "title": "", "group": null },
            { "id": "t4", "url": "http://localhost:5173/login", "title": "Sign in \u{b7} acme web", "group": "ses-login" },
            { "id": "t5", "url": "https://docs.stripe.com/testing", "title": "Test cards", "group": "ses-login" },
            { "id": "t6", "url": "https://developer.mozilla.org/en-US/docs/Web/API/Fetch_API", "title": "Fetch API - Web APIs | MDN", "group": "ses-docs" },
            { "id": "t7", "url": "https://vite.dev/guide/", "title": "Getting Started | Vite", "group": "ses-docs" },
        ],
        "groups": [
            { "key": "ses-login", "name": "Fix login bug", "collapsed": false, "active": "t4" },
            { "key": "ses-docs", "name": "Write the API docs", "collapsed": true, "active": "t6" },
        ],
        "active": "t1", "own": "t1", "next": 8, "args": "",
    });
    let mut cx = app();
    let pages: [(&str, String, &str, f32); 4] = [
        ("tab", module("browser"), "", 640.0),
        ("page", module("browser"), "", 640.0),
        ("tabs", module("browser"), "", 640.0),
        ("settings", module("settings"), "Search engine", 1100.0),
    ];
    for width in [1000.0, 640.0] {
        for (name, source, expected, height) in &pages {
            let args = (*name == "page").then_some(&page_args);
            let kept = (*name == "tabs").then(|| tabs_kept.clone());
            let handle = cx
                .open_offscreen_window(size(px(width), px(*height)), |_, cx| {
                    cx.new(|cx| match kept {
                        Some(kept) => {
                            let state = wings_native::page::plugin::FrameState::new(wings_native::page::plugin::StatePlace::Tab, Some(kept), |_| Ok(()));
                            PageView::in_plugin_with_state(&host, source, args, state, cx)
                        }
                        None => PageView::in_plugin_with_args(&host, source, args, cx),
                    })
                })
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
                while !handle.read_with(&cx, |view, _| view.tell_web_state("tab-t1", &page_state)).unwrap() {
                    assert!(Instant::now() < deadline, "the page has no web view");
                    wait(&mut cx, window, Duration::from_millis(20));
                }
            }
            // The settings ask for the other browsers and extensions as they open.
            wait(&mut cx, window, Duration::from_millis(1500));
            save(&mut cx, window, out.join(format!("browser-{name}-{width}.png")));
            if *name == "page" {
                // The More menu of a web page, its item's question, and what the fixture's host (no web engines) answers.
                // A menu's button has no click of its own for the page: the app opens the menu.
                assert!(handle.update(&mut cx, |view, _, cx| view.open_menu("More", 0, cx)).unwrap(), "no More menu");
                wait(&mut cx, window, Duration::from_millis(500));
                save(&mut cx, window, out.join(format!("browser-page-menu-{width}.png")));
                assert!(handle.read_with(&cx, |view, _| view.press("Clear data for github.com", 0)).unwrap(), "no Clear data item");
                wait(&mut cx, window, Duration::from_millis(500));
                save(&mut cx, window, out.join(format!("browser-page-clear-{width}.png")));
                assert!(handle.read_with(&cx, |view, _| view.press("Clear", 0)).unwrap(), "no Clear button");
                wait(&mut cx, window, Duration::from_millis(1000));
                save(&mut cx, window, out.join(format!("browser-page-cleared-{width}.png")));
            }
            if *name == "settings" {
                // Look, as a click on it does, then Add on the first extension it lists.
                assert!(handle.read_with(&cx, |view, _| view.press("Look", 0)).unwrap(), "no Look button");
                wait(&mut cx, window, Duration::from_millis(1500));
                save(&mut cx, window, out.join(format!("browser-settings-look-{width}.png")));
                // The trash icon of the added extension asks in a dialog over the page.
                assert!(handle.read_with(&cx, |view, _| view.press("Remove Bitwarden Password Manager", 0)).unwrap(), "no Remove button");
                wait(&mut cx, window, Duration::from_millis(500));
                save(&mut cx, window, out.join(format!("browser-settings-remove-{width}.png")));
                // Add failing, as when an extension's folder went after Look listed it: the error shows as a toast over the page's foot.
                assert!(handle.read_with(&cx, |view, _| view.press("Cancel", 0)).unwrap(), "no Cancel button");
                let manifests: Vec<_> = std::fs::read_dir(&extensions).unwrap().flatten().map(|folder| folder.path().join("1.0_0/manifest.json")).collect();
                let kept: Vec<_> = manifests.iter().map(|path| (path, std::fs::read(path).unwrap())).collect();
                manifests.iter().for_each(|path| std::fs::remove_file(path).unwrap());
                assert!(handle.read_with(&cx, |view, _| view.press("Add", 0)).unwrap(), "no Add button");
                wait(&mut cx, window, Duration::from_millis(1500));
                save(&mut cx, window, out.join(format!("browser-settings-error-{width}.png")));
                // The close button takes it away; then with the page scrolled to its end the same error shows over the lower
                // part of the page, where a notice at the page's foot used to be the only place it showed.
                assert!(handle.read_with(&cx, |view, _| view.press("Dismiss", 0)).unwrap(), "no Dismiss button");
                wait(&mut cx, window, Duration::from_millis(300));
                cx.simulate_event(window, gpui::ScrollEvent {
                    position: point(px(width / 2.0), px(300.0)),
                    delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(-100_000.0))),
                    modifiers: gpui::Modifiers::default(),
                    touch_phase: gpui::TouchPhase::Moved,
                    source: Default::default(),
                });
                wait(&mut cx, window, Duration::from_millis(300));
                assert!(handle.read_with(&cx, |view, _| view.press("Add", 0)).unwrap(), "no Add button");
                wait(&mut cx, window, Duration::from_millis(1500));
                save(&mut cx, window, out.join(format!("browser-settings-error-scrolled-{width}.png")));
                kept.into_iter().for_each(|(path, bytes)| std::fs::write(path, bytes).unwrap());
            }
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
    }
    // macOS refusing a browser's folder (what a missing Full Disk Access does), as `chmod 000` does: Chrome alone, so
    // Brave's extension is listed with a notice, then both, so there is nothing to list.
    use std::os::unix::fs::PermissionsExt;
    let lock = |paths: &[&std::path::Path], mode: u32| {
        for path in paths {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        }
    };
    for width in [1000.0, 640.0] {
        for (name, locked) in [("partial", vec![chrome.as_path()]), ("refused", vec![chrome.as_path(), brave.as_path()])] {
            lock(&locked, 0o000);
            let handle = cx
                .open_offscreen_window(size(px(width), px(1700.0)), |_, cx| cx.new(|cx| PageView::in_plugin_with_args(&host, &module("settings"), None, cx)))
                .unwrap();
            let window = handle.into();
            handle.read_with(&cx, |view, _| view.set_visible(true)).unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            while handle.read_with(&cx, |view, _| view.trees_sent()).unwrap() == 0 || !handle.read_with(&cx, |view, _| view.text().contains("Search engine")).unwrap() {
                assert!(Instant::now() < deadline, "the settings did not show");
                wait(&mut cx, window, Duration::from_millis(20));
            }
            wait(&mut cx, window, Duration::from_millis(1500));
            assert!(handle.read_with(&cx, |view, _| view.press("Look", 0)).unwrap(), "no Look button");
            wait(&mut cx, window, Duration::from_millis(1500));
            save(&mut cx, window, out.join(format!("browser-settings-{name}-{width}.png")));
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
            lock(&locked, 0o700);
        }
    }
    // The Engine section, in each of its states. Each state has a home of its own, which is where this build keeps the
    // engine (`web::engine::root`), so nothing here reads or writes this Mac's. Wings does the real calls: the download is
    // a file or a pipe that never opens (it stays on "Downloading"), and what fails is a file that isn't the pinned archive.
    let engine_root = |home: &std::path::Path| home.join("Library/Application Support/Wings Native/chromium");
    let fifo = out.join("fixture-engine-pipe");
    let _ = std::fs::remove_file(&fifo);
    assert!(std::process::Command::new("mkfifo").arg(&fifo).status().unwrap().success(), "mkfifo");
    let garbage = out.join("fixture-engine-garbage.bin");
    std::fs::write(&garbage, b"not the archive").unwrap();
    // (state, installed and on, the file Download reads, whether to press Download first). Last, "broken": installed and
    // on, but its files failed the check as Chromium started, which Wings remembers for the rest of the run.
    let states: [(&str, bool, Option<&std::path::Path>, bool); 5] = [
        ("webkit", false, None, false),
        ("downloading", false, Some(fifo.as_path()), true),
        ("on", true, None, false),
        ("failed", false, Some(garbage.as_path()), true),
        ("broken", true, None, false),
    ];
    for (name, on, archive, download) in &states {
        for width in [1000.0, 640.0] {
            let home = out.join(format!("fixture-home-engine-{name}"));
            let _ = std::fs::remove_dir_all(&home);
            if *on {
                let version = engine_root(&home).join(wings_native::web::engine::PINNED.version);
                std::fs::create_dir_all(version.join("Wings Chromium.app")).unwrap();
                std::fs::write(version.join("finished"), b"").unwrap();
                std::fs::write(engine_root(&home).join("choice.json"), json!({ "browser.shot": "chromium" }).to_string()).unwrap();
                // The fixture's empty bundle is not the engine Wings pins.
                if *name == "broken" {
                    assert!(wings_native::web::engine::verify_bundle(&version.join("Wings Chromium.app")).is_err());
                }
            }
            std::fs::create_dir_all(&home).unwrap();
            unsafe { std::env::set_var("HOME", &home) };
            match archive {
                Some(archive) => unsafe { std::env::set_var("WINGS_CHROMIUM_ARCHIVE", archive) },
                None => unsafe { std::env::remove_var("WINGS_CHROMIUM_ARCHIVE") },
            }
            let handle = cx
                .open_offscreen_window(size(px(width), px(1100.0)), |_, cx| cx.new(|cx| PageView::in_plugin_with_args(&host, &module("settings"), None, cx)))
                .unwrap();
            let window = handle.into();
            handle.read_with(&cx, |view, _| view.set_visible(true)).unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            while handle.read_with(&cx, |view, _| view.trees_sent()).unwrap() == 0 || !handle.read_with(&cx, |view, _| view.text().contains("Built in.")).unwrap() {
                assert!(Instant::now() < deadline, "the engine section did not show: {}", handle.read_with(&cx, |view, _| view.text()).unwrap());
                wait(&mut cx, window, Duration::from_millis(20));
            }
            if *download {
                assert!(handle.read_with(&cx, |view, _| view.press("Download", 0)).unwrap(), "no Download button");
                wait(&mut cx, window, Duration::from_millis(1500));
            }
            save(&mut cx, window, out.join(format!("browser-engine-{name}-{width}.png")));
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
    }
}
