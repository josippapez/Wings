//! Pages of the desktop's plugins as tabs. The sidebar's Plugins list offers the pages that open as tabs and have a native
//! build, and each one open runs a frame of its plugin until the tab closes.
//!
//! The desktop also lists these pages in the + menu, as title bar buttons and under a project's sections, and a page can
//! be opened with arguments by a row of another page. Only the sidebar list is here, and a page opens without arguments.

use gpui::{
    AnyElement, Context, Entity, FocusHandle, FontWeight, Role, SharedString, Window, div,
    prelude::*, px, rgb, rgba,
};

use crate::{
    icons::{Icon, icon},
    page::PageView,
    saved::SavedPluginTab,
    theme,
};

use super::{
    Active, HAIRLINE, Workspace,
    project::{SIDEBAR_ACCENT, SIDEBAR_FOREGROUND},
};

/// A page of a plugin open as a tab. Its frame stops when the tab is dropped.
pub(super) struct PluginTab {
    pub(super) id: usize,
    pub(super) plugin: String,
    pub(super) view: String,
    pub(super) title: SharedString,
    pub(super) page: Entity<PageView>,
    pub(super) focus: FocusHandle,
}

impl Workspace {
    /// Shows the plugin's page in this project, opening it unless it is open already. A page that sets `multiple` opens
    /// another tab each time. False when no running plugin has that page.
    pub fn open_plugin_view(
        &mut self,
        plugin: &str,
        view: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let multiple = self
            .plugins
            .iter()
            .filter(|installed| installed.id == plugin)
            .flat_map(|installed| &installed.pages)
            .find(|page| page.id == view)
            .map(|page| page.multiple);
        let Some(multiple) = multiple else {
            return false;
        };
        let open = self
            .plugin_tabs
            .iter()
            .find(|tab| tab.plugin == plugin && tab.view == view);
        let id = match open {
            Some(tab) if !multiple => Some(tab.id),
            _ => self.new_plugin_tab(plugin, view, cx),
        };
        let Some(id) = id else {
            return false;
        };
        self.show(Active::Plugin(id), window, cx);
        true
    }

    /// The tab's id: it starts the page's frame, so nothing but the tab and its place in the strip is left to do.
    pub(super) fn new_plugin_tab(
        &mut self,
        plugin: &str,
        view: &str,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        let installed = self
            .plugins
            .iter()
            .find(|installed| installed.id == plugin)?;
        let page = installed.pages.iter().find(|page| page.id == view)?;
        let (host, source, title) = (
            installed.host.clone(),
            page.source.clone(),
            page.title.clone(),
        );
        let page = cx.new(|cx| PageView::in_plugin(&host, &source, cx));
        let id = self.next_tab;
        self.next_tab += 1;
        self.plugin_tabs.push(PluginTab {
            id,
            plugin: plugin.into(),
            view: view.into(),
            title: title.into(),
            page,
            focus: cx.focus_handle().tab_stop(true),
        });
        Some(id)
    }

    /// Opens the pages a project had open, except those whose plugin is off, gone or no longer has the page, as the
    /// desktop drops the view tabs of a plugin that is turned off. Each saved page's new tab, if it got one.
    pub(super) fn restore_plugin_tabs(
        &mut self,
        saved: &[SavedPluginTab],
        cx: &mut Context<Self>,
    ) -> Vec<Option<usize>> {
        saved
            .iter()
            .map(|tab| self.new_plugin_tab(&tab.plugin, &tab.view, cx))
            .collect()
    }

    /// Stops the page's frame, and shows the tab next to it, as closing a terminal does.
    pub(super) fn close_plugin_tab(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab = self.plugin_tabs.remove(index);
        self.after_closing(
            Active::Plugin(tab.id),
            self.terminals.len() + index,
            window,
            cx,
        );
    }

    /// The sidebar's Plugins section, or nothing while no plugin offers a page.
    pub(super) fn plugin_list(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let shown = match self.active {
            Active::Plugin(id) => self.plugin_tabs.iter().find(|tab| tab.id == id),
            _ => None,
        };
        let rows: Vec<_> = self
            .plugins
            .iter()
            .flat_map(|installed| installed.pages.iter().map(move |page| (installed, page)))
            .enumerate()
            .map(|(index, (installed, page))| {
                let active =
                    shown.is_some_and(|tab| tab.plugin == installed.id && tab.view == page.id);
                let (plugin, view) = (installed.id.clone(), page.id.clone());
                div()
                    .id(("plugin-page", index))
                    .role(Role::Button)
                    .aria_label(page.title.clone())
                    .aria_selected(active)
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .py(px(3.0))
                    .px(px(8.0))
                    .rounded(px(8.0))
                    .cursor_pointer()
                    .when(active, |row| row.bg(rgba(SIDEBAR_ACCENT)))
                    .when(!active, |row| {
                        row.hover(|style| style.bg(rgba(theme::HOVER)))
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_plugin_view(&plugin, &view, window, cx);
                    }))
                    .child(icon(Icon::Puzzle, 16.0, theme::MUTED_FOREGROUND))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(px(13.0))
                            .line_height(px(18.0))
                            .font_weight(if active {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::MEDIUM
                            })
                            .text_color(rgb(SIDEBAR_FOREGROUND))
                            .child(page.title.clone()),
                    )
            })
            .collect();
        if rows.is_empty() {
            return None;
        }
        Some(
            div()
                .id("plugins")
                .role(Role::List)
                .aria_label("Plugins")
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .p(px(8.0))
                .border_t_1()
                .border_color(rgba(HAIRLINE))
                .child(
                    div()
                        .h(px(28.0))
                        .flex()
                        .items_center()
                        .px(px(8.0))
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(theme::MUTED_FOREGROUND))
                        .child("Plugins"),
                )
                .children(rows)
                .into_any_element(),
        )
    }
}
