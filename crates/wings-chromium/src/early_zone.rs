//! A malloc zone that lets Chromium's allocator take over malloc while Wings' threads allocate.
//!
//! When the CEF framework loads, a static constructor of Chromium's PartitionAlloc makes its zone malloc's default
//! (`InitializeDefaultMallocZoneWithPartitionAlloc`, `partition_alloc/shim/allocator_shim_override_apple_default_zone.h`
//! at 158). With the system's zone first, it takes that zone out of malloc's list and puts it back, and a `free` on
//! another thread in between finds no zone for its block and stops the process
//! (`allocator_shim::TryFreeDefaultFallbackToFindZoneAndFree`). Wings loads CEF when the Browser first needs it, with
//! dozens of threads allocating.
//!
//! Chrome's `main` registers a zone named `DelegatingDefaultZoneForPartitionAlloc` first, before it has threads
//! (`base/allocator/early_zone_registration_apple.cc`), which PartitionAlloc then swaps for its own while the system
//! zone stays in the list. This is that zone, with two differences, as it stays first for all of a run in which
//! Chromium never starts: it answers for the system zone's blocks, so `realloc` finds them on malloc's fast path
//! (2.4 times slower across 8 threads otherwise, measured), and it passes on the typed calls, so the system zone still
//! sorts allocations by type.

use std::ffi::{c_char, c_void, CStr};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::Once;

/// The name PartitionAlloc looks for to swap its zone in for this one.
const NAME: &CStr = c"DelegatingDefaultZoneForPartitionAlloc";

/// `malloc_zone_t` (macOS SDK `malloc/malloc.h`), through version 16.
#[repr(C)]
pub struct Zone {
    reserved1: *mut c_void,
    reserved2: *mut c_void,
    size: Option<unsafe extern "C" fn(*mut Zone, *const c_void) -> usize>,
    malloc: Option<unsafe extern "C" fn(*mut Zone, usize) -> *mut c_void>,
    calloc: Option<unsafe extern "C" fn(*mut Zone, usize, usize) -> *mut c_void>,
    valloc: Option<unsafe extern "C" fn(*mut Zone, usize) -> *mut c_void>,
    free: Option<unsafe extern "C" fn(*mut Zone, *mut c_void)>,
    realloc: Option<unsafe extern "C" fn(*mut Zone, *mut c_void, usize) -> *mut c_void>,
    destroy: Option<unsafe extern "C" fn(*mut Zone)>,
    zone_name: *const c_char,
    batch_malloc: Option<unsafe extern "C" fn(*mut Zone, usize, *mut *mut c_void, u32) -> u32>,
    batch_free: Option<unsafe extern "C" fn(*mut Zone, *mut *mut c_void, u32)>,
    introspect: *mut Introspection,
    version: u32,
    memalign: Option<unsafe extern "C" fn(*mut Zone, usize, usize) -> *mut c_void>,
    free_definite_size: Option<unsafe extern "C" fn(*mut Zone, *mut c_void, usize)>,
    pressure_relief: Option<unsafe extern "C" fn(*mut Zone, usize) -> usize>,
    claimed_address: Option<unsafe extern "C" fn(*mut Zone, *mut c_void) -> i32>,
    try_free_default: Option<unsafe extern "C" fn(*mut Zone, *mut c_void)>,
    malloc_with_options: Option<unsafe extern "C" fn(*mut Zone, usize, usize, u64) -> *mut c_void>,
    malloc_type_malloc: Option<unsafe extern "C" fn(*mut Zone, usize, u64) -> *mut c_void>,
    malloc_type_calloc: Option<unsafe extern "C" fn(*mut Zone, usize, usize, u64) -> *mut c_void>,
    malloc_type_realloc: Option<unsafe extern "C" fn(*mut Zone, *mut c_void, usize, u64) -> *mut c_void>,
    malloc_type_memalign: Option<unsafe extern "C" fn(*mut Zone, usize, usize, u64) -> *mut c_void>,
    malloc_type_malloc_with_options: Option<unsafe extern "C" fn(*mut Zone, usize, usize, u64, u64) -> *mut c_void>,
}

/// `malloc_introspection_t`.
#[repr(C)]
struct Introspection {
    enumerator: Option<unsafe extern "C" fn(u32, *mut c_void, u32, usize, *mut c_void, *mut c_void) -> i32>,
    good_size: Option<unsafe extern "C" fn(*mut Zone, usize) -> usize>,
    check: Option<unsafe extern "C" fn(*mut Zone) -> i32>,
    print: Option<unsafe extern "C" fn(*mut Zone, i32)>,
    log: Option<unsafe extern "C" fn(*mut Zone, *mut c_void)>,
    force_lock: Option<unsafe extern "C" fn(*mut Zone)>,
    force_unlock: Option<unsafe extern "C" fn(*mut Zone)>,
    statistics: Option<unsafe extern "C" fn(*mut Zone, *mut Statistics)>,
    zone_locked: Option<unsafe extern "C" fn(*mut Zone) -> i32>,
    enable_discharge_checking: Option<unsafe extern "C" fn(*mut Zone) -> i32>,
    disable_discharge_checking: Option<unsafe extern "C" fn(*mut Zone)>,
    discharge: Option<unsafe extern "C" fn(*mut Zone, *mut c_void)>,
    enumerate_discharged_pointers: *mut c_void,
    reinit_lock: Option<unsafe extern "C" fn(*mut Zone)>,
    print_task: *mut c_void,
    task_statistics: *mut c_void,
    zone_type: u32,
}

/// `malloc_statistics_t`.
#[repr(C)]
#[derive(Default)]
struct Statistics {
    blocks_in_use: u32,
    size_in_use: usize,
    max_size_in_use: usize,
    size_allocated: usize,
}

extern "C" {
    static mach_task_self_: u32;
    fn malloc_get_all_zones(task: u32, reader: *const c_void, zones: *mut *const *mut Zone, count: *mut u32) -> i32;
    fn malloc_zone_register(zone: *mut Zone);
    fn malloc_zone_unregister(zone: *mut Zone);
    fn malloc_default_purgeable_zone() -> *mut Zone;
}

/// The system's zone, which every call goes on to.
static SYSTEM: AtomicPtr<Zone> = AtomicPtr::new(std::ptr::null_mut());

/// Puts the zone first in malloc's list. Call it at the start of `main`, before any thread starts.
pub fn register() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        // As in Chrome, the purgeable zone, made now if it wasn't, ends up behind the system zone.
        let purgeable = malloc_default_purgeable_zone();
        let system = zones()[0];
        SYSTEM.store(system, Ordering::Relaxed);
        let zone = Box::leak(Box::new(delegating(&*system)));
        // Registering appends, and unregistering moves the last zone into the place it frees: ours goes first.
        malloc_zone_register(zone);
        malloc_zone_unregister(system);
        malloc_zone_register(system);
        malloc_zone_unregister(purgeable);
        malloc_zone_register(purgeable);
    });
}

/// The names of malloc's zones, the default first.
pub fn names() -> Vec<String> {
    zones()
        .iter()
        .map(|zone| match unsafe { (**zone).zone_name } {
            name if name.is_null() => String::new(),
            name => unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned(),
        })
        .collect()
}

fn zones() -> Vec<*mut Zone> {
    let (mut zones, mut count) = (std::ptr::null(), 0);
    // With no reader, malloc's own list, which registering changes: copied.
    unsafe {
        malloc_get_all_zones(mach_task_self_, std::ptr::null(), &mut zones, &mut count);
        std::slice::from_raw_parts(zones, count as usize).to_vec()
    }
}

fn delegating(system: &Zone) -> Zone {
    let version = system.version.min(16);
    // Only what the system zone has, as it would have it first.
    let has = |since: u32, present: bool| version >= since && present;
    let introspect = Box::leak(Box::new(Introspection {
        enumerator: Some(enumerator),
        good_size: Some(good_size),
        check: Some(check),
        print: Some(print),
        log: Some(log),
        // The system zone locks and unlocks itself around a fork, as it's in the list too.
        force_lock: Some(no_lock),
        force_unlock: Some(no_lock),
        statistics: Some(statistics),
        zone_locked: Some(zone_locked),
        enable_discharge_checking: None,
        disable_discharge_checking: None,
        discharge: None,
        enumerate_discharged_pointers: std::ptr::null_mut(),
        reinit_lock: Some(no_lock),
        print_task: std::ptr::null_mut(),
        task_statistics: std::ptr::null_mut(),
        zone_type: 0,
    }));
    Zone {
        reserved1: std::ptr::null_mut(),
        reserved2: std::ptr::null_mut(),
        size: Some(size),
        malloc: Some(malloc),
        calloc: Some(calloc),
        valloc: Some(valloc),
        free: Some(free),
        realloc: Some(realloc),
        destroy: None,
        zone_name: NAME.as_ptr(),
        batch_malloc: system.batch_malloc.is_some().then_some(batch_malloc),
        batch_free: system.batch_free.is_some().then_some(batch_free),
        introspect,
        version,
        memalign: has(5, system.memalign.is_some()).then_some(memalign),
        free_definite_size: has(6, system.free_definite_size.is_some()).then_some(free_definite_size),
        // The system zone gives memory back for itself, as it's in the list too.
        pressure_relief: has(8, true).then_some(pressure_relief),
        claimed_address: None,
        try_free_default: has(13, system.try_free_default.is_some()).then_some(try_free_default),
        malloc_with_options: has(15, system.malloc_with_options.is_some()).then_some(malloc_with_options),
        malloc_type_malloc: has(16, system.malloc_type_malloc.is_some()).then_some(malloc_type_malloc),
        malloc_type_calloc: has(16, system.malloc_type_calloc.is_some()).then_some(malloc_type_calloc),
        malloc_type_realloc: has(16, system.malloc_type_realloc.is_some()).then_some(malloc_type_realloc),
        malloc_type_memalign: has(16, system.malloc_type_memalign.is_some()).then_some(malloc_type_memalign),
        malloc_type_malloc_with_options: has(16, system.malloc_type_malloc_with_options.is_some())
            .then_some(malloc_type_malloc_with_options),
    }
}

/// A zone function that calls the system zone's own with the system zone.
macro_rules! forward {
    ($name:ident($($arg:ident: $ty:ty),*) $(-> $ret:ty)?) => {
        unsafe extern "C" fn $name(_: *mut Zone, $($arg: $ty),*) $(-> $ret)? {
            let system = SYSTEM.load(Ordering::Relaxed);
            unsafe { ((*system).$name.unwrap())(system, $($arg),*) }
        }
    };
}

forward!(size(ptr: *const c_void) -> usize);
forward!(malloc(size: usize) -> *mut c_void);
forward!(calloc(count: usize, size: usize) -> *mut c_void);
forward!(valloc(size: usize) -> *mut c_void);
forward!(free(ptr: *mut c_void));
forward!(realloc(ptr: *mut c_void, size: usize) -> *mut c_void);
forward!(batch_malloc(size: usize, results: *mut *mut c_void, count: u32) -> u32);
forward!(batch_free(ptrs: *mut *mut c_void, count: u32));
forward!(memalign(alignment: usize, size: usize) -> *mut c_void);
forward!(free_definite_size(ptr: *mut c_void, size: usize));
forward!(try_free_default(ptr: *mut c_void));
forward!(malloc_with_options(alignment: usize, size: usize, options: u64) -> *mut c_void);
forward!(malloc_type_malloc(size: usize, type_id: u64) -> *mut c_void);
forward!(malloc_type_calloc(count: usize, size: usize, type_id: u64) -> *mut c_void);
forward!(malloc_type_realloc(ptr: *mut c_void, size: usize, type_id: u64) -> *mut c_void);
forward!(malloc_type_memalign(alignment: usize, size: usize, type_id: u64) -> *mut c_void);
forward!(malloc_type_malloc_with_options(alignment: usize, size: usize, options: u64, type_id: u64) -> *mut c_void);

unsafe extern "C" fn pressure_relief(_: *mut Zone, _: usize) -> usize {
    0
}

/// Nothing to list: the system zone lists the blocks this zone answers for.
unsafe extern "C" fn enumerator(_: u32, _: *mut c_void, _: u32, _: usize, _: *mut c_void, _: *mut c_void) -> i32 {
    0
}

unsafe extern "C" fn good_size(_: *mut Zone, size: usize) -> usize {
    let system = SYSTEM.load(Ordering::Relaxed);
    unsafe { ((*(*system).introspect).good_size.unwrap())(system, size) }
}

unsafe extern "C" fn check(_: *mut Zone) -> i32 {
    1
}

unsafe extern "C" fn print(_: *mut Zone, _: i32) {}

unsafe extern "C" fn log(_: *mut Zone, _: *mut c_void) {}

unsafe extern "C" fn no_lock(_: *mut Zone) {}

unsafe extern "C" fn statistics(_: *mut Zone, stats: *mut Statistics) {
    unsafe { stats.write(Statistics::default()) }
}

unsafe extern "C" fn zone_locked(_: *mut Zone) -> i32 {
    0
}
