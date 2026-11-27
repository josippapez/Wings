//! The malloc zone Wings registers first (`wings_chromium::early_zone`), and Chromium's allocator taking over malloc
//! from it as the engine loads while threads allocate, as in Wings.

#![cfg(target_os = "macos")]

use std::{
    alloc::Layout,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use wings_chromium::{early_zone, engine::Engine};

const DELEGATING: &str = "DelegatingDefaultZoneForPartitionAlloc";
const SYSTEM: &str = "DefaultMallocZone";

/// Every kind of allocation works through the zone, blocks made before it went first included.
#[test]
fn the_zone_goes_first_and_hands_every_allocation_to_the_system_zone() {
    let mut before: Vec<Vec<u8>> = (0..64).map(|i| vec![i as u8; 100 + i * 50]).collect();
    early_zone::register();
    let names = early_zone::names();
    assert_eq!(names[0], DELEGATING, "{names:?}");
    assert!(names.iter().any(|name| name == SYSTEM), "{names:?}");

    for (i, block) in before.iter_mut().enumerate() {
        block.extend_from_slice(&[0xab; 10_000]);
        assert!(block[..100 + i * 50].iter().all(|byte| *byte == i as u8));
    }
    drop(before);
    let zeroed = vec![0u64; 1 << 16];
    assert!(zeroed.iter().all(|word| *word == 0));
    let layout = Layout::from_size_align(10_000, 4096).unwrap();
    let aligned = unsafe { std::alloc::alloc(layout) };
    assert!(!aligned.is_null() && aligned as usize % 4096 == 0);
    unsafe { std::alloc::dealloc(aligned, layout) };
    let grown: String = (0..100_000)
        .map(|i| char::from(b'a' + (i % 26) as u8))
        .collect();
    assert_eq!(grown.len(), 100_000);
}

/// Chromium's allocator takes over malloc as the engine loads while 16 threads allocate, grow and free memory, as
/// Wings' history indexing does. Without the zone first, one of their frees lands while the system zone is out of
/// malloc's list and the process stops in `TryFreeDefaultFallbackToFindZoneAndFree`. Needs an engine:
/// `WINGS_CHROMIUM_ENGINE=<…/chromium/156.0.31/Wings Chromium.app>`, run with `--ignored chromium_takes_over`.
#[test]
#[ignore]
fn chromium_takes_over_malloc_while_threads_allocate() {
    let bundle = std::env::var_os("WINGS_CHROMIUM_ENGINE")
        .expect("set WINGS_CHROMIUM_ENGINE to the engine's .app");
    early_zone::register();
    let stop = Arc::new(AtomicBool::new(false));
    let threads: Vec<_> = (0..16)
        .map(|seed| {
            let stop = stop.clone();
            std::thread::spawn(move || churn(seed, &stop))
        })
        .collect();
    std::thread::sleep(Duration::from_millis(20));
    Engine::new(PathBuf::from(bundle)).load().unwrap();
    std::thread::sleep(Duration::from_millis(100));
    stop.store(true, Ordering::Relaxed);
    let ops: u64 = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .sum();
    assert!(ops > 0);
    let names = early_zone::names();
    assert_eq!(names[0], "PartitionAlloc", "{names:?}");
    assert!(names.iter().any(|name| name == SYSTEM), "{names:?}");
    assert!(!names.iter().any(|name| name == DELEGATING), "{names:?}");
}

/// Replaces and grows blocks until `stop`, returning how many times.
fn churn(seed: u64, stop: &AtomicBool) -> u64 {
    let mut blocks: Vec<Vec<u8>> = (0..256).map(|i| vec![0; 16 + i]).collect();
    let (mut x, mut ops) = (seed + 1, 0);
    while !stop.load(Ordering::Relaxed) {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let block = &mut blocks[(x % 256) as usize];
        if x % 4 == 0 {
            block.extend_from_slice(&[1; 512]);
        } else {
            *block = vec![0; 16 + (x >> 8) as usize % 4096];
        }
        ops += 1;
    }
    ops
}
