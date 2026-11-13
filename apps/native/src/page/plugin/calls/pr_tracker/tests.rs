//! The calls PR tracker makes: what each says when the manifest doesn't grant it, what it does when it does, and the
//! desktop's words for bad arguments. A request to a real server isn't made, since the only URLs a plugin may fetch are https
//! ones: the checks all come before the network.

use std::{sync::Arc, time::Instant};

use serde_json::{Value, json};

use crate::page::{
    plugin::{
        Access, PluginHost,
        testing::{Frame, Rig, Seen, WAIT, plugin, show},
    },
    runtime::fake::{self, Cx, Guest},
};

const API: &str = "https://api.example.test/v1/";

fn banked(builtin: bool) -> Rig {
    Rig::with(
        plugin(json!({
            "name": "Probe",
            "permissions": {
                "fetch": [format!("GET {API}"), "GET https://other.example.test/"],
                "secrets": { "bank": API },
                "post": ["gh repos/*/*/pulls/*/comments/*/replies"],
                "sendToAgents": true,
            },
            "contributes": { "settings": { "page": "dist/settings.html" } },
        })),
        builtin,
        vec![],
    )
}

#[test]
fn fetch_says_what_the_desktop_says_before_it_reaches_for_the_network() {
    let rig = banked(false);
    // What `wings.fetch(url, options)` sends: only the options that were given.
    let fetch = |url: &str, options: Value| {
        let mut params = json!({ "url": url });
        params.as_object_mut().unwrap().extend(options.as_object().unwrap().clone());
        params
    };
    assert_eq!(
        rig.refuses("fetch", fetch("https://example.com/x", json!({}))),
        "probe may not GET https://example.com/x"
    );
    assert_eq!(
        rig.refuses("fetch", fetch(&format!("{API}x"), json!({ "method": "post", "body": "{}" }))),
        format!("probe may not POST {API}x")
    );
    assert_eq!(
        rig.refuses("fetch", fetch(&format!("{API}x"), json!({ "headers": { "Host": "evil.test" } }))),
        "probe may not set the Host header"
    );
    // A secret goes only under the prefix the manifest binds it to.
    assert_eq!(
        rig.refuses("fetch", fetch("https://other.example.test/x", json!({ "bearer": "bank" }))),
        "probe may not send the bank secret to https://other.example.test/x"
    );
    assert_eq!(
        rig.refuses("fetch", fetch(&format!("{API}x"), json!({ "bearer": "bank" }))),
        "no secret named bank"
    );
}

#[test]
fn a_secret_is_kept_under_the_plugin_and_its_scope_and_never_read_back() {
    let rig = banked(false);
    let name = |name: &str| json!({ "name": name });
    assert_eq!(rig.allows("secretHas", name("bank")), json!(false));
    rig.allows("secretSet", json!({ "name": "bank", "value": "abc" }));
    assert_eq!(rig.allows("secretHas", name("bank")), json!(true));
    let scope = rig.root.config("probe").scope;
    let key = ("probe".to_owned(), scope, "bank".to_owned());
    assert_eq!(
        rig.env
            .secrets
            .lock()
            .unwrap()
            .get(&key)
            .map(String::as_str),
        Some("abc")
    );
    rig.allows("secretDelete", name("bank"));
    assert_eq!(rig.allows("secretHas", name("bank")), json!(false));
    assert!(rig.env.secrets.lock().unwrap().is_empty());
    assert_eq!(
        rig.refuses("secretSet", json!({ "name": "elsewhere", "value": "abc" })),
        "probe has no secret named elsewhere in permissions.secrets"
    );
    for value in ["".to_owned(), "x".repeat(8193)] {
        assert_eq!(
            rig.refuses("secretSet", json!({ "name": "bank", "value": value })),
            "A secret needs a name of letters, digits, - and _, and a value up to 8 KB"
        );
    }
}

#[test]
fn a_write_through_a_cli_needs_the_manifest_to_list_the_path() {
    let rig = banked(false);
    assert_eq!(
        rig.refuses("post", json!({ "program": "gh", "url": "repos/o/r/pulls/1/reviews", "fields": { "body": "x" } })),
        "probe may not post to gh repos/o/r/pulls/1/reviews"
    );
    assert_eq!(
        rig.refuses("post", json!({ "program": "gh", "url": "repos/o/r/pulls/1/comments/2/replies", "fields": {} })),
        "a post needs 1 to 10 fields"
    );
}

#[test]
fn a_message_reaches_the_agent_in_a_pane_with_the_plugins_name_when_the_plugin_may() {
    let rig = banked(false);
    assert_eq!(
        rig.allows("sendToAgent", json!({ "paneId": "3", "text": "  Checks failed.\nLook at them.  " })),
        json!({ "submitted": false })
    );
    assert_eq!(
        rig.env.seen(),
        [Seen::SendToAgent(
            "3".into(),
            "(Message from the Probe plugin, sent through Wings.)\nChecks failed.\nLook at them."
                .into()
        )]
    );
    for (text, why) in [
        ("".to_owned(), "sendToAgent needs a message"),
        ("  ".to_owned(), "sendToAgent needs a message"),
        ("x".repeat(8193), "A message is up to 8 KB"),
        (
            "a\u{1b}[201~b".to_owned(),
            "A message can't hold control characters other than newlines and tabs",
        ),
    ] {
        assert_eq!(
            rig.refuses("sendToAgent", json!({ "paneId": "3", "text": text })),
            why
        );
    }
    let rig = Rig::new(json!({}), vec![]);
    assert_eq!(
        rig.refuses("sendToAgent", json!({ "paneId": "3", "text": "hi" })),
        "probe may not send messages to agents"
    );
    assert!(rig.env.seen().is_empty());
}

#[test]
fn the_settings_page_opens_only_for_a_plugin_that_has_one() {
    assert_eq!(
        banked(false).allows("openSettings", json!({})),
        json!(null)
    );
    let rig = Rig::new(json!({}), vec![]);
    assert_eq!(
        rig.refuses("openSettings", json!({})),
        "probe has no settings page. Add contributes.settings to its manifest"
    );
    assert!(rig.env.seen().is_empty());
}

/// A main that keeps the last plugin event it hears, in its storage.
struct Heard;

impl Guest for Heard {
    fn deliver(&mut self, cx: &mut Cx<'_>, kind: &str, _id: u32, text: &str) {
        if kind == "pluginEvent" {
            cx.link().storage(crate::page::runtime::Store::Set("heard".into(), text.into())).unwrap();
        }
    }
}

/// A plugin `listener` that keeps the last event its main heard, in its storage.
fn listener(rig: &Rig, builtin: bool, events: &[&str]) -> Arc<PluginHost> {
    let listener = plugin(json!({ "id": "listener", "permissions": { "events": events } }));
    let host = Arc::new(PluginHost::with_access(
        rig.root.config("listener"),
        Access {
            plugin: listener.clone(),
            builtin,
            env: rig.env.clone(),
        },
    ));
    host.start_main(&fake::source("main", || Box::new(Heard)), |_| {}).unwrap();
    rig.env.running.add(&listener, builtin, &host);
    host
}

fn heard(rig: &Rig) -> Option<serde_json::Value> {
    let config = rig.root.config("listener");
    let deadline = Instant::now() + std::time::Duration::from_millis(1500);
    loop {
        // A storage keeps the key list it first read, so each look needs a new one to see a later write.
        let storage = wings_plugin_storage::Storage::new(config.root.clone());
        if let Some(value) = storage.get("listener", &config.scope, "heard").unwrap() {
            return Some(value);
        }
        if Instant::now() > deadline {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn an_event_reaches_the_main_of_each_builtin_plugin_that_declared_it() {
    let rig = Rig::with(plugin(json!({})), true, vec![]);
    rig.env.running.add(
        &plugin(json!({})),
        true,
        &Arc::new(PluginHost::new(rig.root.config("probe"))),
    );
    let _listener = listener(&rig, true, &["probe pr-state"]);
    assert_eq!(
        rig.allows("emit", json!({ "name": "pr-state", "data": { "state": "merged" } })),
        json!(null)
    );
    assert_eq!(
        heard(&rig),
        Some(json!({ "from": "probe", "name": "pr-state", "data": { "state": "merged" } }))
    );
}

#[test]
fn an_event_is_dropped_for_a_plugin_that_did_not_declare_it_and_by_one_that_is_not_wings_own() {
    let rig = Rig::with(plugin(json!({})), true, vec![]);
    rig.env.running.add(
        &plugin(json!({})),
        true,
        &Arc::new(PluginHost::new(rig.root.config("probe"))),
    );
    let _listener = listener(&rig, true, &["probe other-event"]);
    assert_eq!(
        rig.allows("emit", json!({ "name": "pr-state", "data": null })),
        json!(null)
    );
    assert_eq!(heard(&rig), None);

    let stranger = Rig::with(plugin(json!({})), false, vec![]);
    stranger.env.running.add(
        &plugin(json!({})),
        false,
        &Arc::new(PluginHost::new(stranger.root.config("probe"))),
    );
    let _listener = listener(&stranger, true, &["probe pr-state"]);
    assert_eq!(
        stranger.allows("emit", json!({ "name": "pr-state", "data": null })),
        json!(null)
    );
    assert_eq!(heard(&stranger), None);
}

#[test]
fn an_event_has_a_plain_name_and_up_to_16_kb() {
    let rig = Rig::with(plugin(json!({})), true, vec![]);
    for name in ["", "Upper", "-x", "a_b", &"x".repeat(65)] {
        assert_eq!(
            rig.refuses("emit", json!({ "name": name, "data": null })),
            "emit takes an event name of lowercase letters, digits and dashes",
            "{name}"
        );
    }
    assert_eq!(
        rig.refuses("emit", json!({ "name": "big", "data": "x".repeat(16 * 1024) })),
        "emit data is up to 16 KB of JSON"
    );
}

/// A page that shows what its tab was opened with, as the module's `view_args` import hands it over.
struct ViewArgs;

impl Guest for ViewArgs {
    fn start(&mut self, cx: &mut Cx<'_>) {
        let args = cx.link().view_args().map_or(Value::Null, |args| serde_json::from_str(&args).unwrap());
        show(cx, json!({ "args": args }));
    }
}

#[test]
fn a_page_reads_what_openview_opened_it_with_as_a_copy_and_a_page_without_args_reads_null() {
    let rig = Rig::new(json!({}), vec![]);
    let source = fake::source("view", || Box::new(ViewArgs));
    let args = json!({ "url": "https://example.test/pull/1", "number": 1 });
    let (sender, events) = std::sync::mpsc::channel();
    let page = rig
        .host
        .open_hidden_page_with_args(&source, Some(&args), move |event| {
            let _ = sender.send(event);
        })
        .unwrap();
    page.set_visible(true);
    let tree = loop {
        match events.recv_timeout(WAIT).expect("the page to show") {
            crate::page::runtime::Event::Tree(tree) => break tree,
            _ => continue,
        }
    };
    let shown: serde_json::Value = serde_json::from_str(&tree.text()).unwrap();
    assert_eq!(shown["args"], args);
    assert_eq!(
        Frame::start(&rig.host, &source).report("a page", |r| r.get("args").is_some()),
        json!({ "args": null })
    );
}
