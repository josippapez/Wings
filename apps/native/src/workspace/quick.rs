//! The sidebar's terminals and the quick terminal, as the desktop's `quick-terminal.tsx` and its `looseReducer`: shells
//! in your home folder that belong to no project. The sidebar's Terminal row opens one filling the pane area as a page
//! of its own; Ctrl-` shows the last one floating over whatever is on screen, and hides it again with its shell still
//! running. Drag a floating one's header to move it, its top-left corner to resize it, fill the window with it like a
//! tab, or minimise it to a bar. Like the desktop's, they're in no tab, so nothing saves them.

use std::path::PathBuf;

use gpui::{
    AnyElement, Bounds, BoxShadow, Context, DispatchPhase, Entity, Focusable, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Role, Window, canvas, div, hsla,
    point, prelude::*, px, rgb, rgba,
};

use crate::{
    icons::{Icon, icon},
    terminal_view::TerminalView,
    theme,
    ui::{self, ButtonSize},
};

use super::{HAIRLINE, Workspace, panes::tildify};

/// How the shortcut reads, the desktop's `hotkeyLabel("Ctrl+Backquote")`.
pub(super) const SHORTCUT: &str = if cfg!(target_os = "macos") {
    "⌃`"
} else {
    "Ctrl+`"
};

/// The desktop's `PANEL_EDGE`, and the panel's smallest size.
const EDGE: f32 = 12.0;
const MIN_W: f32 = 280.0;
const MIN_H: f32 = 120.0;

/// Where the panel sits, as distances from the bottom-right corner of the pane area, so a smaller window never leaves
/// it out of view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Rect {
    pub(super) right: f32,
    pub(super) bottom: f32,
    pub(super) w: f32,
    pub(super) h: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Place {
    /// Out of sight, its shell still running.
    Hidden,
    Floating,
    /// Fills the pane area in place of the tab.
    Full,
}

/// A move of the header or a resize from the corner: where the pointer and the panel were when it started.
#[derive(Clone, Copy)]
struct Gesture {
    resize: bool,
    from: Point<Pixels>,
    rect: Rect,
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
        let view = match TerminalView::open(home, Vec::new(), None, cx) {
            Ok(view) => view,
            Err(error) => return self.show_notice("Couldn't start a terminal".into(), error, cx),
        };
        let key = self.next_loose;
        self.next_loose += 1;
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
            view.focus_handle(cx).focus(window, cx);
        } else {
            self.show(self.active, window, cx);
        }
        cx.notify();
    }

    /// Going to a tab or a project puts the one filling the pane area away, as going to another tab would.
    pub(super) fn leave_loose(&mut self, cx: &mut Context<Self>) {
        for quick in &mut self.loose {
            if quick.place == Place::Full {
                quick.place = Place::Hidden;
                quick
                    .view
                    .update(cx, |view, cx| view.set_visible(false, cx));
                cx.notify();
            }
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

    /// Ends its shell and takes its row out of the sidebar.
    pub(super) fn close_quick(&mut self, key: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(at) = self.loose.iter().position(|t| t.key == key) else {
            return;
        };
        self.loose.remove(at);
        if self.loose_last == Some(key) {
            self.loose_last = self.loose.last().map(|t| t.key);
        }
        self.show(self.active, window, cx);
    }

    fn quick_gesture(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(quick) = self.loose.iter_mut().find(|t| t.gesture.is_some()) else {
            return;
        };
        let (Some(gesture), Some(area)) = (quick.gesture, quick.area) else {
            return;
        };
        let (dx, dy) = (
            (position.x - gesture.from.x).as_f32(),
            (position.y - gesture.from.y).as_f32(),
        );
        let (width, height) = (area.size.width.as_f32(), area.size.height.as_f32());
        let r = gesture.rect;
        quick.rect = if gesture.resize {
            Rect {
                w: (r.w - dx).clamp(MIN_W, (width - r.right).max(MIN_W)),
                h: (r.h - dy).clamp(MIN_H, (height - r.bottom).max(MIN_H)),
                ..r
            }
        } else {
            Rect {
                right: (r.right - dx).clamp(0.0, (width - r.w).max(0.0)),
                bottom: (r.bottom - dy).clamp(0.0, (height - r.h).max(0.0)),
                ..r
            }
        };
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
                    let moved = entity.clone();
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                        if phase == DispatchPhase::Bubble && event.dragging() {
                            moved.update(cx, |this, cx| this.quick_gesture(event.position, cx));
                        }
                    });
                    let released = entity.clone();
                    window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                        if phase == DispatchPhase::Bubble {
                            released.update(cx, |this, _| {
                                for quick in &mut this.loose {
                                    quick.gesture = None;
                                }
                            });
                        }
                    });
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
        let header = div()
            .id(("quick-terminal-header", key))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.0))
            .h(px(if collapsed { 28.0 } else { 32.0 }))
            .pl(px(12.0))
            .pr(px(4.0))
            .text_size(px(12.0))
            .when(!full, |header| {
                header.cursor_grab().on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        if let Some(quick) = this.loose.iter_mut().find(|t| t.key == key) {
                            quick.gesture = Some(Gesture {
                                resize: false,
                                from: event.position,
                                rect: quick.rect,
                            });
                        }
                        cx.stop_propagation();
                    }),
                )
            })
            .when(collapsed, |header| {
                header.on_click(cx.listener(move |this, _, window, cx| {
                    this.place_quick(key, Place::Floating, false, window, cx)
                }))
            })
            .child(icon(Icon::SquareTerminal, 14.0, theme::MUTED_FOREGROUND))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(title),
            )
            .when(
                !full && self.prefs.quick_terminal && self.loose_last == Some(key),
                |header| {
                    header.child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(rgb(theme::MUTED_FOREGROUND))
                            .child(SHORTCUT),
                    )
                },
            )
            .child(div().flex_1())
            .when(!collapsed, |header| {
                header.child(
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
                    })),
                )
            })
            .when(!collapsed && !full, |header| {
                header.child(
                    ui::icon_button(
                        ("quick-minimise", key),
                        Icon::Minus,
                        "Minimise to a bar",
                        ButtonSize::Xs,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.place_quick(key, Place::Floating, true, window, cx)
                    })),
                )
            })
            .child(
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
                .on_click(
                    cx.listener(move |this, _, window, cx| this.close_quick(key, window, cx)),
                ),
            );
        let panel = div()
            .id(("quick-terminal", key))
            .role(Role::Group)
            .aria_label("Terminal")
            .flex()
            .flex_col()
            .overflow_hidden()
            .border_1()
            .bg(rgb(theme::SURFACE))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(header)
            .when(!collapsed, |panel| {
                panel.child(div().flex_1().min_h_0().child(quick.view.clone()))
            })
            .map(|panel| {
                if full {
                    panel
                        .relative()
                        .flex_1()
                        .min_w_0()
                        .rounded(px(14.0))
                        .border_color(rgba(HAIRLINE))
                } else {
                    let r = quick.rect;
                    // Never wider or taller than the area it floats over, like the desktop's `max-w-full`.
                    let (w, h) = quick.area.map_or((r.w, r.h), |area| {
                        (
                            r.w.min(area.size.width.as_f32() - r.right),
                            r.h.min(area.size.height.as_f32() - r.bottom),
                        )
                    });
                    panel
                        .absolute()
                        .right(px(r.right))
                        .bottom(px(r.bottom))
                        .border_color(rgba(theme::HAIRLINE_STRONG))
                        .shadow(vec![BoxShadow {
                            color: hsla(0.0, 0.0, 0.0, 0.35),
                            offset: point(px(0.0), px(10.0)),
                            blur_radius: px(15.0),
                            spread_radius: px(-3.0),
                            inset: false,
                        }])
                        .map(|panel| {
                            if collapsed {
                                panel.h(px(28.0)).rounded_full().bg(rgba(theme::POPOVER))
                            } else {
                                panel.w(px(w)).h(px(h)).rounded(px(12.0))
                            }
                        })
                }
            })
            .when(!collapsed && !full, |panel| {
                panel.child(
                    div()
                        .id(("quick-resize", key))
                        .absolute()
                        .top_0()
                        .left_0()
                        .size(px(16.0))
                        .cursor_nwse_resize()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                if let Some(quick) = this.loose.iter_mut().find(|t| t.key == key) {
                                    quick.gesture = Some(Gesture {
                                        resize: true,
                                        from: event.position,
                                        rect: quick.rect,
                                    });
                                }
                                cx.stop_propagation();
                            }),
                        ),
                )
            });
        panel.into_any_element()
    }
}
