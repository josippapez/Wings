//! The tree a page sends, parsed from its JSON and checked, so the view draws only what it knows and a page can't
//! make it hold or draw more than these limits.
//!
//! A page's host components are `wings-plugin-ui/native`'s: a component with named parts (a page's toolbar, an empty
//! state's description) sends each part as a `Slot` child, and a component the host doesn't know is dropped with
//! everything in it.

use serde::Deserialize;
use serde_json::{Map, Value};
use std::fmt;

pub mod code;
mod kit;
mod picture;
pub mod prompt_rail;
mod sunburst;
mod svg;
mod tab_strip;
pub use code::Code;
pub use picture::{Picture, room_in as picture_room, valid_id as valid_image_id};
pub use prompt_rail::PromptRail;
pub use sunburst::{Geometry, Hit, Sunburst, Wedge};
pub use svg::Svg;
pub use tab_strip::{GroupTone, Tab, TabGroup, TabItem, TabStrip};

/// The largest tree JSON the host parses.
pub const MAX_JSON_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_NODES: usize = 20_000;
pub const MAX_DEPTH: usize = 32;
/// A longer text is cut to this many characters: a long paragraph of a chat's reply fits.
pub const MAX_TEXT_CHARS: usize = 20_000;
const MAX_HANDLER_BYTES: usize = 64;
/// A select shows this many of its options; the rest are dropped.
pub const MAX_OPTIONS: usize = 200;
/// The most children a `Scroll`'s `track` names.
const MAX_TRACKED: usize = 2_000;
/// A text input holds this many characters of what the page gave it. The rest of the page's text is cut to
/// `MAX_TEXT_CHARS`, which would cut what the person typed in a long field.
pub const MAX_INPUT_CHARS: usize = 200_000;

#[derive(Debug, PartialEq, Eq)]
pub enum TreeError {
    TooLarge,
    TooManyNodes,
    TooDeep,
    Invalid(String),
}

impl fmt::Display for TreeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TreeError::TooLarge => write!(f, "the page's tree is over {MAX_JSON_BYTES} bytes"),
            TreeError::TooManyNodes => write!(f, "the page's tree has over {MAX_NODES} nodes"),
            TreeError::TooDeep => write!(f, "the page's tree is nested over {MAX_DEPTH} deep"),
            TreeError::Invalid(why) => write!(f, "the page's tree isn't valid: {why}"),
        }
    }
}

impl std::error::Error for TreeError {}

/// A string a component prop can pick from, by its name in ui.d.ts.
trait Choice: Sized {
    fn choose(text: &str) -> Option<Self>;
}

macro_rules! choice {
    ($(#[$doc:meta])* $name:ident { $($variant:ident = $text:literal),+ $(,)? }) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        pub enum $name {
            #[default]
            $($variant),+
        }

        impl Choice for $name {
            fn choose(text: &str) -> Option<Self> {
                match text {
                    $($text => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

// The first variant of each is what a missing or unknown value means, as in ui.d.ts.
choice!(Gap { M = "m", None = "none", Xs = "xs", S = "s", L = "l" });
choice!(Align { Center = "center", Start = "start", Baseline = "baseline" });
choice!(Justify { Start = "start", End = "end", Between = "between", Center = "center" });
choice!(Edge { None = "none", Top = "top", Bottom = "bottom" });
choice!(
    /// `field` is native.js's own, for a `Field`'s label and hint.
    TextSize { Normal = "normal", Small = "small", Tiny = "tiny", Large = "large", Field = "field" }
);
choice!(Weight { Normal = "normal", Medium = "medium", Strong = "strong" });
choice!(
    /// The names `Icon` draws.
    IconName { Check = "check", ChevronDown = "chevron-down", Copy = "copy", Trash = "trash", Claude = "claude" }
);
choice!(
    /// A button's `size`, as plugin-ui.css's `.w-button[data-size]`.
    ButtonSize { Default = "default", Xs = "xs", Sm = "sm", Lg = "lg", Icon = "icon", IconXs = "icon-xs", IconSm = "icon-sm", IconLg = "icon-lg" }
);
choice!(
    /// The variants of both `Button` and `Badge`.
    Variant { Default = "default", Secondary = "secondary", Outline = "outline", Ghost = "ghost", Destructive = "destructive", Link = "link" }
);

/// A colour that says something. A text or badge without one has the plain colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Muted,
    Info,
    Success,
    Warn,
    Danger,
}

impl Choice for Tone {
    fn choose(text: &str) -> Option<Self> {
        Some(match text {
            "muted" => Tone::Muted,
            "info" => Tone::Info,
            "success" => Tone::Success,
            "warn" => Tone::Warn,
            "danger" => Tone::Danger,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tree {
    pub children: Vec<Node>,
    /// The page lays itself out edge to edge with no padding, as a `{ fill: true }` page does on the desktop.
    pub fill: bool,
    /// The colours the page paints in place of the app's, as a mod's theme gave them. None paints in the app's.
    pub palette: Option<crate::theme::Palette>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    /// A string a component holds, like a button's label.
    Str {
        id: u64,
        text: String,
    },
    Slot(Slot),
    Page(Page),
    Toolbar(Toolbar),
    Stack(Stack),
    Row(Row),
    Text(Text),
    Button(Button),
    Notice(Notice),
    Toast(Toast),
    Spinner(Spinner),
    Checkbox(Checkbox),
    Confirm(Confirm),
    Badge(Badge),
    Empty(Empty),
    Loading(Loading),
    Card(Card),
    Heading(Heading),
    Dot(Dot),
    Gauge(Gauge),
    Meter(Meter),
    Board(Board),
    BoardRow(List),
    BoardColumn(BoardColumn),
    List(List),
    ListItem(ListItem),
    Disclosure(Disclosure),
    Select(Select),
    Chips(Select),
    TabStrip(TabStrip),
    Combobox(Combobox),
    Menu(Menu),
    ListButton(ListButton),
    SearchField(SearchField),
    Input(Input),
    Icon(Icon),
    Svg(Svg),
    Image(Picture),
    Graph(Graph),
    Avatar(Avatar),
    Skeleton(Skeleton),
    Scroll(Scroll),
    Split(Split),
    Bar(Bar),
    Table(Table),
    Sunburst(Sunburst),
    PromptRail(PromptRail),
    Dialog(Dialog),
    Sheet(Sheet),
    HoverCard(HoverCard),
    PopupMenu(PopupMenu),
    Terminal(Terminal),
    WebView(WebView),
    Code(Code),
    Actions(Actions),
}

/// A dialog's or a confirm's buttons, as the app lays them out: at the end of the line, the ones that dismiss first and
/// in the ghost variant, then the ones that act.
#[derive(Clone, Debug, PartialEq)]
pub struct Actions {
    pub id: u64,
    pub children: Vec<Node>,
}

/// A named part of the component it's in.
#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    pub name: String,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub id: u64,
    pub toolbar: Vec<Node>,
    pub flush: bool,
    pub body: Vec<Node>,
    /// `onTextScale`: the page sizes its own text, so Cmd-+, Cmd-- and Cmd-0 send it `{ "step": "bigger" | "smaller" |
    /// "reset" }` while its tab has the keyboard, in place of changing the terminals' font.
    pub on_text_scale: Option<String>,
    /// What Wings draws behind the whole page in place of its plain ground (`backdrop`), where the page fills a tab.
    pub backdrop: Option<Backdrop>,
    /// `column`: the width in points of the one centred column the body is set in and the toolbar's content lines up
    /// with. A reading page (a detail view, a form) has one; a wide page (a list, a board) doesn't.
    pub column: Option<f32>,
    /// The `toasts` slot: small cards floating over the body's foot, which don't scroll with it.
    pub toasts: Vec<Node>,
}

/// A page's `backdrop`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Backdrop {
    /// `"pixels"` or `"pixels:<pattern>"`: a slow pattern of dithered pixels in the page's colours (`pixels`,
    /// `ui::backdrop`). A pattern this Wings doesn't know is the drift, `"pixels"`'s own.
    Pixels(crate::pixels::Pattern),
    /// `"gradient:<preset>"`: a slow mesh of a preset's three colours, dimmed under the page's text (`gradients`).
    Gradient(crate::gradients::Preset),
}

impl Backdrop {
    /// The backdrop a page's `backdrop` names, if Wings draws it.
    fn named(name: &str) -> Option<Backdrop> {
        match name {
            "pixels" => Some(Backdrop::Pixels(crate::pixels::Pattern::Drift)),
            _ => match name.strip_prefix("pixels:") {
                Some(pattern) => {
                    Some(Backdrop::Pixels(crate::pixels::Pattern::named(pattern).unwrap_or(crate::pixels::Pattern::Drift)))
                }
                None => name.strip_prefix("gradient:").and_then(crate::gradients::Preset::named).map(Backdrop::Gradient),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Toolbar {
    pub id: u64,
    pub title: Vec<Node>,
    pub filters: Vec<Node>,
    pub actions: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stack {
    pub id: u64,
    pub gap: Gap,
    pub grow: bool,
    /// `fill: true`: it takes what is left of its parent's height and no more, so a `Scroll` in it scrolls rather than push
    /// what follows out of view. A `grow` stack keeps the height of what it holds.
    pub fill: bool,
    /// A fixed width in points, for a column beside others.
    pub width: Option<f32>,
    /// At most this wide, centred: a reading column, as plugin-ui.css's `.w-stack[data-max-width]`.
    pub max_width: Option<f32>,
    /// `enter: "rise"`: a child that wasn't there before fades in and rises into place, as a chat's new message does.
    pub enter: bool,
    /// `links: true`: every `Text` in it finds Wings' links, as one with `links: true` does.
    pub links: bool,
    /// What to send back to the page when the right mouse button is pressed inside it: where, which of its children was
    /// under the pointer and what link or code was (`onContextMenu`).
    pub on_context_menu: Option<String>,
    /// Set only by a component the app lays out (`ListRow`, `Section`), never by a page: in by a `ListButton`'s border and
    /// padding, so what it holds lines up with the list buttons beside it without being a button.
    pub inset: bool,
    /// Set only by a `Section`: at most this wide from its left edge, so a description reads at a measure without being
    /// centred as a `maxWidth` column is.
    pub measure: Option<f32>,
    /// Set only by a `ListRow` with a `trailing` control: its words keep at least this share of the row's width, so a wide
    /// control goes under them instead of squeezing them to a letter a line.
    pub min_share: Option<f32>,
    /// `quote: true`: a bar down its left edge and what it holds set in from it, as a Markdown quote is drawn. The bar is
    /// part of the box, so it runs the quote's whole height and a selection never copies it.
    pub quote: bool,
    /// `pt`: room above it in points, as a heading has in a column of Markdown.
    pub pt: Option<f32>,
    /// `floatBottom: true`: out of the flow, over the foot of its parent, centred, so a small control (a round "to the latest"
    /// button) sits over a scroll area without taking a row of its own.
    pub float_bottom: bool,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: u64,
    pub gap: Gap,
    pub align: Align,
    pub justify: Justify,
    pub wrap: bool,
    pub grow: bool,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub id: u64,
    pub tone: Option<Tone>,
    pub size: TextSize,
    pub weight: Weight,
    pub mono: bool,
    /// `italic: true`: slanted, as Markdown's emphasis is. Only a nested `Text` inside a line draws it.
    pub italic: bool,
    /// `strike: true`: a line through it, as Markdown's `~~words~~`. Only a nested `Text` inside a line draws it.
    pub strike: bool,
    pub truncate: bool,
    /// `shrink: "last"`: beside other text in a row, it keeps its width while they have any to give.
    pub shrink_last: bool,
    /// `expandable: true` with `truncate`: the app adds More after the text while it is cut short, and Less once it
    /// is opened, so a page never guesses from its length whether it fits.
    pub expandable: bool,
    pub clamp: bool,
    pub grow: bool,
    /// What to send back to the page when the text is clicked, for a link inside a line of text.
    pub on_click: Option<String>,
    /// The hover text, drawn as the app's tooltip.
    pub title: Option<String>,
    /// It pulses softly, as a shimmer does, while something it names is under way, like "Thinking".
    pub shimmer: bool,
    /// `mark: true`: a word a search found, marked as Past sessions marks it.
    pub mark: bool,
    /// Milliseconds since the epoch: the text ends with how long ago that was, counting up while it shows, as in
    /// "Running pnpm test · 16s". The app counts, so the page needn't send a new tree each second.
    pub since: Option<i64>,
    /// `links: true`: words in it that a plugin that is on declares in `contributes.links` are links to that plugin.
    pub links: bool,
    /// `href`: the `https:` address a link inside a line goes to, which a right-click on it tells the page
    /// (`onContextMenu`). The page still opens it with its `onClick`.
    pub href: Option<String>,
    pub children: Vec<Node>,
}

/// A terminal the agent of the page's tab opened, by its pane id, drawn live where the node is, `height` points tall.
/// The app draws only a terminal it handed to that tab, and nothing for any other.
#[derive(Clone, Debug, PartialEq)]
pub struct Terminal {
    pub id: u64,
    pub pane: String,
    pub height: f32,
    /// Takes the height its container leaves (the whole column of a docked split, shared with the others in it), not `height`.
    pub fill: bool,
}

/// A web page drawn by the system's web engine where the node is, for a plugin whose manifest asks for `webView`. Wings
/// keeps the engine's view by `name` while the page sends the node, so a new tree never loads the page again: `url` is
/// only where it starts, and `command` (one, or a list of them in the order sent, each run once: those whose `token` is
/// past the last one run) loads, goes back or forward, reloads or stops. A list lets a page send two before its next
/// tree goes out (an agent's step and a click) without dropping one. `onState` hears where it is: `{ url, title,
/// loading, canGoBack, canGoForward, error }`.
#[derive(Clone, Debug, PartialEq)]
pub struct WebView {
    pub id: u64,
    pub name: String,
    pub url: Option<String>,
    pub commands: Vec<WebCommand>,
    /// `keepData: false` keeps its cookies and site data only while Wings runs, in a store apart from the kept one.
    pub keep_data: bool,
    pub height: f32,
    pub fill: bool,
    pub on_state: Option<String>,
    /// Hears what an `eval` or `snapshot` command came to: `{ token, value }` or `{ token, error }`.
    pub on_result: Option<String>,
    /// Draws no place and takes no room, yet its view is made and keeps its commands and tellings: a tab in the
    /// background.
    pub hidden: bool,
    /// With `hidden`: the engine keeps it rendering, so screenshots and scripts work on it.
    pub live: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WebCommand {
    pub go: WebGo,
    pub token: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum WebGo {
    Load(String),
    Back,
    Forward,
    Reload,
    Stop,
    /// Runs `script`, the body of an async function, in the page's own world (`page`, where its scripts' variables are)
    /// or one of the plugin's that sees only the page's document, and answers what it returns, as JSON.
    Eval { script: String, page: bool },
    /// A picture of what shows of the page, as a PNG.
    Snapshot,
    /// Answers the page's open `alert`, `confirm` or `prompt` (`State::dialog`): OK with `text` for a prompt, or Cancel.
    Dialog { accept: bool, text: Option<String> },
    /// The page's recent console messages and `fetch`/`XMLHttpRequest` calls, for a plugin that may run scripts.
    Logs,
    /// Clicks the button of the extension of this id for the page: its popup opens next to the button, or it hears the
    /// click. Only right after your click in the page (`web::needs_click`).
    Extension(String),
}

/// The most bytes of a script an `eval` command runs: a page's helper, not a library.
const WEB_SCRIPT_MAX: usize = 256 * 1024;

/// The most commands a `WebView` takes in one tree.
const WEB_COMMANDS_MAX: usize = 32;

/// The most characters of a `WebView`'s `name`, which is a key and never shown.
const WEB_NAME_MAX: usize = 64;

/// An address a `WebView` may load: `http`, `https` or `about:blank`, up to 8 KB. A `file:`, `javascript:` or `data:`
/// address would read this computer's files or run the plugin's own script in the page, so none of those.
pub fn web_address(url: &str) -> Option<String> {
    let url = url.trim();
    let lower = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    let allowed = lower.starts_with("https://") || lower.starts_with("http://") || url == "about:blank";
    (allowed && url.len() <= 8192 && !url.chars().any(char::is_control)).then(|| url.to_owned())
}

fn web_command(value: &Value) -> Option<WebCommand> {
    let token = value.get("token")?.as_u64()?;
    let go = match value.get("action")?.as_str()? {
        "load" => WebGo::Load(web_address(value.get("url")?.as_str()?)?),
        "back" => WebGo::Back,
        "forward" => WebGo::Forward,
        "reload" => WebGo::Reload,
        "stop" => WebGo::Stop,
        "eval" => WebGo::Eval {
            script: value.get("script")?.as_str().filter(|script| script.len() <= WEB_SCRIPT_MAX)?.to_owned(),
            page: value.get("world").and_then(Value::as_str) == Some("page"),
        },
        "snapshot" => WebGo::Snapshot,
        "dialog" => WebGo::Dialog {
            accept: value.get("accept")?.as_bool()?,
            text: value.get("text").and_then(Value::as_str).map(|text| text.chars().take(10_000).collect()),
        },
        "logs" => WebGo::Logs,
        // An extension's id as Wings named it (`web::extensions`): letters and digits, never a path.
        "extension" => WebGo::Extension(
            value.get("id")?.as_str().filter(|id| (1..=64).contains(&id.len()) && id.bytes().all(|byte| byte.is_ascii_alphanumeric()))?.to_owned(),
        ),
        _ => return None,
    };
    Some(WebCommand { go, token })
}

#[derive(Clone, Debug, PartialEq)]
pub struct Button {
    pub id: u64,
    pub variant: Variant,
    pub size: ButtonSize,
    /// A `Chip`: plugin-ui.css's `.w-chip`, tinted while `pressed`.
    pub chip: bool,
    pub pressed: bool,
    pub disabled: bool,
    /// The name a screen reader says, when it isn't the button's text (an icon button's).
    pub label: Option<String>,
    /// The hover text, drawn as the app's tooltip. It describes the button; `label` still names it.
    pub title: Option<String>,
    /// Its text wraps onto the lines it needs, from its start, and it's as tall as they are: an issue's title that is
    /// a button, as plugin-ui.css's `.w-button[data-wrap]`.
    pub wrap: bool,
    /// `round: true`: fully rounded, a circle at an icon size, as a chat's send button.
    pub round: bool,
    /// What to send back to the page when it's clicked. A disabled button has none.
    pub on_click: Option<String>,
    pub on_key_down: Option<String>,
    /// The key that clicks it while its page's tab has the keyboard, shown in its tooltip.
    pub shortcut: Option<Shortcut>,
    /// A dim second line under the text of an item of a `PopupMenu`.
    pub description: Option<String>,
    /// `showOnHover: true`: inside a `Card`, drawn only while the pointer is on the card or the button has the
    /// keyboard, as a board card's actions.
    pub show_on_hover: bool,
    pub children: Vec<Node>,
}

/// A `Button`'s `shortcut`, as `"cmd-shift-p"` writes it: Cmd or Ctrl and maybe Alt and Shift, with a letter, a digit, a
/// punctuation key or F1 to F12. A key the app or a text field answers to is refused, so a page can't take one away.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub cmd: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// As GPUI names the key: lower case, `"f5"`.
    pub key: String,
}

/// The app's keys (`actions.rs`) and a text field's, as `Shortcut::canonical` writes them, with Cmd-N kept for a new
/// window as on macOS. Elsewhere the app's keys are Ctrl-Shift, which `Shortcut::parse` refuses as a whole.
const RESERVED: &[&str] = &[
    "cmd-q", "cmd-w", "shift-cmd-w", "cmd-t", "cmd-n", "cmd-,", "ctrl-,", "cmd-[", "cmd-]", "alt-cmd-[", "alt-cmd-]",
    "cmd-d", "shift-cmd-d", "cmd-f", "cmd-c", "cmd-v", "cmd-x", "cmd-a", "cmd-z", "shift-cmd-z", "cmd-=",
    "shift-cmd-=", "cmd--", "cmd-0", "cmd-1", "cmd-2", "cmd-3", "cmd-4", "cmd-5", "cmd-6", "cmd-7", "cmd-8", "cmd-9",
    "cmd-b", "cmd-\\", "cmd-k", "ctrl-`", "cmd-h", "alt-cmd-h", "cmd-m", "ctrl-cmd-f", "ctrl-a", "ctrl-c", "ctrl-v",
    "ctrl-x", "ctrl-z",
];

impl Shortcut {
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().to_ascii_lowercase();
        let (mods, key) = match text.strip_suffix("--") {
            Some(mods) => (mods, "-"),
            None => text.rsplit_once('-')?,
        };
        let mut shortcut = Self { cmd: false, ctrl: false, alt: false, shift: false, key: key.to_owned() };
        for part in mods.split('-') {
            match part {
                "cmd" => shortcut.cmd = true,
                "ctrl" => shortcut.ctrl = true,
                "alt" => shortcut.alt = true,
                "shift" => shortcut.shift = true,
                _ => return None,
            }
        }
        let plain = matches!(key.as_bytes(), [b'a'..=b'z' | b'0'..=b'9' | b'`' | b'-' | b'=' | b'[' | b']' | b'\\' | b';' | b'\'' | b',' | b'.' | b'/']);
        let function = key.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()).is_some_and(|n| (1..=12).contains(&n));
        let usable = (plain || function)
            && (shortcut.cmd || shortcut.ctrl)
            && !(shortcut.ctrl && shortcut.shift && !shortcut.cmd)
            && !RESERVED.contains(&shortcut.canonical().as_str());
        usable.then_some(shortcut)
    }

    /// GPUI's order: `ctrl-alt-shift-cmd-key`.
    pub fn canonical(&self) -> String {
        let flag = |on: bool, name: &'static str| if on { name } else { "" };
        format!("{}{}{}{}{}", flag(self.ctrl, "ctrl-"), flag(self.alt, "alt-"), flag(self.shift, "shift-"), flag(self.cmd, "cmd-"), self.key)
    }

    /// As the app writes a shortcut in a tooltip: "⇧⌘P" on macOS, "Ctrl+Shift+P" elsewhere.
    pub fn label(&self) -> String {
        let key = self.key.to_uppercase();
        if cfg!(target_os = "macos") {
            let flag = |on: bool, mark: &'static str| if on { mark } else { "" };
            format!("{}{}{}{}{key}", flag(self.ctrl, "⌃"), flag(self.alt, "⌥"), flag(self.shift, "⇧"), flag(self.cmd, "⌘"))
        } else {
            self.joined(["Ctrl", "Alt", "Shift", "Super"], &key)
        }
    }

    /// As `aria-keyshortcuts` writes it, for a screen reader.
    pub fn aria(&self) -> String {
        self.joined(["Control", "Alt", "Shift", "Meta"], &self.key.to_uppercase())
    }

    fn joined(&self, names: [&str; 4], key: &str) -> String {
        [(self.ctrl, names[0]), (self.alt, names[1]), (self.shift, names[2]), (self.cmd, names[3])]
            .into_iter()
            .filter_map(|(on, name)| on.then_some(name))
            .chain([key])
            .collect::<Vec<_>>()
            .join("+")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub id: u64,
    pub tone: Option<Tone>,
    pub children: Vec<Node>,
}

/// A `Toast`: a small card with a tone's icon, a message and a close button, for an action's result the person may not
/// see where they've scrolled to. A page puts it in its `toasts` slot, which floats over the foot of the body.
#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub id: u64,
    pub tone: Option<Tone>,
    /// What the close button sends back to the page.
    pub on_dismiss: Option<String>,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Spinner {
    pub id: u64,
    pub large: bool,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Checkbox {
    pub id: u64,
    pub checked: bool,
    pub disabled: bool,
    /// `switch: true`: drawn as Settings' on/off switch, the name a screen reader reads out in `label`.
    pub switch: bool,
    pub label: String,
    pub on_change: Option<String>,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Confirm {
    pub id: u64,
    pub title: Vec<Node>,
    pub body: Vec<Node>,
    pub actions: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Badge {
    pub id: u64,
    pub variant: Variant,
    pub tone: Option<Tone>,
    pub small: bool,
    /// A badge that's a button, as a `SummaryChip` with an `onClick` is: plugin-ui.css draws it as the same pill.
    pub on_click: Option<String>,
    /// What a tooltip says: the whole name of a pill that is cut short with an ellipsis.
    pub title: Option<String>,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Empty {
    pub id: u64,
    /// It takes the height it is in and sits in its middle (`fill`), as a new chat's does.
    pub fill: bool,
    /// A soft glow drifts behind it (`backdrop: "glow"`).
    pub glow: bool,
    pub title: Vec<Node>,
    pub description: Vec<Node>,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Loading {
    pub id: u64,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub id: u64,
    /// `variant: "raised"`: rounder, and lifted off the page by a shadow, as a chat's message box floats.
    pub raised: bool,
    /// `variant: "rows"`: each child a row, split from the one above by a hairline, as Wings' Settings groups its rows.
    pub rows: bool,
    /// `scroll: true`: in a column with too little room, as a floating panel over the page, the card shrinks to fit and
    /// what it holds scrolls inside it, so its edge and corners stay drawn.
    pub scrolls: bool,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Heading {
    pub id: u64,
    /// Level 3 names a section of a page, as `h3.w-heading` does.
    pub section: bool,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Dot {
    pub id: u64,
    /// Green, the plain colour, when there is none.
    pub tone: Option<Tone>,
    /// `0xRRGGBB`, a colour from the page's data, which wins over the tone.
    pub color: Option<u32>,
    /// `state`: an agent's state, drawn as the app's own status dot (a spinning ring while working), which wins
    /// over both, so an agent's page names its state as a terminal pane's header does.
    pub state: Option<crate::agents::AgentState>,
}

/// A column that scrolls and fills what is left of its parent.
#[derive(Clone, Debug, PartialEq)]
pub struct Scroll {
    pub id: u64,
    /// `follow: "end"`: it stays at its end as what it holds grows, while you haven't scrolled up.
    pub follow_end: bool,
    /// `onReachStart`, with `follow: "end"`: sent when the scroll comes within a screen of its top, once until what
    /// is above it grows. The page then adds older items, and the view stays on what it showed.
    pub on_reach_start: Option<String>,
    /// `onReachEnd`, with `follow: "end"`: sent when the scroll comes within a screen of its end, once until what is
    /// below it grows, while the page shows an older part of a list too long to send whole. It doesn't follow its end
    /// then. The page adds newer items below and may take older ones away above, and the view stays on what it showed.
    pub on_reach_end: Option<String>,
    /// `textScale`: every text inside is this many times its size (0.5 to 2), as a page's text size setting asks.
    /// None keeps the scale the scroll is in.
    pub text_scale: Option<f32>,
    /// `scrollTo: { index, token }`: scrolls so the `index`th child of its first child is at the top of the view, once for
    /// each new `token`. The page keeps sending the same pair until it scrolls on its own; an unchanged token does nothing.
    pub scroll_to: Option<(usize, u64)>,
    /// `track: [index, ...]` (ascending) with `onTrack`: the children of its first child whose place the page wants to
    /// know. `onTrack` hears `{ at }`, the position in `track` of the last one whose top is at or above the top of the
    /// view (null when the view is above them all), whenever that changes.
    pub track: Vec<usize>,
    pub on_track: Option<String>,
    /// `maxHeight`: points. It is as tall as what it holds up to that, and scrolls inside itself past it, in place of
    /// filling what is left of its parent: a list in a card that is itself long. It sets no padding of its own, and the
    /// wheel over it scrolls it while it can go that way and its parent when it can't.
    pub max_height: Option<f32>,
    pub children: Vec<Node>,
}

/// A table: its first `Row` is the header and each `Row` after it is a body row, a cell for each child. The app sets the
/// columns' widths from what they hold, draws it as a card with hairlines between rows, and scrolls it sideways when it is
/// wider than its place.
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub id: u64,
    pub children: Vec<Node>,
}

/// A strip across its parent, a page's header or footer: padded like a toolbar, with a hairline on one edge.
#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    pub id: u64,
    pub edge: Edge,
    pub gap: Gap,
    /// `variant: "slim"`: as tall as a terminal pane's header, for a title and a row of small icon buttons.
    pub slim: bool,
    pub children: Vec<Node>,
}

/// A dialog over the page, wherever the page puts it: a dimmed backdrop over the whole page and, in its middle, a panel
/// with the `title`, a close button and what the dialog holds, which scrolls. Escape, a press on the backdrop and the
/// close button send `onClose`, and the page then leaves the dialog out.
#[derive(Clone, Debug, PartialEq)]
pub struct Dialog {
    pub id: u64,
    pub title: String,
    /// How wide the panel is, in points, at most: in a narrower page it narrows with it.
    pub width: f32,
    pub on_close: Option<String>,
    pub children: Vec<Node>,
}

/// A panel that slides in over the page from its right edge, as Wings' Past sessions does: its `title` and
/// `description` with a close button, what its `header` slot holds under them (a search, filters) over a hairline, and
/// its other children under that, scrolling. `onClose` hears Escape, a press beside it and its close button.
#[derive(Clone, Debug, PartialEq)]
pub struct Sheet {
    pub id: u64,
    pub title: Vec<Node>,
    pub description: Vec<Node>,
    pub header: Vec<Node>,
    pub body: Vec<Node>,
    pub on_close: Option<String>,
}

/// Two panes side by side: `start` is `size` points wide, `end` takes the rest. With `side: "end"` it is the other
/// way round: `end` is the fixed pane, on the right, and `start` takes the rest.
#[derive(Clone, Debug, PartialEq)]
pub struct Split {
    pub id: u64,
    pub size: f32,
    /// The fixed pane is `end`, on the right.
    pub fixed_end: bool,
    /// The fixed pane isn't drawn while the split itself is narrower than this many points.
    pub collapse_below: Option<f32>,
    /// The fixed pane stays however narrow the split is: the page wants it shown now.
    pub pinned: bool,
    /// With `fixed_end`: the fixed pane floats over the right of the split as a card and takes no width from `start`.
    pub float: bool,
    /// With `fixed_end`, and not `float`: the fixed pane is a column docked beside `start`, full height, flush, with a hairline
    /// between them, in place of a padded pane.
    pub dock: bool,
    /// With `dock`: the column's inner edge can be dragged. The page owns nothing of the drag: the app sets the width while
    /// the pointer moves and sends this once, with `{ "size": points }`, when it is let go (and with `reset_size` on a
    /// double press). The page keeps the number and gives it back as `defaultSize`.
    pub on_resize: Option<String>,
    /// With `onResize`: the narrowest the column is dragged to, and the least the other pane keeps.
    pub min_size: f32,
    pub min_rest: f32,
    /// With `onResize`: the width a double press on the edge sends back.
    pub reset_size: Option<f32>,
    /// Heard, with `{ "narrow": bool }`, when the split's width crosses `collapse_below`, and once it first has a width.
    pub on_narrow: Option<String>,
    /// A floating pane with this handler hears Escape in it and a press outside it.
    pub on_close: Option<String>,
    /// With `float`: while the split is at least this wide, `start` is set in from the right by the floating pane's width,
    /// so nothing of it goes under the pane. `onNarrow` then also says `inset`.
    pub inset_below: Option<f32>,
    /// With `insetBelow`: the width of the centred column `start` holds. Then `start` is set in only as far as keeps that
    /// column clear of the pane, and not at all where it already is, in place of the pane's whole width.
    pub inset_column: Option<f32>,
    /// The space each side of the divider, so cards in the two panes don't touch it. `none` by default: a list or a rail
    /// runs up to the divider.
    pub gap: Gap,
    pub start: Vec<Node>,
    pub end: Vec<Node>,
}

/// A round avatar with the initials of a name, on a colour that depends only on the name.
#[derive(Clone, Debug, PartialEq)]
pub struct Avatar {
    pub id: u64,
    pub name: String,
    pub large: bool,
}

/// How wide a skeleton is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    Points(f32),
    Percent(f32),
}

/// A block that stands in for content that is loading.
#[derive(Clone, Debug, PartialEq)]
pub struct Skeleton {
    pub id: u64,
    /// The whole width when there is none.
    pub width: Option<Length>,
    pub height: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Gauge {
    pub id: u64,
    pub label: String,
    /// From 0 to 100: a bar can't be fuller than full, so what the page sends is cut to that.
    pub percent: f64,
    /// Where the bar turns warn and danger.
    pub levels: Option<(f64, f64)>,
    pub note: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Meter {
    pub id: u64,
    pub label: String,
    pub percent: f64,
    pub tone: Option<Tone>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Board {
    pub id: u64,
    pub label: String,
    pub lanes: bool,
    pub header: Vec<Node>,
    pub body: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BoardColumn {
    pub id: u64,
    pub label: String,
    pub cell: bool,
    pub count: Option<u64>,
    pub title: Vec<Node>,
    pub actions: Vec<Node>,
    pub body: Vec<Node>,
}

/// How full a gauge is, by its `levels`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Normal,
    Warn,
    Danger,
}

impl Gauge {
    pub fn level(&self) -> Level {
        match self.levels {
            Some((_, danger)) if self.percent >= danger => Level::Danger,
            Some((warn, _)) if self.percent >= warn => Level::Warn,
            _ => Level::Normal,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct List {
    pub id: u64,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ListItem {
    pub id: u64,
    pub children: Vec<Node>,
    /// Buttons that show over the row's end while it is pointed at or one of them has the keyboard.
    pub actions: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HoverCard {
    pub id: u64,
    /// What the pointer rests on, drawn where the node sits.
    pub children: Vec<Node>,
    /// The `card` slot: what the card over the page shows while the pointer is on the children or on it.
    pub card: Vec<Node>,
    /// `width`: how wide the card is, in points (300 by default, from 160 to 480).
    pub width: f32,
}

/// A list of actions over the page at a point of the window, as a right-click menu: the page opens it by sending the node
/// and closes it by leaving it out.
#[derive(Clone, Debug, PartialEq)]
pub struct PopupMenu {
    pub id: u64,
    /// Where the menu's top-left corner opens, in window points. It is moved in to stay inside the window.
    pub x: f32,
    pub y: f32,
    /// What to send back to the page when Escape or a press outside the menu asks for it to close.
    pub on_close: Option<String>,
    /// The items, each a `Node::Button`: its text or `label`, its `description`, its click, and red when its variant is
    /// destructive.
    pub items: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Disclosure {
    pub id: u64,
    pub open: Option<bool>,
    pub on_open_change: Option<String>,
    /// Set only by a collapsible `Section` (never by a page): the summary is drawn as the section's heading and the body
    /// starts at the rows' own edge instead of under the summary's words.
    pub section: bool,
    pub summary: Vec<Node>,
    pub body: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
    pub disabled: bool,
    /// A muted line under the label.
    pub detail: Option<String>,
    /// A setting rather than a choice: the row is set apart at the end of the menu with a switch showing this state, and
    /// picking it sends its value as any option does.
    pub toggle: Option<bool>,
    /// A section label over the rows after it: not a choice, so nothing picks it. A Wings before this draws it as a row.
    pub heading: bool,
    /// Whether the row is drawn with a check, for a menu with several groups that each have a choice. None leaves it to
    /// the select's `value`.
    pub checked: Option<bool>,
    /// Beside the label at the row's end, like the level an Effort row is on.
    pub trailing: Option<String>,
    /// A small pill before `trailing`, like "Recommended".
    pub pill: Option<String>,
    /// Rows that open under this one when it is clicked, which then is no choice of its own: an Effort row's levels.
    /// One level deep.
    pub children: Vec<SelectOption>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Select {
    pub id: u64,
    /// The select's accessible name.
    pub label: String,
    pub options: Vec<SelectOption>,
    pub value: Option<String>,
    /// What to send back to the page, with the value picked. A select with none can still be opened.
    pub on_change: Option<String>,
    /// Chips drawn as `Tabs` of the "line" variant are: labels with a line under the picked one.
    pub line: bool,
    /// Chips drawn as a toggle on a faint track, as a sheet's header draws them, wherever they are.
    pub segmented: bool,
    /// `variant: "ghost"`: a select as wide as its choice, with no box until you point at it, for a row of settings
    /// beside a field, like a chat's model and effort.
    pub quiet: bool,
    /// A word in an accent colour after what the select shows, like a chat's "Ultracode" when it is on.
    pub badge: Option<String>,
    /// What the trigger shows instead of the current option's label, like a chat's model with its effort.
    pub display: Option<String>,
}

/// wings-plugin-ui's `Menu`: a button that opens a list of actions over the page, under it. The page sends the button
/// and then one `Button` for each item, with its own click.
#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub id: u64,
    /// The button that opens it, one `Node::Button`. The app opens the menu on its click, and sends the page nothing.
    pub trigger: Vec<Node>,
    /// The items, each a `Node::Button`: its text, its click, and red when its variant is destructive.
    pub items: Vec<Node>,
    /// The menu lines up with the button's right edge instead of its left.
    pub end: bool,
}

/// wings-plugin-ui/native's `Combobox`, which has no node of its own: the page sends a stack of its field and, under
/// it, the options that match the field as chips, or a muted line saying none do. The app draws them as a list over
/// the page, as the desktop does, instead of in the page's flow, where they pushed everything under the field down.
#[derive(Clone, Debug, PartialEq)]
pub struct Combobox {
    pub id: u64,
    /// The field, one `Node::Input`.
    pub field: Vec<Node>,
    /// The chips (`Node::Button`): a click on one picks its option.
    pub options: Vec<Node>,
    /// What the line saying nothing matches holds, when nothing does.
    pub empty: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ListButton {
    pub button: Button,
    pub indent: f32,
    pub expanded: Option<bool>,
    pub vertical: bool,
    /// `align="baseline"`: its parts line up on their first lines, as a prompt's name and time beside its text.
    pub baseline: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SearchField {
    pub id: u64,
    pub label: String,
    pub placeholder: String,
    pub value: Option<String>,
    pub on_input: Option<String>,
}

/// A `TextInput`, or with `rows` a `Textarea`. The page holds its text: the one in the tree is what to show.
#[derive(Clone, Debug, PartialEq)]
pub struct Input {
    pub id: u64,
    pub label: String,
    pub placeholder: String,
    pub value: String,
    pub rows: Option<usize>,
    /// `maxRows`: the lines a `Textarea` grows to, 12 by default and never fewer than `rows`. Past that it scrolls inside
    /// itself.
    pub max_rows: Option<usize>,
    /// `submitOnEnter`: a `Textarea` that sends Enter to `onKeyDown`, and starts a new line on Shift-Enter.
    pub enter_submits: bool,
    /// `takesTab`: a `Textarea` that sends Tab to `onKeyDown`, in place of moving focus on.
    pub takes_tab: bool,
    /// `takesShiftTab`: a `Textarea` that sends Shift-Tab to `onKeyDown` (as `key: "Tab"` with `shiftKey`), in place of
    /// moving focus back. Plain Tab still moves focus on, unless `takesTab` is on.
    pub takes_shift_tab: bool,
    /// `historyKeys`: a `Textarea` that sends Up from its first line and Down from its last to `onKeyDown` (as
    /// `ArrowUp` and `ArrowDown`), in place of moving the caret to the start or end of the text.
    pub history_keys: bool,
    /// `variant: "bare"`: no frame of its own, for a field inside a box that frames it.
    pub bare: bool,
    pub disabled: bool,
    pub max_length: Option<usize>,
    /// What to send back to the page, with the text after each edit.
    pub on_change: Option<String>,
    pub on_key_down: Option<String>,
    pub on_blur: Option<String>,
    /// `onPasteImage`: a paste of an image (png, jpeg, gif or webp, up to 3.75 MB) is sent here as
    /// `{ mediaType, data }`, with `data` in base64. Without it a pasted image does nothing.
    pub on_paste_image: Option<String>,
    pub auto_focus: bool,
    pub select_on_focus: bool,
    /// A combobox's field shows its options when it takes the keyboard, unless the page sends `openOnFocus: false`.
    pub open_on_focus: bool,
    /// A picker's or message box's combobox has its first option highlighted until the arrows move, so Enter takes it,
    /// unless the page sends `autoHighlight: false`: then nothing is highlighted until an arrow key, Enter goes to the
    /// page, and Tab still takes the first option.
    pub auto_highlight: bool,
    /// `type="password"`: the text shows as dots and can't be copied. Any other type is plain text here, but time.
    pub password: bool,
    /// `type="time"`: digits alone make HH:MM, and the page hears "" until it is a whole time, as from a browser.
    pub time: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Icon {
    pub id: u64,
    pub name: IconName,
    /// Points square, 16 unless the page says.
    pub size: f32,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphRow {
    pub hash: String,
    pub subject: String,
    pub author: String,
    pub date: String,
    pub refs: Vec<String>,
    pub lane: usize,
    pub color: usize,
    pub merge: bool,
    pub lines: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Graph {
    pub id: u64,
    pub total: usize,
    pub lanes: usize,
    pub selected: i64,
    pub offset: usize,
    pub rows: Vec<GraphRow>,
    pub on_range: Option<String>,
    pub on_select: Option<String>,
}

fn graph(id: u64, props: &Map<String, Value>) -> Result<Graph, TreeError> {
    let invalid = || {
        TreeError::Invalid(
            "invalid graph window (10,000 total, 256 window rows, bounded fields required)".into(),
        )
    };
    let integer = |key| {
        props
            .get(key)
            .and_then(Value::as_u64)
            .and_then(|v| usize::try_from(v).ok())
            .ok_or_else(invalid)
    };
    let total = integer("total")?;
    let lanes = integer("lanes")?;
    let offset = integer("offset")?;
    let selected = props
        .get("selected")
        .and_then(Value::as_i64)
        .ok_or_else(invalid)?;
    let rows: Vec<GraphRow> =
        serde_json::from_value(props.get("rows").cloned().ok_or_else(invalid)?)
            .map_err(|_| invalid())?;
    let text_ok = |s: &str, max| s.chars().count() <= max;
    if total > 10_000
        || !(1..=24).contains(&lanes)
        || offset > total
        || rows.len() > 256
        || rows.len() > total - offset
        || selected < -1
        || selected >= total as i64
        || rows.iter().any(|r| {
            r.hash.is_empty()
                || !text_ok(&r.hash, 64)
                || !text_ok(&r.subject, 4096)
                || !text_ok(&r.author, 512)
                || !text_ok(&r.date, 64)
                || r.refs.len() > 64
                || r.refs.iter().any(|s| !text_ok(s, 1024))
                || r.lane > 10_000
                || r.color > 100_000
                || r.lines.len() > 4096
                || !r.lines.len().is_multiple_of(4)
                || r.lines
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|l| l[0] > 10_000 || l[1] > 10_000 || l[2] > 2 || l[3] > 100_000)
        })
    {
        return Err(invalid());
    }
    let handler = |key| {
        str_prop(props, key)
            .filter(|s| valid_handler(s))
            .map(str::to_owned)
    };
    Ok(Graph {
        id,
        total,
        lanes,
        selected,
        offset,
        rows,
        on_range: handler("onRangeChange"),
        on_select: handler("onSelect"),
    })
}

impl Select {
    /// The value the select is on: the one the page set, or the first option's when it set none.
    pub fn current(&self) -> Option<&str> {
        self.value
            .as_deref()
            .or_else(|| self.options.first().map(|option| option.value.as_str()))
    }

    /// What the select shows: its current option's label, or the page's value itself when no option has it.
    pub fn shown(&self) -> &str {
        if let Some(display) = &self.display {
            return display;
        }
        let current = self.current().unwrap_or_default();
        self.options
            .iter()
            .find(|option| option.value == current)
            .map_or(current, |option| option.label.as_str())
    }
}

/// What a page sends: `{id, type, props, children}`, or `{id, type: "#text", text}`.
#[derive(Deserialize)]
struct Raw {
    id: u64,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    props: Map<String, Value>,
    #[serde(default)]
    children: Vec<Raw>,
    #[serde(default)]
    text: String,
}

impl Tree {
    pub fn parse(json: &str) -> Result<Tree, TreeError> {
        if json.len() > MAX_JSON_BYTES {
            return Err(TreeError::TooLarge);
        }
        let root: Raw =
            serde_json::from_str(json).map_err(|error| TreeError::Invalid(error.to_string()))?;
        if root.kind != "#root" {
            return Err(TreeError::Invalid("no root".into()));
        }
        let mut walk = Walk { nodes: 0 };
        let children = walk.children(root.children, 0)?;
        // A page that is one `Page` with a toolbar draws that toolbar edge to edge whether or not the plugin asked
        // for `fill`: the shell's own padding put its title 14 points further in than the other pages' and stopped its
        // hairline short of the edges. The `Page` pads its body by itself.
        let toolbar_page = matches!(children.as_slice(), [Node::Page(page)] if !page.toolbar.is_empty());
        Ok(Tree {
            fill: toolbar_page || root.props.get("fill").and_then(Value::as_bool) == Some(true),
            children,
            palette: root.props.get("palette").and_then(palette),
        })
    }

    /// The prompt rail `id`, wherever it is in the tree.
    pub fn find_rail(&self, id: u64) -> Option<&PromptRail> {
        fn find(nodes: &[Node], id: u64) -> Option<&PromptRail> {
            nodes.iter().find_map(|node| match node {
                Node::PromptRail(rail) if rail.id == id => Some(rail),
                other => other.parts().into_iter().find_map(|part| find(part, id)),
            })
        }
        find(&self.children, id)
    }

    /// Every string in the tree, in order: what a reader would see.
    pub fn text(&self) -> String {
        plain_text(&self.children)
    }
}

/// A page's `palette`: each role (`base`, `page`, `dialog`, `popover`, `text`, `muted`, `info`, `danger`, `done`,
/// `working`) a `#rrggbb` colour, a role left out or not a colour keeping the app's, and `ground`
/// the colours the page lies on from `from` to `to` at `angle` degrees. None when it names no colour at all.
fn palette(props: &Value) -> Option<crate::theme::Palette> {
    let color = |value: &Value| {
        let hex = value.as_str()?.strip_prefix('#')?;
        (hex.len() == 6 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())).then(|| u32::from_str_radix(hex, 16).ok())?
    };
    let mut palette = crate::theme::Palette::app();
    let mut named = false;
    for (role, slot) in [
        ("base", &mut palette.background),
        ("page", &mut palette.surface),
        ("dialog", &mut palette.dialog),
        ("text", &mut palette.foreground),
        ("muted", &mut palette.muted_foreground),
        ("info", &mut palette.info),
        ("danger", &mut palette.danger),
        ("done", &mut palette.done),
        ("working", &mut palette.working),
    ] {
        if let Some(found) = props.get(role).and_then(color) {
            *slot = found;
            named = true;
        }
    }
    // A popover is see-through by as much as the app's.
    if let Some(found) = props.get("popover").and_then(color) {
        palette.popover = found << 8 | (crate::theme::popover() & 0xff);
        named = true;
    }
    let ground = props.get("ground").filter(|ground| ground.is_object());
    if let Some(ground) = ground
        && let (Some(from), Some(to)) = (color(&ground["from"]), color(&ground["to"]))
    {
        let angle = (ground["angle"].as_f64().filter(|angle| angle.is_finite()).unwrap_or(180.0) as f32).rem_euclid(360.0);
        palette.ground = Some((from, to, angle));
        named = true;
    }
    named.then_some(palette)
}

/// The strings under `nodes`: what a reader would see. Strings next to each other run together, and the parts of a
/// component (a page's toolbar and body) are told apart by a space.
pub fn plain_text(nodes: &[Node]) -> String {
    nodes
        .iter()
        .map(|node| match node {
            Node::Str { text, .. } => text.clone(),
            // Its text is a prop, not a child, and a reader still sees it.
            Node::Code(code) => code.text.clone(),
            // Its tabs are items, not children, and a reader still sees their titles.
            Node::TabStrip(strip) => strip
                .items
                .iter()
                .filter_map(|item| match item {
                    TabItem::Tab(tab) => Some(tab.title.as_str()),
                    TabItem::Group(group) => Some(group.name.as_str()),
                })
                .collect::<Vec<_>>()
                .join(" "),
            other => other
                .parts()
                .into_iter()
                .map(plain_text)
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join(" "),
        })
        .collect()
}

impl Node {
    /// The node's id, which a keyed node keeps from one build of the page to the next. A slot has none: it is a part of
    /// the component it's in.
    pub fn id(&self) -> Option<u64> {
        match self {
            Node::Str { id, .. } => Some(*id),
            Node::Slot(_) => None,
            Node::Page(n) => Some(n.id),
            Node::Toolbar(n) => Some(n.id),
            Node::Stack(n) => Some(n.id),
            Node::Row(n) => Some(n.id),
            Node::Text(n) => Some(n.id),
            Node::Button(n) => Some(n.id),
            Node::Notice(n) => Some(n.id),
            Node::Toast(n) => Some(n.id),
            Node::Spinner(n) => Some(n.id),
            Node::Checkbox(n) => Some(n.id),
            Node::Confirm(n) => Some(n.id),
            Node::Badge(n) => Some(n.id),
            Node::Empty(n) => Some(n.id),
            Node::Loading(n) => Some(n.id),
            Node::Card(n) => Some(n.id),
            Node::Heading(n) => Some(n.id),
            Node::Dot(n) => Some(n.id),
            Node::Gauge(n) => Some(n.id),
            Node::Meter(n) => Some(n.id),
            Node::Board(n) => Some(n.id),
            Node::BoardRow(n) => Some(n.id),
            Node::BoardColumn(n) => Some(n.id),
            Node::List(n) => Some(n.id),
            Node::ListItem(n) => Some(n.id),
            Node::Disclosure(n) => Some(n.id),
            Node::HoverCard(n) => Some(n.id),
            Node::PopupMenu(n) => Some(n.id),
            Node::Select(n) => Some(n.id),
            Node::Chips(n) => Some(n.id),
            Node::TabStrip(n) => Some(n.id),
            Node::Combobox(n) => Some(n.id),
            Node::Menu(n) => Some(n.id),
            Node::ListButton(n) => Some(n.button.id),
            Node::SearchField(n) => Some(n.id),
            Node::Input(n) => Some(n.id),
            Node::Icon(n) => Some(n.id),
            Node::Svg(n) => Some(n.id),
            Node::Image(n) => Some(n.id),
            Node::Graph(n) => Some(n.id),
            Node::Avatar(n) => Some(n.id),
            Node::Skeleton(n) => Some(n.id),
            Node::Scroll(n) => Some(n.id),
            Node::Split(n) => Some(n.id),
            Node::Bar(n) => Some(n.id),
            Node::Table(n) => Some(n.id),
            Node::Sunburst(n) => Some(n.id),
            Node::PromptRail(n) => Some(n.id),
            Node::Dialog(n) => Some(n.id),
            Node::Sheet(n) => Some(n.id),
            Node::Terminal(n) => Some(n.id),
            Node::WebView(n) => Some(n.id),
            Node::Code(n) => Some(n.id),
            Node::Actions(n) => Some(n.id),
        }
    }

    /// What the component holds, one list for each of its named parts.
    pub fn parts(&self) -> Vec<&[Node]> {
        match self {
            Node::Str { .. } => vec![],
            Node::Slot(n) => vec![&n.children],
            Node::Page(n) => vec![&n.toolbar, &n.body, &n.toasts],
            Node::Toolbar(n) => vec![&n.title, &n.filters, &n.actions],
            Node::Stack(n) => vec![&n.children],
            Node::Row(n) => vec![&n.children],
            Node::Text(n) => vec![&n.children],
            Node::Button(n) => vec![&n.children],
            Node::ListButton(n) => vec![&n.button.children],
            Node::Notice(n) => vec![&n.children],
            Node::Toast(n) => vec![&n.children],
            Node::Spinner(_) => vec![],
            Node::Checkbox(n) => vec![&n.children],
            Node::Confirm(n) => vec![&n.title, &n.body, &n.actions],
            Node::Badge(n) => vec![&n.children],
            Node::Empty(n) => vec![&n.title, &n.description, &n.children],
            Node::Loading(n) => vec![&n.children],
            Node::Card(n) => vec![&n.children],
            Node::Heading(n) => vec![&n.children],
            Node::Dot(_)
            | Node::Select(_)
            | Node::Chips(_)
            | Node::TabStrip(_)
            | Node::SearchField(_)
            | Node::Input(_)
            | Node::Icon(_)
            | Node::Svg(_)
            | Node::Image(_)
            | Node::Graph(_)
            | Node::Avatar(_)
            | Node::Skeleton(_)
            | Node::Sunburst(_)
            | Node::PromptRail(_)
            | Node::Terminal(_)
            | Node::WebView(_)
            | Node::Code(_) => vec![],
            Node::Scroll(n) => vec![&n.children],
            Node::Split(n) => vec![&n.start, &n.end],
            Node::Bar(n) => vec![&n.children],
            Node::Table(n) => vec![&n.children],
            Node::Dialog(n) => vec![&n.children],
            Node::Actions(n) => vec![&n.children],
            Node::Gauge(n) => vec![&n.note],
            Node::Meter(_) => vec![],
            Node::Board(n) => vec![&n.header, &n.body],
            Node::BoardRow(n) => vec![&n.children],
            Node::BoardColumn(n) => vec![&n.title, &n.actions, &n.body],
            Node::List(n) => vec![&n.children],
            Node::ListItem(n) => vec![&n.children, &n.actions],
            Node::Disclosure(n) => vec![&n.summary, &n.body],
            Node::HoverCard(n) => vec![&n.children, &n.card],
            Node::PopupMenu(n) => vec![&n.items],
            Node::Combobox(n) => vec![&n.field, &n.options, &n.empty],
            Node::Menu(n) => vec![&n.trigger, &n.items],
            Node::Sheet(n) => vec![&n.title, &n.description, &n.header, &n.body],
        }
    }

    /// `parts`, to change.
    fn parts_mut(&mut self) -> Vec<&mut Vec<Node>> {
        match self {
            Node::Str { .. } => vec![],
            Node::Slot(n) => vec![&mut n.children],
            Node::Page(n) => vec![&mut n.toolbar, &mut n.body, &mut n.toasts],
            Node::Toolbar(n) => vec![&mut n.title, &mut n.filters, &mut n.actions],
            Node::Stack(n) => vec![&mut n.children],
            Node::Row(n) => vec![&mut n.children],
            Node::Text(n) => vec![&mut n.children],
            Node::Button(n) => vec![&mut n.children],
            Node::ListButton(n) => vec![&mut n.button.children],
            Node::Notice(n) => vec![&mut n.children],
            Node::Toast(n) => vec![&mut n.children],
            Node::Spinner(_) => vec![],
            Node::Checkbox(n) => vec![&mut n.children],
            Node::Confirm(n) => vec![&mut n.title, &mut n.body, &mut n.actions],
            Node::Badge(n) => vec![&mut n.children],
            Node::Empty(n) => vec![&mut n.title, &mut n.description, &mut n.children],
            Node::Loading(n) => vec![&mut n.children],
            Node::Card(n) => vec![&mut n.children],
            Node::Heading(n) => vec![&mut n.children],
            Node::Dot(_)
            | Node::Select(_)
            | Node::Chips(_)
            | Node::TabStrip(_)
            | Node::SearchField(_)
            | Node::Input(_)
            | Node::Icon(_)
            | Node::Svg(_)
            | Node::Image(_)
            | Node::Graph(_)
            | Node::Avatar(_)
            | Node::Skeleton(_)
            | Node::Sunburst(_)
            | Node::PromptRail(_)
            | Node::Terminal(_)
            | Node::WebView(_)
            | Node::Code(_) => vec![],
            Node::Scroll(n) => vec![&mut n.children],
            Node::Split(n) => vec![&mut n.start, &mut n.end],
            Node::Bar(n) => vec![&mut n.children],
            Node::Table(n) => vec![&mut n.children],
            Node::Dialog(n) => vec![&mut n.children],
            Node::Actions(n) => vec![&mut n.children],
            Node::Gauge(n) => vec![&mut n.note],
            Node::Meter(_) => vec![],
            Node::Board(n) => vec![&mut n.header, &mut n.body],
            Node::BoardRow(n) => vec![&mut n.children],
            Node::BoardColumn(n) => vec![&mut n.title, &mut n.actions, &mut n.body],
            Node::List(n) => vec![&mut n.children],
            Node::ListItem(n) => vec![&mut n.children, &mut n.actions],
            Node::Disclosure(n) => vec![&mut n.summary, &mut n.body],
            Node::HoverCard(n) => vec![&mut n.children, &mut n.card],
            Node::PopupMenu(n) => vec![&mut n.items],
            Node::Combobox(n) => vec![&mut n.field, &mut n.options, &mut n.empty],
            Node::Menu(n) => vec![&mut n.trigger, &mut n.items],
            Node::Sheet(n) => vec![&mut n.title, &mut n.description, &mut n.header, &mut n.body],
        }
    }
}

struct Walk {
    nodes: usize,
}

impl Walk {
    fn children(&mut self, raw: Vec<Raw>, depth: usize) -> Result<Vec<Node>, TreeError> {
        let mut out = Vec::with_capacity(raw.len());
        for child in raw {
            if let Some(node) = self.node(child, depth + 1)? {
                out.push(node);
            }
        }
        Ok(out)
    }

    fn node(&mut self, raw: Raw, depth: usize) -> Result<Option<Node>, TreeError> {
        if depth > MAX_DEPTH {
            return Err(TreeError::TooDeep);
        }
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(TreeError::TooManyNodes);
        }
        let Raw {
            id,
            kind,
            props,
            children,
            text,
        } = raw;
        if kind == "#text" {
            return Ok(Some(Node::Str {
                id,
                text: cut(text),
            }));
        }
        // Checked before the children are walked, so a component nobody draws costs nothing.
        if !matches!(
            kind.as_str(),
            "Slot"
                | "Page"
                | "Toolbar"
                | "Stack"
                | "Row"
                | "Text"
                | "Button"
                | "Notice"
                | "Toast"
                | "Spinner"
                | "Checkbox"
                | "Confirm"
                | "Badge"
                | "Empty"
                | "Loading"
                | "Card"
                | "Heading"
                | "Dot"
                | "Gauge"
                | "Meter"
                | "Board"
                | "BoardRow"
                | "BoardColumn"
                | "List"
                | "ListItem"
                | "Disclosure"
                | "HoverCard"
                | "PopupMenu"
                | "Select"
                | "Chips"
                | "TabStrip"
                | "Tab"
                | "TabGroup"
                | "ListButton"
                | "SearchField"
                | "TextInput"
                | "Textarea"
                | "Icon"
                | "Svg"
                | "Image"
                | "Graph"
                | "Avatar"
                | "Skeleton"
                | "Scroll"
                | "Split"
                | "Bar"
                | "Table"
                | "Sunburst"
                | "PromptRail"
                | "Menu"
                | "Dialog"
                | "Sheet"
                | "Terminal"
                | "WebView"
                | "Code"
                | "ListRow"
                | "Section"
                | "Actions"
        ) {
            return Ok(None);
        }
        // A strip reads its own tabs and groups; one anywhere else has nothing to be in.
        match kind.as_str() {
            "TabStrip" => return tab_strip::parse(self, id, &props, children).map(Some),
            "Tab" | "TabGroup" => return Ok(None),
            _ => {}
        }
        let children = self.children(children, depth)?;
        let on = |name: &str| props.get(name) == Some(&Value::Bool(true));
        let tone = str_prop(&props, "tone").and_then(Tone::choose);
        let handler = |name| {
            str_prop(&props, name)
                .filter(|value| !on("disabled") && valid_handler(value))
                .map(str::to_owned)
        };
        Ok(Some(match kind.as_str() {
            "Slot" => Node::Slot(Slot {
                name: str_prop(&props, "name").unwrap_or_default().to_owned(),
                children,
            }),
            "Page" => {
                let mut slots = Slots(children);
                Node::Page(Page {
                    id,
                    toolbar: slots.take("toolbar"),
                    flush: on("flush"),
                    body: slots.take("body"),
                    on_text_scale: handler("onTextScale"),
                    backdrop: str_prop(&props, "backdrop").and_then(Backdrop::named),
                    column: number(&props, "column").filter(|width| *width > 0.0).map(|width| width as f32),
                    toasts: slots.take("toasts"),
                })
            }
            "Toolbar" => {
                let mut slots = Slots(children);
                Node::Toolbar(Toolbar {
                    id,
                    title: slots.take("title"),
                    filters: slots.take("children"),
                    actions: slots.take("actions"),
                })
            }
            "Stack" => combobox(Stack {
                id,
                gap: choice(&props, "gap"),
                grow: on("grow"),
                fill: on("fill"),
                width: number(&props, "width")
                    .filter(|width| *width > 0.0)
                    .map(|width| width.min(2_000.0) as f32),
                max_width: number(&props, "maxWidth")
                    .filter(|width| *width > 0.0)
                    .map(|width| width as f32),
                enter: str_prop(&props, "enter") == Some("rise"),
                links: on("links"),
                on_context_menu: handler("onContextMenu"),
                inset: false,
                measure: None,
                min_share: None,
                quote: on("quote"),
                pt: number(&props, "pt").filter(|pt| *pt > 0.0).map(|pt| pt.min(200.0) as f32),
                float_bottom: on("floatBottom"),
                children: parts(children),
            }),
            "ListRow" => kit::list_row(id, children, handler("onClick"), title(&props)),
            "Section" => {
                let fold = on("collapsible").then(|| (props.get("open").and_then(Value::as_bool), handler("onOpenChange")));
                kit::section(id, children, fold)
            }
            "Actions" => kit::actions(id, children),
            "Row" => Node::Row(Row {
                id,
                gap: choice(&props, "gap"),
                align: choice(&props, "align"),
                justify: choice(&props, "justify"),
                wrap: on("wrap"),
                grow: on("grow"),
                children: parts(children),
            }),
            "Text" => Node::Text(Text {
                id,
                tone,
                size: choice(&props, "size"),
                weight: choice(&props, "weight"),
                mono: on("mono"),
                italic: on("italic"),
                strike: on("strike"),
                truncate: on("truncate"),
                shrink_last: str_prop(&props, "shrink") == Some("last"),
                expandable: on("expandable"),
                clamp: on("clamp"),
                grow: on("grow"),
                on_click: str_prop(&props, "onClick")
                    .filter(|handler| valid_handler(handler))
                    .map(str::to_owned),
                title: title(&props),
                shimmer: on("shimmer"),
                mark: on("mark"),
                since: props.get("since").and_then(Value::as_f64).map(|ms| ms as i64),
                links: on("links"),
                href: str_prop(&props, "href").filter(|href| href.starts_with("https://") && href.len() <= 2048).map(str::to_owned),
                children: parts(children),
            }),
            "Terminal" => Node::Terminal(Terminal {
                id,
                pane: str_prop(&props, "pane").filter(|pane| pane.len() <= 20).unwrap_or_default().to_owned(),
                height: number(&props, "height").unwrap_or(220.0).clamp(80.0, 800.0) as f32,
                fill: on("fill"),
            }),
            "WebView" => Node::WebView(WebView {
                id,
                name: str_prop(&props, "name").filter(|name| name.len() <= WEB_NAME_MAX).unwrap_or_default().to_owned(),
                url: str_prop(&props, "url").and_then(web_address),
                commands: match props.get("command") {
                    Some(Value::Array(list)) => list.iter().take(WEB_COMMANDS_MAX).filter_map(web_command).collect(),
                    Some(one) => web_command(one).into_iter().collect(),
                    None => Vec::new(),
                },
                keep_data: props.get("keepData") != Some(&Value::Bool(false)),
                height: number(&props, "height").unwrap_or(400.0).clamp(80.0, 4000.0) as f32,
                fill: on("fill"),
                on_state: handler("onState"),
                on_result: handler("onResult"),
                hidden: on("hidden"),
                live: on("live"),
            }),
            "Avatar" => Node::Avatar(Avatar {
                id,
                name: text_prop(&props, "name"),
                large: str_prop(&props, "size") == Some("large"),
            }),
            "Skeleton" => Node::Skeleton(Skeleton {
                id,
                width: match props.get("width") {
                    Some(Value::Number(width)) => width
                        .as_f64()
                        .filter(|width| *width > 0.0)
                        .map(|width| Length::Points(width.min(2_000.0) as f32)),
                    Some(Value::String(width)) => width
                        .strip_suffix('%')
                        .and_then(|percent| percent.trim().parse::<f32>().ok())
                        .filter(|percent| *percent > 0.0)
                        .map(|percent| Length::Percent(percent.min(100.0))),
                    _ => None,
                },
                height: number(&props, "height").unwrap_or(14.0).clamp(1.0, 400.0) as f32,
            }),
            "Button" => Node::Button(Button {
                id,
                variant: choice(&props, "variant"),
                size: choice(&props, "size"),
                chip: on("chip"),
                pressed: on("pressed"),
                disabled: on("disabled"),
                label: str_prop(&props, "label").map(|label| cut(label.to_owned())),
                title: title(&props),
                wrap: on("wrap"),
                round: on("round"),
                on_click: handler("onClick"),
                on_key_down: handler("onKeyDown"),
                shortcut: str_prop(&props, "shortcut").and_then(Shortcut::parse),
                description: str_prop(&props, "description")
                    .filter(|text| !text.is_empty())
                    .map(|text| cut(text.to_owned())),
                show_on_hover: on("showOnHover"),
                children: parts(children),
            }),
            "Notice" => Node::Notice(Notice {
                id,
                tone,
                children: parts(children),
            }),
            "Toast" => Node::Toast(Toast {
                id,
                tone,
                on_dismiss: handler("onDismiss"),
                children: parts(children),
            }),
            "Spinner" => Node::Spinner(Spinner {
                id,
                large: str_prop(&props, "size") == Some("large"),
                label: text_prop(&props, "label"),
            }),
            "Checkbox" => Node::Checkbox(Checkbox {
                id,
                checked: on("checked"),
                disabled: on("disabled"),
                switch: on("switch"),
                label: text_prop(&props, "label"),
                on_change: handler("onChange"),
                children: parts(children),
            }),
            "Confirm" => {
                let mut slots = Slots(children);
                Node::Confirm(Confirm {
                    id,
                    title: slots.take("title"),
                    body: slots.take("body"),
                    actions: slots.take("actions"),
                })
            }
            "Badge" => Node::Badge(Badge {
                id,
                // An unset variant is the quiet pill; the loud filled one is only for a badge that asks for `default`.
                variant: str_prop(&props, "variant").and_then(Variant::choose).unwrap_or(Variant::Secondary),
                tone,
                small: str_prop(&props, "size") == Some("sm"),
                on_click: handler("onClick"),
                title: title(&props),
                children: parts(children),
            }),
            "Empty" => {
                let mut slots = Slots(children);
                Node::Empty(Empty {
                    id,
                    fill: on("fill"),
                    glow: str_prop(&props, "backdrop") == Some("glow"),
                    title: slots.take("title"),
                    description: slots.take("description"),
                    children: slots.take("children"),
                })
            }
            "Loading" => Node::Loading(Loading {
                id,
                children: parts(children),
            }),
            "Card" => Node::Card(Card {
                id,
                raised: str_prop(&props, "variant") == Some("raised"),
                rows: str_prop(&props, "variant") == Some("rows"),
                scrolls: on("scroll"),
                children: parts(children),
            }),
            "Heading" => Node::Heading(Heading {
                id,
                section: number(&props, "level") == Some(3.0),
                children: parts(children),
            }),
            "Dot" => Node::Dot(Dot {
                id,
                tone,
                color: number(&props, "color")
                    .filter(|color| (0.0..=16_777_215.0).contains(color))
                    .map(|color| color as u32),
                state: match str_prop(&props, "state") {
                    Some("working") => Some(crate::agents::AgentState::Working),
                    Some("blocked") => Some(crate::agents::AgentState::Blocked),
                    Some("done") => Some(crate::agents::AgentState::Done),
                    Some("idle") => Some(crate::agents::AgentState::Idle),
                    _ => None,
                },
            }),
            "Scroll" => Node::Scroll(Scroll {
                id,
                follow_end: str_prop(&props, "follow") == Some("end"),
                on_reach_start: handler("onReachStart"),
                on_reach_end: handler("onReachEnd"),
                text_scale: number(&props, "textScale").map(|scale| scale.clamp(0.5, 2.0) as f32),
                scroll_to: props.get("scrollTo").and_then(|to| {
                    let index = to.get("index")?.as_u64()?;
                    Some((usize::try_from(index).ok()?, to.get("token")?.as_u64()?))
                }),
                track: props
                    .get("track")
                    .and_then(Value::as_array)
                    .map(|track| track.iter().filter_map(Value::as_u64).take(MAX_TRACKED).filter_map(|at| usize::try_from(at).ok()).collect())
                    .unwrap_or_default(),
                on_track: handler("onTrack"),
                max_height: number(&props, "maxHeight").filter(|height| *height > 0.0).map(|height| height.min(10_000.0) as f32),
                children: parts(children),
            }),
            "Table" => Node::Table(Table { id, children }),
            "Bar" => Node::Bar(Bar {
                id,
                edge: choice(&props, "edge"),
                gap: choice(&props, "gap"),
                slim: str_prop(&props, "variant") == Some("slim"),
                children: parts(children),
            }),
            "Sheet" => {
                let mut slots = Slots(children);
                Node::Sheet(Sheet {
                    id,
                    title: slots.take("title"),
                    description: slots.take("description"),
                    header: slots.take("header"),
                    body: parts(slots.0),
                    on_close: handler("onClose"),
                })
            }
            "Split" => {
                let mut slots = Slots(children);
                Node::Split(Split {
                    id,
                    size: number(&props, "defaultSize")
                        .unwrap_or(280.0)
                        .clamp(16.0, 1_000.0) as f32,
                    fixed_end: str_prop(&props, "side") == Some("end"),
                    collapse_below: number(&props, "collapseBelow")
                        .filter(|below| *below > 0.0)
                        .map(|below| below.min(10_000.0) as f32),
                    pinned: on("pinned"),
                    float: on("float"),
                    dock: on("dock"),
                    on_resize: handler("onResize"),
                    min_size: number(&props, "minSize").unwrap_or(120.0).clamp(16.0, 1_000.0) as f32,
                    min_rest: number(&props, "minRest").unwrap_or(0.0).clamp(0.0, 10_000.0) as f32,
                    reset_size: number(&props, "resetSize").filter(|size| *size > 0.0).map(|size| size.min(1_000.0) as f32),
                    on_narrow: handler("onNarrow"),
                    on_close: handler("onClose"),
                    inset_below: number(&props, "insetBelow")
                        .filter(|below| *below > 0.0)
                        .map(|below| below.min(10_000.0) as f32),
                    inset_column: number(&props, "insetColumn")
                        .filter(|column| *column > 0.0)
                        .map(|column| column.min(10_000.0) as f32),
                    gap: str_prop(&props, "gap").and_then(Gap::choose).unwrap_or(Gap::None),
                    start: slots.take("start"),
                    end: slots.take("end"),
                })
            }
            "Sunburst" => Node::Sunburst(sunburst::parse(id, &props)?),
            "PromptRail" => Node::PromptRail(prompt_rail::parse(id, &props)),
            "Code" => Node::Code(code::parse(id, &props, tone)),
            "Dialog" => Node::Dialog(Dialog {
                id,
                title: text_prop(&props, "title"),
                width: number(&props, "width")
                    .unwrap_or(560.0)
                    .clamp(280.0, 1_000.0) as f32,
                on_close: handler("onClose"),
                children: parts(children),
            }),
            "Gauge" => {
                let mut slots = Slots(children);
                Node::Gauge(Gauge {
                    id,
                    label: text_prop(&props, "label"),
                    percent: number(&props, "percent").unwrap_or(0.0).clamp(0.0, 100.0),
                    levels: levels(&props),
                    note: slots.take("note"),
                })
            }
            "Meter" => Node::Meter(Meter {
                id,
                label: text_prop(&props, "label"),
                percent: number(&props, "percent").unwrap_or(0.0).clamp(0.0, 100.0),
                tone,
            }),
            "Board" => {
                let lanes = on("lanes");
                let (header, body) = if lanes {
                    let mut slots = Slots(children);
                    (slots.take("header"), slots.take("body"))
                } else {
                    (Vec::new(), parts(children))
                };
                Node::Board(Board {
                    id,
                    label: text_prop(&props, "label"),
                    lanes,
                    header,
                    body,
                })
            }
            "BoardRow" => Node::BoardRow(List {
                id,
                children: parts(children),
            }),
            "BoardColumn" => {
                let mut slots = Slots(children);
                Node::BoardColumn(BoardColumn {
                    id,
                    label: text_prop(&props, "label"),
                    cell: on("cell"),
                    count: props.get("count").and_then(Value::as_u64),
                    title: slots.take("title"),
                    actions: slots.take("actions"),
                    body: slots.take("body"),
                })
            }
            "List" => Node::List(List {
                id,
                children: parts(children),
            }),
            "ListItem" => {
                let mut slots = Slots(children);
                let actions = slots.take("actions");
                Node::ListItem(ListItem {
                    id,
                    children: parts(slots.0),
                    actions,
                })
            }
            "HoverCard" => {
                let mut slots = Slots(children);
                let card = slots.take("card");
                Node::HoverCard(HoverCard {
                    id,
                    card,
                    children: parts(slots.0),
                    width: number(&props, "width").map_or(300.0, |width| width.clamp(160.0, 480.0) as f32),
                })
            }
            "PopupMenu" => Node::PopupMenu(PopupMenu {
                id,
                // A point far outside any window is as good as none: the menu is moved in to stay in the window.
                x: number(&props, "x").map_or(0.0, |x| x.clamp(-100_000.0, 100_000.0) as f32),
                y: number(&props, "y").map_or(0.0, |y| y.clamp(-100_000.0, 100_000.0) as f32),
                on_close: handler("onClose"),
                items: children.into_iter().filter(|node| matches!(node, Node::Button(_))).collect(),
            }),
            "Disclosure" => {
                let mut slots = Slots(children);
                Node::Disclosure(Disclosure {
                    id,
                    open: props.get("open").and_then(Value::as_bool),
                    on_open_change: handler("onOpenChange"),
                    section: false,
                    summary: slots.take("summary"),
                    body: slots.take("body"),
                })
            }
            "Graph" => Node::Graph(graph(id, &props)?),
            "SearchField" => Node::SearchField(SearchField {
                id,
                label: text_prop(&props, "label"),
                placeholder: text_prop(&props, "placeholder"),
                value: str_prop(&props, "defaultValue").map(str::to_owned),
                on_input: str_prop(&props, "onInput")
                    .filter(|s| valid_handler(s))
                    .map(str::to_owned),
            }),
            "TextInput" | "Textarea" => Node::Input(Input {
                id,
                label: text_prop(&props, "label"),
                placeholder: text_prop(&props, "placeholder"),
                value: str_prop(&props, "value")
                    .unwrap_or_default()
                    .chars()
                    .take(MAX_INPUT_CHARS)
                    .collect(),
                rows: (kind == "Textarea")
                    .then(|| number(&props, "rows").unwrap_or(2.0).clamp(1.0, 24.0) as usize),
                max_rows: (kind == "Textarea").then(|| {
                    let rows = number(&props, "rows").unwrap_or(2.0).clamp(1.0, 24.0) as usize;
                    let most = number(&props, "maxRows").unwrap_or(12.0).clamp(1.0, 48.0) as usize;
                    most.max(rows)
                }),
                enter_submits: on("submitOnEnter"),
                takes_tab: on("takesTab"),
                takes_shift_tab: on("takesShiftTab"),
                history_keys: on("historyKeys"),
                bare: str_prop(&props, "variant") == Some("bare"),
                disabled: on("disabled"),
                max_length: number(&props, "maxLength")
                    .filter(|max| *max >= 0.0)
                    .map(|max| max as usize),
                on_change: handler("onChange"),
                on_key_down: handler("onKeyDown"),
                on_blur: handler("onBlur"),
                on_paste_image: handler("onPasteImage"),
                auto_focus: on("autoFocus"),
                select_on_focus: on("selectOnFocus"),
                open_on_focus: props.get("openOnFocus") != Some(&Value::Bool(false)),
                auto_highlight: props.get("autoHighlight") != Some(&Value::Bool(false)),
                password: kind == "TextInput" && str_prop(&props, "type") == Some("password"),
                time: kind == "TextInput" && str_prop(&props, "type") == Some("time"),
            }),
            "Icon" => match str_prop(&props, "name").and_then(IconName::choose) {
                Some(name) => Node::Icon(Icon {
                    id,
                    name,
                    size: number(&props, "size").unwrap_or(16.0).clamp(8.0, 32.0) as f32,
                }),
                None => return Ok(None),
            },
            // One that holds anything the app doesn't draw is left out, as an icon it has no name for is.
            "Svg" => match svg::parse(id, &props) {
                Some(drawing) => Node::Svg(drawing),
                None => return Ok(None),
            },
            // One whose `src` isn't an id a page registered is left out, as a path or a URL would be.
            "Image" => match picture::parse(id, &props) {
                Some(picture) => Node::Image(picture),
                None => return Ok(None),
            },
            "ListButton" => Node::ListButton(ListButton {
                button: Button {
                    id,
                    variant: Variant::Ghost,
                    size: ButtonSize::Default,
                    chip: false,
                    pressed: false,
                    disabled: on("disabled"),
                    label: None,
                    title: title(&props),
                    wrap: false,
                    round: false,
                    on_click: handler("onClick"),
                    on_key_down: handler("onKeyDown"),
                    shortcut: str_prop(&props, "shortcut").and_then(Shortcut::parse),
                    description: None,
                    show_on_hover: false,
                    children: parts(children),
                },
                indent: number(&props, "indent")
                    .unwrap_or_default()
                    .clamp(0.0, 240.0) as f32,
                expanded: props.get("aria-expanded").and_then(Value::as_bool),
                vertical: str_prop(&props, "layout") == Some("column"),
                baseline: str_prop(&props, "align") == Some("baseline"),
            }),
            "Select" | "Chips" => {
                let select = Select {
                    id,
                    label: text_prop(&props, "label"),
                    options: options(&props),
                    value: str_prop(&props, "value").map(|value| cut(value.to_owned())),
                    on_change: str_prop(&props, "onChange")
                        .filter(|handler| valid_handler(handler))
                        .map(str::to_owned),
                    line: kind == "Chips" && on("line"),
                    segmented: kind == "Chips" && on("segmented"),
                    quiet: kind == "Select" && str_prop(&props, "variant") == Some("ghost"),
                    badge: str_prop(&props, "badge")
                        .filter(|badge| !badge.is_empty())
                        .map(|badge| cut(badge.to_owned())),
                    display: str_prop(&props, "display")
                        .filter(|display| !display.is_empty())
                        .map(|display| cut(display.to_owned())),
                };
                if kind == "Chips" {
                    Node::Chips(select)
                } else {
                    Node::Select(select)
                }
            }
            "Menu" => {
                let mut buttons = children
                    .into_iter()
                    .filter(|node| matches!(node, Node::Button(_)));
                Node::Menu(Menu {
                    id,
                    trigger: buttons.next().into_iter().collect(),
                    items: buttons.collect(),
                    end: str_prop(&props, "align") == Some("end"),
                })
            }
            _ => return Ok(None),
        }))
    }
}

fn cut(text: String) -> String {
    if text.len() <= MAX_TEXT_CHARS {
        return text;
    }
    text.chars().take(MAX_TEXT_CHARS).collect()
}

fn str_prop<'a>(props: &'a Map<String, Value>, name: &str) -> Option<&'a str> {
    props.get(name).and_then(Value::as_str)
}

/// A string prop as text the page shows, cut to `MAX_TEXT_CHARS`, or nothing when it isn't one.
fn text_prop(props: &Map<String, Value>, name: &str) -> String {
    cut(str_prop(props, name).unwrap_or_default().to_owned())
}

/// A `title`: the hover text, none when empty.
fn title(props: &Map<String, Value>) -> Option<String> {
    str_prop(props, "title")
        .filter(|title| !title.is_empty())
        .map(|title| cut(title.to_owned()))
}

fn number(props: &Map<String, Value>, name: &str) -> Option<f64> {
    props.get(name).and_then(Value::as_f64)
}

/// `[warn, danger]`: anything but two numbers is no levels.
fn levels(props: &Map<String, Value>) -> Option<(f64, f64)> {
    match props.get("levels")?.as_array()?.as_slice() {
        [warn, danger] => Some((warn.as_f64()?, danger.as_f64()?)),
        _ => None,
    }
}

/// `[{value, label, disabled?, detail?, toggle?, heading?, checked?}]`: an entry with no string `value` isn't one, and a missing label is the value.
fn options(props: &Map<String, Value>) -> Vec<SelectOption> {
    option_list(props.get("options"), true)
}

/// `options`' rows, with those of a row's own `options` too when `nested` allows.
fn option_list(all: Option<&Value>, nested: bool) -> Vec<SelectOption> {
    let Some(Value::Array(all)) = all else {
        return Vec::new();
    };
    all.iter()
        .filter_map(|option| {
            let option = option.as_object()?;
            let value = cut(str_prop(option, "value")?.to_owned());
            Some(SelectOption {
                label: str_prop(option, "label")
                    .map_or_else(|| value.clone(), |l| cut(l.to_owned())),
                disabled: option.get("disabled") == Some(&Value::Bool(true)),
                detail: str_prop(option, "detail").map(|detail| cut(detail.to_owned())),
                toggle: option.get("toggle").and_then(Value::as_bool),
                heading: option.get("heading") == Some(&Value::Bool(true)),
                checked: option.get("checked").and_then(Value::as_bool),
                trailing: str_prop(option, "trailing").map(|text| cut(text.to_owned())),
                pill: str_prop(option, "pill").map(|text| cut(text.to_owned())),
                children: option_list(option.get("options").filter(|_| nested), false),
                value,
            })
        })
        .take(MAX_OPTIONS)
        .collect()
}

fn choice<T: Choice + Default>(props: &Map<String, Value>, name: &str) -> T {
    str_prop(props, name)
        .and_then(T::choose)
        .unwrap_or_default()
}

/// An id the page made: `<instance id>:<prop>`. Anything else isn't one, and a click on it would reach nothing.
fn valid_handler(handler: &str) -> bool {
    handler.len() <= MAX_HANDLER_BYTES
        && handler
            .split_once(':')
            .is_some_and(|(id, name)| !id.is_empty() && !name.is_empty())
        && handler
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b':' || b == b'_')
}

/// `stack` as a `Combobox` when it is exactly what wings-plugin-ui/native's `Combobox` renders: a field with no key or
/// blur handler of its own, then its options as plain chips in a wrapping row, or a small muted line when none match.
/// When the field holds an option, or is empty with none to suggest, the stack is the field alone and stays a stack.
fn combobox(stack: Stack) -> Node {
    let plain = stack.on_context_menu.is_none()
        && stack.gap == Gap::S
        && !stack.grow && stack.width.is_none() && stack.max_width.is_none();
    // A picker's field selects its text when it takes the keyboard, and hears when it loses it. A message box that
    // sends on Enter takes suggestions too, like a chat's slash commands: it keeps its Enter while none show.
    let field = |node: &Node| {
        matches!(node, Node::Input(input) if !input.password
            && (input.enter_submits || (input.rows.is_none() && input.on_key_down.is_none())))
    };
    // An option's label, and its detail after it as a small muted text.
    let chip = |node: &Node| {
        matches!(node, Node::Button(button) if button.chip
        && !button.pressed
        && button.variant == Variant::Default
        && button.size == ButtonSize::Default
        && button.label.is_none()
        && button.on_click.is_some()
        && button.on_key_down.is_none()
        && match button.children.split_last() {
            Some((Node::Text(detail), label)) => {
                option_detail(detail) && label.iter().all(|child| matches!(child, Node::Str { .. }))
            }
            _ => button.children.iter().all(|child| matches!(child, Node::Str { .. })),
        })
    };
    let chips = |node: &Node| {
        matches!(node, Node::Row(row) if row.gap == Gap::S
            && row.wrap
            && !row.grow
            && row.align == Align::Center
            && row.justify == Justify::Start
            && !row.children.is_empty()
            && row.children.iter().all(chip))
    };
    let none_match = |node: &Node| matches!(node, Node::Text(text) if option_detail(text));
    if !(plain
        && stack.children.len() == 2
        && field(&stack.children[0])
        && (chips(&stack.children[1]) || none_match(&stack.children[1])))
    {
        return Node::Stack(stack);
    }
    let mut children = stack.children;
    let (options, empty) = match children.pop() {
        Some(Node::Row(row)) => (row.children, Vec::new()),
        Some(Node::Text(text)) => (Vec::new(), text.children),
        _ => unreachable!("matched above"),
    };
    Node::Combobox(Combobox {
        id: stack.id,
        field: children,
        options,
        empty,
    })
}

/// A small muted text with nothing else to it: a combobox option's detail, or its line saying nothing matches.
fn option_detail(text: &Text) -> bool {
    text.size == TextSize::Small
        && text.tone == Some(Tone::Muted)
        && text.weight == Weight::Normal
        && !text.mono
        && !text.truncate
        && !text.clamp
        && !text.grow
        && text.on_click.is_none()
}

/// A component's named parts, taken out by name.
struct Slots(Vec<Node>);

impl Slots {
    fn take(&mut self, name: &str) -> Vec<Node> {
        match self
            .0
            .iter()
            .position(|node| matches!(node, Node::Slot(slot) if slot.name == name))
        {
            Some(at) => match self.0.remove(at) {
                Node::Slot(slot) => slot.children,
                _ => unreachable!("position found a slot"),
            },
            None => Vec::new(),
        }
    }
}

/// What a component without named parts holds: a slot out of place has nothing to belong to, so it's dropped.
fn parts(children: Vec<Node>) -> Vec<Node> {
    children
        .into_iter()
        .filter(|node| !matches!(node, Node::Slot(_)))
        .collect()
}

#[cfg(test)]
mod tests;

impl Tree {
    /// Whether `next` is this tree again but for the text of inputs that already show it (`shows(id, text)`): a page
    /// sending back what was just typed, as each keystroke in a message box makes Claude chat do. Drawing the page again
    /// for that rebuilt a whole long chat once more for every key.
    pub fn echoes(&self, next: &Tree, shows: impl Fn(u64, &str) -> bool) -> bool {
        fn values(nodes: &[Node], into: &mut std::collections::HashMap<u64, String>) {
            for node in nodes {
                if let Node::Input(input) = node {
                    into.insert(input.id, input.value.clone());
                }
                for part in node.parts() {
                    values(part, into);
                }
            }
        }
        /// Puts back the text the inputs had before, saying whether each one that changed already shows its new text.
        fn restore(nodes: &mut [Node], before: &std::collections::HashMap<u64, String>, shows: &dyn Fn(u64, &str) -> bool) -> bool {
            let mut all = true;
            for node in nodes {
                if let Node::Input(input) = node
                    && let Some(old) = before.get(&input.id)
                    && *old != input.value
                {
                    all &= shows(input.id, &input.value);
                    input.value = old.clone();
                }
                for part in node.parts_mut() {
                    all &= restore(part, before, shows);
                }
            }
            all
        }
        let mut before = std::collections::HashMap::new();
        values(&self.children, &mut before);
        let mut next = next.clone();
        restore(&mut next.children, &before, &shows) && next == *self
    }

    /// The nodes that differ between this tree and `next`, the innermost ones whose children are the same, by the path of
    /// node kinds to them, with both versions: what a page changed in a tree it sent.
    #[cfg(feature = "test-support")]
    pub fn differences(&self, next: &Tree) -> Vec<String> {
        fn kind(node: &Node) -> String {
            format!("{node:?}").chars().take_while(|c| c.is_alphanumeric()).collect()
        }
        fn short(node: &Node) -> String {
            let said = format!("{node:?}");
            said.chars().take(600).collect()
        }
        fn walk(a: &[Node], b: &[Node], path: &str, out: &mut Vec<String>) {
            if a.len() != b.len() {
                out.push(format!("{path}: {} children, then {}", a.len(), b.len()));
            }
            for (x, y) in a.iter().zip(b) {
                if x == y || out.len() >= 8 {
                    continue;
                }
                let path = format!("{path} > {}", kind(x));
                let (px, py) = (x.parts(), y.parts());
                if kind(x) == kind(y) && px.len() == py.len() && px != py {
                    for (pa, pb) in px.iter().zip(&py) {
                        if pa != pb {
                            walk(pa, pb, &path, out);
                        }
                    }
                } else {
                    out.push(format!("{path}:\n    was {}\n    now {}", short(x), short(y)));
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.children, &next.children, "", &mut out);
        out
    }
}
