//! Plugin view windows retain their own frame and report focus separately from the workspace.
use crate::workspace::NotifyWorkspace;
use super::Workspace;
use crate::{
    installed::Installed,
    page::{
        PageView,
        plugin::{PluginHost, PluginWindow},
    },
};
use gpui::{
    Bounds, Context, Entity, FocusHandle, Focusable, Subscription, Window, WindowBounds,
    WindowOptions, div, prelude::*, px, size,
};

pub(super) struct PluginViewWindow {
    page: Entity<PageView>,
    windows: Vec<PluginWindow>,
    focus: FocusHandle,
    _activation: Subscription,
    /// Where the window is, kept as it moves and saved as it closes (`remember`).
    _bounds: Subscription,
    /// Closes the window when its page asks (`closeTab`), and sizes it (`resizeWindow`).
    _closing: gpui::Task<()>,
    _resizing: gpui::Task<()>,
}

impl Focusable for PluginViewWindow {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}
impl Render for PluginViewWindow {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // The page is on screen in this window, as the workspace says of a tab's page, so what draws over it (its web
        // pages) finds its places.
        let id = window.window_handle().window_id();
        crate::ui::start_draw(id);
        crate::ui::place_view(self.page.entity_id(), id);
        div()
            .size_full()
            .track_focus(&self.focus)
            .child(self.page.clone())
            .child(crate::web::sync(window))
            .on_key_down(|event, window, cx| {
                if event.keystroke.modifiers.platform && event.keystroke.key == "w" {
                    window.remove_window();
                    cx.stop_propagation();
                }
            })
    }
}

impl PluginViewWindow {
    /// Loads the page from `host`, the plugin's next run after an update or restart, in place of the run `old` it
    /// showed: the window stays open.
    pub(super) fn reload(
        &mut self,
        host: &PluginHost,
        old: Option<&PluginHost>,
        source: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let registered = host.register_window();
        registered.set_active(window.is_window_active());
        let opener = host.page(true, registered.id, None, None, source);
        if let Some(old) = old {
            self.windows.retain(|registered| !registered.is_of(old));
        }
        self.windows.push(registered);
        self.page.update(cx, |page, cx| page.reload_from(opener, cx));
    }
}

/// The page source, title and size of `installed`'s view `id` that opens as a window.
pub(super) fn window_view(installed: &Installed, id: &str) -> Result<(String, String, (u32, u32)), String> {
    // A view open in several windows keys each by the view's id and `#` and a number (`open_plugin_window`).
    let id = id.split('#').next().unwrap_or(id);
    let plugin = &installed.id;
    let view = installed
        .plugin
        .manifest
        .contributes
        .views
        .iter()
        .find(|view| view.id == id && view.opens.iter().any(|place| place == "window"))
        .ok_or_else(|| format!("{plugin} has no window view {id}"))?;
    let stem = std::path::Path::new(&view.page)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or("The view has no native page")?;
    let file = crate::installed::frame_file(&installed.plugin.dir, stem)
        .ok_or("The view has no native window page")?;
    let source = crate::installed::page_source(&installed.plugin.dir, &file)
        .map_err(|error| error.to_string())?;
    let size = (view.width.unwrap_or(1100), view.height.unwrap_or(760));
    Ok((source, view.title.clone(), size))
}

thread_local! {
    /// Where each view's window last was, by plugin and view, which its next window opens at.
    static PLACES: std::cell::RefCell<Option<std::collections::HashMap<String, (f32, f32, f32, f32)>>> = const { std::cell::RefCell::new(None) };
}

/// Where the windows' places are kept: beside the workspace's file, when it has one.
fn places_file(state: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
    Some(state?.parent()?.join("plugin-windows.json"))
}

fn places(state: Option<&std::path::Path>) -> std::collections::HashMap<String, (f32, f32, f32, f32)> {
    PLACES.with_borrow_mut(|places| {
        places
            .get_or_insert_with(|| {
                places_file(state)
                    .and_then(|file| std::fs::read(file).ok())
                    .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                    .unwrap_or_default()
            })
            .clone()
    })
}

/// Keeps where the window of `view` is, and with `save` writes it down: once it closes, not at every step of a drag.
fn remember(state: Option<&std::path::Path>, view: &str, bounds: Bounds<gpui::Pixels>, save: bool) {
    let mut all = places(state);
    let (x, y) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
    all.insert(view.to_owned(), (x, y, f32::from(bounds.size.width), f32::from(bounds.size.height)));
    if save && let (Some(file), Ok(json)) = (places_file(state), serde_json::to_vec(&all)) {
        let _ = std::fs::write(file, json);
    }
    PLACES.with_borrow_mut(|places| *places = Some(all));
}

/// The smallest a plugin's window is: a phone's width fits (390 points).
const WINDOW_MIN: (f32, f32) = (320.0, 240.0);

/// The size a window asked to be `asked` points gets: no smaller than `WINDOW_MIN`, and within `room`, the screen's
/// visible area, when that is known.
pub(super) fn fit(asked: (f64, f64), room: Option<(f32, f32)>) -> (f32, f32) {
    let (width, height) = (asked.0 as f32, asked.1 as f32);
    let (width, height) = (width.max(WINDOW_MIN.0), height.max(WINDOW_MIN.1));
    match room {
        Some((room_width, room_height)) => (width.min(room_width.max(WINDOW_MIN.0)), height.min(room_height.max(WINDOW_MIN.1))),
        None => (width, height),
    }
}

/// The visible area of the screen `window` is on, or the main one.
fn room(window: Option<&Window>, cx: &gpui::App) -> Option<(f32, f32)> {
    let display = window.and_then(|window| window.display(cx)).or_else(|| cx.primary_display())?;
    let visible = display.visible_bounds();
    Some((f32::from(visible.size.width), f32::from(visible.size.height)))
}

impl Workspace {
    /// Opens `installed`'s window view `id` with `args` (`viewArgs`), or shows the one open. A view that may be open
    /// more than once (`multiple`) gets a window each time, at the place its last one was.
    pub(super) fn open_plugin_window(
        &mut self,
        plugin: &str,
        id: &str,
        args: Option<&serde_json::Value>,
        size: Option<(f64, f64)>,
        cx: &mut Context<Self>,
    ) -> Result<Option<(f32, f32)>, String> {
        let multiple = self.plugins.iter().find(|installed| installed.id == plugin).is_some_and(|installed| {
            installed.plugin.manifest.contributes.views.iter().any(|view| view.id == id && view.multiple)
        });
        let key = match multiple {
            true => {
                let open = self.plugin_ui.windows.keys().filter(|(of, _)| of == plugin).count();
                (plugin.to_owned(), format!("{id}#{open}-{}", self.plugin_ui.windows.len()))
            }
            false => (plugin.to_owned(), id.to_owned()),
        };
        if let Some(handle) = self.plugin_ui.windows.get(&key)
            && handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            return Ok(None);
        }
        let installed = self
            .plugins
            .iter()
            .find(|installed| installed.id == plugin)
            .ok_or_else(|| format!("{plugin} is turned off or not installed"))?;
        let (source, title, size_of) = window_view(installed, id)?;
        // A size asked for is the window's, as far as the screen has room; none is the place the view's last window had.
        let asked = size.map(|asked| fit(asked, room(None, cx)));
        let default = (size_of.0 as f32, size_of.1 as f32);
        self.show_plugin_window(key, plugin, id, &source, title, default, asked, args.cloned(), cx)?;
        Ok(asked)
    }

    fn show_plugin_window(
        &mut self,
        key: (String, String),
        plugin: &str,
        view: &str,
        source: &str,
        title: String,
        (width, height): (f32, f32),
        asked: Option<(f32, f32)>,
        args: Option<serde_json::Value>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let state = self.config.state.clone();
        let place = format!("{plugin}/{view}");
        let host = self
            .plugins
            .iter()
            .find(|installed| installed.id == plugin)
            .ok_or_else(|| format!("{plugin} is turned off or not installed"))?
            .host
            .clone();
        let windows: Vec<_> = self
            .plugins
            .iter()
            .map(|installed| installed.host.register_window())
            .collect();
        let owner = self
            .plugins
            .iter()
            .position(|installed| installed.id == plugin)
            .unwrap();
        let window_id = windows[owner].id;
        let source = source.to_owned();
        let bounds = match (places(state.as_deref()).get(&place), asked) {
            (Some(&(x, y, _, _)), Some((w, h))) => Bounds::new(gpui::point(px(x), px(y)), size(px(w), px(h))),
            (Some(&(x, y, w, h)), None) => Bounds::new(gpui::point(px(x), px(y)), size(px(w), px(h))),
            (None, asked) => {
                let (w, h) = asked.unwrap_or((width, height));
                Bounds::centered(None, size(px(w), px(h)), cx)
            }
        };
        let handle = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(WINDOW_MIN.0), px(WINDOW_MIN.1))),
                    ..Default::default()
                },
                move |window, cx| {
                    window.set_window_title(&title);
                    // Tests stand in for WebKit (`web::testing`).
                    #[cfg(all(target_os = "macos", not(test)))]
                    crate::web::host_natively(window);
                    // The page closes its window with `closeTab`, from its own thread: the window hears it here.
                    let (close, closed) = async_channel::bounded::<()>(1);
                    let (resize, resized) = async_channel::unbounded::<(f64, f64, crate::page::runtime::Reply)>();
                    let frame_state = crate::page::plugin::FrameState::new(
                        crate::page::plugin::StatePlace::Window,
                        None,
                        |_| Err("A window's page keeps no state".into()),
                    )
                    .on_close(move |_| {
                        let _ = close.try_send(());
                    })
                    .on_resize(move |width, height, reply| {
                        if let Err(error) = resize.try_send((width, height, reply)) {
                            error.into_inner().2.err("The window is closed".into());
                        }
                    });
                    let (closing_state, closing_place) = (state.clone(), place.clone());
                    window.on_window_should_close(cx, move |window, _| {
                        remember(closing_state.as_deref(), &closing_place, window.bounds(), true);
                        true
                    });
                    cx.new(|cx| {
                        let page = cx.new(|cx| {
                            PageView::in_plugin_window_with(&host, window_id, &source, args.as_ref(), frame_state, cx)
                        });
                        for registered in &windows {
                            registered.set_active(window.is_window_active());
                        }
                        let focus = cx.focus_handle();
                        focus.focus(window, cx);
                        let (moved_state, moved_place) = (state.clone(), place.clone());
                        let (closed_state, closed_place) = (state.clone(), place.clone());
                        PluginViewWindow {
                            page,
                            windows,
                            focus,
                            _bounds: cx.observe_window_bounds(window, move |_, window, _| {
                                remember(moved_state.as_deref(), &moved_place, window.bounds(), false)
                            }),
                            _resizing: cx.spawn_in(window, async move |_, cx| {
                                while let Ok((width, height, reply)) = resized.recv().await {
                                    let sized = cx.update(|window, cx| {
                                        let (width, height) = fit((width, height), room(Some(window), cx));
                                        window.resize(size(px(width), px(height)));
                                        (width, height)
                                    });
                                    match sized {
                                        Ok((width, height)) => reply.ok(serde_json::json!({ "width": width, "height": height }).to_string()),
                                        Err(_) => reply.err("The window is closed".into()),
                                    }
                                }
                            }),
                            _closing: cx.spawn_in(window, async move |_, cx| {
                                if closed.recv().await.is_ok() {
                                    let _ = cx.update(|window, _| {
                                        remember(closed_state.as_deref(), &closed_place, window.bounds(), true);
                                        window.remove_window();
                                    });
                                }
                            }),
                            _activation: cx.observe_window_activation(
                                window,
                                |this: &mut PluginViewWindow, window, _| {
                                    for registered in &this.windows {
                                        registered.set_active(window.is_window_active());
                                    }
                                },
                            ),
                        }
                    })
                },
            )
            .map_err(|error| error.to_string())?;
        self.plugin_ui.windows.insert(key, handle);
        if self.plugin_ui.windows_release.is_none() {
            self.plugin_ui.windows_release = Some(cx.on_release(|this, cx| {
                for handle in this.plugin_ui.windows.values() {
                    let _ = handle.update(cx, |_, window, _| window.remove_window());
                }
            }));
            let workspace = cx.weak_entity();
            self.plugin_ui.windows_closed = Some(cx.on_window_closed(move |cx, id| {
                let workspace = workspace.clone();
                cx.defer(move |cx| {
                    let _ = workspace.update(cx, |workspace, cx| {
                        workspace
                            .plugin_ui
                            .windows
                            .retain(|_, handle| handle.window_id() != id);
                        cx.notify_workspace();
                    });
                });
            }));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::fit;

    #[test]
    fn a_window_asked_for_a_size_keeps_on_the_screen_and_no_smaller_than_a_phone() {
        let screen = Some((1512.0, 944.0));
        // Desktop, tablet and phone presets on a laptop's screen.
        assert_eq!(fit((1440.0, 900.0), screen), (1440.0, 900.0));
        assert_eq!(fit((834.0, 1194.0), screen), (834.0, 944.0), "as tall as the screen has room for");
        assert_eq!(fit((390.0, 844.0), screen), (390.0, 844.0));
        assert_eq!(fit((5000.0, 10.0), screen), (1512.0, 240.0));
        assert_eq!(fit((100.0, 100.0), None), (320.0, 240.0));
    }
}
