//! A plugin's view that opens as a modal: its page in a dialog over the window, as Wings' own dialogs are, until the
//! plugin closes it, or you press Escape or click outside it.

use gpui::{AnyElement, Context, Entity, FocusHandle, KeyDownEvent, MouseButton, Window, div, prelude::*, px};

use super::Workspace;
use crate::{
    page::PageView,
    ui::{self, behaviors::{EscapeExt, FocusRecovery, TrapTabExt}},
};

pub(super) struct PluginModal {
    /// The plugin's id and the view's.
    key: (String, String),
    title: String,
    size: (f32, f32),
    page: Entity<PageView>,
    focus: FocusHandle,
    recovery: FocusRecovery,
}

impl Workspace {
    /// Shows the plugin's modal view `id`, in place of any other plugin's that shows.
    pub(super) fn open_plugin_modal(
        &mut self,
        plugin: &str,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let key = (plugin.to_owned(), id.to_owned());
        if self.plugin_ui.modal.as_ref().is_some_and(|modal| modal.key == key) {
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
            .find(|view| view.id == id && view.opens.iter().any(|place| place == "modal"))
            .ok_or_else(|| format!("{plugin} has no modal view {id}"))?;
        let stem = std::path::Path::new(&view.page)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or("The view has no native page")?;
        let file = crate::installed::frame_file(&installed.plugin.dir, stem)
            .ok_or("The view has no native modal page")?;
        let source = crate::installed::page_source(&installed.plugin.dir, &file)
            .map_err(|error| error.to_string())?;
        let (host, title) = (installed.host.clone(), view.title.clone());
        let size = (
            view.width.unwrap_or(560) as f32,
            view.height.unwrap_or(420) as f32,
        );
        let state = self.plugin_env.tab_state(None);
        let page = cx.new(|cx| PageView::in_plugin_with_state(&host, &source, None, state, cx));
        page.read(cx).set_visible(true);
        let focus = cx.focus_handle();
        // A modal that replaces another keeps what the first one took the keyboard from.
        let recovery = match self.plugin_ui.modal.take() {
            Some(old) => {
                focus.focus(window, cx);
                old.recovery
            }
            None => {
                let mut recovery = FocusRecovery::default();
                recovery.open(&focus, window, cx);
                recovery
            }
        };
        self.plugin_ui.modal = Some(PluginModal {
            key,
            title,
            size,
            page,
            focus,
            recovery,
        });
        cx.notify();
        Ok(())
    }

    /// Closes the modal that shows, if `key` is none or names it, and gives the keys back to the tab shown.
    pub(super) fn close_plugin_modal(
        &mut self,
        key: Option<&(String, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self
            .plugin_ui
            .modal
            .as_ref()
            .is_some_and(|modal| key.is_none_or(|key| &modal.key == key))
        {
            return;
        }
        if let Some(mut modal) = self.plugin_ui.modal.take() {
            modal.page.read(cx).set_visible(false);
            let restored = modal.recovery.restore_within(&modal.focus, window, cx);
            self.leave("plugin-modal", modal, ui::motion::POPUP_IN, cx);
            if !restored {
                self.show(self.active, window, cx);
            }
        }
    }

    pub(super) fn plugin_modal_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.plugin_ui.modal.as_ref()?;
        // As tall as the page, up to the height the plugin gives: an ask of a few lines isn't a tall empty box.
        let (backdrop, panel) = ui::dialog("plugin-modal", modal.title.clone(), &modal.focus, modal.size.0, None);
        let panel = panel.max_h(px(modal.size.1));
        Some(
            backdrop
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.close_plugin_modal(None, window, cx)),
                )
                .child(ui::dialog_panel(
                    panel
                        .trap_tab(&modal.focus)
                        .on_escape(cx.listener(|this, _: &KeyDownEvent, window, cx| {
                            this.close_plugin_modal(None, window, cx);
                        }))
                        .child(div().w_full().child(modal.page.clone())),
                ))
                .into_any_element(),
        )
    }
}
