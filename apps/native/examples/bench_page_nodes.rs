//! What a page with many nodes costs: parsing its tree JSON, drawing it the first time, and a frame that draws the page
//! again (a change of the page, a scroll). Two pages are built for each node count: a `Scroll` column of rows (drawn a
//! window at a time past 40 children) and a `Board` of four `BoardColumn`s of cards (every card drawn). Offscreen.
//!
//! cargo run --release --manifest-path apps/native/Cargo.toml --example bench_page_nodes --features test-support
//!
//! `BENCH_PAGE_NODES=1000,4900,10000,19900` are the node counts (about 3 nodes a row or card).

use std::time::{Duration, Instant};

use gpui::{AppContext, Context, Entity, IntoElement, Render, VisualTestAppContext, Window, div, prelude::*, px, size};
use serde_json::{Value, json};
use wings_native::{
    actions,
    icons::Assets,
    page::{
        PageView,
        plugin::{PluginConfig, PluginHost},
        runtime::fake,
        tree::Tree,
    },
};

struct Root {
    page: Entity<PageView>,
}

impl Render for Root {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(gpui::rgb(wings_native::theme::SURFACE)).child(self.page.clone())
    }
}

/// A card of three nodes: the card, its text and the string in it.
fn card(id: usize) -> Value {
    json!({ "id": id, "type": "Card", "props": {}, "children": [
        { "id": id + 1, "type": "Text", "props": {}, "children": [{ "id": id + 2, "type": "#text", "text": format!("Task {id}: refactor the API client and its retries") }] }
    ] })
}

fn page(body: Value) -> Value {
    json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [{ "id": 1, "type": "Page", "props": { "flush": true }, "children": [
        { "id": 2, "type": "Slot", "props": { "name": "body" }, "children": [body] }
    ] }] })
}

fn scroll(nodes: usize) -> Value {
    let rows: Vec<Value> = (0..nodes / 3).map(|n| card(100 + n * 3)).collect();
    page(json!({ "id": 3, "type": "Scroll", "props": {}, "children": [{ "id": 4, "type": "Stack", "props": { "gap": "m" }, "children": rows }] }))
}

fn board(nodes: usize) -> Value {
    let per = (nodes / 3 / 4).max(1);
    let columns: Vec<Value> = (0..4)
        .map(|c| {
            let base = 100 + c * per * 3 + c * 10;
            let cards: Vec<Value> = (0..per).map(|n| card(base + n * 3 + 10_000 * (c + 1))).collect();
            json!({ "id": 10 + c, "type": "BoardColumn", "props": { "count": per }, "children": [
                { "id": 20 + c, "type": "Slot", "props": { "name": "title" }, "children": [{ "id": 30 + c, "type": "#text", "text": format!("Column {c}") }] },
                { "id": 40 + c, "type": "Slot", "props": { "name": "body" }, "children": cards }
            ] })
        })
        .collect();
    page(json!({ "id": 3, "type": "Board", "props": { "label": "Tasks" }, "children": columns }))
}

fn median(times: &mut [f64]) -> f64 {
    times.sort_by(f64::total_cmp);
    times[times.len() / 2]
}

fn main() {
    let counts: Vec<usize> = std::env::var("BENCH_PAGE_NODES")
        .ok()
        .map(|list| list.split(',').filter_map(|n| n.trim().parse().ok()).collect())
        .unwrap_or_else(|| vec![1_000, 4_900, 10_000, 19_900]);
    let mut cx = VisualTestAppContext::with_asset_source(gpui_platform::current_platform(false), std::sync::Arc::new(Assets));
    cx.executor().allow_parking();
    cx.update(actions::bind);
    let root = std::env::temp_dir().join("wings-native-bench-page-nodes");
    let _ = std::fs::remove_dir_all(&root);
    let host = PluginHost::new(PluginConfig { id: "nodes".into(), root, scope: "bench".into() });
    for (what, build) in [("Scroll rows", scroll as fn(usize) -> Value), ("Board cards", board)] {
        for &nodes in &counts {
            let tree = build(nodes);
            let json = tree.to_string();
            let start = Instant::now();
            let parsed = Tree::parse(&json);
            let parse_ms = start.elapsed().as_secs_f64() * 1000.0;
            let result = match &parsed {
                Ok(_) => "parsed".to_string(),
                Err(error) => format!("refused: {error}"),
            };
            let source = fake::page("page", move |_| tree.clone());
            let handle = cx
                .open_offscreen_window(size(px(1000.0), px(700.0)), |_, cx| {
                    let page = cx.new(|cx| PageView::in_plugin(&host, &source, cx));
                    page.read_with(cx, |view, _| view.set_visible(true));
                    cx.new(|_| Root { page })
                })
                .unwrap();
            let page = handle.read_with(&cx, |root, _| root.page.clone()).unwrap();
            let start = Instant::now();
            let deadline = start + Duration::from_secs(60);
            while parsed.is_ok() && !page.read_with(&cx, |view, _| view.text().contains("Task ")) && Instant::now() < deadline {
                cx.run_until_parked();
                let _ = cx.update_window(handle.into(), |_, window, cx| window.draw(cx).clear(cx));
                std::thread::sleep(Duration::from_millis(5));
            }
            let first_ms = start.elapsed().as_secs_f64() * 1000.0;
            let mut frames = Vec::new();
            for _ in 0..30 {
                page.update(&mut cx, |_, cx| cx.notify());
                let start = Instant::now();
                let _ = cx.update_window(handle.into(), |_, window, cx| window.draw(cx).clear(cx));
                frames.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            let shown = page.read_with(&cx, |view, _| view.text().len());
            println!(
                "[measure] {what}, {nodes} nodes ({} KB JSON): {result}; parse {parse_ms:.1} ms; first page {first_ms:.0} ms; frame that draws the page again median {:.2} ms (p100 {:.2}); {shown} chars of text",
                json.len() / 1024,
                median(&mut frames.clone()),
                frames.iter().copied().fold(0.0, f64::max)
            );
        }
    }
}
