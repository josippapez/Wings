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

use crate::objc_runtime::{class_getMethodImplementation, class_replaceMethod, object_getClass, objc_getAssociatedObject, objc_setAssociatedObject};

/// Called once the window exists and holds its browser view, while it is still hidden: it shows the window.
pub type OnCreated = Rc<dyn Fn(&mut Window, &BrowserView)>;
/// Called when the window becomes or stops being CEF's active window.
pub type OnActivation = Rc<dyn Fn(bool)>;

/// White: what Chromium paints behind a page that sets no background of its own (Blink's default page base colour).
// debt: where a page's base is transparent (see `on_theme_changed`), a page that relies on Chrome's dark canvas
// (`color-scheme: dark` and no background of its own, a plain text file on a dark Mac) shows white under light text.
// Blink paints that canvas only over an opaque base; keeping it opaque needs CEF patched to leave ContentsWebView's
// background visible (chromiumembedded/cef#3891), as no CEF call reaches it.
pub const PAGE_BACKGROUND: u32 = 0xFFFF_FFFF;

wrap_browser_view_delegate! {
    struct ChromeStyleBrowser {
        background: Option<u32>,
    }

    impl ViewDelegate {
        /// CEF's Chrome style hides Chrome's own page background (`ContentsWebView::SetBackgroundVisible(false)`), so a
        /// page's base background can turn transparent (in the test copy, mostly on the first new document after the page
        /// opened), and where the page paints nothing this view's background shows. CEF sets that to the theme's primary background
        /// (#1f1f1f on a dark Mac) at every theme change unless the delegate sets one here.
        fn on_theme_changed(&self, view: Option<&mut View>) {
            if let (Some(view), Some(color)) = (view, self.background) {
                view.set_background_color(color);
            }
        }
    }

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
/// Views takes them), in `context` (the global one for `None`), with `background` behind the page wherever it paints
/// nothing (the theme's for `None`). Call it on CEF's UI thread, the main thread with the external pump.
pub fn open(url: &str, bounds: Rect, client: Option<Client>, context: Option<RequestContext>, background: Option<u32>, on_created: OnCreated, on_activation: OnActivation) {
    let (mut client, mut context) = (client, context);
    let browser_view = browser_view_create(
        client.as_mut(),
        Some(&CefString::from(url)),
        Some(&BrowserSettings::default()),
        None,
        context.as_mut(),
        Some(&mut ChromeStyleBrowser::new(background)),
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

/// Marks the windows `never_fullscreen` keeps windowed.
static WINDOWED: u8 = 0;

/// Keeps `child`, a page or a popup glued over the host, out of macOS fullscreen, and tells Chrome so at once.
///
/// A page's `requestFullscreen` has Chrome ask AppKit for fullscreen (`-[NSWindow toggleFullScreen:]`) and wait in a
/// transition for AppKit's answer. AppKit ignored the request for a page's window (glued over the host's, borderless,
/// and `FullScreenAuxiliary` from `attach`, which Chrome leaves as it is) and never answered, so Chrome waited for
/// ever, and it puts off closing a window until its transition ends: the window never closed, the Chrome browser
/// behind it outlived its page, and `cef::shutdown` crashed destroying it with the profile. Chrome now hears that the window stays windowed,
/// as AppKit's `windowDidExitFullScreen:` says, so it leaves the page's fullscreen and closes the window when asked.
/// Like `never_main`, it replaces the method on the window's class once, for the windows marked here.
pub fn never_fullscreen(child: &NSWindow) -> String {
    keep_windowed(child)
}

/// `never_fullscreen` for any object of a class with `toggleFullScreen:` and `delegate`, as a test's stand-in for a
/// window is: AppKit makes windows only on the main thread.
#[doc(hidden)]
pub fn keep_windowed(object: &AnyObject) -> String {
    static PATCHED: AtomicPtr<AnyClass> = AtomicPtr::new(std::ptr::null_mut());
    static ORIGINAL: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
    extern "C-unwind" fn toggle(this: &AnyObject, cmd: Sel, sender: *mut AnyObject) {
        if unsafe { objc_getAssociatedObject(this, &WINDOWED) }.is_null() {
            let original: extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject) = unsafe { std::mem::transmute(ORIGINAL.load(Ordering::Relaxed)) };
            return original(this, cmd, sender);
        }
        stayed_windowed(this);
    }
    unsafe {
        // `class`, not the runtime class: KVO may have moved the window to a subclass of its own.
        let class: &AnyClass = msg_send![object, class];
        let class_ptr = class as *const AnyClass as *mut AnyClass;
        let patched = PATCHED.load(Ordering::Relaxed);
        if patched.is_null() {
            if let Some(original) = class_getMethodImplementation(class, sel!(toggleFullScreen:)) {
                ORIGINAL.store(original as *mut (), Ordering::Relaxed);
                let replacement: extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject) = toggle;
                class_replaceMethod(class, sel!(toggleFullScreen:), std::mem::transmute(replacement), c"v@:@".as_ptr());
                PATCHED.store(class_ptr, Ordering::Relaxed);
            }
        } else if patched != class_ptr {
            return format!("not kept windowed: {} isn't the class patched before", class.name().to_string_lossy());
        }
        // Any non-nil object marks it; the class object lives forever, and ASSIGN (0) keeps no reference.
        objc_setAssociatedObject(object, &WINDOWED, class as *const AnyClass as *const AnyObject, 0);
        format!("{} kept windowed", class.name().to_string_lossy())
    }
}

/// Tells `window`'s delegate, Chrome's, that it is windowed, as AppKit does after a transition out of fullscreen: Chrome
/// ends the transition it began, leaves the page's fullscreen, and closes the window if it was asked to meanwhile.
fn stayed_windowed(window: &AnyObject) {
    unsafe {
        let delegate: *mut AnyObject = msg_send![window, delegate];
        if delegate.is_null() {
            return;
        }
        let hears: bool = msg_send![delegate, respondsToSelector: sel!(windowDidExitFullScreen:)];
        if !hears {
            return;
        }
        let notification = objc2_foundation::NSNotification::notificationWithName_object(
            objc2_app_kit::NSWindowDidExitFullScreenNotification,
            Some(window),
        );
        let _: () = msg_send![delegate, windowDidExitFullScreen: &*notification];
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

/// Gives `child`'s class a borderless frame for a style without a title bar, from its next style change on: CEF's
/// window class answers `frameViewClassForStyleMask:` with its own themed frame (`CefThemeFrame`, a titled one) whatever
/// the style, whose corner mask macOS rounds the window with, so a borderless page still had three rounded corners that
/// cut off its content. Chrome's own borderless frame (`NativeWidgetMacNSWindowBorderlessFrame`) has square corners. It
/// patches the class method once, the first time; the window's class itself stays as it is.
pub fn square_frame(child: &NSWindow) {
    static ORIGINAL: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
    extern "C-unwind" fn frame_class(this: &AnyClass, cmd: Sel, style: usize) -> *const AnyClass {
        if style & NSWindowStyleMask::Titled.0 == 0 {
            if let Some(borderless) = AnyClass::get(c"NativeWidgetMacNSWindowBorderlessFrame") {
                return borderless;
            }
        }
        let original: extern "C-unwind" fn(&AnyClass, Sel, usize) -> *const AnyClass = unsafe { std::mem::transmute(ORIGINAL.load(Ordering::Relaxed)) };
        original(this, cmd, style)
    }
    if !ORIGINAL.load(Ordering::Relaxed).is_null() {
        return;
    }
    unsafe {
        let class: &AnyClass = msg_send![child, class];
        let meta = object_getClass(class as *const AnyClass as *const AnyObject);
        if let Some(original) = class_getMethodImplementation(meta, sel!(frameViewClassForStyleMask:)) {
            ORIGINAL.store(original as *mut (), Ordering::Relaxed);
            let replacement: extern "C-unwind" fn(&AnyClass, Sel, usize) -> *const AnyClass = frame_class;
            class_replaceMethod(meta, sel!(frameViewClassForStyleMask:), std::mem::transmute(replacement), c"#@:Q".as_ptr());
        }
    }
}

/// A page: square corners flush with the host's content, no shadow. CEF's frameless window keeps a titled style mask,
/// which macOS draws with rounded corners, and Chrome rounds its content view's layer too, masking it
/// (`NativeWidgetNSWindowBridge::CreateContentView`), which cut off a page's content at the corners.
pub fn as_page(child: &NSWindow) {
    let frame = child.frame();
    square_frame(child);
    child.setStyleMask(NSWindowStyleMask::Borderless);
    child.setHasShadow(false);
    if let Some(content) = child.contentView() {
        unsafe {
            let layer: *mut AnyObject = msg_send![&*content, layer];
            if !layer.is_null() {
                let _: () = msg_send![layer, setCornerRadius: 0.0f64];
                let _: () = msg_send![layer, setMasksToBounds: false];
            }
        }
    }
    child.setFrame_display(frame, true);
}

/// A popup: rounded like the host's popovers, with a light shadow.
pub fn as_popup(child: &NSWindow, radius: f64) {
    let frame = child.frame();
    square_frame(child);
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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use cef::rc::ConvertReturnValue;
    use cef::sys::_cef_view_t;

    use super::*;

    /// The colours a stand-in view was given, in order.
    static GIVEN: Mutex<Vec<u32>> = Mutex::new(Vec::new());

    extern "C" fn given(_view: *mut _cef_view_t, color: u32) {
        GIVEN.lock().unwrap().push(color);
    }

    /// The colours `on_theme_changed` gives a view across two theme changes, for a page with `background`. A stand-in
    /// for CEF's view that only takes a background colour: its struct of calls with just that one set.
    fn given_across_two_theme_changes(background: Option<u32>) -> Vec<u32> {
        GIVEN.lock().unwrap().clear();
        // SAFETY: an all-null struct of optional calls and plain numbers is a valid one; the view never frees it.
        let raw: &'static mut _cef_view_t = Box::leak(Box::new(unsafe { std::mem::zeroed() }));
        raw.set_background_color = Some(given);
        let mut view: View = (raw as *mut _cef_view_t).wrap_result();
        let delegate = ChromeStyleBrowser { background, cef_object: std::ptr::null_mut() };
        delegate.on_theme_changed(Some(&mut view));
        delegate.on_theme_changed(Some(&mut view));
        std::mem::take(&mut *GIVEN.lock().unwrap())
    }

    /// CEF sets a view's background to the theme's at every theme change (#1f1f1f on a dark Mac), which showed behind a
    /// page that paints none; a page's view gets its own back every time, and a popup's keeps the theme's.
    #[test]
    fn a_pages_view_gets_its_background_back_at_every_theme_change_and_a_popups_keeps_the_themes() {
        assert_eq!(given_across_two_theme_changes(Some(PAGE_BACKGROUND)), [0xFFFF_FFFF, 0xFFFF_FFFF]);
        assert_eq!(given_across_two_theme_changes(None), Vec::<u32>::new());
    }
}
