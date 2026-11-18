//! The pictures a page is handed from the system's file picker (`wings.pickFiles`), as a paste hands one: each png, jpeg,
//! gif or webp, as its media type and base64.

use std::path::PathBuf;

use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

/// The most bytes of one picture a page is handed, picked or pasted. The Claude API takes an image of up to 5 MB as
/// base64 on its strictest platform, and base64 is a third bigger than the bytes, so this is the bytes that still fit.
pub const MAX_IMAGE_BYTES: usize = 3_750_000;
/// The most bytes of pictures one pick hands over. A page has 64 MB, and holds each picture a few times over on its way
/// to Claude: a pick of many photos would stop it.
const MAX_PICKED_BYTES: usize = 2 * MAX_IMAGE_BYTES;

/// `[{ mediaType, data }]` of the files at `paths` that are pictures Claude reads, in order, until they add up to
/// `MAX_PICKED_BYTES`. One that isn't, is over `MAX_IMAGE_BYTES` or can't be read is left out.
pub fn picked_images(paths: &[PathBuf]) -> String {
    let mut total = 0;
    let mut images = Vec::new();
    for path in paths {
        // A big file is passed over without being read.
        if !std::fs::metadata(path).is_ok_and(|meta| meta.len() <= MAX_IMAGE_BYTES as u64) {
            continue;
        }
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Some(media_type) = media_type(&bytes) else {
            continue;
        };
        if bytes.len() > MAX_IMAGE_BYTES || total + bytes.len() > MAX_PICKED_BYTES {
            continue;
        }
        total += bytes.len();
        images.push(json!({ "mediaType": media_type, "data": STANDARD.encode(&bytes) }));
    }
    Value::Array(images).to_string()
}

/// What a file's first bytes say it is, of the pictures Claude reads.
fn media_type(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, ..] => Some("image/png"),
        [0xff, 0xd8, 0xff, ..] => Some("image/jpeg"),
        [b'G', b'I', b'F', b'8', b'7' | b'9', b'a', ..] => Some("image/gif"),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some("image/webp"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pictures_picked_come_by_their_bytes_and_no_more_than_two_full_ones() {
        let folder = std::env::temp_dir().join(format!("wings-native-pick-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = |name: &str, bytes: &[u8]| {
            let path = folder.join(name);
            std::fs::write(&path, bytes).unwrap();
            path
        };
        let jpeg = |zeros: usize| [b"\xff\xd8\xff".as_slice(), &vec![0u8; zeros]].concat();
        let picked = |paths: &[PathBuf]| -> Value {
            serde_json::from_str(&picked_images(paths)).unwrap()
        };
        // Named for what they aren't: the bytes decide.
        let png = file("a.jpg", b"\x89PNG\r\n\x1a\nrest");
        let webp = file("b.png", b"RIFF\x04\x00\x00\x00WEBPVP8 ");
        let text = file("c.png", b"not a picture");
        let too_big = file("d.jpg", &jpeg(MAX_IMAGE_BYTES));
        let gone = folder.join("gone.png");
        assert_eq!(
            picked(&[png, webp, text, too_big, gone]),
            json!([
                { "mediaType": "image/png", "data": "iVBORw0KGgpyZXN0" },
                { "mediaType": "image/webp", "data": "UklGRgQAAABXRUJQVlA4IA==" },
            ])
        );
        // Two pictures at the most a paste takes fill a pick, so a third is left out.
        let full: Vec<PathBuf> = ["e.jpg", "f.jpg", "g.jpg"]
            .iter()
            .map(|name| file(name, &jpeg(MAX_IMAGE_BYTES - 3)))
            .collect();
        assert_eq!(picked(&full).as_array().map(Vec::len), Some(2));
        let _ = std::fs::remove_dir_all(&folder);
    }
}
