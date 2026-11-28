//! A plugin tab's page takes the keyboard's shortcuts and font keys while its tab has the keyboard, and only then.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use gpui::{TestApp, TestAppWindow};
use serde_json::json;

use super::{
    Active, Workspace,
    plugin_tests::open_plugins,
};
use crate::{
    installed::fixture::Data,
    page::runtime::fake,
    terminal_view::font_size,
};

/// A page with a field and a button that answers to Cmd-Shift-P, saying how often it was clicked, what was typed and
/// which text steps it heard. With `scales`, its `Page` sizes its own text.
fn keyed_page(scales: bool) -> String {
    let (mut clicked, mut typed, mut steps) = (0, String::new(), Vec::<String>::new());
    fake::page("page", move |event| {
        match event {
            Some(("3:onClick", _)) => clicked += 1,
            Some(("5:onChange", change)) => typed = change["value"].as_str().unwrap_or_default().to_owned(),
            Some(("1:onTextScale", step)) => steps.push(step["step"].as_str().unwrap_or_default().to_owned()),
            _ => {}
        }
        let page = if scales { json!({ "onTextScale": "1:onTextScale" }) } else { json!({}) };
        json!({ "id": 0, "type": "#root", "children": [{ "id": 1, "type": "Page", "props": page, "children": [
            { "id": 2, "type": "Slot", "props": { "name": "body" }, "children": [
                { "id": 3, "type": "Button", "props": { "onClick": "3:onClick", "shortcut": "cmd-shift-p", "label": "New chat" },
                    "children": [{ "id": 4, "type": "#text", "text": "New" }] },
                { "id": 5, "type": "TextInput", "props": { "label": "Message", "value": typed, "onChange": "5:onChange" } },
                { "id": 6, "type": "Text", "children": [{ "id": 7, "type": "#text",
                    "text": format!("Clicked {clicked}, typed {typed:?}, steps {}.", steps.join(" ")) }] }] }] }] })
    })
}

/// A workspace with the plugin tab of `keyed_page(scales)` open and drawn, its tab holding the keyboard.
fn open_keyed(data: &Data, scales: bool) -> (TestApp, TestAppWindow<Workspace>) {
    let views = json!([{ "id": "chat", "title": "Chat", "page": "dist/chat.html", "opens": ["tab"] }]);
    data.install_script("chatty", json!({ "contributes": { "views": views } }), &fake::idle("main"),
        &[("chat", &keyed_page(scales))], "chatty-scope");
    let (app, mut window) = open_plugins(data, &Arc::default(), |_| {});
    window.update(|w, window, cx| assert!(w.open_plugin_view("chatty", "chat", None, window, cx)));
    until_shows(&app, &mut window, "Clicked 0");
    let on_tab = window.update(|w, window, _| w.plugin_tabs[0].focus.is_focused(window));
    assert!(on_tab, "the tab has the keyboard once it opens");
    (app, window)
}

fn page_text(window: &TestAppWindow<Workspace>) -> String {
    window.read(|w, cx| w.plugin_tabs[0].page.read(cx).text())
}

fn field(window: &TestAppWindow<Workspace>) -> gpui::FocusHandle {
    window.read(|w, cx| w.plugin_tabs[0].page.read(cx).input_focus("Message", cx)).expect("the field")
}

/// Runs the window until the page shows `text`. The page runs on its own thread, so this waits in real time.
fn until_shows(app: &TestApp, window: &mut TestAppWindow<Workspace>, text: &str) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !page_text(window).contains(text) {
        assert!(Instant::now() < deadline, "waited for {text:?}, the page shows {:?}", page_text(window));
        app.run_until_parked();
        window.draw();
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// What the page shows stays as it is: a key that should not reach it hasn't, after the page had time to hear it.
fn stays(app: &TestApp, window: &mut TestAppWindow<Workspace>, text: &str) {
    for _ in 0..20 {
        app.run_until_parked();
        window.draw();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(page_text(window).contains(text), "the page shows {:?}", page_text(window));
}

#[test]
fn a_buttons_shortcut_clicks_it_only_while_its_tab_has_the_keyboard() {
    let data = Data::temp();
    let (app, mut window) = open_keyed(&data, false);
    // The tab itself has the keyboard, so the page isn't on the key's way: the workspace hands it over.
    window.simulate_keystroke("cmd-shift-p");
    until_shows(&app, &mut window, "Clicked 1");
    // In the page's field, plain keys still type, and the shortcut still clicks.
    let message = field(&window);
    window.update(|_, window, cx| message.focus(window, cx));
    window.draw();
    window.simulate_input("p");
    until_shows(&app, &mut window, "typed \"p\"");
    assert!(page_text(&window).contains("Clicked 1"), "a plain key clicked the button");
    window.simulate_keystroke("cmd-shift-p");
    until_shows(&app, &mut window, "Clicked 2");
    // With a terminal in front, the key isn't the page's.
    window.update(|w, window, cx| {
        let terminal = w.terminals[0].id;
        w.show(Active::Terminal(terminal), window, cx);
    });
    window.draw();
    window.simulate_keystroke("cmd-shift-p");
    stays(&app, &mut window, "Clicked 2");
}

#[test]
fn the_font_keys_size_a_pages_own_text_while_it_has_the_keyboard_and_the_terminals_otherwise() {
    let data = Data::temp();
    let (app, mut window) = open_keyed(&data, true);
    let (bigger, smaller, reset) = if cfg!(target_os = "macos") {
        ("cmd-=", "cmd--", "cmd-0")
    } else {
        ("ctrl-shift-=", "ctrl-shift--", "ctrl-shift-0")
    };
    for key in [bigger, smaller, reset] {
        window.simulate_keystroke(key);
    }
    until_shows(&app, &mut window, "steps bigger smaller reset.");
    assert_eq!(window.read(|_, cx| font_size(cx)), 13.0, "the terminals' font changed");
    // From inside the page, too.
    let message = field(&window);
    window.update(|_, window, cx| message.focus(window, cx));
    window.draw();
    window.simulate_keystroke(bigger);
    until_shows(&app, &mut window, "steps bigger smaller reset bigger.");
    assert_eq!(window.read(|_, cx| font_size(cx)), 13.0, "the terminals' font changed");
    // A terminal in front sizes the terminals.
    window.update(|w, window, cx| {
        let terminal = w.terminals[0].id;
        w.show(Active::Terminal(terminal), window, cx);
    });
    window.draw();
    window.simulate_keystroke(bigger);
    assert_eq!(window.read(|_, cx| font_size(cx)), 14.0);
    stays(&app, &mut window, "steps bigger smaller reset bigger.");
}

#[test]
fn a_page_that_does_not_size_its_text_leaves_the_font_keys_to_the_terminals() {
    let data = Data::temp();
    let (_app, mut window) = open_keyed(&data, false);
    window.simulate_keystroke(if cfg!(target_os = "macos") { "cmd-=" } else { "ctrl-shift-=" });
    assert_eq!(window.read(|_, cx| font_size(cx)), 14.0);
}

/// Each key the app binds for the whole window is one a page's button can't take, so `tree::RESERVED` keeps up with
/// `actions.rs`.
#[test]
fn no_page_shortcut_takes_a_key_the_app_binds() {
    let mut app = TestApp::new();
    app.update(crate::actions::bind);
    app.update(|cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        for binding in keymap.bindings().filter(|binding| binding.predicate().is_none()) {
            let stroke = &binding.keystrokes()[0];
            let mods = stroke.modifiers();
            let flag = |on: bool, name: &'static str| if on { name } else { "" };
            let written = format!(
                "{}{}{}{}{}",
                flag(mods.control, "ctrl-"),
                flag(mods.alt, "alt-"),
                flag(mods.shift, "shift-"),
                flag(mods.platform, "cmd-"),
                stroke.key()
            );
            assert_eq!(crate::page::tree::Shortcut::parse(&written), None, "a page could take {written}");
        }
    });
}

/// A page that takes Cmd-T and Cmd-L for itself and says which it heard.
fn claiming_page() -> String {
    let mut heard = Vec::<String>::new();
    fake::page("page", move |event| {
        if let Some(("1:onShortcut", key)) = event {
            heard.push(key["key"].as_str().unwrap_or_default().to_owned());
        }
        json!({ "id": 0, "type": "#root", "children": [{ "id": 1, "type": "Page",
            "props": { "shortcuts": ["cmd-t", "cmd-l"], "onShortcut": "1:onShortcut" }, "children": [
            { "id": 2, "type": "Slot", "props": { "name": "body" }, "children": [
                { "id": 6, "type": "Text", "children": [{ "id": 7, "type": "#text", "text": format!("Heard {}.", heard.join(" ")) }] }] }] }] })
    })
}

/// A page that names a key Wings binds (Cmd-T) gets it before Wings' own action, so it does not open a terminal tab; a key
/// it doesn't name (Cmd-N, Cmd-D) still goes where it did; and with a terminal in front the page takes nothing.
#[test]
fn a_page_takes_the_keys_it_names_before_wings_acts_on_them_while_it_has_the_keyboard() {
    let data = Data::temp();
    let views = json!([{ "id": "chat", "title": "Chat", "page": "dist/chat.html", "opens": ["tab"] }]);
    data.install_script("chatty", json!({ "contributes": { "views": views } }), &fake::idle("main"), &[("chat", &claiming_page())], "chatty-scope");
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    window.update(|w, window, cx| assert!(w.open_plugin_view("chatty", "chat", None, window, cx)));
    until_shows(&app, &mut window, "Heard .");
    let tabs_before = window.read(|w, _| w.terminals.len());
    window.simulate_keystroke("cmd-t");
    until_shows(&app, &mut window, "Heard cmd-t.");
    window.simulate_keystroke("cmd-l");
    until_shows(&app, &mut window, "Heard cmd-t cmd-l.");
    assert_eq!(window.read(|w, _| w.terminals.len()), tabs_before, "Cmd-T opened no terminal tab");
    // With a terminal in front the page isn't asked, and Wings opens its tab as before.
    window.update(|w, window, cx| {
        let terminal = w.terminals[0].id;
        w.show(Active::Terminal(terminal), window, cx);
    });
    window.draw();
    window.simulate_keystroke("cmd-t");
    stays(&app, &mut window, "Heard cmd-t cmd-l.");
    assert_eq!(window.read(|w, _| w.terminals.len()), tabs_before + 1, "Wings opened its own tab");
}

/// A key pressed in the page's own web page is the page's even when Wings' keyboard is elsewhere, as after the browser tab
/// whose field had it closed; a key from no web page still needs the page to have the keyboard.
#[test]
fn a_key_from_the_pages_web_page_reaches_it_wherever_wings_keyboard_was() {
    let data = Data::temp();
    let views = json!([{ "id": "chat", "title": "Chat", "page": "dist/chat.html", "opens": ["tab"] }]);
    data.install_script("chatty", json!({ "contributes": { "views": views } }), &fake::idle("main"), &[("chat", &claiming_page())], "chatty-scope");
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    window.update(|w, window, cx| assert!(w.open_plugin_view("chatty", "chat", None, window, cx)));
    until_shows(&app, &mut window, "Heard .");
    window.update(|_, window, cx| window.blur(cx));
    window.draw();
    window.simulate_keystroke("cmd-l");
    stays(&app, &mut window, "Heard .");
    let page = window.read(|w, _| w.plugin_tabs[0].page.entity_id());
    crate::web::key_from(page);
    window.simulate_keystroke("cmd-l");
    until_shows(&app, &mut window, "Heard cmd-l.");
}
