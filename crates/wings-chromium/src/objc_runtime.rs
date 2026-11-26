//! The Objective-C runtime calls the crate uses to change classes it doesn't own (NSApp's, NSBundle's, CEF's window).

use std::ffi::c_char;

use objc2::runtime::{AnyClass, AnyObject, AnyProtocol, Bool, Imp, Sel};

extern "C" {
    pub fn object_getClass(object: *const AnyObject) -> *const AnyClass;
    pub fn class_getName(class: *const AnyClass) -> *const c_char;
    pub fn class_addMethod(class: *const AnyClass, name: Sel, imp: Imp, types: *const c_char) -> Bool;
    pub fn class_replaceMethod(class: *const AnyClass, name: Sel, imp: Imp, types: *const c_char) -> Option<Imp>;
    pub fn class_getMethodImplementation(class: *const AnyClass, name: Sel) -> Option<Imp>;
    pub fn class_addProtocol(class: *const AnyClass, protocol: *const AnyProtocol) -> Bool;
    pub fn objc_getProtocol(name: *const c_char) -> *const AnyProtocol;
    pub fn objc_setAssociatedObject(object: *const AnyObject, key: *const u8, value: *const AnyObject, policy: usize);
    pub fn objc_getAssociatedObject(object: *const AnyObject, key: *const u8) -> *const AnyObject;
}
