//! The pictures in a transcript's entries, for a plugin that asks for `images: "register"`. A picture is megabytes of
//! base64 and a page has 64 MB, so the app keeps each one in its own store (`page::images`) and the entry says which
//! id to name in an `Image` node, as `{ "type": "image", "source": { "type": "registered", "id": "t..." } }`.

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// Takes each picture out of the entries, registers it for `plugin`'s pages, and leaves its id in its place. One the
/// store won't keep is left as `{ "type": "unavailable" }`. The second copy of a read picture that Claude Code writes
/// into `toolUseResult` goes too.
pub(super) fn register_in(plugin: &str, entries: &mut [Value]) {
    for entry in entries {
        if let Some(file) = entry.pointer_mut("/toolUseResult/file").and_then(Value::as_object_mut) {
            file.remove("base64");
        }
        if let Some(blocks) = entry.pointer_mut("/message/content").and_then(Value::as_array_mut) {
            swap(plugin, blocks);
        }
    }
}

fn swap(plugin: &str, blocks: &mut [Value]) {
    for block in blocks {
        match block["type"].as_str() {
            Some("image") => {
                let data = block["source"]["data"].as_str().unwrap_or_default();
                let id = id_of(data);
                *block = match register(plugin, &id, data) {
                    true => json!({ "type": "image", "source": { "type": "registered", "id": id } }),
                    false => json!({ "type": "image", "source": { "type": "unavailable" } }),
                };
            }
            Some("tool_result") => {
                if let Some(inner) = block.get_mut("content").and_then(Value::as_array_mut) {
                    swap(plugin, inner);
                }
            }
            _ => {}
        }
    }
}

/// The same picture always gets the same id, so a transcript read again names what it did before.
fn id_of(data: &str) -> String {
    let digest = Sha256::digest(data.as_bytes());
    format!("t{}", digest[..8].iter().map(|byte| format!("{byte:02x}")).collect::<String>())
}

#[cfg(feature = "ui")]
fn register(plugin: &str, id: &str, data: &str) -> bool {
    crate::page::images::register(plugin, id, data).is_ok()
}

#[cfg(not(feature = "ui"))]
fn register(_: &str, _: &str, _: &str) -> bool {
    false
}

/// `registerImage`: keeps a picture a page sends once, base64, under an id of its own, for its `Image` nodes.
pub(super) fn register_image(plugin: &str, params: &Value) -> Result<String, String> {
    let (Some(id), Some(data)) = (params.get("id").and_then(Value::as_str), params.get("data").and_then(Value::as_str)) else {
        return Err("registerImage takes an id and base64 data".into());
    };
    #[cfg(feature = "ui")]
    crate::page::images::register(plugin, id, data)?;
    #[cfg(not(feature = "ui"))]
    let _ = (plugin, id, data);
    Ok("null".into())
}

#[cfg(all(test, feature = "ui"))]
mod tests {
    use std::io::Cursor;

    use base64::{Engine, engine::general_purpose::STANDARD};

    use super::*;
    use crate::page::{
        images::{Shown, show},
        plugin::testing::Rig,
    };

    fn png(width: u32, height: u32) -> String {
        let mut bytes = Vec::new();
        image::RgbaImage::from_pixel(width, height, image::Rgba([40, 90, 200, 255])).write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png).unwrap();
        STANDARD.encode(bytes)
    }

    #[test]
    fn a_transcripts_pictures_are_kept_by_the_app_and_the_entries_name_their_ids_instead() {
        let (mine, read) = (png(20, 10), png(24, 12));
        let mut entries = vec![
            json!({"type":"user","message":{"role":"user","content":[
                {"type":"image","source":{"type":"base64","media_type":"image/png","data":mine}},
                {"type":"text","text":"what is this?"}]}}),
            json!({"type":"user","message":{"role":"user","content":[
                {"type":"tool_result","tool_use_id":"t1","content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":read}}]}]},
                "toolUseResult":{"type":"image","file":{"base64":read,"type":"image/png","originalSize":93}}}),
            json!({"type":"user","message":{"role":"user","content":[{"type":"image","source":{"type":"base64","data":"not a picture"}}]}}),
        ];
        register_in("pictures-transcript", &mut entries);
        let id = |entry: &Value| entry.pointer("/message/content/0/source/id").and_then(Value::as_str).map(str::to_owned);
        let mine_id = id(&entries[0]).expect("a registered id");
        assert_eq!(entries[0]["message"]["content"][0]["source"], json!({"type":"registered","id":mine_id}));
        assert_eq!(entries[0]["message"]["content"][1]["text"], "what is this?");
        let inner = &entries[1]["message"]["content"][0]["content"][0]["source"];
        assert_eq!(inner["type"], "registered");
        // No base64 of a picture is left in an entry, and the one that isn't a picture is said to be gone.
        assert!(!entries[1].to_string().contains(&read) && !entries[0].to_string().contains(&png(20, 10)));
        assert_eq!(entries[1]["toolUseResult"]["file"]["originalSize"], 93);
        assert_eq!(entries[2]["message"]["content"][0]["source"], json!({"type":"unavailable"}));
        // The store has them for this plugin's pages, and not for another's.
        assert!(!matches!(show("pictures-transcript", &mine_id, 64, None), Shown::Missing));
        assert!(matches!(show("someone-else", &mine_id, 64, None), Shown::Missing));
        // Read again, a picture has the id it had.
        let mut again = vec![json!({"type":"user","message":{"content":[{"type":"image","source":{"data":png(20, 10)}}]}})];
        register_in("pictures-transcript", &mut again);
        assert_eq!(id(&again[0]), Some(mine_id));
    }

    #[test]
    fn a_page_registers_a_picture_once_and_what_is_not_one_is_refused() {
        let rig = Rig::new(json!({}), Vec::new());
        rig.allows("registerImage", json!({"id":"i0","data":png(8, 8)}));
        assert!(!matches!(show("probe", "i0", 64, None), Shown::Missing));
        for refused in [
            json!({"id":"../etc/passwd","data":png(8, 8)}),
            json!({"id":"i1","data":"AAAA"}),
            json!({"id":"i1"}),
            json!({"data":png(8, 8)}),
        ] {
            rig.refuses("registerImage", refused);
        }
    }
}
