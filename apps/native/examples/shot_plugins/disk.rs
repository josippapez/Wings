//! Disk usage's overview and the map of a folder of made-up files.
use super::*;

/// Disk usage's real main and page modules: the disks are this Mac's, and the map is of a folder of made-up files, which it
/// scans for real. The folder is removed afterwards.
pub(super) fn disk(out: PathBuf) {
    let folder = out.join("fixture-disk");
    let _ = std::fs::remove_dir_all(&folder);
    // Files of the sizes (in MiB) that give the map a few rings, written out so that they take space on disk.
    for (path, mib) in [
        ("Downloads/installer.dmg", 11),
        ("Downloads/archive.zip", 6),
        ("Downloads/old/backup.tar", 4),
        ("Code/app/target/debug.bin", 9),
        ("Code/app/node_modules/react.bin", 3),
        ("Code/app/node_modules/vite.bin", 2),
        ("Photos/2025/trip.raw", 5),
        ("Photos/2025/party.raw", 3),
        ("Movies/clip.mov", 7),
    ] {
        let file = folder.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let mut data = std::fs::File::create(file).unwrap();
        for _ in 0..mib {
            std::io::Write::write_all(&mut data, &vec![7u8; 1 << 20]).unwrap();
        }
    }
    std::fs::write(folder.join("notes.txt"), "made up\n").unwrap();
    let id = folder.to_string_lossy().into_owned();
    let module = |frame: &str| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../plugins/disk/dist/native/{frame}.wasm"));
        format!("{}{}", wings_native::page::runtime::wasm::MARKER, path.canonicalize().expect("sh build-rust.sh disk, in plugins/").display())
    };
    let fail = |event: wings_native::page::runtime::Event| {
        if let wings_native::page::runtime::Event::Error { message, .. } = event {
            panic!("{message}");
        }
    };
    let project = |name: &str, path: String| wings_native::page::plugin::Project { hidden_sections: Vec::new(), id: path.clone(), name: name.into(), parent_id: None, path };
    let host = disk_host(&out, "fixture-storage-disk", &folder, vec![project("Fixture", id)]);
    host.start_main(&module("main"), fail).unwrap();
    let mut cx = app();
    let source = module("map");
    // The Scan buttons come in the page's order: a disk's each, then the home folder's, then the projects'.
    let scan = wings_disk_usage::disks().len() + 1;
    // What to press to reach each view, one after the other with the text each shows, and what undoes it: main keeps
    // the Collector between windows.
    let views: [(&str, Steps, Steps); 4] = [
        ("overview", vec![], vec![]),
        ("map", vec![("Scan", scan, "Downloads")], vec![]),
        ("menu", vec![("Scan", scan, "Downloads"), ("⋯", 0, "Add to Collector")], vec![]),
        (
            "collected",
            vec![
                ("Scan", scan, "Downloads"),
                ("Add to Collector", 0, "1 item"),
            ],
            vec![("Remove from Collector", 0, "Drag folders")],
        ),
    ];
    shoot_disk(&mut cx, &host, &source, &out, &views);
    // The other states of the tab, which a scan of a few files is too quick to show: a main that says what the page
    // hears, as the real one does, for a scan in progress, one that failed and one that has not begun.
    let states = disk_host(&out, "fixture-storage-disk-states", &folder, vec![project("Slow", "/fixture/slow".into()), project("Broken", "/fixture/broken".into()), project("Quiet", "/fixture/quiet".into())]);
    states.start_main(&disk_states_main(), fail).unwrap();
    let views: [(&str, Steps, Steps); 3] = [
        ("scanning", vec![("Scan", scan, "Found so far")], vec![]),
        ("error", vec![("Scan", scan + 1, "Try again")], vec![]),
        ("starting", vec![("Scan", scan + 2, "Starting the scan")], vec![]),
    ];
    shoot_disk(&mut cx, &states, &source, &out, &views);
    let _ = std::fs::remove_dir_all(&folder);
}

/// A stand-in for Disk usage's main that tells its page what the real one tells it, for the states a scan of a few files
/// is too quick to show: a scan in progress (the "Slow" project), one that failed ("Broken"), and one that is asked for and
/// not begun ("Quiet"), which main neither shows nor answers.
pub(super) fn disk_states_main() -> String {
    fn tell(cx: &mut Cx<'_>, name: &str, payload: serde_json::Value) {
        let message = json!({ "wings:sdk": 1, "kind": "tell", "name": name, "payload": payload });
        cx.link().broadcast(message.to_string()).unwrap();
    }
    fake::script("main", || {
        let mut scans = json!([]);
        move |cx: &mut Cx<'_>, hear: Hear<'_>| {
            let Hear::Deliver("broadcast", _, text) = hear else { return };
            let message: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
            if message["wings:sdk"] != 1 || message["kind"] != "tell" {
                return;
            }
            let state = |scans: &serde_json::Value| json!({ "scans": scans, "collector": { "items": [], "total": 0 } });
            if message["name"] == "disk-state:want" {
                tell(cx, "disk-state", state(&scans));
            }
            // The SDK's `messages::ask`: `{ id, payload }` out, `{ id, result }` back on `<name>:answer`.
            let asked = &message["payload"]["payload"];
            if message["name"] != "disk-ask" || asked["op"] != "scan" {
                return;
            }
            let (id, root) = (&message["payload"]["id"], asked["root"].as_str().unwrap_or_default());
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
            scans = if root.ends_with("/slow") {
                json!([{ "root": root, "state": "scanning", "expected": 8_000_000_000u64, "startedAt": now - 83_000, "stamp": now,
                         "progress": { "entries": 48213, "bytes": 3_100_000_000u64, "path": format!("{root}/node_modules/react/cjs") } }])
            } else if root.ends_with("/broken") {
                json!([{ "root": root, "state": "error", "message": format!("There's no file or folder at {root}"), "stamp": now }])
            } else {
                return;
            };
            tell(cx, "disk-state", state(&scans));
            tell(cx, "disk-ask:answer", json!({ "id": id, "result": null }));
        }
    })
}

/// What to press to reach a view of Disk usage, with the text each press shows.
pub(super) type Steps<'a> = Vec<(&'a str, usize, &'a str)>;

/// Disk usage's plugin host over a storage folder of its own, its projects as given and its permissions its manifest's.
pub(super) fn disk_host(out: &std::path::Path, storage: &str, folder: &std::path::Path, projects: Vec<wings_native::page::plugin::Project>) -> PluginHost {
    let storage = out.join(storage);
    let _ = std::fs::remove_dir_all(&storage);
    let manifest: wings_plugins::manifest::Manifest = serde_json::from_str(include_str!("../../../../plugins/disk/wings-plugin.json")).unwrap();
    let host = PluginHost::with_access(
        PluginConfig { id: "disk".into(), root: storage, scope: "shot".into() },
        Access {
            plugin: wings_plugins::manifest::Plugin { manifest, dir: folder.to_path_buf(), icon_svg: None },
            builtin: true,
            env: std::sync::Arc::new(FixtureEnv(folder.to_path_buf())),
        },
    );
    host.publish_projects(projects);
    host
}

/// Renders each of `views` of Disk usage's page at 1000 and 640 points wide.
pub(super) fn shoot_disk(cx: &mut VisualTestAppContext, host: &PluginHost, source: &str, out: &std::path::Path, views: &[(&str, Steps, Steps)]) {
    for width in [1000.0, 640.0] {
        for (name, steps, undo) in views {
            let handle = cx
                .open_offscreen_window(size(px(width), px(900.0)), |_, cx| {
                    cx.new(|cx| PageView::in_plugin(host, source, cx))
                })
                .unwrap();
            let window = handle.into();
            handle
                .read_with(cx, |view, _| view.set_visible(true))
                .unwrap();
            let until = |cx: &mut VisualTestAppContext, text: &str| {
                let deadline = Instant::now() + Duration::from_secs(20);
                while !handle
                    .read_with(cx, |view, _| view.text().contains(text))
                    .unwrap()
                {
                    assert!(
                        Instant::now() < deadline,
                        "page did not show {text:?}: {}",
                        handle.read_with(cx, |view, _| view.text()).unwrap()
                    );
                    wait(cx, window, Duration::from_millis(20));
                }
            };
            until(cx, "Folders");
            // The disks come from the page's own calls.
            let deadline = Instant::now() + Duration::from_secs(20);
            while handle
                .read_with(cx, |view, _| view.text().contains("Looking for disks"))
                .unwrap()
            {
                assert!(Instant::now() < deadline, "the disks never came");
                wait(cx, window, Duration::from_millis(20));
            }
            let press = |cx: &mut VisualTestAppContext,
                         (label, nth, shows): &(&str, usize, &str)| {
                // A menu's button has no click of its own for the page: the app opens the menu.
                let pressed = handle
                    .read_with(cx, |view, _| view.press(label, *nth))
                    .unwrap()
                    || handle
                        .update(cx, |view, _, cx| view.open_menu(label, *nth, cx))
                        .unwrap();
                assert!(pressed, "no button {label} number {nth}");
                until(cx, shows);
            };
            for step in steps {
                press(cx, step);
            }
            wait(cx, window, Duration::from_millis(500));
            save(cx, window, out.join(format!("disk-{name}-{width}.png")));
            for step in undo {
                press(cx, step);
            }
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
    }
}
