//! The keyboard between Wings and a Chromium page. The page is a window of its own over Wings', so clicking into it
//! makes that window key: Wings still looks and acts as the active window then (`count_children_as_key`), and its own
//! shortcuts pressed in the page go back to it (`keys`, `to_wings`). A click anywhere in Wings makes Wings' window key
//! again, which AppKit does by itself.

use objc2::{Message, msg_send, runtime::AnyObject};
use objc2_app_kit::{NSApplication, NSEvent, NSEventMask, NSEventModifierFlags, NSEventType, NSWindow};
use objc2_foundation::NSPoint;
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

/// The pointer went from Wings' window `parent` into a page's or a popup's window over it: GPUI's view hears it moved
/// out of the window, as it hears nothing while the pointer is over another window, so what it showed for where the
/// pointer was goes. A button's tooltip stayed over the popup the button opened for as long as the popup showed.
pub fn pointer_left(parent: &NSWindow) {
    let Some(gpui) = parent.contentView().as_deref().and_then(crate::web::mac::gpui_among) else { return };
    let outside = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
        NSEventType::MouseMoved,
        NSPoint::new(-10_000.0, -10_000.0),
        NSEventModifierFlags::empty(),
        0.0,
        parent.windowNumber(),
        None,
        0,
        0,
        0.0,
    );
    if let Some(outside) = outside {
        gpui.mouseMoved(&outside);
    }
}

/// Which window a press of the mouse gives the keyboard to first, before AppKit hands it out, if any: the window
/// pressed (`pressed`, of the app's active windows) when it isn't the key window (`key`) and one of the two is a page's
/// or a popup's (`is_page`). AppKit takes a press in a window that isn't key as the one that makes it key, and the
/// press didn't act: after typing in a page, the first click on a Wings tab only hovered it, and the first on a popup's
/// button did nothing, as a page's window and Wings' are two windows where WebKit's view and Wings' are one.
pub(super) fn takes_keys_on_press<W: PartialEq>(pressed: &W, key: Option<&W>, is_page: impl Fn(&W) -> bool) -> bool {
    key != Some(pressed) && (is_page(pressed) || key.is_some_and(&is_page))
}

/// Follows the mouse over the pages' and popups' windows, from the mouse events the app hands out: a press gives the
/// keyboard to the window pressed first (`takes_keys_on_press`), and the pointer going from another window into one of
/// theirs tells GPUI it left (`pointer_left`), once each time. Installed once.
pub fn follow_mouse() {
    thread_local!(static ON: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });
    thread_local!(static IN_PAGE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });
    if ON.replace(true) {
        return;
    }
    let block = block2::RcBlock::new(|event: std::ptr::NonNull<NSEvent>| -> *mut NSEvent {
        let Some(mtm) = objc2::MainThreadMarker::new() else { return event.as_ptr() };
        // SAFETY: AppKit hands the event it is about to send, alive for this call, on the main thread.
        let event_ref = unsafe { event.as_ref() };
        let Some(window) = event_ref.window(mtm) else { return event.as_ptr() };
        let in_page = frameless::is_marked(&window);
        if matches!(event_ref.r#type(), NSEventType::LeftMouseDown | NSEventType::RightMouseDown) {
            let app = NSApplication::sharedApplication(mtm);
            let key = app.keyWindow();
            if app.isActive() && takes_keys_on_press(&*window, key.as_deref(), |window| frameless::is_marked(window)) {
                window.makeKeyWindow();
            }
            return event.as_ptr();
        }
        if in_page && !IN_PAGE.replace(true)
            && let Some(parent) = window.parentWindow()
        {
            pointer_left(&parent);
        }
        if !in_page {
            IN_PAGE.set(false);
        }
        event.as_ptr()
    });
    let mask = NSEventMask::MouseMoved | NSEventMask::MouseEntered | NSEventMask::LeftMouseDragged | NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown;
    // SAFETY: the block returns the event it was given. The monitor lasts as long as the app, so it is never removed.
    std::mem::forget(unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &block) });
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

#[cfg(test)]
mod tests {
    use super::takes_keys_on_press;

    /// A press gives the keyboard first only when it moves between Wings' window and a page's or a popup's: a press in
    /// the window that has it, or between two windows that are neither, is AppKit's as ever.
    #[test]
    fn a_press_moving_the_keyboard_between_wings_and_a_page_gives_it_first() {
        let page = |window: &&str| *window == "page" || *window == "popup";
        assert!(takes_keys_on_press(&"wings", Some(&"page"), page), "a Wings tab after typing in the page");
        assert!(takes_keys_on_press(&"popup", Some(&"wings"), page), "a popup's button while Wings has the keyboard");
        assert!(takes_keys_on_press(&"page", Some(&"popup"), page));
        assert!(takes_keys_on_press(&"page", None, page));
        for (pressed, key) in [("wings", Some("wings")), ("page", Some("page")), ("wings", Some("settings")), ("wings", None)] {
            assert!(!takes_keys_on_press(&pressed, key.as_ref(), page), "{pressed} with {key:?} key");
        }
    }
}
