//! What a frame costs the window while an agent works: one drawn after the visible terminal is told to redraw with nothing
//! new on it, one drawn for the working agent's spinner alone, 30 times a second, one after output that changes a
//! single row (Claude Code's spinner and timer line), one after output that scrolls the whole screen, and the one-row
//! frame again with the tab split in two, the other pane full of text too. Offscreen,
//! with 16 made-up projects in the sidebar, a made-up Claude Code (a renamed `sleep`) working in the first pane, and the
//! components page open as a plugin's sidebar on the right.
//!
//! cargo run --release --manifest-path apps/native/Cargo.toml --example bench_redraw --features test-support

use std::time::{Duration, Instant};

use gpui::{AnyWindowHandle, AppContext, VisualTestAppContext, px, size};
use wings_native::{
    actions,
    host::Config,
    icons::Assets,
    installed::fixture::{Data, FIXTURE_MAIN},
    terminal_view::TerminalView,
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
    // A screen like a working Claude Code's: coloured words, dim lines, and diff lines with a background across the
    // pane. `BENCH_SCREEN=<file>` prints that file instead, such as a recording of a real session.
    let screen = fake.join("screen.txt");
    match std::env::var_os("BENCH_SCREEN") {
        Some(file) => std::fs::copy(file, &screen).map(|_| ()).unwrap(),
        None => std::fs::write(&screen, claude_like_screen()).unwrap(),
    }
    cx.simulate_input(
        window,
        &format!(
            "cat '{}'; exec '{}' 3600",
            screen.display(),
            fake_claude.display()
        ),
    );
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

    // `-- <png>` saves the window as measured, to check what the frames draw, and `<png>` with `.split.png` for its ending the split tab.
    let png = std::env::args().nth(1);
    if let Some(path) = &png {
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
    report(
        "frame after a terminal update with nothing new on it",
        frames,
    );
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
    // Output the model has already taken in, drawn as the view draws output that came. Only the draw is timed.
    let output_frame = |cx: &mut VisualTestAppContext, bytes: String| {
        cx.read(|cx| TerminalView::print_and_wait(bytes.into_bytes(), &terminal, cx));
        let start = Instant::now();
        cx.update(|cx| terminal.update(cx, TerminalView::take_output));
        cx.run_until_parked();
        draw(cx);
        start.elapsed()
    };
    // Claude Code's spinner line, three rows from the bottom, with a new spinner frame, time and token count each time.
    let spinner_line = |i: usize| {
        let spinner = ['·', '✢', '✳', '✶', '✻', '✽'];
        format!(
            "\x1b7\x1b[999;1H\x1b[3A\x1b[38;5;174m{} Refactoring the API client…\x1b[0m \x1b[2m({}s · ↓ {:.1}k tokens · esc to interrupt)\x1b[0m\x1b[K\x1b8",
            spinner[i % spinner.len()],
            i / 30,
            i as f32 / 10.0,
        )
    };
    let mut frames = Vec::new();
    for i in 0..300 {
        frames.push(output_frame(&mut cx, spinner_line(i)));
    }
    report("frame after output that changes one row", frames);
    // A screenful of new lines, so every row shows something else.
    let lines: Vec<String> = claude_like_screen().lines().map(str::to_owned).collect();
    let mut frames = Vec::new();
    for i in 0..300 {
        let mut screen = String::new();
        for line in lines
            .iter()
            .cycle()
            .skip(i % lines.len() + 1)
            .take(lines.len())
        {
            screen.push_str(line);
            screen.push_str("\r\n");
        }
        frames.push(output_frame(&mut cx, screen));
    }
    report("frame after output that scrolls the whole screen", frames);
    // Split right; the new pane shows the same screen, and the first one goes on working.
    cx.simulate_keystrokes(window, "cmd-d");
    settle(&mut cx, Duration::from_secs(2));
    cx.simulate_input(window, &format!("cat '{}'", screen.display()));
    cx.simulate_keystrokes(window, "enter");
    settle(&mut cx, Duration::from_secs(2));
    if let Some(path) = &png {
        let split = std::path::Path::new(path).with_extension("split.png");
        cx.capture_screenshot(window).unwrap().save(split).unwrap();
    }
    let mut frames = Vec::new();
    for i in 0..300 {
        frames.push(output_frame(&mut cx, spinner_line(i)));
    }
    report(
        "frame after output that changes one row, in one of two panes",
        frames,
    );
    // `<png>` with `.quick.png` for its ending: a floating quick terminal over the split tab.
    if let Some(path) = &png {
        cx.simulate_keystrokes(window, "ctrl-`");
        settle(&mut cx, Duration::from_secs(2));
        let quick = std::path::Path::new(path).with_extension("quick.png");
        cx.capture_screenshot(window).unwrap().save(quick).unwrap();
    }
    let _ = std::process::Command::new("pkill")
        .args(["-f", &fake_claude.display().to_string()])
        .status();
}

fn claude_like_screen() -> String {
    let words = [
        "Read",
        "src/workspace/mod.rs",
        "Edit",
        "the",
        "terminal",
        "frame",
        "paints",
        "every",
        "cell",
        "again",
    ];
    let mut out = String::new();
    for line in 0..60 {
        match line % 6 {
            // A diff line: one background across the whole width.
            0 => out.push_str(&format!(
                "\x1b[48;5;22m\x1b[38;5;114m+ {:<200}\x1b[0m\n",
                words.join(" ")
            )),
            1 => out.push_str(&format!(
                "\x1b[48;5;52m\x1b[38;5;210m- {:<200}\x1b[0m\n",
                words[..6].join(" ")
            )),
            2 => out.push_str(&format!("\x1b[2m  {}\x1b[0m\n", words.join(" ").repeat(2))),
            _ => {
                for (i, word) in words.iter().cycle().take(18).enumerate() {
                    out.push_str(&format!("\x1b[38;5;{}m{word} ", 30 + (line * 7 + i) % 200));
                }
                out.push_str("\x1b[0m\n");
            }
        }
    }
    out
}
