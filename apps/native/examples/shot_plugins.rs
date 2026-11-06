//! Renders what plugins draw in the window offscreen at 1000 and 640 points wide: a badge in a pane header, its card, the
//! status strip under the terminal, and a sidebar page in the panel on the right. The plugin is made up, in a made-up
//! desktop data folder, never the user's.
//! Nothing shows on screen and focus doesn't move. macOS only (real Metal rendering).
//!
//! cargo run --manifest-path apps/native/Cargo.toml --example shot_plugins --features test-support -- <out-dir>

use std::{
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use gpui::{
    AnyWindowHandle, AppContext, Modifiers, MouseButton, MouseDownEvent, VisualTestAppContext,
    point, px, size,
};
use serde_json::json;
use wings_native::{
    actions, host::Config, icons::Assets, installed::fixture::Data, workspace::Workspace,
};

const MAIN: &str = r#"
wings.onPanes(async (panes) => {
  for (const p of panes) {
    await wings.setBadge(p.paneId, {
      label: 'localhost:5173 +1', tone: 'success', title: 'Listening on 2 ports',
      rows: [{ label: 'node', value: ':5173 · up 3 min' }, { label: 'Python', value: ':8765 · up 1 h 5 min', tone: 'info' }],
      actions: [{ id: 'a', label: 'Open in browser', primary: true }, { id: 'b', label: 'Refresh' }],
    });
    await wings.setPaneStatus(p.paneId, {
      items: [
        { label: '5h', value: '23% · 3h10m', percent: 23, tone: 'success' },
        { label: 'wk', value: '81%', percent: 81, tone: 'warning' },
        { label: 'ctx', value: '97%', percent: 97, tone: 'danger' },
        { label: 'MCP', value: '1 failed', tone: 'danger' },
      ],
      rows: [{ label: 'Context', value: '970k of 1M tokens (97%)', tone: 'danger' }],
      open: 'side',
    });
  }
});
wings.onAction(() => new Promise((resolve) => setTimeout(resolve, 600)));
"#;

const PAGE: &str = r#"
wings.render(JSON.stringify({ id: 0, type: '#root', props: {}, children: [{ id: 1, type: '#text', text: 'Side page' }] }));
"#;

fn wait(cx: &mut VisualTestAppContext, window: AnyWindowHandle, duration: Duration) {
    let until = Instant::now() + duration;
    while Instant::now() < until {
        cx.run_until_parked();
        cx.advance_clock(Duration::from_millis(20));
        let _ = cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx));
        thread::sleep(Duration::from_millis(20));
    }
}

fn click(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    position: gpui::Point<gpui::Pixels>,
) {
    cx.simulate_event(
        window,
        MouseDownEvent {
            position,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        },
    );
    cx.simulate_event(
        window,
        gpui::MouseUpEvent {
            position,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: 1,
        },
    );
}

fn save(cx: &mut VisualTestAppContext, window: AnyWindowHandle, path: PathBuf) {
    let _ = cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx));
    cx.capture_screenshot(window)
        .expect("render the window")
        .save(&path)
        .expect("save the PNG");
    println!("{}", path.display());
}

fn main() {
    let out = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "shots".into()));
    std::fs::create_dir_all(&out).expect("create the output folder");
    let data = Data::temp();
    data.install_script(
        "demo",
        json!({
            "icon": "icon.svg",
            "contributes": { "ui": ["badges", "status"], "sidebars": [{ "id": "side", "title": "Side", "page": "dist/side.html" }] },
        }),
        MAIN,
        &[("side", PAGE)],
        "shot",
    );
    let project = std::env::temp_dir().join("wings-native-shot-plugins-project");
    std::fs::create_dir_all(&project).expect("make a project folder");
    let args = ["--project".into(), project.clone().into_os_string()];
    let mut config = Config::parse(args, &project)
        .expect("parse the options")
        .expect("options, not --help");
    config.state = None;
    config.projects = None;
    config.agents = None;
    config.plugins = Some(data.paths());
    let mut cx = VisualTestAppContext::with_asset_source(
        gpui_platform::current_platform(false),
        std::sync::Arc::new(Assets),
    );
    cx.update(actions::bind);
    for width in [1000.0, 640.0] {
        let config = config.clone();
        let handle = cx
            .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                cx.new(|cx| Workspace::new(config, window, cx))
            })
            .expect("open an offscreen window");
        let window: AnyWindowHandle = handle.into();
        wait(&mut cx, window, Duration::from_secs(3));
        save(
            &mut cx,
            window,
            out.join(format!("plugin-badge-{width}.png")),
        );
        // The pill, left of the pane's buttons; 102px of them, 8px of gap, the pane's 1px border and 8px of padding.
        let pill = point(px(width - 8.0 - 1.0 - 4.0 - 102.0 - 8.0 - 20.0), px(99.0));
        click(&mut cx, window, pill);
        wait(&mut cx, window, Duration::from_millis(300));
        save(
            &mut cx,
            window,
            out.join(format!("plugin-card-{width}.png")),
        );
        click(&mut cx, window, pill);
        // The strip: the card's last 22px, at its left.
        click(&mut cx, window, point(px(290.0), px(620.0)));
        wait(&mut cx, window, Duration::from_millis(500));
        save(
            &mut cx,
            window,
            out.join(format!("plugin-sidebar-{width}.png")),
        );
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
}
