use super::*;
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
    let source = r#"
        const controller = new AbortController();
        let looked = 0;
        wings.diskFind('/fixture', {
            names: ['.git'], signal: controller.signal,
            onProgress: progress => { looked = progress.entries; controller.abort(); controller.abort(); }
        }).catch(error => wings.render(JSON.stringify({ type: '#root', id: 0, children: [
            { type: '#text', id: 1, text: JSON.stringify({ looked, stopped: controller.signal.aborted, error: error.message }) }
        ] })));
    "#;
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

#[test]
fn timers_run_in_order_after_their_real_delay_with_their_arguments_and_can_be_cleared() {
    let page = start(
        &report(
            "const t0 = Date.now(); const order = [];
             setTimeout(() => order.push('b'), 60);
             setTimeout(() => order.push('a'), 20);
             clearTimeout(setTimeout(() => order.push('cleared'), 30));
             setTimeout((x, y) => { order.push('args' + x + y); show({ order, elapsed: Date.now() - t0 }); }, 90, 1, 2);",
        ),
        Limits::default(),
    );
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
    let page = start(
        "wings.render('{\"not\":\"a tree\"}'); wings.render('x'.repeat(3 * 1024 * 1024));",
        Limits::default(),
    );
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
    let page = start_in_plugin(&report(
        "let runs = 0;
         const again = () => { if (++runs === 3) show('spinning'); setTimeout(again, 0); };
         again();
         globalThis.__wingsEvent = (handler) => { show(handler); return true; };",
    ));
    // The click comes once the timer is going round, not while the bundle is still starting.
    assert_eq!(shown(&page.tree()), "spinning");
    assert!(page.runtime.dispatch("clicked", None));
    assert_eq!(shown(&page.tree()), "clicked");
}
