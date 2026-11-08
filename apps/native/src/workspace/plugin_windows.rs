//! Plugin view windows retain their own frame and report focus separately from the workspace.
use super::Workspace;
use crate::page::{PageView, plugin::PluginWindow};
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
        let file = wings_plugins::loading::resolve(
            &installed.plugin.dir,
            &format!("dist/native/{stem}.js"),
        )
        .filter(|file| file.is_file())
        .ok_or("The view has no native window page")?;
        let source = std::fs::read_to_string(file).map_err(|error| error.to_string())?;
        let title = view.title.clone();
        let (width, height) = (view.width.unwrap_or(1100), view.height.unwrap_or(760));
        self.show_plugin_window(key, plugin, &source, title, (width, height), cx)
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
                        cx.notify();
                    });
                });
            }));
        }
        Ok(())
    }
}
