//! shadcn's `Tooltip` with a `Kbd`, as the desktop's `IconButton` shows one: the button's name and its shortcut in a
//! light pill, half a second after the pointer stops on it (GPUI's delay, the desktop's `TooltipProvider delay={500}`).
//! It fades, zooms and slides in as the desktop's does. GPUI drops a tooltip the moment the pointer leaves
//! (`clear_active_tooltip` in gpui's elements/div.rs, with no way to keep it), so the window draws a copy of the last
//! one where it was, fading and shrinking out as the desktop's `data-closed:animate-out` (`tooltip_exit`).

use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, AnyView, App, AvailableSpace, Bounds, Context, Div, Element, ElementId, FontWeight,
    GlobalElementId, InspectorElementId, IntoElement, LayoutId, Pixels, Point, Position, Render,
    SharedString, Style, Window, WindowId, canvas, div, prelude::*, px, rgb, rgba,
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
        let (label, shortcut) = (self.label.clone(), self.shortcut.clone());
        // tw-animate-css's default 150ms, as the desktop's tooltip sets no duration.
        tip(
            motion::TOOLTIP_IN,
            Rc::new(move || pill(label.clone(), shortcut.clone())),
        )
    }
}

fn pill(label: SharedString, shortcut: Option<SharedString>) -> Div {
    div()
        .max_w(px(320.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .px(px(12.0))
        .py(px(6.0))
        .when(shortcut.is_some(), |pill| pill.pr(px(6.0)))
        .rounded(px(6.0))
        .bg(rgb(theme::FOREGROUND))
        .text_size(px(12.0))
        .text_color(rgb(theme::BACKGROUND))
        .child(label)
        .when_some(shortcut, |pill, keys| {
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
}

/// What a tooltip's view draws: the card `card` makes, coming in over `duration`, and a note of where it is for the
/// window to fade it out from when GPUI drops it. Below the pointer, it comes down into place, as the desktop's
/// `slide-in-from-top-2` on a tooltip below its trigger.
pub fn tip(duration: Duration, card: Rc<dyn Fn() -> Div>) -> impl IntoElement {
    let shown = card.clone();
    // GPUI puts a tooltip's corner at the pointer, so it starts below the cursor's arrow.
    div()
        .pt(px(20.0))
        .child(motion::popup(
            "tooltip",
            motion::From::Corner,
            duration,
            card(),
        ))
        .child(
            canvas(
                move |bounds, window, _| shown_at(window, bounds.origin, duration, shown),
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
}

/// The last tooltip a window showed, until its exit is over.
#[derive(Default)]
struct Exit {
    /// Counts the window's frames, from `tooltip_exit`'s prepaint, which comes before the tooltip's in a frame.
    frame: u64,
    /// The frame the tooltip was last drawn in.
    seen: u64,
    shown: Option<Shown>,
    /// When GPUI stopped drawing it.
    hidden_at: Option<Instant>,
}

struct Shown {
    origin: Point<Pixels>,
    duration: Duration,
    card: Rc<dyn Fn() -> Div>,
}

thread_local! {
    static EXITS: RefCell<HashMap<WindowId, Exit>> = RefCell::new(HashMap::new());
}

fn shown_at(window: &Window, origin: Point<Pixels>, duration: Duration, card: Rc<dyn Fn() -> Div>) {
    EXITS.with_borrow_mut(|exits| {
        let exit = exits.entry(window.window_handle().window_id()).or_default();
        exit.seen = exit.frame;
        exit.shown = Some(Shown {
            origin,
            duration,
            card,
        });
        exit.hidden_at = None;
    });
}

/// Draws the window's last tooltip fading and shrinking out where it was, from the frame GPUI drops it, as the
/// desktop's `animate-out fade-out-0 zoom-out-95`. Put it in a window's root, `deferred` over everything else.
pub fn tooltip_exit() -> TooltipExit {
    TooltipExit
}

pub struct TooltipExit;

impl IntoElement for TooltipExit {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TooltipExit {
    type RequestLayoutState = ();
    type PrepaintState = Option<AnyElement>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let style = Style {
            position: Position::Absolute,
            ..Style::default()
        };
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let id = window.window_handle().window_id();
        let now = cx.background_executor().now();
        let (origin, card, t) = EXITS.with_borrow_mut(|exits| {
            let exit = exits.entry(id).or_default();
            exit.frame += 1;
            let shown = exit.shown.as_ref()?;
            // Built as it would be if the tooltip has just gone; paint knows whether it has.
            let t = exit.hidden_at.map_or(0.0, |at| {
                now.saturating_duration_since(at).as_secs_f32() / shown.duration.as_secs_f32()
            });
            if t >= 1.0 {
                exit.shown = None;
                exit.hidden_at = None;
                return None;
            }
            Some((shown.origin, shown.card.clone(), t))
        })?;
        let frame = motion::exit_frame(t, cx.reduce_motion());
        let mut element = div()
            .pt(px(20.0))
            .child(div().opacity(frame.opacity).child(motion::surface_scaled(
                card(),
                (0.0, 0.0),
                frame.scale,
            )))
            .into_any_element();
        element.prepaint_as_root(origin, AvailableSpace::min_size(), window, cx);
        Some(element)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        element: &mut Option<AnyElement>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(element) = element else {
            return;
        };
        let id = window.window_handle().window_id();
        let now = cx.background_executor().now();
        let gone = EXITS.with_borrow_mut(|exits| {
            exits.get_mut(&id).is_some_and(|exit| {
                let gone = exit.seen != exit.frame;
                if gone {
                    exit.hidden_at.get_or_insert(now);
                }
                gone
            })
        });
        if gone {
            element.paint(window, cx);
            window.request_animation_frame();
        }
    }
}

/// How far the window's last tooltip is through its exit, while it is going: for tests.
#[cfg(all(test, feature = "test-support"))]
pub(crate) fn exit_progress(window: &Window, now: Instant) -> Option<f32> {
    EXITS.with_borrow(|exits| {
        let exit = exits.get(&window.window_handle().window_id())?;
        let shown = exit.shown.as_ref()?;
        let at = exit.hidden_at?;
        Some(now.saturating_duration_since(at).as_secs_f32() / shown.duration.as_secs_f32())
    })
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
                .child(gpui::deferred(tooltip_exit()))
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

    /// GPUI drops the tooltip as the pointer leaves; the window goes on drawing it, fading and shrinking, for the
    /// desktop's 150ms, then lets it go.
    #[test]
    fn a_tooltip_fades_out_over_150ms_after_the_pointer_leaves() {
        let mut app = TestApp::new();
        let mut window = app.open_window(|_, _| Trigger(Rc::new(Cell::new(0))));
        window.draw();
        window.simulate_mouse_move(point(px(10.0), px(10.0)));
        let wait = |window: &mut gpui::TestAppWindow<Trigger>, ms: u64| {
            app.advance_clock(Duration::from_millis(ms));
            app.run_until_parked();
            window.draw();
        };
        let progress = |window: &mut gpui::TestAppWindow<Trigger>| {
            window.update(|_, window, cx| exit_progress(window, cx.background_executor().now()))
        };
        wait(&mut window, 700);
        assert_eq!(progress(&mut window), None, "showing, not going");
        window.simulate_mouse_move(point(px(300.0), px(300.0)));
        wait(&mut window, 0);
        assert_eq!(
            progress(&mut window),
            Some(0.0),
            "going from the frame it was dropped"
        );
        wait(&mut window, 100);
        let going = progress(&mut window).expect("still drawn 100ms on");
        assert!(going > 0.6 && going < 1.0, "{going}");
        wait(&mut window, 60);
        assert_eq!(progress(&mut window), None, "gone after 150ms");
    }
}
