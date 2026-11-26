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
}

impl Tell {
    pub fn tell(&self, heard: Heard) {
        let _ = self.told.try_send((self.name.clone(), heard));
    }
}

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
    /// `scripts`: the plugin may run scripts in its pages, so the page's console and requests are kept for `logs`.
    fn make(&self, profile: &Profile, keep_data: bool, scripts: bool, url: Option<&str>, told: Tell) -> Box<dyn Native>;
}

struct Entry {
    profile: Profile,
    keep_data: bool,
    scripts: bool,
    url: Option<String>,
    told: Told,
    native: Option<Box<dyn Native>>,
    /// What the page asked before the engine's view was made, done once it is, with each command's token.
    waiting: Vec<(WebGo, u64)>,
    /// The token of the last command done, so each runs once: a page's tokens only grow.
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
                waiting: Vec::new(),
                // A command the page sent before the view was made runs once it is.
                token: None,
                shown: None,
                window: None,
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
                let tell = Tell { told: entry.told.clone(), name: name.clone() };
                let mut native = maker.make(&entry.profile, entry.keep_data, entry.scripts, entry.url.as_deref(), tell);
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
        pub tells: Rc<RefCell<Vec<Tell>>>,
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
        fn make(&self, profile: &Profile, keep_data: bool, _: bool, url: Option<&str>, told: Tell) -> Box<dyn Native> {
            self.did.borrow_mut().push(Did::Made { name: Some(profile.plugin.clone()), keep_data, url: url.map(str::to_owned) });
            self.tells.borrow_mut().push(told);
            Box::new(Fake(self.did.clone()))
        }
    }

    /// Records the web views of the window `id`: what was done to them, and how to tell their pages.
    pub fn record(id: WindowId) -> (Rc<RefCell<Vec<Did>>>, Rc<RefCell<Vec<Tell>>>) {
        let record = Record::default();
        let did = record.did.clone();
        let tells = record.tells.clone();
        make_with(id, Rc::new(record));
        (did, tells)
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
