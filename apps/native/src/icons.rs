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
    Check = "check",
    ChevronDown = "chevron-down",
    Claude = "claude",
    ChevronRight = "chevron-right",
    Columns = "columns-2",
    Copy = "copy",
    Folder = "folder",
    GitBranch = "git-branch",
    Keyboard = "keyboard",
    Maximize = "maximize-2",
    Minimize = "minimize-2",
    PanelLeft = "panel-left",
    Pin = "pin",
    Plus = "plus",
    Puzzle = "puzzle",
    Rows = "rows-2",
    Settings = "settings",
    Terminal = "terminal",
    TextCursor = "text-cursor",
    Trash = "trash",
    Type = "type",
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
