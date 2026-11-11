//! The trial of plugin pages written in Rust: Cyclops' sidebar as a WebAssembly module (`wings-cyclops/rust`) beside
//! its TypeScript build, both over made-up Cyclops answers, never the network or your account.
//!
//! `shot <out-dir> <wings-cyclops>` shows each in the app offscreen at 1000 and 640 points wide, presses Stop, Edit,
//! Save and Start in it, and prints what the page shows after, so the two can be compared line by line.
//! `bench <wings-cyclops> [cache-folder]` starts each page alone, as `bench_plugins` does, and prints the memory the process grew by
//! and the time to its first tree.
//!
//! Build the modules first, in `wings-cyclops/rust`:
//!   cargo build --release --target wasm32-unknown-unknown
//!   cargo build --release --target wasm32-unknown-unknown --features fake --target-dir target/fake
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
    installed::{fixture::Data, library, page_source},
    page::{
        PageView,
        plugin::{PluginConfig, PluginHost},
        runtime::{Event, wasm},
    },
    workspace::Workspace,
};

/// The answers `wings-cyclops/rust/src/fake.rs` makes up, for the TypeScript page.
const CYCLOPS: &str = r#"
globalThis.__shot = (() => {
  const now = Date.now(), minute = 60e3;
  const iso = (t) => new Date(t).toISOString();
  const pad = (n) => String(n).padStart(2, '0');
  const local = (t) => { const d = new Date(t); return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`; };
  const projects = [{ id: 'p1', name: 'Wings', clientName: 'Prototyp' }, { id: 'p2', name: 'Website', clientName: 'Acme' },
    { id: 'p3', name: 'Storefront rebuild for the spring launch', clientName: 'Northwind Traders' }];
  const timer = (id, projectId, task, minutes) => ({ id, projectId, task, startedAt: iso(now - minutes * minute), endedAt: null, entryDate: local(now) });
  let timers = [timer('t1', 'p1', 'Timer one', 65), timer('t2', 'p2', 'Timer two', 20)];
  const entry = (id, projectId, task, days, hoursAgo, minutes) => {
    const start = Math.floor((now - days * 24 * 60 * minute - hoursAgo * 60 * minute) / minute) * minute;
    return { id, projectId, task, startedAt: iso(start), endedAt: iso(start + minutes * minute), entryDate: local(start), durationMinutes: minutes };
  };
  let entries = [
    entry('e1', 'p1', 'Code review', 0, 3, 45),
    entry('e6', 'p2', 'Report table keeps the "Unassigned" group last when sorted ascending', 0, 5, 60),
    entry('e2', 'p2', 'Checkout redesign', 1, 3, 210),
  ];
  const answer = (body) => ({ status: 200, contentType: 'application/json', body: JSON.stringify(body) });
  return {
    has: async () => true,
    fetch: async (url, options = {}) => {
      const path = String(url).replace('https://cyclops-api.prototyp.digital/api', '');
      const method = options.method ?? 'GET';
      const one = path.startsWith('/time-entries/') ? path.slice('/time-entries/'.length) : null;
      if (method === 'POST' && one === 'timer') {
        timers.push({ ...JSON.parse(options.body), id: `t${Date.now()}`, startedAt: iso(Date.now()), endedAt: null, entryDate: local(Date.now()) });
        return answer({});
      }
      if (method === 'POST' && one?.endsWith('/stop')) {
        const stopped = timers.find((t) => t.id === one.slice(0, -'/stop'.length));
        if (stopped) {
          timers = timers.filter((t) => t !== stopped);
          entries = [{ ...stopped, endedAt: iso(Date.now()), durationMinutes: Math.round((Date.now() - Date.parse(stopped.startedAt)) / minute) }, ...entries];
        }
        return answer({});
      }
      if (method === 'PATCH' && one) {
        const patch = (e) => (e.id === one ? { ...e, ...JSON.parse(options.body) } : e);
        timers = timers.map(patch);
        entries = entries.map(patch);
        return answer({});
      }
      if (method === 'DELETE' && one) {
        timers = timers.filter((e) => e.id !== one);
        entries = entries.filter((e) => e.id !== one);
        return answer({});
      }
      if (path.startsWith('/auth/me')) return answer({ name: 'Ada Lovelace', timezone: 'Europe/Zagreb' });
      if (path.startsWith('/projects')) return answer(projects);
      if (path.startsWith('/time-entries/timers')) return answer(timers);
      return answer(entries);
    },
  };
})();
"#;

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
        _ => panic!("shot <out-dir> <wings-cyclops>, or bench <wings-cyclops>"),
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
    let module = repo.join("rust/target/wasm32-unknown-unknown/release/cyclops_sidebar.wasm");
    let bundle = repo.join("dist/native/sidebar.js");
    let shared = library(repo);
    let wasm = format!("{}{}", wasm::MARKER, module.display());
    let rows: [(&str, String, bool, u64); 5] = [
        ("TypeScript, own React", read(bundle.clone()), false, bundle.metadata().unwrap().len()),
        ("TypeScript, shared React", page_source(repo, &bundle, shared.as_deref()).unwrap(), true, 0),
        ("TypeScript, shared, 2nd page", page_source(repo, &bundle, shared.as_deref()).unwrap(), true, 0),
        ("Rust (WebAssembly)", wasm.clone(), false, module.metadata().unwrap().len()),
        ("Rust (WebAssembly), 2nd page", wasm, false, module.metadata().unwrap().len()),
    ];
    println!("{:<30} {:>8} {:>8} {:>12}", "page", "file KB", "+MB", "first tree");
    let mut kept = Vec::new();
    for (name, source, shares, bytes) in rows {
        // No access and no data, as `bench_plugins`: each page's floor, which is its first, loading, tree.
        let host = PluginHost::new(PluginConfig { id: "cyclops".into(), root: storage.path().to_owned(), scope: "bench".into() });
        host.set_library(shares.then(|| shared.clone()).flatten());
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
        let bytes = if bytes == 0 { source.len() as u64 } else { bytes };
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
    Type(Option<&'static str>, &'static str),
}

fn shot(out: PathBuf, repo: PathBuf) {
    std::fs::create_dir_all(&out).unwrap();
    let mut cx = VisualTestAppContext::with_asset_source(gpui_platform::current_platform(false), std::sync::Arc::new(Assets));
    cx.executor().allow_parking();
    cx.update(actions::bind);
    let fake = |page: &str| {
        let source = read(repo.join(format!("dist/native/{page}.js")))
            .replace("wings.fetch(", "globalThis.__shot.fetch(")
            .replace("wings.secrets.has(", "globalThis.__shot.has(");
        format!("{CYCLOPS}\n{source}")
    };
    let (main, sidebar, popover) = (fake("main"), fake("sidebar"), fake("popover"));
    let module = repo.join("rust/target/fake/wasm32-unknown-unknown/release/cyclops_sidebar.wasm");
    let mut manifest: serde_json::Value = serde_json::from_str(&read(repo.join("wings-plugin.json"))).unwrap();
    manifest.as_object_mut().unwrap().remove("minWingsVersion");
    let project = out.join("project");
    std::fs::create_dir_all(&project).unwrap();

    let scenarios: &[(&str, f32, &[Step])] = &[
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
        for rust in [false, true] {
            let kind = if rust { "rust" } else { "ts" };
            let data = Data::temp();
            let dir = data.install_script("cyclops", manifest.clone(), &main, &[("sidebar", &sidebar), ("popover", &popover)], "shot");
            if rust {
                std::fs::copy(&module, dir.join("dist/native/sidebar.wasm")).expect("the module built with --features fake");
            }
            let args = ["--project".into(), project.as_os_str().to_owned()];
            let mut config = Config::parse(args, &project).unwrap().unwrap();
            config.state = None;
            config.projects = None;
            config.agents = None;
            config.plugins = Some(data.paths());
            let handle: WindowHandle<Workspace> = cx
                .open_offscreen_window(size(px(*width), px(900.0)), move |window, cx| cx.new(|cx| Workspace::new(config, window, cx)))
                .unwrap();
            let window: AnyWindowHandle = handle.into();
            wait(&mut cx, window, Duration::from_millis(500));
            handle.update(&mut cx, |w, _, cx| w.shot_sidebar("cyclops", "tracker", cx)).unwrap();
            wait(&mut cx, window, Duration::from_millis(200));
            let page: Entity<PageView> = handle.read_with(&cx, |w, _| w.sidebar_page("cyclops", "tracker")).unwrap().expect("a sidebar page");
            let deadline = Instant::now() + Duration::from_secs(10);
            while !cx.read(|cx| page.read(cx).text()).contains("Timer one") && Instant::now() < deadline {
                wait(&mut cx, window, Duration::from_millis(50));
            }
            for step in *steps {
                match step {
                    Step::Press(label) => {
                        let deadline = Instant::now() + Duration::from_secs(3);
                        while !cx.read(|cx| page.read(cx).press(label, 0)) {
                            if Instant::now() > deadline {
                                println!("{kind} {name}: NOTHING TO PRESS for {label}");
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
                        None => println!("{kind} {name}: NO FIELD {field:?}"),
                    },
                }
                wait(&mut cx, window, Duration::from_millis(600));
            }
            wait(&mut cx, window, Duration::from_millis(300));
            println!("{kind:<4} {name}-{width}: {}", cx.read(|cx| page.read(cx).text()));
            let _ = cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx));
            cx.capture_screenshot(window).expect("render the window").save(out.join(format!("{name}-{width}-{kind}.png"))).expect("save the PNG");
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
    }
}
