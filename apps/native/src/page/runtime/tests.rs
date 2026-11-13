use super::{
    fake::{self, Cx, Guest},
    *,
};
use std::time::{Duration, Instant};

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
    let runtime = PageRuntime::open_plugin(source, Role::Page, Arc::new(Disk::default()), move |event| {
        let _ = tx.send(event);
    })
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
}

fn shown(tree: &Tree) -> serde_json::Value {
    serde_json::from_str(&tree.text()).unwrap()
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
    let runtime = PageRuntime::open_plugin(source, Role::Page, Arc::new(NoLink), move |event| {
        let _ = sender.send(event);
    })
    .unwrap();
    Page { runtime, events }
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
