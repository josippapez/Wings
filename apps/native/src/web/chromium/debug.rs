//! A debug build's hand for a script checking Chromium pages with no real input (the Mac may be locked): with
//! `WINGS_CHROMIUM_CONTROL=<file>` set, SIGUSR1 runs the commands in that file, one per line, and each says what it did
//! on stderr (Wings' log) after `[chromium-check]`. A release build has none of it.
//!
//! - `popup`: clicks the first extension's button on the first page, as a click of yours would.
//! - `key <keystroke> [popup]`: sends a key press, written as GPUI writes one (`cmd-t`, `cmd-c`, `escape`), to the first
//!   page's window (or the popup's), as AppKit sends a typed one.
//! - `click <wings|page|popup> <x> <y>`: posts a left click at `x`, `y` points from the top left of that window's
//!   content to the app's event queue, so AppKit handles it as a click of the mouse, a window that isn't key included.
//! - `click web <x> <y> [cmd]`: the same at `x`, `y` points from the top left of the web page that shows, a WebKit one
//!   when no Chromium page is open, with Cmd held for `cmd`.
//! - `unload`: takes the first extension out of Chromium as turning it off does (`extensions::unload`).
//! - `clear`: clears the first page's profile's Chromium data as Clear data does (`page::clear_data`).
//! - `windows`: every window of the app, with its frame, level and whether it shows and has the keyboard.
//! - `focus page` / `focus wings`: makes the first page's window, or Wings', the key window.
//! - `state`: which window has the keyboard, whether Wings counts as active, and whether a popup shows.
//! - `quit`: quits Wings as Cmd-Q does (`-[NSApplication terminate:]`).
//! - `cookies <domain> <keep|session>`: imports a made-up cookie `wings_check` of `domain` into the first page's
//!   profile, kept past this run or not, by the same code an import from another browser runs (`cookies::import`), so
//!   no real browser's cookies are read; what it took shows in Wings' log later.
//! - `buttons`: the extensions' buttons the first page's toolbar gets, with their badges.
//! - `page`: where the first page is, as the gate reads it, and what its DevTools channel knows.

use std::ffi::c_void;

use cef::{ImplBrowser, ImplBrowserHost};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSEvent, NSEventModifierFlags, NSEventType};
use objc2_foundation::{NSPoint, NSString};

use super::{extensions, keys::Press, page};

/// Listens for SIGUSR1, once, if the environment names a control file.
pub fn listen() {
    static ON: std::sync::Once = std::sync::Once::new();
    let Some(file) = std::env::var_os("WINGS_CHROMIUM_CONTROL") else { return };
    ON.call_once(move || {
        let file = Box::leak(Box::new(std::path::PathBuf::from(file)));
        unsafe extern "C" {
            static _dispatch_main_q: c_void;
            static _dispatch_source_type_signal: c_void;
            fn dispatch_source_create(kind: *const c_void, handle: usize, mask: usize, queue: *const c_void) -> *mut c_void;
            fn dispatch_set_context(object: *mut c_void, context: *mut c_void);
            fn dispatch_source_set_event_handler_f(source: *mut c_void, handler: extern "C" fn(*mut c_void));
            fn dispatch_resume(object: *mut c_void);
            fn signal(signal: i32, handler: usize) -> usize;
        }
        extern "C" fn heard(context: *mut c_void) {
            // SAFETY: the leaked path `listen` set as the source's context.
            let file = unsafe { &*context.cast::<std::path::PathBuf>() };
            for command in std::fs::read_to_string(file).unwrap_or_default().lines().map(str::trim).filter(|line| !line.is_empty()) {
                eprintln!("[chromium-check] {command}: {}", run(command));
            }
        }
        // SAFETY: a signal source on the main queue, as the spike's; SIGUSR1 (30) is ignored so only the source hears it.
        unsafe {
            signal(30, 1);
            let source = dispatch_source_create(&raw const _dispatch_source_type_signal, 30, 0, &raw const _dispatch_main_q);
            dispatch_set_context(source, std::ptr::from_mut(file).cast());
            dispatch_source_set_event_handler_f(source, heard);
            dispatch_resume(source);
        }
    });
}

fn run(command: &str) -> String {
    let Some(mtm) = MainThreadMarker::new() else { return "not on the main thread".into() };
    let first = page::first();
    let mut words = command.split_whitespace();
    match (words.next(), words.next()) {
        (Some("popup"), _) => match (&first, extensions::first_id()) {
            (Some(page), Some(id)) => format!("{:?}", extensions::click(page, &id)),
            _ => "no page or no extension".into(),
        },
        (Some("key"), Some(keystroke)) => {
            let (Some(page), Some(press)) = (&first, Press::parse(keystroke)) else { return "no page, or not a keystroke".into() };
            let to_popup = words.next() == Some("popup");
            let Some(ns) = (if to_popup { extensions::popup_window() } else { page.ns.borrow().clone() }) else { return "no such window yet".into() };
            let mut flags = NSEventModifierFlags::empty();
            for (on, flag) in [(press.cmd, NSEventModifierFlags::Command), (press.ctrl, NSEventModifierFlags::Control), (press.alt, NSEventModifierFlags::Option), (press.shift, NSEventModifierFlags::Shift)] {
                if on {
                    flags |= flag;
                }
            }
            let (code, characters) = key_code(&press.key);
            let characters = NSString::from_str(&characters);
            let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
                NSEventType::KeyDown, NSPoint::new(0.0, 0.0), flags, 0.0, ns.windowNumber(), None, &characters, &characters, false, code,
            );
            let Some(event) = event else { return "AppKit made no event".into() };
            // To the page's window itself: AppKit hands an app's key press only to its key window, and a locked Mac's
            // app is never active, so has none.
            ns.sendEvent(&event);
            format!("sent; key window now {}", key_window(mtm, page))
        }
        (Some("focus"), Some(which)) => {
            let Some(page) = &first else { return "no page".into() };
            match which {
                "page" => page.ns.borrow().iter().for_each(|ns| ns.makeKeyWindow()),
                _ => page.parent.borrow().iter().for_each(|parent| parent.makeKeyWindow()),
            }
            format!("key window now {}", key_window(mtm, page))
        }
        (Some("click"), Some("web")) => {
            let (Some(x), Some(y)) = (words.next().and_then(|x| x.parse::<f64>().ok()), words.next().and_then(|y| y.parse::<f64>().ok())) else { return "click takes x and y".into() };
            let flags = if words.next() == Some("cmd") { NSEventModifierFlags::Command } else { NSEventModifierFlags::empty() };
            let target = match &first {
                Some(page) => page.ns.borrow().clone().map(|window| {
                    let height = window.contentView().map_or(0.0, |content| content.frame().size.height);
                    (window, NSPoint::new(x, height - y))
                }),
                None => crate::web::mac::shown_page_point(x, y, mtm),
            };
            let Some((window, at)) = target else { return "no page shows".into() };
            let app = NSApplication::sharedApplication(mtm);
            for kind in [NSEventType::LeftMouseDown, NSEventType::LeftMouseUp] {
                let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                    kind, at, flags, 0.0, window.windowNumber(), None, 0, 1, 1.0,
                );
                let Some(event) = event else { continue };
                // On a locked Mac the app is never active, and a WebKit page in a window that isn't key takes no first
                // click, which only makes the window key: the press goes to the page itself, through what Wings' own
                // monitor of presses does with one (`mac::debug_press`).
                if first.is_none() {
                    crate::web::mac::debug_press(&event, mtm);
                } else {
                    app.postEvent_atStart(&event, false);
                }
            }
            format!("clicked at {:.0},{:.0} of window #{}", at.x, at.y, window.windowNumber())
        }
        (Some("click"), Some(which)) => {
            let Some(page) = &first else { return "no page".into() };
            let (Some(x), Some(y)) = (words.next().and_then(|x| x.parse::<f64>().ok()), words.next().and_then(|y| y.parse::<f64>().ok())) else { return "click takes x and y".into() };
            let window = match which {
                "wings" => page.parent.borrow().clone(),
                "popup" => extensions::popup_window(),
                _ => page.ns.borrow().clone(),
            };
            let Some(window) = window else { return "no such window".into() };
            let height = window.contentView().map_or(0.0, |content| content.frame().size.height);
            let at = NSPoint::new(x, height - y);
            let app = NSApplication::sharedApplication(mtm);
            for kind in [NSEventType::LeftMouseDown, NSEventType::LeftMouseUp] {
                let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                    kind, at, NSEventModifierFlags::empty(), 0.0, window.windowNumber(), None, 0, 1, 1.0,
                );
                if let Some(event) = event {
                    app.postEvent_atStart(&event, false);
                }
            }
            format!("posted; key window was {}", key_window(mtm, page))
        }
        (Some("exitfs"), _) => match first.as_ref().and_then(|page| page.browser.borrow().clone()).and_then(|browser| browser.host()) {
            Some(host) => {
                let was = host.is_fullscreen();
                host.exit_fullscreen(1);
                format!("was fullscreen: {was}")
            }
            None => "no page".into(),
        },
        (Some("unload"), _) => match (&first, extensions::first_id()) {
            (Some(page), Some(id)) => {
                extensions::unload(&page.profile, &id, || eprintln!("[chromium-check] unloaded"));
                "unloading".into()
            }
            _ => "no page or no extension".into(),
        },
        (Some("clear"), _) => match &first {
            Some(page) => {
                page::clear_data(&page.profile, || eprintln!("[chromium-check] cleared"));
                "clearing".into()
            }
            None => "no page".into(),
        },
        (Some("targets"), _) => {
            extensions::print_targets();
            "asked".into()
        }
        (Some("counts"), _) => {
            let (open, browsers) = super::runtime::counts();
            format!("open {open}, browsers {browsers}, work every 1/30 s: {}", wings_chromium::pump::fallback())
        }
        (Some("windows"), _) => NSApplication::sharedApplication(mtm)
            .windows()
            .iter()
            .map(|window| {
                let frame = window.frame();
                format!(
                    "#{} {} {:?} level {} {}{}{}",
                    window.windowNumber(),
                    window.class().name().to_string_lossy(),
                    (frame.origin.x, frame.origin.y, frame.size.width, frame.size.height),
                    window.level(),
                    if window.isVisible() { "shown" } else { "hidden" },
                    if window.isKeyWindow() { ", key" } else { "" },
                    if window.parentWindow().is_some() { ", child" } else { "" },
                )
            })
            .collect::<Vec<_>>()
            .join("; "),
        (Some("state"), _) => match &first {
            Some(page) => {
                let parent_counts = page.parent.borrow().as_ref().is_some_and(|parent| parent.isKeyWindow());
                let shown = page.ns.borrow().as_ref().is_some_and(|ns| ns.isVisible() && ns.parentWindow().is_some());
                format!(
                    "key window {}; Wings' window counts as key: {parent_counts}; page shown: {shown}; popup open: {}; app active: {}",
                    key_window(mtm, page),
                    extensions::popup_open(),
                    NSApplication::sharedApplication(mtm).isActive()
                )
            }
            None => "no page".into(),
        },
        (Some("cookies"), Some(domain)) => {
            let Some(page) = &first else { return "no page".into() };
            let keep_signins = words.next() == Some("keep");
            let made = wings_browser_cookies::Cookie {
                domain: domain.into(),
                name: "wings_check".into(),
                value: format!("{}", std::process::id()),
                path: "/".into(),
                expires: Some(4_102_444_800.0),
                secure: false,
                http_only: false,
                same_site: None,
            };
            super::cookies::import(&page.profile, page.keep_data, keep_signins, &[made], |took| eprintln!("[chromium-check] cookies took: {took:?}"));
            "importing".into()
        }
        (Some("page"), _) => first.as_ref().map_or_else(|| "no page".into(), |page| page.describe()),
        (Some("buttons"), _) => match &first {
            Some(page) => {
                let buttons = extensions::buttons(&page.profile, false, &page.url_now());
                buttons.iter().map(|button| format!("{}={:?}", button.name, button.badge)).collect::<Vec<_>>().join(", ")
            }
            None => "no page".into(),
        },
        (Some("quit"), _) => {
            NSApplication::sharedApplication(mtm).terminate(None);
            "quitting".into()
        }
        _ => "unknown command".into(),
    }
}

/// Which of the page's windows has the keyboard.
fn key_window(mtm: MainThreadMarker, page: &page::Shared) -> &'static str {
    let key = NSApplication::sharedApplication(mtm).keyWindow();
    let is = |window: Option<objc2::rc::Retained<objc2_app_kit::NSWindow>>| key.as_ref().zip(window).is_some_and(|(key, window)| std::ptr::eq(&**key, &*window));
    if is(page.ns.borrow().clone()) {
        "page"
    } else if is(page.parent.borrow().clone()) {
        "wings"
    } else if key.is_some() {
        "another"
    } else {
        "none"
    }
}

/// The macOS key code and characters of a GPUI key name, for the keys a check presses.
fn key_code(key: &str) -> (u16, String) {
    match key {
        "escape" => (53, "\u{1b}".into()),
        "enter" => (36, "\r".into()),
        "t" => (17, "t".into()),
        "w" => (13, "w".into()),
        "k" => (40, "k".into()),
        "c" => (8, "c".into()),
        "v" => (9, "v".into()),
        "a" => (0, "a".into()),
        "1" => (18, "1".into()),
        "`" => (50, "`".into()),
        "=" => (24, "=".into()),
        other => (0, other.into()),
    }
}
