//! Builders for the rest of the kinds of node the app draws (`apps/native/src/page/tree.rs`), used as `ui::<name>`. Each
//! sets the props the app reads for its kind; more go on with `.prop(...)`. `tests/ui_coverage.rs` fails when the app
//! draws a kind no builder here or in `ui` makes.

use super::{Node, slot};
use serde_json::{Value, json};

/// One of the app's own icons by `name`: `check`, `chevron-down`, `copy`, `trash` or `claude`, `size` points wide (8 to 32).
/// A name the app doesn't have draws nothing. `icon` draws your own paths instead.
pub fn icon_named(name: &str, size: u8) -> Node {
    Node::new("Icon").prop("name", name).prop("size", size)
}

/// A round mark with the initials of `name`, in a colour that name always gets. `large` for the bigger one.
pub fn avatar(name: &str, large: bool) -> Node {
    let avatar = Node::new("Avatar").prop("name", name);
    if large { avatar.prop("size", "large") } else { avatar }
}

/// A grey placeholder for something still loading, `height` points high: `width` is a number of points, or a text like
/// `"60%"` of the width it has.
pub fn skeleton(width: impl Into<Value>, height: f64) -> Node {
    Node::new("Skeleton").prop("width", width.into()).prop("height", height)
}

/// A live terminal: the pane `pane` of the terminals the agent of this tab opened (`host::on_terminals`), `height` points
/// high (80 to 800). Needs `terminals` in `contributes.ui`.
pub fn terminal(pane: &str, height: u32) -> Node {
    Node::new("Terminal").prop("pane", pane).prop("height", height)
}

/// A thin bar of `percent` (0 to 100) with `label` for a screen reader, in the colour of `tone`
/// (`muted`, `info`, `success`, `warn` or `danger`) when it has one.
pub fn meter(label: &str, percent: f64, tone: Option<&str>) -> Node {
    let meter = Node::new("Meter").prop("label", label).prop("percent", percent);
    match tone {
        Some(tone) => meter.prop("tone", tone),
        None => meter,
    }
}

/// A strip across the page holding `children`, with a rule on its `edge` (`top`, `bottom` or `none`). `.prop("gap", ..)`
/// spaces the children and `.prop("variant", "slim")` makes it thinner.
pub fn bar(edge: &str, children: impl IntoIterator<Item = Node>) -> Node {
    Node::new("Bar").prop("edge", edge).children(children)
}

/// Two panes side by side, `start` and `end`, the first `size` points wide (16 to 1000) and the other filling the rest, or
/// with `.prop("side", "end")` the second `size` wide. More props: `collapseBelow` (a width under which only one shows),
/// `pinned`, `float`, `insetBelow`, `insetColumn`, `gap`, and `onNarrow` and `onClose` handlers.
pub fn split(start: Node, end: Node, size: f64) -> Node {
    Node::new("Split").prop("defaultSize", size).child(slot("start", start)).child(slot("end", end))
}

/// A board of `columns` (`board_column`) side by side, each its own list, named `label` for a screen reader.
pub fn board(label: &str, columns: impl IntoIterator<Item = Node>) -> Node {
    Node::new("Board").prop("label", label).children(columns)
}

/// A board in lanes: a `header` (a `board_row` of `board_column`s with titles) over the `lanes`, each of which holds a
/// `board_row` of `board_cell`s that line up under the header's columns.
pub fn board_lanes(label: &str, header: Node, lanes: impl IntoIterator<Item = Node>) -> Node {
    Node::new("Board").prop("label", label).prop("lanes", true).child(slot("header", header)).child(Node::new("Slot").prop("name", "body").children(lanes))
}

/// A row of a board's columns or cells.
pub fn board_row(columns: impl IntoIterator<Item = Node>) -> Node {
    Node::new("BoardRow").children(columns)
}

/// A column of a board, with its `title`, how many it holds when you say (`count`), and its `body`. In a lanes board's
/// header give it only a title. `.child(slot("actions", ..))` puts buttons by the title.
pub fn board_column(title: Node, count: Option<u64>, body: Option<Node>) -> Node {
    let column = Node::new("BoardColumn").child(slot("title", title));
    let column = match count {
        Some(count) => column.prop("count", count),
        None => column,
    };
    column.maybe(body.map(|body| slot("body", body)))
}

/// A cell of a lanes board's lane, under the column named `label`, holding `body`.
pub fn board_cell(label: &str, body: Node) -> Node {
    Node::new("BoardColumn").prop("label", label).prop("cell", true).child(slot("body", body))
}

/// `trigger` (a button, or whatever you point at) with `card` over it as you point: `width` points (160 to 480).
pub fn hover_card(trigger: Node, card: Node, width: u32) -> Node {
    Node::new("HoverCard").prop("width", width).child(trigger).child(slot("card", card))
}

/// A button that opens `items` (buttons, each named by its text) in a menu under it. `end` lines the menu up with the
/// button's right edge. A press on an item closes it.
pub fn menu(trigger: Node, items: impl IntoIterator<Item = Node>, end: bool) -> Node {
    let menu = Node::new("Menu").child(trigger).children(items);
    if end { menu.prop("align", "end") } else { menu }
}

/// A menu of `items` (buttons) opened at the window point `(x, y)`, as a context menu is. `on_close` hears Escape and a
/// press outside: leave it out of the page then.
pub fn popup_menu(x: f64, y: f64, items: impl IntoIterator<Item = Node>, on_close: impl Fn() + 'static) -> Node {
    Node::new("PopupMenu").prop("x", x).prop("y", y).on("onClose", move |_| on_close()).children(items)
}

/// A browser-style strip of tabs on one line, drawn as Wings' own tabs are: `children` are `tab`s and `tab_group` labels in
/// display order. `on_select` and `on_close` hear a tab's id (a click, its close button, a middle click), `on_new` the "+"
/// at the end, `on_group` a group's id when its label is clicked. `label` is the strip's accessible name.
pub fn tab_strip(
    label: &str,
    children: impl IntoIterator<Item = Node>,
    on_select: impl Fn(&str) + 'static,
    on_close: impl Fn(&str) + 'static,
    on_new: impl Fn() + 'static,
    on_group: impl Fn(&str) + 'static,
) -> Node {
    let id = |event: &Value| event["id"].as_str().unwrap_or_default().to_owned();
    Node::new("TabStrip")
        .prop("label", label)
        .on("onSelect", move |event| on_select(&id(event)))
        .on("onClose", move |event| on_close(&id(event)))
        .on("onNew", move |_| on_new())
        .on("onGroup", move |event| on_group(&id(event)))
        .children(children)
}

/// A tab of a `tab_strip`. `group` is the id of the `tab_group` it belongs to; `loading` shows a spinner. The full `title`
/// and the `url` show as its tooltip.
pub fn tab(id: &str, title: &str, url: &str, active: bool, loading: bool, group: Option<&str>) -> Node {
    let tab = Node::new("Tab").prop("id", id).prop("title", title).prop("url", url).prop("active", active).prop("loading", loading);
    match group {
        Some(group) => tab.prop("group", group),
        None => tab,
    }
}

/// A group's label in a `tab_strip`, before its tabs. `tone` is `info`, `success`, `warning`, `danger`, `neutral` or
/// `accent`; `count` is how many tabs it holds, which a collapsed group's label shows.
pub fn tab_group(id: &str, name: &str, tone: &str, collapsed: bool, count: usize) -> Node {
    Node::new("TabGroup").prop("id", id).prop("name", name).prop("tone", tone).prop("collapsed", collapsed).prop("count", count)
}

/// A wedge of a `sunburst`: from `from` to `to` radians (0 to 2π) in ring `ring` (0 is the innermost), in the colour
/// `color` (0xRRGGBB) at `alpha` (0 to 1).
pub struct Wedge {
    pub from: f64,
    pub to: f64,
    pub ring: usize,
    pub color: u32,
    pub alpha: f64,
}

/// Rings of `wedges` round a middle, like a disk map: `rings` of them (1 to the app's limit), up to the app's limit of
/// wedges. More props: `selected` (a wedge's index), `title` and `detail` for the middle, `up` for a way back out, and the
/// `onPoint` and `onSelect` handlers, which hear `{ index }` (`-2` for the middle).
pub fn sunburst(rings: usize, wedges: &[Wedge]) -> Node {
    let wedges: Vec<Value> = wedges.iter().map(|wedge| json!([wedge.from, wedge.to, wedge.ring, wedge.color, wedge.alpha])).collect();
    Node::new("Sunburst").prop("rings", rings).prop("wedges", wedges)
}

/// A rail of ticks, one per `(title, preview)` mark, with the `current` one lit. Alone it fills the place it is put in, like a
/// narrow `split` pane. With `beside` it sits against the left of a column that wide in a page, and only where the page
/// leaves it room. `on_select` hears the index of the mark picked.
pub fn prompt_rail(marks: &[(&str, &str)], current: usize, beside: Option<u32>, on_select: impl Fn(usize) + 'static) -> Node {
    let marks: Vec<Value> = marks.iter().map(|(title, preview)| json!({ "title": title, "preview": preview })).collect();
    let rail = Node::new("PromptRail").prop("marks", marks).prop("current", current);
    let rail = match beside {
        Some(width) => rail.prop("beside", width),
        None => rail,
    };
    rail.on("onSelect", move |event| on_select(event["index"].as_u64().unwrap_or_default() as usize))
}

/// A text box of `rows` lines (1 to 24) that grows to `maxRows` (set it with `.prop`). `on_change` hears the text after each
/// change. More props: `placeholder`, `submitOnEnter`, `maxLength`, `autoFocus`, and the `onKeyDown` and `onPasteImage`
/// handlers.
pub fn textarea(label: &str, value: &str, rows: u32, on_change: impl Fn(String) + 'static) -> Node {
    Node::new("Textarea")
        .prop("label", label)
        .prop("value", value)
        .prop("rows", rows)
        .on("onChange", move |event| on_change(event["value"].as_str().unwrap_or_default().to_owned()))
}

/// A web page drawn by WebKit over this page (Wings 0.2.113). Wings keeps the web engine's view by `name`, the key within
/// the page, for as long as the node stays in the tree, so building the page again never reloads it. Props, set with
/// `.prop`: `url` (http, https or `about:blank`, loaded when the view is first made), `command` (`web_command`),
/// `keepData` (default true; false keeps cookies and site data only while Wings runs), `height` in points (default 400)
/// or `fill: true` to take the rest of its container's height, and `onState` (`WebState::from_event`). With `onPopup`
/// (`WebPopup`) the windows the page opens come to the plugin, a `popup` prop takes the view made for one, and `onClose`
/// hears it close itself; with `onDownload` (`WebDownload`) it downloads (Wings 0.2.132). It is drawn under a dialog or
/// sheet of the page and an app menu, and is an empty area in a headless render.
pub fn web_view(name: &str) -> Node {
    Node::new("WebView").prop("name", name)
}

/// The `command` prop of a `web_view`: `action` is `load` (with `url`), `back`, `forward`, `reload` or `stop`. Each new
/// `token` runs once, like `Scroll`'s `scrollTo`, so the page keeps a counter and raises it for every command.
pub fn web_command(action: &str, url: Option<&str>, token: u64) -> Value {
    json!({ "action": action, "url": url, "token": token })
}

/// The `command` prop that runs `script` in the page (Wings 0.2.114, needs `permissions.webScripts`). `script` is the body of
/// an async JavaScript function: what it returns comes back as JSON in `onResult` as `{ token, value }`, or `{ token, error }`.
/// By default it runs in an isolated world that shares the page's DOM but not its globals; `page_world` runs it in the page's
/// own, which the app refuses on a site whose cookies were imported from another browser.
pub fn web_eval(script: &str, page_world: bool, token: u64) -> Value {
    let mut command = json!({ "action": "eval", "script": script, "token": token });
    if page_world {
        command["world"] = json!("page");
    }
    command
}

/// The `command` prop that takes a picture of the visible area (Wings 0.2.114): `onResult` gets `{ token, value: { path,
/// width, height } }`, a PNG saved in the plugin's web folder whose long side is at most 1,600 pixels. `tools::with_image`
/// sends that path to Claude.
pub fn web_snapshot(token: u64) -> Value {
    json!({ "action": "snapshot", "token": token })
}

/// A file for `web_upload`.
#[derive(Clone, Debug, PartialEq)]
pub enum WebUploadFile<'a> {
    /// A path: absolute, or relative to the plugin's web folder. Wings reads it only inside the temporary folders and the
    /// ones `WINGS_BROWSER_FILE_ROOTS` names, and refuses symlinks, hard links and files others can write.
    Path(&'a str),
    /// A file whose bytes aren't on this computer, as base64 under a plain name: at most 10 MB together.
    Inline { name: &'a str, data: &'a str },
}

/// The `command` prop that puts files in a file input of the page (Chromium engine only, needs `permissions.webScripts`):
/// `reference` is a ref of the plugin's own helper (`window.__wingsAgent.element(ref)`) or `selector` a CSS selector that
/// matches exactly one element, a `<label>` meaning its control. `onResult` gets `{ token, value: { files } }`, or `{ token,
/// error }`: not a file input, several files for one that isn't `multiple`, a path outside the allowed folders, more than
/// 50 MB of files by path, or a page signed in with imported cookies. WebKit answers that it needs Chromium.
pub fn web_upload(reference: Option<&str>, selector: Option<&str>, files: &[WebUploadFile], token: u64) -> Value {
    let files: Vec<Value> = files
        .iter()
        .map(|file| match file {
            WebUploadFile::Path(path) => json!(path),
            WebUploadFile::Inline { name, data } => json!({ "name": name, "data": data }),
        })
        .collect();
    json!({ "action": "upload", "ref": reference, "selector": selector, "files": files, "token": token })
}

/// The `command` prop that takes a fresh picture of the page (secret fields painted over, as `web_snapshot`'s) and compares
/// it with the PNG at `baseline` (Chromium engine only, needs `permissions.webScripts`): `threshold` is how far a channel may
/// differ, 0 to 1 of 255 (0.1 when `None`), `clip` is `[x, y, width, height]` in CSS pixels. The picture taken goes to
/// `current` and the picture of the changes (changed pixels red) to `diff` when given; a diff with changes and no `diff` is
/// saved in the web folder. `onResult` gets `{ matches, changedPixels, totalPixels, changedRatio, width, height, diff,
/// current }`, with an error when the pictures differ in size. Paths are as `web_upload`'s, a relative one in the web folder.
pub fn web_screenshot_diff(baseline: &str, threshold: Option<f64>, clip: Option<[f64; 4]>, current: Option<&str>, diff: Option<&str>, token: u64) -> Value {
    let clip = clip.map(|[x, y, width, height]| json!({ "x": x, "y": y, "width": width, "height": height }));
    json!({ "action": "screenshotDiff", "baseline": baseline, "threshold": threshold, "clip": clip, "current": current, "diff": diff, "token": token })
}

/// What `web_record` does.
#[derive(Clone, Debug, PartialEq)]
pub enum WebRecord<'a> {
    /// Records into the .mp4 at `path` (an absolute path or one relative to the web folder, as `web_upload`'s) at most `fps`
    /// frames a second (1 to 60, 30 when `None`) and for at most `max_ms` (900,000 when `None`). Answers `{ recording, path }`.
    Start { path: &'a str, fps: Option<u32>, max_ms: Option<u64> },
    /// `{ recording, durationMs, frames }`, with `endedBecause` for one that ended by itself.
    Status,
    /// Ends it and answers `{ path, bytes, durationMs, width, height, frames }`.
    Stop,
    /// Ends it and drops the file: `{ cancelled: true }`.
    Cancel,
}

/// The `command` prop that records the page as an H.264 .mp4 (Chromium engine only, needs `permissions.webScripts`). One
/// recording a page; it ends by itself after `max_ms`, when its page closes, or when a password or card field shows or the
/// page moves to a site signed in with imported cookies.
pub fn web_record(record: &WebRecord, token: u64) -> Value {
    match record {
        WebRecord::Start { path, fps, max_ms } => json!({ "action": "record", "do": "start", "path": path, "fps": fps, "maxMs": max_ms, "token": token }),
        WebRecord::Status => json!({ "action": "record", "do": "status", "token": token }),
        WebRecord::Stop => json!({ "action": "record", "do": "stop", "token": token }),
        WebRecord::Cancel => json!({ "action": "record", "do": "cancel", "token": token }),
    }
}

/// One event of `web_input`: what the DevTools protocol's `Input` domain takes.
#[derive(Clone, Debug, PartialEq)]
pub enum WebInput {
    /// A mouse event at `x`, `y` in CSS pixels of the page's viewport. `kind` is `mouseMoved`, `mousePressed` or
    /// `mouseReleased`, `button` `none` or `left`, `buttons` the buttons held (1 for the left), `click_count` 1 to 3 on a
    /// press and release, `modifiers` Alt 1, Ctrl 2, Meta 4, Shift 8.
    Mouse { kind: &'static str, x: f64, y: f64, button: &'static str, buttons: u32, click_count: u32, modifiers: u32 },
    /// A key event: `kind` is `keyDown`, `rawKeyDown` (no text), `keyUp` or `char`; `vk` the Windows virtual key code;
    /// `commands` any of `selectAll`, `copy`, `cut`, `paste`, `undo`, `redo`, which a shortcut needs on macOS.
    Key { kind: &'static str, key: String, code: String, vk: u32, text: Option<String>, modifiers: u32, commands: Vec<&'static str> },
    /// Text typed in one go, as an input method makes it, into what has the focus.
    Text(String),
}

/// The `command` prop that sends real mouse and keyboard input to the page, in order (Chromium engine only, needs
/// `permissions.webScripts`): what the page hears is trusted (`isTrusted`), as the person's own. At most 256 events. It is
/// refused as `web_eval` in the page's world is: on a page that shows a password or card field, and one signed in with
/// imported cookies. `onResult` gets `{ token, value: { events } }`, or `{ token, error }`; WebKit answers that it needs
/// Chromium.
pub fn web_input(events: &[WebInput], token: u64) -> Value {
    let events: Vec<Value> = events
        .iter()
        .map(|event| match event {
            WebInput::Mouse { kind, x, y, button, buttons, click_count, modifiers } => {
                json!({ "t": "mouse", "type": kind, "x": x, "y": y, "button": button, "buttons": buttons, "clickCount": click_count, "modifiers": modifiers })
            }
            WebInput::Key { kind, key, code, vk, text, modifiers, commands } => {
                json!({ "t": "key", "type": kind, "key": key, "code": code, "vk": vk, "text": text, "modifiers": modifiers, "commands": commands })
            }
            WebInput::Text(text) => json!({ "t": "text", "text": text }),
        })
        .collect();
    json!({ "action": "input", "events": events, "token": token })
}

/// What `web_network` asks for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WebNetwork<'a> {
    /// Only requests whose address holds this.
    pub url: Option<&'a str>,
    /// `2xx`, `3xx`, `4xx`, `5xx`, an exact status like `404`, or `failed`.
    pub status: Option<&'a str>,
    /// A resource type: `Document`, `XHR`, `Fetch`, `Script`, `Image`...
    pub kind: Option<&'a str>,
    /// Only requests that began after this, in milliseconds since 1970.
    pub since: Option<f64>,
    /// The most requests listed, 1 to 500 (100 when `None`).
    pub limit: Option<u32>,
    /// One request's id (from the list): its headers instead of a list.
    pub detail: Option<&'a str>,
    /// With `detail`: its response body too, a text one up to 1 MB.
    pub body: bool,
}

/// The `command` prop that lists the page's recent requests, or one request in full (Chromium engine only, needs
/// `permissions.webScripts`). Wings keeps the last 500 of a page from when it opens, with `Authorization`, `Cookie`,
/// `Set-Cookie` and `Proxy-Authorization` values replaced by `(hidden)`. `onResult` gets `{ requests, matching, kept, now }`,
/// or with `detail` the request with `requestHeaders` and `responseHeaders` and, with `body`, its `body`. A body is refused
/// for a page that shows a password or card field, and nothing is given of a site signed in with imported cookies. WebKit
/// answers that it needs Chromium.
pub fn web_network(wanted: &WebNetwork, token: u64) -> Value {
    json!({
        "action": "network", "url": wanted.url, "status": wanted.status, "type": wanted.kind, "since": wanted.since,
        "limit": wanted.limit, "detail": wanted.detail, "body": wanted.body, "token": token,
    })
}

/// The `command` prop that clicks the button of the extension `id` (one of `WebState::extensions`) for the page (Wings
/// 0.2.116): its popup opens under the button you pressed, or the extension hears the click. Send it only from a click
/// handler: Wings runs it only right after your click or key in the page, which it uses up as `copyText` does, and
/// answers `onResult` with `{ token, error }` otherwise.
pub fn web_extension_action(id: &str, token: u64) -> Value {
    json!({ "action": "extension", "id": id, "token": token })
}

/// The `command` prop that acts on a file the page's web views downloaded (`WebDownload::id`, Wings 0.2.132): `action` is
/// `reveal` (shows it in Finder), `open` (opens it in its app once it is done) or `cancel` (stops one under way). Wings
/// reveals or opens one only right after your click or key in the page, and answers `onResult` with `{ token, error }`
/// otherwise.
pub fn web_download(action: &str, id: u64, token: u64) -> Value {
    json!({ "action": "download", "do": action, "id": id, "token": token })
}

/// The `command` prop that carries out the person's choice for the page's certificate notice (`WebState::certificate`,
/// Wings 0.2.133, Chromium): `proceed` opens the site anyway for the rest of this run, otherwise the load is given up. Send
/// it only from the notice's buttons: Wings runs it only right after the person's click, and answers `onResult` with
/// `{ token, error }` otherwise.
pub fn web_certificate(proceed: bool, token: u64) -> Value {
    json!({ "action": "certificate", "proceed": proceed, "token": token })
}

/// The `command` prop that searches the page for `text` and marks the matches (Wings 0.2.133, Chromium); `again` goes on to
/// the next match (the previous one for `forward: false`) of the same text. The count comes back in `WebState::find`.
pub fn web_find(text: &str, forward: bool, again: bool, token: u64) -> Value {
    json!({ "action": "find", "text": text, "forward": forward, "again": again, "token": token })
}

/// The `command` prop that ends the page's search and clears its marks (Wings 0.2.133, Chromium).
pub fn web_stop_find(token: u64) -> Value {
    json!({ "action": "stopFind", "token": token })
}

/// The `command` prop that zooms the page: `step` is `in`, `out` or `reset` (Wings 0.2.133, Chromium). The level comes back
/// in `WebState::zoom`.
pub fn web_zoom(step: &str, token: u64) -> Value {
    json!({ "action": "zoom", "do": step, "token": token })
}

/// The `command` prop that opens the engine's inspector for the page in a window of its own (Wings 0.2.133, Chromium). Wings
/// runs it only right after the person's click or key.
pub fn web_inspect(token: u64) -> Value {
    json!({ "action": "inspect", "token": token })
}

/// What a `web_view`'s `onDownload` hears of one download (Wings 0.2.132): a view whose node has no `onDownload`
/// downloads nothing. Files go into ~/Downloads, under a name no file there has, marked as from the web.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WebDownload {
    pub id: u64,
    pub url: String,
    /// The file's name and full path: empty for one that was blocked.
    pub name: String,
    pub path: String,
    /// `progress`, `done`, `failed`, `canceled` or `blocked` (the page started more than its first download by itself).
    pub state: String,
    pub received: u64,
    /// None while the server hasn't said how big it is.
    pub total: Option<u64>,
    pub error: Option<String>,
}

impl WebDownload {
    pub fn from_event(event: &Value) -> Self {
        let text = |key: &str| event[key].as_str().unwrap_or_default().to_owned();
        Self {
            id: event["id"].as_u64().unwrap_or_default(),
            url: text("url"),
            name: text("name"),
            path: text("path"),
            state: text("state"),
            received: event["received"].as_u64().unwrap_or_default(),
            total: event["total"].as_u64(),
            error: event["error"].as_str().map(str::to_owned),
        }
    }
}

/// What a `web_view`'s `onPopup` hears (Wings 0.2.132): its page opened a window at `url` (a link to a new one,
/// `window.open`, a Cmd-click), to show as a tab, in the background for a Cmd-click. `popup` is the view the engine made
/// for it, which keeps its opener: a `web_view` with the `popup` prop set to it shows that view. None is a plain new tab
/// at `url`. `blocked`: the page opened it with no click of yours, and it wasn't opened.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WebPopup {
    pub url: String,
    pub background: bool,
    pub popup: Option<u64>,
    pub blocked: bool,
}

impl WebPopup {
    pub fn from_event(event: &Value) -> Self {
        Self {
            url: event["url"].as_str().unwrap_or_default().to_owned(),
            background: event["background"] == true,
            popup: event["popup"].as_u64(),
            blocked: event["blocked"] == true,
        }
    }
}

/// An extension's button for the page a `web_view` shows, which a browser draws in its toolbar (Wings 0.2.116).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExtensionButton {
    /// What `web_extension_action` clicks.
    pub id: String,
    pub name: String,
    /// Its hover text, which the extension may set for each page.
    pub label: String,
    /// Its icon, an image id for `ui::image`, when it has one.
    pub icon: Option<String>,
    /// A few characters over the icon, a count most often; empty for none.
    pub badge: String,
    pub enabled: bool,
}

/// What a `web_view`'s `onState` sends when any of it changes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WebState {
    pub url: String,
    pub title: String,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub error: Option<String>,
    /// The page's `alert`, `confirm` or `prompt` waiting for an answer: `{ kind, message, defaultText, site }`. Wings shows
    /// it to the person itself; a page only needs it to tell an agent.
    pub dialog: Option<Value>,
    /// The page is on a site signed in with cookies imported from another browser: show its address, but tell an agent no
    /// more than its origin (Wings 0.2.114).
    pub imported: bool,
    /// The buttons of the extensions that are on and have one, by name (Wings 0.2.116).
    pub extensions: Vec<ExtensionButton>,
    /// The engine that draws the page, `chromium` or `webkit`, empty from a Wings that doesn't say (Wings 0.2.133).
    pub engine: String,
    /// The site's certificate isn't valid and the person hasn't chosen: `(site, reason)`. Draw a notice with Go back and
    /// Proceed (`web_certificate`) (Wings 0.2.133, Chromium).
    pub certificate: Option<(String, String)>,
    /// The site a server's name and password sheet waits on: Wings shows the sheet itself and the page never hears what is
    /// typed. A page tells an agent that the person has to answer (Wings 0.2.133, Chromium).
    pub auth: Option<String>,
    /// What the page's search found while the find bar is open: matches (none when the engine can't count), the current one,
    /// and whether anything was found (Wings 0.2.133, Chromium).
    pub find: Option<WebFind>,
    /// The page's zoom in percent: 100 from a Wings that doesn't say (Wings 0.2.133, Chromium).
    pub zoom: u32,
}

/// What a find bar shows of the page's search.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WebFind {
    pub matches: Option<u32>,
    pub current: u32,
    pub found: bool,
}

impl WebState {
    pub fn from_event(event: &Value) -> Self {
        let text = |key: &str| event[key].as_str().unwrap_or_default().to_owned();
        Self {
            url: text("url"),
            title: text("title"),
            loading: event["loading"].as_bool().unwrap_or_default(),
            can_go_back: event["canGoBack"].as_bool().unwrap_or_default(),
            can_go_forward: event["canGoForward"].as_bool().unwrap_or_default(),
            error: event["error"].as_str().map(str::to_owned),
            dialog: Some(event["dialog"].clone()).filter(|dialog| dialog.is_object()),
            imported: event["imported"] == true,
            engine: text("engine"),
            certificate: event["certificate"].as_object().map(|found| {
                let text = |key: &str| found.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
                (text("site"), text("reason"))
            }),
            auth: event["auth"]["site"].as_str().map(str::to_owned),
            find: event["find"].as_object().map(|found| WebFind {
                matches: found.get("matches").and_then(Value::as_u64).map(|count| count as u32),
                current: found.get("current").and_then(Value::as_u64).unwrap_or_default() as u32,
                found: found.get("found").and_then(Value::as_bool).unwrap_or_default(),
            }),
            zoom: event["zoom"].as_u64().filter(|zoom| *zoom > 0).map_or(100, |zoom| zoom as u32),
            extensions: event["extensions"]
                .as_array()
                .map(|list| {
                    list.iter()
                        .filter_map(|button| {
                            let text = |key: &str| button[key].as_str().unwrap_or_default().to_owned();
                            Some(ExtensionButton {
                                id: button["id"].as_str().filter(|id| !id.is_empty())?.to_owned(),
                                name: text("name"),
                                label: text("label"),
                                icon: button["icon"].as_str().map(str::to_owned),
                                badge: text("badge"),
                                enabled: button["enabled"] == true,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{export, text, tree_json};
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    /// The props and children each builder sets, against what `tree.rs` reads for the kind: `name` and `size` of an Icon,
    /// `percent` and `tone` of a Meter, the `title`, `body` and `actions` slots of a BoardColumn, `defaultSize` and the
    /// `start` and `end` slots of a Split, and so on.
    fn props(node: Node) -> Value {
        let tree = tree_json(node);
        json!([tree["type"], tree["props"]])
    }

    fn slots(node: Node) -> Vec<String> {
        tree_json(node)["children"].as_array().unwrap().iter().map(|slot| slot["props"]["name"].as_str().unwrap_or("-").to_owned()).collect()
    }

    #[test]
    fn a_tab_strip_sends_its_tabs_and_groups_and_hands_each_event_its_id() {
        let heard = Rc::new(RefCell::new(Vec::<String>::new()));
        let (a, b, c, d) = (heard.clone(), heard.clone(), heard.clone(), heard.clone());
        let strip = tab_strip(
            "Tabs",
            [tab_group("g1", "Fix login", "info", false, 2), tab("t1", "Sign in", "https://a.b", true, false, Some("g1")), tab("t2", "Docs", "", false, true, None)],
            move |id| a.borrow_mut().push(format!("select {id}")),
            move |id| b.borrow_mut().push(format!("close {id}")),
            move || c.borrow_mut().push("new".into()),
            move |id| d.borrow_mut().push(format!("group {id}")),
        );
        let (tree, handlers) = export(vec![strip]);
        let strip = &tree[0];
        assert_eq!((strip["type"].clone(), strip["props"]["label"].clone()), (json!("TabStrip"), json!("Tabs")));
        let kinds: Vec<&Value> = strip["children"].as_array().unwrap().iter().map(|child| &child["type"]).collect();
        assert_eq!(kinds, ["TabGroup", "Tab", "Tab"]);
        assert_eq!(strip["children"][0]["props"], json!({ "id": "g1", "name": "Fix login", "tone": "info", "collapsed": false, "count": 2 }));
        assert_eq!(
            strip["children"][1]["props"],
            json!({ "id": "t1", "title": "Sign in", "url": "https://a.b", "active": true, "loading": false, "group": "g1" })
        );
        assert!(strip["children"][2]["props"].get("group").is_none(), "a tab with no group sends none");
        for (name, payload) in [("onSelect", json!({ "id": "t1" })), ("onClose", json!({ "id": "t2" })), ("onNew", json!({})), ("onGroup", json!({ "id": "g1" }))] {
            assert!(handlers.call(strip["props"][name].as_str().unwrap(), &payload), "{name}");
        }
        assert_eq!(*heard.borrow(), ["select t1", "close t2", "new", "group g1"]);
    }

    #[test]
    fn an_eval_and_a_snapshot_are_commands_with_a_token_and_the_page_world_only_when_asked() {
        assert_eq!(web_eval("return 1", false, 4), json!({ "action": "eval", "script": "return 1", "token": 4 }));
        assert_eq!(web_eval("return 1", true, 5)["world"], "page");
        assert_eq!(web_snapshot(6), json!({ "action": "snapshot", "token": 6 }));
    }

    #[test]
    fn input_and_network_are_commands_with_a_token_and_the_events_as_the_app_reads_them() {
        let events = [
            WebInput::Mouse { kind: "mousePressed", x: 1.5, y: 2.0, button: "left", buttons: 1, click_count: 1, modifiers: 0 },
            WebInput::Key { kind: "rawKeyDown", key: "a".into(), code: "KeyA".into(), vk: 65, text: None, modifiers: 4, commands: vec!["selectAll"] },
            WebInput::Text("hi".into()),
        ];
        assert_eq!(
            web_input(&events, 3),
            json!({ "action": "input", "token": 3, "events": [
                { "t": "mouse", "type": "mousePressed", "x": 1.5, "y": 2.0, "button": "left", "buttons": 1, "clickCount": 1, "modifiers": 0 },
                { "t": "key", "type": "rawKeyDown", "key": "a", "code": "KeyA", "vk": 65, "text": null, "modifiers": 4, "commands": ["selectAll"] },
                { "t": "text", "text": "hi" },
            ] })
        );
        assert_eq!(
            web_network(&WebNetwork { url: Some("/api"), status: Some("4xx"), limit: Some(5), ..Default::default() }, 4),
            json!({ "action": "network", "url": "/api", "status": "4xx", "type": null, "since": null, "limit": 5, "detail": null, "body": false, "token": 4 })
        );
        assert_eq!(web_network(&WebNetwork { detail: Some("1.2"), body: true, ..Default::default() }, 5)["body"], true);
    }

    #[test]
    fn an_upload_a_picture_diff_and_a_recording_are_commands_with_a_token() {
        assert_eq!(
            web_upload(Some("e12"), None, &[WebUploadFile::Path("/tmp/a.png"), WebUploadFile::Inline { name: "b.txt", data: "aGk=" }], 7),
            json!({ "action": "upload", "ref": "e12", "selector": null, "files": ["/tmp/a.png", { "name": "b.txt", "data": "aGk=" }], "token": 7 })
        );
        assert_eq!(web_upload(None, Some("input"), &[], 8)["selector"], "input");
        assert_eq!(
            web_screenshot_diff("/tmp/base.png", Some(0.2), Some([1.0, 2.0, 30.0, 40.0]), Some("cur.png"), None, 9),
            json!({ "action": "screenshotDiff", "baseline": "/tmp/base.png", "threshold": 0.2, "clip": { "x": 1.0, "y": 2.0, "width": 30.0, "height": 40.0 }, "current": "cur.png", "diff": null, "token": 9 })
        );
        assert_eq!(web_screenshot_diff("b.png", None, None, None, None, 10)["clip"], Value::Null);
        assert_eq!(
            web_record(&WebRecord::Start { path: "/tmp/x/out.mp4", fps: Some(15), max_ms: None }, 11),
            json!({ "action": "record", "do": "start", "path": "/tmp/x/out.mp4", "fps": 15, "maxMs": null, "token": 11 })
        );
        for (record, name) in [(WebRecord::Status, "status"), (WebRecord::Stop, "stop"), (WebRecord::Cancel, "cancel")] {
            assert_eq!(web_record(&record, 12), json!({ "action": "record", "do": name, "token": 12 }));
        }
    }

    #[test]
    fn a_web_view_sends_its_name_and_a_state_reads_what_the_app_sends() {
        assert_eq!(props(web_view("main").prop("fill", true)), json!(["WebView", { "name": "main", "fill": true }]));
        assert_eq!(web_command("load", Some("https://a.b"), 3), json!({ "action": "load", "url": "https://a.b", "token": 3 }));
        let heard = Rc::new(RefCell::new(WebState::default()));
        let seen = heard.clone();
        let (tree, handlers) = export(vec![web_view("main").on("onState", move |event| *seen.borrow_mut() = WebState::from_event(event))]);
        let event = json!({ "url": "https://a.b/", "title": "A", "loading": true, "canGoBack": true, "canGoForward": false, "error": null });
        assert!(handlers.call(tree[0]["props"]["onState"].as_str().unwrap(), &event));
        assert_eq!(*heard.borrow(), WebState { url: "https://a.b/".into(), title: "A".into(), loading: true, can_go_back: true, can_go_forward: false, error: None, dialog: None, imported: false, extensions: vec![], engine: String::new(), certificate: None, auth: None, find: None, zoom: 100 });
    }

    #[test]
    fn a_state_reads_the_certificate_the_password_sheet_the_find_and_the_zoom_and_the_commands_name_them() {
        let event = json!({ "url": "https://a.b/", "engine": "chromium", "certificate": { "site": "a.b:8443", "reason": "self-signed" },
            "auth": { "site": "a.b" }, "find": { "matches": 3, "current": 2, "found": true }, "zoom": 110 });
        let state = WebState::from_event(&event);
        assert_eq!(state.engine, "chromium");
        assert_eq!(state.certificate, Some(("a.b:8443".into(), "self-signed".into())));
        assert_eq!(state.auth.as_deref(), Some("a.b"));
        assert_eq!(state.find, Some(WebFind { matches: Some(3), current: 2, found: true }));
        assert_eq!(state.zoom, 110);
        let bare = WebState::from_event(&json!({ "url": "https://a.b/", "zoom": 0, "find": { "found": false } }));
        assert_eq!((bare.certificate, bare.auth, bare.zoom), (None, None, 100));
        assert_eq!(bare.find, Some(WebFind { matches: None, current: 0, found: false }));
        assert_eq!(web_certificate(true, 1), json!({ "action": "certificate", "proceed": true, "token": 1 }));
        assert_eq!(web_find("cat", false, true, 2), json!({ "action": "find", "text": "cat", "forward": false, "again": true, "token": 2 }));
        assert_eq!(web_stop_find(3), json!({ "action": "stopFind", "token": 3 }));
        assert_eq!(web_zoom("in", 4), json!({ "action": "zoom", "do": "in", "token": 4 }));
        assert_eq!(web_inspect(5), json!({ "action": "inspect", "token": 5 }));
        // A page names the keys it takes, and hears each as it wrote it.
        let heard = Rc::new(RefCell::new(String::new()));
        let seen = heard.clone();
        let (tree, handlers) = export(vec![Node::new("Page").shortcuts(&["cmd-t", "cmd-shift-g"], move |key| *seen.borrow_mut() = key.to_owned())]);
        assert_eq!(tree[0]["props"]["shortcuts"], json!(["cmd-t", "cmd-shift-g"]));
        assert!(handlers.call(tree[0]["props"]["onShortcut"].as_str().unwrap(), &json!({ "key": "cmd-shift-g" })));
        assert_eq!(*heard.borrow(), "cmd-shift-g");
    }

    #[test]
    fn a_popup_and_a_download_read_what_the_app_sends_and_a_download_command_names_one() {
        let popup = WebPopup::from_event(&json!({ "url": "https://login.example/", "background": false, "popup": 4 }));
        assert_eq!(popup, WebPopup { url: "https://login.example/".into(), background: false, popup: Some(4), blocked: false });
        assert_eq!(WebPopup::from_event(&json!({ "url": "https://ads.example/", "blocked": true })).blocked, true);
        let event = json!({ "id": 3, "url": "https://a.b/r.pdf", "name": "r.pdf", "path": "/Users/me/Downloads/r.pdf", "state": "progress", "received": 10, "total": null, "error": null });
        assert_eq!(
            WebDownload::from_event(&event),
            WebDownload { id: 3, url: "https://a.b/r.pdf".into(), name: "r.pdf".into(), path: "/Users/me/Downloads/r.pdf".into(), state: "progress".into(), received: 10, total: None, error: None }
        );
        assert_eq!(web_download("reveal", 3, 9), json!({ "action": "download", "do": "reveal", "id": 3, "token": 9 }));
    }

    #[test]
    fn a_state_reads_the_extensions_buttons_and_a_click_is_a_command_naming_one() {
        let event = json!({ "url": "https://a.b/", "extensions": [
            { "id": "e1", "name": "Bitwarden", "label": "Bitwarden", "icon": "ext-btn-1", "badge": "3", "enabled": true },
            { "id": "e2", "name": "React Developer Tools", "label": "React", "icon": null, "badge": "", "enabled": false },
            { "name": "no id" },
        ] });
        let state = WebState::from_event(&event);
        assert_eq!(
            state.extensions,
            [
                ExtensionButton { id: "e1".into(), name: "Bitwarden".into(), label: "Bitwarden".into(), icon: Some("ext-btn-1".into()), badge: "3".into(), enabled: true },
                ExtensionButton { id: "e2".into(), name: "React Developer Tools".into(), label: "React".into(), icon: None, badge: String::new(), enabled: false },
            ]
        );
        assert!(WebState::from_event(&json!({ "url": "https://a.b/" })).extensions.is_empty(), "an older Wings sends none");
        assert_eq!(web_extension_action("e1", 9), json!({ "action": "extension", "id": "e1", "token": 9 }));
    }

    #[test]
    fn small_kinds_send_the_props_the_app_reads() {
        assert_eq!(props(icon_named("trash", 14)), json!(["Icon", { "name": "trash", "size": 14 }]));
        assert_eq!(props(avatar("Ada", false)), json!(["Avatar", { "name": "Ada" }]));
        assert_eq!(props(avatar("Ada", true)), json!(["Avatar", { "name": "Ada", "size": "large" }]));
        assert_eq!(props(skeleton(120, 14.0)), json!(["Skeleton", { "width": 120, "height": 14.0 }]));
        assert_eq!(props(skeleton("60%", 14.0)), json!(["Skeleton", { "width": "60%", "height": 14.0 }]));
        assert_eq!(props(terminal("7", 220)), json!(["Terminal", { "pane": "7", "height": 220 }]));
        assert_eq!(props(meter("2 of 5 done", 40.0, None)), json!(["Meter", { "label": "2 of 5 done", "percent": 40.0 }]));
        assert_eq!(props(meter("Full", 100.0, Some("danger"))), json!(["Meter", { "label": "Full", "percent": 100.0, "tone": "danger" }]));
        assert_eq!(props(bar("bottom", [])), json!(["Bar", { "edge": "bottom" }]));
        assert_eq!(props(menu(text("Menu"), [], true)), json!(["Menu", { "align": "end" }]));
        assert_eq!(props(menu(text("Menu"), [], false)), json!(["Menu", {}]));
        assert_eq!(props(hover_card(text("PR"), text("details"), 300)), json!(["HoverCard", { "width": 300 }]));
    }

    #[test]
    fn layout_kinds_put_their_children_in_the_slots_the_app_takes() {
        assert_eq!(props(split(text("a"), text("b"), 280.0)), json!(["Split", { "defaultSize": 280.0 }]));
        assert_eq!(slots(split(text("a"), text("b"), 280.0)), ["start", "end"]);
        assert_eq!(slots(hover_card(text("PR"), text("details"), 300)), ["-", "card"]);
        assert_eq!(props(board("Issues", [])), json!(["Board", { "label": "Issues" }]));
        assert_eq!(props(board_lanes("Issues", board_row([]), [])), json!(["Board", { "label": "Issues", "lanes": true }]));
        assert_eq!(slots(board_lanes("Issues", board_row([]), [])), ["header", "body"]);
        assert_eq!(props(board_column(text("Todo"), Some(3), Some(text("x")))), json!(["BoardColumn", { "count": 3 }]));
        assert_eq!(slots(board_column(text("Todo"), None, Some(text("x")))), ["title", "body"]);
        assert_eq!(slots(board_column(text("Todo"), None, None)), ["title"]);
        assert_eq!(props(board_cell("Todo", text("x"))), json!(["BoardColumn", { "label": "Todo", "cell": true }]));
        assert_eq!(props(board_row([])), json!(["BoardRow", {}]));
    }

    #[test]
    fn a_sunburst_sends_its_wedges_as_the_arrays_the_app_parses() {
        let wedges = [Wedge { from: 0.0, to: 3.0, ring: 0, color: 0xff, alpha: 1.0 }, Wedge { from: 3.0, to: 6.0, ring: 1, color: 0, alpha: 0.3 }];
        assert_eq!(props(sunburst(2, &wedges)), json!(["Sunburst", { "rings": 2, "wedges": [[0.0, 3.0, 0, 255, 1.0], [3.0, 6.0, 1, 0, 0.3]] }]));
    }

    #[test]
    fn a_rail_and_a_text_box_hand_back_what_the_app_sends() {
        let picked = Rc::new(Cell::new(usize::MAX));
        let seen = picked.clone();
        let (tree, handlers) = export(vec![prompt_rail(&[("Fix it", "Done."), ("Ship it", "")], 1, Some(760), move |at| seen.set(at))]);
        let rail = &tree[0];
        assert_eq!(rail["props"]["marks"], json!([{ "title": "Fix it", "preview": "Done." }, { "title": "Ship it", "preview": "" }]));
        assert_eq!((&rail["props"]["current"], &rail["props"]["beside"]), (&json!(1), &json!(760)));
        assert!(handlers.call(rail["props"]["onSelect"].as_str().unwrap(), &json!({ "index": 1 })));
        assert_eq!(picked.get(), 1);

        let typed = Rc::new(RefCell::new(String::new()));
        let kept = typed.clone();
        let (tree, handlers) = export(vec![textarea("Notes", "hi", 6, move |text| *kept.borrow_mut() = text)]);
        let area = &tree[0];
        assert_eq!((&area["type"], &area["props"]["label"], &area["props"]["value"], &area["props"]["rows"]), (&json!("Textarea"), &json!("Notes"), &json!("hi"), &json!(6)));
        assert!(handlers.call(area["props"]["onChange"].as_str().unwrap(), &json!({ "value": "hello" })));
        assert_eq!(*typed.borrow(), "hello");
    }

    /// The pages `apps/native/examples/shot_plugins.rs --ui-gallery` renders, drawn with the builders of this module, so a
    /// builder that sends a prop the app doesn't read shows in the screenshots. The committed JSON is what the shot reads;
    /// running this test with `WINGS_UPDATE_GALLERY=1` set rewrites it.
    fn gallery() -> Value {
        use crate::ui::{badge, button, card, heading, muted, page, row, stack, toolbar};
        let titled = |title: &str, body: Node| stack().prop("gap", "s").child(heading(3, title)).child(body);
        let issue = |title: &str| card().child(text(title)).child(muted("WIN-1").prop("size", "small"));
        let widgets = page(
            toolbar("Builders", None),
            [stack().prop("gap", "l").children([
                titled("Icon and Avatar", row().prop("gap", "m").prop("align", "center").children([
                    icon_named("check", 16), icon_named("chevron-down", 16), icon_named("copy", 16), icon_named("trash", 16), icon_named("claude", 24),
                    avatar("Ada Lovelace", false), avatar("Grace Hopper", true),
                ])),
                titled("Meter", stack().prop("gap", "s").children([meter("Plain", 35.0, None), meter("Success", 80.0, Some("success")), meter("Warn", 60.0, Some("warn")), meter("Danger", 95.0, Some("danger"))])),
                titled("Skeleton", stack().prop("gap", "xs").children([skeleton("100%", 14.0), skeleton("70%", 14.0), skeleton(160, 14.0)])),
                titled("Menu and HoverCard", row().prop("gap", "m").prop("align", "center").children([
                    menu(button("Actions", "outline", false, || {}), [button("Rename", "ghost", false, || {}).key("rename"), button("Delete", "ghost", false, || {}).key("delete")], false),
                    hover_card(badge("Point at me", None), card().child(text("The card shows over its trigger.")), 300),
                ])),
                titled("Textarea", textarea("Notes", "Two rows of text,\nthe second one here.", 3, |_| {})),
                titled("Board", board("Issues", [
                    board_column(text("Todo"), Some(2), Some(stack().prop("gap", "s").children([issue("Fix the total").key("a"), issue("Cache the lookup").key("b")]))).key("todo"),
                    board_column(text("Doing"), Some(1), Some(stack().prop("gap", "s").child(issue("Port the board")))).key("doing"),
                    board_column(text("Done"), None, Some(muted("Nothing yet."))).key("done"),
                ])),
                titled("Board in lanes", board_lanes(
                    "Issues by parent",
                    board_row([board_column(text("Todo"), Some(1), None).key("todo"), board_column(text("Done"), Some(1), None).key("done")]),
                    [stack().prop("gap", "s").key("lane").child(text("Epic: ship the SDK").prop("weight", "medium")).child(board_row([board_cell("Todo", issue("Builders")).key("todo"), board_cell("Done", issue("Wrappers")).key("done")]))],
                )),
            ])],
        );
        let wedges = [
            Wedge { from: 0.0, to: 3.6, ring: 0, color: 0x3b82f6, alpha: 1.0 },
            Wedge { from: 3.6, to: 6.28, ring: 0, color: 0x22c55e, alpha: 1.0 },
            Wedge { from: 0.0, to: 1.8, ring: 1, color: 0x3b82f6, alpha: 0.6 },
            Wedge { from: 1.8, to: 3.6, ring: 1, color: 0x3b82f6, alpha: 0.35 },
            Wedge { from: 3.6, to: 6.28, ring: 1, color: 0x22c55e, alpha: 0.6 },
        ];
        let marks = [("Fix the checkout total", "I read the cart module first."), ("Add a test", "It fails on the old code."), ("Ship it", "Merged.")];
        let panes = page(
            toolbar("Layout", None),
            [split(
                stack().prop("grow", true).child(sunburst(2, &wedges).prop("title", "src").prop("detail", "12 files")),
                stack().prop("gap", "s").prop("grow", true).children([
                    bar("bottom", [row().prop("gap", "s").prop("align", "center").children([text("A bar with its rule below"), button("Act", "outline", false, || {})])]),
                    split(stack().prop("grow", true).child(prompt_rail(&marks, 1, None, |_| {})), stack().child(text("The conversation beside the rail.")), 64.0),
                ]),
                320.0,
            )],
        );
        let strip = |label: &str, children: Vec<Node>| tab_strip(label, children, |_| {}, |_| {}, || {}, |_| {});
        let work = [("g1", "Fix login bug", "info"), ("g2", "Release 0.2", "success"), ("g3", "Flaky CI", "warning"), ("g4", "Outage", "danger")];
        let mut few = vec![tab("t1", "Start page", "", false, false, None), tab_group("g1", "Fix login bug", "info", false, 2)];
        few.extend([tab("t2", "Sign in \u{b7} acme web", "http://localhost:5173/login", true, false, Some("g1")), tab("t3", "Loading the report", "https://acme.test/report", false, true, Some("g1"))]);
        few.extend([tab_group("g2", "Release 0.2", "success", true, 3), tab("t4", "Changelog", "https://acme.test/changelog", false, false, Some("g2"))]);
        few.push(tab("t5", "Intl.NumberFormat - JavaScript | MDN", "https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Intl/NumberFormat", false, false, None));
        let mut many = Vec::new();
        for (at, (id, name, tone)) in work.iter().enumerate() {
            many.push(tab_group(id, name, tone, false, 3));
            for n in 0..3 {
                many.push(tab(&format!("m{at}{n}"), &format!("{name} \u{b7} page {}", n + 1), "https://acme.test/", at == 1 && n == 1, false, Some(id)));
            }
        }
        let tabs = page(
            toolbar("Tabs", None),
            [stack().prop("gap", "l").children([
                titled("A few tabs, two groups, one folded and one tab loading", strip("A few tabs", few)),
                titled("More tabs than fit: they stop shrinking and the strip scrolls", strip("Many tabs", many)),
            ])],
        );
        let root = |nodes: Vec<Node>| json!({ "id": 0, "type": "#root", "children": crate::ui::export(nodes).0 });
        json!({ "widgets": root(vec![widgets]), "panes": root(vec![panes]), "tabs": root(vec![tabs]) })
    }

    #[test]
    fn the_gallery_the_shot_renders_is_the_one_the_builders_draw() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/native/examples/shot_plugins/ui_gallery.json");
        let drawn = serde_json::to_string_pretty(&gallery()).unwrap() + "\n";
        if std::env::var_os("WINGS_UPDATE_GALLERY").is_some() {
            std::fs::write(&path, &drawn).unwrap();
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(committed == drawn, "{} is out of date: run this test with WINGS_UPDATE_GALLERY=1 set", path.display());
    }
}
