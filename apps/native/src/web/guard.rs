//! A page with a password, card or one-time-code field runs no script in its own world (the world an agent's `eval`
//! runs in), filled or not, and a picture of it shows none of those fields: the rules both engines follow, WebKit
//! (`mac/guard.rs`) and Chromium (`chromium/agent.rs`). Wings checks right before (`guard.js`), in a world of its own
//! that neither the page's scripts nor the plugin's reach, over the main frame and the frames of its origin; an answer it
//! can't read, or one that comes late, refuses. Every frame also tells Wings as it holds one (`watch.js`), and Wings
//! keeps that for the rest of the document (`Seen`): a password a page turns into plain text, or a card number in
//! another origin's frame, refuses too. The plugin's own world (its fixed helper: snapshot, click, fill) still runs
//! there: it reads no field's value. A closed shadow root can't be looked into.

use std::{cell::Cell, time::Duration};

use serde_json::Value;

/// `wingsSecret` and `wingsScan`, which the check and the watch both run.
pub const FUNCTIONS: &str = include_str!("guard.js");
/// What each frame runs, after `FUNCTIONS`, in `WORLD`: it tells Wings once the frame holds a secret field, through
/// `webkit.messageHandlers.wingsGuard`, which each engine gives that world alone.
pub const WATCH_FUNCTION: &str = include_str!("watch.js");
/// The check, an async function body that answers `{ secret, rects, foreign }`.
pub const CHECK: &str = concat!(include_str!("guard.js"), "\nreturn wingsScan(document);");
/// Wings' own world, which neither the page's scripts nor the plugin's reach.
pub const WORLD: &str = "wings-guard";

/// Why an agent's script didn't run.
pub const SECRET_FIELDS: &str = "This page has a password or card field, so scripts can't run on it. Use handoff for sign-ins.";
/// Why nothing ran when the check didn't answer, or not in time.
pub const UNCHECKED: &str = "Wings couldn't check this page for password and card fields, so nothing ran: try again";

/// The longest the check may take: a page busy for longer may have changed since it answered.
const CHECK_TIME: Duration = Duration::from_secs(3);

/// What Wings saw of a web view's document: a secret field in any of its frames, and one in a frame in the page (which
/// may be of another origin, that the check can't look into), each by the generation of the main frame's document it
/// was seen in (WebKit's `Observer::generation`, Chromium's `agent::Agent::doc`), so it lasts until that document goes,
/// whatever the page does to the field.
#[derive(Default)]
pub struct Seen {
    secret: Cell<Option<u64>>,
    foreign: Cell<Option<u64>>,
}

impl Seen {
    /// A secret field was seen in the document `generation`, in a frame in the page if `foreign`. Only ever forward: a
    /// check of an older document that answers late never clears what a newer one showed.
    pub fn note(&self, generation: u64, foreign: bool) {
        let later = |cell: &Cell<Option<u64>>| cell.set(Some(cell.get().map_or(generation, |kept| kept.max(generation))));
        later(&self.secret);
        if foreign {
            later(&self.foreign);
        }
    }

    pub fn secret(&self, generation: u64) -> bool {
        self.secret.get() == Some(generation)
    }

    pub fn foreign(&self, generation: u64) -> bool {
        self.foreign.get() == Some(generation)
    }
}

/// Whether a script may run in the page's own world, from what the check found and how long it took, and whether a
/// secret field was `seen` in the document before: only on a clear answer of no secret field, in time, in a document
/// that never showed one.
pub fn may_run(found: &Result<Value, String>, took: Duration, seen: bool) -> Result<(), &'static str> {
    match found {
        _ if took > CHECK_TIME => Err(UNCHECKED),
        _ if seen => Err(SECRET_FIELDS),
        Ok(found) if found["secret"] == false => Ok(()),
        Ok(found) if found["secret"] == true => Err(SECRET_FIELDS),
        _ => Err(UNCHECKED),
    }
}

/// `script` with the check run again in front of it, in the page's world, so a field that came between the check and
/// the script refuses it too. That second check uses the page's own DOM, which a script run there before (before the
/// field came) may have changed; the first one, in Wings' world, it can't.
pub fn checked_again(script: &str) -> String {
    let why = serde_json::to_string(SECRET_FIELDS).expect("a string is JSON");
    format!("if ((() => {{\n{CHECK}\n}})().secret) throw new Error({why});\n{script}")
}

/// What to paint over in a picture, in the check's CSS pixels, or why it couldn't say: where the secret fields showed,
/// and the frames of other origins it couldn't look into, whole, once a frame in the page said it holds one (`foreign`).
pub fn rects(found: &Result<Value, String>, foreign: bool) -> Result<Vec<[f64; 4]>, &'static str> {
    let found = found.as_ref().map_err(|_| UNCHECKED)?;
    let list = |key: &str| -> Result<Vec<[f64; 4]>, &'static str> {
        found[key]
            .as_array()
            .ok_or(UNCHECKED)?
            .iter()
            .map(|rect| {
                let at = |n: usize| rect.get(n).and_then(Value::as_f64).ok_or(UNCHECKED);
                Ok([at(0)?, at(1)?, at(2)?, at(3)?])
            })
            .collect()
    };
    let mut rects = list("rects")?;
    if foreign {
        rects.extend(list("foreign")?);
    }
    Ok(rects)
}

/// The picture `png` of a view `points_wide` points wide, its page at `zoom`, with each of `rects` (CSS pixels of the
/// view) painted over, a little past its edges.
pub fn masked(png: &[u8], rects: &[[f64; 4]], points_wide: f64, zoom: f64) -> Result<Vec<u8>, String> {
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

    use serde_json::{Value, json};

    use super::*;

    /// An agent's script runs in the page's world only on a clear "no secret field" in time: a password field, filled or
    /// not, refuses it, and so does a check that failed, said something else, or came late.
    #[test]
    fn a_script_runs_in_the_page_only_when_the_check_found_no_secret_field_in_time() {
        let fast = Duration::from_millis(5);
        assert_eq!(may_run(&Ok(json!({ "secret": false, "rects": [] })), fast, false), Ok(()));
        assert_eq!(may_run(&Ok(json!({ "secret": true, "rects": [] })), fast, false), Err(SECRET_FIELDS), "an empty or hidden one");
        assert_eq!(may_run(&Ok(json!({ "secret": true, "rects": [[8, 8, 155, 21]] })), fast, false), Err(SECRET_FIELDS));
        for unclear in [Ok(json!(null)), Ok(json!({ "secret": "no" })), Err("A JavaScript exception occurred".to_owned())] {
            assert_eq!(may_run(&unclear, fast, false), Err(UNCHECKED), "{unclear:?}");
        }
        assert_eq!(may_run(&Ok(json!({ "secret": false, "rects": [] })), Duration::from_secs(4), false), Err(UNCHECKED), "late");
    }

    /// A password the page turned into plain text (Show password) is no secret field to a check, but the document showed
    /// one: its scripts stay refused until the main frame has a new document.
    #[test]
    fn a_secret_field_seen_once_refuses_scripts_until_the_next_document() {
        let (seen, fast, clean) = (Seen::default(), Duration::from_millis(5), Ok(json!({ "secret": false, "rects": [], "foreign": [] })));
        seen.note(3, false);
        assert_eq!(may_run(&clean, fast, seen.secret(3)), Err(SECRET_FIELDS), "the same document, the field now plain text");
        assert_eq!(may_run(&clean, fast, seen.secret(4)), Ok(()), "the next document");
        assert!(!seen.foreign(3), "seen in the page's own origin");
        // The next document shows one too, then a check of the last one answers late: the newer one stays refused.
        seen.note(4, true);
        seen.note(3, true);
        assert!(seen.secret(4) && seen.foreign(4), "a late note of an older document cleared the newer one's");
    }

    /// The script carries the check in front of it, so a field that came after the check refuses it too.
    #[test]
    fn the_script_checks_again_before_it_runs() {
        let script = checked_again("return document.title");
        assert!(script.starts_with("if ((() => {") && script.ends_with("\nreturn document.title"), "{script}");
        assert!(script.contains(CHECK) && script.contains(&serde_json::to_string(SECRET_FIELDS).unwrap()));
    }

    /// A picture of a page with a card field at 20,30 (100 by 20 CSS pixels) on a 2x screen: the field is painted over,
    /// the rest is as it was. A frame of another origin is painted over whole once one said it holds a secret field.
    #[test]
    fn a_picture_paints_over_the_secret_fields() {
        let white = image::RgbaImage::from_pixel(400, 200, image::Rgba([255, 255, 255, 255]));
        let mut png = Vec::new();
        white.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let found = Ok(json!({ "secret": true, "rects": [[20, 30, 100, 20]], "foreign": [[150, 10, 40, 40]] }));
        let out = masked(&png, &rects(&found, false).unwrap(), 200.0, 1.0).unwrap();
        let out = image::load_from_memory(&out).unwrap().into_rgba8();
        let grey = image::Rgba([128, 128, 128, 255]);
        for (x, y) in [(40, 60), (239, 99), (100, 80)] {
            assert_eq!(*out.get_pixel(x, y), grey, "{x},{y} inside the field");
        }
        for (x, y) in [(10, 10), (340, 60), (40, 120)] {
            assert_eq!(*out.get_pixel(x, y), image::Rgba([255, 255, 255, 255]), "{x},{y} outside it");
        }
        assert_eq!(rects(&found, true).unwrap(), [[20.0, 30.0, 100.0, 20.0], [150.0, 10.0, 40.0, 40.0]]);
        // A check that couldn't say where takes no picture.
        assert_eq!(rects(&Err("gone".into()), false), Err(UNCHECKED));
        assert_eq!(masked(&png, &[], 200.0, 1.0).unwrap(), png);
    }

    /// A made-up page for `guard.js` and `watch.js` run by JavaScriptCore (`osascript`): no DOM, so the elements are plain
    /// objects with what they read of them, and a `MutationObserver` the test fires by hand.
    const FAKE: &str = r#"
        const all = (node, sel) => { const out = []; const walk = (n) => { for (const c of n.children) { if (sel === "*" || /^(INPUT|TEXTAREA|SELECT)$/.test(c.tagName)) out.push(c); walk(c); } }; walk(node); return out; };
        const doc = (children) => ({ nodeType: 11, children, querySelectorAll(sel) { return all(this, sel); } });
        const el = (tag, o = {}) => ({ tagName: tag, type: o.type || "", nodeType: 1, children: o.children || [], shadowRoot: o.shadow || null,
            get contentDocument() { if (o.foreign) throw new Error("cross-origin"); return o.doc || null; }, clientLeft: 0, clientTop: 0,
            getAttribute: (name) => (o.attrs || {})[name] || null, getClientRects: () => [o.rect || { left: 0, top: 0, width: 10, height: 10 }],
            getBoundingClientRect: () => o.rect || { left: 0, top: 0, width: 10, height: 10 }, querySelectorAll(sel) { return all(this, sel); } });
        const observed = [], told = [], listeners = {};
        class MutationObserver { constructor(heard) { this.heard = heard; } observe(root) { observed.push({ root, observer: this }); } disconnect() {} }
        const webkit = { messageHandlers: { wingsGuard: { postMessage: (m) => told.push(m) } } };
        const addEventListener = (kind, listener) => { listeners[kind] = listener; };
        const removeEventListener = () => {};
        const fire = (root, records) => observed.filter((o) => o.root === root).forEach((o) => o.observer.heard(records));
    "#;

    /// Runs `script` after `FAKE` and `guard.js`, and answers the JSON its last line makes.
    fn js(script: &str) -> Value {
        let script = format!("{FAKE}\n{FUNCTIONS}\n{script}");
        let mut run = std::process::Command::new("osascript")
            .args(["-l", "JavaScript", "-"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        std::io::Write::write_all(&mut run.stdin.take().unwrap(), script.as_bytes()).unwrap();
        let out = run.wait_with_output().unwrap();
        serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
    }

    /// `wingsScan` over the made-up `page`.
    fn scan(page: &str) -> Value {
        js(&format!("JSON.stringify(wingsScan({page}))"))
    }

    /// A field that becomes a secret one inside an open shadow root after the page loaded (one there then, and one that
    /// came with what was added later) tells Wings, as a field in the document does; so does focusing one whose shadow
    /// root came with no change to watch.
    #[test]
    fn a_secret_field_in_an_open_shadow_root_after_load_tells_wings() {
        let page = "const box = doc([el('INPUT', { type: 'text' })]); const document = doc([el('X-LOGIN', { shadow: box })]); document.nodeType = 9;";
        let watch = WATCH_FUNCTION;
        let run = |then: &str| js(&format!("{page}\n{watch}\n{then}\nJSON.stringify(told)"));
        assert_eq!(run(""), json!([]), "a plain page tells nothing");
        // Turned into a password inside the shadow root that was there as it loaded.
        assert_eq!(run("fire(box, [{ type: 'attributes', target: el('INPUT', { type: 'password' }), addedNodes: [] }]);"), json!([true]));
        // A shadow root that came with an element added later, then a card field added inside it.
        let later = "const inner = doc([]); fire(document, [{ type: 'childList', addedNodes: [el('X-PAY', { shadow: inner })] }]); \
                     fire(inner, [{ type: 'childList', addedNodes: [el('INPUT', { attrs: { autocomplete: 'cc-number' } })] }]);";
        assert_eq!(run(later), json!([true]), "a shadow root that came later");
        // Focus in a secret field no change showed.
        assert_eq!(run("listeners.focusin({ composedPath: () => [el('INPUT', { type: 'password' }), box] });"), json!([true]), "focus");
    }

    /// Past the deepest it looks (nine shadow roots in), the scan says a secret field may be there, and paints over what
    /// it stopped at, rather than call the page clear.
    #[test]
    fn a_page_deeper_than_the_scan_looks_counts_as_holding_a_secret_field() {
        let nested = (0..10).fold("doc([el('INPUT', { type: 'text' })])".to_owned(), |inner, _| format!("doc([el('X-BOX', {{ shadow: {inner} }})])"));
        let found = scan(&nested);
        assert_eq!(found["secret"], true, "{found}");
        assert_eq!(found["rects"].as_array().map(Vec::len), Some(1), "{found}");
        assert_eq!(scan("doc([el('X-BOX', { shadow: doc([el('INPUT', { type: 'text' })]) })])")["secret"], false, "a shallow one is looked into");
    }

    /// A frame of another origin can't be looked into: it is listed apart, at its place, to paint over whole; one of the
    /// page's origin is looked into, its fields placed by the frame's place.
    #[test]
    fn frames_of_another_origin_are_listed_to_paint_over_and_those_of_the_pages_are_looked_into() {
        let found = scan(
            "doc([el('IFRAME', { foreign: true, rect: { left: 5, top: 6, width: 300, height: 100 } }),              el('IFRAME', { rect: { left: 10, top: 20, width: 50, height: 50 }, doc: doc([el('INPUT', { type: 'password', rect: { left: 1, top: 2, width: 3, height: 4 } })]) })])",
        );
        assert_eq!(found, json!({ "secret": true, "rects": [[11, 22, 3, 4]], "foreign": [[5, 6, 300, 100]] }));
        assert_eq!(scan("doc([el('INPUT'), el('TEXTAREA')])"), json!({ "secret": false, "rects": [], "foreign": [] }));
    }
}
