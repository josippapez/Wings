//! What a key typed in a long chat's message box costs, over a real session's transcript: `CHAT_TYPING=<session.jsonl>`
//! with `--claude-chat` resumes that session in a tab drawn as the workspace draws one (the page kept as last drawn in the
//! main column's piece, itself kept as last drawn under the window's root), types keys into the message box and prints
//! the median ms of a key's frame. Then it prints the page's outline, and the same measure with each big part of the page
//! left out, which is what that part costs a key.
//!
//! `CHAT_TYPING_SIZE=<width>,<height>` sizes the window (1600,1000 by default). `CHAT_TYPING_KEYS=<n>` keys are timed (60).
//! `CHAT_TYPING_PARTS=<path>[,<path>...]` measures again without each node under each path of child indexes (`0.1` is
//! the second child of the first node), one at a time.

use std::{
    collections::HashSet,
    path::Path,
    time::{Duration, Instant},
};

use gpui::{AnyWindowHandle, AppContext, Entity, VisualTestAppContext, px, size};
use wings_native::page::{PageView, plugin::PluginHost};

use super::wait;

/// The window's root, as the workspace: it draws the main column's piece, kept as last drawn.
struct Root {
    main: Entity<Main>,
}

impl gpui::Render for Root {
    fn render(&mut self, window: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl gpui::IntoElement {
        use gpui::prelude::*;
        wings_native::ui::readout::render("workspace");
        gpui::div()
            .relative()
            .size_full()
            .bg(gpui::rgb(wings_native::theme::BACKGROUND))
            // It starts each draw of the slots, as the workspace's does.
            .child(wings_native::ui::spinner_layer(window))
            .child(self.main.clone().cached(gpui::StyleRefinement::default().size_full()))
    }
}

/// The main column's piece (`workspace::parts::Part`): the tab's card, holding the page kept as last drawn.
struct Main {
    page: Entity<PageView>,
}

impl gpui::Render for Main {
    fn render(&mut self, window: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl gpui::IntoElement {
        use gpui::prelude::*;
        wings_native::ui::readout::render("main");
        let id = window.window_handle().window_id();
        wings_native::ui::drawing_piece("main", id, || {
            wings_native::ui::place_view(self.page.entity_id(), id);
            gpui::div().size_full().p(px(8.0)).pt_0().child(
                gpui::div()
                    .size_full()
                    .overflow_hidden()
                    .rounded(px(14.0))
                    .border_1()
                    .border_color(gpui::rgba(wings_native::ui::HAIRLINE))
                    .bg(gpui::rgb(wings_native::theme::SURFACE))
                    .child(self.page.clone().cached(gpui::StyleRefinement::default().size_full())),
            )
        })
    }
}

/// The median and p90 ms of a key's frame: the key, and the window drawn. The page's own answer to the key (its tree,
/// sent from its thread) is let in between keys and drawn, untimed, and timed apart as the echo.
fn keys(cx: &mut VisualTestAppContext, window: AnyWindowHandle, page: &Entity<PageView>, n: usize) -> (f64, f64, f64, usize) {
    let mut frames = Vec::new();
    let mut echoes = Vec::new();
    let renders = page.read_with(cx, |view, _| view.renders());
    // `CHAT_TYPING_GAP_MS`: how long to wait between keys, as a person types: the processor slows down between them.
    let gap = Duration::from_millis(std::env::var("CHAT_TYPING_GAP_MS").ok().and_then(|ms| ms.parse().ok()).unwrap_or(0));
    for at in 0..n {
        std::thread::sleep(gap);
        let sent = page.read_with(cx, |view, _| view.trees_sent());
        let key = if at % 7 == 6 { " " } else { "x" };
        let start = Instant::now();
        let _ = cx.update_window(window, |_, window, cx| {
            window.dispatch_keystroke(gpui::Keystroke { modifiers: Default::default(), key: key.into(), key_char: Some(key.into()) }, cx);
            window.draw(cx).clear(cx)
        });
        frames.push(start.elapsed().as_secs_f64() * 1000.0);
        // The page hears the key on its own thread and sends a tree back.
        let deadline = Instant::now() + Duration::from_millis(500);
        while page.read_with(cx, |view, _| view.trees_sent()) == sent && Instant::now() < deadline {
            cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(1));
        }
        let start = Instant::now();
        let _ = cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx));
        echoes.push(start.elapsed().as_secs_f64() * 1000.0);
        cx.run_until_parked();
    }
    let renders = page.read_with(cx, |view, _| view.renders()) - renders;
    let median = |times: &mut Vec<f64>, at: f64| {
        times.sort_by(f64::total_cmp);
        times[((times.len() - 1) as f64 * at) as usize]
    };
    (median(&mut frames, 0.5), median(&mut frames, 0.9), median(&mut echoes, 0.5), renders)
}

/// Gives the message box the keyboard.
fn focus(cx: &mut VisualTestAppContext, window: AnyWindowHandle, page: &Entity<PageView>) {
    let _ = cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear(cx)
    });
    let _ = cx.update_window(window, |_, window, cx| {
        let focus = page.read(cx).input_focus("Message", cx).unwrap();
        focus.focus(window, cx);
    });
    wait(cx, window, Duration::from_millis(300));
}

/// Empties the message box, so each measure starts from the same draft.
fn clear(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.simulate_keystrokes(window, "cmd-a backspace");
    wait(cx, window, Duration::from_millis(200));
}

pub(super) fn typing(cx: &mut VisualTestAppContext, host: &PluginHost, source: &str, root: &Path, transcript: &Path, lines: impl Fn(&str) -> Vec<serde_json::Value>) {
    let id = transcript.file_stem().unwrap().to_string_lossy().into_owned();
    let folder = root.join("projects/-bench");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::copy(transcript, folder.join(format!("{id}.jsonl"))).expect("copy the transcript");
    // Its subagents' transcripts, beside it as Claude Code keeps them.
    let beside = transcript.with_extension("");
    if beside.is_dir() {
        let _ = std::os::unix::fs::symlink(&beside, folder.join(&id));
    }
    // `CHAT_TYPING_LIVE=1`: the session's messages come as Claude Code prints them while it runs, in a new chat, rather
    // than read back from its transcript.
    let live = std::env::var_os("CHAT_TYPING_LIVE").is_some();
    let mut printed: String = lines(&id).iter().map(|line| format!("{line}\n")).collect();
    if live {
        for line in std::fs::read_to_string(transcript).unwrap().lines() {
            let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else { continue };
            if matches!(entry["type"].as_str(), Some("user" | "assistant")) {
                printed.push_str(line);
                printed.push('\n');
            }
        }
    }
    let fixture = root.join("typing.jsonl");
    std::fs::write(&fixture, printed).unwrap();
    let answers = root.join("answers-typing");
    let _ = std::fs::remove_dir_all(&answers);
    std::fs::create_dir_all(&answers).unwrap();
    // SAFETY: no page is open yet, so nothing reads these while they change.
    unsafe {
        for empty in ["CHAT_DRIP", "CHAT_ISSUES", "CHAT_PR", "CHAT_BRANCHES", "CHAT_EXIT", "CHAT_STDERR"] {
            std::env::set_var(empty, "");
        }
        std::env::set_var("CHAT_FIXTURE", &fixture);
        std::env::set_var("CHAT_ANSWERS", &answers);
    }
    let (width, height) = std::env::var("CHAT_TYPING_SIZE")
        .ok()
        .and_then(|said| said.split_once(',').map(|(w, h)| (w.parse().unwrap(), h.parse().unwrap())))
        .unwrap_or((1600.0, 1000.0));
    let n: usize = std::env::var("CHAT_TYPING_KEYS").ok().and_then(|n| n.parse().ok()).unwrap_or(60);
    let args = match live {
        true => serde_json::json!({ "cwd": root.canonicalize().unwrap() }),
        false => serde_json::json!({ "resume": id, "cwd": root.canonicalize().unwrap() }),
    };
    let handle = cx
        .open_offscreen_window(size(px(width), px(height)), |_, cx| {
            let page = cx.new(|cx| PageView::in_plugin_with_args(host, source, Some(&args), cx).in_card());
            let main = cx.new(|_| Main { page });
            cx.new(|_| Root { main })
        })
        .unwrap();
    let window: AnyWindowHandle = handle.into();
    let page = handle.read_with(cx, |root, cx| root.main.read(cx).page.clone()).unwrap();
    page.read_with(cx, |view, _| view.set_visible(true));
    // Loaded once the message box is there and the page has stopped sending trees for a second.
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut quiet_since = Instant::now();
    let mut sent = 0;
    loop {
        wait(cx, window, Duration::from_millis(100));
        let now = page.read_with(cx, |view, _| view.trees_sent());
        if now != sent {
            sent = now;
            quiet_since = Instant::now();
        }
        let has_box = page.read_with(cx, |view, cx| view.input_focus("Message", cx).is_some());
        if has_box && quiet_since.elapsed() > Duration::from_secs(1) {
            break;
        }
        assert!(Instant::now() < deadline, "the chat never settled: {}", page.read_with(cx, |view, _| view.text()));
    }
    focus(cx, window, &page);
    if let Ok(out) = std::env::var("CHAT_TYPING_SHOT") {
        super::save(cx, window, out.into());
    }
    println!("[outline]\n{}", page.read_with(cx, |view, _| view.outline(7, 40)));
    // Warm the caches (fonts, shaped lines), then measure.
    keys(cx, window, &page, 10);
    clear(cx, window);
    let (median, p90, echo, renders) = keys(cx, window, &page, n);
    println!("[measure] typing {n} keys in {id} at {width}x{height}: key frame median {median:.2} ms, p90 {p90:.2} ms; echo frame median {echo:.2} ms; page rendered {renders} times");
    for (scroll, drawn, of) in page.read_with(cx, |view, _| view.windows_drawn()) {
        println!("[measure] the column of Scroll #{scroll} drew children {drawn:?} of {of}");
    }
    // What each part costs: the same keys with it left out, one at a time.
    if std::env::var("CHAT_TYPING_PARTS").is_ok_and(|said| !said.is_empty()) {
        for path in std::env::var("CHAT_TYPING_PARTS").unwrap().split(',') {
            let path: Vec<usize> = path.split('.').filter(|at| !at.is_empty()).map(|at| at.parse().unwrap()).collect();
            let parts = page.read_with(cx, |view, _| view.ids_at(&path));
            for (id, kind) in parts {
                page.read_with(cx, |view, _| view.hide_for_bench(HashSet::new()));
                focus(cx, window, &page);
                page.read_with(cx, |view, _| view.hide_for_bench(HashSet::from([id])));
                clear(cx, window);
                // Left out with the message box in it, a key reaches nothing: there is nothing to measure.
                if keys(cx, window, &page, 1).3 == 0 {
                    println!("[measure] without {kind} #{id} (under {path:?}): holds the message box, not measured");
                    continue;
                }
                let (median, p90, echo, _) = keys(cx, window, &page, n / 2);
                println!("[measure] without {kind} #{id} (under {path:?}): key frame median {median:.2} ms, p90 {p90:.2} ms; echo {echo:.2} ms");
            }
        }
        page.read_with(cx, |view, _| view.hide_for_bench(HashSet::new()));
    }
    let _ = cx.update_window(window, |_, window, _| window.remove_window());
}
