//! The extensions' buttons in a page's toolbar. Each web page is a tab to the extensions (`WKWebExtensionTab`), and a
//! controller's pages are the tabs of one window of its own (`WKWebExtensionWindow`), so an extension knows which page its
//! popup opened from and can fill it in. A page's plugin hears the buttons with the page's state (`views::Extension`) and
//! clicks one with the `extension` command, which needs your click (`web::needs_click`). Its popup opens in WebKit's
//! `NSPopover` under the button and takes the keyboard, which goes back to GPUI's view as it closes. Only that click opens a
//! popup: one an extension asks for itself (`action.openPopup()`) is refused. The popup's web view is WebKit's own, never
//! one of a page's web views, so no plugin or agent reads or acts on it.

use std::{cell::RefCell, time::{Duration, Instant}};

use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send,
    rc::{Retained, Weak},
    runtime::{NSObject, NSObjectProtocol, ProtocolObject},
};
use objc2_app_kit::{NSApplication, NSImage, NSPopoverDelegate, NSView, NSWindow};
use objc2_foundation::{NSArray, NSError, NSNotification, NSPoint, NSRect, NSRectEdge, NSSize, NSString};
use objc2_web_kit::{
    WKWebExtension, WKWebExtensionAction, WKWebExtensionContext, WKWebExtensionController, WKWebExtensionControllerDelegate,
    WKWebExtensionTab, WKWebExtensionTabChangedProperties, WKWebExtensionWindow, WKWebView,
};

use super::{LAST_PRESS, Observer, gpui_among};
use crate::web::views::Extension;

/// Why a click of an extension's button did nothing.
pub(super) const NOT_ON: &str = "That extension isn't on in this browser";
const NO_BUTTON: &str = "That extension has no button";

/// How long after the click a popup may still open: WebKit loads it first, which takes a moment for a big one.
const ARMED_FOR: Duration = Duration::from_secs(10);
/// How long a press counts as the one that clicked a button or closed a popup.
const PRESS_RECENT: Duration = Duration::from_secs(5);

pub(super) struct TabIvars {
    view: Weak<WKWebView>,
    observer: Weak<Observer>,
    window: Weak<Tabs>,
    controller: Weak<WKWebExtensionController>,
    /// Its plugin, whose images the buttons' icons are.
    plugin: String,
}

define_class!(
    // SAFETY: NSObject has no rules for subclasses, and this has no `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = TabIvars]
    pub(super) struct Tab;

    unsafe impl NSObjectProtocol for Tab {}

    // Its title, address and loading come from its web view, as WebKit reads them when these say nothing.
    unsafe impl WKWebExtensionTab for Tab {
        #[unsafe(method_id(windowForWebExtensionContext:))]
        fn window_for(&self, _context: &WKWebExtensionContext) -> Option<Retained<ProtocolObject<dyn WKWebExtensionWindow>>> {
            self.ivars().window.load().map(ProtocolObject::from_retained)
        }

        #[unsafe(method_id(webViewForWebExtensionContext:))]
        fn web_view_for(&self, _context: &WKWebExtensionContext) -> Option<Retained<WKWebView>> {
            self.ivars().view.load()
        }
    }
);

pub(super) struct TabsIvars {
    tabs: RefCell<Vec<Retained<Tab>>>,
    /// The page last shown or whose button was clicked.
    active: RefCell<Option<Retained<Tab>>>,
}

define_class!(
    // SAFETY: NSObject has no rules for subclasses, and this has no `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = TabsIvars]
    pub(super) struct Tabs;

    unsafe impl NSObjectProtocol for Tabs {}

    unsafe impl WKWebExtensionWindow for Tabs {
        #[unsafe(method_id(tabsForWebExtensionContext:))]
        fn tabs_for(&self, _context: &WKWebExtensionContext) -> Retained<NSArray<ProtocolObject<dyn WKWebExtensionTab>>> {
            let tabs: Vec<Retained<ProtocolObject<dyn WKWebExtensionTab>>> =
                self.ivars().tabs.borrow().iter().map(|tab| ProtocolObject::from_retained(tab.clone())).collect();
            NSArray::from_retained_slice(&tabs)
        }

        #[unsafe(method_id(activeTabForWebExtensionContext:))]
        fn active_tab_for(&self, _context: &WKWebExtensionContext) -> Option<Retained<ProtocolObject<dyn WKWebExtensionTab>>> {
            self.ivars().active.borrow().clone().map(ProtocolObject::from_retained)
        }
    }
);

pub(super) struct DelegateIvars {
    window: Retained<Tabs>,
    /// The window a popup shows over, which gets the keyboard back as it closes, and its extension's context by address.
    under: RefCell<Option<(Weak<NSWindow>, usize)>>,
}

define_class!(
    // SAFETY: NSObject has no rules for subclasses, and this has no `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = DelegateIvars]
    pub(super) struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    // Not here, so WebKit refuses them as before: an extension opening tabs or windows of its own, asking for more
    // permissions than it was allowed as it was added, and talking to an app on this Mac (native messaging).
    unsafe impl WKWebExtensionControllerDelegate for Delegate {
        #[unsafe(method_id(webExtensionController:openWindowsForExtensionContext:))]
        fn open_windows(
            &self,
            _controller: &WKWebExtensionController,
            _context: &WKWebExtensionContext,
        ) -> Retained<NSArray<ProtocolObject<dyn WKWebExtensionWindow>>> {
            NSArray::from_retained_slice(&[ProtocolObject::from_retained(self.ivars().window.clone())])
        }

        #[unsafe(method_id(webExtensionController:focusedWindowForExtensionContext:))]
        fn focused_window(
            &self,
            _controller: &WKWebExtensionController,
            _context: &WKWebExtensionContext,
        ) -> Option<Retained<ProtocolObject<dyn WKWebExtensionWindow>>> {
            Some(ProtocolObject::from_retained(self.ivars().window.clone()))
        }

        /// A button changed: the page it is for, or every page for the button each page has until it sets its own.
        #[unsafe(method(webExtensionController:didUpdateAction:forExtensionContext:))]
        fn updated(&self, _controller: &WKWebExtensionController, action: &WKWebExtensionAction, _context: &WKWebExtensionContext) {
            // SAFETY: the action's tab, on the main thread.
            let only = unsafe { action.associatedTab() };
            let tabs = self.ivars().window.ivars().tabs.borrow().clone();
            for tab in tabs.iter().filter(|tab| only.as_deref().is_none_or(|only| std::ptr::addr_eq(only, &**tab))) {
                refresh(tab);
            }
        }

        #[unsafe(method(webExtensionController:presentPopupForAction:forExtensionContext:completionHandler:))]
        fn present(
            &self,
            _controller: &WKWebExtensionController,
            action: &WKWebExtensionAction,
            context: &WKWebExtensionContext,
            done: &block2::DynBlock<dyn Fn(*mut NSError)>,
        ) {
            match self.show(action, context) {
                Ok(()) => done.call((std::ptr::null_mut(),)),
                // WebKit closes the popup it loaded.
                Err(_) => done.call((Retained::as_ptr(&NSError::new(1, &NSString::from_str("Wings"))).cast_mut(),)),
            }
        }
    }

    unsafe impl NSPopoverDelegate for Delegate {
        /// As it starts to close, which a press outside it does at once, before that press's click reaches the page.
        #[unsafe(method(popoverWillClose:))]
        fn closing(&self, _notification: &NSNotification) {
            if let Some((_, context)) = &*self.ivars().under.borrow() {
                CLOSED.set(Some((*context, Instant::now())));
            }
        }

        #[unsafe(method(popoverDidClose:))]
        fn closed(&self, _notification: &NSNotification) {
            if let Some(window) = self.ivars().under.take().and_then(|(window, _)| window.load()) {
                keys_back(&window);
            }
        }
    }
);

/// A click of a button that opens a popup, which the popup WebKit loads for it then shows at.
struct Armed {
    /// The extension's context, by its address.
    context: usize,
    at: Instant,
    window: Weak<NSWindow>,
    /// Where it shows in GPUI's view, and the side of that it opens on.
    rect: NSRect,
    edge: NSRectEdge,
}

thread_local! {
    static ARMED: RefCell<Option<Armed>> = const { RefCell::new(None) };
    /// The extension whose popup closed last, by its context's address, and when it started to close.
    static CLOSED: std::cell::Cell<Option<(usize, Instant)>> = const { std::cell::Cell::new(None) };
}

/// How soon after a press a popup that starts to close was closed by it: AppKit closes it as it hears the press.
const CLOSED_BY_PRESS: Duration = Duration::from_millis(500);

/// Whether a click of the button of the extension `context` comes from the press that just closed its popup (`closed`,
/// pressed at `pressed`), as a press on the button while the popup shows does: the click closes it, as in a browser,
/// rather than opening it again. A popup closed by Escape, or long after the press that opened it, isn't.
fn toggles_off(closed: Option<(usize, Instant)>, pressed: Option<Instant>, context: usize, now: Instant) -> bool {
    let (Some((closed, at)), Some(pressed)) = (closed, pressed) else { return false };
    closed == context && pressed <= at && at - pressed < CLOSED_BY_PRESS && now.saturating_duration_since(at) < PRESS_RECENT
}

impl Delegate {
    /// The delegate of a controller, whose pages are the tabs of one window.
    pub(super) fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let window = Tabs::alloc(mtm).set_ivars(TabsIvars { tabs: RefCell::default(), active: RefCell::new(None) });
        // SAFETY: NSObject's `init`.
        let window: Retained<Tabs> = unsafe { msg_send![super(window), init] };
        let this = Self::alloc(mtm).set_ivars(DelegateIvars { window, under: RefCell::new(None) });
        // SAFETY: NSObject's `init`.
        unsafe { msg_send![super(this), init] }
    }

    /// Shows the popup `action` loaded, only for the click that asked for it: under its button, with the keyboard.
    fn show(&self, action: &WKWebExtensionAction, context: &WKWebExtensionContext) -> Result<(), &'static str> {
        let address = std::ptr::from_ref(context).addr();
        let armed = ARMED.take().filter(|armed| armed.context == address && armed.at.elapsed() < ARMED_FOR);
        let armed = armed.ok_or("Wings opens an extension's popup only when you click its button")?;
        let window = armed.window.load().filter(|window| window.isVisible()).ok_or("The page is gone")?;
        let gpui = window.contentView().as_deref().and_then(gpui_among).ok_or("The page is gone")?;
        // SAFETY: the action's popover, made with its web view, on the main thread.
        let popover = unsafe { action.popupPopover() }.ok_or(NO_BUTTON)?;
        popover.setDelegate(Some(ProtocolObject::from_ref(self)));
        popover.showRelativeToRect_ofView_preferredEdge(armed.rect, &gpui, armed.edge);
        // AppKit shows nothing where the place isn't on screen, and WebKit would wait on that popup for good.
        if !popover.isShown() {
            return Err("The button isn't on screen");
        }
        self.ivars().under.replace(Some((Weak::from_retained(&window), address)));
        // What you type goes into the popup at once, as its field asks for the keyboard as it opens.
        // SAFETY: the action's web view, on the main thread.
        if let Some(web) = unsafe { action.popupWebView() }
            && let Some(popup) = web.window()
        {
            popup.makeKeyWindow();
            popup.makeFirstResponder(Some(&web));
        }
        Ok(())
    }

    /// The pages of this controller.
    pub(super) fn tabs(&self) -> Vec<Retained<Tab>> {
        self.ivars().window.ivars().tabs.borrow().clone()
    }
}

/// The page `view` of `plugin` is a tab to the extensions of `controller` from now on.
pub(super) fn open(delegate: &Delegate, controller: &WKWebExtensionController, view: &WKWebView, observer: &Observer, plugin: &str) -> Retained<Tab> {
    let mtm = MainThreadMarker::from(delegate);
    let tab = Tab::alloc(mtm).set_ivars(TabIvars {
        view: Weak::new(view),
        observer: Weak::new(observer),
        window: Weak::from_retained(&delegate.ivars().window),
        controller: Weak::new(controller),
        plugin: plugin.to_owned(),
    });
    // SAFETY: NSObject's `init`.
    let tab: Retained<Tab> = unsafe { msg_send![super(tab), init] };
    delegate.ivars().window.ivars().tabs.borrow_mut().push(tab.clone());
    // SAFETY: WebKit's call for a new tab, on the main thread.
    unsafe { controller.didOpenTab(ProtocolObject::from_ref(&*tab)) };
    tab
}

/// The page `tab` is gone.
pub(super) fn close(tab: &Tab) {
    if let Some(window) = tab.ivars().window.load() {
        window.ivars().tabs.borrow_mut().retain(|kept| !std::ptr::eq(&**kept, tab));
        let mut active = window.ivars().active.borrow_mut();
        if active.as_deref().is_some_and(|active| std::ptr::eq(active, tab)) {
            *active = None;
        }
    }
    if let Some(controller) = tab.ivars().controller.load() {
        // SAFETY: WebKit's call for a closed tab, on the main thread.
        unsafe { controller.didCloseTab_windowIsClosing(ProtocolObject::from_ref(tab), false) };
    }
}

/// `tab` is the page you look at or act on now.
pub(super) fn activate(tab: &Tab) {
    let (Some(window), Some(controller)) = (tab.ivars().window.load(), tab.ivars().controller.load()) else { return };
    let previous = window.ivars().active.replace(Some(tab.retain()));
    if previous.as_deref().is_some_and(|previous| std::ptr::eq(previous, tab)) {
        return;
    }
    // SAFETY: WebKit's call for the tab in front, on the main thread.
    unsafe { controller.didActivateTab_previousActiveTab(ProtocolObject::from_ref(tab), previous.as_deref().map(ProtocolObject::from_ref)) };
}

/// The page's `key` (one its observer watches) changed, which the extensions hear of (`tabs.onUpdated`).
pub(super) fn changed(tab: &Tab, key: &str) {
    let property = match key {
        "URL" => WKWebExtensionTabChangedProperties::URL,
        "title" => WKWebExtensionTabChangedProperties::Title,
        "loading" => WKWebExtensionTabChangedProperties::Loading,
        _ => return,
    };
    if let Some(controller) = tab.ivars().controller.load() {
        // SAFETY: WebKit's call for a tab that changed, on the main thread.
        unsafe { controller.didChangeTabProperties_forTab(property, ProtocolObject::from_ref(tab)) };
    }
}

/// Tells `tab`'s page its extensions' buttons as they are now.
pub(super) fn refresh(tab: &Tab) {
    let (Some(view), Some(observer), Some(controller)) = (tab.ivars().view.load(), tab.ivars().observer.load(), tab.ivars().controller.load())
    else {
        return;
    };
    let imported = crate::web::holds_imported(&observer.ivars().web_dir, &observer.ivars().committed.borrow());
    // SAFETY: the controller's loaded extensions, on the main thread.
    let contexts = unsafe { controller.extensionContexts() };
    let found = contexts.iter().filter_map(|context| button(&context, tab, imported)).collect();
    observer.ivars().extensions.replace(crate::web::shown(found, imported));
    observer.tell(&view);
}

/// The button of `context`'s extension for `tab`, if it has one. On a page signed in with imported cookies it is the button
/// every page has, not one the extension set for that page (`views::shown`).
fn button(context: &WKWebExtensionContext, tab: &Tab, imported: bool) -> Option<Extension> {
    // SAFETY: WebKit's objects, on the main thread.
    unsafe {
        let extension = context.webExtension();
        if !has_button(&extension) {
            return None;
        }
        let action = context.actionForTab((!imported).then(|| ProtocolObject::from_ref(tab)))?;
        Some(Extension {
            id: context.uniqueIdentifier().to_string(),
            name: extension.displayName().map(|name| name.to_string()).unwrap_or_default(),
            label: action.label().to_string(),
            icon: action.iconForSize(NSSize::new(32.0, 32.0)).and_then(|image| icon(&image, &tab.ivars().plugin)),
            badge: action.badgeText().to_string(),
            enabled: action.isEnabled(),
        })
    }
}

/// Whether the extension's manifest gives it a button: WebKit has an action for every extension.
fn has_button(extension: &WKWebExtension) -> bool {
    // SAFETY: the manifest WebKit read, on the main thread.
    let manifest = unsafe { extension.manifest() };
    ["action", "browser_action", "page_action"].iter().any(|key| manifest.objectForKey(&NSString::from_str(key)).is_some())
}

/// Registers `image` among `plugin`'s images under an id made of its bytes: its id.
fn icon(image: &NSImage, plugin: &str) -> Option<String> {
    use sha2::{Digest, Sha256};
    let png = super::png_of(image).ok()?;
    let id = format!("ext-btn-{}", Sha256::digest(&png).iter().take(12).map(|byte| format!("{byte:02x}")).collect::<String>());
    crate::page::images::register_bytes(plugin, &id, png).ok()?;
    Some(id)
}

/// Clicks the button of the extension `id` for `tab`, the page `view`: its popup opens under the button (`Delegate::show`)
/// or it hears the click. Only for a click of yours (`web::needs_click`), which this tells the extension of (`activeTab`).
pub(super) fn perform(tab: &Tab, view: &WKWebView, id: &str) -> Result<(), String> {
    let controller = tab.ivars().controller.load().ok_or(NOT_ON)?;
    // SAFETY: WebKit's objects, on the main thread.
    unsafe {
        let context = controller.extensionContexts().iter().find(|context| context.uniqueIdentifier().to_string() == id).ok_or(NOT_ON)?;
        if !has_button(&context.webExtension()) {
            return Err(NO_BUTTON.into());
        }
        activate(tab);
        let tab = ProtocolObject::from_ref(tab);
        let popup = context.actionForTab(Some(tab)).is_some_and(|action| action.presentsPopup());
        let address = std::ptr::from_ref(&*context).addr();
        if popup && toggles_off(CLOSED.get(), LAST_PRESS.get().map(|(_, _, when)| when), address, Instant::now()) {
            CLOSED.set(None);
            return Ok(());
        }
        let anchor = if popup { anchor(view) } else { None };
        ARMED.set(anchor.map(|(window, rect, edge)| Armed { context: address, at: Instant::now(), window, rect, edge }));
        context.performActionForTab(Some(tab));
    }
    Ok(())
}

/// Where a popup for the page `view` shows: on its top edge, under the toolbar's button you just pressed (or near the
/// toolbar's end, for a key), in GPUI's view, and the side it opens on there.
fn anchor(view: &WKWebView) -> Option<(Weak<NSWindow>, NSRect, NSRectEdge)> {
    let window = view.window()?;
    // SAFETY: AppKit's views, on the main thread.
    let content = unsafe { view.superview() }?;
    let gpui = gpui_among(&content)?;
    let frame = view.frame();
    let top = if content.isFlipped() { frame.origin.y } else { frame.origin.y + frame.size.height };
    let left = gpui.convertPoint_fromView(NSPoint::new(frame.origin.x, top), Some(&content));
    let right = gpui.convertPoint_fromView(NSPoint::new(frame.origin.x + frame.size.width, top), Some(&content));
    let pressed = LAST_PRESS
        .get()
        .filter(|(at, _, when)| *at == std::ptr::from_ref(&*window).addr() && when.elapsed() < PRESS_RECENT)
        .map(|(_, point, _)| gpui.convertPoint_fromView(point, None).x);
    let (low, high) = (left.x.min(right.x) + 12.0, left.x.max(right.x) - 12.0);
    let x = pressed.unwrap_or(high - 12.0).min(high).max(low);
    // A sliver just over the page's top edge, which the popover's arrow points up at from under it.
    let flipped = gpui.isFlipped();
    let rect = NSRect::new(NSPoint::new(x - 1.0, if flipped { left.y - 1.0 } else { left.y }), NSSize::new(2.0, 1.0));
    Some((Weak::from_retained(&window), rect, if flipped { NSRectEdge::MaxY } else { NSRectEdge::MinY }))
}

/// A popup over `window` closed: WebKit left the keyboard with the window itself, so typing went nowhere. A press on a page
/// that closed it keeps it there, as any press on a page does; otherwise GPUI's view takes it, found by its class.
fn keys_back(window: &NSWindow) {
    let Some(content) = window.contentView() else { return };
    let here = std::ptr::from_ref(window).addr();
    let pressed = LAST_PRESS.get().is_some_and(|(at, _, when)| at == here && when.elapsed() < PRESS_RECENT);
    let page = super::PRESSED_WEB_VIEW.get();
    let page = (pressed && page != 0)
        .then(|| content.subviews().iter().find(|view| std::ptr::from_ref(&**view).addr() == page && !view.isHidden()))
        .flatten();
    let Some(to) = page.or_else(|| gpui_among(&content)) else { return };
    let focused = window.firstResponder().and_then(|responder| responder.downcast::<NSView>().ok());
    if focused.is_none_or(|focused| !focused.isDescendantOf(&to)) {
        window.makeFirstResponder(Some(&to));
    }
    // Closed from the popup itself (Escape, or the extension closing it), which had the keyboard: its window takes it back.
    let app = NSApplication::sharedApplication(MainThreadMarker::from(window));
    if app.isActive() && app.keyWindow().is_none() && window.isVisible() {
        window.makeKeyWindow();
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::toggles_off;

    /// A press on the button while its popup shows closes the popup, and its click must not open it again; Escape, or
    /// another extension's button, or a press long before, opens it as any click does.
    #[test]
    fn a_click_on_the_button_of_the_popup_that_its_press_closed_closes_it_and_nothing_else_does() {
        let now = Instant::now();
        let pressed = now - Duration::from_millis(200);
        let closed = Some((7, pressed + Duration::from_millis(5)));
        assert!(toggles_off(closed, Some(pressed), 7, now), "the press on its button closed it");
        assert!(!toggles_off(closed, Some(pressed), 8, now), "another extension's button opens its own");
        assert!(!toggles_off(None, Some(pressed), 7, now), "nothing closed");
        // Closed by Escape some time after the press that opened it.
        assert!(!toggles_off(Some((7, pressed + Duration::from_secs(3))), Some(pressed), 7, now + Duration::from_secs(4)));
        // Closed before this press: the press opens it again.
        assert!(!toggles_off(Some((7, pressed - Duration::from_millis(50))), Some(pressed), 7, now));
        // A click that comes long after.
        assert!(!toggles_off(closed, Some(pressed), 7, now + Duration::from_secs(10)));
    }
}
