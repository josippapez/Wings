//! The scans. Main runs every one and keeps what describes it, so each Disk usage tab and Claude's tools see the same
//! ones, and tells every tab on each change. The trees stay in Wings (see `level.rs`).

use crate::{
    collector,
    disks::{disks_now, hidden_of},
    gate::Gate,
    level::detail_for,
    paths::{expand_home, home_folder, inside, tidy},
    state::{Collected, Done, Phase, Progress, STATE, ScanInfo},
};
use serde_json::{Value, json};
use std::{cell::{Cell, RefCell}, collections::HashMap, rc::Rc};
use wings_plugin::{host, messages, task::spawn};

/// What a scan's own answer holds: the root and nothing under it. Wings keeps the tree, and what is under a folder is
/// asked for when it is shown.
const MAX_ENTRIES: u64 = 1;
/// The scan's own message when it is stopped.
const STOPPED: &str = "The scan was stopped";

/// A scan that is running.
pub struct Job {
    stopped: Cell<bool>,
    stopper: RefCell<Option<host::Stopper>>,
    /// Opens once the scan shows as scanning, or has ended without.
    pub started: Gate,
    /// Opens when the scan has ended, in any state.
    pub done: Gate,
}

thread_local! {
    static SCANS: RefCell<Vec<ScanInfo>> = const { RefCell::new(Vec::new()) };
    static JOBS: RefCell<HashMap<String, Rc<Job>>> = RefCell::new(HashMap::new());
}

/// The scans, newest first.
pub fn summaries() -> Vec<ScanInfo> {
    let mut list = SCANS.with(|scans| scans.borrow().clone());
    list.sort_by(|a, b| b.at().total_cmp(&a.at()));
    list
}

pub fn scan_of(root: &str) -> Option<ScanInfo> {
    SCANS.with(|scans| scans.borrow().iter().find(|scan| scan.root == root).cloned())
}

/// Tells every tab the scans and the Collector.
pub fn publish() {
    messages::tell(STATE, &json!({ "scans": summaries().iter().map(ScanInfo::to_value).collect::<Vec<_>>(), "collector": collector::now().to_value() }));
}

/// Keeps `phase` for `root`, or drops the scan with none, and tells every tab. `quiet` keeps the time the scan last changed state: progress changes none.
fn keep(root: &str, phase: Option<Phase>, quiet: bool) {
    SCANS.with(|scans| {
        let mut scans = scans.borrow_mut();
        let before = scans.iter().position(|scan| scan.root == root);
        let stamp = match before {
            Some(at) if quiet => scans[at].stamp,
            _ => host::now(),
        };
        if let Some(at) = before {
            scans.remove(at);
        }
        if let Some(phase) = phase {
            scans.push(ScanInfo { root: root.to_owned(), phase, stamp });
        }
    });
    publish();
}

/// The finished scans that hold `path`, newest first: the ones whose root is `path` or a folder above it.
pub fn holders(path: &str, max_age_ms: f64) -> Vec<(ScanInfo, Done)> {
    let mut found: Vec<(ScanInfo, Done)> = summaries()
        .into_iter()
        .filter_map(|scan| scan.done().cloned().map(|done| (scan, done)))
        .filter(|(_, done)| host::now() - done.at <= max_age_ms && (done.name == path || inside(path, &done.name)))
        .collect();
    found.sort_by(|a, b| b.1.at.total_cmp(&a.1.at));
    found
}

/// `path` made absolute with the home folder the scans know, tidied; none for a `~` path before any scan of one is done.
pub fn resolve(path: &str) -> Option<String> {
    let scans = summaries();
    let done: Vec<(&str, &str)> = scans.iter().filter_map(|scan| scan.done().map(|done| (scan.root.as_str(), done.name.as_str()))).collect();
    expand_home(path, home_folder(done).as_deref())
}

/// What a scan is told: the bytes it should find, the least size to keep, and the disk when `root` is one's mount.
struct Plan {
    disk: Option<String>,
    expected: Option<u64>,
    min_bytes: u64,
}

async fn plan_of(root: &str) -> Result<Plan, String> {
    let rows = disks_now(30_000.0).await?.rows;
    let mount = tidy(root);
    if let Some(disk) = rows.iter().find(|d| d.disk.mount == mount) {
        let expected = disk.used + disk.purgeable;
        return Ok(Plan { disk: Some(mount), expected: Some(expected), min_bytes: 65_536.max((expected as f64 / 40_000.0).round() as u64) });
    }
    // Only a scan of the folder itself says how big it was: Wings holds the rest.
    let known = resolve(root).and_then(|full| holders(&full, f64::INFINITY).into_iter().find(|(_, done)| done.name == full).map(|(_, done)| done.size));
    Ok(match known {
        Some(size) => Plan { disk: None, expected: Some(size), min_bytes: detail_for(size) },
        None => Plan { disk: None, expected: None, min_bytes: 1_048_576 },
    })
}

async fn scan(root: &str, job: &Rc<Job>) -> Result<(), String> {
    let plan = plan_of(root).await?;
    if job.stopped.get() {
        return Err(STOPPED.into());
    }
    let (expected, started_at) = (plan.expected, host::now());
    keep(root, Some(Phase::Scanning { progress: Progress::default(), expected, started_at }), false);
    job.started.open();
    let this = job.clone();
    let named = root.to_owned();
    // A progress that arrives after the scan ended, or was stopped, must not bring its state back.
    let on_progress: Rc<dyn Fn(&Value)> = Rc::new(move |value| {
        let live = JOBS.with(|jobs| jobs.borrow().get(&named).is_some_and(|job| Rc::ptr_eq(job, &this)));
        if live && scan_of(&named).is_some_and(|scan| matches!(scan.phase, Phase::Scanning { .. })) {
            keep(&named, Some(Phase::Scanning { progress: Progress::from_value(value), expected, started_at }), true);
        }
    });
    let search = host::disk_usage(root, &json!({ "minBytes": plan.min_bytes, "maxEntries": MAX_ENTRIES }), Some(on_progress));
    *job.stopper.borrow_mut() = Some(search.stopper());
    let result = search.await?;
    let size = result["root"]["size"].as_u64().unwrap_or(0);
    // The tree is the result: a disk whose snapshots or numbers can't be read shows without its hidden space.
    let hidden = match &plan.disk {
        Some(mount) => hidden_of(mount, size).await.ok().flatten(),
        None => None,
    };
    if job.stopped.get() {
        return Err(STOPPED.into());
    }
    let done = Done {
        name: result["root"]["name"].as_str().unwrap_or(root).to_owned(),
        size,
        found: size,
        min_bytes: plan.min_bytes,
        restricted: result["restricted"].as_u64().unwrap_or(0),
        at: host::now(),
        disk: plan.disk,
        hidden,
    };
    keep(root, Some(Phase::Done(done)), false);
    Ok(())
}

async fn run(root: String, job: Rc<Job>) {
    let result = scan(&root, &job).await;
    // A job that is stopped can be replaced by a new scan of the root before it ends: it then leaves the root alone.
    let current = JOBS.with(|jobs| jobs.borrow().get(&root).is_some_and(|running| Rc::ptr_eq(running, &job)));
    if let (Err(error), true) = (result, current) {
        if job.stopped.get() || error == STOPPED { keep(&root, None, false) } else { keep(&root, Some(Phase::Failed(error)), false) }
    }
    job.started.open();
    if current {
        JOBS.with(|jobs| jobs.borrow_mut().remove(&root));
    }
    job.done.open();
}

/// Scans `root`, or gives the scan of it that is running.
pub fn start_scan(root: &str) -> Rc<Job> {
    if let Some(running) = JOBS.with(|jobs| jobs.borrow().get(root).cloned()).filter(|job| !job.stopped.get()) {
        return running;
    }
    let job = Rc::new(Job { stopped: Cell::new(false), stopper: RefCell::new(None), started: Gate::default(), done: Gate::default() });
    JOBS.with(|jobs| jobs.borrow_mut().insert(root.to_owned(), job.clone()));
    spawn(run(root.to_owned(), job.clone()));
    job
}

pub fn stop_scan(root: &str) {
    if let Some(job) = JOBS.with(|jobs| jobs.borrow().get(root).cloned()) {
        job.stopped.set(true);
        if let Some(stopper) = job.stopper.borrow().as_ref() {
            stopper.stop();
        }
    }
}

pub fn forget_scan(root: &str) {
    stop_scan(root);
    keep(root, None, false);
}

/// Scans `root`, waits for the scan, and gives it done, or says why it isn't. The tool that asked hears why, so a scan that
/// failed and was begun here, like one of a path Claude mistyped, isn't left in the tab's list.
pub async fn scan_folder(root: &str) -> Result<(ScanInfo, Done), String> {
    let running = JOBS.with(|jobs| jobs.borrow().get(root).cloned());
    let job = start_scan(root);
    let begun = running.is_none_or(|running| !Rc::ptr_eq(&running, &job));
    job.done.wait().await;
    match scan_of(root) {
        Some(scan @ ScanInfo { phase: Phase::Done(_), .. }) => {
            let done = scan.done().cloned().expect("a scan that is done");
            Ok((scan, done))
        }
        Some(ScanInfo { phase: Phase::Failed(message), .. }) => {
            if begun {
                forget_scan(root);
            }
            Err(message)
        }
        _ => Err(format!("The scan of {root} was stopped.")),
    }
}

/// Takes what moved to the Trash out of every done scan: its size comes down by it, and a scan of a folder that is in the Trash now is dropped.
pub fn remove_from_scans(items: &[Collected]) {
    for scan in summaries() {
        let Phase::Done(mut done) = scan.phase.clone() else { continue };
        let mut moved = 0;
        let mut root_gone = false;
        for item in items {
            if item.path == done.name {
                root_gone = true;
                break;
            }
            if inside(&item.path, &done.name) {
                moved += item.size;
            }
        }
        if root_gone {
            keep(&scan.root, None, false);
        } else if moved > 0 {
            done.size = done.size.saturating_sub(moved);
            keep(&scan.root, Some(Phase::Done(done)), false);
        }
    }
}

/// Works the hidden space of every done disk scan out again, from the disks as they are now.
pub async fn recompute_hidden() {
    for scan in summaries() {
        let Phase::Done(mut done) = scan.phase.clone() else { continue };
        let Some(mount) = done.disk.clone() else { continue };
        let Ok(hidden) = hidden_of(&mount, done.found).await else { continue };
        if scan_of(&scan.root).is_some_and(|now| now.at() == scan.at()) {
            done.hidden = hidden;
            keep(&scan.root, Some(Phase::Done(done)), true);
        }
    }
}
