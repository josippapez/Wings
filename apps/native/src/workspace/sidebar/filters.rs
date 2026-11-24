//! The project list's two persisted visibility choices, using the shared popup and menu behaviors.

use std::{cell::Cell, rc::Rc};
use gpui::{AnyElement, Bounds, Context, FocusHandle, KeyDownEvent, Pixels, Role, Toggled, Window, div, prelude::*, px};

use crate::{icons::{Icon, icon}, theme, ui::{self, behaviors::{self, DismissReason, MenuKey, MenuKeys}}, workspace::{NotifyWorkspace, Workspace}};

pub(super) struct Filters {
    pub(super) idle: bool,
    pub(super) worktrees: bool,
    pub(super) trigger: Rc<Cell<Option<Bounds<Pixels>>>>,
}

impl Default for Filters {
    fn default() -> Self { Self { idle: true, worktrees: true, trigger: Rc::default() } }
}

pub(super) struct Menu {
    focus: FocusHandle,
    keys: MenuKeys,
}

impl Workspace {
    pub(super) fn toggle_project_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.side.filter_menu.is_some() {
            self.close_project_filters(window, cx);
        } else {
            let focus = cx.focus_handle();
            self.side.recovery.open(&focus, window, cx);
            self.side.filter_menu = Some(Menu { focus, keys: MenuKeys::default() });
            cx.notify_workspace();
        }
    }

    fn close_project_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.side.filter_menu.take() {
            self.side.recovery.restore(&menu.focus, window, cx);
            cx.notify_workspace();
        }
    }

    fn pick_project_filter(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        match at {
            0 => self.side.filters.idle = !self.side.filters.idle,
            1 => self.side.filters.worktrees = !self.side.filters.worktrees,
            _ => return,
        }
        self.save(cx);
        self.close_project_filters(window, cx);
    }

    pub(super) fn project_filter_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.side.filter_menu.as_ref()?;
        let choices = [("Show idle projects", self.side.filters.idle), ("Show worktrees", self.side.filters.worktrees)];
        let items = choices.into_iter().enumerate().map(|(at, (label, on))| {
            let check = if on {
                icon(Icon::Check, 16.0, theme::MUTED_FOREGROUND).into_any_element()
            } else {
                div().size(px(16.0)).into_any_element()
            };
            ui::menu_highlight(
                ui::menu_item_with(("project-filter", at), check, label, false, false)
                    .role(Role::MenuItemCheckBox)
                    .aria_toggled(if on { Toggled::True } else { Toggled::False })
                    .on_click(cx.listener(move |this, _, window, cx| this.pick_project_filter(at, window, cx))),
                menu.keys.highlight == Some(at),
            ).into_any_element()
        });
        let content = behaviors::bind_layer(
            ui::menu("project-filters", "Project filters")
                .track_focus(&menu.focus)
                .children(items)
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    let labels = vec![Some("Show idle projects".into()), Some("Show worktrees".into())];
                    let Some(menu) = this.side.filter_menu.as_mut() else { return };
                    match menu.keys.press(event, &labels) {
                        MenuKey::Ignored => return,
                        MenuKey::Moved => cx.notify_workspace(),
                        MenuKey::Pick(at) => this.pick_project_filter(at, window, cx),
                    }
                    cx.stop_propagation();
                })),
            cx.listener(|this, reason: &DismissReason, window, cx| {
                // The trigger's own click toggles it, rather than closing and reopening it.
                let on_trigger = match reason {
                    DismissReason::OutsidePress(at) => this.side.filters.trigger.get()
                        .is_some_and(|bounds| bounds.contains(at)),
                    _ => false,
                };
                if !on_trigger { this.close_project_filters(window, cx); }
            }),
        );
        Some(behaviors::popup_on_parent(content).align(behaviors::Align::End).layer())
    }
}
