//! The `Image` a page shows: a picture the page registered with `wings.registerImage` and names by the id it gave. The
//! node never carries bytes, a path or a URL: `src` is only that id, so a page can't read a file or fetch anything
//! through it (`page::images` holds the bytes and decodes them).

use serde_json::{Map, Value};

use super::{number, str_prop, text_prop, valid_handler};

/// The size a picture is drawn at when the page says none, and the most it may ask for, in points.
const DEFAULT_WIDTH: f64 = 240.0;
const DEFAULT_HEIGHT: f64 = 180.0;
const MAX_SIDE: f64 = 2_000.0;
const MAX_RADIUS: f64 = 24.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    pub id: u64,
    pub src: String,
    /// What a screen reader says for it.
    pub label: Option<String>,
    pub max_width: f32,
    pub max_height: f32,
    /// Fill the whole box and crop what overflows, in place of fitting the picture inside it.
    pub cover: bool,
    pub radius: f32,
    /// Also keep within the window less a dialog's frame, for a picture shown large.
    pub fit_window: bool,
    pub on_click: Option<String>,
}

/// An id a page may register an image under: short, and only letters, digits, `-` and `_`, so it can never be a path or
/// a URL.
pub fn valid_id(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The `Image` in `props`, or `None` when its `src` isn't an id a page can register (so never a path or a URL).
pub fn parse(id: u64, props: &Map<String, Value>) -> Option<Picture> {
    let src = str_prop(props, "src").filter(|src| valid_id(src))?;
    let side = |name: &str, default: f64| number(props, name).unwrap_or(default).clamp(16.0, MAX_SIDE) as f32;
    Some(Picture {
        id,
        src: src.to_owned(),
        label: Some(text_prop(props, "label")).filter(|label| !label.is_empty()),
        max_width: side("maxWidth", DEFAULT_WIDTH),
        max_height: side("maxHeight", DEFAULT_HEIGHT),
        cover: str_prop(props, "fit") == Some("cover"),
        radius: number(props, "radius").unwrap_or(6.0).clamp(0.0, MAX_RADIUS) as f32,
        fit_window: props.get("fitWindow") == Some(&Value::Bool(true)),
        on_click: str_prop(props, "onClick").filter(|handler| valid_handler(handler)).map(str::to_owned),
    })
}

impl Picture {
    /// The box to draw a `width` by `height` picture in, in points: the picture scaled down to fit the most the page
    /// allows (never up), or with `cover` the most itself. `room` is what its dialog leaves it (`room_in`), which
    /// `fit_window` keeps within.
    pub fn size(&self, width: u32, height: u32, room: Option<(f32, f32)>) -> (f32, f32) {
        let (mut most_w, mut most_h) = (self.max_width, self.max_height);
        if let (true, Some((room_w, room_h))) = (self.fit_window, room) {
            most_w = most_w.min(room_w.max(64.0));
            most_h = most_h.min(room_h.max(64.0));
        }
        if self.cover {
            return (most_w, most_h);
        }
        let (width, height) = (width.max(1) as f32, height.max(1) as f32);
        let scale = (most_w / width).min(most_h / height).min(1.0);
        ((width * scale).round().max(1.0), (height * scale).round().max(1.0))
    }
}

/// What a dialog `width` points wide drawn over an `area` (the page's, not the window's, which sidebars share) leaves
/// what it holds: it is at most 92% of the area wide with 24 points either side, and as tall as the area less 32 points above and below,
/// with the title over it and the padding under it.
pub fn room_in(area: (f32, f32), width: f32) -> (f32, f32) {
    (width.min(area.0 * 0.92) - 48.0, area.1 - 150.0)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn picture(props: Value) -> Option<Picture> {
        parse(1, props.as_object().unwrap())
    }

    #[test]
    fn an_image_is_named_by_the_id_the_page_registered_and_by_nothing_else() {
        assert_eq!(picture(json!({"src":"i0"})).unwrap().src, "i0");
        for refused in [
            "/etc/passwd",
            "../a",
            "file:///etc/passwd",
            "https://example.com/a.png",
            "data:image/png;base64,AAAA",
            "a.png",
            "",
            &"a".repeat(65),
        ] {
            assert_eq!(picture(json!({ "src": refused })), None, "{refused}");
        }
        assert_eq!(picture(json!({})), None);
        assert_eq!(picture(json!({"src": 7})), None);
    }

    #[test]
    fn what_the_page_asks_for_is_kept_within_what_the_app_draws() {
        let big = picture(json!({"src":"i0","maxWidth":99999,"maxHeight":-4,"radius":500})).unwrap();
        assert_eq!((big.max_width, big.max_height, big.radius), (2000.0, 16.0, 24.0));
        let plain = picture(json!({"src":"i0","label":"A chart","fit":"cover","onClick":"3:onClick","fitWindow":true})).unwrap();
        assert_eq!((plain.max_width, plain.max_height, plain.radius), (240.0, 180.0, 6.0));
        assert_eq!(plain.label.as_deref(), Some("A chart"));
        assert!(plain.cover && plain.fit_window);
        assert_eq!(plain.on_click.as_deref(), Some("3:onClick"));
        // A handler that isn't a handler id is dropped, as for every node.
        assert_eq!(picture(json!({"src":"i0","onClick":"no handler"})).unwrap().on_click, None);
    }

    #[test]
    fn a_picture_is_scaled_down_to_its_box_and_never_up() {
        let thumb = picture(json!({"src":"i0","maxWidth":100,"maxHeight":100})).unwrap();
        assert_eq!(thumb.size(400, 200, None), (100.0, 50.0));
        assert_eq!(thumb.size(200, 400, None), (50.0, 100.0));
        assert_eq!(thumb.size(40, 20, None), (40.0, 20.0));
        let cover = picture(json!({"src":"i0","maxWidth":64,"maxHeight":64,"fit":"cover"})).unwrap();
        assert_eq!(cover.size(400, 200, None), (64.0, 64.0));
    }

    #[test]
    fn a_picture_shown_large_keeps_within_the_window() {
        let large = picture(json!({"src":"i0","maxWidth":1800,"maxHeight":1800,"fitWindow":true})).unwrap();
        // A wide dialog over 1000 x 700: 92% less 48 wide, the area less 150 tall.
        let room = Some(room_in((1000.0, 700.0), 2000.0));
        let (w, h) = large.size(2000, 1000, room);
        assert_eq!((w, h), (872.0, 436.0));
        let (w, h) = large.size(1000, 2000, room);
        assert_eq!((w, h), (275.0, 550.0));
        // A dialog 760 wide holds it to its own width, less its sides, though the area would leave more.
        let (w, h) = large.size(2000, 1520, Some(room_in((1000.0, 1200.0), 760.0)));
        assert_eq!((w, h), (712.0, 541.0));
        // Without the window it is as the page said.
        assert_eq!(large.size(2000, 1000, None), (1800.0, 900.0));
    }
}
