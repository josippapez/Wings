//! The desktop app's icons: Lucide's (ISC) and the Claude spark, see `assets/icons/LICENSE`. Built into
//! the binary.

use std::borrow::Cow;

use gpui::{AssetSource, SharedString, Svg, prelude::*, px, rgb, svg};

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
    ArrowDown = "arrow-down",
    ArrowLeft = "arrow-left",
    ArrowRight = "arrow-right",
    ArrowUp = "arrow-up",
    ArrowUpDown = "arrow-up-down",
    Check = "check",
    Circle = "circle",
    CircleCheck = "circle-check",
    CircleDot = "circle-dot",
    CircleDotDashed = "circle-dot-dashed",
    CircleX = "circle-x",
    ChevronDown = "chevron-down",
    Claude = "claude",
    Code = "code",
    ChevronRight = "chevron-right",
    Columns = "columns-2",
    Copy = "copy",
    Ellipsis = "ellipsis",
    FileText = "file-text",
    Folder = "folder",
    FolderPlus = "folder-plus",
    FolderOpen = "folder-open",
    GitBranch = "git-branch",
    GitFork = "git-fork",
    GitMerge = "git-merge",
    GitPullRequest = "git-pull-request",
    GitPullRequestClosed = "git-pull-request-closed",
    GitPullRequestDraft = "git-pull-request-draft",
    Inbox = "inbox",
    Keyboard = "keyboard",
    Maximize = "maximize-2",
    Minimize = "minimize-2",
    Minus = "minus",
    Opencode = "opencode",
    PanelLeft = "panel-left",
    Pencil = "pencil",
    Pin = "pin",
    PinOff = "pin-off",
    Plus = "plus",
    Puzzle = "puzzle",
    RefreshCw = "refresh-cw",
    Rows = "rows-2",
    ScrollText = "scroll-text",
    Search = "search",
    SearchX = "search-x",
    Settings = "settings",
    SlidersHorizontal = "sliders-horizontal",
    Sparkles = "sparkles",
    SquarePlus = "square-plus",
    SquareTerminal = "square-terminal",
    StickyNote = "sticky-note",
    Terminal = "terminal",
    TextCursor = "text-cursor",
    Trash = "trash",
    Type = "type",
    User = "user",
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
