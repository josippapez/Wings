use super::testing::{Frame, Rig, barrier, echoes, show};
use crate::page::runtime::fake::{self, Cx, Guest};
use serde_json::json;

/// A frame that follows the window's focus as `wings.onFocus` hands it over: the first message always counts, and after
/// that only a change does. `main` broadcasts the states it has seen, and a page shows them, with the focus it started with.
fn follows_focus(main: bool) -> String {
    struct Follows {
        main: bool,
        initial: bool,
        states: Vec<bool>,
    }
    impl Guest for Follows {
        fn start(&mut self, cx: &mut Cx<'_>) {
            self.initial = cx.link().focused();
        }
        fn deliver(&mut self, cx: &mut Cx<'_>, kind: &str, _id: u32, text: &str) {
            let focused = match kind {
                "focus" => {
                    serde_json::from_str::<serde_json::Value>(text).unwrap()["focused"] == true
                }
                "broadcast" if !self.main => {
                    return show(
                        cx,
                        json!({ "initial": self.initial, "states": self.states }),
                    );
                }
                _ => return,
            };
            if Some(&focused) == self.states.last() {
                return;
            }
            self.states.push(focused);
            if self.main {
                cx.link().broadcast(json!(self.states).to_string()).unwrap();
            } else {
                show(
                    cx,
                    json!({ "initial": self.initial, "states": self.states }),
                );
            }
        }
    }
    fake::source("focus", move || {
        Box::new(Follows {
            main,
            initial: false,
            states: Vec::new(),
        })
    })
}

#[test]
fn auxiliary_window_focus_is_local_to_its_page_but_keeps_main_active() {
    let rig = Rig::new(json!({}), vec![]);
    let primary = Frame::start(&rig.host, &follows_focus(false));
    primary.report("primary starts focused", |r| r["states"] == json!([true]));
    let auxiliary = rig.host.register_window();
    let page = Frame::in_window(&rig.host, auxiliary.id, &follows_focus(false));
    page.report("auxiliary starts inactive", |r| {
        r["states"] == json!([false])
    });
    let reporter = Frame::hidden(&rig.host, &echoes());
    rig.host.start_main(&follows_focus(true), |_| {}).unwrap();
    reporter.report("main starts focused", |r| r == &json!([true]));
    auxiliary.set_active(true);
    rig.host.publish_focus(false);
    primary.report("primary loses focus", |r| {
        r["states"] == json!([true, false])
    });
    page.report("auxiliary gains focus", |r| {
        r["states"] == json!([false, true])
    });
    auxiliary.set_active(false);
    reporter.report("all windows backgrounded", |r| r == &json!([true, false]));
    auxiliary.set_active(true);
    reporter.report("main follows active auxiliary", |r| {
        r == &json!([true, false, true])
    });
    drop(auxiliary);
    page.report("disposed window loses focus", |r| {
        r["states"] == json!([false, true, false, true, false])
    });
    assert_eq!(rig.host.shared.focused.lock().unwrap().len(), 1);
}

#[test]
fn page_focus_combines_window_activation_with_each_frames_visibility() {
    let rig = Rig::new(json!({}), vec![]);
    let first = Frame::hidden(&rig.host, &follows_focus(false));
    let second = Frame::hidden(&rig.host, &follows_focus(false));
    for frame in [&first, &second] {
        assert_eq!(
            frame.report("hidden startup", |r| r["states"] == json!([false]))["initial"],
            false
        );
    }
    first.set_visible(true);
    first.report("first shown", |r| r["states"] == json!([false, true]));
    first.set_visible(false);
    second.set_visible(true);
    first.report("first hidden", |r| {
        r["states"] == json!([false, true, false])
    });
    second.report("second shown", |r| r["states"] == json!([false, true]));
    rig.host.publish_focus(false);
    second.report("window backgrounded", |r| {
        r["states"] == json!([false, true, false])
    });
    second.set_visible(false);
    first.set_visible(true);
    rig.host.publish_focus(true);
    first.report("only visible frame regains focus", |r| {
        r["states"] == json!([false, true, false, true])
    });
    let barrier = Frame::start(&rig.host, &barrier());
    assert_eq!(
        second.report("hidden frame stayed unfocused", |r| r["states"].is_array())["states"],
        json!([false, true, false])
    );
    drop(barrier);
    drop(first);
    assert_eq!(rig.host.shared.frames.lock().unwrap().len(), 1);
    let replacement = Frame::hidden(&rig.host, &follows_focus(false));
    replacement.report("replacement starts hidden", |r| {
        r["initial"] == false && r["states"] == json!([false])
    });
}

#[test]
fn main_follows_window_focus_even_when_every_page_is_hidden() {
    let rig = Rig::new(json!({}), vec![]);
    let reporter = Frame::hidden(&rig.host, &echoes());
    rig.host.start_main(&follows_focus(true), |_| {}).unwrap();
    reporter.report("main initially focused", |r| r == &json!([true]));
    rig.host.publish_focus(false);
    reporter.report("main backgrounded", |r| r == &json!([true, false]));
    rig.host.publish_focus(true);
    reporter.report("main foregrounded", |r| r == &json!([true, false, true]));
}
