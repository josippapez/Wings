//! The Disk usage tab: the disks and folders to scan, a scan as a map you click into, and the Collector. Main runs every
//! scan and tells the tab what describes it; the folder in the middle of the map is asked of Wings (`level.rs`), one at a
//! time. Where the tab is, the scan it shows and the folder in the middle, is kept while the tab is open.

use crate::{
    ask::{ask, listen},
    bytes::{bytes, count},
    df::DiskRow,
    disks::load_disks,
    entry::{Entry, parent_path},
    level::{Level, MOST, fetch},
    rows::{Extras, Row, rows_of},
    state::{Collector, Done, Phase, ScanInfo, STATE, TrashResult, REFRESH, WANT},
    sunburst::{Layout, MIN_ANGLE, RINGS, layout},
};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};
use wings_plugin::{host, host::storage, messages, poll, task::spawn};

/// The list's width, in points.
pub const SPLIT: f64 = 320.0;
/// How many rows the list draws at first, and how many more each press of its button adds. The page may hold 6,000 entries.
pub const FIRST_ROWS: usize = 120;
/// How many folders' answers are kept, so going back into one is at once.
const MEMO: usize = 12;
/// How many folders' smaller items are kept.
const EXTRAS: usize = 3;
/// How many seconds Move to Trash waits for a Cancel.
const COUNTDOWN: u32 = 5;
const REFETCH_MS: u32 = 15_000;

/// Where the folders you went into are, to go back and forward through. Starts again for another scan.
#[derive(Default)]
pub struct Trail {
    pub root: Option<String>,
    pub stack: Vec<Option<String>>,
    pub at: usize,
}

/// A message the Collector bar shows under its head, until it is dismissed.
pub struct Message {
    pub id: u64,
    pub tone: &'static str,
    pub text: String,
}

/// What is asked before it is done: freeing the purgeable space, or deleting one snapshot.
#[derive(Clone, PartialEq)]
pub enum Confirm {
    Purge,
    Snapshot(String),
}

#[derive(Clone, Copy, PartialEq)]
pub enum Countdown {
    Idle,
    Counting(u32),
    Moving,
}

/// The folder in the middle of the map, with the wedges and rows it is drawn from.
pub struct Shown {
    pub key: String,
    pub level: Rc<Level>,
    pub layout: Layout,
    pub rows: Vec<Row>,
    /// What the layout and rows were made from, to know when they have to be again.
    made: (Option<u64>, bool, u64),
}

pub struct Page {
    /// The scan the tab shows, by the root it was asked for, or none for the disks.
    pub root: Option<String>,
    /// The folder in the middle of its map, or none for the scan's root.
    pub focus: Option<String>,
    pub disks: Option<Result<Vec<DiskRow>, String>>,
    /// The scans main keeps, or none before it has said.
    pub scans: Option<Vec<ScanInfo>>,
    pub collector: Collector,
    pub projects: Vec<(String, String)>,
    pub starred: Vec<String>,
    /// Asked for by this page and not in the state yet: a scan just begun is not "no scan".
    pub starting: Option<String>,
    pub problem: Option<String>,
    pub scan_text: String,
    pub scan_error: Option<String>,
    pub trail: Trail,
    /// The sizes of the folders gone into, to ask for each in the detail it needs.
    pub sizes: HashMap<String, u64>,
    pub memo: Vec<(String, Rc<Level>)>,
    pub shown: Option<Shown>,
    /// The folder being asked for, by its key.
    pub loading: Option<String>,
    pub level_error: Option<(String, String)>,
    pub selected: Option<String>,
    pub hidden_open: bool,
    pub confirm: Option<Confirm>,
    pub confirm_busy: bool,
    pub confirm_error: Option<String>,
    /// The folders whose smaller items are open, and what `listAll` found in them.
    pub smaller: HashSet<String>,
    pub extras: HashMap<String, Extras>,
    extras_made: u64,
    pub messages: Vec<Message>,
    next_message: u64,
    /// The menu that is open, by what it belongs to.
    pub menu: Option<String>,
    /// What the pointer is on in the map: a wedge's index, -2 for the middle, -1 for nothing.
    pub pointed: i64,
    pub more_rows: usize,
    pub collector_open: bool,
    pub countdown: Countdown,
    countdown_run: u64,
    counting_what: String,
    pub action_error: Option<String>,
    ticking: bool,
}

thread_local! {
    static PAGE: RefCell<Page> = RefCell::new(Page::new());
}

pub fn with<T>(change: impl FnOnce(&mut Page) -> T) -> T {
    PAGE.with(|page| change(&mut page.borrow_mut()))
}

/// What the Collector holds, as one string that changes when its items do.
fn what(collector: &Collector) -> String {
    format!("{}\n{}", collector.total(), collector.items.iter().map(|i| i.path.as_str()).collect::<Vec<_>>().join("\n"))
}

impl Page {
    fn new() -> Self {
        Self {
            root: None,
            focus: None,
            disks: None,
            scans: None,
            collector: Collector::default(),
            projects: Vec::new(),
            starred: Vec::new(),
            starting: None,
            problem: None,
            scan_text: String::new(),
            scan_error: None,
            trail: Trail::default(),
            sizes: HashMap::new(),
            memo: Vec::new(),
            shown: None,
            loading: None,
            level_error: None,
            selected: None,
            hidden_open: false,
            confirm: None,
            confirm_busy: false,
            confirm_error: None,
            smaller: HashSet::new(),
            extras: HashMap::new(),
            extras_made: 0,
            messages: Vec::new(),
            next_message: 0,
            menu: None,
            pointed: -1,
            more_rows: FIRST_ROWS,
            collector_open: false,
            countdown: Countdown::Idle,
            countdown_run: 0,
            counting_what: String::new(),
            action_error: None,
            ticking: false,
        }
    }

    pub fn scan(&self, root: &str) -> Option<&ScanInfo> {
        self.scans.as_ref()?.iter().find(|scan| scan.root == root)
    }

    /// The finished scan the tab shows.
    pub fn done(&self) -> Option<(&ScanInfo, &Done)> {
        let scan = self.scan(self.root.as_deref()?)?;
        scan.done().map(|done| (scan, done))
    }

    /// The folder in the middle of the map: a full path.
    pub fn focus_path(&self) -> Option<String> {
        let (_, done) = self.done()?;
        Some(self.focus.clone().unwrap_or_else(|| done.name.clone()))
    }

    /// The hidden space to draw a wedge for: a disk scan's, while its root is in the middle.
    fn hidden_wanted(&self) -> Option<u64> {
        let (_, done) = self.done()?;
        let (hidden, at_root) = (done.hidden.as_ref()?, self.focus.as_ref().is_none_or(|focus| *focus == done.name));
        (done.disk.is_some() && at_root && hidden.size > 0).then_some(hidden.size)
    }

    /// Makes the wedges and rows again when what they are made from changed.
    pub fn refresh_shown(&mut self) {
        let made = (self.hidden_wanted(), self.shown.as_ref().is_some_and(|shown| self.smaller.contains(&shown.level.tree.name)), self.extras_made);
        let extras = self.shown.as_ref().and_then(|shown| self.extras.get(&shown.key));
        if let Some(shown) = self.shown.as_mut().filter(|shown| shown.made != made) {
            let tree = &shown.level.tree;
            shown.layout = layout(tree, &tree.name, RINGS, made.0, MIN_ANGLE);
            shown.rows = rows_of(&tree.name, tree, made.1, extras, made.0);
            shown.made = made;
        }
    }

    pub fn say(&mut self, list: Vec<(&'static str, String)>) {
        self.messages = list
            .into_iter()
            .map(|(tone, text)| {
                self.next_message += 1;
                Message { id: self.next_message, tone, text }
            })
            .collect();
    }

    pub fn leave(&mut self) {
        self.root = None;
        self.focus = None;
    }

    /// Puts another folder in the middle of the map, which starts with nothing picked.
    pub fn set_focus(&mut self, focus: Option<String>) {
        if self.focus != focus {
            self.selected = None;
            self.hidden_open = false;
            self.confirm = None;
            self.pointed = -1;
            self.more_rows = FIRST_ROWS;
            self.menu = None;
        }
        self.focus = focus;
    }

    /// The trail for the scan the tab shows, started again when the tab is on another scan.
    fn trail_here(&mut self) -> &mut Trail {
        if self.trail.root != self.root {
            self.trail = Trail { root: self.root.clone(), stack: vec![self.focus.clone()], at: 0 };
        }
        &mut self.trail
    }

    /// Goes into the folder at `path`, which holds `size` bytes when that is known.
    pub fn go(&mut self, path: String, size: Option<u64>) {
        if self.focus_path().as_ref() == Some(&path) {
            return;
        }
        if let Some(size) = size {
            self.sizes.insert(path.clone(), size);
        }
        let trail = self.trail_here();
        trail.stack.truncate(trail.at + 1);
        trail.stack.push(Some(path.clone()));
        trail.at = trail.stack.len() - 1;
        self.set_focus(Some(path));
    }

    pub fn walk(&mut self, at: usize) {
        let trail = self.trail_here();
        let Some(focus) = trail.stack.get(at).cloned() else { return };
        trail.at = at;
        self.set_focus(focus);
    }

    /// Whether the scan on screen is running, which shows how long it has.
    fn scanning(&self) -> bool {
        self.root.as_deref().and_then(|root| self.scan(root)).is_some_and(|scan| matches!(scan.phase, Phase::Scanning { .. }))
    }

    /// What names a folder of a scan: the scan, as of when it ran, and the folder.
    pub fn key_of(scan: &ScanInfo, folder: &str) -> String {
        format!("{}@{}\n{folder}", scan.root, scan.at())
    }

    /// The key of the folder in the middle of the map.
    pub fn current_key(&self) -> Option<String> {
        let (scan, _) = self.done()?;
        Some(Self::key_of(scan, &self.focus_path()?))
    }

    /// What to ask Wings for the folder on screen, unless it is here or on its way: its key, path, the scan's floor and its size when known.
    fn wanted(&mut self) -> Option<(String, String, u64, Option<u64>)> {
        let (scan, done) = self.done()?;
        let path = self.focus_path()?;
        let key = Self::key_of(scan, &path);
        let (min_bytes, size) = (done.min_bytes, if self.focus.is_none() { Some(done.size) } else { self.sizes.get(&path).copied() });
        if self.shown.as_ref().is_some_and(|shown| shown.key == key) {
            return None;
        }
        if let Some((_, level)) = self.memo.iter().find(|(known, _)| *known == key) {
            let made = (None, false, u64::MAX);
            self.shown = Some(Shown { key, level: level.clone(), layout: Layout { segments: Vec::new(), by_path: HashMap::new(), rings: 0 }, rows: Vec::new(), made });
            return None;
        }
        if self.loading.as_ref() == Some(&key) || self.level_error.as_ref().is_some_and(|(failed, _)| *failed == key) {
            return None;
        }
        self.loading = Some(key.clone());
        Some((key, path, min_bytes, size))
    }

    /// What Wings answered for the folder `key`.
    fn loaded(&mut self, key: &str, answer: Result<Level, String>) {
        if self.loading.as_deref() != Some(key) {
            return;
        }
        self.loading = None;
        match answer {
            Ok(level) => {
                self.sizes.insert(level.tree.name.clone(), level.tree.size);
                self.memo.retain(|(known, _)| known != key);
                self.memo.push((key.to_owned(), Rc::new(level)));
                if self.memo.len() > MEMO {
                    self.memo.remove(0);
                }
            }
            // A folder that is gone, like one that moved to the Trash, leaves the nearest one above it.
            Err(error) => match self.focus.as_deref().and_then(parent_path) {
                Some(parent) => {
                    let root = self.done().map(|(_, done)| done.name.clone());
                    self.set_focus(Some(parent).filter(|parent| Some(parent) != root.as_ref()));
                }
                None => self.level_error = Some((key.to_owned(), error)),
            },
        }
    }
}

/// Asks Wings for the folder on screen when it isn't here, and goes on to the next once it is.
pub fn sync_level() {
    let Some((key, path, min_bytes, size)) = with(Page::wanted) else { return };
    spawn(async move {
        let answer = fetch(&path, min_bytes, size, RINGS as u64, MOST).await;
        with(|page| page.loaded(&key, answer));
        sync_level();
    });
}

/// What the state main tells says about scans and the Collector.
fn on_state(state: &Value) {
    let scans: Vec<ScanInfo> = state["scans"].as_array().map_or_else(Vec::new, |list| list.iter().filter_map(ScanInfo::from_value).collect());
    let collector = Collector::from_value(&state["collector"]);
    let scanning = with(|page| {
        if page.starting.as_ref().is_some_and(|root| scans.iter().any(|scan| scan.root == *root)) {
            page.starting = None;
        }
        // The countdown names what it will move, so a change to the Collector ends it.
        if matches!(page.countdown, Countdown::Counting(_)) && what(&collector) != page.counting_what {
            page.countdown = Countdown::Idle;
            page.countdown_run += 1;
        }
        page.collector = collector;
        page.scans = Some(scans);
        page.scanning()
    });
    if scanning {
        tick();
    }
    sync_level();
}

/// Ticks once a second while a scan is on screen, so how long it has run goes on.
fn tick() {
    if with(|page| std::mem::replace(&mut page.ticking, true)) {
        return;
    }
    spawn(async {
        loop {
            host::sleep(1000).await;
            if !with(|page| page.scanning()) {
                with(|page| page.ticking = false);
                break;
            }
        }
    });
}

pub fn reload_disks() {
    spawn(async {
        let disks = load_disks().await.map(|disks| disks.rows);
        with(|page| page.disks = Some(disks));
    });
}

/// Scans `target` and shows it.
pub fn start_scan(target: String) {
    with(|page| {
        page.problem = None;
        page.starting = Some(target.clone());
        let same = page.root.as_ref() == Some(&target);
        page.root = Some(target.clone());
        if !same {
            page.focus = None;
        }
    });
    sync_level();
    spawn(async move {
        if let Err(error) = ask("scan", json!({ "root": target })).await {
            with(|page| {
                page.problem = Some(format!("Couldn't scan {target}: {error}"));
                page.starting = None;
                if page.root.as_ref() == Some(&target) {
                    page.leave();
                }
            });
        }
    });
}

/// Asks main for something that isn't answered with a screen: a failure shows as a problem.
pub fn tell_main(op: &'static str, args: Value) {
    spawn(async move {
        if let Err(error) = ask(op, args).await {
            with(|page| page.problem = Some(error));
        }
    });
}

/// `tell_main`, for the map: a failure shows over the map.
pub fn run(op: &'static str, args: Value) {
    spawn(async move {
        if let Err(error) = ask(op, args).await {
            with(|page| page.action_error = Some(error));
        }
    });
}

pub fn stop_scan(root: String) {
    tell_main("stop", json!({ "root": root }));
    with(Page::leave);
}

pub fn collect(items: Vec<crate::state::Collected>) {
    with(|page| page.say(Vec::new()));
    spawn(async move {
        let items: Vec<Value> = items.iter().map(crate::state::Collected::to_value).collect();
        if let Err(error) = ask("collect", json!({ "items": items })).await {
            with(|page| page.say(vec![("warn", error)]));
        }
    });
}

/// What to tell about a move to the Trash: what moved, and each thing that didn't with its reason.
pub fn trash_messages(result: &TrashResult) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    if !result.trashed.is_empty() {
        out.push(("success", format!("Moved {}, {}, to the Trash. Empty the Trash to get the space back.", count(result.trashed.len() as u64, "item"), bytes(result.freed))));
    }
    if !result.failed.is_empty() {
        let lines: Vec<String> = result.failed.iter().map(|(path, message)| format!("{}: {message}", wings_plugin::format::basename(path))).collect();
        out.push(("danger", format!("{} didn't move:\n{}", count(result.failed.len() as u64, "item"), lines.join("\n"))));
    }
    out
}

fn trash() {
    with(|page| page.say(Vec::new()));
    spawn(async {
        let answer = ask("trash", Value::Null).await;
        let moved = with(|page| {
            page.countdown = Countdown::Idle;
            match answer {
                Ok(result) => {
                    let result = TrashResult::from_value(&result);
                    page.say(trash_messages(&result));
                    !result.trashed.is_empty()
                }
                Err(error) => {
                    page.say(vec![("danger", error)]);
                    false
                }
            }
        });
        if moved {
            // Wings leaves out of what it kept what is gone since, so the folder is asked for again.
            with(|page| {
                page.memo.clear();
                page.shown = None;
            });
            sync_level();
        }
    });
}

/// Starts Move to Trash, which waits for a Cancel.
pub fn start_trash() {
    let run = with(|page| {
        page.countdown_run += 1;
        page.countdown = Countdown::Counting(COUNTDOWN);
        page.counting_what = what(&page.collector);
        page.countdown_run
    });
    spawn(async move {
        loop {
            host::sleep(1000).await;
            let step = with(|page| match page.countdown {
                _ if page.countdown_run != run => None,
                Countdown::Counting(left) if left > 1 => {
                    page.countdown = Countdown::Counting(left - 1);
                    Some(false)
                }
                Countdown::Counting(_) => {
                    page.countdown = Countdown::Moving;
                    Some(true)
                }
                _ => None,
            });
            match step {
                Some(false) => {}
                Some(true) => return trash(),
                None => return,
            }
        }
    });
}

pub fn cancel_trash() {
    with(|page| {
        page.countdown_run += 1;
        page.countdown = Countdown::Idle;
    });
}

/// Opens or closes the smaller items of `folder`, and asks Wings for what is in them once.
pub fn toggle_smaller(folder: String) {
    let Some(key) = with(|page| {
        let key = Page::key_of(page.done()?.0, &folder);
        if page.smaller.remove(&folder) {
            return None;
        }
        page.smaller.insert(folder.clone());
        page.extras.get(&key).is_none_or(|extras| matches!(extras, Extras::Failed(_))).then(|| {
            page.extras.insert(key.clone(), Extras::Loading);
            page.extras_made += 1;
            key
        })
    }) else {
        return;
    };
    spawn(async move {
        let answer = host::disk_usage(&folder, &json!({ "minBytes": 0, "depth": 1, "maxEntries": 20_000, "reuse": true }), None).await;
        with(|page| {
            if page.extras.len() >= EXTRAS {
                page.extras.retain(|known, _| *known == key);
            }
            page.extras.insert(key, match answer {
                Ok(answer) => Extras::Ready(Entry::from_value(&answer["root"]).children),
                Err(error) => Extras::Failed(error),
            });
            page.extras_made += 1;
        });
    });
}

/// Does what the hidden space's confirmation asked.
pub fn confirmed(mount: String) {
    let Some(what) = with(|page| {
        page.confirm_busy = true;
        page.confirm_error = None;
        page.confirm.clone()
    }) else {
        return;
    };
    spawn(async move {
        let answer = match &what {
            Confirm::Purge => ask("purge", json!({ "mount": mount })).await,
            Confirm::Snapshot(date) => ask("deleteSnapshot", json!({ "date": date })).await,
        };
        with(|page| {
            page.confirm_busy = false;
            match answer {
                Ok(_) => page.confirm = None,
                Err(error) => page.confirm_error = Some(error),
            }
        });
    });
}

/// The folders the user starred, as the plugin keeps them.
fn read_starred() -> Vec<String> {
    storage::get("starred").ok().and_then(|saved| saved.as_array().map(|list| list.iter().filter_map(|p| p.as_str().filter(|p| !p.is_empty()).map(str::to_owned)).collect())).unwrap_or_default()
}

pub fn star(path: &str, on: bool) {
    with(|page| {
        page.starred.retain(|starred| starred != path);
        if on {
            page.starred.push(path.to_owned());
        }
        let _ = storage::set("starred", &json!(page.starred));
    });
}

#[unsafe(no_mangle)]
pub fn start() {
    wings_plugin::run_filled(crate::views::view);
    listen();
    messages::on(STATE, on_state);
    messages::on(REFRESH, |_| reload_disks());
    host::on_projects(|projects| {
        let list = projects.iter().map(|project| (project["name"].as_str().unwrap_or_default().to_owned(), project["path"].as_str().unwrap_or_default().to_owned())).collect();
        with(|page| page.projects = list);
    });
    with(|page| page.starred = read_starred());
    reload_disks();
    poll::while_focused(REFETCH_MS, false, reload_disks);
    messages::tell(WANT, &Value::Null);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Done, Phase};
    use wings_plugin::task::block_on;

    fn done(name: &str, size: u64) -> ScanInfo {
        ScanInfo { root: "~".into(), phase: Phase::Done(Done { name: name.into(), size, found: size, min_bytes: 1 << 20, restricted: 0, at: 5.0, disk: None, hidden: None }), stamp: 5.0 }
    }

    fn showing(scan: ScanInfo) {
        with(|page| {
            page.root = Some("~".into());
            page.scans = Some(vec![scan]);
        });
    }

    /// What Wings answers for a folder of `children` files of 8 KB each.
    fn answer(children: usize) -> Value {
        let files: Vec<Value> = (0..children).map(|i| json!({ "name": format!("file-{i:05}.bin"), "size": 8192 + (children - i) as u64, "kind": "file", "items": 0 })).collect();
        json!({ "root": { "name": "/Users/ada", "size": 50_000_000u64, "kind": "dir", "items": children, "children": files }, "floor": 4096 })
    }

    #[test]
    fn the_trail_goes_back_and_forward_through_the_folders_gone_into() {
        showing(done("/Users/ada", 100));
        with(|page| {
            page.go("/Users/ada/Code".into(), Some(60));
            page.go("/Users/ada/Code/app".into(), Some(30));
            assert_eq!(page.focus.as_deref(), Some("/Users/ada/Code/app"));
            page.walk(0);
            assert_eq!(page.focus, None);
            page.walk(1);
            assert_eq!(page.focus.as_deref(), Some("/Users/ada/Code"));
            // Going somewhere else from the middle of the trail drops what was ahead.
            page.go("/Users/ada/Movies".into(), None);
            assert_eq!((page.trail.stack.len(), page.trail.at), (3, 2));
            assert_eq!(page.sizes.get("/Users/ada/Code/app"), Some(&30));
        });
    }

    #[test]
    fn a_folder_that_is_gone_leaves_the_nearest_one_above_it() {
        showing(done("/Users/ada", 100));
        with(|page| {
            page.go("/Users/ada/Code/app".into(), None);
            page.loading = Some("k".into());
            page.loaded("k", Err("There's no file or folder at /Users/ada/Code/app".into()));
            assert_eq!(page.focus.as_deref(), Some("/Users/ada/Code"));
            // The scan's root can't be left: the tab says why it couldn't read it.
            page.set_focus(None);
            page.loading = Some("root".into());
            page.loaded("root", Err("No access".into()));
            assert_eq!(page.level_error, Some(("root".into(), "No access".into())));
        });
    }

    #[test]
    fn what_moved_to_the_trash_and_what_didnt_are_told_with_their_reasons() {
        let result = TrashResult { trashed: vec!["/a".into(), "/b".into()], failed: vec![("/Users/ada/x.zip".into(), "Operation not permitted".into())], freed: 12_300_000 };
        assert_eq!(
            trash_messages(&result),
            [("success", "Moved 2 items, 12.3 MB, to the Trash. Empty the Trash to get the space back.".to_owned()), ("danger", "1 item didn't move:\nx.zip: Operation not permitted".to_owned())]
        );
        assert!(trash_messages(&TrashResult::default()).is_empty());
    }

    #[test]
    fn a_folder_of_five_thousand_entries_is_asked_for_once_and_shown_as_a_list_that_draws_its_biggest_rows() {
        showing(done("/Users/ada", 50_000_000));
        let asked = Rc::new(RefCell::new(Vec::new()));
        let log = asked.clone();
        host::testing::answer(move |method, params| {
            log.borrow_mut().push((method.to_owned(), params["depth"].as_u64(), params["maxEntries"].as_u64(), params["minBytes"].as_u64()));
            Ok(answer(5000))
        });
        sync_level();
        sync_level();
        block_on(async {});
        // The scan was of a folder of 50 MB with a floor of 1 MB: Wings is asked for it at the 4 KB its size calls for, once.
        assert_eq!(*asked.borrow(), [("diskUsage".to_owned(), Some(6), Some(6000), Some(4096))]);
        let rows = with(|page| {
            page.refresh_shown();
            let shown = page.shown.as_ref().expect("the folder is shown");
            assert!(shown.layout.segments.len() < 400, "{}", shown.layout.segments.len());
            shown.rows.len()
        });
        assert_eq!(rows, 5000);
        assert!(!crate::views::view().is_empty());
    }
}
