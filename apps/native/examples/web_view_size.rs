//! Whether a web view placed over a window's content, as `web/mac.rs` places one, changes the window's size when its page
//! loads. Opens a window with a view standing in for GPUI's, puts a WKWebView over part of it, loads a page far bigger
//! than the window, and prints the window's frame before and after. macOS only.
//!
//! `cargo run --manifest-path apps/native/Cargo.toml --example web_view_size --features test-support`

#[cfg(target_os = "macos")]
fn main() {
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSAutoresizingMaskOptions, NSBackingStoreType, NSView, NSWindow,
        NSWindowOrderingMode, NSWindowStyleMask,
    };
    use objc2_foundation::{NSDate, NSPoint, NSRect, NSRunLoop, NSSize, NSString};
    use objc2_web_kit::{WKWebView, WKWebViewConfiguration};

    let mtm = MainThreadMarker::new().expect("the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let frame = NSRect::new(NSPoint::new(100.0, 100.0), NSSize::new(800.0, 600.0));
    let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Resizable | NSWindowStyleMask::Closable;
    // SAFETY: AppKit's window and views, made and used on the main thread.
    unsafe {
        let window = NSWindow::initWithContentRect_styleMask_backing_defer(NSWindow::alloc(mtm), frame, style, NSBackingStoreType::Buffered, false);
        window.setReleasedWhenClosed(false);
        let content = window.contentView().unwrap();
        // GPUI's view: the whole content, following it as the window resizes.
        let gpui = NSView::initWithFrame(NSView::alloc(mtm), content.bounds());
        gpui.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
        content.addSubview(&gpui);
        let view = WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), NSRect::ZERO, &WKWebViewConfiguration::new(mtm));
        super_place(&view, &content, &gpui);
        window.makeKeyAndOrderFront(None);
        let before = window.frame();
        let page = "<html><body style='margin:0'><div style='width:3000px;height:4000px;background:linear-gradient(red,blue)'>Wide and tall</div></body></html>";
        view.loadHTMLString_baseURL(&NSString::from_str(page), None);
        for _ in 0..30 {
            NSRunLoop::mainRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.1));
        }
        let after = window.frame();
        println!("before {:?}\nafter  {:?}", before.size, after.size);
        let grew = (after.size.width - before.size.width).abs() > 0.5 || (after.size.height - before.size.height).abs() > 0.5;
        println!("{}", if grew { "THE WINDOW CHANGED SIZE" } else { "The window kept its size." });
        window.close();
        std::process::exit(i32::from(grew));
    }

    /// As `web/mac.rs` places a web view: above GPUI's view, at a frame inside the content.
    unsafe fn super_place(view: &WKWebView, content: &NSView, gpui: &NSView) {
        unsafe { wings_native::web::mac::attach(view, content, gpui) };
        view.setFrame(NSRect::new(NSPoint::new(40.0, 40.0), NSSize::new(500.0, 400.0)));
        let _ = NSWindowOrderingMode::Above;
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {}
