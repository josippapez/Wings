//! The peek of a closed sidebar, like the desktop's `SidebarPeek`: pointing at the window's left edge floats the sidebar
//! over the panes, which keep their size and don't refit, and it goes when the pointer leaves it or you click elsewhere.

use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, BoxShadow, Context, ElementId, MouseDownEvent, Role, Task,
    div, ease_out_quint, hsla, point, prelude::*, px, rgb, rgba,
};

use crate::theme;

use crate::workspace::Workspace;

/// The pointer is on the edge this long before the sidebar slides out, and off the sidebar this long before it goes.
const SHOW_AFTER: Duration = Duration::from_millis(60);
const HIDE_AFTER: Duration = Duration::from_millis(200);
/// The slide, in and out.
const SLIDE: Duration = Duration::from_millis(250);
/// The width of the strip at the window's edge that brings the sidebar out.
const STRIP: f32 = 8.0;

#[derive(Default)]
pub(super) struct Peek {
    pub(super) open: bool,
    /// Slid out and still sliding, before it is gone.
    pub(super) leaving: bool,
    /// Tells one slide from the next, so each starts over.
    slides: usize,
    /// What happens next, if the pointer stays where it is.
    timer: Option<Task<()>>,
}

impl Workspace {
    /// The strip at the edge, and the sidebar floating over the panes while it shows. Nothing while the sidebar is docked.
    pub(in crate::workspace) fn sidebar_peek(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if self.sidebar {
            return Vec::new();
        }
        let peek = &self.side.peek;
        let mut parts = vec![
            div()
                .id("peek-strip")
                .absolute()
                .top_0()
                .bottom_0()
                .left_0()
                .w(px(STRIP))
                .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                    if *hovered {
                        this.peek_in(SHOW_AFTER, cx, |peek| peek.open = true);
                    } else if !this.side.peek.open {
                        this.side.peek.timer = None;
                    }
                }))
                .into_any_element(),
        ];
        if peek.open || peek.leaving {
            let (open, width) = (peek.open, self.side.width);
            let slide = Animation::new(SLIDE).with_easing(ease_out_quint());
            parts.push(
                self.sidebar_column(cx)
                    .id("peek")
                    .role(Role::Complementary)
                    .aria_label("Projects")
                    .occlude()
                    .absolute()
                    // The column is full height: top and bottom set the panel's height instead.
                    .h_auto()
                    .top(px(8.0))
                    .bottom(px(8.0))
                    .left_0()
                    .w(px(width))
                    .overflow_hidden()
                    .rounded_r(px(12.0))
                    .border_1()
                    .border_l_0()
                    .border_color(rgba(theme::HAIRLINE_STRONG))
                    .bg(rgb(theme::POPOVER >> 8))
                    .shadow(vec![BoxShadow {
                        color: hsla(0.0, 0.0, 0.0, 0.5),
                        offset: point(px(0.0), px(24.0)),
                        blur_radius: px(64.0),
                        spread_radius: px(0.0),
                        inset: false,
                    }])
                    .pt(px(4.0))
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.side.peek.timer = None;
                        } else if this.side.peek.open && !this.peek_holds() {
                            this.peek_in(HIDE_AFTER, cx, |peek| peek.open = false);
                        }
                    }))
                    .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        if this.side.peek.open && !this.peek_holds() {
                            this.leave_peek(cx);
                        }
                    }))
                    .with_animation(
                        ElementId::Name(format!("peek-{}-{open}", peek.slides).into()),
                        slide,
                        move |panel, t| {
                            let t = if open { t } else { 1.0 - t };
                            panel.left(px(-width * (1.0 - t)))
                        },
                    )
                    .into_any_element(),
            );
        }
        parts
    }

    /// A menu or a question of the sidebar is open: they sit outside the panel, so the pointer leaving it for them
    /// mustn't hide it.
    fn peek_holds(&self) -> bool {
        self.side.sort_menu || self.side.removing.is_some()
    }

    /// Runs `change` on the peek after `after`, unless the pointer moved on and that was replaced.
    fn peek_in(
        &mut self,
        after: Duration,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut Peek) + 'static,
    ) {
        self.side.peek.timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(after).await;
            let _ = this.update(cx, |this, cx| {
                let peek = &mut this.side.peek;
                let was = peek.open;
                change(peek);
                if peek.open && !was {
                    peek.slides += 1;
                    peek.leaving = false;
                } else if !peek.open && was {
                    this.leave_peek(cx);
                    return;
                }
                cx.notify();
            });
        }));
    }

    /// Slides the sidebar back out, and drops it when that is done.
    fn leave_peek(&mut self, cx: &mut Context<Self>) {
        let peek = &mut self.side.peek;
        peek.open = false;
        peek.leaving = true;
        peek.slides += 1;
        peek.timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SLIDE).await;
            let _ = this.update(cx, |this, cx| {
                this.side.peek.leaving = false;
                cx.notify();
            });
        }));
        cx.notify();
    }
}
