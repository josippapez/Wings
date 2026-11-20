//! What turns a window's spinners outside its drawing (`native_spinners::Host`): from the window's draws it learns which
//! spinners show where, and it turns them itself, so a step of theirs draws no frame of the window. A window without
//! one (offscreen: tests, `pnpm shot`, the benches) turns them in its layers (`status_dot::rings`).
//!
//! The rings it turns are over everything GPUI draws, so where an overlay of the draw lies over one (`slots::cover`), the
//! window turns that one in its layer instead, under the overlay, until the overlay is gone.

use std::{cell::RefCell, collections::HashMap};

use gpui::{App, Bounds, IntoElement, Pixels, SharedString, Styled, Window, WindowId, canvas};

use super::slots::Piece;

/// A spinner by the piece that left its slot, the layer of slots it is in and its slot's name there: two pages can name
/// a slot alike.
pub(crate) type Key = (Piece, &'static str, SharedString);

/// Where a spinner is and how it looks: a ring in `bounds`, the window's points from its top left corner, cut to
/// `clip`, in the theme's working colour `color`, for a window of `scale` device pixels a point; standing still under
/// Reduce motion.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Placed {
    pub bounds: Bounds<Pixels>,
    pub clip: Bounds<Pixels>,
    pub color: u32,
    pub scale: f32,
    pub still: bool,
}

/// What turns a window's spinners outside its drawing, or a stand-in in tests.
pub(crate) trait Present {
    /// Puts the spinner `key` as `placed`, adding it if new. A new one starts `phase` of a turn round, so it turns with
    /// the others; one already there keeps turning as it was.
    fn place(&mut self, key: &Key, placed: &Placed, phase: f64);
    /// Takes the spinner `key` away.
    fn remove(&mut self, key: &Key);
}

struct Host {
    present: Box<dyn Present>,
    /// What it shows now.
    placed: HashMap<Key, Placed>,
    /// What the window's draw going on has handed it so far (`hand`).
    handed: HashMap<Key, Placed>,
}

thread_local! {
    /// Each window's host, for the windows that have one.
    static HOSTS: RefCell<HashMap<WindowId, Host>> = RefCell::default();
}

/// Has `present` turn the spinners of the window `id` from its next draw.
pub(crate) fn present_with(id: WindowId, present: Box<dyn Present>) {
    HOSTS.with_borrow_mut(|hosts| {
        hosts.insert(id, Host { present, placed: HashMap::new(), handed: HashMap::new() })
    });
}

/// Whether something outside the window `id`'s drawing turns its spinners.
pub(super) fn hosted(id: WindowId) -> bool {
    HOSTS.with_borrow(|hosts| hosts.contains_key(&id))
}

/// Says that the window `id`'s draw going on shows the spinner `key` as `placed`, for its host to turn.
pub(super) fn hand(id: WindowId, key: Key, placed: Placed) {
    HOSTS.with_borrow_mut(|hosts| {
        if let Some(host) = hosts.get_mut(&id) {
            host.handed.insert(key, placed);
        }
    });
}

/// Hands the window's host the spinners its draw showed (`hand`): those new are added, those moved are moved, those no
/// longer shown taken away, and those unchanged left turning. Last in the window's root, in each draw of it, after the
/// spinner layers: it works as the draw paints, when every overlay has been laid out and placed (`slots::cover`). A draw
/// that shows no spinner layer, a full-screen diff say, takes them all away.
pub fn spinner_sync(window: &Window) -> impl IntoElement {
    let id = window.window_handle().window_id();
    canvas(|_, _, _| {}, move |_, _, _, cx| sync(id, cx))
        .absolute()
        .top_0()
        .left_0()
        .size_full()
}

fn sync(id: WindowId, cx: &App) {
    // Where a turn of the spinners' clock is now, so a new ring turns with the rings the window paints.
    let phase = super::status_dot::clock(cx).as_secs_f64().fract();
    HOSTS.with_borrow_mut(|hosts| {
        let Some(host) = hosts.get_mut(&id) else { return };
        let handed = std::mem::take(&mut host.handed);
        let gone: Vec<Key> = host.placed.keys().filter(|key| !handed.contains_key(*key)).cloned().collect();
        for key in gone {
            host.present.remove(&key);
            host.placed.remove(&key);
        }
        for (key, placed) in handed {
            if host.placed.get(&key) != Some(&placed) {
                host.present.place(&key, &placed, phase);
                host.placed.insert(key, placed);
            }
        }
    });
}

/// Turns the window's spinners in Core Animation over its drawing from now on, so a step of one never draws the window.
/// Where the layer can't be set up the window turns them itself, as an offscreen window does.
#[cfg(target_os = "macos")]
pub fn turn_spinners_natively(window: &mut Window) {
    match super::native_spinners::Host::new(window) {
        Ok(host) => {
            present_with(window.window_handle().window_id(), Box::new(host));
            window.refresh();
        }
        Err(why) => eprintln!("Spinners are drawn in the window: {why}"),
    }
}
