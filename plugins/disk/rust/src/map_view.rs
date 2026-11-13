//! The map of a finished scan: the circle you click into, the list of what is in the folder beside it, and the Collector
//! under both. In the native app there is no zoom between folders, no dragging a wedge or row to the Collector (a row's
//! menu does it), no keyboard shortcuts, a list that draws its biggest rows and a button for more, and a list that
//! stays on the folder in the middle.

use crate::{
    bytes::{bytes, count, snapshot_label},
    df::Hidden,
    entry::{Kind, child_path, parent_path, parts_below},
    map_page::{Confirm, Countdown, FIRST_ROWS, Message, Page, SPLIT, cancel_trash, collect, confirmed, run, start_scan, start_trash, sync_level, toggle_smaller, with},
    rows::{Extras, Row, RowKind, collected_of, entry_key, smaller_key},
    state::{Collector, Done, ScanInfo},
    sunburst::{FADED, Target},
    widgets::{button, dismissible, gap, icon, menu, named_button, title},
};
use serde_json::{Value, json};
use wings_plugin::{
    format::basename,
    ui::{Node, confirm, dot, empty, heading, list_button, loading, muted, notice, page, row, slot, spinner, stack, text},
};

/// More folders than this and the ones in the middle go behind a menu.
const MAX_CRUMBS: usize = 4;
const NO_ACCESS: &str = "Wings can't open this folder. Allow Full Disk Access for Wings in System Settings, under Privacy & Security, to see it.";

fn toggle_menu(key: String) -> impl Fn() + 'static {
    move || with(|p| p.menu = if p.menu.as_deref() == Some(key.as_str()) { None } else { Some(key.clone()) })
}

fn close_menu() {
    with(|p| p.menu = None);
}

// ---------- the toolbar ----------

/// The folders from the scan's root down to `focus`, the root called `label`.
fn crumbs(done: &Done, label: &str, focus: &str) -> Vec<(String, String)> {
    let mut path = done.name.clone();
    let mut out = vec![(label.to_owned(), path.clone())];
    for part in parts_below(&done.name, focus).unwrap_or_default() {
        path = child_path(&path, &part);
        out.push((part, path.clone()));
    }
    out
}

fn crumb_row(p: &Page, crumbs: &[(String, String)]) -> Node {
    let long = crumbs.len() > MAX_CRUMBS;
    let folded: &[(String, String)] = if long { &crumbs[1..crumbs.len() - 2] } else { &[] };
    let shown: Vec<&(String, String)> = if long { [&crumbs[0]].into_iter().chain(&crumbs[crumbs.len() - 2..]).collect() } else { crumbs.iter().collect() };
    let last = crumbs.last().map(|(_, path)| path.as_str());
    let separator = || muted("›");
    gap(row(), "none").prop("grow", true).children(shown.into_iter().enumerate().map(|(i, (name, path))| {
        let item = if Some(path.as_str()) == last {
            text(name.as_str()).prop("weight", "strong").prop("truncate", true).prop("title", path.as_str())
        } else {
            let target = path.clone();
            button(name, "ghost", "sm", move || go_to(&target)).prop("title", path.as_str())
        };
        let between = (i == 1 && !folded.is_empty()).then(|| {
            let items: Vec<(String, Box<dyn Fn()>)> = folded
                .iter()
                .map(|(name, path)| {
                    let path = path.clone();
                    let go: Box<dyn Fn()> = Box::new(move || {
                        close_menu();
                        go_to(&path);
                    });
                    (name.clone(), go)
                })
                .collect();
            menu("…", "Folders in between", "ghost", "sm", p.menu.as_deref() == Some("crumbs"), toggle_menu("crumbs".into()), items)
        });
        gap(row(), "none").key(path.as_str()).maybe((i > 0).then(separator)).maybe(between.is_some().then(separator)).maybe(between).child(item)
    }))
}

/// Goes into the folder at `path`: one the map has shown before, or one below the one in the middle.
fn go_to(path: &str) {
    with(|p| {
        let size = p.sizes.get(path).copied();
        p.go(path.to_owned(), size);
    });
    sync_level();
}

fn walk(by: isize) {
    with(|p| {
        let at = p.trail.at.saturating_add_signed(by);
        p.walk(at);
    });
    sync_level();
}

fn toolbar(p: &Page, done: &Done, name: &str, focus: &str) -> Node {
    let (rescan, reveal) = (p.root.clone().unwrap_or_default(), focus.to_owned());
    let followed = p.trail.root == p.root;
    let actions = gap(row(), "s")
        .child(button("Rescan", "outline", "sm", move || start_scan(rescan.clone())))
        .child(button("Reveal in Finder", "outline", "sm", move || run("reveal", json!({ "path": reveal }))));
    let history = gap(row(), "none")
        .child(named_button("‹", "Back", "ghost", "icon-sm", || walk(-1)).prop("title", "Back").prop("disabled", !(followed && p.trail.at > 0)))
        .child(named_button("›", "Forward", "ghost", "icon-sm", || walk(1)).prop("title", "Forward").prop("disabled", !(followed && p.trail.at + 1 < p.trail.stack.len())));
    let left = Node::new("Slot")
        .prop("name", "children")
        .child(button("← Disks", "ghost", "sm", || with(|p| p.leave())))
        .child(history)
        .child(crumb_row(p, &crumbs(done, name, focus)));
    Node::new("Toolbar").child(left).child(slot("actions", actions))
}

// ---------- the map ----------

fn up() {
    let parent = with(|p| {
        let focus = p.focus_path()?;
        (focus != p.done()?.1.name).then(|| parent_path(&focus)).flatten()
    });
    if let Some(parent) = parent {
        go_to(&parent);
    }
}

/// A click on the wedge at `index` of the map.
fn wedge(index: usize, meta: bool) {
    let Some((segment, focus)) = with(|p| Some((p.shown.as_ref()?.layout.segments.get(index)?.clone(), p.focus_path()))) else { return };
    match segment.target {
        Target::Hidden => {
            if !meta {
                with(|p| p.hidden_open = true);
            }
        }
        Target::Smaller(folder) if meta => run("reveal", json!({ "path": folder })),
        Target::Smaller(folder) => {
            // Its row is in the folder's list, so go into the folder and open the row.
            if Some(&folder) != focus.as_ref() {
                go_to(&folder);
            }
            if !with(|p| p.smaller.contains(&folder)) {
                toggle_smaller(folder.clone());
            }
            with(|p| p.selected = Some(smaller_key(&folder)));
        }
        Target::Entry(path) if meta => run("reveal", json!({ "path": path })),
        Target::Entry(path) if segment.is_dir => {
            with(|p| p.go(path, Some(segment.size)));
            sync_level();
        }
        Target::Entry(path) => with(|p| p.selected = Some(entry_key(&path))),
    }
}

/// What the middle of the map says, as its title and detail: what the pointer is on, or else the folder in the middle.
fn words(p: &Page, name: &str, focus: &str, done: &Done, size: u64, items: u64) -> (String, String) {
    let sized = |size: u64, items: Option<u64>| items.map_or_else(|| bytes(size), |items| format!("{} · {}", bytes(size), count(items, "item")));
    let label_of = |path: &str| if path == done.name { name.to_owned() } else { basename(path).to_owned() };
    let segment = usize::try_from(p.pointed).ok().and_then(|at| p.shown.as_ref()?.layout.segments.get(at));
    if let Some(segment) = segment {
        let items = (matches!(segment.target, Target::Smaller(_)) || segment.is_dir).then_some(segment.items);
        return (segment.name.clone(), sized(segment.size, items));
    }
    if p.pointed == -2 && focus != done.name && let Some(up) = parent_path(focus) {
        return (format!("Up to {}", label_of(&up)), "Click to go up".into());
    }
    (label_of(focus), sized(size, Some(items)))
}

/// Whether the wedge for `target` is in the Collector, or inside a folder that is.
fn is_collected(collector: &Collector, target: &Target) -> bool {
    let (Target::Entry(path) | Target::Smaller(path)) = target else { return false };
    collector.items.iter().any(|item| *path == item.path || path.starts_with(&format!("{}/", item.path.trim_end_matches('/'))))
}

/// An angle to four places, which a wedge of a few pixels doesn't miss.
fn angle(a: f64) -> f64 {
    (a * 1e4).round() / 1e4
}

fn sunburst(p: &Page, name: &str, focus: &str, done: &Done) -> Node {
    let Some(shown) = &p.shown else { return stack() };
    let tree = &shown.level.tree;
    let (title_, detail) = words(p, name, focus, done, tree.size, tree.items);
    let wedges: Vec<Value> = shown.layout.segments.iter().map(|s| json!([angle(s.a0), angle(s.a1), s.ring, s.color, s.alpha * if is_collected(&p.collector, &s.target) { FADED } else { 1.0 }])).collect();
    let picked = p.selected.as_ref().and_then(|key| shown.rows.iter().find(|row| row.key == *key)).filter(|row| row.kind == RowKind::Entry).and_then(|row| shown.layout.by_path.get(&row.path));
    let map = Node::new("Sunburst")
        .prop("rings", shown.layout.rings.max(1))
        .prop("wedges", wedges)
        .prop("title", title_)
        .prop("detail", detail)
        .prop("up", focus != done.name)
        .on("onPoint", |event| with(|p| p.pointed = event["index"].as_i64().unwrap_or(-1)))
        .on("onSelect", |event| match event["index"].as_i64() {
            Some(-2) => up(),
            Some(index) if index >= 0 => wedge(index as usize, event["meta"] == true),
            _ => {}
        });
    stack().prop("grow", true).child(match picked {
        Some(picked) => map.prop("selected", *picked),
        None => map,
    })
}

// ---------- the list ----------

fn row_menu(p: &Page, row: &Row, collected: bool) -> Node {
    let files = matches!(row.kind, RowKind::Entry | RowKind::Extra);
    let mut items: Vec<(String, Box<dyn Fn()>)> = Vec::new();
    if let Some(item) = collected_of(row) {
        let toggle: Box<dyn Fn()> = Box::new(move || {
            close_menu();
            if collected { run("uncollect", json!({ "paths": [item.path] })) } else { collect(vec![item.clone()]) }
        });
        items.push((if collected { "Remove from Collector" } else { "Add to Collector" }.into(), toggle));
    }
    for (label, op, shown) in [("Reveal in Finder", "reveal", files), ("Quick Look", "preview", files && !row.restricted)] {
        if shown {
            let path = row.path.clone();
            let act: Box<dyn Fn()> = Box::new(move || {
                close_menu();
                run(op, json!({ "path": path }));
            });
            items.push((label.into(), act));
        }
    }
    if items.is_empty() {
        return stack().prop("gap", "none");
    }
    let key = format!("row:{}", row.key);
    menu("⋯", &format!("More for {}", row.name), "ghost", "icon-xs", p.menu.as_deref() == Some(key.as_str()), toggle_menu(key.clone()), items)
}

/// A click on a row: a folder is gone into, and the smaller items and the hidden space are opened.
fn open_row(row: &Row) {
    with(|p| p.selected = Some(row.key.clone()));
    match row.kind {
        RowKind::Entry if row.openable => {
            with(|p| p.go(row.path.clone(), Some(row.size)));
            sync_level();
        }
        RowKind::Smaller => toggle_smaller(row.path.clone()),
        RowKind::Hidden => with(|p| p.hidden_open = true),
        _ => {}
    }
}

fn entry_row(p: &Page, row: &Row, swatch: Option<u32>, loading: bool) -> Node {
    let collected = p.collector.items.iter().any(|item| item.path == row.path);
    let label = if row.kind == RowKind::Smaller { count(row.items, "smaller item") } else { row.name.clone() };
    let marker = match (row.restricted, swatch.filter(|_| row.entry_kind == Some(Kind::Dir))) {
        (true, _) => dot().prop("tone", "info"),
        (false, Some(color)) => dot().prop("color", color),
        (false, None) => dot().prop("tone", "muted"),
    };
    let trailing = if loading {
        spinner()
    } else if row.openable || matches!(row.kind, RowKind::Hidden | RowKind::Smaller) {
        icon("chevron-down")
    } else {
        // Where a row with nothing to open has its chevron, so its size lines up with the sizes above and below it.
        text("\u{2003}\u{2004}")
    };
    let size = if row.restricted { text("No access").prop("tone", "info").prop("size", "small") } else { muted(bytes(row.size)) };
    let clicked = row.clone();
    let mut main = list_button([marker, text(label).prop("grow", true).prop("truncate", true)].into_iter().chain(collected.then(|| icon("check"))).chain([size, trailing]), move || open_row(&clicked));
    if row.restricted {
        main = main.prop("title", NO_ACCESS);
    } else if collected {
        main = main.prop("title", format!("{} is in the Collector", row.name));
    }
    Node::new("ListItem").key(row.key.as_str()).child(main).child(row_menu(p, row, collected))
}

/// The colour of the wedge at a row's path, or the nearest one above it that the map draws.
fn swatch_of(p: &Page, row: &Row, focus: &str) -> Option<u32> {
    let shown = p.shown.as_ref()?;
    let mut path = Some(row.path.clone());
    while let Some(at) = path.filter(|at| at.len() >= focus.len()) {
        if let Some(index) = shown.layout.by_path.get(&at) {
            return Some(shown.layout.segments[*index].color);
        }
        path = parent_path(&at);
    }
    None
}

fn list_pane(p: &Page, name: &str, focus: &str, done: &Done) -> Node {
    let Some(shown) = &p.shown else { return stack() };
    let tree = &shown.level.tree;
    let folder = if focus == done.name { name.to_owned() } else { basename(focus).to_owned() };
    let open = p.smaller.contains(&tree.name);
    let extras = p.extras.get(&shown.key).filter(|_| open);
    let failed = match extras {
        Some(Extras::Failed(message)) => Some(muted(format!("Couldn't read the folder: {message}")).prop("tone", "danger").prop("size", "small")),
        _ => None,
    };
    let sub = format!("{}{}", bytes(tree.size), if tree.items > 0 { format!(", {}", count(tree.items, "item")) } else { String::new() });
    let head = gap(stack(), "xs").child(title(folder).prop("truncate", true)).child(gap(row(), "m").child(muted(sub).prop("size", "small"))).maybe(failed);
    let rows = &shown.rows;
    let drawn = rows.len().min(p.more_rows);
    let loading = matches!(extras, Some(Extras::Loading));
    let list = if rows.is_empty() {
        muted("Nothing in this folder.").prop("size", "small")
    } else {
        Node::new("List").children(rows[..drawn].iter().map(|r| entry_row(p, r, swatch_of(p, r, focus), r.kind == RowKind::Smaller && loading)))
    };
    let more = (rows.len() > drawn).then(|| button(&format!("Show more ({})", rows.len() - drawn), "ghost", "default", || with(|p| p.more_rows += FIRST_ROWS)));
    let hint = rows.iter().any(|r| r.restricted).then(|| muted("Folders marked No access need Full Disk Access for Wings. Allow it in System Settings, under Privacy & Security, to see them.").prop("size", "small"));
    stack().prop("grow", true).prop("gap", "none").child(Node::new("Bar").prop("edge", "bottom").prop("gap", "xs").child(head)).child(Node::new("Scroll").child(list).maybe(more).maybe(hint))
}

// ---------- the hidden space ----------

fn part(name: &str, note: Option<&str>, size: Option<u64>, action: Option<Node>) -> Node {
    let line = gap(row(), "m").child(dot().prop("tone", "muted")).child(text(name).prop("grow", true).prop("truncate", true)).maybe(size.map(|size| muted(bytes(size)))).maybe(action);
    Node::new("ListItem").key(name).child(gap(stack(), "xs").prop("grow", true).child(line).maybe(note.map(|note| muted(note).prop("size", "tiny"))))
}

fn confirm_card(p: &Page, title_: String, body: &str, label: &str, mount: &str) -> Node {
    let (busy, mount) = (p.confirm_busy, mount.to_owned());
    let shown_body = match &p.confirm_error {
        Some(error) => notice("danger", error.as_str()),
        None => muted(body).prop("size", "small"),
    };
    let action = button(label, "destructive", "default", move || confirmed(mount.clone())).prop("disabled", busy);
    let cancel = button("Cancel", "ghost", "default", || with(|p| p.confirm = None)).prop("disabled", busy);
    confirm(title_, [shown_body], [action, cancel])
}

fn hidden_panel(p: &Page, hidden: &Hidden, mount: &str) -> Node {
    let purgeable = (hidden.purgeable > 0).then(|| {
        let free = named_button("Free it", "Free the purgeable space", "outline", "xs", || with(|p| p.confirm = Some(Confirm::Purge)));
        let list = Node::new("List").child(part("Purgeable", Some("Caches and local snapshots that macOS can remove on its own"), Some(hidden.purgeable), Some(free)));
        let asked = (p.confirm == Some(Confirm::Purge)).then(|| {
            let body = format!("macOS decides what goes: caches and local snapshots it can make again. You may get back less than {}.", bytes(hidden.purgeable));
            confirm_card(p, "Free the purgeable space?".into(), &body, "Free it", mount)
        });
        gap(stack(), "s").child(list).maybe(asked)
    });
    let others = (!hidden.others.is_empty()).then(|| {
        gap(stack(), "xs").child(heading(2, "Other volumes")).child(Node::new("List").children(hidden.others.iter().map(|v| part(&v.name, Some(&v.mount), Some(v.used), None))))
    });
    let snapshots = (!hidden.snapshots.is_empty()).then(|| {
        let asked = match &p.confirm {
            Some(Confirm::Snapshot(date)) => Some(confirm_card(p, format!("Delete the snapshot of {}?", snapshot_label(date)), "It is deleted for good. Files that only it kept can't be restored.", "Delete snapshot", mount)),
            _ => None,
        };
        let list = Node::new("List").children(hidden.snapshots.iter().map(|date| {
            let (picked, label) = (date.clone(), snapshot_label(date));
            let delete = named_button("Delete", &format!("Delete the snapshot of {label}"), "outline", "xs", move || with(|p| p.confirm = Some(Confirm::Snapshot(picked.clone()))));
            part(&label, None, None, Some(delete))
        }));
        gap(stack(), "xs").child(heading(2, "Time Machine snapshots")).child(list).child(muted("Their space is part of the purgeable space.").prop("size", "tiny")).maybe(asked)
    });
    let rest = (hidden.rest > 0).then(|| Node::new("List").child(part("Still hidden", Some("Folders Wings can't open, other users' files and system data"), Some(hidden.rest), None)));
    let head = Node::new("Bar")
        .prop("edge", "bottom")
        .prop("gap", "xs")
        .child(button("← Back", "ghost", "xs", || with(|p| p.hidden_open = false)))
        .child(title("Hidden space"))
        .child(muted(bytes(hidden.size)).prop("size", "small"));
    stack().prop("grow", true).prop("gap", "none").child(head).child(Node::new("Scroll").child(gap(stack(), "m").maybe(purgeable).maybe(others).maybe(snapshots).maybe(rest)))
}

// ---------- the Collector ----------

fn message(m: &Message) -> Node {
    let id = m.id;
    dismissible(m.tone, &m.text, move || with(|p| p.messages.retain(|m| m.id != id))).key(id.to_string())
}

fn collector_bar(p: &Page) -> Node {
    let items = &p.collector.items;
    let (total, empty) = (p.collector.total(), items.is_empty());
    let fold = Node::new("Button")
        .prop("variant", "ghost")
        .prop("size", "icon-xs")
        .prop("label", "Show what is in the Collector")
        .on("onClick", |_| with(|p| p.collector_open = !p.collector_open))
        .child(icon("chevron-down"));
    let status = match p.countdown {
        Countdown::Counting(left) => text(format!("Moving {}, {}, to the Trash in {left} s", count(items.len() as u64, "item"), bytes(total))),
        Countdown::Moving => gap(row(), "s").child(spinner()).child(text("Moving to the Trash")),
        Countdown::Idle if empty => muted("Drag folders and files here"),
        Countdown::Idle => muted(format!("{}, {}", count(items.len() as u64, "item"), bytes(total))),
    };
    let act = match p.countdown {
        Countdown::Counting(_) => button("Cancel", "outline", "sm", cancel_trash),
        Countdown::Moving => Node::new("Button").prop("variant", "destructive").prop("size", "sm").prop("disabled", true).child(spinner()).text("Move to Trash"),
        Countdown::Idle => button("Move to Trash", "destructive", "sm", start_trash).prop("disabled", empty),
    };
    let list = p.collector_open.then(|| {
        if empty {
            return muted("Nothing yet. Drag folders and files here, or choose Add to Collector in a row's menu.").prop("size", "small");
        }
        Node::new("List").children(items.iter().map(|item| {
            let (reveal, remove, name) = (item.path.clone(), item.path.clone(), basename(&item.path).to_owned());
            Node::new("ListItem")
                .key(item.path.as_str())
                .child(gap(stack(), "none").prop("grow", true).child(text(name.as_str()).prop("truncate", true)).child(muted(parent_path(&item.path).unwrap_or_default()).prop("size", "tiny").prop("truncate", true)))
                .child(muted(bytes(item.size)))
                .child(named_button("Reveal", &format!("Reveal {name} in Finder"), "ghost", "xs", move || run("reveal", json!({ "path": reveal }))))
                .child(named_button("×", &format!("Remove {name} from the Collector"), "ghost", "icon-xs", move || run("uncollect", json!({ "paths": [remove] }))).prop("title", "Remove"))
        }))
    });
    Node::new("Bar")
        .prop("edge", "top")
        .child(gap(row(), "m").child(fold).child(title("Collector")).child(status).child(act))
        .maybe((!p.messages.is_empty()).then(|| gap(stack(), "s").children(p.messages.iter().map(message))))
        .maybe(list)
}

// ---------- the page ----------

/// What fills the page under the toolbar when the folder in the middle is here, is on its way, or couldn't be read.
fn content(p: &Page, name: &str, focus: &str, done: &Done) -> Node {
    let key = p.current_key();
    if p.shown.as_ref().is_some_and(|shown| Some(&shown.key) == key.as_ref()) {
        let start = match (p.hidden_open, &done.hidden, &done.disk) {
            (true, Some(hidden), Some(mount)) if focus == done.name => hidden_panel(p, hidden, mount),
            _ => list_pane(p, name, focus, done),
        };
        return Node::new("Split").prop("defaultSize", SPLIT).child(slot("start", stack().prop("grow", true).prop("gap", "none").child(start))).child(slot("end", sunburst(p, name, focus, done)));
    }
    match &p.level_error {
        Some((failed, error)) if Some(failed) == key.as_ref() => {
            let retry = button("Try again", "default", "default", || {
                with(|p| p.level_error = None);
                sync_level();
            });
            stack().prop("grow", true).child(empty(&format!("Couldn't read {}", basename(focus)), error).child(slot("children", retry)))
        }
        _ => stack().prop("grow", true).child(loading().text("Adding detail…")),
    }
}

pub fn mapped(p: &Page, _scan: &ScanInfo, done: &Done, name: &str) -> Vec<Node> {
    let focus = p.focus_path().unwrap_or_else(|| done.name.clone());
    let error = p.action_error.as_ref().map(|error| Node::new("Bar").prop("edge", "bottom").child(dismissible("danger", error, || with(|p| p.action_error = None))));
    let body = error.into_iter().chain([content(p, name, &focus, done), collector_bar(p)]);
    vec![page(toolbar(p, done, name, &focus), body).prop("flush", true)]
}
