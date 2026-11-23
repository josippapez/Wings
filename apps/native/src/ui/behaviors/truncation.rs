//! `cut_text`: text that ends in an ellipsis shows all of itself in a tooltip, and only then (UI SDK's truncation
//! tooltip). GPUI already shapes each frame's text to its width, ellipsis included, so nothing is measured for this:
//! when the pointer rests on the text, the tooltip compares what was drawn with what was meant, and shows nothing
//! when they are the same.

use gpui::{
    Context, ElementId, InteractiveElement, IntoElement, ParentElement, Render, SharedString, Stateful,
    StatefulInteractiveElement, StyledText, TextLayout, Window, div, prelude::*,
};

use crate::ui::tooltip;

/// A line of `text` in a box the caller sizes, ending in an ellipsis where it doesn't fit, with the full text in a
/// tooltip then. `id` names the box among its siblings. Style the box (size, color, `flex_1`) on what comes back.
pub fn cut_text(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Stateful<gpui::Div> {
    let text: SharedString = text.into();
    let styled = StyledText::new(text.clone());
    let layout = styled.layout().clone();
    let full = tooltip(text.clone(), None);
    div().id(id).min_w_0().truncate().child(styled).tooltip(move |window, cx| {
        if is_cut(&layout, &text) { full(window, cx) } else { cx.new(|_| Nothing).into() }
    })
}

/// Whether the last frame drew `text` shorter than it is.
fn is_cut(layout: &TextLayout, text: &str) -> bool {
    layout.text() != text
}

/// The tooltip of text that fits: nothing to draw.
struct Nothing;

impl Render for Nothing {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use gpui::{TestApp, px};

    use super::*;

    /// A box `width` wide with a long line in it, keeping the line's layout.
    struct Line(f32, Rc<RefCell<Option<TextLayout>>>);

    impl Render for Line {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let styled = StyledText::new("A long name that will not fit");
            *self.1.borrow_mut() = Some(styled.layout().clone());
            div().w(px(self.0)).truncate().child(styled)
        }
    }

    fn drawn_cut(width: f32) -> bool {
        let mut app = TestApp::new();
        let kept = Rc::new(RefCell::new(None));
        let share = kept.clone();
        let mut window = app.open_window(move |_, _| Line(width, share));
        window.draw();
        let layout = kept.borrow().clone().unwrap();
        is_cut(&layout, "A long name that will not fit")
    }

    /// A `cut_text` `width` wide, with the window's tooltip exit beside it, as the app draws.
    struct Hover(f32);

    impl Render for Hover {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(gpui::deferred(crate::ui::tooltip_exit()))
                .child(cut_text("line", "A long name that will not fit").w(px(self.0)).h(px(20.0)))
        }
    }

    /// Whether a tooltip showed after the pointer rested on a `cut_text` of `width` and left.
    fn tooltip_showed(width: f32) -> bool {
        let mut app = TestApp::new();
        let mut window = app.open_window(move |_, _| Hover(width));
        window.draw();
        window.simulate_mouse_move(gpui::point(px(10.0), px(10.0)));
        let wait = |window: &mut gpui::TestAppWindow<Hover>, ms: u64| {
            app.advance_clock(std::time::Duration::from_millis(ms));
            app.run_until_parked();
            window.draw();
        };
        wait(&mut window, 700);
        window.simulate_mouse_move(gpui::point(px(500.0), px(500.0)));
        wait(&mut window, 0);
        window.update(|_, window, cx| {
            crate::ui::tooltip::exit_progress(window, cx.background_executor().now()).is_some()
        })
    }

    #[test]
    fn a_tooltip_shows_only_on_text_that_is_cut() {
        // The wide one first: a window's last tooltip is kept per window id, which a test's windows share.
        assert!(!tooltip_showed(600.0));
        assert!(tooltip_showed(60.0));
    }

    #[test]
    fn text_is_cut_only_when_it_does_not_fit() {
        assert!(drawn_cut(60.0));
        assert!(!drawn_cut(600.0));
    }
}
