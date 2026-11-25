use super::*;
use crate::{diff, page::plugin::PluginConfig};
use gpui::{Bounds, MouseButton, TestApp, WindowBounds, WindowOptions, point, size};

#[test]
fn large_patches_render_only_viewport_rows_and_file_selection_works_by_keyboard() {
    let patch = format!(
        "diff --git a/large b/large\nnew file mode 100644\n--- /dev/null\n+++ b/large\n@@ -0,0 +1,10000 @@\n{}diff --git a/empty b/empty\nnew file mode 100644\n",
        "+line\n".repeat(10_000)
    );
    let view = View {
        id: "test:1".into(),
        plugin_id: "test".into(),
        sidebar: None,
        title: "Large diff".into(),
        subtitle: String::new(),
        cwd: String::new(),
        refresh: false,
        update: diff::clean_update(&serde_json::json!({ "patch":patch })).unwrap(),
    };
    let host = Arc::new(PluginHost::new(PluginConfig {
        id: "test".into(),
        root: std::env::temp_dir(),
        scope: "test".into(),
    }));
    let mut app = TestApp::new();
    app.background_executor().allow_parking();
    let mut window = app.open_window_with_options(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                point(px(0.0), px(0.0)),
                size(px(1000.0), px(640.0)),
            ))),
            ..Default::default()
        },
        move |window, cx| {
            let viewer = Viewer::new(view, host, cx);
            viewer.focus_handle(cx).focus(window, cx);
            viewer
        },
    );
    window.draw();
    window.read(|viewer, _| {
        assert_eq!(viewer.rows.len(), 10_001);
        assert!(
            (1..100).contains(&viewer.rendered_code_rows()),
            "eagerly rendered {} rows",
            viewer.rendered_code_rows()
        );
    });
    window.simulate_click(point(px(80.0), px(130.0)), MouseButton::Left);
    window.simulate_keystroke("down");
    assert_eq!(window.read(|viewer, _| viewer.selected_file()), 1);
    window.simulate_keystroke("up");
    assert_eq!(window.read(|viewer, _| viewer.selected_file()), 0);
    window.simulate_keystroke("tab");
    window.simulate_keystroke("end");
    window.draw();
    assert!(
        window.read(|viewer, _| viewer.scroll.0.borrow().base_handle.offset().y)
            < px(-100_000.0)
    );
}
