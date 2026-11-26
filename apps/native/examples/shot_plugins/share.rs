//! Share's modal, and when `opentunnel` isn't installed.
use super::*;

/// Share's modal (`plugins/share/rust`) over made-up links: a port Claude asks to share with two links under it, the two
/// links alone, and nothing shared. A fake `opentunnel` that lists no routes stands in for the real one, so the ask shows
/// without a tunnel being made.
pub(super) fn share(out: PathBuf) {
    let project = out.join("fixture-project");
    std::fs::create_dir_all(&project).unwrap();
    let bin = out.join("fixture-bin");
    std::fs::create_dir_all(&bin).unwrap();
    let fake = bin.join("opentunnel");
    std::fs::write(&fake, "#!/bin/sh\necho 'No routes configured.'\n").unwrap();
    std::process::Command::new("chmod").arg("+x").arg(&fake).status().unwrap();
    // Wings' own search path comes first, so a real `opentunnel` on this Mac would win: it has to not be there.
    unsafe { std::env::set_var("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default())) };
    let manifest: wings_plugins::manifest::Manifest = serde_json::from_str(include_str!("../../../../plugins/share/wings-plugin.json")).unwrap();
    let module = |frame: &str| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../plugins/share/dist/native/{frame}.wasm"));
        format!("{}{}", wings_native::page::runtime::wasm::MARKER, path.canonicalize().expect("build the plugin: cd plugins && sh build-rust.sh share").display())
    };
    let host_over = |folder: &str, shares: serde_json::Value| {
        let storage = out.join(folder);
        let _ = std::fs::remove_dir_all(&storage);
        let host = PluginHost::with_access(
            PluginConfig { id: "share".into(), root: storage.clone(), scope: "shot".into() },
            Access {
                plugin: wings_plugins::manifest::Plugin { manifest: manifest.clone(), dir: project.clone(), icon_svg: None },
                builtin: true,
                env: std::sync::Arc::new(FixtureEnv(project.clone())),
            },
        );
        wings_plugin_storage::Storage::new(storage).set("share", "shot", "shares", shares).unwrap();
        host.start_main(&module("main"), |event| {
            if let wings_native::page::runtime::Event::Error { message, .. } = event {
                panic!("{message}");
            }
        })
        .unwrap();
        host
    };
    let links = json!([
        { "name": "21992cc9713e5fc5", "port": 3000, "url": "https://21992cc9713e5fc5.k7q2m9xv4p.opentunnel.xyz", "seen": true },
        { "name": "storybook", "port": 6006, "url": "https://storybook.k7q2m9xv4p.opentunnel.xyz", "seen": true },
    ]);
    let ask_host = host_over("fixture-storage-share-ask", links.clone());
    let list_host = host_over("fixture-storage-share-list", links);
    let empty_host = host_over("fixture-storage-share-empty", json!([]));
    // Claude asks to share port 5173, which waits for the user in the modal.
    let (reply, _answer) = wings_native::page::runtime::Reply::channel();
    ask_host.run_tool("share_port", json!({ "port": 5173 }), None, reply);
    let mut cx = app();
    let source = module("share");
    let views: [(&str, &PluginHost, &str); 3] = [
        ("ask", &ask_host, "Share localhost:5173 publicly?"),
        ("list", &list_host, "Shared now"),
        ("empty", &empty_host, "Nothing is shared"),
    ];
    for width in [1000.0, 640.0] {
        for (name, host, expected) in views {
            let handle = cx.open_offscreen_window(size(px(width), px(520.0)), |_, cx| cx.new(|cx| PageView::in_plugin(host, &source, cx))).unwrap();
            let window = handle.into();
            handle.read_with(&cx, |view, _| view.set_visible(true)).unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            while !handle.read_with(&cx, |view, _| view.text().contains(expected)).unwrap() {
                assert!(Instant::now() < deadline, "page did not show {expected:?}: {}", handle.read_with(&cx, |view, _| view.text()).unwrap());
                wait(&mut cx, window, Duration::from_millis(20));
            }
            // Allow waits a second after the ask shows.
            wait(&mut cx, window, Duration::from_millis(if name == "ask" { 1300 } else { 300 }));
            save(&mut cx, window, out.join(format!("share-{name}-{width}.png")));
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
    }
}

/// Share's modal when Claude asked to share and `opentunnel` isn't installed: it offers Install. Its own run (`--share-missing`),
/// as Wings would find a real `opentunnel` through the login shell's PATH: no shell, and a PATH with nothing on it.
pub(super) fn share_missing(out: PathBuf) {
    let project = out.join("fixture-project");
    std::fs::create_dir_all(&project).unwrap();
    let nothing = out.join("fixture-bin-empty");
    std::fs::create_dir_all(&nothing).unwrap();
    unsafe {
        std::env::set_var("SHELL", "/nonexistent-shell");
        std::env::set_var("PATH", &nothing);
    }
    let manifest: wings_plugins::manifest::Manifest = serde_json::from_str(include_str!("../../../../plugins/share/wings-plugin.json")).unwrap();
    let module = |frame: &str| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../plugins/share/dist/native/{frame}.wasm"));
        format!("{}{}", wings_native::page::runtime::wasm::MARKER, path.canonicalize().expect("build the plugin: cd plugins && sh build-rust.sh share").display())
    };
    let storage = out.join("fixture-storage-share-missing");
    let _ = std::fs::remove_dir_all(&storage);
    let host = PluginHost::with_access(
        PluginConfig { id: "share".into(), root: storage, scope: "shot".into() },
        Access {
            plugin: wings_plugins::manifest::Plugin { manifest, dir: project.clone(), icon_svg: None },
            builtin: true,
            env: std::sync::Arc::new(FixtureEnv(project)),
        },
    );
    host.start_main(&module("main"), |event| {
        if let wings_native::page::runtime::Event::Error { message, .. } = event {
            panic!("{message}");
        }
    })
    .unwrap();
    let (reply, _answer) = wings_native::page::runtime::Reply::channel();
    host.run_tool("share_port", json!({ "port": 5173 }), None, reply);
    let mut cx = app();
    let source = module("share");
    for width in [1000.0, 640.0] {
        let handle = cx.open_offscreen_window(size(px(width), px(520.0)), |_, cx| cx.new(|cx| PageView::in_plugin(&host, &source, cx))).unwrap();
        let window = handle.into();
        handle.read_with(&cx, |view, _| view.set_visible(true)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !handle.read_with(&cx, |view, _| view.text().contains("OpenTunnel isn't installed")).unwrap() {
            assert!(Instant::now() < deadline, "page did not offer Install: {}", handle.read_with(&cx, |view, _| view.text()).unwrap());
            wait(&mut cx, window, Duration::from_millis(20));
        }
        wait(&mut cx, window, Duration::from_millis(300));
        save(&mut cx, window, out.join(format!("share-missing-{width}.png")));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
}
