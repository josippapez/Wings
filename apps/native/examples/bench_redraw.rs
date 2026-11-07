//! What a frame costs the window while an agent works: one drawn after the visible terminal changes, as it does up to 60
//! times a second, and one drawn for the working agent's spinner alone, 30 times a second. Offscreen, with 16 made-up
//! projects in the sidebar, a made-up Claude Code (a renamed `sleep`) working in the first pane, and the components page
//! open as a plugin's sidebar on the right.
//!
//! cargo run --release --manifest-path apps/native/Cargo.toml --example bench_redraw --features test-support

use std::time::{Duration, Instant};

use gpui::{AnyWindowHandle, AppContext, VisualTestAppContext, px, size};
use wings_native::{
    actions,
    host::Config,
    icons::Assets,
    installed::fixture::{Data, FIXTURE_MAIN},
    workspace::Workspace,
};

fn main() {
    let fake = std::env::temp_dir().join("wings-native-bench-redraw");
    let _ = std::fs::remove_dir_all(&fake);
    let mut spaces = Vec::new();
    for i in 0..16 {
        let folder = fake.join(format!("project-{i}"));
        std::fs::create_dir_all(folder.join(".git")).unwrap();
        std::fs::write(folder.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        spaces.push(serde_json::json!({ "id": format!("p{i}"), "name": format!("project-{i}"), "path": folder }));
    }
    std::fs::write(
        fake.join("spaces.json"),
        serde_json::to_string(&spaces).unwrap(),
    )
    .unwrap();
    let project = fake.join("project-0");
    let mut config = Config::parse(
        ["--project".into(), project.clone().into_os_string()],
        &project,
    )
    .unwrap()
    .unwrap();
    config.state = None;
    config.projects = Some(fake.join("spaces.json"));
    let claude = fake.join("claude");
    std::fs::create_dir_all(claude.join("sessions")).unwrap();
    std::fs::create_dir_all(fake.join("bin")).unwrap();
    let fake_claude = fake.join("bin/claude");
    std::os::unix::fs::symlink("/bin/sleep", &fake_claude).unwrap();
    config.agents = Some(claude.clone());
    let data = Data::temp();
    data.install_script(
        "side",
        serde_json::json!({ "contributes": { "sidebars": [{ "id": "side", "title": "Side", "page": "dist/side.html" }] } }),
        FIXTURE_MAIN,
        &[("side", include_str!("../pages/dist/components-page.js"))],
        "bench",
    );
    config.plugins = Some(data.paths());

    let mut cx = VisualTestAppContext::with_asset_source(
        gpui_platform::current_platform(false),
        std::sync::Arc::new(Assets),
    );
    cx.update(actions::bind);
    let handle = cx
        .open_offscreen_window(size(px(1400.0), px(900.0)), move |window, cx| {
            cx.new(|cx| Workspace::new(config, window, cx))
        })
        .unwrap();
    let window: AnyWindowHandle = handle.into();
    let draw = |cx: &mut VisualTestAppContext| {
        let _ = cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx));
    };
    let settle = |cx: &mut VisualTestAppContext, duration: Duration| {
        let until = Instant::now() + duration;
        while Instant::now() < until {
            cx.run_until_parked();
            cx.advance_clock(Duration::from_millis(20));
            let _ = cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx));
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    settle(&mut cx, Duration::from_secs(2));
    handle
        .update(&mut cx, |workspace, _, cx| {
            workspace.shot_sidebar("side", "side", cx)
        })
        .unwrap();
    settle(&mut cx, Duration::from_secs(2));
    cx.simulate_input(window, &format!("exec '{}' 3600", fake_claude.display()));
    cx.simulate_keystrokes(window, "enter");
    settle(&mut cx, Duration::from_secs(1));
    let pids = std::process::Command::new("pgrep")
        .args(["-f", &format!("{} 3600", fake_claude.display())])
        .output()
        .unwrap();
    for pid in String::from_utf8_lossy(&pids.stdout).split_whitespace() {
        std::fs::write(
            claude.join(format!("sessions/{pid}.json")),
            r#"{"status":"busy","name":"Refactor the API client"}"#,
        )
        .unwrap();
    }
    settle(&mut cx, Duration::from_secs(2));
    let terminal = handle
        .update(&mut cx, |workspace, _, _| {
            let (panes, _) = workspace.panes(0).expect("a tab");
            workspace.terminal(panes[0]).expect("a terminal")
        })
        .unwrap();

    // `-- <png>` saves the window as measured, to check what the frames draw.
    if let Some(path) = std::env::args().nth(1) {
        cx.capture_screenshot(window).unwrap().save(path).unwrap();
    }
    let report = |what: &str, mut frames: Vec<Duration>| {
        frames.sort();
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        let mean = frames.iter().sum::<Duration>() / frames.len() as u32;
        println!(
            "{what}: median {:.2} ms, mean {:.2} ms, p90 {:.2} ms ({} frames)",
            ms(frames[frames.len() / 2]),
            ms(mean),
            ms(frames[frames.len() * 9 / 10]),
            frames.len()
        );
    };
    let mut frames = Vec::new();
    for _ in 0..300 {
        let start = Instant::now();
        cx.update(|cx| terminal.update(cx, |_, cx| cx.notify()));
        cx.run_until_parked();
        draw(&mut cx);
        frames.push(start.elapsed());
    }
    report("frame after a terminal update", frames);
    // The spinner asks to be drawn again 1/30 s on: only the app's clock moves, so nothing else changes. The time
    // includes whatever runs and draws when the clock moves, not only the last draw.
    let mut frames = Vec::new();
    for _ in 0..300 {
        let start = Instant::now();
        cx.advance_clock(Duration::from_millis(34));
        cx.run_until_parked();
        draw(&mut cx);
        frames.push(start.elapsed());
    }
    report("frame for the spinner alone", frames);
    let _ = std::process::Command::new("pkill")
        .args(["-f", &fake_claude.display().to_string()])
        .status();
}
