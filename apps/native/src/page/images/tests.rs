use std::io::Cursor;

use super::*;

/// A made-up picture: `width` by `height`, one colour, as a png.
pub(super) fn png(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(width, height, image::Rgba(color)).write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png).unwrap();
    bytes
}

fn base64_of(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

#[test]
fn an_id_is_letters_digits_dashes_and_underscores_and_so_never_a_path_or_a_url() {
    for good in ["i0", "t-3f9a", "a_b", &"x".repeat(64)] {
        assert!(valid_id(good), "{good}");
    }
    for bad in ["", "/etc/passwd", "a/b", "a.png", "https://x", "a b", &"x".repeat(65)] {
        assert!(!valid_id(bad), "{bad}");
    }
}

#[test]
fn only_a_picture_of_a_kind_claude_reads_within_the_limits_is_kept() {
    let mut store = Inner::default();
    assert!(store.add("p", "a", png(8, 8, [1, 2, 3, 255])).is_ok());
    assert_eq!(store.add("p", "b", b"just some text".to_vec()), Err("not a png, jpeg, gif or webp".into()));
    // A bmp is a picture, but not one Claude reads.
    let mut bmp = Vec::new();
    image::RgbaImage::new(2, 2).write_to(&mut Cursor::new(&mut bmp), ImageFormat::Bmp).unwrap();
    assert!(store.add("p", "c", bmp).is_err());
    // Over the pixels the app decodes in full.
    let mut big = Vec::new();
    image::GrayImage::new(4001, 4001).write_to(&mut Cursor::new(&mut big), ImageFormat::Png).unwrap();
    assert_eq!(store.add("p", "d", big), Err("too large to show".into()));
    assert_eq!(store.entries.len(), 1);
    // The public call checks the id and the size of the text before it decodes any of it.
    let data = base64_of(&png(2, 2, [0, 0, 0, 255]));
    assert!(register("limits", "ok", &data).is_ok());
    assert!(register("limits", "../x", &data).is_err());
    assert!(register("limits", "big", &"A".repeat(MAX_BYTES / 3 * 4 + 8)).is_err());
    assert!(register("limits", "bad", "not base64!").is_err());
}

#[test]
fn the_same_bytes_under_two_ids_are_kept_once() {
    let mut store = Inner::default();
    let bytes = png(8, 8, [9, 9, 9, 255]);
    store.add("p", "a", bytes.clone()).unwrap();
    store.add("p", "b", bytes.clone()).unwrap();
    store.add("q", "a", bytes.clone()).unwrap();
    assert_eq!((store.entries.len(), store.names.len(), store.bytes), (1, 3, bytes.len()));
}

#[test]
fn the_least_recently_used_picture_goes_first_and_its_id_goes_with_it() {
    let mut store = Inner::default();
    for n in 0..=MAX_ENTRIES as u32 {
        store.add("p", &format!("i{n}"), png(1, 1, [(n % 256) as u8, (n / 256) as u8, 0, 255])).unwrap();
    }
    assert_eq!(store.entries.len(), MAX_ENTRIES);
    assert!(!store.names.contains_key(&("p".to_owned(), "i0".to_owned())));
    assert!(store.names.contains_key(&("p".to_owned(), format!("i{MAX_ENTRIES}"))));
}

#[cfg(feature = "ui")]
mod drawn {
    use std::{sync::mpsc, time::Duration};

    use gpui::RenderImage;

    use super::*;

    /// Shows `id` of `plugin` at `level` until it is decoded.
    fn shown(plugin: &str, id: &str, level: u32) -> Shown {
        let (done, heard) = mpsc::channel();
        match show(plugin, id, level, Some(Box::new(move || { let _ = done.send(()); }))) {
            Shown::Waiting { .. } => {
                heard.recv_timeout(Duration::from_secs(20)).expect("the picture was decoded");
                show(plugin, id, level, None)
            }
            other => other,
        }
    }

    #[test]
    fn a_picture_is_decoded_once_at_the_size_it_is_drawn_and_shared_by_its_ids() {
        let bytes = png(3200, 1000, [10, 20, 30, 255]);
        register("decode-once", "a", &base64_of(&bytes)).unwrap();
        register("decode-once", "b", &base64_of(&bytes)).unwrap();
        let Shown::Ready { decoded, width, height } = shown("decode-once", "a", 800) else {
            panic!("ready")
        };
        assert_eq!((width, height), (3200, 1000));
        let size = decoded.image.size(0);
        assert_eq!((size.width.0, size.height.0), (800, 250));
        // The second id is the same bytes, so the same pixels, with no decode to wait for.
        let Shown::Ready { decoded: again, .. } = show("decode-once", "b", 800, None) else {
            panic!("ready at once")
        };
        assert!(Arc::ptr_eq(&decoded, &again));
        // Drawn again at a smaller size it is the same too.
        let Shown::Ready { decoded: smaller, .. } = show("decode-once", "a", 400, None) else {
            panic!("ready")
        };
        assert!(Arc::ptr_eq(&decoded, &smaller));
    }

    #[test]
    fn a_picture_wanted_larger_than_it_was_decoded_is_decoded_again() {
        register("decode-up", "a", &base64_of(&png(3200, 1000, [1, 1, 1, 255]))).unwrap();
        assert!(matches!(shown("decode-up", "a", 400), Shown::Ready { .. }));
        let Shown::Ready { decoded, .. } = shown("decode-up", "a", 1600) else {
            panic!("ready")
        };
        assert_eq!(decoded.image.size(0).width.0, 1600);
    }

    #[test]
    fn a_page_sees_only_its_own_plugins_pictures_and_a_small_one_keeps_its_size() {
        register("own-a", "a", &base64_of(&png(30, 20, [255, 0, 0, 255]))).unwrap();
        assert!(matches!(show("own-b", "a", 400, None), Shown::Missing));
        let Shown::Ready { decoded, .. } = shown("own-a", "a", 400) else {
            panic!("ready")
        };
        assert_eq!(decoded.image.size(0).width.0, 30);
        // Red as a texture takes it: blue, green, red, alpha.
        assert_eq!(&decoded.image.as_bytes(0).unwrap()[..4], [0, 0, 255, 255]);
    }

    #[test]
    fn a_picture_that_does_not_decode_says_so_in_place_of_waiting_again() {
        // The header of a png is enough to be taken, and the pixels after it are cut off.
        let mut cut = png(40, 40, [5, 5, 5, 255]);
        cut.truncate(cut.len() - 30);
        register("broken", "a", &base64_of(&cut)).unwrap();
        assert!(matches!(shown("broken", "a", 400), Shown::Broken { width: 40, height: 40 }));
        assert!(matches!(show("broken", "a", 400, None), Shown::Broken { .. }));
    }

    #[test]
    fn what_is_held_of_decoded_pixels_is_bounded_and_the_oldest_are_let_go() {
        let mut store = Inner::default();
        let frame = || Arc::new(RenderImage::new([image::Frame::new(image::RgbaImage::new(1, 1))]));
        let mut hashes = Vec::new();
        for n in 0..2u8 {
            store.add("p", &format!("i{n}"), png(4, 4, [n, 0, 0, 255])).unwrap();
            let hash = store.names[&("p".to_owned(), format!("i{n}"))].0;
            store.entries.get_mut(&hash).unwrap().work = Work::Queued(100, Vec::new());
            store.finish(hash, Ok(Decoded { image: frame(), level: u32::MAX, bytes: KEPT_DECODED / 2 + 1 }));
            hashes.push(hash);
        }
        assert!(store.entries[&hashes[0]].decoded.is_none());
        assert!(store.entries[&hashes[1]].decoded.is_some());
        assert_eq!((store.retired.len(), store.decoded_bytes), (1, KEPT_DECODED / 2 + 1));
    }
}
