//! The desktop app's icons: Lucide's (ISC) and the Claude spark, see `assets/icons/LICENSE`. Built into
//! the binary.

use std::borrow::Cow;

use gpui::{AnyElement, AssetSource, SharedString, Svg, prelude::*, px, rgb, svg};

macro_rules! icons {
    ($($name:ident = $file:literal),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Icon {
            $($name),*
        }

        impl Icon {
            fn path(self) -> &'static str {
                match self {
                    $(Icon::$name => concat!("icons/", $file, ".svg")),*
                }
            }
        }

        fn bytes(path: &str) -> Option<&'static [u8]> {
            match path {
                $(concat!("icons/", $file, ".svg") => {
                    Some(include_bytes!(concat!("../assets/icons/", $file, ".svg")))
                })*
                _ => None,
            }
        }
    };
}

icons!(
    Activity = "activity",
    ArrowDown = "arrow-down",
    ArrowLeft = "arrow-left",
    ArrowRight = "arrow-right",
    ArrowUp = "arrow-up",
    ArrowUpDown = "arrow-up-down",
    Bell = "bell",
    Bot = "bot",
    Calendar = "calendar",
    CaseSensitive = "case-sensitive",
    ChartColumn = "chart-column",
    Check = "check",
    Circle = "circle",
    CircleArrowUp = "circle-arrow-up",
    CircleCheck = "circle-check",
    CircleDot = "circle-dot",
    CircleDotDashed = "circle-dot-dashed",
    CircleX = "circle-x",
    ChevronDown = "chevron-down",
    Claude = "claude",
    Clock = "clock",
    Code = "code",
    Codex = "codex",
    ChevronRight = "chevron-right",
    Columns = "columns-2",
    Copy = "copy",
    Cursor = "cursor",
    Ellipsis = "ellipsis",
    ExternalLink = "external-link",
    EyeOff = "eye-off",
    FileText = "file-text",
    Folder = "folder",
    FolderPlus = "folder-plus",
    FolderOpen = "folder-open",
    FolderTree = "folder-tree",
    Gauge = "gauge",
    GitBranch = "git-branch",
    GitCompareArrows = "git-compare-arrows",
    GitFork = "git-fork",
    GitGraph = "git-graph",
    GitMerge = "git-merge",
    GitPullRequest = "git-pull-request",
    GitPullRequestClosed = "git-pull-request-closed",
    GitPullRequestDraft = "git-pull-request-draft",
    Globe = "globe",
    Grok = "grok",
    Inbox = "inbox",
    Keyboard = "keyboard",
    List = "list",
    ListTree = "list-tree",
    ListTodo = "list-todo",
    Maximize = "maximize-2",
    MessageSquare = "message-square",
    MessageSquareText = "message-square-text",
    Minimize = "minimize-2",
    Minus = "minus",
    Opencode = "opencode",
    PanelBottom = "panel-bottom",
    PanelLeft = "panel-left",
    PanelRight = "panel-right",
    Pencil = "pencil",
    PictureInPicture = "picture-in-picture-2",
    Pin = "pin",
    PinOff = "pin-off",
    Plus = "plus",
    Puzzle = "puzzle",
    RefreshCw = "refresh-cw",
    Regex = "regex",
    Reply = "reply",
    Rows = "rows-2",
    RotateCcwClock = "rotate-ccw-clock",
    Rows3 = "rows-3",
    ScrollText = "scroll-text",
    Search = "search",
    SearchX = "search-x",
    Server = "server",
    Settings = "settings",
    SlidersHorizontal = "sliders-horizontal",
    Sparkles = "sparkles",
    SquareKanban = "square-kanban",
    SquarePlus = "square-plus",
    SquareTerminal = "square-terminal",
    StickyNote = "sticky-note",
    Terminal = "terminal",
    TextCursor = "text-cursor",
    TextWrap = "text-wrap",
    Timer = "timer",
    Trash = "trash",
    Type = "type",
    User = "user",
    Wrench = "wrench",
    X = "x",
);

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Ok(bytes(path).map(Cow::Borrowed))
    }

    fn list(&self, _: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}

/// `icon`, `size` points square, in `color`.
pub fn icon(icon: Icon, size: f32, color: u32) -> Svg {
    svg()
        .path(icon.path())
        .size(px(size))
        .flex_none()
        .text_color(rgb(color))
}

/// A plugin's icon, as the desktop's `PluginGlyph`: the built-in icon its manifest names (one of
/// `plugin-icons.json`), else the plugin's own SVG, else a globe for a name it doesn't know or a puzzle piece when it
/// names none.
pub fn plugin_glyph(name: &str, own: Option<&str>, size: f32, color: u32) -> AnyElement {
    let named = match name {
        "clock" => Some(Icon::Clock),
        "globe" => Some(Icon::Globe),
        "calendar" => Some(Icon::Calendar),
        "chart" => Some(Icon::ChartColumn),
        "list" => Some(Icon::List),
        "git-compare" => Some(Icon::GitCompareArrows),
        "git-graph" => Some(Icon::GitGraph),
        "git-branch" => Some(Icon::GitBranch),
        "folder-tree" => Some(Icon::FolderTree),
        "message" => Some(Icon::MessageSquareText),
        "scroll" => Some(Icon::ScrollText),
        "kanban" => Some(Icon::SquareKanban),
        "gauge" => Some(Icon::Gauge),
        "activity" => Some(Icon::Activity),
        "timer" => Some(Icon::Timer),
        "server" => Some(Icon::Server),
        "terminal" => Some(Icon::SquareTerminal),
        "todo" => Some(Icon::ListTodo),
        "puzzle" => Some(Icon::Puzzle),
        _ => None,
    };
    let (glyph, _drew) = match (named, own) {
        (Some(glyph), _) => (icon(glyph, size, color).into_any_element(), name),
        (None, Some(own)) => (
            svg()
                .data(own.as_bytes())
                .size(px(size))
                .flex_none()
                .text_color(rgb(color))
                .into_any_element(),
            "own",
        ),
        (None, None) if name.is_empty() => {
            (icon(Icon::Puzzle, size, color).into_any_element(), "puzzle")
        }
        (None, None) => (icon(Icon::Globe, size, color).into_any_element(), "globe"),
    };
    // Tests find where each plugin icon was drawn, as `plugin-glyph:<name>`, `:own` for the plugin's SVG.
    #[cfg(feature = "test-support")]
    return gpui::div()
        .relative()
        .flex_none()
        .child(glyph)
        .children(crate::ui::spot(|| format!("plugin-glyph:{_drew}")))
        .into_any_element();
    #[cfg(not(feature = "test-support"))]
    glyph
}
