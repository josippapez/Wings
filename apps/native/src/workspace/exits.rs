//! Layers and popovers that close animate out, as the desktop's do (`data-closed:animate-out`): what a closed layer
//! was drawn from is kept, and the layer is drawn from it on its way out, out of the pointer's reach, until its exit is
//! over. Opening it again cuts the exit short.

use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::HashMap,
    time::Duration,
};

use gpui::{AnyElement, Context, SharedString, Task, Window};

use crate::ui::motion::{self, Phase};

use super::Workspace;

#[derive(Default)]
pub(super) struct Exits {
    count: u64,
    /// Closed layers on their way out, by name: the state each closed with, while it isn't being drawn from.
    layers: HashMap<&'static str, Exit>,
    pub(super) popovers: Popovers,
}

struct Exit {
    state: Option<Box<dyn Any>>,
    number: u64,
    _timer: Task<()>,
}

struct Popover {
    seen: u64,
    exit: Option<(u64, Task<()>)>,
}

/// A view's popovers, which are drawn from what they show rather than from state of their own, by name.
#[derive(Default)]
pub(super) struct Popovers {
    /// Counts the view's renders, so a popover knows whether it was drawn in the last one.
    render: u64,
    count: Cell<u64>,
    /// The render each was last drawn open in, and its exit once it closed.
    shown: RefCell<HashMap<SharedString, Popover>>,
}

impl Popovers {
    /// A new render of the view begins.
    pub(super) fn render(&mut self) {
        self.render += 1;
    }

    /// Whether the popover `key` is drawn, and in which phase: while `open`, and for its exit after it closes. One whose
    /// parent wasn't drawn as it closed just goes. Inside a layer on its way out, it goes out with the layer. `slot`
    /// finds these popovers in the view, to end the exit.
    pub(super) fn phase<V: 'static>(
        &self,
        key: impl Into<SharedString>,
        open: bool,
        slot: fn(&mut V) -> &mut Popovers,
        cx: &mut Context<V>,
    ) -> Option<Phase> {
        if let Phase::Out(exit) = motion::phase() {
            return open.then_some(Phase::Out(exit));
        }
        let key = key.into();
        let mut shown = self.shown.borrow_mut();
        if open {
            shown.insert(
                key,
                Popover {
                    seen: self.render,
                    exit: None,
                },
            );
            return Some(Phase::In);
        }
        let popover = shown.get_mut(&key)?;
        if let Some((number, _)) = &popover.exit {
            return Some(Phase::Out(*number));
        }
        // Closed while its parent wasn't on screen: there is nothing to see go.
        if popover.seen + 1 < self.render {
            shown.remove(&key);
            return None;
        }
        self.count.set(self.count.get() + 1);
        let number = self.count.get();
        let timer = cx.spawn({
            let key = key.clone();
            async move |this, cx| {
                cx.background_executor().timer(motion::POPUP_IN).await;
                let _ = this.update(cx, |this, cx| {
                    let mut shown = slot(this).shown.borrow_mut();
                    if shown
                        .get(&key)
                        .is_some_and(|popover| matches!(popover.exit, Some((n, _)) if n == number))
                    {
                        shown.remove(&key);
                        drop(shown);
                        cx.notify();
                    }
                });
            }
        });
        popover.exit = Some((number, timer));
        Some(Phase::Out(number))
    }
}

impl Workspace {
    /// The popover `key`'s phase, if it is drawn (`Popovers::phase`).
    pub(super) fn popover_phase(
        &self,
        key: impl Into<SharedString>,
        open: bool,
        cx: &mut Context<Self>,
    ) -> Option<Phase> {
        self.exits
            .popovers
            .phase(key, open, |this: &mut Self| &mut this.exits.popovers, cx)
    }
}

impl Workspace {
    /// The layer `key` closed with `state`: it goes on being drawn, on its way out, for `duration`.
    pub(super) fn leave<T: 'static>(
        &mut self,
        key: &'static str,
        state: T,
        duration: Duration,
        cx: &mut Context<Self>,
    ) {
        self.exits.count += 1;
        let number = self.exits.count;
        let timer = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(duration).await;
            let _ = this.update(cx, |this, cx| {
                if this
                    .exits
                    .layers
                    .get(key)
                    .is_some_and(|exit| exit.number == number)
                {
                    this.exits.layers.remove(key);
                    cx.notify();
                }
            });
        });
        self.exits.layers.insert(
            key,
            Exit {
                state: Some(Box::new(state)),
                number,
                _timer: timer,
            },
        );
        cx.notify();
    }

    /// The layer `key` drawn on its way out, while it is: `put` puts the state it closed with back where `layer` draws
    /// it from for the one call, and `take` takes it out again. Nothing while it is `open` again.
    pub(super) fn leaving<T: 'static>(
        &mut self,
        key: &'static str,
        open: bool,
        put: impl FnOnce(&mut Self, T),
        take: impl FnOnce(&mut Self) -> Option<T>,
        layer: impl FnOnce(&Self, &mut Context<Self>) -> Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> Option<(AnyElement, Phase)> {
        if open {
            self.exits.layers.remove(key);
            return None;
        }
        let exit = self.exits.layers.get_mut(key)?;
        let phase = Phase::Out(exit.number);
        let state = *exit.state.take()?.downcast::<T>().ok()?;
        put(self, state);
        let element = motion::with_phase(phase, || layer(self, cx));
        if let (Some(state), Some(exit)) = (take(self), self.exits.layers.get_mut(key)) {
            exit.state = Some(Box::new(state));
        }
        element.map(|element| (element, phase))
    }
}

impl Workspace {
    /// A layer drawn from `slot`, open or on its way out, with its phase.
    fn layer<T: 'static>(
        &mut self,
        key: &'static str,
        slot: impl Fn(&mut Self) -> &mut Option<T> + Copy,
        draw: impl Fn(&Self, &mut Context<Self>) -> Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> Option<(AnyElement, Phase)> {
        if slot(self).is_some() {
            self.exits.layers.remove(key);
            return draw(self, cx).map(|layer| (layer, Phase::In));
        }
        self.leaving(
            key,
            false,
            |this, state| *slot(this) = Some(state),
            |this| slot(this).take(),
            draw,
            cx,
        )
    }

    /// A dialog shown one of a queue at a time, open while the queue has one, and on its way out after the last.
    fn queued<T: 'static>(
        &mut self,
        key: &'static str,
        queue: impl Fn(&mut Self) -> &mut Vec<T> + Copy,
        draw: impl Fn(&Self, &mut Context<Self>) -> Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> Option<(AnyElement, Phase)> {
        if !queue(self).is_empty() {
            self.exits.layers.remove(key);
            return draw(self, cx).map(|layer| (layer, Phase::In));
        }
        self.leaving(
            key,
            false,
            |this, state| queue(this).push(state),
            |this| queue(this).pop(),
            draw,
            cx,
        )
    }

    /// The layers over the window, in the order they stack, each coming in or on its way out: those under the plugins'
    /// notices, and those over them.
    pub(super) fn layers(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> (Vec<AnyElement>, Vec<AnyElement>) {
        let dialog = |name, layer: Option<(AnyElement, Phase)>| {
            layer.map(|(layer, phase)| motion::dialog_layer(name, phase, layer))
        };
        let settings = self.layer(
            "settings",
            |this| &mut this.settings,
            Self::settings_dialog,
            cx,
        );
        let review = self.queued(
            "review",
            |this| &mut this.manage.reviews,
            Self::plugin_review_dialog,
            cx,
        );
        let setup = self.queued(
            "setup",
            |this| &mut this.setup_asks.queue,
            Self::setup_approval_dialog,
            cx,
        );
        let sheet = self.layer("sheet", |this| &mut this.sheet, Self::sheet_layer, cx);
        let palette = self.layer(
            "palette",
            |this| &mut this.palette,
            |this, cx| this.palette_layer(window, cx),
            cx,
        );
        let remove = self.layer(
            "remove",
            |this| &mut this.side.removing,
            Self::remove_dialog,
            cx,
        );
        let inbox = self.layer("inbox", |this| &mut this.inbox.open, Self::inbox_layer, cx);
        let menu = self.layer("menu", |this| &mut this.menu, Self::menu_layer, cx);
        let update = self.update_layer(cx);
        let under = [
            dialog("settings", settings),
            dialog("plugin-review", review),
            dialog("setup-approval", setup),
            sheet.map(|(sheet, phase)| motion::sheet_layer(phase, sheet)),
            dialog("palette", palette),
            dialog("remove", remove),
            inbox.map(|(inbox, phase)| {
                motion::popup_layer("inbox", motion::From::Left, phase, inbox)
            }),
        ];
        let over = [menu.map(|(menu, _)| menu), dialog("update", update)];
        (
            under.into_iter().flatten().collect(),
            over.into_iter().flatten().collect(),
        )
    }
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use std::time::Duration;

    use gpui::{TestApp, TestAppWindow, point, px};

    use super::super::{
        Active, Workspace, context_menu::MenuTarget, tests::open_config, tests::press,
    };
    use crate::ui::motion;

    fn open() -> (TestApp, TestAppWindow<Workspace>) {
        open_config(|_| {})
    }

    /// The layers drawn now: those under the notices, and those over them.
    fn drawn(window: &mut TestAppWindow<Workspace>) -> (usize, usize) {
        window.update(|w, window, cx| {
            let (under, over) = w.layers(window, cx);
            (under.len(), over.len())
        })
    }

    fn wait(app: &TestApp, window: &mut TestAppWindow<Workspace>, duration: Duration) {
        app.advance_clock(duration);
        app.run_until_parked();
        window.draw();
    }

    /// Closed, Settings goes on being drawn on its way out for the desktop's 100ms, then it's gone; its state isn't
    /// back, so nothing else takes it for open.
    #[test]
    fn a_closed_dialog_is_drawn_on_its_way_out_then_removed() {
        let (app, mut window) = open();
        press(&mut window, ",");
        window.draw();
        assert_eq!(drawn(&mut window), (1, 0));
        window.update(|w, window, cx| w.close_settings(window, cx));
        window.draw();
        assert!(window.read(|w, _| w.settings.is_none()));
        assert_eq!(drawn(&mut window), (1, 0), "on its way out");
        wait(&app, &mut window, motion::POPUP_IN / 2);
        assert_eq!(drawn(&mut window), (1, 0), "still going halfway");
        wait(&app, &mut window, motion::POPUP_IN);
        assert_eq!(drawn(&mut window), (0, 0), "gone once its exit is over");
        assert!(window.read(|w, _| w.settings.is_none()));
    }

    /// A click lands on what is under a dialog on its way out, as though it had gone: here the dialog's own backdrop,
    /// which would close it, doesn't take a click meant for the window.
    #[test]
    fn a_dialog_on_its_way_out_lets_clicks_through() {
        let (_app, mut window) = open();
        press(&mut window, ",");
        window.draw();
        window.update(|w, window, cx| w.close_settings(window, cx));
        window.draw();
        // The sidebar's Search row, under the dialog's backdrop.
        window.simulate_click(point(px(60.0), px(54.0)), gpui::MouseButton::Left);
        window.draw();
        assert!(
            window.read(|w, _| w.palette.is_some()),
            "the click reached the row"
        );
    }

    /// Opened again on its way out, it's open at once, with no copy going out beside it.
    #[test]
    fn opening_a_dialog_again_cuts_its_exit_short() {
        let (_app, mut window) = open();
        press(&mut window, ",");
        window.update(|w, window, cx| w.close_settings(window, cx));
        window.draw();
        press(&mut window, ",");
        window.draw();
        assert!(window.read(|w, _| w.settings.is_some()));
        assert_eq!(drawn(&mut window), (1, 0));
        assert!(window.read(|w, _| !w.exits.layers.contains_key("settings")));
    }

    #[test]
    fn a_closed_menu_and_sheet_go_out_over_their_own_time() {
        let (app, mut window) = open();
        window.update(|w, _, cx| {
            w.open_menu(
                MenuTarget::Tab(Active::Issues),
                point(px(100.0), px(100.0)),
                cx,
            )
        });
        window.draw();
        assert_eq!(drawn(&mut window), (0, 1));
        window.update(|w, _, cx| w.close_menu(cx));
        window.draw();
        assert_eq!(drawn(&mut window), (0, 1), "the menu on its way out");
        wait(
            &app,
            &mut window,
            motion::POPUP_IN + Duration::from_millis(1),
        );
        assert_eq!(drawn(&mut window), (0, 0));

        window.update(|w, window, cx| w.open_history(window, cx));
        window.draw();
        window.update(|w, window, cx| w.close_history(window, cx));
        window.draw();
        // The sheet takes the desktop's 200ms to go, longer than a menu.
        wait(&app, &mut window, Duration::from_millis(150));
        assert_eq!(drawn(&mut window), (1, 0), "the sheet still going");
        wait(&app, &mut window, Duration::from_millis(60));
        assert_eq!(drawn(&mut window), (0, 0));
    }

    /// A popover closed goes out over 100ms; one whose parent wasn't drawn as it closed just goes.
    #[test]
    fn a_closed_popover_goes_out_then_is_forgotten() {
        let (app, mut window) = open();
        let phase = |window: &mut TestAppWindow<Workspace>, open: bool| {
            window.update(|w, _, cx| {
                w.exits.popovers.render();
                w.popover_phase("test", open, cx)
            })
        };
        assert_eq!(phase(&mut window, true), Some(motion::Phase::In));
        let Some(motion::Phase::Out(exit)) = phase(&mut window, false) else {
            panic!("on its way out");
        };
        assert_eq!(phase(&mut window, false), Some(motion::Phase::Out(exit)));
        wait(
            &app,
            &mut window,
            motion::POPUP_IN + Duration::from_millis(1),
        );
        assert_eq!(phase(&mut window, false), None);

        assert_eq!(phase(&mut window, true), Some(motion::Phase::In));
        window.update(|w, _, _| w.exits.popovers.render());
        assert_eq!(phase(&mut window, false), None, "not drawn as it closed");
    }
}
