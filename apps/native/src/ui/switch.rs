//! shadcn's `Switch`: a 32 by 18.4 track inside a transparent 1px border, with a 16px knob, on the right and dark when
//! on. The knob slides across when the switch flips, as shadcn's `transition-transform` has it; it jumped before.

use std::time::Duration;

use gpui::{
    Animation, AnimationExt, Div, ElementId, InteractiveElement, ParentElement, Role, Stateful, Styled, Toggled, div,
    prelude::*, px, rgb, rgba,
};

use crate::{theme, ui::motion};

/// Tailwind's `transition` time, which shadcn's switch takes.
const FLIP: Duration = Duration::from_millis(150);
/// How far the knob moves: the 30 inside the track's border, less the knob's 16.
const TRAVEL: f32 = 14.0;

pub fn switch(id: impl Into<ElementId>, on: bool) -> Stateful<Div> {
    let id = id.into();
    let flipped = motion::flips(&id, on);
    let to = if on { TRAVEL } else { 0.0 };
    let knob = div()
        .relative()
        .children(super::spot(|| format!("switch-knob:{id}")))
        .size(px(16.0))
        .rounded_full()
        .bg(rgb(if on { 0x17151a } else { theme::palette().foreground }));
    // A new animation each flip, from where the knob was; GPUI draws its end at once under Reduce motion.
    let knob = match flipped {
        0 => knob.ml(px(to)).into_any_element(),
        flipped => {
            let ease = motion::cubic_bezier(motion::EASE);
            let from = TRAVEL - to;
            knob.with_animation(("switch-knob", flipped), Animation::new(FLIP).with_easing(ease), move |knob, t| {
                knob.ml(px(from + (to - from) * t))
            })
            .into_any_element()
        }
    };
    div()
        .id(id)
        .role(Role::Switch)
        .aria_toggled(if on { Toggled::True } else { Toggled::False })
        .w(px(32.0))
        .h(px(18.4))
        .flex_none()
        .flex()
        .items_center()
        .rounded_full()
        .border_1()
        .border_color(rgba(0))
        .cursor_pointer()
        // Off, `dark:bg-input/80`.
        .bg(rgba(if on { theme::palette().foreground << 8 | 0xff } else { 0xffffff14 }))
        .child(knob)
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use gpui::{Context, Render, TestApp, Window};

    use super::*;

    struct Probe {
        on: bool,
    }

    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().p(px(20.0)).child(switch("probe", self.on))
        }
    }

    #[test]
    fn the_knob_slides_across_when_the_switch_flips_and_not_when_it_first_shows() {
        let mut app = TestApp::new();
        let mut window = app.open_window(|_, _| Probe { on: true });
        window.draw();
        let knob = |window: &mut gpui::TestAppWindow<Probe>| {
            window.draw();
            crate::ui::spot_at("switch-knob:probe").expect("the knob").left()
        };
        let right = knob(&mut window);
        app.advance_clock(FLIP / 2);
        assert_eq!(knob(&mut window), right, "shown on, it is already at the right");
        window.update(|probe, _, cx| {
            probe.on = false;
            cx.notify();
        });
        let start = knob(&mut window);
        app.advance_clock(FLIP / 2);
        app.run_until_parked();
        let halfway = knob(&mut window);
        app.advance_clock(FLIP);
        app.run_until_parked();
        let left = knob(&mut window);
        assert_eq!(right - left, px(TRAVEL), "it ends at the left");
        assert!(start > left && halfway > left && halfway < right, "on its way: {start:?}, {halfway:?}, from {right:?} to {left:?}");
    }
}
