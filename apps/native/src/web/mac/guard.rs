//! A page with a password, card or one-time-code field runs no script in its own world (the world an agent's `eval`
//! runs in), filled or not, and a picture of it shows none of those fields. Wings checks right before (`guard.js`), in a
//! world of its own that neither the page's scripts nor the plugin's reach, over the main frame and the frames of its
//! origin; an answer it can't read, or one that comes late, refuses. The plugin's own world (its fixed helper: snapshot,
//! click, fill) still runs there: it reads no field's value.

use std::time::{Duration, Instant};

use block2::RcBlock;
use objc2::{MainThreadMarker, runtime::AnyObject};
use objc2_foundation::{NSError, NSString};
use objc2_web_kit::{WKContentWorld, WKWebView};
use serde_json::Value;

/// The check, an async function body that answers `{ secret, rects }`.
const CHECK: &str = include_str!("guard.js");

/// Why an agent's script didn't run.
pub const SECRET_FIELDS: &str = "This page has a password or card field, so scripts can't run on it. Use handoff for sign-ins.";
/// Why nothing ran when the check didn't answer, or not in time.
pub const UNCHECKED: &str = "Wings couldn't check this page for password and card fields, so nothing ran: try again";

/// The longest the check may take: a page busy for longer may have changed since it answered.
const CHECK_TIME: Duration = Duration::from_secs(3);

/// Runs the check in Wings' own world of `view`'s main frame, then `then` with what it found, or why it couldn't, and
/// how long it took.
pub(super) fn check(view: &WKWebView, mtm: MainThreadMarker, then: impl Fn(Result<Value, String>, Duration) + 'static) {
    let started = Instant::now();
    let done = RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
        // SAFETY: WebKit hands one of the two, alive for the call, on the main thread.
        let found = match unsafe { (value.as_ref(), error.as_ref()) } {
            (_, Some(error)) => Err(error.localizedDescription().to_string()),
            (Some(value), None) => super::json_of(value),
            (None, None) => Err("The check answered nothing".into()),
        };
        then(found, started.elapsed());
    });
    // SAFETY: Wings' own script in a world of its own, and a block WebKit calls once on the main thread.
    unsafe {
        let world = WKContentWorld::worldWithName(&NSString::from_str("wings-guard"), mtm);
        view.callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(&NSString::from_str(CHECK), None, None, &world, Some(&done));
    }
}

/// Whether a script may run in the page's own world, from what the check found and how long it took: only on a clear
/// answer of no secret field, in time.
pub(super) fn may_run(found: &Result<Value, String>, took: Duration) -> Result<(), &'static str> {
    match found {
        _ if took > CHECK_TIME => Err(UNCHECKED),
        Ok(found) if found["secret"] == false => Ok(()),
        Ok(found) if found["secret"] == true => Err(SECRET_FIELDS),
        _ => Err(UNCHECKED),
    }
}

/// `script` with the check run again in front of it, in the page's world, so a field that came between the check and
/// the script refuses it too. That second check uses the page's own DOM, which a script run there before (before the
/// field came) may have changed; the first one, in Wings' world, it can't.
pub(super) fn checked_again(script: &str) -> String {
    let why = serde_json::to_string(SECRET_FIELDS).expect("a string is JSON");
    format!("if ((() => {{\n{CHECK}\n}})().secret) throw new Error({why});\n{script}")
}

/// Where the secret fields showed, in the check's CSS pixels, or why it couldn't say.
pub(super) fn rects(found: &Result<Value, String>) -> Result<Vec<[f64; 4]>, &'static str> {
    let found = found.as_ref().map_err(|_| UNCHECKED)?;
    let rects = found["rects"].as_array().ok_or(UNCHECKED)?;
    rects
        .iter()
        .map(|rect| {
            let at = |n: usize| rect.get(n).and_then(Value::as_f64).ok_or(UNCHECKED);
            Ok([at(0)?, at(1)?, at(2)?, at(3)?])
        })
        .collect()
}

/// The picture `png` of a view `points_wide` points wide, its page at `zoom`, with each of `rects` (CSS pixels of the
/// view) painted over, a little past its edges.
pub(super) fn masked(png: &[u8], rects: &[[f64; 4]], points_wide: f64, zoom: f64) -> Result<Vec<u8>, String> {
    if rects.is_empty() {
        return Ok(png.to_vec());
    }
    let mut image = image::load_from_memory_with_format(png, image::ImageFormat::Png).map_err(|error| error.to_string())?.into_rgba8();
    let scale = f64::from(image.width()) / points_wide.max(1.0) * zoom;
    let (wide, high) = (f64::from(image.width()), f64::from(image.height()));
    for [x, y, w, h] in rects {
        let pad = 2.0 * scale;
        let (left, top) = ((x * scale - pad).clamp(0.0, wide), (y * scale - pad).clamp(0.0, high));
        let (right, bottom) = (((x + w) * scale + pad).clamp(0.0, wide), ((y + h) * scale + pad).clamp(0.0, high));
        for py in top as u32..bottom.ceil() as u32 {
            for px in left as u32..right.ceil() as u32 {
                image.put_pixel(px, py, image::Rgba([128, 128, 128, 255]));
            }
        }
    }
    let mut out = Vec::new();
    image.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).map_err(|error| error.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;

    use super::*;

    /// An agent's script runs in the page's world only on a clear "no secret field" in time: a password field, filled or
    /// not, refuses it, and so does a check that failed, said something else, or came late.
    #[test]
    fn a_script_runs_in_the_page_only_when_the_check_found_no_secret_field_in_time() {
        let fast = Duration::from_millis(5);
        assert_eq!(may_run(&Ok(json!({ "secret": false, "rects": [] })), fast), Ok(()));
        assert_eq!(may_run(&Ok(json!({ "secret": true, "rects": [] })), fast), Err(SECRET_FIELDS), "an empty or hidden one");
        assert_eq!(may_run(&Ok(json!({ "secret": true, "rects": [[8, 8, 155, 21]] })), fast), Err(SECRET_FIELDS));
        for unclear in [Ok(json!(null)), Ok(json!({ "secret": "no" })), Err("A JavaScript exception occurred".to_owned())] {
            assert_eq!(may_run(&unclear, fast), Err(UNCHECKED), "{unclear:?}");
        }
        assert_eq!(may_run(&Ok(json!({ "secret": false, "rects": [] })), Duration::from_secs(4)), Err(UNCHECKED), "late");
    }

    /// The script carries the check in front of it, so a field that came after the check refuses it too.
    #[test]
    fn the_script_checks_again_before_it_runs() {
        let script = checked_again("return document.title");
        assert!(script.starts_with("if ((() => {") && script.ends_with("\nreturn document.title"), "{script}");
        assert!(script.contains(CHECK) && script.contains(&serde_json::to_string(SECRET_FIELDS).unwrap()));
    }

    /// A picture of a page with a card field at 20,30 (100 by 20 CSS pixels) on a 2x screen: the field is painted over,
    /// the rest is as it was.
    #[test]
    fn a_picture_paints_over_the_secret_fields() {
        let white = image::RgbaImage::from_pixel(400, 200, image::Rgba([255, 255, 255, 255]));
        let mut png = Vec::new();
        white.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let found = Ok(json!({ "secret": true, "rects": [[20, 30, 100, 20]] }));
        let out = masked(&png, &rects(&found).unwrap(), 200.0, 1.0).unwrap();
        let out = image::load_from_memory(&out).unwrap().into_rgba8();
        let grey = image::Rgba([128, 128, 128, 255]);
        for (x, y) in [(40, 60), (239, 99), (100, 80)] {
            assert_eq!(*out.get_pixel(x, y), grey, "{x},{y} inside the field");
        }
        for (x, y) in [(10, 10), (300, 150), (40, 120)] {
            assert_eq!(*out.get_pixel(x, y), image::Rgba([255, 255, 255, 255]), "{x},{y} outside it");
        }
        // A check that couldn't say where takes no picture.
        assert_eq!(rects(&Err("gone".into())), Err(UNCHECKED));
        assert_eq!(masked(&png, &[], 200.0, 1.0).unwrap(), png);
    }
}
