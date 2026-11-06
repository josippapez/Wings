//! The calls PR tracker makes: what each says when the manifest doesn't grant it, what it does when it does, and the
//! desktop's words for bad arguments. A request to a real server isn't made, since the only URLs a plugin may fetch are https
//! ones: the checks all come before the network.

use std::{sync::Arc, time::Instant};

use serde_json::json;

use crate::page::plugin::{
    Access, PluginHost,
    testing::{Frame, PRELUDE, Rig, Seen, WAIT, plugin},
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
    assert_eq!(
        rig.refused("return await wings.fetch('https://example.com/x');"),
        "probe may not GET https://example.com/x"
    );
    assert_eq!(
        rig.refused(&format!(
            "return await wings.fetch('{API}x', {{ method: 'post', body: '{{}}' }});"
        )),
        format!("probe may not POST {API}x")
    );
    assert_eq!(
        rig.refused(&format!(
            "return await wings.fetch('{API}x', {{ headers: {{ Host: 'evil.test' }} }});"
        )),
        "probe may not set the Host header"
    );
    // A secret goes only under the prefix the manifest binds it to.
    assert_eq!(
        rig.refused(
            "return await wings.fetch('https://other.example.test/x', { bearer: 'bank' });"
        ),
        "probe may not send the bank secret to https://other.example.test/x"
    );
    assert_eq!(
        rig.refused(&format!(
            "return await wings.fetch('{API}x', {{ bearer: 'bank' }});"
        )),
        "no secret named bank"
    );
}

#[test]
fn a_secret_is_kept_under_the_plugin_and_its_scope_and_never_read_back() {
    let rig = banked(false);
    assert_eq!(
        rig.allowed("return await wings.secrets.has('bank');"),
        json!(false)
    );
    assert_eq!(
        rig.allowed(
            "await wings.secrets.set('bank', 'abc'); return await wings.secrets.has('bank');"
        ),
        json!(true)
    );
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
    assert_eq!(
        rig.allowed("await wings.secrets.delete('bank'); return await wings.secrets.has('bank');"),
        json!(false)
    );
    assert!(rig.env.secrets.lock().unwrap().is_empty());
    assert_eq!(
        rig.refused("return await wings.secrets.set('elsewhere', 'abc');"),
        "probe has no secret named elsewhere in permissions.secrets"
    );
    for value in ["''", "'x'.repeat(8193)"] {
        assert_eq!(
            rig.refused(&format!("return await wings.secrets.set('bank', {value});")),
            "A secret needs a name of letters, digits, - and _, and a value up to 8 KB"
        );
    }
}

#[test]
fn a_write_through_a_cli_needs_the_manifest_to_list_the_path() {
    let rig = banked(false);
    assert_eq!(
        rig.refused("return await wings.post('gh', 'repos/o/r/pulls/1/reviews', { body: 'x' });"),
        "probe may not post to gh repos/o/r/pulls/1/reviews"
    );
    assert_eq!(
        rig.refused("return await wings.post('gh', 'repos/o/r/pulls/1/comments/2/replies', {});"),
        "a post needs 1 to 10 fields"
    );
}

#[test]
fn a_message_reaches_the_agent_in_a_pane_with_the_plugins_name_when_the_plugin_may() {
    let rig = banked(false);
    assert_eq!(
        rig.allowed("return await wings.sendToAgent('3', '  Checks failed.\\nLook at them.  ');"),
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
        ("''", "sendToAgent needs a message"),
        ("'  '", "sendToAgent needs a message"),
        ("'x'.repeat(8193)", "A message is up to 8 KB"),
        (
            "'a\\u001b[201~b'",
            "A message can't hold control characters other than newlines and tabs",
        ),
    ] {
        assert_eq!(
            rig.refused(&format!("return await wings.sendToAgent('3', {text});")),
            why
        );
    }
    let rig = Rig::new(json!({}), vec![]);
    assert_eq!(
        rig.refused("return await wings.sendToAgent('3', 'hi');"),
        "probe may not send messages to agents"
    );
    assert!(rig.env.seen().is_empty());
}

#[test]
fn the_settings_page_opens_only_for_a_plugin_that_has_one() {
    assert_eq!(
        banked(false).allowed("return await wings.openSettings();"),
        json!(null)
    );
    let rig = Rig::new(json!({}), vec![]);
    assert_eq!(
        rig.refused("return await wings.openSettings();"),
        "probe has no settings page. Add contributes.settings to its manifest"
    );
    assert!(rig.env.seen().is_empty());
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
    host.start_main(
        "wings.onEvent((event) => wings.storage.set('heard', event));",
        |_| {},
    )
    .unwrap();
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
        rig.allowed("return await wings.emit('pr-state', { state: 'merged' });"),
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
        rig.allowed("return await wings.emit('pr-state', null);"),
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
        stranger.allowed("return await wings.emit('pr-state', null);"),
        json!(null)
    );
    assert_eq!(heard(&stranger), None);
}

#[test]
fn an_event_has_a_plain_name_and_up_to_16_kb() {
    let rig = Rig::with(plugin(json!({})), true, vec![]);
    for name in ["''", "'Upper'", "'-x'", "'a_b'", "'x'.repeat(65)"] {
        assert_eq!(
            rig.refused(&format!("return await wings.emit({name}, null);")),
            "emit takes an event name of lowercase letters, digits and dashes",
            "{name}"
        );
    }
    assert_eq!(
        rig.refused("return await wings.emit('big', 'x'.repeat(16 * 1024));"),
        "emit data is up to 16 KB of JSON"
    );
}

#[test]
fn a_page_reads_what_openview_opened_it_with_as_a_copy_and_a_page_without_args_reads_null() {
    let rig = Rig::new(json!({}), vec![]);
    let source = format!(
        "{PRELUDE}
        const first = wings.viewArgs();
        first.url = 'changed';
        show({{ first, again: wings.viewArgs() }});"
    );
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
    assert_eq!(shown["first"]["url"], "changed");
    assert_eq!(shown["again"], args);
    assert_eq!(
        Frame::start(
            &rig.host,
            &format!("{PRELUDE}\nshow({{ args: wings.viewArgs() }});")
        )
        .report("a page", |r| r.get("args").is_some()),
        json!({ "args": null })
    );
}
