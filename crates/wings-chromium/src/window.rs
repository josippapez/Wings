//! A frameless Chrome-style CEF window holding one browser, glued over a host window as a child window.
//!
//! Chrome extensions only run in Chrome-style browsers, and on macOS a browser given a parent view always runs Alloy
//! style. So the browser can't be a view inside the host's window: it gets a CEF Views window of its own (frameless,
//! no Chrome toolbar), and the host attaches that window's `NSWindow` as a child window over the region it reserves.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicPtr, Ordering};

use cef::*;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, Sel};
use objc2::{msg_send, sel, ClassType};
use objc2_app_kit::{NSColor, NSWindow, NSWindowCollectionBehavior, NSWindowOrderingMode, NSWindowStyleMask};
use objc2_foundation::NSRect;

use crate::objc_runtime::{class_getMethodImplementation, class_replaceMethod, objc_getAssociatedObject, objc_setAssociatedObject};

/// Called once the window exists and holds its browser view, while it is still hidden: it shows the window.
pub type OnCreated = Rc<dyn Fn(&mut Window, &BrowserView)>;
/// Called when the window becomes or stops being CEF's active window.
pub type OnActivation = Rc<dyn Fn(bool)>;

wrap_browser_view_delegate! {
    struct ChromeStyleBrowser {}

    impl ViewDelegate {}

    impl BrowserViewDelegate {
        fn browser_runtime_style(&self) -> RuntimeStyle {
            RuntimeStyle::CHROME
        }

        fn chrome_toolbar_type(&self, _browser_view: Option<&mut BrowserView>) -> ChromeToolbarType {
            ChromeToolbarType::NONE
        }
    }
}

wrap_window_delegate! {
    struct FramelessWindow {
        browser_view: RefCell<Option<BrowserView>>,
        bounds: Rect,
        on_created: OnCreated,
        on_activation: OnActivation,
    }

    impl ViewDelegate {}

    impl PanelDelegate {}

    impl WindowDelegate {
        fn on_window_created(&self, window: Option<&mut Window>) {
            let browser_view = self.browser_view.borrow().clone();
            let (Some(window), Some(browser_view)) = (window, browser_view) else {
                return;
            };
            window.add_child_view(Some(&mut View::from(&browser_view)));
            (self.on_created)(window, &browser_view);
        }

        fn on_window_destroyed(&self, _window: Option<&mut Window>) {
            self.browser_view.borrow_mut().take();
        }

        fn can_close(&self, _window: Option<&mut Window>) -> i32 {
            let host = self.browser_view.borrow().as_ref().and_then(|view| view.browser()).and_then(|browser| browser.host());
            host.map_or(1, |host| host.try_close_browser())
        }

        fn on_window_activation_changed(&self, _window: Option<&mut Window>, active: i32) {
            (self.on_activation)(active != 0);
        }

        fn initial_bounds(&self, _window: Option<&mut Window>) -> Rect {
            self.bounds.clone()
        }

        fn initial_show_state(&self, _window: Option<&mut Window>) -> ShowState {
            // `on_created` shows it and glues it over the host.
            ShowState::HIDDEN
        }

        fn is_frameless(&self, _window: Option<&mut Window>) -> i32 {
            1
        }

        fn with_standard_window_buttons(&self, _window: Option<&mut Window>) -> i32 {
            0
        }

        fn can_resize(&self, _window: Option<&mut Window>) -> i32 {
            0
        }

        fn can_maximize(&self, _window: Option<&mut Window>) -> i32 {
            0
        }

        fn can_minimize(&self, _window: Option<&mut Window>) -> i32 {
            0
        }

        fn accepts_first_mouse(&self, _window: Option<&mut Window>) -> State {
            State::ENABLED
        }

        fn window_runtime_style(&self) -> RuntimeStyle {
            RuntimeStyle::CHROME
        }
    }
}

/// Opens `url` in a new frameless Chrome-style window at `bounds` (screen coordinates in DIPs, top-left origin, as CEF
/// Views takes them), in `context` (the global one for `None`). Call it on CEF's UI thread, the main thread with the
/// external pump.
pub fn open(url: &str, bounds: Rect, client: Option<Client>, context: Option<RequestContext>, on_created: OnCreated, on_activation: OnActivation) {
    let (mut client, mut context) = (client, context);
    let browser_view = browser_view_create(
        client.as_mut(),
        Some(&CefString::from(url)),
        Some(&BrowserSettings::default()),
        None,
        context.as_mut(),
        Some(&mut ChromeStyleBrowser::new()),
    );
    let mut delegate = FramelessWindow::new(RefCell::new(browser_view), bounds, on_created, on_activation);
    window_create_top_level(Some(&mut delegate));
}

/// The `NSWindow` behind a CEF window. `GetWindowHandle` gives an `NSView` on macOS; take its window either way.
pub fn ns_window(window: &Window) -> Option<Retained<NSWindow>> {
    let handle = window.window_handle() as *mut AnyObject;
    if handle.is_null() {
        return None;
    }
    unsafe {
        let is_window: bool = msg_send![handle, isKindOfClass: NSWindow::class()];
        if is_window {
            Retained::retain(handle.cast::<NSWindow>())
        } else {
            msg_send![handle, window]
        }
    }
}

/// Marks the windows `never_main` made never main: the host's pages and popups.
static MARK: u8 = 0;

/// Whether `window` is one of the host's pages or popups (`never_main` marked it).
pub fn is_marked(window: &NSWindow) -> bool {
    !unsafe { objc_getAssociatedObject(window as *const NSWindow as *const AnyObject, &MARK) }.is_null()
}

/// Makes `child` never the main window, so the window it's glued over stays main, and its title bar lit, while the
/// child has the keyboard. CEF's window class answers `canBecomeMainWindow` with YES. This replaces that method on the
/// class with one that answers NO for the windows marked here and asks the original for the rest. Changing the
/// window's class instead broke the KVO observers AppKit keeps on it (an NSRangeException at the next style change).
pub fn never_main(child: &NSWindow) -> String {
    static PATCHED: AtomicPtr<AnyClass> = AtomicPtr::new(std::ptr::null_mut());
    static ORIGINAL: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
    extern "C-unwind" fn can_become_main(this: &AnyObject, cmd: Sel) -> Bool {
        if !unsafe { objc_getAssociatedObject(this, &MARK) }.is_null() {
            return Bool::NO;
        }
        let original: extern "C-unwind" fn(&AnyObject, Sel) -> Bool = unsafe { std::mem::transmute(ORIGINAL.load(Ordering::Relaxed)) };
        original(this, cmd)
    }
    unsafe {
        // `class`, not the runtime class: KVO may have moved the window to a subclass of its own.
        let class: &AnyClass = msg_send![child, class];
        let class_ptr = class as *const AnyClass as *mut AnyClass;
        let patched = PATCHED.load(Ordering::Relaxed);
        if patched.is_null() {
            if let Some(original) = class_getMethodImplementation(class, sel!(canBecomeMainWindow)) {
                ORIGINAL.store(original as *mut (), Ordering::Relaxed);
                let replacement: extern "C-unwind" fn(&AnyObject, Sel) -> Bool = can_become_main;
                class_replaceMethod(class, sel!(canBecomeMainWindow), std::mem::transmute(replacement), c"B@:".as_ptr());
                PATCHED.store(class_ptr, Ordering::Relaxed);
            }
        } else if patched != class_ptr {
            return format!("not patched: {} isn't the class patched before", class.name().to_string_lossy());
        }
        // Any non-nil object marks it; the class object lives forever, and ASSIGN (0) keeps no reference.
        objc_setAssociatedObject(child as *const NSWindow as *const AnyObject, &MARK, class as *const AnyClass as *const AnyObject, 0);
        format!("{} marked never main", class.name().to_string_lossy())
    }
}

/// Glues `child` over `parent` at `frame` (screen coordinates, AppKit's bottom-left origin): it then moves with the
/// parent and stays ordered right above it. It stays out of window cycling and the Window menu, and Mission Control
/// treats it as part of the parent instead of a window of its own.
pub fn attach(child: &NSWindow, parent: &NSWindow, frame: NSRect) {
    child.setFrame_display(frame, true);
    child.setCollectionBehavior(
        NSWindowCollectionBehavior::Transient
            | NSWindowCollectionBehavior::IgnoresCycle
            | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
    child.setExcludedFromWindowsMenu(true);
    unsafe { parent.addChildWindow_ordered(child, NSWindowOrderingMode::Above) };
}

/// Takes `child` off `parent` and off the screen. The keyboard stays with `parent` if `child` had it: a hidden page
/// that kept it swallowed what was typed next.
pub fn detach(child: &NSWindow, parent: &NSWindow) {
    if child.isKeyWindow() {
        parent.makeKeyWindow();
    }
    parent.removeChildWindow(child);
    child.orderOut(None);
}

/// A page: square corners flush with the host's content, no shadow. CEF's frameless window keeps a titled style mask,
/// which macOS draws with rounded corners.
pub fn as_page(child: &NSWindow) {
    let frame = child.frame();
    child.setStyleMask(NSWindowStyleMask::Borderless);
    child.setHasShadow(false);
    child.setFrame_display(frame, true);
}

/// A popup: rounded like the host's popovers, with a light shadow.
pub fn as_popup(child: &NSWindow, radius: f64) {
    let frame = child.frame();
    child.setStyleMask(NSWindowStyleMask::Borderless);
    child.setOpaque(false);
    child.setBackgroundColor(Some(&NSColor::clearColor()));
    if let Some(content) = child.contentView() {
        content.setWantsLayer(true);
        unsafe {
            let layer: *mut AnyObject = msg_send![&*content, layer];
            if !layer.is_null() {
                let _: () = msg_send![layer, setCornerRadius: radius];
                let _: () = msg_send![layer, setMasksToBounds: true];
            }
        }
    }
    child.setHasShadow(true);
    child.setFrame_display(frame, true);
    child.invalidateShadow();
}

/// Makes `parent` answer `isKeyWindow` with YES while one of the windows `never_main` marked, glued over it, has the
/// keyboard. GPUI takes its window as active only while it is the key window (`window_did_change_key_status`), so a
/// click into a page dimmed Wings as if another app were in front: the terminal's caret went hollow and its title bar
/// grey. It patches `parent`'s class once, the first time.
pub fn count_children_as_key(parent: &NSWindow) {
    static PATCHED: AtomicPtr<AnyClass> = AtomicPtr::new(std::ptr::null_mut());
    static ORIGINAL: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
    extern "C-unwind" fn is_key_window(this: &AnyObject, cmd: Sel) -> Bool {
        let original: extern "C-unwind" fn(&AnyObject, Sel) -> Bool = unsafe { std::mem::transmute(ORIGINAL.load(Ordering::Relaxed)) };
        if original(this, cmd).as_bool() {
            return Bool::YES;
        }
        let Some(mtm) = objc2::MainThreadMarker::new() else { return Bool::NO };
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        let key = app.keyWindow().filter(|_| app.isActive());
        let child_has_it = key.is_some_and(|key| {
            is_marked(&key) && key.parentWindow().is_some_and(|parent| std::ptr::eq(&*parent as *const NSWindow as *const AnyObject, this))
        });
        Bool::new(child_has_it)
    }
    unsafe {
        let class: &AnyClass = msg_send![parent, class];
        let class_ptr = class as *const AnyClass as *mut AnyClass;
        if !PATCHED.load(Ordering::Relaxed).is_null() {
            return;
        }
        if let Some(original) = class_getMethodImplementation(class, sel!(isKeyWindow)) {
            ORIGINAL.store(original as *mut (), Ordering::Relaxed);
            let replacement: extern "C-unwind" fn(&AnyObject, Sel) -> Bool = is_key_window;
            class_replaceMethod(class, sel!(isKeyWindow), std::mem::transmute(replacement), c"B@:".as_ptr());
            PATCHED.store(class_ptr, Ordering::Relaxed);
        }
    }
}
