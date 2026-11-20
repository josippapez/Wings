//! The `performance` tool (`claude::performance`): what it counts over its window while the test draws frames, that the
//! readout is as it was afterwards, and its limits.

use std::{
    io::{BufRead, BufReader, Write},
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, Sender, channel},
    },
    time::Duration,
};

use gpui::{TestApp, TestAppWindow};
use serde_json::{Value, json};

use super::{
    Workspace,
    claude::performance,
    plugin_tests::{open_plugins, until},
    tests::open_config,
};
use crate::{installed::fixture::Data, ui::readout};

/// What the tool's two waits tell the test, which draws in between.
enum Wait {
    /// The frame counter is in the window's tree once the window has drawn.
    Settling,
    /// The measured window has started.
    Started,
}

/// Runs the tool with `arguments`, and draws `frames` frames, each one the workspace renders in, inside its measured
/// window. The tool's waits block until the test says go, and the test draws only then, so the count is exact. Gives the
/// tool's answer and how long it waited each time.
fn measure(
    app: &TestApp,
    window: &mut TestAppWindow<Workspace>,
    arguments: Value,
    frames: usize,
) -> (Result<String, String>, Vec<Duration>) {
    let env = window.read(|w, _| w.plugin_env.clone());
    let (said, heard): (Sender<Wait>, Receiver<Wait>) = channel();
    let (go, gone) = channel::<()>();
    let waits = Arc::new(Mutex::new(Vec::new()));
    let recorded = waits.clone();
    let tool = std::thread::spawn(move || {
        performance(&env, &arguments, &|wait| {
            let mut waits = recorded.lock().unwrap();
            waits.push(wait);
            let first = waits.len() == 1;
            drop(waits);
            let _ = said.send(if first { Wait::Settling } else { Wait::Started });
            let _ = gone.recv();
        })
    });
    while !tool.is_finished() {
        match heard.try_recv() {
            Ok(Wait::Settling) => {
                window.draw();
                go.send(()).unwrap();
            }
            Ok(Wait::Started) => {
                for _ in 0..frames {
                    window.update(|_, _, cx| cx.notify());
                }
                go.send(()).unwrap();
                continue;
            }
            Err(_) => {}
        }
        app.run_until_parked();
        std::thread::sleep(Duration::from_millis(1));
    }
    let answer = tool.join().unwrap();
    let waits = waits.lock().unwrap().clone();
    (answer, waits)
}

fn view_count(answer: &Value, view: &str) -> Option<u64> {
    answer["renders"].as_array()?.iter().find(|r| r["view"] == view)?["count"].as_u64()
}

#[test]
fn it_counts_the_frames_and_renders_the_test_draws_and_leaves_the_readout_off() {
    let (app, mut window) = open_config(|_| {});
    window.draw();
    let (answer, waits) = measure(&app, &mut window, json!({ "seconds": 2 }), 7);
    let answer: Value = serde_json::from_str(&answer.unwrap()).unwrap();
    assert_eq!(answer["frames"], 7, "{answer}");
    assert_eq!(view_count(&answer, "workspace"), Some(7), "{answer}");
    assert!(answer["cpuPercent"].is_number() && answer["cpuPercent"].as_f64().unwrap() >= 0.0, "{answer}");
    assert!(answer["framesPerSecond"].as_f64().unwrap() > 0.0, "{answer}");
    assert_eq!(answer["cardShown"], false);
    assert!(answer["summary"].as_str().unwrap().contains("7 frames"), "{answer}");
    assert_eq!(waits[1], Duration::from_secs(2));
    // As it was: off, counting nothing, and the card's own tally untouched.
    assert!(window.read(|w, cx| {
        let readout = w.parts.as_ref().unwrap().readout.read(cx);
        !readout.is_on() && !readout.counting() && !readout.is_measuring()
    }));
    for _ in 0..3 {
        window.draw();
    }
    assert_eq!(readout::counted(), (0, 0));
}

#[test]
fn with_the_card_on_it_stays_on_and_its_own_counts_go_on() {
    let (app, mut window) = open_config(|_| {});
    window.draw();
    window.update(|w, _, cx| w.toggle_readout(cx));
    window.draw();
    let (frames_before, _) = readout::counted();
    let (answer, _) = measure(&app, &mut window, json!({}), 4);
    let answer: Value = serde_json::from_str(&answer.unwrap()).unwrap();
    assert_eq!(answer["frames"], 4, "{answer}");
    assert_eq!(answer["cardShown"], true);
    assert!(window.read(|w, cx| w.parts.as_ref().unwrap().readout.read(cx).is_on()));
    assert!(readout::counted().0 >= frames_before + 4, "the card's tally lost the measured frames");
}

#[test]
fn seconds_is_one_to_thirty_and_three_without_it_and_a_number() {
    let (app, mut window) = open_config(|_| {});
    window.draw();
    let seconds = |window: &mut TestAppWindow<Workspace>, arguments: Value| {
        measure(&app, window, arguments, 0).1.last().copied()
    };
    assert_eq!(seconds(&mut window, json!({})), Some(Duration::from_secs(3)));
    assert_eq!(seconds(&mut window, json!({ "seconds": 0 })), Some(Duration::from_secs(1)));
    assert_eq!(seconds(&mut window, json!({ "seconds": -5 })), Some(Duration::from_secs(1)));
    assert_eq!(seconds(&mut window, json!({ "seconds": 500 })), Some(Duration::from_secs(30)));
    assert_eq!(seconds(&mut window, json!({ "seconds": 2.5 })), Some(Duration::from_millis(2500)));
    let (answer, waits) = measure(&app, &mut window, json!({ "seconds": "soon" }), 0);
    assert!(answer.unwrap_err().contains("seconds is a number"));
    assert!(waits.is_empty(), "it waited and counted for a request it refused");
    assert!(window.read(|w, cx| !w.parts.as_ref().unwrap().readout.read(cx).counting()));
}

#[test]
fn a_second_measurement_waits_for_the_first_and_leaves_it_running() {
    let (app, mut window) = open_config(|_| {});
    window.draw();
    window.update(|w, _, cx| w.measure_begin(cx).unwrap());
    let (answer, waits) = measure(&app, &mut window, json!({ "seconds": 1 }), 0);
    assert!(answer.unwrap_err().contains("already running"));
    assert!(waits.is_empty());
    assert!(window.read(|w, cx| w.parts.as_ref().unwrap().readout.read(cx).is_measuring()));
    window.update(|w, _, cx| {
        let _ = w.measure_end(cx);
    });
}

/// Claude's `performance` on the app's socket, as `wings performance` calls it: the tool is listed, and the answer is JSON.
#[test]
fn the_tool_is_on_the_socket_and_answers_json() {
    let data = Data::temp();
    let socket = data.path().join("mcp.sock");
    let (app, mut window) = open_plugins(&data, &Arc::default(), |config| config.socket = Some(socket.clone()));
    let ask = |window: &mut TestAppWindow<Workspace>, request: Value| {
        let answer = Arc::new(Mutex::new(None));
        let (found, socket) = (answer.clone(), socket.clone());
        std::thread::spawn(move || {
            let stream = std::os::unix::net::UnixStream::connect(socket).unwrap();
            writeln!(&stream, "{request}").unwrap();
            for line in BufReader::new(&stream).lines() {
                let message: Value = serde_json::from_str(&line.unwrap()).unwrap_or(Value::Null);
                if message.get("id") == Some(&json!(1)) {
                    *found.lock().unwrap() = Some(message);
                    return;
                }
            }
        });
        until(&app, window, "the app never answered on the socket", |_| answer.lock().unwrap().is_some());
        let reply = answer.lock().unwrap().take().unwrap();
        reply
    };
    let list = ask(&mut window, json!({ "id": 1, "op": "list" }));
    let tool = list["tools"].as_array().unwrap().iter().find(|t| t["name"] == "performance").expect("performance is listed");
    assert_eq!(tool["inputSchema"]["properties"]["seconds"]["type"], "number");
    let reply = ask(&mut window, json!({ "id": 1, "op": "call", "name": "performance", "arguments": { "seconds": 1 } }));
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    let answer: Value = serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(answer["cpuPercent"].is_number() && answer["frames"].is_number() && answer["renders"].is_array(), "{answer}");
    assert!(answer["summary"].is_string());
    assert!(window.read(|w, cx| !w.parts.as_ref().unwrap().readout.read(cx).counting()));
}
