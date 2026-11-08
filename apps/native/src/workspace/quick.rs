//! The sidebar's terminals and the quick terminal, as the desktop's `quick-terminal.tsx` and its `looseReducer`: shells
//! in your home folder that belong to no project. The sidebar's Terminal row opens one filling the pane area as a page
//! of its own; Ctrl-` shows the last one floating over whatever is on screen, and hides it again with its shell still
//! running. Drag a floating one's header to move it, its top-left corner to resize it, fill the window with it like a
//! tab, or minimise it to a bar. Like the desktop's, they're in no tab, so nothing saves them.

use std::{path::PathBuf, rc::Rc};

use gpui::{
    AnyElement, Bounds, Context, Entity, FocusHandle, Focusable, Pixels, Point, Subscription,
    Window, canvas, div, prelude::*, px, rgb,
};

#[cfg(unix)]
use crate::terminal_server::LOOSE;
use crate::{
    icons::{Icon, icon},
    terminal_view::TerminalView,
    theme,
    ui::{self, ButtonSize},
};

use super::{
    Workspace,
    floating::{EDGE, Gesture, Panel, Rect},
    panes::{Ended, tildify},
};

/// How the shortcut reads, the desktop's `hotkeyLabel("Ctrl+Backquote")`.
pub(super) const SHORTCUT: &str = if cfg!(target_os = "macos") {
    "⌃`"
} else {
    "Ctrl+`"
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Place {
    /// Out of sight, its shell still running.
    Hidden,
    Floating,
    /// Fills the pane area in place of the tab.
    Full,
}

pub(super) struct QuickTerminal {
    pub(super) key: usize,
    view: Entity<TerminalView>,
    pub(super) place: Place,
    pub(super) minimised: bool,
    pub(super) rect: Rect,
    gesture: Option<Gesture>,
    /// The pane area the panel floats over, measured every frame.
    area: Option<Bounds<Pixels>>,
    /// Takes its row out once its shell exits.
    _exit: Subscription,
    /// Its sidebar row, which shows the close button while the row or the button has focus, as the desktop's
    /// `group-focus-within/row`.
    pub(super) row: FocusHandle,
    pub(super) row_focused: bool,
    _row_focus: [Subscription; 2],
}

impl QuickTerminal {
    pub(super) fn view(&self) -> &Entity<TerminalView> {
        &self.view
    }

    /// Where its shell is now, as the desktop's `looseTitle`.
    pub(super) fn title(&self, cx: &gpui::App) -> String {
        self.view
            .read(cx)
            .cwd()
            .map_or_else(|| "~".to_owned(), |cwd| tildify(&cwd.to_string_lossy()))
    }
}

impl Workspace {
    pub(super) fn loose_terminal(&self, key: usize) -> Option<&QuickTerminal> {
        self.loose.iter().find(|t| t.key == key)
    }

    /// The one filling the pane area in place of the tab, the desktop's `loose.page`.
    pub(super) fn loose_page(&self) -> Option<usize> {
        self.loose
            .iter()
            .find(|t| t.place == Place::Full)
            .map(|t| t.key)
    }

    /// Ctrl-`: shows the last one floating, or hides it when it's on screen. With none, it opens one.
    pub(super) fn toggle_quick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = self
            .loose_last
            .and_then(|key| self.loose_terminal(key))
            .map(|t| (t.key, t.place));
        match target {
            None => self.open_loose(Place::Floating, window, cx),
            Some((key, Place::Hidden)) => self.place_quick(key, Place::Floating, false, window, cx),
            Some((key, _)) => self.place_quick(key, Place::Hidden, false, window, cx),
        }
    }

    /// Starts a shell in the home folder, there: the sidebar's Terminal row fills the pane area with it.
    pub(super) fn open_loose(&mut self, place: Place, window: &mut Window, cx: &mut Context<Self>) {
        let home = std::env::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        let key = self.next_loose;
        #[cfg(unix)]
        let served = {
            // Unique among the server's panes, though nothing looks it up: no saved pane names a loose terminal.
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let history = format!("{nanos:x}-loose-{key}");
            self.open_new_in_server(LOOSE, &history, &home, &[], &None, cx)
        };
        #[cfg(not(unix))]
        let served = None;
        let view = match served
            .map(Ok)
            .unwrap_or_else(|| TerminalView::open(home, Vec::new(), None, cx))
        {
            Ok(view) => view,
            Err(error) => return self.show_notice("Couldn't start a terminal".into(), error, cx),
        };
        self.next_loose += 1;
        let exit = self.close_on_exit(&view, Ended::Loose(key), cx);
        let row = cx.focus_handle();
        let row_focus = [
            cx.on_focus_in(&row, window, move |this, _, cx| {
                this.loose_row_focused(key, true, cx)
            }),
            cx.on_focus_out(&row, window, move |this, _, _, cx| {
                this.loose_row_focused(key, false, cx)
            }),
        ];
        self.loose.push(QuickTerminal {
            key,
            view,
            place: Place::Hidden,
            minimised: false,
            rect: Rect {
                right: EDGE * 2.0,
                bottom: EDGE * 2.0,
                w: 640.0,
                h: 380.0,
            },
            gesture: None,
            area: None,
            _exit: exit,
            row,
            row_focused: false,
            _row_focus: row_focus,
        });
        self.place_quick(key, place, false, window, cx);
    }

    /// Puts one there. Filling the pane area, it takes the place of any other that did. On screen it takes the keys;
    /// going out of sight gives them back to the tab.
    pub(super) fn place_quick(
        &mut self,
        key: usize,
        place: Place,
        minimised: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.loose_terminal(key).is_none() {
            return;
        }
        if place == Place::Full {
            for other in &mut self.loose {
                if other.key != key && other.place == Place::Full {
                    other.place = Place::Hidden;
                    other
                        .view
                        .update(cx, |view, cx| view.set_visible(false, cx));
                }
            }
        }
        let Some(quick) = self.loose.iter_mut().find(|t| t.key == key) else {
            return;
        };
        quick.place = place;
        quick.minimised = minimised && place == Place::Floating;
        let shown = place != Place::Hidden && !quick.minimised;
        let view = quick.view.clone();
        // Showing or floating one makes it the one Ctrl-` acts on, as the desktop's `last`.
        if shown {
            self.loose_last = Some(key);
        }
        view.update(cx, |view, cx| view.set_visible(shown, cx));
        if shown {
            self.sync_pane_visibility(cx);
            view.focus_handle(cx).focus(window, cx);
        } else {
            self.show(self.active, window, cx);
        }
        cx.notify();
    }

    /// Going to a tab or a project puts the one filling the pane area away, as going to another tab would.
    pub(super) fn leave_loose(&mut self, cx: &mut Context<Self>) {
        let Some(quick) = self.loose.iter_mut().find(|t| t.place == Place::Full) else {
            return;
        };
        quick.place = Place::Hidden;
        quick
            .view
            .update(cx, |view, cx| view.set_visible(false, cx));
        self.sync_pane_visibility(cx);
        cx.notify();
    }

    fn loose_row_focused(&mut self, key: usize, focused: bool, cx: &mut Context<Self>) {
        if let Some(quick) = self.loose.iter_mut().find(|t| t.key == key) {
            quick.row_focused = focused;
            cx.notify();
        }
    }

    /// The floating one being typed in.
    pub(super) fn focused_loose(&self, window: &Window, cx: &gpui::App) -> Option<usize> {
        self.loose
            .iter()
            .find(|t| {
                t.place == Place::Floating && t.view.focus_handle(cx).contains_focused(window, cx)
            })
            .map(|t| t.key)
    }

    /// Ends its shell and takes its row out of the sidebar, as its shell exiting does.
    pub(super) fn close_quick(&mut self, key: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(at) = self.loose.iter().position(|t| t.key == key) else {
            return;
        };
        let quick = self.loose.remove(at);
        if self.loose_last == Some(key) {
            self.loose_last = self.loose.last().map(|t| t.key);
        }
        // The keys go back to the tab when they were in this terminal or its row, or nowhere, as the desktop's
        // `focusBackFrom`; a shell ending out of sight leaves them where you're typing.
        let had_keys = window.focused(cx).is_none()
            || quick.view.focus_handle(cx).contains_focused(window, cx)
            || quick.row.contains_focused(window, cx);
        if had_keys {
            self.show(self.active, window, cx);
        } else {
            self.sync_pane_visibility(cx);
            cx.notify();
        }
    }

    fn quick_gesture(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(quick) = self.loose.iter_mut().find(|t| t.gesture.is_some()) else {
            return;
        };
        let (Some(gesture), Some(area)) = (quick.gesture, quick.area) else {
            return;
        };
        quick.rect = gesture.rect_at(position, area);
        cx.notify();
    }

    /// The panels over the pane area: the one filling it, and the floating ones over that.
    pub(super) fn quick_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let shown: Vec<&QuickTerminal> = self
            .loose
            .iter()
            .filter(|t| t.place != Place::Hidden)
            .collect();
        if shown.is_empty() {
            return None;
        }
        let full = shown.iter().any(|t| t.place == Place::Full);
        // The one filling the area first, so the floating ones paint over it.
        let panels: Vec<AnyElement> = shown
            .iter()
            .filter(|t| t.place == Place::Full)
            .chain(shown.iter().filter(|t| t.place != Place::Full))
            .map(|quick| self.quick_panel(quick, cx))
            .collect();
        let entity = cx.entity();
        let measure = canvas(
            move |bounds, _, cx| {
                entity.update(cx, |this, _| {
                    for quick in &mut this.loose {
                        quick.area = Some(bounds);
                    }
                })
            },
            {
                let entity = cx.entity();
                move |_, _, window, _| {
                    ui::window_drag(
                        window,
                        &entity,
                        |this, event, _, cx| {
                            if event.dragging() {
                                this.quick_gesture(event.position, cx);
                            }
                        },
                        |this, _, _| {
                            for quick in &mut this.loose {
                                quick.gesture = None;
                            }
                        },
                    );
                }
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        Some(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .child(measure)
                .when(full, |layer| layer.flex().p(px(8.0)).pt_0())
                .children(panels)
                .into_any_element(),
        )
    }

    fn quick_panel(&self, quick: &QuickTerminal, cx: &mut Context<Self>) -> AnyElement {
        let key = quick.key;
        let full = quick.place == Place::Full;
        let collapsed = quick.minimised && !full;
        let title = quick.title(cx);
        let mut header = vec![
            icon(Icon::SquareTerminal, 14.0, theme::MUTED_FOREGROUND).into_any_element(),
            div()
                .min_w_0()
                .truncate()
                .font_weight(gpui::FontWeight::MEDIUM)
                .child(title)
                .into_any_element(),
        ];
        if !full && self.prefs.quick_terminal && self.loose_last == Some(key) {
            header.push(
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(rgb(theme::MUTED_FOREGROUND))
                    .child(SHORTCUT)
                    .into_any_element(),
            );
        }
        header.push(div().flex_1().into_any_element());
        if !collapsed {
            header.push(
                ui::icon_button(
                    ("quick-fill", key),
                    if full { Icon::Minimize } else { Icon::Maximize },
                    if full {
                        "Detach to a floating panel"
                    } else {
                        "Fill the window like a tab"
                    },
                    ButtonSize::Xs,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    let to = if full { Place::Floating } else { Place::Full };
                    this.place_quick(key, to, false, window, cx)
                }))
                .into_any_element(),
            );
        }
        if !collapsed && !full {
            header.push(
                ui::icon_button(
                    ("quick-minimise", key),
                    Icon::Minus,
                    "Minimise to a bar",
                    ButtonSize::Xs,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.place_quick(key, Place::Floating, true, window, cx)
                }))
                .into_any_element(),
            );
        }
        header.push(
            ui::icon_button(
                ("quick-close", key),
                Icon::X,
                "Close and end the shell",
                if collapsed {
                    ButtonSize::Tiny
                } else {
                    ButtonSize::Xs
                },
            )
            .on_click(cx.listener(move |this, _, window, cx| this.close_quick(key, window, cx)))
            .into_any_element(),
        );
        let panel = Panel {
            id: format!("quick-terminal-{key}").into(),
            label: "Terminal".into(),
            rect: quick.rect,
            area: quick.area,
            collapsed,
            full,
        };
        panel.render(
            header,
            quick.view.clone().into_any_element(),
            Rc::new(move |this: &mut Workspace, resize, from| {
                if let Some(quick) = this.loose.iter_mut().find(|t| t.key == key) {
                    quick.gesture = Some(Gesture::new(resize, from, quick.rect));
                }
            }),
            Rc::new(move |this: &mut Workspace, window, cx| {
                this.place_quick(key, Place::Floating, false, window, cx)
            }),
            cx,
        )
    }
}
