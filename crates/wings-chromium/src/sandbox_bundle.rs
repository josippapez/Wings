//! Lets Chromium's child process sandbox read the engine, which is outside the host app.
//!
//! A sandboxed child (renderer, GPU, network, ...) may only read inside `bundle-path` (sandbox/policy/mac/common.sb), which
//! the browser process sets from `base::apple::MainBundle()`: `+[NSBundle mainBundle]`, the host app. The engine is
//! outside it, so a child can't load the framework or read its resources, and every child dies at start. CEF's
//! `main_bundle_path` setting doesn't change this; it only overrides Chromium's "outer" bundle.
//!
//! [`install`] makes `+[NSBundle mainBundle]` answer the engine's bundle when the caller is code inside the CEF
//! framework, and the real main bundle to everyone else (AppKit, GPUI, the host).
//!
//! debt: it reads the caller from the frame record, which holds as long as `objc_msgSend` reaches this method by a
//! branch and the framework calls `+mainBundle` itself. The clean fix is in CEF: name `framework_dir_path` (or a
//! setting of its own) as readable in the child sandbox, as Chromium already does for `component-path`.

use std::ffi::{c_char, c_void, CString};
use std::os::unix::ffi::OsStrExt;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{class, msg_send, sel};
use objc2_foundation::NSString;

use crate::objc_runtime::{class_getMethodImplementation, class_replaceMethod, object_getClass};

#[repr(C)]
struct DlInfo {
    file: *const c_char,
    base: *mut c_void,
    symbol: *const c_char,
    address: *mut c_void,
}

extern "C" {
    fn dladdr(address: *const c_void, info: *mut DlInfo) -> i32;
    fn dlopen(path: *const c_char, mode: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

const RTLD_LAZY: i32 = 0x1;
const RTLD_NOLOAD: i32 = 0x10;

static CEF_IMAGE: AtomicUsize = AtomicUsize::new(0);
static ENGINE_BUNDLE: AtomicPtr<AnyObject> = AtomicPtr::new(std::ptr::null_mut());
static ORIGINAL: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

fn image_of(address: *const c_void) -> usize {
    let mut info = DlInfo { file: std::ptr::null(), base: std::ptr::null_mut(), symbol: std::ptr::null(), address: std::ptr::null_mut() };
    if unsafe { dladdr(address, &mut info) } == 0 {
        return 0;
    }
    info.base as usize
}

#[inline(never)]
extern "C-unwind" fn main_bundle(this: *mut AnyObject, cmd: Sel) -> *mut AnyObject {
    // The return address in this function's frame record: objc_msgSend branches here without a call of its own, so it
    // points into whoever sent `mainBundle`.
    let frame: usize;
    unsafe { std::arch::asm!("mov {}, x29", out(reg) frame) };
    let caller = unsafe { *(frame as *const usize).add(1) } as *const c_void;
    if image_of(caller) == CEF_IMAGE.load(Ordering::Relaxed) {
        return ENGINE_BUNDLE.load(Ordering::Relaxed);
    }
    let original: extern "C-unwind" fn(*mut AnyObject, Sel) -> *mut AnyObject =
        unsafe { std::mem::transmute(ORIGINAL.load(Ordering::Relaxed)) };
    original(this, cmd)
}

/// Call it after loading the framework and before `cef::initialize`.
pub fn install(engine: &crate::engine::Engine) -> Result<(), String> {
    unsafe {
        // Looked up in the framework itself: the wrapper cef-rs links into the host defines a `cef_initialize` too.
        let library = CString::new(engine.library().as_os_str().as_bytes()).map_err(|error| error.to_string())?;
        let handle = dlopen(library.as_ptr(), RTLD_LAZY | RTLD_NOLOAD);
        let symbol = if handle.is_null() { handle } else { dlsym(handle, c"cef_initialize".as_ptr()) };
        if symbol.is_null() {
            return Err("the CEF framework isn't loaded".into());
        }
        CEF_IMAGE.store(image_of(symbol), Ordering::Relaxed);

        let engine = engine.bundle();
        let path = NSString::from_str(&engine.to_string_lossy());
        let bundle: *mut AnyObject = msg_send![class!(NSBundle), bundleWithPath: &*path];
        if bundle.is_null() {
            return Err(format!("no bundle at {}", engine.display()));
        }
        let _: *mut AnyObject = msg_send![bundle, retain];
        ENGINE_BUNDLE.store(bundle, Ordering::Relaxed);

        let metaclass = object_getClass(class!(NSBundle) as *const AnyClass as *const AnyObject);
        let original = class_getMethodImplementation(metaclass, sel!(mainBundle)).ok_or("no +mainBundle")?;
        ORIGINAL.store(original as *mut (), Ordering::Relaxed);
        let replacement: extern "C-unwind" fn(*mut AnyObject, Sel) -> *mut AnyObject = main_bundle;
        class_replaceMethod(metaclass, sel!(mainBundle), std::mem::transmute(replacement), c"@@:".as_ptr());
    }
    Ok(())
}
