//! The trial of plugins written in Rust: Cyclops' main script, sidebar and popover as WebAssembly modules
//! (`wings-cyclops/rust`), over made-up Cyclops answers, never the network or your account.
//!
//! `shot <out-dir> <wings-cyclops>` shows each in the app offscreen at 1000 and 640 points wide, presses Stop, Edit,
//! Save and Start in the sidebar and adds an entry in the popover, and prints what the page shows after. `tools
//! <wings-cyclops>` calls each of the main script's tools as Claude does and prints the answers. `bench <wings-cyclops>
//! [cache-folder]` starts each page alone, as `bench_plugins` does, and prints the memory the process grew by and the
//! time to its first tree.
//!
//! Build the modules first, in `wings-cyclops`: `sh rust/build.sh fake` for `shot` and `tools`, and `sh rust/build.sh`
//! for `bench`.
//!
//! cargo run --release --manifest-path apps/native/Cargo.toml --example trial_wasm_page --features test-support -- shot <out-dir> <wings-cyclops>

use std::{
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use gpui::{AnyWindowHandle, AppContext, Entity, VisualTestAppContext, WindowHandle, px, size};
use wings_native::{
    actions,
    host::Config,
    icons::Assets,
    installed::fixture::Data,
    page::{
        PageView,
        plugin::{PluginConfig, PluginHost},
        runtime::{Event, Reply, wasm},
    },
    workspace::Workspace,
};

fn read(path: PathBuf) -> String {
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("shot") => shot(PathBuf::from(&args[1]), PathBuf::from(&args[2])),
        Some("bench") => {
            // With a folder, compiled modules are kept there, as the app keeps them: run it twice for a second launch.
            if let Some(dir) = args.get(2) {
                wasm::keep_compiled_in(PathBuf::from(dir));
            }
            bench(&PathBuf::from(&args[1]))
        }
        Some("tools") => tools(&PathBuf::from(&args[1])),
        _ => panic!("shot <out-dir> <wings-cyclops>, tools <wings-cyclops>, or bench <wings-cyclops>"),
    }
}

// ---------- bench ----------

/// The process's physical footprint, as Activity Monitor and `footprint` count it.
fn footprint() -> i64 {
    let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::proc_pid_rusage(std::process::id() as i32, libc::RUSAGE_INFO_V2, (&raw mut info).cast()) };
    assert_eq!(ok, 0, "proc_pid_rusage failed");
    info.ri_phys_footprint as i64
}

fn bench(repo: &Path) {
    let storage = tempfile::tempdir().expect("a storage folder");
    let module = repo.join("rust/target/sidebar/wasm32-unknown-unknown/release/cyclops.wasm");
    let wasm = format!("{}{}", wasm::MARKER, module.display());
    let rows: [(&str, String, u64); 2] = [
        ("Rust (WebAssembly)", wasm.clone(), module.metadata().unwrap().len()),
        ("Rust (WebAssembly), 2nd page", wasm, module.metadata().unwrap().len()),
    ];
    println!("{:<30} {:>8} {:>8} {:>12}", "page", "file KB", "+MB", "first tree");
    let mut kept = Vec::new();
    for (name, source, bytes) in rows {
        // No access and no data, as `bench_plugins`: each page's floor, which is its first, loading, tree.
        let host = PluginHost::new(PluginConfig { id: "cyclops".into(), root: storage.path().to_owned(), scope: "bench".into() });
        let (tx, events) = mpsc::channel();
        let before = footprint();
        let started = Instant::now();
        let page = host.open_page(&source, move |event| drop(tx.send(event))).expect("a page");
        let first = loop {
            match events.recv_timeout(Duration::from_secs(5)) {
                Ok(Event::Tree(_)) => break format!("{:.1} ms", started.elapsed().as_secs_f64() * 1000.0),
                Ok(Event::Error { message, fatal: true }) => break format!("stopped: {message}"),
                Ok(Event::Error { .. }) => {}
                Err(_) => break "no tree in 5 s".into(),
            }
        };
        thread::sleep(Duration::from_millis(300));
        println!("{:<30} {:>8} {:>8.1} {:>12}", name, bytes / 1024, (footprint() - before) as f64 / 1_048_576.0, first);
        kept.push((host, page, events));
    }
}

// ---------- shot ----------

fn wait(cx: &mut VisualTestAppContext, window: AnyWindowHandle, duration: Duration) {
    let until = Instant::now() + duration;
    while Instant::now() < until {
        cx.run_until_parked();
        cx.advance_clock(Duration::from_millis(20));
        let _ = cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx));
        thread::sleep(Duration::from_millis(20));
    }
}

enum Step {
    Press(&'static str),
    /// Picks the chip of a row of chips.
    Chip(&'static str),
    Type(Option<&'static str>, &'static str),
}

/// Cyclops installed in a made-up data folder, over made-up answers, as its package has it: the manifest and the three
/// modules (`sh rust/build.sh fake`).
fn install(repo: &Path) -> Data {
    let mut manifest: serde_json::Value = serde_json::from_str(&read(repo.join("wings-plugin.json"))).unwrap();
    manifest.as_object_mut().unwrap().remove("minWingsVersion");
    let data = Data::temp();
    let package = tempfile::tempdir().expect("a folder for the package");
    std::fs::create_dir_all(package.path().join("dist/native")).unwrap();
    std::fs::write(package.path().join("wings-plugin.json"), manifest.to_string()).unwrap();
    for frame in ["main", "sidebar", "popover"] {
        let module = repo.join(format!("rust/target/fake/native/{frame}.wasm"));
        std::fs::copy(&module, package.path().join(format!("dist/native/{frame}.wasm")))
            .unwrap_or_else(|error| panic!("{}: {error}. Run sh rust/build.sh fake", module.display()));
    }
    data.install_folder(package.path(), "shot");
    data
}

fn window(cx: &mut VisualTestAppContext, data: &Data, project: &Path, width: f32) -> WindowHandle<Workspace> {
    let args = ["--project".into(), project.as_os_str().to_owned()];
    let mut config = Config::parse(args, project).unwrap().unwrap();
    config.state = None;
    config.projects = None;
    config.agents = None;
    config.plugins = Some(data.paths());
    cx.open_offscreen_window(size(px(width), px(900.0)), move |window, cx| cx.new(|cx| Workspace::new(config, window, cx))).unwrap()
}

// ---------- tools ----------

fn tools(repo: &Path) {
    let mut cx = VisualTestAppContext::with_asset_source(gpui_platform::current_platform(false), std::sync::Arc::new(Assets));
    cx.executor().allow_parking();
    let project = tempfile::tempdir().expect("a project folder");
    let id = "123e4567-e89b-12d3-a456-426614174000";
    let calls = [
        ("list_projects", serde_json::json!({})),
        ("running_timers", serde_json::json!({})),
        ("stop_timer", serde_json::json!({})),
        ("stop_timer", serde_json::json!({ "timerId": "t2" })),
        ("stop_timer", serde_json::json!({})),
        ("stop_timer", serde_json::json!({})),
        ("start_timer", serde_json::json!({ "projectId": "p1" })),
        ("start_timer", serde_json::json!({ "projectId": id, "task": "  Write the port  " })),
        ("list_entries", serde_json::json!({})),
        ("list_entries", serde_json::json!({ "from": "2026-02-30" })),
        ("list_entries", serde_json::json!({ "from": "2999-01-02", "to": "2999-01-01" })),
        ("update_entry", serde_json::json!({ "entryId": id })),
        ("update_entry", serde_json::json!({ "entryId": id, "startedAt": "yesterday" })),
        ("delete_entry", serde_json::json!({ "entryId": id })),
        ("delete_entry", serde_json::json!({ "entry": id })),
    ];
    let data = install(repo);
    let handle = window(&mut cx, &data, project.path(), 1000.0);
    wait(&mut cx, handle.into(), Duration::from_millis(1500));
    let host = handle.read_with(&cx, |w, _| w.plugin_host("cyclops")).unwrap().expect("Cyclops running");
    for (name, input) in &calls {
        let (reply, answer) = Reply::channel();
        host.run_tool(name, input.clone(), None, reply);
        let answer = match answer.wait(Duration::from_secs(10)) {
            Some(Ok(answer)) => answer,
            Some(Err(error)) => format!("failed: {error}"),
            None => "no answer in 10 s".into(),
        };
        println!("{name} {input}\n  {answer}");
    }
    let _ = cx.update_window(handle.into(), |_, window, _| window.remove_window());
}

fn shot(out: PathBuf, repo: PathBuf) {
    std::fs::create_dir_all(&out).unwrap();
    let mut cx = VisualTestAppContext::with_asset_source(gpui_platform::current_platform(false), std::sync::Arc::new(Assets));
    cx.executor().allow_parking();
    cx.update(actions::bind);
    let project = out.join("project");
    std::fs::create_dir_all(&project).unwrap();

    // A name that starts with "popover" is of the popover, shown under its title bar button.
    let scenarios: &[(&str, f32, &[Step])] = &[
        ("popover", 1000.0, &[]),
        ("popover", 640.0, &[]),
        ("popover-manual", 1000.0, &[Step::Chip("Manual")]),
        (
            "popover-add",
            1000.0,
            &[Step::Chip("Manual"), Step::Type(Some("What are you working on?"), "Standup"), Step::Type(Some("Start"), "0100"), Step::Type(Some("End"), "0130"), Step::Press("Add")],
        ),
        ("popover-timer", 1000.0, &[Step::Type(Some("What are you working on?"), "From the popover"), Step::Press("Start")]),
        ("first", 1000.0, &[]),
        ("first", 640.0, &[]),
        ("stop", 1000.0, &[Step::Press("Stop")]),
        ("edit-timer", 1000.0, &[Step::Press("Edit Timer one timer"), Step::Type(Some("Task"), "X"), Step::Press("Save")]),
        ("edit-entry", 1000.0, &[Step::Press("Edit Code review")]),
        ("edit-entry-time", 1000.0, &[Step::Press("Edit Code review"), Step::Type(Some("Start"), "0700"), Step::Press("Save")]),
        ("delete-entry", 1000.0, &[Step::Press("Delete Code review"), Step::Press("Delete")]),
        ("start", 1000.0, &[Step::Type(Some("What are you working on?"), "New task"), Step::Press("Start")]),
        ("week", 1000.0, &[Step::Press("Week")]),
    ];
    for (name, width, steps) in scenarios {
        let data = install(&repo);
        let handle = window(&mut cx, &data, &project, *width);
        let window: AnyWindowHandle = handle.into();
        wait(&mut cx, window, Duration::from_millis(500));
        let in_popover = name.starts_with("popover");
        handle.update(&mut cx, |w, _, cx| if in_popover { w.shot_popover("cyclops", "tracker", cx) } else { w.shot_sidebar("cyclops", "tracker", cx) }).unwrap();
        wait(&mut cx, window, Duration::from_millis(200));
        let page: Entity<PageView> = handle
            .read_with(&cx, |w, _| if in_popover { w.popover_page("cyclops", "tracker") } else { w.sidebar_page("cyclops", "tracker") })
            .unwrap()
            .expect("the page");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !cx.read(|cx| page.read(cx).text()).contains("Timer one") && Instant::now() < deadline {
            wait(&mut cx, window, Duration::from_millis(50));
        }
        for step in *steps {
            match step {
                Step::Press(label) | Step::Chip(label) => {
                    let chip = matches!(step, Step::Chip(_));
                    let deadline = Instant::now() + Duration::from_secs(3);
                    while !cx.read(|cx| if chip { page.read(cx).press_chip(label) } else { page.read(cx).press(label, 0) }) {
                        if Instant::now() > deadline {
                            println!("{name}: NOTHING TO PRESS for {label}");
                            break;
                        }
                        wait(&mut cx, window, Duration::from_millis(50));
                    }
                }
                Step::Type(field, text) => match cx.read(|cx| match field {
                    Some(label) => page.read(cx).input_focus(label, cx),
                    None => page.read(cx).first_input_focus(cx),
                }) {
                    Some(focus) => {
                        let _ = cx.update_window(window, |_, window, cx| focus.focus(window, cx));
                        wait(&mut cx, window, Duration::from_millis(100));
                        cx.simulate_input(window, text);
                    }
                    None => println!("{name}: NO FIELD {field:?}"),
                },
            }
            wait(&mut cx, window, Duration::from_millis(600));
        }
        wait(&mut cx, window, Duration::from_millis(300));
        println!("{name}-{width}: {}", cx.read(|cx| page.read(cx).text()));
        let _ = cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx));
        cx.capture_screenshot(window).expect("render the window").save(out.join(format!("{name}-{width}.png"))).expect("save the PNG");
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
}
