//! A page in a headless window: its tree is drawn, and a click on a button reaches the page's handler.

use super::{
    plugin::{PluginHost, tests::Root},
    runtime::fake::{self, Cx, Guest},
    tree::{Level, Node, Tree},
    view::{PageView, baseline_drops, level_color, three_lines},
};
use crate::{
    installed::fixture::{fixture_main, fixture_page},
    theme,
};
use gpui::{Global, Keystroke, MouseButton, TestApp, TestAppWindow, point, px};
use serde_json::{Value, json};
use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

/// A text node.
fn text_node(id: usize, text: &str) -> Value {
    json!({ "id": id, "type": "#text", "text": text })
}

/// A `Text` of one string.
fn text(id: usize, text_id: usize, text: String) -> Value {
    json!({ "id": id, "type": "Text", "children": [text_node(text_id, &text)] })
}

/// A page that draws `tree` from the number of events it has heard.
fn counting(tree: impl Fn(u32) -> Value + Clone + Send + Sync + 'static) -> String {
    let mut heard = 0;
    fake::page("page", move |event| {
        heard += u32::from(event.is_some());
        tree(heard)
    })
}

/// The number in a handler such as `102:onClick`.
fn number(handler: &str) -> usize {
    handler.split(':').next().unwrap().parse().unwrap()
}

/// The node `id` of `tree`.
fn node(tree: &mut Value, id: u64) -> &mut Value {
    find(tree, id).unwrap()
}

fn find(tree: &mut Value, id: u64) -> Option<&mut Value> {
    if tree["id"] == id {
        return Some(tree);
    }
    tree.get_mut("children")?
        .as_array_mut()?
        .iter_mut()
        .find_map(|child| find(child, id))
}

/// The pilot page as React drew it (`trees/pilot.json`): "Starting" at first, then "Ready" once it had mounted, and a
/// count that its button adds one to.
fn pilot() -> String {
    struct Pilot {
        ready: bool,
        count: u32,
    }
    impl Pilot {
        fn draw(&self, cx: &mut Cx<'_>) {
            let mut tree: Value = serde_json::from_str(include_str!("trees/pilot.json")).unwrap();
            node(&mut tree, 10)["text"] = format!("Count: {}", self.count).into();
            if self.ready {
                node(&mut tree, 3)["text"] = "Ready".into();
                node(&mut tree, 4)["props"]["tone"] = "success".into();
            }
            cx.render(&tree);
        }
    }
    impl Guest for Pilot {
        fn start(&mut self, cx: &mut Cx<'_>) {
            self.draw(cx);
            cx.later(1);
        }
        fn event(&mut self, cx: &mut Cx<'_>, _handler: &str, _payload: &Value) {
            self.count += 1;
            self.draw(cx);
        }
        fn timer(&mut self, cx: &mut Cx<'_>, _id: u32) {
            self.ready = true;
            self.draw(cx);
        }
    }
    fake::source("page", || {
        Box::new(Pilot {
            ready: false,
            count: 0,
        })
    })
}

/// The components page as React drew it (`trees/components.json`), with what its two buttons have counted and what its
/// select was set to.
fn components() -> String {
    let (mut session, mut refreshed, mut retried) = ("web".to_owned(), 0, 0);
    fake::page("page", move |event| {
        match event {
            Some(("9:onClick", _)) => refreshed += 1,
            Some(("30:onClick", _)) => retried += 1,
            Some(("13:onChange", change)) => {
                session = change["value"].as_str().unwrap_or_default().to_owned()
            }
            _ => {}
        }
        let mut tree: Value = serde_json::from_str(include_str!("trees/components.json")).unwrap();
        node(&mut tree, 10)["text"] =
            format!("Session {session}, refreshed {refreshed}, retried {retried}").into();
        node(&mut tree, 13)["props"]["value"] = session.clone().into();
        tree
    })
}

fn graph() -> String {
    let (mut selected, mut offset, mut end, mut requests) = (-1_i64, 0_u64, 64_u64, 0);
    fake::page("page", move |event| {
        match event {
            Some(("5:range", range)) => {
                offset = range["start"].as_u64().unwrap();
                end = range["end"].as_u64().unwrap();
                requests += 1;
            }
            Some((_, pick)) => selected = pick["index"].as_i64().unwrap(),
            None => {}
        }
        let rows: Vec<Value> = (offset..end)
            .map(|i| {
                json!({ "hash": i.to_string(), "subject": format!("Commit {i}"), "author": "Ada", "date": "2026-11-06",
                    "refs": [], "lane": 0, "color": 0, "merge": false, "lines": [0, 0, 2, 0] })
            })
            .collect();
        json!({ "id": 0, "type": "#root", "children": [{ "id": 1, "type": "Page", "props": { "flush": true }, "children": [
            { "id": 2, "type": "Slot", "props": { "name": "body" }, "children": [
                text(3, 4, format!("Selected {selected}, requests {requests}")),
                { "id": 5, "type": "Graph", "props": { "total": 10000, "lanes": 1, "selected": selected, "offset": offset,
                    "rows": rows, "onRangeChange": "5:range", "onSelect": "5:select" } }] }] }] })
    })
}

fn search() -> String {
    let mut query = String::new();
    fake::page("page", move |event| {
        if let Some((_, input)) = event {
            query = input["value"].as_str().unwrap_or_default().to_owned();
        }
        json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [{ "id": 1, "type": "Page", "children": [
            { "id": 2, "type": "Slot", "props": { "name": "body" }, "children": [
                { "id": 3, "type": "SearchField", "props": { "label": "Search commits", "placeholder": "Search commits", "onInput": "3:input" } },
                text(4, 5, format!("Query={query}")) ] }] }] })
    })
}

/// A collapsible `Section` the page keeps the state of: `open` is what its last `onOpenChange` said.
fn folding() -> String {
    let (mut open, mut heard) = (true, 0);
    fake::page("page", move |event| {
        if let Some((handler, change)) = event {
            assert_eq!(handler, "5:onOpenChange");
            open = change["open"].as_bool().unwrap();
            heard += 1;
        }
        json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [{ "id": 1, "type": "Page", "children": [
            { "id": 2, "type": "Slot", "props": { "name": "body" }, "children": [
                text(3, 4, format!("Heard {heard}")),
                // As Claude chat does: a closed section sends its title alone. (`text` reads the whole tree, a closed
                // body too, so the rows leaving is what the page sees.)
                { "id": 5, "type": "Section", "props": { "collapsible": true, "open": open, "onOpenChange": "5:onOpenChange" }, "children": if open { json!([
                    { "id": 6, "type": "Slot", "props": { "name": "title" }, "children": [text_node(7, "Subagents · 4 running")] },
                    { "id": 8, "type": "Slot", "props": { "name": "description" }, "children": [text_node(9, "What this session started.")] },
                    text(10, 11, "Explore the cache".to_owned())]) } else { json!([
                    { "id": 6, "type": "Slot", "props": { "name": "title" }, "children": [text_node(7, "Subagents · 4 running")] }]) } }] }] }] })
    })
}

#[test]
fn a_collapsible_section_folds_on_its_heading_tells_the_page_and_keeps_its_count_when_closed() {
    let (app, mut window) = open(folding());
    settle(&app, &mut window, shows("Explore the cache"));
    assert!(window.read(|view, _| view.text().contains("What this session started.")));

    // A click on the heading line asks the page to close it, and the page's answer hides the rows and the description.
    assert!(window.update(|view, _, cx| view.press_disclosure("Subagents", cx)));
    settle(&app, &mut window, shows("Heard 1"));
    settle(&app, &mut window, |view| !view.text().contains("Explore the cache"));
    assert!(window.read(|view, _| view.text().contains("Subagents · 4 running") && !view.text().contains("What this session started.")));

    assert!(window.update(|view, _, cx| view.press_disclosure("Subagents", cx)));
    settle(&app, &mut window, shows("Heard 2"));
    settle(&app, &mut window, shows("Explore the cache"));
}

#[test]
fn search_field_physical_input_reaches_the_query_and_backspace_edits_it() {
    let (app, mut window) = open(search());
    settle(&app, &mut window, shows("Query="));
    window.simulate_click(point(px(70.0), px(27.0)), MouseButton::Left);
    window.simulate_input("coupon");
    settle(&app, &mut window, shows("Query=coupon"));
    key(&mut window, "backspace");
    settle(&app, &mut window, shows("Query=coupo"));
}

fn form() -> String {
    let (mut name, mut note) = ("x".to_owned(), String::new());
    fake::page("page", move |event| {
        if let Some((handler, change)) = event {
            let value = change["value"].as_str().unwrap_or_default().to_owned();
            if handler == "3:change" {
                name = value
            } else {
                note = value
            }
        }
        json!({ "id": 0, "type": "#root", "children": [{ "id": 1, "type": "Page", "children": [
            { "id": 2, "type": "Slot", "props": { "name": "body" }, "children": [
                { "id": 3, "type": "TextInput", "props": { "label": "Name", "value": name, "maxLength": 5, "onChange": "3:change" } },
                { "id": 4, "type": "Textarea", "props": { "label": "Note", "rows": 3, "value": note, "onChange": "4:change" } },
                text(5, 6, format!("Name={name} Note={}", json!(note))) ] }] }] })
    })
}

/// A page that renders each value it hears a while later, as a busy page does.
fn slow_form() -> String {
    struct Slow {
        name: String,
        heard: Vec<(u32, String)>,
    }
    impl Slow {
        fn draw(&self, cx: &mut Cx<'_>) {
            cx.render(&json!({ "id": 0, "type": "#root", "children": [{ "id": 1, "type": "Page", "children": [
                { "id": 2, "type": "Slot", "props": { "name": "body" }, "children": [
                    { "id": 3, "type": "TextInput", "props": { "label": "Name", "value": self.name, "onChange": "3:change" } },
                    text(5, 6, format!("Name={}.", self.name)) ] }] }] }));
        }
    }
    impl Guest for Slow {
        fn start(&mut self, cx: &mut Cx<'_>) {
            self.draw(cx);
        }
        fn event(&mut self, cx: &mut Cx<'_>, _handler: &str, payload: &Value) {
            let id = cx.later(60);
            self.heard
                .push((id, payload["value"].as_str().unwrap_or_default().to_owned()));
        }
        fn timer(&mut self, cx: &mut Cx<'_>, id: u32) {
            if let Some(at) = self.heard.iter().position(|(heard, _)| *heard == id) {
                self.name = self.heard.remove(at).1;
                self.draw(cx);
            }
        }
    }
    fake::source("page", || {
        Box::new(Slow {
            name: String::new(),
            heard: Vec::new(),
        })
    })
}

#[test]
fn a_page_that_gives_typed_text_back_late_does_not_undo_the_keys_typed_since() {
    let (app, mut window) = open(slow_form());
    settle(&app, &mut window, shows("Name=."));
    window.update(|_, window, cx| window.focus_next(cx));
    window.draw();
    for key in ["a", "b", "c", "d"] {
        window.simulate_input(key);
        // Let the page's late echo of the earlier text arrive while typing goes on.
        thread::sleep(Duration::from_millis(25));
        app.run_until_parked();
        window.draw();
    }
    settle(&app, &mut window, shows("Name=abcd."));
}

#[test]
fn form_inputs_start_from_the_pages_text_stop_at_their_max_length_and_a_textarea_takes_enter() {
    let (app, mut window) = open(form());
    settle(&app, &mut window, shows("Name=x "));
    window.update(|_, window, cx| window.focus_next(cx));
    window.draw();
    window.simulate_input("abcdefgh");
    settle(&app, &mut window, shows("Name=xabcd "));
    window.update(|_, window, cx| window.focus_next(cx));
    window.draw();
    window.simulate_input("hi");
    key(&mut window, "enter");
    window.simulate_input("yo");
    settle(&app, &mut window, shows(r#"Note="hi\nyo""#));
}

/// Cyclops' manual entry: a start and an end, `type="time"`.
fn times() -> String {
    let (mut start, mut end) = (String::new(), String::new());
    fake::page("page", move |event| {
        if let Some((handler, change)) = event {
            let value = change["value"].as_str().unwrap_or_default().to_owned();
            if handler == "3:change" {
                start = value
            } else {
                end = value
            }
        }
        json!({ "id": 0, "type": "#root", "children": [{ "id": 1, "type": "Page", "children": [
            { "id": 2, "type": "Slot", "props": { "name": "body" }, "children": [
                { "id": 3, "type": "TextInput", "props": { "label": "Start", "type": "time", "value": start, "onChange": "3:change" } },
                { "id": 4, "type": "TextInput", "props": { "label": "End", "type": "time", "value": end, "onChange": "4:change" } },
                text(5, 6, format!("Start={start} End={end}.")) ] }] }] })
    })
}

#[test]
fn time_fields_take_digits_and_tab_goes_between_them() {
    let (app, mut window) = open(times());
    settle(&app, &mut window, shows("Start= End=."));
    window.update(|_, window, cx| window.focus_next(cx));
    window.draw();
    window.simulate_input("930");
    // The page hears no time until the minutes are whole.
    settle(&app, &mut window, shows("Start=09:30 End=."));
    key(&mut window, "tab");
    window.draw();
    window.simulate_input("1745");
    settle(&app, &mut window, shows("Start=09:30 End=17:45."));
    // Tab from the last field comes round to the first, and Shift-Tab goes back.
    key(&mut window, "tab");
    window.draw();
    key(&mut window, "backspace");
    settle(&app, &mut window, shows("Start= End=17:45."));
    key(&mut window, "shift-tab");
    window.draw();
    key(&mut window, "backspace");
    settle(&app, &mut window, shows("Start= End=."));
}

#[test]
fn search_field_can_be_reached_with_tab() {
    let (app, mut window) = open(search());
    settle(&app, &mut window, shows("Query="));
    window.update(|_, window, cx| window.focus_next(cx));
    window.draw();
    window.simulate_input("coupon");
    settle(&app, &mut window, shows("Query=coupon"));
}

fn key(window: &mut View, key: &'static str) {
    window.simulate_event(gpui::KeyDownEvent {
        keystroke: Keystroke::parse(key).unwrap(),
        is_held: false,
        prefer_character_input: false,
    });
    window.simulate_event(gpui::KeyUpEvent {
        keystroke: Keystroke::parse(key).unwrap(),
    });
}

#[test]
fn graph_virtualizes_ten_thousand_commits_and_keyboard_scrolls_before_opening() {
    let (app, mut window) = open(graph());
    settle(&app, &mut window, |view| {
        view.graph_window()
            .is_some_and(|(_, _, n)| n > 0 && n <= 256)
    });
    window.update(|_, window, cx| window.focus_next(cx));
    window.draw();
    key(&mut window, "end");
    settle(&app, &mut window, |view| {
        view.graph_window()
            .is_some_and(|(total, offset, n)| total == 10000 && offset > 9900 && n <= 256)
    });
    key(&mut window, "enter");
    settle(&app, &mut window, shows("Selected 9999"));
    let before = window.read(|view, _| view.text());
    for _ in 0..8 {
        window.draw();
        app.run_until_parked();
    }
    assert_eq!(
        window.read(|view, _| view.text()),
        before,
        "unchanged range must not request another tree"
    );
    key(&mut window, "home");
    settle(&app, &mut window, |view| {
        view.graph_window()
            .is_some_and(|(_, offset, _)| offset == 0)
    });
    window.simulate_click(point(px(230.0), px(40.0)), MouseButton::Left);
    settle(&app, &mut window, shows("Selected 0"));
}

#[test]
fn a_narrow_graph_drops_the_author_then_the_date() {
    // (window width, author shown, date shown); the graph is 30 points narrower than its window.
    for (width, author, date) in [(640.0, true, true), (480.0, false, true), (300.0, false, false)] {
        let (app, mut window) = open_narrow(graph(), width);
        settle(&app, &mut window, |view| view.graph_window().is_some());
        settle(&app, &mut window, |view| view.graph_columns() == Some((author, date)));
    }
}

type View = TestAppWindow<PageView>;

/// Two buttons, one in the top left corner of the page and one in the top right, and the presses each has heard.
/// `props` is what `mount` puts on the root.
fn corners(props: Value) -> String {
    let (mut left, mut right) = (0, 0);
    fake::page("page", move |event| {
        match event {
            Some(("2:click", _)) => left += 1,
            Some(_) => right += 1,
            None => {}
        }
        json!({ "id": 0, "type": "#root", "props": props, "children": [
            { "id": 1, "type": "Row", "props": { "justify": "between" }, "children": [
                { "id": 2, "type": "Button", "props": { "onClick": "2:click" }, "children": [text_node(3, "Left")] },
                { "id": 4, "type": "Button", "props": { "onClick": "4:click" }, "children": [text_node(5, "Right")] }] },
            text(6, 7, format!("Pressed {left} {right}.")) ] })
    })
}

/// Clicks `inset` points in from the top left and the top right corners of the window.
fn click_corners(app: &TestApp, window: &mut View, inset: f32) {
    let width = window.update(|_, window, _| window.viewport_size().width);
    for x in [px(inset), width - px(inset)] {
        window.simulate_click(point(x, px(inset)), MouseButton::Left);
    }
    thread::sleep(Duration::from_millis(100));
    app.run_until_parked();
    window.draw();
}

#[test]
fn a_fill_page_has_no_padding_and_fills_the_frame_edge_to_edge() {
    let (app, mut window) = open(corners(json!({ "fill": true })));
    settle(&app, &mut window, shows("Pressed 0 0."));
    // 2pt in from either top corner is already on a button: nothing pads the page, and it spans the window's width.
    click_corners(&app, &mut window, 2.0);
    settle(&app, &mut window, shows("Pressed 1 1."));
}

#[test]
fn a_page_that_does_not_fill_keeps_the_desktops_14pt_padding() {
    let (app, mut window) = open(corners(json!({})));
    settle(&app, &mut window, shows("Pressed 0 0."));
    // plugin-ui.css's `.w-shell { padding: 14px }`: the corners themselves are padding, and just inside it are the buttons.
    click_corners(&app, &mut window, 2.0);
    assert!(window.read(|view, _| view.text().contains("Pressed 0 0.")));
    click_corners(&app, &mut window, 16.0);
    settle(&app, &mut window, shows("Pressed 1 1."));
}

#[test]
fn a_popover_page_has_the_desktops_popover_shell() {
    // `.w-shell[data-place="popover"] { padding: 12px 14px 14px }`: 13pt down is padding on a sidebar's page and a
    // button on a popover's.
    let at = |popover: bool| {
        let mut app = TestApp::new();
        app.background_executor().allow_parking();
        let host = hosted(&mut app);
        let source = corners(json!({}));
        let mut window = app.open_window(move |_, cx| {
            if popover {
                PageView::in_plugin_popover(&host, &source, cx)
            } else {
                PageView::in_plugin(&host, &source, cx)
            }
        });
        settle(&app, &mut window, shows("Pressed 0 0."));
        window.simulate_click(point(px(16.0), px(13.0)), MouseButton::Left);
        thread::sleep(Duration::from_millis(100));
        app.run_until_parked();
        window.draw();
        window.read(|view, _| view.text())
    };
    assert!(at(false).contains("Pressed 0 0."));
    assert!(at(true).contains("Pressed 1 0."));
}

/// Thirty lines, then a field that takes the keyboard: Cyclops' entry edit form, opened below the fold.
fn focus_below() -> String {
    fake::page("page", |_| {
        let mut children: Vec<Value> = (0..30)
            .map(|i| text(10 + i, 100 + i, format!("Line {i}")))
            .collect();
        children.push(json!({ "id": 5, "type": "TextInput", "props": { "label": "Task", "value": "Code review", "autoFocus": true } }));
        json!({ "id": 0, "type": "#root", "children": children })
    })
}

#[test]
fn a_field_that_takes_the_keyboard_below_the_fold_is_scrolled_into_view() {
    let mut app = TestApp::new();
    app.background_executor().allow_parking();
    let host = hosted(&mut app);
    let source = focus_below();
    let bounds = gpui::Bounds::new(point(px(0.0), px(0.0)), gpui::size(px(400.0), px(300.0)));
    let mut window = app.open_window_with_options(
        gpui::WindowOptions {
            window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
        |_, cx| PageView::in_plugin(&host, &source, cx),
    );
    settle(&app, &mut window, shows("Line 29"));
    for _ in 0..5 {
        app.run_until_parked();
        window.draw();
    }
    // Thirty 19pt lines and the field run far past 300pt; the shell scrolls until the field's bottom is 14pt in.
    assert!(window.read(|view, _| view.scrolled()) > 300.0);
}

/// Two buttons at either end of a 720pt column, the desktop's Schedules form, on a page that fills its frame.
fn column() -> String {
    let (mut left, mut right) = (0, 0);
    fake::page("page", move |event| {
        match event {
            Some(("3:click", _)) => left += 1,
            Some(_) => right += 1,
            None => {}
        }
        json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [{ "id": 1, "type": "Stack", "props": { "maxWidth": 720 }, "children": [
            { "id": 2, "type": "Row", "props": { "justify": "between" }, "children": [
                { "id": 3, "type": "Button", "props": { "onClick": "3:click" }, "children": [text_node(4, "Left")] },
                { "id": 5, "type": "Button", "props": { "onClick": "5:click" }, "children": [text_node(6, "Right")] }] },
            text(7, 8, format!("Pressed {left} {right}.")) ] }] })
    })
}

/// Clicks 6pt from the top at each of `xs`.
fn click_along(app: &TestApp, window: &mut View, xs: [f32; 2]) {
    for x in xs {
        window.simulate_click(point(px(x), px(6.0)), MouseButton::Left);
    }
    thread::sleep(Duration::from_millis(100));
    app.run_until_parked();
    window.draw();
}

#[test]
fn a_stack_with_a_max_width_is_a_centred_column_that_narrow_frames_fill() {
    let (app, mut window) = open(column());
    settle(&app, &mut window, shows("Pressed 0 0."));
    let width = window.update(|_, window, _| window.viewport_size().width.as_f32());
    assert!(width > 800.0, "the window is narrower than the column");
    // The column is 720pt wide in the middle of the window: outside it is nothing, and its ends are the buttons.
    let (left, right) = ((width - 720.0) / 2.0, (width + 720.0) / 2.0);
    click_along(&app, &mut window, [left - 6.0, right + 6.0]);
    assert!(window.read(|view, _| view.text().contains("Pressed 0 0.")));
    click_along(&app, &mut window, [left + 6.0, right - 6.0]);
    settle(&app, &mut window, shows("Pressed 1 1."));

    // In a window narrower than the column, it takes the whole width.
    let mut app = TestApp::new();
    app.background_executor().allow_parking();
    let host = hosted(&mut app);
    let source = column();
    let bounds = gpui::Bounds::new(point(px(0.0), px(0.0)), gpui::size(px(640.0), px(400.0)));
    let mut window = app.open_window_with_options(
        gpui::WindowOptions {
            window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
        |_, cx| PageView::in_plugin(&host, &source, cx),
    );
    settle(&app, &mut window, shows("Pressed 0 0."));
    assert_eq!(
        window.update(|_, window, _| window.viewport_size().width),
        px(640.0)
    );
    click_along(&app, &mut window, [4.0, 636.0]);
    settle(&app, &mut window, shows("Pressed 1 1."));
}

/// A list row opened to a card of three lines, as Session detail's activity rows open to their detail.
fn opened_row() -> String {
    counting(|opened| {
        let detail: Vec<Value> = (1..=3)
            .map(|i| text(10 + i, 20 + i, format!("Detail line {i}")))
            .collect();
        json!({ "id": 0, "type": "#root", "children": [
            { "id": 1, "type": "List", "children": [{ "id": 2, "type": "ListItem", "children": [
                { "id": 3, "type": "ListButton", "props": { "onClick": "3:click", "aria-expanded": true }, "children": [text(4, 5, "pnpm test".into())] },
                { "id": 6, "type": "Card", "children": detail }] }] },
            text(7, 8, format!("Opened {opened}.")) ] })
    })
}

#[test]
fn a_row_opened_to_a_card_keeps_its_whole_line() {
    let (app, mut window) = open(opened_row());
    settle(&app, &mut window, shows("Opened 0."));
    let width = window.update(|_, window, _| window.viewport_size().width.as_f32());
    // The row's own line reaches the far end: beside it was the card, which took that end and pushed the row down to
    // the card's middle.
    window.simulate_click(point(px(width - 30.0), px(24.0)), MouseButton::Left);
    settle(&app, &mut window, shows("Opened 1."));
}

/// The plugin a page runs in and the folder of its storage, which the app keeps for as long as it runs.
struct Hosted {
    _host: Arc<PluginHost>,
    _root: Root,
}

impl Global for Hosted {}

/// A plugin that runs as long as `app` does, for a page to open in.
fn hosted(app: &mut TestApp) -> Arc<PluginHost> {
    let root = Root::new();
    let host = Arc::new(PluginHost::new(root.config("fixture")));
    app.update(|cx| {
        cx.set_global(Hosted {
            _host: host.clone(),
            _root: root,
        })
    });
    host
}

fn open(source: String) -> (TestApp, View) {
    let mut app = TestApp::new();
    app.background_executor().allow_parking();
    let host = hosted(&mut app);
    let window = app.open_window(move |_, cx| PageView::in_plugin(&host, &source, cx));
    (app, window)
}

/// The page runs on its own thread, so this waits in real time for the view to have what it needs.
fn settle(app: &TestApp, window: &mut View, done: impl Fn(&PageView) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.run_until_parked();
        window.draw();
        if window.read(|view, _| done(view)) {
            return;
        }
        let text = window.read(|view, _| view.text());
        assert!(Instant::now() < deadline, "the view shows {text:?}");
        thread::sleep(Duration::from_millis(5));
    }
}

fn shows(text: &'static str) -> impl Fn(&PageView) -> bool {
    move |view| view.text().contains(text)
}

/// Inside the pilot's button: under its 44px toolbar and 14px of body padding, at the body's left edge.
const PILOT_BUTTON: (f32, f32) = (22.0, 74.0);

#[test]
fn the_pilot_page_is_drawn_and_a_click_on_its_button_renders_the_new_count() {
    let (app, mut window) = open(pilot());
    settle(&app, &mut window, shows("Ready"));
    window.read(|view, _| {
        let text = view.text();
        assert!(text.contains("Pilot") && text.contains("Increment") && text.contains("Count: 0"));
    });

    let (x, y) = PILOT_BUTTON;
    let clicked = Instant::now();
    window.simulate_click(point(px(x), px(y)), MouseButton::Left);
    settle(&app, &mut window, shows("Count: 1"));
    eprintln!("click to new tree drawn: {:?}", clicked.elapsed());
    window.simulate_click(point(px(x), px(y)), MouseButton::Left);
    settle(&app, &mut window, shows("Count: 2"));

    // A click beside the button reaches nothing.
    window.simulate_click(point(px(600.0), px(300.0)), MouseButton::Left);
    thread::sleep(Duration::from_millis(100));
    app.run_until_parked();
    assert!(window.read(|view, _| view.text().contains("Count: 2")));
}

#[test]
fn the_keyboard_presses_the_focused_button() {
    let (app, mut window) = open(pilot());
    settle(&app, &mut window, shows("Ready"));
    window.update(|_, window, cx| window.focus_next(cx));
    window.draw();
    window.simulate_event(gpui::KeyDownEvent {
        keystroke: Keystroke::parse("enter").unwrap(),
        is_held: false,
        prefer_character_input: false,
    });
    window.simulate_event(gpui::KeyUpEvent {
        keystroke: Keystroke::parse("enter").unwrap(),
    });
    settle(&app, &mut window, shows("Count: 1"));
}

fn throws() -> String {
    struct Throws;
    impl Guest for Throws {
        fn start(&mut self, cx: &mut Cx<'_>) {
            cx.render(&json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [
                { "id": 1, "type": "Page", "props": {}, "children": [
                    { "id": 2, "type": "Slot", "props": { "name": "body" }, "children": [
                        { "id": 3, "type": "Button", "props": { "onClick": "3:onClick" }, "children": [text_node(4, "Boom")] },
                        { "id": 5, "type": "Hologram", "props": {}, "children": [text_node(6, "not drawn")] }] }] }] }));
        }
        // A tree the host refuses is an error shown under the page, as a throw was.
        fn event(&mut self, cx: &mut Cx<'_>, _handler: &str, _payload: &Value) {
            cx.render(&json!({ "id": 0, "type": "#root", "children": "nope" }));
        }
    }
    fake::source("page", || Box::new(Throws))
}

fn checkbox() -> String {
    let mut checked = false;
    fake::page("page", move |event| {
        if let Some((_, change)) = event {
            checked = change["checked"].as_bool().unwrap_or_default();
        }
        json!({ "id": 0, "type": "#root", "props": {}, "children": [
            { "id": 1, "type": "Page", "props": {}, "children": [{ "id": 2, "type": "Slot", "props": { "name": "body" }, "children": [
                { "id": 3, "type": "Checkbox", "props": { "checked": checked, "onChange": "3:onChange" }, "children": [text_node(4, "Delete branch")] }
            ] }] }] })
    })
}

fn disabled_checkbox() -> String {
    fake::page("page", |_| {
        json!({ "id": 0, "type": "#root", "props": {}, "children": [
            { "id": 1, "type": "Page", "props": {}, "children": [{ "id": 2, "type": "Slot", "props": { "name": "body" }, "children": [
                { "id": 3, "type": "Checkbox", "props": { "checked": false, "disabled": true, "onChange": "3:onChange" }, "children": [text_node(4, "Delete branch")] }
            ] }] }] })
    })
}

#[test]
fn what_the_page_throws_is_shown_under_it_and_what_the_host_does_not_know_is_not_drawn() {
    let (app, mut window) = open(throws());
    settle(&app, &mut window, shows("Boom"));
    assert!(window.read(|view, _| !view.text().contains("not drawn")));
    window.simulate_click(point(px(22.0), px(30.0)), MouseButton::Left);
    settle(&app, &mut window, |view| {
        view.error().is_some_and(|error| error.contains("nope"))
    });
}

#[test]
fn a_checkbox_is_keyboard_operable_and_its_disabled_state_is_skipped() {
    let (app, mut window) = open(checkbox());
    settle(&app, &mut window, shows("Delete branch"));
    assert_eq!(window.read(|view, _| view.checkbox_checked()), Some(false));
    window.update(|_, window, cx| window.focus_next(cx));
    window.draw();
    window.simulate_event(gpui::KeyDownEvent {
        keystroke: Keystroke::parse("space").unwrap(),
        is_held: false,
        prefer_character_input: false,
    });
    settle(&app, &mut window, |view| {
        view.checkbox_checked() == Some(true)
    });
    window.simulate_event(gpui::KeyDownEvent {
        keystroke: Keystroke::parse("enter").unwrap(),
        is_held: false,
        prefer_character_input: false,
    });
    settle(&app, &mut window, |view| {
        view.checkbox_checked() == Some(false)
    });

    let (disabled_app, mut disabled) = open(disabled_checkbox());
    settle(&disabled_app, &mut disabled, shows("Delete branch"));
    disabled.update(|_, window, cx| window.focus_next(cx));
    disabled.draw();
    disabled.simulate_event(gpui::KeyDownEvent {
        keystroke: Keystroke::parse("space").unwrap(),
        is_held: false,
        prefer_character_input: false,
    });
    nothing_happens(&disabled_app);
    assert_eq!(
        disabled.read(|view, _| view.checkbox_checked()),
        Some(false)
    );
}

#[test]
fn a_plugin_page_in_a_window_asks_main_and_shows_the_count_main_saved() {
    let root = Root::new();
    let host = Arc::new(PluginHost::new(root.config("fixture")));
    host.start_main(&fixture_main(), |_| {}).unwrap();
    let mut app = TestApp::new();
    app.background_executor().allow_parking();
    let page_host = host.clone();
    let mut window =
        app.open_window(move |_, cx| PageView::in_plugin(&page_host, &fixture_page(), cx));
    settle(&app, &mut window, shows("Count: 0"));

    // The fixture lays out like the pilot: its button is first in the body.
    let (x, y) = PILOT_BUTTON;
    let clicked = Instant::now();
    window.simulate_click(point(px(x), px(y)), MouseButton::Left);
    settle(&app, &mut window, shows("Count: 1"));
    eprintln!(
        "click, ask to main, saved, pushed back and drawn: {:?}",
        clicked.elapsed()
    );
    assert_eq!(root.saved("fixture", "count"), Some(serde_json::json!(1)));
}

/// Draws, then loops until it is stopped.
fn stops() -> String {
    struct Stops;
    impl Guest for Stops {
        fn start(&mut self, cx: &mut Cx<'_>) {
            cx.render(&json!({ "id": 0, "type": "#root", "children": [text(1, 2, "Up".into())] }));
            cx.later(300);
        }
        fn timer(&mut self, cx: &mut Cx<'_>, _id: u32) {
            cx.stop("the page ran for too long and was stopped");
        }
    }
    fake::source("page", || Box::new(Stops))
}

/// A stopped page stayed stopped wherever it was shown but a sidebar, whose Reload had to be pressed. Any page now
/// starts again by itself, Reload does it at once, and why it stopped is in the log.
#[test]
fn a_plugin_page_that_stopped_starts_again_by_itself_or_at_once_with_reload_and_its_stop_is_logged()
{
    let logs = tempfile::tempdir().unwrap();
    crate::page::log::keep_in(logs.path().join("plugins.log"));
    let root = Root::new();
    let host = Arc::new(PluginHost::new(root.config("fixture")));
    let mut app = TestApp::new();
    app.background_executor().allow_parking();
    let page_host = host.clone();
    let mut window = app.open_window(move |_, cx| PageView::in_plugin(&page_host, &stops(), cx));
    let up = |view: &PageView| view.text().contains("Up") && !view.stopped();
    settle(&app, &mut window, up);
    settle(&app, &mut window, |view| view.stopped());
    let kept = std::fs::read_to_string(logs.path().join("plugins.log")).unwrap();
    assert!(
        kept.contains(" fixture page.wasm: the page ran for too long and was stopped"),
        "{kept}"
    );

    // Two seconds after it stopped, and not before.
    app.advance_clock(Duration::from_millis(1900));
    app.run_until_parked();
    assert!(window.read(|view, _| view.stopped()));
    app.advance_clock(Duration::from_millis(200));
    settle(&app, &mut window, up);

    settle(&app, &mut window, |view| view.stopped());
    window.update(|view, _, cx| view.reload(cx));
    settle(&app, &mut window, up);

    // Not once its plugin is gone.
    settle(&app, &mut window, |view| view.stopped());
    drop(host);
    app.advance_clock(Duration::from_secs(600));
    app.run_until_parked();
    window.draw();
    assert!(window.read(|view, _| view.stopped()));
}

// The components page, under its 44px toolbar and 14px of padding and 10px apart: the refresh button (as the pilot's,
// 58 to 90), the select (100 to 132) with its menu below it (items 32px tall from 141, so centred at 157, 189 and 221),
// and the disclosure's line (142 to 167), whose body has two 25px rows and then a Retry button (218 to 250).
const REFRESH: (f32, f32) = PILOT_BUTTON;
const SELECT: (f32, f32) = (60.0, 116.0);
const FIRST_OPTION: (f32, f32) = (60.0, 157.0);
const SECOND_OPTION: (f32, f32) = (60.0, 189.0);
const DISABLED_OPTION: (f32, f32) = (60.0, 221.0);
const DISCLOSURE: (f32, f32) = (40.0, 150.0);
const RETRY: (f32, f32) = (40.0, 234.0);

fn click(window: &mut View, (x, y): (f32, f32)) {
    window.simulate_click(point(px(x), px(y)), MouseButton::Left);
}

/// Waits long enough for a click that should do nothing to have done it.
fn nothing_happens(app: &TestApp) {
    thread::sleep(Duration::from_millis(100));
    app.run_until_parked();
}

#[test]
fn a_disclosure_opens_on_a_click_and_stays_as_it_is_through_the_renders_the_page_makes() {
    let (app, mut window) = open(components());
    settle(&app, &mut window, shows("refreshed 0, retried 0"));
    assert_eq!(window.read(|view, _| view.open_disclosures()), 0);

    click(&mut window, DISCLOSURE);
    settle(&app, &mut window, |view| view.open_disclosures() == 1);
    // Its body is drawn now: the button in it reaches the page.
    click(&mut window, RETRY);
    settle(&app, &mut window, shows("retried 1"));

    // The page renders again, with new text in the same tree: the disclosure is still open.
    click(&mut window, REFRESH);
    settle(&app, &mut window, shows("refreshed 1"));
    assert_eq!(window.read(|view, _| view.open_disclosures()), 1);
    click(&mut window, RETRY);
    settle(&app, &mut window, shows("retried 2"));

    click(&mut window, DISCLOSURE);
    settle(&app, &mut window, |view| view.open_disclosures() == 0);
    // And its body is gone: where the button was is nothing now.
    click(&mut window, RETRY);
    nothing_happens(&app);
    assert!(window.read(|view, _| view.text().contains("retried 2")));
}

#[test]
fn picking_in_a_select_tells_the_page_the_value_and_a_disabled_option_cannot_be_picked() {
    let (app, mut window) = open(components());
    settle(&app, &mut window, shows("Session web,"));
    assert!(!window.read(|view, _| view.menu_open()));

    click(&mut window, SELECT);
    settle(&app, &mut window, PageView::menu_open);
    click(&mut window, DISABLED_OPTION);
    nothing_happens(&app);
    assert!(window.read(|view, _| view.menu_open() && view.text().contains("Session web,")));

    click(&mut window, SECOND_OPTION);
    settle(&app, &mut window, shows("Session api,"));
    assert!(!window.read(|view, _| view.menu_open()));

    // The page's `onChange` got `api` as a plain value, and the select shows what the page set it to: picking the
    // first option again goes back.
    click(&mut window, SELECT);
    settle(&app, &mut window, PageView::menu_open);
    click(&mut window, FIRST_OPTION);
    settle(&app, &mut window, shows("Session web,"));
}

#[test]
fn a_select_menu_closes_on_a_press_outside_and_on_its_own_trigger() {
    let (app, mut window) = open(components());
    settle(&app, &mut window, shows("Session web,"));

    click(&mut window, SELECT);
    settle(&app, &mut window, PageView::menu_open);
    click(&mut window, (600.0, 600.0));
    settle(&app, &mut window, |view| !view.menu_open());

    click(&mut window, SELECT);
    settle(&app, &mut window, PageView::menu_open);
    click(&mut window, SELECT);
    settle(&app, &mut window, |view| !view.menu_open());
    // Nothing was picked.
    assert!(window.read(|view, _| view.text().contains("Session web,")));
}

/// A row's `⋯` menu at the end of a line, as Disk usage's: what was picked last, and the menu with two items.
fn row_menu() -> String {
    let mut picked = "nothing".to_owned();
    fake::page("page", move |event| {
        match event {
            Some(("5:onClick", _)) => picked = "reveal".into(),
            Some(("7:onClick", _)) => picked = "trash".into(),
            _ => {}
        }
        json!({ "id": 0, "type": "#root", "children": [
            { "id": 1, "type": "Row", "props": { "justify": "between" }, "children": [
                { "id": 2, "type": "Text", "children": [text_node(9, &format!("picked {picked}"))] },
                { "id": 3, "type": "Menu", "props": { "align": "end" }, "children": [
                    { "id": 4, "type": "Button", "props": { "variant": "ghost", "size": "icon-xs", "label": "More" }, "children": [text_node(10, "⋯")] },
                    { "id": 5, "type": "Button", "props": { "onClick": "5:onClick" }, "children": [text_node(11, "Reveal")] },
                    { "id": 7, "type": "Button", "props": { "variant": "destructive", "onClick": "7:onClick" }, "children": [text_node(12, "Trash")] },
                ]},
            ]},
        ]})
    })
}

fn center(bounds: gpui::Bounds<gpui::Pixels>) -> (f32, f32) {
    let at = bounds.center();
    (f32::from(at.x), f32::from(at.y))
}

#[test]
fn a_menu_opens_over_the_page_under_its_button_and_closes_on_a_pick_a_press_outside_and_escape() {
    let (app, mut window) = open(row_menu());
    settle(&app, &mut window, shows("picked nothing"));
    window.draw();
    let trigger = crate::ui::spot_at("page-menu:More").expect("the menu's button is drawn");
    assert!(!window.read(|view, _| view.menu_open()));

    click(&mut window, center(trigger));
    settle(&app, &mut window, PageView::menu_open);
    window.draw();
    // Under the button and lined up with its right edge, not in the row, where its items ran past the window.
    let reveal = crate::ui::spot_at("menu:Reveal").expect("the item is drawn");
    assert!(reveal.top() > trigger.bottom(), "{reveal:?} under {trigger:?}");
    assert!(reveal.right() <= trigger.right() && reveal.right() > trigger.right() - px(12.0), "{reveal:?} at {trigger:?}");
    assert_eq!(crate::ui::spot_at("page-menu:More"), Some(trigger), "the row kept its place");

    click(&mut window, center(reveal));
    settle(&app, &mut window, shows("picked reveal"));
    assert!(!window.read(|view, _| view.menu_open()));

    click(&mut window, center(trigger));
    settle(&app, &mut window, PageView::menu_open);
    click(&mut window, (20.0, 400.0));
    settle(&app, &mut window, |view| !view.menu_open());

    click(&mut window, center(trigger));
    settle(&app, &mut window, PageView::menu_open);
    key(&mut window, "escape");
    settle(&app, &mut window, |view| !view.menu_open());
    assert!(window.read(|view, _| view.text().contains("picked reveal")));
}

#[test]
fn a_menu_s_items_are_picked_with_the_arrow_keys_and_enter() {
    let (app, mut window) = open(row_menu());
    settle(&app, &mut window, shows("picked nothing"));
    window.draw();
    let trigger = crate::ui::spot_at("page-menu:More").expect("the menu's button is drawn");

    click(&mut window, center(trigger));
    settle(&app, &mut window, PageView::menu_open);
    // Down twice reaches Trash; Enter picks it and closes the menu.
    key(&mut window, "down");
    key(&mut window, "down");
    key(&mut window, "enter");
    settle(&app, &mut window, shows("picked trash"));
    assert!(!window.read(|view, _| view.menu_open()));

    // Down wraps past the end, Home and End jump.
    click(&mut window, center(trigger));
    settle(&app, &mut window, PageView::menu_open);
    key(&mut window, "end");
    key(&mut window, "down");
    key(&mut window, "end");
    key(&mut window, "home");
    key(&mut window, "enter");
    settle(&app, &mut window, shows("picked reveal"));
}

/// `row_menu` at the foot of a 400pt window, under lines that fill it, as a chat's menus by its message box are.
fn foot_menu() -> String {
    fake::page("page", move |_| {
        let lines: Vec<Value> = (0..17).map(|n| text(100 + 2 * n, 101 + 2 * n, format!("Line {n}"))).collect();
        json!({ "id": 0, "type": "#root", "children": [
            { "id": 1, "type": "Stack", "props": { "gap": "none" }, "children": lines },
            { "id": 2, "type": "Row", "children": [
                { "id": 3, "type": "Menu", "props": { "align": "end" }, "children": [
                    { "id": 4, "type": "Button", "props": { "variant": "ghost", "size": "icon-xs", "label": "More" }, "children": [text_node(10, "⋯")] },
                    { "id": 5, "type": "Button", "props": { "onClick": "5:onClick" }, "children": [text_node(11, "Reveal")] },
                    { "id": 7, "type": "Button", "props": { "onClick": "7:onClick" }, "children": [text_node(12, "Trash")] },
                ]},
            ]},
        ]})
    })
}

#[test]
fn a_menu_with_no_room_under_its_button_opens_over_it() {
    let (app, mut window) = open_narrow(foot_menu(), 300.0);
    settle(&app, &mut window, shows("Line 16"));
    window.draw();
    let trigger = crate::ui::spot_at("page-menu:More").expect("the menu's button is drawn");
    assert!(trigger.bottom() > px(330.0), "the button is at the window's foot: {trigger:?}");
    click(&mut window, center(trigger));
    settle(&app, &mut window, PageView::menu_open);
    window.draw();
    // It covered the button, pushed up into the window from under it.
    let trash = crate::ui::spot_at("menu:Trash").expect("the item is drawn");
    assert!(trash.bottom() < trigger.top(), "{trash:?} over {trigger:?}");
}

#[test]
fn a_gauge_bar_is_blue_until_its_level_and_then_warn_and_danger() {
    let json = r##"{"id":0,"type":"#root","children":[{"id":1,"type":"Gauge","props":{"label":"x","percent":PERCENT,"levels":[70,90]}}]}"##;
    let color = |percent: &str| {
        let tree = Tree::parse(&json.replace("PERCENT", percent)).unwrap();
        let Node::Gauge(gauge) = &tree.children[0] else {
            panic!("a gauge")
        };
        level_color(gauge.level())
    };
    assert_eq!(color("40"), theme::palette().info);
    assert_eq!(color("85"), theme::palette().working);
    assert_eq!(color("96"), theme::palette().danger);
    assert_eq!(level_color(Level::Normal), theme::palette().info);
}

/// A page in a frame `width` points wide.
fn open_narrow(source: String, width: f32) -> (TestApp, View) {
    let mut app = TestApp::new();
    app.background_executor().allow_parking();
    let host = hosted(&mut app);
    let bounds = gpui::Bounds::new(point(px(0.0), px(0.0)), gpui::size(px(width), px(400.0)));
    let window = app.open_window_with_options(
        gpui::WindowOptions {
            window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
        move |_, cx| PageView::in_plugin(&host, &source, cx),
    );
    (app, window)
}

/// Clicks at `at`, waits, and says whether the page heard it.
fn pressed_at(app: &TestApp, window: &mut View, at: (f32, f32), heard: &'static str) -> bool {
    click(window, at);
    nothing_happens(app);
    window.draw();
    window.read(|view, _| view.text().contains(heard))
}

/// A pull request's summary chip that shows its checks: an outlined pill with a click.
fn summary_chip() -> String {
    counting(|heard| {
        json!({ "id": 0, "type": "#root", "children": [
            { "id": 1, "type": "Badge", "props": { "variant": "outline", "tone": "danger", "onClick": "1:click" }, "children": [text_node(2, "1 failed ›")] },
            text(3, 4, format!("Heard {heard}.")) ] })
    })
}

#[test]
fn a_badge_with_a_click_is_a_button_for_the_keyboard_and_the_pointer() {
    let (app, mut window) = open_narrow(summary_chip(), 300.0);
    settle(&app, &mut window, shows("Heard 0."));
    window.update(|_, window, cx| window.focus_next(cx));
    window.draw();
    key(&mut window, "enter");
    settle(&app, &mut window, shows("Heard 1."));
    // On the pill, under the page's 14pt of padding.
    assert!(pressed_at(&app, &mut window, (30.0, 24.0), "Heard 2."));
}

/// A pull request's tabs, of the "line" variant.
fn line_tabs() -> String {
    let mut picked = "a".to_owned();
    fake::page("page", move |event| {
        if let Some((_, pick)) = event {
            picked = pick["value"].as_str().unwrap_or_default().to_owned();
        }
        json!({ "id": 0, "type": "#root", "children": [
            { "id": 1, "type": "Chips", "props": { "label": "Pull request", "line": true, "value": picked,
                "options": [{ "value": "a", "label": "Overview" }, { "value": "b", "label": "Checks" }], "onChange": "1:change" } },
            text(2, 3, format!("Picked {picked}.")) ] })
    })
}

#[test]
fn line_tabs_are_picked_with_the_arrow_keys_and_a_click() {
    let (app, mut window) = open_narrow(line_tabs(), 300.0);
    settle(&app, &mut window, shows("Picked a."));
    window.update(|_, window, cx| window.focus_next(cx));
    window.draw();
    key(&mut window, "right");
    settle(&app, &mut window, shows("Picked b."));
    assert!(pressed_at(&app, &mut window, (30.0, 26.0), "Picked a."));
}

/// A chip whose label is longer than its line, as a Cyclops task suggested under its field.
fn long_chip() -> String {
    counting(|pressed| {
        json!({ "id": 0, "type": "#root", "children": [
            { "id": 1, "type": "Row", "props": { "gap": "s", "wrap": true }, "children": [
                { "id": 2, "type": "Button", "props": { "chip": true, "onClick": "2:click" }, "children": [
                    text_node(3, "Report table keeps the Unassigned group last when sorted ascending, on the web and on mobile")] }] },
            text(4, 5, format!("Pressed {pressed}.")) ] })
    })
}

#[test]
fn a_chip_longer_than_its_line_ends_at_the_pages_padding() {
    let (app, mut window) = open_narrow(long_chip(), 300.0);
    settle(&app, &mut window, shows("Pressed 0."));
    // The page's 14pt padding on the right is past the chip's end: the chip used to run on through it and off the frame.
    assert!(!pressed_at(&app, &mut window, (293.0, 28.0), "Pressed 1."));
    assert!(pressed_at(&app, &mut window, (280.0, 28.0), "Pressed 1."));
}

#[test]
fn a_clamped_text_ends_its_third_line_in_an_ellipsis() {
    use gpui::{Styled, TextOverflow, div};
    let mut clamped = three_lines(div());
    let style = clamped.text_style();
    assert_eq!(style.line_clamp, Some(3));
    // Without it the third line of a long message ran on to the box's edge and was cut there, mid-word.
    assert!(
        matches!(style.text_overflow, Some(TextOverflow::Truncate(ref end)) if end.as_ref() == "…")
    );
}

/// A chip that holds a dot and a word, as Work item's state menu and Issues' parent chip: the word can be pressed.
fn dot_chip() -> String {
    counting(|pressed| {
        json!({ "id": 0, "type": "#root", "children": [
            { "id": 1, "type": "Row", "props": { "gap": "s" }, "children": [
                { "id": 2, "type": "Button", "props": { "chip": true }, "children": [
                    { "id": 3, "type": "Dot", "props": { "tone": "info" } },
                    { "id": 4, "type": "Text", "props": { "onClick": "4:click" }, "children": [text_node(5, "Active")] }] }] },
            text(6, 7, format!("Pressed {pressed}.")) ] })
    })
}

#[test]
fn a_dot_in_a_chip_keeps_the_chips_gap_from_the_word_after_it() {
    let (app, mut window) = open_narrow(dot_chip(), 300.0);
    settle(&app, &mut window, shows("Pressed 0."));
    // The chip's content starts 11pt in, at 25: the 6pt dot, then 4pt of gap, and the word at 35. The word used to start
    // right after the dot, at 31, so 33 was on it.
    assert!(!pressed_at(&app, &mut window, (33.0, 28.0), "Pressed 1."));
    assert!(pressed_at(&app, &mut window, (40.0, 28.0), "Pressed 1."));
}

/// A row of Properties as wings-plugin-ui/native draws it: a label, and a value that starts with a dot, like Sprint
/// board's "State ● New". The value's text can be pressed.
fn dot_value() -> String {
    counting(|pressed| {
        json!({ "id": 0, "type": "#root", "children": [
            { "id": 1, "type": "Row", "props": { "gap": "m", "align": "baseline" }, "children": [
                { "id": 2, "type": "Text", "props": { "tone": "muted", "size": "small" }, "children": [text_node(3, "State")] },
                { "id": 4, "type": "Text", "props": { "size": "small", "grow": true }, "children": [{ "id": 5, "type": "Row", "props": { "gap": "s" }, "children": [
                    { "id": 6, "type": "Dot", "props": {} },
                    { "id": 7, "type": "Text", "props": { "onClick": "7:click" }, "children": [text_node(8, "New, the state it is in")] }] }] }] },
            text(9, 10, format!("Pressed {pressed}.")) ] })
    })
}

#[test]
fn a_value_that_starts_with_a_dot_sits_level_with_its_label() {
    let (app, mut window) = open_narrow(dot_value(), 400.0);
    settle(&app, &mut window, shows("Pressed 0."));
    // 2pt under the row's top, beside the label, is the value's text: the dot's bottom used to be lined up with the
    // label's, which pushed the value down by half a line.
    assert!(pressed_at(&app, &mut window, (150.0, 16.0), "Pressed 1."));
}

/// A line of a check run's log as PR tracker draws it: its number, small, and its text, which wraps in a narrow frame.
/// The number can be pressed.
fn log_line() -> String {
    counting(|pressed| {
        json!({ "id": 0, "type": "#root", "children": [
            { "id": 1, "type": "Row", "props": { "gap": "m", "align": "baseline" }, "children": [
                { "id": 2, "type": "Text", "props": { "mono": true, "size": "tiny", "tone": "muted", "onClick": "2:click" }, "children": [text_node(3, "3")] },
                { "id": 4, "type": "Text", "props": { "mono": true, "size": "small", "grow": true }, "children": [
                    text_node(5, "src/sync/module-1.test.ts (8 tests) 120ms, then the next file and the one after it")] }] },
            text(6, 7, format!("Pressed {pressed}.")) ] })
    })
}

#[test]
fn a_log_lines_number_sits_by_its_first_line_when_the_line_wraps() {
    let (app, mut window) = open_narrow(log_line(), 300.0);
    settle(&app, &mut window, shows("Pressed 0."));
    // The text takes three lines from the top of the row, 14pt down; the number used to sit by the last of them.
    assert!(pressed_at(&app, &mut window, (17.0, 22.0), "Pressed 1."));
}

/// A line of a check run's log that is one word wider than the frame, an Azure log's "=====" rule, with a text under it
/// that can be pressed.
fn log_rule() -> String {
    counting(|pressed| {
        json!({ "id": 0, "type": "#root", "children": [
            { "id": 1, "type": "Row", "props": { "gap": "m", "align": "baseline" }, "children": [
                { "id": 2, "type": "Text", "props": { "mono": true, "size": "tiny", "tone": "muted" }, "children": [text_node(3, "3")] },
                { "id": 4, "type": "Text", "props": { "mono": true, "size": "small", "grow": true }, "children": [text_node(5, &"=".repeat(78))] }] },
            { "id": 6, "type": "Text", "props": { "onClick": "6:click" }, "children": [text_node(7, &format!("Pressed {pressed}."))] } ] })
    })
}

#[test]
fn a_log_line_wider_than_the_frame_wraps_inside_it() {
    let (app, mut window) = open_narrow(log_rule(), 300.0);
    settle(&app, &mut window, shows("Pressed 0."));
    // The rule takes three lines inside the frame, so the text under it starts 76pt down. The rule used to be one line
    // as wide as its 78 characters, past the frame's edge, with the text right under it.
    assert!(pressed_at(&app, &mut window, (20.0, 85.0), "Pressed 1."));
}

/// A folded group of a check run's log as PR tracker draws it: its number, which can be pressed, beside a list button
/// that holds its title, which wraps in a narrow frame.
fn log_group() -> String {
    let mut pressed = 0;
    fake::page("page", move |event| {
        if let Some(("2:click", _)) = event {
            pressed += 1;
        }
        json!({ "id": 0, "type": "#root", "children": [
            { "id": 1, "type": "Row", "props": { "gap": "m", "align": "baseline" }, "children": [
                { "id": 2, "type": "Text", "props": { "mono": true, "size": "tiny", "tone": "muted", "onClick": "2:click" }, "children": [text_node(3, "  1")] },
                { "id": 4, "type": "ListButton", "props": { "align": "baseline", "aria-expanded": false, "onClick": "4:open" }, "children": [
                    { "id": 5, "type": "Text", "props": { "mono": true, "size": "small", "grow": true, "weight": "strong" }, "children": [
                        text_node(6, "Run pnpm --filter web test -- --reporter=verbose --coverage.enabled=true")] }] }] },
            text(7, 8, format!("Pressed {pressed}.")) ] })
    })
}

#[test]
fn a_log_groups_number_sits_on_one_line_by_its_titles_first() {
    let (app, mut window) = open_narrow(log_group(), 300.0);
    settle(&app, &mut window, shows("Pressed 0."));
    // The number is on the title's first line, 17pt to 35pt down: it used to sit by the button's foot, three lines down.
    assert!(pressed_at(&app, &mut window, (16.0, 26.0), "Pressed 1."));
    // Under it is the title's second line and no number: squeezed by the button, "  1" broke at its padding there.
    assert!(!pressed_at(&app, &mut window, (16.0, 45.0), "Pressed 2."));
}

/// A board of one column with a card in it, as Issues' board in its page with no padding of its own. The card can be
/// pressed.
fn board() -> String {
    counting(|pressed| {
        json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [
            { "id": 8, "type": "Page", "props": { "flush": true }, "children": [{ "id": 9, "type": "Slot", "props": { "name": "body" }, "children": [
                { "id": 1, "type": "Board", "props": { "label": "Issues" }, "children": [
                    { "id": 2, "type": "BoardColumn", "children": [
                        { "id": 3, "type": "Slot", "props": { "name": "title" }, "children": [text_node(4, "To do")] },
                        { "id": 5, "type": "Slot", "props": { "name": "body" }, "children": [
                            { "id": 6, "type": "Button", "props": { "variant": "outline", "onClick": "6:click" }, "children": [
                                text_node(7, &format!("Pressed {pressed}."))] }] }] }] }] }] }] })
    })
}

#[test]
fn a_boards_columns_are_boxes_inside_the_pages_padding() {
    let (app, mut window) = open_narrow(board(), 300.0);
    settle(&app, &mut window, shows("Pressed 0."));
    // The column is 14pt in from the frame, the page's padding the board takes on, and its card 9pt into the column, so
    // the card starts at 23. Both used to start at the frame's edge.
    assert!(!pressed_at(&app, &mut window, (6.0, 54.0), "Pressed 1."));
    assert!(pressed_at(&app, &mut window, (30.0, 54.0), "Pressed 1."));
}

/// A stack of two strings and a button, as a `Time` with its prefix ("Updated ", "14 h ago") makes. The button can be
/// pressed.
fn two_strings() -> String {
    counting(|pressed| {
        json!({ "id": 0, "type": "#root", "children": [
            { "id": 1, "type": "Stack", "children": [text_node(2, "Updated "), text_node(3, "14 h ago"),
                { "id": 4, "type": "Button", "props": { "onClick": "4:click" }, "children": [text_node(5, &format!("Pressed {pressed}."))] }] }] })
    })
}

#[test]
fn strings_side_by_side_in_a_stack_are_one_line_of_it() {
    let (app, mut window) = open_narrow(two_strings(), 300.0);
    settle(&app, &mut window, shows("Pressed 0."));
    // One line of text and the stack's 8pt gap put the button at 41 to 73. Each string was a line of its own, which put
    // it at 68 to 100.
    assert!(pressed_at(&app, &mut window, (40.0, 50.0), "Pressed 1."));
    assert!(!pressed_at(&app, &mut window, (40.0, 90.0), "Pressed 2."));
}

/// An empty text over a button, as PR tracker's line that says whether a token is saved, before it knows. The button
/// can be pressed.
fn empty_text() -> String {
    counting(|pressed| {
        json!({ "id": 0, "type": "#root", "children": [
            { "id": 1, "type": "Stack", "children": [text(2, 3, String::new()),
                { "id": 4, "type": "Button", "props": { "onClick": "4:click" }, "children": [text_node(5, &format!("Pressed {pressed}."))] }] }] })
    })
}

#[test]
fn an_empty_text_takes_no_line() {
    let (app, mut window) = open_narrow(empty_text(), 300.0);
    settle(&app, &mut window, shows("Pressed 0."));
    // Under the stack's 8pt gap the button is at 22 to 54. The empty text took a line, which put it at 41 to 73.
    assert!(pressed_at(&app, &mut window, (40.0, 28.0), "Pressed 1."));
    assert!(!pressed_at(&app, &mut window, (40.0, 64.0), "Pressed 2."));
}

/// A commit in Changes' history: a list button of two lines, its subject over a line that can be pressed.
fn commit() -> String {
    counting(|pressed| {
        json!({ "id": 0, "type": "#root", "children": [
            { "id": 1, "type": "ListButton", "props": { "layout": "column" }, "children": [
                text(2, 3, "Add coupon validation".into()),
                { "id": 4, "type": "Text", "props": { "onClick": "4:click" }, "children": [text_node(5, "aaaaaaa Ada Lovelace")] }] },
            text(6, 7, format!("Pressed {pressed}.")) ] })
    })
}

#[test]
fn a_column_list_buttons_lines_are_close_together() {
    let (app, mut window) = open_narrow(commit(), 300.0);
    settle(&app, &mut window, shows("Pressed 0."));
    // The second line is 2pt under the subject, from 38 to 57; it was 8pt under it, from 44 to 63, further than the
    // next commit was from this one.
    assert!(pressed_at(&app, &mut window, (40.0, 40.0), "Pressed 1."));
    assert!(!pressed_at(&app, &mut window, (40.0, 60.0), "Pressed 2."));
}

/// A prompt in Session detail's activity: a list button lined up on its first lines, the name that can be pressed, the
/// prompt that wraps, and its time.
fn prompt() -> String {
    counting(|pressed| {
        json!({ "id": 0, "type": "#root", "children": [
            { "id": 1, "type": "ListButton", "props": { "align": "baseline", "aria-expanded": false }, "children": [
                { "id": 2, "type": "Text", "props": { "tone": "info", "weight": "strong", "onClick": "2:click" }, "children": [text_node(3, "You")] },
                { "id": 4, "type": "Text", "props": { "grow": true }, "children": [
                    text_node(5, "Rework the checkout page so the coupon field sits under the order summary")] },
                { "id": 6, "type": "Text", "props": { "tone": "muted" }, "children": [text_node(7, "11:46 PM")] }] },
            text(8, 9, format!("Pressed {pressed}.")) ] })
    })
}

#[test]
fn a_list_button_lined_up_on_baselines_keeps_its_name_by_the_first_line() {
    let (app, mut window) = open_narrow(prompt(), 300.0);
    settle(&app, &mut window, shows("Pressed 0."));
    // "You" is on the prompt's first line, 17 to 36 down; it sat halfway down the prompt's three lines, from 36.
    assert!(pressed_at(&app, &mut window, (45.0, 24.0), "Pressed 1."));
}

/// A disclosure whose summary is longer than its line, as Sprint board's "What was detected", with a button under it.
fn long_summary() -> String {
    counting(|pressed| {
        json!({ "id": 0, "type": "#root", "children": [
            { "id": 1, "type": "Disclosure", "children": [{ "id": 2, "type": "Slot", "props": { "name": "summary" }, "children": [
                text_node(3, "What was detected in the repos, the remotes and the Azure DevOps organizations this Mac is signed in to (12 calls, 3.4 s)")] }] },
            { "id": 4, "type": "Button", "props": { "onClick": "4:click" }, "children": [text_node(5, "Next")] },
            text(6, 7, format!("Pressed {pressed}.")) ] })
    })
}

#[test]
fn a_disclosure_summary_longer_than_its_line_wraps() {
    let (app, mut window) = open_narrow(long_summary(), 300.0);
    settle(&app, &mut window, shows("Pressed 0."));
    // The summary takes three lines, so the button under it is this far down; on one line it ran off the frame and the
    // button sat 50pt higher.
    assert!(pressed_at(&app, &mut window, (40.0, 110.0), "Pressed 1."));
}

/// A select of sixty options at the top of a 400pt frame, as Cyclops' projects in its form.
fn long_select() -> String {
    let mut picked = "none".to_owned();
    fake::page("page", move |event| {
        if let Some((_, pick)) = event {
            picked = pick["value"].as_str().unwrap_or_default().to_owned();
        }
        let options: Vec<Value> = (0..60)
            .map(|i| json!({ "value": format!("o{i}"), "label": format!("Project {i}") }))
            .collect();
        json!({ "id": 0, "type": "#root", "children": [
            { "id": 1, "type": "Select", "props": { "label": "Project", "value": "o0", "onChange": "1:change", "options": options } },
            text(2, 3, format!("Picked {picked}.")) ] })
    })
}

#[test]
fn a_select_menu_longer_than_the_window_scrolls_to_its_last_option() {
    let (app, mut window) = open_narrow(long_select(), 300.0);
    settle(&app, &mut window, shows("Picked none."));
    click(&mut window, (100.0, 30.0));
    settle(&app, &mut window, PageView::menu_open);
    // The menu ends 8pt above the frame's bottom and scrolls, so its last option is pressed there. It ran on past the
    // window, which moved it up to start at the top, and there was option 10.
    window.simulate_scroll(point(px(100.0), px(200.0)), point(px(0.0), px(-10_000.0)));
    window.draw();
    click(&mut window, (100.0, 370.0));
    settle(&app, &mut window, shows("Picked o59."));
}

/// A task field with recent tasks to pick from and a Start button under it, as Cyclops' timer form: the tree is what
/// wings-plugin-ui/native's `Combobox` renders, the field and the tasks that match it as chips, or a muted line when
/// none do, and only the field once it holds a task.
fn task_field() -> String {
    const TASKS: [&str; 5] = [
        "Code review",
        "Checkout redesign",
        "Sidebar labels",
        "Password fields",
        "Standup",
    ];
    let (mut typed, mut started) = (String::new(), 0);
    fake::page("page", move |event| {
        match event {
            Some(("3:onChange", change)) => {
                typed = change["value"].as_str().unwrap_or_default().to_owned()
            }
            Some(("7:onClick", _)) => started += 1,
            Some((handler, _)) => typed = TASKS[(number(handler) - 100) / 2].to_owned(),
            None => {}
        }
        let found: Vec<&str> = TASKS
            .into_iter()
            .filter(|task| task.to_lowercase().contains(&typed.to_lowercase()))
            .take(8)
            .collect();
        let under = if found.len() == 1 && found[0] == typed {
            None
        } else if !found.is_empty() {
            let chips: Vec<Value> = found
                .iter()
                .map(|task| {
                    let i = TASKS.iter().position(|t| t == task).unwrap();
                    json!({ "id": 100 + 2 * i, "type": "Button", "props": { "chip": true, "pressed": false, "onClick": format!("{}:onClick", 100 + 2 * i) },
                        "children": [text_node(101 + 2 * i, task)] })
                })
                .collect();
            Some(
                json!({ "id": 4, "type": "Row", "props": { "gap": "s", "wrap": true }, "children": chips }),
            )
        } else if !typed.is_empty() {
            Some(
                json!({ "id": 5, "type": "Text", "props": { "size": "small", "tone": "muted" }, "children": [text_node(6, "No matching tasks.")] }),
            )
        } else {
            None
        };
        let mut stack = vec![
            json!({ "id": 3, "type": "TextInput", "props": { "label": "Task", "value": typed, "maxLength": 2000, "onChange": "3:onChange" } }),
        ];
        stack.extend(under);
        json!({ "id": 0, "type": "#root", "children": [
            { "id": 1, "type": "Stack", "props": { "gap": "s" }, "children": stack },
            { "id": 7, "type": "Button", "props": { "onClick": "7:onClick" }, "children": [text_node(8, "Start")] },
            text(9, 10, format!("Task={typed}, started {started}.")) ] })
    })
}

/// The task field, 14pt in from the page's corner and 32pt tall.
const TASK: (f32, f32) = (100.0, 30.0);
/// Start, 10pt under the field: it stays there whatever the field suggests.
const START: (f32, f32) = (40.0, 72.0);
/// The first suggestion: the list starts 4pt under the field, over Start.
const FIRST_TASK: (f32, f32) = (60.0, 71.0);

#[test]
fn a_combobox_lists_its_options_over_the_page_and_the_keyboard_or_a_click_picks_one() {
    let (app, mut window) = open(task_field());
    settle(&app, &mut window, shows("Task=, started 0."));
    // The five tasks don't take a place in the page: Start is right under the field. They pushed it down, and a press
    // there picked the first of them.
    click(&mut window, START);
    settle(&app, &mut window, shows("Task=, started 1."));

    // Typed into, the field lists what matches; Down, Down and Enter pick the second.
    click(&mut window, TASK);
    window.simulate_input("re");
    settle(&app, &mut window, shows("Task=re,"));
    for name in ["down", "down", "enter"] {
        key(&mut window, name);
    }
    settle(
        &app,
        &mut window,
        shows("Task=Checkout redesign, started 1."),
    );

    // Escape puts the list away, and Start under it can be pressed again.
    key(&mut window, "backspace");
    settle(&app, &mut window, shows("Task=Checkout redesig,"));
    key(&mut window, "escape");
    nothing_happens(&app);
    click(&mut window, START);
    settle(
        &app,
        &mut window,
        shows("Task=Checkout redesig, started 2."),
    );

    // Taking the keyboard again lists the task, and a click picks it without pressing Start under it. The field keeps
    // the keyboard.
    click(&mut window, TASK);
    nothing_happens(&app);
    window.draw();
    click(&mut window, FIRST_TASK);
    settle(
        &app,
        &mut window,
        shows("Task=Checkout redesign, started 2."),
    );
    window.simulate_input("!");
    settle(
        &app,
        &mut window,
        shows("Task=Checkout redesign!, started 2."),
    );

    // Nothing matches now, which the list says over Start. A press outside puts it away and leaves the field the
    // keyboard; a press in the field brings it back.
    click(&mut window, (300.0, 300.0));
    nothing_happens(&app);
    window.draw();
    click(&mut window, TASK);
    nothing_happens(&app);
    window.draw();
    assert!(!pressed_at(&app, &mut window, START, "started 3."));
    click(&mut window, (300.0, 300.0));
    nothing_happens(&app);
    window.draw();
    click(&mut window, START);
    settle(
        &app,
        &mut window,
        shows("Task=Checkout redesign!, started 3."),
    );
}

/// Cyclops' edit form: the task field takes the keyboard when the form opens, holding a task that more than one recent
/// task matches, and the project select sits right under it. The page asks for the tasks not to open on focus
/// (`openOnFocus: false`, sent on the field by wings-plugin-ui/native's `Combobox`), as the desktop's form does.
fn edit_form() -> String {
    const TASKS: [&str; 2] = ["Standup", "FE Standup"];
    let (mut typed, mut project) = ("Standup".to_owned(), "a".to_owned());
    fake::page("page", move |event| {
        match event {
            Some(("3:onChange", change)) => {
                typed = change["value"].as_str().unwrap_or_default().to_owned()
            }
            Some(("7:onChange", change)) => {
                project = change["value"].as_str().unwrap_or_default().to_owned()
            }
            Some((handler, _)) => typed = TASKS[(number(handler) - 100) / 2].to_owned(),
            None => {}
        }
        let found: Vec<&str> = TASKS
            .into_iter()
            .filter(|task| task.to_lowercase().contains(&typed.to_lowercase()))
            .collect();
        let mut stack = vec![
            json!({ "id": 3, "type": "TextInput", "props": { "label": "Task", "value": typed, "maxLength": 2000,
            "autoFocus": true, "openOnFocus": false, "onChange": "3:onChange" } }),
        ];
        if !(found.len() == 1 && found[0] == typed) && !found.is_empty() {
            let chips: Vec<Value> = found
                .iter()
                .map(|task| {
                    let i = TASKS.iter().position(|t| t == task).unwrap();
                    json!({ "id": 100 + 2 * i, "type": "Button", "props": { "chip": true, "pressed": false, "onClick": format!("{}:onClick", 100 + 2 * i) },
                        "children": [text_node(101 + 2 * i, task)] })
                })
                .collect();
            stack.push(json!({ "id": 4, "type": "Row", "props": { "gap": "s", "wrap": true }, "children": chips }));
        }
        json!({ "id": 0, "type": "#root", "children": [
            { "id": 1, "type": "Stack", "props": { "gap": "s" }, "children": stack },
            { "id": 7, "type": "Select", "props": { "label": "Project", "value": project, "onChange": "7:onChange",
                "options": [{ "value": "a", "label": "Healthcare" }, { "value": "b", "label": "57hours" }] } },
            text(9, 10, format!("Task={typed}, project={project}.")) ] })
    })
}

/// The project select, 10pt under the task field.
const PROJECT: (f32, f32) = (100.0, 72.0);
/// Its second option, in the menu under it (from 97pt, 32pt a row).
const SECOND_PROJECT: (f32, f32) = (100.0, 145.0);

#[test]
fn a_combobox_that_does_not_open_on_focus_leaves_the_select_under_it_to_be_picked() {
    let (app, mut window) = open(edit_form());
    settle(&app, &mut window, shows("Task=Standup, project=a."));
    // The field has the keyboard, and its two tasks aren't listed over the select: a press there opens the select. The
    // list was open, and the press picked "FE Standup" as the task, so the project couldn't be changed.
    click(&mut window, PROJECT);
    settle(&app, &mut window, PageView::menu_open);
    click(&mut window, SECOND_PROJECT);
    settle(&app, &mut window, shows("Task=Standup, project=b."));

    // A press in the field lists its tasks, as on the desktop, and a click picks one: the second, a row under the first.
    click(&mut window, TASK);
    nothing_happens(&app);
    window.draw();
    click(&mut window, (FIRST_TASK.0, FIRST_TASK.1 + 32.0));
    settle(&app, &mut window, shows("Task=FE Standup, project=b."));
}

/// Cyclops' project picker as wings-plugin-ui/native's `Combobox` renders one without `freeText`: the field shows the
/// project picked, selects it when it takes the keyboard and hears when it loses it, and every project that matches
/// what's typed is a chip with its client after its name.
fn project_picker() -> String {
    let mut projects: Vec<(String, String, String)> = vec![
        ("p1".into(), "Wings".into(), "Prototyp".into()),
        ("p2".into(), "Website".into(), "Acme".into()),
    ];
    projects.extend((0..20).map(|i| {
        (
            format!("q{i}"),
            format!("Project {}", i + 1),
            format!("Client {}", i + 1),
        )
    }));
    let (mut picked, mut query) = ("p1".to_owned(), None::<String>);
    fake::page("page", move |event| {
        match event {
            Some(("3:onChange", change)) => {
                query = Some(change["value"].as_str().unwrap_or_default().to_owned())
            }
            Some(("3:onBlur", _)) => query = None,
            Some((handler, _)) => {
                picked = projects[(number(handler) - 100) / 3].0.clone();
                query = None;
            }
            None => {}
        }
        let pick = projects.iter().find(|p| p.0 == picked).unwrap();
        let asked = query.clone().unwrap_or_default().to_lowercase();
        let under = if projects
            .iter()
            .any(|p| format!("{} {}", p.1, p.2).to_lowercase().contains(&asked))
        {
            let chips: Vec<Value> = projects
                .iter()
                .enumerate()
                .filter(|(_, p)| format!("{} {}", p.1, p.2).to_lowercase().contains(&asked))
                .map(|(i, p)| {
                    json!({ "id": 100 + 3 * i, "type": "Button", "props": { "chip": true, "pressed": false, "onClick": format!("{}:onClick", 100 + 3 * i) },
                        "children": [text_node(101 + 3 * i, &p.1),
                            { "id": 102 + 3 * i, "type": "Text", "props": { "size": "small", "tone": "muted" }, "children": [text_node(1000 + i, &p.2)] }] })
                })
                .collect();
            json!({ "id": 4, "type": "Row", "props": { "gap": "s", "wrap": true }, "children": chips })
        } else {
            json!({ "id": 5, "type": "Text", "props": { "size": "small", "tone": "muted" }, "children": [text_node(6, "No matching projects.")] })
        };
        let shown = query
            .clone()
            .unwrap_or_else(|| format!("{} · {}", pick.1, pick.2));
        json!({ "id": 0, "type": "#root", "children": [
            { "id": 1, "type": "Stack", "props": { "gap": "s" }, "children": [
                { "id": 3, "type": "TextInput", "props": { "label": "Project", "value": shown, "maxLength": 2000, "selectOnFocus": true,
                    "onBlur": "3:onBlur", "onChange": "3:onChange" } }, under] },
            text(9, 10, format!("Project={picked}, query={}.", query.as_deref().unwrap_or("null"))) ] })
    })
}

#[test]
fn a_picker_lists_every_project_with_its_client_and_typing_replaces_the_pick_to_search_them() {
    let (app, mut window) = open(project_picker());
    settle(&app, &mut window, shows("Project=p1, query=null."));
    // Taking the keyboard lists the projects over the page, each name with its client beside it, not run into it.
    click(&mut window, TASK);
    nothing_happens(&app);
    window.draw();
    assert!(
        crate::ui::spot_at("combobox-option:Wings").is_some(),
        "Wings is listed on its own"
    );
    // The pick is selected, so typing replaces it and searches, by client too; Enter takes the first match.
    window.simulate_input("client 2");
    settle(&app, &mut window, shows("query=client 2."));
    key(&mut window, "enter");
    settle(&app, &mut window, shows("Project=q1, query=null."));
    // Taking the keyboard again selects the new pick; Down from the first match, then Enter, takes the second.
    window.update(|_, window, cx| window.blur(cx));
    window.draw();
    click(&mut window, TASK);
    nothing_happens(&app);
    window.simulate_input("project 1");
    settle(&app, &mut window, shows("query=project 1."));
    for name in ["down", "enter"] {
        key(&mut window, name);
    }
    settle(&app, &mut window, shows("Project=q9, query=null."));
}

/// A chat's message box, as the app draws it: a Textarea that takes Shift-Tab and the history keys, and what the page
/// heard of its keys.
fn message_box() -> String {
    let mut heard: Vec<String> = Vec::new();
    fake::page("page", move |event| {
        if let Some((handler, key)) = event
            && handler.ends_with("onKeyDown")
        {
            let shift = if key["shiftKey"] == true { "shift-" } else { "" };
            heard.push(format!("{shift}{}", key["key"].as_str().unwrap_or_default()));
        }
        json!({ "id": 0, "type": "#root", "children": [
            { "id": 3, "type": "Textarea", "props": { "label": "Message", "rows": 2, "value": "one", "submitOnEnter": true,
                "takesShiftTab": true, "historyKeys": true, "onKeyDown": "3:onKeyDown" } },
            text(9, 10, format!("heard={}.", heard.join(","))) ] })
    })
}

#[test]
fn a_message_box_sends_escape_shift_tab_and_the_history_keys_to_the_page_as_your_own_keys() {
    let (app, mut window) = open(message_box());
    settle(&app, &mut window, shows("heard=."));
    window.update(|_, window, cx| window.focus_next(cx));
    window.draw();
    assert!(!window.read(|view, _| view.acted()));
    key(&mut window, "escape");
    settle(&app, &mut window, shows("heard=Escape."));
    // Escape lets the page write to a program for a moment, as Enter does, so a chat can stop its turn.
    assert!(window.read(|view, _| view.acted()));
    // The caret is at the end of the one line: Up leaves it to the page, as does Down.
    key(&mut window, "up");
    key(&mut window, "down");
    settle(&app, &mut window, shows("heard=Escape,ArrowUp,ArrowDown."));
    key(&mut window, "shift-tab");
    settle(&app, &mut window, shows("heard=Escape,ArrowUp,ArrowDown,shift-Tab."));
}

/// A chat's message box with its slash commands under it as wings-plugin-ui/native's `Combobox` renders them: a Textarea
/// that sends on Enter, then the commands that match as chips. What it heard: the text, the pick, and the keys.
fn slash_box() -> String {
    let (mut typed, mut picked, mut heard) = (String::new(), String::from("none"), Vec::<String>::new());
    fake::page("page", move |event| {
        match event {
            Some(("3:onChange", change)) => typed = change["value"].as_str().unwrap_or_default().to_owned(),
            Some(("3:onKeyDown", key)) => heard.push(key["key"].as_str().unwrap_or_default().to_owned()),
            Some((handler, _)) => {
                picked = ["help", "compact"][(number(handler) - 100) / 2].to_owned();
                typed = format!("/{picked} ");
            }
            None => {}
        }
        let mut stack = vec![json!({ "id": 3, "type": "Textarea", "props": { "label": "Message", "rows": 2, "value": typed,
            "submitOnEnter": true, "takesShiftTab": true, "onChange": "3:onChange", "onKeyDown": "3:onKeyDown" } })];
        if typed == "/" {
            let chips: Vec<Value> = ["help", "compact"]
                .iter()
                .enumerate()
                .map(|(i, name)| {
                    json!({ "id": 100 + 2 * i, "type": "Button", "props": { "chip": true, "pressed": false, "onClick": format!("{}:onClick", 100 + 2 * i) },
                        "children": [text_node(101 + 2 * i, &format!("/{name}"))] })
                })
                .collect();
            stack.push(json!({ "id": 4, "type": "Row", "props": { "gap": "s", "wrap": true }, "children": chips }));
        }
        json!({ "id": 0, "type": "#root", "children": [
            { "id": 1, "type": "Stack", "props": { "gap": "s" }, "children": stack },
            text(9, 10, format!("text={typed}, picked={picked}, heard={}.", heard.join(","))) ] })
    })
}

#[test]
fn tab_in_a_message_box_with_a_list_picks_the_first_option_or_the_one_the_arrows_reached_and_types_no_tab() {
    let (app, mut window) = open(slash_box());
    settle(&app, &mut window, shows("text=, picked=none, heard=."));
    window.update(|_, window, cx| window.focus_next(cx));
    window.draw();
    window.simulate_input("/");
    settle(&app, &mut window, shows("text=/, picked=none,"));
    key(&mut window, "tab");
    // The page hears the pick, not a Tab key, and the text has no tab in it.
    settle(&app, &mut window, shows("text=/help , picked=help, heard=."));

    // With the arrows moved to the second, Tab takes that one.
    for _ in "/help ".chars() {
        key(&mut window, "backspace");
    }
    window.simulate_input("/");
    settle(&app, &mut window, shows("text=/, picked=help,"));
    key(&mut window, "down");
    key(&mut window, "tab");
    settle(&app, &mut window, shows("text=/compact , picked=compact, heard=."));
}

/// Cyclops' start time, a `type="time"` input holding 08:30, and what the page hears.
fn start_time() -> String {
    let mut start = "08:30".to_owned();
    fake::page("page", move |event| {
        if let Some((_, change)) = event {
            start = change["value"].as_str().unwrap_or_default().to_owned();
        }
        json!({ "id": 0, "type": "#root", "children": [
            { "id": 3, "type": "TextInput", "props": { "label": "Start", "type": "time", "value": start, "onChange": "3:onChange" } },
            text(9, 10, format!("Start={start}.")) ] })
    })
}

#[test]
fn a_time_input_types_over_its_hours_and_minutes_and_steps_them_with_the_arrows() {
    let (app, mut window) = open(start_time());
    settle(&app, &mut window, shows("Start=08:30."));
    // The keyboard comes to the hours: 1 and 0 make them 10, over the 08 that is there, and move on to the minutes.
    window.update(|_, window, cx| window.focus_next(cx));
    window.draw();
    window.simulate_input("1");
    window.simulate_input("0");
    settle(&app, &mut window, shows("Start=10:30."));
    key(&mut window, "up");
    settle(&app, &mut window, shows("Start=10:31."));
    key(&mut window, "left");
    key(&mut window, "down");
    settle(&app, &mut window, shows("Start=09:31."));
    // Emptied, the hours make no time; a 7 is a whole hour, and Tab goes on to the minutes, which 4 and 5 fill.
    key(&mut window, "backspace");
    settle(&app, &mut window, shows("Start=."));
    window.simulate_input("7");
    settle(&app, &mut window, shows("Start=07:31."));
    key(&mut window, "left");
    key(&mut window, "tab");
    window.simulate_input("4");
    window.simulate_input("5");
    settle(&app, &mut window, shows("Start=07:45."));
}

/// Cyclops' entry: its task and its time, and an edit button among the row's actions.
fn entry_actions() -> String {
    counting(|edited| {
        json!({ "id": 0, "type": "#root", "children": [
            { "id": 1, "type": "List", "children": [{ "id": 2, "type": "ListItem", "children": [
                { "id": 3, "type": "Text", "props": { "grow": true }, "children": [text_node(4, "Code review")] },
                text(5, 6, "45m".into()),
                { "id": 7, "type": "Slot", "props": { "name": "actions" }, "children": [
                    { "id": 8, "type": "Button", "props": { "variant": "ghost", "size": "icon-sm", "label": "Edit Code review", "onClick": "8:click" },
                        "children": [text_node(9, "E")] }] }] }] },
            text(10, 11, format!("Edited {edited}.")) ] })
    })
}

#[test]
fn a_list_items_actions_sit_over_its_end_and_take_a_click_or_the_keyboard() {
    let (app, mut window) = open_narrow(entry_actions(), 300.0);
    settle(&app, &mut window, shows("Edited 0."));
    // Over the row's end, 6pt in from it, where the 45m is: the page dropped them.
    assert!(pressed_at(&app, &mut window, (266.0, 32.0), "Edited 1."));
    // Tab reaches the button, and Enter presses it.
    window.update(|_, window, cx| window.focus_next(cx));
    window.draw();
    key(&mut window, "enter");
    settle(&app, &mut window, shows("Edited 2."));
}

/// An issue's title row: a large title that doesn't grow, and its key after it on its baseline.
fn title_row() -> String {
    counting(|heard| {
        json!({ "id": 0, "type": "#root", "children": [
            { "id": 1, "type": "Row", "props": { "gap": "l", "align": "baseline" }, "children": [
                { "id": 2, "type": "Text", "props": { "size": "large", "weight": "strong" }, "children": [
                    text_node(3, "Backfill order totals for rows created before the currency column existed")] },
                { "id": 4, "type": "Text", "props": { "size": "small", "mono": true, "onClick": "4:click" }, "children": [text_node(5, "WEB-3")] }] },
            text(6, 7, format!("Heard {heard}.")) ] })
    })
}

#[test]
fn a_title_in_a_baseline_row_wraps_and_keeps_its_key_beside_it() {
    let (app, mut window) = open_narrow(title_row(), 300.0);
    settle(&app, &mut window, shows("Heard 0."));
    // The key is clickable, so a click finds it: on the title's first line, at the frame's right, after the title has
    // wrapped. GPUI took the narrowest the title could be as its whole line, so the key sat past the frame's edge.
    assert!(pressed_at(&app, &mut window, (266.0, 27.0), "Heard 1."));
}

/// Issues' title as the native page draws it: a ghost button that wraps the large title, then the key, which can be
/// pressed here. The page hears the title's click and the F2 sent to it.
fn wrapped_title() -> String {
    let (mut edited, mut keyed, mut f2) = (0, 0, 0);
    fake::page("page", move |event| {
        match event {
            Some(("2:click", _)) => edited += 1,
            Some(("5:click", _)) => keyed += 1,
            Some((_, key)) if key["key"] == "F2" => f2 += 1,
            _ => {}
        }
        json!({ "id": 0, "type": "#root", "children": [
            { "id": 1, "type": "Row", "props": { "gap": "m", "align": "baseline" }, "children": [
                { "id": 2, "type": "Button", "props": { "variant": "ghost", "wrap": true, "label": "Edit the title", "onClick": "2:click", "onKeyDown": "2:key" }, "children": [
                    { "id": 3, "type": "Text", "props": { "size": "large", "weight": "strong" }, "children": [
                        text_node(4, "Backfill order totals for rows created before the currency column existed")] }] },
                { "id": 5, "type": "Text", "props": { "size": "small", "mono": true, "onClick": "5:click" }, "children": [text_node(6, "WEB-3")] }] },
            text(7, 8, format!("Edited {edited}, keyed {keyed}, F2 {f2}.")) ] })
    })
}

#[test]
fn a_wrap_button_is_as_tall_as_its_lines_keeps_the_key_beside_it_and_hears_f2() {
    let (app, mut window) = open_narrow(wrapped_title(), 300.0);
    settle(&app, &mut window, shows("Edited 0, keyed 0, F2 0."));
    // The title's third line, 26pt lines under the button's 3pt of border and padding, is the button: it was one 32pt
    // line that ran on past the frame, pushing the key out of it.
    assert!(pressed_at(&app, &mut window, (40.0, 82.0), "Edited 1,"));
    // The key, on the title's first line at the frame's right.
    assert!(pressed_at(&app, &mut window, (268.0, 30.0), "keyed 1,"));
    window.update(|_, window, cx| window.focus_next(cx));
    window.draw();
    key(&mut window, "f2");
    settle(&app, &mut window, shows("F2 1."));
    key(&mut window, "enter");
    settle(&app, &mut window, shows("Edited 2,"));
}

/// Issues' linked issue as the native page draws it: an outlined chip of a dot, the key and a title cut short, with an
/// × beside it.
fn linked_issue() -> String {
    let (mut opened, mut removed) = (0, 0);
    fake::page("page", move |event| {
        match event {
            Some(("8:click", _)) => removed += 1,
            Some(_) => opened += 1,
            None => {}
        }
        json!({ "id": 0, "type": "#root", "children": [
            { "id": 1, "type": "Row", "props": { "gap": "s" }, "children": [
                { "id": 2, "type": "Button", "props": { "variant": "outline", "size": "xs", "onClick": "2:click" }, "children": [
                    { "id": 3, "type": "Dot", "props": { "tone": "info" } },
                    { "id": 4, "type": "Text", "props": { "tone": "muted", "size": "tiny", "mono": true }, "children": [text_node(5, "WEB-8")] },
                    { "id": 6, "type": "Text", "props": { "grow": true, "truncate": true }, "children": [
                        text_node(7, "Order history landing page: tabs, design alignment, page and tab events")] }] },
                { "id": 8, "type": "Button", "props": { "variant": "ghost", "size": "icon-xs", "label": "Remove WEB-8", "onClick": "8:click" }, "children": [text_node(9, "×")] }] },
            text(10, 11, format!("Opened {opened}, removed {removed}.")) ] })
    })
}

#[test]
fn a_button_with_a_truncated_text_narrows_to_its_row_and_leaves_room_for_the_rest() {
    let (app, mut window) = open_narrow(linked_issue(), 300.0);
    settle(&app, &mut window, shows("Opened 0, removed 0."));
    // The × is the row's last 24pt, inside the page's 14pt of padding: the chip kept the whole title's width and ran past
    // the frame's edge, taking the × with it.
    assert!(pressed_at(&app, &mut window, (274.0, 26.0), "removed 1."));
    assert!(pressed_at(&app, &mut window, (40.0, 26.0), "Opened 1,"));
}

#[test]
fn a_baseline_row_starts_a_smaller_text_lower_so_their_baselines_meet() {
    let row = |texts: &str| {
        let tree = Tree::parse(&format!(
            r##"{{"id":0,"type":"#root","children":[{{"id":1,"type":"Row","props":{{"align":"baseline"}},"children":[{texts}]}}]}}"##
        ))
        .unwrap();
        let Node::Row(row) = &tree.children[0] else {
            panic!("a row")
        };
        row.clone()
    };
    let title = r##"{"id":2,"type":"Text","props":{"size":"large","weight":"strong"},"children":[{"id":3,"type":"#text","text":"Checkout summary card"}]}"##;
    let key = r##"{"id":4,"type":"Text","props":{"size":"small","mono":true},"children":[{"id":5,"type":"#text","text":"WEB-4"}]}"##;
    let mut app = TestApp::new();
    app.update(|cx| {
        // The key is set in the middle of a line as tall as the title's, so its baseline was higher by the difference
        // of their half ascent-less-descent; it starts that much lower now, and the title where it was.
        let drops = baseline_drops(&row(&format!("{title},{key}")), cx).unwrap();
        assert_eq!(drops[0], 0.0);
        assert!(drops[1] > 1.0, "{drops:?}");
        // Texts of one size stay level, and a row that isn't a baseline row of texts is left as it is.
        assert_eq!(
            baseline_drops(&row(&format!("{key},{key}")), cx),
            Some(vec![0.0, 0.0])
        );
        let mut center = row(&format!("{title},{key}"));
        center.align = crate::page::tree::Align::Center;
        assert_eq!(baseline_drops(&center, cx), None);
        // A title that is a `wrap` button lines up as its text, which is 3pt into it, under its border and padding: the
        // key beside it starts that much lower again.
        let button = format!(
            r##"{{"id":8,"type":"Button","props":{{"wrap":true,"variant":"ghost"}},"children":[{title}]}}"##
        );
        let wrapped = baseline_drops(&row(&format!("{button},{key}")), cx).unwrap();
        assert_eq!(wrapped[0], 0.0);
        assert!((wrapped[1] - drops[1] - 3.0).abs() < 0.01, "{wrapped:?}");
        // A log's folded group: its number beside a list button that holds its title, 3pt into it as well. The row
        // wasn't one of texts, so the number sat 3pt over the title's baseline.
        let group = format!(
            r##"{key},{{"id":9,"type":"ListButton","props":{{"aria-expanded":false}},"children":[{key}]}}"##
        );
        assert_eq!(
            baseline_drops(&row(&group), cx),
            Some(vec![3.0, 0.0])
        );
    });
}

/// Forty buttons in the start pane of a split on a page that fills its frame, Disk usage's list of files.
fn split_list() -> String {
    let mut hit = -1;
    fake::page("page", move |event| {
        if let Some((handler, _)) = event {
            hit = number(handler) as i32 - 10;
        }
        let rows: Vec<Value> = (0..40)
            .map(|i| json!({ "id": 10 + i, "type": "Button", "props": { "onClick": format!("{}:click", 10 + i) }, "children": [text_node(100 + i, &format!("Row {i}"))] }))
            .collect();
        json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [{ "id": 1, "type": "Split", "props": { "defaultSize": 200 }, "children": [
            { "id": 2, "type": "Slot", "props": { "name": "start" }, "children": [{ "id": 3, "type": "Stack", "props": { "grow": true }, "children": rows }] },
            { "id": 4, "type": "Slot", "props": { "name": "end" }, "children": [text(5, 6, format!("Hit {hit}"))] }] }] })
    })
}

/// As the desktop's split panes do, a pane taller than the frame scrolls: the wheel brings later rows up to click.
#[test]
fn a_split_pane_longer_than_its_frame_scrolls() {
    let mut app = TestApp::new();
    app.background_executor().allow_parking();
    let host = hosted(&mut app);
    let source = split_list();
    let bounds = gpui::Bounds::new(point(px(0.0), px(0.0)), gpui::size(px(500.0), px(300.0)));
    let mut window = app.open_window_with_options(
        gpui::WindowOptions {
            window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
        |_, cx| PageView::in_plugin(&host, &source, cx),
    );
    settle(&app, &mut window, shows("Hit -1"));
    window.simulate_event(gpui::ScrollEvent {
        position: point(px(60.0), px(150.0)),
        delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(-2000.0))),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
        source: Default::default(),
    });
    app.run_until_parked();
    window.draw();
    window.simulate_click(point(px(60.0), px(280.0)), MouseButton::Left);
    thread::sleep(Duration::from_millis(100));
    app.run_until_parked();
    window.draw();
    settle(&app, &mut window, shows("Hit 39"));
}

/// A chat's scroll: 60 rows, the newest last, and once the page has heard from the app, 30 older ones above them.
/// `onReachStart` is there while there is anything older to add.
fn older_rows(heard: u32) -> Value {
    let loaded = heard > 0;
    let mut rows: Vec<Value> = (0..60).map(|i| text(100 + i, 1_000 + i, format!("Row {i}"))).collect();
    if loaded {
        let older = (0..30).map(|i| text(200 + i, 2_000 + i, format!("Older {i}")));
        rows.splice(0..0, older);
    }
    let props = if loaded { json!({ "follow": "end" }) } else { json!({ "follow": "end", "onReachStart": "1:onReachStart" }) };
    json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [
        { "id": 1, "type": "Scroll", "props": props, "children": [{ "id": 2, "type": "Stack", "props": {}, "children": rows }] }] })
}

/// Scrolling to the top of a following `Scroll` asks the page for older items, and when they arrive above, the view
/// stays on what it showed: it doesn't jump to the top of the older items.
#[test]
fn a_following_scroll_asks_for_older_items_at_its_top_and_stays_on_what_it_showed() {
    let mut app = TestApp::new();
    app.background_executor().allow_parking();
    let host = hosted(&mut app);
    let source = counting(older_rows);
    let bounds = gpui::Bounds::new(point(px(0.0), px(0.0)), gpui::size(px(500.0), px(300.0)));
    let mut window = app.open_window_with_options(
        gpui::WindowOptions { window_bounds: Some(gpui::WindowBounds::Windowed(bounds)), ..Default::default() },
        |_, cx| PageView::in_plugin(&host, &source, cx),
    );
    // It starts at its end, as a chat does.
    settle(&app, &mut window, |view| view.follow_scrolled(1).is_some_and(|(top, max)| max > 300.0 && top >= max - 24.0));
    window.simulate_event(gpui::ScrollEvent {
        position: point(px(60.0), px(150.0)),
        delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(100_000.0))),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
        source: Default::default(),
    });
    // The 30 older rows are at least 300 points, so a view left at the top would show them and one kept in place is
    // further down.
    settle(&app, &mut window, |view| view.text().contains("Older 0") && view.follow_scrolled(1).is_some_and(|(top, _)| top >= 300.0));
}

/// A chat too long to send whole: 300 rows, of which the page sends 60, ending at the newest. `onReachStart` moves them 30
/// rows older and `onReachEnd` 30 newer, and each is there while there is more that way.
fn sliding_rows() -> String {
    let mut end = 300;
    fake::page("page", move |event| {
        match event {
            Some(("1:onReachStart", _)) => end -= 30,
            Some(("1:onReachEnd", _)) => end += 30,
            _ => {}
        }
        let rows: Vec<Value> = (end - 60..end).map(|i| text(100 + i, 1_000 + i, format!("Row {i}"))).collect();
        let mut props = json!({ "follow": "end" });
        if end > 60 {
            props["onReachStart"] = json!("1:onReachStart");
        }
        if end < 300 {
            props["onReachEnd"] = json!("1:onReachEnd");
        }
        json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [
            { "id": 1, "type": "Scroll", "props": props, "children": [{ "id": 2, "type": "Stack", "props": {}, "children": rows }] }] })
    })
}

/// A following `Scroll` over part of a long list moves along it both ways: at its top the page sends older rows and
/// drops the newest, at its end newer ones and drops the oldest, and each time the view stays on what it showed instead
/// of jumping to the end.
#[test]
fn a_following_scroll_moves_along_a_list_too_long_to_send_whole_and_stays_on_what_it_showed() {
    let mut app = TestApp::new();
    app.background_executor().allow_parking();
    let host = hosted(&mut app);
    let source = sliding_rows();
    let bounds = gpui::Bounds::new(point(px(0.0), px(0.0)), gpui::size(px(500.0), px(300.0)));
    let mut window = app.open_window_with_options(
        gpui::WindowOptions { window_bounds: Some(gpui::WindowBounds::Windowed(bounds)), ..Default::default() },
        |_, cx| PageView::in_plugin(&host, &source, cx),
    );
    settle(&app, &mut window, |view| view.text().contains("Row 299") && view.follow_scrolled(1).is_some_and(|(top, max)| max > 300.0 && top >= max - 24.0));
    let scroll = |window: &mut View, by: f32| {
        window.simulate_event(gpui::ScrollEvent {
            position: point(px(60.0), px(150.0)),
            delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(by))),
            modifiers: gpui::Modifiers::default(),
            touch_phase: gpui::TouchPhase::Moved,
            source: Default::default(),
        })
    };
    // Up to the top: rows 210 to 269 come, and the 30 older rows are above the view, which stays where it was.
    scroll(&mut window, 100_000.0);
    settle(&app, &mut window, |view| {
        let text = view.text();
        text.contains("Row 210") && !text.contains("Row 299") && view.follow_scrolled(1).is_some_and(|(top, _)| top >= 300.0)
    });
    // Down to the end: rows 240 to 299 again, and the view stays on the rows it showed rather than follow to the newest.
    scroll(&mut window, -100_000.0);
    settle(&app, &mut window, |view| {
        let text = view.text();
        text.contains("Row 299") && !text.contains("Row 210") && view.follow_scrolled(1).is_some_and(|(top, max)| top < max - 200.0)
    });
    // It stays there, and only scrolling on to the end follows it again.
    for _ in 0..5 {
        app.run_until_parked();
        window.draw();
    }
    assert!(window.read(|view, _| view.follow_scrolled(1).is_some_and(|(top, max)| top < max - 200.0)));
    scroll(&mut window, -100_000.0);
    settle(&app, &mut window, |view| view.follow_scrolled(1).is_some_and(|(top, max)| top >= max - 24.0));
}

/// A `Stack` with `fill` holding a following `Scroll` of 60 rows, over a line at the foot of the page, as a chat lays its
/// conversation over its message box.
fn filled_rows(fill: bool) -> String {
    fake::page("page", move |_| {
        let rows: Vec<Value> = (0..60).map(|i| text(100 + i, 1_000 + i, format!("Row {i}"))).collect();
        json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [
            { "id": 3, "type": "Stack", "props": { "fill": fill }, "children": [
                { "id": 1, "type": "Scroll", "props": { "follow": "end" }, "children": [{ "id": 2, "type": "Stack", "props": {}, "children": rows }] }] },
            text(50, 51, "Foot".into())] })
    })
}

/// A `fill` stack is as tall as its page leaves it, so the scroll in it scrolls and what follows stays in view; one
/// without `fill` is as tall as the rows, which push the foot out.
#[test]
fn a_filling_stack_keeps_its_scroll_within_the_page() {
    for fill in [true, false] {
        let mut app = TestApp::new();
        app.background_executor().allow_parking();
        let host = hosted(&mut app);
        let source = filled_rows(fill);
        let bounds = gpui::Bounds::new(point(px(0.0), px(0.0)), gpui::size(px(500.0), px(300.0)));
        let mut window = app.open_window_with_options(
            gpui::WindowOptions { window_bounds: Some(gpui::WindowBounds::Windowed(bounds)), ..Default::default() },
            |_, cx| PageView::in_plugin(&host, &source, cx),
        );
        settle(&app, &mut window, shows("Foot"));
        for _ in 0..3 {
            app.run_until_parked();
            window.draw();
        }
        let scrolls = window.read(|view, _| view.follow_scrolled(1).is_some_and(|(_, max)| max > 300.0));
        assert_eq!(scrolls, fill, "fill: {fill}");
    }
}

/// A column of 60 rows in a plain `Scroll` that is asked to scroll to row 40 and to track rows 10, 20, 30 and 40, over a
/// line that says what `onTrack` last said.
fn aimed_rows() -> String {
    let mut said = "nothing".to_owned();
    fake::page("page", move |event| {
        if let Some(("2:onTrack", payload)) = event {
            said = payload["at"].to_string();
        }
        let rows: Vec<Value> = (0..60).map(|i| text(100 + i, 1_000 + i, format!("Row {i}"))).collect();
        let props = json!({ "scrollTo": { "index": 40, "token": 1 }, "track": [10, 20, 30, 40], "onTrack": "2:onTrack" });
        json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [
            text(50, 51, format!("Tracked {said}.")),
            { "id": 2, "type": "Scroll", "props": props, "children": [{ "id": 3, "type": "Stack", "props": {}, "children": rows }] }] })
    })
}

/// A `Scroll` told to scroll to a child puts it at the top of the view once, and says which tracked child that is.
#[test]
fn a_scroll_goes_to_the_child_it_is_told_to_and_says_which_tracked_child_it_is_at() {
    let mut app = TestApp::new();
    app.background_executor().allow_parking();
    let host = hosted(&mut app);
    let source = aimed_rows();
    let bounds = gpui::Bounds::new(point(px(0.0), px(0.0)), gpui::size(px(500.0), px(300.0)));
    let mut window = app.open_window_with_options(
        gpui::WindowOptions { window_bounds: Some(gpui::WindowBounds::Windowed(bounds)), ..Default::default() },
        |_, cx| PageView::in_plugin(&host, &source, cx),
    );
    // Row 40 is at the top, so the last tracked row at or above the top is the fourth, and the view is well down the column.
    settle(&app, &mut window, |view| view.text().contains("Tracked 3.") && view.follow_scrolled(2).is_some_and(|(top, max)| top > 300.0 && top <= max));
    // It stays there: the same token is not acted on again.
    for _ in 0..5 {
        app.run_until_parked();
        window.draw();
    }
    assert!(window.read(|view, _| view.follow_scrolled(2).is_some_and(|(top, _)| top > 300.0) && view.text().contains("Tracked 3.")));
    // Scrolled back to the top, the view is above every tracked row.
    window.simulate_event(gpui::ScrollEvent {
        position: point(px(60.0), px(150.0)),
        delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(100_000.0))),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
        source: Default::default(),
    });
    settle(&app, &mut window, |view| view.text().contains("Tracked null."));
}

/// The rows of `aimed_rows`, tracking one near the end that a scroll to the end shows below the top of the view.
fn end_rows() -> String {
    let mut said = "nothing".to_owned();
    fake::page("page", move |event| {
        if let Some(("2:onTrack", payload)) = event {
            said = payload["at"].to_string();
        }
        let rows: Vec<Value> = (0..60).map(|i| text(100 + i, 1_000 + i, format!("Row {i}"))).collect();
        let props = json!({ "track": [10, 20, 57], "onTrack": "2:onTrack" });
        json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [
            text(50, 51, format!("Tracked {said}.")),
            { "id": 2, "type": "Scroll", "props": props, "children": [{ "id": 3, "type": "Stack", "props": {}, "children": rows }] }] })
    })
}

/// Scrolled to its end, a `Scroll` is at its last tracked child, though that child's top never reaches the view's top.
#[test]
fn at_the_end_of_a_scroll_its_last_tracked_child_in_view_is_the_one_it_is_at() {
    let mut app = TestApp::new();
    app.background_executor().allow_parking();
    let host = hosted(&mut app);
    let source = end_rows();
    let bounds = gpui::Bounds::new(point(px(0.0), px(0.0)), gpui::size(px(500.0), px(300.0)));
    let mut window = app.open_window_with_options(
        gpui::WindowOptions { window_bounds: Some(gpui::WindowBounds::Windowed(bounds)), ..Default::default() },
        |_, cx| PageView::in_plugin(&host, &source, cx),
    );
    settle(&app, &mut window, |view| view.text().contains("Tracked null."));
    window.simulate_event(gpui::ScrollEvent {
        position: point(px(60.0), px(150.0)),
        delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(-100_000.0))),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
        source: Default::default(),
    });
    settle(&app, &mut window, |view| view.text().contains("Tracked 2."));
}

/// A chat's shape: a main pane and a pane on the right that folds away below 600 points, or stays when pinned.
fn side_split(pinned: bool) -> String {
    fake::page("page", move |_| {
        json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [{ "id": 1, "type": "Split",
            "props": { "defaultSize": 200, "side": "end", "collapseBelow": 600, "pinned": pinned }, "children": [
            { "id": 2, "type": "Slot", "props": { "name": "start" }, "children": [text(3, 4, "Main".into())] },
            { "id": 5, "type": "Slot", "props": { "name": "end" }, "children": [text(6, 7, "Side".into())] }] }] })
    })
}

/// A chat's floating panel: the split with `float`, which tells the page when it is narrower than 600 points and, while
/// the page gives it an `onClose`, hears Escape in the panel.
fn float_split(with_close: bool) -> String {
    let (mut narrow, mut closed) = ("unknown".to_owned(), 0);
    fake::page("page", move |event| {
        match event {
            Some(("5:narrow", payload)) => narrow = payload["narrow"].to_string(),
            Some(("5:close", _)) => closed += 1,
            _ => {}
        }
        let mut props = json!({ "defaultSize": 200, "side": "end", "float": true, "collapseBelow": 600, "onNarrow": "5:narrow" });
        if with_close && closed == 0 {
            props["onClose"] = json!("5:close");
        }
        json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [{ "id": 1, "type": "Split",
            "props": props, "children": [
            { "id": 2, "type": "Slot", "props": { "name": "start" }, "children": [text(3, 4, format!("Main, narrow {narrow}, closed {closed}."))] },
            { "id": 6, "type": "Slot", "props": { "name": "end" }, "children": [text(7, 8, "Side".into())] }] }] })
    })
}

/// A floating split keeps its pane at any width, takes no width from the main pane, and tells the page which side of
/// `collapseBelow` it is on.
#[test]
fn a_floating_split_reports_its_width_and_keeps_its_pane() {
    for (width, narrow) in [(500.0, "narrow true,"), (900.0, "narrow false,")] {
        let mut app = TestApp::new();
        app.background_executor().allow_parking();
        let host = hosted(&mut app);
        let source = float_split(false);
        let bounds = gpui::Bounds::new(point(px(0.0), px(0.0)), gpui::size(px(width), px(300.0)));
        let mut window = app.open_window_with_options(
            gpui::WindowOptions { window_bounds: Some(gpui::WindowBounds::Windowed(bounds)), ..Default::default() },
            |_, cx| PageView::in_plugin(&host, &source, cx),
        );
        settle(&app, &mut window, shows(narrow));
        assert!(window.read(|view, _| view.text().contains("Side")));
    }
}

/// How long a click that swaps a long conversation for another window of it, scrolled to its first message, takes to
/// draw: as a click on a far prompt on Claude chat's rail does. Run with `--ignored --nocapture`.
#[test]
#[ignore]
fn measure_a_jump_to_a_far_window_of_a_long_conversation() {
    let source = counting(|heard| {
        // 600 messages of a few lines, about 1,800 nodes, as the chat's window holds.
        let base = if heard == 0 { 1_000 } else { 100_000 };
        let items: Vec<Value> = (0..600)
            .map(|i| {
                let id = base + 3 * i;
                json!({ "id": id, "type": "Stack", "children": [text(id + 1, id + 2, format!("Message {} of window {heard}. {}", i, "Some words of a reply that wraps over a line or two. ".repeat(4)))] })
            })
            .collect();
        let mut scroll = json!({ "id": 2, "type": "Scroll", "props": { "follow": "end" }, "children": items });
        if heard > 0 {
            scroll["props"]["scrollTo"] = json!({ "index": 0, "token": heard });
        }
        json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [
            { "id": 1, "type": "Button", "props": { "onClick": "1:onClick" }, "children": [text_node(9, "Jump")] }, scroll] })
    });
    let (app, mut window) = open(source);
    settle(&app, &mut window, shows("window 0."));
    let clicked = Instant::now();
    window.simulate_click(point(px(30.0), px(16.0)), MouseButton::Left);
    settle(&app, &mut window, shows("window 1."));
    let drawn = clicked.elapsed();
    let frames = Instant::now();
    window.draw();
    window.draw();
    eprintln!("[measure] click to the new window drawn: {drawn:?}; two frames after: {:?}", frames.elapsed());
}

/// An expandable text has More only while it is cut short, as Claude chat's queued messages: "skill" fit its line and still
/// had More after it, because the page guessed from its length.
#[test]
fn only_a_text_cut_short_offers_more() {
    let line = |id: usize, said: &str| json!({ "id": id, "type": "Row", "children": [
        { "id": id + 1, "type": "Text", "props": { "truncate": true, "expandable": true, "grow": true }, "children": [text_node(id + 2, said)] }] });
    let source = fake::page("page", move |_| {
        json!({ "id": 0, "type": "#root", "children": [{ "id": 1, "type": "Stack", "children": [
            line(10, "skill"),
            line(20, &"i thought that this is given when you're doing a figma design diff ".repeat(4))] }] })
    });
    let (app, mut window) = open_narrow(source, 500.0);
    settle(&app, &mut window, shows("skill"));
    window.draw();
    window.draw();
    assert_eq!(window.read(|view, _| (view.text_cut(11), view.text_cut(21))), ((Some(false), false), (Some(true), false)));
    // More sits at the long line's right end, under the short one, inside the page's padding: it opens the text, and Less closes it.
    click(&mut window, (440.0, 48.0));
    window.draw();
    assert_eq!(window.read(|view, _| view.text_cut(21)), (Some(true), true));
}

/// A floating pane taller than its page scrolls under the pointer, and its last row can then be pressed: the chat's side
/// panel with a mod's long Feedback pane wouldn't scroll at all.
#[test]
fn a_floating_pane_taller_than_its_page_scrolls() {
    let source = fake::page("page", {
        let mut pressed = "none".to_owned();
        move |event| {
            if let Some((handler, _)) = event {
                pressed = handler.trim_end_matches(":onClick").to_owned();
            }
            let rows: Vec<Value> = (0..40)
                .map(|i| json!({ "id": 100 + 2 * i, "type": "Button", "props": { "onClick": format!("{}:onClick", 100 + 2 * i) },
                    "children": [text_node(101 + 2 * i, &format!("Row {i}"))] }))
                .collect();
            json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [{ "id": 1, "type": "Split",
                "props": { "defaultSize": 240, "side": "end", "float": true }, "children": [
                { "id": 2, "type": "Slot", "props": { "name": "start" }, "children": [text(3, 4, format!("Pressed {pressed}."))] },
                { "id": 5, "type": "Slot", "props": { "name": "end" }, "children": [{ "id": 6, "type": "Stack", "children": rows }] }] }] })
        }
    });
    let (app, mut window) = open_narrow(source, 900.0);
    settle(&app, &mut window, shows("Pressed none."));
    window.simulate_scroll(point(px(780.0), px(200.0)), point(px(0.0), px(-10_000.0)));
    window.draw();
    // The last row's button, the 40th, is id 178.
    assert!(pressed_at(&app, &mut window, (780.0, 370.0), "Pressed 178."), "{}", window.read(|view, _| view.text()));
}

/// A floating pane with `onClose` has the keyboard, and Escape in it sends `onClose`.
#[test]
fn escape_in_a_floating_pane_sends_its_on_close() {
    let (app, mut window) = open(float_split(true));
    settle(&app, &mut window, shows("Side"));
    window.draw();
    key(&mut window, "escape");
    settle(&app, &mut window, shows("closed 1."));
}

/// A page with a button that opens a dialog, which the page leaves out once it hears `onClose`, and how often it has.
fn dialog_page() -> String {
    let (mut open, mut closed) = (false, 0);
    fake::page("page", move |event| {
        match event {
            Some(("3:onClick", _)) => open = true,
            Some(("6:onClose", _)) => {
                open = false;
                closed += 1;
            }
            _ => {}
        }
        let mut body = vec![
            json!({ "id": 3, "type": "Button", "props": { "onClick": "3:onClick" }, "children": [text_node(4, "Open")] }),
            text(10, 11, format!("Closed {closed}.")),
        ];
        if open {
            body.push(json!({ "id": 6, "type": "Dialog", "props": { "title": "Plugins", "width": 360, "onClose": "6:onClose" },
                "children": [text(7, 8, "Inside the dialog".into())] }));
        }
        json!({ "id": 0, "type": "#root", "children": [{ "id": 1, "type": "Page", "children": [
            { "id": 2, "type": "Slot", "props": { "name": "body" }, "children": body }] }] })
    })
}

/// Inside the dialog page's Open button: under the shell's and the body's 14pt of padding each.
const OPEN_BUTTON: (f32, f32) = (40.0, 40.0);

/// A dialog is drawn over the middle of the page, wherever the page put it, and Escape or a press beside it close it.
#[test]
fn a_dialog_is_drawn_over_the_page_and_escape_or_the_backdrop_sends_on_close() {
    let (app, mut window) = open(dialog_page());
    settle(&app, &mut window, shows("Closed 0."));
    click(&mut window, OPEN_BUTTON);
    settle(&app, &mut window, shows("Inside the dialog"));
    window.draw();
    let panel = crate::ui::spot_at("page-dialog").expect("the dialog is drawn");
    let page = window.update(|_, window, _| window.viewport_size());
    // In the middle of the page, as wide as the page asked (measured inside its 1-point border), and not in the body's
    // flow under the button.
    assert!((f32::from(panel.size.width) - 358.0).abs() < 1.0, "{panel:?}");
    assert!((f32::from(panel.center().x) - f32::from(page.width) / 2.0).abs() < 1.0, "{panel:?} in {page:?}");
    assert!((f32::from(panel.center().y) - f32::from(page.height) / 2.0).abs() < 1.0, "{panel:?} in {page:?}");

    key(&mut window, "escape");
    settle(&app, &mut window, shows("Closed 1."));
    assert!(window.read(|view, _| !view.text().contains("Inside the dialog")));

    click(&mut window, OPEN_BUTTON);
    settle(&app, &mut window, shows("Inside the dialog"));
    // The backdrop, beside the panel: the button under it doesn't hear the press.
    click(&mut window, OPEN_BUTTON);
    settle(&app, &mut window, shows("Closed 2."));
}

/// A dialog that holds only a select, as Claude chat's Background in Settings: its menu hangs past the panel's foot.
fn dialog_with_select() -> String {
    let (mut picked, mut closed) = ("a".to_owned(), 0);
    fake::page("page", move |event| {
        match event {
            Some(("7:change", pick)) => picked = pick["value"].as_str().unwrap_or_default().to_owned(),
            Some(("6:onClose", _)) => closed += 1,
            _ => {}
        }
        let options: Vec<Value> =
            ["a", "b", "c", "d"].iter().map(|value| json!({ "value": value, "label": value.to_uppercase() })).collect();
        json!({ "id": 0, "type": "#root", "children": [{ "id": 1, "type": "Page", "children": [
            { "id": 2, "type": "Slot", "props": { "name": "body" }, "children": [
                text(3, 4, format!("Picked {picked}, closed {closed}.")),
                { "id": 6, "type": "Dialog", "props": { "title": "Background", "width": 300, "onClose": "6:onClose" },
                    "children": [{ "id": 7, "type": "Select", "props": { "label": "Background", "value": picked,
                        "onChange": "7:change", "options": options } }] }] }] }] })
    })
}

/// An option of a select's menu that hangs past its dialog's panel, over the backdrop, is picked by a press, and the
/// dialog stays: the press reached the backdrop too, which closed the dialog, as Settings closed on Halo.
#[test]
fn an_option_past_the_dialogs_panel_is_picked_and_the_dialog_stays() {
    let mut app = TestApp::new();
    app.background_executor().allow_parking();
    let host = hosted(&mut app);
    let source = dialog_with_select();
    let bounds = gpui::Bounds::new(point(px(0.0), px(0.0)), gpui::size(px(500.0), px(700.0)));
    let mut window = app.open_window_with_options(
        gpui::WindowOptions {
            window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
        move |_, cx| PageView::in_plugin(&host, &source, cx),
    );
    settle(&app, &mut window, shows("Picked a, closed 0."));
    let select = window.read(|view, _| view.select_box(7)).expect("the select is drawn");
    click(&mut window, center(select));
    settle(&app, &mut window, PageView::menu_open);
    window.draw();
    let panel = crate::ui::spot_at("page-dialog").expect("the dialog is drawn");
    // The third option: under the select's foot, 4pt of room, the menu's 1pt border and 4pt of padding, then 32pt rows.
    let (x, y) = (center(select).0, f32::from(select.bottom()) + 9.0 + 64.0 + 16.0);
    assert!(y > f32::from(panel.bottom()) + 16.0, "the option at {y} is past the panel {panel:?}");
    click(&mut window, (x, y));
    settle(&app, &mut window, shows("Picked c,"));
    assert!(window.read(|view, _| view.text().contains("Picked c, closed 0.")), "the dialog closed");
}

/// A page that opens a dialog by itself, on a timer, with nothing clicked or typed in it.
fn dialog_by_itself() -> String {
    struct Later;
    impl Later {
        fn draw(cx: &mut Cx<'_>, open: bool) {
            let mut body = vec![text(3, 4, "Waiting.".into())];
            if open {
                body.push(json!({ "id": 6, "type": "Dialog", "props": { "title": "Type here", "onClose": "6:onClose" },
                    "children": [{ "id": 7, "type": "TextInput", "props": { "label": "Secret", "autoFocus": true, "onChange": "7:change" } },
                        text(8, 9, "Inside the dialog".into())] }));
            }
            cx.render(&json!({ "id": 0, "type": "#root", "children": [{ "id": 1, "type": "Page", "children": [
                { "id": 2, "type": "Slot", "props": { "name": "body" }, "children": body }] }] }));
        }
    }
    impl Guest for Later {
        fn start(&mut self, cx: &mut Cx<'_>) {
            Self::draw(cx, false);
            cx.later(300);
        }
        fn event(&mut self, _: &mut Cx<'_>, _: &str, _: &Value) {}
        fn timer(&mut self, cx: &mut Cx<'_>, _: u32) {
            Self::draw(cx, true);
        }
    }
    fake::source("page", || Box::new(Later))
}

/// A page can't pull the keyboard away from what you type in elsewhere, like a terminal, by opening a dialog or a field
/// that asks for focus: it would read what you typed for that.
#[test]
fn a_dialog_a_page_opens_by_itself_leaves_the_keyboard_where_you_type() {
    let (app, mut window) = open(dialog_by_itself());
    settle(&app, &mut window, shows("Waiting."));
    let elsewhere = window.update(|_, window, cx| {
        let elsewhere = cx.focus_handle();
        window.focus(&elsewhere, cx);
        elsewhere
    });
    settle(&app, &mut window, shows("Inside the dialog"));
    window.draw();
    app.run_until_parked();
    assert!(window.update(|_, window, _| elsewhere.is_focused(window)), "the dialog took the keyboard");
}

/// A page that puts a new field in place of the one you type in, as a chat's message box is built again: the new one
/// asks for the keyboard after a moment.
fn field_swapped() -> String {
    struct Swap;
    impl Swap {
        fn draw(cx: &mut Cx<'_>, id: u64) {
            cx.render(&json!({ "id": 0, "type": "#root", "children": [{ "id": 1, "type": "Page", "children": [
                { "id": 2, "type": "Slot", "props": { "name": "body" }, "children": [
                    { "id": id, "type": "TextInput", "props": { "label": "Message", "autoFocus": true, "onChange": "change" } },
                    text(5, 6, format!("Field {id}.")) ] }] }] }));
        }
    }
    impl Guest for Swap {
        fn start(&mut self, cx: &mut Cx<'_>) {
            Self::draw(cx, 3);
            cx.later(200);
        }
        fn event(&mut self, _: &mut Cx<'_>, _: &str, _: &Value) {}
        fn timer(&mut self, cx: &mut Cx<'_>, _: u32) {
            Self::draw(cx, 13);
        }
    }
    fake::source("page", || Box::new(Swap))
}

/// A field the page puts in place of the one that had the keyboard takes it over: the keyboard was the page's.
#[test]
fn a_field_that_takes_the_place_of_the_focused_one_gets_the_keyboard() {
    let (app, mut window) = open(field_swapped());
    settle(&app, &mut window, shows("Field 3."));
    window.draw();
    settle(&app, &mut window, shows("Field 13."));
    window.draw();
    app.run_until_parked();
    let field = window.read(|view, cx| view.input_focus("Message", cx)).expect("the new field");
    assert!(window.update(|_, window, _| field.is_focused(window)), "the new field didn't get the keyboard");
}

/// The split measures its own width while it is drawn, and a window under `collapseBelow` leaves the right pane out
/// unless the page pins it.
#[test]
fn a_split_measures_its_width_and_folds_its_side_pane_when_narrow() {
    for (width, pinned, narrow) in [(500.0, false, true), (900.0, false, false), (500.0, true, true)] {
        let mut app = TestApp::new();
        app.background_executor().allow_parking();
        let host = hosted(&mut app);
        let source = side_split(pinned);
        let bounds = gpui::Bounds::new(point(px(0.0), px(0.0)), gpui::size(px(width), px(300.0)));
        let mut window = app.open_window_with_options(
            gpui::WindowOptions {
                window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| PageView::in_plugin(&host, &source, cx),
        );
        settle(&app, &mut window, |view| view.split_narrow(1) == Some(narrow));
    }
}

/// A page under a sheet that has a search in its header, until the sheet's `onClose` takes it away.
fn sheet() -> String {
    let (mut closed, mut query) = (0, String::new());
    fake::page("page", move |event| {
        match event {
            Some(("5:close", _)) => closed += 1,
            Some(("7:input", input)) => query = input["value"].as_str().unwrap_or_default().to_owned(),
            _ => {}
        }
        let sheet = json!({ "id": 3, "type": "Sheet", "props": { "onClose": "5:close" }, "children": [
            { "id": 4, "type": "Slot", "props": { "name": "title" }, "children": [text_node(40, "Past sessions")] },
            { "id": 6, "type": "Slot", "props": { "name": "header" }, "children": [
                { "id": 7, "type": "SearchField", "props": { "label": "Search", "placeholder": "Search", "onInput": "7:input" } }] },
            text(8, 9, "Fix the checkout".into())] });
        let body: Vec<Value> = std::iter::once(text(10, 11, format!("Closed {closed}, query {query}.")))
            .chain((closed == 0).then_some(sheet))
            .collect();
        json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [{ "id": 1, "type": "Page", "props": { "flush": true }, "children": [
            { "id": 2, "type": "Slot", "props": { "name": "body" }, "children": body }] }] })
    })
}

/// A sheet draws its title and what it lists, its search has the keyboard as it opens, and Escape there closes it.
#[test]
fn a_sheet_takes_the_keys_to_its_search_and_escape_sends_its_on_close() {
    let (app, mut window) = open(sheet());
    settle(&app, &mut window, shows("Past sessions"));
    assert!(window.read(|view, _| view.text().contains("Fix the checkout")));
    window.draw();
    window.simulate_input("checkout");
    settle(&app, &mut window, shows("query checkout."));
    key(&mut window, "escape");
    settle(&app, &mut window, shows("Closed 1,"));
    assert!(!window.read(|view, _| view.text().contains("Past sessions")));
}

/// A rail of ten prompts in a slim split, over a line that says which one was picked.
fn prompt_rail_page() -> String {
    let mut picked = "none".to_owned();
    fake::page("page", move |event| {
        if let Some(("2:onSelect", payload)) = event {
            picked = payload["index"].to_string();
        }
        let marks: Vec<Value> = (0..10).map(|n| json!({ "title": format!("Prompt {n}"), "preview": format!("Reply {n}") })).collect();
        json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [{ "id": 1, "type": "Split", "props": { "defaultSize": 64 }, "children": [
            { "id": 4, "type": "Slot", "props": { "name": "start" }, "children": [
                { "id": 2, "type": "PromptRail", "props": { "marks": marks, "current": 3, "onSelect": "2:onSelect" } }] },
            { "id": 5, "type": "Slot", "props": { "name": "end" }, "children": [text(6, 7, format!("Picked {picked}."))] }] }] })
    })
}

/// The middle of tick `at` of a rail of ten, a few points in from the rail's left.
fn rail_tick(bounds: gpui::Bounds<gpui::Pixels>, at: usize) -> gpui::Point<gpui::Pixels> {
    use super::tree::prompt_rail as rail;
    let pitch = rail::pitch(10, f32::from(bounds.size.height));
    point(bounds.origin.x + px(rail::PAD_X + 4.0), bounds.origin.y + px(rail::row_top(at, pitch) + pitch / 2.0))
}

/// Hovering a tick shows its card, which goes when the pointer leaves, and a click on a tick tells the page which prompt.
#[test]
fn a_prompt_rails_hover_shows_a_card_and_a_click_picks_the_prompt() {
    let (app, mut window) = open(prompt_rail_page());
    settle(&app, &mut window, |view| view.rail_bounds(2).is_some());
    let bounds = window.read(|view, _| view.rail_bounds(2).unwrap());
    // Nothing is hovered before the pointer is on a tick.
    assert_eq!(window.read(|view, _| view.rail_card(2)), None);
    window.simulate_mouse_move(rail_tick(bounds, 6));
    settle(&app, &mut window, |view| view.rail_card(2) == Some((6, "Prompt 6".to_owned())));
    // Another tick moves the card without the page hearing of it.
    window.simulate_mouse_move(rail_tick(bounds, 2));
    settle(&app, &mut window, |view| view.rail_card(2) == Some((2, "Prompt 2".to_owned())));
    assert!(!window.read(|view, _| view.acted()));
    window.simulate_click(rail_tick(bounds, 5), MouseButton::Left);
    settle(&app, &mut window, shows("Picked 5."));
    // Away from the rail, the card goes.
    window.simulate_mouse_move(point(px(400.0), px(200.0)));
    settle(&app, &mut window, |view| view.rail_card(2).is_none());
}

/// A page of an issue key that shows a card of the issue on hover, over a line elsewhere.
fn hover_card_page() -> String {
    fake::page("page", |_| {
        json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [
            { "id": 1, "type": "HoverCard", "props": { "width": 240 }, "children": [
                text(2, 3, "ABC-12".into()),
                { "id": 4, "type": "Slot", "props": { "name": "card" }, "children": [text(5, 6, "Fix the build".into())] }] },
            text(7, 8, "Elsewhere".into())] })
    })
}

#[test]
fn a_hover_card_shows_while_the_pointer_is_on_what_it_is_for_and_goes_once_it_leaves() {
    let (app, mut window) = open(hover_card_page());
    settle(&app, &mut window, shows("Elsewhere"));
    window.simulate_mouse_move(point(px(400.0), px(280.0)));
    settle(&app, &mut window, |view| !view.hover_card_shown(1));
    window.simulate_mouse_move(point(px(10.0), px(8.0)));
    settle(&app, &mut window, |view| view.hover_card_shown(1));
    // The page never hears of it.
    assert!(!window.read(|view, _| view.acted()));
    window.simulate_mouse_move(point(px(400.0), px(280.0)));
    settle(&app, &mut window, |view| !view.hover_card_shown(1));
}

/// A page of one `Code` node.
fn code_page(language: &'static str, code: &'static str) -> String {
    fake::page("page", move |_| {
        json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [
            { "id": 1, "type": "Code", "props": { "code": code, "language": language } }] })
    })
}

/// A `Code` node with a language is coloured by the app on a thread of its own, once, and the page draws it again when
/// the colours are ready; one in a language with no grammar is kept as having none.
#[test]
fn a_code_node_is_highlighted_off_the_window_thread_by_its_language() {
    let code = "let answer = 42; // the question\nfn f() {}";
    let (app, mut window) = open(code_page("rust", code));
    settle(&app, &mut window, |_| PageView::first_line_colours("rust", code).is_some());
    let colours = PageView::first_line_colours("rust", code).unwrap();
    assert!(colours.iter().collect::<std::collections::HashSet<_>>().len() > 1, "{colours:?}");

    let plain = "let answer = 42;";
    let (app, mut window) = open(code_page("no-such-language", plain));
    settle(&app, &mut window, |_| PageView::first_line_colours("no-such-language", plain).is_some());
    assert_eq!(PageView::first_line_colours("no-such-language", plain), Some(Vec::new()));
}

/// Dragging over a `Code` block selects its text, across its lines, as it does a paragraph's.
#[test]
fn dragging_over_a_code_block_selects_its_text() {
    let (app, mut window) = open(code_page("no-such-language", "let a = 1;\nlet b = 2;"));
    settle(&app, &mut window, |view| view.text().contains("let b"));
    drag(&mut window, point(px(20.0), px(14.0)), point(px(300.0), px(30.0)));
    let selected = window.read(|view, _| view.selected_text()).unwrap_or_default();
    assert!(selected.ends_with("1;\nlet b = 2;"), "{selected:?}");
}

/// A copy of a code block's lines keeps their indent, which the block draws as a margin so a wrapped line stays under it.
#[test]
fn a_copy_of_code_lines_keeps_their_indent() {
    let (app, mut window) = open(code_page("no-such-language", "fn a() {\n    let x = 1;\n}"));
    settle(&app, &mut window, |view| view.text().contains("let x"));
    drag(&mut window, point(px(20.0), px(14.0)), point(px(300.0), px(30.0)));
    let selected = window.read(|view, _| view.selected_text()).unwrap_or_default();
    assert!(selected.ends_with("\n    let x = 1;"), "{selected:?}");
}

/// A stack with `onContextMenu` of two 40 point blocks and a `Code`, over a line that says what the page heard.
fn context_menu_page() -> String {
    let mut heard = "nothing".to_owned();
    fake::page("page", move |event| {
        if let Some(("1:onContextMenu", payload)) = event {
            heard = format!(
                "x={} y={} child={} code={}",
                payload["x"].as_f64().unwrap(),
                payload["y"].as_f64().unwrap(),
                payload["child"],
                payload["code"]["text"].as_str().map_or("none".to_owned(), |text| format!("{}:{}", text.lines().next().unwrap(), payload["code"]["isBlock"])),
            );
        }
        json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [
            { "id": 1, "type": "Stack", "props": { "onContextMenu": "1:onContextMenu" }, "children": [
                { "id": 2, "type": "Skeleton", "props": { "height": 40 } },
                { "id": 3, "type": "Skeleton", "props": { "height": 40 } },
                { "id": 4, "type": "Code", "props": { "code": "let a = 1;\nlet b = 2;\nlet c = 3;" } }] },
            text(5, 6, format!("Heard {heard}."))] })
    })
}

/// The right button pressed and let go at `at`.
fn right_click(window: &mut View, at: gpui::Point<gpui::Pixels>) {
    window.simulate_mouse_move(at);
    window.simulate_event(gpui::MouseDownEvent { button: MouseButton::Right, position: at, modifiers: Default::default(), click_count: 1, first_mouse: false });
    window.simulate_event(gpui::MouseUpEvent { button: MouseButton::Right, position: at, modifiers: Default::default(), click_count: 1 });
}

/// A right press in a stack with `onContextMenu` tells the page where it was and which child was under it, and a `Code`
/// under it adds its text.
#[test]
fn a_right_click_in_a_stack_with_on_context_menu_tells_the_page_the_child_and_what_it_hit() {
    let (app, mut window) = open(context_menu_page());
    settle(&app, &mut window, shows("Heard nothing."));
    // A left click is not one.
    window.simulate_click(point(px(10.0), px(60.0)), MouseButton::Left);
    nothing_happens(&app);
    assert!(!window.read(|view, _| view.text().contains("x=")));
    // The second block.
    right_click(&mut window, point(px(10.0), px(60.0)));
    settle(&app, &mut window, shows("Heard x=10 y=60 child=1 code=none."));
    // The code block, the third child: its whole text, as a block.
    right_click(&mut window, point(px(20.0), px(120.0)));
    settle(&app, &mut window, shows("Heard x=20 y=120 child=2 code=let a = 1;:true."));
}

/// A line with a link in it, in a stack with `onContextMenu`, over a line that says what the page heard.
fn link_menu_page() -> String {
    let mut heard = "nothing".to_owned();
    fake::page("page", move |event| {
        if let Some(("1:onContextMenu", payload)) = event {
            heard = format!("link={} text={}", payload["link"]["href"].as_str().unwrap_or("none"), payload["link"]["text"].as_str().unwrap_or("none"));
        }
        json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [
            { "id": 1, "type": "Stack", "props": { "onContextMenu": "1:onContextMenu" }, "children": [
                { "id": 2, "type": "Text", "children": [
                    { "id": 3, "type": "Text", "props": { "href": "https://example.com/docs", "onClick": "3:onClick" }, "children": [text_node(4, "the docs")] },
                    text_node(5, " and more words after it")] }] },
            text(6, 7, format!("Heard {heard}."))] })
    })
}

/// A right-click on a link in a line tells the page its address and its words; one beside it doesn't.
#[test]
fn a_right_click_on_a_link_tells_the_page_its_address() {
    let (app, mut window) = open(link_menu_page());
    settle(&app, &mut window, shows("Heard nothing."));
    right_click(&mut window, point(px(20.0), px(8.0)));
    settle(&app, &mut window, shows("Heard link=https://example.com/docs text=the docs."));
    right_click(&mut window, point(px(160.0), px(8.0)));
    settle(&app, &mut window, shows("Heard link=none text=none."));
}

/// A `PopupMenu` over the page with two items and a close handler.
fn popup_menu_page() -> String {
    let mut heard = "nothing".to_owned();
    fake::page("page", move |event| {
        if let Some((handler, _)) = event {
            heard = handler.to_owned();
        }
        json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [
            text(1, 2, format!("Heard {heard}.")),
            { "id": 3, "type": "PopupMenu", "props": { "x": 100, "y": 100, "onClose": "3:onClose" }, "children": [
                { "id": 4, "type": "Button", "props": { "onClick": "4:onCopy", "description": "Puts it on the clipboard" }, "children": [text_node(5, "Copy link")] },
                { "id": 6, "type": "Button", "props": { "onClick": "6:onOpen" }, "children": [text_node(7, "Open")] }] }] })
    })
}

/// A `PopupMenu` draws its items, a click on one sends its `onClick`, and Escape sends `onClose`.
#[test]
fn a_popup_menu_draws_its_items_sends_a_click_on_one_and_hears_escape() {
    let (app, mut window) = open(popup_menu_page());
    settle(&app, &mut window, shows("Copy link"));
    assert!(window.read(|view, _| view.text().contains("Open")));
    // Inside the first item, under the menu's corner at (100, 100).
    window.simulate_click(point(px(130.0), px(118.0)), MouseButton::Left);
    settle(&app, &mut window, shows("Heard 4:onCopy."));
    key(&mut window, "escape");
    settle(&app, &mut window, shows("Heard 3:onClose."));
}

/// A press outside a `PopupMenu` sends `onClose`.
#[test]
fn a_press_outside_a_popup_menu_sends_its_on_close() {
    let (app, mut window) = open(popup_menu_page());
    settle(&app, &mut window, shows("Copy link"));
    window.simulate_click(point(px(600.0), px(400.0)), MouseButton::Left);
    settle(&app, &mut window, shows("Heard 3:onClose."));
}

/// A line of words in a stack with `onContextMenu`, over a line that says what the page heard: which handler, and the
/// selection the right-click carried.
fn selection_page() -> String {
    let mut heard = "nothing".to_owned();
    fake::page("page", move |event| {
        if let Some((handler, payload)) = event {
            heard = format!("{handler} selection={}", payload["selection"]["text"].as_str().unwrap_or("none"));
        }
        json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [
            { "id": 1, "type": "Stack", "props": { "onContextMenu": "1:onContextMenu" }, "children": [
                { "id": 2, "type": "Text", "children": [
                    { "id": 3, "type": "Text", "props": { "onClick": "3:onClick" }, "children": [text_node(4, "the docs")] },
                    text_node(5, " and more words after it")] },
                text(6, 7, "alpha beta gamma delta".into())] },
            text(8, 9, format!("Heard {heard}."))] })
    })
}

/// The left button pressed at `from`, dragged to `to` and let go.
fn drag(window: &mut View, from: gpui::Point<gpui::Pixels>, to: gpui::Point<gpui::Pixels>) {
    window.simulate_mouse_move(from);
    window.simulate_event(gpui::MouseDownEvent { button: MouseButton::Left, position: from, modifiers: Default::default(), click_count: 1, first_mouse: false });
    window.simulate_event(gpui::MouseMoveEvent { position: to, pressed_button: Some(MouseButton::Left), modifiers: Default::default() });
    window.simulate_event(gpui::MouseUpEvent { button: MouseButton::Left, position: to, modifiers: Default::default(), click_count: 1 });
}

/// Dragging over a line selects its words, and a right-click in the stack sends them with the press.
#[test]
fn a_right_click_after_dragging_over_words_sends_them_as_the_selection() {
    let (app, mut window) = open(selection_page());
    settle(&app, &mut window, shows("Heard nothing."));
    // The second line, from its left edge to well past its end, takes all of it.
    drag(&mut window, point(px(0.0), px(30.0)), point(px(500.0), px(30.0)));
    assert_eq!(window.read(|view, _| view.selected_text()).as_deref(), Some("alpha beta gamma delta"));
    right_click(&mut window, point(px(500.0), px(30.0)));
    settle(&app, &mut window, shows("Heard 1:onContextMenu selection=alpha beta gamma delta."));
    // A press elsewhere lets go of it.
    window.simulate_click(point(px(300.0), px(200.0)), MouseButton::Left);
    assert_eq!(window.read(|view, _| view.selected_text()), None);
    right_click(&mut window, point(px(500.0), px(30.0)));
    settle(&app, &mut window, shows("Heard 1:onContextMenu selection=none."));
}

/// Dragging from one line into the next selects the end of the first and the start of the second, a line apart.
#[test]
fn dragging_from_one_line_into_the_next_selects_across_them() {
    let (app, mut window) = open(selection_page());
    settle(&app, &mut window, shows("Heard nothing."));
    drag(&mut window, point(px(150.0), px(8.0)), point(px(40.0), px(30.0)));
    let selected = window.read(|view, _| view.selected_text()).unwrap_or_default();
    let (first, second) = selected.split_once('\n').unwrap_or_else(|| panic!("two lines: {selected:?}"));
    assert!(first.ends_with("after it") && !second.is_empty() && "alpha beta gamma delta".starts_with(second), "{selected:?}");
    // Dragged back up past the first line's start, it takes the whole of the first line and nothing of the second.
    drag(&mut window, point(px(40.0), px(30.0)), point(px(0.0), px(0.0)));
    let selected = window.read(|view, _| view.selected_text()).unwrap_or_default();
    assert!(selected.ends_with("the docs and more words after it") || selected.starts_with("the docs"), "{selected:?}");
}

/// A double press selects the word under it.
#[test]
fn a_double_press_selects_a_word() {
    let (app, mut window) = open(selection_page());
    settle(&app, &mut window, shows("Heard nothing."));
    let at = point(px(3.0), px(30.0));
    window.simulate_mouse_move(at);
    window.simulate_event(gpui::MouseDownEvent { button: MouseButton::Left, position: at, modifiers: Default::default(), click_count: 2, first_mouse: false });
    assert_eq!(window.read(|view, _| view.selected_text()).as_deref(), Some("alpha"));
}

/// A press and release on a clickable span without moving is a click, and selects nothing.
#[test]
fn a_plain_click_on_a_clickable_span_still_clicks_and_selects_nothing() {
    let (app, mut window) = open(selection_page());
    settle(&app, &mut window, shows("Heard nothing."));
    window.simulate_mouse_move(point(px(10.0), px(8.0)));
    window.simulate_click(point(px(10.0), px(8.0)), MouseButton::Left);
    settle(&app, &mut window, shows("Heard 3:onClick selection=none."));
    assert_eq!(window.read(|view, _| view.selected_text()), None);
}

/// Cmd-C with words selected puts them on the clipboard.
#[test]
fn cmd_c_copies_the_selected_words() {
    let (app, mut window) = open(selection_page());
    settle(&app, &mut window, shows("Heard nothing."));
    drag(&mut window, point(px(0.0), px(30.0)), point(px(500.0), px(30.0)));
    key(&mut window, "cmd-c");
    let copied = app.read(|cx| cx.read_from_clipboard().and_then(|item| item.text()));
    assert_eq!(copied.as_deref(), Some("alpha beta gamma delta"));
}

/// A dialog's `Actions` (Cancel and Save) over a clickable `ListRow`, on a page that fills its frame: what each click
/// reached.
fn kit() -> String {
    let (mut cancelled, mut saved, mut opened) = (0, 0, 0);
    fake::page("page", move |event| {
        match event {
            Some(("3:click", _)) => cancelled += 1,
            Some(("5:click", _)) => saved += 1,
            Some(("10:click", _)) => opened += 1,
            _ => {}
        }
        json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [
            { "id": 1, "type": "Actions", "children": [
                { "id": 2, "type": "Slot", "props": { "name": "dismiss" }, "children": [
                    { "id": 3, "type": "Button", "props": { "variant": "outline", "onClick": "3:click" }, "children": [text_node(4, "Cancel")] }] },
                { "id": 5, "type": "Button", "props": { "onClick": "5:click" }, "children": [text_node(6, "Save")] }] },
            { "id": 10, "type": "ListRow", "props": { "onClick": "10:click" }, "children": [
                { "id": 11, "type": "Slot", "props": { "name": "title" }, "children": [text_node(12, "Build")] }] },
            text(20, 21, format!("Cancelled {cancelled}, saved {saved}, opened {opened}.")) ] })
    })
}

#[test]
fn actions_sit_at_the_end_of_the_line_with_the_acting_button_last_and_a_list_row_is_clickable_across_its_width() {
    let (app, mut window) = open(kit());
    settle(&app, &mut window, shows("Cancelled 0, saved 0, opened 0."));
    let width = window.update(|_, window, _| window.viewport_size().width.as_f32());
    // The start of the line is empty, and its end is Save, the button that acts.
    click_along(&app, &mut window, [6.0, width - 6.0]);
    settle(&app, &mut window, shows("Cancelled 0, saved 1, opened 0."));
    // Under the 32pt buttons, the row's far end opens it as its title does.
    window.simulate_click(point(px(width - 6.0), px(40.0)), MouseButton::Left);
    settle(&app, &mut window, shows("Cancelled 0, saved 1, opened 1."));
}

/// A page over the backdrop it asks for, in a tab as the workspace draws one.
mod backdrop {
    use gpui::{Entity, StyleRefinement, Window, div, prelude::*};

    use super::*;
    use crate::ui::{BACKDROP_STEP, BackdropLayer, backdrop_slot, spinner_layer};

    /// A fill page, as Claude chat's, that asks for the pixels behind it.
    fn pixels() -> String {
        fake::page("page", |_| {
            json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [{ "id": 1, "type": "Page", "props": { "flush": true, "backdrop": "pixels" }, "children": [
                { "id": 2, "type": "Slot", "props": { "name": "body" }, "children": [text(3, 4, "Step 99 passes".into())] }] }] })
        })
    }

    /// The window's backdrop layer, then the tab's card over it, which leaves the backdrop's slot and holds the page, kept
    /// as last drawn.
    struct Tab {
        page: Entity<PageView>,
        layer: Entity<BackdropLayer>,
    }

    impl Render for Tab {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let slot = self.page.read(cx).backdrop().map(|(kind, palette)| backdrop_slot("tab", kind, palette, 13.0));
            div()
                .relative()
                .size_full()
                .child(spinner_layer(window))
                .child(self.layer.clone())
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .children(slot)
                        .child(self.page.clone().cached(StyleRefinement::default().size_full())),
                )
        }
    }

    #[test]
    fn a_step_of_the_backdrop_draws_the_layer_and_the_page_keeps_its_drawing() {
        let mut app = TestApp::new();
        app.background_executor().allow_parking();
        let host = hosted(&mut app);
        let source = pixels();
        let mut window = app.open_window(move |_, cx| Tab {
            page: cx.new(|cx| PageView::in_plugin(&host, &source, cx).in_card()),
            layer: cx.new(|_| BackdropLayer::default()),
        });
        window.update(|_, window, _| window.activate_window());
        let deadline = Instant::now() + Duration::from_secs(10);
        while !window.read(|tab, cx| tab.page.read(cx).backdrop().is_some()) {
            assert!(Instant::now() < deadline, "the page never asked for its backdrop");
            app.run_until_parked();
            window.draw();
            thread::sleep(Duration::from_millis(5));
        }
        window.draw();
        let drawn = |window: &TestAppWindow<Tab>| window.read(|tab, cx| (tab.page.read(cx).renders(), tab.layer.read(cx).drawn));
        let (pages, layers) = drawn(&window);
        for _ in 0..15 {
            app.advance_clock(BACKDROP_STEP);
            app.run_until_parked();
        }
        let (pages, layers) = (drawn(&window).0 - pages, drawn(&window).1 - layers);
        eprintln!("[measure] 15 steps of the backdrop: the page rendered {pages} times, the layer {layers}");
        assert_eq!(pages, 0, "the page kept its drawing");
        assert!((14..=16).contains(&layers), "the layer drew {layers} times in 15 steps");
    }
}

/// A chat at work in a tab, as the workspace draws one: what moves in it turns in the window's layer, not the page.
mod spinners {
    use gpui::{Entity, StyleRefinement, Window, div, prelude::*};

    use super::*;
    use crate::ui::{SPIN_STEP, place_view, spinner_layer, view_spinner_layer};

    /// A page with everything a working chat moves: a running step's spinner, a large one, a working agent's dot and a
    /// shimmering "Working" line that counts the seconds since it started.
    fn working() -> String {
        fake::page("page", |_| {
            json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [{ "id": 1, "type": "Page", "props": { "flush": true }, "children": [
                { "id": 2, "type": "Slot", "props": { "name": "body" }, "children": [
                    { "id": 3, "type": "Spinner", "props": { "label": "Running" } },
                    { "id": 4, "type": "Spinner", "props": { "size": "large" } },
                    { "id": 5, "type": "Dot", "props": { "state": "working" } },
                    { "id": 6, "type": "Text", "props": { "tone": "muted", "shimmer": true, "since": 1_763_470_000_000u64 },
                      "children": [text_node(7, "Working")] },
                    text(8, 9, "Step 99 passes".into())
                ] }] }] })
        })
    }

    /// The window: the tab's page, kept as last drawn and put on screen while `shown`, the layer of what moves in it,
    /// and the window's spinner layer, counting the window's draws.
    struct Tab {
        page: Entity<PageView>,
        shown: bool,
        drawn: usize,
    }

    impl Render for Tab {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.drawn += 1;
            let layer = spinner_layer(window);
            if self.shown {
                place_view(self.page.entity_id(), window.window_handle().window_id());
            }
            div()
                .relative()
                .size_full()
                .child(self.page.clone().cached(StyleRefinement::default().size_full()))
                .child(view_spinner_layer(window))
                .child(layer)
        }
    }

    fn open_tab(shown: bool) -> (TestApp, TestAppWindow<Tab>) {
        let mut app = TestApp::new();
        app.background_executor().allow_parking();
        let host = hosted(&mut app);
        let source = working();
        let mut window = app.open_window(move |_, cx| Tab {
            page: cx.new(|cx| PageView::in_plugin(&host, &source, cx).in_card()),
            shown,
            drawn: 0,
        });
        // In front, where a shimmer pulses.
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

    /// The page's renders and the window's draws so far.
    fn drawn(window: &TestAppWindow<Tab>) -> (usize, usize) {
        window.read(|tab, cx| (tab.page.read(cx).renders(), tab.drawn))
    }

    /// The guard for every page: a node that moves on its own (a spinner, a working dot, a shimmer, a `since` that
    /// counts) must move in the window's layer. One that animates in the page draws the whole page again at each step,
    /// which in a long chat kept a core busy while an agent worked.
    #[test]
    fn a_second_of_a_working_chat_draws_the_layer_and_never_the_page() {
        let (app, window) = open_tab(true);
        let (pages, frames) = drawn(&window);
        for _ in 0..30 {
            app.advance_clock(SPIN_STEP);
            app.run_until_parked();
        }
        let (pages, frames) = (drawn(&window).0 - pages, drawn(&window).1 - frames);
        eprintln!("[measure] 30 spinner steps of a working chat: the page rendered {pages} times, the window {frames}");
        assert_eq!(pages, 0, "the page kept its drawing");
        assert!((29..=31).contains(&frames), "the layer turned {frames} times in 30 steps");
    }

    /// A page nobody puts on screen with the layer, as one in a window of its own, still turns its spinners and pulses
    /// its shimmer, in place.
    #[test]
    fn a_page_without_the_layer_moves_in_place() {
        let (app, window) = open_tab(false);
        let pages = drawn(&window).0;
        for _ in 0..30 {
            app.advance_clock(SPIN_STEP);
            app.run_until_parked();
        }
        let pages = drawn(&window).0 - pages;
        assert!(pages >= 10, "the page turned its spinners {pages} times in 30 steps");
    }
}

/// A long chat with its message box, as Claude chat draws one: 120 messages with copy and bookmark buttons, and a box
/// whose text the page sends back on each change.
fn long_chat_with_box() -> String {
    let mut typed = String::new();
    fake::page("page", move |event| {
        if let Some((_, change)) = event {
            typed = change["value"].as_str().unwrap_or_default().to_owned();
        }
        let turns: Vec<Value> = (0..120usize)
            .map(|turn| {
                let id = 100 + turn * 10;
                json!({ "id": id, "type": "Card", "props": {}, "children": [
                    text(id + 1, id + 2, format!("Turn {turn}: refactor the API client")),
                    text(id + 3, id + 4, "The client now retries once on a dropped connection and reads the token from the keychain. ".repeat(4)),
                    { "id": id + 5, "type": "Button", "props": { "onClick": format!("{}:onClick", id + 5) }, "children": [text_node(id + 6, "Copy")] },
                    { "id": id + 7, "type": "Button", "props": { "onClick": format!("{}:onClick", id + 7) }, "children": [text_node(id + 8, "Bookmark")] }
                ] })
            })
            .collect();
        json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [{ "id": 1, "type": "Page", "props": { "flush": true }, "children": [
            { "id": 2, "type": "Slot", "props": { "name": "body" }, "children": [
                { "id": 3, "type": "Scroll", "props": {}, "children": [{ "id": 4, "type": "Stack", "props": { "gap": "l" }, "children": turns }] },
                { "id": 5, "type": "Textarea", "props": { "label": "Message", "rows": 3, "value": typed, "onChange": "5:change" } }
            ] }] }] })
    })
}

/// The page in a tab, kept as last drawn, as the workspace draws one.
struct InTab(gpui::Entity<PageView>);

impl gpui::Render for InTab {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl gpui::IntoElement {
        use gpui::{ParentElement, Styled};
        gpui::div().size_full().child(self.0.clone().cached(gpui::StyleRefinement::default().size_full()))
    }
}

/// The long chat in a tab, drawn, with the keyboard in its message box.
fn open_long_chat() -> (TestApp, TestAppWindow<InTab>, gpui::Entity<PageView>) {
    let mut app = TestApp::new();
    app.background_executor().allow_parking();
    let host = hosted(&mut app);
    let source = long_chat_with_box();
    let mut window = app.open_window(move |_, cx| InTab(gpui::AppContext::new(cx, |cx| PageView::in_plugin(&host, &source, cx))));
    let page = window.read(|tab, _| tab.0.clone());
    let deadline = Instant::now() + Duration::from_secs(10);
    while !window.read(|_, cx| page.read(cx).text().contains("Turn 119")) {
        assert!(Instant::now() < deadline, "the page never drew its tree");
        app.run_until_parked();
        window.draw();
        thread::sleep(Duration::from_millis(5));
    }
    window.update(|_, window, cx| page.read(cx).input_focus("Message", cx).expect("the box").focus(window, cx));
    window.draw();
    (app, window, page)
}

/// What a keystroke in a long chat's message box costs: how many times the page renders for it.
#[test]
fn a_key_typed_in_a_long_chat_renders_the_page_once() {
    let (app, mut window, page) = open_long_chat();
    let renders = |window: &TestAppWindow<InTab>| window.read(|_, cx| page.read(cx).renders());
    let before = renders(&window);
    let keys = "hello there";
    for key in keys.chars() {
        let trees = window.read(|_, cx| page.read(cx).trees_sent());
        window.simulate_input(&key.to_string());
        window.draw();
        // The page's echo of the key arrives from its own thread. Typed before it came, the next key makes it stale, which
        // draws the page: under a whole test run's load the echo took longer than a fixed wait.
        let deadline = Instant::now() + Duration::from_secs(10);
        while window.read(|_, cx| page.read(cx).trees_sent()) == trees {
            assert!(Instant::now() < deadline, "no echo of {key:?}");
            app.run_until_parked();
            window.draw();
            thread::sleep(Duration::from_millis(2));
        }
        let until = Instant::now() + Duration::from_millis(30);
        while Instant::now() < until {
            app.run_until_parked();
            window.draw();
            thread::sleep(Duration::from_millis(2));
        }
    }
    let rendered = renders(&window) - before;
    // How long a key's frame takes: the field's change and the page drawn again.
    let started = Instant::now();
    for _ in 0..20 {
        window.simulate_input("x");
        window.draw();
    }
    eprintln!("[measure] a key's frame in a long chat: {:.2} ms", started.elapsed().as_secs_f64() * 1000.0 / 20.0);
    eprintln!("[measure] {} keys typed in a long chat rendered the page {rendered} times", keys.len());
    // The field drawing its new text is one; the page's echo of it and the field's own notice of the change were two
    // more, each a whole long chat laid out again.
    assert!(rendered <= keys.len() + 1, "{rendered} renders for {} keys", keys.len());
}

/// A key typed in a long chat's message box draws the messages in view and half a view either side, not all 120: each
/// one drawn is laid out and its lines wrapped again.
#[test]
fn a_key_typed_in_a_long_chat_lays_out_only_the_messages_in_view() {
    let (app, mut window, page) = open_long_chat();
    window.simulate_input("x");
    window.draw();
    app.run_until_parked();
    window.draw();
    let (drawn, of, _) = window.read(|_, cx| page.read(cx).windowed_drawn(3)).expect("the chat is drawn a window at a time");
    eprintln!("[measure] a key in a {of}-message chat drew messages {drawn:?}, {} of them", drawn.len());
    assert_eq!(of, 120);
    assert!(drawn.len() * 4 <= of, "{drawn:?}");
}

/// Row `i` of a long column: one to five lines, as a chat's messages are, so a row never drawn isn't as tall as the
/// guess for it.
fn varied_row(i: usize) -> Value {
    text(1_000 + 2 * i, 1_001 + 2 * i, format!("Row {i}. {}", "Some words of a reply. ".repeat(i * 7 % 12)))
}

/// 120 varied rows in the column `3` of the `Scroll` `2`, which has `props`.
fn long_column(props: Value) -> String {
    fake::page("page", move |_| {
        let rows: Vec<Value> = (0..120).map(varied_row).collect();
        json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [
            { "id": 2, "type": "Scroll", "props": props.clone(), "children": [{ "id": 3, "type": "Stack", "props": {}, "children": rows }] }] })
    })
}

/// The wheel turned by `by` points over the page's scroll, down for less than 0.
fn scroll_by(window: &mut View, by: f32) {
    window.simulate_event(gpui::ScrollEvent {
        position: point(px(60.0), px(150.0)),
        delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(by))),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
        source: Default::default(),
    });
}

/// A few draws, for what a draw asks of the next.
fn draws(app: &TestApp, window: &mut View) {
    for _ in 0..4 {
        app.run_until_parked();
        window.draw();
    }
}

/// A long column draws the rows in view, at its top and scrolled to its middle, and none far from them.
#[test]
fn a_long_column_draws_the_rows_in_view_wherever_it_is_scrolled() {
    let (app, mut window) = open_narrow(long_column(json!({})), 500.0);
    settle(&app, &mut window, |view| view.windowed_drawn(2).is_some_and(|(_, of, tops)| of == 120 && tops.len() == 120));
    let (drawn, _, _) = window.read(|view, _| view.windowed_drawn(2)).unwrap();
    assert!(drawn.start == 0 && drawn.len() < 40, "{drawn:?}");
    scroll_by(&mut window, -3_000.0);
    draws(&app, &mut window);
    let (drawn, _, tops) = window.read(|view, _| view.windowed_drawn(2)).unwrap();
    // Every row with any of it in the 400 point window is drawn.
    let in_view: Vec<usize> = (0..120).filter(|at| tops[*at] < 400.0 && tops.get(at + 1).is_none_or(|next| *next > 0.0)).collect();
    assert!(in_view.len() > 2 && in_view.iter().all(|at| drawn.contains(at)), "{in_view:?} not all in {drawn:?}");
    assert!(drawn.start > 20 && drawn.len() < 40, "{drawn:?}");
}

/// A long column told to scroll to a row it hasn't drawn puts that row at the top of the view, though the rows above it
/// were only guessed at, and keeps it there.
#[test]
fn a_long_column_scrolled_to_a_row_it_has_not_drawn_puts_it_at_the_top() {
    let (app, mut window) = open_narrow(long_column(json!({ "scrollTo": { "index": 100, "token": 1 } })), 500.0);
    let at_top = |view: &PageView| view.windowed_drawn(2).is_some_and(|(drawn, _, tops)| drawn.contains(&100) && tops[100].abs() < 1.0);
    settle(&app, &mut window, at_top);
    draws(&app, &mut window);
    let tops = window.read(|view, _| view.windowed_drawn(2)).unwrap().2;
    assert!(tops[100].abs() < 1.0, "row 100 is {} below the view's top", tops[100]);
}

/// 100 varied rows and three more for each press of More, in a following `Scroll` whose rows rise in.
fn growing_column() -> String {
    counting(|heard| {
        let rows: Vec<Value> = (0..100 + 3 * heard as usize).map(varied_row).collect();
        json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [
            { "id": 1, "type": "Button", "props": { "onClick": "1:onClick" }, "children": [text_node(9, "More")] },
            { "id": 2, "type": "Scroll", "props": { "follow": "end" }, "children": [{ "id": 3, "type": "Stack", "props": { "enter": "rise" }, "children": rows }] }] })
    })
}

/// A following long column stays at its end as rows come, and draws the newest.
#[test]
fn a_following_long_column_stays_at_its_end_as_rows_come() {
    let (app, mut window) = open_narrow(growing_column(), 500.0);
    settle(&app, &mut window, |view| view.follow_scrolled(2).is_some_and(|(top, max)| max > 1_000.0 && top >= max - 1.0));
    for more in 1..=3usize {
        window.simulate_click(point(px(30.0), px(16.0)), MouseButton::Left);
        let newest = format!("Row {}.", 99 + 3 * more);
        settle(&app, &mut window, move |view| view.text().contains(&newest));
        draws(&app, &mut window);
        let (drawn, of, _) = window.read(|view, _| view.windowed_drawn(2)).unwrap();
        let (top, max) = window.read(|view, _| view.follow_scrolled(2)).unwrap();
        assert!(of == 100 + 3 * more && drawn.end == of && drawn.len() < 40, "{drawn:?} of {of}");
        assert!(top >= max - 1.0, "scrolled {top} of {max}");
    }
}

/// 100 varied rows in a following `Scroll` that asks for older ones at its top, and 50 older rows above them once asked.
fn older_long_rows(heard: u32) -> Value {
    let mut rows: Vec<Value> = (0..100).map(varied_row).collect();
    if heard > 0 {
        let older = (0..50).map(|i| text(5_000 + 2 * i, 5_001 + 2 * i, format!("Older {i}. {}", "Earlier words. ".repeat(i % 9))));
        rows.splice(0..0, older);
    }
    let props = if heard > 0 { json!({ "follow": "end" }) } else { json!({ "follow": "end", "onReachStart": "2:onReachStart" }) };
    json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [
        { "id": 2, "type": "Scroll", "props": props, "children": [{ "id": 3, "type": "Stack", "props": {}, "children": rows }] }] })
}

/// Older rows loaded above a long column, never drawn and so only guessed at, leave its first row where it was, at the
/// top of the view, as they come and as the ones near it are drawn.
#[test]
fn older_rows_loaded_above_a_long_column_keep_the_row_in_view_where_it_was() {
    let (app, mut window) = open_narrow(counting(older_long_rows), 500.0);
    settle(&app, &mut window, |view| view.follow_scrolled(2).is_some_and(|(top, max)| max > 1_000.0 && top >= max - 1.0));
    scroll_by(&mut window, 100_000.0);
    settle(&app, &mut window, |view| view.text().contains("Older 0.") && view.windowed_drawn(2).is_some_and(|(_, of, tops)| of == 150 && tops.len() == 150));
    draws(&app, &mut window);
    let (drawn, _, tops) = window.read(|view, _| view.windowed_drawn(2)).unwrap();
    assert!(tops[50].abs() < 1.0, "row 0 is {} below the view's top, drawn {drawn:?}", tops[50]);
}

/// 120 rows of a line each in a `Scroll`.
fn worded_column() -> String {
    fake::page("page", |_| {
        let rows: Vec<Value> = (0..120).map(|i| text(1_000 + 2 * i, 1_001 + 2 * i, format!("Line {i} words"))).collect();
        json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [
            { "id": 2, "type": "Scroll", "props": {}, "children": [{ "id": 3, "type": "Stack", "props": {}, "children": rows }] }] })
    })
}

/// A selection over rows of a long column copies them whole, and still does once they are scrolled far out of view: the
/// rows it holds are drawn wherever they are.
#[test]
fn a_selection_over_rows_of_a_long_column_copies_them_after_they_are_scrolled_away() {
    let (app, mut window) = open_narrow(worded_column(), 500.0);
    settle(&app, &mut window, |view| view.windowed_drawn(2).is_some_and(|(_, _, tops)| tops.len() == 120));
    let (tops, view_top) = window.read(|view, _| (view.windowed_drawn(2).unwrap().2, view.scroll_view_top(2).unwrap()));
    let y = |row: usize| px(view_top + tops[row] + 6.0);
    drag(&mut window, point(px(20.0), y(1)), point(px(400.0), y(3)));
    let copied = |window: &mut View| {
        key(window, "cmd-c");
        app.read(|cx| cx.read_from_clipboard().and_then(|item| item.text())).unwrap_or_default()
    };
    let whole = |copy: &str| copy.contains("1 words\nLine 2 words\nLine 3 words");
    let copy = copied(&mut window);
    assert!(whole(&copy), "{copy:?}");
    scroll_by(&mut window, -2_000.0);
    draws(&app, &mut window);
    let (drawn, _, tops) = window.read(|view, _| view.windowed_drawn(2)).unwrap();
    assert!(tops[3] < -1_000.0 && drawn.start <= 1, "row 3 at {}, drawn {drawn:?}", tops[3]);
    let selected = window.read(|view, _| view.selected_text()).unwrap_or_default();
    assert!(whole(&selected), "{selected:?}");
    let copy = copied(&mut window);
    assert!(whole(&copy), "{copy:?}");
}

/// 120 varied rows in a following `Scroll` whose text is 1.5 times as big once More is pressed, as Cmd-+ makes a chat's.
fn scaled_column() -> String {
    counting(|heard| {
        let rows: Vec<Value> = (0..120).map(varied_row).collect();
        let scale = if heard > 0 { 1.5 } else { 1.0 };
        json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [
            { "id": 1, "type": "Button", "props": { "onClick": "1:onClick" }, "children": [text_node(9, "More")] },
            { "id": 2, "type": "Scroll", "props": { "follow": "end", "textScale": scale }, "children": [{ "id": 3, "type": "Stack", "props": {}, "children": rows }] }] })
    })
}

/// Bigger text wraps every row again, and the kept heights of the rows not drawn are of the old size: the row at the top
/// of the view stays there.
#[test]
fn a_long_column_keeps_the_row_at_its_top_through_a_change_of_text_size() {
    let (app, mut window) = open_narrow(scaled_column(), 500.0);
    settle(&app, &mut window, |view| view.follow_scrolled(2).is_some_and(|(top, max)| max > 1_000.0 && top >= max - 1.0));
    scroll_by(&mut window, 2_500.0);
    draws(&app, &mut window);
    // The row whose top is the last at or above the view's top.
    let at_top = |window: &View| {
        let tops = window.read(|view, _| view.windowed_drawn(2)).unwrap().2;
        let at = tops.iter().rposition(|top| *top <= 0.5).unwrap();
        (at, tops[at])
    };
    let (row, top) = at_top(&window);
    window.simulate_click(point(px(30.0), px(16.0)), MouseButton::Left);
    settle(&app, &mut window, |view| view.trees_sent() >= 2);
    draws(&app, &mut window);
    let (after, moved) = at_top(&window);
    assert!(after == row && (moved - top).abs() < 1.0, "row {row} was {top} below the view's top, now row {after} is, {moved}");
}
