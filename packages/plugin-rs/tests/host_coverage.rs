//! The SDK has a typed function or listener for every call the app accepts from a plugin frame, and for every message
//! the app sends one. The names are read from the app's source, so a call added there fails this test until
//! `src/host.rs` (or another module of the SDK) wraps it.
//!
//! The app's side is the match arms of its call dispatch and the `Message::name` table; the SDK's side is every
//! `call("name", ...)` and `on("name", ...)` in `src`. A name that is on purpose not wrapped goes in `NOT_WRAPPED` or
//! `NOT_HEARD` with the reason.

mod common;

use common::*;
use std::collections::BTreeSet;

/// Calls the SDK doesn't wrap, and why.
const NOT_WRAPPED: &[(&str, &str)] = &[];

/// Calls the SDK sends that the native app does not accept (they only worked in the old JavaScript desktop app), and why
/// they are still sent.
const NOT_ACCEPTED: &[(&str, &str)] = &[("installTool", "`cli::Cli::install` asks for it; the app has no handler yet, so it answers `unknown method installTool`")];

/// Messages the SDK doesn't hear as a listener, and why.
const NOT_HEARD: &[(&str, &str)] = &[
    ("resolve", "answers a call, `host::call` waits for it"),
    ("reject", "answers a call, `host::call` waits for it"),
    ("output", "a line of a running call, `host::exec_lines` and `spawn` hear it"),
    ("broadcast", "`host::on_broadcast`, which has its own branch in the dispatcher"),
    ("focus", "`host::on_focus`, which keeps the last value"),
];

/// Every call the app's frames can make.
fn host_calls() -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    // The dispatch of `Shared`: its own arms, the forge calls, the ones that take a tab, and the pages' calls.
    let calls = code(&read("apps/native/src/page/plugin/calls.rs"));
    names.extend(arms(between(&calls, "fn tab_call", "fn open_view(")));
    names.extend(arms(between(&calls, "pub(super) fn call(", "\"unknown method")));
    let forge = code(&read("apps/native/src/page/plugin/calls/pr_tracker.rs"));
    names.extend(arms(between(&forge, "pub(super) fn forge_call", "\"unknown method")));
    let pages = code(&read("apps/native/src/page/plugin/pages.rs"));
    names.extend(arms(between(&pages, "pub(super) fn call(", "_ =>")));
    // The frame's own link answers some before the rest.
    let link = code(&read("apps/native/src/page/plugin.rs"));
    let link = between(&link, "fn call(&self, method: &str, mut params", "self.shared.call(method, params, reply)");
    let link_arms = arms(link);
    for (name, before, _) in literals(link) {
        if is_name(&name) && (before.ends_with("==") || before == "\"|" || link_arms.contains(&name)) {
            names.insert(name);
        }
    }
    // The process calls.
    let process = code(&read("apps/native/src/page/plugin/process.rs"));
    names.extend(literals(between(&process, "pub(super) const CALLS", "];")).into_iter().map(|(name, ..)| name).filter(|name| is_name(name)));
    names
}

/// Every message the app sends a frame.
fn host_messages() -> BTreeSet<String> {
    let runtime = code(&read("apps/native/src/page/runtime/mod.rs"));
    literals(between(&runtime, "fn name(self)", "\n    }\n}")).into_iter().map(|(name, ..)| name).filter(|name| is_name(name)).collect()
}

#[test]
fn the_scan_reads_the_app() {
    // A scan that found little has lost its markers or its patterns: don't pass on an empty list.
    let calls = host_calls();
    for known in ["exec", "setBadge", "openPane", "tabState", "namePage", "processArm", "diskFind", "openDiff", "closeTab", "sendToAgent"] {
        assert!(calls.contains(known), "{known} is not in {calls:?}");
    }
    assert!(!calls.contains("working"), "{calls:?}");
    assert!(host_messages().contains("agentMessage"));
}

#[test]
fn every_call_the_app_accepts_has_a_wrapper() {
    let wrapped = sdk_names(&["call(", "call_as(id, ", "send_changed(&SENT_BADGES, ", "send_changed(&SENT_STATUSES, "]);
    let missing: Vec<_> = host_calls().into_iter().filter(|name| !wrapped.contains(name) && !NOT_WRAPPED.iter().any(|(skipped, _)| skipped == name)).collect();
    assert!(missing.is_empty(), "the app accepts these calls and the SDK has no typed function for them (add one to src/host.rs): {missing:?}");
}

#[test]
fn every_message_the_app_sends_has_a_listener() {
    let heard = sdk_names(&["on(", "answered("]);
    let missing: Vec<_> = host_messages().into_iter().filter(|name| !heard.contains(name) && !NOT_HEARD.iter().any(|(skipped, _)| skipped == name)).collect();
    assert!(missing.is_empty(), "the app sends these messages and the SDK has no listener for them (add one to src/host.rs): {missing:?}");
}

#[test]
fn what_is_left_out_is_still_in_the_app() {
    let calls = host_calls();
    let messages = host_messages();
    for (name, _) in NOT_WRAPPED {
        assert!(calls.contains(*name), "NOT_WRAPPED names {name}, which the app no longer accepts");
    }
    for (name, _) in NOT_HEARD {
        assert!(messages.contains(*name), "NOT_HEARD names {name}, which the app no longer sends");
    }
}

#[test]
fn the_sdk_sends_only_calls_the_app_accepts() {
    let accepted = host_calls();
    let wrong: Vec<_> = sdk_names(&["call(", "call_as(id, ", "send_changed(&SENT_BADGES, ", "send_changed(&SENT_STATUSES, "])
        .into_iter()
        // `call("x"` also opens test code and doc examples that name a method of their own.
        .filter(|name| is_name(name) && !accepted.contains(name) && !NOT_ACCEPTED.iter().any(|(skipped, _)| skipped == name))
        .collect();
    assert!(wrong.is_empty(), "the SDK sends these calls and the app answers unknown method: {wrong:?}");
    for (name, _) in NOT_ACCEPTED {
        assert!(!accepted.contains(*name), "NOT_ACCEPTED names {name}, which the app accepts now: drop it from the list");
    }
}
