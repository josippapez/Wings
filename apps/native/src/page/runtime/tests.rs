use super::{
    fake::{self, Cx, Guest},
    *,
};
use crate::page::tree::Node;
use std::collections::BTreeSet;

const PILOT: &str = include_str!("../../../pages/dist/pilot.js");
const WAIT: Duration = Duration::from_secs(10);

#[test]
fn disk_progress_is_decoded_and_aborting_cancels_the_same_call() {
    #[derive(Default)]
    struct Disk {
        pending: std::sync::Mutex<Option<(String, Reply)>>,
    }
    impl Link for Disk {
        fn broadcast(&self, _: String) -> Result<(), String> {
            Ok(())
        }
        fn storage(&self, _: Store) -> Result<String, String> {
            Ok("null".into())
        }
        fn call(&self, method: &str, params: serde_json::Value, reply: Reply) {
            match method {
                "diskFind" => {
                    assert_eq!(params["stream"], true);
                    let token = params["token"].as_str().unwrap().to_owned();
                    reply.lines()(r#"{"entries":123,"bytes":0,"path":"/fixture"}"#);
                    *self.pending.lock().unwrap() = Some((token, reply));
                }
                "diskUsageCancel" => {
                    let (token, pending) = self.pending.lock().unwrap().take().unwrap();
                    assert_eq!(params["token"], token);
                    pending.err("The scan was stopped".into());
                    reply.ok("null".into());
                }
                _ => panic!("unexpected call {method}"),
            }
        }
    }
    /// A page that scans, stops the scan when it hears how far it got, and shows how the scan ended.
    struct Scans {
        find: u32,
        looked: u64,
    }
    impl Guest for Scans {
        fn start(&mut self, cx: &mut Cx<'_>) {
            self.find = cx.call(
                "diskFind",
                serde_json::json!({ "path": "/fixture", "names": [".git"], "token": "1", "stream": true }),
            );
        }
        fn deliver(&mut self, cx: &mut Cx<'_>, kind: &str, id: u32, text: &str) {
            match kind {
                "output" if id == self.find && self.looked == 0 => {
                    let progress: serde_json::Value = serde_json::from_str(text).unwrap();
                    self.looked = progress["entries"].as_u64().unwrap();
                    cx.call("diskUsageCancel", serde_json::json!({ "token": "1" }));
                }
                "reject" if id == self.find => {
                    let result = serde_json::json!({ "looked": self.looked, "stopped": true, "error": text });
                    cx.render(&serde_json::json!({ "type": "#root", "id": 0, "children": [
                        { "type": "#text", "id": 1, "text": result.to_string() }] }));
                }
                _ => {}
            }
        }
    }
    let source = fake::source("page", || Box::new(Scans { find: 0, looked: 0 }));
    let (tx, events) = mpsc::channel();
    let runtime = PageRuntime::spawn_plugin(
        source,
        Role::Page,
        Limits::default(),
        Arc::new(Disk::default()),
        move |event| {
            let _ = tx.send(event);
        },
    )
    .unwrap();
    let page = Page { runtime, events };
    let text = page.tree().text();
    let result: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        result,
        serde_json::json!({ "looked": 123, "stopped": true, "error": "The scan was stopped" })
    );
}

struct Page {
    runtime: PageRuntime,
    events: mpsc::Receiver<Event>,
}

fn start(source: &str, limits: Limits) -> Page {
    let (sender, events) = mpsc::channel();
    let runtime = PageRuntime::spawn_with(source, limits, move |event| {
        let _ = sender.send(event);
    })
    .unwrap();
    Page { runtime, events }
}

impl Page {
    fn next(&self) -> Event {
        self.events
            .recv_timeout(WAIT)
            .expect("an event from the page")
    }

    /// The next tree; the page's errors fail the test.
    fn tree(&self) -> Tree {
        match self.next() {
            Event::Tree(tree) => tree,
            Event::Error { message, .. } => panic!("page error: {message}"),
        }
    }

    /// Trees until one shows `text`.
    fn until(&self, text: &str) -> Tree {
        loop {
            let tree = self.tree();
            if tree.text().contains(text) {
                return tree;
            }
        }
    }
}

fn button(nodes: &[Node]) -> Option<String> {
    nodes.iter().find_map(|node| match node {
        Node::Button(button) => button.on_click.clone(),
        other => other.parts().into_iter().find_map(button),
    })
}

#[test]
fn the_pilot_page_renders_and_a_click_renders_the_new_count() {
    let started = Instant::now();
    let page = start(PILOT, Limits::default());
    let first = page.tree();
    let first_render = started.elapsed();
    assert!(first.text().contains("Count: 0"), "{}", first.text());
    // The effect's state update comes a timer turn later.
    let ready = page.until("Ready");
    let handler = button(&ready.children).expect("the page's button has a handler");
    assert!(handler.ends_with(":onClick"), "{handler}");

    let clicked = Instant::now();
    assert!(page.runtime.dispatch(&handler, None));
    let after_one = page.until("Count: 1");
    let click_to_tree = clicked.elapsed();
    eprintln!("first tree {first_render:?}, click to new tree {click_to_tree:?}");
    // The id is the same in the new tree, so a host that kept the old one can still click.
    assert_eq!(button(&after_one.children), Some(handler.clone()));
    assert!(page.runtime.dispatch(&handler, None));
    assert!(page.until("Count: 2").text().contains("Ready"));
}

fn report(body: &str) -> String {
    format!(
        "const show = (value) => wings.render(JSON.stringify({{id: 0, type: '#root', props: {{}}, \
         children: [{{id: 1, type: '#text', text: JSON.stringify(value)}}]}}));{body}"
    )
}

fn shown(tree: &Tree) -> serde_json::Value {
    serde_json::from_str(&tree.text()).unwrap()
}

#[test]
fn the_page_sees_wings_and_timers_and_nothing_else_of_the_host() {
    // What a bare engine has as globals (QuickJS has its own `queueMicrotask`, which only queues a promise job):
    // everything past that was added by the host.
    let engine = Runtime::new().unwrap();
    let bare = Context::full(&engine).unwrap();
    let bare: BTreeSet<String> = bare.with(|ctx| {
        let names: String = ctx
            .eval("JSON.stringify(Object.getOwnPropertyNames(globalThis))")
            .unwrap();
        serde_json::from_str(&names).unwrap()
    });

    let page = start(
        &report(
            "'use strict';
             const denied = ['fetch', 'require', 'process', 'std', 'os', 'console', 'XMLHttpRequest', 'WebSocket', 'window',
                 'document', 'Worker', 'setInterval', 'setImmediate', 'print', 'Deno', 'Bun']
                 .filter((name) => typeof globalThis[name] !== 'undefined');
             const descriptor = Object.getOwnPropertyDescriptor(globalThis, 'wings');
             let replaced = true;
             try { globalThis.wings = {}; } catch (_) { replaced = false; }
             let import_ = 'loaded';
             const done = (ok) => show({
                 names: Object.getOwnPropertyNames(globalThis), denied, replaced, frozen: Object.isFrozen(wings),
                 writable: descriptor.writable, wings: Object.keys(wings), import: ok,
             });
             import('node:fs').then(() => done('loaded'), () => done('blocked'));",
        ),
        Limits::default(),
    );
    let seen = shown(&page.tree());
    assert_eq!(seen["denied"], serde_json::json!([]));
    assert_eq!(seen["import"], "blocked");
    assert_eq!(seen["wings"], serde_json::json!(["render"]));
    assert_eq!(seen["replaced"], false);
    assert_eq!(seen["frozen"], true);
    assert_eq!(seen["writable"], false);
    let names: BTreeSet<String> = serde_json::from_value(seen["names"].clone()).unwrap();
    let added: Vec<_> = names.difference(&bare).cloned().collect();
    assert_eq!(added, ["clearTimeout", "setTimeout", "wings"]);
}

/// Renders `value` for a test to read with `shown`.
fn show(cx: &mut Cx<'_>, value: serde_json::Value) {
    cx.render(
        &serde_json::json!({ "id": 0, "type": "#root", "props": {}, "children": [
        { "id": 1, "type": "#text", "text": value.to_string() }] }),
    );
}

#[test]
fn timers_run_in_order_after_their_real_delay_with_their_arguments_and_can_be_cleared() {
    /// Sets three timers, `b` first, and shows the order they ran in when the last has. A fake has no way to clear one,
    /// so there is no timer here that is cleared.
    struct Timers {
        started: Instant,
        order: Vec<&'static str>,
        ids: [u32; 3],
    }
    impl Guest for Timers {
        fn start(&mut self, cx: &mut Cx<'_>) {
            self.ids = [cx.later(60), cx.later(20), cx.later(90)];
        }
        fn timer(&mut self, cx: &mut Cx<'_>, id: u32) {
            let [b, a, last] = self.ids;
            self.order.push(match id {
                id if id == b => "b",
                id if id == a => "a",
                id if id == last => "args12",
                _ => return,
            });
            if id == last {
                let elapsed = self.started.elapsed().as_millis() as u64;
                show(
                    cx,
                    serde_json::json!({ "order": self.order, "elapsed": elapsed }),
                );
            }
        }
    }
    let page = start_in_plugin(&fake::source("page", || {
        Box::new(Timers {
            started: Instant::now(),
            order: Vec::new(),
            ids: [0; 3],
        })
    }));
    let seen = shown(&page.tree());
    assert_eq!(seen["order"], serde_json::json!(["a", "b", "args12"]));
    let elapsed = seen["elapsed"].as_u64().unwrap();
    assert!((85..2_000).contains(&elapsed), "ran after {elapsed} ms");
}

/// Starts `source` with a short budget and waits for it to be stopped.
fn stops(source: &str) -> Page {
    let started = Instant::now();
    let page = start(
        source,
        Limits {
            turn: Duration::from_millis(100),
            ..Limits::default()
        },
    );
    loop {
        match page.next() {
            Event::Error {
                fatal: true,
                message,
            } => {
                eprintln!("stopped after {:?}: {message}", started.elapsed());
                assert!(started.elapsed() < Duration::from_secs(5));
                return page;
            }
            Event::Error { .. } | Event::Tree(_) => {}
        }
    }
}

#[test]
fn a_page_that_never_stops_is_stopped_wherever_it_loops() {
    for source in [
        "while (true) {}",
        "const next = () => Promise.resolve().then(next); next();",
        "const next = async () => { await null; next(); }; next();",
        "let held = []; for (;;) held.push(new Array(100000).fill(0));",
    ] {
        let page = stops(source);
        // Stopped for good: the host gets no more clicks in.
        let until = Instant::now() + Duration::from_secs(2);
        while page.runtime.dispatch("1:onClick", None) {
            assert!(Instant::now() < until, "{source}");
            thread::sleep(Duration::from_millis(5));
        }
    }
}

/// A turn that waits hasn't run: here in a host call that takes three times the budget, as any turn does on a Mac too
/// busy to give the page's thread the CPU. Cyclops' sidebar was stopped for good that way.
#[test]
fn a_turn_that_waits_past_its_budget_is_not_stopped() {
    struct Slow;
    impl Link for Slow {
        fn broadcast(&self, _: String) -> Result<(), String> {
            Ok(())
        }
        fn storage(&self, _: Store) -> Result<String, String> {
            thread::sleep(Duration::from_millis(300));
            Ok("null".into())
        }
        fn call(&self, method: &str, _: serde_json::Value, reply: Reply) {
            reply.err(format!("no {method}"));
        }
    }
    let (sender, events) = mpsc::channel();
    // The loop is long enough for the engine to ask whether the turn's time is up.
    let source = report("wings.storage.get('x').then(() => { let n = 0; for (let i = 0; i < 200000; i++) n += i; show('after'); });");
    let limits = Limits {
        turn: Duration::from_millis(100),
        ..Limits::default()
    };
    let runtime = PageRuntime::spawn_plugin(source, Role::Page, limits, Arc::new(Slow), move |event| {
        let _ = sender.send(event);
    })
    .unwrap();
    assert_eq!(shown(&Page { runtime, events }.tree()), "after");
}

#[test]
fn a_page_that_loops_in_a_click_or_a_timer_is_stopped_too() {
    let source = report("show('up'); globalThis.__wingsEvent = () => { for (;;) {} };");
    let page = start(
        &source,
        Limits {
            turn: Duration::from_millis(100),
            ..Limits::default()
        },
    );
    assert_eq!(shown(&page.tree()), "up");
    assert!(page.runtime.dispatch("1:onClick", None));
    while !matches!(page.next(), Event::Error { fatal: true, .. }) {}
    stops(&report(
        "show('up'); setTimeout(() => { for (;;) {} }, 10);",
    ));
}

#[test]
fn an_error_in_a_click_is_shown_and_the_page_goes_on() {
    let page = start(
        &report(
            "show('up');
             globalThis.__wingsEvent = (handler) => { if (handler === 'x:throws') throw new Error('boom'); show(handler); };",
        ),
        Limits::default(),
    );
    assert_eq!(shown(&page.tree()), "up");
    assert!(page.runtime.dispatch("x:throws", None));
    match page.next() {
        Event::Error { message, fatal } => {
            assert!(message.contains("boom") && !fatal, "{message}")
        }
        other => panic!("{other:?}"),
    }
    assert!(page.runtime.dispatch("x:fine", None));
    assert_eq!(shown(&page.tree()), "x:fine");
}

#[test]
fn a_tree_the_host_refuses_is_an_error_and_not_a_tree() {
    struct Refused;
    impl Guest for Refused {
        fn start(&mut self, cx: &mut Cx<'_>) {
            cx.render(&serde_json::json!({ "not": "a tree" }));
            cx.render(&serde_json::Value::String("x".repeat(3 * 1024 * 1024)));
        }
    }
    let page = start_in_plugin(&fake::source("page", || Box::new(Refused)));
    for _ in 0..2 {
        assert!(matches!(page.next(), Event::Error { fatal: false, .. }));
    }
}

/// A link that gives a frame its plugin globals and nothing else: no frame hears a broadcast, and nothing is stored.
struct NoLink;

impl Link for NoLink {
    fn broadcast(&self, _: String) -> Result<(), String> {
        Ok(())
    }

    fn storage(&self, _: Store) -> Result<String, String> {
        Err("no storage".into())
    }

    fn call(&self, method: &str, _: serde_json::Value, reply: Reply) {
        reply.err(format!("no {method}"));
    }
}

fn start_in_plugin(source: &str) -> Page {
    let (sender, events) = mpsc::channel();
    let runtime = PageRuntime::spawn_plugin(
        source,
        Role::Page,
        Limits::default(),
        Arc::new(NoLink),
        move |event| {
            let _ = sender.send(event);
        },
    )
    .unwrap();
    Page { runtime, events }
}

#[test]
fn an_interval_repeats_until_it_is_cleared_and_either_clear_function_stops_it() {
    let page = start_in_plugin(&report(
        "const t0 = Date.now(); const ticks = [];
         const id = setInterval((tag) => {
           ticks.push(tag);
           if (ticks.length === 3) {
             clearInterval(id);
             setTimeout(() => show({ ticks, elapsed: Date.now() - t0 }), 100);
           }
         }, 20, 'tick');
         // A timeout's clear works on an interval too, as in a browser.
         clearTimeout(setInterval(() => ticks.push('cleared'), 5));",
    ));
    let seen = shown(&page.tree());
    assert_eq!(seen["ticks"], serde_json::json!(["tick", "tick", "tick"]));
    let elapsed = seen["elapsed"].as_u64().unwrap();
    assert!((160..2_000).contains(&elapsed), "ran for {elapsed} ms");
}

#[test]
fn a_timer_that_sets_itself_again_does_not_keep_the_frame_from_its_events() {
    struct Again(u32);
    impl Again {
        fn again(&mut self, cx: &mut Cx<'_>) {
            self.0 += 1;
            if self.0 == 3 {
                show(cx, serde_json::json!("spinning"));
            }
            cx.later(0);
        }
    }
    impl Guest for Again {
        fn start(&mut self, cx: &mut Cx<'_>) {
            self.again(cx);
        }
        fn timer(&mut self, cx: &mut Cx<'_>, _id: u32) {
            self.again(cx);
        }
        fn event(&mut self, cx: &mut Cx<'_>, handler: &str, _payload: &serde_json::Value) {
            show(cx, serde_json::json!(handler));
        }
    }
    let page = start_in_plugin(&fake::source("page", || Box::new(Again(0))));
    // The click comes once the timer is going round, not while the bundle is still starting.
    assert_eq!(shown(&page.tree()), "spinning");
    assert!(page.runtime.dispatch("clicked", None));
    assert_eq!(shown(&page.tree()), "clicked");
}

/// How many times each library, by hash, was compiled, so a test can see its pages shared one copy.
pub(super) static COMPILED: std::sync::Mutex<std::collections::BTreeMap<String, usize>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

/// A library like `dist/native/shared.js`: it defines `__wingsLib`, with a count of its own for each instance, a
/// check of what a page throws, and `show` drawing through the page's own `wings`. `tag` makes it a library of its own.
fn library(tag: &str) -> Arc<Library> {
    Library::new(format!(
        "var __wingsLib = (() => {{ let count = 0; return {{
            tag: {tag:?},
            next: () => ++count,
            isError: (thrown) => thrown instanceof Error,
            show: (value) => globalThis.wings.render(JSON.stringify({{ id: 0, type: '#root', props: {{}},
                children: [{{ id: 1, type: '#text', text: JSON.stringify(value) }}] }})),
        }}; }})();"
    ))
}

/// A page built to share `library`, running `body` with the library as `lib`.
fn shared_page(library: &Library, body: &str) -> String {
    format!("{SHARED_MARKER}{}\n(() => {{ const lib = __wingsShared; {body} }})();", library.hash())
}

fn open_in(engine: &Arc<Engine>, source: &str, library: Option<Arc<Library>>) -> Page {
    let (sender, events) = mpsc::channel();
    let runtime = PageRuntime::open_plugin(engine, source, Role::Page, Arc::new(NoLink), library, move |event| {
        let _ = sender.send(event);
    });
    Page { runtime, events }
}

#[test]
fn pages_built_to_share_a_library_compile_it_once_and_each_get_an_instance_and_globals_of_their_own() {
    let lib = library("once");
    let engine = Engine::start(Limits::default()).unwrap();
    let page = "lib.next(); globalThis.mine = (globalThis.mine ?? 0) + 1;
        globalThis.__wingsEvent = (handler) => { lib.show({ handler, count: lib.next(), mine: globalThis.mine }); return true; };
        lib.show({ count: lib.next(), mine: globalThis.mine, wings: typeof wings.render, timers: typeof setTimeout });";
    let first = open_in(&engine, &shared_page(&lib, page), Some(lib.clone()));
    assert_eq!(shown(&first.tree()), serde_json::json!({ "count": 2, "mine": 1, "wings": "function", "timers": "function" }));
    let second = open_in(&engine, &shared_page(&lib, page), Some(lib.clone()));
    // Its own count and its own globals: the first page's don't show through.
    assert_eq!(shown(&second.tree()), serde_json::json!({ "count": 2, "mine": 1, "wings": "function", "timers": "function" }));
    assert!(first.runtime.dispatch("a", None));
    assert_eq!(shown(&first.tree()), serde_json::json!({ "handler": "a", "count": 3, "mine": 1 }));
    assert!(second.runtime.dispatch("b", None));
    assert_eq!(shown(&second.tree()), serde_json::json!({ "handler": "b", "count": 3, "mine": 1 }));
    assert_eq!(COMPILED.lock().unwrap().get(lib.hash()), Some(&1));
}

#[test]
fn a_page_and_its_library_share_a_realm_so_what_the_page_throws_is_an_error_to_the_library() {
    let lib = library("realm");
    let engine = Engine::start(Limits::default()).unwrap();
    let page = open_in(
        &engine,
        &shared_page(&lib, "lib.show({ error: lib.isError(new Error('x')), type: lib.isError(new TypeError('y')) });"),
        Some(lib),
    );
    assert_eq!(shown(&page.tree()), serde_json::json!({ "error": true, "type": true }));
}

#[test]
fn a_page_that_loops_is_stopped_and_the_other_pages_of_its_engine_go_on() {
    let lib = library("isolation");
    let engine = Engine::start(Limits { turn: Duration::from_millis(100), ..Limits::default() }).unwrap();
    let steady = "globalThis.__wingsEvent = (handler) => { lib.show(handler); return true; };
        setInterval(() => {}, 5); lib.show('up');";
    let kept = open_in(&engine, &shared_page(&lib, steady), Some(lib.clone()));
    let own = open_in(&engine, &report("globalThis.__wingsEvent = (handler) => { show(handler); return true; }; show('up');"), None);
    assert_eq!(shown(&kept.tree()), "up");
    assert_eq!(shown(&own.tree()), "up");
    // One loops in a click, one in a chain of promise jobs that would outlive it, one as it starts.
    for looping in [
        "globalThis.__wingsEvent = () => { for (;;) {} }; lib.show('up');",
        "globalThis.__wingsEvent = () => { const next = () => Promise.resolve().then(next); next(); }; lib.show('up');",
    ] {
        let stuck = open_in(&engine, &shared_page(&lib, looping), Some(lib.clone()));
        assert_eq!(shown(&stuck.tree()), "up");
        assert!(stuck.runtime.dispatch("loop", None));
        while !matches!(stuck.next(), Event::Error { fatal: true, .. }) {}
        assert!(!stuck.runtime.dispatch("again", None));
        for (page, said) in [(&kept, "after"), (&own, "after too")] {
            assert!(page.runtime.dispatch(said, None));
            assert_eq!(shown(&page.tree()), said);
        }
    }
    let starting = open_in(&engine, &shared_page(&lib, "for (;;) {}"), Some(lib.clone()));
    assert!(matches!(starting.next(), Event::Error { fatal: true, .. }));
    assert!(kept.runtime.dispatch("still", None));
    assert_eq!(shown(&kept.tree()), "still");
}

#[test]
fn a_page_built_with_another_library_or_none_stops_saying_to_rebuild() {
    let built_with = library("built");
    let engine = Engine::start(Limits::default()).unwrap();
    for given in [None, Some(library("other"))] {
        let page = open_in(&engine, &shared_page(&built_with, "lib.show('up');"), given);
        match page.next() {
            Event::Error { message, fatal: true } => assert!(message.contains("Rebuild the plugin"), "{message}"),
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn a_page_that_brings_its_own_react_runs_beside_shared_pages_with_the_globals_it_always_had() {
    let lib = library("beside");
    let engine = Engine::start(Limits::default()).unwrap();
    let shared = open_in(&engine, &shared_page(&lib, "lib.show('shared');"), Some(lib.clone()));
    assert_eq!(shown(&shared.tree()), "shared");
    // The pilot page bundles React itself, as every page built before pages shared one did.
    let own = open_in(&engine, PILOT, Some(lib));
    let ready = own.until("Ready");
    let handler = button(&ready.children).unwrap();
    assert!(own.runtime.dispatch(&handler, None));
    own.until("Count: 1");
}
