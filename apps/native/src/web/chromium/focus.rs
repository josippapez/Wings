//! The keyboard between Wings and a Chromium page. The page is a window of its own over Wings', so clicking into it
//! makes that window key: Wings still looks and acts as the active window then (`count_children_as_key`), and its own
//! shortcuts pressed in the page go back to it (`keys`, `to_wings`). A click anywhere in Wings makes Wings' window key
//! again, which AppKit does by itself.

use objc2::{Message, msg_send, runtime::AnyObject};
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSWindow};
use wings_chromium::window as frameless;

use super::{keys::{Press, named_key}, page::Shared};

/// The page's window became or stopped being the key window: GPUI reads its own window's state again, which counts a
/// page over it having the keyboard as its own (`frameless::count_children_as_key`). On the next turn, once AppKit has
/// settled which window is key.
pub fn page_active(page: &Shared, _active: bool) {
    let Some(parent) = page.parent.borrow().clone() else { return };
    resync(&parent);
}

/// GPUI's window `parent` reads whether it is active again.
pub fn resync(parent: &NSWindow) {
    frameless::count_children_as_key(parent);
    let parent = parent.retain();
    super::next_turn(move || {
        let Some(delegate) = parent.delegate() else { return };
        let delegate: &AnyObject = (*delegate).as_ref();
        // GPUI's delegate reads the window's `isKeyWindow` whichever of the two it hears.
        let key: bool = parent.isKeyWindow();
        // SAFETY: GPUI's window delegate implements both, and ignores the notification.
        unsafe {
            if key {
                let _: () = msg_send![delegate, windowDidBecomeKey: std::ptr::null::<AnyObject>()];
            } else {
                let _: () = msg_send![delegate, windowDidResignKey: std::ptr::null::<AnyObject>()];
            }
        }
    });
}

/// A key press as `keys` reads it.
pub fn press_of(event: &NSEvent) -> Option<Press> {
    let flags = event.modifierFlags();
    let key = match named_key(event.keyCode()) {
        Some(named) => named.to_owned(),
        None => event.charactersIgnoringModifiers()?.to_string().to_lowercase(),
    };
    (!key.is_empty()).then(|| Press {
        key,
        cmd: flags.contains(NSEventModifierFlags::Command),
        ctrl: flags.contains(NSEventModifierFlags::Control),
        alt: flags.contains(NSEventModifierFlags::Option),
        shift: flags.contains(NSEventModifierFlags::Shift),
    })
}

/// Hands the press `event` to Wings' window `parent`, which takes the keyboard: Cmd-T opens a Wings tab, Cmd-K the
/// palette, as anywhere else in Wings.
pub fn to_wings(event: &NSEvent, parent: &NSWindow) {
    parent.makeKeyWindow();
    let (Some(characters), Some(ignoring)) = (event.characters(), event.charactersIgnoringModifiers()) else { return };
    let copy = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
        event.r#type(),
        event.locationInWindow(),
        event.modifierFlags(),
        event.timestamp(),
        parent.windowNumber(),
        None,
        &characters,
        &ignoring,
        event.isARepeat(),
        event.keyCode(),
    );
    // Straight to Wings' window, on the next turn, out of the page's handling of the press: AppKit hands a posted key
    // press only to the key window.
    if let Some(copy) = copy {
        let parent = parent.retain();
        super::next_turn(move || parent.sendEvent(&copy));
    }
}
