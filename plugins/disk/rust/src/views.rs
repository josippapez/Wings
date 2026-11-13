//! What the tab shows in place of the map: the disks and folders to scan, a scan that runs, one that failed, and one main no
//! longer has. Each is a card in the middle of the page, under the same toolbar: the way back to the disks, and the scan's root.

use crate::{
    bytes::{bytes, duration},
    df::DiskRow,
    map_page::{Page, star, start_scan, stop_scan, sync_level, tell_main, with},
    map_view::mapped,
    state::Phase,
    widgets::{button, dismissible, gap, named_button, section, stats, title},
};
use serde_json::json;
use wings_plugin::{
    format::{ago, basename},
    host,
    ui::{Node, badge, card, empty, gauge, loading, muted, notice, page, row, slot, spinner, stack, text, text_input, toolbar},
};

/// What a scan of `root` is called: the disk's name for a whole disk, the home folder, or the folder's own name.
pub fn root_name(root: &str, disks: Option<&Vec<DiskRow>>) -> String {
    match disks.and_then(|disks| disks.iter().find(|d| d.disk.mount == root)) {
        Some(disk) => disk.disk.name.clone(),
        None if root == "~" => "Home folder".into(),
        None => basename(root).to_owned(),
    }
}

pub fn view() -> Vec<Node> {
    with(|p| {
        p.refresh_shown();
        build(p)
    })
}

fn build(p: &Page) -> Vec<Node> {
    let Some(root) = p.root.as_deref() else { return overview(p) };
    let name = root_name(root, p.disks.as_ref().and_then(|d| d.as_ref().ok()));
    let Some(scan) = p.scan(root) else {
        // Main says there is none once it has told the scans; before that, or just after a scan was asked for, one is on its way.
        return if p.scans.is_some() && p.starting.as_deref() != Some(root) { no_scan(&name, root) } else { scanning(&name, None, root) };
    };
    match &scan.phase {
        Phase::Scanning { .. } => scanning(&name, Some(&scan.phase), root),
        Phase::Failed(message) => scan_error(&name, message, root),
        Phase::Done(done) => mapped(p, scan, done, &name),
    }
}

// ---------- the disks ----------

fn disk_card(disk: &DiskRow, startup: bool) -> Node {
    let total = disk.disk.total;
    let percent = if total > 0 { (disk.used as f64 / total as f64 * 100.0).round() } else { 0.0 };
    let purgeable = if disk.purgeable > 0 { format!(", includes {} purgeable", bytes(disk.purgeable)) } else { String::new() };
    let mount = disk.disk.mount.clone();
    let name = disk.disk.name.clone();
    let heading = gap(row(), "s")
        .prop("grow", true)
        .child(title(name.as_str()).prop("truncate", true))
        .maybe((disk.disk.kind != "unknown").then(|| badge_of(if disk.disk.kind == "ssd" { "SSD" } else { "HDD" }, "secondary")))
        .maybe(disk.disk.removable.then(|| badge_of("Removable", "outline")));
    let scan = named_button("Scan", &format!("Scan {name}"), if startup { "default" } else { "secondary" }, "sm", move || start_scan(mount.clone()));
    card()
        .key(disk.disk.mount.clone())
        .child(gap(row(), "m").child(heading).child(scan))
        .child(gauge(format!("{} used", bytes(disk.used)), percent, (75, 90), Some(format!("{} free of {}{purgeable}", bytes(disk.disk.free), bytes(total)))))
}

fn badge_of(label: &str, variant: &str) -> Node {
    badge(label, None).prop("variant", variant).prop("size", "sm")
}

/// The folders to scan: the home folder, each project's and the ones starred.
fn places(p: &Page) -> Vec<(String, String)> {
    let mut places = vec![("Home folder".to_owned(), "~".to_owned())];
    for (name, path) in p.projects.iter().cloned().chain(p.starred.iter().map(|path| (basename(path).to_owned(), path.clone()))) {
        if places.iter().all(|(_, known)| *known != path) {
            places.push((name, path));
        }
    }
    places
}

fn place_row(name: &str, path: &str, starred: bool) -> Node {
    let (star_path, scan_path) = (path.to_owned(), path.to_owned());
    let star_button = named_button(if starred { "★" } else { "☆" }, &format!("Star {name}"), "ghost", "icon-sm", move || star(&star_path, !starred)).prop("title", if starred { "Remove the star" } else { "Star this folder" });
    Node::new("ListItem")
        .key(path)
        .child(gap(stack(), "none").prop("grow", true).child(text(name).prop("weight", "medium").prop("truncate", true)).child(muted(path).prop("size", "small").prop("truncate", true)))
        .child(star_button)
        .child(named_button("Scan", &format!("Scan {name}"), "secondary", "sm", move || start_scan(scan_path.clone())))
}

fn submit_scan() {
    let root = with(|p| {
        let root = p.scan_text.trim().to_owned();
        if !root.starts_with('/') && !root.starts_with('~') {
            p.scan_error = Some("Use a full path, like ~/Downloads or /Users/you/Downloads.".into());
            return None;
        }
        p.scan_error = None;
        p.scan_text.clear();
        Some(root)
    });
    if let Some(root) = root {
        start_scan(root);
    }
}

/// Scan a folder by its path. A path has to be full or start with `~`.
fn scan_folder(p: &Page) -> Node {
    let field = text_input("Scan a folder", &p.scan_text, |text| with(|p| p.scan_text = text)).prop("placeholder", "~/Downloads");
    let labelled = gap(stack(), "s")
        .child(row().child(text("Scan a folder").prop("weight", "medium").prop("size", "field")).child(stack().prop("grow", true).child(field)))
        .maybe(p.scan_error.as_ref().map(|error| text(error.as_str()).prop("size", "field").prop("tone", "danger")));
    let scan = button("Scan", "secondary", "default", submit_scan).prop("disabled", p.scan_text.trim().is_empty());
    gap(row(), "m").prop("align", "start").child(labelled).child(scan)
}

fn scan_row(scan: &crate::state::ScanInfo, disks: Option<&Vec<DiskRow>>) -> Node {
    let name = root_name(&scan.root, disks);
    let size = match &scan.phase {
        Phase::Done(done) => bytes(done.size),
        Phase::Scanning { .. } => "Scanning…".into(),
        Phase::Failed(_) => "Failed".into(),
    };
    let (open_root, forget_root) = (scan.root.clone(), scan.root.clone());
    Node::new("ListItem")
        .key(scan.root.clone())
        .child(gap(stack(), "none").prop("grow", true).child(text(name.as_str()).prop("weight", "medium").prop("truncate", true)).child(muted(scan.root.as_str()).prop("size", "small").prop("truncate", true)))
        .child(text(size).prop("tone", if matches!(scan.phase, Phase::Failed(_)) { "danger" } else { "muted" }))
        .child(muted(ago(scan.at(), host::now())).prop("size", "small"))
        .child(named_button("Open", &format!("Open the scan of {name}"), "secondary", "sm", move || {
            with(|p| {
                p.root = Some(open_root.clone());
                p.focus = None;
            });
            sync_level();
        }))
        .child(named_button("Forget", &format!("Forget the scan of {name}"), "ghost", "sm", move || tell_main("forget", json!({ "root": forget_root }))))
}

fn overview(p: &Page) -> Vec<Node> {
    let disks = match &p.disks {
        Some(Err(error)) => vec![notice("danger", error.as_str())],
        None => vec![loading().text("Looking for disks")],
        Some(Ok(rows)) if rows.is_empty() => vec![muted("Wings found no disks.")],
        Some(Ok(rows)) => rows.iter().enumerate().map(|(i, disk)| disk_card(disk, i == 0)).collect(),
    };
    let places = places(p);
    let folders = card()
        .child(Node::new("List").children(places.iter().map(|(name, path)| place_row(name, path, p.starred.contains(path)))))
        .child(scan_folder(p));
    let rows = p.disks.as_ref().and_then(|d| d.as_ref().ok());
    let scans = p.scans.as_deref().filter(|scans| !scans.is_empty()).map(|scans| {
        section("Scans", vec![card().child(Node::new("List").children(scans.iter().map(|scan| scan_row(scan, rows))))])
    });
    let content = gap(stack(), "l")
        .prop("maxWidth", 860)
        .maybe(p.problem.as_ref().map(|problem| dismissible("danger", problem, || with(|p| p.problem = None))))
        .child(section("Disks", disks))
        .child(section("Folders", vec![folders]))
        .maybe(scans);
    vec![page(toolbar("Disk usage", None), [content])]
}

// ---------- a scan that isn't a map ----------

fn frame(name: &str, actions: Option<Node>, body: Node) -> Vec<Node> {
    let back = button("← Disks", "ghost", "sm", || with(|p| p.leave()));
    let bar = Node::new("Toolbar")
        .child(Node::new("Slot").prop("name", "children").child(back).child(title(name).prop("truncate", true)))
        .maybe(actions.map(|actions| slot("actions", actions)));
    vec![page(bar, [stack().prop("grow", true).child(body)])]
}

fn scanning(name: &str, phase: Option<&Phase>, root: &str) -> Vec<Node> {
    let stop_root = root.to_owned();
    let stop = button("Stop", "outline", "sm", move || stop_scan(stop_root.clone()));
    let expected = match phase {
        Some(Phase::Scanning { expected, .. }) => expected.filter(|expected| *expected > 0),
        _ => None,
    };
    let head = gap(row(), "m").child(title(format!("Scanning {name}")).prop("truncate", true)).maybe(expected.is_none().then(|| spinner().prop("label", "Scanning")));
    let body = match phase {
        Some(Phase::Scanning { progress, started_at, .. }) => {
            let found = progress.bytes;
            gap(stack(), "m")
                .child(head)
                .maybe(expected.map(|expected| gauge("Progress", ((found as f64 / expected as f64 * 100.0).round()).min(100.0), (75, 90), Some(format!("Expecting about {}", bytes(expected))))))
                .child(stats(vec![
                    ("Files scanned", wings_plugin::format::grouped(progress.entries)),
                    ("Found so far", bytes(found)),
                    ("Elapsed", duration(host::now() - started_at)),
                ]))
                .child(muted(if progress.path.is_empty() { " " } else { progress.path.as_str() }).prop("size", "small").prop("mono", true).prop("truncate", true))
        }
        _ => gap(stack(), "m").child(head).child(muted("Starting the scan")),
    };
    frame(name, Some(stop), card().child(body))
}

fn scan_error(name: &str, message: &str, root: &str) -> Vec<Node> {
    let retry_root = root.to_owned();
    let body = gap(stack(), "m")
        .child(title(format!("Couldn't scan {name}")).prop("truncate", true))
        .child(notice("danger", message))
        .child(gap(row(), "s").child(button("Try again", "default", "default", move || start_scan(retry_root.clone()))));
    frame(name, None, card().child(body))
}

/// The tab was left on a scan and main has lost it, like after Wings restarted.
fn no_scan(name: &str, root: &str) -> Vec<Node> {
    let again = root.to_owned();
    let content = empty(&format!("No results for {name}"), "Wings doesn't have this scan any more. Scan it again to see what takes the space.").child(slot("children", button("Scan again", "default", "default", move || start_scan(again.clone()))));
    frame(name, None, content)
}
