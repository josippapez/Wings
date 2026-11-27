//! Which web views the pages hold, and where each shows in the window's last draw.

use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::{Arc, Mutex},
};

use gpui::{App, Bounds, EntityId, IntoElement, Pixels, SharedString, Styled, Window, WindowId, canvas};
use serde::Serialize;

use super::{LAYER, Profile};
use crate::{
    page::tree::{WebCommand, WebGo},
    ui::slots,
};

/// What a page says of one of its web views, from its `WebView` node.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub name: SharedString,
    pub url: Option<String>,
    pub keep_data: bool,
    pub commands: Vec<WebCommand>,
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
    /// The `alert`, `confirm` or `prompt` the page waits on, which a `dialog` command answers.
    pub dialog: Option<Dialog>,
    /// The page is on a site signed in with imported cookies: its plugin shows its address, and tells no agent more than
    /// its origin.
    pub imported: bool,
    /// The buttons of the extensions that are on and have one, for the page's toolbar.
    pub extensions: Vec<Extension>,
}

/// An extension's button for a page (`WKWebExtensionAction`): what its plugin draws in its toolbar, and the `id` its
/// `extension` command clicks.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Extension {
    pub id: String,
    pub name: String,
    /// What the button says on hover, which the extension may set for each page.
    pub label: String,
    /// The id of its icon among the plugin's images.
    pub icon: Option<String>,
    /// A few characters over the icon, a count most often; empty for none.
    pub badge: String,
    pub enabled: bool,
}

/// The most characters of a button's name or label, and of its badge, which an extension sets: a toolbar shows a few.
const LABEL_MOST: usize = 100;
const BADGE_MOST: usize = 8;

/// The buttons a page's plugin gets, in a steady order (by name), each name and label one line cut short. On a page signed
/// in with imported cookies (`imported`) none has a badge, which an extension works out from the page, and the caller
/// reads each extension's default button rather than the page's own: no plugin learns what an extension saw there.
pub fn shown(mut extensions: Vec<Extension>, imported: bool) -> Vec<Extension> {
    let line = |text: &str, most: usize| -> String {
        let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
        line.chars().filter(|c| !c.is_control()).take(most).collect()
    };
    for extension in &mut extensions {
        extension.name = line(&extension.name, LABEL_MOST);
        extension.label = line(&extension.label, LABEL_MOST);
        extension.badge = if imported { String::new() } else { line(&extension.badge, BADGE_MOST) };
    }
    extensions.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id)));
    extensions
}

/// A page's `alert`, `confirm` or `prompt`, and the site that shows it, which its plugin's page shows as its own.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dialog {
    pub kind: String,
    pub message: String,
    pub default_text: Option<String>,
    pub site: String,
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

/// How one web view tells its page, from any thread (a snapshot is encoded off the main one).
#[derive(Clone)]
pub struct Tell {
    told: Told,
    name: SharedString,
    /// The last address it told, which the other engine opens it at when the page switches engines.
    at: Arc<Mutex<Option<String>>>,
}

impl Tell {
    pub fn tell(&self, heard: Heard) {
        if let Heard::State(state) = &heard
            && !state.url.is_empty()
        {
            *self.at.lock().unwrap() = Some(state.url.clone());
        }
        let _ = self.told.try_send((self.name.clone(), heard));
    }
}

/// The engine's view of one web view, or a stand-in in tests.
pub trait Native {
    /// Shows it at `frame`, the window's points from its top left, in `window`, or hides it for `None`.
    fn place(&mut self, window: &Window, frame: Option<Bounds<Pixels>>);
    /// Lets what lies over the page show through it and take the clicks there, and dims it under a dialog's backdrop, which
    /// then takes every click: true when it can, and false for one that is hidden instead while anything does.
    fn overlaid(&mut self, _over: &Overlaid) -> bool {
        false
    }
    /// Does `go`, the command of `token`, which `eval` and `snapshot` answer by.
    fn go(&mut self, go: &WebGo, token: u64);
}

/// Where an overlay lies over a web view, in the view's own points from its top left, and how round each of its corners
/// is: top left, top right, bottom right, bottom left.
#[derive(Clone, Debug, PartialEq)]
pub struct Hole {
    pub at: Bounds<Pixels>,
    pub corners: [f32; 4],
}

/// What lies over a web view: the overlays it shows through, and whether a dialog's backdrop dims all of it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Overlaid {
    pub holes: Vec<Hole>,
    pub dimmed: bool,
}

/// How round an overlay's corners are when it doesn't say, as a menu's (`ui::menu`).
const ROUND: f32 = 8.0;

/// What of a web view at `at` the overlays `covers` (menus, popovers, dialogs, tooltips, each with its corners' radius
/// when it says) lie over, under the dialogs' `backdrops`: `None` when one lies over all of it but a dialog's backdrop,
/// which dims it. An overlay's own corners are round in its hole, so the page doesn't show dark corners around a menu, and
/// a corner the page's edge cuts off is square. Overlays that overlap are cut as rectangles that don't, all square.
fn overlaid(at: Bounds<Pixels>, covers: &[(Bounds<Pixels>, Option<f32>)], backdrops: &[Bounds<Pixels>]) -> Option<Overlaid> {
    let whole = |over: &Bounds<Pixels>, of: &Bounds<Pixels>| over.intersect(of) == *of;
    let dim: Vec<_> = backdrops.iter().filter(|backdrop| whole(backdrop, &at)).collect();
    let mut over: Vec<(Bounds<Pixels>, f32)> = Vec::new();
    for (cover, round) in covers.iter().filter(|(cover, _)| cover.intersects(&at)) {
        // The layer a window's dialog is drawn in, as large as its backdrop.
        if dim.iter().any(|backdrop| whole(cover, backdrop)) {
            continue;
        }
        if whole(cover, &at) {
            return None;
        }
        // A tooltip draws twice as it comes in.
        if !over.iter().any(|(seen, _)| seen == cover) {
            over.push((*cover, round.unwrap_or(ROUND)));
        }
    }
    let dimmed = !dim.is_empty();
    let in_page = |cut: Bounds<Pixels>| Bounds::new(cut.origin - at.origin, cut.size);
    let apart = over.iter().enumerate().all(|(index, (one, _))| over[index + 1..].iter().all(|(other, _)| !one.intersects(other)));
    if !apart {
        let cuts: Vec<_> = over.iter().map(|(cover, _)| in_page(cover.intersect(&at))).collect();
        let holes = disjoint(&cuts).into_iter().map(|at| Hole { at, corners: [0.0; 4] }).collect();
        return Some(Overlaid { holes, dimmed });
    }
    let holes = over
        .iter()
        .map(|(cover, round)| {
            let cut = cover.intersect(&at);
            let round = round.min(f32::from(cut.size.width) / 2.0).min(f32::from(cut.size.height) / 2.0);
            let (top, bottom) = (cut.top() == cover.top(), cut.bottom() == cover.bottom());
            let (left, right) = (cut.left() == cover.left(), cut.right() == cover.right());
            let corner = |own: bool| if own { round } else { 0.0 };
            Hole {
                at: in_page(cut),
                corners: [corner(top && left), corner(top && right), corner(bottom && right), corner(bottom && left)],
            }
        })
        .collect();
    Some(Overlaid { holes, dimmed })
}

/// The area `rects` cover, as rectangles that don't overlap: the cells of the grid their edges make that one of them
/// covers. A page's mask cuts every rectangle it gets, and two that overlapped cut each other back in: a tooltip, which
/// draws twice as it comes in, didn't show over the page.
fn disjoint(rects: &[Bounds<Pixels>]) -> Vec<Bounds<Pixels>> {
    let edges = |of: &dyn Fn(&Bounds<Pixels>) -> [Pixels; 2]| {
        let mut edges: Vec<Pixels> = rects.iter().flat_map(of).collect();
        edges.sort_by(|a, b| f32::from(*a).total_cmp(&f32::from(*b)));
        edges.dedup();
        edges
    };
    let xs = edges(&|rect| [rect.left(), rect.right()]);
    let ys = edges(&|rect| [rect.top(), rect.bottom()]);
    let mut cells = Vec::new();
    for x in xs.windows(2) {
        for y in ys.windows(2) {
            let cell = Bounds::from_corners(gpui::point(x[0], y[0]), gpui::point(x[1], y[1]));
            if rects.iter().any(|rect| rect.contains(&cell.center())) {
                cells.push(cell);
            }
        }
    }
    cells
}

/// Makes the engine's views of a window (`host_natively`). A window without one (offscreen: tests, `pnpm shot`) draws
/// only the places.
pub trait Maker {
    /// `scripts`: the plugin may run scripts in its pages, so the page's console and requests are kept for `logs`.
    fn make(&self, profile: &Profile, keep_data: bool, scripts: bool, url: Option<&str>, told: Tell) -> Box<dyn Native>;
    /// Which engine it makes `profile`'s web views with now (`web::engine::choice`): a view made by another is made
    /// again by this one, at the address it was on.
    fn engine(&self, _profile: &Profile) -> super::engine::Kind {
        super::engine::Kind::WebKit
    }
}

struct Entry {
    profile: Profile,
    keep_data: bool,
    scripts: bool,
    url: Option<String>,
    told: Told,
    native: Option<Box<dyn Native>>,
    /// The engine that made `native`, and the last address it told its page.
    engine: super::engine::Kind,
    at: Arc<Mutex<Option<String>>>,
    /// What the page asked before the engine's view was made, done once it is, with each command's token.
    waiting: Vec<(WebGo, u64)>,
    /// The token of the last command done, so each runs once: a page's tokens only grow.
    token: Option<u64>,
    /// Where it shows now, and in which window.
    shown: Option<Bounds<Pixels>>,
    window: Option<WindowId>,
    /// What lies over it now, and whether that shows through it (`Native::overlaid`).
    over: Overlaid,
    cuts: bool,
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
/// end, and each command the page sends for the first time is done. An `eval` is refused unless the plugin may run
/// scripts in its pages (`scripts`, `permissions.webScripts`), and one that needs your click (`needs_click`) unless
/// `take_click` uses up a click of yours in the page.
pub fn update(page: EntityId, profile: &Profile, specs: &[Spec], told: &Told, scripts: bool, take_click: &mut dyn FnMut() -> bool) {
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
                scripts,
                url: spec.url.clone(),
                told: told.clone(),
                native: None,
                engine: super::engine::Kind::WebKit,
                at: Arc::default(),
                waiting: Vec::new(),
                // A command the page sent before the view was made runs once it is.
                token: None,
                shown: None,
                window: None,
                over: Overlaid::default(),
                cuts: false,
            });
            // In the order sent, each past the last one run.
            for command in &spec.commands {
                if entry.token.is_some_and(|done| command.token <= done) {
                    continue;
                }
                entry.token = Some(command.token);
                // Whether it may at all; WebKit's side checks the page it is on as it runs (`gate`).
                let refused = super::gate(&command.go, scripts, false, true)
                    .err()
                    .or_else(|| (super::needs_click(&command.go) && !take_click()).then_some(super::NO_CLICK));
                if let Some(why) = refused {
                    let refused = Err(why.to_owned());
                    let _ = told.try_send((spec.name.clone(), Heard::Result { token: command.token, result: refused }));
                    continue;
                }
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
/// page is drawn as a view a piece puts on screen. A dialog of the page's own over it says where it is, as the window's
/// do, so the page shows dimmed under it.
pub fn slot(page: EntityId, name: SharedString) -> impl IntoElement {
    canvas(
        move |bounds, window, _| {
            if let Some(piece) = slots::view_piece() {
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
    // Every overlay is a hole in a page whose engine can show through it, and a dialog's backdrop dims it; one that takes
    // clicks hides a page whose engine can't, and a tooltip leaves that one as it is, as hiding it for a tooltip made it
    // blink.
    let covers = slots::overlays(id);
    let backdrops = slots::backdrops(id);
    let mut blocking = slots::blocking_covers(id);
    blocking.extend(backdrops.iter().copied());
    // Dropped after the borrow, as a view that ends may call back.
    let mut ended = Vec::new();
    VIEWS.with_borrow_mut(|views| {
        for ((page, name), entry) in views.iter_mut() {
            if entry.window.is_some_and(|window| window != id) {
                continue;
            }
            let slot = places
                .iter()
                .find(|(slot, _)| *slot == slot_name(*page, name))
                .map(|(_, slot)| slot.bounds.intersect(&slot.mask.bounds))
                .filter(|at| !at.is_empty());
            // What lies over part of it shows through it where its engine can, and else hides it, as an overlay over all
            // of it does: hiding the page for a menu left it black under the menu.
            let over = slot.and_then(|at| overlaid(at, &covers, &backdrops));
            let at = slot.filter(|_| over.is_some());
            // The profile switched engines: its page opens again in the other one, where it was.
            if let Some(maker) = &maker
                && entry.native.is_some()
                && maker.engine(&entry.profile) != entry.engine
            {
                ended.extend(entry.native.take());
                entry.shown = None;
                entry.over = Overlaid::default();
                entry.cuts = false;
                if let Some(at) = entry.at.lock().unwrap().clone() {
                    entry.url = Some(at);
                }
            }
            if entry.native.is_none() {
                let (Some(maker), Some(_)) = (&maker, at) else { continue };
                let tell = Tell { told: entry.told.clone(), name: name.clone(), at: entry.at.clone() };
                entry.engine = maker.engine(&entry.profile);
                let mut native = maker.make(&entry.profile, entry.keep_data, entry.scripts, entry.url.as_deref(), tell);
                for (go, token) in entry.waiting.drain(..) {
                    native.go(&go, token);
                }
                entry.native = Some(native);
                entry.window = Some(id);
            }
            let over = over.unwrap_or_default();
            if let Some(native) = &mut entry.native
                && over != entry.over
            {
                entry.cuts = native.overlaid(&over);
                entry.over = over;
            }
            let at = at.filter(|at| entry.cuts || !blocking.iter().any(|cover| cover.intersects(at)));
            if entry.shown != at
                && let Some(native) = &mut entry.native
            {
                native.place(window, at);
                entry.shown = at;
            }
        }
    });
    drop(ended);
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
        /// What lies over it, told to an engine that shows through it.
        Over(Overlaid),
        Went(WebGo, u64),
        Ended,
    }

    #[derive(Default)]
    pub struct Record {
        pub did: Rc<RefCell<Vec<Did>>>,
        /// What each view made tells its page, by the order they were made in.
        pub tells: Rc<RefCell<Vec<Tell>>>,
        /// It says it makes views with Chromium, which a test switches.
        pub chromium: Rc<std::cell::Cell<bool>>,
    }

    /// What it did, and whether it shows through what lies over it, as Chromium's pages do.
    struct Fake(Rc<RefCell<Vec<Did>>>, bool);

    impl Native for Fake {
        fn place(&mut self, _: &Window, frame: Option<Bounds<Pixels>>) {
            self.0.borrow_mut().push(Did::Placed(frame));
        }
        fn overlaid(&mut self, over: &Overlaid) -> bool {
            if self.1 {
                self.0.borrow_mut().push(Did::Over(over.clone()));
            }
            self.1
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
        fn make(&self, profile: &Profile, keep_data: bool, _: bool, url: Option<&str>, told: Tell) -> Box<dyn Native> {
            self.did.borrow_mut().push(Did::Made { name: Some(profile.plugin.clone()), keep_data, url: url.map(str::to_owned) });
            self.tells.borrow_mut().push(told);
            Box::new(Fake(self.did.clone(), self.chromium.get()))
        }
        fn engine(&self, _: &Profile) -> crate::web::engine::Kind {
            if self.chromium.get() { crate::web::engine::Kind::Chromium } else { crate::web::engine::Kind::WebKit }
        }
    }

    /// Records the web views of the window `id`: what was done to them, and how to tell their pages.
    pub fn record(id: WindowId) -> (Rc<RefCell<Vec<Did>>>, Rc<RefCell<Vec<Tell>>>) {
        let (did, tells, _) = record_engines(id);
        (did, tells)
    }

    /// `record`, with whether it says it makes views with Chromium, which the test switches.
    pub fn record_engines(id: WindowId) -> (Rc<RefCell<Vec<Did>>>, Rc<RefCell<Vec<Tell>>>, Rc<std::cell::Cell<bool>>) {
        let record = Record::default();
        let (did, tells, chromium) = (record.did.clone(), record.tells.clone(), record.chromium.clone());
        make_with(id, Rc::new(record));
        (did, tells, chromium)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn button(id: &str, name: &str, badge: &str) -> Extension {
        Extension { id: id.into(), name: name.into(), label: format!("{name}\n  for this page"), badge: badge.into(), enabled: true, ..Extension::default() }
    }

    #[test]
    fn a_pages_extension_buttons_come_by_name_on_one_line_each_and_with_no_badge_on_a_signed_in_site() {
        let found = vec![button("b2", "react Developer Tools", ""), button("a1", "Bitwarden", "3"), button("a0", "Bitwarden", "12345678910")];
        let buttons = shown(found.clone(), false);
        let order: Vec<&str> = buttons.iter().map(|button| button.id.as_str()).collect();
        assert_eq!(order, ["a0", "a1", "b2"], "by name, then by id");
        assert_eq!(buttons[1].label, "Bitwarden for this page");
        assert_eq!((buttons[0].badge.as_str(), buttons[1].badge.as_str()), ("12345678", "3"), "a badge is a few characters");
        let long = shown(vec![button("c", &"x".repeat(500), "")], false);
        assert_eq!(long[0].name.chars().count(), 100);
        // What an extension worked out from a page signed in with imported cookies never reaches the plugin.
        assert!(shown(found, true).iter().all(|button| button.badge.is_empty()));
    }

    #[test]
    fn the_page_hears_its_extension_buttons_with_its_state() {
        let state = State { extensions: vec![Extension { icon: Some("ext-btn-1".into()), ..button("a1", "Bitwarden", "3") }], ..State::default() };
        let heard = serde_json::to_value(&state).unwrap();
        assert_eq!(
            heard["extensions"],
            serde_json::json!([{ "id": "a1", "name": "Bitwarden", "label": "Bitwarden\n  for this page", "icon": "ext-btn-1", "badge": "3", "enabled": true }])
        );
    }
}

#[cfg(test)]
mod holes_tests {
    use gpui::{point, px, size};

    use super::*;

    /// A menu over part of a web page is a hole in it, which the page shows through; an overlay over all of it hides it,
    /// but a dialog's backdrop, which dims it around the dialog.
    #[test]
    fn an_overlay_over_part_of_a_web_page_is_a_hole_in_it_and_a_dialog_dims_it() {
        let at = |x: f32, y: f32, w: f32, h: f32| Bounds::new(point(px(x), px(y)), size(px(w), px(h)));
        let hole = |at, corners| Hole { at, corners };
        let holes = |holes: Vec<Hole>| Some(Overlaid { holes, dimmed: false });
        let over = |covers: &[Bounds<Pixels>]| {
            let covers: Vec<_> = covers.iter().map(|cover| (*cover, None)).collect();
            overlaid(at(100.0, 50.0, 800.0, 600.0), &covers, &[])
        };
        assert_eq!(over(&[]), holes(vec![]));
        assert_eq!(over(&[at(0.0, 0.0, 50.0, 40.0)]), holes(vec![]), "an overlay beside it");
        assert_eq!(
            over(&[at(300.0, 100.0, 200.0, 150.0)]),
            holes(vec![hole(at(200.0, 50.0, 200.0, 150.0), [ROUND; 4])]),
            "a menu inside it, with its round corners"
        );
        // Only the corner the page's edges don't cut off is round.
        let menu = at(700.0, 40.0, 250.0, 60.0);
        assert_eq!(over(&[menu]), holes(vec![hole(at(600.0, 0.0, 200.0, 50.0), [0.0, 0.0, 0.0, ROUND])]));
        assert_eq!(over(&[menu, at(0.0, 0.0, 1200.0, 900.0)]), None, "an overlay over all of it");
        // A tooltip drawn twice as it comes in is one hole, which the mask cuts once.
        let tooltip = at(800.0, 40.0, 150.0, 30.0);
        assert_eq!(over(&[tooltip, tooltip]), holes(vec![hole(at(700.0, 0.0, 100.0, 20.0), [0.0, 0.0, 0.0, ROUND])]));
        // Two that overlap cover what both do, once each part.
        let cut = over(&[at(200.0, 100.0, 100.0, 100.0), at(250.0, 150.0, 100.0, 100.0)]).unwrap().holes;
        let area: f32 = cut.iter().map(|hole| f32::from(hole.at.size.width) * f32::from(hole.at.size.height)).sum();
        assert_eq!(area, 100.0 * 100.0 * 2.0 - 50.0 * 50.0);
        for (index, one) in cut.iter().enumerate() {
            assert!(cut[index + 1..].iter().all(|other| !other.at.intersects(&one.at)), "{cut:?}");
        }
        // A dialog: its layer and backdrop over the whole window, and its panel, as round as it says.
        let window = at(0.0, 0.0, 1200.0, 900.0);
        let dialog = [(window, None), (at(400.0, 250.0, 400.0, 300.0), Some(14.0))];
        assert_eq!(
            overlaid(at(100.0, 50.0, 800.0, 600.0), &dialog, &[window]),
            Some(Overlaid { holes: vec![hole(at(300.0, 200.0, 400.0, 300.0), [14.0; 4])], dimmed: true })
        );
    }
}
