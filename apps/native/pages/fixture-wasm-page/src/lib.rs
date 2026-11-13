//! A fixture page in Rust with the controls the Rust SDK adds for Cyclops' popover: a row of chips to pick one of, a
//! chip that is on or off, and a time field. Built by the tests that run it (`page::plugin::wasm_tests`).

use std::cell::RefCell;
use wings_plugin::ui;

struct State {
    range: String,
    billable: bool,
    start: String,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State { range: "week".into(), billable: false, start: "08:30".into() });
}

fn change(change: impl FnOnce(&mut State)) {
    STATE.with(|state| change(&mut state.borrow_mut()));
}

#[unsafe(no_mangle)]
pub fn start() {
    wings_plugin::run(|| {
        let args = wings_plugin::host::view_args();
        STATE.with(|state| {
            let state = state.borrow();
            vec![
                // Named by what its tab was opened with, when it was.
                ui::chips(args.as_ref().and_then(|args| args["label"].as_str()).unwrap_or("Range"), &[("week", "Week"), ("month", "Month")], &state.range, |value| {
                    let value = value.to_owned();
                    change(|state| state.range = value);
                }),
                ui::chip("Billable", state.billable, || change(|state| state.billable = !state.billable)),
                ui::text_input("Start", &state.start, |text| change(|state| state.start = text)).prop("type", "time"),
            ]
        })
    });
}
