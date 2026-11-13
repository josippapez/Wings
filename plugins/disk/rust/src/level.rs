//! A folder of a finished scan, asked of Wings one at a time. Wings keeps the scans' trees, so neither the tab nor main
//! holds one: the tab asks for the folder in the middle of the map down as many rings as it draws, and a tool asks for
//! the folder it answers about. Wings answers from the scan that holds the folder, and without what is gone since.

use crate::entry::Entry;
use serde_json::json;
use wings_plugin::host;

/// The most entries one folder's answer keeps, so a turn never walks more than a few thousand of them.
pub const MOST: u64 = 6_000;

/// The least size Wings keeps when it scans a folder of `size` bytes alone: about 40,000 entries' worth, and nothing under 4 KB.
pub fn detail_for(size: u64) -> u64 {
    (size as f64 / 40_000.0).round().max(4096.0) as u64
}

/// The least size to ask for a folder of `size` bytes in a scan that kept `scan_min`: the scan's own, or a finer one when
/// the scan's hides most of the folder and a scan of the folder alone would keep more.
pub fn floor_for(scan_min: u64, size: u64) -> u64 {
    if scan_min.saturating_mul(2000) > size && detail_for(size) < scan_min { detail_for(size) } else { scan_min }
}

/// A folder with what is in it down `depth` levels, and the size under which what is in it isn't kept one by one.
pub struct Level {
    pub tree: Entry,
    pub floor: u64,
}

/// The folder at `path` of a scan that kept `scan_min`, `depth` levels down and at most `most` entries. With `size`, what
/// the folder holds when it is known, the first answer is as fine as the folder needs; without, one answer tells it, and
/// a folder that needs more is asked for again.
pub async fn fetch(path: &str, scan_min: u64, size: Option<u64>, depth: u64, most: u64) -> Result<Level, String> {
    let ask = |min_bytes: u64| async move {
        let answer = host::disk_usage(path, &json!({ "minBytes": min_bytes, "depth": depth, "maxEntries": most, "reuse": true }), None).await?;
        Ok::<_, String>(Level { tree: Entry::from_value(&answer["root"]), floor: answer["floor"].as_u64().unwrap_or(0) })
    };
    let first = size.map_or(scan_min, |size| floor_for(scan_min, size));
    let level = ask(first).await?;
    let finer = floor_for(scan_min, level.tree.size);
    if size.is_none() && finer < first { ask(finer).await } else { Ok(level) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wings_plugin::task::block_on;

    // A scan of a disk kept entries of 17 MB and up. The Users folder is looked at again at its own floor.
    const SCAN: u64 = 17_000_000;

    #[test]
    fn a_folder_is_asked_for_more_finely_when_the_floor_hides_most_of_it_and_a_scan_of_it_alone_would_keep_more() {
        assert_eq!(floor_for(SCAN, 5_000_000_000), detail_for(5_000_000_000));
        assert_eq!(detail_for(5_000_000_000), 125_000);
        // The floor is small next to a folder of 100 GB.
        assert_eq!(floor_for(SCAN, 100_000_000_000), SCAN);
        // 4 KB is the least Wings keeps, and it is what the scan kept a small folder to.
        assert_eq!(floor_for(4096, 1_000_000), 4096);
        assert_eq!(detail_for(1_000), 4096);
    }

    #[test]
    fn a_folder_whose_size_isnt_known_is_asked_for_again_when_its_answer_says_it_needs_more() {
        let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let log = asked.clone();
        host::testing::answer(move |method, params| {
            assert_eq!(method, "diskUsage");
            log.borrow_mut().push((params["minBytes"].as_u64().unwrap(), params["depth"].as_u64().unwrap(), params["reuse"].clone()));
            Ok(json!({ "root": { "name": "/Users/ada/tmp", "size": 5_000_000_000u64, "kind": "dir", "items": 2, "children": [{ "name": "a", "size": 4_000_000_000u64, "kind": "file", "items": 0 }] }, "floor": 17_000_000 }))
        });
        let level = block_on(fetch("/Users/ada/tmp", SCAN, None, 6, MOST)).unwrap();
        assert_eq!(level.tree.children.len(), 1);
        assert_eq!(*asked.borrow(), [(SCAN, 6, json!(true)), (125_000, 6, json!(true))]);
        // With its size known, once.
        asked.borrow_mut().clear();
        block_on(fetch("/Users/ada/tmp", SCAN, Some(5_000_000_000), 6, MOST)).unwrap();
        assert_eq!(*asked.borrow(), [(125_000, 6, json!(true))]);
    }
}
