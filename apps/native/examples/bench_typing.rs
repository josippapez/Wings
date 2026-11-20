//! What a key typed in a text field costs the window, wherever the field sits: a long chat's message box and a one-line
//! filter in a plugin tab's page (inside the main column's piece), the same page's message box in the right panel, and
//! the command palette's field (over the window). For each it prints the median and p90 ms of a key's frame (the key and
//! the window drawn), of the frame for the page's answer to the key (its tree, with the typed text), and how many times
//! each view rendered per key. Offscreen, with 16 made-up projects in the sidebar, as `bench_redraw`.
//!
//! cargo run --release --manifest-path apps/native/Cargo.toml --example bench_typing --features test-support
//!
//! `BENCH_TYPING_KEYS=<n>` keys are timed for each field (120). `BENCH_TYPING_GAP_MS=<ms>` waits between keys as a person
//! types (0): the processor slows down between them, so a key's frame takes longer than back to back.

use std::time::{Duration, Instant};

use gpui::{AnyWindowHandle, AppContext, Entity, Keystroke, VisualTestAppContext, px, size};
use serde_json::{Value, json};
use wings_native::{
    actions,
    host::Config,
    icons::Assets,
    installed::fixture::{Data, fixture_main},
    page::{PageView, runtime::fake},
    text_field::FieldLayer,
    ui::readout::Readout,
    workspace::Workspace,
};

fn main() {
    let fake = std::env::temp_dir().join("wings-native-bench-typing");
    let _ = std::fs::remove_dir_all(&fake);
    let mut spaces = Vec::new();
    for i in 0..16 {
        let folder = fake.join(format!("project-{i}"));
        std::fs::create_dir_all(folder.join(".git")).unwrap();
        std::fs::write(folder.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        spaces.push(json!({ "id": format!("p{i}"), "name": format!("project-{i}"), "path": folder }));
    }
    std::fs::write(fake.join("spaces.json"), serde_json::to_string(&spaces).unwrap()).unwrap();
    let project = fake.join("project-0");
    let mut config = Config::parse(["--project".into(), project.clone().into_os_string()], &project)
        .unwrap()
        .unwrap();
    config.state = None;
    config.ai = None;
    config.projects = Some(fake.join("spaces.json"));
    std::fs::create_dir_all(fake.join("claude/sessions")).unwrap();
    config.agents = Some(fake.join("claude"));
    let data = Data::temp();
    let chat = long_chat_with_box();
    data.install_script(
        "typing",
        json!({ "contributes": {
            "views": [{ "id": "chat", "title": "Chat", "page": "dist/chat.html", "opens": ["tab"], "newTab": true, "button": false }],
            "sidebars": [{ "id": "side", "title": "Side", "page": "dist/side.html" }]
        } }),
        &fixture_main(),
        &[("chat", &chat), ("side", &chat)],
        "bench-typing",
    );
    config.plugins = Some(data.paths());

    let mut cx = VisualTestAppContext::with_asset_source(gpui_platform::current_platform(false), std::sync::Arc::new(Assets));
    cx.executor().allow_parking();
    cx.update(actions::bind);
    let handle = cx
        .open_offscreen_window(size(px(1400.0), px(900.0)), move |window, cx| cx.new(|cx| Workspace::new(config, window, cx)))
        .unwrap();
    let window: AnyWindowHandle = handle.into();
    settle(&mut cx, window, Duration::from_secs(2));
    handle
        .update(&mut cx, |workspace, window, cx| {
            assert!(workspace.open_plugin_view("typing", "chat", None, window, cx), "the chat opens");
            window.activate_window();
        })
        .unwrap();
    settle(&mut cx, window, Duration::from_secs(2));
    handle.update(&mut cx, |workspace, _, cx| workspace.shot_sidebar("typing", "side", cx)).unwrap();
    settle(&mut cx, window, Duration::from_secs(2));
    let tab = handle.read_with(&cx, |workspace, _| workspace.tab_page()).unwrap().expect("the chat's tab");
    let side = handle.read_with(&cx, |workspace, _| workspace.sidebar_page("typing", "side")).unwrap().expect("the side page");
    if let Some(path) = std::env::args().nth(1) {
        cx.capture_screenshot(window).unwrap().save(&path).unwrap();
    }
    let n: usize = std::env::var("BENCH_TYPING_KEYS").ok().and_then(|n| n.parse().ok()).unwrap_or(120);
    for (what, page, label) in [
        ("a message box in a tab's page", &tab, "Message"),
        ("a one-line filter in a tab's page", &tab, "Filter"),
        ("a message box in the right panel's page", &side, "Message"),
    ] {
        focus(&mut cx, window, page, label);
        keys(&mut cx, window, Some(page), 10);
        clear(&mut cx, window);
        let measured = keys(&mut cx, window, Some(page), n);
        report(what, n, measured);
    }
    // The palette, over the window: it takes the keyboard as it opens.
    cx.simulate_keystrokes(window, "cmd-k");
    settle(&mut cx, window, Duration::from_millis(500));
    keys(&mut cx, window, None, 10);
    clear(&mut cx, window);
    let measured = keys(&mut cx, window, None, n);
    report("the command palette's field", n, measured);
    look(&mut cx, &chat);
}

/// The page in a piece kept as last drawn, as the main column holds a tab's page, and with or without the window's field
/// layer over it.
struct Look {
    piece: Entity<LookPiece>,
    fields: Option<Entity<FieldLayer>>,
}

struct LookPiece {
    page: Entity<PageView>,
}

impl gpui::Render for LookPiece {
    fn render(&mut self, window: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl gpui::IntoElement {
        use gpui::prelude::*;
        let id = window.window_handle().window_id();
        let card = wings_native::ui::drawing_piece("main", id, || {
            wings_native::ui::place_view(self.page.entity_id(), id);
            gpui::div().size_full().child(self.page.clone().cached(gpui::StyleRefinement::default().size_full()))
        });
        wings_native::ui::in_piece(Some("main".into()), false, card)
    }
}

impl gpui::Render for Look {
    fn render(&mut self, window: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl gpui::IntoElement {
        use gpui::prelude::*;
        let spinners = wings_native::ui::spinner_layer(window);
        gpui::div()
            .size_full()
            .bg(gpui::rgb(wings_native::theme::SURFACE))
            .child(self.piece.clone().cached(gpui::StyleRefinement::default().size_full()))
            .children(self.fields.clone())
            .child(spinners)
    }
}

/// The same keys typed in the same page in two windows, one whose field layer draws the field's text and one where the
/// field draws it in place, as before the layer: the two must look the same to the pixel. `-- <png> <folder>` saves
/// them in `<folder>`.
fn look(cx: &mut VisualTestAppContext, source: &str) {
    let root = std::env::temp_dir().join("wings-native-bench-typing-look");
    let _ = std::fs::remove_dir_all(&root);
    let host = wings_native::page::plugin::PluginHost::new(wings_native::page::plugin::PluginConfig {
        id: "look".into(),
        root,
        scope: "shot".into(),
    });
    let open = |cx: &mut VisualTestAppContext, layer: bool| -> (AnyWindowHandle, Entity<PageView>) {
        let handle = cx
            .open_offscreen_window(size(px(900.0), px(700.0)), |_, cx| {
                let page = cx.new(|cx| PageView::in_plugin(&host, source, cx).in_card());
                let piece = cx.new(|_| LookPiece { page });
                let fields = layer.then(|| cx.new(|_| FieldLayer));
                cx.new(|_| Look { piece, fields })
            })
            .unwrap();
        let page = handle.read_with(cx, |look, cx| look.piece.read(cx).page.clone()).unwrap();
        page.read_with(cx, |view, _| view.set_visible(true));
        (handle.into(), page)
    };
    let windows = [open(cx, true), open(cx, false)];
    for (window, page) in &windows {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !page.read_with(cx, |view, _| view.text().contains("Turn 119")) {
            assert!(Instant::now() < deadline, "the page never drew its tree");
            settle(cx, *window, Duration::from_millis(50));
        }
    }
    let folder = std::env::args().nth(2).map(std::path::PathBuf::from);
    let steps: [(&str, &str, &[&str]); 4] = [
        ("message", "Message", &["The quick brown fox jumps over the lazy dog, then naps by the fire, and wakes up when the kettle sings; ", "the dog never wakes, and the fox runs off into the woods before the farmer comes home. ", "The end."]),
        ("message-selected", "Message", &["shift-left shift-left shift-left shift-left"]),
        ("filter", "Filter", &["a long query that runs past the end of the field, so the field scrolls to keep the caret in view as it moves on to the end, ", "and on past it, well past where the field ends"]),
        ("filter-selected", "Filter", &["shift-left shift-left shift-left alt-shift-left"]),
    ];
    let mut focused = "";
    for (name, label, typed) in steps {
        let renders: Vec<usize> = windows.iter().map(|(_, page)| page.read_with(cx, |view, _| view.renders())).collect();
        for (window, page) in &windows {
            if focused != label {
                focus(cx, *window, page, label);
            }
            for keys in typed {
                if keys.starts_with("shift-") || keys.starts_with("alt-") {
                    cx.simulate_keystrokes(*window, keys);
                } else {
                    cx.simulate_input(*window, keys);
                }
                settle(cx, *window, Duration::from_millis(100));
            }
            if label == "Message" && name == "message" {
                // A line of its own, so the box grows.
                cx.simulate_keystrokes(*window, "shift-enter");
                cx.simulate_input(*window, "A second line.");
                settle(cx, *window, Duration::from_millis(300));
            }
        }
        focused = label;
        let shots: Vec<_> = windows.iter().map(|(window, _)| cx.capture_screenshot(*window).expect("render the window")).collect();
        let differ = shots[0].pixels().zip(shots[1].pixels()).filter(|(a, b)| a != b).count();
        let renders: Vec<usize> =
            windows.iter().zip(renders).map(|((_, page), before)| page.read_with(cx, |view, _| view.renders()) - before).collect();
        println!(
            "[look] {name}: {differ} pixels differ between the field layer and in place (the page rendered {} times with the layer, {} in place)",
            renders[0], renders[1]
        );
        if let Some(folder) = &folder {
            std::fs::create_dir_all(folder).unwrap();
            shots[0].save(folder.join(format!("{name}-layer.png"))).unwrap();
            shots[1].save(folder.join(format!("{name}-in-place.png"))).unwrap();
        }
    }
}

/// The median and p90 ms of a key's frame and the median of its echo's, and each view's renders over the keys.
struct Measured {
    key: (f64, f64),
    echo: f64,
    renders: Vec<(&'static str, u32)>,
}

fn report(what: &str, n: usize, measured: Measured) {
    let per_key: Vec<String> = measured
        .renders
        .iter()
        .map(|(view, count)| format!("{view} {:.2}", *count as f64 / n as f64))
        .collect();
    println!(
        "[measure] {what}, {n} keys: key frame median {:.2} ms, p90 {:.2} ms; echo frame median {:.2} ms; renders per key: {}",
        measured.key.0,
        measured.key.1,
        measured.echo,
        per_key.join(", ")
    );
}

fn settle(cx: &mut VisualTestAppContext, window: AnyWindowHandle, duration: Duration) {
    let until = Instant::now() + duration;
    while Instant::now() < until {
        cx.run_until_parked();
        cx.advance_clock(Duration::from_millis(20));
        let _ = cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx));
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Gives the page's field named `label` the keyboard.
fn focus(cx: &mut VisualTestAppContext, window: AnyWindowHandle, page: &Entity<PageView>, label: &str) {
    let _ = cx.update_window(window, |_, window, cx| {
        let focus = page.read(cx).input_focus(label, cx).unwrap_or_else(|| panic!("no field {label}"));
        focus.focus(window, cx);
    });
    settle(cx, window, Duration::from_millis(300));
}

/// Empties the field with the keyboard, so each measure starts from the same text.
fn clear(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.simulate_keystrokes(window, "cmd-a backspace");
    settle(cx, window, Duration::from_millis(300));
}

/// Types `n` keys into the field with the keyboard, each drawn, then the page's answer to it let in and drawn apart.
fn keys(cx: &mut VisualTestAppContext, window: AnyWindowHandle, page: Option<&Entity<PageView>>, n: usize) -> Measured {
    let gap = Duration::from_millis(std::env::var("BENCH_TYPING_GAP_MS").ok().and_then(|ms| ms.parse().ok()).unwrap_or(0));
    let mut readout = Readout::new();
    readout.begin_measure().unwrap();
    readout.restart_measure().unwrap();
    let (mut frames, mut echoes) = (Vec::new(), Vec::new());
    for at in 0..n {
        std::thread::sleep(gap);
        let sent = page.map(|page| page.read_with(cx, |view, _| view.trees_sent()));
        let key = if at % 7 == 6 { " " } else { "x" };
        let start = Instant::now();
        let _ = cx.update_window(window, |_, window, cx| {
            window.dispatch_keystroke(Keystroke { modifiers: Default::default(), key: key.into(), key_char: Some(key.into()) }, cx);
            window.draw(cx).clear(cx)
        });
        frames.push(start.elapsed().as_secs_f64() * 1000.0);
        // A page hears the key on its own thread and sends its tree back.
        let deadline = Instant::now() + Duration::from_millis(500);
        while let (Some(page), Some(sent)) = (page, sent) {
            if page.read_with(cx, |view, _| view.trees_sent()) != sent || Instant::now() > deadline {
                break;
            }
            cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(1));
        }
        cx.run_until_parked();
        let start = Instant::now();
        let _ = cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx));
        echoes.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    let renders = readout.end_measure().unwrap().renders;
    let at = |times: &mut Vec<f64>, share: f64| {
        times.sort_by(f64::total_cmp);
        times[((times.len() - 1) as f64 * share) as usize]
    };
    Measured { key: (at(&mut frames, 0.5), at(&mut frames, 0.9)), echo: at(&mut echoes, 0.5), renders }
}

/// A long chat with a filter over it and a message box under it, as Claude chat draws one: 120 messages with copy and
/// bookmark buttons, and fields whose text the page sends back on each change.
fn long_chat_with_box() -> String {
    let mut typed = (String::new(), String::new());
    fake::page("page", move |event| {
        if let Some((handler, change)) = event {
            let value = change["value"].as_str().unwrap_or_default().to_owned();
            match handler {
                "5:change" => typed.0 = value,
                _ => typed.1 = value,
            }
        }
        let text = |id: usize, words: String, props: Value| {
            json!({ "id": id, "type": "Text", "props": props, "children": [{ "id": id + 1, "type": "#text", "text": words }] })
        };
        let button = |at: usize, label: &str| {
            json!({ "id": at, "type": "Button", "props": { "onClick": format!("{at}:onClick") }, "children": [{ "id": at + 1000, "type": "#text", "text": label }] })
        };
        let turns: Vec<Value> = (0..120usize)
            .map(|turn| {
                let id = 100 + turn * 10;
                json!({ "id": id, "type": "Card", "props": {}, "children": [
                    text(id + 1, format!("Turn {turn}: refactor the API client"), json!({ "weight": "strong" })),
                    text(id + 3, "The client now retries once on a dropped connection and reads the token from the keychain. ".repeat(4), json!({})),
                    text(id + 5, "All 214 checks passed in 12.3 s.".into(), json!({ "tone": "muted" })),
                    button(id + 7, "Copy"),
                    button(id + 8, "Bookmark"),
                ] })
            })
            .collect();
        json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [{ "id": 1, "type": "Page", "props": { "flush": true }, "children": [
            { "id": 2, "type": "Slot", "props": { "name": "body" }, "children": [
                { "id": 6, "type": "TextInput", "props": { "label": "Filter", "placeholder": "Filter the messages", "value": typed.1, "onChange": "6:change" } },
                { "id": 3, "type": "Scroll", "props": {}, "children": [{ "id": 4, "type": "Stack", "props": { "gap": "l" }, "children": turns }] },
                { "id": 5, "type": "Textarea", "props": { "label": "Message", "rows": 3, "submitOnEnter": true, "value": typed.0, "onChange": "5:change" } }
            ] }
        ] }] })
    })
}
