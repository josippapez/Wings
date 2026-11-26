//! Which web views the pages hold, and where each shows in the window's last draw.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use gpui::{App, Bounds, EntityId, IntoElement, Pixels, SharedString, Styled, Window, WindowId, canvas};
use serde::Serialize;

use super::{LAYER, Profile};
use crate::{
    page::tree::{WebCommand, WebGo},
    ui::slots::{self, Piece},
};

/// What a page says of one of its web views, from its `WebView` node.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub name: SharedString,
    pub url: Option<String>,
    pub keep_data: bool,
    pub command: Option<WebCommand>,
}

/// Where a web view is, which its page hears with `onState`.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub url: String,
    pub title: String,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    /// Why the last page didn't load, until another starts.
    pub error: Option<String>,
}

/// What a web view tells its page: where it is, or what a command came to.
#[derive(Clone, Debug, PartialEq)]
pub enum Heard {
    State(State),
    /// What the `eval` or `snapshot` command of `token` came to: its value as JSON, or why it failed.
    Result { token: u64, result: Result<serde_json::Value, String> },
}

/// A web view's page hears what it does through this: the view's name and what it heard.
pub type Told = async_channel::Sender<(SharedString, Heard)>;

/// The engine's view of one web view, or a stand-in in tests.
pub trait Native {
    /// Shows it at `frame`, the window's points from its top left, in `window`, or hides it for `None`.
    fn place(&mut self, window: &Window, frame: Option<Bounds<Pixels>>);
    /// Does `go`, the command of `token`, which `eval` and `snapshot` answer by.
    fn go(&mut self, go: &WebGo, token: u64);
}

/// Makes the engine's views of a window (`host_natively`). A window without one (offscreen: tests, `pnpm shot`) draws
/// only the places.
pub trait Maker {
    fn make(&self, profile: &Profile, keep_data: bool, url: Option<&str>, told: Rc<dyn Fn(Heard)>) -> Box<dyn Native>;
}

struct Entry {
    profile: Profile,
    keep_data: bool,
    url: Option<String>,
    told: Told,
    native: Option<Box<dyn Native>>,
    /// What the page asked before the engine's view was made, done once it is, with each command's token.
    waiting: Vec<(WebGo, u64)>,
    /// The token of the last command done, so each runs once.
    token: Option<u64>,
    /// Where it shows now, and in which window.
    shown: Option<Bounds<Pixels>>,
    window: Option<WindowId>,
}

thread_local! {
    static VIEWS: RefCell<HashMap<(EntityId, SharedString), Entry>> = RefCell::default();
    static MAKERS: RefCell<HashMap<WindowId, Rc<dyn Maker>>> = RefCell::default();
}

/// Has `maker` make the engine's views of the pages shown in the window `id`.
pub(crate) fn make_with(id: WindowId, maker: Rc<dyn Maker>) {
    MAKERS.with_borrow_mut(|makers| makers.insert(id, maker));
}

/// The page `page` of `profile`'s plugin holds these web views now, from its newest tree: new ones are added, gone ones
/// end, and each command the page sends for the first time is done.
pub fn update(page: EntityId, profile: &Profile, specs: &[Spec], told: &Told) {
    // Dropped after the borrow, as a view that ends may call back.
    let mut ended = Vec::new();
    VIEWS.with_borrow_mut(|views| {
        let keep: Vec<_> = views
            .keys()
            .filter(|(of, name)| *of != page || specs.iter().any(|spec| spec.name == *name))
            .cloned()
            .collect();
        views.retain(|key, entry| {
            let kept = keep.contains(key);
            if !kept {
                ended.extend(entry.native.take());
            }
            kept
        });
        for spec in specs {
            let entry = views.entry((page, spec.name.clone())).or_insert_with(|| Entry {
                profile: profile.clone(),
                keep_data: spec.keep_data,
                url: spec.url.clone(),
                told: told.clone(),
                native: None,
                waiting: Vec::new(),
                // A command the page sent before the view was made runs once it is.
                token: None,
                shown: None,
                window: None,
            });
            if let Some(command) = &spec.command
                && entry.token != Some(command.token)
            {
                entry.token = Some(command.token);
                match &mut entry.native {
                    Some(native) => native.go(&command.go, command.token),
                    None => entry.waiting.push((command.go.clone(), command.token)),
                }
            }
        }
    });
    drop(ended);
}

/// The page `page` is gone: its web views end.
pub fn forget_page(page: EntityId) {
    let ended: Vec<Entry> = VIEWS.with_borrow_mut(|views| {
        let gone: Vec<_> = views.keys().filter(|(of, _)| *of == page).cloned().collect();
        gone.into_iter().filter_map(|key| views.remove(&key)).collect()
    });
    drop(ended);
}

fn slot_name(page: EntityId, name: &str) -> SharedString {
    format!("{}/{name}", page.as_u64()).into()
}

/// The place of the page `page`'s web view `name`, filling what holds it. It is left for the window's draw only while the
/// page is drawn as a view a piece puts on screen with nothing of its own over it.
pub fn slot(page: EntityId, name: SharedString) -> impl IntoElement {
    canvas(
        move |bounds, window, _| {
            if let Some(piece @ Piece::View(_)) = slots::uncovered_piece() {
                slots::leave_slot(window, piece, LAYER, slot_name(page, &name), bounds);
            }
        },
        |_, _, _, _| {},
    )
    .size_full()
}

/// Where each web view goes in the window's draw that just ended: at its place while that shows and no overlay that takes
/// clicks lies over it, else hidden. Last in the window's root, once every overlay has its place (`ui::cover`).
pub fn sync(window: &Window) -> impl IntoElement {
    let _ = window;
    canvas(|_, _, _| {}, move |_, _, window, cx| place(window, cx))
        .absolute()
        .top_0()
        .left_0()
        .size_full()
}

/// `sync`'s work, in a draw of `window`.
pub fn place(window: &Window, _: &App) {
    let id = window.window_handle().window_id();
    let maker = MAKERS.with_borrow(|makers| makers.get(&id).cloned());
    let places = slots::shown(id, LAYER);
    let covers = slots::blocking_covers(id);
    VIEWS.with_borrow_mut(|views| {
        for ((page, name), entry) in views.iter_mut() {
            if entry.window.is_some_and(|window| window != id) {
                continue;
            }
            let at = places
                .iter()
                .find(|(slot, _)| *slot == slot_name(*page, name))
                .map(|(_, slot)| slot.bounds.intersect(&slot.mask.bounds))
                .filter(|at| !at.is_empty() && !covers.iter().any(|cover| cover.intersects(at)));
            if entry.native.is_none() {
                let (Some(maker), Some(_)) = (&maker, at) else { continue };
                let told = entry.told.clone();
                let key = name.clone();
                let tell: Rc<dyn Fn(Heard)> = Rc::new(move |heard| {
                    let _ = told.try_send((key.clone(), heard));
                });
                let mut native = maker.make(&entry.profile, entry.keep_data, entry.url.as_deref(), tell);
                for (go, token) in entry.waiting.drain(..) {
                    native.go(&go, token);
                }
                entry.native = Some(native);
                entry.window = Some(id);
            }
            if entry.shown != at
                && let Some(native) = &mut entry.native
            {
                native.place(window, at);
                entry.shown = at;
            }
        }
    });
}

#[cfg(all(test, feature = "test-support"))]
pub(crate) mod testing {
    //! A stand-in for the engine, which records what it was asked.

    use std::cell::RefCell;

    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    pub enum Did {
        Made { name: Option<String>, keep_data: bool, url: Option<String> },
        Placed(Option<Bounds<Pixels>>),
        Went(WebGo, u64),
        Ended,
    }

    #[derive(Default)]
    pub struct Record {
        pub did: Rc<RefCell<Vec<Did>>>,
        /// What each view made tells its page, by the order they were made in.
        pub tells: Rc<RefCell<Vec<Rc<dyn Fn(Heard)>>>>,
    }

    struct Fake(Rc<RefCell<Vec<Did>>>);

    impl Native for Fake {
        fn place(&mut self, _: &Window, frame: Option<Bounds<Pixels>>) {
            self.0.borrow_mut().push(Did::Placed(frame));
        }
        fn go(&mut self, go: &WebGo, token: u64) {
            self.0.borrow_mut().push(Did::Went(go.clone(), token));
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            self.0.borrow_mut().push(Did::Ended);
        }
    }

    impl Maker for Record {
        fn make(&self, profile: &Profile, keep_data: bool, url: Option<&str>, told: Rc<dyn Fn(Heard)>) -> Box<dyn Native> {
            self.did.borrow_mut().push(Did::Made { name: Some(profile.plugin.clone()), keep_data, url: url.map(str::to_owned) });
            self.tells.borrow_mut().push(told);
            Box::new(Fake(self.did.clone()))
        }
    }

    /// Records the web views of the window `id`: what was done to them, and how to tell their pages.
    pub fn record(id: WindowId) -> (Rc<RefCell<Vec<Did>>>, Rc<RefCell<Vec<Rc<dyn Fn(Heard)>>>>) {
        let record = Record::default();
        let did = record.did.clone();
        let tells = record.tells.clone();
        make_with(id, Rc::new(record));
        (did, tells)
    }
}
