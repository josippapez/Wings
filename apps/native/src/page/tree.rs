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
pub use code::Code;
pub use picture::{Picture, room_in as picture_room, valid_id as valid_image_id};
pub use prompt_rail::PromptRail;
pub use sunburst::{Geometry, Hit, Sunburst, Wedge};
pub use svg::Svg;

/// The largest tree JSON the host parses.
pub const MAX_JSON_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_NODES: usize = 5_000;
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
pub const MAX_INPUT_CHARS: usize = 20_000;

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
choice!(Justify { Start = "start", End = "end", Between = "between" });
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
    Sunburst(Sunburst),
    PromptRail(PromptRail),
    Dialog(Dialog),
    Sheet(Sheet),
    HoverCard(HoverCard),
    PopupMenu(PopupMenu),
    Terminal(Terminal),
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
    /// Heard, with `{ "narrow": bool }`, when the split's width crosses `collapse_below`, and once it first has a width.
    pub on_narrow: Option<String>,
    /// A floating pane with this handler hears Escape in it and a press outside it.
    pub on_close: Option<String>,
    /// With `float`: while the split is at least this wide, `start` is set in from the right by the floating pane's width,
    /// so nothing of it goes under the pane. `onNarrow` then also says `inset`.
    pub inset_below: Option<f32>,
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
        Ok(Tree {
            children: walk.children(root.children, 0)?,
            fill: root.props.get("fill").and_then(Value::as_bool) == Some(true),
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
    let mut palette = crate::theme::Palette::APP;
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
        palette.popover = found << 8 | (crate::theme::POPOVER & 0xff);
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
            Node::Sunburst(n) => Some(n.id),
            Node::PromptRail(n) => Some(n.id),
            Node::Dialog(n) => Some(n.id),
            Node::Sheet(n) => Some(n.id),
            Node::Terminal(n) => Some(n.id),
            Node::Code(n) => Some(n.id),
            Node::Actions(n) => Some(n.id),
        }
    }

    /// What the component holds, one list for each of its named parts.
    pub fn parts(&self) -> Vec<&[Node]> {
        match self {
            Node::Str { .. } => vec![],
            Node::Slot(n) => vec![&n.children],
            Node::Page(n) => vec![&n.toolbar, &n.body],
            Node::Toolbar(n) => vec![&n.title, &n.filters, &n.actions],
            Node::Stack(n) => vec![&n.children],
            Node::Row(n) => vec![&n.children],
            Node::Text(n) => vec![&n.children],
            Node::Button(n) => vec![&n.children],
            Node::ListButton(n) => vec![&n.button.children],
            Node::Notice(n) => vec![&n.children],
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
            | Node::Code(_) => vec![],
            Node::Scroll(n) => vec![&n.children],
            Node::Split(n) => vec![&n.start, &n.end],
            Node::Bar(n) => vec![&n.children],
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
            Node::Page(n) => vec![&mut n.toolbar, &mut n.body],
            Node::Toolbar(n) => vec![&mut n.title, &mut n.filters, &mut n.actions],
            Node::Stack(n) => vec![&mut n.children],
            Node::Row(n) => vec![&mut n.children],
            Node::Text(n) => vec![&mut n.children],
            Node::Button(n) => vec![&mut n.children],
            Node::ListButton(n) => vec![&mut n.button.children],
            Node::Notice(n) => vec![&mut n.children],
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
            | Node::Code(_) => vec![],
            Node::Scroll(n) => vec![&mut n.children],
            Node::Split(n) => vec![&mut n.start, &mut n.end],
            Node::Bar(n) => vec![&mut n.children],
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
                | "Sunburst"
                | "PromptRail"
                | "Menu"
                | "Dialog"
                | "Sheet"
                | "Terminal"
                | "Code"
                | "ListRow"
                | "Section"
                | "Actions"
        ) {
            return Ok(None);
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
                children: parts(children),
            }),
            "Notice" => Node::Notice(Notice {
                id,
                tone,
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
                variant: choice(&props, "variant"),
                tone,
                small: str_prop(&props, "size") == Some("sm"),
                on_click: handler("onClick"),
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
                    on_narrow: handler("onNarrow"),
                    on_close: handler("onClose"),
                    inset_below: number(&props, "insetBelow")
                        .filter(|below| *below > 0.0)
                        .map(|below| below.min(10_000.0) as f32),
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

/// `[{value, label, disabled?}]`: an entry with no string `value` isn't one, and a missing label is the value.
fn options(props: &Map<String, Value>) -> Vec<SelectOption> {
    let Some(Value::Array(all)) = props.get("options") else {
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
mod tests {
    use super::*;
    use serde_json::json;

    fn tree(children: Value) -> Result<Tree, TreeError> {
        Tree::parse(&json!({"id":0,"type":"#root","props":{},"children":children}).to_string())
    }

    fn button(props: Value) -> Value {
        json!({"id":2,"type":"Button","props":props,"children":[{"id":3,"type":"#text","text":"Go"}]})
    }

    #[test]
    fn a_page_paints_in_the_colours_of_its_palette_and_the_apps_for_the_rest() {
        let painted = |palette: Value| {
            Tree::parse(&json!({"id":0,"type":"#root","props":{"palette":palette},"children":[]}).to_string())
                .unwrap()
                .palette
        };
        let palette = painted(json!({
            "page": "#1f1a15", "text": "#F0E7D7", "popover": "#3a3229",
            // Not colours: the app's stay.
            "muted": "red", "info": "#12345", "danger": 7,
            "ground": {"from": "#1f1a15", "to": "#312a22", "angle": -90},
        }))
        .unwrap();
        assert_eq!((palette.surface, palette.foreground), (0x1f1a15, 0xf0e7d7));
        assert_eq!(palette.popover, 0x3a3229f2, "see-through as the app's popover");
        let app = crate::theme::Palette::APP;
        assert_eq!((palette.muted_foreground, palette.info, palette.danger), (app.muted_foreground, app.info, app.danger));
        assert_eq!(palette.ground, Some((0x1f1a15, 0x312a22, 270.0)));
        // Nothing it can paint is no palette at all.
        assert_eq!(painted(json!({"page": "url(x)"})), None);
        assert_eq!(tree(json!([])).unwrap().palette, None);
    }

    #[test]
    fn parses_a_page_into_its_parts() {
        let parsed = tree(json!([{"id":1,"type":"Page","props":{"flush":true},"children":[
            {"id":4,"type":"Slot","props":{"name":"toolbar"},"children":[
                {"id":5,"type":"Toolbar","props":{},"children":[
                    {"id":6,"type":"Slot","props":{"name":"title"},"children":[{"id":7,"type":"#text","text":"Pilot"}]}]}]},
            {"id":8,"type":"Slot","props":{"name":"body"},"children":[
                button(json!({"variant":"outline","onClick":"2:onClick"}))]}]}]))
        .unwrap();
        let Node::Page(page) = &parsed.children[0] else {
            panic!("a page")
        };
        assert!(page.flush);
        let Node::Toolbar(toolbar) = &page.toolbar[0] else {
            panic!("a toolbar")
        };
        assert_eq!(plain_text(&toolbar.title), "Pilot");
        let Node::Button(button) = &page.body[0] else {
            panic!("a button")
        };
        assert_eq!(button.variant, Variant::Outline);
        assert_eq!(button.on_click.as_deref(), Some("2:onClick"));
        assert_eq!(parsed.text(), "Pilot Go");
    }

    #[test]
    fn a_button_and_a_text_keep_their_title_and_none_when_it_is_absent_or_empty() {
        let parsed = tree(json!([
            button(json!({"title":"Opens a terminal"})),
            button(json!({"title":""})),
            button(json!({})),
            {"id":50,"type":"Text","props":{"title":"/work"},"children":[]},
            {"id":51,"type":"Text","props":{},"children":[]},
        ]))
        .unwrap();
        let titles: Vec<_> = parsed
            .children
            .iter()
            .map(|node| match node {
                Node::Button(button) => button.title.as_deref(),
                Node::Text(text) => text.title.as_deref(),
                _ => panic!("a button or a text"),
            })
            .collect();
        assert_eq!(
            titles,
            [Some("Opens a terminal"), None, None, Some("/work"), None]
        );
    }

    #[test]
    fn a_list_button_keeps_its_title() {
        let parsed = tree(json!([
            {"id":1,"type":"ListButton","props":{"title":"Open Changes"},"children":[]},
            {"id":2,"type":"ListButton","props":{},"children":[]},
        ]))
        .unwrap();
        let titles: Vec<_> = parsed
            .children
            .iter()
            .map(|node| match node {
                Node::ListButton(item) => item.button.title.as_deref(),
                _ => panic!("a list button"),
            })
            .collect();
        assert_eq!(titles, [Some("Open Changes"), None]);
    }

    #[test]
    fn a_button_keeps_its_size_and_a_chip_its_pressed_state() {
        let parsed = tree(json!([
            button(json!({"size":"sm"})),
            button(json!({"chip":true,"pressed":true})),
            button(json!({"size":"huge"})),
        ]))
        .unwrap();
        let buttons: Vec<_> = parsed
            .children
            .iter()
            .map(|node| match node {
                Node::Button(button) => (button.size, button.chip, button.pressed),
                _ => panic!("a button"),
            })
            .collect();
        assert_eq!(
            buttons,
            [
                (ButtonSize::Sm, false, false),
                (ButtonSize::Default, true, true),
                (ButtonSize::Default, false, false),
            ]
        );
    }

    #[test]
    fn a_button_is_round_only_when_it_says_so() {
        let parsed = tree(json!([
            button(json!({"size":"icon-sm","round":true})),
            button(json!({"round":"yes"})),
            button(json!({})),
        ]))
        .unwrap();
        let round: Vec<_> = parsed
            .children
            .iter()
            .map(|node| matches!(node, Node::Button(button) if button.round))
            .collect();
        assert_eq!(round, [true, false, false]);
    }

    #[test]
    fn unknown_components_are_dropped_with_what_is_in_them() {
        let parsed = tree(json!([
            {"id":1,"type":"Hologram","props":{},"children":[{"id":2,"type":"#text","text":"never"}]},
            {"id":3,"type":"Stack","props":{},"children":[
                {"id":4,"type":"script","props":{},"children":[]},
                {"id":5,"type":"#text","text":"kept"}]},
        ]))
        .unwrap();
        assert_eq!(parsed.children.len(), 1);
        assert_eq!(parsed.text(), "kept");
    }

    #[test]
    fn unknown_values_are_the_defaults_and_a_bad_handler_is_none() {
        let parsed = tree(json!([
            button(json!({"variant":"flashy","onClick":"../../etc"})),
            button(json!({"onClick":"2:onClick","disabled":true})),
            {"id":9,"type":"Stack","props":{"gap":"huge"},"children":[]},
        ]))
        .unwrap();
        let Node::Button(first) = &parsed.children[0] else {
            panic!("a button")
        };
        assert_eq!(
            (first.variant, first.on_click.clone()),
            (Variant::Default, None)
        );
        let Node::Button(disabled) = &parsed.children[1] else {
            panic!("a button")
        };
        assert!(disabled.disabled && disabled.on_click.is_none());
        let Node::Stack(stack) = &parsed.children[2] else {
            panic!("a stack")
        };
        assert_eq!(stack.gap, Gap::M);
    }

    #[test]
    fn notices_spinners_checkboxes_and_confirms_have_only_their_supported_parts() {
        let parsed = tree(json!([
            {"id":1,"type":"Notice","props":{"tone":"warn"},"children":[text(2, "Careful")]},
            {"id":3,"type":"Spinner","props":{"size":"large","label":"Refreshing"},"children":[]},
            {"id":4,"type":"Checkbox","props":{"checked":true,"onChange":"4:onChange"},"children":[text(5, "Delete branch")]},
            {"id":6,"type":"Confirm","props":{},"children":[
                slot("title", json!([text(7, "Remove?")])),
                slot("body", json!([text(8, "This cannot be undone.")])),
                slot("actions", json!([button(json!({"onClick":"9:onClick"}))]))]}]))
        .unwrap();
        let Node::Notice(notice) = &parsed.children[0] else {
            panic!("a notice")
        };
        assert_eq!(
            (notice.tone, plain_text(&notice.children)),
            (Some(Tone::Warn), "Careful".to_owned())
        );
        let Node::Spinner(spinner) = &parsed.children[1] else {
            panic!("a spinner")
        };
        assert!(spinner.large && spinner.label == "Refreshing");
        let Node::Checkbox(checkbox) = &parsed.children[2] else {
            panic!("a checkbox")
        };
        assert_eq!(
            (
                checkbox.checked,
                checkbox.on_change.as_deref(),
                plain_text(&checkbox.children)
            ),
            (true, Some("4:onChange"), "Delete branch".to_owned())
        );
        let Node::Confirm(confirm) = &parsed.children[3] else {
            panic!("a confirm")
        };
        assert_eq!(plain_text(&confirm.title), "Remove?");
        assert_eq!(plain_text(&confirm.body), "This cannot be undone.");
        assert_eq!(plain_text(&confirm.actions), "Go");
    }

    fn slot(name: &str, children: Value) -> Value {
        json!({"id":90,"type":"Slot","props":{"name":name},"children":children})
    }

    fn text(id: u64, text: &str) -> Value {
        json!({"id":id,"type":"#text","text":text})
    }

    fn parse_one(node: Value) -> Node {
        tree(json!([node])).unwrap().children.remove(0)
    }

    #[test]
    fn a_chat_column_rises_in_a_scroll_that_follows_its_end_and_its_thinking_shimmers() {
        let Node::Scroll(scroll) = parse_one(json!({"id":1,"type":"Scroll","props":{"follow":"end"},"children":[
            {"id":2,"type":"Stack","props":{"enter":"rise"},"children":[
                {"id":3,"type":"Text","props":{"shimmer":true},"children":[text(4, "Thinking")]}
            ]}
        ]})) else { panic!() };
        assert!(scroll.follow_end && scroll.on_reach_start.is_none());
        let Node::Scroll(older) = parse_one(json!({"id":1,"type":"Scroll","props":{"follow":"end","onReachStart":"1:onReachStart"},"children":[]})) else { panic!() };
        assert_eq!(older.on_reach_start.as_deref(), Some("1:onReachStart"));
        let Node::Stack(column) = &scroll.children[0] else { panic!() };
        assert!(column.enter);
        let Node::Text(thinking) = &column.children[0] else { panic!() };
        assert!(thinking.shimmer);
        assert_eq!(thinking.since, None);
        let Node::Text(running) = parse_one(json!({"id":1,"type":"Text","props":{"shimmer":true,"since":1_763_470_000_000u64},"children":[text(2, "Running pnpm test")]})) else { panic!() };
        assert_eq!(running.since, Some(1_763_470_000_000));
        assert_eq!(thinking.children[0].id(), Some(4));
        let Node::Scroll(plain) = parse_one(json!({"id":1,"type":"Scroll","props":{},"children":[
            {"id":2,"type":"Stack","props":{"enter":"fade"},"children":[]}
        ]})) else { panic!() };
        assert!(!plain.follow_end);
        assert!(matches!(&plain.children[0], Node::Stack(stack) if !stack.enter));
    }

    #[test]
    fn a_scroll_takes_a_place_to_scroll_to_and_children_to_track() {
        let scroll = |props: Value| match parse_one(json!({"id":1,"type":"Scroll","props":props,"children":[]})) {
            Node::Scroll(scroll) => scroll,
            _ => panic!(),
        };
        let plain = scroll(json!({}));
        assert_eq!((plain.scroll_to, plain.track.is_empty(), plain.on_track), (None, true, None));
        let aimed = scroll(json!({"scrollTo":{"index":7,"token":3},"track":[2,5,9,"x",-1],"onTrack":"1:onTrack"}));
        assert_eq!(aimed.scroll_to, Some((7, 3)));
        assert_eq!(aimed.track, [2, 5, 9]);
        assert_eq!(aimed.on_track.as_deref(), Some("1:onTrack"));
        // A place that isn't two whole numbers is no place.
        assert_eq!(scroll(json!({"scrollTo":{"index":-1,"token":3}})).scroll_to, None);
        assert_eq!(scroll(json!({"scrollTo":{"index":1}})).scroll_to, None);
        // A slim pane, like a rail, is as narrow as asked down to 16 points.
        let Node::Split(rail) = parse_one(json!({"id":1,"type":"Split","props":{"defaultSize":26},"children":[]})) else { panic!() };
        assert_eq!(rail.size, 26.0);
    }

    #[test]
    fn a_scroll_takes_a_text_scale_within_reason() {
        let scale = |props: Value| match parse_one(json!({"id":1,"type":"Scroll","props":props,"children":[]})) {
            Node::Scroll(scroll) => scroll.text_scale,
            _ => panic!(),
        };
        assert_eq!(scale(json!({})), None);
        assert_eq!(scale(json!({"textScale":1.25})), Some(1.25));
        assert_eq!(scale(json!({"textScale":40})), Some(2.0));
        assert_eq!(scale(json!({"textScale":0})), Some(0.5));
        assert_eq!(scale(json!({"textScale":"big"})), None);
    }

    /// A Markdown quote's bar is a prop of the stack, so it is drawn by the box and never copied as a word.
    #[test]
    fn a_stack_with_quote_set_is_a_quote_and_a_plain_one_is_not() {
        let quote = |props: serde_json::Value| match parse_one(json!({"id":1,"type":"Stack","props":props,"children":[]})) {
            Node::Stack(stack) => stack.quote,
            _ => panic!("a stack"),
        };
        assert!(quote(json!({"quote":true})));
        assert!(!quote(json!({})));
        assert!(!quote(json!({"quote":"yes"})));
    }

    /// A plugin's settings page grouped its rows in a plain card, with no hairlines between them as Wings' own have.
    #[test]
    fn a_card_of_rows_says_so_and_a_plain_or_raised_one_does_not() {
        let card = |variant: serde_json::Value| match parse_one(json!({"id":1,"type":"Card","props":{"variant":variant},"children":[]})) {
            Node::Card(card) => (card.rows, card.raised),
            _ => panic!("a card"),
        };
        assert_eq!(card(json!("rows")), (true, false));
        let Node::Card(scrolls) = parse_one(json!({"id":1,"type":"Card","props":{"scroll":true},"children":[]})) else { panic!("a card") };
        assert!(scrolls.scrolls);
        assert_eq!(card(json!("raised")), (false, true));
        assert_eq!(card(serde_json::Value::Null), (false, false));
    }

    #[test]
    fn card_heading_list_and_list_item_hold_their_children() {
        let Node::Card(card) = parse_one(json!({"id":1,"type":"Card","props":{},"children":[
            {"id":2,"type":"Heading","props":{},"children":[text(3, "Usage")]},
            {"id":4,"type":"List","props":{},"children":[
                {"id":5,"type":"ListItem","props":{},"children":[text(6, "one")]},
                // A slot out of place has nothing to belong to.
                slot("note", json!([text(7, "lost")]))]}]}))
        else {
            panic!("a card")
        };
        assert_eq!(plain_text(&card.children), "Usageone");
        let [Node::Heading(_), Node::List(list)] = card.children.as_slice() else {
            panic!("a heading and a list")
        };
        assert!(matches!(list.children.as_slice(), [Node::ListItem(_)]));
    }

    #[test]
    fn a_list_items_actions_slot_is_its_actions_and_not_one_of_its_parts() {
        let Node::ListItem(item) = parse_one(
            json!({"id":1,"type":"ListItem","props":{},"children":[
            text(2, "Code review"),
            slot("actions", json!([{"id":4,"type":"Button","props":{"onClick":"4:click"},"children":[text(5, "Edit")]}]))]}),
        ) else {
            panic!("a list item")
        };
        assert_eq!(plain_text(&item.children), "Code review");
        assert!(matches!(item.actions.as_slice(), [Node::Button(_)]));
    }

    #[test]
    fn a_heading_of_level_3_names_a_section() {
        let section = |props: Value| match parse_one(
            json!({"id":1,"type":"Heading","props":props,"children":[text(2, "Entries")]}),
        ) {
            Node::Heading(heading) => heading.section,
            other => panic!("a heading, not {other:?}"),
        };
        // It was drawn as every other heading, small and muted, where the desktop draws a section's 14px title.
        assert!(section(json!({"level":3})));
        assert!(!section(json!({"level":2})));
        assert!(!section(json!({})));
    }

    #[test]
    fn only_a_text_input_of_type_password_is_a_password() {
        let password = |kind: &str, kind_prop: &str| match parse_one(
            json!({"id":1,"type":kind,"props":{"label":"Token","type":kind_prop},"children":[]}),
        ) {
            Node::Input(input) => input.password,
            other => panic!("{other:?}"),
        };
        assert!(password("TextInput", "password"));
        assert!(!password("TextInput", "time"));
        assert!(!password("TextInput", "text"));
        assert!(!password("Textarea", "password"));
    }

    #[test]
    fn a_textarea_takes_shift_tab_and_history_keys_only_when_the_page_asks() {
        let props = |props: Value| match parse_one(json!({"id":1,"type":"Textarea","props":props,"children":[]})) {
            Node::Input(input) => (input.takes_shift_tab, input.history_keys),
            other => panic!("{other:?}"),
        };
        assert_eq!(props(json!({"takesShiftTab":true,"historyKeys":true})), (true, true));
        assert_eq!(props(json!({"takesShiftTab":true})), (true, false));
        assert_eq!(props(json!({"historyKeys":false})), (false, false));
        assert_eq!(props(json!({})), (false, false));
    }

    #[test]
    fn a_textarea_takes_tab_only_when_the_page_asks() {
        let takes = |props: Value| match parse_one(json!({"id":1,"type":"Textarea","props":props,"children":[]})) {
            Node::Input(input) => input.takes_tab,
            other => panic!("{other:?}"),
        };
        assert!(takes(json!({"takesTab":true})));
        assert!(!takes(json!({"takesTab":false})));
        assert!(!takes(json!({})));
    }

    #[test]
    fn a_dot_has_a_tone_or_is_green() {
        let dot = |props| match parse_one(json!({"id":1,"type":"Dot","props":props,"children":[]}))
        {
            Node::Dot(dot) => dot.tone,
            _ => panic!("a dot"),
        };
        assert_eq!(dot(json!({})), None);
        assert_eq!(dot(json!({"tone":"danger"})), Some(Tone::Danger));
        assert_eq!(dot(json!({"tone":"muted"})), Some(Tone::Muted));
        assert_eq!(dot(json!({"tone":"purple"})), None);
        assert_eq!(dot(json!({"tone":7})), None);
    }

    #[test]
    fn a_dot_can_show_an_agents_state() {
        let state = |props| match parse_one(json!({"id":1,"type":"Dot","props":props,"children":[]})) {
            Node::Dot(dot) => dot.state,
            _ => panic!("a dot"),
        };
        assert_eq!(state(json!({"state":"working"})), Some(crate::agents::AgentState::Working));
        assert_eq!(state(json!({"state":"blocked"})), Some(crate::agents::AgentState::Blocked));
        assert_eq!(state(json!({"state":"asleep"})), None);
    }

    fn gauge(props: Value, children: Value) -> Gauge {
        match parse_one(json!({"id":1,"type":"Gauge","props":props,"children":children})) {
            Node::Gauge(gauge) => gauge,
            _ => panic!("a gauge"),
        }
    }

    #[test]
    fn a_gauge_has_a_label_a_bounded_percent_levels_and_a_note() {
        let full = gauge(
            json!({"label":"Memory","percent":42.5,"levels":[70,90]}),
            json!([slot("note", json!([text(2, "Resets in 2h")]))]),
        );
        assert_eq!(full.label, "Memory");
        assert_eq!(full.percent, 42.5);
        assert_eq!(full.levels, Some((70.0, 90.0)));
        assert_eq!(plain_text(&full.note), "Resets in 2h");

        // A page that sends nonsense gets a bar that is empty or full, not one that draws outside its box.
        let bounded =
            |percent: Value| gauge(json!({"label":"x","percent":percent}), json!([])).percent;
        assert_eq!(bounded(json!(-5)), 0.0);
        assert_eq!(bounded(json!(1e300)), 100.0);
        assert_eq!(bounded(json!("40")), 0.0);
        assert_eq!(bounded(Value::Null), 0.0);

        let none = gauge(json!({}), json!([]));
        assert_eq!(
            (none.label.as_str(), none.percent, none.levels),
            ("", 0.0, None)
        );
        assert!(none.note.is_empty());
        // Levels are two numbers or nothing.
        for levels in [
            json!([70]),
            json!([1, 2, 3]),
            json!(["a", "b"]),
            json!(70),
            json!([70, null]),
        ] {
            assert_eq!(
                gauge(json!({"levels":levels}), json!([])).levels,
                None,
                "{levels}"
            );
        }
    }

    #[test]
    fn a_gauge_turns_warn_and_danger_at_its_levels() {
        let level = |percent: f64, levels: Value| {
            gauge(json!({"percent":percent,"levels":levels}), json!([])).level()
        };
        assert_eq!(level(40.0, json!([70, 90])), Level::Normal);
        assert_eq!(level(69.0, json!([70, 90])), Level::Normal);
        assert_eq!(level(70.0, json!([70, 90])), Level::Warn);
        assert_eq!(level(85.0, json!([70, 90])), Level::Warn);
        assert_eq!(level(90.0, json!([70, 90])), Level::Danger);
        assert_eq!(level(100.0, json!([70, 90])), Level::Danger);
        assert_eq!(level(99.0, json!(null)), Level::Normal);
    }

    #[test]
    fn a_disclosure_has_a_summary_and_a_body() {
        let Node::Disclosure(disclosure) =
            parse_one(json!({"id":1,"type":"Disclosure","props":{},"children":[
            slot("body", json!([text(2, "details")])),
            slot("summary", json!([text(3, "2 need sign-in")]))]}))
        else {
            panic!("a disclosure")
        };
        assert_eq!(plain_text(&disclosure.summary), "2 need sign-in");
        assert_eq!(plain_text(&disclosure.body), "details");
        assert_eq!(disclosure.id, 1);
    }

    fn select(props: Value) -> Select {
        match parse_one(json!({"id":1,"type":"Select","props":props,"children":[]})) {
            Node::Select(select) => select,
            _ => panic!("a select"),
        }
    }

    #[test]
    fn chips_can_be_a_segmented_toggle_and_a_select_cannot() {
        let props = json!({"label":"Mode","segmented":true,"options":[{"value":"a"}]});
        let Node::Chips(chips) = parse_one(json!({"id":1,"type":"Chips","props":props,"children":[]})) else {
            panic!("chips")
        };
        assert!(chips.segmented);
        assert!(!select(props).segmented);
    }

    #[test]
    fn a_select_has_options_a_value_and_a_handler() {
        let select = select(
            json!({"label":"Session","value":"b","onChange":"1:onChange","options":[
            {"value":"a","label":"Alpha"},
            {"value":"b","label":"Beta","disabled":true},
            {"value":"c"},
            {"label":"no value"},
            "not an object",
            {"value":3,"label":"not a string"}]}),
        );
        assert_eq!(select.label, "Session");
        assert_eq!(select.on_change.as_deref(), Some("1:onChange"));
        assert_eq!(
            select.options,
            [
                SelectOption {
                    value: "a".into(),
                    label: "Alpha".into(),
                    disabled: false
                },
                SelectOption {
                    value: "b".into(),
                    label: "Beta".into(),
                    disabled: true
                },
                // No label is the value, so the option still shows something.
                SelectOption {
                    value: "c".into(),
                    label: "c".into(),
                    disabled: false
                },
            ]
        );
        assert_eq!((select.current(), select.shown()), (Some("b"), "Beta"));
    }

    /// Tabs of the "line" variant are chips the app underlines; a select has no such look.
    #[test]
    fn chips_can_be_line_tabs_and_a_select_cannot() {
        let props = json!({"label":"Pull request","line":true,"options":[{"value":"a"}]});
        let Node::Chips(tabs) =
            parse_one(json!({"id":1,"type":"Chips","props":props,"children":[]}))
        else {
            panic!("chips")
        };
        assert!(tabs.line);
        assert!(!select(props).line);
    }

    #[test]
    fn a_select_without_a_value_shows_its_first_option_and_a_value_nobody_has_shows_itself() {
        let options = json!([{"value":"a","label":"Alpha"},{"value":"b","label":"Beta"}]);
        let first = select(json!({"options":options}));
        assert_eq!((first.current(), first.shown()), (Some("a"), "Alpha"));
        let unknown = select(json!({"options":options,"value":"zzz"}));
        assert_eq!(unknown.shown(), "zzz");
        let empty = select(json!({}));
        assert_eq!((empty.current(), empty.shown()), (None, ""));
    }

    #[test]
    fn a_select_is_bounded_and_its_bad_handler_is_none() {
        // One label too long: all of them would be more than a page may send.
        let label = |n: usize| if n == 0 { "x".repeat(MAX_TEXT_CHARS * 2) } else { "x".into() };
        let many: Vec<Value> = (0..MAX_OPTIONS * 2)
            .map(|n| json!({"value":n.to_string(),"label":label(n)}))
            .collect();
        let bounded = select(json!({"options":many,"onChange":"../../etc"}));
        assert_eq!(bounded.options.len(), MAX_OPTIONS);
        assert_eq!(bounded.options[0].label.chars().count(), MAX_TEXT_CHARS);
        assert_eq!(bounded.on_change, None);
    }

    #[test]
    fn sizes_are_bounded() {
        let long = "x".repeat(MAX_TEXT_CHARS * 2);
        let parsed = tree(json!([{"id":1,"type":"#text","text":long}])).unwrap();
        assert_eq!(parsed.text().chars().count(), MAX_TEXT_CHARS);

        let many: Vec<Value> = (0..=MAX_NODES)
            .map(|id| json!({"id":id,"type":"#text","text":"a"}))
            .collect();
        assert_eq!(tree(Value::Array(many)), Err(TreeError::TooManyNodes));

        let mut deep = json!([]);
        for id in 0..=MAX_DEPTH {
            deep = json!([{"id":id,"type":"Stack","props":{},"children":deep}]);
        }
        assert_eq!(tree(deep), Err(TreeError::TooDeep));

        let huge = format!(
            r##"{{"id":0,"type":"#root","children":[{{"id":1,"type":"#text","text":"{}"}}]}}"##,
            "x".repeat(MAX_JSON_BYTES)
        );
        assert_eq!(Tree::parse(&huge), Err(TreeError::TooLarge));
    }

    #[test]
    fn what_is_not_a_tree_is_refused() {
        for json in [
            "",
            "[]",
            "null",
            r#"{"id":0,"type":"Page"}"#,
            r##"{"type":"#root"}"##,
        ] {
            assert!(Tree::parse(json).is_err(), "{json}");
        }
    }

    #[test]
    fn graph_windows_preserve_geometry_and_reject_oversized_or_malformed_data() {
        let mut props = serde_json::json!({ "total": 10000, "lanes": 2, "selected": -1, "offset": 5000,
            "rows": [{"hash":"abc", "subject":"x".repeat(3000), "author":"Ada", "date":"2026-11-06", "refs":["H:main"], "lane":1, "color":2, "merge":true, "lines":[0,1,0,2,1,1,1,2]}]
        }).as_object().unwrap().clone();
        let parsed = graph(1, &props).unwrap();
        assert_eq!(
            parsed.rows[0].subject.len(),
            3000,
            "graph data is not generic truncated text"
        );
        assert_eq!(parsed.rows[0].lines, [0, 1, 0, 2, 1, 1, 1, 2]);
        assert!(parsed.rows[0].merge);
        let row = props["rows"][0].clone();
        props.insert("rows".into(), serde_json::json!(vec![row.clone(); 257]));
        assert!(graph(1, &props).is_err());
        props.insert("rows".into(), serde_json::json!([row]));
        props["rows"][0]["lines"] = serde_json::json!([0, 1, 3, 0]);
        assert!(graph(1, &props).is_err());
        props["rows"][0]["lines"] = serde_json::json!([0, 1, 0, 0]);
        props["rows"][0]["subject"] = serde_json::json!("x".repeat(4097));
        assert!(graph(1, &props).is_err());
    }
    fn svg(props: Value) -> Option<Svg> {
        match tree(json!([{"id":1,"type":"Svg","props":props,"children":[]}]))
            .unwrap()
            .children
            .pop()
        {
            Some(Node::Svg(drawing)) => Some(drawing),
            None => None,
            Some(other) => panic!("an svg, not {other:?}"),
        }
    }

    #[test]
    fn an_svg_is_drawn_from_markup_the_app_writes_from_its_checked_shapes() {
        let drawing = svg(json!({
            "viewBox":"0 0 16 16","size":18,"tone":"warn","label":"Epic",
            "fill":"none","stroke":"currentColor","strokeWidth":"1.4","strokeLinecap":"round",
            "shapes":[
                ["rect",{"x":"2.5","y":2.5,"width":11,"height":"11","rx":2.5}],
                ["path",{"d":"M9 1.8 3.5 9H8l-1 5.2L12.5 7H8Z","fillRule":"evenodd"}],
                ["g",{"opacity":0.3},[["circle",{"cx":8,"cy":8,"r":2}],["line",{"x1":0,"y1":0,"x2":-4,"y2":4}]]],
                ["polygon",{"points":"0,0 4,0 4,4"}]]
        }))
        .unwrap();
        assert_eq!(
            (drawing.size, drawing.tone, drawing.label.as_deref()),
            (18.0, Some(Tone::Warn), Some("Epic"))
        );
        assert_eq!(
            drawing.markup,
            concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round">"#,
                r#"<rect x="2.5" y="2.5" width="11" height="11" rx="2.5"/>"#,
                r#"<path d="M9 1.8 3.5 9H8l-1 5.2L12.5 7H8Z" fill-rule="evenodd"/>"#,
                r#"<g opacity="0.3"><circle cx="8" cy="8" r="2"/><line x1="0" y1="0" x2="-4" y2="4"/></g>"#,
                r#"<polygon points="0,0 4,0 4,4"/>"#,
                "</svg>"
            )
        );
        // Without a size it is 16 points, and a size past an icon's is cut to 64.
        let plain = |props: Value| svg(props).unwrap();
        assert_eq!(plain(json!({"viewBox":"0 0 24 24","shapes":[]})).size, 16.0);
        assert_eq!(
            plain(json!({"viewBox":"0,0,24,24","size":1e9,"shapes":[]})).size,
            64.0
        );
    }

    #[test]
    fn an_svg_with_anything_the_app_does_not_draw_is_left_out() {
        let with = |shapes: Value| json!({"viewBox":"0 0 16 16","shapes":shapes});
        let mut nested = json!([["path",{"d":"M0 0h1"}]]);
        for _ in 0..5 {
            nested = json!([["g", {}, nested]]);
        }
        let many: Vec<Value> = (0..=svg::MAX_SHAPES)
            .map(|_| json!(["path",{"d":"M0 0h1"}]))
            .collect();
        // At the limits, it is drawn: what is left out below is left out for the one thing wrong with it.
        let mut deepest = json!([["path",{"d":"M0 0h1"}]]);
        for _ in 0..4 {
            deepest = json!([["g", {}, deepest]]);
        }
        let most = Value::Array(many[1..].to_vec());
        for drawn in [
            with(json!([["path",{"d":"M0 0h1"}]])),
            with(deepest),
            with(most),
        ] {
            assert!(svg(drawn.clone()).is_some(), "{drawn}");
        }
        for refused in [
            // Elements that load files, fonts or styles, and attributes outside the allowlist.
            with(json!([["image",{"href":"/etc/passwd","width":16,"height":16}]])),
            with(json!([["text",{"x":0,"y":0},[]]])),
            with(json!([["style", {}]])),
            with(json!([["use",{"href":"#a"}]])),
            with(json!([["path",{"d":"M0 0h1","href":"x"}]])),
            with(json!([["path",{"d":"M0 0h1","style":"fill:red"}]])),
            with(json!([["path",{"d":"M0 0h1","transform":"scale(9)"}]])),
            with(json!([["rect",{"d":"M0 0h1"}]])),
            // Values that aren't what the attribute takes, or would end it.
            with(json!([["path",{"d":"M0 0\" onload=\"x"}]])),
            with(json!([["path",{"d":"M0 0<script>"}]])),
            with(json!([["polygon",{"points":"0,0 M 4,4"}]])),
            with(json!([["path",{"d":"M0 0h1","fill":"red"}]])),
            with(json!([["path",{"d":"M0 0h1","stroke":"url(#paint)"}]])),
            with(json!([["path",{"d":"M0 0h1","strokeLinecap":"pointy"}]])),
            with(json!([["rect",{"width":-1}]])),
            with(json!([["rect",{"width":"10px"}]])),
            with(json!([["rect",{"x":"NaN"}]])),
            with(json!([["rect",{"x":1e9}]])),
            with(json!([["rect",{"x":true}]])),
            with(json!([["g",{"opacity":2}]])),
            // Shapes that aren't shapes, or hold shapes when only a g does.
            with(json!([["path",{"d":"M0 0h1"},[["path",{"d":"M0 0h1"}]]]])),
            with(json!([[7, {}]])),
            with(json!([["path"]])),
            with(json!(["path"])),
            with(json!({"path":{}})),
            json!({"viewBox":"0 0 16 16"}),
            // Over the limits.
            with(Value::Array(many)),
            with(nested),
            with(json!([["path",{"d":"M0 0".repeat(svg::MAX_MARKUP_BYTES)}]])),
            // A view box that isn't four numbers of a square.
            json!({"shapes":[]}),
            json!({"viewBox":"0 0 16","shapes":[]}),
            json!({"viewBox":"0 0 16 8","shapes":[]}),
            json!({"viewBox":"0 0 0 0","shapes":[]}),
            json!({"viewBox":"0 0 1e9 1e9","shapes":[]}),
            json!({"viewBox":"a b c d","shapes":[]}),
            json!({"viewBox":"0 0 16 16","fill":"blue","shapes":[]}),
        ] {
            assert_eq!(svg(refused.clone()), None, "{refused}");
        }
    }

    #[test]
    fn parses_a_split_a_scroll_a_bar_and_a_dot_with_a_colour() {
        let parsed = tree(json!([
            {"id":1,"type":"Split","props":{"defaultSize":320},"children":[
                {"id":2,"type":"Slot","props":{"name":"start"},"children":[
                    {"id":3,"type":"Bar","props":{"edge":"bottom","gap":"xs"},"children":[{"id":4,"type":"#text","text":"Head"}]},
                    {"id":5,"type":"Scroll","props":{},"children":[{"id":6,"type":"Dot","props":{"color":1122867},"children":[]}]}]},
                {"id":7,"type":"Slot","props":{"name":"end"},"children":[{"id":8,"type":"Dot","props":{"color":99999999},"children":[]}]}]}
        ]))
        .unwrap();
        let Node::Split(split) = &parsed.children[0] else {
            panic!("a split")
        };
        assert_eq!(split.size, 320.0);
        let (Node::Bar(bar), Node::Scroll(scroll)) = (&split.start[0], &split.start[1]) else {
            panic!("a bar over a scroll")
        };
        assert_eq!((bar.edge, bar.gap), (Edge::Bottom, Gap::Xs));
        assert!(matches!(
            &scroll.children[0],
            Node::Dot(Dot {
                color: Some(0x112233),
                ..
            })
        ));
        // A colour that isn't one is no colour.
        assert!(matches!(&split.end[0], Node::Dot(Dot { color: None, .. })));
        // The fixed pane is the start one, always shown, unless the page says otherwise.
        assert!(!split.fixed_end && split.collapse_below.is_none() && !split.pinned);
    }

    #[test]
    fn parses_a_split_whose_fixed_pane_is_on_the_right_and_folds_when_narrow() {
        let parsed = tree(json!([
            {"id":1,"type":"Split","props":{"defaultSize":260,"side":"end","collapseBelow":1100,"pinned":true},"children":[]},
            {"id":2,"type":"Split","props":{"side":"start","collapseBelow":-5,"gap":"l"},"children":[]},
            {"id":3,"type":"Split","props":{"side":"end","collapseBelow":"wide","gap":"huge"},"children":[]}
        ]))
        .unwrap();
        let splits: Vec<&Split> = parsed
            .children
            .iter()
            .map(|node| match node {
                Node::Split(split) => split,
                _ => panic!("a split"),
            })
            .collect();
        assert_eq!(
            (splits[0].size, splits[0].fixed_end, splits[0].collapse_below, splits[0].pinned),
            (260.0, true, Some(1100.0), true)
        );
        // Only "end" moves the pane, and a threshold that is no positive number is none.
        assert_eq!((splits[1].fixed_end, splits[1].collapse_below), (false, None));
        assert_eq!((splits[2].fixed_end, splits[2].collapse_below), (true, None));
        // Room at the divider only when asked for, and a gap it doesn't know is none.
        assert_eq!((splits[0].gap, splits[1].gap, splits[2].gap), (Gap::None, Gap::L, Gap::None));
    }

    #[test]
    fn parses_a_dialog_with_its_title_width_and_close_and_bounds_its_width() {
        let parsed = tree(json!([
            {"id":1,"type":"Dialog","props":{"title":"Plugins","width":640,"onClose":"1:onClose"},"children":[
                {"id":2,"type":"Text","props":{},"children":[{"id":3,"type":"#text","text":"Inside"}]}]},
            {"id":4,"type":"Dialog","props":{"width":99999,"onClose":"no handler"},"children":[]},
            {"id":5,"type":"Dialog","props":{"width":-3},"children":[]}
        ]))
        .unwrap();
        let dialogs: Vec<&Dialog> = parsed
            .children
            .iter()
            .map(|node| match node {
                Node::Dialog(dialog) => dialog,
                _ => panic!("a dialog"),
            })
            .collect();
        assert_eq!(
            (dialogs[0].title.as_str(), dialogs[0].width, dialogs[0].on_close.as_deref()),
            ("Plugins", 640.0, Some("1:onClose"))
        );
        assert_eq!(plain_text(&parsed.children[..1]), "Inside");
        // A width past the bounds is held to them, and a close that isn't a handler is none.
        assert_eq!((dialogs[1].width, dialogs[1].on_close.as_deref()), (1_000.0, None));
        assert_eq!(dialogs[2].width, 280.0);
    }

    #[test]
    fn parses_a_terminal_and_a_text_that_asks_for_links() {
        let parsed = tree(json!([
            {"id":1,"type":"Terminal","props":{"pane":"12","height":5000}},
            {"id":2,"type":"Terminal","props":{"pane":"x".repeat(21)}},
            {"id":3,"type":"Text","props":{"links":true},"children":[{"id":4,"type":"#text","text":"See ABC-12"}]},
            {"id":5,"type":"Stack","props":{"links":true},"children":[]}
        ]))
        .unwrap();
        assert_eq!(parsed.children[0], Node::Terminal(Terminal { id: 1, pane: "12".into(), height: 800.0 }));
        assert_eq!(parsed.children[1], Node::Terminal(Terminal { id: 2, pane: String::new(), height: 220.0 }));
        assert!(matches!(&parsed.children[2], Node::Text(text) if text.links));
        assert!(matches!(&parsed.children[3], Node::Stack(stack) if stack.links));
    }

    #[test]
    fn a_sheet_has_a_title_a_description_a_header_and_a_body_and_hears_its_close() {
        let parsed = tree(json!([{"id":1,"type":"Sheet","props":{"onClose":"1:onClose"},"children":[
            slot("title", json!([{"id":3,"type":"#text","text":"Past sessions"}])),
            slot("description", json!([{"id":4,"type":"#text","text":"In shop."}])),
            slot("header", json!([{"id":5,"type":"SearchField","props":{"label":"Search"}}])),
            text(6, "Fix the checkout"),
            text(7, "Split the cart")
        ]}]))
        .unwrap();
        let Node::Sheet(sheet) = &parsed.children[0] else { panic!("a sheet") };
        assert_eq!(sheet.on_close.as_deref(), Some("1:onClose"));
        assert_eq!((plain_text(&sheet.title), plain_text(&sheet.description)), ("Past sessions".into(), "In shop.".into()));
        assert!(matches!(sheet.header.as_slice(), [Node::SearchField(_)]));
        assert_eq!(sheet.body.len(), 2);
        // What a reader would see has its title, so a page that waits on it finds it.
        assert_eq!(parsed.text(), "Past sessions In shop. Fix the checkoutSplit the cart");
    }

    /// A shortcut the app or a text field answers to is dropped, so the page's button can't take it away from them.
    #[test]
    fn a_buttons_shortcut_is_kept_only_when_no_app_or_text_key_has_it() {
        let shortcut = |value: &str| {
            let parsed = tree(json!([button(json!({"shortcut": value, "onClick": "2:onClick"}))])).unwrap();
            let Node::Button(button) = &parsed.children[0] else { panic!("a button") };
            button.shortcut.clone()
        };
        let kept = shortcut("Cmd-Shift-P").expect("a free shortcut is kept");
        assert_eq!((kept.canonical(), kept.aria()), ("shift-cmd-p".into(), "Shift+Meta+P".into()));
        assert!(shortcut("cmd-alt-b").is_some() && shortcut("cmd-shift-[").is_some());
        for refused in [
            // The app's: quit, close, new tab, settings, tab by number, font size, the sidebar, the palette.
            "cmd-q", "cmd-w", "cmd-t", "cmd-n", "cmd-,", "cmd-3", "cmd-=", "cmd--", "cmd-0", "cmd-b", "cmd-k",
            "cmd-shift-d", "ctrl-shift-t",
            // A text field's.
            "cmd-a", "cmd-z", "cmd-shift-z",
            // Plain keys and keys that move a caret belong to what has the keyboard.
            "p", "shift-p", "cmd-left", "cmd-enter", "cmd-backspace",
            // Not a key.
            "cmd-", "cmd-shift", "hyper-p", "",
        ] {
            assert_eq!(shortcut(refused), None, "{refused:?} was kept");
        }
    }

    #[test]
    fn a_page_that_sizes_its_own_text_names_its_handler() {
        let parsed = tree(json!([{"id":1,"type":"Page","props":{"onTextScale":"1:onTextScale"},"children":[]}])).unwrap();
        let Node::Page(page) = &parsed.children[0] else { panic!("a page") };
        assert_eq!(page.on_text_scale.as_deref(), Some("1:onTextScale"));
    }

    #[test]
    fn a_page_asks_for_the_pixels_or_a_gradient_behind_it_and_any_other_backdrop_is_its_plain_ground() {
        let backdrop = |props: Value| {
            let parsed = tree(json!([{"id":1,"type":"Page","props":props,"children":[]}])).unwrap();
            let Node::Page(page) = &parsed.children[0] else { panic!("a page") };
            page.backdrop
        };
        use crate::pixels::Pattern;
        assert_eq!(backdrop(json!({"backdrop":"pixels"})), Some(Backdrop::Pixels(Pattern::Drift)));
        assert_eq!(backdrop(json!({"backdrop":"pixels:rain"})), Some(Backdrop::Pixels(Pattern::Rain)));
        assert_eq!(backdrop(json!({"backdrop":"pixels:checker"})), Some(Backdrop::Pixels(Pattern::Checker)));
        assert_eq!(backdrop(json!({"backdrop":"pixels:embers"})), Some(Backdrop::Pixels(Pattern::Embers)));
        // A pattern from a newer plugin than this Wings knows is the drift, not the plain ground.
        assert_eq!(backdrop(json!({"backdrop":"pixels:snow"})), Some(Backdrop::Pixels(Pattern::Drift)));
        for (name, pattern) in [
            ("mono", Pattern::Mono),
            ("ocean", Pattern::Ocean),
            ("meadow", Pattern::Meadow),
            ("dusk", Pattern::Dusk),
            ("blocks", Pattern::Blocks),
            ("bayer", Pattern::Bayer),
            ("grain", Pattern::Grain),
            ("grain-lagoon", Pattern::GrainLagoon),
        ] {
            assert_eq!(backdrop(json!({"backdrop": format!("pixels:{name}")})), Some(Backdrop::Pixels(pattern)));
        }
        // A page of an older plugin that still asks for the dropped moire gets the plain ground, not a gradient.
        assert_eq!(backdrop(json!({"backdrop":"gradient:moire"})), None);
        assert_eq!(backdrop(json!({"backdrop":"gradient:halo"})), Some(Backdrop::Gradient(crate::gradients::Preset::Halo)));
        assert_eq!(
            backdrop(json!({"backdrop":"gradient:nighty-night"})),
            Some(Backdrop::Gradient(crate::gradients::Preset::NightyNight))
        );
        assert_eq!(backdrop(json!({"backdrop":"gradient:aurora"})), None);
        assert_eq!(backdrop(json!({"backdrop":"gradient:"})), None);
        assert_eq!(backdrop(json!({"backdrop":"halo"})), None);
        assert_eq!(backdrop(json!({"backdrop":"glow"})), None);
        assert_eq!(backdrop(json!({"backdrop":3})), None);
        assert_eq!(backdrop(json!({})), None);
    }

    fn slot_text(id: u64, name: &str, text: &str) -> Value {
        json!({"id":id,"type":"Slot","props":{"name":name},"children":[{"id":id + 1,"type":"#text","text":text}]})
    }

    /// The row a `ListRow` lays out: the lead's box, then the title over the detail, then what trails.
    fn row_line(node: &Node) -> &Row {
        let children = match node {
            Node::ListButton(item) => &item.button.children,
            Node::Stack(stack) => &stack.children,
            _ => panic!("a list button or a stack"),
        };
        let [Node::Row(line)] = children.as_slice() else { panic!("one row") };
        line
    }

    #[test]
    fn a_list_row_with_a_click_is_a_list_button_and_one_without_sits_in_its_inset() {
        let lead = json!({"id":20,"type":"Slot","props":{"name":"lead"},"children":[{"id":21,"type":"Spinner","props":{}}]});
        let trailing = json!({"id":30,"type":"Slot","props":{"name":"trailing"},"children":[button(json!({}))]});
        let parsed = tree(json!([
            {"id":1,"type":"ListRow","props":{"onClick":"1:onClick","title":"Open it"},"children":[lead, slot_text(10, "title", "Build"), slot_text(12, "detail", "Done · 2s"), trailing]},
            {"id":40,"type":"ListRow","props":{},"children":[slot_text(41, "title", "Earlier")]},
            // A disabled one takes no click: it is drawn as one without.
            {"id":50,"type":"ListRow","props":{"onClick":"50:onClick","disabled":true},"children":[slot_text(51, "title", "Off")]},
        ]))
        .unwrap();
        let Node::ListButton(clickable) = &parsed.children[0] else { panic!("a list button") };
        assert_eq!((clickable.button.on_click.as_deref(), clickable.button.title.as_deref()), (Some("1:onClick"), Some("Open it")));
        let line = row_line(&parsed.children[0]);
        assert!(line.grow, "it takes the button's width, so what trails sits at its end");
        let [Node::Stack(lead), Node::Stack(words), Node::Button(_)] = line.children.as_slice() else { panic!("lead, words, trailing: {line:?}") };
        assert_eq!(lead.width, Some(16.0));
        assert!(words.grow);
        assert_eq!(plain_text(&words.children), "BuildDone · 2s");
        // Beside a control the words keep part of the row, and a control too wide for the rest goes under them.
        assert!(line.wrap && words.min_share == Some(0.4), "{line:?}");
        for plain in &parsed.children[1..] {
            let Node::Stack(inset) = plain else { panic!("a stack") };
            assert!(inset.inset && !inset.grow);
            // No lead: the title is the row's first part, at the inset's edge.
            assert!(matches!(row_line(plain).children.as_slice(), [Node::Stack(words)] if words.width.is_none() && words.min_share.is_none()));
            assert!(!row_line(plain).wrap, "nothing trails, so nothing goes under");
        }
    }

    #[test]
    fn a_section_puts_its_heading_and_description_in_the_rows_inset_and_its_rows_under_them() {
        let parsed = tree(json!([{"id":1,"type":"Section","props":{},"children":[
            slot_text(2, "title", "Subagents"),
            slot_text(4, "description", "What this session started."),
            {"id":6,"type":"ListRow","props":{"onClick":"6:onClick"},"children":[slot_text(7, "title", "Explore")]},
        ]}]))
        .unwrap();
        let Node::Stack(section) = &parsed.children[0] else { panic!("a stack") };
        assert_eq!((section.id, section.gap), (1, Gap::Xs));
        let [Node::Stack(head), Node::Stack(about), Node::Stack(rows)] = section.children.as_slice() else { panic!("{section:?}") };
        assert!(head.inset && matches!(head.children.as_slice(), [Node::Heading(heading)] if plain_text(&heading.children) == "Subagents"));
        assert!(about.inset && about.measure.is_some() && about.max_width.is_none(), "read at a measure, not centred");
        assert!(matches!(about.children.as_slice(), [Node::Text(text)] if text.tone == Some(Tone::Muted) && text.size == TextSize::Small));
        assert_eq!(rows.gap, Gap::None);
        assert!(matches!(rows.children.as_slice(), [Node::ListButton(_)]));
        // Without a description there is no line for one.
        let bare = tree(json!([{"id":1,"type":"Section","props":{},"children":[slot_text(2, "title", "Folders")]}])).unwrap();
        let Node::Stack(bare) = &bare.children[0] else { panic!("a stack") };
        assert_eq!(bare.children.len(), 2);
    }

    #[test]
    fn a_collapsible_section_is_a_disclosure_with_its_heading_as_the_summary_and_its_rows_as_the_body() {
        let parsed = tree(json!([{"id":1,"type":"Section","props":{"collapsible":true,"open":false,"onOpenChange":"1:onOpenChange"},"children":[
            slot_text(2, "title", "Subagents · 4 running"),
            slot_text(4, "description", "What this session started."),
            {"id":6,"type":"ListRow","props":{},"children":[slot_text(7, "title", "Explore")]},
        ]}]))
        .unwrap();
        let Node::Disclosure(fold) = &parsed.children[0] else { panic!("{:?}", parsed.children) };
        assert_eq!((fold.id, fold.open, fold.section), (1, Some(false), true));
        assert_eq!(fold.on_open_change.as_deref(), Some("1:onOpenChange"));
        assert_eq!(plain_text(&fold.summary), "Subagents · 4 running");
        assert_eq!(fold.body.len(), 2, "the description, then the rows");
        // Without the flag the section is the plain stack, as an older page draws it.
        let plain = tree(json!([{"id":1,"type":"Section","props":{"open":false},"children":[slot_text(2, "title", "Folders")]}])).unwrap();
        assert!(matches!(plain.children[0], Node::Stack(_)));
    }

    #[test]
    fn actions_put_the_dismissing_buttons_first_and_ghost_whatever_variant_they_came_in() {
        let parsed = tree(json!([{"id":1,"type":"Actions","props":{},"children":[
            {"id":5,"type":"Button","props":{"variant":"default"},"children":[{"id":6,"type":"#text","text":"Review"}]},
            {"id":3,"type":"Slot","props":{"name":"dismiss"},"children":[button(json!({"variant":"outline"}))]},
        ]}]))
        .unwrap();
        let Node::Actions(actions) = &parsed.children[0] else { panic!("actions") };
        let variants: Vec<_> = actions
            .children
            .iter()
            .map(|node| match node {
                Node::Button(button) => (plain_text(&button.children), button.variant),
                _ => panic!("a button"),
            })
            .collect();
        assert_eq!(variants, [("Go".to_owned(), Variant::Ghost), ("Review".to_owned(), Variant::Default)]);
    }
}

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
