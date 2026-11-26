//! A plugin tab's web pages (`crate::web`): made where the page leaves a place for one, shown only while that place is
//! on screen with nothing over it, telling the page where they are, and gone with the tab.

use std::sync::Arc;

use gpui::{TestApp, TestAppWindow};
use serde_json::{Value, json};

use super::{
    Active, Workspace,
    plugin_tests::{open_plugins, until},
    tests::press,
};
use crate::{
    installed::fixture::Data,
    page::runtime::fake::{self, Cx, Hear, idle},
    web::{Heard, State, testing::{Did, record}},
};

/// A page that is a `WebView` filling it, and a `Dialog` over it with `dialog`, which saves what its web view tells it.
fn web_page(dialog: bool) -> String {
    fake::script("web", move || {
        move |cx: &mut Cx<'_>, hear: Hear<'_>| match hear {
            Hear::Start => {
                let mut children = vec![json!({ "id": 1, "type": "WebView", "props": {
                    "name": "main", "url": "https://example.com/", "fill": true, "onState": "1:state",
                }, "children": [] })];
                if dialog {
                    children.push(json!({ "id": 2, "type": "Dialog", "props": { "title": "Add extension" }, "children": [] }));
                }
                cx.render(&json!({ "id": 0, "type": "#root", "props": {}, "children": children }));
            }
            Hear::Event("1:state", payload) => cx.save("state", payload),
            _ => {}
        }
    })
}

fn install(data: &Data, id: &str, permissions: Value, dialog: bool) {
    let views = json!([{ "id": "web", "title": "Web", "page": "dist/web.html", "opens": ["tab"], "multiple": true }]);
    data.install_script(
        id,
        json!({ "permissions": permissions, "contributes": { "views": views } }),
        &idle("main"),
        &[("web", &web_page(dialog))],
        &format!("{id}-scope"),
    );
}

fn settle(app: &TestApp, window: &mut TestAppWindow<Workspace>) {
    for _ in 0..5 {
        app.run_until_parked();
        window.draw();
    }
}

/// Where the web view was last put: shown at a place, hidden, or never made.
fn last_place(did: &[Did]) -> Option<bool> {
    did.iter().rev().find_map(|did| match did {
        Did::Placed(at) => Some(at.is_some()),
        _ => None,
    })
}

#[test]
fn a_tabs_web_page_shows_at_its_place_hides_with_the_tab_and_ends_with_it() {
    let data = Data::temp();
    install(&data, "browser", json!({ "webView": true }), false);
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    let id = window.update(|_, window, _| window.window_handle().window_id());
    let (did, tells) = record(id);
    let tab = window.update(|w, window, cx| {
        assert!(w.open_plugin_view("browser", "web", None, window, cx));
        w.plugin_tabs.iter().find(|tab| tab.plugin == "browser").unwrap().id
    });
    until(&app, &mut window, "the web page was never made", |_| !did.borrow().is_empty());
    settle(&app, &mut window);
    {
        let did = did.borrow();
        assert_eq!(
            did[0],
            Did::Made { name: Some("browser".into()), keep_data: true, url: Some("https://example.com/".into()) }
        );
        let shown = did.iter().find_map(|did| match did {
            Did::Placed(Some(at)) => Some(*at),
            _ => None,
        });
        let shown = shown.expect("shown at its place");
        assert!(f32::from(shown.size.width) > 300.0 && f32::from(shown.size.height) > 200.0, "it fills the tab: {shown:?}");
    }
    // Where it is reaches the page's `onState`, as WebKit tells it.
    let state = State { url: "https://example.com/docs".into(), title: "Docs".into(), can_go_back: true, ..State::default() };
    tells.borrow()[0].tell(Heard::State(state));
    until(&app, &mut window, "the page never heard where its web view is", |_| {
        data.saved("browser", "browser-scope", "state")
            .is_some_and(|state| state["url"] == "https://example.com/docs" && state["canGoBack"] == true)
    });

    // Another tab in front: the web page hides, as it would draw over that tab.
    press(&mut window, "t");
    settle(&app, &mut window);
    assert!(matches!(window.read(|w, _| w.active), Active::Terminal(_)));
    assert_eq!(last_place(&did.borrow()), Some(false), "hidden behind another tab");
    window.update(|w, window, cx| w.show(Active::Plugin(tab), window, cx));
    settle(&app, &mut window);
    assert_eq!(last_place(&did.borrow()), Some(true), "back with its tab");

    window.update(|w, window, cx| {
        let index = w.plugin_tabs.iter().position(|t| t.id == tab).unwrap();
        w.close_plugin_tab(index, window, cx);
    });
    settle(&app, &mut window);
    until(&app, &mut window, "the web page outlived its tab", |_| did.borrow().last() == Some(&Did::Ended));
}

#[test]
fn a_web_page_is_never_made_without_the_permission_or_under_the_pages_own_dialog() {
    let data = Data::temp();
    install(&data, "nope", json!({ "openUrl": ["https://"] }), false);
    install(&data, "covered", json!({ "webView": true }), true);
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    let id = window.update(|_, window, _| window.window_handle().window_id());
    let (did, _) = record(id);
    for plugin in ["nope", "covered"] {
        window.update(|w, window, cx| assert!(w.open_plugin_view(plugin, "web", None, window, cx)));
        let sent = |window: &mut TestAppWindow<Workspace>| {
            window.read(|w, cx| w.plugin_tabs.iter().any(|tab| tab.plugin == plugin && tab.page.read(cx).trees_sent() > 0))
        };
        for _ in 0..100 {
            if sent(&mut window) {
                break;
            }
            settle(&app, &mut window);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(sent(&mut window), "{plugin}'s page never sent its tree");
        settle(&app, &mut window);
    }
    assert!(did.borrow().is_empty(), "nothing made: {:?}", did.borrow());
}

/// A page that opens as a window with `args`, saves them, shows a web view at their `url`, and closes its own window when
/// its web view says it went somewhere: how the Browser pops out and goes back.
fn window_page() -> String {
    fake::script("pop", || {
        |cx: &mut Cx<'_>, hear: Hear<'_>| match hear {
            Hear::Start => {
                let args: Value = cx.link().view_args().and_then(|args| serde_json::from_str(&args).ok()).unwrap_or_default();
                cx.save("args", &args);
                let url = args["url"].clone();
                cx.render(&json!({ "id": 0, "type": "#root", "props": {}, "children": [{ "id": 1, "type": "WebView", "props": {
                    "name": "main", "url": url, "fill": true, "onState": "1:state",
                }, "children": [] }] }));
            }
            Hear::Event("1:state", _) => {
                cx.call("closeTab", json!({}));
            }
            _ => {}
        }
    })
}

#[test]
fn a_view_opens_in_a_window_of_its_own_with_its_args_shows_its_web_page_there_and_closes_it() {
    let data = Data::temp();
    let views = json!([{ "id": "web", "title": "Web", "page": "dist/web.html", "opens": ["tab", "window"], "multiple": true }]);
    data.install_script(
        "browser",
        json!({ "permissions": { "webView": true }, "contributes": { "views": views } }),
        &idle("main"),
        &[("web", &window_page())],
        "browser-scope",
    );
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    // The Browser's Pop out: the same address, in a window (`openView` with `as: "window"` and `args`).
    let args = json!({ "url": "https://example.com/docs" });
    window.update(|w, _, cx| w.open_plugin_window("browser", "web", Some(&args), None, cx).map(|_| ())).unwrap();
    until(&app, &mut window, "the window's page never had its args", |_| {
        data.saved("browser", "browser-scope", "args") == Some(json!({ "url": "https://example.com/docs" }))
    });
    let handle = window.read(|w, _| *w.plugin_ui.windows.values().next().unwrap());
    let id = handle.window_id();
    let (did, tells) = record(id);
    for _ in 0..50 {
        window.update(|_, _, cx| drop(handle.update(cx, |_, window, _| window.refresh())));
        settle(&app, &mut window);
        if last_place(&did.borrow()) == Some(true) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(did.borrow()[0], Did::Made { name: Some("browser".into()), keep_data: true, url: Some("https://example.com/docs".into()) });
    assert_eq!(last_place(&did.borrow()), Some(true), "shown in its window: {:?}", did.borrow());
    // The page closes its own window (the Browser's Put back, once it has opened the address in a tab).
    tells.borrow()[0].tell(Heard::State(State::default()));
    until(&app, &mut window, "the window stayed open", |w| w.plugin_ui.windows.is_empty());
    until(&app, &mut window, "its web page outlived its window", |_| did.borrow().last() == Some(&Did::Ended));
}

/// A page whose web view runs a script as it opens, which saves what came of it.
fn scripting_page() -> String {
    fake::script("web", || {
        |cx: &mut Cx<'_>, hear: Hear<'_>| match hear {
            Hear::Start => cx.render(&json!({ "id": 0, "type": "#root", "props": {}, "children": [{ "id": 1, "type": "WebView", "props": {
                "name": "main", "url": "https://example.com/", "fill": true, "onResult": "1:result",
                "command": { "action": "eval", "script": "return document.title", "token": 7 },
            }, "children": [] }] })),
            Hear::Event("1:result", payload) => cx.save("result", payload),
            _ => {}
        }
    })
}

#[test]
fn a_page_runs_scripts_in_its_web_pages_only_with_its_own_permission() {
    let data = Data::temp();
    let views = json!([{ "id": "web", "title": "Web", "page": "dist/web.html", "opens": ["tab"], "multiple": true }]);
    for (id, permissions) in [("viewer", json!({ "webView": true })), ("scripter", json!({ "webView": true, "webScripts": true }))] {
        data.install_script(id, json!({ "permissions": permissions, "contributes": { "views": views } }), &idle("main"), &[("web", &scripting_page())], &format!("{id}-scope"));
    }
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    let id = window.update(|_, window, _| window.window_handle().window_id());
    let (did, _) = record(id);
    window.update(|w, window, cx| assert!(w.open_plugin_view("viewer", "web", None, window, cx)));
    until(&app, &mut window, "the page never heard its script was refused", |_| {
        data.saved("viewer", "viewer-scope", "result") == Some(json!({ "token": 7, "error": crate::web::NO_SCRIPTS }))
    });
    settle(&app, &mut window);
    assert!(!did.borrow().iter().any(|did| matches!(did, Did::Went(..))), "nothing ran: {:?}", did.borrow());
    window.update(|w, window, cx| assert!(w.open_plugin_view("scripter", "web", None, window, cx)));
    until(&app, &mut window, "the script never ran", |_| did.borrow().iter().any(|did| matches!(did, Did::Went(crate::page::tree::WebGo::Eval { .. }, 7))));
}

#[test]
fn two_commands_sent_before_the_next_tree_both_run_in_order() {
    let data = Data::temp();
    let views = json!([{ "id": "web", "title": "Web", "page": "dist/web.html", "opens": ["tab"], "multiple": true }]);
    let page = fake::script("web", || {
        |cx: &mut Cx<'_>, hear: Hear<'_>| {
            if let Hear::Start = hear {
                cx.render(&json!({ "id": 0, "type": "#root", "props": {}, "children": [{ "id": 1, "type": "WebView", "props": {
                    "name": "main", "url": "https://example.com/", "fill": true,
                    "command": [
                        { "action": "load", "url": "https://example.com/a", "token": 1 },
                        { "action": "reload", "token": 2 },
                    ],
                }, "children": [] }] }));
            }
        }
    });
    data.install_script("browser", json!({ "permissions": { "webView": true }, "contributes": { "views": views } }), &idle("main"), &[("web", &page)], "browser-scope");
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    let id = window.update(|_, window, _| window.window_handle().window_id());
    let (did, _) = record(id);
    window.update(|w, window, cx| assert!(w.open_plugin_view("browser", "web", None, window, cx)));
    until(&app, &mut window, "the commands never ran", |_| did.borrow().iter().filter(|did| matches!(did, Did::Went(..))).count() == 2);
    let went: Vec<_> = did.borrow().iter().filter_map(|did| match did {
        Did::Went(go, token) => Some((go.clone(), *token)),
        _ => None,
    }).collect();
    use crate::page::tree::WebGo;
    assert_eq!(went, [(WebGo::Load("https://example.com/a".into()), 1), (WebGo::Reload, 2)]);
}

/// A page with an extension's button over its web view: the command to click it goes out as the page opens (with no click
/// of yours), on a press of the button (`token` 2), and again when its web view next tells it where it is (no click).
fn extension_page() -> String {
    fake::script("web", || {
        |cx: &mut Cx<'_>, hear: Hear<'_>| {
            let token = match hear {
                Hear::Start => 1,
                Hear::Event("2:click", _) => 2,
                Hear::Event("1:state", _) => 3,
                Hear::Event("1:result", payload) => {
                    let token = payload["token"].as_u64().unwrap_or_default();
                    return cx.save(&format!("result{token}"), payload);
                }
                _ => return,
            };
            cx.render(&json!({ "id": 0, "type": "#root", "props": {}, "children": [
                { "id": 2, "type": "Button", "props": { "onClick": "2:click" }, "children": [{ "id": 3, "type": "#text", "text": "Bitwarden" }] },
                { "id": 1, "type": "WebView", "props": {
                    "name": "main", "url": "https://example.com/", "fill": true, "onState": "1:state", "onResult": "1:result",
                    "command": { "action": "extension", "id": "e1", "token": token },
                }, "children": [] },
            ] }));
        }
    })
}

/// An extension's popup opens only on your click, as `copyText` copies: a plugin or an agent can't open one alone, and one
/// click opens one.
#[test]
fn an_extensions_button_is_clicked_only_right_after_your_click_and_once_for_it() {
    use crate::page::tree::WebGo;
    let data = Data::temp();
    let views = json!([{ "id": "web", "title": "Web", "page": "dist/web.html", "opens": ["tab"], "multiple": true }]);
    data.install_script("browser", json!({ "permissions": { "webView": true }, "contributes": { "views": views } }), &idle("main"), &[("web", &extension_page())], "browser-scope");
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    let id = window.update(|_, window, _| window.window_handle().window_id());
    let (did, tells) = record(id);
    window.update(|w, window, cx| assert!(w.open_plugin_view("browser", "web", None, window, cx)));
    let refused = |token: u64| Some(json!({ "token": token, "error": crate::web::NO_CLICK }));
    until(&app, &mut window, "the page's own click was never refused", |_| data.saved("browser", "browser-scope", "result1") == refused(1));
    until(&app, &mut window, "the web page was never made", |_| !did.borrow().is_empty());
    let clicked = |did: &[Did]| did.iter().filter_map(|did| if let Did::Went(go, token) = did { Some((go.clone(), *token)) } else { None }).collect::<Vec<_>>();
    assert!(clicked(&did.borrow()).is_empty(), "nothing clicked without you: {:?}", did.borrow());

    window.update(|w, _, cx| assert!(w.plugin_tabs.iter().find(|tab| tab.plugin == "browser").unwrap().page.read(cx).press("Bitwarden", 0)));
    until(&app, &mut window, "your click never reached the extension", |_| !clicked(&did.borrow()).is_empty());
    assert_eq!(clicked(&did.borrow()), [(WebGo::Extension("e1".into()), 2)]);

    // The click is used up: the next one the page sends alone is refused.
    tells.borrow()[0].tell(Heard::State(State::default()));
    until(&app, &mut window, "a second click on one of yours was let through", |_| data.saved("browser", "browser-scope", "result3") == refused(3));
    assert_eq!(clicked(&did.borrow()).len(), 1);
}
