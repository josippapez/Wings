use super::*;
use crate::page::tree::{Node, Tree};
use serde_json::{Value, json};
use std::{
    fs,
    sync::mpsc,
    time::{Duration, Instant},
};

const MAIN: &str = include_str!("../../../pages/dist/fixture-main.js");
const PAGE: &str = include_str!("../../../pages/dist/fixture-page.js");
const BIG_PAGE: &str = include_str!("../../../pages/dist/fixture-big-page.js");
const WAIT: Duration = Duration::from_secs(10);
const SCOPE: &str = "test-scope";

/// A bare page that shows what it heard and what became of what it sent. An event's payload is the message it
/// broadcasts: `{ value }`, or a text that is `chars` JSON characters long, or `emoji` emoji (2 JS string units each).
const PROBE: &str = r#"
const heard = [];
const results = [];
const show = () => wings.render(JSON.stringify({ id: 0, type: '#root', props: {}, children: [
  { id: 1, type: '#text', text: JSON.stringify({ heard, results }) }] }));
wings.onBroadcast((message) => {
  heard.push(typeof message === 'string' && message.length > 50 ? `text of ${message.length}` : message);
  show();
});
globalThis.__wingsEvent = (_handler, payload) => {
  const { value, chars, emoji } = JSON.parse(payload);
  const message = chars !== undefined ? 'x'.repeat(chars - 2) : emoji !== undefined ? '😀'.repeat(emoji) : value;
  wings.broadcast(message).then(() => results.push('sent'), (error) => results.push(error.message)).then(show);
  return true;
};
show();
"#;

/// Everything a plugin frame has on `wings`, sorted: broadcast, storage and focus, and the calls the host answers
/// (`calls`). A page has `render` too.
const WINGS_CALLS: &[&str] = &[
    "agentActivity",
    "blockState",
    "broadcast",
    "callTool",
    "claudeAccounts",
    "closeView",
    "copyText",
    "createWorktree",
    "diskFind",
    "diskUsage",
    "disks",
    "emit",
    "exec",
    "fetch",
    "focusPane",
    "focused",
    "notify",
    "onAction",
    "onAgentActivity",
    "onBroadcast",
    "onDiffClosed",
    "onDiffRefresh",
    "onEvent",
    "onFocus",
    "onGitChange",
    "onPanes",
    "onProjects",
    "onReply",
    "onSectionSort",
    "onTool",
    "onTranscript",
    "openDiff",
    "openPane",
    "openSettings",
    "openUrl",
    "openView",
    "ports",
    "post",
    "project",
    "projects",
    "secrets",
    "sendToAgent",
    "setBadge",
    "setBlockState",
    "setClaudeAccount",
    "setPaneStatus",
    "setProjectSection",
    "setSectionSort",
    "setTabState",
    "setWorktreeSetup",
    "statusline",
    "storage",
    "tabState",
    "transcript",
    "updateDiff",
    "viewArgs",
    "worktreeSetup",
];
const WINGS_CALLS_AND_RENDER: &[&str] = &[
    "agentActivity",
    "blockState",
    "broadcast",
    "callTool",
    "claudeAccounts",
    "closeView",
    "copyText",
    "createWorktree",
    "diskFind",
    "diskUsage",
    "disks",
    "emit",
    "exec",
    "fetch",
    "focusPane",
    "focused",
    "notify",
    "onAction",
    "onAgentActivity",
    "onBroadcast",
    "onDiffClosed",
    "onDiffRefresh",
    "onEvent",
    "onFocus",
    "onGitChange",
    "onPanes",
    "onProjects",
    "onReply",
    "onSectionSort",
    "onTool",
    "onTranscript",
    "openDiff",
    "openPane",
    "openSettings",
    "openUrl",
    "openView",
    "ports",
    "post",
    "project",
    "projects",
    "render",
    "secrets",
    "sendToAgent",
    "setBadge",
    "setBlockState",
    "setClaudeAccount",
    "setPaneStatus",
    "setProjectSection",
    "setSectionSort",
    "setTabState",
    "setWorktreeSetup",
    "statusline",
    "storage",
    "tabState",
    "transcript",
    "updateDiff",
    "viewArgs",
    "worktreeSetup",
];

/// A folder for a plugin's storage, removed when the test ends.
pub(in crate::page) struct Root(PathBuf);

impl Root {
    pub(in crate::page) fn new() -> Self {
        static COUNT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "wings-native-plugin-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub(in crate::page) fn config(&self, id: &str) -> PluginConfig {
        self.scoped(id, SCOPE)
    }

    fn scoped(&self, id: &str, scope: &str) -> PluginConfig {
        PluginConfig {
            id: id.into(),
            root: self.0.clone(),
            scope: scope.into(),
        }
    }

    /// What the plugin has saved for `key`, read by a storage of its own, as the desktop app would.
    pub(in crate::page) fn saved(&self, id: &str, key: &str) -> Option<Value> {
        Storage::new(self.0.clone()).get(id, SCOPE, key).unwrap()
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A frame and what it has sent the host.
struct Frame {
    runtime: PageRuntime,
    events: mpsc::Receiver<Event>,
}

fn events() -> (impl FnMut(Event) + Send + 'static, mpsc::Receiver<Event>) {
    let (sender, received) = mpsc::channel();
    (
        move |event| {
            let _ = sender.send(event);
        },
        received,
    )
}

fn open(host: &PluginHost, source: &str) -> Frame {
    let (on_event, events) = events();
    let runtime = host.open_page(source, on_event).unwrap();
    Frame { runtime, events }
}

impl Frame {
    fn next(&self) -> Event {
        self.events
            .recv_timeout(WAIT)
            .expect("an event from the frame")
    }

    /// The trees the frame sends until `done` likes one. Its errors fail the test.
    fn until(&self, what: &str, done: impl Fn(&Tree) -> bool) -> Tree {
        let mut last = String::new();
        loop {
            let event = self.events.recv_timeout(WAIT);
            match event {
                Ok(Event::Tree(tree)) if done(&tree) => return tree,
                Ok(Event::Tree(tree)) => last = tree.text(),
                Ok(Event::Error { message, .. }) => panic!("waiting for {what}: {message}"),
                Err(_) => panic!("waiting for {what}: the frame last showed {last:?}"),
            }
        }
    }

    fn shows(&self, text: &str) -> Tree {
        self.until(text, |tree| tree.text().contains(text))
    }

    /// A probe's report: what it heard and what became of what it sent, once `done` likes it.
    fn report(&self, what: &str, done: impl Fn(&Value) -> bool) -> Value {
        let tree = self.until(what, |tree| done(&report_of(tree)));
        report_of(&tree)
    }

    fn send(&self, payload: Value) {
        assert!(self.runtime.dispatch("probe", Some(&payload)));
    }
}

fn report_of(tree: &Tree) -> Value {
    serde_json::from_str(&tree.text()).unwrap()
}

fn button(nodes: &[Node]) -> Option<String> {
    nodes.iter().find_map(|node| match node {
        Node::Button(button) => button.on_click.clone(),
        other => other.parts().into_iter().find_map(button),
    })
}

/// Opens the fixture's page and returns it with the id of its button, once it shows `count`.
fn counter(host: &PluginHost, count: u32) -> (Frame, String) {
    let page = open(host, PAGE);
    let tree = page.shows(&format!("Count: {count}"));
    let handler = button(&tree.children).expect("the page's button has a handler");
    (page, handler)
}

#[test]
fn a_click_asks_main_which_saves_the_count_and_the_page_shows_what_main_pushed() {
    let root = Root::new();
    let host = PluginHost::new(root.config("fixture"));
    let (on_event, main_events) = events();
    host.start_main(MAIN, on_event).unwrap();
    let started = Instant::now();
    let (page, handler) = counter(&host, 0);
    let first = started.elapsed();

    let clicked = Instant::now();
    assert!(page.runtime.dispatch(&handler, None));
    let after = page.shows("Count: 1");
    let round_trip = clicked.elapsed();
    eprintln!(
        "page up and showing main's count {first:?}, click to the new count in the tree {round_trip:?}"
    );
    // The id is the same in the new tree, so a host that kept the old one can still click.
    assert_eq!(button(&after.children), Some(handler.clone()));
    assert_eq!(root.saved("fixture", "count"), Some(json!(1)));

    assert!(page.runtime.dispatch(&handler, None));
    page.shows("Count: 2");
    assert_eq!(root.saved("fixture", "count"), Some(json!(2)));
    assert!(
        main_events.try_recv().is_err(),
        "main had nothing to report"
    );
}

#[test]
fn every_page_of_the_plugin_shows_the_count_one_of_them_raised() {
    let root = Root::new();
    let host = PluginHost::new(root.config("fixture"));
    host.start_main(MAIN, events().0).unwrap();
    let (first, handler) = counter(&host, 0);
    let (second, _) = counter(&host, 0);
    assert!(first.runtime.dispatch(&handler, None));
    first.shows("Count: 1");
    second.shows("Count: 1");
}

#[test]
fn the_count_survives_a_new_host_over_the_same_storage_root_but_not_another_scope() {
    let root = Root::new();
    {
        let host = PluginHost::new(root.config("fixture"));
        host.start_main(MAIN, events().0).unwrap();
        let (page, handler) = counter(&host, 0);
        for count in 1..=2 {
            assert!(page.runtime.dispatch(&handler, None));
            page.shows(&format!("Count: {count}"));
        }
    }
    let host = PluginHost::new(root.config("fixture"));
    host.start_main(MAIN, events().0).unwrap();
    let (page, handler) = counter(&host, 2);
    assert!(page.runtime.dispatch(&handler, None));
    page.shows("Count: 3");

    // An install of the same plugin that came after it was removed has a scope of its own, so it starts empty.
    let other = PluginHost::new(root.scoped("fixture", "another-install"));
    other.start_main(MAIN, events().0).unwrap();
    counter(&other, 0);
}

#[test]
fn a_broadcast_reaches_the_other_frames_of_its_plugin_in_order_and_never_the_sender_or_another_plugin()
 {
    let root = Root::new();
    let host = PluginHost::new(root.config("fixture"));
    let (sender, first, second) = (open(&host, PROBE), open(&host, PROBE), open(&host, PROBE));
    let stranger = PluginHost::new(root.config("other"));
    let (other_sender, other) = (open(&stranger, PROBE), open(&stranger, PROBE));
    for frame in [&sender, &first, &second, &other_sender, &other] {
        frame.report("the probe to start", |_| true);
    }

    let messages: Vec<Value> = (0..30).map(|n| json!({ "n": n, "text": "é😀" })).collect();
    for message in &messages {
        sender.send(json!({ "value": message }));
    }
    for frame in [&first, &second] {
        let report = frame.report("30 messages", |r| {
            r["heard"].as_array().unwrap().len() == 30
        });
        assert_eq!(report["heard"], json!(messages));
    }
    // The sender takes its events in order, so anything it sent itself would be in its queue before this one.
    sender.send(json!({ "value": "last" }));
    let report = sender.report("31 results", |r| {
        r["results"].as_array().unwrap().len() == 31
    });
    assert_eq!(report["heard"], json!([]), "the sender heard itself");
    assert_eq!(report["results"], json!(vec!["sent"; 31]));
    first.report("last", |r| r["heard"].as_array().unwrap().len() == 31);

    // Everything above was delivered before the senders heard it was sent, so a leak would be in `other` by now.
    // Its own sender's message comes after, and it hears only that.
    other_sender.send(json!({ "value": "from the other plugin" }));
    let report = other.report("the other plugin's message", |r| {
        !r["heard"].as_array().unwrap().is_empty()
    });
    assert_eq!(report["heard"], json!(["from the other plugin"]));
    // Null and a missing message are both `null`, as on the desktop.
    sender.send(json!({}));
    let report = first.report("null", |r| r["heard"].as_array().unwrap().len() == 32);
    assert_eq!(report["heard"][31], Value::Null);
}

#[test]
fn a_broadcast_over_64000_characters_of_json_is_refused_and_one_of_64000_goes_through() {
    let root = Root::new();
    let host = PluginHost::new(root.config("fixture"));
    let (sender, listener) = (open(&host, PROBE), open(&host, PROBE));
    sender.send(json!({ "chars": 64_000 }));
    // JS string units, as on the desktop: 31,999 emoji and two quotes are 64,000, one more is not.
    sender.send(json!({ "emoji": 31_999 }));
    sender.send(json!({ "chars": 64_001 }));
    sender.send(json!({ "emoji": 32_000 }));
    let report = sender.report("four results", |r| {
        r["results"].as_array().unwrap().len() == 4
    });
    let refused = "A broadcast is limited to 64 KB";
    assert_eq!(report["results"], json!(["sent", "sent", refused, refused]));

    // The refused ones went to no one: what comes after them is the listener's next message.
    sender.send(json!({ "value": "after" }));
    let report = listener.report("the last message", |r| {
        r["heard"].as_array().unwrap().len() == 3
    });
    assert_eq!(
        report["heard"],
        json!(["text of 63998", "text of 63998", "after"])
    );
}

#[test]
fn a_message_too_big_for_one_broadcast_goes_to_main_in_parts_and_back_as_the_answer() {
    let root = Root::new();
    let host = PluginHost::new(root.config("fixture"));
    host.start_main(MAIN, events().0).unwrap();
    let page = open(&host, BIG_PAGE);
    page.shows("ready");
    let started = Instant::now();
    page.send(json!({ "size": 300_000 }));
    page.shows("echo 300000");
    eprintln!(
        "300,000 characters asked of main in parts and answered: {:?}",
        started.elapsed()
    );
}

#[test]
fn a_main_that_throws_or_loops_leaves_the_pages_and_their_talk_alone() {
    for source in ["throw new Error('boom');", "while (true) {}"] {
        let root = Root::new();
        // The default budget: the page boots React, which a busy machine can take a while over.
        let host = PluginHost::new(root.config("fixture"));
        let (on_event, main_events) = events();
        let fixture_page = open(&host, PAGE);
        let handler = button(&fixture_page.shows("Loading").children).unwrap();
        let (a, b) = (open(&host, PROBE), open(&host, PROBE));
        let started = Instant::now();
        host.start_main(source, on_event).unwrap();

        // Main is told about the page's ask and the probes' message while it spins or after it has stopped: neither
        // the sender nor the page waits for it.
        assert!(fixture_page.runtime.dispatch(&handler, None));
        a.send(json!({ "value": "hello" }));
        // The page's own messages to main reach the probes too: they are frames of the plugin.
        b.report("hello", |r| {
            r["heard"].as_array().unwrap().contains(&json!("hello"))
        });
        // Main's one and last event: it never started, or it was stopped.
        match main_events.recv_timeout(WAIT).expect("main's end") {
            Event::Error { message, fatal } => {
                eprintln!("`{source}`: {message}, after {:?}", started.elapsed());
                assert!(fatal, "{message}");
            }
            Event::Tree(_) => panic!("main drew a tree"),
        }
        // Stopped for good, and the others still talk.
        a.send(json!({ "value": "again" }));
        b.report("again", |r| {
            r["heard"].as_array().unwrap().contains(&json!("again"))
        });
        assert!(fixture_page.runtime.dispatch(&handler, None));
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}

#[test]
fn a_page_that_loops_in_a_broadcast_stops_alone() {
    let root = Root::new();
    let limits = Limits {
        turn: Duration::from_millis(300),
        ..Limits::default()
    };
    let host = PluginHost::with_limits(root.config("fixture"), limits);
    let stuck = open(
        &host,
        "wings.onBroadcast(() => { for (;;) {} }); wings.render('{\"id\":0,\"type\":\"#root\",\"props\":{},\"children\":[]}');",
    );
    let (a, b) = (open(&host, PROBE), open(&host, PROBE));
    a.send(json!({ "value": 1 }));
    loop {
        match stuck.next() {
            Event::Error { fatal: true, .. } => break,
            Event::Error { .. } | Event::Tree(_) => {}
        }
    }
    // The event comes before the thread lets go of its queue, so wait for that.
    let inbox = stuck.runtime.inbox();
    while inbox.deliver("null".into()) != Delivery::Stopped {
        std::thread::sleep(Duration::from_millis(5));
    }
    a.send(json!({ "value": 2 }));
    assert_eq!(
        b.report("both", |r| r["heard"].as_array().unwrap().len() == 2)["heard"],
        json!([1, 2])
    );
    // The stopped frame is let go of from the list as soon as something is sent to it.
    assert_eq!(host.shared.frames.lock().unwrap().len(), 2);
}

#[test]
fn a_frame_that_is_dropped_leaves_the_plugin_and_its_thread_ends() {
    let root = Root::new();
    let host = PluginHost::new(root.config("fixture"));
    let kept = open(&host, PROBE);
    let gone = open(&host, PROBE);
    // The host, and a link and a place in the list for each frame.
    assert_eq!(Arc::strong_count(&host.shared), 5);
    assert_eq!(host.shared.frames.lock().unwrap().len(), 2);
    drop(gone);
    assert_eq!(host.shared.frames.lock().unwrap().len(), 1);
    // Nothing holds its queue open any more, so its thread ends and lets go of its link.
    let until = Instant::now() + Duration::from_secs(5);
    while Arc::strong_count(&host.shared) > 3 {
        assert!(
            Instant::now() < until,
            "the dropped frame's thread is still running"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    kept.report("the probe", |_| true);
}

/// A page that stores, reads and removes values, and shows how each call went.
const STORAGE_PAGE: &str = r#"
(async () => {
  const out = {};
  const trying = async (name, call) => { try { const value = await call(); out[name] = value === undefined ? 'ok' : value; } catch (error) { out[name] = `refused: ${error.message}`; } };
  await wings.storage.set('a/1', { x: [1, 2], y: 'é' });
  await wings.storage.set('a/2', 'two');
  await wings.storage.set('b', null);
  await trying('get', () => wings.storage.get('a/1'));
  await trying('missing', () => wings.storage.get('nope'));
  await trying('null', () => wings.storage.get('b'));
  await trying('keys', () => wings.storage.keys());
  await trying('prefix', () => wings.storage.entries('a/'));
  await trying('all', async () => (await wings.storage.entries()).length);
  await wings.storage.delete('a/2');
  await wings.storage.delete('never-set');
  await trying('after', () => wings.storage.keys());
  await trying('number key', async () => { await wings.storage.set(5, 'five'); return wings.storage.get('5'); });
  await trying('bad key', () => wings.storage.set('bad key!', 1));
  await trying('empty key', () => wings.storage.get(''));
  await trying('nothing to store', () => wings.storage.set('k', undefined));
  await trying('big', () => wings.storage.set('k', 'x'.repeat(1024 * 1024)));
  await trying('just fits', () => wings.storage.set('k', 'x'.repeat(1024 * 1024 - 2)));
  wings.render(JSON.stringify({ id: 0, type: '#root', props: {}, children: [{ id: 1, type: '#text', text: JSON.stringify(out) }] }));
})();
"#;

#[test]
fn storage_keeps_json_by_key_for_this_plugin_and_scope_with_the_desktops_limits() {
    let root = Root::new();
    let host = PluginHost::new(root.config("fixture"));
    let page = open(&host, STORAGE_PAGE);
    let seen = report_of(&page.until("the storage calls", |_| true));
    let refused = |why: &str| json!(format!("refused: {why}"));
    assert_eq!(seen["get"], json!({ "x": [1, 2], "y": "é" }));
    assert_eq!(seen["missing"], Value::Null);
    assert_eq!(seen["null"], Value::Null);
    assert_eq!(seen["keys"], json!(["a/1", "a/2", "b"]));
    assert_eq!(
        seen["prefix"],
        json!([["a/1", { "x": [1, 2], "y": "é" }], ["a/2", "two"]])
    );
    assert_eq!(seen["all"], json!(3));
    assert_eq!(seen["after"], json!(["a/1", "b"]));
    assert_eq!(seen["number key"], json!("five"));
    let key = "A storage key is 1 to 128 letters, digits and - _ . : /";
    assert_eq!(seen["bad key"], refused(key));
    assert_eq!(seen["empty key"], refused(key));
    assert_eq!(
        seen["nothing to store"],
        refused("wings.storage.set needs a JSON value. Use delete to remove a key")
    );
    assert_eq!(seen["big"], refused("A stored value is limited to 1 MB"));
    assert_eq!(seen["just fits"], "ok");

    // The same files the desktop app reads, under this plugin's id and scope only.
    let files = Storage::new(root.0.clone());
    assert_eq!(
        files.keys("fixture", SCOPE).unwrap(),
        ["5", "a/1", "b", "k"]
    );
    assert_eq!(files.keys("other", SCOPE).unwrap(), Vec::<String>::new());
    assert_eq!(
        files.keys("fixture", "another-install").unwrap(),
        Vec::<String>::new()
    );
}

#[test]
fn a_frame_has_the_wings_calls_the_host_answers_and_the_timers_and_nothing_else_of_the_host() {
    let root = Root::new();
    let host = PluginHost::new(root.config("fixture"));
    let page = open(
        &host,
        r#"
        const denied = ['fetch', 'require', 'process', 'std', 'os', 'XMLHttpRequest', 'WebSocket', 'window', 'document', 'Worker',
          'setImmediate', 'print', 'Deno', 'Bun'].filter((name) => typeof globalThis[name] !== 'undefined');
        const names = Object.getOwnPropertyNames(globalThis).filter((name) => /^(wings|console|set|clear|__)/.test(name)).sort();
        let replaced = true;
        try { globalThis.wings = {}; } catch (_) { replaced = false; }
        let storageReplaced = true;
        try { wings.storage.get = null; } catch (_) { storageReplaced = false; }
        wings.render(JSON.stringify({ id: 0, type: '#root', props: {}, children: [{ id: 1, type: '#text', text: JSON.stringify({
          denied, names, replaced, storageReplaced, wings: Object.keys(wings).sort(), storage: Object.keys(wings.storage).sort(),
          frozen: Object.isFrozen(wings) && Object.isFrozen(wings.storage), focused: wings.focused,
          console: Object.keys(console).sort(), logged: console.error(new Error('x')) ?? 'nothing' }) }] }));
        "#,
    );
    let seen = report_of(&page.until("the report", |_| true));
    assert_eq!(seen["denied"], json!([]));
    assert_eq!(
        seen["names"],
        json!([
            "clearInterval",
            "clearTimeout",
            "console",
            "setInterval",
            "setTimeout",
            "wings"
        ])
    );
    assert_eq!(seen["wings"], json!(WINGS_CALLS_AND_RENDER));
    assert_eq!(
        seen["storage"],
        json!(["delete", "entries", "get", "keys", "set"])
    );
    assert_eq!(
        seen["console"],
        json!(["debug", "error", "info", "log", "warn"])
    );
    assert_eq!(seen["logged"], "nothing");
    assert_eq!(seen["replaced"], false);
    assert_eq!(seen["storageReplaced"], false);
    assert_eq!(seen["frozen"], true);
    assert_eq!(seen["focused"], true);
}

#[test]
fn main_has_the_same_wings_less_render() {
    let root = Root::new();
    let host = PluginHost::new(root.config("fixture"));
    let listener = open(&host, PROBE);
    listener.report("the probe", |_| true);
    host.start_main(
        "wings.broadcast({ wings: Object.keys(wings).sort(), render: typeof wings.render, \
         timers: [typeof setInterval, typeof clearInterval] });",
        events().0,
    )
    .unwrap();
    let report = listener.report("main's message", |r| {
        !r["heard"].as_array().unwrap().is_empty()
    });
    assert_eq!(
        report["heard"][0],
        json!({ "wings": WINGS_CALLS, "render": "undefined", "timers": ["function", "function"] })
    );
}

#[test]
fn starting_main_again_replaces_it_and_the_old_one_stops() {
    let root = Root::new();
    let host = PluginHost::new(root.config("fixture"));
    let listener = open(&host, PROBE);
    host.start_main("wings.broadcast('first');", events().0)
        .unwrap();
    listener.report("first", |r| r["heard"] == json!(["first"]));
    host.start_main("wings.broadcast('second');", events().0)
        .unwrap();
    listener.report("second", |r| r["heard"] == json!(["first", "second"]));
    assert_eq!(host.shared.frames.lock().unwrap().len(), 2);
}
