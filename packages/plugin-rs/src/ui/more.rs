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
/// or `fill: true` to take the rest of its container's height, and `onState` (`WebState::from_event`). It is drawn
/// under a dialog or sheet of the page and an app menu, and is an empty area in a headless render.
pub fn web_view(name: &str) -> Node {
    Node::new("WebView").prop("name", name)
}

/// The `command` prop of a `web_view`: `action` is `load` (with `url`), `back`, `forward`, `reload` or `stop`. Each new
/// `token` runs once, like `Scroll`'s `scrollTo`, so the page keeps a counter and raises it for every command.
pub fn web_command(action: &str, url: Option<&str>, token: u64) -> Value {
    json!({ "action": action, "url": url, "token": token })
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
    fn a_web_view_sends_its_name_and_a_state_reads_what_the_app_sends() {
        assert_eq!(props(web_view("main").prop("fill", true)), json!(["WebView", { "name": "main", "fill": true }]));
        assert_eq!(web_command("load", Some("https://a.b"), 3), json!({ "action": "load", "url": "https://a.b", "token": 3 }));
        let heard = Rc::new(RefCell::new(WebState::default()));
        let seen = heard.clone();
        let (tree, handlers) = export(vec![web_view("main").on("onState", move |event| *seen.borrow_mut() = WebState::from_event(event))]);
        let event = json!({ "url": "https://a.b/", "title": "A", "loading": true, "canGoBack": true, "canGoForward": false, "error": null });
        assert!(handlers.call(tree[0]["props"]["onState"].as_str().unwrap(), &event));
        assert_eq!(*heard.borrow(), WebState { url: "https://a.b/".into(), title: "A".into(), loading: true, can_go_back: true, can_go_forward: false, error: None });
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
        let root = |nodes: Vec<Node>| json!({ "id": 0, "type": "#root", "children": crate::ui::export(nodes).0 });
        json!({ "widgets": root(vec![widgets]), "panes": root(vec![panes]) })
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
