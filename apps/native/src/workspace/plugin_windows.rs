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
}

impl Focusable for PluginViewWindow {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}
impl Render for PluginViewWindow {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .track_focus(&self.focus)
            .child(self.page.clone())
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

impl Workspace {
    pub(super) fn open_plugin_window(
        &mut self,
        plugin: &str,
        id: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let key = (plugin.to_owned(), id.to_owned());
        if let Some(handle) = self.plugin_ui.windows.get(&key)
            && handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            return Ok(());
        }
        let installed = self
            .plugins
            .iter()
            .find(|installed| installed.id == plugin)
            .ok_or_else(|| format!("{plugin} is turned off or not installed"))?;
        let (source, title, size) = window_view(installed, id)?;
        self.show_plugin_window(key, plugin, &source, title, size, cx)
    }

    fn show_plugin_window(
        &mut self,
        key: (String, String),
        plugin: &str,
        source: &str,
        title: String,
        (width, height): (u32, u32),
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
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
        let bounds = Bounds::centered(None, size(px(width as f32), px(height as f32)), cx);
        let handle = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(640.0), px(320.0))),
                    ..Default::default()
                },
                move |window, cx| {
                    window.set_window_title(&title);
                    cx.new(|cx| {
                        let page =
                            cx.new(|cx| PageView::in_plugin_window(&host, window_id, &source, cx));
                        for registered in &windows {
                            registered.set_active(window.is_window_active());
                        }
                        let focus = cx.focus_handle();
                        focus.focus(window, cx);
                        PluginViewWindow {
                            page,
                            windows,
                            focus,
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
