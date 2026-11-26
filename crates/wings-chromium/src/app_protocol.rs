//! CEF on macOS needs `NSApp` to implement `CefAppProtocol`: answer `isHandlingSendEvent` and `setHandlingSendEvent:`,
//! and set the flag while `sendEvent:` dispatches an event. Chromium asks `conformsToProtocol:@protocol(CrAppProtocol)`
//! to pick its message pump and uses the flag to tell an event's nested run loop from its own.
//!
//! CEF's samples get there by making `NSApp` an `NSApplication` subclass of their own. In Wings GPUI creates `NSApp`
//! from its own subclass, so [`install`] adds the methods and the protocols to whatever class `NSApp` already has, at
//! run time.

use std::ffi::CStr;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use objc2::runtime::{AnyObject, Bool, Sel};
use objc2::{class, msg_send, sel};

use crate::objc_runtime::*;

static HANDLING_SEND_EVENT: AtomicBool = AtomicBool::new(false);
/// The `sendEvent:` the class had before [`install`] wrapped it (its own or the one it inherits).
static ORIGINAL_SEND_EVENT: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

extern "C-unwind" fn is_handling_send_event(_this: *mut AnyObject, _cmd: Sel) -> Bool {
    Bool::new(HANDLING_SEND_EVENT.load(Ordering::Relaxed))
}

extern "C-unwind" fn set_handling_send_event(_this: *mut AnyObject, _cmd: Sel, handling: Bool) {
    HANDLING_SEND_EVENT.store(handling.as_bool(), Ordering::Relaxed);
}

extern "C-unwind" fn send_event(this: *mut AnyObject, cmd: Sel, event: *mut AnyObject) {
    let was_handling = HANDLING_SEND_EVENT.swap(true, Ordering::Relaxed);
    let original = ORIGINAL_SEND_EVENT.load(Ordering::Relaxed);
    let original: extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) = unsafe { std::mem::transmute(original) };
    original(this, cmd, event);
    HANDLING_SEND_EVENT.store(was_handling, Ordering::Relaxed);
}

/// What [`install`] did, for the host to log.
#[derive(Debug)]
pub struct Installed {
    pub class: String,
    /// The protocols found in the loaded framework and added to the class.
    pub protocols: Vec<&'static str>,
    /// Whether `NSApp` now answers `conformsToProtocol:` for `CrAppProtocol`, which is what Chromium checks.
    pub conforms: bool,
}

/// Adds `CefAppProtocol` to the class of `NSApp`. Call it on the main thread after `NSApp` exists and after the CEF
/// framework is loaded (the protocols are registered by the framework), before `cef::initialize`.
pub fn install() -> Result<Installed, String> {
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let class = object_getClass(app);
        let name = CStr::from_ptr(class_getName(class)).to_string_lossy().into_owned();

        let is_handling: extern "C-unwind" fn(*mut AnyObject, Sel) -> Bool = is_handling_send_event;
        let set_handling: extern "C-unwind" fn(*mut AnyObject, Sel, Bool) = set_handling_send_event;
        // `B` is BOOL on arm64. class_addMethod leaves a method the class already has alone.
        if !class_addMethod(class, sel!(isHandlingSendEvent), std::mem::transmute(is_handling), c"B@:".as_ptr())
            .as_bool()
            || !class_addMethod(class, sel!(setHandlingSendEvent:), std::mem::transmute(set_handling), c"v@:B".as_ptr())
                .as_bool()
        {
            return Err(format!("{name} already has isHandlingSendEvent; it may already be a CEF app"));
        }

        let original = class_getMethodImplementation(class, sel!(sendEvent:)).ok_or("no sendEvent:")?;
        ORIGINAL_SEND_EVENT.store(original as *mut (), Ordering::Relaxed);
        let wrapper: extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) = send_event;
        class_replaceMethod(class, sel!(sendEvent:), std::mem::transmute(wrapper), c"v@:@".as_ptr());

        let mut protocols = Vec::new();
        for protocol in [c"CrAppProtocol", c"CrAppControlProtocol", c"CefAppProtocol"] {
            let found = objc_getProtocol(protocol.as_ptr());
            if !found.is_null() && class_addProtocol(class, found).as_bool() {
                protocols.push(protocol.to_str().unwrap_or_default());
            }
        }
        let cr_app = objc_getProtocol(c"CrAppProtocol".as_ptr());
        let conforms = !cr_app.is_null() && {
            let conforms: bool = msg_send![app, conformsToProtocol: cr_app];
            conforms
        };
        Ok(Installed { class: name, protocols, conforms })
    }
}
