use super::testing::{Frame, PRELUDE, Rig};
use serde_json::json;

fn focus_source() -> String {
    format!(
        "{PRELUDE} const initial = wings.focused; const states = []; wings.onFocus(value => {{ states.push(value.focused); show({{initial, states}}); }}); wings.onBroadcast(() => show({{initial, states}}));"
    )
}

#[test]
fn auxiliary_window_focus_is_local_to_its_page_but_keeps_main_active() {
    let rig = Rig::new(json!({}), vec![]);
    let primary = Frame::start(&rig.host, &focus_source());
    primary.report("primary starts focused", |r| r["states"] == json!([true]));
    let auxiliary = rig.host.register_window();
    let page = Frame::in_window(&rig.host, auxiliary.id, &focus_source());
    page.report("auxiliary starts inactive", |r| {
        r["states"] == json!([false])
    });
    let reporter = Frame::hidden(&rig.host, &format!("{PRELUDE} wings.onBroadcast(show);"));
    rig.host.start_main("const states=[]; wings.onFocus(({focused})=>{states.push(focused); wings.broadcast(states);});", |_| {}).unwrap();
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
    let first = Frame::hidden(&rig.host, &focus_source());
    let second = Frame::hidden(&rig.host, &focus_source());
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
    let barrier = Frame::start(&rig.host, "wings.broadcast({});");
    assert_eq!(
        second.report("hidden frame stayed unfocused", |r| r["states"].is_array())["states"],
        json!([false, true, false])
    );
    drop(barrier);
    drop(first);
    assert_eq!(rig.host.shared.frames.lock().unwrap().len(), 1);
    let replacement = Frame::hidden(&rig.host, &focus_source());
    replacement.report("replacement starts hidden", |r| {
        r["initial"] == false && r["states"] == json!([false])
    });
}

#[test]
fn main_follows_window_focus_even_when_every_page_is_hidden() {
    let rig = Rig::new(json!({}), vec![]);
    let reporter = Frame::hidden(&rig.host, &format!("{PRELUDE} wings.onBroadcast(show);"));
    rig.host.start_main("const states = []; wings.onFocus(value => { states.push(value.focused); wings.broadcast(states); });", |_| {}).unwrap();
    reporter.report("main initially focused", |r| r == &json!([true]));
    rig.host.publish_focus(false);
    reporter.report("main backgrounded", |r| r == &json!([true, false]));
    rig.host.publish_focus(true);
    reporter.report("main foregrounded", |r| r == &json!([true, false, true]));
}

#[test]
fn real_sdk_git_helper_defers_hidden_and_background_events_until_visible_focus() {
    // These modules have only named imports/exports. Keep the shipped helper bodies, without module linkage.
    let sdk = [
        include_str!("../../../../../packages/plugin-sdk/poll.js"),
        include_str!("../../../../../packages/plugin-sdk/async.js"),
        include_str!("../../../../../packages/plugin-sdk/git.js"),
    ]
    .into_iter()
    .flat_map(|source| source.lines())
    .filter(|line| !line.starts_with("import "))
    .collect::<Vec<_>>()
    .join("\n")
    .replace("export ", "");
    let rig = Rig::new(json!({ "permissions": { "exec": ["git"] } }), vec![]);
    let source = format!(
        "{PRELUDE}\n{sdk}\nlet calls = 0; let batches = []; onGitChange(async ids => {{ calls++; batches.push(ids); show({{calls, batches}}); }}); let barriers = 0; wings.onBroadcast(() => show({{calls, barriers: ++barriers}})); show({{ready:true}});"
    );
    let page = Frame::hidden(&rig.host, &source);
    page.report("git helper ready", |r| r["ready"] == true);
    rig.host.publish_git(&["one".into()]);
    let barrier = Frame::start(&rig.host, "wings.broadcast({});");
    assert_eq!(
        page.report("hidden git barrier", |r| r["barriers"] == 1)["calls"],
        0
    );
    page.set_visible(true);
    page.report("visible git flush", |r| {
        r["calls"] == 1 && r["batches"] == json!([["one"]])
    });
    rig.host.publish_focus(false);
    rig.host.publish_git(&["two".into()]);
    let second = Frame::start(&rig.host, "wings.broadcast({});");
    assert_eq!(
        page.report("background git barrier", |r| r["barriers"] == 2)["calls"],
        1
    );
    rig.host.publish_focus(true);
    page.report("foreground git flush", |r| {
        r["calls"] == 2 && r["batches"] == json!([["one"], ["two"]])
    });
    drop((barrier, second));
}
