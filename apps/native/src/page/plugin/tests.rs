use super::testing::show;
use super::*;
use crate::page::{
    runtime::fake::{self, Cx, Guest},
    tree::{Node, Tree},
};
use serde_json::{Value, json};
use std::{
    fs,
    sync::mpsc,
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(10);
const SCOPE: &str = "test-scope";

/// A page that shows what it heard and what became of what it sent. An event's payload is the message it
/// broadcasts: `{ value }`, or a text that is `chars` JSON characters long, or `emoji` emoji (2 JS string units each).
fn probe() -> String {
    struct Probe {
        heard: Vec<Value>,
        results: Vec<String>,
    }
    impl Probe {
        fn show(&self, cx: &mut Cx<'_>) {
            show(cx, json!({ "heard": self.heard, "results": self.results }));
        }
    }
    impl Guest for Probe {
        fn start(&mut self, cx: &mut Cx<'_>) {
            self.show(cx);
        }
        fn event(&mut self, cx: &mut Cx<'_>, _handler: &str, payload: &Value) {
            let message = if let Some(chars) = payload["chars"].as_u64() {
                json!("x".repeat(chars as usize - 2))
            } else if let Some(emoji) = payload["emoji"].as_u64() {
                json!("😀".repeat(emoji as usize))
            } else {
                payload["value"].clone()
            };
            self.results
                .push(match cx.link().broadcast(message.to_string()) {
                    Ok(()) => "sent".into(),
                    Err(error) => error,
                });
            self.show(cx);
        }
        fn deliver(&mut self, cx: &mut Cx<'_>, kind: &str, _id: u32, text: &str) {
            if kind != "broadcast" {
                return;
            }
            let message: Value = serde_json::from_str(text).unwrap();
            self.heard.push(match message.as_str() {
                Some(text) if text.encode_utf16().count() > 50 => {
                    json!(format!("text of {}", text.encode_utf16().count()))
                }
                _ => message,
            });
            self.show(cx);
        }
    }
    fake::source("probe", || {
        Box::new(Probe {
            heard: Vec::new(),
            results: Vec::new(),
        })
    })
}

/// A text node.
fn text_node(id: usize, text: &str) -> Value {
    json!({ "id": id, "type": "#text", "text": text })
}

/// The fixture plugin's page: the count its main keeps, and a button that asks main to add one.
fn fixture_page() -> String {
    struct Page(Option<u64>);
    impl Page {
        fn draw(&self, cx: &mut Cx<'_>) {
            let count = self
                .0
                .map_or("Loading…".to_owned(), |count| format!("Count: {count}"));
            cx.render(&json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [
                { "id": 15, "type": "Page", "props": {}, "children": [
                    { "id": 7, "type": "Slot", "props": { "name": "toolbar" }, "children": [
                        { "id": 6, "type": "Toolbar", "props": {}, "children": [
                            { "id": 2, "type": "Slot", "props": { "name": "title" }, "children": [text_node(1, "Fixture")] },
                            { "id": 5, "type": "Slot", "props": { "name": "actions" }, "children": [
                                { "id": 4, "type": "Badge", "props": { "tone": "success" }, "children": [text_node(3, "Plugin")] }] }] }] },
                    { "id": 14, "type": "Slot", "props": { "name": "body" }, "children": [
                        { "id": 13, "type": "Stack", "props": { "gap": "l" }, "children": [
                            { "id": 12, "type": "Row", "props": { "gap": "m" }, "children": [
                                { "id": 9, "type": "Button", "props": { "onClick": "9:onClick" }, "children": [text_node(8, "Increment")] },
                                { "id": 11, "type": "Text", "props": { "weight": "medium" }, "children": [text_node(10, &count)] }] }] }] }] }] }));
        }
    }
    impl Guest for Page {
        fn start(&mut self, cx: &mut Cx<'_>) {
            self.draw(cx);
            cx.link()
                .broadcast(json!({ "ask": "count" }).to_string())
                .unwrap();
        }
        fn event(&mut self, cx: &mut Cx<'_>, _handler: &str, _payload: &Value) {
            cx.link()
                .broadcast(json!({ "ask": "increment" }).to_string())
                .unwrap();
        }
        fn deliver(&mut self, cx: &mut Cx<'_>, kind: &str, _id: u32, text: &str) {
            let heard: Value = serde_json::from_str(text).unwrap_or_default();
            if let (true, Some(count)) = (kind == "broadcast", heard["count"].as_u64()) {
                self.0 = Some(count);
                self.draw(cx);
            }
        }
    }
    fake::source("page", || Box::new(Page(None)))
}

/// The fixture plugin's main: a count kept in storage, which it broadcasts when asked for it or to add one to it.
fn fixture_main() -> String {
    struct Main;
    impl Guest for Main {
        fn deliver(&mut self, cx: &mut Cx<'_>, kind: &str, _id: u32, text: &str) {
            let heard: Value = serde_json::from_str(text).unwrap_or_default();
            if kind != "broadcast" || heard["ask"].is_null() {
                return;
            }
            let link = cx.link();
            let saved: Value =
                serde_json::from_str(&link.storage(Store::Get("count".into())).unwrap()).unwrap();
            let mut count = saved.as_u64().unwrap_or(0);
            if heard["ask"] == "increment" {
                count += 1;
                link.storage(Store::Set("count".into(), count.to_string()))
                    .unwrap();
            }
            link.broadcast(json!({ "count": count }).to_string())
                .unwrap();
        }
    }
    fake::source("main", || Box::new(Main))
}

/// A frame that spins for `spin`, as a busy loop does, and then stops as a trap does, with `message`.
fn stopping(spin: Duration, message: &'static str) -> String {
    struct Stops(Duration, &'static str);
    impl Guest for Stops {
        fn start(&mut self, cx: &mut Cx<'_>) {
            std::thread::sleep(self.0);
            cx.stop(self.1);
        }
    }
    fake::source("main", move || Box::new(Stops(spin, message)))
}

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

    /// Stores `value` as the plugin `id` kept it, before its main starts.
    pub(in crate::page) fn save(&self, id: &str, key: &str, value: Value) {
        Storage::new(self.0.clone())
            .set(id, SCOPE, key, value)
            .unwrap();
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
    let page = open(host, &fixture_page());
    let tree = page.shows(&format!("Count: {count}"));
    let handler = button(&tree.children).expect("the page's button has a handler");
    (page, handler)
}

#[test]
fn a_click_asks_main_which_saves_the_count_and_the_page_shows_what_main_pushed() {
    let root = Root::new();
    let host = PluginHost::new(root.config("fixture"));
    let (on_event, main_events) = events();
    host.start_main(&fixture_main(), on_event).unwrap();
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
    host.start_main(&fixture_main(), events().0).unwrap();
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
        host.start_main(&fixture_main(), events().0).unwrap();
        let (page, handler) = counter(&host, 0);
        for count in 1..=2 {
            assert!(page.runtime.dispatch(&handler, None));
            page.shows(&format!("Count: {count}"));
        }
    }
    let host = PluginHost::new(root.config("fixture"));
    host.start_main(&fixture_main(), events().0).unwrap();
    let (page, handler) = counter(&host, 2);
    assert!(page.runtime.dispatch(&handler, None));
    page.shows("Count: 3");

    // An install of the same plugin that came after it was removed has a scope of its own, so it starts empty.
    let other = PluginHost::new(root.scoped("fixture", "another-install"));
    other.start_main(&fixture_main(), events().0).unwrap();
    counter(&other, 0);
}

#[test]
fn a_broadcast_reaches_the_other_frames_of_its_plugin_in_order_and_never_the_sender_or_another_plugin()
 {
    let root = Root::new();
    let host = PluginHost::new(root.config("fixture"));
    let (sender, first, second) = (
        open(&host, &probe()),
        open(&host, &probe()),
        open(&host, &probe()),
    );
    let stranger = PluginHost::new(root.config("other"));
    let (other_sender, other) = (open(&stranger, &probe()), open(&stranger, &probe()));
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
    let (sender, listener) = (open(&host, &probe()), open(&host, &probe()));
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
fn a_main_that_throws_or_loops_leaves_the_pages_and_their_talk_alone() {
    for (what, spin) in [
        ("boom", Duration::ZERO),
        (
            "the main ran for too long and was stopped",
            Duration::from_millis(300),
        ),
    ] {
        let root = Root::new();
        let host = PluginHost::new(root.config("fixture"));
        let (on_event, main_events) = events();
        let fixture_page = open(&host, &fixture_page());
        let handler = button(&fixture_page.shows("Loading").children).unwrap();
        let (a, b) = (open(&host, &probe()), open(&host, &probe()));
        let started = Instant::now();
        host.start_main(&stopping(spin, what), on_event).unwrap();

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
                eprintln!("`{what}`: {message}, after {:?}", started.elapsed());
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
    /// A page that draws nothing and is stopped by the first message it hears, as a loop in its listener is.
    struct Stuck;
    impl Guest for Stuck {
        fn start(&mut self, cx: &mut Cx<'_>) {
            cx.render(&json!({ "id": 0, "type": "#root", "props": {}, "children": [] }));
        }
        fn deliver(&mut self, cx: &mut Cx<'_>, _kind: &str, _id: u32, _text: &str) {
            cx.stop("the page ran for too long and was stopped");
        }
    }
    let root = Root::new();
    let limits = Limits {
        turn: Duration::from_millis(300),
        ..Limits::default()
    };
    let host = PluginHost::with_limits(root.config("fixture"), limits);
    let stuck = open(&host, &fake::source("stuck", || Box::new(Stuck)));
    let (a, b) = (open(&host, &probe()), open(&host, &probe()));
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
    let kept = open(&host, &probe());
    let gone = open(&host, &probe());
    // The host, and a link and a place in the list for each frame.
    assert_eq!(Arc::strong_count(&host.shared), 5);
    assert_eq!(host.shared.frames.lock().unwrap().len(), 2);
    drop(gone);
    assert_eq!(host.shared.frames.lock().unwrap().len(), 1);
    // Its engine lets go of its context, and of its link with it.
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
fn storing() -> String {
    struct Storing;
    impl Guest for Storing {
        fn start(&mut self, cx: &mut Cx<'_>) {
            let link = cx.link().clone();
            let ask = |call: Store| {
                link.storage(call)
                    .map(|json| serde_json::from_str::<Value>(&json).unwrap())
            };
            let set = |key: &str, value: Value| ask(Store::Set(key.into(), value.to_string()));
            let get = |key: &str| ask(Store::Get(key.into()));
            let mut out = serde_json::Map::new();
            let mut trying = |name: &str, result: Result<Value, String>| {
                out.insert(
                    name.into(),
                    result.unwrap_or_else(|why| json!(format!("refused: {why}"))),
                );
            };
            let done = |result: Result<Value, String>| result.map(|_| json!("ok"));
            set("a/1", json!({ "x": [1, 2], "y": "é" })).unwrap();
            set("a/2", json!("two")).unwrap();
            set("b", Value::Null).unwrap();
            trying("get", get("a/1"));
            trying("missing", get("nope"));
            trying("null", get("b"));
            trying("keys", ask(Store::Keys));
            trying("prefix", ask(Store::Entries("a/".into())));
            trying(
                "all",
                ask(Store::Entries(String::new())).map(|all| json!(all.as_array().unwrap().len())),
            );
            ask(Store::Delete("a/2".into())).unwrap();
            ask(Store::Delete("never-set".into())).unwrap();
            trying("after", ask(Store::Keys));
            trying("number key", set("5", json!("five")).and_then(|_| get("5")));
            trying("bad key", done(set("bad key!", json!(1))));
            trying("empty key", get(""));
            trying("big", done(set("k", json!("x".repeat(1024 * 1024)))));
            trying(
                "just fits",
                done(set("k", json!("x".repeat(1024 * 1024 - 2)))),
            );
            show(cx, Value::Object(out));
        }
    }
    fake::source("page", || Box::new(Storing))
}

#[test]
fn storage_keeps_json_by_key_for_this_plugin_and_scope_with_the_desktops_limits() {
    let root = Root::new();
    let host = PluginHost::new(root.config("fixture"));
    let page = open(&host, &storing());
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
    // The storage says how big the value was, which the JS glue's own check didn't.
    assert!(
        seen["big"]
            .as_str()
            .unwrap()
            .starts_with("refused: A stored value is limited to 1 MB"),
        "{}",
        seen["big"]
    );
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

/// A module that imports `wings.exec`, which the host never gave a frame: the header, a type of `() -> ()`, and that
/// import. A frame reaches the host through the imports in `wasm::imports` alone, and the linker refuses any other.
#[test]
fn a_module_that_imports_anything_but_the_wings_functions_does_not_start() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("page.wasm");
    fs::write(
        &module,
        b"\0asm\x01\0\0\0\x01\x04\x01\x60\0\0\x02\x0e\x01\x05wings\x04exec\0\0",
    )
    .unwrap();
    let root = Root::new();
    let host = PluginHost::new(root.config("fixture"));
    let page = open(
        &host,
        &format!("{}{}", crate::page::runtime::wasm::MARKER, module.display()),
    );
    match page.next() {
        Event::Error { message, fatal } => assert!(fatal && message.contains("exec"), "{message}"),
        Event::Tree(tree) => panic!("the module drew {}", tree.text()),
    }
}

#[test]
fn starting_main_again_replaces_it_and_the_old_one_stops() {
    /// A main that broadcasts `what` as it starts.
    fn says(what: &'static str) -> String {
        struct Says(&'static str);
        impl Guest for Says {
            fn start(&mut self, cx: &mut Cx<'_>) {
                cx.link().broadcast(json!(self.0).to_string()).unwrap();
            }
        }
        fake::source("main", move || Box::new(Says(what)))
    }
    let root = Root::new();
    let host = PluginHost::new(root.config("fixture"));
    let listener = open(&host, &probe());
    host.start_main(&says("first"), events().0).unwrap();
    listener.report("first", |r| r["heard"] == json!(["first"]));
    host.start_main(&says("second"), events().0).unwrap();
    listener.report("second", |r| r["heard"] == json!(["first", "second"]));
    assert_eq!(host.shared.frames.lock().unwrap().len(), 2);
}

/// A main that stops for good is started again, so its badges, tools and actions come back without restarting Wings:
/// Changes' and Tasks' mains stopped on a busy Mac and stayed stopped. Once the plugin is dropped, nothing restarts.
#[test]
fn a_main_that_stops_is_started_again_until_its_plugin_is_dropped() {
    /// Counts its starts, then is stopped a moment later, as a loop is.
    struct Counts;
    impl Guest for Counts {
        fn start(&mut self, cx: &mut Cx<'_>) {
            let link = cx.link();
            let before: Value =
                serde_json::from_str(&link.storage(Store::Get("starts".into())).unwrap()).unwrap();
            let starts = before.as_u64().unwrap_or(0) + 1;
            link.storage(Store::Set("starts".into(), starts.to_string()))
                .unwrap();
            cx.later(50);
        }
        fn timer(&mut self, cx: &mut Cx<'_>, _id: u32) {
            cx.stop("the main ran for too long and was stopped");
        }
    }
    let root = Root::new();
    let limits = Limits {
        turn: Duration::from_millis(100),
        ..Limits::default()
    };
    let host = Arc::new(PluginHost::with_limits(root.config("fixture"), limits));
    let (stops, stopped) = mpsc::channel();
    host.keep_main(
        fake::source("main", || Box::new(Counts)).into(),
        Arc::new(move |event| {
            if matches!(event, Event::Error { fatal: true, .. }) {
                let _ = stops.send(());
            }
        }),
    )
    .unwrap();
    let starts = |root: &Root| {
        root.saved("fixture", "starts")
            .and_then(|n| n.as_u64())
            .unwrap_or(0)
    };
    stopped.recv_timeout(WAIT).expect("the first main to stop");
    stopped
        .recv_timeout(WAIT)
        .expect("the main started again, and stopped again");
    assert_eq!(starts(&root), 2);
    drop(host);
    // The next restart would have come 4 s after that stop.
    std::thread::sleep(Duration::from_secs(5));
    assert_eq!(starts(&root), 2, "a dropped plugin's main started again");
}
