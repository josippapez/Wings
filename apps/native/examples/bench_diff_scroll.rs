//! What a frame costs while a big diff scrolls: a made-up file of 10,000 changed and unchanged lines, scrolled three
//! rows a frame by the mouse wheel, in one column and side by side, without review comments and with 300 threads of a
//! comment and two replies each spread through it. Offscreen, in a 1000 x 640 window filled by the diff.
//!
//! cargo run --release --manifest-path apps/native/Cargo.toml --example bench_diff_scroll --features test-support
//!
//! `-- <dir>` saves each case's last frame there, to check what the frames draw.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use gpui::{
    AnyWindowHandle, AppContext, ScrollDelta, ScrollEvent, TouchPhase, VisualTestAppContext, point,
    px, size,
};
use wings_native::{
    diff::{self, View, viewer::Viewer},
    icons::Assets,
    page::plugin::{PluginConfig, PluginHost},
};

const HUNKS: usize = 200;
const THREADS: usize = 300;

/// 200 hunks of 30 unchanged lines, 10 removed and 10 added: 10,000 lines. Returns the patch and the new line numbers
/// of the added lines, where the threads go.
fn patch() -> (String, Vec<usize>) {
    let mut out =
        String::from("diff --git a/src/big.rs b/src/big.rs\n--- a/src/big.rs\n+++ b/src/big.rs\n");
    let (mut old, mut new) = (1, 1);
    let mut added = Vec::new();
    for h in 0..HUNKS {
        out.push_str(&format!("@@ -{old},40 +{new},40 @@ fn section_{h}()\n"));
        for i in 0..30 {
            out.push_str(&format!(
                "     let value_{h}_{i} = compute(input_{i}, \"a string literal long enough to look like code\");\n"
            ));
            old += 1;
            new += 1;
            if i == 14 {
                for d in 0..10 {
                    out.push_str(&format!(
                        "-    let removed_{h}_{d} = legacy(value_{h}_{d});\n"
                    ));
                    old += 1;
                }
                for a in 0..10 {
                    out.push_str(&format!(
                        "+    let added_{h}_{a} = modern(value_{h}_{a}).expect(\"the value is there\");\n"
                    ));
                    added.push(new);
                    new += 1;
                }
            }
        }
    }
    (out, added)
}

/// `THREADS` threads on added lines spread through the file, each a comment and two replies.
fn comments(added: &[usize]) -> serde_json::Value {
    let step = added.len() / THREADS;
    let mut list = Vec::new();
    for t in 0..THREADS {
        let root = (t * 10) as i64 + 1;
        let line = added[t * step];
        let comment = |id: i64, reply: Option<i64>, author: &str, body: &str, minute: usize| {
            serde_json::json!({
                "id": id, "replyTo": reply, "path": "src/big.rs", "line": line, "side": "additions",
                "author": author, "body": body, "createdAt": format!("2026-11-0{}T1{}:{:02}:00Z", 1 + t % 7, t % 10, minute),
                "url": format!("https://github.com/acme/web/pull/42#discussion_r{id}"), "canReply": reply.is_none(),
            })
        };
        list.push(comment(root, None, "maria", "This `expect` panics on a missing value. Could it return the error instead, as **the other loaders** do? See [the guide](https://example.com/guide).", 0));
        list.push(comment(
            root + 1,
            Some(root),
            "tom",
            "Good point, the caller already handles `Err`. I'll change it.",
            5,
        ));
        list.push(comment(root + 2, Some(root), "maria", "Thanks!", 9));
    }
    serde_json::Value::Array(list)
}

fn main() {
    let host = Arc::new(PluginHost::new(PluginConfig {
        id: "bench".into(),
        root: std::env::temp_dir(),
        scope: "bench".into(),
    }));
    let mut cx = VisualTestAppContext::with_asset_source(
        gpui_platform::current_platform(false),
        Arc::new(Assets),
    );
    let (patch, added) = patch();
    let comments = comments(&added);
    for split in [false, true] {
        for threads in [false, true] {
            let payload = if threads {
                serde_json::json!({ "patch": patch, "comments": comments })
            } else {
                serde_json::json!({ "patch": patch })
            };
            let what = format!(
                "{}, {}",
                if split { "side by side" } else { "one column" },
                if threads {
                    "300 threads"
                } else {
                    "no comments"
                }
            );
            let update = match diff::clean_update(&payload) {
                Ok(update) => update,
                Err(error) => {
                    println!("{what}: not opened: {error}");
                    continue;
                }
            };
            let view = View {
                id: "bench:1".into(),
                plugin_id: "bench".into(),
                title: "Big diff".into(),
                subtitle: String::new(),
                cwd: String::new(),
                refresh: false,
                update,
            };
            let host = host.clone();
            let opened = Instant::now();
            let handle = cx
                .open_offscreen_window(size(px(1000.0), px(640.0)), move |_, cx| {
                    cx.new(|cx| {
                        let mut viewer = Viewer::new(view, host, cx);
                        viewer.set_split(split, cx);
                        viewer
                    })
                })
                .unwrap();
            let window: AnyWindowHandle = handle.into();
            let draw = |cx: &mut VisualTestAppContext| {
                let _ = cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx));
            };
            cx.run_until_parked();
            draw(&mut cx);
            let first = opened.elapsed();
            let mut frames = Vec::new();
            for _ in 0..400 {
                let start = Instant::now();
                cx.simulate_event(
                    window,
                    ScrollEvent {
                        position: point(px(640.0), px(400.0)),
                        delta: ScrollDelta::Pixels(point(px(0.0), px(-66.0))),
                        touch_phase: TouchPhase::Moved,
                        ..Default::default()
                    },
                );
                draw(&mut cx);
                frames.push(start.elapsed());
            }
            if let Some(dir) = std::env::args().nth(1) {
                let name = what.replace([',', ' '], "-").replace("--", "-");
                let path = std::path::Path::new(&dir).join(format!("bench-{name}.png"));
                cx.capture_screenshot(window).unwrap().save(&path).unwrap();
                println!("{}", path.display());
            }
            frames.sort();
            let ms = |d: Duration| d.as_secs_f64() * 1000.0;
            let mean = frames.iter().sum::<Duration>() / frames.len() as u32;
            println!(
                "{what}: first frame {:.1} ms; scrolling median {:.2} ms, mean {:.2} ms, p90 {:.2} ms, max {:.2} ms ({} frames)",
                ms(first),
                ms(frames[frames.len() / 2]),
                ms(mean),
                ms(frames[frames.len() * 9 / 10]),
                ms(frames[frames.len() - 1]),
                frames.len()
            );
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
            cx.run_until_parked();
        }
    }
}
