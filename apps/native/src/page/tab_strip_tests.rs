//! A page's `TabStrip`: one line that scrolls rather than wraps, and a click or key on a tab, its close button, the "+"
//! or a group's label reaching the page with the id.

use super::{View, draws, key, nothing_happens, open_narrow, settle, shows, text};
use crate::page::runtime::fake;
use gpui::{Bounds, MouseButton, Pixels, point, px};
use serde_json::{Value, json};

const STRIP: u64 = 1;

fn tab(n: usize, active: bool) -> Value {
    json!({ "id": 100 + n, "type": "Tab", "props": { "id": format!("t{n}"), "title": format!("Tab number {n} with a long title"), "url": format!("https://site{n}.test/"), "active": active } })
}

fn group(id: &str, collapsed: bool, count: usize) -> Value {
    json!({ "id": 200, "type": "TabGroup", "props": { "id": id, "name": "Fix login", "tone": "info", "collapsed": collapsed, "count": count } })
}

fn grouped(n: usize, group: &str) -> Value {
    let mut tab = tab(n, false);
    tab["props"]["group"] = group.into();
    tab
}

/// A page of one strip over `items`, and a line saying what it last heard: the handler and its payload.
fn page(items: Vec<Value>) -> String {
    let mut heard = "nothing".to_owned();
    fake::page("page", move |event| {
        if let Some((handler, payload)) = event {
            heard = format!("{handler} {payload}");
        }
        json!({ "id": 0, "type": "#root", "children": [
            { "id": STRIP, "type": "TabStrip", "props": { "label": "Tabs", "onSelect": "1:onSelect", "onClose": "1:onClose", "onNew": "1:onNew", "onGroup": "1:onGroup" }, "children": items.clone() },
            text(2, 3, format!("Heard {heard}.")) ] })
    })
}

fn bounds(window: &mut View) -> (Bounds<Pixels>, Vec<Bounds<Pixels>>, f32) {
    window.read(|view, _| view.tab_strip_bounds(STRIP)).expect("the strip was drawn")
}

fn click_at(app: &gpui::TestApp, window: &mut View, at: gpui::Point<Pixels>, button: MouseButton) {
    window.simulate_click(at, button);
    nothing_happens(app);
    window.draw();
}

/// With room, tabs are 200 points wide on a line 32 points high.
#[test]
fn tabs_with_room_are_at_their_widest_on_one_line() {
    let (app, mut window) = open_narrow(page((1..=3).map(|n| tab(n, n == 1)).collect()), 900.0);
    settle(&app, &mut window, shows("Heard nothing."));
    draws(&app, &mut window);
    let (strip, tabs, _) = bounds(&mut window);
    assert_eq!(tabs.len(), 3);
    assert!(tabs.iter().all(|t| (t.size.width.as_f32() - 200.0).abs() < 0.5 && t.size.height.as_f32() == 28.0), "{tabs:?}");
    assert_eq!(strip.size.height.as_f32(), 32.0);
}

/// More tabs than fit stay on one row, each shrunk to the least width, and the strip scrolls; its height doesn't change.
#[test]
fn more_tabs_than_fit_stay_on_one_line_at_the_least_width_and_scroll() {
    let (app, mut window) = open_narrow(page((1..=14).map(|n| tab(n, n == 1)).collect()), 500.0);
    settle(&app, &mut window, shows("Heard nothing."));
    draws(&app, &mut window);
    let (strip, tabs, _) = bounds(&mut window);
    assert_eq!(tabs.len(), 14);
    assert!(tabs.iter().all(|t| t.origin.y == tabs[0].origin.y && t.size.height.as_f32() == 28.0), "one row: {tabs:?}");
    assert!(tabs.iter().all(|t| (t.size.width.as_f32() - 72.0).abs() < 0.5), "least width: {tabs:?}");
    assert_eq!(strip.size.height.as_f32(), 32.0);
    assert!(tabs.last().unwrap().right() > strip.right(), "past the strip's end, so it scrolls");
}

/// Between the widest and the least, tabs share the room evenly.
#[test]
fn tabs_that_nearly_fill_the_strip_shrink_evenly() {
    let (app, mut window) = open_narrow(page((1..=6).map(|n| tab(n, n == 1)).collect()), 600.0);
    settle(&app, &mut window, shows("Heard nothing."));
    draws(&app, &mut window);
    let (strip, tabs, _) = bounds(&mut window);
    let widths: Vec<f32> = tabs.iter().map(|t| t.size.width.as_f32()).collect();
    assert!(widths.iter().all(|w| (w - widths[0]).abs() < 1.0) && widths[0] > 72.0 && widths[0] < 200.0, "{widths:?}");
    assert!(tabs.last().unwrap().right() <= strip.right() + px(0.5));
}

/// The tab that turns active comes into view.
#[test]
fn a_tab_that_is_active_off_the_strips_end_is_scrolled_into_view() {
    let (app, mut window) = open_narrow(page((1..=14).map(|n| tab(n, n == 14)).collect()), 500.0);
    settle(&app, &mut window, shows("Heard nothing."));
    draws(&app, &mut window);
    let (strip, tabs, offset) = bounds(&mut window);
    assert!(offset < 0.0, "scrolled: {offset}");
    // The tabs' bounds are where they lie before the scroll, which moves them by `offset`.
    let shown = tabs[13].right() + px(offset);
    assert!(shown <= strip.right() + px(0.5) && tabs[13].left() + px(offset) >= strip.left(), "{:?} by {offset} in {strip:?}", tabs[13]);
}

/// A click on a tab selects it, its close button closes it, a middle click closes it, and the "+" opens a new one.
#[test]
fn a_click_on_a_tab_its_close_button_its_middle_and_the_plus_reach_the_page_with_the_id() {
    let (app, mut window) = open_narrow(page((1..=3).map(|n| tab(n, n == 1)).collect()), 900.0);
    settle(&app, &mut window, shows("Heard nothing."));
    draws(&app, &mut window);
    let (strip, tabs, _) = bounds(&mut window);
    let middle = |b: &Bounds<Pixels>| point(b.center().x - px(20.0), b.center().y);
    click_at(&app, &mut window, middle(&tabs[1]), MouseButton::Left);
    settle(&app, &mut window, shows(r#"Heard 1:onSelect {"id":"t2"}."#));
    // The shown tab's close button, in its end.
    click_at(&app, &mut window, point(tabs[0].right() - px(13.0), tabs[0].top() + px(13.0)), MouseButton::Left);
    settle(&app, &mut window, shows(r#"Heard 1:onClose {"id":"t1"}."#));
    click_at(&app, &mut window, middle(&tabs[2]), MouseButton::Middle);
    settle(&app, &mut window, shows(r#"Heard 1:onClose {"id":"t3"}."#));
    click_at(&app, &mut window, point(strip.right() + px(16.0), strip.center().y), MouseButton::Left);
    settle(&app, &mut window, shows("Heard 1:onNew {}."));
}

/// A group's label is a button that names its group; its tabs carry the label's place before them.
#[test]
fn a_click_on_a_groups_label_reaches_the_page_with_its_id() {
    let (app, mut window) = open_narrow(page(vec![group("g1", false, 2), grouped(1, "g1"), grouped(2, "g1"), tab(3, true)]), 900.0);
    settle(&app, &mut window, shows("Heard nothing."));
    draws(&app, &mut window);
    let (_, items, _) = bounds(&mut window);
    assert_eq!(items.len(), 4);
    assert!(items[0].size.width < px(120.0) && items[0].right() <= items[1].left());
    click_at(&app, &mut window, items[0].center(), MouseButton::Left);
    settle(&app, &mut window, shows(r#"Heard 1:onGroup {"id":"g1"}."#));
}

/// A group with a long name gives up room with the tabs when the strip fills, so neither is squeezed to a few letters
/// while the other keeps its full width.
#[test]
fn a_long_groups_label_shrinks_with_the_tabs() {
    let mut label = group("g1", false, 6);
    label["props"]["name"] = "Write the API docs for the billing service".into();
    let items = std::iter::once(label).chain((1..=6).map(|n| grouped(n, "g1"))).collect();
    let (app, mut window) = open_narrow(page(items), 640.0);
    settle(&app, &mut window, shows("Heard nothing."));
    draws(&app, &mut window);
    let (_, items, _) = bounds(&mut window);
    assert_eq!(items.len(), 7);
    assert!(items[0].size.width < px(150.0), "the label shrank: {items:?}");
    assert!(items[1..].iter().all(|tab| tab.size.width > px(80.0)), "the tabs kept room: {items:?}");
}

/// A collapsed group shows only its label.
#[test]
fn a_collapsed_group_draws_its_label_and_none_of_its_tabs() {
    let (app, mut window) = open_narrow(page(vec![group("g1", true, 2), grouped(1, "g1"), grouped(2, "g1"), tab(3, true)]), 900.0);
    settle(&app, &mut window, shows("Heard nothing."));
    draws(&app, &mut window);
    let (_, items, _) = bounds(&mut window);
    assert_eq!(items.len(), 2, "the label and the tab outside it: {items:?}");
    assert!(window.read(|view, _| view.text()).contains("Fix login"));
}

/// Tab reaches the first tab, the arrows move along them, Enter selects and Backspace closes.
#[test]
fn the_arrow_keys_move_between_tabs_enter_selects_and_backspace_closes() {
    let (app, mut window) = open_narrow(page((1..=3).map(|n| tab(n, n == 1)).collect()), 900.0);
    settle(&app, &mut window, shows("Heard nothing."));
    window.update(|_, window, cx| window.focus_next(cx));
    window.draw();
    key(&mut window, "right");
    window.draw();
    key(&mut window, "enter");
    settle(&app, &mut window, shows(r#"Heard 1:onSelect {"id":"t2"}."#));
    key(&mut window, "backspace");
    settle(&app, &mut window, shows(r#"Heard 1:onClose {"id":"t2"}."#));
    key(&mut window, "end");
    window.draw();
    key(&mut window, "delete");
    settle(&app, &mut window, shows(r#"Heard 1:onClose {"id":"t3"}."#));
}
