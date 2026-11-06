//! A page in a headless window: its tree is drawn, and a click on a button reaches the page's handler.

use super::{
    plugin::{PluginHost, tests::Root},
    tree::{Level, Node, Tree},
    view::{PageView, level_color},
};
use crate::theme;
use gpui::{Keystroke, MouseButton, TestApp, TestAppWindow, point, px};
use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

const PILOT: &str = include_str!("../../pages/dist/pilot.js");
const COMPONENTS: &str = include_str!("../../pages/dist/components-page.js");
const FIXTURE_MAIN: &str = include_str!("../../pages/dist/fixture-main.js");
const FIXTURE_PAGE: &str = include_str!("../../pages/dist/fixture-page.js");

type View = TestAppWindow<PageView>;

fn open(source: &'static str) -> (TestApp, View) {
    let mut app = TestApp::new();
    app.background_executor().allow_parking();
    let window = app.open_window(move |_, cx| PageView::new(source, cx));
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
    let (app, mut window) = open(PILOT);
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
    let (app, mut window) = open(PILOT);
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

const THROWS: &str = r#"
globalThis.__wingsEvent = () => { throw new Error('nope'); };
wings.render(JSON.stringify({ id: 0, type: '#root', props: {}, children: [
  { id: 1, type: 'Page', props: {}, children: [
    { id: 2, type: 'Slot', props: { name: 'body' }, children: [
      { id: 3, type: 'Button', props: { onClick: '3:onClick' }, children: [{ id: 4, type: '#text', text: 'Boom' }] },
      { id: 5, type: 'Hologram', props: {}, children: [{ id: 6, type: '#text', text: 'not drawn' }] },
    ] }] }] }));
"#;

#[test]
fn what_the_page_throws_is_shown_under_it_and_what_the_host_does_not_know_is_not_drawn() {
    let (app, mut window) = open(THROWS);
    settle(&app, &mut window, shows("Boom"));
    assert!(window.read(|view, _| !view.text().contains("not drawn")));
    window.simulate_click(point(px(22.0), px(30.0)), MouseButton::Left);
    settle(&app, &mut window, |view| {
        view.error().is_some_and(|error| error.contains("nope"))
    });
}

#[test]
fn a_plugin_page_in_a_window_asks_main_and_shows_the_count_main_saved() {
    let root = Root::new();
    let host = Arc::new(PluginHost::new(root.config("fixture")));
    host.start_main(FIXTURE_MAIN, |_| {}).unwrap();
    let mut app = TestApp::new();
    app.background_executor().allow_parking();
    let page_host = host.clone();
    let mut window =
        app.open_window(move |_, cx| PageView::in_plugin(&page_host, FIXTURE_PAGE, cx));
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
    let (app, mut window) = open(COMPONENTS);
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
    let (app, mut window) = open(COMPONENTS);
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
    let (app, mut window) = open(COMPONENTS);
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
    assert_eq!(color("40"), theme::INFO);
    assert_eq!(color("85"), theme::WORKING);
    assert_eq!(color("96"), theme::DANGER);
    assert_eq!(level_color(Level::Normal), theme::INFO);
}
