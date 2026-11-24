//! The screenshot examples' check for what a frame cuts off (`ui::cutoff`), on pages drawn in a headless window 640 and
//! 1000 points wide, as the examples draw each page: a row, a row of buttons and a card too wide for 640 are cut off
//! there and not at 1000; text cut short with an ellipsis, a board that scrolls sideways and a paragraph that wraps are
//! never cut off.

use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use gpui::{AnyWindowHandle, AppContext, Global, TestApp, WindowBounds, WindowOptions, point, px, size};
use serde_json::{Value, json};

use super::{
    plugin::{PluginHost, tests::Root},
    runtime::fake,
    view::PageView,
};
use crate::ui::cutoff::{self, Cut};

/// The plugin a page runs in and the folder of its storage, kept for as long as the app runs.
struct Hosted {
    _host: Arc<PluginHost>,
    _root: Root,
}

impl Global for Hosted {}

fn text(id: u64, words: &str) -> Value {
    json!({ "id": id, "type": "Text", "children": [{ "id": id + 1, "type": "#text", "text": words }] })
}

fn button(id: u64, label: &str) -> Value {
    json!({ "id": id, "type": "Button", "props": { "onClick": format!("{id}:onClick") }, "children": [{ "id": id + 1, "type": "#text", "text": label }] })
}

/// A box `width` points wide with nothing in it.
fn spacer(id: u64, width: u32) -> Value {
    json!({ "id": id, "type": "Stack", "props": { "width": width } })
}

/// What the check says a page of `body` cuts off in a window `width` points wide.
fn cut_off(body: Value, width: f32) -> Vec<Cut> {
    let mut app = TestApp::new();
    app.background_executor().allow_parking();
    let root = Root::new();
    let host = Arc::new(PluginHost::new(root.config("fixture")));
    app.update(|cx| cx.set_global(Hosted { _host: host.clone(), _root: root }));
    let source = fake::page("page", move |_| {
        json!({ "id": 0, "type": "#root", "children": [body.clone(), text(900, "Drawn.")] })
    });
    let bounds = gpui::Bounds::new(point(px(0.0), px(0.0)), size(px(width), px(400.0)));
    let mut window = app.open_window_with_options(
        WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() },
        move |_, cx| PageView::in_plugin(&host, &source, cx),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !window.read(|view, _| view.text().contains("Drawn.")) {
        assert!(Instant::now() < deadline, "the page never drew its tree");
        app.run_until_parked();
        window.draw();
        thread::sleep(Duration::from_millis(5));
    }
    let handle: AnyWindowHandle = window.handle().into();
    app.update(|cx| cx.update_window(handle, |_, window, cx| cutoff::check(window, cx)))
        .expect("the window is open")
}

/// Whether one of `cuts` names `words`.
fn names(cuts: &[Cut], words: &str) -> bool {
    cuts.iter().any(|cut| cut.what.contains(words))
}

/// A word a text holds out for whole (the page breaks only a word wider than 240 points inside itself).
const WORD: &str = "Supercalifragilisticexpialido";

/// Buttons, which never shrink, and a text never narrower than its longest word: the row is wider than 640.
fn long_word_row() -> Value {
    json!({ "id": 1, "type": "Row", "props": { "gap": "m" }, "children": [
        button(2, "Assign to me"),
        button(4, "Move to In progress"),
        button(6, "Close issue"),
        { "id": 8, "type": "Text", "props": { "grow": true }, "children": [{ "id": 9, "type": "#text", "text": WORD }] }] })
}

#[test]
fn a_long_word_in_a_fixed_width_row_is_cut_off_at_640_and_not_at_1000() {
    let cuts = cut_off(long_word_row(), 640.0);
    assert!(names(&cuts, "Supercalifragilistic"), "{cuts:#?}");
    assert!(cuts.iter().any(|cut| cut.width == 640.0 && cut.how.contains("window's right edge")), "{cuts:#?}");
    let cuts = cut_off(long_word_row(), 1000.0);
    assert!(cuts.is_empty(), "{cuts:#?}");
}

/// Five buttons in a row that doesn't wrap, as a toolbar's actions were before it wrapped them.
fn actions_row() -> Value {
    let buttons: Vec<Value> = (1..=5).map(|n| button(10 + 2 * n, &format!("Publish draft {n}"))).collect();
    json!({ "id": 1, "type": "Row", "props": { "gap": "s" }, "children": buttons })
}

#[test]
fn a_row_of_actions_that_doesnt_fit_is_cut_off_at_640_and_not_at_1000() {
    let cuts = cut_off(actions_row(), 640.0);
    assert!(names(&cuts, "Publish draft 5"), "{cuts:#?}");
    assert!(!names(&cuts, "Publish draft 1"), "the first button fits: {cuts:#?}");
    let cuts = cut_off(actions_row(), 1000.0);
    assert!(cuts.is_empty(), "{cuts:#?}");
}

/// A card whose row holds a box 560 points wide and a button: wider than the card at 640.
fn wide_card() -> Value {
    json!({ "id": 1, "type": "Card", "children": [
        text(2, "Card title"),
        { "id": 4, "type": "Row", "props": { "gap": "s" }, "children": [spacer(5, 560), button(6, "Open details")] }] })
}

#[test]
fn a_card_wider_than_its_column_is_cut_off_at_640_and_not_at_1000() {
    let cuts = cut_off(wide_card(), 640.0);
    assert!(names(&cuts, "Open details"), "{cuts:#?}");
    let cuts = cut_off(wide_card(), 1000.0);
    assert!(cuts.is_empty(), "{cuts:#?}");
}

/// In a window wide enough for it, a button still runs past the edge of a card 400 points wide.
#[test]
fn a_button_past_its_cards_edge_is_flagged_inside_the_window() {
    let card = json!({ "id": 1, "type": "Stack", "props": { "width": 400 }, "children": [
        { "id": 2, "type": "Card", "children": [
            { "id": 3, "type": "Row", "props": { "gap": "s" }, "children": [spacer(4, 360), button(5, "Open details")] }] }] });
    let cuts = cut_off(card, 1000.0);
    assert!(
        cuts.iter().any(|cut| cut.what.contains("Open details") && cut.how.contains("past its card's right edge")),
        "{cuts:#?}"
    );
}

/// A long line cut short with an ellipsis beside a box of a fixed width fits its row at any width.
#[test]
fn text_cut_short_with_an_ellipsis_is_never_cut_off() {
    let row = json!({ "id": 1, "type": "Row", "props": { "gap": "m" }, "children": [
        spacer(2, 240),
        { "id": 3, "type": "Text", "props": { "grow": true, "truncate": true }, "children": [
            { "id": 4, "type": "#text", "text": "A long line that is cut short with an ellipsis where it runs out of room. ".repeat(4) }] }] });
    let cuts = cut_off(row, 640.0);
    assert!(cuts.is_empty(), "{cuts:#?}");
}

/// Four columns of a board, wider than 640 together: the board scrolls sideways to them.
#[test]
fn a_board_that_scrolls_sideways_is_never_cut_off() {
    let columns: Vec<Value> = (0..4)
        .map(|n| {
            let id = 10 + 10 * n;
            json!({ "id": id, "type": "BoardColumn", "children": [
                { "id": id + 1, "type": "Slot", "props": { "name": "title" }, "children": [{ "id": id + 2, "type": "#text", "text": format!("Column {n}") }] },
                { "id": id + 3, "type": "Slot", "props": { "name": "body" }, "children": [button(id + 4, &format!("Card in column {n}"))] }] })
        })
        .collect();
    let board = json!({ "id": 1, "type": "Board", "props": { "label": "Issues" }, "children": columns });
    let cuts = cut_off(board, 640.0);
    assert!(cuts.is_empty(), "{cuts:#?}");
}

/// A name in a box three letters wide comes apart a few letters a line; a paragraph that wraps between its words
/// doesn't.
#[test]
fn text_squeezed_to_a_few_letters_a_line_is_flagged() {
    let squeezed = json!({ "id": 1, "type": "Row", "children": [
        { "id": 2, "type": "Stack", "props": { "width": 24 }, "children": [text(3, "Settings")] }] });
    let cuts = cut_off(squeezed, 640.0);
    assert!(cuts.iter().any(|cut| cut.what.contains("Settings") && cut.how.contains("squeezed")), "{cuts:#?}");
    let paragraph = json!({ "id": 1, "type": "Stack", "props": { "width": 200 }, "children": [
        text(2, "A paragraph that wraps between its words, line after line, as a paragraph does.")] });
    let cuts = cut_off(paragraph, 640.0);
    assert!(cuts.is_empty(), "{cuts:#?}");
}

/// A split's fixed pane has a border on its right side only, which GPUI paints as a piece of its own with a clip of its
/// own: that clip is a sliver at the pane's right edge, and the pane is not cut off.
#[test]
fn a_splits_divider_is_never_cut_off() {
    let split = json!({ "id": 1, "type": "Split", "props": { "defaultSize": 200 }, "children": [
        { "id": 2, "type": "Slot", "props": { "name": "start" }, "children": [text(3, "The list")] },
        { "id": 5, "type": "Slot", "props": { "name": "end" }, "children": [text(6, "The item")] }] });
    let cuts = cut_off(split, 640.0);
    assert!(cuts.is_empty(), "{cuts:#?}");
}

/// A button in a column narrower than its label is squeezed to the column's width, and its label, centred, runs past
/// both its edges and the window's.
#[test]
fn a_button_squeezed_narrower_than_its_label_is_flagged() {
    let cuts = cut_off(json!({ "id": 1, "type": "Stack", "props": { "width": 30 }, "children": [button(2, "Sign in to GitHub")] }), 640.0);
    assert!(names(&cuts, "Sign in to GitHub"), "{cuts:#?}");
}
