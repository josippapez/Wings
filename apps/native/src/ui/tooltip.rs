//! shadcn's `Tooltip` with a `Kbd`, as the desktop's `IconButton` shows one: the button's name and its shortcut in a
//! light pill, half a second after the pointer stops on it (GPUI's delay, the desktop's `TooltipProvider delay={500}`).
//! It fades and slides in as the desktop's does; GPUI drops it at once when the pointer leaves, so it doesn't fade out.

use gpui::{
    Animation, AnimationExt, AnyView, App, Context, FontWeight, IntoElement, Render, SharedString,
    Window, div, prelude::*, px, rgb, rgba,
};

use crate::{theme, ui::motion};

pub struct Tooltip {
    label: SharedString,
    shortcut: Option<SharedString>,
}

/// What `.tooltip(..)` takes: a tooltip of `label`, with `shortcut` in a key cap when there is one.
pub fn tooltip(
    label: impl Into<SharedString>,
    shortcut: Option<SharedString>,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let label = label.into();
    move |_, cx| {
        let (label, shortcut) = (label.clone(), shortcut.clone());
        cx.new(|_| Tooltip { label, shortcut }).into()
    }
}

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // GPUI puts a tooltip's corner at the pointer, so it starts below the cursor's arrow.
        div().pt(px(20.0)).child(
            div()
                .relative()
                .max_w(px(320.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .px(px(12.0))
                .py(px(6.0))
                .when(self.shortcut.is_some(), |pill| pill.pr(px(6.0)))
                .rounded(px(6.0))
                .bg(rgb(theme::FOREGROUND))
                .text_size(px(12.0))
                .text_color(rgb(theme::BACKGROUND))
                .child(self.label.clone())
                .when_some(self.shortcut.clone(), |pill, keys| {
                    pill.child(
                        div()
                            .h(px(20.0))
                            .min_w(px(20.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .px(px(4.0))
                            .rounded(px(4.0))
                            .bg(rgba(theme::BACKGROUND << 8 | 0x1a))
                            .font_weight(FontWeight::MEDIUM)
                            .child(keys),
                    )
                })
                // Below the pointer, it comes down into place, as the desktop's `slide-in-from-top-2` on a tooltip
                // below its trigger.
                .with_animation(
                    "tooltip-in",
                    Animation::new(motion::TOOLTIP_IN),
                    |pill, t| {
                        let (opacity, offset) = motion::enter_frame(t);
                        pill.opacity(opacity).top(px(-offset))
                    },
                ),
        )
    }
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use std::{cell::Cell, rc::Rc, time::Duration};

    use gpui::{TestApp, point};

    use super::*;

    /// A button-sized box with one of our tooltips, counting each time GPUI makes it.
    struct Trigger(Rc<Cell<usize>>);

    impl Render for Trigger {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let made = self.0.clone();
            div()
                .size_full()
                .child(
                    div()
                        .id("trigger")
                        .size(px(28.0))
                        .tooltip(move |window, cx| {
                            made.set(made.get() + 1);
                            tooltip("Close tab", Some("⌘W".into()))(window, cx)
                        }),
                )
        }
    }

    #[test]
    fn a_tooltip_shows_half_a_second_after_the_pointer_stops_on_its_button() {
        let mut app = TestApp::new();
        let made = Rc::new(Cell::new(0));
        let counter = made.clone();
        let mut window = app.open_window(move |_, _| Trigger(counter));
        window.draw();
        window.simulate_mouse_move(point(px(10.0), px(10.0)));
        let wait = |window: &mut gpui::TestAppWindow<Trigger>, ms: u64| {
            app.advance_clock(Duration::from_millis(ms));
            app.run_until_parked();
            window.draw();
        };
        wait(&mut window, 490);
        assert_eq!(made.get(), 0, "not before the desktop's 500ms");
        wait(&mut window, 20);
        assert!(made.get() > 0, "shown after 500ms");
        // It fades and slides in over 150ms, and stays.
        wait(&mut window, motion::TOOLTIP_IN.as_millis() as u64);
        assert!(made.get() > 0);
    }

    #[test]
    fn a_tooltip_comes_in_from_8px_above_and_transparent() {
        assert_eq!(motion::enter_frame(0.0), (0.0, motion::SLIDE_IN));
        assert_eq!(motion::enter_frame(1.0), (1.0, 0.0));
        let (opacity, offset) = motion::enter_frame(0.5);
        assert!(opacity > 0.5 && offset < motion::SLIDE_IN / 2.0);
    }
}
