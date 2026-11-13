//! What the JavaScript plugin layer costs: for each installed plugin, starts its main and each native page in QuickJS
//! the way the app does, one frame at a time, and prints the memory the process grew by and the time to the page's
//! first tree. The frames get no data (no access, no panes), so the numbers are each bundle's floor: its engine, its
//! code and an empty first render. A page full of data costs more. macOS only (it reads the process's footprint).
//! Each page's module is what the app runs (`installed::page_source`); the column `shares` is always empty now.
//!
//! cargo run --release --manifest-path apps/native/Cargo.toml --example bench_plugins --features test-support -- [plugins-folder]

use std::{
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};

use wings_native::{
    installed::page_source,
    page::{
        plugin::{PluginConfig, PluginHost},
        runtime::{Event, PageRuntime},
    },
};

/// The process's physical footprint, as Activity Monitor and `footprint` count it.
fn footprint() -> u64 {
    let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        libc::proc_pid_rusage(
            std::process::id() as i32,
            libc::RUSAGE_INFO_V2,
            (&raw mut info).cast(),
        )
    };
    assert_eq!(ok, 0, "proc_pid_rusage failed");
    info.ri_phys_footprint
}

fn mb(bytes: i64) -> String {
    format!("{:.1}", bytes as f64 / 1_048_576.0)
}

/// Waits for the frame's first tree, or its first fatal error, or the time to run out.
fn first(events: &mpsc::Receiver<Event>, started: Instant) -> String {
    match events.recv_timeout(Duration::from_secs(5)) {
        Ok(Event::Tree(_)) => format!("{:.1} ms", started.elapsed().as_secs_f64() * 1000.0),
        Ok(Event::Error { message, fatal: true }) => format!("stopped: {}", message.lines().next().unwrap_or("")),
        Ok(Event::Error { .. }) => first(events, started),
        Err(_) => "no tree in 5 s".into(),
    }
}

fn main() {
    let folder = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| {
        std::env::home_dir()
            .expect("a home folder")
            .join("Library/Application Support/dev.wings.app/plugins")
    });
    let storage = tempfile::tempdir().expect("a storage folder");
    let mut plugins: Vec<PathBuf> = std::fs::read_dir(&folder)
        .expect("the plugins folder")
        .filter_map(|entry| Some(entry.ok()?.path().join("dist/native")))
        .filter(|native| native.is_dir())
        .collect();
    plugins.sort();
    // Keeps every frame alive, so each one's memory stays counted, as the app keeps them.
    let mut hosts: Vec<PluginHost> = Vec::new();
    let mut pages: Vec<PageRuntime> = Vec::new();
    let mut receivers = Vec::new();
    let start = footprint();
    let mut total_bundle = 0u64;
    println!("{:<18} {:<22} {:>9} {:>9} {:>16} {:>7}", "plugin", "frame", "bundle KB", "+MB", "first tree", "shares");
    for native in plugins {
        let id = native.parent().and_then(Path::parent).and_then(Path::file_name).unwrap().to_string_lossy().into_owned();
        let host = PluginHost::new(PluginConfig {
            id: id.clone(),
            root: storage.path().to_owned(),
            scope: "bench".into(),
        });
        let dir = native.parent().and_then(Path::parent).unwrap().to_owned();
        let mut files: Vec<PathBuf> = std::fs::read_dir(&native)
            .unwrap()
            .filter_map(|entry| Some(entry.ok()?.path()))
            .filter(|path| path.extension().is_some_and(|ext| ext == "wasm"))
            .collect();
        // Main first, as the app starts it before any page.
        files.sort_by_key(|path| (path.file_stem().unwrap() != "main", path.clone()));
        for file in files {
            let source = page_source(&dir, &file).unwrap();
            let shares = source.starts_with(wings_native::page::runtime::SHARED_MARKER);
            total_bundle += source.len() as u64;
            let name = file.file_stem().unwrap().to_string_lossy().into_owned();
            let (tx, events) = mpsc::channel();
            let before = footprint() as i64;
            let started = Instant::now();
            let result = if name == "main" {
                host.start_main(&source, move |event| {
                    let _ = tx.send(event);
                })
                .map(|()| "main".to_owned())
            } else {
                host.open_page(&source, move |event| {
                    let _ = tx.send(event);
                })
                .map(|page| {
                    pages.push(page);
                    first(&events, started)
                })
            };
            let shown = match result {
                Ok(text) if text == "main" => {
                    // A main draws nothing: give it the time its first turn and timers take.
                    std::thread::sleep(Duration::from_millis(300));
                    "(main)".to_owned()
                }
                Ok(text) => text,
                Err(error) => format!("didn't start: {error}"),
            };
            std::thread::sleep(Duration::from_millis(100));
            let grew = footprint() as i64 - before;
            println!("{:<18} {:<22} {:>9} {:>9} {:>16} {:>7}", id, name, source.len() / 1024, mb(grew), shown, if shares { "yes" } else { "" });
            receivers.push(events);
        }
        hosts.push(host);
    }
    let all = footprint() as i64 - start as i64;
    println!(
        "\n{} frames, {} KB of bundles, the process grew {} MB in all ({} MB before any plugin)",
        pages.len() + hosts.len(),
        total_bundle / 1024,
        mb(all),
        mb(start as i64)
    );
}
