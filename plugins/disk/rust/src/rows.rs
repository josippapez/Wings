//! The rows of the list beside the map.

use crate::{
    entry::{Entry, Kind, child_path},
    state::Collected,
};
use std::collections::HashSet;

/// An entry of the folder, one of its smaller items that `listAll` found, the row that stands for those, or the hidden space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Entry,
    Extra,
    Smaller,
    Hidden,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub key: String,
    pub kind: RowKind,
    /// An entry's path, or the folder a `smaller` row is in. Empty for the hidden space.
    pub path: String,
    pub name: String,
    pub size: u64,
    /// What a folder holds, or what the smaller items add up to.
    pub items: u64,
    pub entry_kind: Option<Kind>,
    /// A folder the map can go into.
    pub openable: bool,
    /// A folder Wings couldn't open.
    pub restricted: bool,
}

pub const HIDDEN_KEY: &str = "hidden";
pub fn smaller_key(folder: &str) -> String {
    format!("smaller:{folder}")
}
pub fn entry_key(path: &str) -> String {
    format!("entry:{path}")
}

/// What `listAll` is doing for a folder: one's smaller items, every entry in it.
#[derive(Clone, Debug, PartialEq)]
pub enum Extras {
    Loading,
    Ready(Vec<Entry>),
    Failed(String),
}

fn row_of(extra: bool, folder: &str, entry: &Entry) -> Row {
    let path = child_path(folder, &entry.name);
    Row {
        key: if extra { format!("extra:{path}") } else { entry_key(&path) },
        kind: if extra { RowKind::Extra } else { RowKind::Entry },
        path,
        name: entry.name.clone(),
        size: entry.size,
        items: entry.items,
        entry_kind: Some(entry.kind),
        openable: !extra && entry.kind == Kind::Dir && !entry.restricted,
        restricted: entry.restricted,
    }
}

/// The rows for the folder at `folder`: its entries, a row for what it folded into smaller items, the ones `listAll` found
/// under that row once it is open (the ones the folder doesn't already list, biggest first), and the hidden space.
pub fn rows_of(folder: &str, entry: &Entry, smaller_open: bool, extras: Option<&Extras>, hidden: Option<u64>) -> Vec<Row> {
    let mut rows: Vec<Row> = entry.children.iter().map(|child| row_of(false, folder, child)).collect();
    if let Some(smaller) = entry.smaller {
        rows.push(Row { key: smaller_key(folder), kind: RowKind::Smaller, path: folder.to_owned(), name: "smaller items".into(), size: smaller.size, items: smaller.items, entry_kind: None, openable: false, restricted: false });
        if let (true, Some(Extras::Ready(all))) = (smaller_open, extras) {
            let listed: HashSet<&str> = entry.children.iter().map(|c| c.name.as_str()).collect();
            let mut more: Vec<&Entry> = all.iter().filter(|e| !listed.contains(e.name.as_str())).collect();
            more.sort_by(|a, b| b.size.cmp(&a.size));
            rows.extend(more.into_iter().map(|e| row_of(true, folder, e)));
        }
    }
    if let Some(size) = hidden.filter(|size| *size > 0) {
        rows.push(Row { key: HIDDEN_KEY.into(), kind: RowKind::Hidden, path: String::new(), name: "Hidden space".into(), size, items: 0, entry_kind: None, openable: false, restricted: false });
    }
    rows
}

/// What goes in the Collector for a row, or none for a row that can't: a folder Wings couldn't open, the smaller items, the hidden space.
pub fn collected_of(row: &Row) -> Option<Collected> {
    match (row.kind, row.entry_kind) {
        (RowKind::Entry | RowKind::Extra, Some(kind)) if !row.restricted => Some(Collected { path: row.path.clone(), size: row.size, kind }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::Smaller;

    fn file(name: &str, size: u64) -> Entry {
        Entry { name: name.into(), size, kind: Kind::File, items: 0, children: vec![], smaller: None, restricted: false }
    }

    fn folder() -> Entry {
        let mail = Entry { name: "Mail".into(), size: 0, kind: Kind::Dir, items: 0, children: vec![], smaller: None, restricted: true };
        let code = Entry { name: "Code".into(), size: 600, kind: Kind::Dir, items: 3, children: vec![file("a.ts", 600)], smaller: None, restricted: false };
        Entry { name: "/Users/ada".into(), size: 1000, kind: Kind::Dir, items: 9, children: vec![code, file("notes.txt", 300), mail], smaller: Some(Smaller { size: 100, items: 5 }), restricted: false }
    }

    #[test]
    fn the_rows_are_the_entries_the_smaller_items_what_listall_found_under_them_and_the_hidden_space() {
        let all = Extras::Ready(vec![file("notes.txt", 300), file("tiny.log", 70), file("tinier.log", 30)]);
        let closed = rows_of("/Users/ada", &folder(), false, Some(&all), Some(50));
        assert_eq!(closed.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(), ["entry:/Users/ada/Code", "entry:/Users/ada/notes.txt", "entry:/Users/ada/Mail", "smaller:/Users/ada", "hidden"]);
        let open = rows_of("/Users/ada", &folder(), true, Some(&all), None);
        // The ones the folder lists already aren't there twice, and the biggest of the rest come first.
        assert_eq!(open.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(), ["entry:/Users/ada/Code", "entry:/Users/ada/notes.txt", "entry:/Users/ada/Mail", "smaller:/Users/ada", "extra:/Users/ada/tiny.log", "extra:/Users/ada/tinier.log"]);
        assert!(rows_of("/Users/ada", &folder(), true, Some(&Extras::Loading), None).iter().all(|r| r.kind != RowKind::Extra));
    }

    #[test]
    fn only_a_row_that_is_a_file_or_a_folder_wings_could_open_can_be_collected() {
        let rows = rows_of("/Users/ada", &folder(), false, None, Some(50));
        let collectable: Vec<String> = rows.iter().filter_map(collected_of).map(|c| c.path).collect();
        assert_eq!(collectable, ["/Users/ada/Code", "/Users/ada/notes.txt"]);
        assert_eq!([rows[0].openable, rows[1].openable, rows[2].openable], [true, false, false]);
    }
}
