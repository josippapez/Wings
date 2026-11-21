//! The guard for every page: no node that moves on its own draws its page again for a step of it. A spinner, a working
//! dot, a pulsing line, a line that counts its seconds, and a pulsing line that counts, each moves in the window's layer
//! for its kind (`ui::live`), drawn as the workspace draws them: a view of its own for each kind over the page, kept as
//! last drawn and put on screen. One that moved in the page drew the whole page again at each step, which in a long
//! chat kept a core busy while an agent worked.

use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use gpui::{Entity, Global, StyleRefinement, TestApp, TestAppWindow, Window, div, prelude::*};
use serde_json::{Value, json};

use super::{
    plugin::{PluginHost, tests::Root},
    runtime::fake,
    view::PageView,
};
use crate::ui::{
    SPIN_STEP,
    live::{self, Kind},
    place_view, slots,
};

/// The plugin a page runs in and the folder of its storage, kept for as long as the app runs.
struct Hosted {
    _host: Arc<PluginHost>,
    _root: Root,
}

impl Global for Hosted {}

/// The tab's page, kept as last drawn and put on screen, with the window's layers for what moves in pages over it, each
/// a view of its own, counting the window's draws.
struct Tab {
    page: Entity<PageView>,
    layers: [Entity<live::Layer>; 3],
    drawn: usize,
}

impl Render for Tab {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.drawn += 1;
        let id = window.window_handle().window_id();
        slots::start_draw(id);
        place_view(self.page.entity_id(), id);
        div()
            .relative()
            .size_full()
            .child(self.page.clone().cached(StyleRefinement::default().size_full()))
            .children(self.layers.clone())
    }
}

/// Every node kind that moves on its own, the layer it moves in, and the node.
fn moving() -> Vec<(&'static str, Kind, Value)> {
    let line = |props: Value| json!({ "id": 3, "type": "Text", "props": props, "children": [{ "id": 4, "type": "#text", "text": "Working" }] });
    let since = 1_763_470_000_000u64;
    vec![
        ("a spinner", Kind::ViewSpinners, json!({ "id": 3, "type": "Spinner", "props": { "label": "Running" } })),
        ("a large spinner", Kind::ViewSpinners, json!({ "id": 3, "type": "Spinner", "props": { "size": "large" } })),
        ("a working dot", Kind::ViewSpinners, json!({ "id": 3, "type": "Dot", "props": { "state": "working" } })),
        ("a pulsing line", Kind::Pulses, line(json!({ "tone": "muted", "shimmer": true }))),
        ("a pulsing line that counts", Kind::Pulses, line(json!({ "tone": "muted", "shimmer": true, "since": since }))),
        // A background task's "Running · 12s": it drew its page each second.
        ("a line that counts", Kind::Counts, line(json!({ "tone": "muted", "since": since }))),
    ]
}

/// A fill page, as Claude chat's, of `node` and a line to know it is drawn, in a tab, in front, where a line pulses.
fn open_tab(node: Value) -> (TestApp, TestAppWindow<Tab>) {
    let mut app = TestApp::new();
    app.background_executor().allow_parking();
    let root = Root::new();
    let host = Arc::new(PluginHost::new(root.config("fixture")));
    app.update(|cx| cx.set_global(Hosted { _host: host.clone(), _root: root }));
    let source = fake::page("page", move |_| {
        json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [{ "id": 1, "type": "Page", "props": { "flush": true }, "children": [
            { "id": 2, "type": "Slot", "props": { "name": "body" }, "children": [
                node.clone(),
                { "id": 8, "type": "Text", "children": [{ "id": 9, "type": "#text", "text": "Step 99 passes" }] }
            ] }] }] })
    });
    let mut window = app.open_window(move |_, cx| Tab {
        page: cx.new(|cx| PageView::in_plugin(&host, &source, cx).in_card()),
        layers: [Kind::ViewSpinners, Kind::Pulses, Kind::Counts].map(|kind| cx.new(|_| live::Layer::new(kind))),
        drawn: 0,
    });
    window.update(|_, window, _| window.activate_window());
    let deadline = Instant::now() + Duration::from_secs(10);
    while !window.read(|tab, cx| tab.page.read(cx).text().contains("Step 99")) {
        assert!(Instant::now() < deadline, "the page never drew its tree");
        app.run_until_parked();
        window.draw();
        thread::sleep(Duration::from_millis(5));
    }
    window.draw();
    (app, window)
}

#[test]
fn no_node_that_moves_draws_its_page_for_a_step() {
    for (what, kind, node) in moving() {
        let (app, mut window) = open_tab(node);
        let in_layer = window.update(|_, window, _| slots::shown(window.window_handle().window_id(), kind.layer()).len());
        assert_eq!(in_layer, 1, "{what} moves in the {} layer", kind.layer());
        let drawn = |window: &TestAppWindow<Tab>| window.read(|tab, cx| (tab.page.read(cx).renders(), tab.drawn));
        let (pages, frames) = drawn(&window);
        // A second of the spinners' steps, then two of a count's.
        for _ in 0..30 {
            app.advance_clock(SPIN_STEP);
            app.run_until_parked();
        }
        for _ in 0..2 {
            app.advance_clock(Duration::from_secs(1));
            app.run_until_parked();
        }
        let (pages, frames) = (drawn(&window).0 - pages, drawn(&window).1 - frames);
        eprintln!("[measure] {what}, 3 s: the page rendered {pages} times, the window drew {frames}");
        assert_eq!(pages, 0, "{what} drew its page {pages} times");
        assert!(frames >= 2, "{what} never moved: the window drew {frames} times");
    }
}
