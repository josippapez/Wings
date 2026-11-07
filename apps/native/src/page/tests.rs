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

const GRAPH: &str = r#"
let selected = -1, offset = 0, end = 64, requests = 0;
function render() {
  const rows = Array.from({length:end-offset}, (_,i)=>({hash:String(offset+i), subject:'Commit '+(offset+i), author:'Ada', date:'2026-11-06', refs:[], lane:0, color:0, merge:false, lines:[0,0,2,0]}));
  wings.render(JSON.stringify({id:0,type:'#root',children:[{id:1,type:'Page',children:[{id:2,type:'Slot',props:{name:'body'},children:[
    {id:3,type:'Text',children:[{id:4,type:'#text',text:'Selected '+selected+', requests '+requests}]},
    {id:5,type:'Graph',props:{total:10000,lanes:1,selected,offset,rows,onRangeChange:'5:range',onSelect:'5:select'}}
  ]}],props:{flush:true}}]}));
}
globalThis.__wingsEvent=(handler,json)=>{ const e=JSON.parse(json); if(handler==='5:range'){offset=e.start;end=e.end;requests++;}else selected=e.index;render();return true;};
render();
"#;

const SEARCH: &str = r#"
let query = '';
function render(){wings.render(JSON.stringify({id:0,type:'#root',props:{fill:true},children:[{id:1,type:'Page',children:[{id:2,type:'Slot',props:{name:'body'},children:[
{id:3,type:'SearchField',props:{label:'Search commits',placeholder:'Search commits',onInput:'3:input'}},
{id:4,type:'Text',children:[{id:5,type:'#text',text:'Query='+query}]}]}]}]}));}
globalThis.__wingsEvent=(_,json)=>{query=JSON.parse(json).value;render();return true;};render();
"#;

#[test]
fn search_field_physical_input_reaches_the_query_and_backspace_edits_it() {
    let (app, mut window) = open(SEARCH);
    settle(&app, &mut window, shows("Query="));
    window.simulate_click(point(px(70.0), px(27.0)), MouseButton::Left);
    window.simulate_input("coupon");
    settle(&app, &mut window, shows("Query=coupon"));
    key(&mut window, "backspace");
    settle(&app, &mut window, shows("Query=coupo"));
}

const FORM: &str = r#"
let name = 'x', note = '';
function render(){wings.render(JSON.stringify({id:0,type:'#root',children:[{id:1,type:'Page',children:[{id:2,type:'Slot',props:{name:'body'},children:[
{id:3,type:'TextInput',props:{label:'Name',value:name,maxLength:5,onChange:'3:change'}},
{id:4,type:'Textarea',props:{label:'Note',rows:3,value:note,onChange:'4:change'}},
{id:5,type:'Text',children:[{id:6,type:'#text',text:'Name='+name+' Note='+JSON.stringify(note)}]}]}]}]}));}
globalThis.__wingsEvent=(handler,json)=>{const v=JSON.parse(json).value;if(handler==='3:change')name=v;else note=v;render();return true;};render();
"#;

/// A page that renders each value it hears a while later, as a busy page does.
const SLOW_FORM: &str = r#"
let name = '';
function render(){wings.render(JSON.stringify({id:0,type:'#root',children:[{id:1,type:'Page',children:[{id:2,type:'Slot',props:{name:'body'},children:[
{id:3,type:'TextInput',props:{label:'Name',value:name,onChange:'3:change'}},
{id:5,type:'Text',children:[{id:6,type:'#text',text:'Name='+name+'.'}]}]}]}]}));}
globalThis.__wingsEvent=(_,json)=>{const v=JSON.parse(json).value;setTimeout(()=>{name=v;render();},60);return true;};render();
"#;

#[test]
fn a_page_that_gives_typed_text_back_late_does_not_undo_the_keys_typed_since() {
    let (app, mut window) = open(SLOW_FORM);
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
    let (app, mut window) = open(FORM);
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

#[test]
fn search_field_can_be_reached_with_tab() {
    let (app, mut window) = open(SEARCH);
    settle(&app, &mut window, shows("Query="));
    window.update(|_, window, cx| window.focus_next(cx));
    window.draw();
    window.simulate_input("coupon");
    settle(&app, &mut window, shows("Query=coupon"));
}

#[test]
fn built_changes_sidebar_supports_keyboard_chips_and_live_history_search_in_quickjs() {
    let root = Root::new();
    let host = Arc::new(PluginHost::new(root.config("changes")));
    host.start_main(
        include_str!("../../../../plugins/changes/preview/native-host.js"),
        |_| {},
    )
    .unwrap();
    let mut app = TestApp::new();
    app.background_executor().allow_parking();
    let page_host = host.clone();
    let mut window = app.open_window(move |_, cx| {
        PageView::in_plugin(
            &page_host,
            include_str!("../../../../plugins/changes/dist/native/sidebar.js"),
            cx,
        )
    });
    settle(&app, &mut window, shows("Coupon.tsx"));
    // A sidebar doesn't fill, so it sits inside the desktop's 14pt of padding.
    window.simulate_click(point(px(59.0), px(92.0)), MouseButton::Left);
    // Arrow through Branch to History. Focus must follow the selected chip.
    key(&mut window, "right");
    settle(&app, &mut window, shows("against main"));
    key(&mut window, "right");
    settle(&app, &mut window, shows("Add coupon validation"));
    window.simulate_click(point(px(84.0), px(130.0)), MouseButton::Left);
    window.simulate_input("not-a-commit");
    settle(&app, &mut window, shows("No commits match."));
    assert!(window.read(|view, _| view.error().is_none()));
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
    let (app, mut window) = open(GRAPH);
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

type View = TestAppWindow<PageView>;

/// Two buttons, one in the top left corner of the page and one in the top right, and the presses each has heard.
/// `PROPS` is what `mount` puts on the root.
macro_rules! corners {
    ($props:literal) => {
        concat!(
            "let left = 0, right = 0;\n",
            "function render(){wings.render(JSON.stringify({id:0,type:'#root',props:",
            $props,
            ",children:[{id:1,type:'Row',props:{justify:'between'},children:[",
            "{id:2,type:'Button',props:{onClick:'2:click'},children:[{id:3,type:'#text',text:'Left'}]},",
            "{id:4,type:'Button',props:{onClick:'4:click'},children:[{id:5,type:'#text',text:'Right'}]}]},",
            "{id:6,type:'Text',children:[{id:7,type:'#text',text:'Pressed '+left+' '+right+'.'}]}]}));}\n",
            "globalThis.__wingsEvent=(handler)=>{if(handler==='2:click')left++;else right++;render();return true;};render();",
        )
    };
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
    let (app, mut window) = open(corners!("{fill:true}"));
    settle(&app, &mut window, shows("Pressed 0 0."));
    // 2pt in from either top corner is already on a button: nothing pads the page, and it spans the window's width.
    click_corners(&app, &mut window, 2.0);
    settle(&app, &mut window, shows("Pressed 1 1."));
}

#[test]
fn a_page_that_does_not_fill_keeps_the_desktops_14pt_padding() {
    let (app, mut window) = open(corners!("{}"));
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
        let mut window = app.open_window(move |_, cx| {
            if popover {
                PageView::new_popover(corners!("{}"), cx)
            } else {
                PageView::new(corners!("{}"), cx)
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
const FOCUS_BELOW: &str = r#"
const lines = Array.from({length:30}, (_,i)=>({id:10+i,type:'Text',children:[{id:100+i,type:'#text',text:'Line '+i}]}));
wings.render(JSON.stringify({id:0,type:'#root',children:[...lines,{id:5,type:'TextInput',props:{label:'Task',value:'Code review',autoFocus:true}}]}));
"#;

#[test]
fn a_field_that_takes_the_keyboard_below_the_fold_is_scrolled_into_view() {
    let mut app = TestApp::new();
    app.background_executor().allow_parking();
    let bounds = gpui::Bounds::new(point(px(0.0), px(0.0)), gpui::size(px(400.0), px(300.0)));
    let mut window = app.open_window_with_options(
        gpui::WindowOptions {
            window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
        |_, cx| PageView::new(FOCUS_BELOW, cx),
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
const COLUMN: &str = r#"
let left = 0, right = 0;
function render(){wings.render(JSON.stringify({id:0,type:'#root',props:{fill:true},children:[{id:1,type:'Stack',props:{maxWidth:720},children:[
{id:2,type:'Row',props:{justify:'between'},children:[
{id:3,type:'Button',props:{onClick:'3:click'},children:[{id:4,type:'#text',text:'Left'}]},
{id:5,type:'Button',props:{onClick:'5:click'},children:[{id:6,type:'#text',text:'Right'}]}]},
{id:7,type:'Text',children:[{id:8,type:'#text',text:'Pressed '+left+' '+right+'.'}]}]}]}));}
globalThis.__wingsEvent=(handler)=>{if(handler==='3:click')left++;else right++;render();return true;};render();
"#;

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
    let (app, mut window) = open(COLUMN);
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
    let bounds = gpui::Bounds::new(point(px(0.0), px(0.0)), gpui::size(px(640.0), px(400.0)));
    let mut window = app.open_window_with_options(
        gpui::WindowOptions {
            window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
        |_, cx| PageView::new(COLUMN, cx),
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
const OPENED_ROW: &str = r#"
let opened = 0;
function render(){wings.render(JSON.stringify({id:0,type:'#root',children:[{id:1,type:'List',children:[{id:2,type:'ListItem',children:[
{id:3,type:'ListButton',props:{onClick:'3:click','aria-expanded':true},children:[{id:4,type:'Text',children:[{id:5,type:'#text',text:'pnpm test'}]}]},
{id:6,type:'Card',children:[1,2,3].map(i=>({id:10+i,type:'Text',children:[{id:20+i,type:'#text',text:'Detail line '+i}]}))}]}]},
{id:7,type:'Text',children:[{id:8,type:'#text',text:'Opened '+opened+'.'}]}]}));}
globalThis.__wingsEvent=()=>{opened++;render();return true;};render();
"#;

#[test]
fn a_row_opened_to_a_card_keeps_its_whole_line() {
    let (app, mut window) = open(OPENED_ROW);
    settle(&app, &mut window, shows("Opened 0."));
    let width = window.update(|_, window, _| window.viewport_size().width.as_f32());
    // The row's own line reaches the far end: beside it was the card, which took that end and pushed the row down to
    // the card's middle.
    window.simulate_click(point(px(width - 30.0), px(24.0)), MouseButton::Left);
    settle(&app, &mut window, shows("Opened 1."));
}

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
wings.render(JSON.stringify({ id: 0, type: '#root', props: { fill: true }, children: [
  { id: 1, type: 'Page', props: {}, children: [
    { id: 2, type: 'Slot', props: { name: 'body' }, children: [
      { id: 3, type: 'Button', props: { onClick: '3:onClick' }, children: [{ id: 4, type: '#text', text: 'Boom' }] },
      { id: 5, type: 'Hologram', props: {}, children: [{ id: 6, type: '#text', text: 'not drawn' }] },
    ] }] }] }));
"#;

const CHECKBOX: &str = r#"
let checked = false;
const render = () => wings.render(JSON.stringify({ id: 0, type: '#root', props: {}, children: [
  { id: 1, type: 'Page', props: {}, children: [{ id: 2, type: 'Slot', props: { name: 'body' }, children: [
    { id: 3, type: 'Checkbox', props: { checked, onChange: '3:onChange' }, children: [{ id: 4, type: '#text', text: 'Delete branch' }] }
  ] }] }
] }));
globalThis.__wingsEvent = (_handler, payload) => { checked = JSON.parse(payload).checked; render(); return true; };
render();
"#;

const DISABLED_CHECKBOX: &str = r#"
wings.render(JSON.stringify({ id: 0, type: '#root', props: {}, children: [
  { id: 1, type: 'Page', props: {}, children: [{ id: 2, type: 'Slot', props: { name: 'body' }, children: [
    { id: 3, type: 'Checkbox', props: { checked: false, disabled: true, onChange: '3:onChange' }, children: [{ id: 4, type: '#text', text: 'Delete branch' }] }
  ] }] }
] }));
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
fn a_checkbox_is_keyboard_operable_and_its_disabled_state_is_skipped() {
    let (app, mut window) = open(CHECKBOX);
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

    let (disabled_app, mut disabled) = open(DISABLED_CHECKBOX);
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

/// A page in a frame `width` points wide.
fn open_narrow(source: &'static str, width: f32) -> (TestApp, View) {
    let mut app = TestApp::new();
    app.background_executor().allow_parking();
    let bounds = gpui::Bounds::new(point(px(0.0), px(0.0)), gpui::size(px(width), px(400.0)));
    let window = app.open_window_with_options(
        gpui::WindowOptions {
            window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
        move |_, cx| PageView::new(source, cx),
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

/// A chip whose label is longer than its line, as a Cyclops task suggested under its field.
const LONG_CHIP: &str = r#"
let pressed = 0;
function render(){wings.render(JSON.stringify({id:0,type:'#root',children:[
{id:1,type:'Row',props:{gap:'s',wrap:true},children:[
{id:2,type:'Button',props:{chip:true,onClick:'2:click'},children:[{id:3,type:'#text',text:'Report table keeps the Unassigned group last when sorted ascending, on the web and on mobile'}]}]},
{id:4,type:'Text',children:[{id:5,type:'#text',text:'Pressed '+pressed+'.'}]}]}));}
globalThis.__wingsEvent=()=>{pressed++;render();return true;};render();
"#;

#[test]
fn a_chip_longer_than_its_line_ends_at_the_pages_padding() {
    let (app, mut window) = open_narrow(LONG_CHIP, 300.0);
    settle(&app, &mut window, shows("Pressed 0."));
    // The page's 14pt padding on the right is past the chip's end: the chip used to run on through it and off the frame.
    assert!(!pressed_at(&app, &mut window, (293.0, 28.0), "Pressed 1."));
    assert!(pressed_at(&app, &mut window, (280.0, 28.0), "Pressed 1."));
}

/// A row of Properties as wings-plugin-ui/native draws it: a label, and a value that starts with a dot, like Sprint
/// board's "State ● New". The value's text can be pressed.
const DOT_VALUE: &str = r#"
let pressed = 0;
function render(){wings.render(JSON.stringify({id:0,type:'#root',children:[
{id:1,type:'Row',props:{gap:'m',align:'baseline'},children:[
{id:2,type:'Text',props:{tone:'muted',size:'small'},children:[{id:3,type:'#text',text:'State'}]},
{id:4,type:'Text',props:{size:'small',grow:true},children:[{id:5,type:'Row',props:{gap:'s'},children:[
{id:6,type:'Dot',props:{}},
{id:7,type:'Text',props:{onClick:'7:click'},children:[{id:8,type:'#text',text:'New, the state it is in'}]}]}]}]},
{id:9,type:'Text',children:[{id:10,type:'#text',text:'Pressed '+pressed+'.'}]}]}));}
globalThis.__wingsEvent=()=>{pressed++;render();return true;};render();
"#;

#[test]
fn a_value_that_starts_with_a_dot_sits_level_with_its_label() {
    let (app, mut window) = open_narrow(DOT_VALUE, 400.0);
    settle(&app, &mut window, shows("Pressed 0."));
    // 2pt under the row's top, beside the label, is the value's text: the dot's bottom used to be lined up with the
    // label's, which pushed the value down by half a line.
    assert!(pressed_at(&app, &mut window, (150.0, 16.0), "Pressed 1."));
}

/// A disclosure whose summary is longer than its line, as Sprint board's "What was detected", with a button under it.
const LONG_SUMMARY: &str = r#"
let pressed = 0;
function render(){wings.render(JSON.stringify({id:0,type:'#root',children:[
{id:1,type:'Disclosure',children:[{id:2,type:'Slot',props:{name:'summary'},children:[{id:3,type:'#text',text:'What was detected in the repos, the remotes and the Azure DevOps organizations this Mac is signed in to (12 calls, 3.4 s)'}]}]},
{id:4,type:'Button',props:{onClick:'4:click'},children:[{id:5,type:'#text',text:'Next'}]},
{id:6,type:'Text',children:[{id:7,type:'#text',text:'Pressed '+pressed+'.'}]}]}));}
globalThis.__wingsEvent=()=>{pressed++;render();return true;};render();
"#;

#[test]
fn a_disclosure_summary_longer_than_its_line_wraps() {
    let (app, mut window) = open_narrow(LONG_SUMMARY, 300.0);
    settle(&app, &mut window, shows("Pressed 0."));
    // The summary takes three lines, so the button under it is this far down; on one line it ran off the frame and the
    // button sat 50pt higher.
    assert!(pressed_at(&app, &mut window, (40.0, 110.0), "Pressed 1."));
}
