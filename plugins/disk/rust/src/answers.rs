//! What Claude's tools answer. They share main's scans and Collector, so what Claude scans shows in the Disk usage tab,
//! and what it collects waits there for the user.

use crate::{
    bytes::bytes,
    collector::{self, Refusal},
    df::Hidden,
    disks::{disks_now, load_disks},
    entry::{Entry, Kind, child_path},
    level::{Level, fetch},
    paths::tidy,
    protect::protected_reason,
    scans::{holders, resolve, scan_folder},
    state::{Collected, Done},
};
use serde_json::{Value, json};
use std::{cell::RefCell, rc::Rc};
use wings_plugin::{host, task::map_limit};

/// Up to this many entries go into one answer.
const NODES: usize = 300;
/// The most entries Wings is asked to keep of a folder an answer lists, and of one it looks for the biggest files in.
const LISTED: u64 = 3_000;
const FILES: u64 = 5_000;
/// A scan this old, at most, answers a tool.
const FRESH_MS: f64 = 30.0 * 60_000.0;

fn sized(n: u64) -> Value {
    json!({ "size": n, "sizeText": bytes(n) })
}

/// The fields of `extra`, added to the object `into`.
fn with(mut into: Value, extra: Value) -> Value {
    if let (Some(into), Value::Object(extra)) = (into.as_object_mut(), extra) {
        into.extend(extra);
    }
    into
}

fn node_of(e: &Entry) -> Value {
    let mut node = with(json!({ "name": e.name, "kind": e.kind.name(), "items": e.items }), sized(e.size));
    if e.restricted {
        node["restricted"] = true.into();
    }
    node
}

/// `root` as a node, with each folder's `limit` biggest entries under it, `depth` levels down. Folders open biggest first
/// until `NODES` entries are in, so a deep, wide tree can't make an answer too big to read: a folder not opened has no children.
pub fn listing(root: &Entry, depth: u64, limit: usize) -> Value {
    struct Out {
        node: Value,
        children: Vec<usize>,
    }
    let mut arena = vec![Out { node: node_of(root), children: Vec::new() }];
    let mut budget = NODES.max(limit);
    let mut queue: Vec<(&Entry, usize, u64)> = vec![(root, 0, 1)];
    while !queue.is_empty() {
        queue.sort_by(|a, b| b.0.size.cmp(&a.0.size));
        let (entry, at, level) = queue.remove(0);
        let shown = &entry.children[..entry.children.len().min(limit.min(budget))];
        budget -= shown.len();
        let first = arena.len();
        arena.extend(shown.iter().map(|child| Out { node: node_of(child), children: Vec::new() }));
        arena[at].children = (first..first + shown.len()).collect();
        let node = &mut arena[at].node;
        if entry.children.len() > shown.len() {
            let rest = &entry.children[shown.len()..];
            node["others"] = with(json!({ "count": rest.len() }), sized(rest.iter().map(|c| c.size).sum()));
        }
        if let Some(smaller) = entry.smaller {
            node["smaller"] = with(json!({ "items": smaller.items }), sized(smaller.size));
        }
        if level >= depth {
            continue;
        }
        for (i, child) in shown.iter().enumerate() {
            if child.kind == Kind::Dir {
                queue.push((child, first + i, level + 1));
            }
        }
    }
    fn build(arena: &[Out], at: usize) -> Value {
        let mut node = arena[at].node.clone();
        if !arena[at].children.is_empty() {
            node["children"] = arena[at].children.iter().map(|child| build(arena, *child)).collect();
        }
        node
    }
    build(&arena, 0)
}

fn hidden_view(h: &Hidden) -> Value {
    json!({
        "size": h.size, "sizeText": bytes(h.size),
        "purgeable": sized(h.purgeable),
        "otherVolumes": h.others.iter().map(|v| with(json!({ "name": v.name, "mount": v.mount }), sized(v.used))).collect::<Vec<_>>(),
        "snapshots": h.snapshots,
        "rest": sized(h.rest),
    })
}

/// Every file the tree keeps at or under `entry`, with its full path.
fn files_under(entry: &Entry, at: &str, out: &mut Vec<(String, u64)>) {
    if entry.kind == Kind::File {
        out.push((at.to_owned(), entry.size));
    }
    for child in &entry.children {
        files_under(child, &child_path(at, &child.name), out);
    }
}

/// `ms` since the epoch as an ISO 8601 time in UTC, like `JSON.stringify` writes a date.
fn iso(ms: f64) -> String {
    let (days, in_day) = ((ms / 86_400_000.0).floor() as i64, ms.rem_euclid(86_400_000.0) as i64);
    // A civil date from days since 1970 (Hinnant's algorithm).
    let z = days + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", in_day / 3_600_000, in_day / 60_000 % 60, in_day / 1000 % 60, in_day % 1000)
}

/// A folder or file an answer is about, and the scan it comes from.
struct Found {
    scan: Done,
    level: Level,
    path: String,
}

/// The entry at `path` for a tool, `depth` levels down: from a finished scan under 30 minutes old that holds it, else from a
/// scan of `path` itself, which then shows in the tab too. `fresh` always scans. The tree comes from Wings, which answers
/// from the scan that holds the folder, and walks it when none does.
async fn entry_at(path: &str, fresh: bool, depth: u64, most: u64) -> Result<Found, String> {
    let full = resolve(path);
    if let (Some(full), false) = (&full, fresh) {
        let mut held = holders(full, FRESH_MS);
        // The finest first: the scan whose root is nearest.
        held.sort_by(|a, b| b.1.name.len().cmp(&a.1.name.len()).then(b.1.at.total_cmp(&a.1.at)));
        if let Some((_, scan)) = held.into_iter().next() {
            let size = (scan.name == *full).then_some(scan.size);
            return Ok(Found { level: fetch(full, scan.min_bytes, size, depth, most).await?, scan, path: full.clone() });
        }
    }
    let (_, scan) = scan_folder(&tidy(path)).await?;
    let (name, size) = (scan.name.clone(), scan.size);
    Ok(Found { level: fetch(&name, scan.min_bytes, Some(size), depth, most).await?, scan, path: name })
}

pub async fn list_disks() -> Result<Value, String> {
    let rows = load_disks().await?.rows;
    Ok(rows
        .iter()
        .map(|d| {
            let volumes: Vec<Value> = d.others.iter().map(|v| json!({ "name": v.name, "mount": v.mount, "used": v.used, "usedText": bytes(v.used) })).collect();
            json!({
                "name": d.disk.name, "mount": d.disk.mount, "kind": d.disk.kind,
                "total": d.disk.total, "totalText": bytes(d.disk.total),
                "used": d.used, "usedText": bytes(d.used),
                "free": d.disk.free, "freeText": bytes(d.disk.free),
                "purgeable": d.purgeable, "purgeableText": bytes(d.purgeable),
                "others": volumes,
            })
        })
        .collect())
}

pub async fn folder_usage(input: Value) -> Result<Value, String> {
    let path = input["path"].as_str().unwrap_or_default();
    let (depth, limit) = (input["depth"].as_u64().unwrap_or(2), input["limit"].as_u64().unwrap_or(15) as usize);
    let Found { scan, level, path: full } = entry_at(path, input["fresh"] == true, depth, LISTED).await?;
    let top = listing(&level.tree, depth, limit);
    let mut answer = json!({ "path": full, "kind": top["kind"], "size": top["size"], "sizeText": top["sizeText"], "items": top["items"], "scannedAt": iso(scan.at) });
    for name in ["children", "others", "smaller"] {
        if !top[name].is_null() {
            answer[name] = top[name].clone();
        }
    }
    if let Some(hidden) = scan.hidden.as_ref().filter(|_| full == scan.name) {
        answer["hidden"] = hidden_view(hidden);
    }
    if scan.restricted > 0 {
        answer["notes"] = json!([format!("{} folders couldn't be opened, so their sizes aren't counted. Full Disk Access for Wings in System Settings shows them.", scan.restricted)]);
    }
    Ok(answer)
}

pub async fn largest_files(input: Value) -> Result<Value, String> {
    let limit = input["limit"].as_u64().unwrap_or(25) as usize;
    let Found { scan, level, path: full } = entry_at(input["path"].as_str().unwrap_or_default(), false, 64, FILES).await?;
    let mut files = Vec::new();
    files_under(&level.tree, &full, &mut files);
    files.sort_by(|a, b| b.1.cmp(&a.1));
    Ok(json!({
        "path": full,
        "scannedAt": iso(scan.at),
        "files": files.into_iter().take(limit).map(|(path, size)| with(json!({ "path": path }), sized(size))).collect::<Vec<_>>(),
        "note": format!("Files under {} aren't listed: the scan folded them into their folders.", bytes(level.floor)),
    }))
}

/// Why `raw` can't be collected, before anything is spent on measuring it: measuring `/` or a home folder is a long walk
/// for a refusal. A `~` path is judged as under a home folder while no scan has told which one is the user's.
fn screen(raw: &str, mounts: &[String]) -> Option<String> {
    if let Some(full) = resolve(raw) {
        return protected_reason(&full, mounts);
    }
    let stand = match raw.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("/Users/home{rest}"),
        _ => raw.to_owned(),
    };
    protected_reason(&stand, mounts).map(|reason| reason.replacen(&stand, raw, 1))
}

/// What `raw` is: from a scan that holds it, else measured now, which also gives its full path.
async fn measure(raw: &str) -> Result<Collected, String> {
    let full = resolve(raw);
    let answer = host::disk_usage(full.as_deref().unwrap_or(raw), &json!({ "depth": 0, "maxEntries": 1, "reuse": true }), None).await?;
    let root = Entry::from_value(&answer["root"]);
    Ok(Collected { path: root.name, size: root.size, kind: root.kind })
}

/// Whether `real`, the path Wings named, is the one `raw` reads as: a `~` path by what follows the `~`. macOS ignores case.
fn reads_as(raw: &str, real: &str) -> bool {
    let full = resolve(raw);
    let wanted = full.clone().unwrap_or_else(|| raw.strip_prefix('~').unwrap_or(raw).to_owned()).to_lowercase();
    if full.is_some() { real.to_lowercase() == wanted } else { real.to_lowercase().ends_with(&wanted) }
}

pub async fn collect(input: Value) -> Result<Value, String> {
    let paths: Vec<String> = input["paths"].as_array().map_or_else(Vec::new, |list| list.iter().filter_map(|p| p.as_str().map(str::to_owned)).collect());
    if paths.is_empty() {
        return Err("paths needs at least 1 path.".into());
    }
    let mounts: Vec<String> = disks_now(30_000.0).await?.rows.into_iter().map(|d| d.disk.mount).collect();
    let refused = Rc::new(RefCell::new(Vec::<Refusal>::new()));
    let measured = {
        let (refused, mounts) = (refused.clone(), Rc::new(mounts));
        map_limit(paths, 4, move |raw| {
            let (refused, mounts) = (refused.clone(), mounts.clone());
            async move {
                if let Some(reason) = screen(&raw, &mounts) {
                    refused.borrow_mut().push(Refusal { path: raw, reason });
                    return None;
                }
                match measure(&raw).await {
                    // Wings names the real path, with every link on the way followed, and the Trash would act on that one.
                    Ok(item) if !reads_as(&raw, &item.path) => {
                        let reason = format!("{raw} is a link or goes through one. Its real path is {}: collect that instead.", item.path);
                        refused.borrow_mut().push(Refusal { path: raw, reason });
                        None
                    }
                    Ok(item) => Some(item),
                    Err(error) => {
                        refused.borrow_mut().push(Refusal { path: raw.clone(), reason: format!("{raw} couldn't be measured: {error}") });
                        None
                    }
                }
            }
        })
        .await
    };
    let (added, more) = collector::collect(measured.into_iter().flatten().collect()).await?;
    refused.borrow_mut().extend(more);
    let now = collector::now();
    Ok(json!({
        "added": added.iter().map(|item| json!({ "path": item.path, "sizeText": bytes(item.size) })).collect::<Vec<_>>(),
        "refused": refused.borrow().iter().map(|r| json!({ "path": r.path, "reason": r.reason })).collect::<Vec<_>>(),
        "count": now.items.len(),
        "total": now.total(),
        "totalText": bytes(now.total()),
        "next": "The user reviews these in the Disk usage tab's Collector and moves them to the Trash from there.",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::Smaller;

    fn file(name: &str, size: u64) -> Entry {
        Entry { name: name.into(), size, kind: Kind::File, items: 0, children: vec![], smaller: None, restricted: false }
    }

    fn dir(name: &str, children: Vec<Entry>, smaller: Option<Smaller>) -> Entry {
        let mut children = children;
        children.sort_by(|a, b| b.size.cmp(&a.size));
        Entry { name: name.into(), size: children.iter().map(|c| c.size).sum::<u64>() + smaller.map_or(0, |s| s.size), kind: Kind::Dir, items: children.len() as u64, children, smaller, restricted: false }
    }

    /// A folder of 30 projects of a dozen files each, a big file and a note.
    fn tree() -> Entry {
        let projects: Vec<Entry> = (0..30u64)
            .map(|i| {
                let mut files: Vec<Entry> = (0..10).map(|n| file(&format!("f{n}"), 1)).collect();
                files.extend([file("a.zip", 1000 + i), dir("src", vec![file("b.rs", 10 + i)], None)]);
                dir(&format!("p{i:02}"), files, Some(Smaller { size: 5, items: 3 }))
            })
            .collect();
        dir("/Users/ada/Code", vec![dir("projects", projects, None), file("big.iso", 90_000), file("notes.md", 4)], Some(Smaller { size: 77, items: 12 }))
    }

    fn count(node: &Value) -> usize {
        1 + node["children"].as_array().map_or(0, |list| list.iter().map(count).sum())
    }

    #[test]
    fn a_listing_has_the_biggest_entries_of_each_folder_and_says_what_it_left_out() {
        let top = listing(&tree(), 2, 2);
        assert_eq!((top["name"].as_str(), top["kind"].as_str(), top["sizeText"].as_str()), (Some("/Users/ada/Code"), Some("dir"), Some("122 KB")));
        let names: Vec<&str> = top["children"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["big.iso", "projects"]);
        // `others` is what `limit` left out, and `smaller` is what the scan didn't keep one by one.
        assert_eq!(top["others"], json!({ "count": 1, "size": 4, "sizeText": "4 bytes" }));
        assert_eq!(top["smaller"], json!({ "items": 12, "size": 77, "sizeText": "77 bytes" }));
        let projects = &top["children"][1];
        assert_eq!(projects["children"].as_array().unwrap().len(), 2);
        assert_eq!(projects["others"]["count"], 28);
        // Two levels down is as far as it goes.
        assert!(projects["children"][0]["children"].is_null());
        assert!(top["children"][0]["children"].is_null());
    }

    #[test]
    fn a_deep_wide_listing_stops_at_the_nodes_budget_and_opens_the_biggest_folders_first() {
        let top = listing(&tree(), 4, 50);
        assert!(count(&top) <= NODES + 1, "{}", count(&top));
        // The budget ran out before the smallest folders were opened.
        let projects = top["children"][1]["children"].as_array().unwrap();
        assert_eq!(projects.len(), 30);
        assert!(projects[0]["children"].is_array() && projects[29]["children"].is_null());
    }

    #[test]
    fn a_time_is_written_the_way_json_writes_a_date() {
        assert_eq!(iso(0.0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso(1_794_477_600_123.0), "2026-11-12T10:00:00.123Z");
        assert_eq!(iso(951_782_400_000.0), "2000-02-29T00:00:00.000Z");
    }

    #[test]
    fn a_path_is_screened_before_it_is_measured_and_a_tilde_path_is_taken_for_one_under_a_home_folder() {
        let mounts = vec!["/".to_owned()];
        assert_eq!(screen("/Library", &mounts).as_deref(), Some("/Library is a system folder."));
        assert_eq!(screen("~", &mounts).as_deref(), Some("~ is a home folder."));
        assert_eq!(screen("~/Downloads", &mounts).as_deref(), Some("~/Downloads is one of the standard folders of a home folder."));
        assert_eq!(screen("~/Downloads/old.zip", &mounts), None);
        assert_eq!(screen("relative", &mounts).as_deref(), Some("relative isn't a full path."));
        // Wings names the real path, so a link on the way is told.
        assert!(reads_as("/private/tmp/x", "/Private/Tmp/X") && !reads_as("/tmp/x", "/private/tmp/x"));
        assert!(reads_as("~/Code/x", "/Users/ada/code/x") && !reads_as("~/Code/x", "/Users/ada/other/x"));
    }

    const DF: &str = "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s1s1 97656250 20000000 48828125 30% /\n/dev/disk3s6 97656250 5000000 48828125 10% /System/Volumes/VM\n";

    /// Wings with one startup disk of 100 GB, 50 GB free, and a home folder that scans as `home`. Gives what `diskUsage` was asked.
    fn wings_with(home: Value) -> Rc<RefCell<Vec<Value>>> {
        let asked = Rc::new(RefCell::new(Vec::new()));
        let log = asked.clone();
        host::testing::answer(move |method, params| match method {
            "diskUsage" if params["path"] != "/" => {
                log.borrow_mut().push(params.clone());
                Ok(json!({ "root": home.clone(), "floor": 1_048_576, "entries": 9, "restricted": 0, "ms": 5 }))
            }
            "disks" => Ok(json!([{ "name": "Macintosh HD", "mount": "/", "fs": "apfs", "kind": "ssd", "removable": false, "readOnly": false, "total": 100_000_000_000u64, "free": 50_000_000_000u64 }])),
            "exec" if params["program"] == "df" => Ok(json!({ "code": 0, "stdout": DF, "stderr": "" })),
            "exec" => Ok(json!({ "code": 0, "stdout": "2026-10-22-101010\nnot a date\n", "stderr": "" })),
            "diskUsage" if params["path"] == "/" => Ok(json!({ "root": { "name": "/", "size": 30_000_000_000u64, "kind": "dir", "items": 9 }, "floor": 1, "entries": 9, "restricted": 2, "ms": 5 })),
            other => Err(format!("no answer for {other}")),
        });
        asked
    }

    #[test]
    fn a_scan_of_a_disk_keeps_its_hidden_space_and_a_tool_answers_with_it() {
        wings_with(Value::Null);
        let answer = wings_plugin::task::block_on(async {
            crate::scans::scan_folder("/").await?;
            folder_usage(json!({ "path": "/" })).await
        })
        .unwrap();
        assert_eq!((answer["path"].as_str(), answer["size"].as_u64(), answer["scannedAt"].as_str()), (Some("/"), Some(30_000_000_000), Some("1970-01-01T00:00:00.000Z")));
        // Of the 50 GB the disk uses, the scan found 30; 5.1 GB of the rest is another volume, and the snapshot is listed.
        let hidden = &answer["hidden"];
        assert_eq!((hidden["size"].as_u64(), hidden["snapshots"][0].as_str()), (Some(20_000_000_000), Some("2026-10-22-101010")));
        assert_eq!(hidden["otherVolumes"][0]["mount"], "/System/Volumes/VM");
        assert!(answer["notes"][0].as_str().unwrap().starts_with("2 folders couldn't be opened"));
        let disks = wings_plugin::task::block_on(list_disks()).unwrap();
        assert_eq!((disks[0]["name"].as_str(), disks[0]["usedText"].as_str(), disks[0]["others"][0]["name"].as_str()), (Some("Macintosh HD"), Some("50 GB"), Some("VM")));
    }

    #[test]
    fn a_folder_inside_a_scan_is_asked_of_wings_by_its_full_path_and_a_path_it_cant_open_says_why() {
        let files = json!([{ "name": "app.bin", "size": 5000, "kind": "file", "items": 0 }, { "name": "old", "size": 40, "kind": "dir", "items": 1 }]);
        let asked = wings_with(json!({ "name": "/Users/ada", "size": 5040, "kind": "dir", "items": 3, "children": files }));
        let (big, biggest) = wings_plugin::task::block_on(async {
            crate::scans::scan_folder("~").await.unwrap();
            let big = folder_usage(json!({ "path": "~/Code", "depth": 1 })).await.unwrap();
            (big, largest_files(json!({ "path": "~/Code" })).await.unwrap())
        });
        // The scan of `~` named the home folder, so `~/Code` is a path in it, and the folder is asked of Wings: what the
        // scan found is kept there, down to what a folder of its size needs.
        assert_eq!((big["path"].as_str(), big["children"][0]["name"].as_str(), big["hidden"].is_null()), (Some("/Users/ada/Code"), Some("app.bin"), true));
        let asks = asked.borrow();
        assert_eq!((asks[1]["path"].as_str(), asks[1]["depth"].as_u64(), asks[1]["reuse"].clone()), (Some("/Users/ada/Code"), Some(1), json!(true)));
        // The folder holds 5 KB, which the scan's 1 MB floor hides: the answer says so, and it is asked for again at the floor its size needs.
        assert_eq!((asks[1]["minBytes"].as_u64(), asks[2]["minBytes"].as_u64()), (Some(1_048_576), Some(4096)));
        assert_eq!((asks[3]["depth"].as_u64(), asks[3]["maxEntries"].as_u64()), (Some(64), Some(FILES)));
        assert_eq!(biggest["files"][0], json!({ "path": "/Users/ada/Code/app.bin", "size": 5000, "sizeText": "5 KB" }));
        assert!(biggest["note"].as_str().unwrap().starts_with("Files under 1 MB"), "{}", biggest["note"]);
        host::testing::answer(|_, _| Err("There's no file or folder at /nope".into()));
        let missing = wings_plugin::task::block_on(folder_usage(json!({ "path": "/nope", "fresh": true }))).unwrap_err();
        assert_eq!(missing, "There's no file or folder at /nope");
    }

    #[test]
    fn the_biggest_files_are_every_file_the_tree_keeps_with_its_full_path() {
        let mut files = Vec::new();
        files_under(&tree(), "/Users/ada/Code", &mut files);
        files.sort_by(|a, b| b.1.cmp(&a.1));
        assert_eq!(files[0], ("/Users/ada/Code/big.iso".to_owned(), 90_000));
        assert_eq!(files[1], ("/Users/ada/Code/projects/p29/a.zip".to_owned(), 1029));
        assert_eq!(files.len(), 362);
    }
}
