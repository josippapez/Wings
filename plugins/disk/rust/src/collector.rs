//! The Collector: what the user, or Claude, has gathered to move to the Trash. It is kept in `wings.storage`, so it is
//! still there after a restart, and told to every tab on each change.

use crate::{
    disks::disks_now,
    entry::Kind,
    paths::{inside, tidy},
    protect::protected_reason,
    scans::publish,
    state::{Collected, Collector},
};
use serde_json::Value;
use std::cell::RefCell;
use wings_plugin::host::storage;

const KEY: &str = "collector";

thread_local! {
    static ITEMS: RefCell<Option<Vec<Collected>>> = const { RefCell::new(None) };
}

/// What storage holds, as far as it is a list of collected items.
fn clean(stored: &Value) -> Vec<Collected> {
    stored.as_array().map_or_else(Vec::new, |list| list.iter().filter_map(Collected::from_value).collect())
}

/// The Collector, read from storage the first time.
pub fn now() -> Collector {
    ITEMS.with(|items| Collector { items: items.borrow_mut().get_or_insert_with(|| clean(&storage::get(KEY).unwrap_or(Value::Null))).clone() })
}

fn save(items: Vec<Collected>) -> Result<(), String> {
    storage::set(KEY, &Value::Array(items.iter().map(Collected::to_value).collect()))?;
    ITEMS.with(|kept| *kept.borrow_mut() = Some(items));
    publish();
    Ok(())
}

/// Adds `item` unless it, or a folder that holds it, is collected. A folder takes out what it holds.
fn add(items: &mut Vec<Collected>, item: Collected) -> bool {
    if items.iter().any(|c| c.path == item.path || (c.kind == Kind::Dir && inside(&item.path, &c.path))) {
        return false;
    }
    items.retain(|c| !inside(&c.path, &item.path));
    items.push(item);
    true
}

/// A path that may not be collected, with why.
pub struct Refusal {
    pub path: String,
    pub reason: String,
}

/// Adds what may be collected, and says what it added and what it refused, with why.
pub async fn collect(incoming: Vec<Collected>) -> Result<(Vec<Collected>, Vec<Refusal>), String> {
    let mounts: Vec<String> = disks_now(30_000.0).await?.rows.into_iter().map(|d| d.disk.mount).collect();
    let mut items = now().items;
    let (mut made, mut refused) = (Vec::new(), Vec::new());
    for Collected { path, size, kind } in incoming {
        match protected_reason(&path, &mounts) {
            Some(reason) => refused.push(Refusal { path, reason }),
            None => {
                let item = Collected { path: tidy(&path), size, kind };
                if add(&mut items, item.clone()) {
                    made.push(item);
                }
            }
        }
    }
    // A folder added later in the same call takes out what an earlier item of it added.
    made.retain(|item| items.contains(item));
    if !made.is_empty() {
        save(items)?;
    }
    Ok((made, refused))
}

/// Takes the items at `paths` out of the Collector.
pub fn uncollect(paths: &[String]) -> Result<(), String> {
    let gone: Vec<String> = paths.iter().map(|path| tidy(path)).collect();
    let mut items = now().items;
    items.retain(|c| !gone.contains(&c.path));
    save(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wings_plugin::{host, task::block_on};

    fn item(path: &str, kind: Kind) -> Collected {
        Collected { path: path.into(), size: 10, kind }
    }

    fn disks_and_nothing_stored() {
        host::testing::answer(|method, _| match method {
            "disks" => Ok(json!([{ "name": "Macintosh HD", "mount": "/", "fs": "apfs", "kind": "ssd", "removable": false, "readOnly": false, "total": 100, "free": 50 }])),
            _ => Ok(json!({ "code": 1, "stdout": "", "stderr": "" })),
        });
    }

    #[test]
    fn a_folder_takes_out_what_is_collected_inside_it_and_a_path_inside_a_collected_folder_is_left_as_it_is() {
        let mut items = vec![item("/Users/ada/Code/a", Kind::Dir), item("/Users/ada/Code/b.zip", Kind::File), item("/Users/ada/Movies/x.mov", Kind::File)];
        assert!(add(&mut items, item("/Users/ada/Code", Kind::Dir)));
        assert_eq!(items.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(), ["/Users/ada/Movies/x.mov", "/Users/ada/Code"]);
        assert!(!add(&mut items, item("/Users/ada/Code/c", Kind::Dir)));
        assert!(!add(&mut items, item("/Users/ada/Code", Kind::Dir)));
        // A file holds nothing.
        assert!(add(&mut items, item("/Users/ada/Movies/x.mov/y", Kind::File)));
    }

    #[test]
    fn collecting_refuses_what_may_not_be_collected_with_why_and_tidies_the_rest() {
        disks_and_nothing_stored();
        let (added, refused) = block_on(collect(vec![item("/Users/ada", Kind::Dir), item("/Users/ada/Downloads/old/", Kind::Dir), item("/Users/ada/Downloads/old/deeper", Kind::Dir)])).unwrap();
        assert_eq!(added.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(), ["/Users/ada/Downloads/old"]);
        assert_eq!(refused.iter().map(|r| (r.path.as_str(), r.reason.as_str())).collect::<Vec<_>>(), [("/Users/ada", "/Users/ada is a home folder.")]);
        assert_eq!(now().items.len(), 1);
        uncollect(&["/Users/ada/Downloads/old/".to_owned()]).unwrap();
        assert!(now().items.is_empty());
    }
}
