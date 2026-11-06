//! Offscreen native diff states. Never opens a visible window or reads app data.
use gpui::{AnyWindowHandle, AppContext, VisualTestAppContext, px, size};
use std::{path::PathBuf, sync::Arc};
use wings_native::{
    diff::{self, View, viewer::Viewer},
    icons::Assets,
    page::plugin::{PluginConfig, PluginHost},
};

const PATCH: &str = "diff --git a/src/config.rs b/src/config.rs\n--- a/src/config.rs\n+++ b/src/config.rs\n@@ -1,4 +1,6 @@\n pub fn load() {\n-    let timeout = 10;\n+    // Thirty seconds gives a slow network room to answer before the client gives up and reports the server as unreachable.\n+    let timeout = 30;\n+    validate(timeout);\n     connect(timeout);\n }\ndiff --git a/old-name.txt b/new-name.txt\nsimilarity index 100%\nrename from old-name.txt\nrename to new-name.txt\n";

fn main() {
    let out = PathBuf::from(std::env::args().nth(1).expect("output directory"));
    std::fs::create_dir_all(&out).unwrap();
    let host = Arc::new(PluginHost::new(PluginConfig {
        id: "diff-shot".into(),
        root: out.clone(),
        scope: "shot".into(),
    }));
    let mut cx = VisualTestAppContext::with_asset_source(
        gpui_platform::current_platform(false),
        Arc::new(Assets),
    );
    for width in [1000.0, 640.0] {
        for state in [
            "normal",
            "loading",
            "error",
            "binary",
            "selection",
            "tree",
            "wrap",
        ] {
            let payload = match state {
                "loading" => serde_json::json!({}),
                "error" => {
                    serde_json::json!({"error":"This file has no changes any more. Refresh to load the working tree again."})
                }
                "binary" => {
                    serde_json::json!({"patch":"diff --git a/assets/logo.png b/assets/logo.png\nBinary files a/assets/logo.png and b/assets/logo.png differ\n"})
                }
                _ => serde_json::json!({"patch":PATCH}),
            };
            let view = View {
                id: "diff-shot:1".into(),
                plugin_id: "diff-shot".into(),
                title: "Uncommitted changes".into(),
                subtitle: "Wings · Working tree against HEAD".into(),
                cwd: String::new(),
                refresh: true,
                update: diff::clean_update(&payload).unwrap(),
            };
            let host = host.clone();
            let handle = cx
                .open_offscreen_window(size(px(width), px(640.0)), move |_, cx| {
                    cx.new(|cx| Viewer::new(view, host, cx))
                })
                .unwrap();
            // The folder tree; and one wrapped column, with a long line.
            if state == "tree" || state == "wrap" {
                handle
                    .update(&mut cx, |viewer, _, cx| {
                        viewer.set_layout(state == "wrap", state == "tree", cx);
                        viewer.set_split(state != "wrap", cx);
                    })
                    .unwrap();
            }
            if state == "selection" {
                handle
                    .update(&mut cx, |viewer, _, cx| viewer.select_file(1, cx))
                    .unwrap();
            }
            let window: AnyWindowHandle = handle.into();
            cx.run_until_parked();
            cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))
                .unwrap();
            let path = out.join(format!("diff-{state}-{width}.png"));
            cx.capture_screenshot(window).unwrap().save(&path).unwrap();
            println!("{}", path.display());
            cx.update_window(window, |_, window, _| window.remove_window())
                .unwrap();
        }
    }
}
