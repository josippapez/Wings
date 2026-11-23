//! Pictures in the conversation: the ones you attach, the ones a tool gives back (Read of an image, a screenshot, an MCP
//! tool) and any in Claude's own content. Their bytes go to Wings once, under an id, and the conversation keeps only that
//! id (`host::register_image`, shown by `ui::image`), so a screenshot-heavy session holds no megabytes here and the page
//! never sends them again. A transcript's pictures are kept by Wings itself, which says their ids.

use std::{cell::RefCell, collections::VecDeque};

use serde_json::Value;
use wings_plugin::host;

/// How many of the newest pictures still show. Older ones give way to a note: Wings keeps the pictures it is given
/// within a budget too, and decodes and draws only what a page shows.
pub const SHOWN: usize = 12;
/// The most pictures one tool result shows.
pub const PER_RESULT: usize = 6;
/// The id of a picture that can't show: one Wings wouldn't keep, or too big to send.
const GONE: &str = "gone";
/// The longest base64 text of a picture Wings keeps (5 MB of bytes).
const MOST: usize = 5 * 1024 * 1024 / 3 * 4 + 4;

thread_local! {
    /// The pictures heard and not yet sent to Wings, by id.
    static PENDING: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
    /// The ids of the newest pictures, oldest first.
    static RECENT: RefCell<VecDeque<String>> = const { RefCell::new(VecDeque::new()) };
}

/// The id to show the picture `data` (base64) by. It goes to Wings at the next `flush`. Every chat tab registers into the
/// plugin's one store, so the id comes from the picture, as Wings names a transcript's: ids counted from 1 in each tab
/// had a chat's first picture replace the first picture of every other chat.
pub fn keep(data: &str) -> String {
    if data.len() > MOST {
        return GONE.to_owned();
    }
    let id = format!("p{:016x}", fnv(data.as_bytes()));
    PENDING.with(|pending| pending.borrow_mut().push((id.clone(), data.to_owned())));
    remember(&id);
    id
}

/// The id of the picture in an `image` content block: its base64 is kept for Wings, and one a transcript read already
/// registered (`source.type` `registered`) names its id. A picture Wings wouldn't keep is `GONE`.
pub fn of_block(block: &Value) -> Option<String> {
    let source = &block["source"];
    match source["type"].as_str()? {
        "base64" => Some(source["data"].as_str().map_or_else(|| GONE.to_owned(), keep)),
        "registered" => source["id"].as_str().inspect(|id| remember(id)).map(str::to_owned),
        _ => Some(GONE.to_owned()),
    }
}

/// Sends Wings the pictures heard since the last call.
pub fn flush() {
    for (id, data) in PENDING.with(|pending| std::mem::take(&mut *pending.borrow_mut())) {
        host::register_image(&id, &data);
    }
}

/// Whether the picture is one of the newest `SHOWN`, which the page shows.
pub fn is_shown(id: &str) -> bool {
    RECENT.with(|recent| recent.borrow().iter().any(|shown| shown == id))
}

/// FNV-1a, 64 bits: enough to tell pictures apart, with no hashing crate in the module.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3))
}

fn remember(id: &str) {
    RECENT.with(|recent| {
        let mut recent = recent.borrow_mut();
        recent.push_back(id.to_owned());
        while recent.len() > SHOWN {
            recent.pop_front();
        }
    });
}

/// What a test starts from: nothing heard yet.
#[cfg(test)]
pub fn reset() {
    RECENT.with(|recent| recent.borrow_mut().clear());
    PENDING.with(|pending| pending.borrow_mut().clear());
}

/// The pictures heard and not yet sent to Wings.
#[cfg(test)]
pub fn pending() -> Vec<(String, String)> {
    PENDING.with(|pending| pending.borrow().clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture's id names that picture, whatever was kept before it, so the first picture of two chat tabs (each its
    /// own count before) never share an id in the plugin's one store.
    #[test]
    fn a_pictures_id_comes_from_the_picture_not_from_how_many_came_before() {
        reset();
        let first = keep("aGVsbG8=");
        let other = keep("d29ybGQ=");
        assert_ne!(first, other);
        assert_eq!(keep("aGVsbG8="), first, "the same picture kept again, after another, has the same id");
        reset();
    }
}
