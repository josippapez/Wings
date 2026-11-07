//! Pages of the desktop's plugins as tabs. The sidebar's Plugins list offers the pages that open as tabs and have a native
//! build, and each one open runs a frame of its plugin until the tab closes. A plugin that is on but has no native build
//! is listed too, as needing an update for the native app.
//!
//! The desktop also lists these pages in the + menu, as title bar buttons and under a project's sections. Only the
//! sidebar list is here, and a page opens from it without arguments; `wings.openView` can open one with arguments.

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
    /// What `wings.openView` opened it with, which the page reads with `viewArgs`.
    pub(super) args: Option<serde_json::Value>,
    pub(super) state: crate::page::plugin::FrameState,
    pub(super) page: Entity<PageView>,
    pub(super) focus: FocusHandle,
}

impl Workspace {
    pub(super) fn has_tasks_board(&self) -> bool {
        self.plugins.iter().any(|plugin| {
            plugin.id == "tasks" && plugin.pages.iter().any(|page| page.id == "board")
        })
    }

    fn is_tasks_board(plugin: &str, view: &str) -> bool {
        plugin == "tasks" && view == "board"
    }
    /// Shows the plugin's page in this project, opening it unless it is open already. A page that sets `multiple` opens
    /// another tab each time, unless one has the same `args`. A tab that is open with other `args` takes the new ones and
    /// starts over, as the desktop's `openViewTab` does. False when no running plugin has that page.
    pub fn open_plugin_view(
        &mut self,
        plugin: &str,
        view: &str,
        args: Option<serde_json::Value>,
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
        let same_view = |tab: &&PluginTab| tab.plugin == plugin && tab.view == view;
        let with_these_args = if multiple && args.is_some() {
            self.plugin_tabs
                .iter()
                .find(|tab| same_view(tab) && tab.args == args)
                .map(|tab| tab.id)
        } else {
            None
        };
        let open = if multiple {
            None
        } else {
            self.plugin_tabs.iter().find(same_view)
        };
        let id = match (with_these_args, open) {
            (Some(id), _) => Some(id),
            (None, Some(tab)) if args.is_none() || tab.args == args => Some(tab.id),
            (None, Some(tab)) => {
                let id = tab.id;
                let state = self.plugin_env.tab_state(None);
                self.plugin_page(plugin, view, args.as_ref(), state.clone(), cx)
                    .and_then(|(page, _)| {
                        let tab = self.plugin_tabs.iter_mut().find(|tab| tab.id == id)?;
                        tab.page = page;
                        tab.args = args;
                        tab.state = state;
                        Some(id)
                    })
            }
            (None, None) => self.new_plugin_tab(plugin, view, args, cx),
        };
        let Some(id) = id else {
            return false;
        };
        if Self::is_tasks_board(plugin, view) {
            self.issues_open = true;
        }
        self.show(Active::Plugin(id), window, cx);
        true
    }

    /// The page's frame, started with `args`, and the page's title.
    fn plugin_page(
        &self,
        plugin: &str,
        view: &str,
        args: Option<&serde_json::Value>,
        state: crate::page::plugin::FrameState,
        cx: &mut Context<Self>,
    ) -> Option<(Entity<PageView>, String)> {
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
        let page = cx.new(|cx| PageView::in_plugin_with_state(&host, &source, args, state, cx));
        Some((page, title))
    }

    /// The tab's id: it starts the page's frame, so nothing but the tab and its place in the strip is left to do.
    pub(super) fn new_plugin_tab(
        &mut self,
        plugin: &str,
        view: &str,
        args: Option<serde_json::Value>,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        self.new_plugin_tab_with_state(plugin, view, args, None, cx)
    }

    fn new_plugin_tab_with_state(
        &mut self,
        plugin: &str,
        view: &str,
        args: Option<serde_json::Value>,
        initial: Option<serde_json::Value>,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        let state = self.plugin_env.tab_state(initial);
        let (page, title) = self.plugin_page(plugin, view, args.as_ref(), state.clone(), cx)?;
        let id = self.next_tab;
        self.next_tab += 1;
        self.plugin_tabs.push(PluginTab {
            id,
            plugin: plugin.into(),
            view: view.into(),
            title: title.into(),
            args,
            state,
            page,
            focus: cx.focus_handle().tab_stop(true),
        });
        Some(id)
    }

    /// Closes the plugin's page `view` wherever it shows: its tabs in this project, its place in the panel on the right,
    /// and its window. A view a plugin has no page for is left alone.
    pub(super) fn close_plugin_view(
        &mut self,
        plugin: &str,
        view: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        while let Some(index) = self
            .plugin_tabs
            .iter()
            .position(|tab| tab.plugin == plugin && tab.view == view)
        {
            self.close_plugin_tab(index, window, cx);
        }
        let key = (plugin.to_owned(), view.to_owned());
        if self.right.as_ref() == Some(&key) {
            self.right = None;
        }
        self.sidebar_pages.remove(&key);
        if let Some(handle) = self.plugin_ui.windows.remove(&key) {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
        self.sync_plugin_visibility(cx);
        cx.notify();
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
            .map(|tab| {
                self.new_plugin_tab_with_state(
                    &tab.plugin,
                    &tab.view,
                    tab.args.clone(),
                    tab.state.clone(),
                    cx,
                )
            })
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
        if Self::is_tasks_board(&tab.plugin, &tab.view) {
            self.issues_open = false;
        }
        self.after_closing(
            Active::Plugin(tab.id),
            self.terminals.len() + index,
            window,
            cx,
        );
    }

    /// The plugins that are on and approved but don't run here, which the desktop runs: they have no native build. As
    /// `(id, name)`. Kept until `plugins.json` or the running plugins change, since listing the store takes about half a
    /// millisecond for a dozen plugins and the sidebar draws every frame.
    pub(super) fn plugins_without_native_build(&self) -> Vec<(String, String)> {
        let written = self
            .config
            .plugins
            .as_ref()
            .and_then(|paths| paths.state.metadata().ok())
            .and_then(|meta| meta.modified().ok());
        let running: Vec<String> = self
            .plugins
            .iter()
            .map(|installed| installed.id.clone())
            .collect();
        let key = (written, running);
        let mut cached = self.plugin_ui.needs_update.borrow_mut();
        if cached.0.as_ref() != Some(&key) {
            let list = self
                .plugin_views()
                .into_iter()
                .filter(|view| {
                    view.enabled && view.approved && !self.plugin_runs(&view.manifest.id)
                })
                .map(|view| (view.manifest.id, view.manifest.name))
                .collect();
            *cached = (Some(key), list);
        }
        cached.1.clone()
    }

    /// The sidebar's Plugins section, or nothing while no plugin offers a page or waits for a native build.
    pub(super) fn plugin_list(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let shown = match self.active {
            Active::Plugin(id) => self.plugin_tabs.iter().find(|tab| tab.id == id),
            _ => None,
        };
        let rows: Vec<_> = self
            .plugins
            .iter()
            .flat_map(|installed| {
                installed
                    .pages
                    .iter()
                    .filter(|page| page.new_tab)
                    .map(move |page| (installed, page))
            })
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
                    .rounded(px(theme::RADIUS))
                    .cursor_pointer()
                    .when(active, |row| row.bg(rgba(SIDEBAR_ACCENT)))
                    .when(!active, |row| {
                        row.hover(|style| style.bg(rgba(theme::HOVER)))
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_plugin_view(&plugin, &view, None, window, cx);
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
        // On and approved, but with no native build: the desktop runs it, and here it is listed with what it needs,
        // rather than missing, and opens its entry in Settings → Plugins. More than two take one row, so a dozen
        // desktop-only plugins don't push the projects off the sidebar.
        let mut waiting = self.plugins_without_native_build();
        let mut needs = "Needs an update for the native app";
        if waiting.len() > 2 {
            let first = waiting[0].0.clone();
            waiting = vec![(first, format!("{} plugins", waiting.len()))];
            needs = "Need an update for the native app";
        }
        let waiting: Vec<_> = waiting
            .into_iter()
            .enumerate()
            .map(|(index, (id, name))| {
                div()
                    .id(("plugin-needs-update", index))
                    .role(Role::Button)
                    .aria_label(format!("{name}, {}", needs.to_lowercase()))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .py(px(3.0))
                    .px(px(8.0))
                    .rounded(px(theme::RADIUS))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgba(theme::HOVER)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_settings(window, cx);
                        if let Some(dialog) = &mut this.settings {
                            dialog.section = super::settings::Section::Plugins;
                        }
                        this.manage.selected = Some(id.clone());
                        cx.notify();
                    }))
                    .child(icon(Icon::Puzzle, 16.0, theme::MUTED_FOREGROUND))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(13.0))
                                    .line_height(px(18.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(theme::MUTED_FOREGROUND))
                                    .child(name),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(11.0))
                                    .line_height(px(15.0))
                                    .text_color(rgb(theme::MUTED_FOREGROUND))
                                    .child(needs),
                            ),
                    )
            })
            .collect();
        if rows.is_empty() && waiting.is_empty() {
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
                .gap(px(1.0))
                // Drawn like the Agents section under it.
                .mx(px(8.0))
                .pt(px(8.0))
                .pb(px(8.0))
                .border_t_1()
                .border_color(rgba(HAIRLINE))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .px(px(8.0))
                        .pb(px(4.0))
                        .line_height(px(18.0))
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(theme::MUTED_FOREGROUND))
                        .child("Plugins"),
                )
                .children(rows)
                .children(waiting)
                .into_any_element(),
        )
    }
}
